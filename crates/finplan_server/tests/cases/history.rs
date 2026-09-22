use super::*;

#[tokio::test]
async fn snapshots_survive_edits_and_account_deletion_and_are_owner_scoped() {
    let mut app = TestApp::new().await;
    app.login_as("history-owner@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    app.patch(&base, json!({"duration_years":1})).await;
    let (_, before) = app.get(&format!("{base}/input-hash")).await;
    let (status, run) = app
        .post(&format!("{base}/runs"), json!({"iterations":10}))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    assert!(run["seed"].is_i64());
    assert_eq!(run["input_hash"], before["input_hash"]);
    let id = run["id"].as_i64().unwrap();
    let (_, inputs) = app.get(&format!("/api/runs/{id}/inputs")).await;
    assert_eq!(inputs["snapshot"]["accounts"][0]["name"], "Checking");
    assert_eq!(app.await_run(id).await, "succeeded");
    let (_, original) = app.get(&format!("/api/runs/{id}/results")).await;
    let (status, report) = app.get(&format!("/api/runs/{id}/report")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["results"], original);
    assert_eq!(report["inputs"], inputs);
    let (status, pair) = app
        .post(
            "/api/run-comparisons",
            json!({"left_run_id":id,"right_run_id":id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pair["left"], pair["right"]);

    app.patch(
        &format!("{base}/accounts/{checking}"),
        json!({"name":"Renamed"}),
    )
    .await;
    let (_, changed) = app.get(&format!("{base}/input-hash")).await;
    assert_ne!(before["input_hash"], changed["input_hash"]);
    assert_eq!(
        app.delete(&format!("{base}/accounts/{checking}")).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, retained) = app.get(&format!("/api/runs/{id}/results")).await;
    assert_eq!(
        original, retained,
        "account deletion must not rewrite historical results"
    );
    assert_eq!(inputs, app.get(&format!("/api/runs/{id}/inputs")).await.1);
    let (status, archive) = app.get(&format!("/api/runs/{id}/archive")).await;
    assert_eq!(status, StatusCode::OK, "{archive}");
    assert_eq!(archive["plans"][0]["accounts"][0]["name"], "Checking");
    app.login_as("history-stranger@example.com").await;
    for path in [
        format!("/api/runs/{id}/inputs"),
        format!("/api/runs/{id}/report"),
        format!("/api/runs/{id}/archive"),
        format!("{base}/input-hash"),
        format!("{base}/runs"),
    ] {
        assert_eq!(app.get(&path).await.0, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn shared_definition_changes_affect_hash_without_timestamp_precision() {
    let mut app = TestApp::new().await;
    app.login_as("history-hash@example.com").await;
    let (scenario, _, _) = app.seed_scenario().await;
    let path = format!("/api/scenarios/{scenario}/input-hash");
    let (_, first) = app.get(&path).await;
    assert_eq!(
        first,
        app.get(&path).await.1,
        "unordered graph maps must hash deterministically"
    );
    let (_, profiles) = app.get("/api/return-profiles").await;
    let unused = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "US Small Cap")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        app.patch(
            &format!("/api/return-profiles/{unused}"),
            json!({"distribution":{"kind":"Fixed","rate":0.456}})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        first,
        app.get(&path).await.1,
        "unused library definitions are not inputs to this run"
    );
    let profile = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "US Total Market")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        app.patch(
            &format!("/api/return-profiles/{profile}"),
            json!({"distribution":{"kind":"Fixed","rate":0.123}})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_ne!(first, app.get(&path).await.1);
}

#[tokio::test]
async fn restart_replays_snapshot_even_when_live_inputs_are_deleted() {
    let mut app = TestApp::new().await;
    app.login_as("history-restart@example.com").await;
    let (scenario, checking, _) = app.seed_scenario().await;
    let base = format!("/api/scenarios/{scenario}");
    app.patch(&base, json!({"duration_years":1})).await;
    let (_, run) = app
        .post(&format!("{base}/runs"), json!({"iterations":12, "seed":81}))
        .await;
    let id = run["id"].as_i64().unwrap();
    assert_eq!(app.await_run(id).await, "succeeded");
    let (_, before) = app.get(&format!("/api/runs/{id}/results")).await;
    app.delete(&format!("{base}/accounts/{checking}")).await;
    let db = finplan_server::db::connect(
        &format!("sqlite://{}", app._dir.path().join("test.db").display()),
        2,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE runs SET status = 'running' WHERE id = ?1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let queue = finplan_server::runner::spawn(db.clone(), 1);
    finplan_server::runner::requeue_orphans(&db, &queue)
        .await
        .unwrap();
    assert_eq!(app.await_run(id).await, "succeeded");
    assert_eq!(before, app.get(&format!("/api/runs/{id}/results")).await.1);
    // Legacy queued work cannot claim newly loaded inputs as its own provenance.
    sqlx::query("UPDATE runs SET status = 'queued', snapshot_json = NULL, model_version = NULL, input_hash = NULL WHERE id = ?1").bind(id).execute(&db).await.unwrap();
    queue.enqueue(id).await.unwrap();
    assert_eq!(app.await_run(id).await, "failed");
    let (_, inputs) = app.get(&format!("/api/runs/{id}/inputs")).await;
    assert!(inputs["snapshot"].is_null());
}
