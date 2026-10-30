use super::*;
use hantong_domain::imap_observation::{ImapUidObservation, ImapUidObservationRequest};

fn observation_request() -> ImapUidObservationRequest {
    ImapUidObservationRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
    )
    .unwrap()
}
fn observation_script(validity: u32, next: u32) -> Script {
    vec![
        (
            b"h6 EXAMINE INBOX\r\n".to_vec(),
            selection("h6", validity, next),
        ),
        (
            b"h7 LOGOUT\r\n".to_vec(),
            b"* BYE fixture\r\nh7 OK bye\r\n".to_vec(),
        ),
    ]
}
type ObservationOutcome = (
    Result<ImapUidObservation, ImapUidPageError>,
    usize,
    Vec<Vec<u8>>,
);
async fn run_observation(
    req: ImapUidObservationRequest,
    steps: Script,
    implicit: bool,
    mode: Mode,
    trusted: bool,
    duration: Duration,
) -> ObservationOutcome {
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
        windows::run_uid_observation_connected(
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
    .unwrap_or(Err(Failure::Timeout.into()));
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
async fn good(steps: Script) -> ObservationOutcome {
    run_observation(
        observation_request(),
        steps,
        true,
        Mode::Good,
        true,
        Duration::from_secs(10),
    )
    .await
}

#[tokio::test]
async fn sync01_initial_observation_performs_one_examine_without_any_fetch_under_both_tls_modes() {
    for implicit in [true, false] {
        for next in [1, 194, u32::MAX] {
            let request = observation_request();
            let (result, accesses, log) = run_observation(
                request.clone(),
                observation_script(u32::MAX, next),
                implicit,
                Mode::Good,
                true,
                Duration::from_secs(10),
            )
            .await;
            let observation = result.unwrap();
            assert_eq!(observation.request(), &request);
            assert_eq!(observation.uid_validity(), u32::MAX);
            assert_eq!(observation.uid_next(), next);
            assert_eq!(accesses, 1);
            assert_eq!(
                log.iter()
                    .filter(|line| line.windows(7).any(|v| v == b"EXAMINE"))
                    .count(),
                1
            );
            assert!(
                !log.iter().any(|line| line.windows(5).any(|v| v == b"FETCH")
                    || line.windows(6).any(|v| v == b"SELECT"))
            );
            assert_eq!(log.last().unwrap(), b"h7 LOGOUT\r\n");
        }
    }
}

#[tokio::test]
async fn ext04_initial_observation_requires_unique_nonzero_read_only_uid_facts_and_exact_tag() {
    let valid = String::from_utf8(selection("h6", 7, 194)).unwrap();
    for response in [
        valid.replace("* OK [UIDVALIDITY 7] epoch\r\n", ""),
        valid.replace("* OK [UIDNEXT 194] next\r\n", ""),
        valid.replace("UIDVALIDITY 7", "UIDVALIDITY 0"),
        valid.replace("UIDNEXT 194", "UIDNEXT 0"),
        valid.replace("UIDNEXT 194", "UIDNEXT 4294967296"),
        valid.replace("[READ-ONLY] ", ""),
        valid.replace("READ-ONLY", "READ-WRITE"),
        valid.replace("h6 OK", "wrong OK"),
        valid.replace("h6 OK", "h6 NO"),
        format!("* OK [UIDVALIDITY 7] duplicate\r\n{valid}"),
        format!("* OK [UIDNEXT 194] duplicate\r\n{valid}"),
        format!("* OK [READ-ONLY] duplicate\r\n{valid}"),
    ] {
        let mut steps = observation_script(7, 194);
        steps[0].1 = response.into_bytes();
        let (result, _, log) = good(steps).await;
        assert_eq!(result, Err(Failure::Protocol.into()));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}

#[tokio::test]
async fn ext04_initial_observation_rejects_unsolicited_fetch_expunge_literal_and_bad_logout() {
    for reply in [
        message(1, 1),
        b"* 2 EXPUNGE\r\n".to_vec(),
        b"* FLAGS ({999999}\r\n".to_vec(),
        b"+ unexpected\r\n".to_vec(),
    ] {
        let mut steps = observation_script(7, 194);
        steps[0].1 = reply;
        let (result, _, _) = good(steps).await;
        assert_eq!(result, Err(Failure::Protocol.into()));
    }
    let mut steps = observation_script(7, 194);
    steps[1].1 = b"h7 OK missing BYE\r\n".to_vec();
    assert_eq!(good(steps).await.0, Err(Failure::Protocol.into()));
}

#[tokio::test]
async fn ext03_initial_observation_does_not_unseal_before_tls_or_identity_validation() {
    let (result, accesses, log) = run_observation(
        observation_request(),
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
    for (account, version) in [("other", 7), ("acct", 8)] {
        let request = ImapUidObservationRequest::new(
            account,
            version,
            observation_request().mailbox().clone(),
        )
        .unwrap();
        let (result, accesses, log) = run_observation(
            request,
            vec![],
            true,
            Mode::Good,
            true,
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(result, Err(ImapUidPageError::CursorInvalid));
        assert_eq!(accesses, 0);
        assert!(log.is_empty());
    }
    let (_cleanup, store) = fixture();
    let stale =
        ImapUidObservationRequest::new("acct", 8, observation_request().mailbox().clone()).unwrap();
    assert_eq!(
        store
            .fetch_imap_uid_observation(&stale, &endpoint(true, "localhost", "app_password", 7))
            .await,
        Err(ImapUidPageError::CursorInvalid)
    );
}

#[tokio::test]
async fn ext04_initial_observation_utf8_literal_and_encoding_are_preserved() {
    let request = ImapUidObservationRequest::new(
        "acct",
        7,
        ImapMailboxName::from_wire("받은편지".as_bytes(), MailboxEncoding::Utf8).unwrap(),
    )
    .unwrap();
    let steps = vec![
        (b"h6 EXAMINE {12}\r\n".to_vec(), b"+ ready\r\n".to_vec()),
        ("받은편지\r\n".as_bytes().to_vec(), selection("h6", 7, 1)),
        (
            b"h7 LOGOUT\r\n".to_vec(),
            b"* BYE fixture\r\nh7 OK bye\r\n".to_vec(),
        ),
    ];
    let (result, _, log) = run_observation(
        request.clone(),
        steps,
        true,
        Mode::Rev2,
        true,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(result.unwrap().request(), &request);
    assert!(log.iter().any(|line| line == "받은편지\r\n".as_bytes()));
    let (result, _, log) = run_observation(
        observation_request(),
        vec![],
        true,
        Mode::Rev2,
        true,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(result, Err(Failure::Protocol.into()));
    assert_eq!(log.last().unwrap(), b"h5 CAPABILITY\r\n");
}

#[tokio::test(start_paused = true)]
async fn sec08_initial_observation_has_whole_operation_deadline_and_cancellation_discards_session()
{
    for seconds in [10, 40] {
        let mut steps = observation_script(7, 194);
        steps[0].1.clear();
        let start = tokio::time::Instant::now();
        let (result, _, log) = run_observation(
            observation_request(),
            steps,
            true,
            Mode::Good,
            true,
            Duration::from_secs(seconds),
        )
        .await;
        assert_eq!(result, Err(Failure::Timeout.into()));
        assert_eq!(start.elapsed(), Duration::from_secs(seconds.min(30)));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}

#[tokio::test]
async fn sec08_initial_observation_frame_count_and_total_byte_budgets_remain_bounded() {
    let mut over_frame = b"* OK ".to_vec();
    over_frame.extend(vec![b'x'; 128 * 1024]);
    over_frame.extend(b"\r\n");
    let mut status = b"* OK ".to_vec();
    status.extend(vec![b'x'; 65529]);
    status.extend(b"\r\n");
    for response in [
        b"* OK notice\r\n".repeat(129),
        over_frame,
        status.repeat(128),
    ] {
        let mut steps = observation_script(7, 194);
        steps[0].1 = response;
        let (result, _, log) = run_observation(
            observation_request(),
            steps,
            true,
            Mode::Good,
            true,
            Duration::from_secs(30),
        )
        .await;
        assert_eq!(result, Err(Failure::Protocol.into()));
        assert_eq!(log.last().unwrap(), b"h6 EXAMINE INBOX\r\n");
    }
}
