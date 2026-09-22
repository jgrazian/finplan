use super::*;

#[derive(Default)]
pub(super) struct Hook {
    pub claimed: tokio::sync::Notify,
    pub proceed: tokio::sync::Notify,
    pub panic: bool,
    pub pause_after_engine: bool,
    pub engine_finished: tokio::sync::Notify,
    pub finish_allowed: tokio::sync::Notify,
}

async fn fixture() -> (Db, RunQueue, String, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let db = crate::db::connect(
        &format!("sqlite://{}", directory.path().join("worker.db").display()),
        2,
    )
    .await
    .unwrap();
    let user = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?, 'unused')")
        .bind(&user)
        .bind(format!("{user}@example.test"))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scenarios(id,user_id,name,start_date,duration_years) VALUES (1,?,'Test','2026-01-01',1)")
        .bind(&user).execute(&db).await.unwrap();
    let queue = spawn_with_telemetry(db.clone(), 1, Telemetry::new(1));
    (db, queue, user, directory)
}
async fn insert(db: &Db, user: &str, valid: bool) -> i64 {
    let graph = ScenarioGraph::load(db, 1, user).await.unwrap();
    let (snapshot, _) = inputs::snapshot(&graph).unwrap();
    sqlx::query_scalar("INSERT INTO runs(scenario_id,user_id,iterations,seed,snapshot_json,model_version) VALUES (1,?,3,42,?,?) RETURNING id")
        .bind(user).bind(valid.then_some(snapshot)).bind(inputs::MODEL_VERSION).fetch_one(db).await.unwrap()
}
fn send(queue: &RunQueue, user: &str, id: i64) {
    let permit = crate::billing::admit_compute(user).unwrap();
    queue.reserve(permit, Origin::Request).unwrap().send(
        id,
        JobContext::new(JobKind::Run, Origin::Request, user, 1, id),
    );
}
async fn idle(queue: &RunQueue) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if queue.tx.capacity() == 16
                && queue.worker_permits.available_permits() == 1
                && queue.waiting.lock().unwrap().is_empty()
                && queue.in_flight.lock().unwrap().is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
async fn status(db: &Db, id: i64) -> String {
    sqlx::query_scalar("SELECT status FROM runs WHERE id=?")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}
fn metric(queue: &RunQueue, line: &str) -> bool {
    queue.telemetry.encode().unwrap().lines().any(|l| l == line)
}

#[tokio::test]
async fn queued_cancel_is_resolved_once_and_drained_without_an_attempt() {
    let (db, queue, user, _directory) = fixture().await;
    let worker = queue.worker_permits.clone().acquire_owned().await.unwrap();
    let id = insert(&db, &user, true).await;
    send(&queue, &user, id);
    assert!(queue.cancel(id).await.unwrap());
    assert!(!queue.cancel(id).await.unwrap());
    assert_eq!(status(&db, id).await, "canceled");
    drop(worker);
    idle(&queue).await;
    assert!(metric(
        &queue,
        "finplan_jobs_canceled_before_start_total{kind=\"run\"} 1"
    ));
    assert!(
        queue
            .telemetry
            .encode()
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("finplan_job_attempts_total{"))
            .all(|l| l.ends_with(" 0"))
    );
    assert!(queue.waiting.lock().unwrap().is_empty());
}

#[tokio::test]
async fn running_cancel_is_not_a_failure_and_releases_all_guards() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, true).await;
    let hook = Arc::new(Hook::default());
    queue.hooks.lock().unwrap().insert(id, hook.clone());
    send(&queue, &user, id);
    hook.claimed.notified().await;
    assert!(queue.cancel(id).await.unwrap());
    hook.proceed.notify_one();
    idle(&queue).await;
    assert_eq!(status(&db, id).await, "canceled");
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"canceled\"} 1"
    ));
    assert!(metric(&queue, "finplan_jobs_running{kind=\"run\"} 0"));
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"failed\"} 0"
    ));
}

#[tokio::test]
async fn duplicate_delivery_records_one_success_and_actual_iterations() {
    let (db, queue, user, _directory) = fixture().await;
    let worker = queue.worker_permits.clone().acquire_owned().await.unwrap();
    let id = insert(&db, &user, true).await;
    send(&queue, &user, id);
    queue.enqueue(id).await.unwrap();
    drop(worker);
    idle(&queue).await;
    assert_eq!(status(&db, id).await, "succeeded");
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"succeeded\"} 1"
    ));
    assert!(metric(&queue, "finplan_run_iterations_completed_total 3"));
    assert!(metric(
        &queue,
        "finplan_job_queue_wait_seconds_count{kind=\"run\",exit=\"started\"} 1"
    ));
}

#[tokio::test]
async fn deleted_queued_delivery_does_not_claim_a_reused_sqlite_id() {
    let (db, queue, user, _directory) = fixture().await;
    let worker = queue.worker_permits.clone().acquire_owned().await.unwrap();
    let old = insert(&db, &user, true).await;
    send(&queue, &user, old);
    sqlx::query("DELETE FROM runs WHERE id=?")
        .bind(old)
        .execute(&db)
        .await
        .unwrap();
    queue.deleted(old, Some(0.0));
    let new = insert(&db, &user, true).await;
    assert_eq!(old, new);
    send(&queue, &user, new);
    drop(worker);
    idle(&queue).await;
    assert_eq!(status(&db, new).await, "succeeded");
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"succeeded\"} 1"
    ));
    assert!(metric(
        &queue,
        "finplan_job_queue_wait_seconds_count{kind=\"run\",exit=\"deleted\"} 1"
    ));
}

#[tokio::test]
async fn cascade_deletion_during_execution_records_interrupted_not_success() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, true).await;
    let hook = Arc::new(Hook::default());
    queue.hooks.lock().unwrap().insert(id, hook.clone());
    send(&queue, &user, id);
    hook.claimed.notified().await;
    sqlx::query("DELETE FROM scenarios WHERE id=1")
        .execute(&db)
        .await
        .unwrap();
    hook.proceed.notify_one();
    idle(&queue).await;
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"interrupted\"} 1"
    ));
    assert!(metric(&queue, "finplan_jobs_running{kind=\"run\"} 0"));
}

#[tokio::test]
async fn preparation_and_terminal_storage_failures_remain_distinct() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, false).await;
    sqlx::query("CREATE TRIGGER reject_failure BEFORE UPDATE OF status ON runs WHEN NEW.status='failed' BEGIN SELECT RAISE(ABORT,'synthetic private database text'); END")
        .execute(&db).await.unwrap();
    send(&queue, &user, id);
    idle(&queue).await;
    assert_eq!(
        status(&db, id).await,
        "running",
        "terminal storage failure must leave recoverable work"
    );
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"interrupted\"} 1"
    ));
    assert!(metric(
        &queue,
        "finplan_server_errors_total{component=\"run\",class=\"preparation\"} 1"
    ));
    assert!(metric(
        &queue,
        "finplan_server_errors_total{component=\"run\",class=\"persistence\"} 1"
    ));
    assert!(!queue.telemetry.encode().unwrap().contains("private"));
}

#[tokio::test]
async fn engine_panic_is_sanitized_and_failure_is_recorded_once() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, true).await;
    let hook = Arc::new(Hook {
        panic: true,
        ..Default::default()
    });
    queue.hooks.lock().unwrap().insert(id, hook.clone());
    send(&queue, &user, id);
    hook.claimed.notified().await;
    hook.proceed.notify_one();
    idle(&queue).await;
    assert_eq!(status(&db, id).await, "failed");
    let error: String = sqlx::query_scalar("SELECT error_message FROM runs WHERE id=?")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(error, "Simulation worker failed");
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"failed\"} 1"
    ));
    assert!(metric(
        &queue,
        "finplan_server_errors_total{component=\"run\",class=\"engine_panic\"} 1"
    ));
}

#[tokio::test]
async fn reserve_failure_and_rollback_release_admission_and_channel_capacity() {
    let (db, queue, user, _directory) = fixture().await;
    // Channel capacity is reserved even before any INSERT has happened.
    let reserved = queue
        .reserve(
            crate::billing::admit_compute(&user).unwrap(),
            Origin::Request,
        )
        .unwrap();
    assert_eq!(queue.tx.capacity(), 15);
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO runs(scenario_id,user_id,iterations) VALUES (1,?,1)")
        .bind(&user)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    drop(reserved);
    assert_eq!(queue.tx.capacity(), 16);
    let one = crate::billing::admit_compute(&user).unwrap();
    let two = crate::billing::admit_compute(&user).unwrap();
    drop((one, two));
    let sender = queue.tx.clone();
    let full = sender.try_reserve_many(16).unwrap();
    assert!(
        queue
            .reserve(
                crate::billing::admit_compute(&user).unwrap(),
                Origin::Request
            )
            .is_err()
    );
    drop(full);
    let mut closed = queue.clone();
    let (tx, rx) = mpsc::channel(1);
    closed.tx = tx;
    drop(rx);
    assert!(
        closed
            .reserve(
                crate::billing::admit_compute(&user).unwrap(),
                Origin::Request
            )
            .is_err()
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn recovery_admits_large_persisted_backlog_once_and_preserves_original_age() {
    let (db, queue, user, _directory) = fixture().await;
    let worker = queue.worker_permits.clone().acquire_owned().await.unwrap();
    for _ in 0..20 {
        let id = insert(&db, &user, true).await;
        sqlx::query(
            "UPDATE runs SET status='running',created_at=datetime('now','-120 seconds') WHERE id=?",
        )
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    }
    requeue_orphans(&db, &queue).await.unwrap();
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE status='queued'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        queued, 20,
        "persistent backlog exceeds bounded channel capacity"
    );
    drop(worker);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let done: i64 =
                sqlx::query_scalar("SELECT count(*) FROM runs WHERE status='succeeded'")
                    .fetch_one(&db)
                    .await
                    .unwrap();
            if done == 20 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    idle(&queue).await;
    assert!(metric(&queue, "finplan_jobs_recovered_total 20"));
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"succeeded\"} 20"
    ));
    assert!(
        queue
            .telemetry
            .encode()
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("finplan_job_submissions_total{"))
            .all(|l| l.ends_with(" 0"))
    );
    let text = queue.telemetry.encode().unwrap();
    let wait: f64 = text
        .lines()
        .find(|l| {
            l.starts_with("finplan_job_queue_wait_seconds_sum{kind=\"run\",exit=\"started\"}")
        })
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        wait >= 2400.0,
        "queue wait includes original persisted age and downtime"
    );
}

#[tokio::test]
async fn failed_claim_releases_bookkeeping_without_fictitious_start() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, true).await;
    sqlx::query("CREATE TRIGGER fail_claim BEFORE UPDATE OF status ON runs WHEN NEW.status='running' BEGIN SELECT RAISE(ABORT,'private claim detail'); END")
        .execute(&db).await.unwrap();
    send(&queue, &user, id);
    idle(&queue).await;
    assert_eq!(status(&db, id).await, "queued");
    assert!(queue.waiting.lock().unwrap().is_empty());
    assert!(
        queue
            .telemetry
            .encode()
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("finplan_job_attempts_total{"))
            .all(|l| l.ends_with(" 0"))
    );
    assert!(metric(
        &queue,
        "finplan_server_errors_total{component=\"run\",class=\"database\"} 1"
    ));
}

#[tokio::test]
async fn finished_old_engine_cannot_persist_into_or_uncancel_a_reused_running_id() {
    let (db, queue, user, _directory) = fixture().await;
    queue.worker_permits.add_permits(1);
    let old = insert(&db, &user, true).await;
    let old_hook = Arc::new(Hook {
        pause_after_engine: true,
        ..Default::default()
    });
    queue.hooks.lock().unwrap().insert(old, old_hook.clone());
    send(&queue, &user, old);
    old_hook.claimed.notified().await;
    old_hook.proceed.notify_one();
    old_hook.engine_finished.notified().await;
    // Remove the entire plan, then recreate even the same owner/scenario ids.
    sqlx::query("DELETE FROM scenarios WHERE id=1")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scenarios(id,user_id,name,start_date,duration_years) VALUES (1,?,'Replacement','2026-01-01',1)")
        .bind(&user).execute(&db).await.unwrap();
    let new = insert(&db, &user, true).await;
    assert_eq!(old, new);
    let new_hook = Arc::new(Hook::default());
    queue.hooks.lock().unwrap().insert(new, new_hook.clone());
    send(&queue, &user, new);
    new_hook.claimed.notified().await;
    let new_flag = queue.in_flight.lock().unwrap().get(&new).unwrap().clone();
    old_hook.finish_allowed.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !metric(
            &queue,
            "finplan_job_attempts_total{kind=\"run\",outcome=\"interrupted\"} 1",
        ) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status(&db, new).await, "running");
    assert!(Arc::ptr_eq(
        queue.in_flight.lock().unwrap().get(&new).unwrap(),
        &new_flag
    ));
    assert!(queue.cancel(new).await.unwrap());
    new_hook.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        while queue.worker_permits.available_permits() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    queue.worker_permits.forget_permits(1);
    idle(&queue).await;
    assert_eq!(status(&db, new).await, "canceled");
    let results: i64 = sqlx::query_scalar("SELECT count(*) FROM run_stats")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        results, 0,
        "old results must not attach to the replacement row"
    );
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"canceled\"} 1"
    ));
}

#[tokio::test]
async fn result_persistence_failure_never_records_success_or_completed_samples() {
    let (db, queue, user, _directory) = fixture().await;
    let id = insert(&db, &user, true).await;
    sqlx::query("CREATE TRIGGER fail_stats BEFORE INSERT ON run_stats BEGIN SELECT RAISE(ABORT,'private result detail'); END")
        .execute(&db).await.unwrap();
    send(&queue, &user, id);
    idle(&queue).await;
    assert_eq!(status(&db, id).await, "failed");
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"failed\"} 1"
    ));
    assert!(metric(
        &queue,
        "finplan_job_attempts_total{kind=\"run\",outcome=\"succeeded\"} 0"
    ));
    assert!(metric(&queue, "finplan_run_iterations_completed_total 0"));
    assert!(metric(
        &queue,
        "finplan_server_errors_total{component=\"run\",class=\"persistence\"} 1"
    ));
}

#[tokio::test]
async fn cascaded_queued_deletion_resolves_queue_without_a_worker_attempt() {
    let (db, queue, user, _directory) = fixture().await;
    let worker = queue.worker_permits.clone().acquire_owned().await.unwrap();
    let id = insert(&db, &user, true).await;
    send(&queue, &user, id);
    sqlx::query("DELETE FROM scenarios WHERE id=1")
        .execute(&db)
        .await
        .unwrap();
    drop(worker);
    idle(&queue).await;
    assert!(metric(
        &queue,
        "finplan_job_queue_wait_seconds_count{kind=\"run\",exit=\"deleted\"} 1"
    ));
    assert!(
        queue
            .telemetry
            .encode()
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("finplan_job_attempts_total{"))
            .all(|l| l.ends_with(" 0"))
    );
}
