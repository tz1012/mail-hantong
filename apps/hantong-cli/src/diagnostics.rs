//! One explicitly selected, local diagnostic snapshot; no file writes or upload.
use crate::{ErrorCode, Lang, emit_error, resource};
use hantong_contracts::{
    diagnostic_aggregate::{DIAGNOSTIC_ERRORS, DiagnosticSection},
    diagnostic_snapshot::DiagnosticSnapshotRequest,
};

fn number(value: &str) -> Result<u32, ErrorCode> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|v| v.is_ascii_digit())
    {
        return Err(ErrorCode::InvalidInput);
    }
    value.parse().map_err(|_| ErrorCode::InvalidInput)
}

fn parse(
    command: &[String],
    options: &[(String, String)],
    now: i64,
) -> Result<DiagnosticSnapshotRequest, ErrorCode> {
    if !matches!(command,[a,b] if a=="diagnostics" && b=="snapshot") {
        return Err(ErrorCode::InvalidInput);
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in options {
        if !matches!(key.as_str(), "--sections" | "--hours" | "--limit")
            || values.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let sections = values
        .get("--sections")
        .ok_or(ErrorCode::InvalidInput)?
        .split(',')
        .map(DiagnosticSection::parse)
        .collect::<Result<Vec<_>, _>>()?;
    let hours = values
        .get("--hours")
        .map(|v| number(v))
        .transpose()?
        .unwrap_or(24);
    if !(1..=744).contains(&hours) {
        return Err(ErrorCode::InvalidInput);
    }
    let limit = values
        .get("--limit")
        .map(|v| number(v))
        .transpose()?
        .unwrap_or(100);
    let from = now
        .checked_sub(i64::from(hours) * 3_600_000)
        .ok_or(ErrorCode::InvalidInput)?;
    DiagnosticSnapshotRequest::new(from, now, limit, &sections)
}

pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|v| i64::try_from(v.as_millis()).ok());
    let Some(now) = now else {
        return emit_error(ErrorCode::InternalError, lang, json);
    };
    let request = match parse(command, options, now) {
        Ok(v) => v,
        Err(e) => return emit_error(e, lang, json),
    };
    let lang = resolve_lang();
    match hantong_ipc_client::diagnostic_snapshot::snapshot_default(request) {
        Ok(value) => {
            if json {
                println!("{}", value.snapshot_json());
            } else {
                print!("{}", human(value.aggregate(), lang));
            }
            0
        }
        Err(e) => emit_error(e, lang, json),
    }
}

fn human(
    value: &hantong_contracts::diagnostic_aggregate::DiagnosticAggregate,
    lang: Lang,
) -> String {
    let window = value.window();
    let mut out = format!(
        "{}\n{}: {}\n{}: {}\n",
        resource(lang, "diagnostics_title"),
        resource(lang, "diagnostics_executions"),
        window.included_executions(),
        resource(lang, "diagnostics_truncated"),
        resource(
            lang,
            if window.truncated() {
                "diagnostics_yes"
            } else {
                "diagnostics_no"
            }
        )
    );
    if let Some(sync) = value.sync() {
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "diagnostics_sync"),
            sync.count()
        ));
    }
    if let Some(recent) = value.recent_work() {
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "diagnostics_recent"),
            recent.count()
        ));
    }
    if let Some(errors) = value.errors() {
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "diagnostics_errors"),
            errors.count()
        ));
        for code in DIAGNOSTIC_ERRORS {
            let count = errors.count_for(code);
            if count > 0 {
                out.push_str(&format!("  {}: {count}\n", code.as_str()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ops05_diagnostic_cli_requires_explicit_unique_sections_and_bounded_window() {
        let command = ["diagnostics".into(), "snapshot".into()];
        let options = vec![("--sections".into(), "errors,sync".into())];
        let value = parse(&command, &options, 100_000_000).unwrap();
        assert_eq!(
            value.sections(),
            &[DiagnosticSection::Sync, DiagnosticSection::Errors]
        );
        assert_eq!(value.max_executions(), 100);
        for (key, text) in [
            ("--hours", "0"),
            ("--hours", "745"),
            ("--hours", "01"),
            ("--limit", "1001"),
            ("--account", "private"),
        ] {
            let mut invalid = options.clone();
            invalid.push((key.into(), text.into()));
            assert!(parse(&command, &invalid, 100_000_000).is_err());
        }
        assert!(parse(&command, &[], 100_000_000).is_err());
        assert!(
            parse(
                &command,
                &[("--sections".into(), "sync,sync".into())],
                100_000_000
            )
            .is_err()
        );
        assert!(parse(&command, &options, 0).is_err());
    }
}
