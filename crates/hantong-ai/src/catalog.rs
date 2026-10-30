use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
    pub fast: bool,
    pub default: bool,
}
impl Model {
    pub fn effort_for_mode(&self, mode: &str) -> Result<&'static str, AiError> {
        let requested = effort_for_mode(mode)?;
        if self.efforts.iter().any(|e| e == requested) {
            Ok(requested)
        } else if mode == "ultra-high" && self.efforts.iter().any(|e| e == "max") {
            Ok("max")
        } else {
            Err(AiError::Unsupported)
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
}
pub fn effort_for_mode(mode: &str) -> Result<&'static str, AiError> {
    match mode {
        "light" => Ok("low"),
        "med" => Ok("medium"),
        "high" => Ok("high"),
        "ultra-high" => Ok("xhigh"),
        _ => Err(AiError::InvalidInput),
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/-[]".contains(&b))
}
fn strings(v: &Value) -> Result<Vec<String>, AiError> {
    let a = v.as_array().ok_or(AiError::Protocol)?;
    if a.len() > 16 {
        return Err(AiError::TooLarge);
    }
    let mut values = Vec::new();
    for v in a {
        let s = v
            .as_str()
            .filter(|s| identifier(s))
            .ok_or(AiError::Protocol)?;
        if values.iter().any(|v| v == s) {
            return Err(AiError::Protocol);
        }
        values.push(s.into());
    }
    Ok(values)
}
pub(super) fn parse_models(value: &Value, provider: Provider) -> Result<Vec<Model>, AiError> {
    let rows = value
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 256)
        .ok_or(AiError::Protocol)?;
    let mut models = Vec::new();
    for row in rows {
        if row["hidden"] == true {
            continue;
        }
        let id = row[if provider == Provider::Codex {
            "model"
        } else {
            "value"
        }]
        .as_str()
        .filter(|s| identifier(s))
        .ok_or(AiError::Protocol)?
        .to_owned();
        if models.iter().any(|m: &Model| m.id == id) {
            return Err(AiError::Protocol);
        }
        let label = row["displayName"]
            .as_str()
            .filter(|s| s.len() <= 512)
            .map(terminal_text)
            .ok_or(AiError::Protocol)?;
        let (efforts, fast) = if provider == Provider::Codex {
            let levels = row["supportedReasoningEfforts"]
                .as_array()
                .filter(|a| a.len() <= 16)
                .ok_or(AiError::Protocol)?;
            let efforts = strings(&Value::Array(
                levels
                    .iter()
                    .map(|v| v["reasoningEffort"].clone())
                    .collect(),
            ))?;
            let tiers = row.get("serviceTiers").and_then(Value::as_array);
            let fast =
                tiers.is_some_and(|a| a.len() <= 16 && a.iter().any(|v| v["id"] == "priority"));
            (efforts, fast)
        } else {
            let efforts = if row["supportsEffort"] == true {
                strings(&row["supportedEffortLevels"])?
            } else {
                Vec::new()
            };
            (efforts, row["supportsFastMode"] == true)
        };
        models.push(Model {
            id: id.clone(),
            label,
            efforts,
            fast,
            default: row["isDefault"] == true || id == "default",
        });
    }
    if models.is_empty() {
        return Err(AiError::Protocol);
    }
    Ok(models)
}
pub(super) fn codex_models(p: &mut Process, next: &mut u64) -> Result<Vec<Model>, AiError> {
    let mut all = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..16 {
        let id = *next;
        *next += 1;
        p.send(&json!({"id":id,"method":"model/list","params":{"limit":100,"includeHidden":false,"cursor":cursor}}))?;
        let r = p.wait_result(id, Instant::now() + Duration::from_secs(30))?;
        all.extend(
            r["data"]
                .as_array()
                .ok_or(AiError::Protocol)?
                .iter()
                .cloned(),
        );
        if all.len() > 256 {
            return Err(AiError::TooLarge);
        }
        cursor = r["nextCursor"].clone();
        if cursor.is_null() {
            return parse_models(&Value::Array(all), Provider::Codex);
        }
        if cursor
            .as_str()
            .is_none_or(|s| s.is_empty() || s.len() > 2048)
        {
            return Err(AiError::Protocol);
        }
    }
    Err(AiError::TooLarge)
}
pub(super) fn claude_initialize(p: &mut Process) -> Result<Value, AiError> {
    claude_control(
        p,
        "hantong-initialize",
        json!({"subtype":"initialize","hooks":{},"skills":[]}),
    )
}
pub(super) fn claude_control(p: &mut Process, id: &str, request: Value) -> Result<Value, AiError> {
    p.send(&json!({"type":"control_request","request_id":id,"request":request}))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    for _ in 0..256 {
        let v = p.receive(deadline)?.ok_or(AiError::ProviderFailed)?;
        if v["type"] == "control_response" && v["response"]["request_id"] == id {
            if v["response"]["subtype"] != "success" {
                return Err(AiError::ProviderFailed);
            }
            return v["response"]
                .get("response")
                .cloned()
                .ok_or(AiError::Protocol);
        }
        if v["type"] == "control_request" {
            p.send(&json!({"type":"control_response","response":{"subtype":"error","request_id":v["request_id"],"error":"Only explicit hantong mail tools are available"}}))?;
        }
    }
    Err(AiError::TooLarge)
}
pub(super) fn claude_models() -> Result<Vec<Model>, AiError> {
    let mut args = claude_arguments(&Preferences::default(), None);
    args.extend(["--input-format".into(), "stream-json".into()]);
    let refs: Vec<_> = args.iter().map(String::as_str).collect();
    let mut p = Process::spawn(&resolve(Provider::Claude)?, &refs)?;
    let r = claude_initialize(&mut p)?;
    parse_models(&r["models"], Provider::Claude)
}
pub(super) fn claude_arguments(p: &Preferences, mcp: Option<&Value>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--safe-mode",
        "--restricted",
        "--disable-slash-commands",
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--setting-sources",
        "",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.extend([
        "--mcp-config".into(),
        mcp.cloned()
            .unwrap_or_else(|| json!({"mcpServers":{}}))
            .to_string(),
    ]);
    if mcp.is_some() {
        args.extend(["--allowedTools".into(), "mcp__hantong__*".into()]);
    } else {
        args.extend(["--disallowedTools".into(), "*".into()]);
    }
    args.extend([
        "--settings".into(),
        json!({"fastMode":p.fast,"hooks":{}}).to_string(),
    ]);
    if let Some(model) = &p.model {
        args.extend(["--model".into(), model.clone()]);
    }
    if let Some(effort) = &p.effort {
        args.extend(["--effort".into(), effort.clone()]);
    }
    args
}

pub(super) fn claude_mcp_ready(p: &mut Process) -> Result<(), AiError> {
    let result = claude_control(p, "hantong-mcp-status", json!({"subtype":"mcp_status"}))?;
    let servers = result["mcpServers"]
        .as_array()
        .filter(|s| s.len() == 1)
        .ok_or(AiError::AgentUnavailable)?;
    if servers[0]["name"] != "hantong" || servers[0]["status"] != "connected" {
        return Err(AiError::AgentUnavailable);
    }
    Ok(())
}
