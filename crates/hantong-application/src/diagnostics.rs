use hantong_contracts::ErrorCode;
use serde_json::{Map, Value};
use std::fmt;

const MAX_METRIC: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    CoreTransportReady,
    CoreStopping,
    CoreStopped,
    OperationCompleted,
    OperationFailed,
}
impl EventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CoreTransportReady => "core_transport_ready",
            Self::CoreStopping => "core_stopping",
            Self::CoreStopped => "core_stopped",
            Self::OperationCompleted => "operation_completed",
            Self::OperationFailed => "operation_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}
impl Severity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogError {
    NegativeTimestamp,
    MetricOverflow,
    Serialization,
}
impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NegativeTimestamp => "diagnostic timestamp is negative",
            Self::MetricOverflow => "diagnostic metric is out of range",
            Self::Serialization => "diagnostic serialization failed",
        })
    }
}
impl std::error::Error for LogError {}

/// Closed record: no free-form text, path, account, address, subject, token,
/// or body can be put into this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticEvent {
    kind: EventKind,
    severity: Severity,
    observed_at_utc_ms: i64,
    error_code: Option<ErrorCode>,
    count: Option<u64>,
    duration_ms: Option<u64>,
}
impl DiagnosticEvent {
    pub fn new(
        kind: EventKind,
        severity: Severity,
        observed_at_utc_ms: i64,
    ) -> Result<Self, LogError> {
        if observed_at_utc_ms < 0 {
            return Err(LogError::NegativeTimestamp);
        };
        Ok(Self {
            kind,
            severity,
            observed_at_utc_ms,
            error_code: None,
            count: None,
            duration_ms: None,
        })
    }
    pub fn with_error(mut self, code: ErrorCode) -> Self {
        self.error_code = Some(code);
        self
    }
    pub fn with_metrics(
        mut self,
        count: Option<u64>,
        duration_ms: Option<u64>,
    ) -> Result<Self, LogError> {
        if count.is_some_and(|v| v > MAX_METRIC) || duration_ms.is_some_and(|v| v > MAX_METRIC) {
            return Err(LogError::MetricOverflow);
        };
        self.count = count;
        self.duration_ms = duration_ms;
        Ok(self)
    }
    pub fn to_json_line(self) -> Result<String, LogError> {
        let mut o = Map::new();
        o.insert("schema_version".into(), Value::from(1u8));
        o.insert("event".into(), Value::from(self.kind.as_str()));
        o.insert("severity".into(), Value::from(self.severity.as_str()));
        o.insert(
            "observed_at_utc_ms".into(),
            Value::from(self.observed_at_utc_ms),
        );
        if let Some(c) = self.error_code {
            o.insert("error_code".into(), Value::from(c.as_str()));
        }
        if let Some(v) = self.count {
            o.insert("count".into(), Value::from(v));
        }
        if let Some(v) = self.duration_ms {
            o.insert("duration_ms".into(), Value::from(v));
        }
        serde_json::to_string(&Value::Object(o))
            .map(|mut s| {
                s.push('\n');
                s
            })
            .map_err(|_| LogError::Serialization)
    }
}

pub fn redact_sensitive(_: &str) -> &'static str {
    "<redacted>"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn sec08_allowlisted_one_line_record() {
        let s = DiagnosticEvent::new(
            EventKind::OperationFailed,
            Severity::Error,
            1_700_000_000_000,
        )
        .unwrap()
        .with_error(ErrorCode::InternalError)
        .with_metrics(Some(4), Some(12))
        .unwrap()
        .to_json_line()
        .unwrap();
        assert!(s.ends_with('\n'));
        let v: Value = serde_json::from_str(s.trim_end()).unwrap();
        let k = v
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            k,
            [
                "count",
                "duration_ms",
                "error_code",
                "event",
                "observed_at_utc_ms",
                "schema_version",
                "severity"
            ]
            .into_iter()
            .collect()
        );
        assert!(!s.contains("secret"));
    }
    #[test]
    fn sec08_bounds() {
        assert_eq!(
            DiagnosticEvent::new(EventKind::CoreStopped, Severity::Info, -1),
            Err(LogError::NegativeTimestamp)
        );
        let e = DiagnosticEvent::new(EventKind::CoreStopped, Severity::Info, 0).unwrap();
        assert_eq!(
            e.with_metrics(Some(i64::MAX as u64 + 1), None),
            Err(LogError::MetricOverflow)
        );
    }
    #[test]
    fn sec08_redacts_untrusted_text() {
        for s in [
            "Bearer token-secret",
            "To: person@example.com",
            "Subject: confidential",
            "x\n{\"event\":\"forged\"}",
        ] {
            assert_eq!(redact_sensitive(s), "<redacted>");
        }
    }
    #[test]
    fn ai10_uses_stable_error_code() {
        let s = DiagnosticEvent::new(EventKind::OperationCompleted, Severity::Info, 10)
            .unwrap()
            .with_error(ErrorCode::Offline)
            .to_json_line()
            .unwrap();
        assert!(s.contains("OFFLINE"));
        assert!(!s.contains("success"));
    }
}
