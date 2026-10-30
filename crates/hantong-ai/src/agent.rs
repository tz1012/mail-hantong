//! A closed bridge to the packaged MCP executable, never a CLI identity.
use super::*;
const MAIL_TOOLS: &[&str] = &[
    "mail.accounts.list",
    "mail.folders.list",
    "mail.folders.refresh",
    "mail.sync.page",
    "mail.messages.search",
    "mail.messages.get",
    "mail.drafts.create",
    "mail.send.request",
];
pub(super) struct Agent {
    process: Process,
    pub tools: Vec<Value>,
    next: u64,
    pub executable: PathBuf,
}
impl Agent {
    #[cfg(test)]
    pub(super) fn synthetic() -> Self {
        Self::initialize(
            crate::tests::fake("mcp"),
            PathBuf::from("synthetic-mcp-never-executed"),
        )
        .unwrap()
    }
    pub fn connect() -> Result<Self, AiError> {
        let current = env::current_exe().map_err(|_| AiError::Io)?;
        let executable = current.parent().ok_or(AiError::Io)?.join(if cfg!(windows) {
            "hantong-mcp.exe"
        } else {
            "hantong-mcp"
        });
        if !executable.is_file() {
            return Err(AiError::AgentUnavailable);
        }
        let process = Process::spawn(&executable, &[])?;
        Self::initialize(process, executable)
    }
    fn initialize(mut process: Process, executable: PathBuf) -> Result<Self, AiError> {
        process.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","clientInfo":{"name":"mail-hantong-ai","version":env!("CARGO_PKG_VERSION")},"capabilities":{}}}))?;
        let result = process.wait_result(1, Instant::now() + Duration::from_secs(20))?;
        if result["protocolVersion"] != "2025-11-25" {
            return Err(AiError::Protocol);
        }
        process.send(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        process.send(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))?;
        let result = process.wait_result(2, Instant::now() + Duration::from_secs(20))?;
        let rows = result["tools"]
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= MAIL_TOOLS.len())
            .ok_or(AiError::AgentUnavailable)?;
        let mut tools = Vec::new();
        for row in rows {
            let name = row["name"]
                .as_str()
                .filter(|n| MAIL_TOOLS.contains(n))
                .ok_or(AiError::Protocol)?;
            if tools
                .iter()
                .any(|t: &Value| t["name"] == name.replace('.', "_"))
            {
                return Err(AiError::Protocol);
            }
            let description = row["description"]
                .as_str()
                .filter(|s| s.len() <= 2048)
                .ok_or(AiError::Protocol)?;
            if !row["inputSchema"].is_object() {
                return Err(AiError::Protocol);
            }
            tools.push(json!({"name":name.replace('.',"_"),"namespace":"hantong","description":description,"inputSchema":row["inputSchema"]}));
        }
        if !tools.iter().any(|t| t["name"] == "mail_accounts_list") {
            return Err(AiError::AgentUnavailable);
        }
        let mut agent = Self {
            process,
            tools,
            next: 3,
            executable,
        };
        // Current Core policy checks every explicitly scoped account consent.
        // The preflight result is deliberately discarded, not sent to a model.
        let result = agent.call("mail_accounts_list", json!({}))?;
        if result["isError"] != false {
            return Err(AiError::AgentUnavailable);
        }
        Ok(agent)
    }
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, AiError> {
        if !args.is_object() || !self.tools.iter().any(|t| t["name"] == tool) {
            return Err(AiError::InvalidInput);
        }
        let name = MAIL_TOOLS
            .iter()
            .find(|n| n.replace('.', "_") == tool)
            .ok_or(AiError::InvalidInput)?;
        let id = self.next;
        self.next += 1;
        self.process.send(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}))?;
        let result = self
            .process
            .wait_result(id, Instant::now() + Duration::from_secs(30))?;
        if !result["content"].is_array() || !result["isError"].is_boolean() {
            return Err(AiError::Protocol);
        }
        Ok(result)
    }
    pub fn claude_config(&self) -> Value {
        json!({"mcpServers":{"hantong":{"type":"stdio","command":self.executable,"args":[]}}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ai07_bridge_uses_mcp_discovery_and_rejects_non_mail_tools() {
        let mut a = Agent::synthetic();
        assert_eq!(a.tools.len(), 1);
        assert_eq!(a.call("shell", json!({})), Err(AiError::InvalidInput));
        assert_eq!(
            a.call("mail_send_execute", json!({})),
            Err(AiError::InvalidInput)
        );
        assert_eq!(
            a.call("mail_accounts_list", json!({})).unwrap()["isError"],
            false
        );
    }
}
