//! Temporary test-only module injected into hantong-secrets/src/lib.rs by the
//! macOS compatibility harness. It is never part of a product build.

use super::{MailCredentialSlot, SecretKind, SecretStore};
use std::env;
use std::path::PathBuf;

const ACCOUNT: &str = "synthetic-upgrade-account";
const ROOT_ENV: &str = "HANTONG_COMPAT_ROOT";
const PHASE_ENV: &str = "HANTONG_COMPAT_PHASE";
const IMAP_OLD: &[u8] = b"synthetic-old-imap";
const IMAP_NEW: &[u8] = b"synthetic-new-imap";
const SMTP_OLD: &[u8] = b"synthetic-old-smtp";

fn root() -> PathBuf {
    let root = PathBuf::from(env::var_os(ROOT_ENV).expect("compatibility root"));
    assert!(root.is_absolute() && root.is_dir());
    root
}

fn slot(kind: MailCredentialSlot) -> SecretKind {
    SecretKind::Mail(kind)
}

fn matches_bytes(
    store: &SecretStore,
    credential_slot: MailCredentialSlot,
    expected: &[u8],
    phase: &str,
    label: &str,
) -> bool {
    marker(phase, &format!("{label}-selector"));
    let selector = store
        .unix_selector(ACCOUNT, slot(credential_slot))
        .expect("mail selector");
    marker(phase, &format!("{label}-backend"));
    let protected = store.unix_store.read(&selector).expect("read opaque item");
    marker(phase, &format!("{label}-returned"));
    protected.as_bytes_in_secrets() == expected
}

fn marker(phase: &str, step: &str) {
    eprintln!("\nHANTONG_COMPAT_MARKER phase={phase} step={step}");
}

#[test]
fn mac_keychain_compatibility_fixture() {
    let phase = env::var(PHASE_ENV).expect("compatibility phase");
    assert!(matches!(phase.as_str(), "seed" | "upgrade" | "reopen"));
    marker(&phase, "open");
    let store = SecretStore::open(root()).expect("open isolated secret root");
    marker(&phase, "opened");
    let imap = MailCredentialSlot::ImapOAuthRefresh;
    let smtp = MailCredentialSlot::SmtpOAuthRefresh;
    match phase.as_str() {
        "seed" => {
            store
                .store_from_reader(ACCOUNT, slot(imap), IMAP_OLD)
                .expect("seed IMAP item");
            store
                .store_from_reader(ACCOUNT, slot(smtp), SMTP_OLD)
                .expect("seed SMTP item");
            marker("seed", "read");
            assert!(matches_bytes(&store, imap, IMAP_OLD, &phase, "imap-old"));
            assert!(matches_bytes(&store, smtp, SMTP_OLD, &phase, "smtp-old"));
            marker("seed", "done");
        }
        "upgrade" => {
            marker("upgrade", "read-old");
            assert!(matches_bytes(&store, imap, IMAP_OLD, &phase, "imap-old"));
            assert!(matches_bytes(&store, smtp, SMTP_OLD, &phase, "smtp-old"));
            marker("upgrade", "write-new");
            store
                .store_from_reader(ACCOUNT, slot(imap), IMAP_NEW)
                .expect("update IMAP item");
            marker("upgrade", "read-new");
            assert!(matches_bytes(&store, imap, IMAP_NEW, &phase, "imap-new"));
            assert!(matches_bytes(&store, smtp, SMTP_OLD, &phase, "smtp-old"));
            marker("upgrade", "done");
        }
        "reopen" => {
            marker("reopen", "read");
            assert!(matches_bytes(&store, imap, IMAP_NEW, &phase, "imap-new"));
            assert!(matches_bytes(&store, smtp, SMTP_OLD, &phase, "smtp-old"));
            marker("reopen", "done");
        }
        other => panic!("unsupported compatibility phase: {other}"),
    }
}
