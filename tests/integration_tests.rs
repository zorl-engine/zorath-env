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

use zorath_env::commands::{check, docs, fix, init};
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
