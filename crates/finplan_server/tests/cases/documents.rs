//! Documents on a draft: upload, limits, cascade and retention (spec 16,
//! phase 3). Originals are never stored, so most of this checks what is *not*
//! anywhere: in the database, or on disk after the draft is gone.

use super::*;

const SSN: &str = "512-44-9021";

const CSV: &str = "Date,Description,Amount\n\
                   01/05/2026,RENT SUNSET APTS,-1450.00\n\
                   01/15/2026,PAYROLL ACME,3641.20\n\
                   01/18/2026,ZELLE TO 512-44-9021,-40.00\n";

const OFX: &str = "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n<OFX><SIGNONMSGSRSV1><SONRS>\
    <FI><ORG>USAA<FID>1</FI></SONRS></SIGNONMSGSRSV1><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>USD\
    <BANKACCTFROM><BANKID>314074269<ACCTID>00012345671234<ACCTTYPE>CHECKING</BANKACCTFROM>\
    <BANKTRANLIST><STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260105<TRNAMT>-1450.00<FITID>1<NAME>RENT</STMTTRN>\
    </BANKTRANLIST><LEDGERBAL><BALAMT>12004.55<DTASOF>20260131</LEDGERBAL></STMTRS></STMTTRNRS>\
    </BANKMSGSRSV1></OFX>\n";

const STUB: &str = "Earnings Statement  Pay Period 09/01/2026 - 09/14/2026\n\
                    Employee SSN 512-44-9021  Date of birth 03/14/1984\n\
                    Gross Pay 5,461.54  Net Pay 3,641.20  YTD 136,538.50\n";

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&[7; 64]);
    bytes
}

fn pdf(pages: &[&str]) -> Vec<u8> {
    use lopdf::content::{Content, Operation};
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for text in pages {
        let mut ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 10.into()]),
            Operation::new("TL", vec![14.into()]),
            Operation::new("Td", vec![50.into(), 750.into()]),
        ];
        for line in text.lines() {
            ops.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            ops.push(Operation::new("T*", vec![]));
        }
        ops.push(Operation::new("ET", vec![]));
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        });
        kids.push(Object::from(page));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

impl TestApp {
    /// POST a multipart body of file parts to a draft's documents.
    async fn upload(&self, draft: i64, files: &[(&str, Vec<u8>)]) -> (StatusCode, Value) {
        self.upload_to(&format!("/api/drafts/{draft}/documents"), files)
            .await
    }

    async fn upload_to(&self, path: &str, files: &[(&str, Vec<u8>)]) -> (StatusCode, Value) {
        let boundary = "----finplan-test-boundary";
        let mut body: Vec<u8> = Vec::new();
        for (name, bytes) in files {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                     filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let mut request = Request::builder().method("POST").uri(path).header(
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
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body");
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn documents(&self, draft: i64) -> Value {
        let (status, list) = self.get(&format!("/api/drafts/{draft}/documents")).await;
        assert_eq!(status, StatusCode::OK, "{list}");
        list
    }

    /// A read-only look at the database file, for what must not be in it.
    async fn database(&self) -> sqlx::SqlitePool {
        sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self._dir.path().join("test.db").display()
        ))
        .await
        .unwrap()
    }

    async fn document_rows(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM documents")
            .fetch_one(&self.database().await)
            .await
            .unwrap()
    }

    /// Files held for the model, across all drafts.
    fn held_files(&self) -> Vec<std::path::PathBuf> {
        let root = self._dir.path().join("draft-files");
        let mut out = Vec::new();
        let Ok(drafts) = std::fs::read_dir(&root) else {
            return out;
        };
        for draft in drafts.flatten() {
            if let Ok(files) = std::fs::read_dir(draft.path()) {
                out.extend(files.flatten().map(|f| f.path()));
            }
        }
        out
    }

    async fn draft_app(email: &str) -> (Self, i64) {
        let script = Script::new(vec![]);
        let mut app = TestApp::with_review_ai(script.client()).await;
        app.login_as(email).await;
        let id = app.start_draft().await["id"].as_i64().unwrap();
        (app, id)
    }
}

#[tokio::test]
async fn csv_ofx_pdf_and_text_are_read_redacted_and_listed() {
    let (app, id) = TestApp::draft_app("docs-formats@example.com").await;
    let statement = pdf(&[
        "Fidelity Investments Brokerage Account Statement\nAccount Number: 123456789\n\
         Total Account Value $52,310.44 as of 01/31/2026\nHoldings Symbol Shares Market Value\n\
         Cost basis unrealized gain dividends",
        "Page two.\nSSN 512-44-9021",
    ]);
    let (status, uploaded) = app
        .upload(
            id,
            &[
                ("jan.csv", CSV.as_bytes().to_vec()),
                ("usaa.qfx", OFX.as_bytes().to_vec()),
                ("statement.pdf", statement),
                ("paystub_sept.txt", STUB.as_bytes().to_vec()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let docs = uploaded.as_array().unwrap();
    assert_eq!(docs.len(), 4);
    let kinds: Vec<&str> = docs.iter().map(|d| d["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "transactions",
            "bank_statement",
            "brokerage_statement",
            "pay_stub"
        ]
    );
    assert!(docs.iter().all(|d| d["status"] == "parsed"), "{docs:?}");
    assert_eq!(docs[2]["pages"], 2);
    assert_eq!(docs[2]["mime"], "application/pdf");
    assert_eq!(docs[3]["birth_date_hint"], "1984-03-14");
    assert!(
        docs[0]["snippet"]
            .as_str()
            .unwrap()
            .starts_with("Date | Description | Amount")
    );
    assert!(
        docs[1]["snippet"]
            .as_str()
            .unwrap()
            .contains("Institution: USAA")
    );

    // The manifest is the same list, and a snapshot of the draft counts them.
    assert_eq!(app.documents(id).await, uploaded);
    let (_, draft) = app.get(&format!("/api/drafts/{id}")).await;
    assert_eq!(draft["document_count"], 4);
    assert_eq!(draft["retain_documents"], false);

    // What is stored is redacted text and a hash: no SSN, no full account
    // number, no original bytes anywhere in the database file.
    let db = app.database().await;
    let texts: Vec<String> =
        sqlx::query_scalar("SELECT text || coalesce(data_json, '') FROM documents")
            .fetch_all(&db)
            .await
            .unwrap();
    for text in &texts {
        assert!(!text.contains(SSN), "{text}");
        assert!(
            !text.contains("00012345671234") && !text.contains("123456789"),
            "{text}"
        );
    }
    let hashes: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM documents")
        .fetch_all(&db)
        .await
        .unwrap();
    assert!(hashes.iter().all(|h| h.len() == 64));
    assert!(app.held_files().is_empty(), "text documents hold no file");
}

#[tokio::test]
async fn an_image_is_held_on_disk_until_the_document_or_draft_goes() {
    let (app, id) = TestApp::draft_app("docs-image@example.com").await;
    let (status, uploaded) = app
        .upload(
            id,
            &[
                ("shot.png", png()),
                ("other.jpg", {
                    let mut j = vec![0xff, 0xd8, 0xff, 0xe0];
                    j.extend_from_slice(&[1; 40]);
                    j
                }),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    assert_eq!(uploaded[0]["kind"], "image");
    assert_eq!(uploaded[0]["status"], "image_unredacted");
    assert!(uploaded[0]["note"].as_str().unwrap().contains("unredacted"));
    assert_eq!(app.held_files().len(), 2);
    // Never in the database.
    let db = app.database().await;
    let blob_free: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM documents WHERE text <> '' OR data_json IS NOT NULL",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(blob_free, 0);
    let path = &app.held_files()[0];
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Removing one document removes its file.
    let doc = uploaded[0]["id"].as_i64().unwrap();
    let (status, _) = app
        .delete(&format!("/api/drafts/{id}/documents/{doc}"))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.held_files().len(), 1);
    let (status, _) = app
        .delete(&format!("/api/drafts/{id}/documents/{doc}"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Cancelling the draft removes the rest, and the documents cascade.
    let (status, _) = app.delete(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.held_files().is_empty());
    assert_eq!(app.document_rows().await, 0);
}

#[tokio::test]
async fn deleting_or_replacing_a_draft_deletes_its_documents() {
    let (app, id) = TestApp::draft_app("docs-cascade@example.com").await;
    let (status, _) = app
        .upload(
            id,
            &[("a.csv", CSV.as_bytes().to_vec()), ("shot.png", png())],
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.document_rows().await, 2);
    assert_eq!(app.held_files().len(), 1);

    // Starting another draft replaces this one.
    let second = app.start_draft().await["id"].as_i64().unwrap();
    assert_ne!(second, id);
    assert_eq!(app.document_rows().await, 0);
    assert!(app.held_files().is_empty());
    let (status, _) = app.get(&format!("/api/drafts/{id}/documents")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = app
        .upload(second, &[("a.csv", CSV.as_bytes().to_vec())])
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app.delete(&format!("/api/drafts/{second}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.document_rows().await, 0);
}

#[tokio::test]
async fn deleting_a_draft_through_the_scenario_route_removes_its_held_files() {
    let (app, id) = TestApp::draft_app("docs-scenario-delete@example.com").await;
    let (status, _) = app.upload(id, &[("shot.png", png())]).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(app.held_files().len(), 1);

    let (status, _) = app.delete(&format!("/api/scenarios/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        app.held_files().is_empty(),
        "the original outlived its draft"
    );
    assert_eq!(app.document_rows().await, 0);
}

#[tokio::test]
async fn orphaned_files_are_purged_and_live_drafts_kept() {
    let (app, id) = TestApp::draft_app("docs-purge@example.com").await;
    app.upload(id, &[("shot.png", png())]).await;
    let root = app._dir.path().join("draft-files");
    std::fs::create_dir_all(root.join("987654")).unwrap();
    std::fs::write(root.join("987654").join("stale"), b"x").unwrap();
    let db = app.database().await;
    let purged = finplan_server::documents::images::purge_orphans(&db, &root)
        .await
        .unwrap();
    assert_eq!(purged, 1);
    assert_eq!(app.held_files().len(), 1, "the live draft's file stays");
    assert!(!root.join("987654").exists());
}

#[tokio::test]
async fn limits_count_across_the_draft_and_a_refused_request_stores_nothing() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_draft_config(script.client(), |d| {
        d.free_max_files = 3;
        d.pro_max_files = 3;
        d.free_max_bytes = 4_000;
        d.pro_max_bytes = 4_000;
        d.free_max_pages = 3;
        d.pro_max_pages = 3;
    })
    .await;
    app.login_as("docs-limits@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();

    // Files: three fit, a fourth (across requests) does not.
    let (status, _) = app.upload(id, &[("a.csv", CSV.as_bytes().to_vec())]).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = app
        .upload(
            id,
            &[
                ("b.txt", STUB.as_bytes().to_vec()),
                ("c.qfx", OFX.as_bytes().to_vec()),
                ("d.txt", b"Gross pay net pay ytd\n".to_vec()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at most 3 files")
    );
    assert_eq!(
        app.documents(id).await.as_array().unwrap().len(),
        1,
        "all or nothing"
    );

    // Pages: PDF pages and images count; text files count one each.
    let (status, body) = app
        .upload(id, &[("long.pdf", pdf(&["one", "two", "three"]))])
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(body["error"]["message"].as_str().unwrap().contains("pages"));
    let (status, _) = app.upload(id, &[("two.pdf", pdf(&["one", "two"]))]).await;
    assert_eq!(status, StatusCode::CREATED);
    // Three pages used (1 csv + 2 pdf): an image is one too many.
    let (status, _) = app.upload(id, &[("shot.png", png())]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(
        app.held_files().is_empty(),
        "a refused image is not left on disk"
    );

    // Bytes: the request body is capped by the draft's remaining allowance.
    let (status, _) = app.delete(&format!("/api/drafts/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let id = app.start_draft().await["id"].as_i64().unwrap();
    let big = vec![b'a'; 5_000];
    let (status, body) = app.upload(id, &[("big.txt", big)]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(app.document_rows().await, 0);
    // A request past the route's own body cap (the larger tier's bytes plus
    // multipart framing) is refused before it is read.
    let (status, body) = app.upload(id, &[("huge.txt", vec![b'a'; 1_200_000])]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    let (status, _) = app.upload(id, &[("ok.txt", vec![b'a'; 3_000])]).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app.upload(id, &[("more.txt", vec![b'b'; 1_500])]).await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "3,000 + 1,500 is over 4,000"
    );
}

#[tokio::test]
async fn uploads_are_refused_for_the_wrong_file_the_wrong_draft_and_the_wrong_user() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("docs-refused@example.com").await;
    let id = app.start_draft().await["id"].as_i64().unwrap();

    let (status, body) = app
        .upload(id, &[("a.zip", b"PK\x03\x04data".to_vec())])
        .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");
    let (status, _) = app.upload(id, &[("empty.csv", Vec::new())]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app.upload(id, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = app.upload(id, &[("a.csv", CSV.as_bytes().to_vec())]).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = app
        .upload(id, &[("copy.csv", CSV.as_bytes().to_vec())])
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // An unreadable PDF is stored as failed, with the reason.
    let (status, bad) = app
        .upload(id, &[("broken.pdf", b"%PDF-1.4 nope".to_vec())])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{bad}");
    assert_eq!(bad[0]["status"], "failed");
    assert!(
        bad[0]["note"]
            .as_str()
            .unwrap()
            .contains("could not be read")
    );

    // A real plan is not a draft.
    let (plan, _, _) = app.seed_scenario().await;
    let (status, _) = app
        .upload(plan, &[("a.csv", CSV.as_bytes().to_vec())])
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get(&format!("/api/drafts/{plan}/documents")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Another user's draft is not found.
    let mut other = TestApp::with_review_ai(script.client()).await;
    other.router = app.router.clone();
    other.login_as("docs-other@example.com").await;
    let (status, _) = other
        .upload(id, &[("b.csv", b"Date,Amount,Memo\n".to_vec())])
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = other.get(&format!("/api/drafts/{id}/documents")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = other.delete(&format!("/api/drafts/{id}/documents/1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn documents_are_deleted_when_the_draft_is_created_unless_retained() {
    let (app, id) = TestApp::draft_app("docs-retain-no@example.com").await;
    app.upload(
        id,
        &[("a.csv", CSV.as_bytes().to_vec()), ("shot.png", png())],
    )
    .await;
    assert_eq!(app.document_rows().await, 2);
    assert_eq!(app.held_files().len(), 1);
    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(app.document_rows().await, 0);
    assert!(app.held_files().is_empty());
}

#[tokio::test]
async fn retained_documents_stay_with_the_plan_but_held_originals_never_do() {
    let (app, id) = TestApp::draft_app("docs-retain@example.com").await;
    app.upload(
        id,
        &[("a.csv", CSV.as_bytes().to_vec()), ("shot.png", png())],
    )
    .await;

    // The choice can be made after documents are attached ...
    let (status, draft) = app
        .patch(
            &format!("/api/drafts/{id}"),
            json!({"retain_documents": true}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["retain_documents"], true);
    let docs = app.documents(id).await;
    assert!(docs.as_array().unwrap().iter().all(|d| d["retain"] == true));
    // ... and undone.
    let (_, draft) = app
        .patch(
            &format!("/api/drafts/{id}"),
            json!({"retain_documents": false}),
        )
        .await;
    assert_eq!(draft["retain_documents"], false);
    app.patch(
        &format!("/api/drafts/{id}"),
        json!({"retain_documents": true}),
    )
    .await;
    // Later uploads follow the choice.
    app.upload(id, &[("b.txt", STUB.as_bytes().to_vec())]).await;
    assert!(
        app.documents(id)
            .await
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["retain"] == true)
    );

    let (status, created) = app
        .post(&format!("/api/drafts/{id}/create"), json!({}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    // The redacted text and hash stay for the later refresh; the image, which
    // was never read into text, has nothing to keep and its file is gone.
    let db = app.database().await;
    let kept: Vec<(String, String)> =
        sqlx::query_as("SELECT filename, status FROM documents ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        kept,
        [
            ("a.csv".to_string(), "parsed".to_string()),
            ("b.txt".to_string(), "parsed".to_string())
        ]
    );
    assert!(app.held_files().is_empty());

    // The plan owns them now: deleting it deletes them.
    let (status, _) = app.delete(&format!("/api/scenarios/{id}")).await;
    assert!(status.is_success(), "{status}");
    assert_eq!(app.document_rows().await, 0);
}

#[tokio::test]
async fn a_draft_can_be_started_with_retention_chosen() {
    let script = Script::new(vec![]);
    let mut app = TestApp::with_review_ai(script.client()).await;
    app.login_as("docs-retain-start@example.com").await;
    let (status, draft) = app
        .post("/api/drafts", json!({"retain_documents": true}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(draft["retain_documents"], true);
    let id = draft["id"].as_i64().unwrap();
    let (_, uploaded) = app.upload(id, &[("a.csv", CSV.as_bytes().to_vec())]).await;
    assert_eq!(uploaded[0]["retain"], true);
}
