use super::*;
#[path = "uid_reconcile_tests.rs"]
mod reconcile;
#[path = "uid_recovery_tests.rs"]
mod recovery;
use hantong_domain::{
    imap_sync::{ImapUidPage, ImapUidPageRequest},
    mailboxes::{ImapMailboxName, MailboxEncoding},
};
type Outcome = (Result<ImapUidPage, ImapUidPageError>, usize, Vec<Vec<u8>>);

fn request(expected: Option<u32>, after: u32) -> ImapUidPageRequest {
    ImapUidPageRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
        expected,
        after,
    )
    .unwrap()
}
fn selection(tag: &str, validity: u32, next: u32) -> Vec<u8> {
    format!("* FLAGS (\\Seen)\r\n* 99 EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {validity}] epoch\r\n* OK [UIDNEXT {next}] next\r\n{tag} OK [READ-ONLY] selected\r\n").into_bytes()
}
fn message(sequence: u32, uid: u32) -> Vec<u8> {
    format!("* {sequence} FETCH (UID {uid} FLAGS (\\Seen \\Recent custom) INTERNALDATE \"01-Jan-2026 01:00:00 +0100\" RFC822.SIZE 0 ENVELOPE (NIL \"UID FLAGS ENVELOPE BODY\" NIL NIL NIL NIL NIL NIL NIL NIL))\r\n").into_bytes()
}
fn script(next: u32, through: u32, messages: Vec<u8>, final_next: u32) -> Script {
    let mut fetch = messages;
    fetch.extend(b"h7 OK fetched\r\n");
    let mut steps = vec![(b"h6 EXAMINE INBOX\r\n".to_vec(), selection("h6", 7, next))];
    if through > 0 {
        steps.push((
            format!("h7 UID FETCH 1:{through} (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)\r\n")
                .into_bytes(),
            fetch,
        ));
    }
    steps.push((
        b"h8 EXAMINE INBOX\r\n".to_vec(),
        selection("h8", 7, final_next),
    ));
    steps.push((
        b"h9 LOGOUT\r\n".to_vec(),
        b"* BYE fixture\r\nh9 OK bye\r\n".to_vec(),
    ));
    steps
}
async fn run_page(
    req: ImapUidPageRequest,
    steps: Script,
    implicit: bool,
    mode: Mode,
    trusted: bool,
) -> Outcome {
    run_page_for(req, steps, implicit, mode, trusted, Duration::from_secs(10)).await
}
async fn run_page_for(
    req: ImapUidPageRequest,
    steps: Script,
    implicit: bool,
    mode: Mode,
    trusted: bool,
    duration: Duration,
) -> Outcome {
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
        windows::run_uid_connected(
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

#[tokio::test]
async fn ext04_uid_page_uses_real_tls_auth_and_only_one_finite_fetch() {
    for implicit in [true, false] {
        let mut messages = message(999, 64);
        messages.extend(message(1, 2));
        let (result, accesses, log) = run_page(
            request(None, 0),
            script(1000, 64, messages, 1001),
            implicit,
            Mode::Good,
            true,
        )
        .await;
        let page = result.unwrap();
        assert_eq!(accesses, 1);
        assert_eq!(page.uid_next_at_start(), 1000);
        assert_eq!(page.covered_through_uid(), 64);
        assert_eq!(
            page.messages().iter().map(|m| m.uid()).collect::<Vec<_>>(),
            [2, 64]
        );
        assert_eq!(page.messages()[0].flags(), ["\\seen", "custom"]);
        assert_eq!(
            log.iter()
                .filter(|line| line.windows(9).any(|s| s == b"UID FETCH"))
                .count(),
            1
        );
        assert_eq!(log.last().unwrap(), b"h9 LOGOUT\r\n");
        assert!(!format!("{page:?}").contains("BODY"));
    }
}
#[tokio::test]
async fn sync05_uid_empty_range_skips_fetch_but_revalidates() {
    let (result, _, log) = run_page(
        request(None, 0),
        script(1, 0, vec![], 1),
        true,
        Mode::Good,
        true,
    )
    .await;
    assert!(result.unwrap().messages().is_empty());
    assert!(
        !log.iter()
            .any(|line| line.windows(5).any(|s| s == b"FETCH"))
    );
    assert!(log.iter().any(|line| line == b"h8 EXAMINE INBOX\r\n"));
}
#[tokio::test]
async fn sync05_uid_gaps_are_covered_without_search_or_retries() {
    let (result, _, log) = run_page(
        request(Some(7), 0),
        script(20, 19, vec![], 20),
        true,
        Mode::Good,
        true,
    )
    .await;
    assert_eq!(result.unwrap().covered_through_uid(), 19);
    assert_eq!(
        log.iter()
            .filter(|line| line.windows(5).any(|s| s == b"FETCH"))
            .count(),
        1
    );
}

#[tokio::test]
async fn ext04_uid_single_message_keeps_explicit_finite_range() {
    let (result, _, log) = run_page(
        request(None, 0),
        script(2, 1, message(999, 1), 2),
        true,
        Mode::Good,
        true,
    )
    .await;
    assert_eq!(result.unwrap().covered_through_uid(), 1);
    assert!(
        log.iter()
            .any(|line| line.starts_with(b"h7 UID FETCH 1:1 "))
    );
}
#[tokio::test]
async fn sync05_uid_initial_epoch_and_cursor_errors_stop_before_fetch() {
    for (req, want) in [
        (request(Some(8), 0), ImapUidPageError::UidValidityChanged),
        (request(Some(7), 20), ImapUidPageError::CursorInvalid),
    ] {
        let (result, _, log) =
            run_page(req, script(20, 19, vec![], 20), true, Mode::Good, true).await;
        assert_eq!(result, Err(want));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}
#[tokio::test]
async fn sync05_uid_final_epoch_or_next_regression_discards_metadata() {
    for (validity, next, want) in [
        (8, 20, ImapUidPageError::UidValidityChanged),
        (7, 19, ImapUidPageError::CursorInvalid),
    ] {
        let mut steps = script(20, 19, message(1, 2), 20);
        steps[2].1 = selection("h8", validity, next);
        let (result, _, log) = run_page(request(None, 0), steps, true, Mode::Good, true).await;
        assert_eq!(result, Err(want));
        assert_eq!(log.last().unwrap(), b"h8 EXAMINE INBOX\r\n");
    }
}
#[tokio::test]
async fn ext04_uid_required_selection_facts_and_tags_fail_closed() {
    let valid = String::from_utf8(selection("h6", 7, 20)).unwrap();
    for wire in [
        valid.replace("* OK [UIDVALIDITY 7] epoch\r\n", ""),
        valid.replace("* OK [UIDNEXT 20] next\r\n", ""),
        valid.replace("[READ-ONLY] ", ""),
        valid.replace("UIDNEXT 20", "UIDNEXT 0"),
        valid.replace("h6 OK", "wrong OK"),
        format!("* OK [UIDNEXT 20] duplicate\r\n{valid}"),
        valid.replace("READ-ONLY", "READ-WRITE"),
    ] {
        let mut steps = script(20, 19, vec![], 20);
        steps[0].1 = wire.into_bytes();
        let (result, _, log) = run_page(request(None, 0), steps, true, Mode::Good, true).await;
        assert_eq!(result, Err(Failure::Protocol.into()));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}
#[tokio::test]
async fn ext04_uid_duplicate_outside_missing_and_body_data_discard_whole_page() {
    let normal = String::from_utf8(message(1, 2)).unwrap();
    for data in [
        format!("{normal}{normal}"),
        String::from_utf8(message(1, 20)).unwrap(),
        normal.replace("UID 2 ", ""),
        normal.replace("RFC822.SIZE 0", "RFC822.SIZE 0 BODY[] {1}\r\nx"),
        normal.replace("FLAGS (\\Seen \\Recent custom)", "FLAGS (\\Unknown)"),
    ] {
        let (result, _, log) = run_page(
            request(None, 0),
            script(20, 19, data.into_bytes(), 20),
            true,
            Mode::Good,
            true,
        )
        .await;
        assert_eq!(result, Err(Failure::Protocol.into()));
        assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
    }
}
#[tokio::test]
async fn ext03_uid_certificate_failure_prevents_credentials_and_commands() {
    let (result, accesses, log) = run_page(
        request(None, 0),
        script(20, 19, vec![], 20),
        true,
        Mode::Good,
        false,
    )
    .await;
    assert_eq!(result, Err(Failure::Tls.into()));
    assert_eq!(accesses, 0);
    assert!(log.is_empty());
}
#[tokio::test]
async fn sec06_uid_endpoint_binding_precedes_transport_and_unsealing() {
    let req = ImapUidPageRequest::new(
        "acct",
        8,
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
        None,
        0,
    )
    .unwrap();
    let (result, accesses, log) = run_page(req, vec![], true, Mode::Good, true).await;
    assert_eq!(result, Err(ImapUidPageError::CursorInvalid));
    assert_eq!(accesses, 0);
    assert!(log.is_empty());
}
#[tokio::test]
async fn ext04_uid_utf8_mailbox_waits_for_literal_continuation() {
    let req = ImapUidPageRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire("받은편지".as_bytes(), MailboxEncoding::Utf8).unwrap(),
        None,
        0,
    )
    .unwrap();
    let steps = vec![
        (b"h6 EXAMINE {12}\r\n".to_vec(), b"+ ready\r\n".to_vec()),
        ("받은편지\r\n".as_bytes().to_vec(), selection("h6", 7, 1)),
        (b"h8 EXAMINE {12}\r\n".to_vec(), b"+ ready\r\n".to_vec()),
        ("받은편지\r\n".as_bytes().to_vec(), selection("h8", 7, 1)),
        (
            b"h9 LOGOUT\r\n".to_vec(),
            b"* BYE fixture\r\nh9 OK bye\r\n".to_vec(),
        ),
    ];
    let (result, _, log) = run_page(req, steps, true, Mode::Rev2, true).await;
    assert!(result.is_ok());
    assert!(log.iter().any(|line| line == "받은편지\r\n".as_bytes()));
}
#[tokio::test]
async fn ext04_uid_response_flood_aborts_without_followup() {
    let mut steps = script(20, 19, vec![], 20);
    steps[1].1 = b"* OK notice\r\n".repeat(129);
    let (result, _, log) = run_page(request(None, 0), steps, true, Mode::Good, true).await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}

#[tokio::test]
async fn ext04_uid_upper_u32_boundary_and_resume_range_do_not_overflow() {
    let mut steps = script(u32::MAX, 64, message(999, u32::MAX - 1), u32::MAX);
    steps[1].0 = format!(
        "h7 UID FETCH {}:{} (UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE)\r\n",
        u32::MAX - 3,
        u32::MAX - 1
    )
    .into_bytes();
    let (result, _, _) = run_page(
        request(Some(7), u32::MAX - 4),
        steps,
        true,
        Mode::Good,
        true,
    )
    .await;
    let page = result.unwrap();
    assert_eq!(page.covered_through_uid(), u32::MAX - 1);
    assert_eq!(page.messages()[0].uid(), u32::MAX - 1);
}

#[tokio::test]
async fn ext04_uid_binary_literal_crosses_tls_without_loss() {
    let mut metadata=b"* 1 FETCH (UID 2 FLAGS () INTERNALDATE \"01-Jan-2026 00:00:00 +0000\" RFC822.SIZE 0 ENVELOPE (NIL {3}\r\n".to_vec();
    metadata.extend(b"a\xffz NIL NIL NIL NIL NIL NIL \"\" NIL))\r\n");
    let (result, _, _) = run_page(
        request(None, 0),
        script(20, 19, metadata, 20),
        true,
        Mode::Good,
        true,
    )
    .await;
    let page = result.unwrap();
    assert_eq!(
        page.messages()[0].envelope().fields().subject.as_deref(),
        Some(b"a\xffz".as_slice())
    );
    assert_eq!(
        page.messages()[0]
            .envelope()
            .fields()
            .in_reply_to
            .as_deref(),
        Some(b"".as_slice())
    );
}

#[tokio::test]
async fn ext04_uid_encoding_change_is_not_identity_remapping() {
    let (result, _, log) = run_page(
        request(None, 0),
        script(1, 0, vec![], 1),
        true,
        Mode::Rev2,
        true,
    )
    .await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert_eq!(log.last().unwrap(), b"h5 CAPABILITY\r\n");
}

#[tokio::test(start_paused = true)]
async fn sec08_uid_cancellation_discards_incomplete_fetch_and_drops_session() {
    let mut steps = script(20, 19, vec![], 20);
    steps[1].1 = b"* 1 FETCH (UID 2 ENVELOPE (NIL {8192}\r\nshort".to_vec();
    let (result, _, log) = run_page(request(None, 0), steps, true, Mode::Good, true).await;
    assert_eq!(result, Err(Failure::Timeout.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}

#[tokio::test(start_paused = true)]
async fn sec08_uid_whole_operation_retains_thirty_second_deadline() {
    let mut steps = script(20, 19, vec![], 20);
    steps[0].1.clear();
    let start = tokio::time::Instant::now();
    let (result, _, log) = run_page_for(
        request(None, 0),
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
async fn sec08_uid_eight_mib_budget_spans_multiple_exchanges() {
    let mut status = b"* OK ".to_vec();
    status.extend(vec![b'x'; 60 * 1024]);
    status.extend(b"\r\n");
    let mut steps = script(20, 19, vec![], 20);
    let mut first = status.repeat(70);
    first.extend_from_slice(&steps[0].1);
    steps[0].1 = first;
    let mut second = status.repeat(70);
    second.extend(b"h7 OK fetched\r\n");
    steps[1].1 = second;
    let (result, _, log) = run_page(request(None, 0), steps, true, Mode::Good, true).await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert!(log.last().unwrap().starts_with(b"h7 UID FETCH"));
}
