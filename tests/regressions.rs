//! End-to-end regressions for file handling and MCP/CLI configuration wiring.
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;
use zorath_env::commands::export::{export_to_string, ExportFormat};
use zorath_env::envfile::parse_env_str;

fn cli(dir: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(dir.path())
        .env_remove("ZENV_QUIET")
        .env_remove("ZENV_VERBOSE")
        .args(args)
        .output()
        .unwrap()
}

fn mcp(dir: &TempDir, args: &[&str], frames: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(dir.path())
        .env_remove("ZENV_QUIET")
        .env_remove("ZENV_VERBOSE")
        .args(args)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for frame in frames {
        writeln!(input, "{frame}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn export_prefix_requires_a_word_boundary_and_tracks_lines() {
    let parsed = zorath_env::envfile::parse_env_str_detailed(
        "\u{feff}exported=value\nexport=value2\nexport\tKEY=three\nexample=four\n",
    );
    assert_eq!(
        parsed.values.get("exported").map(String::as_str),
        Some("value")
    );
    assert_eq!(
        parsed.values.get("export").map(String::as_str),
        Some("value2")
    );
    assert_eq!(parsed.values.get("KEY").map(String::as_str), Some("three"));
    assert_eq!(parsed.line_numbers["example"], 4);
    let continued =
        zorath_env::envfile::parse_env_str_detailed("A=\"one\\\r\ntwo\"\r\nNEXT=value\r\n");
    assert_eq!(continued.values["A"], "onetwo");
    assert_eq!(continued.line_numbers["NEXT"], 3);
}

#[test]
fn fix_removes_entire_multiline_assignments_without_touching_kept_values() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("env.schema.json"),
        r#"{"KEEP":{"type":"string"}}"#,
    )
    .unwrap();
    let source = "DROP=\"first\nKEEP=inside removed value\nlast\"\n# preserve\nKEEP=\"kept\nDROP=inside kept value\nend\"\n";
    std::fs::write(dir.path().join(".env"), source).unwrap();
    let output = cli(&dir, &["fix", "--remove-unknown"]);
    assert!(output.status.success(), "{output:?}");
    let fixed = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert_eq!(
        parse_env_str(&fixed)["KEEP"],
        "kept\nDROP=inside kept value\nend"
    );
    assert!(!fixed.contains("first"));
    assert!(!fixed.contains("last"));
    assert!(fixed.contains("# preserve"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".env.backup")).unwrap(),
        source
    );
}

#[test]
fn generated_defaults_round_trip_special_characters() {
    let dir = TempDir::new().unwrap();
    let value = " leading # \"quote\" \\ path\nsecond\tline\r ";
    let schema = json!({"TEXT":{"type":"string","required":true,"default":value}});
    std::fs::write(dir.path().join("env.schema.json"), schema.to_string()).unwrap();
    std::fs::write(dir.path().join(".env"), "").unwrap();
    let output = cli(&dir, &["fix"]);
    assert!(output.status.success());
    let fixed = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert_eq!(parse_env_str(&fixed)["TEXT"], value);
    let example = cli(&dir, &["example", "--include-defaults"]);
    assert!(example.status.success());
    assert_eq!(
        parse_env_str(&String::from_utf8(example.stdout).unwrap())["TEXT"],
        value
    );
}

#[test]
fn boolean_cli_switches_accept_bare_flags_and_explicit_false() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("env.schema.json"), "{}").unwrap();
    let output = cli(&dir, &["check", "--allow-missing-env", "--no-cache"]);
    assert!(output.status.success(), "{output:?}");
    let output = cli(
        &dir,
        &["check", "--allow-missing-env=false", "--no-cache=false"],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn mcp_masks_all_lines_and_duplicate_assignments_of_secrets() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".env"), "API_KEY=\"first-secret\ncontinuation-secret\"\nFOO='postgres://user:private@host/db'\nFOO=public\nNORMAL=hello\n").unwrap();
    let frames = mcp(
        &dir,
        &[],
        &[json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"zenv://env"}})],
    );
    let content = frames[0]["result"]["contents"][0]["text"].as_str().unwrap();
    for secret in ["first-secret", "continuation-secret", "private"] {
        assert!(!content.contains(secret), "leaked {secret}: {content}");
    }
    assert!(content.contains("NORMAL=hello"));
}

#[test]
fn mcp_custom_config_is_used_by_tools_and_resources() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("custom.json"),
        r#"{"schema":"contract.yaml","env":"custom.env","detect_secrets":true}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("contract.yaml"),
        "NORMAL:\n  type: string\n  required: true\nPRIVATE:\n  type: string\n  secret: true\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("custom.env"),
        "NORMAL=hello\nPRIVATE=confidential-value\n",
    )
    .unwrap();
    let frames = mcp(
        &dir,
        &["--config", "custom.json"],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"zenv_check","arguments":{"detect_secrets":false}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"zenv://schema"}}),
            json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"zenv://env"}}),
        ],
    );
    assert_eq!(frames[0]["result"]["isError"], false, "{frames:?}");
    let result: Value =
        serde_json::from_str(frames[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["valid"], true);
    assert_eq!(result["secret_warnings"], json!([]));
    let schema = &frames[1]["result"]["contents"][0];
    assert_eq!(schema["mimeType"], "application/yaml");
    assert!(schema["text"].as_str().unwrap().contains("NORMAL:"));
    let env = frames[2]["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(env.contains("NORMAL=hello"));
    assert!(!env.contains("confidential-value"));
}

#[test]
fn mcp_notifications_do_not_respond_and_invalid_requests_do() {
    let dir = TempDir::new().unwrap();
    let frames = mcp(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","method":"ping"}),
            json!({"jsonrpc":"1.0","id":2,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"zenv_check","arguments":{"env":42}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":5,"method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":6,"method":"notifications/cancelled"}),
        ],
    );
    assert_eq!(frames.len(), 5);
    assert_eq!(frames[0]["error"]["code"], -32600);
    assert_eq!(frames[1]["result"]["isError"], true);
    assert_eq!(frames[2]["id"], 4);
    assert_eq!(frames[3]["error"]["code"], -32601);
    assert_eq!(frames[4]["error"]["code"], -32601);
}

#[test]
fn mcp_tools_and_resources_use_the_same_discovered_env() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(
        dir.path().join("env.schema.yaml"),
        "PORT:\n  type: int\n  required: true\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".env.local"), "PORT=3000\n").unwrap();
    let frames = mcp(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"zenv://env"}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"zenv_check","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"zenv_check","arguments":{"env":"missing.env"}}}),
        ],
    );
    assert!(frames[0]["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("PORT=3000"));
    assert_eq!(frames[1]["result"]["isError"], false, "{frames:?}");
    let listed = frames[2]["result"]["resources"].as_array().unwrap();
    assert_eq!(
        listed.iter().find(|r| r["uri"] == "zenv://schema").unwrap()["mimeType"],
        "application/yaml"
    );
    assert_eq!(frames[3]["result"]["isError"], true);
}

#[test]
fn mcp_paths_are_literal_even_when_they_look_like_options() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(
        dir.path().join("--contract.json"),
        r#"{"PORT":{"type":"int"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("--help"), "PORT=3000\n").unwrap();
    std::fs::write(dir.path().join("--version"), "PORT=3001\n").unwrap();
    std::fs::create_dir(dir.path().join("--sources")).unwrap();
    std::fs::write(
        dir.path().join("--sources/app.js"),
        "console.log(process.env.PORT)",
    )
    .unwrap();
    let calls = [
        (
            "zenv_check",
            json!({"env":"--help","schema":"--contract.json"}),
        ),
        (
            "zenv_diff",
            json!({"env_a":"--help","env_b":"--version","schema":"--contract.json"}),
        ),
        (
            "zenv_scan",
            json!({"path":"--sources","schema":"--contract.json"}),
        ),
        ("zenv_docs", json!({"schema":"--contract.json"})),
    ];
    let requests: Vec<_> = calls.iter().enumerate().map(|(id, (name, args))| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).collect();
    for frame in mcp(&dir, &[], &requests) {
        assert_eq!(frame["result"]["isError"], false, "{frame}");
        let content = frame["result"]["content"][0]["text"].as_str().unwrap();
        assert!(serde_json::from_str::<Value>(content).is_ok(), "{content}");
        if frame["id"] == 0 {
            assert_eq!(
                serde_json::from_str::<Value>(content).unwrap()["valid"],
                true
            );
        } else if frame["id"] == 2 {
            let result = serde_json::from_str::<Value>(content).unwrap();
            assert_eq!(result["scanned_path"], "--sources");
            assert_eq!(result["matched_in_schema"], 1);
        } else {
            assert!(content.contains("PORT"), "{content}");
        }
    }
}

#[test]
fn kubernetes_export_preserves_scalar_types_and_control_characters() {
    for value in [
        "TRUE",
        "Null",
        "~",
        "0x12",
        "yes",
        "no",
        "line\rbreak",
        "nul\0byte",
        "\u{1b}",
        "trailing\t",
    ] {
        let env = [("VALUE".to_string(), value.to_string())]
            .into_iter()
            .collect();
        let exported = export_to_string(&env, ExportFormat::K8s).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&exported).unwrap();
        assert_eq!(parsed["data"]["VALUE"].as_str(), Some(value), "{exported}");
    }
}

#[test]
fn executable_exports_reject_invalid_variable_names() {
    let env = [("X;echo injected".to_string(), "value".to_string())]
        .into_iter()
        .collect();
    for format in [
        ExportFormat::Shell,
        ExportFormat::Docker,
        ExportFormat::Systemd,
        ExportFormat::GithubSecrets,
    ] {
        assert!(export_to_string(&env, format).is_err(), "{format}");
    }
    assert!(export_to_string(&env, ExportFormat::Json).is_ok());
}

#[test]
fn docker_export_escapes_expansions_and_rejects_multiline_instructions() {
    let env = [(
        "VALUE".to_string(),
        "$UNRESOLVED 'quote' \\path".to_string(),
    )]
    .into_iter()
    .collect();
    let exported = export_to_string(&env, ExportFormat::Docker).unwrap();
    assert!(exported.contains("\\$UNRESOLVED"));
    assert!(exported.contains("\\\\path"));
    let env = [("VALUE".to_string(), "first\nRUN echo injected".to_string())]
        .into_iter()
        .collect();
    assert!(export_to_string(&env, ExportFormat::Docker).is_err());
}

#[test]
fn example_multiline_metadata_cannot_create_extra_assignments() {
    let dir = TempDir::new().unwrap();
    let schema = json!({"TEXT":{"type":"string","default":"first\nINJECTED=bad\rlast","description":"description\nALSO_INJECTED=bad"}});
    std::fs::write(dir.path().join("env.schema.json"), schema.to_string()).unwrap();
    let output = cli(&dir, &["example", "--include-defaults"]);
    assert!(output.status.success());
    let values = parse_env_str(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(values.len(), 1);
    assert_eq!(values["TEXT"], "first\nINJECTED=bad\rlast");
}

#[test]
fn fix_dry_run_masks_schema_secrets_and_password_urls() {
    let dir = TempDir::new().unwrap();
    let schema = json!({"NORMAL":{"type":"string","required":true,"default":"hidden-value","secret":true},"FOO":{"type":"url","required":true,"default":"postgres://user:hidden-password@host/db"}});
    std::fs::write(dir.path().join("env.schema.json"), schema.to_string()).unwrap();
    let output = cli(&dir, &["fix", "--dry-run"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("hidden-value"));
    assert!(!text.contains("hidden-password"));
    assert_eq!(text.matches("***MASKED***").count(), 2);
    assert!(!dir.path().join(".env").exists());
}

#[test]
fn systemd_export_preserves_literal_percent_specifiers() {
    let env = [(
        "URL".to_string(),
        "https://example.com/a%20b/%n".to_string(),
    )]
    .into_iter()
    .collect();
    let output = export_to_string(&env, ExportFormat::Systemd).unwrap();
    assert!(output.contains("a%%20b/%%n"));
}

#[test]
fn mcp_yaml_schema_discovery_agrees_across_resources_and_tools() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("env.schema.yaml"),
        "FOO:\n  type: string\n  secret: true\n",
    )
    .unwrap();
    std::fs::write(dir.path().join(".env.local"), "FOO=private-value\n").unwrap();
    let frames = mcp(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"zenv://schema"}}),
            json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"zenv://env"}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"zenv_check"}}),
            json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"zenv://docs"}}),
        ],
    );
    assert_eq!(
        frames[0]["result"]["contents"][0]["mimeType"],
        "application/yaml"
    );
    assert!(!frames[1].to_string().contains("private-value"));
    assert!(frames[1]["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("***MASKED***"));
    assert_eq!(frames[2]["result"]["isError"], false);
    assert!(frames[3]["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("FOO"));
}

#[cfg(unix)]
#[test]
fn shell_and_github_exports_preserve_values_without_executing_them() {
    let value = "back\\slash 'quote' $HOME `echo unsafe`\nEOF\nprintf injected\n";
    let env = [("VALUE".to_string(), value.to_string())]
        .into_iter()
        .collect();
    for format in [ExportFormat::Shell, ExportFormat::GithubSecrets] {
        let exported = export_to_string(&env, format).unwrap();
        let script = match format {
            ExportFormat::Shell => format!("{exported}\nprintf '%s' \"$VALUE\""),
            // Mock gh locally; never contact GitHub or write actual secrets.
            _ => format!("gh() {{ printf '%s' \"$5\"; }}\n{exported}"),
        };
        let output = Command::new("sh").args(["-c", &script]).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    }
}
