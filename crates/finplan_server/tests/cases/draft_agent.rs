//! The drafting agent end to end (spec 16, phase 4): a scripted model drives a
//! whole conversation against the real routes, database, documents and
//! simulator. Read a document, write notes (some added at once, some open, one
//! blocked), ask a question, suspend, answer, resume, finish; then the draft
//! creates and runs like any plan.

use finplan_server::suggest::ai::TransportError;

use super::review_model::{call, end};
use super::*;

const STATEMENT: &str = "Acme Bank statement\nAccount Number: 000123451234\n\
                         Checking Balance: $12,345.00 as of 09/15/2026\n";

const DESCRIPTION: &str = "I was born May 4 1986. I spend $3,000 a month and earn $205,000 a year.";

impl TestApp {
    async fn agent_upload(&self, draft: i64, name: &str, text: &str) -> Value {
        let boundary = "----finplan-agent-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(text.as_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/drafts/{draft}/documents"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            );
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Poll the draft until its state is one of `wanted`.
    async fn await_draft(&self, id: i64, wanted: &[&str]) -> Value {
        for _ in 0..400 {
            let (status, draft) = self.get(&format!("/api/drafts/{id}")).await;
            assert_eq!(status, StatusCode::OK, "{draft}");
            if wanted.contains(&draft["state"].as_str().unwrap()) {
                return draft;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("the draft never reached {wanted:?}");
    }

    async fn agent_db(&self) -> sqlx::SqlitePool {
        sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self._dir.path().join("test.db").display()
        ))
        .await
        .unwrap()
    }

    async fn draft_notes(&self, id: i64) -> Vec<Value> {
        let (status, notes) = self.get(&format!("/api/scenarios/{id}/suggestions")).await;
        assert_eq!(status, StatusCode::OK, "{notes}");
        notes.as_array().unwrap().clone()
    }
}

fn note(kind: &str, title: &str, evidence: Value, changes: Value, extra: Value) -> Value {
    let mut note = json!({
        "kind": kind, "section": "plan", "title": title,
        "reasoning": "From what the person gave.",
        "evidence": evidence,
        "paths": [{"key": "a", "label": "Add it", "recommended": true,
                   "steps": [{"key": "a", "title": "Add it", "changes": changes}]}]
    });
    for (key, value) in extra.as_object().unwrap() {
        note[key] = value.clone();
    }
    note
}

fn message(replies: Vec<Value>) -> Vec<Reply> {
    replies.into_iter().map(Reply::Message).collect()
}

fn poll_titles(notes: &[Value]) -> Vec<String> {
    notes
        .iter()
        .map(|n| n["title"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_scripted_model_drafts_asks_waits_resumes_and_the_draft_runs() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("agent-full@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    let uploaded = app.agent_upload(id, "acme.txt", STATEMENT).await;
    let doc = uploaded[0]["id"].as_i64().unwrap();
    let (_, profiles) = app.get("/api/return-profiles").await;
    let savings = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Savings Account")
        .unwrap()["id"]
        .clone();

    let statement_quote = json!([{"ref": "document", "document_id": doc, "page": 1,
                                  "excerpt": "Checking Balance: $12,345.00"}]);
    let settings = note(
        "add",
        "Born 1986-05-04, planning to age 95",
        json!([{"ref": "description", "excerpt": "born May 4 1986"}]),
        json!([
            {"op": "replace", "target": "scenario", "path": "/birth_date",
             "expect": null, "value": "1986-05-04"},
            {"op": "replace", "target": "scenario", "path": "/duration_years",
             "expect": 30, "value": 69}
        ]),
        json!({"auto_add": true, "key": "settings", "column": "plan"}),
    );
    let account = note(
        "add",
        "Acme checking holds $12,345",
        statement_quote,
        json!([{"op": "add", "target": {"new_account": "acme"}, "path": "",
                "value": {"name": "Acme Checking", "flavor": "Bank",
                          "cash_value": 12345.0, "return_profile_id": savings}}]),
        json!({"auto_add": true, "section": "portfolio", "column": "portfolio", "key": "acme"}),
    );
    let bonus = note(
        "add",
        "Add a yearly bonus of $20,000",
        json!([{"ref": "description", "excerpt": "earn $205,000 a year"}]),
        json!([{"op": "add", "target": {"new_parameter": "bonus"}, "path": "",
                "value": {"name": "Yearly bonus", "value": {"kind": "Money", "value": 20000.0}}}]),
        json!({"blocked_by": ["bonus"], "key": "bonus-note", "column": "plan"}),
    );
    let filing = note(
        "check",
        "Filing status is assumed to be single",
        json!([]),
        json!([{"op": "replace", "target": "scenario", "path": "/description",
                "expect": null, "value": "Assumes a single filer."}]),
        json!({}),
    );

    let first_gate = call("read", "read_document", json!({"id": doc}));
    script.push(vec![
        // Held until the test has looked at the running draft.
        Reply::Gated(first_gate),
        Reply::Message(call("notes", "submit_suggestion", settings)),
    ]);
    script.push(message(vec![call("acct", "submit_suggestion", account)]));
    // One reply holds a note that waits, a check note and the question.
    script.push(vec![Reply::Message(json!({
        "id": "msg", "type": "message", "role": "assistant", "model": "anthropic/claude-sonnet-5.5",
        "stop_reason": "tool_use", "usage": {"input_tokens": 100, "output_tokens": 20},
        "content": [
            {"type": "tool_use", "id": "blocked", "name": "submit_suggestion", "input": bonus},
            {"type": "tool_use", "id": "check", "name": "submit_suggestion", "input": filing},
            {"type": "tool_use", "id": "ask", "name": "ask_user", "input": {"questions": [{
                "key": "bonus", "prompt": "Do you get a yearly bonus?", "answer_type": "choice",
                "options": [{"value": "none", "label": "No bonus"},
                            {"value": "yes", "label": "Yes, add one"}]
            }]}}
        ]
    }))]);

    // No description and no documents would have nothing to draft from; this
    // one has both.
    let (status, started) = app
        .post(
            &format!("/api/drafts/{id}/start"),
            json!({"description": DESCRIPTION}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["state"], "drafting");
    // Started twice is refused, and so is creating a draft still being written.
    let (status, again) = app
        .post(
            &format!("/api/drafts/{id}/start"),
            json!({"description": "again"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    let (status, refused) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    let (_, still) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(still["state"], "drafting");
    assert_eq!(still["scenario"]["status"], "draft");
    script.open_gate();

    // Suspended on its question, with the notes it could write already in.
    let waiting = app.await_draft(id, &["awaiting_answers", "failed"]).await;
    assert_eq!(waiting["state"], "awaiting_answers", "{waiting}");
    assert_eq!(waiting["error"], Value::Null);
    assert_eq!(waiting["progress"], "Waiting for your answers");
    let counts = &waiting["counts"];
    assert_eq!(counts["accounts"], 1, "{waiting}");
    assert_eq!(counts["events"], 0);
    assert_eq!(
        counts["parameters"], 0,
        "the bonus waits, it is not in the plan"
    );
    assert_eq!(counts["notes"], 4);
    assert_eq!(counts["notes_added"], 2);
    assert_eq!(counts["notes_to_confirm"], 1);
    assert_eq!(counts["open_suggestions"], 2);
    let questions = waiting["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0]["key"], "bonus");
    assert_eq!(questions[0]["answer_type"], "choice");
    assert_eq!(questions[0]["options"][1]["label"], "Yes, add one");
    assert_eq!(questions[0]["answer"], Value::Null);
    assert_eq!(waiting["answered"], json!([]));
    let blocked = waiting["blocked_notes"].as_array().unwrap();
    assert_eq!(blocked.len(), 1, "{waiting}");
    assert_eq!(blocked[0]["key"], "bonus-note");
    assert_eq!(blocked[0]["waiting_on"], json!(["bonus"]));
    assert_eq!(waiting["scenario"]["birth_date"], "1986-05-04");
    assert_eq!(waiting["scenario"]["duration_years"], 69);

    // The auto-added facts landed in the draft graph and read as Added.
    let (_, accounts) = app.get(&format!("/api/scenarios/{id}/accounts")).await;
    assert_eq!(accounts[0]["name"], "Acme Checking");
    assert_eq!(accounts[0]["cash_value"], 12345.0);
    let notes = app.draft_notes(id).await;
    let by_title = |title: &str| {
        notes
            .iter()
            .find(|n| n["title"] == title)
            .unwrap_or_else(|| panic!("no note {title}: {:?}", poll_titles(&notes)))
    };
    let acme = by_title("Acme checking holds $12,345");
    assert_eq!(acme["kind"], "add");
    assert_eq!(acme["status"], "applied");
    assert_eq!(acme["auto_added"], true);
    assert_eq!(acme["column"], "portfolio");
    assert_eq!(acme["note_key"], "acme");
    assert_eq!(acme["run_id"], Value::Null);
    assert_eq!(acme["source"], "ai");
    assert_eq!(acme["evidence"][0]["ref"], "document");
    assert_eq!(acme["paths"][0]["steps"][0]["applied"], true);
    let waiting_note = by_title("Add a yearly bonus of $20,000");
    assert_eq!(waiting_note["status"], "open");
    assert_eq!(waiting_note["auto_added"], false);
    assert_eq!(waiting_note["blocked_by"], json!(["bonus"]));
    let check = by_title("Filing status is assumed to be single");
    assert_eq!(check["kind"], "check");
    assert_eq!(check["column"], "to_confirm");
    assert_eq!(check["status"], "open");
    // A note waiting on an answer cannot be added.
    let (status, refused) = app
        .post(
            &format!("/api/suggestions/{}/apply", waiting_note["id"]),
            json!({"path": "a", "through_step": null, "to": "plan"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    // The stored conversation keeps the statement by reference.
    let db = app.agent_db().await;
    let transcript: String =
        sqlx::query_scalar("SELECT transcript_json FROM draft_jobs WHERE scenario_id = ?1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert!(!transcript.contains("Checking Balance"), "{transcript}");
    assert!(!transcript.contains("Acme Bank statement"), "{transcript}");
    assert!(!transcript.contains("000123451234"), "{transcript}");
    assert!(transcript.contains("read_document"));

    let account_id = accounts[0]["id"].as_i64().unwrap();

    // Wrong answers are refused, all together, and nothing is stored.
    for (answers, expect) in [
        (json!({}), "at least one answer"),
        (json!({"bonus": "maybe"}), "pick one of none, yes"),
        (json!({"nope": "x"}), "no question `nope`"),
    ] {
        let (status, refused) = app
            .post(
                &format!("/api/drafts/{id}/answers"),
                json!({"answers": answers}),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains(expect),
            "{refused}"
        );
    }
    let (_, unchanged) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(unchanged["state"], "awaiting_answers");

    // The second segment: the expense from the description, the answer, a
    // simulation, and the end. It can use the account's real id now.
    let living = note(
        "add",
        "Living costs are $3,000 a month",
        json!([{"ref": "description", "excerpt": "I spend $3,000 a month"}]),
        json!([{"op": "add", "target": {"new_event": "living"}, "path": "",
                "value": {"name": "Living", "trigger": {"kind": "Repeating", "interval": "Monthly"},
                          "effects": [{"kind": "Expense", "from_account_id": account_id,
                                       "amount": {"kind": "Fixed", "value": 3000.0}}]}}]),
        json!({"auto_add": true, "key": "living", "column": "plan"}),
    );
    let no_bonus = note(
        "add",
        "The person gets no yearly bonus",
        json!([{"ref": "answer", "question_key": "bonus"}]),
        json!([{"op": "add", "target": {"new_parameter": "nobonus"}, "path": "",
                "value": {"name": "Yearly bonus", "value": {"kind": "Money", "value": 0.0}}}]),
        json!({"auto_add": true, "replaces": "bonus-note"}),
    );
    script.push(message(vec![
        call("living", "submit_suggestion", living),
        call("nobonus", "submit_suggestion", no_bonus),
        call("sim", "simulate_draft", json!({})),
        end(),
    ]));

    let (status, resumed) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"bonus": "none"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["state"], "drafting");
    let ready = app.await_draft(id, &["ready", "failed"]).await;
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["stop"], "finished");
    assert_eq!(ready["progress"], "One note added.");
    assert_eq!(ready["questions"], json!([]));
    assert_eq!(ready["answered"][0]["key"], "bonus");
    assert_eq!(ready["answered"][0]["answer"], "none");
    assert_eq!(ready["blocked_notes"], json!([]));
    let counts = &ready["counts"];
    assert_eq!(counts["accounts"], 1);
    assert_eq!(counts["events"], 1);
    assert_eq!(counts["parameters"], 1, "{ready}");
    assert_eq!(
        counts["notes"], 5,
        "the answered note replaced the blocked one"
    );
    assert_eq!(counts["notes_added"], 4);
    assert_eq!(counts["notes_to_confirm"], 1);
    // The last simulation of the draft as it stands.
    let estimate = &ready["estimate"];
    assert_eq!(estimate["iterations"], 400, "{ready}");
    assert!(estimate["success_rate"].as_f64().is_some(), "{estimate}");
    assert_eq!(estimate["blocked"], Value::Null);
    // The answer is cited; the finished job keeps no conversation.
    let notes = app.draft_notes(id).await;
    assert!(
        !poll_titles(&notes).contains(&"Add a yearly bonus of $20,000".to_owned()),
        "the replaced note is gone"
    );
    let transcript: Option<String> =
        sqlx::query_scalar("SELECT transcript_json FROM draft_jobs WHERE scenario_id = ?1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(transcript, None);
    // Every request asked for zero data retention.
    for request in script.requests.lock().unwrap().iter() {
        assert_eq!(request["provider"]["zdr"], true);
    }

    // Answers to a job that is not waiting are refused.
    let (status, _) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"bonus": "yes"}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Create & run: the drafted plan is a plan, its documents and job go, and
    // its run succeeds.
    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let run_id = created["run"]["id"].as_i64().unwrap();
    assert_eq!(app.await_run(run_id).await, "succeeded");
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM draft_jobs")
        .fetch_one(&db)
        .await
        .unwrap();
    let documents: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!((jobs, documents), (0, 0));
    let (_, plan) = app.get(&format!("/api/scenarios/{id}")).await;
    assert_eq!(plan["status"], "active");
    assert_eq!(plan["birth_date"], "1986-05-04");
}

#[tokio::test]
async fn starting_needs_something_to_draft_from_and_a_model_and_a_failure_can_be_retried() {
    // No model, no drafting.
    let mut none = TestApp::new().await;
    none.login_as("agent-off@example.com").await;
    let (status, _) = none.post("/api/drafts", json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("agent-retry@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    let path = format!("/api/drafts/{id}/start");
    let (status, body) = app.post(&path, json!({"description": "   "})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = app
        .post(&path, json!({"description": "x".repeat(4_001)}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = app
        .post("/api/drafts/999999/start", json!({"description": "x"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Answers before there is a job.
    let (status, _) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"a": "b"}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // The model cannot be reached: failed, with a public reason and no key.
    script.push(vec![Reply::Fail(TransportError::Status {
        status: 400,
        error_type: None,
        message: "bad request".into(),
    })]);
    let (status, _) = app.post(&path, json!({"description": "I am 40."})).await;
    assert_eq!(status, StatusCode::OK);
    let failed = app.await_draft(id, &["failed", "ready"]).await;
    assert_eq!(failed["state"], "failed", "{failed}");
    assert_eq!(failed["error"], "the drafting model could not be reached");
    // A failed draft can still be created (it is empty, so it cannot run) ...
    // ... and drafted again from the start.
    script.push(vec![Reply::Message(end())]);
    let (status, again) = app.post(&path, json!({"description": "I am 41."})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["state"], "drafting");
    assert_eq!(again["error"], Value::Null);
    let ready = app.await_draft(id, &["ready", "failed"]).await;
    assert_eq!(ready["state"], "ready", "{ready}");
    assert_eq!(ready["counts"]["notes"], 0);
    // The description reached the model.
    let requests = script.requests.lock().unwrap().clone();
    let opening = requests.last().unwrap()["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(opening.contains("I am 41."), "{opening}");
    assert!(
        opening.contains(&format!("<today>{}</today>", today_utc())),
        "{opening}"
    );
    assert!(
        opening.contains("Savings Account"),
        "the user's library is in the context"
    );
    assert!(opening.contains("Tax configurations"), "{opening}");
}

#[tokio::test]
async fn a_question_can_be_answered_in_parts_and_deleting_the_draft_stops_the_job() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("agent-parts@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    script.push(message(vec![
        call("ask", "ask_user", json!({"questions": [
            {"key": "mix", "prompt": "One mix or per fund?", "answer_type": "choice",
             "options": [{"value": "per-fund", "label": "Per fund"}, {"value": "one", "label": "One 60/40 mix"}]},
            {"key": "retire", "prompt": "When do you retire?", "answer_type": "date"},
            {"key": "extra", "prompt": "How much extra a month?", "answer_type": "money"},
        ]})),
    ]));
    let (status, _) = app
        .post(
            &format!("/api/drafts/{id}/start"),
            json!({"description": "Hello"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let waiting = app.await_draft(id, &["awaiting_answers", "failed"]).await;
    assert_eq!(
        waiting["questions"].as_array().unwrap().len(),
        3,
        "{waiting}"
    );

    // Two of three answered: still waiting, the answered ones move over.
    let (status, partial) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"mix": "one", "retire": "2050-06-30"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{partial}");
    assert_eq!(partial["state"], "awaiting_answers");
    assert_eq!(partial["questions"].as_array().unwrap().len(), 1);
    assert_eq!(partial["answered"].as_array().unwrap().len(), 2);
    let (status, dup) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"mix": "per-fund"}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{dup}");

    // The last answer resumes the job; the model hangs, and deleting the
    // draft cancels the job and takes it with the draft.
    script.push(vec![Reply::Hang]);
    let (status, resumed) = app
        .post(
            &format!("/api/drafts/{id}/answers"),
            json!({"answers": {"extra": "$250.50"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["state"], "drafting");
    assert_eq!(resumed["answered"].as_array().unwrap().len(), 3);
    assert_eq!(resumed["answered"][2]["answer"], 250.5);
    let (status, _) = app.delete(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM draft_jobs")
        .fetch_one(&app.agent_db().await)
        .await
        .unwrap();
    assert_eq!(jobs, 0);
    let (status, _) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_restart_fails_a_running_job_and_keeps_one_waiting_for_answers() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("agent-restart@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();
    script.push(vec![Reply::Hang]);
    let (status, _) = app
        .post(
            &format!("/api/drafts/{id}/start"),
            json!({"description": "Hello"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.await_draft(id, &["drafting"]).await;
    app.restart().await;
    let draft = app.await_draft(id, &["failed"]).await;
    assert_eq!(draft["error"], "interrupted by a server restart");
}
