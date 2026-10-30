//! Initial preview through the signed production pipe; only a synthetic provider.
use super::*;
use crate::uid_worker::InitialObservation;
use hantong_contracts::initial_sync::*;
use hantong_domain::{imap_observation::ImapUidObservation, imap_reconcile::ImapUidReconcilePage};
use hantong_store::initial_sync::{InitialSyncFetch, InitialSyncJob};

struct PreviewFake {
    observe: Rc<Cell<u32>>,
    pages: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    dropped: Rc<Cell<u32>>,
}
impl crate::uid_worker::Discovery for PreviewFake {
    async fn discover(&self, _: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        panic!("unexpected base FETCH")
    }
    async fn initial(&self, job: &InitialSyncJob) -> Result<InitialObservation, ErrorCode> {
        match job.fetch_request() {
            InitialSyncFetch::Observe(r) => {
                self.observe.set(self.observe.get() + 1);
                Ok(InitialObservation::Target(
                    ImapUidObservation::new(r.clone(), 7, 513).unwrap(),
                ))
            }
            InitialSyncFetch::Page(r) => {
                let _drop = Dropped(self.dropped.clone());
                self.pages.set(self.pages.get() + 1);
                self.entered.notify_one();
                std::future::pending::<()>().await;
                Ok(InitialObservation::Page(
                    ImapUidReconcilePage::new(r.clone(), 513, vec![]).unwrap(),
                ))
            }
        }
    }
}
fn mutation(op: &str) -> InitialSyncMutation {
    InitialSyncMutation::new(op, op, &"00".repeat(32)).unwrap()
}
fn prepare(r: InitialSyncRequest) -> InitialSyncRequest {
    hantong_ipc_client::initial_sync::prepare_request(r).unwrap()
}
fn params(r: &InitialSyncRequest) -> Value {
    let mut v = json!({"action":r.action().as_str(),"account_id":r.account_id(),"wire_hex":r.mailbox().wire_hex(),"encoding":r.mailbox().encoding().as_str(),"operation_id":r.operation_id(),"idempotency_key":r.idempotency_key(),"intent_hash":r.intent_hash()});
    if let Some(n) = r.expected_catalog_generation() {
        v["expected_catalog_generation"] = json!(n)
    }
    if let Some(n) = r.expected_preview_generation() {
        v["expected_preview_generation"] = json!(n)
    }
    if let Some(n) = r.expected_plan_version() {
        v["expected_plan_version"] = json!(n)
    }
    if let Some(n) = r.recent_pages() {
        v["recent_pages"] = json!(n)
    }
    if let Some(n) = r.plan_id() {
        v["plan_id"] = json!(hantong_contracts::wire_hex::encode(n))
    }
    v
}
async fn terminal(store: &Store, owner: [u8; 16], id: [u8; 32]) -> InitialSyncReceipt {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let r = store
                .load_initial_sync_receipt(
                    owner,
                    &UidPageStatusRequest::new("account_a", id).unwrap(),
                )
                .unwrap();
            if r.state().is_terminal() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn sync01_sec03_initial_actions_follow_signed_pipe_grants_and_audit_ownership() {
    let _owner = PIPE_TEST.lock().await;
    tokio::time::timeout(Duration::from_secs(15),async {
        let keys=TestKeys::new();let client=keys.client.as_ref().unwrap();let server=keys.server.as_ref().unwrap();
        let instance=random_nonce().unwrap();let mut listener=bind_fixture_pipe().await;
        let fixture=PolicyStoreFixture::new(server,client.public_key().unwrap()).unwrap();
        fixture.mutate(PolicyFixtureMutation::AllowUidPage).unwrap();
        let store=fixture.open().unwrap();catalog(&store,0);
        let observe=Rc::new(Cell::new(0));let pages=Rc::new(Cell::new(0));let entered=Rc::new(Notify::new());let dropped=Rc::new(Cell::new(0));
        let fake=PreviewFake{observe:observe.clone(),pages:pages.clone(),entered:entered.clone(),dropped:dropped.clone()};
        let shared=Rc::new(Cell::new(None));
        let(folder,_fw)=crate::folder_worker::coordinator_with_discovery_and_slot(&store,crate::folder_worker::NoDiscovery,shared.clone());
        let(uid,worker)=crate::uid_worker::coordinator_with_instance(&store,fake,shared,instance);
        let mut h=Harness{listener:&mut listener,client,server,instance,store:&store,folder:&folder,uid:&uid,registration:fixture.registration_id,policy:hantong_policy::ReadPolicy::new()};
        let flow=async {
            let start=prepare(InitialSyncRequest::new_start("account_a",mailbox(),mutation("pipe-initial"),1,0,2).unwrap());
            let raw=h.rpc("mail.sync.page",params(&start)).await;
            let receipt=hantong_ipc_client::initial_sync::parse_mutation_receipt(&raw,"uid-call",&start).unwrap();
            let id=*receipt.execution_id();
            let status=UidPageStatusRequest::new("account_a",id).unwrap();
            let raw=h.rpc("mail.sync.page",json!({"action":"initial-status","account_id":"account_a","execution_id":hantong_contracts::wire_hex::encode(&id)})).await;
            hantong_ipc_client::initial_sync::parse_status_receipt(&raw,"uid-call",&status).unwrap();
            let original=terminal(&store,fixture.registration_id,id).await;
            assert_eq!((observe.get(),pages.get()),(1,0));
            let plan_request=InitialSyncPlanRequest::new("account_a",mailbox(),id).unwrap();
            let raw=h.rpc("mail.sync.page",json!({"action":"initial-plan","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7","plan_id":hantong_contracts::wire_hex::encode(&id)})).await;
            let plan=hantong_ipc_client::initial_sync::parse_plan_snapshot(&raw,"uid-call",&plan_request).unwrap();
            let resume=prepare(InitialSyncRequest::new_resume("account_a",mailbox(),mutation("pipe-resume"),id,plan.version(),1).unwrap());
            let raw=h.rpc("mail.sync.page",params(&resume)).await;
            let receipt=hantong_ipc_client::initial_sync::parse_mutation_receipt(&raw,"uid-call",&resume).unwrap();let run=*receipt.execution_id();
            entered.notified().await;
            let plan=store.load_initial_sync_plan(fixture.registration_id,&plan_request).unwrap();
            let pause=prepare(InitialSyncRequest::new_pause("account_a",mailbox(),mutation("pipe-pause"),id,plan.version()).unwrap());
            let raw=h.rpc("mail.sync.page",params(&pause)).await;
            let paused=hantong_ipc_client::initial_sync::parse_mutation_receipt(&raw,"uid-call",&pause).unwrap();
            assert_eq!(paused.state(),InitialSyncJobState::Succeeded);
            assert_eq!(terminal(&store,fixture.registration_id,run).await.error(),Some(ErrorCode::Conflict));
            assert_eq!(store.load_initial_sync_receipt(fixture.registration_id,&status).unwrap(),original);
            let plan=store.load_initial_sync_plan(fixture.registration_id,&plan_request).unwrap();
            assert_eq!(plan.progress().state(),InitialSyncState::PausedUser);assert_eq!(plan.progress().completed_page_count(),0);
            assert_eq!((observe.get(),pages.get(),dropped.get()),(1,1,1));
            let base=ImapMailboxName::from_wire(b"INBOX",MailboxEncoding::ModifiedUtf7).unwrap();
            assert!(store.load_imap_uid_cursor("account_a",&base).unwrap().is_none());
            for (execution,error) in [(id,None),(run,Some(ErrorCode::Conflict)),(*paused.execution_id(),None)]{
                let audit=store.load_audit(execution).unwrap().unwrap();
                assert_eq!(audit.registration_id(),&fixture.registration_id);assert_eq!(audit.account_ids(),["account_a"]);
                assert_eq!(audit.completion().unwrap().terminal().error(),error);
            }
        };
        tokio::select!{biased;result=worker.run()=>panic!("unexpected worker exit: {result:?}"),_=flow=>{}}
    }).await.unwrap();
}
