use super::*;
use hantong_domain::mailboxes::{ImapMailboxSnapshot, SpecialUseRole};

async fn discover(
    list: &[u8],
    implicit: bool,
    mode: Mode,
    trusted: bool,
) -> (Result<ImapMailboxSnapshot, Failure>, usize, Vec<Vec<u8>>) {
    let (_cleanup, store) = fixture();
    let endpoint = endpoint(implicit, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &endpoint, b"secret".as_slice())
        .unwrap();
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let accesses = Arc::new(AtomicUsize::new(0));
    let mut task = tokio::spawn(server_with_list(
        right,
        implicit,
        mode,
        log.clone(),
        Some(list.to_vec()),
    ));
    let result = timeout(
        if list.is_empty() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(10)
        },
        windows::run_discovery_connected(
            Source {
                store: &store,
                account: "acct",
                endpoint: &endpoint,
                accesses: Some(&accesses),
            },
            left,
            configs(trusted).0,
        ),
    )
    .await
    .unwrap_or(Err(Failure::Timeout));
    match timeout(Duration::from_millis(300), &mut task).await {
        Ok(joined) => {
            let _ = joined.expect("fixture server must not panic");
        }
        Err(_) => {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        }
    }
    let transcript = log.lock().unwrap().clone();
    (result, accesses.load(Ordering::SeqCst), transcript)
}

#[tokio::test]
async fn ext06_discovery_binds_account_version_and_reads_quoted_literal_roles() {
    for implicit in [true, false] {
        let (result, accesses, log) = discover(
            b"* LIST () \"/\" inbox\r\n* LIST (\\sEnT \\HasNoChildren) \"/\" {5}\r\n&ZeU-\r\nh6 OK listed\r\n",
            implicit, Mode::Good, true,
        ).await;
        let snapshot = result.unwrap();
        assert_eq!(snapshot.account_id(), "acct");
        assert_eq!(snapshot.settings_version(), 7);
        assert_eq!(snapshot.mailboxes().len(), 2);
        assert!(!snapshot.special_use_complete());
        assert_eq!(accesses, 1);
        assert!(
            snapshot
                .mailboxes()
                .iter()
                .any(|m| m.name().display_name() == "日"
                    && m.roles().contains(&SpecialUseRole::Sent))
        );
        assert!(log.iter().any(|line| line == b"h6 LIST \"\" *\r\n"));
        assert_eq!(log.last().unwrap(), b"h7 LOGOUT\r\n");
    }
}

#[tokio::test]
async fn ext03_discovery_tls_and_auth_failures_cannot_start_list() {
    for (trusted, mode, expected, expected_accesses) in [
        (false, Mode::Good, Failure::Tls, 0),
        (true, Mode::RejectAuth, Failure::Authentication, 1),
    ] {
        let (result, accesses, log) = discover(b"h6 OK empty\r\n", true, mode, trusted).await;
        assert_eq!(result, Err(expected));
        assert_eq!(accesses, expected_accesses);
        assert!(
            !log.iter()
                .any(|line| line.windows(4).any(|part| part == b"LIST"))
        );
    }
}

#[tokio::test]
async fn ext06_discovery_requires_complete_list_and_logout_without_partial_success() {
    for list in [
        b"* LIST () NIL INBOX\r\nh6 NO failed\r\n".as_slice(),
        b"* LIST () NIL INBOX\r\nwrong OK done\r\n",
        b"* LIST () NIL INBOX\r\n* LIST () NIL inbox\r\nh6 OK done\r\n",
        b"* LIST () NIL {1025}\r\n",
        b"* 1 FETCH (BODY[] {4294967295}\r\n",
    ] {
        let (result, _, log) = discover(list, true, Mode::Good, true).await;
        assert_eq!(result, Err(Failure::Protocol));
        assert!(log.iter().any(|line| line == b"h6 LIST \"\" *\r\n"));
        assert!(
            !log.iter()
                .any(|line| line.windows(6).any(|p| p == b"LOGOUT"))
        );
    }
    let (result, _, _) = discover(b"h6 OK empty\r\n", true, Mode::MissingBye, true).await;
    assert_eq!(result, Err(Failure::Protocol));
}

#[tokio::test]
async fn ext06_discovery_rejects_untagged_failure_even_with_final_ok() {
    for list in [
        b"* NO unavailable\r\nh6 OK done\r\n".as_slice(),
        b"* LIST () NIL INBOX\r\n* BAD partial\r\nh6 OK done\r\n",
    ] {
        let (result, _, log) = discover(list, true, Mode::Good, true).await;
        assert_eq!(result, Err(Failure::Protocol));
        assert!(
            !log.iter()
                .any(|line| line.windows(6).any(|p| p == b"LOGOUT"))
        );
    }
}

#[tokio::test]
async fn ext06_discovery_uses_only_post_auth_revision_for_name_encoding() {
    let list = "* LIST () NIL {6}\r\n한글\r\nh6 OK done\r\n".as_bytes();
    let (rev2, _, _) = discover(list, true, Mode::Rev2, true).await;
    assert_eq!(rev2.unwrap().mailboxes()[0].name().display_name(), "한글");
    let (dual, _, _) = discover(list, true, Mode::DualRevision, true).await;
    assert_eq!(dual, Err(Failure::Protocol));
}

#[tokio::test]
async fn ext06_discovery_caps_mailbox_count_and_status_frames_without_truncation() {
    let mut maximum = Vec::new();
    for i in 0..1024 {
        maximum.extend_from_slice(format!("* LIST () NIL box-{i:04}\r\n").as_bytes());
    }
    let mut success = maximum.clone();
    success.extend_from_slice(b"h6 OK done\r\n");
    assert_eq!(
        discover(&success, true, Mode::Good, true)
            .await
            .0
            .unwrap()
            .mailboxes()
            .len(),
        1024
    );
    maximum.extend_from_slice(b"* LIST () NIL excess\r\nh6 OK done\r\n");
    let mut flood = b"* OK status\r\n".repeat(1056);
    flood.extend_from_slice(b"h6 OK done\r\n");
    for response in [maximum, flood] {
        let (result, _, log) = discover(&response, true, Mode::Good, true).await;
        assert_eq!(result, Err(Failure::Protocol));
        assert!(
            !log.iter()
                .any(|line| line.windows(6).any(|p| p == b"LOGOUT"))
        );
    }
}

#[tokio::test]
async fn ext06_discovery_plaintext_budget_is_cumulative_across_list_frames() {
    let mut frame = b"* OK ".to_vec();
    frame.extend_from_slice(&vec![b'x'; 16000]);
    frame.extend_from_slice(b"\r\n");
    let mut response = frame.repeat(140);
    response.extend_from_slice(b"h6 OK done\r\n");
    let (result, accesses, _) = discover(&response, true, Mode::Good, true).await;
    assert_eq!(result, Err(Failure::Protocol));
    assert_eq!(accesses, 1);
}

#[tokio::test]
async fn ext06_discovery_cancel_drops_the_only_connection_without_retry() {
    let (result, accesses, log) = discover(b"", true, Mode::Good, true).await;
    assert_eq!(result, Err(Failure::Timeout));
    assert_eq!(accesses, 1);
    assert_eq!(
        log.iter()
            .filter(|line| line.as_slice() == b"h6 LIST \"\" *\r\n")
            .count(),
        1
    );
    assert!(
        !log.iter()
            .any(|line| line.windows(6).any(|p| p == b"LOGOUT"))
    );
}

#[tokio::test]
async fn ext06_discovery_literal_body_cannot_inject_another_response() {
    let mut response = b"* LIST () NIL {1024}\r\n".to_vec();
    response.extend_from_slice(&vec![b'x'; 1024]);
    response.extend_from_slice(b"\r\nh6 OK done\r\n");
    let result = discover(&response, true, Mode::Good, true).await.0.unwrap();
    assert_eq!(result.mailboxes()[0].name().wire_name().len(), 1024);
    let (result, _, _) = discover(
        b"* LIST () NIL {4}\r\na\r\nb\r\nh6 OK done\r\n",
        true,
        Mode::Good,
        true,
    )
    .await;
    assert_eq!(result, Err(Failure::Protocol));
}
