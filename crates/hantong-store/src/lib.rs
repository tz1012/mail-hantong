#![forbid(unsafe_code)]

//! Typed access to the encrypted local database.
//!
//! The database connection remains owned by `hantong-secrets`.  This crate
//! exposes only fixed repository operations; callers never receive a SQLite
//! connection or an arbitrary SQL entry point.

use hantong_contracts::ErrorCode;
use std::fmt;

pub mod account_provisioning;
mod accounts;
mod audit;
mod config;
pub mod diagnostic_aggregate;
pub mod folder_refresh;
pub mod imap_catalog;
#[cfg(all(test, windows))]
mod imap_catalog_security_tests;
pub mod imap_setup;
pub mod imap_sync;
pub mod initial_sync;
pub mod mail_connection;
pub mod mail_send;
pub mod message_body;
#[cfg(windows)]
mod message_identities;
pub mod message_search;
mod registration_activation;
mod registrations;
pub mod scope_refresh;
#[cfg(windows)]
mod uid_epochs;
pub mod uid_inspection;
pub mod uid_page_jobs;
pub mod uid_reconcile_jobs;
pub mod uid_recovery_jobs;
pub use registration_activation::{
    CliRegistrationStage, DEFAULT_CLI_LABEL, DEFAULT_GUI_LABEL, DEFAULT_MCP_LABEL,
    GuiRegistrationStage, McpMailRegistrationStage, McpRegistrationStage,
};
mod server_identity;
pub use registrations::{
    ClientKind, RegistrationRecord, RegistrationRequest, RegistrationRequestState,
    RegistrationState,
};
pub use server_identity::ServerIdentity;

#[cfg(windows)]
const CURRENT_SCHEMA_VERSION: u32 = 18;
#[cfg(windows)]
const DATABASE_FORMAT: &str = "mail-hantong";
#[cfg(windows)]
const MAX_SETTING_KEY_BYTES: usize = 128;
#[cfg(windows)]
const MAX_SETTING_JSON_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const SCHEMA: &str = include_str!("../../../docs/contracts/db-schema.sql");
#[cfg(windows)]
const SCHEMA_V4: &str = include_str!("../../../docs/contracts/migrations/schema-v4.sql");
#[cfg(windows)]
const SCHEMA_V5: &str = include_str!("../../../docs/contracts/migrations/schema-v5.sql");
#[cfg(windows)]
const SCHEMA_V6: &str = include_str!("../../../docs/contracts/migrations/schema-v6.sql");
#[cfg(windows)]
const SCHEMA_V7: &str = include_str!("../../../docs/contracts/migrations/schema-v7.sql");
#[cfg(windows)]
const SCHEMA_V8: &str = include_str!("../../../docs/contracts/migrations/schema-v8.sql");
#[cfg(windows)]
const SCHEMA_V10: &str = include_str!("../../../docs/contracts/migrations/schema-v10.sql");
#[cfg(windows)]
const SCHEMA_V11: &str = include_str!("../../../docs/contracts/migrations/schema-v11.sql");
#[cfg(windows)]
const SCHEMA_V12: &str = include_str!("../../../docs/contracts/migrations/schema-v12.sql");
#[cfg(windows)]
const SCHEMA_V13: &str = include_str!("../../../docs/contracts/migrations/schema-v13.sql");
#[cfg(windows)]
const SCHEMA_V14: &str = include_str!("../../../docs/contracts/migrations/schema-v14.sql");
#[cfg(windows)]
const SCHEMA_V16: &str = include_str!("../../../docs/contracts/migrations/schema-v16.sql");
#[cfg(windows)]
const SCHEMA_V17: &str = include_str!("../../../docs/contracts/migrations/schema-v17.sql");
#[cfg(windows)]
const MIGRATION_V17_TO_V18: &str =
    include_str!("../../../docs/contracts/migrations/017-to-018.sql");
#[cfg(windows)]
const MIGRATION_V16_TO_V17: &str =
    include_str!("../../../docs/contracts/migrations/016-to-017.sql");
#[cfg(windows)]
const SCHEMA_V15: &str = include_str!("../../../docs/contracts/migrations/schema-v15.sql");
#[cfg(windows)]
const MIGRATION_V15_TO_V16: &str =
    include_str!("../../../docs/contracts/migrations/015-to-016.sql");
#[cfg(windows)]
const MIGRATION_V14_TO_V15: &str =
    include_str!("../../../docs/contracts/migrations/014-to-015.sql");
#[cfg(windows)]
const MIGRATION_V13_TO_V14: &str =
    include_str!("../../../docs/contracts/migrations/013-to-014.sql");
#[cfg(windows)]
const MIGRATION_V12_TO_V13: &str =
    include_str!("../../../docs/contracts/migrations/012-to-013.sql");
#[cfg(windows)]
const MIGRATION_V11_TO_V12: &str =
    include_str!("../../../docs/contracts/migrations/011-to-012.sql");
#[cfg(windows)]
const MIGRATION_V10_TO_V11: &str =
    include_str!("../../../docs/contracts/migrations/010-to-011.sql");
#[cfg(windows)]
const SCHEMA_V9: &str = include_str!("../../../docs/contracts/migrations/schema-v9.sql");
#[cfg(windows)]
const MIGRATION_V8_TO_V9: &str = include_str!("../../../docs/contracts/migrations/008-to-009.sql");
#[cfg(windows)]
const MIGRATION_V9_TO_V10: &str = include_str!("../../../docs/contracts/migrations/009-to-010.sql");
#[cfg(windows)]
const MIGRATION_V7_TO_V8: &str = include_str!("../../../docs/contracts/migrations/007-to-008.sql");
#[cfg(windows)]
const MIGRATION_V6_TO_V7: &str = include_str!("../../../docs/contracts/migrations/006-to-007.sql");
#[cfg(windows)]
const SCHEMA_V1: &str = include_str!("../../../docs/contracts/migrations/schema-v1.sql");
#[cfg(windows)]
const MIGRATION_V1_TO_V2: &str = include_str!("../../../docs/contracts/migrations/001-to-002.sql");
#[cfg(windows)]
const SCHEMA_V2: &str = include_str!("../../../docs/contracts/migrations/schema-v2.sql");
#[cfg(windows)]
const MIGRATION_V2_TO_V3: &str = include_str!("../../../docs/contracts/migrations/002-to-003.sql");
#[cfg(windows)]
const SCHEMA_V3: &str = include_str!("../../../docs/contracts/migrations/schema-v3.sql");
#[cfg(windows)]
const MIGRATION_V3_TO_V4: &str = include_str!("../../../docs/contracts/migrations/003-to-004.sql");
#[cfg(windows)]
const MIGRATION_V4_TO_V5: &str = include_str!("../../../docs/contracts/migrations/004-to-005.sql");
#[cfg(windows)]
const MIGRATION_V5_TO_V6: &str = include_str!("../../../docs/contracts/migrations/005-to-006.sql");

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Setting {
    pub value_json: String,
    pub version: u64,
}

impl fmt::Debug for Setting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Setting")
            .field("key", &"<redacted>")
            .field("value_json", &"<redacted>")
            .field("version", &self.version)
            .finish()
    }
}

#[cfg(windows)]
use hantong_secrets::{DatabaseError, KeyedConnection};
#[cfg(windows)]
use rusqlite::OptionalExtension;

/// An encrypted, schema-checked local store.
pub struct Store {
    #[cfg(windows)]
    connection: KeyedConnection,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens the fixed encrypted database, initializes the current schema, or
    /// advances validated schemas one version at a time after each backup.
    pub fn open(
        data_root: impl AsRef<std::path::Path>,
        key_root: impl AsRef<std::path::Path>,
    ) -> Result<Self, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (data_root, key_root);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            let connection =
                KeyedConnection::open(data_root, key_root).map_err(map_database_error)?;
            let store = Self { connection };
            store.ensure_schema()?;
            Ok(store)
        }
    }

    /// Returns a setting without exposing the underlying connection.
    pub(crate) fn get_setting(&self, key: &str) -> Result<Option<Setting>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = key;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            validate_setting_key(key)?;
            self.with_connection(|connection| {
                let mut statement = connection
                    .prepare("SELECT value_json, version FROM settings WHERE setting_key = ?1")?;
                let mut rows = statement.query([key])?;
                match rows.next()? {
                    Some(row) => Ok(Some(Setting {
                        value_json: row.get(0)?,
                        version: row
                            .get::<_, i64>(1)?
                            .try_into()
                            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, i64::MAX))?,
                    })),
                    None => Ok(None),
                }
            })
        }
    }

    /// Inserts a setting at version 1, or updates an existing setting using a
    /// compare-and-swap expected version. A failed CAS preserves the row.
    pub(crate) fn put_setting(
        &self,
        key: &str,
        value_json: &str,
        expected_version: u64,
    ) -> Result<u64, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (key, value_json, expected_version);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            validate_setting(key, value_json)?;
            let outcome = self.with_connection(|connection| {
                let valid = connection.query_row("SELECT json_valid(?1)", [value_json], |row| {
                    row.get::<_, i64>(0)
                })?;
                if valid != 1 {
                    return Ok(SettingWrite::Invalid);
                }
                let now = utc_millis()?;
                if expected_version == 0 {
                    let exists = connection.query_row(
                        "SELECT count(*) FROM settings WHERE setting_key = ?1",
                        [key],
                        |row| row.get::<_, i64>(0),
                    )?;
                    if exists != 0 {
                        return Ok(SettingWrite::Conflict);
                    }
                    connection.execute(
                        "INSERT INTO settings(setting_key, value_json, version, updated_at_utc_ms) VALUES (?1, ?2, 1, ?3)",
                        rusqlite::params![key, value_json, now],
                    )?;
                    Ok(SettingWrite::Updated(1))
                } else {
                    if expected_version >= i64::MAX as u64 {
                        return Ok(SettingWrite::Conflict);
                    }
                    let Some(next) = expected_version.checked_add(1) else {
                        return Ok(SettingWrite::Conflict);
                    };
                    let current = connection
                        .query_row(
                            "SELECT version FROM settings WHERE setting_key = ?1",
                            [key],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?;
                    if current != Some(expected_version as i64) {
                        return Ok(SettingWrite::Conflict);
                    }
                    let changed = connection.execute(
                        "UPDATE settings SET value_json = ?1, version = ?2, updated_at_utc_ms = ?3 WHERE setting_key = ?4 AND version = ?5",
                        rusqlite::params![value_json, next as i64, now, key, expected_version as i64],
                    )?;
                    Ok(if changed == 1 {
                        SettingWrite::Updated(next)
                    } else {
                        SettingWrite::Conflict
                    })
                }
            })?;
            match outcome {
                SettingWrite::Invalid => Err(ErrorCode::InvalidInput),
                SettingWrite::Conflict => Err(ErrorCode::Conflict),
                SettingWrite::Updated(version) => Ok(version),
            }
        }
    }

    #[cfg(windows)]
    fn ensure_schema(&self) -> Result<(), ErrorCode> {
        let state = self.with_connection(|connection| {
            let user_version =
                connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
            if !(0..=i64::from(u32::MAX)).contains(&user_version) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let object_count = connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT GLOB 'sqlite_*'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            match (user_version as u32, object_count) {
                (0, 0) => Ok(SchemaState::Fresh),
                (0, _) => Ok(SchemaState::Corrupt),
                (version, _) if version > CURRENT_SCHEMA_VERSION => Ok(SchemaState::Newer),
                (1, _) => Ok(SchemaState::V1),
                (2, _) => Ok(SchemaState::V2),
                (3, _) => Ok(SchemaState::V3),
                (4, _) => Ok(SchemaState::V4),
                (5, _) => Ok(SchemaState::V5),
                (6, _) => Ok(SchemaState::V6),
                (7, _) => Ok(SchemaState::V7),
                (8, _) => Ok(SchemaState::V8),
                (9, _) => Ok(SchemaState::V9),
                (10, _) => Ok(SchemaState::V10),
                (11, _) => Ok(SchemaState::V11),
                (12, _) => Ok(SchemaState::V12),
                (13, _) => Ok(SchemaState::V13),
                (14, _) => Ok(SchemaState::V14),
                (15, _) => Ok(SchemaState::V15),
                (16, _) => Ok(SchemaState::V16),
                (17, _) => Ok(SchemaState::V17),
                (CURRENT_SCHEMA_VERSION, _) => Ok(SchemaState::Current),
                _ => Ok(SchemaState::Unsupported),
            }
        })?;
        match state {
            SchemaState::Fresh => self.with_connection(initialize_schema).map(|_| ()),
            SchemaState::V1 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V1, 1))?;
                self.migrate_v1_to_v2()?;
                self.migrate_v2_to_v3()?;
                self.migrate_v3_to_v4()?;
                self.migrate_v4_to_v5()?;
                self.migrate_v5_to_v6()?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V2 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V2, 2))?;
                self.migrate_v2_to_v3()?;
                self.migrate_v3_to_v4()?;
                self.migrate_v4_to_v5()?;
                self.migrate_v5_to_v6()?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V3 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V3, 3))?;
                self.migrate_v3_to_v4()?;
                self.migrate_v4_to_v5()?;
                self.migrate_v5_to_v6()?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V4 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V4, 4))?;
                self.migrate_v4_to_v5()?;
                self.migrate_v5_to_v6()?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V5 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V5, 5))?;
                self.migrate_v5_to_v6()?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V6 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V6, 6))?;
                self.migrate_v6_to_v7()?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V7 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V7, 7))?;
                self.migrate_v7_to_v8()?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V8 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V8, 8))?;
                self.migrate_v8_to_v9()?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V9 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V9, 9))?;
                self.migrate_v9_to_v10()?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V10 => {
                self.with_connection(|connection| validate_schema(connection, SCHEMA_V10, 10))?;
                self.migrate_v10_to_v11()?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V11 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V11, 11))?;
                self.migrate_v11_to_v12()?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V12 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V12, 12))?;
                self.migrate_v12_to_v13()?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V13 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V13, 13))?;
                self.migrate_v13_to_v14()?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V14 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V14, 14))?;
                self.migrate_v14_to_v15()?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V15 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V15, 15))?;
                self.migrate_v15_to_v16()?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V16 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V16, 16))?;
                self.migrate_v16_to_v17()?;
                self.migrate_v17_to_v18()
            }
            SchemaState::V17 => {
                self.with_connection(|c| validate_schema(c, SCHEMA_V17, 17))?;
                self.migrate_v17_to_v18()
            }
            SchemaState::Current => self.with_connection(|connection| {
                validate_schema(connection, SCHEMA, CURRENT_SCHEMA_VERSION)
            }),
            SchemaState::Newer => Err(ErrorCode::VersionMismatch),
            SchemaState::Corrupt | SchemaState::Unsupported => Err(ErrorCode::InternalError),
        }
    }

    #[cfg(windows)]
    fn migrate_v1_to_v2(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(1, 2, SCHEMA_V2, |connection| {
            connection.execute_batch(MIGRATION_V1_TO_V2)
        })
    }

    #[cfg(windows)]
    fn migrate_v2_to_v3(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(2, 3, SCHEMA_V3, |connection| {
            connection.execute_batch(MIGRATION_V2_TO_V3)
        })
    }

    #[cfg(windows)]
    fn migrate_v3_to_v4(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(3, 4, SCHEMA_V4, |connection| {
            connection.execute_batch(MIGRATION_V3_TO_V4)
        })
    }

    #[cfg(windows)]
    fn migrate_v4_to_v5(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(4, 5, SCHEMA_V5, |connection| {
            connection.execute_batch(MIGRATION_V4_TO_V5)
        })
    }

    #[cfg(windows)]
    fn migrate_v5_to_v6(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(5, 6, SCHEMA_V6, |connection| {
            connection.execute_batch(MIGRATION_V5_TO_V6)
        })
    }

    #[cfg(windows)]
    fn migrate_v6_to_v7(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(6, 7, SCHEMA_V7, |connection| {
            connection.execute_batch(MIGRATION_V6_TO_V7)
        })
    }

    #[cfg(windows)]
    fn migrate_v7_to_v8(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(7, 8, SCHEMA_V8, |connection| {
            connection.execute_batch(MIGRATION_V7_TO_V8)
        })
    }

    #[cfg(windows)]
    fn migrate_v8_to_v9(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(8, 9, SCHEMA_V9, |connection| {
            connection.execute_batch(MIGRATION_V8_TO_V9)
        })
    }

    #[cfg(windows)]
    fn migrate_v9_to_v10(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(9, 10, SCHEMA_V10, |connection| {
            connection.execute_batch(MIGRATION_V9_TO_V10)
        })
    }

    #[cfg(windows)]
    fn migrate_v10_to_v11(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(10, 11, SCHEMA_V11, |c| {
            c.execute_batch(MIGRATION_V10_TO_V11)
        })
    }

    #[cfg(windows)]
    fn migrate_v11_to_v12(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(11, 12, SCHEMA_V12, |c| {
            c.execute_batch(MIGRATION_V11_TO_V12)
        })
    }

    #[cfg(windows)]
    fn migrate_v13_to_v14(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(13, 14, SCHEMA_V14, apply_v14_migration)
    }

    #[cfg(windows)]
    fn migrate_v14_to_v15(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(14, 15, SCHEMA_V15, apply_v15_migration)
    }

    #[cfg(windows)]
    fn migrate_v15_to_v16(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(15, 16, SCHEMA_V16, apply_v16_migration)
    }

    #[cfg(windows)]
    fn migrate_v17_to_v18(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(17, 18, SCHEMA, |c| c.execute_batch(MIGRATION_V17_TO_V18))
    }

    #[cfg(windows)]
    fn migrate_v16_to_v17(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(16, 17, SCHEMA_V17, |c| {
            c.execute_batch(MIGRATION_V16_TO_V17)
        })
    }

    #[cfg(windows)]
    fn migrate_v12_to_v13(&self) -> Result<(), ErrorCode> {
        self.migrate_with_backup(12, 13, SCHEMA_V13, |c| {
            c.execute_batch(MIGRATION_V12_TO_V13)
        })
    }

    #[cfg(windows)]
    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>,
    ) -> Result<T, ErrorCode> {
        self.connection
            .with_connection(operation)
            .map_err(|error| match error {
                DatabaseError::Conflict => ErrorCode::Conflict,
                DatabaseError::Unsupported => ErrorCode::CapabilityUnsupported,
                DatabaseError::Internal => ErrorCode::InternalError,
            })
    }

    /// Runs one fixed migration step after creating and validating the
    /// encrypted same-key backup. Callers cannot supply migration SQL through
    /// the public Store API; future schema versions add ordered private steps.
    #[cfg(windows)]
    fn migrate_with_backup(
        &self,
        current_version: u32,
        next_version: u32,
        expected_schema: &str,
        migration: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<()>,
    ) -> Result<(), ErrorCode> {
        if current_version == 0
            || current_version.checked_add(1) != Some(next_version)
            || next_version > i32::MAX as u32
        {
            return Err(ErrorCode::InternalError);
        }
        self.with_connection(|connection| {
            let recorded: u32 = connection.query_row(
                "SELECT schema_version FROM app_metadata WHERE singleton=1",
                [],
                |r| r.get(0),
            )?;
            if recorded != current_version {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(())
        })?;
        self.connection
            .backup_before_migration(current_version)
            .map_err(map_database_error)?;
        self.with_connection(|connection| {
            let transaction = rusqlite::Transaction::new_unchecked(
                connection,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            migration(&transaction)?;
            if transaction.execute(
                "UPDATE app_metadata SET schema_version=?1,updated_at_utc_ms=?2 WHERE singleton=1 AND schema_version=?3",
                rusqlite::params![next_version, utc_millis()?, current_version],
            )? != 1 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            transaction.pragma_update(None, "user_version", next_version)?;
            validate_schema(&transaction, expected_schema, next_version)?;
            transaction.commit()
        })
        .map(|_| ())
    }
}

#[cfg(windows)]
fn map_database_error(error: DatabaseError) -> ErrorCode {
    match error {
        DatabaseError::Conflict => ErrorCode::Conflict,
        DatabaseError::Unsupported => ErrorCode::CapabilityUnsupported,
        DatabaseError::Internal => ErrorCode::InternalError,
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaState {
    Fresh,
    Current,
    V1,
    V2,
    V3,
    V4,
    V5,
    V6,
    V7,
    V8,
    V9,
    V10,
    V11,
    V12,
    V13,
    V14,
    V15,
    V16,
    V17,
    Newer,
    Unsupported,
    Corrupt,
}

#[cfg(windows)]
enum SettingWrite {
    Invalid,
    Conflict,
    Updated(u64),
}

#[cfg(windows)]
fn initialize_schema(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA)?;
    let now = utc_millis()?;
    transaction.execute(
            "INSERT INTO app_metadata(singleton, database_format, schema_version, created_at_utc_ms, updated_at_utc_ms) VALUES (1, ?1, ?2, ?3, ?3)",
            rusqlite::params![DATABASE_FORMAT, CURRENT_SCHEMA_VERSION, now],
        )?;
    transaction.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
    validate_schema(&transaction, SCHEMA, CURRENT_SCHEMA_VERSION)?;
    transaction.commit()
}

#[cfg(windows)]
fn validate_schema(
    connection: &rusqlite::Connection,
    schema: &str,
    schema_version: u32,
) -> rusqlite::Result<()> {
    // Compare the complete persisted DDL, not just object names. Extra triggers,
    // weakened constraints, changed columns and missing indexes all fail closed.
    let without_comments = schema
        .lines()
        .map(|line| line.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let mut expected = without_comments
        .split(';')
        .map(normalize_ddl)
        .filter(|s| s.starts_with("CREATE "))
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare("SELECT sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name")?;
    let mut actual = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|sql| sql.map(|s| normalize_ddl(&s)))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    expected.sort();
    actual.sort();
    if expected != actual {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let metadata = connection.query_row(
        "SELECT database_format, schema_version, created_at_utc_ms, updated_at_utc_ms FROM app_metadata WHERE singleton = 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)),
    )?;
    if metadata.0 != DATABASE_FORMAT
        || metadata.1 != i64::from(schema_version)
        || metadata.2 <= 0
        || metadata.3 < metadata.2
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let singleton_count = connection.query_row("SELECT count(*) FROM app_metadata", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if singleton_count != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if schema_version >= 3 {
        let capacity = connection.query_row(
            "SELECT count(*), retained_executions, retained_account_refs FROM audit_capacity WHERE singleton=1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
        )?;
        let actual = connection.query_row(
            "SELECT (SELECT count(*) FROM audit_starts), (SELECT count(*) FROM audit_start_accounts)",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if capacity.0 != 1
            || capacity.1 < 0
            || capacity.1 > 100_000
            || capacity.2 < 0
            || capacity.2 > 1_000_000
            || capacity.1 != actual.0
            || capacity.2 != actual.1
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    if schema_version == 5 {
        crate::imap_setup::validate_journal_v5(connection)?;
    } else if schema_version >= 6 {
        crate::imap_setup::validate_journal(connection)?;
    }
    if matches!(schema_version, 7 | 8) {
        crate::scope_refresh::validate_history_v8(connection)?;
    } else if (9..=11).contains(&schema_version) {
        crate::scope_refresh::validate_history_v11(connection)?;
    } else if (12..=14).contains(&schema_version) {
        crate::scope_refresh::validate_history_v14(connection)?;
    } else if schema_version == 15 {
        crate::scope_refresh::validate_history_v15(connection)?;
    } else if (16..18).contains(&schema_version) {
        crate::scope_refresh::validate_history_v17(connection)?;
    } else if schema_version >= 18 {
        crate::scope_refresh::validate_history(connection)?;
    }
    if schema_version >= 10 {
        let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
        if foreign_keys.query([])?.next()?.is_some() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        crate::folder_refresh::validate_history(connection)?;
    }
    if schema_version >= 11 {
        crate::imap_sync::validate_history(connection)?;
    }
    if schema_version >= 12 {
        crate::uid_page_jobs::validate_history(connection)?;
    }
    if schema_version >= 13 {
        crate::uid_reconcile_jobs::validate_history(connection)?;
    }
    if schema_version >= 14 {
        crate::uid_epochs::validate_history(connection)?;
        crate::uid_recovery_jobs::validate_history(connection)?;
    }
    if schema_version >= 15 {
        crate::message_identities::validate_history(connection)?;
    }
    if schema_version >= 17 {
        crate::initial_sync::validate_history(connection)?;
    }
    Ok(())
}

#[cfg(windows)]
fn apply_v16_migration(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let (count, sequence, maximum): (i64, Option<i64>, Option<i64>) = c.query_row(
        "SELECT count(*),max(seq),(SELECT max(audit_seq) FROM audit_starts) FROM sqlite_sequence WHERE name='audit_starts'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if count > 1
        || sequence.is_some_and(|v| v < 0)
        || maximum.is_some_and(|v| sequence.is_none_or(|s| s < v))
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    c.execute_batch(MIGRATION_V15_TO_V16)
}

#[cfg(windows)]
fn apply_v15_migration(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let (count, sequence, maximum): (i64, Option<i64>, Option<i64>) = c.query_row(
        "SELECT count(*),max(seq),(SELECT max(audit_seq) FROM audit_starts) FROM sqlite_sequence WHERE name='audit_starts'",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if count > 1
        || sequence.is_some_and(|v| v < 0)
        || maximum.is_some_and(|v| sequence.is_none_or(|s| s < v))
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    c.execute_batch(MIGRATION_V14_TO_V15)?;
    crate::message_identities::backfill_v14(c)
}

#[cfg(windows)]
fn apply_v14_migration(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let (before, after) = MIGRATION_V13_TO_V14
        .split_once("-- BACKFILL_EPOCHS --")
        .ok_or(rusqlite::Error::InvalidQuery)?;
    c.execute_batch(before)?;
    uid_epochs::backfill_v13(c)?;
    c.execute_batch(after)
}

#[cfg(windows)]
fn normalize_ddl(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(windows)]
fn validate_setting_key(key: &str) -> Result<(), ErrorCode> {
    if key.is_empty() || key.len() > MAX_SETTING_KEY_BYTES || key.chars().any(char::is_control) {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn validate_setting(key: &str, value_json: &str) -> Result<(), ErrorCode> {
    validate_setting_key(key)?;
    if value_json.len() > MAX_SETTING_JSON_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(windows)]
fn utc_millis() -> rusqlite::Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .filter(|millis| *millis > 0)
        .ok_or(rusqlite::Error::InvalidQuery)
}

#[cfg(all(test, windows))]
mod tests {
    include!("initial_migration_tests.rs");
    include!("migration_v18_tests.rs");
    use super::*;
    include!("migration_v15_tests.rs");
    include!("migration_v16_tests.rs");
    use hantong_domain::{
        account_provisioning::NewImapAccountDraft, mail_connection::MailConnectionConfig,
    };
    use hantong_secrets::KeyedConnection;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).expect("unique fixture directory");
            Self(root)
        }

        fn open(&self) -> Result<Store, ErrorCode> {
            Store::open(self.0.join("data"), self.0.join("keys"))
        }

        fn keyed(&self) -> KeyedConnection {
            KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap()
        }

        fn seed_v1(&self) {
            let keyed = KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap();
            keyed.with_connection(|connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                transaction.execute_batch(SCHEMA_V1)?;
                transaction.execute(
                    "INSERT INTO app_metadata(singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,'mail-hantong',1,1,1)",
                    [],
                )?;
                transaction.pragma_update(None, "user_version", 1)?;
                transaction.commit()
            }).unwrap();
        }

        fn seed_v2(&self) {
            let keyed = KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap();
            keyed.with_connection(|connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                transaction.execute_batch(SCHEMA_V2)?;
                transaction.execute(
                    "INSERT INTO app_metadata(singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,'mail-hantong',2,1,1)",
                    [],
                )?;
                transaction.pragma_update(None, "user_version", 2)?;
                transaction.commit()
            }).unwrap();
        }

        fn seed_v3(&self) {
            let keyed = KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap();
            keyed.with_connection(|connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                transaction.execute_batch(SCHEMA_V3)?;
                transaction.execute(
                    "INSERT INTO app_metadata(singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,'mail-hantong',3,1,1)",
                    [],
                )?;
                transaction.pragma_update(None, "user_version", 3)?;
                transaction.commit()
            }).unwrap();
        }

        fn seed_v4(&self) {
            let keyed = KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap();
            keyed.with_connection(|connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                transaction.execute_batch(SCHEMA_V4)?;
                transaction.execute(
                    "INSERT INTO app_metadata(singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,'mail-hantong',4,1,4)",
                    [],
                )?;
                transaction.pragma_update(None, "user_version", 4)?;
                transaction.execute(
                    "INSERT INTO settings(setting_key,value_json,version,updated_at_utc_ms) VALUES('migration.fixture','{\"v\":4}',1,2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('v4-account','imap',1,'connecting','[]','[]',1)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('v4-account','imap',1,'imap.example',993,'implicit_tls','user','app_password',3)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('v4-account','smtp',2,'smtp.example',587,'starttls_required','user','app_password',4)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES('v4-account','imap',1,1,'ready',5)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) VALUES('v4-operation','v4-account','v4-idempotency','smtp',?1,2,3,6)",
                    [vec![6u8; 32]],
                )?;
                transaction.commit()
            }).unwrap();
        }

        fn seed_v5(&self) {
            let keyed = KeyedConnection::open(self.0.join("data"), self.0.join("keys")).unwrap();
            let draft = NewImapAccountDraft::new(
                "v5-setup-account".into(),
                Some("Setup account".into()),
                Some("#654321".into()),
            )
            .unwrap();
            let endpoint = MailConnectionConfig::from_json(
                br#"{"schema_version":1,"account_id":"v5-setup-account","imap":{"settings_version":1,"host":"imap.setup.example","port":993,"tls":"implicit_tls","login_name":"setup-user","authentication":"app_password"}}"#,
            )
            .unwrap()
            .imap()
            .clone();
            let setup_hash =
                crate::account_provisioning::imap_account_stage_payload_hash(&draft, &endpoint)
                    .unwrap();
            let pending_draft = NewImapAccountDraft::new(
                "v5-pending".into(),
                Some("v5 setup".into()),
                Some("#123456".into()),
            )
            .unwrap();
            let pending_endpoint = MailConnectionConfig::from_json(
                br#"{"schema_version":1,"account_id":"v5-pending","imap":{"settings_version":1,"host":"imap.pending.example","port":993,"tls":"implicit_tls","login_name":"pending-user","authentication":"app_password"}}"#,
            )
            .unwrap()
            .imap()
            .clone();
            let pending_hash = crate::account_provisioning::imap_account_stage_payload_hash(
                &pending_draft,
                &pending_endpoint,
            )
            .unwrap();
            keyed.with_connection(|connection| {
                let transaction = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                transaction.execute_batch(SCHEMA_V5)?;
                transaction.execute(
                    "INSERT INTO app_metadata(singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,'mail-hantong',5,1,5)",
                    [],
                )?;
                transaction.pragma_update(None, "user_version", 5)?;
                transaction.execute(
                    "INSERT INTO settings(setting_key,value_json,version,updated_at_utc_ms) VALUES('migration.v5','{\"v\":5}',2,2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO accounts(account_id,provider,provider_subject,primary_address,display_name,color,account_number,status,capabilities,granted_scopes,settings_version) VALUES('v5-account','imap','v5-subject','v5@example.test','V5 account','#123456',1,'ready','[\"imap\"]','[\"mail.read\"]',2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO accounts(account_id,provider,provider_subject,primary_address,display_name,color,account_number,status,capabilities,granted_scopes,settings_version) VALUES('v5-setup-account','imap',NULL,NULL,'Setup account','#654321',2,'reauth_required','[]','[]',2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','v5-fixture','[\"v5-account\"]','[\"mail.read\"]','approved','gui',1,1,300001,2)",
                    [vec![21u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli','v5-fixture',?3,1,'active',1,60,'gui',2,2,NULL,1)",
                    rusqlite::params![vec![22u8; 16], vec![21u8; 32], vec![23u8; 72]],
                )?;
                transaction.execute(
                    "INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,'v5-account')",
                    [vec![22u8; 16]],
                )?;
                transaction.execute(
                    "INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.read')",
                    [vec![22u8; 16]],
                )?;
                transaction.execute(
                    "INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'active',?2,1,2,2,3)",
                    rusqlite::params![vec![28u8; 32], vec![29u8; 72]],
                )?;
                transaction.execute(
                    "INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) VALUES(?1,?2,1,'cli','v5-fixture','mail.accounts.list',6)",
                    rusqlite::params![vec![30u8; 32], vec![22u8; 16]],
                )?;
                transaction.execute(
                    "INSERT INTO audit_start_accounts(execution_id,account_id) VALUES(?1,'v5-account')",
                    [vec![30u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO audit_terminals(execution_id,completed_at_utc_ms,result_kind,error_code,result_count) VALUES(?1,7,'returned_ok',NULL,1)",
                    [vec![30u8; 32]],
                )?;
                transaction.execute(
                    "UPDATE audit_capacity SET retained_executions=1,retained_account_refs=1 WHERE singleton=1",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('v5-account','imap',2,'imap.v5.example',993,'implicit_tls','v5-user','app_password',3)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('v5-account','smtp',1,'smtp.v5.example',587,'starttls_required','v5-user','app_password',3)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('v5-setup-account','imap',1,'imap.setup.example',993,'implicit_tls','setup-user','app_password',3)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES('v5-account','imap',2,1,'ready',4)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES('v5-setup-account','imap',1,1,'auth_required',15)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) VALUES('v5-operation','v5-account','v5-idempotency','imap',?1,1,2,5)",
                    [vec![24u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) VALUES('v5-stage-operation','v5-setup-account','v5-stage-idempotency','imap',?1,0,1,11)",
                    [setup_hash.as_slice()],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_requests(request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms) VALUES(?1,'v5-pending','v5-setup-operation','v5-setup-key','v5 setup','#123456',1,'imap.pending.example',993,'implicit_tls','pending-user','app_password',?2,'local_same_logon_bootstrap','review_pending',1,10,300010,10)",
                    rusqlite::params![vec![25u8; 32], pending_hash.as_slice()],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms) VALUES(?1,1,'request_created',10)",
                    [vec![25u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_requests(request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms) VALUES(?1,'v5-setup-account','v5-stage-operation','v5-stage-idempotency','Setup account','#654321',1,'imap.setup.example',993,'implicit_tls','setup-user','app_password',?2,'local_same_logon_bootstrap','probe_finished',6,10,300010,15)",
                    rusqlite::params![vec![27u8; 32], setup_hash.as_slice()],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,account_version,endpoint_version,observation_version) VALUES(?1,1,'request_created',10,NULL,NULL,NULL)",
                    [vec![27u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,account_version,endpoint_version,observation_version) VALUES(?1,2,'account_staged',11,1,1,0)",
                    [vec![27u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms) VALUES(?1,3,'credential_store_started',12)",
                    [vec![27u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,result_kind) VALUES(?1,4,'credential_store_returned',13,'returned_ok')",
                    [vec![27u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms) VALUES(?1,5,'probe_started',14)",
                    [vec![27u8; 32]],
                )?;
                transaction.execute(
                    "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,probe_outcome,capabilities,account_version,endpoint_version,observation_version) VALUES(?1,6,'probe_finished',15,'auth_required','[]',2,1,1)",
                    [vec![27u8; 32]],
                )?;
                transaction.commit()
            }).unwrap();
        }

        fn seed_populated_v3(&self) {
            self.seed_v3();
            let keyed = self.keyed();
            keyed.with_connection(|c| {
                c.execute("INSERT INTO settings(setting_key,value_json,version,updated_at_utc_ms) VALUES('ui.theme','\"dark\"',2,2)", [])?;
                c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('migration-account','imap',1,'ready','[]','[]',1)", [])?;
                c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','fixture','[\"migration-account\"]','[\"mail.accounts.list\"]','approved','gui',1,1,300001,2)", [vec![8u8; 32]])?;
                c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli','fixture',?3,1,'active',1,60,'gui',2,2,NULL,1)", rusqlite::params![vec![9u8; 16], vec![8u8; 32], vec![7u8; 72]])?;
                c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,'migration-account')", [vec![9u8; 16]])?;
                c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.accounts.list')", [vec![9u8; 16]])?;
                c.execute("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) VALUES(?1,?2,1,'cli','fixture','mail.accounts.list',3)", rusqlite::params![vec![3u8; 32], vec![9u8; 16]])?;
                c.execute("INSERT INTO audit_start_accounts(execution_id,account_id) VALUES(?1,'migration-account')", [vec![3u8; 32]])?;
                c.execute("INSERT INTO audit_terminals(execution_id,completed_at_utc_ms,result_kind,error_code,result_count) VALUES(?1,4,'returned_ok',NULL,1)", [vec![3u8; 32]])?;
                c.execute("UPDATE audit_capacity SET retained_executions=1,retained_account_refs=1 WHERE singleton=1", [])?;
                Ok(())
            }).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn m104_initializes_schema_and_settings_cas() {
        let fixture = Fixture::new();
        let store = fixture.open().unwrap();
        assert_eq!(store.get_setting("ui.theme").unwrap(), None);
        assert_eq!(store.put_setting("ui.theme", "\"system\"", 0), Ok(1));
        assert_eq!(store.put_setting("ui.theme", "\"dark\"", 1), Ok(2));
        assert_eq!(
            store.put_setting("ui.theme", "\"light\"", 1),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            store.get_setting("ui.theme").unwrap().unwrap().value_json,
            "\"dark\""
        );
        drop(store);
        let keyed = fixture.keyed();
        assert_eq!(
            keyed
                .with_connection(|connection| connection.pragma_query_value(
                    None,
                    "user_version",
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            keyed
                .with_connection(|connection| connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            41
        );
    }

    #[test]
    fn m104_rejects_invalid_setting_without_modifying_row() {
        let fixture = Fixture::new();
        let store = fixture.open().unwrap();
        assert_eq!(
            store.put_setting("bad", "not-json", 0),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            store.put_setting("bad", &"x".repeat(MAX_SETTING_JSON_BYTES + 1), 0),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(store.get_setting("bad").unwrap(), None);
    }

    #[test]
    fn m104_v1_migrates_to_v2_with_settings_and_no_active_registrations() {
        let fixture = Fixture::new();
        fixture.seed_v1();
        let before = fixture.keyed().with_connection(|c| {
            c.execute("INSERT INTO settings(setting_key,value_json,version,updated_at_utc_ms) VALUES('legacy','1',1,2)", [])
        }).unwrap();
        assert_eq!(before, 1);
        let store = fixture.open().unwrap();
        assert_eq!(
            store.get_setting("legacy").unwrap().unwrap().value_json,
            "1"
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM client_registrations",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert!(fixture.0.join("data/backup-before-v1.db").is_file());
        assert!(fixture.0.join("data/backup-before-v2.db").is_file());
        assert!(fixture.0.join("data/backup-before-v3.db").is_file());
    }

    #[test]
    fn m105_fresh_v3_has_zero_audit_capacity() {
        let fixture = Fixture::new();
        let store = fixture.open().unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(store.with_connection(|c| c.query_row("SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))).unwrap(), (0, 0));
    }

    #[test]
    fn m201_populated_v3_migrates_to_v4_without_losing_existing_rows() {
        fn previous_rows(
            connection: &rusqlite::Connection,
        ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
            let mut tables = Vec::new();
            for table in [
                "settings",
                "accounts",
                "server_identity",
                "client_registration_requests",
                "client_registrations",
                "client_registration_accounts",
                "client_registration_tools",
                "audit_capacity",
                "audit_starts",
                "audit_start_accounts",
                "audit_terminals",
            ] {
                let mut statement =
                    connection.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
                let columns = statement.column_count();
                let rows = statement.query_map([], |row| {
                    (0..columns)
                        .map(|i| row.get(i))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })?;
                tables.push(rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            Ok(tables)
        }
        let fixture = Fixture::new();
        fixture.seed_populated_v3();
        let keyed = fixture.keyed();
        let before = keyed.with_connection(previous_rows).unwrap();
        drop(keyed);
        let store = fixture.open().unwrap();
        assert_eq!(store.with_connection(previous_rows).unwrap(), before);
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT value_json FROM settings WHERE setting_key='ui.theme'",
                    [],
                    |r| r.get::<_, String>(0)
                ))
                .unwrap(),
            "\"dark\""
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM accounts WHERE account_id='migration-account'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM client_registrations",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.query_row("SELECT count(*) FROM audit_starts", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM audit_start_accounts",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.query_row("SELECT count(*) FROM audit_terminals", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            1
        );
        assert_eq!(store.with_connection(|c| c.query_row("SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))).unwrap(), (1, 1));
        for table in [
            "mail_connection_settings",
            "mail_connection_observations",
            "mail_connection_operations",
        ] {
            assert_eq!(
                store
                    .with_connection(|c| c.query_row(
                        &format!("SELECT count(*) FROM {table}"),
                        [],
                        |r| r.get::<_, i64>(0)
                    ))
                    .unwrap(),
                0
            );
        }
        assert!(fixture.0.join("data/backup-before-v3.db").is_file());
    }

    #[test]
    fn m201_v3_migration_rollback_removes_new_tables_and_preserves_v3() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let store = Store {
            connection: fixture.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(3, 4, SCHEMA_V4, |connection| {
                connection.execute_batch(MIGRATION_V3_TO_V4)?;
                connection.execute_batch("CREATE TABLE unwanted_v4(x)")
            }),
            Err(ErrorCode::InternalError)
        );
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V3, 3))
            .unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            3
        );
        assert_eq!(store.with_connection(|c| c.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('mail_connection_settings','mail_connection_observations','mail_connection_operations','unwanted_v4')", [], |r| r.get::<_, i64>(0))).unwrap(), 0);
    }

    #[test]
    fn m201_v3_existing_backup_conflict_preserves_database_and_artifact() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let database_path = fixture.0.join("data/mail-hantong.db");
        let backup_path = fixture.0.join("data/backup-before-v3.db");
        let database_before = fs::read(&database_path).unwrap();
        fs::write(&backup_path, b"preexisting-v3-artifact").unwrap();
        let backup_before = fs::read(&backup_path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(database_path).unwrap(), database_before);
        assert_eq!(fs::read(backup_path).unwrap(), backup_before);
    }

    #[test]
    fn m201_malformed_v3_fails_before_backup() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute_batch("DROP TABLE audit_capacity"))
            .unwrap();
        drop(keyed);
        let database_path = fixture.0.join("data/mail-hantong.db");
        let database_before = fs::read(&database_path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(fs::read(database_path).unwrap(), database_before);
        assert!(!fixture.0.join("data/backup-before-v3.db").exists());
    }

    #[test]
    fn m105_missing_audit_capacity_is_rejected_without_repair_or_overwrite() {
        let fixture = Fixture::new();
        drop(fixture.open().unwrap());
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute("DELETE FROM audit_capacity", []))
            .unwrap();
        drop(keyed);
        let path = fixture.0.join("data/mail-hantong.db");
        let before = fs::read(&path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn m105_inconsistent_audit_capacity_is_rejected_without_repair_or_overwrite() {
        let fixture = Fixture::new();
        drop(fixture.open().unwrap());
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute("UPDATE audit_capacity SET retained_executions=1", []))
            .unwrap();
        drop(keyed);
        let path = fixture.0.join("data/mail-hantong.db");
        let before = fs::read(&path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn m105_v2_migrates_to_v3_preserving_identity_and_registrations() {
        let fixture = Fixture::new();
        fixture.seed_v2();
        let keyed = fixture.keyed();
        keyed.with_connection(|c| {
            let mut public_key = vec![0u8; 72];
            public_key[..4].copy_from_slice(b"ECS1");
            public_key[4..8].copy_from_slice(&32u32.to_le_bytes());
            c.execute("INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'creating',NULL,1,1,1,1)", [vec![7u8; 32]])?;
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('migration-account','imap',1,'ready','[]','[]',1)", [])?;
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','migration','[\"migration-account\"]','[\"mail.read\"]','approved','gui',1,1,300001,2)", [vec![8u8; 32]])?;
            c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli','migration',?3,1,'active',1,60,'gui',2,2,NULL,1)", rusqlite::params![vec![9u8;16], vec![8u8;32], public_key])?;
            Ok(())
        }).unwrap();
        drop(keyed);
        let store = fixture.open().unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.query_row("SELECT count(*) FROM server_identity", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM client_registrations",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
        assert_eq!(store.with_connection(|c| c.query_row("SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))).unwrap(), (0, 0));
        assert!(fixture.0.join("data/backup-before-v2.db").is_file());
    }

    #[test]
    fn m105_v2_existing_backup_conflict_preserves_database_and_artifact() {
        let fixture = Fixture::new();
        fixture.seed_v2();
        let database_path = fixture.0.join("data/mail-hantong.db");
        let backup_path = fixture.0.join("data/backup-before-v2.db");
        let database_before = fs::read(&database_path).unwrap();
        fs::write(&backup_path, b"preexisting-v2-artifact").unwrap();
        let backup_before = fs::read(&backup_path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(database_path).unwrap(), database_before);
        assert_eq!(fs::read(backup_path).unwrap(), backup_before);
    }

    #[test]
    fn m105_v2_migration_schema_validation_rolls_back_before_commit() {
        let fixture = Fixture::new();
        fixture.seed_v2();
        let store = Store {
            connection: fixture.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(2, 3, SCHEMA_V3, |connection| {
                connection.execute_batch(MIGRATION_V2_TO_V3)?;
                connection.execute_batch("CREATE TABLE unwanted_v3(x)")
            }),
            Err(ErrorCode::InternalError)
        );
        drop(store);
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| validate_schema(c, SCHEMA_V2, 2))
            .unwrap();
        assert_eq!(
            keyed
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            2
        );
        assert_eq!(keyed.with_connection(|c| c.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('unwanted_v3','audit_capacity')", [], |r| r.get::<_, i64>(0))).unwrap(), 0);
        assert!(fixture.0.join("data/backup-before-v2.db").is_file());
    }

    #[test]
    fn m104_invalid_v1_schema_is_rejected_without_migration_or_backup_replacement() {
        let fixture = Fixture::new();
        fixture.seed_v1();
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute_batch("DROP INDEX accounts_number_idx"))
            .unwrap();
        drop(keyed);
        let before = fs::read(fixture.0.join("data/mail-hantong.db")).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(
            fs::read(fixture.0.join("data/mail-hantong.db")).unwrap(),
            before
        );
        assert!(!fixture.0.join("data/backup-before-v1.db").exists());
    }

    #[test]
    fn m104_v1_existing_backup_conflict_preserves_database_and_artifact_bytes() {
        let fixture = Fixture::new();
        fixture.seed_v1();
        let database_path = fixture.0.join("data/mail-hantong.db");
        let backup_path = fixture.0.join("data/backup-before-v1.db");
        let database_before = fs::read(&database_path).unwrap();
        fs::write(&backup_path, b"preexisting-recovery-artifact").unwrap();
        let backup_before = fs::read(&backup_path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(database_path).unwrap(), database_before);
        assert_eq!(fs::read(backup_path).unwrap(), backup_before);
    }

    #[test]
    fn m104_v1_metadata_version_mismatch_fails_before_backup_and_preserves_database() {
        let fixture = Fixture::new();
        fixture.seed_v1();
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute_batch("UPDATE app_metadata SET schema_version=2"))
            .unwrap();
        drop(keyed);
        let database_path = fixture.0.join("data/mail-hantong.db");
        let database_before = fs::read(&database_path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(fs::read(database_path).unwrap(), database_before);
        assert!(!fixture.0.join("data/backup-before-v1.db").exists());
    }

    #[test]
    fn m104_v1_migration_schema_validation_rolls_back_unwanted_objects_before_commit() {
        let fixture = Fixture::new();
        fixture.seed_v1();
        let store = Store {
            connection: fixture.keyed(),
        };
        let result = store.migrate_with_backup(1, 2, SCHEMA_V2, |connection| {
            connection.execute_batch(MIGRATION_V1_TO_V2)?;
            connection.execute_batch("CREATE TABLE unwanted(x)")
        });
        assert_eq!(result, Err(ErrorCode::InternalError));
        drop(store);
        let keyed = fixture.keyed();
        keyed
            .with_connection(|connection| validate_schema(connection, SCHEMA_V1, 1))
            .unwrap();
        assert_eq!(
            keyed
                .with_connection(|connection| connection.pragma_query_value(
                    None,
                    "user_version",
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
        assert_eq!(
            keyed
                .with_connection(|connection| connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name IN ('unwanted','server_identity')",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert!(fixture.0.join("data/backup-before-v1.db").is_file());
    }

    #[test]
    fn m104_newer_schema_is_rejected_without_overwrite() {
        let fixture = Fixture::new();
        drop(fixture.open().unwrap());
        let keyed = fixture.keyed();
        keyed
            .with_connection(|connection| connection.execute_batch("PRAGMA user_version = 99"))
            .unwrap();
        drop(keyed);
        let before = fs::read(fixture.0.join("data/mail-hantong.db")).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::VersionMismatch)));
        assert_eq!(
            fs::read(fixture.0.join("data/mail-hantong.db")).unwrap(),
            before
        );
    }

    #[test]
    fn m104_migration_failure_keeps_source_and_verified_backup() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let store = Store {
            connection: fixture.keyed(),
        };
        let expected_schema = format!("{SCHEMA_V3}\nCREATE TABLE migration_fixture(x);");
        assert_eq!(store.put_setting("migration", "1", 0), Ok(1));
        assert_eq!(
            store.migrate_with_backup(3, 4, &expected_schema, |connection| {
                connection.execute_batch("CREATE TABLE migration_fixture(x); UPDATE settings SET value_json='2',version=2 WHERE setting_key='migration'; PRAGMA user_version=3;")?;
                Err(rusqlite::Error::InvalidQuery)
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            store.get_setting("migration").unwrap().unwrap().value_json,
            "1"
        );
        assert!(fixture.0.join("data/backup-before-v3.db").is_file());
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='migration_fixture'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            3
        );
        assert_eq!(store.get_setting("migration").unwrap().unwrap().version, 1);
    }

    #[test]
    fn m104_backup_failure_never_starts_migration_and_preserves_existing_artifact() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let store = Store {
            connection: fixture.keyed(),
        };
        let path = fixture.0.join("data/backup-before-v3.db");
        fs::write(&path, b"preserved-artifact").unwrap();
        let entered = std::cell::Cell::new(false);
        let expected_schema = SCHEMA_V3;
        assert_eq!(
            store.migrate_with_backup(3, 4, expected_schema, |_| {
                entered.set(true);
                Ok(())
            }),
            Err(ErrorCode::Conflict)
        );
        assert!(!entered.get());
        assert_eq!(fs::read(path).unwrap(), b"preserved-artifact");
        assert_eq!(store.ensure_schema(), Err(ErrorCode::Conflict));
    }

    #[test]
    fn m104_migration_success_updates_both_versions_atomically() {
        let fixture = Fixture::new();
        fixture.seed_v3();
        let store = Store {
            connection: fixture.keyed(),
        };
        let expected_schema = format!("{SCHEMA_V3}\nCREATE TABLE migration_fixture(x);");
        store
            .migrate_with_backup(3, 4, &expected_schema, |c| {
                c.execute_batch("CREATE TABLE migration_fixture(x)")
            })
            .unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            4
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT schema_version FROM app_metadata",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            4
        );
        assert_eq!(store.ensure_schema(), Err(ErrorCode::InternalError));
    }

    #[test]
    fn m104_metadata_version_mismatch_is_rejected_without_modification() {
        let fixture = Fixture::new();
        drop(fixture.open().unwrap());
        let keyed = fixture.keyed();
        keyed
            .with_connection(|c| c.execute_batch("UPDATE app_metadata SET schema_version=2"))
            .unwrap();
        drop(keyed);
        let path = fixture.0.join("data/mail-hantong.db");
        let before = fs::read(&path).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn m104_schema_mutations_are_rejected_before_use_without_repairing_them() {
        for change in [
            "DROP INDEX accounts_provider_subject_uq",
            "CREATE TRIGGER unexpected AFTER INSERT ON settings BEGIN DELETE FROM settings; END",
            "ALTER TABLE accounts ADD COLUMN unexpected TEXT",
            "DROP TABLE settings; CREATE TABLE settings(setting_key TEXT,value_json TEXT,version INTEGER,updated_at_utc_ms INTEGER)",
            "CREATE TABLE extra(value TEXT)",
            "CREATE TABLE sqlitex_hidden(value TEXT)",
            "CREATE TRIGGER sqlitez_hidden AFTER INSERT ON settings BEGIN DELETE FROM settings; END",
        ] {
            let fixture = Fixture::new();
            drop(fixture.open().unwrap());
            let keyed = fixture.keyed();
            keyed.with_connection(|c| c.execute_batch(change)).unwrap();
            drop(keyed);
            let path = fixture.0.join("data/mail-hantong.db");
            let before = fs::read(&path).unwrap();
            assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
            assert_eq!(fs::read(path).unwrap(), before);
        }
    }

    #[test]
    fn m104_account_constraints_reject_unknown_provider_and_nonarray_capabilities() {
        let fixture = Fixture::new();
        let store = fixture.open().unwrap();
        let sql = "INSERT INTO accounts(account_id,provider,provider_subject,account_number,status,capabilities,granted_scopes,settings_version) VALUES (?1,?2,?3,1,'ready',?4,'[]',1)";
        for (id, provider, subject, capabilities, valid) in [
            ("a", "imap", "subject", "[]", true),
            ("b", "smtp", "other", "[]", false),
            ("c", "imap", "subject", "[]", false),
            ("d", "gmail", "g", "{}", false),
        ] {
            assert_eq!(
                store
                    .with_connection(|c| c.execute(sql, [id, provider, subject, capabilities]))
                    .is_ok(),
                valid
            );
        }
    }

    #[test]
    fn m104_invalid_updates_and_exhausted_versions_keep_previous_setting() {
        let fixture = Fixture::new();
        let store = fixture.open().unwrap();
        store.put_setting("test", "1", 0).unwrap();
        for (json, expected, error) in [
            ("invalid", 1, ErrorCode::InvalidInput),
            ("2", u64::MAX, ErrorCode::Conflict),
            ("2", i64::MAX as u64, ErrorCode::Conflict),
            ("2", 0, ErrorCode::Conflict),
        ] {
            assert_eq!(store.put_setting("test", json, expected), Err(error));
            assert_eq!(store.get_setting("test").unwrap().unwrap().value_json, "1");
        }
    }

    #[test]
    fn m104_debug_redacts_setting_contents() {
        let setting = Setting {
            value_json: "sentinel-setting-value".into(),
            version: 1,
        };
        let debug = format!("{setting:?}");
        assert!(!debug.contains("sentinel-setting-value"));
        assert!(!debug.contains("secret.setting"));
    }

    #[test]
    fn setup01_v4_migrates_to_v5_preserving_setup_rows_and_writes_v4_backup() {
        let fixture = Fixture::new();
        fixture.seed_v4();
        fn rows(
            connection: &rusqlite::Connection,
        ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
            let mut snapshots = Vec::new();
            for table in [
                "settings",
                "accounts",
                "mail_connection_settings",
                "mail_connection_observations",
                "mail_connection_operations",
            ] {
                let mut statement =
                    connection.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
                let columns = statement.column_count();
                let table_rows = statement.query_map([], |row| {
                    (0..columns)
                        .map(|index| row.get(index))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })?;
                snapshots.push(table_rows.collect::<rusqlite::Result<Vec<_>>>()?);
            }
            Ok(snapshots)
        }
        let before = fixture.keyed().with_connection(rows).unwrap();
        let store = fixture.open().unwrap();
        assert_eq!(before, store.with_connection(rows).unwrap());
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(fixture.0.join("data/backup-before-v4.db").is_file());
    }

    #[test]
    fn setup01_v4_backup_conflict_preserves_source_and_does_not_start_v5() {
        let fixture = Fixture::new();
        fixture.seed_v4();
        let path = fixture.0.join("data/backup-before-v4.db");
        fs::write(&path, b"preserved-v4-backup").unwrap();
        let source = fs::read(fixture.0.join("data/mail-hantong.db")).unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(&path).unwrap(), b"preserved-v4-backup");
        assert_eq!(
            fs::read(fixture.0.join("data/mail-hantong.db")).unwrap(),
            source
        );
        let keyed = fixture.keyed();
        assert_eq!(
            keyed
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            4
        );
    }

    #[test]
    fn setup01_malformed_v4_is_rejected_before_backup() {
        let fixture = Fixture::new();
        fixture.seed_v4();
        fixture
            .keyed()
            .with_connection(|c| c.execute_batch("DROP INDEX accounts_number_idx"))
            .unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert!(!fixture.0.join("data/backup-before-v4.db").exists());
    }

    #[test]
    fn setup01_v5_migration_rollback_preserves_v4_and_verified_backup() {
        let fixture = Fixture::new();
        fixture.seed_v4();
        let store = Store {
            connection: fixture.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(4, 5, SCHEMA_V5, |connection| {
                connection.execute_batch(MIGRATION_V4_TO_V5)?;
                connection.execute_batch("CREATE TABLE unwanted_v5(x)")?;
                Ok(())
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            4
        );
        assert!(fixture.0.join("data/backup-before-v4.db").is_file());
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='unwanted_v5'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name IN ('imap_setup_requests','imap_setup_events')",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='mail_connection_operations'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
    }

    #[test]
    fn setup01_v5_migrates_to_v6_preserving_existing_rows_and_history() {
        fn rows(
            connection: &rusqlite::Connection,
        ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
            let queries = [
                "SELECT singleton,database_format,created_at_utc_ms FROM app_metadata ORDER BY singleton",
                "SELECT * FROM settings ORDER BY rowid",
                "SELECT * FROM accounts ORDER BY rowid",
                "SELECT * FROM server_identity ORDER BY rowid",
                "SELECT * FROM client_registration_requests ORDER BY rowid",
                "SELECT * FROM client_registrations ORDER BY rowid",
                "SELECT * FROM client_registration_accounts ORDER BY rowid",
                "SELECT * FROM client_registration_tools ORDER BY rowid",
                "SELECT singleton,retained_executions,retained_account_refs FROM audit_capacity ORDER BY singleton",
                "SELECT * FROM audit_starts ORDER BY rowid",
                "SELECT * FROM audit_start_accounts ORDER BY rowid",
                "SELECT * FROM audit_terminals ORDER BY rowid",
                "SELECT * FROM mail_connection_settings ORDER BY rowid",
                "SELECT * FROM mail_connection_observations ORDER BY rowid",
                "SELECT * FROM mail_connection_operations ORDER BY rowid",
                "SELECT request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms FROM imap_setup_requests ORDER BY request_id",
                "SELECT request_id,seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version FROM imap_setup_events ORDER BY request_id,seq",
            ];
            queries
                .iter()
                .map(|query| {
                    let mut statement = connection.prepare(query)?;
                    let columns = statement.column_count();
                    let rows = statement.query_map([], |row| {
                        (0..columns)
                            .map(|index| row.get(index))
                            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                    })?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                })
                .collect()
        }
        let fixture = Fixture::new();
        fixture.seed_v5();
        let before = fixture.keyed().with_connection(rows).unwrap();
        let store = fixture.open().unwrap();
        assert_eq!(before, store.with_connection(rows).unwrap());
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM imap_setup_requests WHERE resume_of_request_id IS NOT NULL OR resume_parent_version IS NOT NULL OR resume_account_version IS NOT NULL OR resume_observation_version IS NOT NULL OR resume_payload_hash IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM imap_setup_events WHERE child_request_id IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(fixture.0.join("data/backup-before-v5.db").is_file());
    }

    #[test]
    fn setup01_v5_backup_conflict_preserves_source_and_artifact() {
        let fixture = Fixture::new();
        fixture.seed_v5();
        let database = fixture.0.join("data/mail-hantong.db");
        let backup = fixture.0.join("data/backup-before-v5.db");
        let database_before = fs::read(&database).unwrap();
        fs::write(&backup, b"preserved-v5-backup").unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(database).unwrap(), database_before);
        assert_eq!(fs::read(backup).unwrap(), b"preserved-v5-backup");
    }

    #[test]
    fn setup01_malformed_v5_is_rejected_before_backup() {
        let fixture = Fixture::new();
        fixture.seed_v5();
        fixture
            .keyed()
            .with_connection(|c| c.execute_batch("DROP INDEX accounts_number_idx"))
            .unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert!(!fixture.0.join("data/backup-before-v5.db").exists());
    }

    #[test]
    fn setup01_malformed_v5_journal_hash_is_rejected_before_backup() {
        let fixture = Fixture::new();
        fixture.seed_v5();
        fixture
            .keyed()
            .with_connection(|c| {
                c.execute(
                    "UPDATE imap_setup_requests SET payload_hash=?1 WHERE request_id=?2",
                    rusqlite::params![vec![31u8; 32], vec![27u8; 32]],
                )
            })
            .unwrap();
        assert!(matches!(fixture.open(), Err(ErrorCode::InternalError)));
        assert!(!fixture.0.join("data/backup-before-v5.db").exists());
    }

    #[test]
    fn setup01_v6_validation_failure_rolls_back_real_ddl_and_preserves_v5() {
        let fixture = Fixture::new();
        fixture.seed_v5();
        let snapshot = |connection: &rusqlite::Connection| {
            let queries = [
                "SELECT * FROM settings ORDER BY rowid",
                "SELECT * FROM accounts ORDER BY rowid",
                "SELECT * FROM server_identity ORDER BY rowid",
                "SELECT * FROM client_registration_requests ORDER BY rowid",
                "SELECT * FROM client_registrations ORDER BY rowid",
                "SELECT * FROM client_registration_accounts ORDER BY rowid",
                "SELECT * FROM client_registration_tools ORDER BY rowid",
                "SELECT singleton,retained_executions,retained_account_refs FROM audit_capacity ORDER BY singleton",
                "SELECT * FROM audit_starts ORDER BY rowid",
                "SELECT * FROM audit_start_accounts ORDER BY rowid",
                "SELECT * FROM audit_terminals ORDER BY rowid",
                "SELECT * FROM mail_connection_settings ORDER BY rowid",
                "SELECT * FROM mail_connection_observations ORDER BY rowid",
                "SELECT * FROM mail_connection_operations ORDER BY rowid",
                "SELECT request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms FROM imap_setup_requests ORDER BY request_id",
                "SELECT request_id,seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version FROM imap_setup_events ORDER BY request_id,seq",
            ];
            queries
                .iter()
                .map(|query| {
                    let mut statement = connection.prepare(query)?;
                    let columns = statement.column_count();
                    let rows = statement.query_map([], |row| {
                        (0..columns)
                            .map(|index| row.get(index))
                            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                    })?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        };
        let before = fixture.keyed().with_connection(snapshot).unwrap();
        let store = Store {
            connection: fixture.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(5, 6, SCHEMA_V6, |connection| {
                connection.execute_batch(MIGRATION_V5_TO_V6)?;
                connection.execute_batch("CREATE TABLE unwanted_v6(x)")?;
                Ok(())
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            5
        );
        assert!(fixture.0.join("data/backup-before-v5.db").is_file());
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='unwanted_v6'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row("SELECT count(*) FROM pragma_table_info('imap_setup_requests') WHERE name='resume_of_request_id'", [], |r| r.get::<_, i64>(0)))
                .unwrap(),
            0
        );
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V5, 5))
            .unwrap();
        assert_eq!(before, store.with_connection(snapshot).unwrap());
    }
    fn seed_v6_fixture(fixture: &Fixture) {
        fixture.seed_v5();
        let store = Store {
            connection: fixture.keyed(),
        };
        store
            .migrate_with_backup(5, 6, SCHEMA_V6, |c| c.execute_batch(MIGRATION_V5_TO_V6))
            .unwrap();
    }
    fn v6_rows(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
        let mut result = Vec::new();
        for table in [
            "settings",
            "accounts",
            "server_identity",
            "client_registration_requests",
            "client_registrations",
            "client_registration_accounts",
            "client_registration_tools",
            "audit_capacity",
            "audit_starts",
            "audit_start_accounts",
            "audit_terminals",
            "mail_connection_settings",
            "mail_connection_observations",
            "mail_connection_operations",
            "imap_setup_requests",
            "imap_setup_events",
            "sqlite_sequence",
        ] {
            let mut statement = c.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let columns = statement.column_count();
            result.push(
                statement
                    .query_map([], |r| {
                        (0..columns)
                            .map(|i| r.get(i))
                            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        Ok(result)
    }
    #[test]
    fn cli15_v6_migrates_to_v7_without_changing_existing_rows() {
        let f = Fixture::new();
        seed_v6_fixture(&f);
        let before = f.keyed().with_connection(v6_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v6_rows).unwrap());
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM client_scope_refreshes",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        assert!(f.0.join("data/backup-before-v6.db").is_file());
    }
    #[test]
    fn cli15_v6_backup_collision_preserves_original_and_artifact() {
        let f = Fixture::new();
        seed_v6_fixture(&f);
        let database = f.0.join("data/mail-hantong.db");
        let before = fs::read(&database).unwrap();
        let backup = f.0.join("data/backup-before-v6.db");
        fs::write(&backup, b"keep-existing-v6").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(&database).unwrap(), before);
        assert_eq!(fs::read(backup).unwrap(), b"keep-existing-v6");
    }
    #[test]
    fn cli15_v7_failed_validation_rolls_back_real_ddl_and_all_v6_rows() {
        let f = Fixture::new();
        seed_v6_fixture(&f);
        let before = f.keyed().with_connection(v6_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(6, 7, SCHEMA_V7, |c| {
                c.execute_batch(MIGRATION_V6_TO_V7)?;
                c.execute_batch("CREATE TABLE unexpected_refresh(x)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v6_rows).unwrap());
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V6, 6))
            .unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            6
        );
        assert!(f.0.join("data/backup-before-v6.db").is_file());
    }
    #[test]
    fn cli15_corrupt_v6_history_is_rejected_before_backup() {
        let f = Fixture::new();
        seed_v6_fixture(&f);
        f.keyed()
            .with_connection(|c| {
                c.execute(
                    "UPDATE imap_setup_requests SET payload_hash=?1 WHERE request_id=?2",
                    rusqlite::params![vec![31u8; 32], vec![27u8; 32]],
                )
            })
            .unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::InternalError)));
        assert!(!f.0.join("data/backup-before-v6.db").exists());
    }
    fn seed_v8_scope_fixture(f: &Fixture) {
        seed_v7_catalog_fixture(f);
        let store = Store {
            connection: f.keyed(),
        };
        store
            .migrate_with_backup(
                7,
                8,
                include_str!("../../../docs/contracts/migrations/schema-v8.sql"),
                |c| c.execute_batch(MIGRATION_V7_TO_V8),
            )
            .unwrap();
    }
    include!("migration_v13_tests.rs");
    include!("migration_v14_tests.rs");
    fn seed_v11_fixture(f: &Fixture) {
        seed_v10_fixture(f);
        let store = Store {
            connection: f.keyed(),
        };
        store
            .migrate_with_backup(
                10,
                11,
                include_str!("../../../docs/contracts/migrations/schema-v11.sql"),
                |c| c.execute_batch(MIGRATION_V10_TO_V11),
            )
            .unwrap();
        use hantong_domain::{
            imap_sync::{
                ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage,
                ImapUidPageRequest,
            },
            mailboxes::{ImapMailboxName, MailboxEncoding},
        };
        let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap();
        let page = ImapUidPage::new(
            ImapUidPageRequest::new("v5-account", 2, mailbox, None, 0).unwrap(),
            9,
            2,
            vec![
                ImapMessageMetadata::new(
                    1,
                    vec![],
                    0,
                    0,
                    ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let raw = page.to_storage_json().unwrap();
        let metadata = page.messages()[0].to_storage_json().unwrap();
        let mut payload = b"mail-hantong.imap-uid-page-payload.v1\0".to_vec();
        for value in [2u64, 1, 0] {
            payload.extend_from_slice(&value.to_be_bytes());
        }
        payload.extend_from_slice(&[0; 32]);
        payload.extend_from_slice(&(raw.len() as u64).to_be_bytes());
        payload.extend_from_slice(&raw);
        let sha = |bytes: &[u8]| hantong_platform::sha256(bytes).unwrap();
        store.with_connection(|c| {
            c.execute("INSERT INTO imap_uid_page_operations VALUES('v11-page','v5-account','v11-key','494e424f58','modified_utf7',0,1,2,1,2,zeroblob(32),?1,?2,?3,9,2,0,1,1,12)", rusqlite::params![std::str::from_utf8(&raw).unwrap(), sha(&raw).as_slice(), sha(&payload).as_slice()])?;
            c.execute("INSERT INTO imap_uid_cursors VALUES('v5-account','494e424f58','modified_utf7',1,2,1,2,zeroblob(32),9,2,1,12,'v11-page')", [])?;
            c.execute("INSERT INTO imap_uid_messages VALUES('v5-account','494e424f58','modified_utf7',9,1,?1,?2,12,'v11-page')", rusqlite::params![std::str::from_utf8(&metadata).unwrap(),sha(&metadata).as_slice()])?;
            validate_schema(c,include_str!("../../../docs/contracts/migrations/schema-v11.sql"),11)
        }).unwrap();
    }
    fn v11_rows(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
        let mut rows = v10_rows(c)?;
        for table in [
            "imap_uid_page_operations",
            "imap_uid_cursors",
            "imap_uid_messages",
        ] {
            let mut query = c.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let n = query.column_count();
            rows.push(
                query
                    .query_map([], |r| {
                        (0..n)
                            .map(|i| r.get(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        Ok(rows)
    }
    #[test]
    fn ext04_v11_to_v12_preserves_uid_rows_folder_job_and_audit_sequence() {
        let f = Fixture::new();
        seed_v11_fixture(&f);
        let before = f.keyed().with_connection(v11_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v11_rows).unwrap());
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM imap_uid_page_requests",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        let mailbox = hantong_domain::mailboxes::ImapMailboxName::from_wire(
            b"INBOX",
            hantong_domain::mailboxes::MailboxEncoding::ModifiedUtf7,
        )
        .unwrap();
        assert_eq!(
            store
                .load_imap_uid_metadata("v5-account", &mailbox, 9, 1)
                .unwrap()
                .unwrap()
                .uid(),
            1
        );
        assert!(
            store
                .load_audit([41; 32])
                .unwrap()
                .unwrap()
                .completion()
                .is_none()
        );
        assert!(f.0.join("data/backup-before-v11.db").is_file());
    }
    #[test]
    fn ext04_v11_to_v12_backup_collision_preserves_original() {
        let f = Fixture::new();
        seed_v11_fixture(&f);
        let db = f.0.join("data/mail-hantong.db");
        let before = fs::read(&db).unwrap();
        let backup = f.0.join("data/backup-before-v11.db");
        fs::write(&backup, b"preserve-v11").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(before, fs::read(db).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"preserve-v11");
    }
    fn seed_v10_fixture(f: &Fixture) {
        seed_v9_fixture(f);
        let store = Store {
            connection: f.keyed(),
        };
        store
            .migrate_with_backup(
                9,
                10,
                include_str!("../../../docs/contracts/migrations/schema-v10.sql"),
                |c| c.execute_batch(MIGRATION_V9_TO_V10),
            )
            .unwrap();
        let request = hantong_contracts::folders::FolderRefreshRequest::new(
            "v5-account",
            "v10-pending",
            "v10-key",
            0,
            &"00".repeat(32),
        )
        .unwrap();
        let intent = hantong_platform::sha256(&request.intent_bytes()).unwrap();
        store.with_connection(|c| {
            c.execute("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) VALUES(?1,?2,1,'cli','v5-fixture','mail.folders.refresh',8)",rusqlite::params![vec![41u8;32],vec![22u8;16]])?;
            c.execute("INSERT INTO audit_start_accounts VALUES(?1,'v5-account')",[vec![41u8;32]])?;
            c.execute("UPDATE audit_capacity SET retained_executions=retained_executions+1,retained_account_refs=retained_account_refs+1 WHERE singleton=1",[])?;
            c.execute("INSERT INTO folder_refresh_requests(execution_id,operation_id,account_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms) VALUES(?1,'v10-pending','v5-account','v10-key',?2,0,2,2,zeroblob(32),'admitted',8)",rusqlite::params![vec![41u8;32],intent.as_slice()])?;
            validate_schema(c,include_str!("../../../docs/contracts/migrations/schema-v10.sql"),10)
        }).unwrap();
    }
    #[test]
    fn ext04_v11_to_v12_real_ddl_failure_rolls_back_rows_and_schema() {
        let f = Fixture::new();
        seed_v11_fixture(&f);
        let before = f.keyed().with_connection(v11_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(11, 12, SCHEMA_V12, |c| {
                c.execute_batch(MIGRATION_V11_TO_V12)?;
                c.execute_batch("CREATE TABLE unexpected_uid_job_schema(value)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v11_rows).unwrap());
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V11, 11))
            .unwrap();
        assert!(f.0.join("data/backup-before-v11.db").is_file());
    }
    fn v10_rows(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
        let mut rows = v9_rows(c)?;
        for table in [
            "folder_refresh_requests",
            "imap_mailbox_catalogs",
            "imap_mailbox_catalog_operations",
        ] {
            let mut q = c.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let columns = q.column_count();
            rows.push(
                q.query_map([], |r| {
                    (0..columns)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        Ok(rows)
    }
    #[test]
    fn ext04_v10_to_v11_preserves_folder_job_and_all_existing_rows() {
        let f = Fixture::new();
        seed_v10_fixture(&f);
        let before = f.keyed().with_connection(v10_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v10_rows).unwrap());
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(store.with_connection(|c|c.query_row("SELECT (SELECT count(*) FROM imap_uid_cursors)+(SELECT count(*) FROM imap_uid_messages)+(SELECT count(*) FROM imap_uid_page_operations)",[],|r|r.get::<_,i64>(0))).unwrap(),0);
        assert!(
            store
                .load_audit([41; 32])
                .unwrap()
                .unwrap()
                .completion()
                .is_none()
        );
        assert!(f.0.join("data/backup-before-v10.db").is_file());
    }
    #[test]
    fn ext04_v10_to_v11_backup_collision_preserves_source_bytes() {
        let f = Fixture::new();
        seed_v10_fixture(&f);
        let path = f.0.join("data/mail-hantong.db");
        let before = fs::read(&path).unwrap();
        let backup = f.0.join("data/backup-before-v10.db");
        fs::write(&backup, b"preserve-v10").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(before, fs::read(path).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"preserve-v10");
    }
    #[test]
    fn ext04_v10_to_v11_actual_ddl_failure_rolls_back_preserving_v10() {
        let f = Fixture::new();
        seed_v10_fixture(&f);
        let before = f.keyed().with_connection(v10_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(10, 11, SCHEMA_V11, |c| {
                c.execute_batch(include_str!(
                    "../../../docs/contracts/migrations/010-to-011.sql"
                ))?;
                c.execute_batch("CREATE TABLE unexpected_uid_schema(value)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v10_rows).unwrap());
        store
            .with_connection(|c| {
                validate_schema(
                    c,
                    include_str!("../../../docs/contracts/migrations/schema-v10.sql"),
                    10,
                )
            })
            .unwrap();
        assert!(f.0.join("data/backup-before-v10.db").is_file());
    }

    fn seed_v9_fixture(fixture: &Fixture) {
        seed_v8_scope_fixture(fixture);
        let store = Store {
            connection: fixture.keyed(),
        };
        store
            .migrate_with_backup(8, 9, SCHEMA_V9, |c| c.execute_batch(MIGRATION_V8_TO_V9))
            .unwrap();
    }
    fn v9_rows(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
        let mut rows = v7_catalog_rows(c)?;
        for table in [
            "audit_starts",
            "audit_start_accounts",
            "audit_terminals",
            "sqlite_sequence",
        ] {
            let mut statement = c.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let columns = statement.column_count();
            rows.push(
                statement
                    .query_map([], |r| {
                        (0..columns)
                            .map(|i| r.get(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        Ok(rows)
    }
    #[test]
    fn ext06_v9_to_v10_preserves_scope_audit_rows_and_unresolved_state() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        let before = f.keyed().with_connection(v9_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v9_rows).unwrap());
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM folder_refresh_requests",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
        store.with_connection(|c| {
            let execution: Vec<u8> = c.query_row("SELECT execution_id FROM audit_starts LIMIT 1", [], |r| r.get(0))?;
            assert!(c.execute("INSERT INTO folder_refresh_requests(execution_id,operation_id,account_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_generation,catalog_operation_id) VALUES(?1,'null-success','v5-account','null-key',zeroblob(32),0,1,1,zeroblob(32),'succeeded',1,1,1,1,NULL)", rusqlite::params![execution]).is_err());
            Ok(())
        }).unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert!(f.0.join("data/backup-before-v9.db").is_file());
    }
    #[test]
    fn ext06_v9_to_v10_never_used_audit_preserves_absent_sequence() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        f.keyed().with_connection(|c|c.execute_batch("DELETE FROM audit_terminals;DELETE FROM audit_start_accounts;DELETE FROM audit_starts;UPDATE audit_capacity SET retained_executions=0,retained_account_refs=0;DELETE FROM sqlite_sequence WHERE name='audit_starts';")).unwrap();
        let store = f.open().unwrap();
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT count(*) FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
    }
    #[test]
    fn ext06_v9_to_v10_preserves_empty_audit_sequence_highwater() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        f.keyed()
            .with_connection(|c| {
                c.execute("DELETE FROM audit_terminals", [])?;
                c.execute("DELETE FROM audit_start_accounts", [])?;
                c.execute("DELETE FROM audit_starts", [])?;
                c.execute(
                    "UPDATE audit_capacity SET retained_executions=0,retained_account_refs=0",
                    [],
                )?;
                c.execute(
                    "UPDATE sqlite_sequence SET seq=777 WHERE name='audit_starts'",
                    [],
                )
            })
            .unwrap();
        let store = f.open().unwrap();
        assert_eq!(
            store
                .with_connection(
                    |c| c.query_row("SELECT count(*) FROM audit_starts", [], |r| r
                        .get::<_, i64>(0))
                )
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            777
        );
    }
    #[test]
    fn ext06_v9_to_v10_backup_collision_preserves_source() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        let database = f.0.join("data/mail-hantong.db");
        let before = fs::read(&database).unwrap();
        let backup = f.0.join("data/backup-before-v9.db");
        fs::write(&backup, b"keep-existing-v9").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(fs::read(&database).unwrap(), before);
        assert_eq!(fs::read(&backup).unwrap(), b"keep-existing-v9");
    }
    #[test]
    fn ext06_v9_to_v10_real_ddl_failure_rolls_back_schema_and_rows() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        let before = f.keyed().with_connection(v9_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(9, 10, SCHEMA_V10, |c| {
                c.execute_batch(MIGRATION_V9_TO_V10)?;
                c.execute_batch("CREATE TABLE unexpected_v10(value)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v9_rows).unwrap());
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V9, 9))
            .unwrap();
    }
    #[test]
    fn ext06_v9_to_v10_deferred_foreign_key_failure_rolls_back() {
        let f = Fixture::new();
        seed_v9_fixture(&f);
        let before = f.keyed().with_connection(v9_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        let result = store.migrate_with_backup(9, 10, SCHEMA_V10, |c| {
            c.execute_batch(MIGRATION_V9_TO_V10)?;
            c.execute_batch("PRAGMA defer_foreign_keys=ON; INSERT INTO folder_refresh_requests(execution_id,operation_id,account_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms) VALUES(x'0101010101010101010101010101010101010101010101010101010101010101','bad-operation','missing-account','bad-key',zeroblob(32),0,1,1,zeroblob(32),'admitted',1);")?;
            assert_eq!(c.query_row("SELECT count(*) FROM folder_refresh_requests",[],|r|r.get::<_,i64>(0))?,1);
            Ok(())
        });
        assert_eq!(result, Err(ErrorCode::InternalError));
        assert_eq!(before, store.with_connection(v9_rows).unwrap());
        store
            .with_connection(|c| validate_schema(c, SCHEMA_V9, 9))
            .unwrap();
    }
    #[test]
    fn cli15_v8_scope_migration_preserves_legacy_columns_and_adds_explicit_tools() {
        let f = Fixture::new();
        seed_v8_scope_fixture(&f);
        let before = f.keyed().with_connection(v7_catalog_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v7_catalog_rows).unwrap());
        store.with_connection(|c| {
            assert_eq!(c.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0))?,i64::from(CURRENT_SCHEMA_VERSION));
            assert_eq!(c.query_row("SELECT count(*) FROM pragma_table_info('client_scope_refreshes') WHERE name IN ('hash_version','base_tools_json','target_tools_json')",[],|r|r.get::<_,i64>(0))?,3);
            Ok(())
        }).unwrap();
        assert!(f.0.join("data/backup-before-v8.db").is_file());
    }
    #[test]
    fn cli15_v8_scope_backup_collision_preserves_source_bytes() {
        let f = Fixture::new();
        seed_v8_scope_fixture(&f);
        let path = f.0.join("data/mail-hantong.db");
        let before = fs::read(&path).unwrap();
        let backup = f.0.join("data/backup-before-v8.db");
        fs::write(&backup, b"preserve").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(before, fs::read(path).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"preserve");
    }
    #[test]
    fn cli15_v8_scope_real_ddl_failure_rolls_back_original_schema_and_values() {
        let f = Fixture::new();
        seed_v8_scope_fixture(&f);
        let before = f.keyed().with_connection(v7_catalog_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(8, 9, SCHEMA_V9, |c| {
                c.execute_batch(include_str!(
                    "../../../docs/contracts/migrations/008-to-009.sql"
                ))?;
                c.execute_batch("CREATE TABLE unexpected_scope_v9(value)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v7_catalog_rows).unwrap());
        store
            .with_connection(|c| {
                validate_schema(
                    c,
                    include_str!("../../../docs/contracts/migrations/schema-v8.sql"),
                    8,
                )
            })
            .unwrap();
        assert!(f.0.join("data/backup-before-v8.db").is_file());
    }
    fn seed_v7_catalog_fixture(fixture: &Fixture) {
        seed_v6_fixture(fixture);
        let store = Store {
            connection: fixture.keyed(),
        };
        store
            .migrate_with_backup(
                6,
                7,
                include_str!("../../../docs/contracts/migrations/schema-v7.sql"),
                |c| c.execute_batch(MIGRATION_V6_TO_V7),
            )
            .unwrap();
    }
    fn v7_catalog_rows(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
        let mut rows = v6_rows(c)?;
        let mut query = c.prepare("SELECT refresh_id,registration_id,base_registration_version,base_profile_epoch,target_profile_epoch,base_accounts_json,target_accounts_json,client_public_key_hash,server_public_key_hash,payload_hash,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms,decided_at_utc_ms,activated_at_utc_ms FROM client_scope_refreshes ORDER BY rowid")?;
        let columns = query.column_count();
        rows.push(
            query
                .query_map([], |r| {
                    (0..columns)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
        Ok(rows)
    }
    #[test]
    fn ext06_v7_catalog_migration_preserves_existing_rows_and_adds_empty_catalogs() {
        let f = Fixture::new();
        seed_v7_catalog_fixture(&f);
        let before = f.keyed().with_connection(v7_catalog_rows).unwrap();
        let store = f.open().unwrap();
        assert_eq!(before, store.with_connection(v7_catalog_rows).unwrap());
        assert_eq!(
            store
                .with_connection(
                    |c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                )
                .unwrap(),
            i64::from(CURRENT_SCHEMA_VERSION)
        );
        assert_eq!(store.with_connection(|c|c.query_row("SELECT (SELECT count(*) FROM imap_mailbox_catalogs)+(SELECT count(*) FROM imap_mailbox_catalog_operations)",[],|r|r.get::<_,i64>(0))).unwrap(),0);
        assert!(f.0.join("data/backup-before-v7.db").is_file());
    }
    #[test]
    fn ext06_v7_catalog_backup_collision_preserves_original() {
        let f = Fixture::new();
        seed_v7_catalog_fixture(&f);
        let database = f.0.join("data/mail-hantong.db");
        let before = fs::read(&database).unwrap();
        let backup = f.0.join("data/backup-before-v7.db");
        fs::write(&backup, b"keep-existing-catalog-backup").unwrap();
        assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
        assert_eq!(before, fs::read(database).unwrap());
        assert_eq!(fs::read(backup).unwrap(), b"keep-existing-catalog-backup");
    }
    #[test]
    fn ext06_v7_catalog_failed_real_ddl_rolls_back_every_existing_row() {
        let f = Fixture::new();
        seed_v7_catalog_fixture(&f);
        let before = f.keyed().with_connection(v7_catalog_rows).unwrap();
        let store = Store {
            connection: f.keyed(),
        };
        assert_eq!(
            store.migrate_with_backup(7, 8, SCHEMA_V8, |c| {
                c.execute_batch(include_str!(
                    "../../../docs/contracts/migrations/007-to-008.sql"
                ))?;
                c.execute_batch("CREATE TABLE unexpected_catalog(x)")
            }),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, store.with_connection(v7_catalog_rows).unwrap());
        store
            .with_connection(|c| {
                validate_schema(
                    c,
                    include_str!("../../../docs/contracts/migrations/schema-v7.sql"),
                    7,
                )
            })
            .unwrap();
        assert!(f.0.join("data/backup-before-v7.db").is_file());
    }
    #[test]
    fn ext06_catalog_schema_rejects_unicode_control_and_nul_identifiers() {
        let f = Fixture::new();
        seed_v7_catalog_fixture(&f);
        let store = f.open().unwrap();
        store.with_connection(|c| {
            let insert=|id:&str,key:&str|c.execute("INSERT INTO imap_mailbox_catalog_operations(operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms) VALUES(?1,'v5-account',?2,2,2,?3,?3,0,1,?3,1)",rusqlite::params![id,key,vec![1u8;32]]);
            for invalid in ["", "한글", "space name", "line\n", "nul\0tail"] {
                assert!(insert(invalid,"valid-key").is_err());
                assert!(insert("valid-id",invalid).is_err());
            }
            assert_eq!(insert("valid-id","valid-key")?,1);
            Ok(())
        }).unwrap();
    }
}
