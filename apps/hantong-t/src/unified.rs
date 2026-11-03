//! Bounded, read-only unified mail projection for the communications TUI.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const MAX_ACCOUNTS: usize = 128;
const MAX_ITEMS_PER_SOURCE: usize = 32;
const MAX_ITEMS: usize = 4096;
const INBOX_WIRE_HEX: &str = "494e424f58";
const INBOX_ENCODING: &str = "modified_utf7";

#[derive(Clone)]
pub struct Source {
    pub account: Value,
    pub folder: Value,
    pub page: Value,
}

#[derive(Clone)]
pub struct Unified {
    pub sources: Vec<Source>,
    pub page: Value,
    pub failures: Vec<(String, String)>,
}

fn account_id(account: &Value) -> Option<&str> {
    account.get("account_id").and_then(Value::as_str)
}

fn account_key(account: &Value) -> String {
    account
        .get("primary_address")
        .and_then(Value::as_str)
        .or_else(|| account.get("address").and_then(Value::as_str))
        .or_else(|| account.get("display_name").and_then(Value::as_str))
        .unwrap_or("(unknown account)")
        .to_owned()
}

fn provider(account: &Value) -> Option<&str> {
    account.get("provider").and_then(Value::as_str)
}

fn bounded_alias(account: &Value, aliases: &BTreeMap<String, String>) -> String {
    let key = account_id(account)
        .or_else(|| account.get("primary_address").and_then(Value::as_str))
        .or_else(|| account.get("address").and_then(Value::as_str));
    let fallback = account
        .get("primary_address")
        .or_else(|| account.get("address"))
        .or_else(|| account.get("display_name"));
    let value = key
        .and_then(|key| aliases.get(key))
        .map(String::as_str)
        .or_else(|| fallback.and_then(Value::as_str))
        .unwrap_or("account");
    super::sanitize_single_line(value)
        .chars()
        .take(128)
        .collect()
}

fn item_rows(value: &Value) -> Vec<Value> {
    value
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| value.get("messages").and_then(Value::as_array))
        .or_else(|| value.get("labels").and_then(Value::as_array))
        .map(|items| items.iter().take(MAX_ITEMS_PER_SOURCE).cloned().collect())
        .unwrap_or_default()
}

fn numeric_date(item: &Value) -> i128 {
    ["internal_date_ms", "internal_date_utc_ms"]
        .iter()
        .find_map(|key| item.get(*key))
        .and_then(|value| match value {
            Value::Number(number) => number.as_i64().map(i128::from),
            Value::String(text) => text.parse::<i128>().ok(),
            _ => None,
        })
        .unwrap_or(i128::MIN)
}

fn validate_items(items: &[Value], id: &str) -> Result<(), String> {
    if items.iter().any(|item| {
        item.get("account_id")
            .and_then(Value::as_str)
            .is_some_and(|item_id| item_id != id)
    }) {
        return Err("mail response account identity did not match the request".to_owned());
    }
    Ok(())
}

fn validate_response_account(value: &Value, id: &str) -> Result<(), String> {
    if value
        .get("account_id")
        .and_then(Value::as_str)
        .is_some_and(|response_id| response_id != id)
    {
        return Err("mail response account identity did not match the request".to_owned());
    }
    Ok(())
}

fn inbox_folder(value: &Value) -> Option<Value> {
    value
        .get("items")
        .and_then(Value::as_array)
        .and_then(|folders| {
            folders.iter().find(|folder| {
                folder.get("wire_hex").and_then(Value::as_str) == Some(INBOX_WIRE_HEX)
                    && folder
                        .get("encoding")
                        .and_then(Value::as_str)
                        .is_some_and(|encoding| encoding == INBOX_ENCODING || encoding == "utf8")
                    && folder.get("selectable").and_then(Value::as_bool) != Some(false)
                    && folder.get("no_select").and_then(Value::as_bool) != Some(true)
                    && folder.get("non_existent").and_then(Value::as_bool) != Some(true)
            })
        })
        .cloned()
}

fn demo_source(account: Value, folder: Value, alias: &str, source_index: usize) -> Source {
    let items: Vec<Value> = (0..8)
        .map(|index| {
            json!({
                "account_id": account.get("account_id").cloned().unwrap_or(Value::Null),
                "internal_message_id": format!("demo-{source_index}-{index}"),
                "internal_date_utc_ms": 1_700_000_000_000_i64 - index as i64,
                "subject": format!("[{alias}] terminal letter {:02}", index + 1),
                "sender": "friend@example.invalid"
            })
        })
        .collect();
    Source {
        account,
        folder,
        page: json!({"items": items}),
    }
}

fn sort_value(
    source_index: usize,
    item_index: usize,
    item: &Value,
) -> (i128, String, String, usize, usize) {
    let account = item
        .get("account_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let id = item
        .get("internal_message_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    (numeric_date(item), account, id, source_index, item_index)
}

pub fn load(
    accounts: &Value,
    aliases: &BTreeMap<String, String>,
    demo: bool,
    call: &mut impl FnMut(&[&str]) -> Result<Value, String>,
) -> Unified {
    let account_rows: Vec<Value> = accounts
        .get("items")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().take(MAX_ACCOUNTS).cloned().collect())
        .unwrap_or_default();
    if demo {
        let demo_accounts = vec![
            json!({"account_id":"demo-imap","primary_address":"reader@example.invalid","provider":"imap"}),
            json!({"account_id":"demo-gmail","primary_address":"work@example.invalid","provider":"gmail"}),
        ];
        let sources = demo_accounts
            .into_iter()
            .enumerate()
            .map(|(index, account)| {
                let alias = bounded_alias(&account, aliases);
                demo_source(
                    account,
                    json!({"name":"Inbox","wire_hex":INBOX_WIRE_HEX,"encoding":INBOX_ENCODING,"selectable":true}),
                    &alias,
                    index,
                )
            })
            .collect();
        return aggregate(sources, aliases);
    }

    let mut sources = Vec::new();
    let mut failures = Vec::new();
    for account in account_rows {
        if account
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "ready")
        {
            failures.push((
                bounded_alias(&account, aliases),
                "account is not ready".to_owned(),
            ));
            continue;
        }
        let Some(id) = account_id(&account).filter(|id| !id.is_empty()) else {
            failures.push((
                account_key(&account),
                "account has no stable account_id".to_owned(),
            ));
            continue;
        };
        let result = match provider(&account) {
            Some("gmail") | Some("google") => {
                let folder = json!({"id":"INBOX","name":"Inbox"});
                call(&[
                    "gmail",
                    "list",
                    "--account",
                    id,
                    "--label-id",
                    "INBOX",
                    "--limit",
                    "32",
                ])
                .map(|page| (folder, super::google_items(page)))
                .and_then(|(folder, page)| {
                    validate_response_account(&page, id)?;
                    Ok((folder, page))
                })
            }
            Some("imap") | Some("smtp") | Some("generic_imap") => {
                call(&["folders", "list", "--account", id]).and_then(|folders| {
                    validate_response_account(&folders, id)?;
                    let folder = inbox_folder(&folders)
                        .ok_or_else(|| "no selectable INBOX mailbox was advertised".to_owned())?;
                    let wire = folder
                        .get("wire_hex")
                        .and_then(Value::as_str)
                        .unwrap_or(INBOX_WIRE_HEX);
                    let encoding = folder
                        .get("encoding")
                        .and_then(Value::as_str)
                        .unwrap_or(INBOX_ENCODING);
                    call(&[
                        "search",
                        "--account",
                        id,
                        "--wire-hex",
                        wire,
                        "--encoding",
                        encoding,
                        "--limit",
                        "32",
                    ])
                    .map(|page| (folder, page))
                    .and_then(|(folder, page)| {
                        validate_response_account(&page, id)?;
                        Ok((folder, page))
                    })
                })
            }
            Some(other) => Err(format!("unsupported account provider: {other}")),
            None => Err("account provider is missing".to_owned()),
        };
        match result {
            Ok((folder, mut page)) => {
                let items = item_rows(&page);
                if let Err(error) = validate_items(&items, id) {
                    failures.push((bounded_alias(&account, aliases), error));
                    continue;
                }
                page["items"] = Value::Array(items);
                sources.push(Source {
                    account: account.clone(),
                    folder,
                    page,
                });
            }
            Err(error) => failures.push((
                bounded_alias(&account, aliases),
                super::sanitize_single_line(&error),
            )),
        }
    }
    let mut unified = aggregate(sources, aliases);
    unified.failures.extend(failures);
    unified
}

fn aggregate(sources: Vec<Source>, aliases: &BTreeMap<String, String>) -> Unified {
    let mut entries: Vec<(i128, String, String, usize, usize, Value)> = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let alias = bounded_alias(&source.account, aliases);
        for (item_index, item) in item_rows(&source.page).iter().enumerate() {
            let mut wrapper = match item {
                Value::Object(map) => map.clone(),
                _ => Map::new(),
            };
            let label = super::projected_label(&source.page, item);
            wrapper.insert("account_tag".to_owned(), Value::String(alias.clone()));
            wrapper.insert("source_index".to_owned(), json!(source_index));
            wrapper.insert("item_index".to_owned(), json!(item_index));
            wrapper.insert(
                "display_label".to_owned(),
                Value::String(format!("[{alias}] {label}")),
            );
            wrapper.insert(
                "subject".to_owned(),
                Value::String(format!("[{alias}] {label}")),
            );
            let key = sort_value(source_index, item_index, item);
            entries.push((key.0, key.1, key.2, key.3, key.4, Value::Object(wrapper)));
        }
    }
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.cmp(&b.3))
            .then_with(|| a.4.cmp(&b.4))
    });
    let items: Vec<Value> = entries
        .into_iter()
        .take(MAX_ITEMS)
        .map(|entry| entry.5)
        .collect();
    Unified {
        page: json!({"items": items, "sources": sources.len()}),
        sources,
        failures: Vec::new(),
    }
}

impl Unified {
    /// Rebuild only the presentation tags from cached source pages.
    ///
    /// Alias edits are local display changes; this method performs no CLI or
    /// provider I/O and retains the source failures collected during loading.
    pub fn retag(&mut self, aliases: &BTreeMap<String, String>) {
        let sources = self.sources.clone();
        let failures = std::mem::take(&mut self.failures);
        let mut updated = aggregate(sources, aliases);
        updated.failures = failures;
        *self = updated;
    }

    pub fn selected(&self, index: usize) -> Result<(&Value, &Value, &Value, &Value), String> {
        let wrapper = self
            .page
            .get("items")
            .and_then(Value::as_array)
            .and_then(|items| items.get(index))
            .ok_or_else(|| "selected message is unavailable".to_owned())?;
        let source_index = wrapper
            .get("source_index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "selected message source is invalid".to_owned())?;
        let item_index = wrapper
            .get("item_index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "selected message identity is invalid".to_owned())?;
        let source = self
            .sources
            .get(source_index)
            .ok_or_else(|| "selected message source is unavailable".to_owned())?;
        let page_items = source
            .page
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| "source page is invalid".to_owned())?;
        let item = page_items
            .get(item_index)
            .ok_or_else(|| "selected message is no longer available".to_owned())?;
        Ok((&source.account, &source.folder, &source.page, item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts() -> Value {
        json!({"items":[
            {"account_id":"a","primary_address":"a@example.invalid","provider":"gmail"},
            {"account_id":"b","primary_address":"b@example.invalid","provider":"gmail"}
        ]})
    }

    #[test]
    fn unified_empty_accounts_has_no_effects() {
        let mut calls = 0;
        let result = load(&json!({"items": []}), &BTreeMap::new(), false, &mut |_| {
            calls += 1;
            Err("must not call".to_owned())
        });
        assert!(result.sources.is_empty());
        assert!(result.page["items"].as_array().unwrap().is_empty());
        assert_eq!(calls, 0);
    }

    #[test]
    fn unified_preserves_account_binding_and_aliases() {
        let mut aliases = BTreeMap::new();
        aliases.insert("a".to_owned(), "Personal".to_owned());
        let result = load(&accounts(), &aliases, false, &mut |args| {
            let id = args
                .iter()
                .position(|arg| *arg == "--account")
                .map(|i| args[i + 1])
                .unwrap();
            Ok(
                json!({"messages":[{"account_id":id,"id":id,"internal_date_ms":1,"subject":"same"}]}),
            )
        });
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.page["items"][0]["account_tag"], "Personal");
        let selected = result.selected(0).unwrap();
        assert_eq!(selected.0["account_id"], "a");
    }

    #[test]
    fn unified_sorts_newest_and_keeps_other_account_on_failure() {
        let result = load(&accounts(), &BTreeMap::new(), false, &mut |args| {
            let id = args
                .iter()
                .position(|arg| *arg == "--account")
                .map(|i| args[i + 1])
                .unwrap();
            if id == "a" {
                Err("offline".to_owned())
            } else {
                Ok(
                    json!({"messages":[{"account_id":"b","internal_date_utc_ms":"20","id":"new"},{"account_id":"b","internal_date_utc_ms":10,"id":"old"}]}),
                )
            }
        });
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.page["items"][0]["id"], "new");
    }

    #[test]
    fn unified_rejects_control_alias() {
        let mut aliases = BTreeMap::new();
        aliases.insert("a".to_owned(), "bad\u{1b}[31m".to_owned());
        let result = load(&accounts(), &aliases, false, &mut |args| {
            let id = args
                .iter()
                .position(|i| *i == "--account")
                .map(|i| args[i + 1])
                .unwrap();
            Ok(json!({"messages":[{"account_id":id,"id":id}]}))
        });
        assert!(
            !result.page["items"][0]["display_label"]
                .as_str()
                .unwrap()
                .contains('\u{1b}')
        );
    }

    #[test]
    fn unified_accepts_utf8_inbox_catalog() {
        let accounts = json!({"items":[
            {"account_id":"imap","primary_address":"imap@example.invalid","provider":"imap"}
        ]});
        let result = load(&accounts, &BTreeMap::new(), false, &mut |args| {
            if args.first() == Some(&"folders") {
                Ok(json!({"items":[
                    {"name":"Inbox","wire_hex":INBOX_WIRE_HEX,"encoding":"utf8",
                     "selectable":true,"no_select":false,"non_existent":false}
                ]}))
            } else {
                Ok(json!({"items":[{"account_id":"imap","id":"m1"}]}))
            }
        });
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].folder["encoding"], "utf8");
    }

    #[test]
    fn unified_rejects_top_level_account_mismatch() {
        let accounts = json!({"items":[
            {"account_id":"a","primary_address":"a@example.invalid","provider":"gmail"}
        ]});
        let result = load(&accounts, &BTreeMap::new(), false, &mut |_| {
            Ok(json!({"account_id":"other","messages":[]}))
        });
        assert!(result.sources.is_empty());
        assert_eq!(result.failures.len(), 1);
    }
}
