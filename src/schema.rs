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
    #[error("failed to write schema file: {0}")]
    Write(String),
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
    /// Detect format from file path extension. Falls back to JSON for paths
    /// without a recognized extension. Use this for permissive paths (remote
    /// URLs, paths that may legitimately omit an extension); prefer
    /// `from_path_strict` for local file loads where a typo like
    /// `schema.jsno` should be rejected before the JSON parser is asked to
    /// chew on a YAML or unrelated file and emit a misleading "expected `{`"
    /// error.
    pub fn from_path(path: &str) -> Self {
        Self::detect(path).unwrap_or(SchemaFormat::Json)
    }

    /// Detect format from extension. Returns None when the path either has
    /// no extension at all or carries an unknown extension (e.g.
    /// `schema.jsno`, `schema.txt`).
    fn detect(path: &str) -> Option<Self> {
        if let Ok(url) = url::Url::parse(path) {
            if matches!(url.scheme(), "https" | "http") {
                return Self::detect(url.path());
            }
        }
        let lower = path.to_lowercase();
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            Some(SchemaFormat::Yaml)
        } else if lower.ends_with(".json") {
            Some(SchemaFormat::Json)
        } else {
            None
        }
    }

    /// Detect format from a LOCAL file path. Errors with a clear message
    /// when the extension is present but unrecognized (e.g. the user typed
    /// `schema.jsno` instead of `.json`) so the resulting failure points at
    /// the path rather than at a confusing serde parse error 200 lines
    /// later. A missing extension (no dot in the basename) is permitted and
    /// defaults to JSON for backwards compatibility with extension-less
    /// schemas already in the wild.
    pub fn from_path_strict(path: &str) -> Result<Self, SchemaError> {
        if let Some(fmt) = Self::detect(path) {
            return Ok(fmt);
        }
        // Use Path::extension which already handles the leading-dot case
        // correctly: `.env` / `.tmpXYZ` (tempfile-style names) report None,
        // so they fall through to the JSON default rather than tripping the
        // typo guard. Only fail when there IS an extension component and we
        // don't recognize it.
        if std::path::Path::new(path).extension().is_some() {
            return Err(SchemaError::Read(format!(
                "schema file '{}' has unrecognized extension; expected .json, .yaml, or .yml",
                path
            )));
        }
        Ok(SchemaFormat::Json)
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

    /// SHA-256 of the immediate parent. Required when this schema is pinned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends_hash: Option<String>,

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
            .with_rate_limit(
                self.rate_limit_seconds
                    .unwrap_or(remote::DEFAULT_RATE_LIMIT_SECS),
            )
    }
}

/// Load schema with options (e.g., no_cache for remote schemas)
pub fn load_schema_with_options(path: &str, options: &LoadOptions) -> Result<Schema, SchemaError> {
    let schema = load_schema_with_chain(
        path,
        &mut Vec::new(),
        options,
        &remote::fetch_remote_schema_secure,
    )?;
    validate_schema(&schema)?;
    Ok(schema)
}

/// Check schema rules and defaults independently of which env values exist.
pub fn validate_schema(schema: &Schema) -> Result<(), SchemaError> {
    let mut keys: Vec<_> = schema.keys().collect();
    keys.sort();
    for key in keys {
        let spec = &schema[key];
        let invalid = |message: &str| {
            SchemaError::Parse("definition".into(), format!("{}: {}", key, message))
        };
        if matches!(spec.var_type, VarType::Enum) {
            if spec.values.as_ref().is_none_or(|values| values.is_empty()) {
                return Err(invalid("enum requires a nonempty values list"));
            }
        } else if spec.values.is_some() {
            return Err(invalid("values is supported only for enum variables"));
        }
        if let Some(rules) = &spec.validate {
            let int_rules = rules.min.is_some() || rules.max.is_some();
            let float_rules = rules.min_value.is_some() || rules.max_value.is_some();
            let string_rules =
                rules.min_length.is_some() || rules.max_length.is_some() || rules.pattern.is_some();
            if (int_rules && !matches!(spec.var_type, VarType::Int))
                || (float_rules && !matches!(spec.var_type, VarType::Float))
                || (string_rules && !matches!(spec.var_type, VarType::String))
            {
                return Err(invalid(
                    "validation rules do not apply to this variable type",
                ));
            }
            if rules.min.zip(rules.max).is_some_and(|(min, max)| min > max)
                || rules
                    .min_value
                    .zip(rules.max_value)
                    .is_some_and(|(min, max)| min > max)
                || rules
                    .min_length
                    .zip(rules.max_length)
                    .is_some_and(|(min, max)| min > max)
            {
                return Err(invalid("minimum cannot exceed maximum"));
            }
            if [rules.min_value, rules.max_value]
                .into_iter()
                .flatten()
                .any(|n| !n.is_finite())
            {
                return Err(invalid("float bounds must be finite"));
            }
            if let Some(pattern) = &rules.pattern {
                crate::commands::check::get_cached_regex(pattern)
                    .map_err(|e| invalid(&format!("invalid validation pattern: {}", e)))?;
            }
        }
        if let Some(default) = &spec.default {
            let value = match default {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Number(_) | serde_json::Value::Bool(_) => default.to_string(),
                _ => return Err(invalid("default must be a string, number, or boolean")),
            };
            // A schema error should never echo a possibly sensitive default.
            let mut private_spec = spec.clone();
            private_spec.secret = Some(true);
            if let Some(error) =
                crate::commands::check::validate_value(key, &value, &private_spec).first()
            {
                return Err(invalid(&format!("invalid default: {}", error)));
            }
        }
    }
    Ok(())
}

/// Parse schema content based on format (JSON or YAML)
fn parse_schema_content(content: &str, format: SchemaFormat) -> Result<SchemaFile, SchemaError> {
    match format {
        SchemaFormat::Json => serde_json::from_str(content)
            .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string())),
        SchemaFormat::Yaml => serde_yaml::from_str(content)
            .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string())),
    }
}

/// Internal: Load schema with circular reference detection
fn load_schema_with_chain<F>(
    path: &str,
    chain: &mut Vec<String>,
    options: &LoadOptions,
    fetch: &F,
) -> Result<Schema, SchemaError>
where
    F: Fn(&str, bool, &SecurityOptions) -> Result<String, RemoteError>,
{
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
        fetch(path, options.no_cache, &options.to_security_options())?
    } else {
        let content = fs::read_to_string(path).map_err(|e| SchemaError::Read(e.to_string()))?;
        if let Some(hash) = &options.verify_hash {
            remote::verify_content_hash(&content, hash)?;
        }
        content
    };

    // Detect format. Remote URLs keep the permissive default-to-JSON path
    // (paths like https://example.com/api/schema legitimately have no
    // extension); local files use the strict variant so `schema.jsno`
    // fails fast with a clear message instead of a misleading JSON parse
    // error pointing at column 1.
    let format = if remote::is_remote_url(path) {
        SchemaFormat::from_path(path)
    } else {
        SchemaFormat::from_path_strict(path)?
    };
    let schema_file: SchemaFile = parse_schema_content(&content, format)?;

    if schema_file.extends_hash.is_some() && schema_file.extends.is_none() {
        return Err(SchemaError::Parse(
            format.name().into(),
            "extends_hash requires extends".into(),
        ));
    }

    // Start with parent schema if extends is specified
    let mut result = if let Some(ref parent_path) = schema_file.extends {
        if options.verify_hash.is_some() && schema_file.extends_hash.is_none() {
            return Err(SchemaError::Parse(
                format.name().into(),
                "pinned schema inheritance requires extends_hash for each parent".into(),
            ));
        }
        // Resolve parent path relative to current schema
        let parent_full_path = if remote::is_remote_url(path) {
            // For remote schemas, resolve relative URLs.
            // Cross-origin extends are warned (not blocked) -- the SSRF
            // host filter still validates the resolved URL on fetch.
            let resolved = remote::resolve_relative_url(path, parent_path)?;
            if let (Ok(base_u), Ok(resolved_u)) =
                (url::Url::parse(path), url::Url::parse(&resolved))
            {
                if base_u.origin() != resolved_u.origin() {
                    eprintln!(
                        "warning: schema extends crosses origins ({} -> {}); verify trust",
                        base_u.host_str().unwrap_or("?"),
                        resolved_u.host_str().unwrap_or("?")
                    );
                }
            }
            resolved
        } else if remote::is_remote_url(parent_path) {
            parent_path.clone()
        } else {
            resolve_relative_path(path, parent_path)?
        };
        let parent_options = LoadOptions {
            verify_hash: schema_file.extends_hash,
            ..options.clone()
        };
        load_schema_with_chain(&parent_full_path, chain, &parent_options, fetch)?
    } else {
        Schema::new()
    };

    // Merge current schema (child overrides parent)
    for (key, spec) in schema_file.vars {
        result.insert(key, spec);
    }

    Ok(result)
}

/// Resolve a relative path based on the parent file's directory, with a
/// containment guard: the resolved path must canonicalize within the
/// canonicalized parent directory. This blocks `extends: "../../../../etc/passwd"`
/// style traversals out of the schema's directory.
///
/// Absolute `relative_path` values are rejected outright (must use a remote
/// URL for cross-tree references). Both the directory and target must be
/// resolvable so containment checks cannot fail open.
fn resolve_relative_path(base_path: &str, relative_path: &str) -> Result<String, SchemaError> {
    // Reject absolute paths in extends -- forces relative-only navigation.
    let rel = Path::new(relative_path);
    if rel.components().any(|component| {
        matches!(
            component,
            std::path::Component::RootDir | std::path::Component::Prefix(_)
        )
    }) {
        return Err(SchemaError::Read(format!(
            "extends path must be relative, got absolute: {}",
            relative_path
        )));
    }

    let base = Path::new(base_path);
    let parent_dir = base
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let joined = parent_dir.join(relative_path);

    // A missing or unreadable parent cannot establish containment.
    let parent_canon = match fs::canonicalize(parent_dir) {
        Ok(p) => p,
        Err(e) => {
            return Err(SchemaError::Read(format!(
                "cannot resolve schema directory: {}",
                e
            )))
        }
    };

    let target_canon = fs::canonicalize(&joined).map_err(|e| {
        SchemaError::Read(format!(
            "cannot resolve extends path '{}': {}",
            relative_path, e
        ))
    })?;

    if !target_canon.starts_with(&parent_canon) {
        return Err(SchemaError::Read(format!(
            "extends path '{}' escapes schema directory '{}'",
            relative_path,
            parent_canon.display()
        )));
    }

    Ok(target_canon.to_string_lossy().to_string())
}

/// Save schema to file atomically (stage to `.tmp` + rename).
/// Format auto-detected from path extension; strict detection rejects
/// typos like `schema.jsno` rather than silently writing JSON to a file
/// whose extension implies a different format.
pub fn save_schema(path: &str, schema: &Schema) -> Result<(), SchemaError> {
    let format = SchemaFormat::from_path_strict(path)?;
    let content = match format {
        SchemaFormat::Json => serde_json::to_string_pretty(schema)
            .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))?,
        SchemaFormat::Yaml => serde_yaml::to_string(schema)
            .map_err(|e| SchemaError::Parse(format.name().to_string(), e.to_string()))?,
    };
    remote::write_atomic(Path::new(path), content.as_bytes())
        .map_err(|e| SchemaError::Write(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_remote_inheritance_uses_distinct_parent_hashes() {
        let parent = r#"{"BASE":{"type":"int"}}"#;
        let child = serde_json::json!({"extends":"base.json", "extends_hash":remote::compute_content_hash(parent), "CHILD":{"type":"bool"}}).to_string();
        let options = LoadOptions {
            verify_hash: Some(remote::compute_content_hash(&child)),
            ..Default::default()
        };
        let fetch = |url: &str, _: bool, security: &SecurityOptions| {
            let content = if url.ends_with("base.json") {
                parent
            } else {
                &child
            };
            remote::verify_content_hash(content, security.verify_hash.as_deref().unwrap())?;
            Ok(content.to_string())
        };
        let schema = load_schema_with_chain(
            "https://example.com/child.json",
            &mut Vec::new(),
            &options,
            &fetch,
        )
        .unwrap();
        assert!(schema.contains_key("BASE") && schema.contains_key("CHILD"));
        let tampered = |url: &str, _: bool, security: &SecurityOptions| {
            let content = if url.ends_with("base.json") {
                "{}"
            } else {
                &child
            };
            remote::verify_content_hash(content, security.verify_hash.as_deref().unwrap())?;
            Ok(content.to_string())
        };
        assert!(matches!(
            load_schema_with_chain(
                "https://example.com/child.json",
                &mut Vec::new(),
                &options,
                &tampered
            ),
            Err(SchemaError::Remote(RemoteError::HashMismatch { .. }))
        ));
    }

    #[test]
    fn pinned_inheritance_rejects_unpinned_parent_before_fetch() {
        let child = r#"{"extends":"base.json"}"#;
        let options = LoadOptions {
            verify_hash: Some(remote::compute_content_hash(child)),
            ..Default::default()
        };
        let fetch = |url: &str, _: bool, _: &SecurityOptions| {
            assert!(
                url.ends_with("child.json"),
                "must not fetch an unpinned parent"
            );
            Ok(child.to_string())
        };
        let error = load_schema_with_chain(
            "https://example.com/child.json",
            &mut Vec::new(),
            &options,
            &fetch,
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires extends_hash"));
    }

    #[test]
    fn local_schema_can_extend_a_pinned_remote_parent() {
        let dir = tempfile::tempdir().unwrap();
        let child_path = dir.path().join("child.json");
        let parent = r#"{"BASE":{"type":"int"}}"#;
        let hash = remote::compute_content_hash(parent);
        fs::write(
            &child_path,
            serde_json::json!({"extends":"https://example.com/base.json", "extends_hash":hash})
                .to_string(),
        )
        .unwrap();
        let fetch = |url: &str, _: bool, security: &SecurityOptions| {
            assert_eq!(url, "https://example.com/base.json");
            assert_eq!(security.verify_hash.as_deref(), Some(hash.as_str()));
            Ok(parent.to_string())
        };
        assert!(load_schema_with_chain(
            child_path.to_str().unwrap(),
            &mut Vec::new(),
            &LoadOptions::default(),
            &fetch
        )
        .unwrap()
        .contains_key("BASE"));
    }

    /// Test helper: load schema with default options
    fn load_schema(path: &str) -> Result<Schema, SchemaError> {
        load_schema_with_options(path, &LoadOptions::default())
    }

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
        assert_eq!(
            spec.values,
            Some(vec![
                "dev".to_string(),
                "staging".to_string(),
                "prod".to_string()
            ])
        );
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
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("child.json");
        let sibling = dir.path().join("base.json");
        fs::write(&sibling, "{}").unwrap();
        let result = resolve_relative_path(base.to_str().unwrap(), "base.json").unwrap();
        assert_eq!(Path::new(&result), fs::canonicalize(sibling).unwrap());
        assert!(resolve_relative_path(base.to_str().unwrap(), "missing.json").is_err());
        assert!(resolve_relative_path("missing-directory/child.json", "base.json").is_err());
    }

    #[test]
    fn test_resolve_relative_path_rejects_absolute() {
        // Use a platform-specific absolute path so this test passes on both
        // Unix (/etc/passwd) and Windows (C:\Windows\System32). Path::is_absolute
        // is platform-dependent.
        #[cfg(unix)]
        let abs = "/etc/passwd";
        #[cfg(windows)]
        let abs = r"C:\Windows\System32\config\sam";
        let result = resolve_relative_path("dir/child.json", abs);
        assert!(result.is_err(), "absolute extends path must be rejected");
    }

    #[test]
    fn test_resolve_relative_path_blocks_traversal_when_canonicalizable() {
        // Real on-disk traversal: parent_dir = canonicalized cwd, target =
        // /etc/passwd via "../../../../etc/passwd". Containment check fires
        // because the canonicalized target doesn't start with parent_canon.
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("schemas").join("child.json");
        std::fs::create_dir_all(base.parent().unwrap()).unwrap();
        std::fs::write(&base, "{}").unwrap();

        // Traversal that escapes tmp via the system root.
        // (We can't write /etc/passwd in tests; use ../../../../tmp -- the
        // canonicalize will resolve to actual /tmp on Unix or escape the
        // tempdir root, which fails containment.)
        let escape = "../../../../../../../../../tmp";
        let result = resolve_relative_path(base.to_str().unwrap(), escape);
        assert!(result.is_err(), "traversal must fail closed");
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
        writeln!(
            base_file,
            r#"{{"BASE_VAR": {{"type": "string", "required": true}}}}"#
        )
        .unwrap();

        // Create child schema that extends base
        let child_path = dir.path().join("child.schema.json");
        let mut child_file = fs::File::create(&child_path).unwrap();
        writeln!(
            child_file,
            r#"{{"extends": "base.schema.json", "CHILD_VAR": {{"type": "int"}}}}"#
        )
        .unwrap();

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
        writeln!(
            base_file,
            r#"{{"PORT": {{"type": "string", "description": "base desc"}}}}"#
        )
        .unwrap();

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
        writeln!(
            p_file,
            r#"{{"extends": "grandparent.json", "P_VAR": {{"type": "string"}}}}"#
        )
        .unwrap();

        // Child extends parent
        let c_path = dir.path().join("child.json");
        let mut c_file = fs::File::create(&c_path).unwrap();
        writeln!(
            c_file,
            r#"{{"extends": "parent.json", "C_VAR": {{"type": "string"}}}}"#
        )
        .unwrap();

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
        writeln!(
            a_file,
            r#"{{"extends": "b.json", "A": {{"type": "string"}}}}"#
        )
        .unwrap();

        // B extends A (circular!)
        let b_path = dir.path().join("b.json");
        let mut b_file = fs::File::create(&b_path).unwrap();
        writeln!(
            b_file,
            r#"{{"extends": "a.json", "B": {{"type": "string"}}}}"#
        )
        .unwrap();

        let result = load_schema(a_path.to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaError::CircularInheritance(_)));
    }

    // YAML schema format tests
    #[test]
    fn test_schema_format_detection_json() {
        assert_eq!(SchemaFormat::from_path("schema.json"), SchemaFormat::Json);
        assert_eq!(
            SchemaFormat::from_path("path/to/schema.JSON"),
            SchemaFormat::Json
        );
        assert_eq!(
            SchemaFormat::from_path("env.schema.json"),
            SchemaFormat::Json
        );
    }

    #[test]
    fn test_schema_format_detection_yaml() {
        assert_eq!(SchemaFormat::from_path("schema.yaml"), SchemaFormat::Yaml);
        assert_eq!(SchemaFormat::from_path("schema.yml"), SchemaFormat::Yaml);
        assert_eq!(
            SchemaFormat::from_path("path/to/schema.YAML"),
            SchemaFormat::Yaml
        );
        assert_eq!(
            SchemaFormat::from_path("env.schema.yml"),
            SchemaFormat::Yaml
        );
    }

    #[test]
    fn test_schema_format_detection_default() {
        // Unknown extensions default to JSON via the permissive helper
        assert_eq!(SchemaFormat::from_path("schema"), SchemaFormat::Json);
        assert_eq!(SchemaFormat::from_path("schema.txt"), SchemaFormat::Json);
    }

    #[test]
    fn test_schema_format_strict_recognized_extensions() {
        assert_eq!(
            SchemaFormat::from_path_strict("schema.json").unwrap(),
            SchemaFormat::Json
        );
        assert_eq!(
            SchemaFormat::from_path_strict("schema.yaml").unwrap(),
            SchemaFormat::Yaml
        );
        assert_eq!(
            SchemaFormat::from_path_strict("schema.yml").unwrap(),
            SchemaFormat::Yaml
        );
    }

    #[test]
    fn test_schema_format_strict_rejects_typo_extension() {
        // The whole point of the strict variant: `schema.jsno` should fail
        // with a path-pointing error, not with a confusing JSON parse error
        // 200 lines later.
        let err = SchemaFormat::from_path_strict("schema.jsno").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unrecognized extension"), "msg was: {}", msg);
        assert!(msg.contains("schema.jsno"), "msg was: {}", msg);

        let err = SchemaFormat::from_path_strict("schema.txt").unwrap_err();
        assert!(err.to_string().contains("unrecognized extension"));
    }

    #[test]
    fn test_schema_format_strict_allows_extensionless() {
        // No dot in basename -> permitted (remote-URL paths legitimately
        // have no extension; bare local files are a tolerated edge case).
        assert_eq!(
            SchemaFormat::from_path_strict("schema").unwrap(),
            SchemaFormat::Json
        );
    }

    #[test]
    fn test_schema_load_rejects_typo_extension() {
        // End-to-end: passing a .jsno path to load_schema must error before
        // serde_json is invoked, with the message pointing at the file.
        let dir = tempfile::TempDir::new().unwrap();
        let bad_path = dir.path().join("env.schema.jsno");
        std::fs::write(&bad_path, r#"{"FOO": {"type": "string"}}"#).unwrap();

        let result = load_schema(bad_path.to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unrecognized extension"), "msg was: {}", msg);
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
        assert_eq!(
            env.values,
            Some(vec![
                "development".to_string(),
                "staging".to_string(),
                "production".to_string()
            ])
        );
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
        writeln!(
            yaml_file,
            "extends: base.schema.json\nCHILD_VAR:\n  type: int"
        )
        .unwrap();

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
        writeln!(
            json_file,
            r#"{{"extends": "base.schema.yaml", "CHILD_VAR": {{"type": "int"}}}}"#
        )
        .unwrap();

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
        schema.insert(
            "TEST_VAR".to_string(),
            VarSpec {
                var_type: VarType::String,
                required: true,
                ..Default::default()
            },
        );

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

    #[test]
    fn test_load_options_default() {
        let opts = LoadOptions::default();
        assert!(!opts.no_cache);
        assert!(opts.verify_hash.is_none());
        assert!(opts.ca_cert.is_none());
        assert!(opts.rate_limit_seconds.is_none());
    }

    #[test]
    fn test_schema_error_display() {
        let read_err = SchemaError::Read("file not found".to_string());
        assert!(read_err.to_string().contains("file not found"));

        let parse_err = SchemaError::Parse("JSON".to_string(), "syntax error".to_string());
        assert!(parse_err.to_string().contains("JSON"));
        assert!(parse_err.to_string().contains("syntax error"));

        let circular_err = SchemaError::CircularInheritance("schema.json".to_string());
        assert!(circular_err.to_string().contains("circular"));

        let depth_err = SchemaError::InheritanceDepthExceeded;
        assert!(depth_err.to_string().contains("depth"));
    }
}
