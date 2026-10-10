use std::process::{Command, Output};
use tempfile::TempDir;

fn run(dir: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(dir.path())
        .env_remove("ZENV_QUIET")
        .env_remove("ZENV_VERBOSE")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn config_errors_do_not_silently_disable_configuration() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
    std::fs::write(dir.path().join("env.schema.json"), "{}").unwrap();
    std::fs::write(dir.path().join(".env"), "").unwrap();
    let output = run(&dir, &["--config", "missing.json", "check"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    for content in ["{", r#"{"detect_secrets":"yes"}"#] {
        std::fs::write(dir.path().join(".zenvrc"), content).unwrap();
        let output = run(&dir, &["check"]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
}

#[test]
fn config_paths_are_relative_to_the_config_file() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(
        dir.path().join(".zenvrc"),
        r#"{"schema":"contract.json","env":"project.env"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("contract.json"),
        r#"{"VALUE":{"type":"int"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("project.env"), "VALUE=5\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(&sub)
        .args(["check"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(&sub)
        .args(["doctor"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let display = String::from_utf8(output.stdout).unwrap();
    assert!(display.contains(".zenvrc (valid)"), "{display}");
    assert!(display.contains("Schema validation passed"), "{display}");
    std::fs::write(sub.join("override.env"), "VALUE=invalid\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zenv"))
        .current_dir(&sub)
        .args(["check", "--env", "override.env"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn schema_typos_and_invalid_defaults_fail_even_without_env_values() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    for schema in [
        r#"{"PORT":{"type":"int","requried":true}}"#,
        r#"{"PORT":{"type":"int","validate":{"minimum":10}}}"#,
        r#"{"PORT":{"type":"int","default":"not-an-int"}}"#,
        r#"{"MODE":{"type":"enum"}}"#,
        r#"{"PORT":{"type":"int","validate":{"min":10,"max":1}}}"#,
        r#"{"NAME":{"type":"string","validate":{"pattern":"["}}}"#,
        r#"{"PORT":{"type":"port","validate":{"min":1000}}}"#,
    ] {
        std::fs::write(dir.path().join("env.schema.json"), schema).unwrap();
        let output = run(&dir, &["check", "--allow-missing-env"]);
        assert_eq!(
            output.status.code(),
            Some(3),
            "accepted {schema}: {output:?}"
        );
    }
}

#[test]
fn invalid_output_format_is_rejected() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(dir.path().join("env.schema.json"), "{}").unwrap();
    std::fs::write(dir.path().join(".env"), "").unwrap();
    let output = run(&dir, &["check", "--format", "jsno"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn doctor_validates_discovered_files_and_reports_selected_config() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("chosen.json"), "{}").unwrap();
    std::fs::write(dir.path().join("env.schema.yaml"), "PORT:\n  type: int\n").unwrap();
    std::fs::write(dir.path().join(".env.local"), "PORT=invalid\n").unwrap();
    let output = run(&dir, &["--config", "chosen.json", "doctor"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let display = String::from_utf8(output.stdout).unwrap();
    assert!(display.contains("Found chosen.json (valid)"), "{display}");
    assert!(display.contains("1 validation error(s)"), "{display}");
    for flag in ["--schema", "--env"] {
        let output = run(
            &dir,
            &["--config", "chosen.json", "doctor", flag, "missing.file"],
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(!String::from_utf8(output.stdout)
            .unwrap()
            .contains("Schema validation passed"));
    }
}

#[test]
fn example_marks_unsynthesized_constraints_as_incomplete() {
    let schema =
        serde_json::from_str(r#"{"VALUE":{"validate":{"pattern":"^[0-9]{4}$"}}}"#).unwrap();
    let example = zorath_env::commands::example::generate(&schema, false);
    assert!(
        example.contains("this placeholder is incomplete"),
        "{example}"
    );
}

#[test]
fn diff_masks_declared_secrets_and_fails_closed_on_bad_schema() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(dir.path().join("a.env"), "VALUE=private-one\n").unwrap();
    std::fs::write(dir.path().join("b.env"), "VALUE=private-two\n").unwrap();
    std::fs::write(
        dir.path().join("schema.json"),
        r#"{"VALUE":{"secret":true}}"#,
    )
    .unwrap();
    for format in ["text", "json"] {
        let output = run(
            &dir,
            &[
                "diff",
                "a.env",
                "b.env",
                "--schema",
                "schema.json",
                "--format",
                format,
            ],
        );
        assert!(output.status.success(), "{output:?}");
        let display = String::from_utf8(output.stdout).unwrap();
        assert!(
            display.contains("***MASKED***") && !display.contains("private-"),
            "{display}"
        );
        let output = run(
            &dir,
            &[
                "diff",
                "a.env",
                "b.env",
                "--schema",
                "missing.json",
                "--format",
                format,
            ],
        );
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn doctor_reports_interpolation_errors_and_honors_warning_severity() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(
        dir.path().join("env.schema.json"),
        r#"{"VALUE":{"type":"int","severity":"warning"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join(".env"), "VALUE=bad\n").unwrap();
    assert!(run(&dir, &["doctor"]).status.success());
    std::fs::write(dir.path().join(".env"), "VALUE=${OTHER}\nOTHER=${VALUE}\n").unwrap();
    let output = run(&dir, &["doctor"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("circular"));
}

#[test]
fn preset_examples_satisfy_their_own_contracts() {
    use zorath_env::{
        commands::{check, example},
        envfile, presets, schema,
    };
    for name in presets::AVAILABLE_PRESETS {
        let contract = presets::get_preset(name).unwrap();
        schema::validate_schema(&contract).unwrap();
        let generated = example::generate(&contract, true);
        let values = envfile::interpolate_env(envfile::parse_env_str(&generated)).unwrap();
        let errors = check::validate(&contract, &values);
        assert!(errors.is_empty(), "{name}: {errors:?}");
        assert!(generated.contains("Placeholder only:"));
    }
}

#[test]
fn example_hints_do_not_override_types_and_comments_mask_secrets() {
    use zorath_env::{
        commands::{check, example},
        envfile, schema,
    };
    let contract: schema::Schema = serde_json::from_value(serde_json::json!({
        "DATABASE_PORT":{"type":"int","validate":{"min":8000,"max":9000}},
        "APP_ENV":{"type":"enum","values":["staging","production"]},
        "HOST":{"type":"ipv4"},
        "VALUE":{"type":"string","secret":true,"default":"do-not-print-me"}
    }))
    .unwrap();
    let generated = example::generate(&contract, false);
    assert!(!generated.contains("do-not-print-me"));
    assert!(
        check::validate(&contract, &envfile::parse_env_str(&generated)).is_empty(),
        "{generated}"
    );
}

struct Watching {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Watching {
    fn start(dir: &TempDir) -> Self {
        use std::io::BufRead;
        let mut child = Command::new(env!("CARGO_BIN_EXE_zenv"))
            .current_dir(dir.path())
            .args(["check", "--watch"])
            .env_remove("ZENV_QUIET")
            .env_remove("ZENV_VERBOSE")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        let streams: Vec<Box<dyn std::io::Read + Send>> = vec![
            Box::new(child.stdout.take().unwrap()),
            Box::new(child.stderr.take().unwrap()),
        ];
        for stream in streams {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(stream)
                    .lines()
                    .map_while(Result::ok)
                {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Self { child, lines }
    }

    fn until(&self, needle: &str) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut output = String::new();
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match self.lines.recv_timeout(remaining) {
                Ok(line) => {
                    output.push_str(&line);
                    output.push('\n');
                    if line.contains(needle) {
                        return output;
                    }
                }
                Err(_) => break,
            }
        }
        panic!("watch never printed {needle:?}: {output}");
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn watch_honors_warning_severity_at_startup() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(
        dir.path().join("env.schema.json"),
        r#"{"VALUE":{"type":"int","severity":"warning"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join(".env"), "VALUE=bad\n").unwrap();
    let watch = Watching::start(&dir);
    assert!(watch.until("Initial:").contains("Initial: OK"));
}

#[test]
fn watch_tracks_parent_schemas_and_repeated_atomic_saves() {
    use std::io::Write;
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".zenvrc"), "{}").unwrap();
    std::fs::write(
        dir.path().join("env.schema.json"),
        r#"{"extends":"base.json"}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("base.json"), r#"{"VALUE":{"type":"int"}}"#).unwrap();
    std::fs::write(dir.path().join(".env"), "VALUE=7\n").unwrap();
    let watch = Watching::start(&dir);
    watch.until("Press Ctrl+C");
    std::fs::write(dir.path().join("base.json"), r#"{"VALUE":{"type":"bool"}}"#).unwrap();
    watch.until("FAILED");
    for value in ["true", "false", "yes"] {
        let mut staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        writeln!(staged, "VALUE={value}").unwrap();
        staged.persist(dir.path().join(".env")).unwrap();
        watch.until(&format!("-> \"{value}\""));
    }
}
