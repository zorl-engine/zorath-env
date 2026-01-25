//! Integration tests for zorath-env
//!
//! These tests verify end-to-end command execution with real files.
//! Uses tempfile for isolated test environments.
//!
//! # Test Strategy
//! - Uses library functions (schema::load_schema_with_options, envfile::parse_env_file)
//!   to parse files, then calls validation/generation functions
//! - For CLI commands (export, fix, init), uses run() functions with file paths

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use zorath_env::commands::{check, docs, example, export, fix, init};
use zorath_env::envfile;
use zorath_env::schema::{self, LoadOptions, Schema, VarSpec, VarType};

/// Test environment helper - creates isolated temp directory with .env and schema files
struct TestEnv {
    #[allow(dead_code)]
    temp_dir: TempDir,
    pub env_path: PathBuf,
    pub schema_path: PathBuf,
    pub base_path: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let base_path = temp_dir.path().to_path_buf();

        TestEnv {
            env_path: base_path.join(".env"),
            schema_path: base_path.join("env.schema.json"),
            base_path,
            temp_dir,
        }
    }

    fn write_env(&self, content: &str) {
        fs::write(&self.env_path, content).expect("Failed to write .env file");
    }

    fn write_schema(&self, content: &str) {
        fs::write(&self.schema_path, content).expect("Failed to write schema file");
    }

    fn read_file(&self, path: &PathBuf) -> String {
        fs::read_to_string(path).expect("Failed to read file")
    }

    fn env_str(&self) -> &str {
        self.env_path.to_str().unwrap()
    }

    fn schema_str(&self) -> &str {
        self.schema_path.to_str().unwrap()
    }

    /// Load schema from file
    fn load_schema(&self) -> Schema {
        let opts = LoadOptions::default();
        schema::load_schema_with_options(self.schema_str(), &opts)
            .expect("Failed to load schema")
    }

    /// Parse env file into HashMap
    fn parse_env(&self) -> HashMap<String, String> {
        envfile::parse_env_file(self.env_str())
            .expect("Failed to parse env file")
    }

    /// Parse env file and interpolate variables
    fn parse_and_interpolate_env(&self) -> HashMap<String, String> {
        let env_map = self.parse_env();
        envfile::interpolate_env(env_map)
            .expect("Failed to interpolate env")
    }
}

// =============================================================================
// CHECK COMMAND TESTS - Using check::validate(schema, env_map)
// =============================================================================

#[test]
fn test_check_valid_env() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "Valid env should pass validation: {:?}", errors);
}

#[test]
fn test_check_missing_required() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "API_KEY": {"type": "string", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Missing required var should fail");
    let err_str = errors.join("\n");
    assert!(err_str.contains("API_KEY"), "Error should mention missing key");
    assert!(err_str.contains("missing"), "Error should say missing");
}

#[test]
fn test_check_type_validation_int() {
    let env = TestEnv::new();
    env.write_env("PORT=not_a_number\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Invalid int should fail");
    let err_str = errors.join("\n");
    assert!(err_str.contains("PORT"), "Error should mention the key");
    assert!(err_str.contains("int"), "Error should mention expected type");
}

#[test]
fn test_check_type_validation_url() {
    let env = TestEnv::new();
    env.write_env("DATABASE_URL=not-a-valid-url\n");
    env.write_schema(r#"{
        "DATABASE_URL": {"type": "url", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Invalid URL should fail");
}

#[test]
fn test_check_type_validation_port() {
    let env = TestEnv::new();
    env.write_env("PORT=99999\n");
    env.write_schema(r#"{
        "PORT": {"type": "port", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Port > 65535 should fail");
}

#[test]
fn test_check_type_validation_bool() {
    let env = TestEnv::new();
    env.write_env("DEBUG=maybe\n");
    env.write_schema(r#"{
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Invalid bool should fail");
}

#[test]
fn test_check_enum_validation() {
    let env = TestEnv::new();
    env.write_env("LOG_LEVEL=verbose\n");
    env.write_schema(r#"{
        "LOG_LEVEL": {"type": "enum", "values": ["error", "warn", "info", "debug"], "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Invalid enum value should fail");
    let err_str = errors.join("\n");
    assert!(err_str.contains("verbose"), "Error should mention the invalid value");
}

#[test]
fn test_check_validation_rules_min_max() {
    let env = TestEnv::new();
    env.write_env("PORT=80\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "validate": {"min": 1024, "max": 65535}}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Value below min should fail");
}

#[test]
fn test_check_default_value_not_required_when_missing() {
    let env = TestEnv::new();
    env.write_env(""); // Empty env
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": false, "default": 3000}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "Optional with default should pass when missing");
}

#[test]
fn test_check_required_with_default_passes_when_missing() {
    let env = TestEnv::new();
    env.write_env(""); // Empty env
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "default": 3000}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    // Required with default should pass - the default satisfies the requirement
    assert!(errors.is_empty(), "Required with default should pass when missing");
}

// =============================================================================
// DOCS COMMAND TESTS - Using docs::generate_markdown and generate_json
// =============================================================================

#[test]
fn test_docs_generates_markdown() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "description": "HTTP server port"},
        "DEBUG": {"type": "bool", "default": false, "description": "Enable debug mode"}
    }"#);

    let schema = env.load_schema();
    let output = docs::generate_markdown(&schema);

    assert!(output.contains("# Environment Variables"), "Should have header");
    assert!(output.contains("PORT"), "Should mention PORT");
    assert!(output.contains("DEBUG"), "Should mention DEBUG");
    assert!(output.contains("HTTP server port"), "Should include description");
}

#[test]
fn test_docs_generates_json() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let schema = env.load_schema();
    let result = docs::generate_json(&schema);

    assert!(result.is_ok(), "Docs generation should succeed");
    let output = result.unwrap();
    assert!(output.contains("\"PORT\""), "Should have PORT in JSON");
    assert!(output.contains("\"type\""), "Should have type field");
}

#[test]
fn test_docs_sorted_alphabetically() {
    let schema: Schema = [
        ("ZEBRA".to_string(), VarSpec { var_type: VarType::String, ..Default::default() }),
        ("ALPHA".to_string(), VarSpec { var_type: VarType::String, ..Default::default() }),
        ("MIDDLE".to_string(), VarSpec { var_type: VarType::String, ..Default::default() }),
    ].into_iter().collect();

    let output = docs::generate_markdown(&schema);
    let alpha_pos = output.find("ALPHA").unwrap();
    let middle_pos = output.find("MIDDLE").unwrap();
    let zebra_pos = output.find("ZEBRA").unwrap();

    assert!(alpha_pos < middle_pos, "ALPHA should come before MIDDLE");
    assert!(middle_pos < zebra_pos, "MIDDLE should come before ZEBRA");
}

// =============================================================================
// FIX COMMAND TESTS - Using fix::run() with file paths
// Signature: run(env_path, schema_path, remove_unknown, dry_run, no_cache, verify_hash, ca_cert)
// =============================================================================

#[test]
fn test_fix_dry_run_no_changes() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let original_content = env.read_file(&env.env_path);

    let _result = fix::run(
        env.env_str(),
        env.schema_str(),
        false, // remove_unknown
        true,  // dry_run
        false, // no_cache
        None,  // verify_hash
        None,  // ca_cert
    );

    // File should be unchanged after dry run
    let after_content = env.read_file(&env.env_path);
    assert_eq!(original_content, after_content, "Dry run should not modify file");
}

#[test]
fn test_fix_removes_unknown_keys() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nUNKNOWN=value\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let _result = fix::run(
        env.env_str(),
        env.schema_str(),
        true,  // remove_unknown
        false, // dry_run
        false, // no_cache
        None,  // verify_hash
        None,  // ca_cert
    );

    let content = env.read_file(&env.env_path);
    assert!(content.contains("PORT"), "Should keep known key");
    assert!(!content.contains("UNKNOWN"), "Should remove unknown key");
}

#[test]
fn test_fix_adds_missing_with_defaults() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "LOG_LEVEL": {"type": "string", "required": true, "default": "info"}
    }"#);

    // Fix will add missing required vars with defaults
    let _result = fix::run(
        env.env_str(),
        env.schema_str(),
        false, // remove_unknown
        false, // dry_run
        false, // no_cache
        None,  // verify_hash
        None,  // ca_cert
    );

    let content = env.read_file(&env.env_path);
    assert!(content.contains("PORT"), "Should keep existing key");
    assert!(content.contains("LOG_LEVEL"), "Should add missing key");
    assert!(content.contains("info"), "Should use default value");
}

// =============================================================================
// INIT COMMAND TESTS - Using init::run() with file paths
// =============================================================================

#[test]
fn test_init_creates_schema_from_env() {
    let env = TestEnv::new();
    let example_path = env.base_path.join(".env.example");
    fs::write(&example_path, "PORT=3000\nDEBUG=true\nAPI_URL=https://api.example.com\n").unwrap();

    let result = init::run(
        example_path.to_str().unwrap(),
        env.schema_str(),
        None, // no preset
    );

    assert!(result.is_ok(), "Init should succeed");

    let schema_content = env.read_file(&env.schema_path);
    assert!(schema_content.contains("PORT"), "Schema should have PORT");
    assert!(schema_content.contains("DEBUG"), "Schema should have DEBUG");
    assert!(schema_content.contains("API_URL"), "Schema should have API_URL");
}

#[test]
fn test_init_type_inference() {
    let env = TestEnv::new();
    let example_path = env.base_path.join(".env.example");
    fs::write(&example_path, "PORT=3000\nDEBUG=true\nRATE=1.5\nURL=https://example.com\n").unwrap();

    let result = init::run(
        example_path.to_str().unwrap(),
        env.schema_str(),
        None,
    );

    assert!(result.is_ok(), "Init should succeed");

    let schema_content = env.read_file(&env.schema_path);
    // Type inference: int for PORT, bool for DEBUG, float for RATE, url for URL
    assert!(schema_content.contains("\"int\""), "Should infer int type");
    assert!(schema_content.contains("\"bool\""), "Should infer bool type");
    assert!(schema_content.contains("\"float\""), "Should infer float type");
    assert!(schema_content.contains("\"url\""), "Should infer url type");
}

// =============================================================================
// YAML SCHEMA TESTS
// =============================================================================

#[test]
fn test_yaml_schema_validation() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");

    let yaml_schema_path = env.base_path.join("env.schema.yaml");
    fs::write(&yaml_schema_path, r#"
PORT:
  type: int
  required: true
  description: HTTP server port
"#).unwrap();

    let opts = LoadOptions::default();
    let schema = schema::load_schema_with_options(yaml_schema_path.to_str().unwrap(), &opts)
        .expect("Should load YAML schema");
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "YAML schema validation should work: {:?}", errors);
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[test]
fn test_empty_env_file() {
    let env = TestEnv::new();
    env.write_env("");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": false, "default": 3000}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "Empty env with optional vars should pass");
}

#[test]
fn test_empty_schema_flags_unknown_keys() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema("{}");

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    // Empty schema means all keys are unknown - validate() correctly flags them
    assert!(!errors.is_empty(), "Empty schema should flag unknown keys");
    assert!(errors[0].contains("PORT"), "Should flag PORT as unknown");
    assert!(errors[0].contains("not in schema"), "Should say not in schema");
}

#[test]
fn test_variable_interpolation() {
    let env = TestEnv::new();
    env.write_env("BASE_URL=https://api.example.com\nAPI_ENDPOINT=${BASE_URL}/v1\n");
    env.write_schema(r#"{
        "BASE_URL": {"type": "url", "required": true},
        "API_ENDPOINT": {"type": "url", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_and_interpolate_env();
    let errors = check::validate(&schema, &env_map);

    // After interpolation, API_ENDPOINT should be https://api.example.com/v1
    assert!(errors.is_empty(), "Variable interpolation should work: {:?}", errors);
    assert_eq!(env_map.get("API_ENDPOINT").unwrap(), "https://api.example.com/v1");
}

#[test]
fn test_comments_and_blank_lines() {
    let env = TestEnv::new();
    env.write_env("# This is a comment\n\nPORT=3000\n\n# Another comment\nDEBUG=true\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "Comments and blank lines should be ignored: {:?}", errors);
}

#[test]
fn test_export_prefix_syntax() {
    let env = TestEnv::new();
    env.write_env("export PORT=3000\nexport DEBUG=true\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "export prefix should be supported: {:?}", errors);
}

// =============================================================================
// ALL TYPE VALIDATIONS
// =============================================================================

#[test]
fn test_all_valid_types() {
    let env = TestEnv::new();
    env.write_env(r#"
STRING_VAR=hello
INT_VAR=42
FLOAT_VAR=3.14
BOOL_VAR=true
URL_VAR=https://example.com
EMAIL_VAR=test@example.com
UUID_VAR=550e8400-e29b-41d4-a716-446655440000
IPV4_VAR=192.168.1.1
PORT_VAR=8080
SEMVER_VAR=1.2.3
DATE_VAR=2024-01-15
HOSTNAME_VAR=api.example.com
ENUM_VAR=production
"#);
    env.write_schema(r#"{
        "STRING_VAR": {"type": "string", "required": true},
        "INT_VAR": {"type": "int", "required": true},
        "FLOAT_VAR": {"type": "float", "required": true},
        "BOOL_VAR": {"type": "bool", "required": true},
        "URL_VAR": {"type": "url", "required": true},
        "EMAIL_VAR": {"type": "email", "required": true},
        "UUID_VAR": {"type": "uuid", "required": true},
        "IPV4_VAR": {"type": "ipv4", "required": true},
        "PORT_VAR": {"type": "port", "required": true},
        "SEMVER_VAR": {"type": "semver", "required": true},
        "DATE_VAR": {"type": "date", "required": true},
        "HOSTNAME_VAR": {"type": "hostname", "required": true},
        "ENUM_VAR": {"type": "enum", "values": ["development", "staging", "production"], "required": true}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "All valid types should pass: {:?}", errors);
}

#[test]
fn test_validation_rules_string_length() {
    let env = TestEnv::new();
    env.write_env("API_KEY=abc\n"); // Too short
    env.write_schema(r#"{
        "API_KEY": {"type": "string", "required": true, "validate": {"min_length": 10, "max_length": 100}}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "String below min_length should fail");
    assert!(errors[0].contains("length"), "Error should mention length");
}

#[test]
fn test_validation_rules_pattern() {
    let env = TestEnv::new();
    env.write_env("API_KEY=invalid_key\n");
    env.write_schema(r#"{
        "API_KEY": {"type": "string", "required": true, "validate": {"pattern": "^sk_[a-zA-Z0-9]+$"}}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "String not matching pattern should fail");
    assert!(errors[0].contains("pattern"), "Error should mention pattern");
}

#[test]
fn test_validation_rules_float_range() {
    let env = TestEnv::new();
    env.write_env("RATE=2.5\n"); // Above max_value
    env.write_schema(r#"{
        "RATE": {"type": "float", "required": true, "validate": {"min_value": 0.0, "max_value": 1.0}}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Float above max_value should fail");
}

// =============================================================================
// SCHEMA INHERITANCE TESTS
// =============================================================================

#[test]
fn test_schema_inheritance() {
    let env = TestEnv::new();

    // Create base schema in the same directory
    let base_path = env.base_path.join("base.schema.json");
    fs::write(&base_path, r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "default": false}
    }"#).unwrap();

    // Create child schema that extends base (using "extends", not "$extends")
    // The path must be relative to the child schema location
    env.write_schema(r#"{
        "extends": "base.schema.json",
        "API_KEY": {"type": "string", "required": true}
    }"#);

    env.write_env("PORT=3000\nDEBUG=true\nAPI_KEY=secret123\n");

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    // Should validate against merged schema
    assert!(errors.is_empty(), "Schema inheritance should work: {:?}", errors);
    assert!(schema.contains_key("PORT"), "Should have PORT from base");
    assert!(schema.contains_key("DEBUG"), "Should have DEBUG from base");
    assert!(schema.contains_key("API_KEY"), "Should have API_KEY from child");
}

// =============================================================================
// EXPORT COMMAND TESTS - Using export::export_to_string()
// =============================================================================

#[test]
fn test_export_shell_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\nAPI_KEY=secret123\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::Shell);

    assert!(result.is_ok(), "Shell export should succeed");
    let output = result.unwrap();
    assert!(output.contains("export PORT="), "Should have export prefix");
    assert!(output.contains("3000"), "Should include value");
}

#[test]
fn test_export_docker_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::Docker);

    assert!(result.is_ok(), "Docker export should succeed");
    let output = result.unwrap();
    assert!(output.contains("ENV PORT="), "Should have ENV prefix");
    assert!(output.contains("ENV DEBUG="), "Should have ENV prefix");
}

#[test]
fn test_export_k8s_configmap_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDATABASE_URL=postgres://localhost/db\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::K8s);

    assert!(result.is_ok(), "K8s export should succeed");
    let output = result.unwrap();
    assert!(output.contains("apiVersion: v1"), "Should have K8s version");
    assert!(output.contains("kind: ConfigMap"), "Should be ConfigMap");
    assert!(output.contains("PORT:"), "Should have PORT key");
}

#[test]
fn test_export_json_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::Json);

    assert!(result.is_ok(), "JSON export should succeed");
    let output = result.unwrap();
    // Verify it's valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&output);
    assert!(parsed.is_ok(), "Output should be valid JSON");
    let json = parsed.unwrap();
    assert_eq!(json["PORT"], "3000");
    assert_eq!(json["DEBUG"], "true");
}

#[test]
fn test_export_systemd_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::Systemd);

    assert!(result.is_ok(), "Systemd export should succeed");
    let output = result.unwrap();
    // Systemd format uses: Environment="KEY=VALUE"
    assert!(output.contains("Environment=\"PORT=3000\""), "Should have systemd format");
}

#[test]
fn test_export_dotenv_format() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::Dotenv);

    assert!(result.is_ok(), "Dotenv export should succeed");
    let output = result.unwrap();
    assert!(output.contains("PORT=3000"), "Should have standard dotenv format");
    assert!(output.contains("DEBUG=true"), "Should include all vars");
}

#[test]
fn test_export_github_secrets_format() {
    let env = TestEnv::new();
    env.write_env("API_KEY=secret123\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::GithubSecrets);

    assert!(result.is_ok(), "GitHub secrets export should succeed");
    let output = result.unwrap();
    assert!(output.contains("gh secret set"), "Should have gh CLI command");
    assert!(output.contains("API_KEY"), "Should include key name");
}

#[test]
fn test_export_format_from_str() {
    // Test format parsing with aliases
    assert!("shell".parse::<export::ExportFormat>().is_ok());
    assert!("bash".parse::<export::ExportFormat>().is_ok());
    assert!("docker".parse::<export::ExportFormat>().is_ok());
    assert!("dockerfile".parse::<export::ExportFormat>().is_ok());
    assert!("k8s".parse::<export::ExportFormat>().is_ok());
    assert!("kubernetes".parse::<export::ExportFormat>().is_ok());
    assert!("json".parse::<export::ExportFormat>().is_ok());
    assert!("systemd".parse::<export::ExportFormat>().is_ok());
    assert!("dotenv".parse::<export::ExportFormat>().is_ok());
    assert!("github-secrets".parse::<export::ExportFormat>().is_ok());
    assert!("invalid".parse::<export::ExportFormat>().is_err());
}

// =============================================================================
// EXAMPLE COMMAND TESTS - Using example::generate()
// =============================================================================

#[test]
fn test_example_generates_env_file() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "description": "HTTP server port"},
        "DEBUG": {"type": "bool", "default": false, "description": "Enable debug mode"}
    }"#);

    let schema = env.load_schema();
    let output = example::generate(&schema, true);

    assert!(output.contains("PORT="), "Should have PORT");
    assert!(output.contains("DEBUG="), "Should have DEBUG");
    assert!(output.contains("# HTTP server port"), "Should include description");
}

#[test]
fn test_example_with_defaults() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "int", "default": 3000},
        "LOG_LEVEL": {"type": "enum", "values": ["debug", "info", "warn"], "default": "info"}
    }"#);

    let schema = env.load_schema();
    let output = example::generate(&schema, true);

    assert!(output.contains("PORT=3000"), "Should use default value");
    assert!(output.contains("LOG_LEVEL=info"), "Should use enum default");
}

#[test]
fn test_example_without_defaults_uses_placeholders() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "API_URL": {"type": "url", "required": true},
        "API_KEY": {"type": "string", "required": true}
    }"#);

    let schema = env.load_schema();
    let output = example::generate(&schema, false);

    // Should have type-aware placeholders, not actual values
    assert!(output.contains("PORT="), "Should have PORT");
    assert!(output.contains("API_URL="), "Should have API_URL");
    assert!(output.contains("API_KEY="), "Should have API_KEY");
}

#[test]
fn test_example_includes_type_comments() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "PORT": {"type": "port", "required": true}
    }"#);

    let schema = env.load_schema();
    let output = example::generate(&schema, false);

    assert!(output.contains("# Type: port"), "Should show type in comment");
    assert!(output.contains("required"), "Should show required status");
}

#[test]
fn test_example_all_types_have_placeholders() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "STRING_VAR": {"type": "string"},
        "INT_VAR": {"type": "int"},
        "FLOAT_VAR": {"type": "float"},
        "BOOL_VAR": {"type": "bool"},
        "URL_VAR": {"type": "url"},
        "EMAIL_VAR": {"type": "email"},
        "UUID_VAR": {"type": "uuid"},
        "IPV4_VAR": {"type": "ipv4"},
        "PORT_VAR": {"type": "port"},
        "SEMVER_VAR": {"type": "semver"},
        "DATE_VAR": {"type": "date"},
        "HOSTNAME_VAR": {"type": "hostname"}
    }"#);

    let schema = env.load_schema();
    let output = example::generate(&schema, false);

    // All variables should be present
    assert!(output.contains("STRING_VAR="), "Should have STRING_VAR");
    assert!(output.contains("INT_VAR="), "Should have INT_VAR");
    assert!(output.contains("FLOAT_VAR="), "Should have FLOAT_VAR");
    assert!(output.contains("BOOL_VAR="), "Should have BOOL_VAR");
    assert!(output.contains("URL_VAR="), "Should have URL_VAR");
    assert!(output.contains("EMAIL_VAR="), "Should have EMAIL_VAR");
    assert!(output.contains("UUID_VAR="), "Should have UUID_VAR");
    assert!(output.contains("IPV4_VAR="), "Should have IPV4_VAR");
    assert!(output.contains("PORT_VAR="), "Should have PORT_VAR");
    assert!(output.contains("SEMVER_VAR="), "Should have SEMVER_VAR");
    assert!(output.contains("DATE_VAR="), "Should have DATE_VAR");
    assert!(output.contains("HOSTNAME_VAR="), "Should have HOSTNAME_VAR");
}

// =============================================================================
// DIFF COMPARISON TESTS - Using envfile parsing and comparison
// =============================================================================

#[test]
fn test_diff_finds_only_in_first_file() {
    let env = TestEnv::new();

    let env_a_path = env.base_path.join(".env.a");
    let env_b_path = env.base_path.join(".env.b");

    fs::write(&env_a_path, "PORT=3000\nAPI_KEY=secret\n").unwrap();
    fs::write(&env_b_path, "PORT=3000\n").unwrap();

    let map_a = envfile::parse_env_file(env_a_path.to_str().unwrap()).unwrap();
    let map_b = envfile::parse_env_file(env_b_path.to_str().unwrap()).unwrap();

    // API_KEY is only in file A
    assert!(map_a.contains_key("API_KEY"), "File A should have API_KEY");
    assert!(!map_b.contains_key("API_KEY"), "File B should not have API_KEY");
}

#[test]
fn test_diff_finds_only_in_second_file() {
    let env = TestEnv::new();

    let env_a_path = env.base_path.join(".env.a");
    let env_b_path = env.base_path.join(".env.b");

    fs::write(&env_a_path, "PORT=3000\n").unwrap();
    fs::write(&env_b_path, "PORT=3000\nDEBUG=true\n").unwrap();

    let map_a = envfile::parse_env_file(env_a_path.to_str().unwrap()).unwrap();
    let map_b = envfile::parse_env_file(env_b_path.to_str().unwrap()).unwrap();

    // DEBUG is only in file B
    assert!(!map_a.contains_key("DEBUG"), "File A should not have DEBUG");
    assert!(map_b.contains_key("DEBUG"), "File B should have DEBUG");
}

#[test]
fn test_diff_finds_value_differences() {
    let env = TestEnv::new();

    let env_a_path = env.base_path.join(".env.a");
    let env_b_path = env.base_path.join(".env.b");

    fs::write(&env_a_path, "PORT=3000\nDEBUG=true\n").unwrap();
    fs::write(&env_b_path, "PORT=8080\nDEBUG=true\n").unwrap();

    let map_a = envfile::parse_env_file(env_a_path.to_str().unwrap()).unwrap();
    let map_b = envfile::parse_env_file(env_b_path.to_str().unwrap()).unwrap();

    // PORT has different values
    assert_eq!(map_a.get("PORT").unwrap(), "3000");
    assert_eq!(map_b.get("PORT").unwrap(), "8080");
    // DEBUG is the same
    assert_eq!(map_a.get("DEBUG"), map_b.get("DEBUG"));
}

#[test]
fn test_diff_identical_files_have_no_differences() {
    let env = TestEnv::new();

    let env_a_path = env.base_path.join(".env.a");
    let env_b_path = env.base_path.join(".env.b");

    fs::write(&env_a_path, "PORT=3000\nDEBUG=true\n").unwrap();
    fs::write(&env_b_path, "PORT=3000\nDEBUG=true\n").unwrap();

    let map_a = envfile::parse_env_file(env_a_path.to_str().unwrap()).unwrap();
    let map_b = envfile::parse_env_file(env_b_path.to_str().unwrap()).unwrap();

    assert_eq!(map_a, map_b, "Identical files should have identical maps");
}

// =============================================================================
// LIBRARY API INTEGRATION TESTS
// =============================================================================

#[test]
fn test_validate_files_library_function() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    // Test the validate_files library function
    let opts = LoadOptions::default();
    let result = check::validate_files(
        env.env_str(),
        env.schema_str(),
        &opts,
    );

    assert!(result.is_ok(), "validate_files should succeed");
    let errors = result.unwrap();
    assert!(errors.is_empty(), "Valid files should have no errors");
}

#[test]
fn test_validate_files_with_errors() {
    let env = TestEnv::new();
    env.write_env("PORT=not_a_number\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(
        env.env_str(),
        env.schema_str(),
        &opts,
    );

    assert!(result.is_ok(), "validate_files should succeed even with validation errors");
    let errors = result.unwrap();
    assert!(!errors.is_empty(), "Invalid PORT should produce errors");
}

#[test]
fn test_docs_generate_library_function() {
    let schema: Schema = [
        ("PORT".to_string(), VarSpec {
            var_type: VarType::Int,
            required: true,
            description: Some("Server port".to_string()),
            ..Default::default()
        }),
    ].into_iter().collect();

    let markdown = docs::generate_markdown(&schema);
    let json_result = docs::generate_json(&schema);

    assert!(markdown.contains("PORT"), "Markdown should contain PORT");
    assert!(markdown.contains("Server port"), "Markdown should contain description");
    assert!(json_result.is_ok(), "JSON generation should succeed");
}

#[test]
fn test_example_generate_library_function() {
    let schema: Schema = [
        ("PORT".to_string(), VarSpec {
            var_type: VarType::Int,
            required: true,
            default: Some(serde_json::Value::from(3000)),
            ..Default::default()
        }),
    ].into_iter().collect();

    let with_defaults = example::generate(&schema, true);
    let without_defaults = example::generate(&schema, false);

    assert!(with_defaults.contains("PORT=3000"), "With defaults should use default value");
    assert!(without_defaults.contains("PORT="), "Without defaults should still have key");
}

#[test]
fn test_export_library_function_all_formats() {
    let mut env_map = HashMap::new();
    env_map.insert("PORT".to_string(), "3000".to_string());
    env_map.insert("DEBUG".to_string(), "true".to_string());

    // Test all export formats
    let formats = [
        export::ExportFormat::Shell,
        export::ExportFormat::Docker,
        export::ExportFormat::K8s,
        export::ExportFormat::Json,
        export::ExportFormat::Systemd,
        export::ExportFormat::Dotenv,
        export::ExportFormat::GithubSecrets,
    ];

    for format in formats {
        let result = export::export_to_string(&env_map, format);
        assert!(result.is_ok(), "Export to {} should succeed", format);
        let output = result.unwrap();
        assert!(!output.is_empty(), "{} output should not be empty", format);
    }
}

// =============================================================================
// GITHUB SECRETS EXPORT INTEGRATION TESTS (v0.3.8)
// =============================================================================

#[test]
fn test_export_github_secrets_multiline_heredoc() {
    let env = TestEnv::new();
    env.write_env("PRIVATE_KEY=\"-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----\"\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::GithubSecrets);

    assert!(result.is_ok(), "GitHub secrets export should succeed");
    let output = result.unwrap();
    // Multiline should use heredoc
    assert!(output.contains("EOF") || output.contains("--body"), "Should handle multiline with heredoc or body");
}

#[test]
fn test_export_github_secrets_special_characters() {
    let env = TestEnv::new();
    env.write_env("CONFIG=value_with_$pecial_chars\n");

    let env_map = env.parse_env();
    let result = export::export_to_string(&env_map, export::ExportFormat::GithubSecrets);

    assert!(result.is_ok(), "GitHub secrets export should succeed");
    let output = result.unwrap();
    assert!(output.contains("gh secret set CONFIG"), "Should include gh secret set command");
}

// =============================================================================
// SEVERITY LEVEL INTEGRATION TESTS (v0.3.5)
// =============================================================================

#[test]
fn test_severity_warning_validation() {
    let env = TestEnv::new();
    env.write_env("PORT=not_a_number\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "severity": "warning"}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    // Should still report validation error even with warning severity
    assert!(!errors.is_empty(), "Should have validation error even with warning severity");
}

#[test]
fn test_severity_error_validation() {
    let env = TestEnv::new();
    env.write_env("PORT=not_a_number\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "severity": "error"}
    }"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Should have validation error with error severity");
}

// =============================================================================
// MIXED FORMAT SCHEMA INHERITANCE TESTS (v0.3.5)
// =============================================================================

#[test]
fn test_yaml_extends_json_schema() {
    let env = TestEnv::new();

    // Create JSON base schema
    let base_path = env.base_path.join("base.schema.json");
    fs::write(&base_path, r#"{
        "PORT": {"type": "int", "required": true}
    }"#).unwrap();

    // Create YAML child schema that extends JSON
    let yaml_schema_path = env.base_path.join("child.schema.yaml");
    fs::write(&yaml_schema_path, r#"
extends: base.schema.json
DEBUG:
  type: bool
  default: false
"#).unwrap();

    env.write_env("PORT=3000\nDEBUG=true\n");

    let opts = LoadOptions::default();
    let schema = schema::load_schema_with_options(yaml_schema_path.to_str().unwrap(), &opts)
        .expect("Should load YAML schema that extends JSON");

    // Should have both keys from inheritance
    assert!(schema.contains_key("PORT"), "Should have PORT from JSON base");
    assert!(schema.contains_key("DEBUG"), "Should have DEBUG from YAML child");
}

#[test]
fn test_json_extends_yaml_schema() {
    let env = TestEnv::new();

    // Create YAML base schema
    let base_path = env.base_path.join("base.schema.yaml");
    fs::write(&base_path, r#"
PORT:
  type: int
  required: true
"#).unwrap();

    // Create JSON child schema that extends YAML
    let json_schema_path = env.base_path.join("child.schema.json");
    fs::write(&json_schema_path, r#"{
        "extends": "base.schema.yaml",
        "DEBUG": {"type": "bool", "default": false}
    }"#).unwrap();

    env.write_env("PORT=3000\nDEBUG=true\n");

    let opts = LoadOptions::default();
    let schema = schema::load_schema_with_options(json_schema_path.to_str().unwrap(), &opts)
        .expect("Should load JSON schema that extends YAML");

    // Should have both keys from inheritance
    assert!(schema.contains_key("PORT"), "Should have PORT from YAML base");
    assert!(schema.contains_key("DEBUG"), "Should have DEBUG from JSON child");
}

// =============================================================================
// CONFIG FILE (.zenvrc) TESTS (v0.3.5)
// =============================================================================

#[test]
fn test_config_file_parsing() {
    let env = TestEnv::new();

    // Create .zenvrc config file
    let config_path = env.base_path.join(".zenvrc");
    fs::write(&config_path, r#"{
        "schema": "env.schema.json",
        "env": ".env",
        "no_cache": true
    }"#).unwrap();

    // Verify config file content is valid JSON
    let content = fs::read_to_string(&config_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("Config should be valid JSON");

    assert_eq!(parsed["schema"], "env.schema.json");
    assert_eq!(parsed["env"], ".env");
    assert_eq!(parsed["no_cache"], true);
}

// =============================================================================
// PRESET TESTS (v0.3.5)
// =============================================================================

#[test]
fn test_preset_nextjs_variables() {
    // Test that nextjs preset includes expected variables
    let preset = zorath_env::presets::get_preset("nextjs").expect("nextjs preset should exist");
    assert!(preset.contains_key("NEXT_PUBLIC_API_URL") || !preset.is_empty(), "nextjs preset should have variables");
}

#[test]
fn test_preset_django_variables() {
    let preset = zorath_env::presets::get_preset("django").expect("django preset should exist");
    assert!(!preset.is_empty(), "django preset should have variables");
}

#[test]
fn test_preset_rails_variables() {
    let preset = zorath_env::presets::get_preset("rails").expect("rails preset should exist");
    assert!(!preset.is_empty(), "rails preset should have variables");
}

#[test]
fn test_preset_fastapi_variables() {
    let preset = zorath_env::presets::get_preset("fastapi").expect("fastapi preset should exist");
    assert!(!preset.is_empty(), "fastapi preset should have variables");
}

#[test]
fn test_preset_express_variables() {
    let preset = zorath_env::presets::get_preset("express").expect("express preset should exist");
    assert!(!preset.is_empty(), "express preset should have variables");
}

#[test]
fn test_preset_laravel_variables() {
    let preset = zorath_env::presets::get_preset("laravel").expect("laravel preset should exist");
    assert!(!preset.is_empty(), "laravel preset should have variables");
}

// =============================================================================
// EXIT CODE BEHAVIOR TESTS (v0.3.0)
// Exit codes: 0 = success, 1 = validation error, 2 = file error, 3 = schema error
// These tests verify the library behavior that maps to exit codes
// =============================================================================

#[test]
fn test_exit_code_0_valid_env() {
    // Exit code 0: All validations pass
    let env = TestEnv::new();
    env.write_env("PORT=3000\nDEBUG=true\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok(), "Should not have file errors");
    let errors = result.unwrap();
    assert!(errors.is_empty(), "Should have zero validation errors -> exit code 0");
}

#[test]
fn test_exit_code_1_validation_error_missing_required() {
    // Exit code 1: Validation error - missing required variable
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "API_KEY": {"type": "string", "required": true}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok(), "Should not have file errors");
    let errors = result.unwrap();
    assert!(!errors.is_empty(), "Should have validation errors -> exit code 1");
    assert!(errors.iter().any(|e| e.contains("API_KEY") && e.contains("missing")));
}

#[test]
fn test_exit_code_1_validation_error_type_mismatch() {
    // Exit code 1: Validation error - type mismatch
    let env = TestEnv::new();
    env.write_env("PORT=not_a_number\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok(), "Should not have file errors");
    let errors = result.unwrap();
    assert!(!errors.is_empty(), "Type mismatch should produce validation errors -> exit code 1");
}

#[test]
fn test_exit_code_2_env_file_not_found() {
    // Exit code 2: File error - .env file does not exist
    let env = TestEnv::new();
    env.write_schema(r#"{"PORT": {"type": "int"}}"#);
    // Don't write env file

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_err(), "Missing env file should return error -> exit code 2");
    let err_msg = result.unwrap_err();
    assert!(err_msg.contains("env") || err_msg.contains("file") || err_msg.contains("read"),
            "Error should mention file issue: {}", err_msg);
}

#[test]
fn test_exit_code_3_schema_parse_error() {
    // Exit code 3: Schema error - invalid JSON
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema("{ invalid json }");

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_err(), "Invalid schema should return error -> exit code 3");
}

#[test]
fn test_exit_code_0_optional_vars_missing() {
    // Exit code 0: Optional variables missing is OK
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": false, "default": false}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok());
    let errors = result.unwrap();
    assert!(errors.is_empty(), "Optional vars missing should not cause errors -> exit code 0");
}

#[test]
fn test_exit_code_0_required_with_default_missing() {
    // Exit code 0: Required variable with default value is satisfied when missing
    let env = TestEnv::new();
    env.write_env("");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true, "default": 3000}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok());
    let errors = result.unwrap();
    assert!(errors.is_empty(), "Required with default satisfied -> exit code 0");
}

#[test]
fn test_exit_code_1_multiple_errors() {
    // Exit code 1: Multiple validation errors still return exit code 1
    let env = TestEnv::new();
    env.write_env("PORT=invalid\nURL=not_a_url\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "URL": {"type": "url", "required": true},
        "API_KEY": {"type": "string", "required": true}
    }"#);

    let opts = LoadOptions::default();
    let result = check::validate_files(env.env_str(), env.schema_str(), &opts);

    assert!(result.is_ok());
    let errors = result.unwrap();
    assert!(errors.len() >= 2, "Should have multiple errors but still exit code 1");
}

// =============================================================================
// COMMAND RUN() FUNCTION TESTS (Phase 2)
// Tests for command entry points that were previously untested
// =============================================================================

#[test]
fn test_diff_run_identical_files() {
    let env = TestEnv::new();
    env.write_env("FOO=bar\nBAZ=qux\n");

    // Create a second file with same content
    let env2_path = env.temp_dir.path().join("second.env");
    std::fs::write(&env2_path, "FOO=bar\nBAZ=qux\n").unwrap();

    let result = zorath_env::commands::diff::run(
        env.env_str(),
        env2_path.to_str().unwrap(),
        None,
        "text",
        false,
        None,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_diff_run_different_files() {
    let env = TestEnv::new();
    env.write_env("FOO=bar\nBAZ=qux\n");

    let env2_path = env.temp_dir.path().join("second.env");
    std::fs::write(&env2_path, "FOO=different\nNEW_KEY=value\n").unwrap();

    let result = zorath_env::commands::diff::run(
        env.env_str(),
        env2_path.to_str().unwrap(),
        None,
        "text",
        false,
        None,
        None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_diff_run_file_not_found() {
    let env = TestEnv::new();
    env.write_env("FOO=bar\n");

    let result = zorath_env::commands::diff::run(
        env.env_str(),
        "nonexistent_file.env",
        None,
        "text",
        false,
        None,
        None,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Error reading"));
}

#[test]
fn test_template_run_github() {
    let result = zorath_env::commands::template::run("github", None, false, false);
    assert!(result.is_ok());
}

#[test]
fn test_template_run_gitlab() {
    let result = zorath_env::commands::template::run("gitlab", None, false, false);
    assert!(result.is_ok());
}

#[test]
fn test_template_run_circleci() {
    let result = zorath_env::commands::template::run("circleci", None, false, false);
    assert!(result.is_ok());
}

#[test]
fn test_template_run_to_file() {
    let env = TestEnv::new();
    let output_path = env.temp_dir.path().join("workflow.yml");

    let result = zorath_env::commands::template::run(
        "github",
        Some(output_path.to_str().unwrap()),
        false,
        false,
    );
    assert!(result.is_ok());
    assert!(output_path.exists());

    let content = std::fs::read_to_string(&output_path).unwrap();
    assert!(content.contains("zenv check"));
}

#[test]
fn test_cache_run_list() {
    let result = zorath_env::commands::cache::run_list();
    assert!(result.is_ok());
}

#[test]
fn test_cache_run_stats() {
    let result = zorath_env::commands::cache::run_stats();
    assert!(result.is_ok());
}

#[test]
fn test_cache_run_path() {
    let result = zorath_env::commands::cache::run_path();
    assert!(result.is_ok());
}

#[test]
fn test_cache_run_clear_specific_url() {
    // Clearing a non-cached URL should not error
    let result = zorath_env::commands::cache::run_clear(Some("https://example.com/test.json"));
    assert!(result.is_ok());
}
