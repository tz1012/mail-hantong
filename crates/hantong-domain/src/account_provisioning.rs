//! Validated display input for creating a new IMAP account; no credentials or authority.
use crate::accounts::{AccountDisplayError, valid_color, valid_display, valid_id};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct NewImapAccountDraft {
    account_id: String,
    display_name: Option<String>,
    color: Option<String>,
}
impl fmt::Debug for NewImapAccountDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NewImapAccountDraft { <redacted> }")
    }
}
impl NewImapAccountDraft {
    pub fn new(
        account_id: String,
        display_name: Option<String>,
        color: Option<String>,
    ) -> Result<Self, AccountDisplayError> {
        let value = Self {
            account_id,
            display_name,
            color,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }
    pub fn validate(&self) -> Result<(), AccountDisplayError> {
        if !valid_id(&self.account_id) {
            return Err(AccountDisplayError::InvalidInput);
        }
        if let Some(name) = &self.display_name {
            valid_display(name, 128, 512)?;
        }
        valid_color(self.color.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc01_new_imap_draft_keeps_optional_display_without_inventing_identity() {
        let empty = NewImapAccountDraft::new("account-1".into(), None, None).unwrap();
        assert_eq!(empty.account_id(), "account-1");
        assert_eq!(empty.display_name(), None);
        assert_eq!(empty.color(), None);
        let named = NewImapAccountDraft::new(
            "A_b-123".into(),
            Some("개인 메일".into()),
            Some("#aB09ef".into()),
        )
        .unwrap();
        assert_eq!(named.display_name(), Some("개인 메일"));
        assert_eq!(named.color(), Some("#aB09ef"));
        assert!(named.validate().is_ok());
    }
    #[test]
    fn acc03_new_imap_id_rejects_paths_unicode_empty_and_overflow() {
        for id in ["", "../account", "a b", "한통", &"a".repeat(129)] {
            assert_eq!(
                NewImapAccountDraft::new(id.into(), None, None),
                Err(AccountDisplayError::InvalidInput)
            );
        }
        assert!(NewImapAccountDraft::new("a".repeat(128), None, None).is_ok());
    }
    #[test]
    fn acc01_display_and_color_match_account_summary_bounds() {
        for name in [
            "".to_owned(),
            " \t".into(),
            "line\nnext".into(),
            "escape\u{1b}".into(),
            "가".repeat(129),
        ] {
            assert_eq!(
                NewImapAccountDraft::new("account".into(), Some(name), None),
                Err(AccountDisplayError::InvalidInput)
            );
        }
        assert!(
            NewImapAccountDraft::new(
                "account".into(),
                Some("😀".repeat(128)),
                Some("#000000".into())
            )
            .is_ok()
        );
        for color in ["", "000000", "#12345", "#1234567", "#gggggg", "#한통"] {
            assert_eq!(
                NewImapAccountDraft::new("account".into(), None, Some(color.into())),
                Err(AccountDisplayError::InvalidInput)
            );
        }
    }
    #[test]
    fn sec08_new_imap_draft_debug_hides_ids_and_display_values() {
        let draft = NewImapAccountDraft::new(
            "private_account".into(),
            Some("Private Name".into()),
            Some("#123abc".into()),
        )
        .unwrap();
        let output = format!("{draft:?}");
        for value in ["private_account", "Private Name", "#123abc"] {
            assert!(!output.contains(value));
        }
    }
}
