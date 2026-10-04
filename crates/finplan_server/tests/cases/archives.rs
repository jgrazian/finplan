use super::*;

#[tokio::test]
async fn archives_restore_independent_inputs_retry_and_reject_cycles() {
    let mut app = TestApp::new().await;
    app.login_as("archive@example.com").await;
    let (sid, _, _) = app.seed_scenario().await;
    let (status, archive) = app.get(&format!("/api/scenarios/{sid}/archive")).await;
    assert_eq!(status, StatusCode::OK, "{archive}");
    assert_eq!(archive["version"], 3);
    assert_eq!(archive["plans"][0]["scenario"]["user_id"], "");
    let mut legacy = archive.clone();
    legacy["version"] = json!(2);
    let graph = legacy["plans"][0].as_object_mut().unwrap();
    graph.remove("parameters");
    for row in graph["amounts"].as_object_mut().unwrap().values_mut() {
        row.as_object_mut().unwrap().remove("expression_source");
    }
    for row in graph["triggers"].as_object_mut().unwrap().values_mut() {
        row.as_object_mut().unwrap().remove("parameter_id");
    }
    let (status, restored_legacy) = app
        .post(
            "/api/archives/import",
            json!({
                "archive": legacy, "name_prefix": "Legacy ", "request_id": "legacy-v2"
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{restored_legacy}");
    let (status, preview) = app.post("/api/archives/preview", archive.clone()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["accounts"], 2);
    let request = json!({"archive":archive,"name_prefix":"Restored ","request_id":"archive-1"});
    let (status, imported) = app.post("/api/archives/import", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert_eq!(app.post("/api/archives/import", request).await.1, imported);
    let new_id = imported["scenario_ids"][0].as_i64().unwrap();
    let (status, result) = app
        .post(&format!("/api/scenarios/{new_id}/compile"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    // Restored into the library it came from, the plan uses that library's
    // rows as they are: importing (or moving a plan back) adds no copies.
    let (_, restored) = app.get(&format!("/api/scenarios/{new_id}/archive")).await;
    assert_eq!(
        restored["plans"][0]["scenario"]["tax_config_id"],
        archive["plans"][0]["scenario"]["tax_config_id"]
    );
    let names = |archive: &Value| -> Vec<String> {
        let mut names: Vec<String> = archive["plans"][0]["return_profiles"]
            .as_object()
            .unwrap()
            .values()
            .map(|p| p["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    };
    let (_, profiles) = app.get("/api/return-profiles").await;
    let listed = |profiles: &Value| {
        profiles
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|p| p["name"].as_str())
                    .filter(|n| n.contains(" ["))
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(listed(&profiles), 0, "no tagged copies: {profiles}");
    assert_eq!(names(&restored), names(&archive));
    // A same-named profile with other numbers is never substituted.
    let mut changed = archive.clone();
    let plan = &mut changed["plans"][0];
    let used = plan["bank"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .map(|b| b["return_profile_id"].to_string())
        .unwrap();
    let distribution = plan["return_profiles"][&used]["distribution_id"].to_string();
    let row = &mut plan["distributions"][&distribution];
    for key in ["rate", "mean"] {
        if let Some(x) = row[key].as_f64() {
            row[key] = json!(x + 0.0123);
        }
    }
    let (status, changed_import) = app
        .post(
            "/api/archives/import",
            json!({"archive":changed,"name_prefix":"Changed ","request_id":"archive-changed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{changed_import}");
    let (_, profiles) = app.get("/api/return-profiles").await;
    assert_eq!(listed(&profiles), 1, "one tagged copy: {profiles}");
    let mut bad = archive.clone();
    let distributions = bad["plans"][0]["distributions"].as_object_mut().unwrap();
    let key = distributions.keys().next().unwrap().clone();
    distributions.get_mut(&key).unwrap()["bull_id"] = json!(key.parse::<i64>().unwrap());
    assert_eq!(
        app.post("/api/archives/preview", bad).await.0,
        StatusCode::BAD_REQUEST
    );
    app.login_as("other-archive@example.com").await;
    assert_eq!(
        app.get(&format!("/api/scenarios/{sid}/archive")).await.0,
        StatusCode::NOT_FOUND
    );
    // Sharing an explicitly exported input archive transfers inputs, never owner identity.
    assert_eq!(
        app.post(
            "/api/archives/import",
            json!({"archive":archive,"name_prefix":"Shared ","request_id":"shared-1"})
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn archive_compound_triggers_roundtrip_and_forged_ids_are_rejected() {
    let mut app = TestApp::new().await;
    app.login_as("compound-archive@example.com").await;
    let (sid, _, _) = app.seed_scenario().await;
    let (status, event) = app.post(&format!("/api/scenarios/{sid}/events"), json!({"name":"Compound condition","enabled":true,"fires_once":true,"trigger":{"kind":"And","children":[{"kind":"Date","on_date":"2026-01-01"},{"kind":"Or","children":[{"kind":"Date","on_date":"2027-01-01"},{"kind":"Date","on_date":"2028-01-01"}]}]},"effects":[]})).await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let (_, archive) = app.get(&format!("/api/scenarios/{sid}/archive")).await;
    let (status, preview) = app.post("/api/archives/preview", archive.clone()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, imported) = app
        .post(
            "/api/archives/import",
            json!({"archive":archive,"name_prefix":"Compound copy ","request_id":"compound-1"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let restored = imported["scenario_ids"][0].as_i64().unwrap();
    let (_, events) = app.get(&format!("/api/scenarios/{restored}/events")).await;
    let restored_event = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Compound condition")
        .unwrap();
    assert_eq!(restored_event["trigger"], event["trigger"]);
    for mutation in ["map_mismatch", "duplicate_account", "detached_parent"] {
        let mut bad = archive.clone();
        let graph = &mut bad["plans"][0];
        match mutation {
            "map_mismatch" => {
                let effects = graph["effects"].as_object_mut().unwrap();
                if let Some((_, effect)) = effects.iter_mut().next() {
                    effect["id"] = json!(i64::MIN);
                } else {
                    let triggers = graph["triggers"].as_object_mut().unwrap();
                    triggers.values_mut().next().unwrap()["id"] = json!(i64::MIN);
                }
            }
            "duplicate_account" => {
                let accounts = graph["accounts"].as_array_mut().unwrap();
                accounts.push(accounts[0].clone());
            }
            _ => {
                let triggers = graph["triggers"].as_object_mut().unwrap();
                let root = triggers
                    .values()
                    .find(|r| r["event_id"] == event["id"])
                    .unwrap()["id"]
                    .clone();
                let row = triggers.values_mut().find(|r| r["id"] == root).unwrap();
                row["start_trigger_id"] = root;
            }
        }
        let (status, result) = app.post("/api/archives/preview", bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{mutation}: {result}");
    }
}
