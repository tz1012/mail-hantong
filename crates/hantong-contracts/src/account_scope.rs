//! Account-qualified mail permissions. This is a typed contract, not an
//! approval or authentication proof.

use crate::{ErrorCode, auth::RequestedScope};
use serde_json::{Map, Value, json};
use std::fmt;

const MAX_ACCOUNTS: usize = 128;
const MAX_TOOLS: usize = 128;
const MAX_ACCOUNT_ID: usize = 128;
const MAX_TOOL_ID: usize = 96;
pub const MAX_SCOPE_BYTES: usize = 24 * 1024;

const ORDINARY_TOOLS: &[&str] = &[
    "app.diagnostics.snapshot",
    "app.settings.display.get",
    "mail.accounts.list",
    "mail.drafts.create",
    "mail.events.ack",
    "mail.events.configure",
    "mail.events.read",
    "mail.folders.list",
    "mail.folders.refresh",
    "mail.messages.get",
    "mail.messages.search",
    "mail.messages.trash.execute",
    "mail.messages.trash.request",
    "mail.send.execute",
    "mail.send.request",
    "mail.sync.page",
];

#[derive(Clone, Eq, PartialEq)]
pub struct AccountGrantV2 {
    account_id: String,
    tools: Vec<String>,
    read_only: bool,
}

impl AccountGrantV2 {
    pub fn new(
        account_id: String,
        mut tools: Vec<String>,
        read_only: bool,
    ) -> Result<Self, ErrorCode> {
        validate_account(&account_id)?;
        validate_tools(&tools)?;
        tools.sort_unstable();
        tools.dedup();
        if tools.len() > MAX_TOOLS {
            return Err(ErrorCode::InvalidInput);
        }
        if tools.iter().any(|tool| {
            principal_tool(tool)
                || crate::google_mutation::grant_account(tool)
                    .is_some_and(|owner| owner != account_id)
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id,
            tools,
            read_only,
        })
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn tools(&self) -> &[String] {
        &self.tools
    }
    pub const fn read_only(&self) -> bool {
        self.read_only
    }
}

impl fmt::Debug for AccountGrantV2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountGrantV2")
            .field("account_id", &"<redacted>")
            .field("tool_count", &self.tools.len())
            .field("read_only", &self.read_only)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AccountScopeV2 {
    principal_tools: Vec<String>,
    grants: Vec<AccountGrantV2>,
}

impl fmt::Debug for AccountScopeV2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountScopeV2")
            .field("principal_tool_count", &self.principal_tools.len())
            .field("grant_count", &self.grants.len())
            .finish()
    }
}

impl AccountScopeV2 {
    pub fn new(
        mut principal_tools: Vec<String>,
        mut grants: Vec<AccountGrantV2>,
    ) -> Result<Self, ErrorCode> {
        validate_tools(&principal_tools)?;
        principal_tools.sort_unstable();
        if principal_tools.windows(2).any(|w| w[0] == w[1]) {
            return Err(ErrorCode::InvalidInput);
        }
        if principal_tools.len() > MAX_TOOLS || grants.len() > MAX_ACCOUNTS {
            return Err(ErrorCode::InvalidInput);
        }
        if principal_tools.iter().any(|tool| !principal_tool(tool)) {
            return Err(ErrorCode::InvalidInput);
        }
        grants.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        if grants
            .windows(2)
            .any(|w| w[0].account_id == w[1].account_id)
        {
            return Err(ErrorCode::InvalidInput);
        }
        if grants
            .iter()
            .any(|g| g.tools.iter().any(|t| !known_tool(t)))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let scope = Self {
            principal_tools,
            grants,
        };
        if scope.to_json()?.len() > MAX_SCOPE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(scope)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, ErrorCode> {
        if bytes.is_empty() || bytes.len() > MAX_SCOPE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
        let canonical = serde_json::to_vec(&value).map_err(|_| ErrorCode::InvalidInput)?;
        if canonical != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        let root = value.as_object().ok_or(ErrorCode::InvalidInput)?;
        require_keys(
            root,
            &[
                "scope_model_version",
                "principal_tools",
                "toolsets",
                "account_grants",
            ],
        )?;
        if root.get("scope_model_version").and_then(Value::as_u64) != Some(2) {
            return Err(ErrorCode::InvalidInput);
        }
        let principal_tools =
            array_strings(root.get("principal_tools").ok_or(ErrorCode::InvalidInput)?)?;
        validate_tools(&principal_tools)?;
        if principal_tools
            .windows(2)
            .any(|window| window[0] >= window[1])
            || principal_tools.iter().any(|tool| !known_tool(tool))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let toolsets = root
            .get("toolsets")
            .ok_or(ErrorCode::InvalidInput)?
            .as_array()
            .ok_or(ErrorCode::InvalidInput)?;
        if toolsets.len() > MAX_ACCOUNTS {
            return Err(ErrorCode::InvalidInput);
        }
        let mut sets = Vec::with_capacity(toolsets.len());
        for set in toolsets {
            let tools = array_strings(set)?;
            validate_tools(&tools)?;
            if tools.windows(2).any(|w| w[0] >= w[1]) || tools.iter().any(|t| !known_tool(t)) {
                return Err(ErrorCode::InvalidInput);
            }
            sets.push(tools);
        }
        if sets.windows(2).any(|window| window[0] >= window[1]) {
            return Err(ErrorCode::InvalidInput);
        }
        let rows = root
            .get("account_grants")
            .ok_or(ErrorCode::InvalidInput)?
            .as_array()
            .ok_or(ErrorCode::InvalidInput)?;
        if rows.len() > MAX_ACCOUNTS {
            return Err(ErrorCode::InvalidInput);
        }
        let mut grants = Vec::with_capacity(rows.len());
        let mut used = vec![false; sets.len()];
        let mut previous_account: Option<String> = None;
        for row in rows {
            let obj = row.as_object().ok_or(ErrorCode::InvalidInput)?;
            require_keys(obj, &["account_id", "toolset", "read_only"])?;
            let account = obj
                .get("account_id")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?
                .to_owned();
            if previous_account
                .as_deref()
                .is_some_and(|previous| previous >= account.as_str())
            {
                return Err(ErrorCode::InvalidInput);
            }
            let index = obj
                .get("toolset")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or(ErrorCode::InvalidInput)?;
            let read_only = obj
                .get("read_only")
                .and_then(Value::as_bool)
                .ok_or(ErrorCode::InvalidInput)?;
            let tools = sets.get(index).ok_or(ErrorCode::InvalidInput)?.clone();
            used[index] = true;
            previous_account = Some(account.clone());
            grants.push(AccountGrantV2::new(account, tools, read_only)?);
        }
        if used.iter().any(|v| !v) {
            return Err(ErrorCode::InvalidInput);
        }
        Self::new(principal_tools, grants)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, ErrorCode> {
        let mut toolsets: Vec<Vec<String>> = Vec::new();
        for grant in &self.grants {
            if !toolsets.iter().any(|set| set == &grant.tools) {
                toolsets.push(grant.tools.clone());
            }
        }
        toolsets.sort();
        let rows = self.grants.iter().map(|grant| {
            let index = toolsets.binary_search(&grant.tools).map_err(|_| ErrorCode::InternalError)?;
            Ok(json!({"account_id": grant.account_id, "toolset": index, "read_only": grant.read_only}))
        }).collect::<Result<Vec<_>, ErrorCode>>()?;
        let value = json!({"scope_model_version": 2, "principal_tools": self.principal_tools, "toolsets": toolsets, "account_grants": rows});
        let bytes = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
        if bytes.len() > MAX_SCOPE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(bytes)
    }

    pub fn permits_account(&self, account: &str, tool: &str) -> bool {
        // Discovery remains principal-level, but may list only explicit cells.
        // This never makes an accountless read or another mail tool a cell.
        if tool == "mail.accounts.list" {
            return self.permits_principal_tool(tool)
                && self.grants.iter().any(|grant| grant.account_id == account);
        }
        let stored_tool = match tool {
            crate::google_mutation::REQUEST_TOOL | crate::google_mutation::EXECUTE_TOOL => {
                crate::google_mutation::account_grant(account).ok()
            }
            _ => Some(tool.to_owned()),
        };
        self.grants
            .iter()
            .find(|g| g.account_id == account)
            .is_some_and(|g| {
                stored_tool.is_some_and(|required| g.tools.iter().any(|t| t == &required))
                    && (!g.read_only || !mutating_tool(tool))
            })
    }
    pub fn permits_principal_tool(&self, tool: &str) -> bool {
        self.principal_tools.iter().any(|t| t == tool)
    }
    pub fn grants(&self) -> &[AccountGrantV2] {
        &self.grants
    }
    pub fn principal_tools(&self) -> &[String] {
        &self.principal_tools
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, ErrorCode> {
        let principal = self
            .principal_tools
            .iter()
            .filter(|t| other.principal_tools.binary_search(t).is_ok())
            .cloned()
            .collect();
        let mut grants = Vec::new();
        for left in &self.grants {
            if let Some(right) = other
                .grants
                .iter()
                .find(|g| g.account_id == left.account_id)
            {
                let tools = left
                    .tools
                    .iter()
                    .filter(|t| right.tools.binary_search(t).is_ok())
                    .cloned()
                    .collect();
                grants.push(AccountGrantV2::new(
                    left.account_id.clone(),
                    tools,
                    left.read_only || right.read_only,
                )?);
            }
        }
        Self::new(principal, grants)
    }

    pub fn from_legacy(scope: &RequestedScope, read_only: bool) -> Result<Self, ErrorCode> {
        let mut grants = Vec::with_capacity(scope.accounts().len());
        for account in scope.accounts() {
            let tools = scope
                .tools()
                .iter()
                .filter(|tool| {
                    !principal_tool(tool)
                        && crate::google_mutation::grant_account(tool)
                            .is_none_or(|owner| owner == *account)
                })
                .cloned()
                .collect();
            grants.push(AccountGrantV2::new(account.clone(), tools, read_only)?);
        }
        let principal = scope
            .tools()
            .iter()
            .filter(|tool| principal_tool(tool))
            .cloned()
            .collect();
        Self::new(principal, grants)
    }
}

fn validate_account(value: &str) -> Result<(), ErrorCode> {
    if value.is_empty()
        || value.len() > MAX_ACCOUNT_ID
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_tools(tools: &[String]) -> Result<(), ErrorCode> {
    if tools.len() > MAX_TOOLS
        || tools.iter().any(|t| {
            t.is_empty()
                || t.len() > MAX_TOOL_ID
                || !t
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
        })
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn principal_tool(tool: &str) -> bool {
    matches!(
        tool,
        "mail.accounts.list" | "app.settings.display.get" | "app.diagnostics.snapshot"
    )
}
fn known_tool(tool: &str) -> bool {
    ORDINARY_TOOLS.contains(&tool) || crate::google_mutation::grant_account(tool).is_some()
}
fn mutating_tool(tool: &str) -> bool {
    matches!(
        tool,
        "mail.drafts.create"
            | "mail.send.request"
            | "mail.send.execute"
            | "mail.messages.trash.request"
            | "mail.messages.trash.execute"
            | "mail.google.changes.request"
            | "mail.google.changes.execute"
    ) || crate::google_mutation::grant_account(tool).is_some()
}
fn require_keys(obj: &Map<String, Value>, expected: &[&str]) -> Result<(), ErrorCode> {
    if obj.len() != expected.len() || expected.iter().any(|key| !obj.contains_key(*key)) {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn array_strings(value: &Value) -> Result<Vec<String>, ErrorCode> {
    let values = value.as_array().ok_or(ErrorCode::InvalidInput)?;
    if values.len() > MAX_TOOLS {
        return Err(ErrorCode::InvalidInput);
    }
    values
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(ErrorCode::InvalidInput))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_account_discovery_requires_both_principal_capability_and_exact_cell() {
        let scope =
            AccountScopeV2::new(vec!["mail.accounts.list".into()], vec![full("a", true)]).unwrap();
        assert!(scope.permits_account("a", "mail.accounts.list"));
        assert!(!scope.permits_account("b", "mail.accounts.list"));
        assert!(!scope.permits_account("a", "app.diagnostics.snapshot"));
        assert!(!scope.permits_account("a", "mail.send.execute"));
        let absent = AccountScopeV2::new(Vec::new(), vec![full("a", false)]).unwrap();
        assert!(!absent.permits_account("a", "mail.accounts.list"));
    }

    fn full(account: &str, read_only: bool) -> AccountGrantV2 {
        AccountGrantV2::new(
            account.into(),
            vec![
                "mail.messages.get".into(),
                "mail.send.execute".into(),
                "mail.messages.trash.execute".into(),
            ],
            read_only,
        )
        .unwrap()
    }

    #[test]
    fn sec03_account_grants_are_isolated_and_read_only_blocks_mutation() {
        let scope = AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                AccountGrantV2::new("a".into(), vec!["mail.messages.get".into()], true).unwrap(),
                full("b", false),
            ],
        )
        .unwrap();
        assert!(scope.permits_account("a", "mail.messages.get"));
        assert!(!scope.permits_account("a", "mail.send.execute"));
        assert!(scope.permits_account("b", "mail.send.execute"));
        assert!(!scope.permits_principal_tool("mail.messages.get"));
    }

    #[test]
    fn sec03_wire_is_canonical_and_rejects_unknown_or_unused_entries() {
        let scope =
            AccountScopeV2::new(vec!["mail.accounts.list".into()], vec![full("a", false)]).unwrap();
        let bytes = scope.to_json().unwrap();
        assert_eq!(AccountScopeV2::from_json(&bytes).unwrap(), scope);
        assert!(AccountScopeV2::from_json(br#"{"scope_model_version":2,"principal_tools":["mail.accounts.list"],"toolsets":[["mail.messages.get"],["mail.send.execute"]],"account_grants":[{"account_id":"a","toolset":0,"read_only":false}]}"#).is_err());
        assert!(AccountScopeV2::from_json(br#"{"scope_model_version":2,"principal_tools":["mail.accounts.list"],"toolsets":[["mail.messages.get"]],"account_grants":[{"account_id":"a","toolset":0,"read_only":false},{"account_id":"a","toolset":0,"read_only":false}]}"#).is_err());
    }

    #[test]
    fn sec03_legacy_translation_and_intersection_never_widen() {
        let old = RequestedScope::new(
            vec!["a".into(), "b".into()],
            vec!["mail.messages.get".into()],
        )
        .unwrap();
        let scope = AccountScopeV2::from_legacy(&old, true).unwrap();
        assert!(scope.permits_account("a", "mail.messages.get"));
        assert!(!scope.permits_principal_tool("mail.messages.get"));
        let narrow = AccountScopeV2::new(
            vec![],
            vec![AccountGrantV2::new("a".into(), vec!["mail.messages.get".into()], true).unwrap()],
        )
        .unwrap();
        assert_eq!(scope.intersection(&narrow).unwrap().grants().len(), 1);
    }

    #[test]
    fn sec03_google_change_sentinel_is_account_bound_and_preserves_legacy_checks() {
        let a = format!("google_{}", "a".repeat(64));
        let b = format!("google_{}", "b".repeat(64));
        let sentinel = crate::google_mutation::account_grant(&a).unwrap();
        let scope = AccountScopeV2::new(
            vec![],
            vec![AccountGrantV2::new(a.clone(), vec![sentinel], false).unwrap()],
        )
        .unwrap();
        assert!(scope.permits_account(&a, crate::google_mutation::REQUEST_TOOL));
        assert!(scope.permits_account(&a, crate::google_mutation::EXECUTE_TOOL));
        assert!(!scope.permits_account(&b, crate::google_mutation::REQUEST_TOOL));
        assert!(
            AccountGrantV2::new(
                a.clone(),
                vec![crate::google_mutation::account_grant(&b).unwrap()],
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn sec03_read_only_preserves_legacy_event_tools() {
        let scope = AccountScopeV2::new(
            vec![],
            vec![
                AccountGrantV2::new(
                    "a".into(),
                    vec!["mail.events.ack".into(), "mail.events.configure".into()],
                    true,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        assert!(scope.permits_account("a", "mail.events.ack"));
        assert!(scope.permits_account("a", "mail.events.configure"));
    }

    #[test]
    fn sec03_legacy_global_reads_remain_principal_only_without_new_permissions() {
        let legacy = RequestedScope::new(
            vec!["a".into()],
            vec![
                "mail.accounts.list".into(),
                "mail.messages.get".into(),
                "app.settings.display.get".into(),
            ],
        )
        .unwrap();
        let scoped = AccountScopeV2::from_legacy(&legacy, true).unwrap();
        assert!(scoped.permits_principal_tool("mail.accounts.list"));
        assert!(scoped.permits_principal_tool("app.settings.display.get"));
        assert!(!scoped.permits_principal_tool("app.diagnostics.snapshot"));
        assert!(!scoped.permits_principal_tool("mail.messages.get"));
        assert!(scoped.permits_account("a", "mail.messages.get"));
        for tool in [
            "mail.accounts.list",
            "app.settings.display.get",
            "app.diagnostics.snapshot",
        ] {
            assert_eq!(
                scoped.permits_account("a", tool),
                tool == "mail.accounts.list"
            );
            assert!(!scoped.permits_account("ungranted", tool));
            assert!(AccountGrantV2::new("a".into(), vec![tool.into()], false).is_err());
        }
        assert!(AccountScopeV2::new(vec!["mail.send.execute".into()], vec![]).is_err());
    }
}
