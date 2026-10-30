use super::*;
use crate::smtp_probe::tests::{acceptor, fixture, tls_settings};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[derive(Clone, Copy)]
enum Outcome {
    Accepted,
    RecipientRejected,
    LostAck,
}
async fn line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> Vec<u8> {
    let mut data = Vec::new();
    let n = reader.read_until(b'\n', &mut data).await.unwrap();
    assert!(n > 0 && n <= 8192);
    data
}
async fn scenario(outcome: Outcome) -> Result<SmtpSendResult, SmtpSendError> {
    let (_cleanup, store) = fixture();
    let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"localhost","port":993,"tls":"implicit_tls","login_name":"synthetic@example.test","authentication":"app_password"},"smtp":{"settings_version":1,"host":"localhost","port":465,"tls":"implicit_tls","login_name":"synthetic@example.test","authentication":"app_password"}}"#).unwrap();
    let endpoint = config.smtp().unwrap();
    store
        .store_smtp_credential("acct", endpoint, b"synthetic-fixture-password".as_slice())
        .unwrap();
    let envelope = SmtpSendEnvelope {
        recipients: vec!["recipient@example.test".into()],
        subject: "안녕하세요".into(),
        body: "line one\n.leading line".into(),
    };
    let (client, server) = tokio::io::duplex(16384);
    let serve = async {
        let secure = acceptor().accept(server).await.unwrap();
        let mut reader = BufReader::new(secure);
        reader
            .get_mut()
            .write_all(b"220 localhost ESMTP\r\n")
            .await
            .unwrap();
        assert!(line(&mut reader).await.starts_with(b"EHLO "));
        reader
            .get_mut()
            .write_all(b"250-localhost\r\n250 AUTH PLAIN\r\n")
            .await
            .unwrap();
        // The synthetic credential stays inside this crate and is never logged.
        assert!(line(&mut reader).await.starts_with(b"AUTH PLAIN"));
        reader
            .get_mut()
            .write_all(b"235 authenticated\r\n")
            .await
            .unwrap();
        assert!(
            line(&mut reader)
                .await
                .starts_with(b"MAIL FROM:<synthetic@example.test>")
        );
        reader
            .get_mut()
            .write_all(b"250 sender ok\r\n")
            .await
            .unwrap();
        assert!(
            line(&mut reader)
                .await
                .starts_with(b"RCPT TO:<recipient@example.test>")
        );
        if matches!(outcome, Outcome::RecipientRejected) {
            reader
                .get_mut()
                .write_all(b"550 rejected\r\n")
                .await
                .unwrap();
            return;
        }
        reader
            .get_mut()
            .write_all(b"250 recipient ok\r\n")
            .await
            .unwrap();
        assert_eq!(line(&mut reader).await, b"DATA\r\n");
        reader
            .get_mut()
            .write_all(b"354 continue\r\n")
            .await
            .unwrap();
        let mut message = Vec::new();
        loop {
            let row = line(&mut reader).await;
            if row == b".\r\n" {
                break;
            }
            message.extend(row);
            assert!(message.len() < 16384)
        }
        let message = String::from_utf8(message).unwrap();
        assert!(message.contains("Content-Transfer-Encoding: base64"));
        assert!(message.contains("Subject: =?UTF-8?B?"));
        assert!(!message.contains("synthetic-fixture-password"));
        if matches!(outcome, Outcome::Accepted) {
            reader
                .get_mut()
                .write_all(b"250 accepted\r\n")
                .await
                .unwrap();
        }
    };
    let send = submit(
        &store,
        "acct",
        endpoint,
        &envelope,
        client,
        "127.0.0.1:465".parse().unwrap(),
        tls_settings("localhost", true),
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        let (result, ()) = tokio::join!(send, serve);
        result
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn ext03_smtp_submit_accepts_only_explicit_tls_authenticated_data_ack() {
    assert_eq!(
        scenario(Outcome::Accepted).await,
        Ok(SmtpSendResult::Accepted)
    );
}
#[tokio::test]
async fn apr01_smtp_rejected_recipient_never_submits_data() {
    assert_eq!(
        scenario(Outcome::RecipientRejected).await,
        Ok(SmtpSendResult::Rejected)
    );
}
#[tokio::test]
async fn sync07_smtp_disconnect_after_data_remains_uncertain() {
    assert_eq!(
        scenario(Outcome::LostAck).await,
        Err(SmtpSendError::Uncertain)
    );
}
