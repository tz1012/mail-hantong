//! Provider-neutral CLI request for one cached IMAP UID move to `\Trash`.
//!
//! The CLI only creates the immutable request and invokes Core's foreground
//! review. It never supplies approval or chooses the destination mailbox.

use super::*;
use hantong_contracts::{
    ErrorCode,
    imap_trash::ImapTrashRequest,
    terminal_review::TerminalReviewKind,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
    wire_hex,
};

enum Command {
    Request(ImapTrashRequest),
    Status(String, [u8; 32]),
}

fn parse(command: &[String], options: &[(String, String)]) -> Result<Command, ErrorCode> {
    let action = match command {
        [verb, action] if verb == "trash" => action.as_str(),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let allowed = if action == "status" {
        ["--account", "--review-id"].as_slice()
    } else if action == "move" {
        [
            "--account",
            "--mailbox-wire-hex",
            "--mailbox-encoding",
            "--expected-catalog-generation",
            "--uidvalidity",
            "--uid",
            "--operation-id",
            "--idempotency-key",
            "--payload-hash",
        ]
        .as_slice()
    } else {
        return Err(ErrorCode::InvalidInput);
    };
    let mut fields = std::collections::BTreeMap::new();
    for (key, value) in options {
        if !allowed.contains(&key.as_str()) || fields.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let get = |key| fields.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let account = get("--account")?;
    if account.is_empty()
        || account.len() > 128
        || !account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    if action == "status" {
        let id = wire_hex::decode_fixed(get("--review-id")?)?;
        return (id != [0; 32])
            .then_some(Command::Status(account.into(), id))
            .ok_or(ErrorCode::InvalidInput);
    }
    let encoding = match get("--mailbox-encoding")? {
        "utf8" => UidMailboxEncoding::Utf8,
        "modified_utf7" => UidMailboxEncoding::ModifiedUtf7,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let mailbox = UidMailboxIdentity::new(get("--mailbox-wire-hex")?, encoding)?;
    let generated_fields = ["--operation-id", "--idempotency-key", "--payload-hash"];
    let supplied = generated_fields
        .iter()
        .filter(|name| fields.contains_key(*name))
        .count();
    if supplied != 0 && supplied != generated_fields.len() {
        return Err(ErrorCode::InvalidInput);
    }
    let operation = if supplied == 0 {
        wire_hex::encode(&hantong_platform::random_nonce()?)
    } else {
        get("--operation-id")?.into()
    };
    let idempotency = if supplied == 0 {
        wire_hex::encode(&hantong_platform::random_nonce()?)
    } else {
        get("--idempotency-key")?.into()
    };
    let request = ImapTrashRequest::new(
        account,
        mailbox,
        get("--expected-catalog-generation")?
            .parse()
            .map_err(|_| ErrorCode::InvalidInput)?,
        get("--uidvalidity")?
            .parse()
            .map_err(|_| ErrorCode::InvalidInput)?,
        get("--uid")?.parse().map_err(|_| ErrorCode::InvalidInput)?,
        &operation,
        &idempotency,
        [0; 32],
    )?;
    let computed = hantong_platform::sha256_storage_payload(&request.payload_bytes())?;
    if supplied != 0 && wire_hex::decode_fixed::<32>(get("--payload-hash")?)? != computed {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(Command::Request(ImapTrashRequest::new(
        request.account_id(),
        request.mailbox().clone(),
        request.catalog_generation(),
        request.uidvalidity(),
        request.uid(),
        &operation,
        &idempotency,
        computed,
    )?))
}

pub(super) fn run(command: &[String], options: &[(String, String)], lang: Lang, json: bool) -> i32 {
    let command = match parse(command, options) {
        Ok(command) => command,
        Err(error) => return emit_error(error, lang, json),
    };
    let result = match command {
        Command::Status(account, id) => {
            hantong_ipc_client::imap_trash::status_default(&account, id)
        }
        Command::Request(request) => {
            use std::io::IsTerminal;
            if json
                || std::env::args_os().any(|arg| arg == "--no-input")
                || !std::io::stdin().is_terminal()
                || !std::io::stdout().is_terminal()
            {
                Err(ErrorCode::HumanStepRequired)
            } else {
                let mut effects = RealEffects { lang };
                execute_if_interactive(&request, lang, true, &mut effects)
            }
        }
    };
    match result {
        Ok(receipt) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "schema_version": 1,
                        "ok": true,
                        "provider": "imap",
                        "review_id": wire_hex::encode(&receipt.review_id),
                        "state": receipt.state,
                        "error_code": receipt.error.map(ErrorCode::as_str),
                    })
                );
            } else {
                println!(
                    "review_id={} state={}",
                    wire_hex::encode(&receipt.review_id),
                    receipt.state
                );
            }
            receipt.error.map_or(0, ErrorCode::exit_code)
        }
        Err(error) => emit_error(error, lang, json),
    }
}

trait Effects {
    fn request(
        &mut self,
        request: &ImapTrashRequest,
    ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode>;
    fn review(
        &mut self,
        request_id: [u8; 32],
        account: &str,
        payload_hash: [u8; 32],
    ) -> Result<(), ErrorCode>;
    fn status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode>;
    fn wait(&mut self);
}

struct RealEffects {
    lang: Lang,
}

impl Effects for RealEffects {
    fn request(
        &mut self,
        request: &ImapTrashRequest,
    ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
        hantong_ipc_client::imap_trash::request_default(request)
    }

    fn review(
        &mut self,
        request_id: [u8; 32],
        account: &str,
        payload_hash: [u8; 32],
    ) -> Result<(), ErrorCode> {
        super::core_start::review(
            TerminalReviewKind::ImapTrash,
            request_id,
            Some(account),
            Some(payload_hash),
            self.lang,
        )
    }

    fn status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
        hantong_ipc_client::imap_trash::status_default(account, review_id)
    }

    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn execute_request(
    request: &ImapTrashRequest,
    _lang: Lang,
    effects: &mut impl Effects,
) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
    let receipt = effects.request(request)?;
    if receipt.state != "pending" {
        return Ok(receipt);
    }
    effects.review(
        receipt.review_id,
        request.account_id(),
        *request.payload_hash(),
    )?;
    let mut status = effects.status(request.account_id(), receipt.review_id)?;
    for _ in 0..300 {
        if !matches!(status.state.as_str(), "pending" | "claimed" | "moving") {
            break;
        }
        effects.wait();
        status = effects.status(request.account_id(), receipt.review_id)?;
    }
    Ok(status)
}

fn execute_if_interactive(
    request: &ImapTrashRequest,
    lang: Lang,
    interactive: bool,
    effects: &mut impl Effects,
) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
    if !interactive {
        return Err(ErrorCode::HumanStepRequired);
    }
    execute_request(request, lang, effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn cli04_trash_generates_bound_intent_for_terminal_navigation() {
        let args = [
            ("--account", "acct"),
            ("--mailbox-wire-hex", "494e424f58"),
            ("--mailbox-encoding", "utf8"),
            ("--expected-catalog-generation", "3"),
            ("--uidvalidity", "42"),
            ("--uid", "17"),
        ]
        .map(|(k, v)| (k.into(), v.into()));
        let command = ["trash".into(), "move".into()];
        let Command::Request(first) = parse(&command, &args).unwrap() else {
            panic!("request expected")
        };
        let Command::Request(second) = parse(&command, &args).unwrap() else {
            panic!("request expected")
        };
        assert_ne!(first.operation_id(), second.operation_id());
        assert_eq!(first.account_id(), "acct");
        assert_eq!(first.uid(), 17);
        assert_eq!(
            *first.payload_hash(),
            hantong_platform::sha256_storage_payload(&first.payload_bytes()).unwrap()
        );
        let mut partial = args.to_vec();
        partial.push(("--operation-id".into(), "partial".into()));
        assert!(matches!(
            parse(&command, &partial),
            Err(ErrorCode::InvalidInput)
        ));
    }

    struct TestEffects {
        requests: usize,
        reviews: usize,
        statuses: Vec<hantong_ipc_client::imap_trash::ImapTrashReceipt>,
        waits: usize,
        review_error: Option<ErrorCode>,
    }

    impl Effects for TestEffects {
        fn request(
            &mut self,
            request: &ImapTrashRequest,
        ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
            self.requests += 1;
            Ok(hantong_ipc_client::imap_trash::ImapTrashReceipt {
                account_id: request.account_id().to_owned(),
                review_id: [0x11; 32],
                state: "pending".into(),
                error: None,
            })
        }

        fn review(
            &mut self,
            _request_id: [u8; 32],
            _account: &str,
            _payload_hash: [u8; 32],
        ) -> Result<(), ErrorCode> {
            self.reviews += 1;
            self.review_error.take().map_or(Ok(()), Err)
        }

        fn status(
            &mut self,
            _account: &str,
            _review_id: [u8; 32],
        ) -> Result<hantong_ipc_client::imap_trash::ImapTrashReceipt, ErrorCode> {
            self.statuses.pop().ok_or(ErrorCode::CoreUnavailable)
        }

        fn wait(&mut self) {
            self.waits += 1;
        }
    }

    fn request() -> ImapTrashRequest {
        let mailbox = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap();
        let draft =
            ImapTrashRequest::new("acct", mailbox, 3, 42, 7, "op1", "idem1", [0; 32]).unwrap();
        let hash = hantong_platform::sha256_storage_payload(&draft.payload_bytes()).unwrap();
        ImapTrashRequest::new(
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            3,
            42,
            7,
            "op1",
            "idem1",
            hash,
        )
        .unwrap()
    }

    #[test]
    fn sec03_trash_requires_exact_cached_identity_and_payload_hash() {
        let request = ImapTrashRequest::new(
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            3,
            42,
            7,
            "op1",
            "idem1",
            [0; 32],
        )
        .unwrap();
        let options = vec![
            ("--account".into(), "acct".into()),
            ("--mailbox-wire-hex".into(), "494e424f58".into()),
            ("--mailbox-encoding".into(), "utf8".into()),
            ("--expected-catalog-generation".into(), "3".into()),
            ("--uidvalidity".into(), "42".into()),
            ("--uid".into(), "7".into()),
            ("--operation-id".into(), "op1".into()),
            ("--idempotency-key".into(), "idem1".into()),
            ("--payload-hash".into(), "00".repeat(32)),
        ];
        assert!(matches!(
            parse(&["trash".into(), "move".into()], &options),
            Err(ErrorCode::InvalidInput)
        ));
        assert_eq!(request.account_id(), "acct");
    }

    #[test]
    fn sec03_cancel_after_pending_request_does_not_poll_or_rerequest() {
        let effects = TestEffects {
            requests: 0,
            reviews: 0,
            statuses: vec![],
            waits: 0,
            review_error: Some(ErrorCode::ApprovalDenied),
        };
        let mut effects = effects;
        assert!(matches!(
            execute_request(&request(), Lang::En, &mut effects),
            Err(ErrorCode::ApprovalDenied)
        ));
        assert_eq!(effects.requests, 1);
        assert_eq!(effects.reviews, 1);
        assert_eq!(effects.waits, 0);
    }

    #[test]
    fn sec03_confirmed_unknown_is_terminal_without_rerequest() {
        let mut effects = TestEffects {
            requests: 0,
            reviews: 0,
            statuses: vec![hantong_ipc_client::imap_trash::ImapTrashReceipt {
                account_id: "acct".into(),
                review_id: [0x11; 32],
                state: "unknown".into(),
                error: Some(ErrorCode::SendUnknown),
            }],
            waits: 0,
            review_error: None,
        };
        let result = execute_request(&request(), Lang::En, &mut effects).unwrap();
        assert_eq!(result.state, "unknown");
        assert_eq!(effects.requests, 1);
        assert_eq!(effects.reviews, 1);
        assert_eq!(effects.waits, 0);
    }

    #[test]
    fn sec03_headless_request_has_no_effects() {
        let mut effects = TestEffects {
            requests: 0,
            reviews: 0,
            statuses: vec![],
            waits: 0,
            review_error: None,
        };
        assert!(matches!(
            execute_if_interactive(&request(), Lang::En, false, &mut effects),
            Err(ErrorCode::HumanStepRequired)
        ));
        assert_eq!(effects.requests, 0);
        assert_eq!(effects.reviews, 0);
        assert_eq!(effects.waits, 0);
    }
}
