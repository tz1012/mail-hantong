//! Validated client identity metadata shared by registration and audit paths.

use crate::ErrorCode;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientKind {
    Cli,
    Mcp,
    Gui,
}
impl ClientKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Mcp => "mcp",
            Self::Gui => "gui",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "cli" => Ok(Self::Cli),
            "mcp" => Ok(Self::Mcp),
            "gui" => Ok(Self::Gui),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

pub fn validate_client_label(label: &str) -> Result<(), ErrorCode> {
    if label.is_empty()
        || label.len() > 512
        || label.chars().count() > 128
        || label.chars().any(char::is_control)
        || label.trim().is_empty()
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuditActor {
    kind: ClientKind,
    display_label: String,
}
impl AuditActor {
    pub fn new(kind: ClientKind, display_label: String) -> Result<Self, ErrorCode> {
        validate_client_label(&display_label)?;
        Ok(Self {
            kind,
            display_label,
        })
    }
    pub fn kind(&self) -> ClientKind {
        self.kind
    }
    pub fn display_label(&self) -> &str {
        &self.display_label
    }
}
impl fmt::Debug for AuditActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuditActor { <redacted> }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_kind_roundtrip_and_label_bounds() {
        for kind in [ClientKind::Cli, ClientKind::Mcp, ClientKind::Gui] {
            assert_eq!(ClientKind::parse(kind.as_str()), Ok(kind));
        }
        assert_eq!(ClientKind::parse("invalid"), Err(ErrorCode::InvalidInput));
        assert!(validate_client_label("desktop").is_ok());
        assert!(validate_client_label(&"😀".repeat(128)).is_ok());
        assert!(validate_client_label("").is_err());
        assert!(validate_client_label("   ").is_err());
        assert!(validate_client_label(&"a".repeat(513)).is_err());
        assert!(validate_client_label(&"가".repeat(129)).is_err());
        assert!(validate_client_label(" \t\n").is_err());
        assert!(validate_client_label("bad\nlabel").is_err());
    }
    #[test]
    fn actor_label_is_private_and_redacted() {
        assert!(AuditActor::new(ClientKind::Gui, "bad\nlabel".into()).is_err());
        let actor = AuditActor::new(ClientKind::Gui, "secret-client".into()).unwrap();
        assert_eq!(actor.kind(), ClientKind::Gui);
        assert_eq!(actor.display_label(), "secret-client");
        assert!(!format!("{actor:?}").contains("secret-client"));
    }
}
