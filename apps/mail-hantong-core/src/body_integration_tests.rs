//! Full signed IPC -> shared policy/audit -> fake provider -> encrypted body cache.
use super::*;
use hantong_contracts::message_body::{MessageBody, MessageBodyRequest};
struct BodyProvider {
    calls: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    release: Rc<Notify>,
}
impl crate::uid_worker::Discovery for BodyProvider {
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        let message = ImapMessageMetadata::new(
            job.fetch_request().after_uid() + 1,
            vec![],
            0,
            0,
            ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
        )
        .unwrap();
        Ok(ImapUidPage::new(job.fetch_request().clone(), 7, 130, vec![message]).unwrap())
    }
    async fn body(
        &self,
        request: &MessageBodyRequest,
        _: &hantong_domain::mail_connection::ImapEndpointConfig,
    ) -> Result<MessageBody, ErrorCode> {
        self.calls.set(self.calls.get() + 1);
        self.entered.notify_one();
        self.release.notified().await;
        MessageBody::new(
            request.clone(),
            b"Content-Type: text/plain; charset=utf-8\r\n\r\nHello\x1b[2J".to_vec(),
            false,
        )
    }
}
pub(super) async fn run(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowBodies).unwrap();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let calls = Rc::new(Cell::new(0));
    let entered = Rc::new(Notify::new());
    let release = Rc::new(Notify::new());
    let slot = Rc::new(Cell::new(None));
    let (folder, _fw) = crate::folder_worker::coordinator_with_discovery_and_slot(
        &store,
        crate::folder_worker::NoDiscovery,
        slot.clone(),
    );
    let (uid, worker) = crate::uid_worker::coordinator_with_discovery(
        &store,
        BodyProvider {
            calls: calls.clone(),
            entered: entered.clone(),
            release: release.clone(),
        },
        slot,
    );
    let mut h = Harness {
        listener,
        client,
        server,
        instance,
        store: &store,
        folder: &folder,
        uid: &uid,
        registration: fixture.registration_id,
        policy: hantong_policy::ReadPolicy::new(),
    };
    let body = MessageBodyRequest::new("account_a", mailbox(), 7, 1, 1024).unwrap();
    let params = || json!({"mode":"imap_body","account_id":"account_a","mailbox_wire_hex":"494e424f58","mailbox_encoding":"modified_utf7","uidvalidity":7,"uid":1,"max_bytes":1024});
    let flow = async {
        h.append_complete("body-base", 0).await;
        let raw = h.rpc("mail.messages.get", params()).await;
        assert_eq!(
            hantong_ipc_client::message_body::parse_response(&raw, "uid-call", &body)
                .unwrap()
                .state,
            "pending"
        );
        entered.notified().await;
        let raw = h.rpc("mail.messages.get", params()).await;
        assert_eq!(
            hantong_ipc_client::message_body::parse_response(&raw, "uid-call", &body)
                .unwrap()
                .state,
            "pending"
        );
        assert_eq!(calls.get(), 1);
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let raw = h.rpc("mail.messages.get", params()).await;
                let r = hantong_ipc_client::message_body::parse_response(&raw, "uid-call", &body)
                    .unwrap();
                if let Some(text) = r.text {
                    assert_eq!(text, "Hello");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(store.cached_message_body(&body).unwrap().is_some());
        let mut wrong = params();
        wrong["account_id"] = json!("account_b");
        error(
            &h.rpc("mail.messages.get", wrong).await,
            ErrorCode::ScopeDenied,
        );
        let mut stale = params();
        stale["uidvalidity"] = json!(8);
        error(
            &h.rpc("mail.messages.get", stale).await,
            ErrorCode::Conflict,
        );
        let search=h.rpc("mail.messages.search",json!({"mode":"imap_metadata","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7"})).await;
        let search: Value = serde_json::from_slice(&search).unwrap();
        let message = search["result"]["items"][0]["internal_message_id"]
            .as_str()
            .unwrap();
        let raw=h.rpc("mail.messages.get",json!({"mode":"locator","account_id":"account_a","internal_message_id":message,"max_bytes":1024})).await;
        let located = hantong_ipc_client::message_body::parse_locator(
            &raw,
            "uid-call",
            "account_a",
            message,
            1024,
        )
        .unwrap();
        assert_eq!(located, body);
        assert_eq!(calls.get(), 1);
    };
    tokio::pin!(flow);
    let run = worker.run();
    tokio::pin!(run);
    tokio::time::timeout(Duration::from_secs(8), async {
        tokio::select! {_=&mut flow=>{},r=&mut run=>panic!("body worker stopped: {r:?}")}
    })
    .await
    .unwrap();
}
