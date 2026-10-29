use std::{collections::BTreeSet, fs, path::Path};

fn keys(path: &Path) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    for line in fs::read_to_string(path).expect("resource").lines() {
        let (key, value) = line.split_once('=').expect("resource line must contain =");
        assert!(
            !key.is_empty() && !value.is_empty(),
            "resource key/value must be non-empty"
        );
        assert!(seen.insert(key.to_owned()), "duplicate resource key");
    }
    seen
}
fn main() {
    let en = keys(Path::new("resources/en.txt"));
    let ko = keys(Path::new("resources/ko.txt"));
    assert_eq!(en, ko, "English and Korean CLI resource keys must match");
    for key in [
        "usage",
        "commands",
        "core_status",
        "core_unavailable",
        "capability_unsupported",
        "protected",
        "help_language",
        "client_not_registered",
        "client_verified",
        "invalid_input",
        "generic_error",
        "registration_pending",
        "registration_denied",
        "registration_expired",
        "registration_provisioning",
        "registration_ready",
        "registration_request_id",
        "setup_request_id",
        "setup_next_action",
        "setup_state_pending",
        "setup_state_connecting",
        "setup_state_ready",
        "setup_state_auth_required",
        "setup_state_tls_failed",
        "setup_state_offline",
        "setup_state_error",
        "setup_state_denied",
        "setup_state_cancelled",
        "setup_state_expired",
        "setup_state_interrupted",
        "setup_action_helper_required",
        "setup_action_wait",
        "setup_action_client_registration_required",
        "setup_action_profile_scope_review_required",
        "setup_action_resume_required",
        "setup_action_new_request_required",
        "account_number",
        "provider",
        "address",
        "capabilities",
        "status",
        "provider_gmail",
        "provider_graph",
        "provider_imap",
        "provider_archive",
        "status_ready",
        "status_connecting",
        "status_reauth_required",
        "status_offline",
        "status_disabled",
        "status_error",
    ] {
        assert!(en.contains(key), "missing required resource key: {key}");
    }
    println!("cargo:rerun-if-changed=resources/en.txt");
    println!("cargo:rerun-if-changed=resources/ko.txt");
}
