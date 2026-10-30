use super::*;
use hantong_domain::imap_reconcile::{ImapUidReconcilePage, ImapUidReconcileRequest};

fn reconcile_request(after: u32, through: u32, minimum: u32) -> ImapUidReconcileRequest {
    ImapUidReconcileRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
        7,
        after,
        through,
        minimum,
    )
    .unwrap()
}
async fn run_reconcile(
    req: ImapUidReconcileRequest,
    steps: Script,
    implicit: bool,
    mode: Mode,
    trusted: bool,
    duration: Duration,
) -> (
    Result<ImapUidReconcilePage, ImapUidPageError>,
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
        windows::run_uid_reconcile_connected(
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
    req: ImapUidReconcileRequest,
    steps: Script,
) -> (
    Result<ImapUidReconcilePage, ImapUidPageError>,
    usize,
    Vec<Vec<u8>>,
) {
    run_reconcile(req, steps, true, Mode::Good, true, Duration::from_secs(10)).await
}

#[tokio::test]
async fn ext04_reconcile_exact_tail_survives_growth_under_both_tls_modes() {
    for implicit in [true, false] {
        let mut messages = message(999, 10);
        messages.extend(message(1, 2));
        let (result, accesses, log) = run_reconcile(
            reconcile_request(0, 10, 11),
            script(100, 10, messages, 200),
            implicit,
            Mode::Good,
            true,
            Duration::from_secs(10),
        )
        .await;
        let page = result.unwrap();
        assert_eq!(accesses, 1);
        assert_eq!(page.request().through_uid(), 10);
        assert_eq!(page.uid_next_at_start(), 100);
        assert_eq!(
            page.messages().iter().map(|m| m.uid()).collect::<Vec<_>>(),
            [2, 10]
        );
        assert_eq!(
            log.iter()
                .filter(|line| line.windows(9).any(|s| s == b"UID FETCH"))
                .count(),
            1
        );
        assert_eq!(log.last().unwrap(), b"h9 LOGOUT\r\n");
    }
}

#[tokio::test]
async fn sync02_reconcile_empty_result_is_success_only_after_final_examine() {
    let (result, _, log) = good(reconcile_request(0, 10, 11), script(11, 10, vec![], 11)).await;
    assert!(result.unwrap().messages().is_empty());
    assert!(log.iter().any(|line| line == b"h8 EXAMINE INBOX\r\n"));
}

#[tokio::test]
async fn ext04_reconcile_rejects_epoch_change_and_uidnext_below_bound_before_fetch() {
    for (validity, next, error) in [
        (8, 100, ImapUidPageError::UidValidityChanged),
        (7, 99, ImapUidPageError::CursorInvalid),
    ] {
        let steps = vec![(
            b"h6 EXAMINE INBOX\r\n".to_vec(),
            selection("h6", validity, next),
        )];
        let (result, _, log) = good(reconcile_request(0, 10, 100), steps).await;
        assert_eq!(result, Err(error));
        assert!(
            !log.iter()
                .any(|line| line.windows(5).any(|s| s == b"FETCH"))
        );
    }
}

#[tokio::test]
async fn sync02_reconcile_final_epoch_or_uidnext_regression_discards_all_metadata() {
    for (validity, next, error) in [
        (8, 100, ImapUidPageError::UidValidityChanged),
        (7, 99, ImapUidPageError::CursorInvalid),
    ] {
        let mut steps = script(100, 10, message(1, 1), 100);
        steps[2].1 = selection("h8", validity, next);
        let (result, _, log) = good(reconcile_request(0, 10, 11), steps).await;
        assert_eq!(result, Err(error));
        assert!(
            !log.iter()
                .any(|line| line.windows(6).any(|s| s == b"LOGOUT"))
        );
    }
}

#[tokio::test]
async fn ext04_reconcile_hostile_or_unsolicited_fetch_events_abort_without_partial_page() {
    let mut duplicate = message(1, 1);
    duplicate.extend(message(2, 1));
    let missing = String::from_utf8(message(1, 1))
        .unwrap()
        .replace("UID 1 ", "")
        .into_bytes();
    for bytes in [duplicate,message(1,11),missing,b"* 2 EXPUNGE\r\n".to_vec(),
        b"* 100 EXISTS\r\n".to_vec(),b"* 2 FETCH (UID 2 FLAGS (\\Seen))\r\n".to_vec(),
        b"h7 NO failed\r\n".to_vec(),b"h7 BAD failed\r\n".to_vec(),
        b"* 1 FETCH (UID 1 FLAGS () INTERNALDATE \"01-Jan-2026 00:00:00 +0000\" RFC822.SIZE 0 ENVELOPE (NIL {8193}\r\n".to_vec()] {
        let mut response=message(99,2);response.extend(bytes);
        let (result,_,log)=good(reconcile_request(0,10,11),script(100,10,response,100)).await;
        assert!(result.is_err());
        assert!(!log.iter().any(|line|line==b"h8 EXAMINE INBOX\r\n"));
    }
}

#[tokio::test]
async fn ext03_reconcile_untrusted_tls_never_reads_credentials() {
    let (result, accesses, _) = run_reconcile(
        reconcile_request(0, 10, 11),
        vec![],
        true,
        Mode::Good,
        false,
        Duration::from_secs(10),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(accesses, 0);
}

#[tokio::test]
async fn sync02_reconcile_finite_range_near_uid_ceiling_never_becomes_star() {
    let first = u32::MAX - 64;
    let last = u32::MAX - 1;
    let mut steps = script(u32::MAX, last, message(1, last), u32::MAX);
    steps[1].0 =
        format!("h7 UID FETCH {first}:{last} (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)\r\n")
            .into_bytes();
    let (result, _, log) = good(reconcile_request(first - 1, last, u32::MAX), steps).await;
    assert_eq!(result.unwrap().messages()[0].uid(), last);
    assert!(!log.iter().any(|line| line.contains(&b'*')));
}

#[tokio::test]
async fn sec06_reconcile_account_endpoint_binding_precedes_unsealing() {
    for (account, version) in [("other", 7), ("acct", 8)] {
        let req = ImapUidReconcileRequest::new(
            account,
            version,
            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
            7,
            0,
            10,
            11,
        )
        .unwrap();
        let (result, accesses, log) = good(req, vec![]).await;
        assert_eq!(result, Err(ImapUidPageError::CursorInvalid));
        assert_eq!(accesses, 0);
        assert!(log.is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn sec08_reconcile_cancellation_discards_incomplete_fetch_and_session() {
    let mut steps = script(11, 10, vec![], 11);
    steps[1].1 = b"* 1 FETCH (UID 2 ENVELOPE (NIL {8192}\r\nshort".to_vec();
    let (result, _, log) = good(reconcile_request(0, 10, 11), steps).await;
    assert_eq!(result, Err(Failure::Timeout.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}

#[tokio::test(start_paused = true)]
async fn sec08_reconcile_whole_operation_has_thirty_second_deadline() {
    let mut steps = script(11, 10, vec![], 11);
    steps[0].1.clear();
    let start = tokio::time::Instant::now();
    let (result, _, log) = run_reconcile(
        reconcile_request(0, 10, 11),
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
async fn sec08_reconcile_total_budget_cannot_restart_between_exchanges() {
    let mut status = b"* OK ".to_vec();
    status.extend(vec![b'x'; 60 * 1024]);
    status.extend(b"\r\n");
    let mut steps = script(11, 10, vec![], 11);
    let mut first = status.repeat(70);
    first.extend_from_slice(&steps[0].1);
    steps[0].1 = first;
    let mut second = status.repeat(70);
    second.extend(b"h7 OK fetched\r\n");
    steps[1].1 = second;
    let (result, _, log) = good(reconcile_request(0, 10, 11), steps).await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}

#[tokio::test]
async fn ext04_reconcile_encoding_and_required_selection_facts_are_not_relaxed() {
    let (result, _, log) = run_reconcile(
        reconcile_request(0, 10, 11),
        vec![],
        true,
        Mode::Rev2,
        true,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert_eq!(log.last().unwrap(), b"h5 CAPABILITY\r\n");
    let normal = String::from_utf8(selection("h6", 7, 11)).unwrap();
    for response in [
        normal.replace("* OK [UIDVALIDITY 7] epoch\r\n", ""),
        normal.replace("* OK [UIDNEXT 11] next\r\n", ""),
        normal.replace("READ-ONLY", "READ-WRITE"),
        normal.replace("h6 OK", "wrong OK"),
    ] {
        let (result, _, log) = good(
            reconcile_request(0, 10, 11),
            vec![(b"h6 EXAMINE INBOX\r\n".to_vec(), response.into_bytes())],
        )
        .await;
        assert_eq!(result, Err(Failure::Protocol.into()));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}
