use super::*;
use hantong_contracts::{google_write::AttachmentSnapshot, mail_send::DraftCreateRequest};
use std::cell::RefCell;

struct Fake {
    response: RefCell<Option<Result<http::Response, ErrorCode>>>,
    calls: RefCell<Vec<(String, Option<Vec<u8>>)>>,
}
impl Fake {
    fn response(status: u32, value: Value) -> Self {
        Self {
            response: RefCell::new(Some(Ok(http::Response {
                status,
                body: SensitiveBuffer(serde_json::to_vec(&value).unwrap()),
            }))),
            calls: RefCell::new(vec![]),
        }
    }
    fn lost() -> Self {
        Self {
            response: RefCell::new(Some(Err(ErrorCode::Offline))),
            calls: RefCell::new(vec![]),
        }
    }
}
impl http::Transport for Fake {
    fn request(&self, request: http::Request<'_>) -> Result<http::Response, ErrorCode> {
        http::validate(&request)?;
        assert_eq!(request.host, http::Host::Gmail);
        self.calls
            .borrow_mut()
            .push((request.path.into(), request.post.map(|b| b.to_vec())));
        self.response
            .borrow_mut()
            .take()
            .expect("exactly one provider request")
    }
}
fn access() -> SecretText {
    SecretText::from_string("synthetic-access".into()).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn compose() -> compose::GoogleMimeMessage {
    let draft = DraftCreateRequest::new(
        &format!("google_{}", "a".repeat(64)),
        vec!["receiver@example.invalid".into()],
        "한통 메일 송수신 테스트".into(),
        "한통 메일 송수신 테스트".into(),
        "operation",
        "idempotency",
        0,
        [1; 32],
    )
    .unwrap();
    let attachment = AttachmentSnapshot::new(
        "hantong-test.txt".into(),
        "text/plain".into(),
        "한통 첨부 테스트".as_bytes().to_vec(),
    )
    .unwrap();
    compose::encode(
        &ComposeSnapshot::new(draft, vec![attachment]).unwrap(),
        "sender@example.invalid",
        &[1; 32],
        0,
    )
    .unwrap()
}

#[test]
fn sync07_send_has_one_attempt_and_never_retries_ambiguous_outcomes() {
    for (fake, expected) in [
        (
            Fake::response(200, serde_json::json!({"id":"message1"})),
            GmailWriteOutcome::Accepted,
        ),
        (
            Fake::response(
                400,
                serde_json::json!({"error":"untrusted-provider-detail"}),
            ),
            GmailWriteOutcome::Rejected,
        ),
        (
            Fake::response(
                503,
                serde_json::json!({"error":"untrusted-provider-detail"}),
            ),
            GmailWriteOutcome::Unknown,
        ),
        (
            Fake::response(200, serde_json::json!({"not_an_id":"untrusted"})),
            GmailWriteOutcome::Unknown,
        ),
        (Fake::lost(), GmailWriteOutcome::Unknown),
    ] {
        let encoded = compose();
        assert_eq!(
            send(
                &fake,
                &encoded,
                &access(),
                deadline(),
                &AtomicBool::new(false)
            )
            .unwrap(),
            expected
        );
        let calls = fake.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "/gmail/v1/users/me/messages/send");
        let value: Value = serde_json::from_slice(calls[0].1.as_ref().unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 1);
        let decoded = crypto::decode64(value["raw"].as_str().unwrap(), 65536).unwrap();
        assert_eq!(decoded, encoded.as_bytes());
        assert!(
            !std::str::from_utf8(calls[0].1.as_ref().unwrap())
                .unwrap()
                .contains("synthetic-access")
        );
    }
    let fake = Fake::lost();
    assert_eq!(
        send(
            &fake,
            &compose(),
            &access(),
            deadline(),
            &AtomicBool::new(true)
        )
        .unwrap(),
        GmailWriteOutcome::Unknown
    );
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn ai09_transport_rejects_delete_trash_spam_arbitrary_paths_and_body_injection() {
    let body = br#"{"raw":"c3ludGhldGlj"}"#;
    let cancel = AtomicBool::new(false);
    for path in [
        "/gmail/v1/users/other/messages/send",
        "/gmail/v1/users/me/messages/message1/trash",
        "/gmail/v1/users/me/messages/batchDelete",
        "/gmail/v1/users/me/drafts/draft1",
        "/gmail/v1/users/me/messages/send?alternate=1",
        "/gmail/v1/users/me/messages/../send",
    ] {
        let request = http::Request {
            host: http::Host::Gmail,
            path,
            post: Some(body),
            bearer: Some("synthetic-access"),
            limit: 65536,
            deadline: deadline(),
            cancel: &cancel,
        };
        assert_eq!(http::validate(&request), Err(ErrorCode::InvalidInput));
    }
    for value in [
        serde_json::json!({"addLabelIds":["TRASH"],"removeLabelIds":[]}),
        serde_json::json!({"addLabelIds":[],"removeLabelIds":["SPAM"]}),
        serde_json::json!({"addLabelIds":["UNREAD"],"removeLabelIds":["UNREAD"]}),
        serde_json::json!({"addLabelIds":["STARRED"],"removeLabelIds":[],"delete":true}),
    ] {
        let body = serde_json::to_vec(&value).unwrap();
        let request = http::Request {
            host: http::Host::Gmail,
            path: "/gmail/v1/users/me/messages/message1/modify",
            post: Some(&body),
            bearer: Some("synthetic-access"),
            limit: 65536,
            deadline: deadline(),
            cancel: &cancel,
        };
        assert_eq!(http::validate(&request), Err(ErrorCode::InvalidInput));
    }
}

#[test]
fn apr01_label_change_and_remote_draft_response_are_checked_without_provider_reflection() {
    let labels = MessageLabelChange::new(
        format!("google_{}", "a".repeat(64)),
        "message1".into(),
        vec!["Label_1".into()],
        vec!["UNREAD".into()],
    )
    .unwrap();
    let fake = Fake::response(200, serde_json::json!({"id":"message1"}));
    assert_eq!(
        change_labels(
            &fake,
            &labels,
            &access(),
            deadline(),
            &AtomicBool::new(false)
        )
        .unwrap(),
        GmailWriteOutcome::Accepted
    );
    assert_eq!(
        fake.calls.borrow()[0].0,
        "/gmail/v1/users/me/messages/message1/modify"
    );
    let fake = Fake::response(200, serde_json::json!({"id":"different_message"}));
    assert_eq!(
        change_labels(
            &fake,
            &labels,
            &access(),
            deadline(),
            &AtomicBool::new(false)
        )
        .unwrap(),
        GmailWriteOutcome::Unknown
    );
    let fake = Fake::response(
        200,
        serde_json::json!({"id":"draft1","message":{"id":"message1"}}),
    );
    assert_eq!(
        post_raw(
            &fake,
            &compose(),
            &access(),
            deadline(),
            &AtomicBool::new(false),
            true
        )
        .unwrap(),
        GmailWriteOutcome::Accepted
    );
    let value: Value = serde_json::from_slice(fake.calls.borrow()[0].1.as_ref().unwrap()).unwrap();
    assert!(value["message"]["raw"].is_string());
}

#[test]
fn sec02_attachment_receive_checks_exact_size_and_decoded_synthetic_bytes() {
    let bytes = "한통 첨부 테스트".as_bytes();
    let fake = Fake::response(
        200,
        serde_json::json!({"size":bytes.len(),"data":crypto::base64url(bytes)}),
    );
    let value = attachment(
        &fake,
        "message1",
        "attachment1",
        23,
        &access(),
        deadline(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(value.as_bytes(), bytes);
    assert_eq!(format!("{value:?}"), "GoogleAttachmentData(<redacted>)");
    for json in [
        serde_json::json!({"size":24,"data":crypto::base64url(bytes)}),
        serde_json::json!({"size":23,"data":crypto::base64url(b"wrong")}),
        serde_json::json!({"size":23,"data":"not-base64!"}),
    ] {
        let fake = Fake::response(200, json);
        assert!(
            attachment(
                &fake,
                "message1",
                "attachment1",
                23,
                &access(),
                deadline(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}

#[test]
fn sec08_attachment_diagnostic_rejects_long_id_without_leaking_values() {
    let id = "synthetic-private-attachment".repeat(200);
    let fake = Fake::response(200, serde_json::json!({"size":23,"data":"private-body"}));
    let (result, lines) = diagnostics::capture(|| {
        attachment(
            &fake,
            "private-message",
            &id,
            23,
            &access(),
            deadline(),
            &AtomicBool::new(false),
        )
    });
    assert_eq!(result.err(), Some(ErrorCode::InvalidInput));
    assert!(fake.calls.borrow().is_empty());
    assert_eq!(lines.len(), 1);
    let record: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(record["event"], "google_attachment_stage");
    assert_eq!(record["stage"], "attachment_identifier_length");
    assert_eq!(record["error_code"], "INVALID_INPUT");
    for private in [
        id.as_str(),
        "private-message",
        "private-body",
        "synthetic-access",
    ] {
        assert!(!lines[0].contains(private));
    }
}

#[test]
fn sec02_attachment_long_identifier_is_bounded_and_not_a_message_or_injection() {
    let bytes = "한통 첨부 테스트".as_bytes();
    for length in [257, 512, http::MAX_ATTACHMENT_ID_BYTES] {
        let id = "a".repeat(length);
        let fake = Fake::response(
            200,
            serde_json::json!({"size":bytes.len(),"data":crypto::base64url(bytes)}),
        );
        let data = attachment(
            &fake,
            "message1",
            &id,
            bytes.len(),
            &access(),
            deadline(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(data.as_bytes(), bytes);
        assert_eq!(fake.calls.borrow().len(), 1);
        assert!(!http::provider_id(&id));
    }
    for id in [
        "../other",
        "valid?x=1",
        "valid%2Fother",
        "valid/other",
        "valid#frag",
        "valid\r\n",
        "한글",
        "",
    ] {
        let fake = Fake::response(200, serde_json::json!({}));
        assert!(
            attachment(
                &fake,
                "message1",
                id,
                23,
                &access(),
                deadline(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(fake.calls.borrow().is_empty());
    }
}
