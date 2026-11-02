use crate::{Lang, onboarding::Io};
use hantong_contracts::{
    ErrorCode, google_setup::GoogleLocalProfile, mail_send::SendReviewRequest, wire_hex,
};
use hantong_ipc_client::{google_mail, mail_send as client};

const EN: &str = include_str!("../resources/compose-en.txt");
const KO: &str = include_str!("../resources/compose-ko.txt");

fn resource(lang: Lang, key: &str) -> &'static str {
    let source = match lang {
        Lang::En => EN,
        Lang::Ko => KO,
    };
    source
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
        .expect("validated compose resource key")
}

fn say(io: &mut dyn Io, lang: Lang, key: &str) -> Result<(), ErrorCode> {
    io.write(resource(lang, key))?;
    io.write("\n")
}

fn ask(io: &mut dyn Io, lang: Lang, key: &str) -> Result<Option<String>, ErrorCode> {
    say(io, lang, key)?;
    io.read_line()
}

fn cancel(value: Option<String>) -> Option<String> {
    value.filter(|value| value != "0")
}

fn recipients(value: &str) -> Result<Vec<String>, ErrorCode> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(values)
}

fn body(io: &mut dyn Io, lang: Lang) -> Result<Option<String>, ErrorCode> {
    say(io, lang, "body_prompt")?;
    let mut lines = Vec::new();
    let mut size = 0usize;
    loop {
        let Some(line) = io.read_text_line(hantong_contracts::mail_send::MAX_COMPOSE_BYTES)? else {
            return Ok(None);
        };
        if line == "0" && lines.is_empty() {
            return Ok(None);
        }
        if line == "." {
            break;
        }
        size = size
            .checked_add(line.len() + usize::from(!lines.is_empty()))
            .ok_or(ErrorCode::QuotaExceeded)?;
        if size > hantong_contracts::mail_send::MAX_COMPOSE_BYTES {
            return Err(ErrorCode::QuotaExceeded);
        }
        lines.push(line);
    }
    if lines.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(Some(lines.join("\n")))
}

fn status(io: &mut dyn Io, lang: Lang, response: &client::ReviewResponse) -> Result<(), ErrorCode> {
    io.write(resource(lang, "review_state"))?;
    io.write(": ")?;
    io.write(&response.state)?;
    if let Some(error) = response.error_code {
        io.write(" (")?;
        io.write(error.as_str())?;
        io.write(")")?;
    }
    io.write("\n")
}

trait Effects {
    fn verify(&mut self, account: &str) -> Result<bool, ErrorCode>;
    fn create(
        &mut self,
        request: &hantong_contracts::google_write::ComposeSnapshot,
    ) -> Result<client::DraftResponse, ErrorCode>;
    fn request(&mut self, request: &SendReviewRequest)
    -> Result<client::ReviewResponse, ErrorCode>;
    fn status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<client::ReviewResponse, ErrorCode>;
    fn wait(&mut self);
}

struct RealEffects;

impl Effects for RealEffects {
    fn verify(&mut self, account: &str) -> Result<bool, ErrorCode> {
        google_mail::verify_default_personal(account, GoogleLocalProfile::PersonalReadSend)
    }

    fn create(
        &mut self,
        request: &hantong_contracts::google_write::ComposeSnapshot,
    ) -> Result<client::DraftResponse, ErrorCode> {
        client::create_compose_default(request)
    }

    fn request(
        &mut self,
        request: &SendReviewRequest,
    ) -> Result<client::ReviewResponse, ErrorCode> {
        client::request_default(request)
    }

    fn status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<client::ReviewResponse, ErrorCode> {
        client::status_default(&hantong_contracts::mail_send::SendReviewStatusRequest {
            account_id: account.into(),
            review_id,
        })
    }

    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

pub(super) fn run(account: &str, lang: Lang, io: &mut dyn Io) -> Result<(), ErrorCode> {
    let mut effects = RealEffects;
    run_with_effects(account, lang, io, &mut effects)
}

fn run_with_effects(
    account: &str,
    lang: Lang,
    io: &mut dyn Io,
    effects: &mut impl Effects,
) -> Result<(), ErrorCode> {
    if !effects.verify(account)? {
        return Err(ErrorCode::ScopeDenied);
    }
    let Some(to) = cancel(ask(io, lang, "recipients_prompt")?) else {
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    let to = recipients(&to)?;
    let Some(subject) = cancel(ask(io, lang, "subject_prompt")?) else {
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    let Some(body) = body(io, lang)? else {
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    let Some(attachment_path) = ask(io, lang, "attachment_prompt")? else {
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    let attachment = match attachment_path.trim() {
        "" | "0" => None,
        path => {
            let Some(media_type) = ask(io, lang, "attachment_type_prompt")? else {
                say(io, lang, "cancelled")?;
                return Ok(());
            };
            let media_type = match media_type.trim() {
                "" | "0" => "application/octet-stream",
                value => value,
            };
            Some(super::mail_send::attachment_file(path, media_type)?)
        }
    };
    let operation = wire_hex::encode(&hantong_platform::random_nonce()?);
    let idempotency = wire_hex::encode(&hantong_platform::random_nonce()?);
    let draft =
        client::prepare_default_draft(account, to, subject, body, &operation, &idempotency)?;
    let snapshot = client::prepare_compose(draft, attachment.into_iter().collect())?;
    let created = effects.create(&snapshot)?;
    say(io, lang, "draft_ready")?;
    let Some(choice) = ask(io, lang, "review_prompt")? else {
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    if choice != "1" {
        say(io, lang, "cancelled")?;
        return Ok(());
    }
    let request = SendReviewRequest::new(
        account,
        created.draft_id,
        created.version,
        &operation,
        &idempotency,
        created.payload_hash,
    )?;
    let mut response = effects.request(&request)?;
    if matches!(response.state.as_str(), "pending" | "sending") {
        say(io, lang, "review_waiting")?;
    }
    for _ in 0..300 {
        if !matches!(response.state.as_str(), "pending" | "sending") {
            break;
        }
        effects.wait();
        response = effects.status(account, response.review_id)?;
    }
    status(io, lang, &response)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestIo {
        input: Vec<Option<String>>,
        cursor: usize,
        output: String,
    }

    impl TestIo {
        fn new(input: &[&str]) -> Self {
            Self {
                input: input.iter().map(|v| Some((*v).into())).collect(),
                cursor: 0,
                output: String::new(),
            }
        }
    }

    impl Io for TestIo {
        fn write(&mut self, text: &str) -> Result<(), ErrorCode> {
            self.output.push_str(text);
            Ok(())
        }

        fn read_line(&mut self) -> Result<Option<String>, ErrorCode> {
            let value = self.input.get(self.cursor).cloned().unwrap_or(None);
            self.cursor += 1;
            Ok(value)
        }
    }

    struct TestEffects {
        verified: bool,
        create_calls: usize,
        request_calls: usize,
        status_calls: usize,
        terminal_state: &'static str,
    }

    impl Effects for TestEffects {
        fn verify(&mut self, _: &str) -> Result<bool, ErrorCode> {
            Ok(self.verified)
        }

        fn create(
            &mut self,
            _: &hantong_contracts::google_write::ComposeSnapshot,
        ) -> Result<client::DraftResponse, ErrorCode> {
            self.create_calls += 1;
            Ok(client::DraftResponse {
                account_id: "google_fixture".into(),
                draft_id: [1; 32],
                version: 1,
                payload_hash: [2; 32],
            })
        }

        fn request(&mut self, _: &SendReviewRequest) -> Result<client::ReviewResponse, ErrorCode> {
            self.request_calls += 1;
            Ok(client::ReviewResponse {
                account_id: "google_fixture".into(),
                review_id: [3; 32],
                state: "pending".into(),
                error_code: None,
            })
        }

        fn status(&mut self, _: &str, _: [u8; 32]) -> Result<client::ReviewResponse, ErrorCode> {
            self.status_calls += 1;
            Ok(client::ReviewResponse {
                account_id: "google_fixture".into(),
                review_id: [3; 32],
                state: self.terminal_state.into(),
                error_code: (self.terminal_state == "unknown").then_some(ErrorCode::SendUnknown),
            })
        }

        fn wait(&mut self) {}
    }

    #[test]
    fn compose_recipient_parser_rejects_empty_input_and_keeps_order() {
        assert_eq!(recipients(""), Err(ErrorCode::InvalidInput));
        assert_eq!(
            recipients(" one@example.test, two@example.test ").unwrap(),
            vec!["one@example.test", "two@example.test"]
        );
    }

    #[test]
    fn compose_scope_denial_has_no_prompt_or_effect() {
        let account = format!("google_{}", "a".repeat(64));
        let mut io = TestIo::new(&[]);
        let mut effects = TestEffects {
            verified: false,
            create_calls: 0,
            request_calls: 0,
            status_calls: 0,
            terminal_state: "sent",
        };
        assert_eq!(
            run_with_effects(&account, Lang::En, &mut io, &mut effects),
            Err(ErrorCode::ScopeDenied)
        );
        assert!(io.output.is_empty());
        assert_eq!(effects.create_calls, 0);
        assert_eq!(effects.request_calls, 0);
    }

    #[test]
    fn compose_eof_and_cancel_never_request_sending() {
        let account = format!("google_{}", "a".repeat(64));
        let mut eof = TestIo::new(&[]);
        let mut effects = TestEffects {
            verified: true,
            create_calls: 0,
            request_calls: 0,
            status_calls: 0,
            terminal_state: "sent",
        };
        assert_eq!(
            run_with_effects(&account, Lang::En, &mut eof, &mut effects),
            Ok(())
        );
        assert_eq!(effects.create_calls, 0);
        assert_eq!(effects.request_calls, 0);

        let mut cancelled = TestIo::new(&["to@example.test", "subject", "body", ".", "", "0"]);
        assert_eq!(
            run_with_effects(&account, Lang::En, &mut cancelled, &mut effects),
            Ok(())
        );
        assert_eq!(effects.create_calls, 1);
        assert_eq!(effects.request_calls, 0);
    }

    #[test]
    fn compose_review_is_requested_once_and_status_is_polled_without_retry() {
        let account = format!("google_{}", "a".repeat(64));
        let mut io = TestIo::new(&["to@example.test", "subject", "body", ".", "", "1"]);
        let mut effects = TestEffects {
            verified: true,
            create_calls: 0,
            request_calls: 0,
            status_calls: 0,
            terminal_state: "sent",
        };
        assert_eq!(
            run_with_effects(&account, Lang::En, &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.create_calls, 1);
        assert_eq!(effects.request_calls, 1);
        assert_eq!(effects.status_calls, 1);
        assert!(io.output.contains("sent"));
    }

    #[test]
    fn sync07_compose_unknown_and_poll_timeout_never_resubmit() {
        let account = format!("google_{}", "a".repeat(64));
        for terminal_state in ["unknown", "pending"] {
            let mut io = TestIo::new(&["to@example.test", "subject", "body", ".", "", "1"]);
            let mut effects = TestEffects {
                verified: true,
                create_calls: 0,
                request_calls: 0,
                status_calls: 0,
                terminal_state,
            };
            run_with_effects(&account, Lang::En, &mut io, &mut effects).unwrap();
            assert_eq!(effects.create_calls, 1);
            assert_eq!(effects.request_calls, 1);
            assert_eq!(
                effects.status_calls,
                if terminal_state == "pending" { 300 } else { 1 }
            );
            assert!(io.output.contains(terminal_state));
        }
    }

    #[test]
    fn compose_body_eof_and_initial_cancel_create_nothing() {
        let account = format!("google_{}", "a".repeat(64));
        for input in [
            vec!["0"],
            vec!["to@example.test", "subject", "unfinished body"],
        ] {
            let mut io = TestIo::new(&input);
            let mut effects = TestEffects {
                verified: true,
                create_calls: 0,
                request_calls: 0,
                status_calls: 0,
                terminal_state: "sent",
            };
            run_with_effects(&account, Lang::En, &mut io, &mut effects).unwrap();
            assert_eq!(effects.create_calls, 0);
            assert_eq!(effects.request_calls, 0);
        }
    }
}
