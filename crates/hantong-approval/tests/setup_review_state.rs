use hantong_approval::{AccountSetupReview, Presentation, ReviewIntent, ReviewModel, ReviewState};
use hantong_domain::{
    account_provisioning::NewImapAccountDraft, mail_connection::MailConnectionConfig,
};
use std::time::Duration;

fn review(nonce: u8) -> AccountSetupReview {
    let draft = NewImapAccountDraft::new("fixture".into(), None, None).unwrap();
    let connection = MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"fixture","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"fixture","authentication":"app_password"}}"#).unwrap();
    AccountSetupReview::new(
        draft,
        connection.imap().clone(),
        [1; 32],
        [2; 32],
        [nonce; 32],
        1_000,
        301_000,
    )
    .unwrap()
}

#[test]
fn inst05_setup_cannot_request_verification_before_exact_snapshot_paint_or_twice() {
    let expected = review(3);
    let mut model = ReviewModel::new(
        expected.clone(),
        Presentation::defaults(false, false, false),
    );
    assert_eq!(
        model.begin_verification(1_001, Duration::from_millis(1)),
        ReviewIntent::None
    );
    assert!(!model.paint_completed(&review(4)));
    assert_eq!(model.state(), ReviewState::AwaitingPaint);
    assert!(model.paint_completed(&expected));
    assert_eq!(
        model.begin_verification(1_002, Duration::from_millis(2)),
        ReviewIntent::RequestVerification
    );
    assert_eq!(model.state(), ReviewState::Verifying);
    assert_eq!(
        model.begin_verification(1_003, Duration::from_millis(3)),
        ReviewIntent::None
    );
    assert_eq!(model.cancel(), ReviewIntent::Cancel);
    assert!(!model.paint_completed(&expected));
    assert_eq!(
        model.begin_verification(1_004, Duration::from_millis(4)),
        ReviewIntent::None
    );
}

#[test]
fn inst05_setup_wall_monotonic_expiry_and_rollback_are_terminal() {
    for (now, elapsed) in [
        (999, Duration::ZERO),
        (301_000, Duration::ZERO),
        (1_010, Duration::from_secs(300)),
    ] {
        let mut model = ReviewModel::new(review(3), Presentation::defaults(false, false, false));
        assert_eq!(model.check_expiry(now, elapsed), ReviewIntent::Expired);
        assert_eq!(model.state(), ReviewState::Expired);
        assert_eq!(
            model.begin_verification(1_011, Duration::ZERO),
            ReviewIntent::None
        );
    }
    let mut model = ReviewModel::new(review(3), Presentation::defaults(false, false, false));
    assert_eq!(
        model.check_expiry(1_010, Duration::from_secs(2)),
        ReviewIntent::None
    );
    assert_eq!(
        model.check_expiry(1_011, Duration::from_secs(1)),
        ReviewIntent::Expired
    );
}
