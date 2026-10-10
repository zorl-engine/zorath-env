use serde_json::json;
use std::collections::HashMap;
use std::process::Command;
use tempfile::TempDir;
use zorath_env::{commands::check, schema};

#[test]
fn rejects_nonfinite_floats_and_impossible_dates() {
    for (kind, values) in [
        ("float", vec!["NaN", "inf", "-inf", "1e999"]),
        (
            "date",
            vec![
                "2025-02-29",
                "2024-02-30",
                "2024-04-31",
                "1900-02-29",
                "٢٠٢٤-01-01",
            ],
        ),
    ] {
        let contract: schema::Schema =
            serde_json::from_value(json!({"VALUE":{"type":kind}})).unwrap();
        for value in values {
            let env = HashMap::from([("VALUE".into(), value.into())]);
            assert!(
                !check::validate(&contract, &env).is_empty(),
                "accepted {kind}: {value}"
            );
        }
    }
}

#[test]
fn validates_ipv6_and_semver_semantics() {
    for (kind, value, valid) in [
        ("ipv6", "2001:db8::1", true),
        ("ipv6", "::ffff:192.0.2.1", true),
        ("ipv6", "1:2:3:4:5:6:7::8", false),
        ("ipv6", "1:2:::3", false),
        ("semver", "1.2.3-alpha-beta.1+build-42", true),
        ("semver", "01.2.3", false),
        ("semver", "1.2.3-01", false),
        ("semver", "1.2.3+001", true),
    ] {
        let contract: schema::Schema =
            serde_json::from_value(json!({"VALUE":{"type":kind}})).unwrap();
        let env = HashMap::from([("VALUE".into(), value.into())]);
        assert_eq!(
            check::validate(&contract, &env).is_empty(),
            valid,
            "{kind}: {value}"
        );
    }
}

#[test]
fn validation_bounds_do_not_disclose_secret_numbers() {
    for (kind, value, rules) in [
        ("int", "8675309", json!({"max":10})),
        ("float", "8675309.25", json!({"max_value":10.0})),
    ] {
        let contract: schema::Schema =
            serde_json::from_value(json!({"PRIVATE":{"type":kind,"secret":true,"validate":rules}}))
                .unwrap();
        let env = HashMap::from([("PRIVATE".into(), value.into())]);
        let errors = check::validate(&contract, &env);
        assert!(!errors.is_empty());
        assert!(!errors.join("\n").contains(value), "{errors:?}");
    }
}

#[test]
fn atomic_export_does_not_clobber_existing_temp_files() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".env"), "FOO=bar").unwrap();
    std::fs::write(dir.path().join("output.env.tmp"), "unrelated file").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(dir.path())
        .args(["export", "--format", "dotenv", "--output", "output.env"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("output.env.tmp")).unwrap(),
        "unrelated file"
    );
}

#[test]
fn bare_schema_filename_cannot_extend_outside_its_directory() {
    let dir = TempDir::new().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        dir.path().join("outside.json"),
        r#"{"ESCAPED":{"type":"string"}}"#,
    )
    .unwrap();
    std::fs::write(
        project.join("schema.json"),
        r#"{"extends":"../outside.json"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(project)
        .args(["docs", "--schema", "schema.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("escapes schema directory"));
}

#[test]
fn remote_yaml_format_uses_url_path_without_query_or_fragment() {
    for path in [
        "https://example.com/schema.yaml?version=2",
        "https://example.com/schema.yml#revision",
    ] {
        assert_eq!(
            schema::SchemaFormat::from_path(path),
            schema::SchemaFormat::Yaml
        );
    }
}

#[test]
fn fix_checks_all_types_and_interpolates_values() {
    let dir = TempDir::new().unwrap();
    let contract = json!({
        "BASE": {"type":"int"},
        "PORT": {"type":"port"},
        "DAY": {"type":"date"},
        "AMOUNT": {"type":"float"},
        "VERSION": {"type":"semver"},
        "ADDRESS": {"type":"ipv6"}
    });
    std::fs::write(dir.path().join("env.schema.json"), contract.to_string()).unwrap();
    let content = "BASE=8080\nPORT=${BASE}\nDAY=2025-02-29\nAMOUNT=NaN\nVERSION=01.2.3\nADDRESS=2001:db8::1\n";
    std::fs::write(dir.path().join(".env"), content).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(dir.path())
        .args(["fix", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for invalid in [
        "DAY: expected date",
        "AMOUNT: expected finite float",
        "VERSION: expected semver",
    ] {
        assert!(stdout.contains(invalid), "{stdout}");
    }
    assert!(!stdout.contains("PORT:"), "{stdout}");
    assert!(!stdout.contains("ADDRESS:"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".env")).unwrap(),
        content
    );
}

#[test]
fn check_and_fix_mask_numeric_secrets_in_diagnostics() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("env.schema.json"),
        r#"{"VALUE":{"type":"int","secret":true,"validate":{"max":10}}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join(".env"), "VALUE=8675309\n").unwrap();
    for arguments in [vec!["check", "--format", "json"], vec!["fix", "--dry-run"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
            .current_dir(dir.path())
            .args(arguments)
            .output()
            .unwrap();
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(combined.contains("***MASKED***"), "{combined}");
        assert!(!combined.contains("8675309"), "{combined}");
    }
}
