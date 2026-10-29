use crate::Store;
use hantong_contracts::ErrorCode;
use hantong_domain::config::{Config, ConfigError};

const CONFIG_KEY: &str = "application.config.v1";

impl Store {
    pub fn load_config(&self) -> Result<Option<Config>, ErrorCode> {
        let Some(row) = self.get_setting(CONFIG_KEY)? else {
            return Ok(None);
        };
        let config =
            Config::from_json(row.value_json.as_bytes()).map_err(|_| ErrorCode::InternalError)?;
        if config.settings_version != row.version {
            return Err(ErrorCode::InternalError);
        }
        Ok(Some(config))
    }

    /// Creates only the safe defaults. Existing settings are never overwritten.
    pub fn initialize_config(&self, timezone_id: &str) -> Result<Config, ErrorCode> {
        if let Some(config) = self.load_config()? {
            return Ok(config);
        }
        let config = Config::defaults(timezone_id).map_err(map_input_error)?;
        let json = serde_json::to_string(&config).map_err(|_| ErrorCode::InternalError)?;
        self.put_setting(CONFIG_KEY, &json, 0)?;
        Ok(config)
    }

    /// Ordinary settings updates cannot change protected policy. Caller/session
    /// authorization belongs to the common application service before this call.
    pub fn update_config(&self, input: &[u8], expected_version: u64) -> Result<Config, ErrorCode> {
        let candidate = Config::from_json(input).map_err(map_input_error)?;
        let previous = self.load_config()?.ok_or(ErrorCode::Conflict)?;
        let accepted = candidate
            .validate_unprivileged_replacement(&previous, expected_version)
            .map_err(map_input_error)?;
        let json = serde_json::to_string(&accepted).map_err(|_| ErrorCode::InternalError)?;
        self.put_setting(CONFIG_KEY, &json, expected_version)?;
        Ok(accepted)
    }
}

fn map_input_error(error: ConfigError) -> ErrorCode {
    match error {
        ConfigError::StaleVersion | ConfigError::VersionExhausted => ErrorCode::Conflict,
        ConfigError::SecurityProtected => ErrorCode::PolicyBlocked,
        _ => ErrorCode::InvalidInput,
    }
}
