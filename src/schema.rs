use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::remote::{self, RemoteError, SecurityOptions};

#[derive(Error, Debug)]
pub enum SchemaError {
    #[error("failed to read schema file: {0}")]
    Read(String),
    #[error("invalid schema {0}: {1}")]
    Parse(String, String),
    #[error("circular inheritance detected: {0}")]
    CircularInheritance(String),
    #[error("inheritance depth exceeded (max 10)")]
    InheritanceDepthExceeded,
    #[error("remote schema error: {0}")]
    Remote(#[from] RemoteError),
}

/// Supported schema file formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchemaFormat {
    Json,
    Yaml,
}

impl SchemaFormat {
    /// Detect format from file path extension
    pub fn from_path(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            SchemaFormat::Yaml
        } else {
            SchemaFormat::Json // Default to JSON for backwards compatibility
        }
    }

    /// Get format name for error messages
    pub fn name(&self) -> &'static str {
        match self {
            SchemaFormat::Json => "JSON",
            SchemaFormat::Yaml => "YAML",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VarType {
    #[default]
    String,
    Int,
    Float,
    Bool,
    Url,
    Enum,
    Uuid,
    Email,
    Ipv4,
    Ipv6,
    Semver,
    Port,
    Date,
    Hostname,
}

/// Custom validation rules for environment variables
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Minimum value for int type
    #[serde(default)]
    pub min: Option<i64>,

    /// Maximum value for int type
    #[serde(default)]
    pub max: Option<i64>,

    /// Minimum value for float type
    #[serde(default)]
    pub min_value: Option<f64>,

    /// Maximum value for float type
    #[serde(default)]
    pub max_value: Option<f64>,

    /// Minimum length for string type
    #[serde(default)]
    pub min_length: Option<usize>,

    /// Maximum length for string type
    #[serde(default)]
    pub max_length: Option<usize>,

    /// Regex pattern for string type
    #[serde(default)]
    pub pattern: Option<String>,
}

/// Severity level for validation failures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Validation failure causes exit code 1 (default)
    #[default]
    Error,
    /// Validation failure is reported but doesn't cause exit code 1
    Warning,
}

fn is_default_severity(severity: &Severity) -> bool {
    *severity == Severity::Error
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VarSpec {
    #[serde(rename = "type", default)]
    pub var_type: VarType,

    #[serde(default)]
    pub required: bool,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub values: Option<Vec<String>>, // for enum

    #[serde(default)]
    pub default: Option<serde_json::Value>,

    /// Custom validation rules
    #[serde(default)]
    pub validate: Option<ValidationRule>,

    /// Secret detection control: false = skip secret check (known safe value)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<bool>,

    /// Severity level: error (default) or warning
    /// Warning-level issues don't cause exit code 1
    #[serde(default, skip_serializing_if = "is_default_severity")]
    pub severity: Severity,
}

pub type Schema = HashMap<String, VarSpec>;

/// Schema file structure that supports inheritance via "extends" field
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaFile {
    /// Path to parent schema file (relative to current schema)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,

    /// Variable specifications
    #[serde(flatten)]
    pub vars: Schema,
}

/// Schema loading options
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Skip cache for remote schemas
    pub no_cache: bool,
    /// Expected SHA-256 hash for remote schema verification
    pub verify_hash: Option<String>,
    /// Custom CA certificate path for enterprise TLS
    pub ca_cert: Option<String>,
    /// Rate limit in seconds between remote fetches (0 to disable)
    pub rate_limit_seconds: Option<u64>,
}

impl LoadOptions {
    /// Convert LoadOptions to SecurityOptions for remote fetching
    pub fn to_security_options(&self) -> SecurityOptions {
        SecurityOptions::new()
            .with_hash(self.verify_hash.clone())
            .with_ca_cert(self.ca_cert.clone())
            .with_rate_limit(self.rate_limit_seconds.unwrap_or(remote::DEFAULT_RATE_LIMIT_SECS))
    }
}

/// Load schema from file or URL, resolving inheritance chain.
/// Convenience wrapper for `load_schema_with_options` with default options.
/// Kept as public API for library consumers who don't need custom options.
#[allow(dead_code)]
pub fn load_schema(path: &str) -> Result<Schema, SchemaError> {
    load_schema_with_options(path, &LoadOptions::default())
}

/// Load schema with options (e.g., no_cache for remote schemas)
pub fn load_schema_with_options(path: &str, options: &LoadOptions) -> Result<Schema, SchemaError> {
    load_schema_with_chain(path, &mut Vec::new(), options)
}

/// Parse schema content based on format (JSON or YAML)
fn parse_schema_content(content: &str, format: SchemaFormat) -> Result<SchemaFile, SchemaError> {
    match format {
        SchemaFormat::Json => {
            serde_json::from_str(content)
                .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))
        }
        SchemaFormat::Yaml => {
            serde_yaml::from_str(content)
                .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))
        }
    }
}

/// Internal: Load schema with circular reference detection
fn load_schema_with_chain(
    path: &str,
    chain: &mut Vec<String>,
    options: &LoadOptions,
) -> Result<Schema, SchemaError> {
    // Check max depth
    if chain.len() > 10 {
        return Err(SchemaError::InheritanceDepthExceeded);
    }

    // For remote URLs, use the URL as the identifier
    // For local files, resolve to absolute path
    let abs_path = if remote::is_remote_url(path) {
        path.to_string()
    } else {
        fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string())
    };

    // Check for circular reference
    if chain.contains(&abs_path) {
        return Err(SchemaError::CircularInheritance(path.to_string()));
    }
    chain.push(abs_path);

    // Read schema content (from file or URL)
    let content = if remote::is_remote_url(path) {
        remote::fetch_remote_schema_secure(path, options.no_cache, &options.to_security_options())?
    } else {
        fs::read_to_string(path).map_err(|e| SchemaError::Read(e.to_string()))?
    };

    // Detect format and parse
    let format = SchemaFormat::from_path(path);
    let schema_file: SchemaFile = parse_schema_content(&content, format)?;

    // Start with parent schema if extends is specified
    let mut result = if let Some(ref parent_path) = schema_file.extends {
        // Resolve parent path relative to current schema
        let parent_full_path = if remote::is_remote_url(path) {
            // For remote schemas, resolve relative URLs
            remote::resolve_relative_url(path, parent_path)?
        } else {
            resolve_relative_path(path, parent_path)
        };
        load_schema_with_chain(&parent_full_path, chain, options)?
    } else {
        Schema::new()
    };

    // Merge current schema (child overrides parent)
    for (key, spec) in schema_file.vars {
        result.insert(key, spec);
    }

    Ok(result)
}

/// Resolve a relative path based on the parent file's directory
fn resolve_relative_path(base_path: &str, relative_path: &str) -> String {
    let base = Path::new(base_path);
    if let Some(parent_dir) = base.parent() {
        parent_dir.join(relative_path).to_string_lossy().to_string()
    } else {
        relative_path.to_string()
    }
}

/// Save schema to file (format auto-detected from path extension)
pub fn save_schema(path: &str, schema: &Schema) -> Result<(), SchemaError> {
    let format = SchemaFormat::from_path(path);
    let content = match format {
        SchemaFormat::Json => {
            serde_json::to_string_pretty(schema)
                .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))?
        }
        SchemaFormat::Yaml => {
            serde_yaml::to_string(schema)
                .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))?
        }
    };
    fs::write(path, content).map_err(|e| SchemaError::Read(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string_type() {
        let json = r#"{"FOO": {"type": "string", "required": true}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert!(matches!(spec.var_type, VarType::String));
        assert!(spec.required);
    }

    #[test]
    fn test_parse_int_type() {
        let json = r#"{"PORT": {"type": "int", "required": false, "default": 3000}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("PORT").unwrap();
        assert!(matches!(spec.var_type, VarType::Int));
        assert!(!spec.required);
        assert_eq!(spec.default, Some(serde_json::json!(3000)));
    }

    #[test]
    fn test_parse_float_type() {
        let json = r#"{"RATE": {"type": "float"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("RATE").unwrap();
        assert!(matches!(spec.var_type, VarType::Float));
    }

    #[test]
    fn test_parse_bool_type() {
        let json = r#"{"DEBUG": {"type": "bool", "default": false}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("DEBUG").unwrap();
        assert!(matches!(spec.var_type, VarType::Bool));
        assert_eq!(spec.default, Some(serde_json::json!(false)));
    }

    #[test]
    fn test_parse_url_type() {
        let json = r#"{"API_URL": {"type": "url", "required": true}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("API_URL").unwrap();
        assert!(matches!(spec.var_type, VarType::Url));
    }

    #[test]
    fn test_parse_enum_type() {
        let json = r#"{"NODE_ENV": {"type": "enum", "values": ["dev", "staging", "prod"]}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("NODE_ENV").unwrap();
        assert!(matches!(spec.var_type, VarType::Enum));
        assert_eq!(spec.values, Some(vec!["dev".to_string(), "staging".to_string(), "prod".to_string()]));
    }

    #[test]
    fn test_parse_description() {
        let json = r#"{"FOO": {"type": "string", "description": "A test variable"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert_eq!(spec.description, Some("A test variable".to_string()));
    }

    #[test]
    fn test_parse_multiple_vars() {
        let json = r#"{
            "FOO": {"type": "string"},
            "BAR": {"type": "int"},
            "BAZ": {"type": "bool"}
        }"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.len(), 3);
    }

    #[test]
    fn test_invalid_json_error() {
        let json = r#"{"FOO": {"type": "string""#;
        let result: Result<Schema, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_type_error() {
        let json = r#"{"FOO": {"type": "invalid_type"}}"#;
        let result: Result<Schema, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_required_defaults_to_false() {
        let json = r#"{"FOO": {"type": "string"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert!(!spec.required);
    }

    #[test]
    fn test_roundtrip_serialization() {
        let json = r#"{"FOO":{"type":"string","required":true,"description":"Test"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&schema).unwrap();
        let reparsed: Schema = serde_json::from_str(&serialized).unwrap();
        assert_eq!(schema.len(), reparsed.len());
    }

    // Schema file parsing (with optional extends field)
    #[test]
    fn test_schema_file_without_extends() {
        let json = r#"{"FOO": {"type": "string"}}"#;
        let schema_file: SchemaFile = serde_json::from_str(json).unwrap();
        assert!(schema_file.extends.is_none());
        assert!(schema_file.vars.contains_key("FOO"));
    }

    #[test]
    fn test_schema_file_with_extends() {
        let json = r#"{"extends": "base.schema.json", "FOO": {"type": "string"}}"#;
        let schema_file: SchemaFile = serde_json::from_str(json).unwrap();
        assert_eq!(schema_file.extends, Some("base.schema.json".to_string()));
        assert!(schema_file.vars.contains_key("FOO"));
    }

    #[test]
    fn test_resolve_relative_path() {
        // Test sibling file
        let result = resolve_relative_path("dir/child.json", "base.json");
        assert!(result.ends_with("dir/base.json") || result.ends_with("dir\\base.json"));

        // Test parent directory
        let result = resolve_relative_path("nested/dir/child.json", "../base.json");
        assert!(result.contains("nested") && result.contains("base.json"));
    }

    // Integration tests with actual files
    #[test]
    fn test_load_schema_without_extends() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"{{"FOO": {{"type": "string"}}}}"#).unwrap();

        let schema = load_schema(file.path().to_str().unwrap()).unwrap();
        assert!(schema.contains_key("FOO"));
    }

    #[test]
    fn test_load_schema_with_extends() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Create base schema
        let base_path = dir.path().join("base.schema.json");
        let mut base_file = fs::File::create(&base_path).unwrap();
        writeln!(base_file, r#"{{"BASE_VAR": {{"type": "string", "required": true}}}}"#).unwrap();

        // Create child schema that extends base
        let child_path = dir.path().join("child.schema.json");
        let mut child_file = fs::File::create(&child_path).unwrap();
        writeln!(child_file, r#"{{"extends": "base.schema.json", "CHILD_VAR": {{"type": "int"}}}}"#).unwrap();

        let schema = load_schema(child_path.to_str().unwrap()).unwrap();

        // Should have both vars
        assert!(schema.contains_key("BASE_VAR"));
        assert!(schema.contains_key("CHILD_VAR"));
        assert_eq!(schema.len(), 2);
    }

    #[test]
    fn test_load_schema_child_overrides_parent() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Base schema with PORT as string
        let base_path = dir.path().join("base.schema.json");
        let mut base_file = fs::File::create(&base_path).unwrap();
        writeln!(base_file, r#"{{"PORT": {{"type": "string", "description": "base desc"}}}}"#).unwrap();

        // Child schema overrides PORT as int
        let child_path = dir.path().join("child.schema.json");
        let mut child_file = fs::File::create(&child_path).unwrap();
        writeln!(child_file, r#"{{"extends": "base.schema.json", "PORT": {{"type": "int", "description": "child desc"}}}}"#).unwrap();

        let schema = load_schema(child_path.to_str().unwrap()).unwrap();
        let port = schema.get("PORT").unwrap();

        // Child should override
        assert!(matches!(port.var_type, VarType::Int));
        assert_eq!(port.description, Some("child desc".to_string()));
    }

    #[test]
    fn test_load_schema_multi_level_inheritance() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // Grandparent
        let gp_path = dir.path().join("grandparent.json");
        let mut gp_file = fs::File::create(&gp_path).unwrap();
        writeln!(gp_file, r#"{{"GP_VAR": {{"type": "string"}}}}"#).unwrap();

        // Parent extends grandparent
        let p_path = dir.path().join("parent.json");
        let mut p_file = fs::File::create(&p_path).unwrap();
        writeln!(p_file, r#"{{"extends": "grandparent.json", "P_VAR": {{"type": "string"}}}}"#).unwrap();

        // Child extends parent
        let c_path = dir.path().join("child.json");
        let mut c_file = fs::File::create(&c_path).unwrap();
        writeln!(c_file, r#"{{"extends": "parent.json", "C_VAR": {{"type": "string"}}}}"#).unwrap();

        let schema = load_schema(c_path.to_str().unwrap()).unwrap();

        // Should have all three vars
        assert!(schema.contains_key("GP_VAR"));
        assert!(schema.contains_key("P_VAR"));
        assert!(schema.contains_key("C_VAR"));
        assert_eq!(schema.len(), 3);
    }

    #[test]
    fn test_load_schema_circular_inheritance_detected() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // A extends B
        let a_path = dir.path().join("a.json");
        let mut a_file = fs::File::create(&a_path).unwrap();
        writeln!(a_file, r#"{{"extends": "b.json", "A": {{"type": "string"}}}}"#).unwrap();

        // B extends A (circular!)
        let b_path = dir.path().join("b.json");
        let mut b_file = fs::File::create(&b_path).unwrap();
        writeln!(b_file, r#"{{"extends": "a.json", "B": {{"type": "string"}}}}"#).unwrap();

        let result = load_schema(a_path.to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaError::CircularInheritance(_)));
    }

    // YAML schema format tests
    #[test]
    fn test_schema_format_detection_json() {
        assert_eq!(SchemaFormat::from_path("schema.json"), SchemaFormat::Json);
        assert_eq!(SchemaFormat::from_path("path/to/schema.JSON"), SchemaFormat::Json);
        assert_eq!(SchemaFormat::from_path("env.schema.json"), SchemaFormat::Json);
    }

    #[test]
    fn test_schema_format_detection_yaml() {
        assert_eq!(SchemaFormat::from_path("schema.yaml"), SchemaFormat::Yaml);
        assert_eq!(SchemaFormat::from_path("schema.yml"), SchemaFormat::Yaml);
        assert_eq!(SchemaFormat::from_path("path/to/schema.YAML"), SchemaFormat::Yaml);
        assert_eq!(SchemaFormat::from_path("env.schema.yml"), SchemaFormat::Yaml);
    }

    #[test]
    fn test_schema_format_detection_default() {
        // Unknown extensions default to JSON
        assert_eq!(SchemaFormat::from_path("schema"), SchemaFormat::Json);
        assert_eq!(SchemaFormat::from_path("schema.txt"), SchemaFormat::Json);
    }

    #[test]
    fn test_parse_yaml_schema() {
        let yaml = r#"
FOO:
  type: string
  required: true
  description: A test variable
BAR:
  type: int
  default: 3000
"#;
        let result = parse_schema_content(yaml, SchemaFormat::Yaml);
        assert!(result.is_ok());
        let schema_file = result.unwrap();
        assert!(schema_file.vars.contains_key("FOO"));
        assert!(schema_file.vars.contains_key("BAR"));
        let foo = schema_file.vars.get("FOO").unwrap();
        assert!(foo.required);
        assert_eq!(foo.description, Some("A test variable".to_string()));
    }

    #[test]
    fn test_parse_yaml_schema_with_extends() {
        let yaml = r#"
extends: base.schema.yaml
PORT:
  type: int
  required: true
"#;
        let result = parse_schema_content(yaml, SchemaFormat::Yaml);
        assert!(result.is_ok());
        let schema_file = result.unwrap();
        assert_eq!(schema_file.extends, Some("base.schema.yaml".to_string()));
        assert!(schema_file.vars.contains_key("PORT"));
    }

    #[test]
    fn test_parse_yaml_schema_with_enum() {
        let yaml = r#"
NODE_ENV:
  type: enum
  values:
    - development
    - staging
    - production
  required: true
"#;
        let result = parse_schema_content(yaml, SchemaFormat::Yaml);
        assert!(result.is_ok());
        let schema_file = result.unwrap();
        let env = schema_file.vars.get("NODE_ENV").unwrap();
        assert!(matches!(env.var_type, VarType::Enum));
        assert_eq!(env.values, Some(vec!["development".to_string(), "staging".to_string(), "production".to_string()]));
    }

    #[test]
    fn test_parse_yaml_invalid_syntax() {
        let yaml = r#"
FOO:
  type: string
  required: [invalid
"#;
        let result = parse_schema_content(yaml, SchemaFormat::Yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_yaml_schema_from_file() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("schema.yaml");
        let mut file = fs::File::create(&yaml_path).unwrap();
        writeln!(file, "API_KEY:\n  type: string\n  required: true").unwrap();

        let schema = load_schema(yaml_path.to_str().unwrap()).unwrap();
        assert!(schema.contains_key("API_KEY"));
        assert!(schema.get("API_KEY").unwrap().required);
    }

    #[test]
    fn test_load_yml_extension() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let yml_path = dir.path().join("schema.yml");
        let mut file = fs::File::create(&yml_path).unwrap();
        writeln!(file, "DEBUG:\n  type: bool\n  default: false").unwrap();

        let schema = load_schema(yml_path.to_str().unwrap()).unwrap();
        assert!(schema.contains_key("DEBUG"));
    }

    #[test]
    fn test_yaml_extends_json() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // JSON base schema
        let json_path = dir.path().join("base.schema.json");
        let mut json_file = fs::File::create(&json_path).unwrap();
        writeln!(json_file, r#"{{"BASE_VAR": {{"type": "string"}}}}"#).unwrap();

        // YAML child extends JSON
        let yaml_path = dir.path().join("child.schema.yaml");
        let mut yaml_file = fs::File::create(&yaml_path).unwrap();
        writeln!(yaml_file, "extends: base.schema.json\nCHILD_VAR:\n  type: int").unwrap();

        let schema = load_schema(yaml_path.to_str().unwrap()).unwrap();
        assert!(schema.contains_key("BASE_VAR"));
        assert!(schema.contains_key("CHILD_VAR"));
    }

    #[test]
    fn test_json_extends_yaml() {
        use std::io::Write;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();

        // YAML base schema
        let yaml_path = dir.path().join("base.schema.yaml");
        let mut yaml_file = fs::File::create(&yaml_path).unwrap();
        writeln!(yaml_file, "BASE_VAR:\n  type: string").unwrap();

        // JSON child extends YAML
        let json_path = dir.path().join("child.schema.json");
        let mut json_file = fs::File::create(&json_path).unwrap();
        writeln!(json_file, r#"{{"extends": "base.schema.yaml", "CHILD_VAR": {{"type": "int"}}}}"#).unwrap();

        let schema = load_schema(json_path.to_str().unwrap()).unwrap();
        assert!(schema.contains_key("BASE_VAR"));
        assert!(schema.contains_key("CHILD_VAR"));
    }

    #[test]
    fn test_save_schema_yaml() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("output.yaml");

        let mut schema = Schema::new();
        schema.insert("TEST_VAR".to_string(), VarSpec {
            var_type: VarType::String,
            required: true,
            ..Default::default()
        });

        save_schema(yaml_path.to_str().unwrap(), &schema).unwrap();

        // Verify it can be read back
        let loaded = load_schema(yaml_path.to_str().unwrap()).unwrap();
        assert!(loaded.contains_key("TEST_VAR"));
    }

    #[test]
    fn test_yaml_with_validation_rules() {
        let yaml = r#"
PORT:
  type: int
  validate:
    min: 1024
    max: 65535
API_KEY:
  type: string
  validate:
    min_length: 32
    pattern: "^sk_"
"#;
        let result = parse_schema_content(yaml, SchemaFormat::Yaml);
        assert!(result.is_ok());
        let schema_file = result.unwrap();

        let port = schema_file.vars.get("PORT").unwrap();
        let port_validate = port.validate.as_ref().unwrap();
        assert_eq!(port_validate.min, Some(1024));
        assert_eq!(port_validate.max, Some(65535));

        let api_key = schema_file.vars.get("API_KEY").unwrap();
        let key_validate = api_key.validate.as_ref().unwrap();
        assert_eq!(key_validate.min_length, Some(32));
        assert_eq!(key_validate.pattern, Some("^sk_".to_string()));
    }
}
