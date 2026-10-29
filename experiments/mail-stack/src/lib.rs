//! Isolated compatibility smoke tests for the proposed mail stack.
//!
//! TLS and SMTP AUTH tests use only in-memory streams and public test keys.
//! No real network, account, secret store, or product broker is accessed.

mod stream;
#[cfg(test)]
mod imap_gate;
#[cfg(test)]
mod imap_probe_tests;
pub use stream::{BoundedAsyncStream, LettreTokioStream, MAX_RECEIVED_BYTES};

#[cfg(test)]
mod tls_tests;

#[cfg(test)]
mod implicit_tls_tests;

/// Compile-time references to the public APIs selected for the experiment.
pub fn selected_public_types() {
    let _ =
        std::any::type_name::<io_imap::rfc3501::fetch_stream_batch::ImapMessageFetchStreamBatch>();
    let _ = std::any::type_name::<
        io_imap::rfc3501::fetch_stream_batch::ImapMessageFetchStreamBatchYield,
    >();
    let _ = std::any::type_name::<lettre::transport::smtp::client::Tls>();
    let _ = std::any::type_name::<mail_parser::MessageParser>();
    let _ = std::any::type_name::<mail_builder::MessageBuilder>();
}

#[cfg(test)]
mod tests {
    use mail_builder::MessageBuilder;
    use mail_parser::MessageParser;

    #[test]
    fn mime_builder_output_can_be_parsed_without_network() {
        let mut output = Vec::new();
        MessageBuilder::new()
            .from(("Sender", "sender@example.com"))
            .to("recipient@example.com")
            .subject("한글 테스트")
            .text_body("본문\r\n")
            .write_to(&mut output)
            .expect("mail-builder should write a small message");

        let parsed = MessageParser::new()
            .parse(&output)
            .expect("mail-parser should parse the generated message");
        assert_eq!(parsed.subject(), Some("한글 테스트"));
        assert_eq!(parsed.body_text(0).as_deref(), Some("본문\r\n"));
    }
}
