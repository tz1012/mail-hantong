use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Host(Option<Child>);
impl Host {
    fn start(args: &[&str]) -> Self {
        Self(Some(
            Command::new(env!("CARGO_BIN_EXE_hantong-mcp"))
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn finish(mut self, input: &[u8]) -> std::process::Output {
        let child = self.0.as_mut().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "MCP child did not finish after EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn session(name: &str) -> Vec<u8> {
    let mut frames = Vec::new();
    for value in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":name,"version":"test"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mail.send.execute","arguments":{}}}),
    ] {
        serde_json::to_writer(&mut frames, &value).unwrap();
        frames.push(b'\n');
    }
    frames
}

#[test]
fn mcp01_two_real_stdio_hosts_have_independent_lifecycle_and_clean_stdout() {
    // Protocol-only: never request allowed tools, load a profile, or contact a user Core.
    let a = Host::start(&[]);
    let b = Host::start(&[]);
    for (host, name) in [(a, "desktop"), (b, "agent")] {
        let output = host.finish(&session(name));
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let frames: Vec<Value> = output
            .stdout
            .split(|b| *b == b'\n')
            .filter(|f| !f.is_empty())
            .map(|f| serde_json::from_slice(f).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0]["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(frames[1], json!({"jsonrpc":"2.0","id":2,"result":{}}));
        assert_eq!(frames[2]["error"]["code"], -32602);
        assert_eq!(frames[2]["error"]["message"], "Unknown tool");
    }
}

#[test]
fn mcp01_config_is_valid_host_json_and_has_no_builtin_identity() {
    let output = Host::start(&["config"]).finish(b"");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let server = &value["mcpServers"]["hantong"];
    assert!(std::path::Path::new(server["command"].as_str().unwrap()).is_absolute());
    assert_eq!(server["args"], json!([]));
    assert_eq!(server.as_object().unwrap().len(), 2);
}

#[test]
fn mcp01_korean_connection_help_lists_setup_and_diagnostic_commands() {
    let output = Host::start(&["--help", "--lang", "ko"]).finish(b"");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("한통 외부 MCP 연결"));
    assert!(help.contains("hantong-mcp doctor"));
    assert!(help.contains("hantong-mcp config"));
}
