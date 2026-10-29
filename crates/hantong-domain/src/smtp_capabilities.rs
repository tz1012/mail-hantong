//! Bounded SMTP server capability snapshots.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpFeature {
    EightBitMime,
    AuthLogin,
    AuthPlain,
    AuthXoauth2,
    SmtpUtf8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpAuthMechanism {
    Plain,
    Login,
    Xoauth2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSmtpCapabilities;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmtpCapabilities {
    features: u8,
}

const FEATURES: [(SmtpFeature, &str); 5] = [
    (SmtpFeature::EightBitMime, "smtp.8bitmime"),
    (SmtpFeature::AuthLogin, "smtp.auth.login"),
    (SmtpFeature::AuthPlain, "smtp.auth.plain"),
    (SmtpFeature::AuthXoauth2, "smtp.auth.xoauth2"),
    (SmtpFeature::SmtpUtf8, "smtp.smtputf8"),
];

impl SmtpCapabilities {
    pub fn from_features(features: impl IntoIterator<Item = SmtpFeature>) -> Self {
        Self {
            features: features
                .into_iter()
                .fold(0, |bits, feature| bits | (1 << feature as u8)),
        }
    }

    pub fn from_ids(ids: &[String]) -> Result<Self, InvalidSmtpCapabilities> {
        if ids.len() > FEATURES.len() || ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(InvalidSmtpCapabilities);
        }
        let mut features = 0;
        for id in ids {
            let (feature, _) = FEATURES
                .iter()
                .find(|(_, known)| id == known)
                .ok_or(InvalidSmtpCapabilities)?;
            features |= 1 << *feature as u8;
        }
        Ok(Self { features })
    }

    pub fn to_ids(&self) -> Vec<String> {
        FEATURES
            .iter()
            .filter(|(feature, _)| self.has(*feature))
            .map(|(_, id)| (*id).to_owned())
            .collect()
    }

    pub fn eight_bit_mime(&self) -> bool {
        self.has(SmtpFeature::EightBitMime)
    }
    pub fn auth_login(&self) -> bool {
        self.has(SmtpFeature::AuthLogin)
    }
    pub fn auth_plain(&self) -> bool {
        self.has(SmtpFeature::AuthPlain)
    }
    pub fn auth_xoauth2(&self) -> bool {
        self.has(SmtpFeature::AuthXoauth2)
    }
    pub fn smtp_utf8(&self) -> bool {
        self.has(SmtpFeature::SmtpUtf8)
    }

    fn has(&self, feature: SmtpFeature) -> bool {
        self.features & (1 << feature as u8) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEATURES_ALL: [SmtpFeature; 5] = [
        SmtpFeature::EightBitMime,
        SmtpFeature::AuthLogin,
        SmtpFeature::AuthPlain,
        SmtpFeature::AuthXoauth2,
        SmtpFeature::SmtpUtf8,
    ];

    #[test]
    fn ext05_smtp_all_32_feature_combinations_roundtrip_in_canonical_order() {
        for mask in 0..32 {
            let value = SmtpCapabilities::from_features(
                FEATURES_ALL
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, feature)| (mask & (1 << index) != 0).then_some(feature)),
            );
            let ids = value.to_ids();
            assert_eq!(SmtpCapabilities::from_ids(&ids), Ok(value));
            assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn ext05_smtp_rejects_noncanonical_unknown_duplicate_and_permission_ids() {
        for ids in [
            vec!["smtp.auth.plain".into(), "smtp.auth.plain".into()],
            vec!["smtp.smtputf8".into(), "smtp.8bitmime".into()],
            vec!["smtp.unknown".into()],
            vec!["SMTPUTF8".into()],
            vec!["mail.send".into()],
            vec!["smtp.auth.plain".into(), "mail.read".into()],
        ] {
            assert_eq!(
                SmtpCapabilities::from_ids(&ids),
                Err(InvalidSmtpCapabilities)
            );
        }
    }
}
