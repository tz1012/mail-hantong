//! Pure IMAP mailbox role resolution. No network, Store, or Secrets access.

use hantong_domain::mailboxes::{ImapMailboxDescriptor, ImapMailboxSnapshot, SpecialUseRole};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapFolderRole {
    Inbox,
    Special(SpecialUseRole),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImapFolderResolution {
    Missing,
    Unavailable,
    Ambiguous,
    Unique(ImapMailboxDescriptor),
}

pub fn resolve_imap_role(
    snapshot: &ImapMailboxSnapshot,
    role: ImapFolderRole,
) -> ImapFolderResolution {
    let candidates = snapshot
        .mailboxes()
        .iter()
        .filter(|mailbox| match role {
            ImapFolderRole::Inbox => mailbox.name().is_inbox(),
            ImapFolderRole::Special(expected) => mailbox.has_role(expected),
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => ImapFolderResolution::Missing,
        [mailbox] if mailbox.selectable() => ImapFolderResolution::Unique((*mailbox).clone()),
        [_] => ImapFolderResolution::Unavailable,
        _ => ImapFolderResolution::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_domain::{capabilities::ImapCapabilities, mailboxes::*};

    fn descriptor(
        name: &[u8],
        roles: Vec<SpecialUseRole>,
        no_select: bool,
    ) -> ImapMailboxDescriptor {
        ImapMailboxDescriptor::new(
            ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
            None,
            no_select,
            false,
            MailboxChildren::Unknown,
            roles,
        )
        .unwrap()
    }
    fn snapshot(mailboxes: Vec<ImapMailboxDescriptor>) -> ImapMailboxSnapshot {
        ImapMailboxSnapshot::new(
            "acct",
            1,
            ImapCapabilities::from_features(std::iter::empty()),
            mailboxes,
        )
        .unwrap()
    }

    #[test]
    fn ext06_role_resolver_returns_unique_missing_and_unavailable() {
        let snapshot = snapshot(vec![
            descriptor(b"INBOX", vec![], false),
            descriptor(b"Archive", vec![SpecialUseRole::Archive], true),
        ]);
        assert!(matches!(
            resolve_imap_role(&snapshot, ImapFolderRole::Inbox),
            ImapFolderResolution::Unique(_)
        ));
        assert_eq!(
            resolve_imap_role(&snapshot, ImapFolderRole::Special(SpecialUseRole::Trash)),
            ImapFolderResolution::Missing
        );
        assert_eq!(
            resolve_imap_role(&snapshot, ImapFolderRole::Special(SpecialUseRole::Archive)),
            ImapFolderResolution::Unavailable
        );
    }

    #[test]
    fn sec08_role_resolver_fails_closed_on_ambiguous_explicit_role() {
        let snapshot = snapshot(vec![
            descriptor(b"A", vec![SpecialUseRole::Sent], false),
            descriptor(b"B", vec![SpecialUseRole::Sent], false),
        ]);
        assert_eq!(
            resolve_imap_role(&snapshot, ImapFolderRole::Special(SpecialUseRole::Sent)),
            ImapFolderResolution::Ambiguous
        );
    }
}
