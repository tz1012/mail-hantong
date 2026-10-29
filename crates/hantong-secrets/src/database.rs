//! Keyed storage boundary. Database paths and keys never reach clients.
use std::fmt;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseError {
    Unsupported,
    Conflict,
    Internal,
}
impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "encrypted storage is unsupported on this platform",
            Self::Conflict => "encrypted storage is already in use",
            Self::Internal => "encrypted storage operation failed",
        })
    }
}
impl std::error::Error for DatabaseError {}

pub struct KeyedConnection {
    #[cfg(windows)]
    inner: windows_database::Database,
}
impl fmt::Debug for KeyedConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyedConnection(<redacted>)")
    }
}
impl KeyedConnection {
    pub fn open(
        data_root: impl AsRef<Path>,
        key_root: impl AsRef<Path>,
    ) -> Result<Self, DatabaseError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_database::Database::open(data_root.as_ref(), key_root.as_ref())?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (data_root, key_root);
            Err(DatabaseError::Unsupported)
        }
    }
    /// For the trusted Store repository implementation only. Borrowed database
    /// objects cannot escape the callback; the owning wrapper keeps guards alive.
    #[cfg(windows)]
    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>,
    ) -> Result<T, DatabaseError> {
        operation(&self.inner.connection).map_err(|_| DatabaseError::Internal)
    }
    pub fn backup_before_migration(&self, current_version: u32) -> Result<(), DatabaseError> {
        #[cfg(windows)]
        {
            self.inner.backup(current_version)
        }
        #[cfg(not(windows))]
        {
            let _ = current_version;
            Err(DatabaseError::Unsupported)
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[path = "windows_database.rs"]
mod windows_database;

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    #[test]
    fn sec02_unsupported_platform_does_not_create_database() {
        assert!(matches!(
            KeyedConnection::open("unused-db", "unused-key"),
            Err(DatabaseError::Unsupported)
        ));
    }
}
