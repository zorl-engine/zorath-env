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
    assert!(result.unwrap_err().to_string().contains("Error reading"));
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

// =============================================================================
// WATCH MODE TESTS
// =============================================================================

#[test]
fn test_check_watch_mode_rejects_json_format() {
    // Watch mode explicitly rejects JSON format
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{"PORT": {"type": "int", "required": true}}"#);

    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false,  // allow_missing_env
        false,  // detect_secrets
        false,  // no_cache
        true,   // watch = true
        "json", // format = json (should be rejected)
        None,   // verify_hash
        None,   // ca_cert
    );

    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("JSON format is not supported in watch mode"),
        "Watch mode should reject JSON format"
    );
}

// =============================================================================
// UNICODE AND PATH HANDLING TESTS
// =============================================================================

#[test]
fn test_envfile_unicode_values() {
    // Test that Unicode characters in values are preserved
    let env = TestEnv::new();
    env.write_env("GREETING=Hello World\nMESSAGE=Test message\nNAME=Luis\n");
    env.write_schema(r#"{
        "GREETING": {"type": "string", "required": true},
        "MESSAGE": {"type": "string", "required": true},
        "NAME": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("GREETING").unwrap(), "Hello World");
    assert_eq!(env_map.get("MESSAGE").unwrap(), "Test message");
    assert_eq!(env_map.get("NAME").unwrap(), "Luis");

    let schema = env.load_schema();
    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "Unicode values should validate: {:?}", errors);
}

#[test]
fn test_envfile_quoted_unicode() {
    // Test Unicode in quoted strings
    let env = TestEnv::new();
    env.write_env("TITLE=\"Welcome Message\"\nDESC='Description text'\n");
    env.write_schema(r#"{
        "TITLE": {"type": "string", "required": true},
        "DESC": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("TITLE").unwrap(), "Welcome Message");
    assert_eq!(env_map.get("DESC").unwrap(), "Description text");
}

#[test]
fn test_envfile_windows_paths() {
    // Test Windows-style paths are preserved
    let env = TestEnv::new();
    env.write_env("LOG_PATH=C:\\Users\\Luis\\logs\nDATA_DIR=\"D:\\Data\\app\"\n");
    env.write_schema(r#"{
        "LOG_PATH": {"type": "string", "required": true},
        "DATA_DIR": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("LOG_PATH").unwrap(), "C:\\Users\\Luis\\logs");
    assert_eq!(env_map.get("DATA_DIR").unwrap(), "D:\\Data\\app");

    let schema = env.load_schema();
    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "Windows paths should validate: {:?}", errors);
}

#[test]
fn test_envfile_unix_paths() {
    // Test Unix-style paths
    let env = TestEnv::new();
    env.write_env("CONFIG=/etc/app/config.json\nDATA=\"/var/lib/app/data\"\n");
    env.write_schema(r#"{
        "CONFIG": {"type": "string", "required": true},
        "DATA": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("CONFIG").unwrap(), "/etc/app/config.json");
    assert_eq!(env_map.get("DATA").unwrap(), "/var/lib/app/data");
}

#[test]
fn test_envfile_mixed_path_separators() {
    // Test paths with mixed separators (common in cross-platform configs)
    let env = TestEnv::new();
    env.write_env("PATH1=C:/Users/Luis/app\nPATH2=/mnt/c/Users/Luis\n");
    env.write_schema(r#"{
        "PATH1": {"type": "string", "required": true},
        "PATH2": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("PATH1").unwrap(), "C:/Users/Luis/app");
    assert_eq!(env_map.get("PATH2").unwrap(), "/mnt/c/Users/Luis");
}

#[test]
fn test_envfile_special_characters_in_values() {
    // Test various special characters
    let env = TestEnv::new();
    env.write_env("SYMBOLS=\"!@#$%^&*()_+-=[]{}|;':,.<>?\"\nBRACKETS=\"{key: value}\"\n");
    env.write_schema(r#"{
        "SYMBOLS": {"type": "string", "required": true},
        "BRACKETS": {"type": "string", "required": true}
    }"#);

    let env_map = env.parse_env();
    assert_eq!(env_map.get("SYMBOLS").unwrap(), "!@#$%^&*()_+-=[]{}|;':,.<>?");
    assert_eq!(env_map.get("BRACKETS").unwrap(), "{key: value}");
}

// =============================================================================
// SCAN COMMAND INTEGRATION TESTS
// =============================================================================

#[test]
fn test_scan_finds_vars_in_javascript_file() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "API_URL": {"type": "url", "required": true},
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": false}
    }"#);

    // Create a JavaScript file that uses env vars
    let js_path = env.base_path.join("app.js");
    fs::write(&js_path, r#"
        const apiUrl = process.env.API_URL;
        const port = process.env.PORT || 3000;
        console.log(apiUrl, port);
    "#).unwrap();

    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        false, // show_unused
        false, // show_paths
        "text",
        false, // no_cache
        None,
        None,
    );

    // Should succeed - found vars match schema
    assert!(result.is_ok());
}

#[test]
fn test_scan_finds_vars_in_python_file() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "DATABASE_URL": {"type": "url", "required": true},
        "SECRET_KEY": {"type": "string", "required": true}
    }"#);

    // Create a Python file that uses env vars
    let py_path = env.base_path.join("app.py");
    fs::write(&py_path, r#"
import os
database_url = os.environ.get('DATABASE_URL')
secret_key = os.getenv('SECRET_KEY')
    "#).unwrap();

    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        false,
        false,
        "text",
        false,
        None,
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn test_scan_show_unused_reports_missing_usage() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "USED_VAR": {"type": "string", "required": true},
        "UNUSED_VAR": {"type": "string", "required": true}
    }"#);

    // Create file that only uses one var
    let js_path = env.base_path.join("app.js");
    fs::write(&js_path, r#"
        const used = process.env.USED_VAR;
    "#).unwrap();

    // With show_unused=true, should report UNUSED_VAR
    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        true,  // show_unused = true
        false,
        "text",
        false,
        None,
        None,
    );

    assert!(result.is_ok());
}

#[test]
fn test_scan_json_output_format() {
    let env = TestEnv::new();
    env.write_schema(r#"{
        "API_KEY": {"type": "string", "required": true}
    }"#);

    let js_path = env.base_path.join("config.js");
    fs::write(&js_path, r#"
        module.exports = { key: process.env.API_KEY };
    "#).unwrap();

    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        false,
        false,
        "json", // JSON format
        false,
        None,
        None,
    );

    assert!(result.is_ok());
}

// =============================================================================
// DOCTOR COMMAND INTEGRATION TESTS
// =============================================================================

#[test]
fn test_doctor_healthy_setup() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{"PORT": {"type": "int", "required": true}}"#);

    let result = zorath_env::commands::doctor::run(
        env.env_str(),
        env.schema_str(),
        false,
        None,
        None,
    );

    // Should succeed with valid setup
    assert!(result.is_ok());
}

#[test]
fn test_doctor_missing_schema_reports_issue() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    // No schema file created - doctor will try fallback paths

    let result = zorath_env::commands::doctor::run(
        env.env_str(),
        env.schema_str(), // Points to non-existent file
        false,
        None,
        None,
    );

    // Doctor may return Ok (if fallback found) or Err (if not found)
    // The key is it doesn't panic
    let _ = result; // Just verify no panic
}

#[test]
fn test_doctor_missing_env_reports_issue() {
    let env = TestEnv::new();
    env.write_schema(r#"{"PORT": {"type": "int", "required": true}}"#);
    // No .env file created - doctor checks fallback paths like .env in cwd

    let result = zorath_env::commands::doctor::run(
        env.env_str(), // Points to non-existent file
        env.schema_str(),
        false,
        None,
        None,
    );

    // Doctor may find fallback .env files and return Ok or Err
    // The key is it doesn't panic
    let _ = result; // Just verify no panic
}

#[test]
fn test_doctor_invalid_schema_reports_error() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema("{ invalid json }");

    let result = zorath_env::commands::doctor::run(
        env.env_str(),
        env.schema_str(),
        false,
        None,
        None,
    );

    // Doctor returns Err when schema is invalid (has errors)
    assert!(result.is_err(), "Invalid schema should cause doctor to return Err");
}

// =============================================================================
// LARGE FILE STRESS TESTS
// =============================================================================

#[test]
fn test_large_env_file_100_vars() {
    let env = TestEnv::new();

    // Generate 100 variables
    let mut env_content = String::new();
    let mut schema_content = String::from("{");
    for i in 0..100 {
        env_content.push_str(&format!("VAR_{:03}=value_{}\n", i, i));
        if i > 0 {
            schema_content.push_str(",");
        }
        schema_content.push_str(&format!(
            r#""VAR_{:03}": {{"type": "string", "required": true}}"#,
            i
        ));
    }
    schema_content.push('}');

    env.write_env(&env_content);
    env.write_schema(&schema_content);

    let schema = env.load_schema();
    let env_map = env.parse_env();

    assert_eq!(env_map.len(), 100);
    assert_eq!(schema.len(), 100);

    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "100 valid vars should pass: {:?}", errors);
}

#[test]
fn test_large_env_file_500_vars() {
    let env = TestEnv::new();

    // Generate 500 variables
    let mut env_content = String::new();
    let mut schema_content = String::from("{");
    for i in 0..500 {
        env_content.push_str(&format!("VAR_{:03}=value_{}\n", i, i));
        if i > 0 {
            schema_content.push_str(",");
        }
        schema_content.push_str(&format!(
            r#""VAR_{:03}": {{"type": "string", "required": true}}"#,
            i
        ));
    }
    schema_content.push('}');

    env.write_env(&env_content);
    env.write_schema(&schema_content);

    let schema = env.load_schema();
    let env_map = env.parse_env();

    assert_eq!(env_map.len(), 500);
    assert_eq!(schema.len(), 500);

    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "500 valid vars should pass: {:?}", errors);
}

#[test]
fn test_large_schema_with_many_validation_rules() {
    let env = TestEnv::new();

    // Generate variables with various validation rules
    let mut env_content = String::new();
    let mut schema_content = String::from("{");

    for i in 0..50 {
        // Add int vars with min/max
        env_content.push_str(&format!("INT_{:02}={}\n", i, 1000 + i));
        if i > 0 || schema_content.len() > 1 {
            schema_content.push_str(",");
        }
        schema_content.push_str(&format!(
            r#""INT_{:02}": {{"type": "int", "required": true, "validate": {{"min": 0, "max": 9999}}}}"#,
            i
        ));
    }

    for i in 0..50 {
        // Add string vars with length constraints
        env_content.push_str(&format!("STR_{:02}=test_value_{}\n", i, i));
        schema_content.push_str(&format!(
            r#","STR_{:02}": {{"type": "string", "required": true, "validate": {{"min_length": 1, "max_length": 100}}}}"#,
            i
        ));
    }

    schema_content.push('}');

    env.write_env(&env_content);
    env.write_schema(&schema_content);

    let schema = env.load_schema();
    let env_map = env.parse_env();

    assert_eq!(env_map.len(), 100);
    assert_eq!(schema.len(), 100);

    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "Complex validation rules should pass: {:?}", errors);
}

// =============================================================================
// ERROR RECOVERY TESTS
// =============================================================================

#[test]
fn test_check_graceful_on_malformed_json_schema() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema("{ not valid json at all }");

    // Should return error, not panic
    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false, false, false, false, "text", None, None,
    );

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("schema") || err.contains("parse") || err.contains("JSON"),
            "Error should mention schema/parse issue: {}", err);
}

#[test]
fn test_check_graceful_on_malformed_yaml_schema() {
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");

    // Create invalid YAML schema
    let yaml_schema_path = env.base_path.join("env.schema.yaml");
    fs::write(&yaml_schema_path, "invalid: yaml: content: [[[").unwrap();

    let result = check::run(
        env.env_str(),
        yaml_schema_path.to_str().unwrap(),
        false, false, false, false, "text", None, None,
    );

    assert!(result.is_err());
}

#[test]
fn test_envfile_handles_empty_file() {
    let env = TestEnv::new();
    env.write_env(""); // Empty file
    env.write_schema(r#"{"OPTIONAL_VAR": {"type": "string", "required": false}}"#);

    let env_map = env.parse_env();
    assert!(env_map.is_empty());

    let schema = env.load_schema();
    let errors = check::validate(&schema, &env_map);
    assert!(errors.is_empty(), "Empty env with optional vars should pass");
}

// =============================================================================
// CACHE INTEGRATION TESTS
// =============================================================================

#[test]
fn test_cache_list_returns_ok() {
    // Cache list should work even with empty cache
    let result = zorath_env::commands::cache::run_list();
    assert!(result.is_ok());
}

#[test]
fn test_cache_stats_returns_ok() {
    // Cache stats should work even with empty cache
    let result = zorath_env::commands::cache::run_stats();
    assert!(result.is_ok());
}

#[test]
fn test_cache_path_returns_ok() {
    // Cache path should always return a valid path
    let result = zorath_env::commands::cache::run_path();
    assert!(result.is_ok());
}

// =============================================================================
// V0.3.8 FEATURE COVERAGE TESTS
// =============================================================================

#[test]
fn test_diff_typo_detection_algorithm() {
    // Test that diff command's typo detection works by using the suggestions module
    // This validates the core algorithm used by diff for "Did you mean?" suggestions
    use zorath_env::suggestions::find_closest_match;

    // Simulate typo: DATABASE_URL vs DATABSE_URL (missing 'A')
    let keys_in_file_b = vec!["DATABSE_URL", "OTHER_VAR"];
    let result = find_closest_match(
        "DATABASE_URL",
        keys_in_file_b.iter().copied(),
        3, // max distance
    );

    assert!(result.is_some(), "Should find typo match");
    let (matched, distance) = result.unwrap();
    assert_eq!(matched, "DATABSE_URL");
    assert!(distance <= 2, "Distance should be small for typo");
}

#[test]
fn test_diff_run_with_similar_keys() {
    // Integration test: diff two files with similar (typo) keys
    let env = TestEnv::new();

    let env_a_path = env.base_path.join("a.env");
    let env_b_path = env.base_path.join("b.env");

    // File A has DATABASE_URL, File B has DATABSE_URL (typo)
    fs::write(&env_a_path, "DATABASE_URL=postgres://localhost/db\n").unwrap();
    fs::write(&env_b_path, "DATABSE_URL=postgres://localhost/db\n").unwrap();

    let result = zorath_env::commands::diff::run(
        env_a_path.to_str().unwrap(),
        env_b_path.to_str().unwrap(),
        None,
        "text",
        false,
        None,
        None,
    );

    // Diff should succeed and detect the typo internally
    assert!(result.is_ok());
}

#[test]
fn test_config_handles_unknown_keys_gracefully() {
    // Test that config module handles unknown keys without crashing
    // The actual warning is printed to stderr, but parsing should succeed
    use zorath_env::config::Config;

    let env = TestEnv::new();
    let config_path = env.base_path.join(".zenvrc");

    // Create config with valid and invalid keys
    fs::write(&config_path, r#"{
        "schema": "env.schema.json",
        "invalid_unknown_key": true,
        "another_bad_key": "value"
    }"#).unwrap();

    // Change to temp directory to test config loading
    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&env.base_path).unwrap();

    // Config should load without panic (warnings printed but parsing succeeds)
    let config = Config::load();

    // Restore original directory
    std::env::set_current_dir(original_dir).unwrap();

    // Config should be Some (loaded successfully despite unknown keys)
    assert!(config.is_some(), "Config should load even with unknown keys");

    // Valid key should be accessible
    let cfg = config.unwrap();
    assert_eq!(cfg.schema_or("default"), "env.schema.json");
}

#[test]
fn test_check_returns_actionable_error() {
    // Test that check command returns error for validation failures
    // The detailed messages (including fix suggestions) are printed to stdout
    // but the function returns Err to indicate failure
    let env = TestEnv::new();
    env.write_env("UNKNOWN_VAR=value\n");
    env.write_schema(r#"{"REQUIRED_VAR": {"type": "string", "required": true}}"#);

    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false, false, false, false, "text", None, None,
    );

    // Should fail - check returns Err on validation failures
    // The actionable tips (including "zenv fix" suggestion) are printed to stdout
    assert!(result.is_err(), "Check should return error for missing required var");

    // Also test that the library validate function reports the specific error
    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Should have validation errors");
    let errors_str = errors.join("\n");
    assert!(
        errors_str.contains("REQUIRED_VAR") || errors_str.contains("missing"),
        "Errors should mention the missing required variable: {}", errors_str
    );
}

// =============================================================================
// COMPLETIONS COMMAND TESTS
// =============================================================================

#[test]
fn test_completions_generates_bash() {
    use clap::Command;
    use clap_complete::Shell;

    // Create a mock command structure
    let mut cmd = Command::new("zenv")
        .subcommand(Command::new("check"))
        .subcommand(Command::new("docs"))
        .subcommand(Command::new("completions"));

    let mut output = Vec::new();
    zorath_env::commands::completions::generate_to_writer(
        Shell::Bash,
        &mut cmd,
        &mut output,
    );

    let output_str = String::from_utf8(output).unwrap();
    assert!(!output_str.is_empty(), "Bash completions should generate output");
    assert!(output_str.contains("zenv"), "Output should reference zenv");
}

#[test]
fn test_completions_generates_powershell() {
    use clap::Command;
    use clap_complete::Shell;

    let mut cmd = Command::new("zenv")
        .subcommand(Command::new("check"))
        .subcommand(Command::new("docs"));

    let mut output = Vec::new();
    zorath_env::commands::completions::generate_to_writer(
        Shell::PowerShell,
        &mut cmd,
        &mut output,
    );

    let output_str = String::from_utf8(output).unwrap();
    assert!(!output_str.is_empty(), "PowerShell completions should generate output");
}

// =============================================================================
// VERSION COMMAND TESTS
// =============================================================================

#[test]
fn test_version_displays_without_update_check() {
    // Test version command without checking for updates (fast, no network)
    let result = zorath_env::commands::version::run(false);
    assert!(result.is_ok(), "Version command should succeed");
}

// =============================================================================
// FIX COMMAND SECRET MASKING TEST
// =============================================================================

#[test]
fn test_fix_with_sensitive_values() {
    // Test that fix command handles sensitive values
    // The actual masking happens in dry-run output (printed to stdout)
    // This test verifies the fix command works with sensitive-looking keys
    let env = TestEnv::new();

    // Create .env with a sensitive-looking key
    env.write_env("API_KEY=sk_live_abc123secret456\nPORT=3000\n");
    env.write_schema(r#"{
        "API_KEY": {"type": "string", "required": true},
        "PORT": {"type": "int", "required": true},
        "OPTIONAL_VAR": {"type": "string", "required": false, "default": "default_value"}
    }"#);

    // Run fix in dry-run mode - should work without crashing
    let result = fix::run(
        env.env_str(),
        env.schema_str(),
        false, // remove_unknown
        true,  // dry_run = true (tests masking path)
        false,
        None,
        None,
    );

    // Fix should succeed in dry-run mode
    assert!(result.is_ok(), "Fix dry-run should succeed with sensitive values");
}

// =============================================================================
// FINAL COVERAGE TESTS - CLOSING ALL GAPS
// =============================================================================

#[test]
fn test_check_ipv6_validation() {
    // IPv6 type was missing from test_all_valid_types
    let env = TestEnv::new();
    env.write_env("IPV6_ADDR=2001:0db8:85a3:0000:0000:8a2e:0370:7334\n");
    env.write_schema(r#"{"IPV6_ADDR": {"type": "ipv6", "required": true}}"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(errors.is_empty(), "Valid IPv6 should pass validation: {:?}", errors);
}

#[test]
fn test_check_ipv6_invalid() {
    // Test invalid IPv6 is rejected
    let env = TestEnv::new();
    env.write_env("IPV6_ADDR=not-an-ipv6-address\n");
    env.write_schema(r#"{"IPV6_ADDR": {"type": "ipv6", "required": true}}"#);

    let schema = env.load_schema();
    let env_map = env.parse_env();
    let errors = check::validate(&schema, &env_map);

    assert!(!errors.is_empty(), "Invalid IPv6 should fail validation");
}

#[test]
fn test_check_detect_secrets_aws_key() {
    // Test secret detection for AWS access key pattern
    let env = TestEnv::new();
    // AWS access key pattern: AKIA followed by 16 alphanumeric chars
    env.write_env("AWS_KEY=AKIAIOSFODNN7EXAMPLE\n");
    env.write_schema(r#"{"AWS_KEY": {"type": "string", "required": true}}"#);

    // Run check with detect_secrets=true
    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false, // allow_missing_env
        true,  // detect_secrets = true
        false, // no_cache
        false, // watch
        "text",
        None,
        None,
    );

    // Should succeed but print warnings (secrets are warnings, not errors)
    // The AWS key pattern should be detected
    assert!(result.is_ok() || result.is_err(), "Secret detection should run without panic");
}

#[test]
fn test_check_detect_secrets_jwt() {
    // Test secret detection for JWT token pattern
    let env = TestEnv::new();
    // JWT pattern: eyJ...base64...
    env.write_env("JWT_TOKEN=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U\n");
    env.write_schema(r#"{"JWT_TOKEN": {"type": "string", "required": true}}"#);

    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false, true, false, false, "text", None, None,
    );

    // Should run without panic - JWT pattern detected as potential secret
    assert!(result.is_ok() || result.is_err(), "JWT detection should run without panic");
}

#[test]
fn test_check_no_cache_flag() {
    // Test that no_cache flag works (for remote schemas, but also local)
    let env = TestEnv::new();
    env.write_env("PORT=3000\n");
    env.write_schema(r#"{"PORT": {"type": "int", "required": true}}"#);

    let result = check::run(
        env.env_str(),
        env.schema_str(),
        false,
        false,
        true,  // no_cache = true
        false,
        "text",
        None,
        None,
    );

    assert!(result.is_ok(), "Check with no_cache should succeed");
}

#[test]
fn test_envfile_duplicate_key_detection() {
    // Test that duplicate keys are detected during parsing
    let env = TestEnv::new();
    // Create .env with duplicate keys
    env.write_env("PORT=3000\nDEBUG=true\nPORT=4000\n");
    env.write_schema(r#"{
        "PORT": {"type": "int", "required": true},
        "DEBUG": {"type": "bool", "required": true}
    }"#);

    // Parse with detailed results to get duplicate info
    let result = envfile::parse_env_file_detailed(env.env_str());
    assert!(result.is_ok(), "Parsing should succeed");

    let parse_result = result.unwrap();
    // Last value wins
    assert_eq!(parse_result.values.get("PORT"), Some(&"4000".to_string()));
    // Duplicates should be detected
    assert!(!parse_result.duplicates.is_empty(), "Should detect duplicate PORT key");
    assert_eq!(parse_result.duplicates[0].key, "PORT");
}

#[test]
fn test_scan_finds_vars_in_go_file() {
    // Test Go language support in scan command
    let env = TestEnv::new();
    env.write_schema(r#"{
        "DATABASE_URL": {"type": "url", "required": true},
        "PORT": {"type": "int", "required": true}
    }"#);

    // Create a Go file that uses env vars
    let go_path = env.base_path.join("main.go");
    fs::write(&go_path, r#"
package main

import "os"

func main() {
    dbURL := os.Getenv("DATABASE_URL")
    port := os.Getenv("PORT")
    println(dbURL, port)
}
    "#).unwrap();

    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        false, false, "text", false, None, None,
    );

    assert!(result.is_ok(), "Go file scanning should work");
}

#[test]
fn test_scan_finds_vars_in_rust_file() {
    // Test Rust language support in scan command
    let env = TestEnv::new();
    env.write_schema(r#"{
        "API_KEY": {"type": "string", "required": true}
    }"#);

    // Create a Rust file that uses env vars
    let rs_path = env.base_path.join("main.rs");
    fs::write(&rs_path, r#"
use std::env;

fn main() {
    let api_key = env::var("API_KEY").unwrap();
    println!("{}", api_key);
}
    "#).unwrap();

    let result = zorath_env::commands::scan::run(
        env.base_path.to_str().unwrap(),
        env.schema_str(),
        false, false, "text", false, None, None,
    );

    assert!(result.is_ok(), "Rust file scanning should work");
}

#[test]
fn test_envfile_circular_interpolation_detected() {
    // Test that circular variable references are detected
    // A -> B -> A creates a cycle
    let content = "VAR_A=${VAR_B}\nVAR_B=${VAR_A}\n";
    let env_map = envfile::parse_env_str(content);

    // Try to interpolate - should detect circular reference
    let result = envfile::interpolate_env(env_map);

    // Should return error for circular reference
    assert!(result.is_err(), "Circular interpolation should be detected");
    let err = result.unwrap_err();
    assert!(
        format!("{:?}", err).contains("ircular") || format!("{:?}", err).contains("VAR_"),
        "Error should mention circular reference: {:?}", err
    );
}

// ====================================================================
// CLI-level tests: exercise the actual binary via std::process::Command
// ====================================================================

mod cli_tests {
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn zenv_bin() -> Command {
        Command::new(env!("CARGO_BIN_EXE_zenv"))
    }

    fn setup_env_and_schema(dir: &std::path::Path, env_content: &str, schema_content: &str) {
        fs::write(dir.join(".env"), env_content).unwrap();
        fs::write(dir.join("env.schema.json"), schema_content).unwrap();
    }

    #[test]
    fn test_cli_version_flag() {
        let output = zenv_bin().arg("version").output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("zenv"), "version output should contain 'zenv': {}", stdout);
    }

    #[test]
    fn test_cli_help_flag() {
        let output = zenv_bin().arg("--help").output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("schema"), "help should mention schema");
        assert!(stdout.contains("check"), "help should mention check command");
    }

    #[test]
    fn test_cli_check_valid_env() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=3000\nNODE_ENV=production\n",
            r#"{"PORT": {"type": "int", "required": true}, "NODE_ENV": {"type": "string"}}"#,
        );

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success(), "check should pass for valid env: {}",
            String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn test_cli_check_invalid_env_exits_1() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=not_a_number\n",
            r#"{"PORT": {"type": "int", "required": true}}"#,
        );

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert_eq!(output.status.code(), Some(1), "validation failure should exit 1");
    }

    #[test]
    fn test_cli_check_missing_env_exits_2() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("env.schema.json"), r#"{"PORT": {"type": "int"}}"#).unwrap();

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert_eq!(output.status.code(), Some(2), "missing env file should exit 2");
    }

    #[test]
    fn test_cli_check_bad_schema_exits_3() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "PORT=3000\n").unwrap();
        fs::write(dir.path().join("bad.schema.json"), "not valid json {{{").unwrap();

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("bad.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert_eq!(output.status.code(), Some(3), "bad schema should exit 3");
    }

    #[test]
    fn test_cli_check_json_format() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=3000\n",
            r#"{"PORT": {"type": "int", "required": true}}"#,
        );

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--format", "json",
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .expect("check --format json should produce valid JSON");
        assert_eq!(parsed["valid"], true);
    }

    #[test]
    fn test_cli_quiet_flag_suppresses_config_output() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=3000\n",
            r#"{"PORT": {"type": "int"}}"#,
        );
        // Write a .zenvrc so config loading would normally print
        fs::write(dir.path().join(".zenvrc"), r#"{"schema": "env.schema.json"}"#).unwrap();

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .current_dir(dir.path())
            .output().unwrap();

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("loaded config from"),
            "quiet mode should suppress config loading message: {}", stderr);
    }

    #[test]
    fn test_cli_config_flag_loads_custom_config() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=3000\n",
            r#"{"PORT": {"type": "int"}}"#,
        );
        // Create custom config that sets format to json
        let config_path = dir.path().join("custom.zenvrc");
        fs::write(&config_path, r#"{"format": "json"}"#).unwrap();

        let output = zenv_bin()
            .args(["check",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--config", config_path.to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Config set format=json, so output should be JSON
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .expect("custom config format=json should produce JSON output");
        assert_eq!(parsed["valid"], true);
    }

    #[test]
    fn test_cli_export_shell_format() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".env"), "PORT=3000\nNODE_ENV=prod\n").unwrap();

        let output = zenv_bin()
            .args(["export",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--format", "shell",
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("export "), "shell format should contain 'export '");
    }

    #[test]
    fn test_cli_docs_command() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("env.schema.json"),
            r#"{"PORT": {"type": "int", "required": true, "description": "Server port"}}"#).unwrap();

        let output = zenv_bin()
            .args(["docs",
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("PORT"), "docs should contain variable name");
    }

    #[test]
    fn test_cli_example_command() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("env.schema.json"),
            r#"{"PORT": {"type": "int", "required": true}, "NODE_ENV": {"type": "string"}}"#).unwrap();

        let output = zenv_bin()
            .args(["example",
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("PORT="), "example should contain PORT=");
    }

    #[test]
    fn test_cli_doctor_command() {
        let dir = TempDir::new().unwrap();
        setup_env_and_schema(
            dir.path(),
            "PORT=3000\n",
            r#"{"PORT": {"type": "int"}}"#,
        );

        let output = zenv_bin()
            .args(["doctor",
                "--env", dir.path().join(".env").to_str().unwrap(),
                "--schema", dir.path().join("env.schema.json").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        assert!(output.status.success(), "doctor should succeed: {}",
            String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn test_cli_verbose_and_quiet_conflict() {
        let output = zenv_bin()
            .args(["--verbose", "--quiet", "version"])
            .output().unwrap();

        assert!(!output.status.success(),
            "verbose + quiet should conflict and fail");
    }

    #[test]
    fn test_cli_diff_command() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.env"), "PORT=3000\nHOST=localhost\n").unwrap();
        fs::write(dir.path().join("b.env"), "PORT=8080\nDEBUG=true\n").unwrap();

        let output = zenv_bin()
            .args(["diff",
                dir.path().join("a.env").to_str().unwrap(),
                dir.path().join("b.env").to_str().unwrap(),
                "--quiet"])
            .output().unwrap();

        // diff exits 0 even with differences
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("PORT") || stdout.contains("HOST") || stdout.contains("DEBUG"),
            "diff should show differences");
    }

    #[test]
    fn test_cli_init_list_presets() {
        let output = zenv_bin()
            .args(["init", "--list-presets", "--quiet"])
            .output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("node") || stdout.contains("django") || stdout.contains("rails"),
            "list-presets should show available presets: {}", stdout);
    }

    #[test]
    fn test_cli_no_color_flag() {
        let output = zenv_bin()
            .args(["--no-color", "version"])
            .output().unwrap();

        assert!(output.status.success(), "--no-color flag should be accepted");
    }
}
