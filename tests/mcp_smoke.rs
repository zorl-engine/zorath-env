//! Integration smoke test for `zenv mcp`.
//!
//! Spawns the real binary, sends a single JSON-RPC frame over stdin, and
//! parses the response off stdout. The point is to catch any regression
//! that would prevent a real MCP client from completing the handshake
//! (line framing, missing newline flushes, stderr contaminating stdout,
//! etc.) -- the unit tests in commands::mcp::tests cover handler logic
//! but not the spawn + stdio plumbing.
//!
//! These tests intentionally do NOT exercise `tools/call` (which would
//! recursively spawn `zenv <subcommand>` on Windows CI runners and slow
//! things down). The unit tests already cover argument construction;
//! this file's job is "does the binary speak MCP at all".

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Resolve the path to the `zenv` binary built by `cargo test`. The
/// CARGO_BIN_EXE_zenv env var is provided by cargo at test compile time
/// and points to the same artifact the unit tests link against.
fn zenv_bin() -> &'static str {
    env!("CARGO_BIN_EXE_zenv")
}

/// Send `frames` over the spawned `zenv mcp` process's stdin, then close
/// stdin (signaling EOF) and read one response line per request. Returns
/// the parsed JSON values in order.
fn round_trip(frames: &[&str]) -> Vec<serde_json::Value> {
    let mut child = Command::new(zenv_bin())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zenv mcp");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for frame in frames {
            writeln!(stdin, "{}", frame).expect("write frame");
        }
        stdin.flush().expect("flush stdin");
    }
    // Drop stdin so the server hits EOF on its read loop and exits.
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("stdout");
    let reader = BufReader::new(stdout);
    let mut responses = Vec::new();
    for line in reader.lines() {
        let line = line.expect("read line");
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("invalid JSON from server: {} (line: {})", e, line));
        responses.push(value);
    }

    let status = child.wait().expect("wait child");
    assert!(
        status.success(),
        "zenv mcp exited with non-zero status: {:?}",
        status
    );

    responses
}

#[test]
fn initialize_returns_protocol_version_and_capabilities() {
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#;
    let responses = round_trip(&[req]);
    assert_eq!(responses.len(), 1, "expected exactly one response");
    let resp = &responses[0];

    assert_eq!(resp["jsonrpc"].as_str(), Some("2.0"));
    assert_eq!(resp["id"].as_i64(), Some(1));
    let result = &resp["result"];
    assert_eq!(result["protocolVersion"].as_str(), Some("2025-11-25"));
    assert_eq!(result["serverInfo"]["name"].as_str(), Some("zenv"));

    // Every advertised capability must be present.
    let caps = &result["capabilities"];
    assert!(caps.get("tools").is_some(), "tools capability missing");
    assert!(
        caps.get("resources").is_some(),
        "resources capability missing"
    );
    assert!(caps.get("prompts").is_some(), "prompts capability missing");
    assert!(caps.get("logging").is_some(), "logging capability missing");
    assert!(
        caps.get("completions").is_some(),
        "completions capability missing"
    );
}

#[test]
fn ping_returns_empty_result() {
    let req = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
    let responses = round_trip(&[req]);
    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert_eq!(resp["id"].as_i64(), Some(2));
    assert!(resp.get("result").is_some(), "ping should produce result");
    assert!(resp.get("error").is_none(), "ping should not error");
}

#[test]
fn tools_list_returns_five_v1_tools() {
    let req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#;
    let responses = round_trip(&[req]);
    let tools = responses[0]["result"]["tools"]
        .as_array()
        .expect("tools array");
    assert_eq!(
        tools.len(),
        5,
        "expected exactly 5 tools, got {}",
        tools.len()
    );
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for required in &[
        "zenv_check",
        "zenv_scan",
        "zenv_diff",
        "zenv_doctor",
        "zenv_docs",
    ] {
        assert!(
            names.contains(required),
            "missing tool: {} (got: {:?})",
            required,
            names
        );
    }
}

#[test]
fn resources_list_returns_three_resources_with_zenv_uris() {
    let req = r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#;
    let responses = round_trip(&[req]);
    let resources = responses[0]["result"]["resources"]
        .as_array()
        .expect("resources array");
    assert_eq!(resources.len(), 3);
    for r in resources {
        let uri = r["uri"].as_str().unwrap_or("");
        assert!(
            uri.starts_with("zenv://"),
            "resource uri should use zenv:// scheme, got: {}",
            uri
        );
    }
}

#[test]
fn prompts_list_returns_three_prompts() {
    let req = r#"{"jsonrpc":"2.0","id":5,"method":"prompts/list"}"#;
    let responses = round_trip(&[req]);
    let prompts = responses[0]["result"]["prompts"]
        .as_array()
        .expect("prompts array");
    assert_eq!(prompts.len(), 3);
    let names: Vec<&str> = prompts
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    for required in &["audit_env", "new_var_workflow", "diagnose_missing"] {
        assert!(
            names.contains(required),
            "missing prompt: {} (got: {:?})",
            required,
            names
        );
    }
}

#[test]
fn unknown_method_returns_method_not_found_error() {
    let req = r#"{"jsonrpc":"2.0","id":6,"method":"this/does/not/exist"}"#;
    let responses = round_trip(&[req]);
    let resp = &responses[0];
    assert!(
        resp.get("error").is_some(),
        "expected error for unknown method"
    );
    assert_eq!(resp["error"]["code"].as_i64(), Some(-32601));
}

#[test]
fn parse_error_uses_null_id_per_json_rpc_spec() {
    // Send garbage that isn't JSON at all. Per JSON-RPC 2.0, the response
    // must use id=null because we cannot recover the original id from
    // unparseable input.
    let responses = round_trip(&["this is not json"]);
    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert!(resp["id"].is_null(), "parse-error id should be null");
    assert_eq!(resp["error"]["code"].as_i64(), Some(-32700));
}

#[test]
fn notifications_initialized_produces_no_response() {
    // A pure notification (no id field) should generate zero response
    // bytes. Sending it alongside a ping confirms ordering doesn't get
    // confused and that the notification is silently consumed.
    let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let ping = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
    let responses = round_trip(&[notif, ping]);
    assert_eq!(
        responses.len(),
        1,
        "notification should produce no response; only ping should reply"
    );
    assert_eq!(responses[0]["id"].as_i64(), Some(7));
}
