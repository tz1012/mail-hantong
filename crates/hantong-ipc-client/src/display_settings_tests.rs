use super::*;

fn response(result: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":"d-1","result":{result}}}"#)
}

#[test]
fn cli16_display_settings_required_nullable_language_and_raw_frame_limit() {
    for (wire, expected) in [
        ("null", None),
        ("\"en\"", Some(CliDisplayLanguage::En)),
        ("\"ko\"", Some(CliDisplayLanguage::Ko)),
    ] {
        let raw = response(&format!(
            r#"{{"schema_version":1,"ok":true,"cli_language":{wire}}}"#
        ));
        assert_eq!(
            parse_display_settings_response(raw.as_bytes(), "d-1")
                .unwrap()
                .language(),
            expected
        );
        let padded = format!("{raw}{}", " ".repeat(4096 - raw.len()));
        assert!(parse_display_settings_response(padded.as_bytes(), "d-1").is_ok());
        assert_eq!(
            parse_display_settings_response(format!("{padded} ").as_bytes(), "d-1"),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn sec03_display_settings_rejects_duplicate_missing_unknown_and_type_confusion() {
    for result in [
        r#"{"schema_version":1,"ok":true}"#,
        r#"{"schema_version":1,"ok":true,"cli_language":null,"cli_language":"ko"}"#,
        r#"{"schema_version":1,"schema_version":1,"ok":true,"cli_language":null}"#,
        r#"{"schema_version":1,"ok":true,"cli_language":null,"account_id":"a"}"#,
        r#"{"schema_version":1,"ok":true,"cli_language":"KO"}"#,
        r#"{"schema_version":1,"ok":true,"cli_language":false}"#,
        r#"{"schema_version":1,"ok":true,"cli_language":[]}"#,
        r#"{"schema_version":1,"ok":false,"cli_language":null}"#,
        r#"{"schema_version":null,"ok":true,"cli_language":null}"#,
        r#"{"schema_version":1.0,"ok":true,"cli_language":null}"#,
        r#"[1,true,null]"#,
        "null",
    ] {
        assert_eq!(
            parse_display_settings_response(response(result).as_bytes(), "d-1"),
            Err(ErrorCode::InvalidInput),
            "{result}"
        );
    }
}

#[test]
fn sec03_app_read_outer_rpc_and_stable_errors_are_strict() {
    let good = response(r#"{"schema_version":1,"ok":true,"cli_language":null}"#);
    for raw in [
        good.replace("\"id\":\"d-1\"", "\"id\":\"d-1\",\"id\":\"d-1\""),
        good.replace("\"id\":\"d-1\"", "\"id\":\"other\""),
        good.replace("\"2.0\"", "\"1.0\""),
        format!("{good}{{}}"), format!("\u{feff}{good}"),
        good.replace("\"result\":", "\"error\":{\"code\":\"OFFLINE\",\"message\":\"OFFLINE\"},\"result\":"),
        r#"["2.0","d-1",{"schema_version":1,"ok":true,"cli_language":null}]"#.into(),
        r#"{"jsonrpc":"2.0","id":"d-1","error":["OFFLINE","OFFLINE"]}"#.into(),
        r#"{"jsonrpc":"2.0","id":"d-1","error":{"code":"OFFLINE","message":"raw server text"}}"#.into(),
        r#"{"jsonrpc":"2.0","id":"d-1","error":{"code":"UNKNOWN","message":"UNKNOWN"}}"#.into(),
        r#"{"jsonrpc":"2.0","id":"d-1","error":{"code":"OFFLINE","code":"OFFLINE","message":"OFFLINE"}}"#.into(),
    ] { assert_eq!(parse_display_settings_response(raw.as_bytes(),"d-1"),Err(ErrorCode::InvalidInput),"{raw}"); }
    for code in ErrorCode::ALL {
        let raw=serde_json::json!({"jsonrpc":"2.0","id":"d-1","error":{"code":code.as_str(),"message":code.as_str()}}).to_string();
        assert_eq!(
            parse_display_settings_response(raw.as_bytes(), "d-1"),
            Err(code)
        );
    }
    assert_eq!(
        parse_display_settings_response(&[0xff], "d-1"),
        Err(ErrorCode::InvalidInput)
    );
    for id in ["", "bad id", "\u{200b}"] {
        assert_eq!(
            parse_display_settings_response(good.as_bytes(), id),
            Err(ErrorCode::InvalidInput)
        );
    }
}
