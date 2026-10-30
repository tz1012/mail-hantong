use super::*;
use hantong_domain::imap_recovery::{ImapUidRecoveryPage, ImapUidRecoveryRequest};
fn recovery_request(previous: u32) -> ImapUidRecoveryRequest {
    ImapUidRecoveryRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
        previous,
    )
    .unwrap()
}
async fn run_recovery(
    req: ImapUidRecoveryRequest,
    steps: Script,
    implicit: bool,
    mode: Mode,
    trusted: bool,
    duration: Duration,
) -> (
    Result<ImapUidRecoveryPage, ImapUidPageError>,
    usize,
    Vec<Vec<u8>>,
) {
    let (_cleanup, store) = fixture();
    let endpoint = endpoint(implicit, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &endpoint, b"secret".as_slice())
        .unwrap();
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let accesses = AtomicUsize::new(0);
    let mut task = tokio::spawn(server_with_operations(
        right,
        implicit,
        mode,
        log.clone(),
        None,
        Some(steps),
    ));
    let result = timeout(
        duration,
        windows::run_uid_recovery_connected(
            Source {
                store: &store,
                account: "acct",
                endpoint: &endpoint,
                accesses: Some(&accesses),
            },
            left,
            configs(trusted).0,
            &req,
        ),
    )
    .await
    .unwrap_or(Err(ImapUidPageError::Probe(Failure::Timeout)));
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
async fn good(
    req: ImapUidRecoveryRequest,
    steps: Script,
) -> (
    Result<ImapUidRecoveryPage, ImapUidPageError>,
    usize,
    Vec<Vec<u8>>,
) {
    run_recovery(req, steps, true, Mode::Good, true, Duration::from_secs(10)).await
}
#[tokio::test]
async fn sync02_recovery_same_validity_stops_before_fetch() {
    let (result, accesses, log) = good(recovery_request(7), script(100, 64, vec![], 100)).await;
    assert_eq!(result, Err(ImapUidPageError::CursorInvalid));
    assert_eq!(accesses, 1);
    assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
}
#[tokio::test]
async fn ext04_recovery_new_namespace_reads_one_first_page_under_both_tls_modes() {
    for implicit in [true, false] {
        let mut messages = message(999, 64);
        messages.extend(message(1, 2));
        let (result, accesses, log) = run_recovery(
            recovery_request(99),
            script(100, 64, messages, 1000),
            implicit,
            Mode::Good,
            true,
            Duration::from_secs(10),
        )
        .await;
        let recovery = result.unwrap();
        let page = recovery.page();
        assert_eq!(accesses, 1);
        assert_eq!(recovery.request().previous_uid_validity(), 99);
        assert_eq!(page.uid_validity(), 7);
        assert_eq!(page.request().expected_uid_validity(), Some(7));
        assert_eq!(page.request().after_uid(), 0);
        assert_eq!(page.covered_through_uid(), 64);
        assert_eq!(
            page.messages().iter().map(|m| m.uid()).collect::<Vec<_>>(),
            [2, 64]
        );
        assert_eq!(
            log.iter()
                .filter(|line| line.windows(9).any(|s| s == b"UID FETCH"))
                .count(),
            1
        );
        assert_eq!(
            log.iter()
                .filter(|line| line.windows(7).any(|s| s == b"EXAMINE"))
                .count(),
            2
        );
    }
}
#[tokio::test]
async fn sync02_recovery_new_empty_namespace_skips_fetch_and_keeps_epoch_guard() {
    let (result, _, log) = good(recovery_request(99), script(1, 0, vec![], 1)).await;
    let page = result.unwrap();
    assert_eq!(page.page().uid_next_at_start(), 1);
    assert_eq!(page.page().covered_through_uid(), 0);
    assert!(page.page().messages().is_empty());
    assert!(
        !log.iter()
            .any(|line| line.windows(5).any(|s| s == b"FETCH"))
    );
    assert!(log.iter().any(|line| line == b"h8 EXAMINE INBOX\r\n"));
}
#[tokio::test]
async fn sync02_recovery_second_epoch_change_or_new_uidnext_regression_discards_page() {
    for (validity, next, error) in [
        (8, 100, ImapUidPageError::UidValidityChanged),
        (7, 99, ImapUidPageError::CursorInvalid),
    ] {
        let mut steps = script(100, 64, message(1, 1), 100);
        steps[2].1 = selection("h8", validity, next);
        let (result, _, log) = good(recovery_request(99), steps).await;
        assert_eq!(result, Err(error));
        assert_eq!(log.last().unwrap(), b"h8 EXAMINE INBOX\r\n");
    }
}
#[tokio::test]
async fn ext04_recovery_hostile_and_unsolicited_replies_reject_partial_results() {
    let mut duplicate = message(1, 1);
    duplicate.extend(message(2, 1));
    for response in [
        duplicate,
        message(1, 65),
        b"* 2 EXPUNGE\r\n".to_vec(),
        b"* 100 EXISTS\r\n".to_vec(),
        b"* 2 FETCH (UID 2 FLAGS (\\Seen))\r\n".to_vec(),
        b"h7 NO failed\r\n".to_vec(),
        b"h7 BAD failed\r\n".to_vec(),
    ] {
        let (result, _, log) = good(recovery_request(99), script(100, 64, response, 100)).await;
        assert!(result.is_err());
        assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
    }
}
#[tokio::test]
async fn ext03_recovery_untrusted_tls_never_unseals_credentials() {
    let (result, accesses, log) = run_recovery(
        recovery_request(99),
        vec![],
        true,
        Mode::Good,
        false,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(result, Err(Failure::Tls.into()));
    assert_eq!(accesses, 0);
    assert!(log.is_empty());
}
#[tokio::test]
async fn sec06_recovery_identity_binding_precedes_transport_and_unsealing() {
    for (account, version) in [("other", 7), ("acct", 8)] {
        let req = ImapUidRecoveryRequest::new(
            account,
            version,
            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
            99,
        )
        .unwrap();
        let (result, accesses, log) = good(req, vec![]).await;
        assert_eq!(result, Err(ImapUidPageError::CursorInvalid));
        assert_eq!(accesses, 0);
        assert!(log.is_empty());
    }
}
#[tokio::test(start_paused = true)]
async fn sec08_recovery_cancellation_discards_incomplete_fetch_and_session() {
    let mut steps = script(100, 64, vec![], 100);
    steps[1].1 = b"* 1 FETCH (UID 2 ENVELOPE (NIL {8192}\r\nshort".to_vec();
    let (result, _, log) = good(recovery_request(99), steps).await;
    assert_eq!(result, Err(Failure::Timeout.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}
#[tokio::test(start_paused = true)]
async fn sec08_recovery_whole_operation_has_thirty_second_deadline() {
    let mut steps = script(100, 64, vec![], 100);
    steps[0].1.clear();
    let start = tokio::time::Instant::now();
    let (result, _, log) = run_recovery(
        recovery_request(99),
        steps,
        true,
        Mode::Good,
        true,
        Duration::from_secs(40),
    )
    .await;
    assert_eq!(result, Err(Failure::Timeout.into()));
    assert_eq!(start.elapsed(), Duration::from_secs(30));
    assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
}
#[tokio::test]
async fn sec08_recovery_eight_mib_budget_spans_exchanges() {
    let mut status = b"* OK ".to_vec();
    status.extend(vec![b'x'; 60 * 1024]);
    status.extend(b"\r\n");
    let mut steps = script(100, 64, vec![], 100);
    let mut first = status.repeat(70);
    first.extend_from_slice(&steps[0].1);
    steps[0].1 = first;
    let mut second = status.repeat(70);
    second.extend(b"h7 OK fetched\r\n");
    steps[1].1 = second;
    let (result, _, log) = good(recovery_request(99), steps).await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}
