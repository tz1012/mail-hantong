use crate::{Lang, onboarding::Io};
use hantong_contracts::{ErrorCode, mail_send::SendReviewRequest, wire_hex};
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

fn provider_supports_attachments(account: &str) -> bool {
    account.starts_with("google_")
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

fn terminal_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')
        {
            out.push_str(&format!("\\u{{{:04x}}}", c as u32));
        } else if c == '\r' {
            out.push_str("\\r");
        } else if c == '\n' || c == '\t' {
            out.push(c);
        } else if c.is_control() {
            out.push('\u{fffd}');
        } else {
            out.push(c);
        }
    }
    out
}

fn show_preview(
    io: &mut dyn Io,
    lang: Lang,
    preview: &client::PersonalSendPreview,
) -> Result<(), ErrorCode> {
    for (key, value) in [
        ("preview_from", preview.from.as_str()),
        ("preview_to", &preview.to.join(", ")),
        ("preview_subject", preview.subject.as_str()),
    ] {
        io.write(resource(lang, key))?;
        io.write(": ")?;
        io.write(&terminal_safe(value))?;
        io.write("\n")?;
    }
    io.write(resource(lang, "preview_body"))?;
    io.write(":\n")?;
    io.write(&terminal_safe(&preview.body))?;
    io.write("\n")?;
    if !preview.attachments.is_empty() {
        io.write(resource(lang, "preview_attachments"))?;
        io.write(":\n")?;
        for attachment in &preview.attachments {
            io.write("- ")?;
            io.write(&terminal_safe(&attachment.filename))?;
            io.write(" | ")?;
            io.write(&terminal_safe(&attachment.media_type))?;
            io.write(" | ")?;
            io.write(&attachment.size.to_string())?;
            io.write(" bytes\n")?;
        }
    }
    Ok(())
}

trait Effects {
    fn attachments_supported(&self) -> bool {
        true
    }
    fn verify(&mut self, account: &str) -> Result<bool, ErrorCode>;
    fn create(
        &mut self,
        request: &hantong_contracts::google_write::ComposeSnapshot,
    ) -> Result<client::DraftResponse, ErrorCode>;
    fn request(
        &mut self,
        request: &SendReviewRequest,
    ) -> Result<client::PersonalPrepareResponse, ErrorCode>;
    fn confirm(
        &mut self,
        request: &hantong_contracts::mail_send::PersonalSendConfirmRequest,
    ) -> Result<client::ReviewResponse, ErrorCode>;
    fn cancel(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<client::ReviewResponse, ErrorCode>;
    fn status(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<client::ReviewResponse, ErrorCode>;
    fn wait(&mut self);
}

struct RealEffects {
    lang: Lang,
    personal: bool,
    google_provider: bool,
}

impl Effects for RealEffects {
    fn attachments_supported(&self) -> bool {
        self.google_provider
    }

    fn verify(&mut self, account: &str) -> Result<bool, ErrorCode> {
        // Google has a provider-specific fast path for the exact personal
        // scope. Other registered providers use the generic Core policy and
        // terminal review; their registration and effective tools are still
        // checked by create/prepare below.
        self.google_provider = provider_supports_attachments(account);
        if self.google_provider {
            self.personal = google_mail::verify_default_personal_accounts(account)?;
            if self.personal {
                return Ok(true);
            }
        }
        self.personal = false;
        client::verify_default_send(account)
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
    ) -> Result<client::PersonalPrepareResponse, ErrorCode> {
        if self.personal {
            return client::prepare_default(request);
        }
        let receipt = super::mail_send::interactive_send(request.clone(), self.lang)?;
        Ok(client::PersonalPrepareResponse {
            receipt,
            preview: None,
        })
    }
    fn confirm(
        &mut self,
        request: &hantong_contracts::mail_send::PersonalSendConfirmRequest,
    ) -> Result<client::ReviewResponse, ErrorCode> {
        client::confirm_default(request)
    }
    fn cancel(
        &mut self,
        account: &str,
        review_id: [u8; 32],
    ) -> Result<client::ReviewResponse, ErrorCode> {
        client::cancel_default(&hantong_contracts::mail_send::SendReviewStatusRequest {
            account_id: account.into(),
            review_id,
        })
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
    let mut effects = RealEffects {
        lang,
        personal: false,
        google_provider: false,
    };
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
    let attachment = if effects.attachments_supported() {
        let Some(attachment_path) = ask(io, lang, "attachment_prompt")? else {
            say(io, lang, "cancelled")?;
            return Ok(());
        };
        match attachment_path.trim() {
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
        }
    } else {
        say(io, lang, "attachments_unsupported")?;
        None
    };
    let operation = wire_hex::encode(&hantong_platform::random_nonce()?);
    let idempotency = wire_hex::encode(&hantong_platform::random_nonce()?);
    let draft =
        client::prepare_default_draft(account, to, subject, body, &operation, &idempotency)?;
    let snapshot = client::prepare_compose(draft, attachment.into_iter().collect())?;
    let created = effects.create(&snapshot)?;
    say(io, lang, "draft_ready")?;
    let request = SendReviewRequest::new(
        account,
        created.draft_id,
        created.version,
        &operation,
        &idempotency,
        created.payload_hash,
    )?;
    let prepared = effects.request(&request)?;
    let Some(preview) = prepared.preview.as_ref() else {
        return status(io, lang, &prepared.receipt);
    };
    show_preview(io, lang, preview)?;
    let Some(choice) = ask(io, lang, "send_prompt")? else {
        let _ = effects.cancel(account, prepared.receipt.review_id);
        say(io, lang, "cancelled")?;
        return Ok(());
    };
    if choice != "1" {
        let _ = effects.cancel(account, prepared.receipt.review_id);
        say(io, lang, "cancelled")?;
        return Ok(());
    }
    let confirmation = hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
        account,
        prepared.receipt.review_id,
        preview.payload_hash,
    )?;
    let mut response = effects.confirm(&confirmation)?;
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
        attachments_supported: bool,
        created_attachments: usize,
        create_calls: usize,
        request_calls: usize,
        confirm_calls: usize,
        cancel_calls: usize,
        status_calls: usize,
        terminal_state: &'static str,
    }

    impl Effects for TestEffects {
        fn attachments_supported(&self) -> bool {
            self.attachments_supported
        }

        fn verify(&mut self, _: &str) -> Result<bool, ErrorCode> {
            Ok(self.verified)
        }

        fn create(
            &mut self,
            request: &hantong_contracts::google_write::ComposeSnapshot,
        ) -> Result<client::DraftResponse, ErrorCode> {
            self.create_calls += 1;
            self.created_attachments = request.attachments().len();
            Ok(client::DraftResponse {
                account_id: "google_fixture".into(),
                draft_id: [1; 32],
                version: 1,
                payload_hash: [2; 32],
            })
        }

        fn request(
            &mut self,
            _: &SendReviewRequest,
        ) -> Result<client::PersonalPrepareResponse, ErrorCode> {
            self.request_calls += 1;
            Ok(client::PersonalPrepareResponse {
                receipt: client::ReviewResponse {
                    account_id: "google_fixture".into(),
                    review_id: [3; 32],
                    state: "pending".into(),
                    error_code: None,
                },
                preview: Some(client::PersonalSendPreview {
                    from: "from@example.test".into(),
                    to: vec!["to@example.test".into()],
                    subject: "subject".into(),
                    body: "body".into(),
                    attachments: vec![],
                    payload_hash: [2; 32],
                    draft_id: [1; 32],
                    version: 1,
                }),
            })
        }
        fn confirm(
            &mut self,
            _: &hantong_contracts::mail_send::PersonalSendConfirmRequest,
        ) -> Result<client::ReviewResponse, ErrorCode> {
            self.confirm_calls += 1;
            Ok(client::ReviewResponse {
                account_id: "google_fixture".into(),
                review_id: [3; 32],
                state: "pending".into(),
                error_code: None,
            })
        }
        fn cancel(&mut self, _: &str, _: [u8; 32]) -> Result<client::ReviewResponse, ErrorCode> {
            self.cancel_calls += 1;
            Ok(client::ReviewResponse {
                account_id: "google_fixture".into(),
                review_id: [3; 32],
                state: "interrupted".into(),
                error_code: Some(ErrorCode::ApprovalRequired),
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
    fn compose_google_manage_keeps_attachments_while_smtp_omits_them() {
        assert!(provider_supports_attachments(&format!(
            "google_{}",
            "a".repeat(64)
        )));
        assert!(!provider_supports_attachments("smtp_account"));
    }

    #[test]
    fn compose_scope_denial_has_no_prompt_or_effect() {
        let account = format!("google_{}", "a".repeat(64));
        let mut io = TestIo::new(&[]);
        let mut effects = TestEffects {
            verified: false,
            attachments_supported: true,
            created_attachments: 0,
            create_calls: 0,
            request_calls: 0,
            confirm_calls: 0,
            cancel_calls: 0,
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
            attachments_supported: true,
            created_attachments: 0,
            create_calls: 0,
            request_calls: 0,
            confirm_calls: 0,
            cancel_calls: 0,
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
        assert_eq!(effects.request_calls, 1);
        assert_eq!(effects.created_attachments, 0);
        assert_eq!(effects.cancel_calls, 1);
    }

    #[test]
    fn compose_review_is_requested_once_and_status_is_polled_without_retry() {
        let account = format!("google_{}", "a".repeat(64));
        let mut io = TestIo::new(&["to@example.test", "subject", "body", ".", "", "1"]);
        let mut effects = TestEffects {
            verified: true,
            attachments_supported: true,
            created_attachments: 0,
            create_calls: 0,
            request_calls: 0,
            confirm_calls: 0,
            cancel_calls: 0,
            status_calls: 0,
            terminal_state: "sent",
        };
        assert_eq!(
            run_with_effects(&account, Lang::En, &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.create_calls, 1);
        assert_eq!(effects.request_calls, 1);
        assert_eq!(effects.confirm_calls, 1);
        assert_eq!(effects.status_calls, 1);
        assert!(io.output.contains("sent"));
    }

    #[test]
    fn compose_smtp_skips_attachment_input_before_creating_draft() {
        let mut io = TestIo::new(&["to@example.test", "subject", "body", ".", "1"]);
        let mut effects = TestEffects {
            verified: true,
            attachments_supported: false,
            created_attachments: 0,
            create_calls: 0,
            request_calls: 0,
            confirm_calls: 0,
            cancel_calls: 0,
            status_calls: 0,
            terminal_state: "sent",
        };
        run_with_effects("smtp_account", Lang::En, &mut io, &mut effects).unwrap();
        assert_eq!(effects.create_calls, 1);
        assert_eq!(effects.request_calls, 1);
        assert_eq!(effects.created_attachments, 0);
        assert!(!io.output.contains("Attachment path"));
        assert!(io.output.contains("Attachments are not supported"));
    }

    #[test]
    fn sync07_compose_unknown_and_poll_timeout_never_resubmit() {
        let account = format!("google_{}", "a".repeat(64));
        for terminal_state in ["unknown", "pending"] {
            let mut io = TestIo::new(&["to@example.test", "subject", "body", ".", "", "1"]);
            let mut effects = TestEffects {
                verified: true,
                attachments_supported: true,
                created_attachments: 0,
                create_calls: 0,
                request_calls: 0,
                confirm_calls: 0,
                cancel_calls: 0,
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
                attachments_supported: true,
                created_attachments: 0,
                create_calls: 0,
                request_calls: 0,
                confirm_calls: 0,
                cancel_calls: 0,
                status_calls: 0,
                terminal_state: "sent",
            };
            run_with_effects(&account, Lang::En, &mut io, &mut effects).unwrap();
            assert_eq!(effects.create_calls, 0);
            assert_eq!(effects.request_calls, 0);
        }
    }

    #[test]
    fn cli04_preview_escapes_controls_keeps_bidi_and_complete_body() {
        let body = format!("start\u{001b}[31m\u{202e}middle\n{}", "x".repeat(20_000));
        let preview = client::PersonalSendPreview {
            from: "from@example.test".into(),
            to: vec!["to@example.test".into()],
            subject: "subject".into(),
            body: body.clone(),
            attachments: vec![],
            payload_hash: [2; 32],
            draft_id: [1; 32],
            version: 1,
        };
        assert!(terminal_safe(&body).contains('\u{fffd}'));
        assert!(terminal_safe(&body).contains("\\u{202e}"));
        let mut io = TestIo::new(&[]);
        show_preview(&mut io, Lang::En, &preview).unwrap();
        assert!(io.output.contains(&"x".repeat(20_000)));
        assert!(!io.output.contains('\u{001b}'));
    }
}
