use hantong_contracts::ErrorCode;
use serde_json::{Map, Value};

const MAX_METRIC: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SnapshotSection {
    Sync,
    RecentWork,
    Errors,
}
impl SnapshotSection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::RecentWork => "recent_work",
            Self::Errors => "errors",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticSelection {
    sections: Vec<SnapshotSection>,
}
impl DiagnosticSelection {
    pub fn try_from_sections(sections: &[SnapshotSection]) -> Result<Self, SnapshotError> {
        if sections.is_empty() {
            return Err(SnapshotError::EmptySelection);
        }
        let mut ordered = sections.to_vec();
        ordered.sort_unstable();
        ordered.dedup();
        if ordered.len() != sections.len() {
            return Err(SnapshotError::DuplicateSelection);
        }
        Ok(Self { sections: ordered })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    NegativeTimestamp,
    MetricOverflow,
    EmptySelection,
    DuplicateSelection,
    Serialization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticSnapshot {
    generated_at_utc_ms: i64,
    selection: DiagnosticSelection,
    error_count: Option<u64>,
    last_error: Option<ErrorCode>,
}
impl DiagnosticSnapshot {
    pub fn new(
        generated_at_utc_ms: i64,
        selection: DiagnosticSelection,
    ) -> Result<Self, SnapshotError> {
        if generated_at_utc_ms < 0 {
            return Err(SnapshotError::NegativeTimestamp);
        }
        Ok(Self {
            generated_at_utc_ms,
            selection,
            error_count: None,
            last_error: None,
        })
    }
    pub fn with_error_summary(
        mut self,
        count: u64,
        last_error: Option<ErrorCode>,
    ) -> Result<Self, SnapshotError> {
        if count > MAX_METRIC {
            return Err(SnapshotError::MetricOverflow);
        }
        self.error_count = Some(count);
        self.last_error = last_error;
        Ok(self)
    }
    pub fn preview_json(&self) -> Result<String, SnapshotError> {
        self.to_json()
    }
    pub fn export_json(&self) -> Result<String, SnapshotError> {
        self.to_json()
    }
    fn to_json(&self) -> Result<String, SnapshotError> {
        let mut root = Map::new();
        root.insert("schema_version".into(), Value::from(1u8));
        root.insert(
            "generated_at_utc_ms".into(),
            Value::from(self.generated_at_utc_ms),
        );
        let mut selected = Vec::with_capacity(self.selection.sections.len());
        for section in &self.selection.sections {
            selected.push(Value::from(section.as_str()));
            match section {
                SnapshotSection::Sync => {
                    root.insert("sync".into(), Value::from("unavailable"));
                }
                SnapshotSection::RecentWork => {
                    root.insert("recent_work".into(), Value::from("unavailable"));
                }
                SnapshotSection::Errors => {
                    let mut errors = Map::new();
                    errors.insert(
                        "count".into(),
                        self.error_count.map_or(Value::Null, Value::from),
                    );
                    errors.insert(
                        "last_error_code".into(),
                        self.last_error
                            .map_or(Value::Null, |code| Value::from(code.as_str())),
                    );
                    root.insert("errors".into(), Value::Object(errors));
                }
            }
        }
        root.insert("selected_sections".into(), Value::Array(selected));
        serde_json::to_string(&Value::Object(root)).map_err(|_| SnapshotError::Serialization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn ops05_sec08_allowlist_and_preview_export_match() {
        let selection = DiagnosticSelection::try_from_sections(&[
            SnapshotSection::Errors,
            SnapshotSection::Sync,
        ])
        .unwrap();
        let snapshot = DiagnosticSnapshot::new(1_700_000_000_000, selection)
            .unwrap()
            .with_error_summary(3, Some(ErrorCode::Offline))
            .unwrap();
        let preview = snapshot.preview_json().unwrap();
        assert_eq!(preview, snapshot.export_json().unwrap());
        let object = serde_json::from_str::<Value>(&preview).unwrap();
        assert_eq!(object["sync"], "unavailable");
        assert_eq!(object["errors"]["last_error_code"], "OFFLINE");
        assert!(object.get("recent_work").is_none());
        assert!(
            !preview.contains('@') && !preview.contains("token") && !preview.contains("subject")
        );
    }
    #[test]
    fn ops05_rejects_empty_duplicate_and_unbounded_values() {
        assert_eq!(
            DiagnosticSelection::try_from_sections(&[]),
            Err(SnapshotError::EmptySelection)
        );
        assert_eq!(
            DiagnosticSelection::try_from_sections(&[SnapshotSection::Sync, SnapshotSection::Sync]),
            Err(SnapshotError::DuplicateSelection)
        );
        let selection = DiagnosticSelection::try_from_sections(&[SnapshotSection::Errors]).unwrap();
        assert_eq!(
            DiagnosticSnapshot::new(-1, selection.clone()),
            Err(SnapshotError::NegativeTimestamp)
        );
        assert_eq!(
            DiagnosticSnapshot::new(0, selection)
                .unwrap()
                .with_error_summary(i64::MAX as u64 + 1, None),
            Err(SnapshotError::MetricOverflow)
        );
    }
    #[test]
    fn ops05_selection_serialization_is_deterministic() {
        let a = DiagnosticSelection::try_from_sections(&[
            SnapshotSection::RecentWork,
            SnapshotSection::Sync,
        ])
        .unwrap();
        let b = DiagnosticSelection::try_from_sections(&[
            SnapshotSection::Sync,
            SnapshotSection::RecentWork,
        ])
        .unwrap();
        assert_eq!(
            DiagnosticSnapshot::new(7, a)
                .unwrap()
                .preview_json()
                .unwrap(),
            DiagnosticSnapshot::new(7, b)
                .unwrap()
                .preview_json()
                .unwrap()
        );
    }
}
