//! Synthetic native first registration and Core broker integration, with no provider I/O.
use super::*;
use hantong_ipc_client::unix_relay::{RelayProfile, RelayRequest};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn cli07_unix_bootstrap_registration_then_authenticated_accounts() {
    const NAME: &str = "unix_core_runtime::product_tests::cli07_unix_bootstrap_registration_then_authenticated_accounts";
    if std::env::var_os("HANTONG_UNIX_PRODUCT_TEST_CHILD").is_none() {
        let root = std::env::temp_dir().join(format!(
            "hantong-product-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["data-home", "config-home", "runtime"] {
            std::fs::create_dir(root.join(name)).unwrap();
            std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        // Production peers are bounded to 64 MiB; debug symbols are not part of that image.
        let executable = root.join("core-product-test");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        assert!(
            std::process::Command::new("/usr/bin/strip")
                .arg("--strip-debug")
                .arg(&executable)
                .status()
                .unwrap()
                .success()
        );
        let status = std::process::Command::new(executable)
            .args(["--exact", NAME, "--nocapture"])
            .env("HANTONG_UNIX_PRODUCT_TEST_CHILD", "1")
            .env("XDG_DATA_HOME", root.join("data-home"))
            .env("XDG_CONFIG_HOME", root.join("config-home"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .status()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(status.success());
        return;
    }
    let paths = hantong_platform::app_directories().unwrap();
    let _keys = hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&paths.secrets).unwrap();
    let _data = hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&paths.data).unwrap();
    let vault = hantong_secrets::headless_vault::HeadlessVault::initialize(
        &paths.secrets,
        "synthetic-product-passphrase".to_owned().into(),
    )
    .unwrap()
    .into_shared();
    hantong_secrets::unix_secret_store::UnixSecretStore::install_headless_backend(vault).unwrap();
    let store = Store::open(&paths.data, &paths.secrets).unwrap();
    store.initialize_config("UTC").unwrap();
    let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
        "fixture_account".into(),
        Some("Synthetic account".into()),
        None,
    )
    .unwrap();
    let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(&serde_json::to_vec(&json!({"schema_version":1,"account_id":"fixture_account","imap":{"settings_version":1,"host":"imap.example.invalid","port":993,"tls":"implicit_tls","login_name":"fixture@example.invalid","authentication":"app_password"}})).unwrap()).unwrap();
    let mutation = hantong_store::mail_connection::MailMutation::new(
        "fixture-operation".into(),
        "fixture-idempotency".into(),
        0,
        hantong_store::account_provisioning::imap_account_stage_payload_hash(&draft, config.imap())
            .unwrap(),
    )
    .unwrap();
    let staged = store
        .stage_imap_account(&draft, config.imap(), &mutation)
        .unwrap();
    store
        .finish_initial_imap_probe(
            "fixture_account",
            staged.account_version,
            staged.imap_version,
            0,
            &hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(vec![
                "imap.move".into(),
            ]),
        )
        .unwrap();
    let signer = crate::unix_identity::initialize_server_identity(&store, &paths.secrets).unwrap();
    let server = serve(
        store,
        signer,
        hantong_platform::random_nonce().unwrap(),
        &paths.secrets,
    );
    tokio::pin!(server);
    let client = async {
        let custom_client = rpc(
            "custom-client",
            "setup.google.request",
            json!({
                "protocol_version":1,"client_id":"123-fixture.apps.googleusercontent.com",
                "display_name":null,"reauthorize_account":null,"force_consent":false,
                "access_mode":"readonly","local_profile":"personal_read"
            }),
        )
        .await;
        assert_eq!(
            custom_client["error"]["code"], "CAPABILITY_UNSUPPORTED",
            "an explicit custom client must never silently use the bundled publisher"
        );
        let before = rpc(
            "before",
            "mail.accounts.list",
            json!({"account_ids":["fixture_account"],"include_capabilities":false}),
        )
        .await;
        assert!(
            before.get("error").is_some(),
            "no profile means no account access"
        );
        let requested = rpc(
            "request",
            "registration.request",
            json!({"protocol_version":1}),
        )
        .await;
        let request_id = requested["result"]["request_id"]
            .as_str()
            .expect("pending request id");
        let reviewed = rpc(
            "get",
            "terminal.review.get",
            json!({"protocol_version":1,"kind":"registration","request_id":request_id,"account_id":null,"payload_hash":"00".repeat(32)}),
        )
        .await;
        let ticket = reviewed["result"]["ticket_id"]
            .as_str()
            .unwrap_or_else(|| panic!("review ticket missing: {reviewed:?}"));
        let hash = reviewed["result"]["snapshot_hash"].as_str().unwrap();
        let still_pending = rpc(
            "pending",
            "mail.accounts.list",
            json!({"account_ids":["fixture_account"],"include_capabilities":false}),
        )
        .await;
        assert!(
            still_pending.get("error").is_some(),
            "Get cannot activate registration"
        );
        let confirmed = rpc(
            "confirm",
            "terminal.review.confirm",
            json!({"protocol_version":1,"ticket_id":ticket,"snapshot_hash":hash}),
        )
        .await;
        assert_eq!(confirmed["result"]["state"], "ready", "{confirmed:?}");
        let accounts = rpc(
            "accounts",
            "mail.accounts.list",
            json!({"account_ids":["fixture_account"],"include_capabilities":false}),
        )
        .await;
        assert!(accounts.get("error").is_none(), "{accounts:?}");
        assert!(accounts["result"].to_string().contains("fixture_account"));
        let outside_scope = rpc(
            "outside",
            "mail.accounts.list",
            json!({"account_ids":["other_account"],"include_capabilities":false}),
        )
        .await;
        assert!(
            outside_scope.get("error").is_some(),
            "account scope cannot widen: {outside_scope:?}"
        );
        let replay = rpc(
            "replay",
            "terminal.review.confirm",
            json!({"protocol_version":1,"ticket_id":ticket,"snapshot_hash":hash}),
        )
        .await;
        assert!(
            replay.get("error").is_some(),
            "review ticket must be one use"
        );
        let scope = rpc(
            "scope-request",
            "registration.scope.request",
            json!({"protocol_version":1}),
        )
        .await;
        let refresh = scope["result"]["refresh_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{scope:?}"));
        let preview = rpc("scope-preview", "terminal.review.get", json!({"protocol_version":1,"kind":"scope_refresh","request_id":refresh,"account_id":null,"payload_hash":"00".repeat(32)})).await;
        let ticket = preview["result"]["ticket_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{preview:?}"));
        let hash = preview["result"]["snapshot_hash"].as_str().unwrap();
        let current_params = json!({"action":"initial-current","account_id":"fixture_account","wire_hex":"494e424f58","encoding":"utf8"});
        let denied = rpc(
            "sync-before-confirm",
            "mail.sync.page",
            current_params.clone(),
        )
        .await;
        assert_eq!(
            denied["error"]["code"], "SCOPE_DENIED",
            "scope request must not grant: {denied:?}"
        );
        let granted = rpc(
            "scope-confirm",
            "terminal.review.confirm",
            json!({"protocol_version":1,"ticket_id":ticket,"snapshot_hash":hash}),
        )
        .await;
        assert_eq!(granted["result"]["state"], "ready", "{granted:?}");
        let current = rpc("current-plan", "mail.sync.page", current_params).await;
        assert_eq!(current["result"]["ok"], true, "{current:?}");
        assert!(current["result"]["plan"].is_null());
    };
    tokio::select! {
        biased;
        result=server.as_mut()=>panic!("server stopped: {result:?}"),
        result=tokio::time::timeout(Duration::from_secs(60),client)=>result.expect("product flow completed before deadline")
    }
}

async fn rpc(id: &str, method: &str, params: Value) -> Value {
    let request = RelayRequest::new(RelayProfile::Default, id, method, params).unwrap();
    let bytes = hantong_ipc_client::unix_relay::broker_exchange(&request)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
