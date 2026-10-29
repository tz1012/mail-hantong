#![forbid(unsafe_code)]

//! Deterministic, network-free provider fixtures. Test support only.

pub mod platform;

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeError {
    EmptyAccount,
    WrongAccount,
    HistoryExpired,
    DeltaExpired,
    RateLimited,
    AuthenticationFailed,
    StateMismatch,
    CallbackReplayed,
    RefreshFailed,
    Revoked,
    PartialScope,
    NoMorePages,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub account_id: String,
    pub operation: String,
    pub accepted: bool,
}

fn check_account(expected: &str, actual: &str) -> Result<(), FakeError> {
    if expected.is_empty() || actual.is_empty() {
        return Err(FakeError::EmptyAccount);
    }
    (expected == actual)
        .then_some(())
        .ok_or(FakeError::WrongAccount)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmailScenario {
    Normal,
    HistoryExpired,
    RateLimited,
    LostSendAck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    Accepted,
    LostAck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSendOutcome {
    Accepted202,
    LostAck,
}

#[derive(Debug)]
pub struct FakeGmail {
    account_id: String,
    scenario: GmailScenario,
    calls: Vec<CallRecord>,
    accepted_submissions: usize,
}

impl FakeGmail {
    pub fn new(account_id: impl Into<String>, scenario: GmailScenario) -> Self {
        Self {
            account_id: account_id.into(),
            scenario,
            calls: Vec::new(),
            accepted_submissions: 0,
        }
    }
    pub fn history(&mut self, account_id: &str) -> Result<Vec<String>, FakeError> {
        check_account(&self.account_id, account_id)?;
        if self.scenario == GmailScenario::HistoryExpired {
            self.calls.push(CallRecord {
                account_id: account_id.into(),
                operation: "history".into(),
                accepted: false,
            });
            return Err(FakeError::HistoryExpired);
        }
        if self.scenario == GmailScenario::RateLimited {
            self.calls.push(CallRecord {
                account_id: account_id.into(),
                operation: "history".into(),
                accepted: false,
            });
            return Err(FakeError::RateLimited);
        }
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: "history".into(),
            accepted: true,
        });
        Ok(vec!["message-1".into()])
    }
    pub fn send(&mut self, account_id: &str, _payload: &str) -> Result<SendOutcome, FakeError> {
        check_account(&self.account_id, account_id)?;
        if self.scenario == GmailScenario::RateLimited {
            self.calls.push(CallRecord {
                account_id: account_id.into(),
                operation: "send".into(),
                accepted: false,
            });
            return Err(FakeError::RateLimited);
        }
        self.accepted_submissions += 1;
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: "send".into(),
            accepted: true,
        });
        Ok(if self.scenario == GmailScenario::LostSendAck {
            SendOutcome::LostAck
        } else {
            SendOutcome::Accepted
        })
    }
    pub fn calls(&self) -> &[CallRecord] {
        &self.calls
    }
    pub fn accepted_submissions(&self) -> usize {
        self.accepted_submissions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphScenario {
    Normal,
    DeltaExpired,
    ChangedIds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPage {
    pub items: Vec<String>,
    pub next_cursor: Option<usize>,
}

#[derive(Debug)]
pub struct FakeGraph {
    account_id: String,
    scenario: GraphScenario,
    pages: Vec<Vec<String>>,
    calls: Vec<CallRecord>,
    accepted_submissions: usize,
}

impl FakeGraph {
    pub fn new(
        account_id: impl Into<String>,
        scenario: GraphScenario,
        pages: Vec<Vec<String>>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            scenario,
            pages,
            calls: Vec::new(),
            accepted_submissions: 0,
        }
    }
    pub fn delta(
        &mut self,
        account_id: &str,
        cursor: Option<usize>,
    ) -> Result<GraphPage, FakeError> {
        check_account(&self.account_id, account_id)?;
        if self.scenario == GraphScenario::DeltaExpired {
            self.calls.push(CallRecord {
                account_id: account_id.into(),
                operation: "delta".into(),
                accepted: false,
            });
            return Err(FakeError::DeltaExpired);
        }
        let index = cursor.unwrap_or(0);
        let Some(items) = self.pages.get(index).cloned() else {
            return Err(FakeError::NoMorePages);
        };
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: "delta".into(),
            accepted: true,
        });
        let items = if self.scenario == GraphScenario::ChangedIds {
            items.into_iter().map(|id| format!("{id}-moved")).collect()
        } else {
            items
        };
        Ok(GraphPage {
            items,
            next_cursor: (index + 1 < self.pages.len()).then_some(index + 1),
        })
    }
    pub fn calls(&self) -> &[CallRecord] {
        &self.calls
    }
    pub fn send_mail(
        &mut self,
        account_id: &str,
        _payload: &str,
    ) -> Result<GraphSendOutcome, FakeError> {
        check_account(&self.account_id, account_id)?;
        self.accepted_submissions += 1;
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: "sendMail".into(),
            accepted: true,
        });
        Ok(GraphSendOutcome::Accepted202)
    }
    pub fn accepted_submissions(&self) -> usize {
        self.accepted_submissions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpScenario {
    Normal,
    AuthenticationFailed,
    LostSendAck,
}

#[derive(Debug)]
pub struct FakeSmtp {
    account_id: String,
    scenario: SmtpScenario,
    calls: Vec<CallRecord>,
    accepted_submissions: usize,
}

impl FakeSmtp {
    pub fn new(account_id: impl Into<String>, scenario: SmtpScenario) -> Self {
        Self {
            account_id: account_id.into(),
            scenario,
            calls: Vec::new(),
            accepted_submissions: 0,
        }
    }
    pub fn submit(&mut self, account_id: &str, _payload: &str) -> Result<SendOutcome, FakeError> {
        check_account(&self.account_id, account_id)?;
        if self.scenario == SmtpScenario::AuthenticationFailed {
            self.calls.push(CallRecord {
                account_id: account_id.into(),
                operation: "submit".into(),
                accepted: false,
            });
            return Err(FakeError::AuthenticationFailed);
        }
        self.accepted_submissions += 1;
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: "submit".into(),
            accepted: true,
        });
        Ok(if self.scenario == SmtpScenario::LostSendAck {
            SendOutcome::LostAck
        } else {
            SendOutcome::Accepted
        })
    }
    pub fn accepted_submissions(&self) -> usize {
        self.accepted_submissions
    }
    pub fn calls(&self) -> &[CallRecord] {
        &self.calls
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthScenario {
    Normal,
    PartialScope,
    RefreshFailure,
    Revoked,
}

#[derive(Debug)]
pub struct FakeOAuth {
    account_id: String,
    scenario: OAuthScenario,
    scopes: BTreeSet<String>,
    expected_state: String,
    callback_used: bool,
    calls: Vec<CallRecord>,
}

impl FakeOAuth {
    pub fn new(
        account_id: impl Into<String>,
        scenario: OAuthScenario,
        expected_state: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            scenario,
            scopes: scopes.into_iter().map(Into::into).collect(),
            expected_state: expected_state.into(),
            callback_used: false,
            calls: Vec::new(),
        }
    }
    pub fn callback(&mut self, account_id: &str, callback_state: &str) -> Result<(), FakeError> {
        check_account(&self.account_id, account_id)?;
        if self.expected_state.is_empty() || callback_state.is_empty() {
            self.record(account_id, "oauth.callback", false);
            return Err(FakeError::StateMismatch);
        }
        if self.expected_state != callback_state {
            self.record(account_id, "oauth.callback", false);
            return Err(FakeError::StateMismatch);
        }
        if self.callback_used {
            self.record(account_id, "oauth.callback", false);
            return Err(FakeError::CallbackReplayed);
        }
        self.callback_used = true;
        self.record(account_id, "oauth.callback", true);
        Ok(())
    }
    pub fn require_scopes(&self, required: &[&str]) -> Result<(), FakeError> {
        if self.scenario == OAuthScenario::PartialScope
            || required.iter().any(|scope| !self.scopes.contains(*scope))
        {
            return Err(FakeError::PartialScope);
        }
        Ok(())
    }
    pub fn refresh(&mut self, account_id: &str) -> Result<OAuthStatus, FakeError> {
        check_account(&self.account_id, account_id)?;
        match self.scenario {
            OAuthScenario::RefreshFailure => {
                self.record(account_id, "oauth.refresh", false);
                Err(FakeError::RefreshFailed)
            }
            OAuthScenario::Revoked => {
                self.record(account_id, "oauth.refresh", false);
                Err(FakeError::Revoked)
            }
            _ => {
                self.record(account_id, "oauth.refresh", true);
                Ok(OAuthStatus::Refreshed)
            }
        }
    }
    pub fn revoke(&mut self, account_id: &str) -> Result<(), FakeError> {
        check_account(&self.account_id, account_id)?;
        self.scenario = OAuthScenario::Revoked;
        self.record(account_id, "oauth.revoke", true);
        Ok(())
    }
    pub fn calls(&self) -> &[CallRecord] {
        &self.calls
    }
    fn record(&mut self, account_id: &str, operation: &str, accepted: bool) {
        self.calls.push(CallRecord {
            account_id: account_id.into(),
            operation: operation.into(),
            accepted,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthStatus {
    Refreshed,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync07_lost_send_ack_records_one_acceptance_without_retry() {
        let mut fake = FakeGmail::new("account-a", GmailScenario::LostSendAck);
        assert_eq!(fake.send("account-a", "fixture"), Ok(SendOutcome::LostAck));
        assert_eq!(fake.accepted_submissions(), 1);
        assert_eq!(fake.calls().len(), 1);
    }
    #[test]
    fn acc03_wrong_account_is_rejected_without_call_record() {
        let mut fake = FakeGmail::new("account-a", GmailScenario::Normal);
        assert_eq!(fake.history("account-b"), Err(FakeError::WrongAccount));
        assert!(fake.calls().is_empty());
    }
    #[test]
    fn acc02_oauth_rejects_mismatch_and_replay() {
        let mut oauth =
            FakeOAuth::new("account-a", OAuthScenario::Normal, "state-a", ["mail.read"]);
        assert_eq!(
            oauth.callback("account-a", "state-b"),
            Err(FakeError::StateMismatch)
        );
        assert_eq!(oauth.callback("account-a", "state-a"), Ok(()));
        assert_eq!(
            oauth.callback("account-a", "state-a"),
            Err(FakeError::CallbackReplayed)
        );
    }

    #[test]
    fn ext03_gmail_history_rate_limit_and_expiry_are_explicit() {
        let mut limited = FakeGmail::new("account-a", GmailScenario::RateLimited);
        assert_eq!(limited.history("account-a"), Err(FakeError::RateLimited));
        let mut expired = FakeGmail::new("account-a", GmailScenario::HistoryExpired);
        assert_eq!(expired.history("account-a"), Err(FakeError::HistoryExpired));
    }

    #[test]
    fn ext08_graph_delta_expiry_changed_id_and_send_202_are_recorded() {
        let mut expired = FakeGraph::new("account-a", GraphScenario::DeltaExpired, vec![]);
        assert_eq!(
            expired.delta("account-a", None),
            Err(FakeError::DeltaExpired)
        );
        let mut moved = FakeGraph::new(
            "account-a",
            GraphScenario::ChangedIds,
            vec![vec!["m1".into()]],
        );
        assert_eq!(
            moved.delta("account-a", None).unwrap().items,
            vec!["m1-moved"]
        );
        assert_eq!(
            moved.send_mail("account-a", "fixture"),
            Ok(GraphSendOutcome::Accepted202)
        );
        assert_eq!(moved.accepted_submissions(), 1);
    }

    #[test]
    fn ext07_smtp_auth_failure_and_lost_ack_are_distinct() {
        let mut auth = FakeSmtp::new("account-a", SmtpScenario::AuthenticationFailed);
        assert_eq!(
            auth.submit("account-a", "fixture"),
            Err(FakeError::AuthenticationFailed)
        );
        let mut lost = FakeSmtp::new("account-a", SmtpScenario::LostSendAck);
        assert_eq!(
            lost.submit("account-a", "fixture"),
            Ok(SendOutcome::LostAck)
        );
        assert_eq!(lost.accepted_submissions(), 1);
    }

    #[test]
    fn acc02_oauth_scope_refresh_revoke_and_account_binding_are_explicit() {
        let mut partial = FakeOAuth::new(
            "account-a",
            OAuthScenario::PartialScope,
            "state-a",
            ["mail.read"],
        );
        assert_eq!(
            partial.require_scopes(&["mail.send"]),
            Err(FakeError::PartialScope)
        );
        assert_eq!(
            partial.callback("account-b", "state-a"),
            Err(FakeError::WrongAccount)
        );
        let mut refresh = FakeOAuth::new(
            "account-a",
            OAuthScenario::RefreshFailure,
            "state-a",
            ["mail.read"],
        );
        assert_eq!(refresh.refresh("account-a"), Err(FakeError::RefreshFailed));
        let mut revoked =
            FakeOAuth::new("account-a", OAuthScenario::Normal, "state-a", ["mail.read"]);
        assert_eq!(revoked.revoke("account-a"), Ok(()));
        assert_eq!(revoked.refresh("account-a"), Err(FakeError::Revoked));
        assert_eq!(revoked.calls().len(), 2);
    }

    #[test]
    fn all_providers_reject_empty_or_wrong_accounts_without_fallback() {
        let mut gmail = FakeGmail::new("account-a", GmailScenario::Normal);
        assert_eq!(gmail.history(""), Err(FakeError::EmptyAccount));
        let mut graph = FakeGraph::new("account-a", GraphScenario::Normal, vec![vec!["m1".into()]]);
        assert_eq!(graph.delta("account-b", None), Err(FakeError::WrongAccount));
        let mut smtp = FakeSmtp::new("account-a", SmtpScenario::Normal);
        assert_eq!(
            smtp.submit("account-b", "fixture"),
            Err(FakeError::WrongAccount)
        );
        let mut oauth =
            FakeOAuth::new("account-a", OAuthScenario::Normal, "state-a", ["mail.read"]);
        assert_eq!(oauth.callback("", "state-a"), Err(FakeError::EmptyAccount));
    }
    #[test]
    fn ext08_graph_pages_exhaust_without_fallback_success() {
        let mut graph = FakeGraph::new(
            "account-a",
            GraphScenario::Normal,
            vec![vec!["m1".into()], vec!["m2".into()]],
        );
        let first = graph.delta("account-a", None).unwrap();
        let second = graph.delta("account-a", first.next_cursor).unwrap();
        assert_eq!(second.next_cursor, None);
        assert_eq!(graph.delta("account-a", None), Ok(first));
        assert_eq!(
            graph.delta("account-a", Some(2)),
            Err(FakeError::NoMorePages)
        );
    }
}
