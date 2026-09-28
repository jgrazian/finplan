use super::*;

/// A draft has no run to pair against: its preview is one unpaired,
/// whole-plan simulation of the draft with the batch applied.
#[tokio::test]
async fn a_draft_preview_simulates_the_draft_with_the_batch_applied() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("draft-preview@example.com").await;
    let draft = app.start_draft().await;
    let id = draft["id"].as_i64().unwrap();
    let (_, profiles) = app.get("/api/return-profiles").await;
    let profile = profiles[0]["id"].clone();
    let changes = json!([
        {"op": "add", "target": {"new_account": "checking"}, "path": "",
         "value": {"name": "Checking", "flavor": "Bank", "cash_value": 200000.0,
                   "return_profile_id": profile}},
        {"op": "add", "target": {"new_event": "living"}, "path": "",
         "value": {"name": "Living", "trigger": {"kind": "Repeating", "interval": "Monthly"},
                   "effects": [{"kind": "Expense", "from_account_id": {"$new": "checking"},
                                "amount": {"kind": "Fixed", "value": 3000.0}}]}},
    ]);
    let (status, preview) = app
        .post(
            &format!("/api/scenarios/{id}/preview"),
            json!({"changes": changes, "iterations": 30}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["base_run_id"], Value::Null);
    assert_eq!(preview["paired"], false);
    assert_eq!(preview["iterations"], 30);
    assert_eq!(preview["problems"], json!([]));
    assert_eq!(preview["base"], Value::Null);
    assert!(!preview["diff"].as_array().unwrap().is_empty(), "{preview}");
    let edited = &preview["edited"];
    assert!(edited["success_rate"].as_f64().is_some(), "{preview}");
    assert!(edited["real_final"]["p50"].as_f64().is_some(), "{preview}");

    // A batch that cannot apply simulates nothing.
    let (status, bad) = app
        .post(
            &format!("/api/scenarios/{id}/preview"),
            json!({"changes": [{"op": "remove", "target": {"event": 999999}, "path": ""}]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{bad}");
    assert_eq!(bad["edited"], Value::Null);
    assert_eq!(bad["iterations"], 0);
    assert!(!bad["problems"].as_array().unwrap().is_empty());

    // Nothing was written to the draft.
    let (_, after) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(after["counts"]["accounts"], 0);
}
