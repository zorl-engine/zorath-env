//! Stdio-based Model Context Protocol (MCP) server for zenv.
//!
//! `zenv mcp` is invoked as a subprocess by MCP-compatible AI clients
//! (Claude Code, Cline, Cursor, Windsurf, etc.). It speaks line-delimited
//! JSON-RPC 2.0 on stdin/stdout and exposes a small set of zenv subcommands
//! as MCP tools.
//!
//! ## Protocol channel discipline
//!
//! stdout is the JSON-RPC channel. Every byte written to stdout must be a
//! valid framed JSON-RPC message. There is exactly ONE `println!` /
//! `writeln!` call in this module and it writes the response frame -- all
//! diagnostics must go to stderr or they will corrupt the protocol stream
//! and the client will hang or panic.
//!
//! ## Why roll our own (not rmcp / mcp-attr / etc.)
//!
//! Every available Rust MCP crate as of May 2026 either: (a) drags tokio
//! into the dep graph, (b) requires Rust 1.85+ via edition = "2024"
//! (rmcp), (c) emits non-ASCII characters in default output, or (d) is
//! pre-0.1 with a single maintainer. zenv is sync, ASCII-only, MSRV 1.74,
//! and lean on deps -- so we own ~300 lines of well-understood code rather
//! than inherit any of those tradeoffs. The protocol surface required for
//! a tools-only stdio server is small.
//!
//! ## Tool model
//!
//! Each tool spawns `zenv <subcommand> --format json` as a child process
//! and returns its stdout as the tool's content. This guarantees the
//! tool's behavior matches the CLI exactly, at the cost of ~5-20ms per
//! call for process spawn. The alternative -- calling library APIs
//! directly -- would require refactoring every `commands::*::run` to
//! return structured data instead of `println!`-ing it. Out of scope
//! for v1.

use crate::errors::CliError;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};

/// Protocol version we negotiate to. Clients on `2024-11-05` (the prior
/// stable revision) are accepted by echoing their requested version back
/// when initialize is called.
const PROTOCOL_VERSION: &str = "2025-11-25";

/// Server identity reported in `initialize` response.
const SERVER_NAME: &str = "zenv";

/// JSON-RPC error codes used by the tools/* surface. -32601 is the
/// JSON-RPC 2.0 mandated "Method not found"; -32602 is "Invalid params".
const ERR_METHOD_NOT_FOUND: i64 = -32601;
const ERR_INVALID_PARAMS: i64 = -32602;
#[allow(dead_code)]
const ERR_INTERNAL: i64 = -32603;

/// MCP logging levels mapped to a numeric scale (lower = more verbose).
/// Stored as u8 in the LOG_LEVEL atomic so logging/setLevel can adjust
/// stderr verbosity at runtime without forcing a global lock.
const LEVEL_DEBUG: u8 = 0;
#[allow(dead_code)]
const LEVEL_INFO: u8 = 1;
#[allow(dead_code)]
const LEVEL_NOTICE: u8 = 2;
const LEVEL_WARNING: u8 = 3;
#[allow(dead_code)]
const LEVEL_ERROR: u8 = 4;
#[allow(dead_code)]
const LEVEL_CRITICAL: u8 = 5;
#[allow(dead_code)]
const LEVEL_ALERT: u8 = 6;
#[allow(dead_code)]
const LEVEL_EMERGENCY: u8 = 7;

/// Default logging level. logging/setLevel from the client can lower this
/// (more verbose) or raise it (more quiet). Set to WARNING by default so
/// stderr stays quiet but lifecycle issues still surface.
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_WARNING);

/// Resolve a level name from the MCP spec to its numeric scale.
fn parse_level(name: &str) -> Option<u8> {
    match name {
        "debug" => Some(LEVEL_DEBUG),
        "info" => Some(LEVEL_INFO),
        "notice" => Some(LEVEL_NOTICE),
        "warning" => Some(LEVEL_WARNING),
        "error" => Some(LEVEL_ERROR),
        "critical" => Some(LEVEL_CRITICAL),
        "alert" => Some(LEVEL_ALERT),
        "emergency" => Some(LEVEL_EMERGENCY),
        _ => None,
    }
}

/// Log to stderr if the current level allows it. stdout is reserved for
/// JSON-RPC frames so this is the ONLY safe diagnostic channel.
fn log_at(level: u8, msg: &str) {
    if level >= LOG_LEVEL.load(Ordering::Relaxed) {
        eprintln!("[zenv mcp] {}", msg);
    }
}

/// Entry point for the `zenv mcp` subcommand.
///
/// Reads JSON-RPC frames from stdin, dispatches them to handlers, and
/// writes responses to stdout one per line. Loops until EOF.
#[doc(hidden)]
pub fn run() -> Result<(), CliError> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    log_at(LEVEL_NOTICE, "server ready");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log_at(LEVEL_ERROR, &format!("stdin read failed: {}", e));
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // Cannot recover the request id from invalid JSON, so emit
                // a parse-error response with id=null per JSON-RPC 2.0.
                let resp = jsonrpc_error(Value::Null, -32700, &format!("Parse error: {}", e));
                write_frame(&mut stdout, &resp);
                continue;
            }
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params");

        let response = match method {
            "initialize" => Some(handle_initialize(&req)),
            "notifications/initialized" => {
                log_at(LEVEL_INFO, "client initialized");
                None
            }
            // Cancellation has no in-flight work to cancel (every tool call
            // is sync and completes before the next stdin read). Drop the
            // notification but log at debug for traceability.
            "notifications/cancelled" => {
                log_at(LEVEL_DEBUG, "cancellation requested (no-op)");
                None
            }
            "tools/list" => Some(handle_tools_list(id.clone())),
            "tools/call" => Some(handle_tools_call(id.clone(), params)),
            "resources/list" => Some(handle_resources_list(id.clone())),
            "resources/read" => Some(handle_resources_read(id.clone(), params)),
            "prompts/list" => Some(handle_prompts_list(id.clone())),
            "prompts/get" => Some(handle_prompts_get(id.clone(), params)),
            "completion/complete" => Some(handle_completion_complete(id.clone(), params)),
            "logging/setLevel" => Some(handle_logging_set_level(id.clone(), params)),
            // Lifecycle hint: ping is optional but cheap to support.
            "ping" => Some(jsonrpc_result(id.clone(), json!({}))),
            // Unknown notification (no id) -- drop silently per spec.
            _ if id.is_none() => None,
            // Unknown request -- standard JSON-RPC error.
            _ => Some(jsonrpc_error(
                id.clone().unwrap_or(Value::Null),
                ERR_METHOD_NOT_FOUND,
                &format!("Method not found: {}", method),
            )),
        };

        if let Some(resp) = response {
            write_frame(&mut stdout, &resp);
        }
    }

    Ok(())
}

/// Write a single JSON-RPC frame followed by a newline, then flush.
/// Flushing is mandatory -- without it the client buffers indefinitely.
fn write_frame<W: Write>(out: &mut W, value: &Value) {
    // The `to_string` is infallible for serde_json::Value; the writes
    // can fail if stdout is closed, in which case there's nothing useful
    // we can do and exiting the loop on the next read is acceptable.
    if let Err(e) = serde_json::to_writer(&mut *out, value) {
        eprintln!("[zenv mcp] response serialize failed: {}", e);
        return;
    }
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// Build a successful JSON-RPC 2.0 response.
fn jsonrpc_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result,
    })
}

/// Build a JSON-RPC 2.0 error response.
fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// Handle `initialize`. Echoes the client's requested protocolVersion
/// when we recognize it; otherwise responds with our own.
fn handle_initialize(req: &Value) -> Value {
    let id = req.get("id").cloned();
    let client_version = req
        .get("params")
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let negotiated = if matches!(client_version, "2025-11-25" | "2024-11-05") {
        client_version
    } else {
        PROTOCOL_VERSION
    };

    jsonrpc_result(
        id,
        json!({
            "protocolVersion": negotiated,
            "capabilities": {
                // tools: 5 zenv subcommand shims. listChanged=false because
                // the registry is hard-coded at compile time.
                "tools":      { "listChanged": false },
                // resources: schema, masked .env, and machine-readable docs
                // exposed via zenv://* URIs. subscribe=false because the
                // filesystem isn't watched.
                "resources":  { "listChanged": false, "subscribe": false },
                // prompts: three baked workflow templates (audit_env,
                // new_var_workflow, diagnose_missing).
                "prompts":    { "listChanged": false },
                // completions: argument autocompletion for path-like params.
                "completions": {},
                // logging: clients can adjust stderr verbosity at runtime
                // via logging/setLevel. stdout stays the protocol channel.
                "logging":    {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )
}

/// Static tool registry. Each entry is (name, description, input schema).
/// The schema follows JSON Schema draft 2020-12 conventions, matching
/// what MCP clients expect.
fn tool_registry() -> Vec<Value> {
    vec![
        json!({
            "name": "zenv_check",
            "description": "Validate an env file against a schema. Returns JSON with valid/errors/warnings/secrets arrays. Use when the user asks to validate, audit, or check an env file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "env":            { "type": "string", "description": "Path to .env file (default: .env)" },
                    "schema":         { "type": "string", "description": "Path to schema file (default: env.schema.json)" },
                    "detect_secrets": { "type": "boolean", "description": "Also scan values for leaked secrets" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "zenv_scan",
            "description": "Scan source code for environment variable usage. Returns JSON with files_scanned, matched_in_schema, missing_from_schema, and (optionally) file:line locations. Use when the user asks which env vars their code references or wants to find schema-vs-code drift.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path":       { "type": "string", "description": "Directory to scan (default: .)" },
                    "schema":     { "type": "string", "description": "Path to schema for cross-reference" },
                    "show_paths": { "type": "boolean", "description": "Include file:line locations in output" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "zenv_diff",
            "description": "Compare two env files. Returns JSON with only_in_a, only_in_b, different_values arrays. Values are auto-masked when sensitive (key heuristic + URL-password detection). Use when the user asks what changed between two env files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "env_a":  { "type": "string", "description": "First env file path" },
                    "env_b":  { "type": "string", "description": "Second env file path" },
                    "schema": { "type": "string", "description": "Optional schema for compliance per file" }
                },
                "required": ["env_a", "env_b"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "zenv_doctor",
            "description": "Run a health check across the env setup. Surfaces missing files, mis-gitignored secrets, schema/code drift, and config issues. Returns plain text (not JSON). Use as the first diagnostic when the user says something is wrong with their env setup.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false
            }
        }),
        json!({
            "name": "zenv_docs",
            "description": "Generate machine-readable docs from the schema. Returns JSON describing every declared env var (type, required, default, validation rules, description). Use when the agent needs to understand the schema without having to read the raw JSON itself.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "schema": { "type": "string", "description": "Path to schema file" }
                },
                "additionalProperties": false
            }
        }),
    ]
}

/// Handle `tools/list`. Always returns the full registry.
fn handle_tools_list(id: Option<Value>) -> Value {
    jsonrpc_result(id, json!({ "tools": tool_registry() }))
}

/// Handle `tools/call`. Dispatches to a known tool or returns method-not-
/// found for unknown tool names. Tool-execution failures (zenv exits
/// non-zero) are reported as `isError: true` on the content payload,
/// not as JSON-RPC errors -- per MCP spec, JSON-RPC errors are reserved
/// for malformed requests, not tool failures.
fn handle_tools_call(id: Option<Value>, params: Option<&Value>) -> Value {
    let id_owned = id.unwrap_or(Value::Null);
    let Some(params) = params else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing params");
    };
    let name = match params.get("name").and_then(|n| n.as_str()) {
        Some(s) => s,
        None => return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing tool name"),
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let cli_args: Result<Vec<String>, String> = match name {
        "zenv_check" => Ok(build_check_args(&args)),
        "zenv_scan" => Ok(build_scan_args(&args)),
        "zenv_diff" => build_diff_args(&args),
        "zenv_doctor" => Ok(vec!["doctor".to_string()]),
        "zenv_docs" => Ok(build_docs_args(&args)),
        _ => {
            return jsonrpc_error(
                id_owned,
                ERR_METHOD_NOT_FOUND,
                &format!("unknown tool: {}", name),
            )
        }
    };

    let cli_args = match cli_args {
        Ok(v) => v,
        Err(e) => return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, &e),
    };

    let outcome = run_zenv_subprocess(&cli_args);
    let content = match outcome {
        Ok(stdout) => json!([{ "type": "text", "text": stdout }]),
        Err(err_text) => {
            return jsonrpc_result(
                Some(id_owned),
                json!({
                    "content": [{ "type": "text", "text": err_text }],
                    "isError": true
                }),
            );
        }
    };

    jsonrpc_result(
        Some(id_owned),
        json!({ "content": content, "isError": false }),
    )
}

fn build_check_args(args: &Value) -> Vec<String> {
    let mut a = vec![
        "check".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];
    if let Some(env) = args.get("env").and_then(|v| v.as_str()) {
        a.push("--env".to_string());
        a.push(env.to_string());
    }
    if let Some(schema) = args.get("schema").and_then(|v| v.as_str()) {
        a.push("--schema".to_string());
        a.push(schema.to_string());
    }
    if args
        .get("detect_secrets")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        a.push("--detect-secrets".to_string());
    }
    a
}

fn build_scan_args(args: &Value) -> Vec<String> {
    let mut a = vec![
        "scan".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        a.push("--path".to_string());
        a.push(p.to_string());
    }
    if let Some(s) = args.get("schema").and_then(|v| v.as_str()) {
        a.push("--schema".to_string());
        a.push(s.to_string());
    }
    if args
        .get("show_paths")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        a.push("--show-paths".to_string());
    }
    a
}

fn build_diff_args(args: &Value) -> Result<Vec<String>, String> {
    let env_a = args
        .get("env_a")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "env_a is required".to_string())?;
    let env_b = args
        .get("env_b")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "env_b is required".to_string())?;
    let mut a = vec![
        "diff".to_string(),
        env_a.to_string(),
        env_b.to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];
    if let Some(s) = args.get("schema").and_then(|v| v.as_str()) {
        a.push("--schema".to_string());
        a.push(s.to_string());
    }
    Ok(a)
}

fn build_docs_args(args: &Value) -> Vec<String> {
    let mut a = vec![
        "docs".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];
    if let Some(s) = args.get("schema").and_then(|v| v.as_str()) {
        a.push("--schema".to_string());
        a.push(s.to_string());
    }
    a
}

/// Spawn `<current-exe> <args...>` and return stdout. On non-zero exit we
/// return an Err containing stderr + stdout merged so the model can see
/// the failure reason. Uses --no-color so escape sequences don't pollute
/// the model's view of the output.
fn run_zenv_subprocess(args: &[String]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot resolve current exe: {}", e))?;
    let output = Command::new(&exe)
        .arg("--no-color")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("spawn failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(stdout)
    } else {
        // exit codes are part of zenv's stable API; surface them so the
        // model can disambiguate validation failures (1) from input
        // errors (2) and schema errors (3).
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated".to_string());
        Err(format!(
            "zenv exited with status {}\n-- stderr --\n{}\n-- stdout --\n{}",
            code, stderr, stdout
        ))
    }
}

// =============================================================================
// resources/*  (URI registry + read dispatch)
//
// Three resources are exposed: the active schema (raw JSON/YAML source),
// the active .env with sensitive values masked, and the schema's
// machine-readable docs (output of `zenv docs --format json`). Resources
// are static within a session -- there is no change notification stream
// so subscribe/unsubscribe are accepted but no-op.
// =============================================================================

const URI_SCHEMA: &str = "zenv://schema";
const URI_ENV_MASKED: &str = "zenv://env";
const URI_DOCS: &str = "zenv://docs";

fn resource_registry() -> Vec<Value> {
    vec![
        json!({
            "uri": URI_SCHEMA,
            "name": "zenv schema",
            "description": "The active env.schema.json (or YAML schema) for this project. Read this to learn the declared env-var contract before suggesting changes.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": URI_ENV_MASKED,
            "name": "zenv .env (masked)",
            "description": "The active .env file with sensitive values masked. Sensitive detection uses both key-name heuristics and value-aware checks (URL passwords, webhook URLs). Safe to display in chat.",
            "mimeType": "text/plain"
        }),
        json!({
            "uri": URI_DOCS,
            "name": "zenv generated docs",
            "description": "Machine-readable schema documentation (output of `zenv docs --format json`). Use when you want a normalized view of every env var instead of the raw schema source.",
            "mimeType": "application/json"
        }),
    ]
}

fn handle_resources_list(id: Option<Value>) -> Value {
    jsonrpc_result(id, json!({ "resources": resource_registry() }))
}

fn handle_resources_read(id: Option<Value>, params: Option<&Value>) -> Value {
    let id_owned = id.unwrap_or(Value::Null);
    let Some(params) = params else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing params");
    };
    let uri = match params.get("uri").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing uri"),
    };

    let (text, mime) = match uri {
        URI_SCHEMA => match read_schema_file() {
            Ok(t) => (t, "application/json"),
            Err(e) => {
                return jsonrpc_error(
                    id_owned,
                    ERR_INVALID_PARAMS,
                    &format!("cannot read schema: {}", e),
                )
            }
        },
        URI_ENV_MASKED => match read_env_masked_at(std::path::Path::new(".env")) {
            Ok(t) => (t, "text/plain"),
            Err(e) => {
                return jsonrpc_error(
                    id_owned,
                    ERR_INVALID_PARAMS,
                    &format!("cannot read .env: {}", e),
                )
            }
        },
        URI_DOCS => match run_zenv_subprocess(&[
            "docs".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ]) {
            Ok(t) => (t, "application/json"),
            Err(e) => {
                return jsonrpc_error(
                    id_owned,
                    ERR_INVALID_PARAMS,
                    &format!("docs subprocess failed: {}", e),
                )
            }
        },
        _ => {
            return jsonrpc_error(
                id_owned,
                ERR_INVALID_PARAMS,
                &format!("unknown resource uri: {}", uri),
            )
        }
    };

    jsonrpc_result(
        Some(id_owned),
        json!({
            "contents": [{
                "uri": uri,
                "mimeType": mime,
                "text": text
            }]
        }),
    )
}

/// Find and read the active schema file (JSON or YAML). Caller treats
/// the content as text -- we do not parse it here.
fn read_schema_file() -> Result<String, String> {
    for candidate in &["env.schema.json", "env.schema.yaml", "env.schema.yml"] {
        if std::path::Path::new(candidate).exists() {
            return std::fs::read_to_string(candidate).map_err(|e| e.to_string());
        }
    }
    Err("no schema file found (looked for env.schema.{json,yaml,yml})".to_string())
}

/// Read .env at the current working directory and mask sensitive values.
/// Thin wrapper around `read_env_masked_at` -- the path-taking version
/// is what tests exercise directly, sidestepping the process-wide CWD
/// race that bit us when two tests both chdir to their own tempdir.
#[allow(dead_code)]
fn read_env_masked() -> Result<String, String> {
    read_env_masked_at(std::path::Path::new(".env"))
}

/// Read .env from an explicit path and mask sensitive values. Uses both
/// key-name heuristics and value-aware checks so URL-password-style
/// leaks under innocuous keys also get masked. Format is preserved
/// otherwise (comments, blank lines, original surrounding quotes).
fn read_env_masked_at(path: &std::path::Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key_trim = key.trim();
            let raw_value = value.trim();
            // Inspect the inner unquoted value for sensitivity, but
            // preserve the original line shape in the non-masked branch.
            let inner = raw_value
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| {
                    raw_value
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                })
                .unwrap_or(raw_value);
            if crate::secrets::is_sensitive_key(key_trim)
                || crate::secrets::value_looks_secret(inner)
            {
                out.push_str(key);
                out.push('=');
                out.push_str("***MASKED***");
            } else {
                out.push_str(line);
            }
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

// =============================================================================
// prompts/*  (registry + parameterized message generation)
//
// Three prompts shipped: a full env audit, a guided new-var workflow,
// and a missing-var diagnostic walkthrough. Each prompt expands to a
// user-role message that an MCP-aware client can drop into the
// conversation; the messages reference zenv's tools + resources so the
// agent already knows how to act on them.
// =============================================================================

fn prompt_registry() -> Vec<Value> {
    vec![
        json!({
            "name": "audit_env",
            "description": "Run a full env-hygiene audit on the current project: validate the schema, detect leaked secrets, and report code-vs-schema drift.",
            "arguments": []
        }),
        json!({
            "name": "new_var_workflow",
            "description": "Guided workflow for safely adding a new env var to the project: schema update, .env update, code reference, and CI validation.",
            "arguments": [
                { "name": "var_name", "description": "Name of the new env var (e.g. STRIPE_API_KEY)", "required": true }
            ]
        }),
        json!({
            "name": "diagnose_missing",
            "description": "Diagnose why an env var appears missing at runtime: schema declaration, .env presence, code reference path, and CI gating.",
            "arguments": [
                { "name": "var_name", "description": "Name of the missing env var", "required": true }
            ]
        }),
    ]
}

fn handle_prompts_list(id: Option<Value>) -> Value {
    jsonrpc_result(id, json!({ "prompts": prompt_registry() }))
}

fn handle_prompts_get(id: Option<Value>, params: Option<&Value>) -> Value {
    let id_owned = id.unwrap_or(Value::Null);
    let Some(params) = params else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing params");
    };
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing prompt name"),
    };
    let args = params.get("arguments");

    let messages = match name {
        "audit_env" => audit_env_messages(),
        "new_var_workflow" => {
            let var = args
                .and_then(|a| a.get("var_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("<NEW_VAR>");
            new_var_workflow_messages(var)
        }
        "diagnose_missing" => {
            let var = args
                .and_then(|a| a.get("var_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("<MISSING_VAR>");
            diagnose_missing_messages(var)
        }
        _ => {
            return jsonrpc_error(
                id_owned,
                ERR_METHOD_NOT_FOUND,
                &format!("unknown prompt: {}", name),
            )
        }
    };

    jsonrpc_result(
        Some(id_owned),
        json!({
            "description": format!("zenv prompt: {}", name),
            "messages": messages
        }),
    )
}

fn audit_env_messages() -> Value {
    json!([{
        "role": "user",
        "content": {
            "type": "text",
            "text": "Audit my project's env setup using the zenv MCP tools. Run, in order:\n\
                     1. zenv_doctor for a health overview.\n\
                     2. zenv_check with detect_secrets=true to validate and scan for leaked secrets.\n\
                     3. zenv_scan with show_paths=true to find code-vs-schema drift.\n\
                     Then summarize what's wrong, what's safe to ignore, and what needs human action. \
                     Mask any sensitive values you see; do not echo raw secrets back."
        }
    }])
}

fn new_var_workflow_messages(var: &str) -> Value {
    json!([{
        "role": "user",
        "content": {
            "type": "text",
            "text": format!(
                "Walk me through adding the env var `{}` to this project safely:\n\
                 1. Read the current schema via the zenv://schema resource.\n\
                 2. Propose a schema update for `{}` with an appropriate type, description, and (if a secret) `secret: true`.\n\
                 3. Update .env (and .env.example) accordingly.\n\
                 4. Use zenv_check with detect_secrets=true to verify the value is not a known-bad pattern.\n\
                 5. Use zenv_scan to confirm the code reference is detected.\n\
                 At each step show me the diff and wait for confirmation before applying.",
                var, var
            )
        }
    }])
}

fn diagnose_missing_messages(var: &str) -> Value {
    json!([{
        "role": "user",
        "content": {
            "type": "text",
            "text": format!(
                "Diagnose why `{}` appears missing at runtime. Use the zenv tools:\n\
                 1. zenv_doctor for environment health.\n\
                 2. zenv_scan with show_paths=true to find where the code reads `{}`.\n\
                 3. Read the zenv://schema resource and confirm `{}` is declared.\n\
                 4. Read the zenv://env resource and confirm `{}` is set.\n\
                 5. If the project has multiple .env files (e.g. .env.local vs .env.production), run zenv_diff between them.\n\
                 Then explain the root cause in one sentence and propose the fix.",
                var, var, var, var
            )
        }
    }])
}

// =============================================================================
// completion/complete  (real handler)
//
// MCP clients call this to autocomplete an argument value the user is
// typing -- the request shape is `{ ref: {type, name}, argument: {name,
// value} }` per the 2025-11-25 spec. We complete three argument kinds:
//   * `schema` -> local *.json / *.yaml / *.yml files with "schema" in
//     the basename
//   * `env` / `env_a` / `env_b` / `env_file` -> local .env* files
//   * `preset` -> finite set of 6 framework presets
// All other argument names fall through to an empty completion list so
// the client renders nothing (the spec discourages -32601 here).
// =============================================================================

fn handle_completion_complete(id: Option<Value>, params: Option<&Value>) -> Value {
    let id_owned = id.unwrap_or(Value::Null);
    let Some(params) = params else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing params");
    };
    let Some(argument) = params.get("argument") else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing argument");
    };
    let arg_name = argument.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let prefix = argument.get("value").and_then(|v| v.as_str()).unwrap_or("");

    let candidates: Vec<String> = match arg_name {
        "schema" => list_local_files_matching(&["schema"], &[".json", ".yaml", ".yml"]),
        "env" | "env_a" | "env_b" | "env_file" => list_local_files_matching(&[".env"], &[]),
        "preset" => vec![
            "nextjs".to_string(),
            "rails".to_string(),
            "django".to_string(),
            "fastapi".to_string(),
            "express".to_string(),
            "laravel".to_string(),
        ],
        _ => Vec::new(),
    };

    // Filter by user-typed prefix (case-insensitive); cap at 100 per the
    // spec's `hasMore` convention (we set hasMore=false because zenv's
    // candidate sets are all finite + small).
    let lower = prefix.to_lowercase();
    let filtered: Vec<String> = candidates
        .into_iter()
        .filter(|c| c.to_lowercase().starts_with(&lower))
        .take(100)
        .collect();
    let total = filtered.len();
    jsonrpc_result(
        Some(id_owned),
        json!({
            "completion": {
                "values": filtered,
                "total": total,
                "hasMore": false
            }
        }),
    )
}

/// List filenames in the current directory whose name contains any of
/// `name_substrs` (case-insensitive) AND whose extension matches any of
/// `extensions`. Empty filters skip the corresponding check.
fn list_local_files_matching(name_substrs: &[&str], extensions: &[&str]) -> Vec<String> {
    let entries = match std::fs::read_dir(".") {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let name_lower = name.to_lowercase();
        if !name_substrs.is_empty() && !name_substrs.iter().any(|s| name_lower.contains(*s)) {
            continue;
        }
        if !extensions.is_empty() && !extensions.iter().any(|e| name_lower.ends_with(e)) {
            continue;
        }
        out.push(name.to_string());
    }
    out.sort();
    out
}

// =============================================================================
// logging/setLevel
//
// Accepts any well-known MCP log level and stores it in LOG_LEVEL.
// Subsequent stderr diagnostics filter through log_at() against the
// stored threshold.
// =============================================================================

fn handle_logging_set_level(id: Option<Value>, params: Option<&Value>) -> Value {
    let id_owned = id.unwrap_or(Value::Null);
    let Some(params) = params else {
        return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing params");
    };
    let level_str = match params.get("level").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return jsonrpc_error(id_owned, ERR_INVALID_PARAMS, "missing level"),
    };
    let Some(level_num) = parse_level(level_str) else {
        return jsonrpc_error(
            id_owned,
            ERR_INVALID_PARAMS,
            &format!("invalid log level: {}", level_str),
        );
    };
    LOG_LEVEL.store(level_num, Ordering::Relaxed);
    log_at(LEVEL_NOTICE, &format!("log level set to {}", level_str));
    jsonrpc_result(Some(id_owned), json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_registry_has_five_v1_tools() {
        let tools = tool_registry();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"zenv_check"));
        assert!(names.contains(&"zenv_scan"));
        assert!(names.contains(&"zenv_diff"));
        assert!(names.contains(&"zenv_doctor"));
        assert!(names.contains(&"zenv_docs"));
    }

    #[test]
    fn test_every_tool_has_required_mcp_fields() {
        for tool in tool_registry() {
            assert!(tool.get("name").and_then(|v| v.as_str()).is_some());
            assert!(tool.get("description").and_then(|v| v.as_str()).is_some());
            let schema = tool.get("inputSchema").expect("inputSchema missing");
            assert_eq!(schema["type"].as_str(), Some("object"));
        }
    }

    #[test]
    fn test_initialize_negotiates_recognized_version() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-11-25" }
        });
        let resp = handle_initialize(&req);
        assert_eq!(resp["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(resp["id"].as_i64(), Some(1));
        assert_eq!(
            resp["result"]["protocolVersion"].as_str(),
            Some("2025-11-25")
        );
        assert_eq!(resp["result"]["serverInfo"]["name"].as_str(), Some("zenv"));
        assert_eq!(
            resp["result"]["capabilities"]["tools"]["listChanged"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn test_initialize_falls_back_to_default_on_unknown_version() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "9999-99-99" }
        });
        let resp = handle_initialize(&req);
        assert_eq!(
            resp["result"]["protocolVersion"].as_str(),
            Some(PROTOCOL_VERSION)
        );
    }

    #[test]
    fn test_initialize_accepts_prior_revision() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05" }
        });
        let resp = handle_initialize(&req);
        assert_eq!(
            resp["result"]["protocolVersion"].as_str(),
            Some("2024-11-05")
        );
    }

    #[test]
    fn test_tools_list_returns_registry() {
        let resp = handle_tools_list(Some(json!(7)));
        assert_eq!(resp["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(resp["id"].as_i64(), Some(7));
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn test_tools_call_unknown_tool_returns_method_not_found() {
        let resp = handle_tools_call(
            Some(json!(2)),
            Some(&json!({ "name": "zenv_bogus", "arguments": {} })),
        );
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_METHOD_NOT_FOUND));
    }

    #[test]
    fn test_tools_call_missing_params_returns_invalid_params() {
        let resp = handle_tools_call(Some(json!(3)), None);
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_tools_call_missing_tool_name_returns_invalid_params() {
        let resp = handle_tools_call(Some(json!(4)), Some(&json!({ "arguments": {} })));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_diff_missing_env_a_returns_invalid_params() {
        let resp = handle_tools_call(
            Some(json!(5)),
            Some(&json!({ "name": "zenv_diff", "arguments": { "env_b": ".env" } })),
        );
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_build_check_args_defaults_to_json_format() {
        let args = build_check_args(&json!({}));
        assert_eq!(args, vec!["check", "--format", "json"]);
    }

    #[test]
    fn test_build_check_args_adds_detect_secrets_flag() {
        let args = build_check_args(&json!({ "detect_secrets": true }));
        assert!(args.contains(&"--detect-secrets".to_string()));
    }

    #[test]
    fn test_build_check_args_threads_env_and_schema_paths() {
        let args = build_check_args(&json!({
            "env": "/tmp/.env.prod",
            "schema": "/tmp/env.schema.json"
        }));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--env" && w[1] == "/tmp/.env.prod"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--schema" && w[1] == "/tmp/env.schema.json"));
    }

    #[test]
    fn test_build_scan_args_adds_show_paths_when_truthy() {
        let args = build_scan_args(&json!({ "path": "src", "show_paths": true }));
        assert!(args.contains(&"--show-paths".to_string()));
        assert!(args.windows(2).any(|w| w[0] == "--path" && w[1] == "src"));
    }

    #[test]
    fn test_build_diff_args_positional_order_preserved() {
        let args = build_diff_args(&json!({ "env_a": "a.env", "env_b": "b.env" })).unwrap();
        // diff takes two positional args before --format
        assert_eq!(args[0], "diff");
        assert_eq!(args[1], "a.env");
        assert_eq!(args[2], "b.env");
        assert!(args.contains(&"--format".to_string()));
    }

    #[test]
    fn test_jsonrpc_result_shape() {
        let r = jsonrpc_result(Some(json!(42)), json!({ "ok": true }));
        assert_eq!(r["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(r["id"].as_i64(), Some(42));
        assert_eq!(r["result"]["ok"].as_bool(), Some(true));
    }

    #[test]
    fn test_jsonrpc_error_shape() {
        let r = jsonrpc_error(json!(99), -32601, "Method not found: foo");
        assert_eq!(r["jsonrpc"].as_str(), Some("2.0"));
        assert_eq!(r["id"].as_i64(), Some(99));
        assert_eq!(r["error"]["code"].as_i64(), Some(-32601));
        assert!(r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not found"));
    }

    // -------- resources/* --------

    #[test]
    fn test_resources_list_returns_three_resources() {
        let resp = handle_resources_list(Some(json!(20)));
        let resources = resp["result"]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 3);
        let uris: Vec<&str> = resources
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        assert!(uris.contains(&URI_SCHEMA));
        assert!(uris.contains(&URI_ENV_MASKED));
        assert!(uris.contains(&URI_DOCS));
    }

    #[test]
    fn test_resources_read_rejects_unknown_uri() {
        let resp = handle_resources_read(
            Some(json!(21)),
            Some(&json!({ "uri": "zenv://does-not-exist" })),
        );
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_resources_read_missing_uri_returns_invalid_params() {
        let resp = handle_resources_read(Some(json!(22)), Some(&json!({})));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_resources_read_missing_params_returns_invalid_params() {
        let resp = handle_resources_read(Some(json!(23)), None);
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    // -------- prompts/* --------

    #[test]
    fn test_prompts_list_returns_three_prompts() {
        let resp = handle_prompts_list(Some(json!(30)));
        let prompts = resp["result"]["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 3);
        let names: Vec<&str> = prompts
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        for expected in &["audit_env", "new_var_workflow", "diagnose_missing"] {
            assert!(names.contains(expected), "missing prompt: {}", expected);
        }
    }

    #[test]
    fn test_prompts_get_audit_env_returns_messages() {
        let resp = handle_prompts_get(Some(json!(31)), Some(&json!({ "name": "audit_env" })));
        let messages = resp["result"]["messages"].as_array().unwrap();
        assert!(!messages.is_empty());
        assert_eq!(messages[0]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_prompts_get_new_var_substitutes_argument() {
        let resp = handle_prompts_get(
            Some(json!(32)),
            Some(&json!({
                "name": "new_var_workflow",
                "arguments": { "var_name": "STRIPE_SECRET_KEY" }
            })),
        );
        let text = resp["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("STRIPE_SECRET_KEY"));
        assert!(!text.contains("<NEW_VAR>"));
    }

    #[test]
    fn test_prompts_get_unknown_returns_method_not_found() {
        let resp = handle_prompts_get(Some(json!(33)), Some(&json!({ "name": "bogus" })));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_METHOD_NOT_FOUND));
    }

    #[test]
    fn test_prompts_get_missing_name_returns_invalid_params() {
        let resp = handle_prompts_get(Some(json!(34)), Some(&json!({})));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    // -------- completion/complete --------

    #[test]
    fn test_completion_returns_invalid_params_when_missing_argument() {
        // Spec requires `params.argument.{name,value}`. Calling without
        // params is a protocol error, not a placeholder.
        let resp = handle_completion_complete(Some(json!(40)), None);
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_completion_returns_preset_candidates() {
        // The handler exposes the 6 init presets when the agent is
        // completing a `preset` argument with an empty prefix.
        let resp = handle_completion_complete(
            Some(json!(41)),
            Some(&json!({
                "ref": { "type": "ref/prompt", "name": "new_var_workflow" },
                "argument": { "name": "preset", "value": "" }
            })),
        );
        let c = &resp["result"]["completion"];
        assert_eq!(c["hasMore"].as_bool(), Some(false));
        let values = c["values"].as_array().unwrap();
        let names: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
        for preset in &["nextjs", "rails", "django", "fastapi", "express", "laravel"] {
            assert!(names.contains(preset), "missing preset: {}", preset);
        }
    }

    #[test]
    fn test_completion_filters_by_prefix() {
        // Same preset list, but only entries matching the typed prefix.
        let resp = handle_completion_complete(
            Some(json!(42)),
            Some(&json!({
                "ref": { "type": "ref/prompt", "name": "x" },
                "argument": { "name": "preset", "value": "ra" }
            })),
        );
        let values = resp["result"]["completion"]["values"].as_array().unwrap();
        let names: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"rails"));
        assert!(!names.contains(&"nextjs"));
    }

    #[test]
    fn test_completion_unknown_argument_returns_empty() {
        // Argument names we don't know about return an empty candidate
        // list (not an error) so clients can call completion broadly.
        let resp = handle_completion_complete(
            Some(json!(43)),
            Some(&json!({
                "ref": { "type": "ref/prompt", "name": "x" },
                "argument": { "name": "unrecognized", "value": "" }
            })),
        );
        assert_eq!(
            resp["result"]["completion"]["values"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            resp["result"]["completion"]["hasMore"].as_bool(),
            Some(false)
        );
    }

    // -------- logging/setLevel --------

    #[test]
    fn test_set_log_level_accepts_valid_level() {
        let resp = handle_logging_set_level(Some(json!(50)), Some(&json!({ "level": "warning" })));
        assert!(resp.get("result").is_some());
        assert!(resp.get("error").is_none());
        assert_eq!(LOG_LEVEL.load(Ordering::Relaxed), LEVEL_WARNING);
    }

    #[test]
    fn test_set_log_level_rejects_invalid_level() {
        let resp =
            handle_logging_set_level(Some(json!(51)), Some(&json!({ "level": "PARTY_MODE" })));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    #[test]
    fn test_set_log_level_missing_level_returns_invalid_params() {
        let resp = handle_logging_set_level(Some(json!(52)), Some(&json!({})));
        assert_eq!(resp["error"]["code"].as_i64(), Some(ERR_INVALID_PARAMS));
    }

    // -------- initialize capabilities cover full surface --------

    #[test]
    fn test_initialize_advertises_full_capability_set() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-11-25" }
        });
        let resp = handle_initialize(&req);
        let caps = &resp["result"]["capabilities"];
        for key in &["tools", "resources", "prompts", "logging", "completions"] {
            assert!(caps.get(key).is_some(), "missing capability key: {}", key);
        }
        assert_eq!(caps["tools"]["listChanged"].as_bool(), Some(false));
        assert_eq!(caps["resources"]["subscribe"].as_bool(), Some(false));
    }

    // -------- read_env_masked masking semantics --------

    #[test]
    fn test_read_env_masked_masks_sensitive_keys() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "FOO=plain\nAPI_KEY=sk_live_abc123\nNOTES=hello\n").unwrap();
        let body = read_env_masked_at(&path).unwrap();
        assert!(body.contains("FOO=plain"));
        assert!(body.contains("API_KEY=***MASKED***"));
        assert!(body.contains("NOTES=hello"));
        assert!(!body.contains("sk_live_abc123"));
    }

    #[test]
    fn test_read_env_masked_masks_url_passwords_under_innocuous_keys() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "INNOCUOUS=postgres://user:Tr0ub4dor!@host/db\n").unwrap();
        let body = read_env_masked_at(&path).unwrap();
        assert!(body.contains("INNOCUOUS=***MASKED***"));
        assert!(!body.contains("Tr0ub4dor"));
    }

    // -------- parse_level --------

    #[test]
    fn test_parse_level_covers_all_mcp_levels() {
        for (name, expected) in &[
            ("debug", LEVEL_DEBUG),
            ("info", LEVEL_INFO),
            ("notice", LEVEL_NOTICE),
            ("warning", LEVEL_WARNING),
            ("error", LEVEL_ERROR),
            ("critical", LEVEL_CRITICAL),
            ("alert", LEVEL_ALERT),
            ("emergency", LEVEL_EMERGENCY),
        ] {
            assert_eq!(parse_level(name), Some(*expected), "for {}", name);
        }
        assert_eq!(parse_level("unknown"), None);
    }
}
