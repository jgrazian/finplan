//! One iteration cap for every simulation path (spec 17): runs, previews,
//! what-ifs and analyses each clamp to `min(their own constant, the caller's
//! cap)`, for a guest (100), a Free account (1,000) and a Pro account (50,000).
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use finplan_server::config::ServerConfig;
use finplan_server::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

const GUEST_CAP: usize = 100;
const FREE_CAP: usize = 1_000;
const PRO_CAP: usize = 50_000;

fn config() -> ServerConfig {
    ServerConfig {
        mail: Default::default(),
        review_ai: Default::default(),
        draft: Default::default(),
        plan_chat: Default::default(),
        log_format: Default::default(),
        metrics_bind: None,
        hosted: true,
        access_mode: Default::default(),
        registration_open: true,
        guest_access: true,
        guest_max_iterations: GUEST_CAP,
        guest_retention_days: 30,
        local_mode: true,
        offload: Default::default(),
        local_mail_sink: None,
        bind: "127.0.0.1:0".into(),
        database_url: "sqlite::memory:".into(),
        db_pool_size: 4,
        sim_workers: 1,
        max_iterations: PRO_CAP,
        secure_cookies: true,
        cors_origins: vec!["https://finplan.example".into()],
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Guest,
    Free,
    Pro,
}

struct Caller {
    router: Router,
    state: AppState,
    cookie: String,
    kind: Kind,
    cap: usize,
}

impl Caller {
    async fn new(kind: Kind) -> Self {
        let (router, state) = finplan_server::build(config()).await.unwrap();
        // Compute admission is process-wide and keyed by user id, so every
        // test's caller needs an id of its own.
        let (prefix, cap) = match kind {
            Kind::Guest => ("caller-guest", GUEST_CAP),
            Kind::Free => ("caller-free", FREE_CAP),
            Kind::Pro => ("caller-pro", PRO_CAP),
        };
        let id = format!("{prefix}-{}", uuid::Uuid::new_v4());
        let id = id.as_str();
        sqlx::query("INSERT INTO users(id,email,password_hash,kind) VALUES (?1,?2,'unused',?3)")
            .bind(id)
            .bind(format!("{id}@example.com"))
            .bind(if kind == Kind::Guest {
                "guest"
            } else {
                "account"
            })
            .execute(&state.db)
            .await
            .unwrap();
        if kind == Kind::Pro {
            sqlx::query("INSERT INTO subscriptions(user_id,provider,subscription_id,state,access_until,revision) VALUES (?1,'test','sub','active',4102444800,1)")
                .bind(id)
                .execute(&state.db)
                .await
                .unwrap();
        }
        finplan_server::seed::seed_user_library(&state.db, id)
            .await
            .unwrap();
        let token = finplan_server::auth::session::issue(&state.db, id, None)
            .await
            .unwrap();
        Self {
            router,
            state,
            cookie: format!("finplan_session={token}"),
            kind,
            cap,
        }
    }

    async fn call(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("origin", "https://finplan.example")
                    .header("cookie", &self.cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
        )
    }
    async fn get(&self, path: &str) -> Value {
        self.call("GET", path, Value::Null).await.1
    }
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call("POST", path, body).await
    }
    /// A submission that may find the caller's last job still releasing its
    /// compute permit: retry the 409 for a moment.
    async fn submit(&self, path: &str, body: Value) -> (StatusCode, Value) {
        for _ in 0..100 {
            let (status, value) = self.post(path, body.clone()).await;
            if status != StatusCode::CONFLICT {
                return (status, value);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("{path} stayed busy");
    }

    /// A one-year plan with a cash account: compiles and simulates instantly.
    async fn plan(&self) -> i64 {
        let profiles = self.get("/api/return-profiles").await;
        let profile = profiles[0]["id"].clone();
        let (status, scenario) = self
            .post(
                "/api/scenarios",
                json!({"name": "Cap", "start_date": "2026-01-01",
                       "birth_date": "1985-01-01", "duration_years": 1}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{scenario}");
        let id = scenario["id"].as_i64().unwrap();
        let (status, account) = self
            .post(
                &format!("/api/scenarios/{id}/accounts"),
                json!({"name": "Checking", "flavor": "Bank",
                       "cash_value": 10_000.0, "return_profile_id": profile}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{account}");
        id
    }

    async fn terminal(&self, run: i64) -> String {
        for _ in 0..400 {
            let status = self.get(&format!("/api/runs/{run}")).await["status"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if matches!(status.as_str(), "succeeded" | "failed" | "canceled") {
                return status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("run {run} never finished");
    }
    async fn seed_of(&self, run: i64) -> Option<i64> {
        sqlx::query_scalar("SELECT seed FROM runs WHERE id = ?")
            .bind(run)
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }
}

/// Runs: over the cap is a 400, exactly the cap is accepted.
async fn runs_hold_the_cap(caller: &Caller, plan: i64) {
    let path = format!("/api/scenarios/{plan}/runs");
    let (status, body) = caller
        .post(&path, json!({"iterations": caller.cap + 1}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"]["message"],
        format!("iterations must not exceed {}", caller.cap),
        "{body}"
    );
    let (status, run) = caller.post(&path, json!({"iterations": caller.cap})).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    // Nothing here needs the answer: stop a Pro run's 50,000 iterations.
    let id = run["id"].as_i64().unwrap();
    caller
        .post(&format!("/api/runs/{id}/cancel"), Value::Null)
        .await;
    caller.terminal(id).await;
}

/// Previews: the paired preview and the draft preview both stop at the cap
/// (or at their own 5,000, which only a Pro account's cap is above).
async fn previews_hold_the_cap(caller: &Caller, plan: i64) {
    let expected = caller.cap.min(5_000);
    let path = format!("/api/scenarios/{plan}/preview");
    let (status, preview) = caller
        .submit(&path, json!({"changes": [], "iterations": 5_000}))
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(preview["base_run_id"].is_i64(), "paired preview: {preview}");
    assert_eq!(preview["iterations"], expected, "{preview}");
    assert!(preview["edited"].is_object(), "{preview}");

    sqlx::query("UPDATE scenarios SET status = 'draft' WHERE id = ?")
        .bind(plan)
        .execute(&caller.state.db)
        .await
        .unwrap();
    let (status, preview) = caller
        .submit(&path, json!({"changes": [], "iterations": 5_000}))
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(preview["base_run_id"].is_null(), "draft preview: {preview}");
    assert_eq!(preview["iterations"], expected, "{preview}");
    // Asking for nothing gets the draft default, still under the cap.
    let (_, preview) = caller.submit(&path, json!({"changes": []})).await;
    assert_eq!(preview["iterations"], caller.cap.min(400), "{preview}");
}

/// What-if quick: a Pro feature, so guests and Free accounts are turned away
/// before the cap matters; Pro is clamped to the quick maximum, not refused.
async fn what_if_quick_holds_the_cap(caller: &Caller, plan: i64) {
    let path = format!("/api/scenarios/{plan}/what-if/quick");
    let (status, body) = caller
        .submit(&path, json!({"layers": [], "iterations": 100_000}))
        .await;
    if caller.kind == Kind::Pro {
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["steps"].as_array().unwrap().len(), 1, "{body}");
    } else {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
}

/// Analyses: the cap is a 403 below the deployment ceiling and the path's own
/// 2,000 is a 400 above it. Guests and Free accounts cannot run sweeps or
/// rankings at all; a goal seek is theirs to try but stops at the cap.
async fn analyses_hold_the_cap(caller: &Caller, plan: i64) {
    let path = format!("/api/scenarios/{plan}/analyses");
    let solve = |iterations: usize| {
        json!({"kind": "solve", "vary": [], "objective": "max-parameter",
               "min_value": 0.9, "iterations": iterations})
    };
    let (status, body) = caller.post(&path, solve(caller.cap + 1)).await;
    if caller.kind == Kind::Pro {
        // The deployment's own ceiling is the Pro cap.
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    } else {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(
            body["error"]["message"],
            format!("Your plan allows at most {} iterations.", caller.cap),
            "{body}"
        );
    }

    let (status, parameter) = caller
        .post(
            &format!("/api/scenarios/{plan}/parameters"),
            json!({"name": "Rate", "value": {"kind": "Rate", "value": 0.2}}),
        )
        .await;
    if caller.kind != Kind::Pro {
        // Whatever the iterations, ranking is Pro.
        assert!(status.is_success(), "{parameter}");
        let (status, body) = caller
            .post(&path, json!({"kind": "sensitivity", "iterations": 50}))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        return;
    }
    assert_eq!(status, StatusCode::CREATED, "{parameter}");
    let (status, body) = caller
        .post(&path, json!({"kind": "sensitivity", "iterations": 2_001}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, job) = caller
        .submit(&path, json!({"kind": "sensitivity", "iterations": 2_000}))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["id"].as_i64().unwrap();
    caller
        .post(&format!("/api/analyses/{id}/cancel"), Value::Null)
        .await;
}

async fn exercise(kind: Kind) {
    let caller = Caller::new(kind).await;
    let plan = caller.plan().await;

    runs_hold_the_cap(&caller, plan).await;

    // The base run the paired preview needs. A guest names no seed.
    let seed = (kind != Kind::Guest).then_some(7);
    let (status, base) = caller
        .submit(
            &format!("/api/scenarios/{plan}/runs"),
            json!({"iterations": 25, "seed": seed}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{base}");
    let base_id = base["id"].as_i64().unwrap();
    assert_eq!(caller.terminal(base_id).await, "succeeded");
    if kind == Kind::Guest {
        assert_eq!(base["seed"], finplan_server::api::runs::GUEST_SEED);
        assert_eq!(
            caller.seed_of(base_id).await,
            Some(finplan_server::api::runs::GUEST_SEED)
        );
    } else {
        assert_eq!(caller.seed_of(base_id).await, Some(7));
    }

    what_if_quick_holds_the_cap(&caller, plan).await;
    analyses_hold_the_cap(&caller, plan).await;
    previews_hold_the_cap(&caller, plan).await;
}

#[tokio::test]
async fn a_guest_is_held_to_the_guest_cap_on_every_path() {
    exercise(Kind::Guest).await;
}

#[tokio::test]
async fn a_free_account_is_held_to_1000_on_every_path() {
    exercise(Kind::Free).await;
}

#[tokio::test]
async fn a_pro_account_is_held_to_its_cap_and_each_paths_own_maximum() {
    exercise(Kind::Pro).await;
}

#[tokio::test]
async fn a_guest_run_may_still_choose_its_seed() {
    let caller = Caller::new(Kind::Guest).await;
    let plan = caller.plan().await;
    let (status, run) = caller
        .post(
            &format!("/api/scenarios/{plan}/runs"),
            json!({"iterations": 25, "seed": 5}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{run}");
    let id = run["id"].as_i64().unwrap();
    assert_eq!(caller.seed_of(id).await, Some(5));
    caller.terminal(id).await;
}
