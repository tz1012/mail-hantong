//! Bounded server feature snapshots. These values do not grant permissions.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapFeature {
    Condstore,
    Idle,
    Move,
    Qresync,
    SpecialUse,
    Uidplus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapCapabilities {
    features: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidImapCapabilities;

const FEATURE_IDS: [(ImapFeature, &str); 6] = [
    (ImapFeature::Condstore, "imap.condstore"),
    (ImapFeature::Idle, "imap.idle"),
    (ImapFeature::Move, "imap.move"),
    (ImapFeature::Qresync, "imap.qresync"),
    (ImapFeature::SpecialUse, "imap.special_use"),
    (ImapFeature::Uidplus, "imap.uidplus"),
];

impl ImapCapabilities {
    /// Collects advertised typed features, treating duplicates as one member.
    pub fn from_features(features: impl IntoIterator<Item = ImapFeature>) -> Self {
        Self {
            features: features
                .into_iter()
                .fold(0, |bits, feature| bits | (1 << feature as u8)),
        }
    }

    /// Reads a canonical stored snapshot without accepting permission names.
    pub fn from_ids(ids: &[String]) -> Result<Self, InvalidImapCapabilities> {
        if ids.len() > FEATURE_IDS.len() || ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(InvalidImapCapabilities);
        }
        let mut features = 0;
        for id in ids {
            let (feature, _) = FEATURE_IDS
                .iter()
                .find(|(_, known)| id == known)
                .ok_or(InvalidImapCapabilities)?;
            features |= 1 << *feature as u8;
        }
        Ok(Self { features })
    }

    /// Returns sorted stable IDs; an empty snapshot grants no implicit feature.
    pub fn to_ids(&self) -> Vec<String> {
        FEATURE_IDS
            .iter()
            .filter(|(feature, _)| self.has(*feature))
            .map(|(_, id)| (*id).to_owned())
            .collect()
    }

    fn has(&self, feature: ImapFeature) -> bool {
        self.features & (1 << feature as u8) != 0
    }

    pub fn idle(&self) -> bool {
        self.has(ImapFeature::Idle)
    }
    pub fn move_messages(&self) -> bool {
        self.has(ImapFeature::Move)
    }
    pub fn uidplus(&self) -> bool {
        self.has(ImapFeature::Uidplus)
    }
    pub fn condstore(&self) -> bool {
        self.has(ImapFeature::Condstore)
    }
    pub fn qresync(&self) -> bool {
        self.has(ImapFeature::Qresync)
    }
    pub fn special_use(&self) -> bool {
        self.has(ImapFeature::SpecialUse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURES: [ImapFeature; 6] = [
        ImapFeature::Condstore,
        ImapFeature::Idle,
        ImapFeature::Move,
        ImapFeature::Qresync,
        ImapFeature::SpecialUse,
        ImapFeature::Uidplus,
    ];
    const IDS: [&str; 6] = [
        "imap.condstore",
        "imap.idle",
        "imap.move",
        "imap.qresync",
        "imap.special_use",
        "imap.uidplus",
    ];

    #[test]
    fn ext05_all_64_feature_combinations_roundtrip_with_exact_ids_and_getters() {
        for mask in 0..64 {
            let enabled: Vec<_> = (0..6).map(|index| mask & (1 << index) != 0).collect();
            let caps = ImapCapabilities::from_features(
                FEATURES
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, feature)| enabled[index].then_some(feature)),
            );
            let expected: Vec<String> = IDS
                .into_iter()
                .enumerate()
                .filter(|(index, _)| enabled[*index])
                .map(|(_, id)| id.to_owned())
                .collect();
            assert_eq!(caps.to_ids(), expected, "combination {mask}");
            assert_eq!(ImapCapabilities::from_ids(&expected), Ok(caps));
            assert_eq!(
                [
                    caps.condstore(),
                    caps.idle(),
                    caps.move_messages(),
                    caps.qresync(),
                    caps.special_use(),
                    caps.uidplus()
                ]
                .as_slice(),
                enabled.as_slice(),
                "combination {mask}",
            );
        }
    }

    #[test]
    fn ext05_typed_feature_duplicates_are_set_membership_in_canonical_order() {
        let caps = ImapCapabilities::from_features([
            ImapFeature::Uidplus,
            ImapFeature::Move,
            ImapFeature::Uidplus,
            ImapFeature::Idle,
        ]);
        assert_eq!(caps.to_ids(), ["imap.idle", "imap.move", "imap.uidplus"]);
        assert_eq!(
            caps,
            ImapCapabilities::from_features([
                ImapFeature::Idle,
                ImapFeature::Move,
                ImapFeature::Uidplus,
            ])
        );
    }

    #[test]
    fn ext05_stored_ids_require_strict_known_canonical_order() {
        for ids in [
            vec!["imap.idle", "imap.idle"],
            vec!["imap.move", "imap.idle"],
            vec!["imap.unknown"],
            vec!["IMAP.IDLE"],
            vec![" imap.idle"],
            vec!["imap.idle "],
            vec!["imap.special-use"],
            vec!["IDLE"],
            vec![""],
            vec!["imap.idle\0"],
            vec!["imap.기능"],
        ] {
            let input: Vec<String> = ids.into_iter().map(str::to_owned).collect();
            assert_eq!(
                ImapCapabilities::from_ids(&input),
                Err(InvalidImapCapabilities)
            );
        }
        let mut excessive: Vec<String> = IDS.into_iter().map(str::to_owned).collect();
        excessive.push("imap.uidplus".into());
        assert_eq!(
            ImapCapabilities::from_ids(&excessive),
            Err(InvalidImapCapabilities)
        );
    }

    #[test]
    fn ext05_permissions_are_never_server_features() {
        for permission in [
            "mail.read",
            "mail.send",
            "mail.accounts.list",
            "mail.*",
            "*",
        ] {
            assert_eq!(
                ImapCapabilities::from_ids(&[permission.into()]),
                Err(InvalidImapCapabilities)
            );
        }
        let empty = ImapCapabilities::from_ids(&[]).unwrap();
        assert_eq!(empty.to_ids(), Vec::<String>::new());
        assert!(!empty.idle() && !empty.move_messages() && !empty.uidplus());
    }
}
