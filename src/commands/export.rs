use std::collections::HashMap;

use crate::envfile;
use crate::errors::CliError;
use crate::schema::{self, LoadOptions};

/// Supported export formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Shell,         // export FOO="bar"
    Docker,        // ENV FOO=bar
    K8s,           // ConfigMap YAML
    Json,          // JSON object
    Systemd,       // Environment=FOO=bar
    Dotenv,        // FOO=bar (standard .env)
    GithubSecrets, // gh secret set commands
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "shell" | "bash" | "sh" => Ok(ExportFormat::Shell),
            "docker" | "dockerfile" => Ok(ExportFormat::Docker),
            "k8s" | "kubernetes" | "configmap" => Ok(ExportFormat::K8s),
            "json" => Ok(ExportFormat::Json),
            "systemd" | "service" => Ok(ExportFormat::Systemd),
            "dotenv" | "env" => Ok(ExportFormat::Dotenv),
            "github-secrets" | "gh-secrets" | "github" => Ok(ExportFormat::GithubSecrets),
            _ => Err(format!(
                "Unknown format '{}'. Valid formats:\n  \
                shell (aliases: bash, sh)\n  \
                docker (alias: dockerfile)\n  \
                k8s (aliases: kubernetes, configmap)\n  \
                json\n  \
                systemd (alias: service)\n  \
                dotenv (alias: env)\n  \
                github-secrets (aliases: gh-secrets, github)",
                s
            )),
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl ExportFormat {
    pub fn name(&self) -> &'static str {
        match self {
            ExportFormat::Shell => "shell",
            ExportFormat::Docker => "docker",
            ExportFormat::K8s => "k8s",
            ExportFormat::Json => "json",
            ExportFormat::Systemd => "systemd",
            ExportFormat::Dotenv => "dotenv",
            ExportFormat::GithubSecrets => "github-secrets",
        }
    }
}

/// Export environment variables to a string (library function)
///
/// This is the pure library function that takes an environment HashMap
/// and returns the exported content as a String. No file I/O.
///
/// # Arguments
/// * `env_map` - HashMap of environment variable key-value pairs
/// * `format` - Export format (use ExportFormat::from_str to parse)
///
/// # Returns
/// The exported content as a String, or an error message
///
/// # Example
/// ```ignore
/// let mut env = HashMap::new();
/// env.insert("PORT".to_string(), "3000".to_string());
/// let output = export_to_string(&env, ExportFormat::Shell)?;
/// ```
pub fn export_to_string(
    env_map: &HashMap<String, String>,
    format: ExportFormat,
) -> Result<String, String> {
    // Sort keys for consistent output
    let mut keys: Vec<&String> = env_map.keys().collect();
    keys.sort();

    match format {
        ExportFormat::Shell => export_shell(&keys, env_map),
        ExportFormat::Docker => export_docker(&keys, env_map),
        ExportFormat::K8s => export_k8s(&keys, env_map),
        ExportFormat::Json => export_json(&keys, env_map),
        ExportFormat::Systemd => export_systemd(&keys, env_map),
        ExportFormat::Dotenv => export_dotenv(&keys, env_map),
        ExportFormat::GithubSecrets => export_github_secrets(&keys, env_map),
    }
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run(
    env_path: &str,
    schema_path: Option<&str>,
    format: &str,
    output: Option<&str>,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    // Parse format
    let export_format: ExportFormat = format.parse().map_err(CliError::Input)?;

    // Load and parse env file
    let env_map = envfile::parse_env_file(env_path)
        .map_err(|e| CliError::Input(format!("Failed to parse {}: {}", env_path, e)))?;

    // Interpolate variables
    let env_map = envfile::interpolate_env(env_map)
        .map_err(|e| CliError::Input(format!("Interpolation error: {}", e)))?;

    // Optionally validate against schema
    if let Some(schema_path) = schema_path {
        let options = LoadOptions {
            no_cache,
            verify_hash: verify_hash.map(|s| s.to_string()),
            ca_cert: ca_cert.map(|s| s.to_string()),
            rate_limit_seconds,
        };
        let schema = schema::load_schema_with_options(schema_path, &options)
            .map_err(|e| CliError::Schema(e.to_string()))?;

        // Filter to only include keys that are in the schema
        let filtered: HashMap<String, String> = env_map
            .into_iter()
            .filter(|(k, _)| schema.contains_key(k))
            .collect();

        // Warn when emitting `secret: true` keys to plaintext formats.
        // ConfigMaps / dotenv / json on disk are not encrypted at rest.
        warn_if_secret_to_plaintext(&filtered, &schema, export_format);

        let result = export(&filtered, export_format).map_err(CliError::Input)?;
        output_result(&result, output)
    } else {
        let result = export(&env_map, export_format).map_err(CliError::Input)?;
        output_result(&result, output)
    }
}

/// Print a warning if any key being exported has `secret: true` in the
/// schema and the target format stores the value in plaintext (k8s
/// ConfigMap, JSON, dotenv, systemd, shell). Skipped for github-secrets
/// which is the intended target for sensitive values.
fn warn_if_secret_to_plaintext(
    env_map: &HashMap<String, String>,
    schema: &schema::Schema,
    format: ExportFormat,
) {
    use ExportFormat::*;
    let target_is_plaintext = matches!(format, K8s | Json | Dotenv | Systemd | Shell | Docker);
    if !target_is_plaintext {
        return;
    }
    let mut leaked: Vec<&str> = env_map
        .keys()
        .filter_map(|k| {
            let spec = schema.get(k)?;
            if spec.secret == Some(true) {
                Some(k.as_str())
            } else {
                None
            }
        })
        .collect();
    leaked.sort();
    if !leaked.is_empty() && std::env::var("ZENV_QUIET").is_err() {
        eprintln!(
            "warning: exporting {} key(s) marked secret: true to plaintext format '{}': {}",
            leaked.len(),
            format.name(),
            leaked.join(", ")
        );
        eprintln!("         consider --format github-secrets or your platform's secrets manager.");
    }
}

fn output_result(result: &str, output: Option<&str>) -> Result<(), CliError> {
    match output {
        Some(path) => {
            crate::remote::write_atomic(std::path::Path::new(path), result.as_bytes())
                .map_err(|e| CliError::Input(format!("Failed to write {}: {}", path, e)))?;
            eprintln!("Exported to {}", path);
            Ok(())
        }
        None => {
            println!("{}", result);
            Ok(())
        }
    }
}

fn export(env_map: &HashMap<String, String>, format: ExportFormat) -> Result<String, String> {
    export_to_string(env_map, format)
}

/// Export as shell script (export FOO="bar")
fn export_shell(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push("#!/bin/sh".to_string());
    lines.push("# Generated by zenv".to_string());
    lines.push("".to_string());

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            let escaped = escape_shell_value(value);
            lines.push(format!("export {}=\"{}\"", key, escaped));
        }
    }

    Ok(lines.join("\n"))
}

/// Export as Dockerfile ENV statements
fn export_docker(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push("# Generated by zenv".to_string());
    lines.push("".to_string());

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            let escaped = escape_docker_value(value);
            lines.push(format!("ENV {}={}", key, escaped));
        }
    }

    Ok(lines.join("\n"))
}

/// Export as Kubernetes ConfigMap YAML
fn export_k8s(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut lines = vec![
        "# Generated by zenv".to_string(),
        "apiVersion: v1".to_string(),
        "kind: ConfigMap".to_string(),
        "metadata:".to_string(),
        "  name: app-config".to_string(),
        "data:".to_string(),
    ];

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            // YAML strings need proper quoting for special characters
            let yaml_value = escape_yaml_value(value);
            lines.push(format!("  {}: {}", key, yaml_value));
        }
    }

    Ok(lines.join("\n"))
}

/// Export as JSON object
fn export_json(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut map = serde_json::Map::new();
    for key in keys {
        if let Some(value) = env_map.get(*key) {
            map.insert((*key).clone(), serde_json::Value::String(value.clone()));
        }
    }
    serde_json::to_string_pretty(&map).map_err(|e| format!("JSON serialization error: {}", e))
}

/// Export as systemd Environment directives
fn export_systemd(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push("# Generated by zenv".to_string());
    lines.push("# Add to [Service] section of your .service file".to_string());
    lines.push("".to_string());

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            let escaped = escape_systemd_value(value);
            lines.push(format!("Environment=\"{}={}\"", key, escaped));
        }
    }

    Ok(lines.join("\n"))
}

/// Export as standard .env format
fn export_dotenv(keys: &[&String], env_map: &HashMap<String, String>) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push("# Generated by zenv".to_string());
    lines.push("".to_string());

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            // Quote values that contain special characters
            if needs_quoting(value) {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                lines.push(format!("{}=\"{}\"", key, escaped));
            } else {
                lines.push(format!("{}={}", key, value));
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Export as GitHub Secrets CLI commands (gh secret set)
fn export_github_secrets(
    keys: &[&String],
    env_map: &HashMap<String, String>,
) -> Result<String, String> {
    let mut lines = vec![
        "#!/bin/sh".to_string(),
        "# Generated by zenv".to_string(),
        "# Run this script to set GitHub repository secrets".to_string(),
        "# Requires: gh auth login".to_string(),
        "".to_string(),
    ];

    for key in keys {
        if let Some(value) = env_map.get(*key) {
            // Use heredoc for multi-line or special characters
            if value.contains('\n') || value.contains('\'') {
                lines.push(format!("gh secret set {} << 'EOF'", key));
                lines.push(value.clone());
                lines.push("EOF".to_string());
            } else {
                // Simple values can use --body
                let escaped = value.replace('\\', "\\\\").replace('\'', "'\\''");
                lines.push(format!("gh secret set {} --body '{}'", key, escaped));
            }
        }
    }

    Ok(lines.join("\n"))
}

/// Escape a value for shell script (double-quoted string)
fn escape_shell_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('\n', "\\n")
}

/// Escape a value for Dockerfile ENV
fn escape_docker_value(value: &str) -> String {
    // Docker ENV values with spaces need quoting
    if value.contains(' ') || value.contains('"') || value.contains('$') {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

/// Escape a value for YAML.
///
/// Force-quotes when the value would be misparsed by `kubectl apply` or any
/// YAML 1.2 parser. Includes the YAML "indicator" leading characters that
/// the prior implementation missed: `&` (anchor), `*` (alias), `!` (tag),
/// `|` `>` (block scalars), `%` (directive), `@` ``` (reserved),
/// `?` (mapping key), `-` (sequence entry), `[` `{` `,` (flow indicators).
fn escape_yaml_value(value: &str) -> String {
    let needs_quote = value.contains(':')
        || value.contains('#')
        || value.contains('\n')
        || value.contains('\t')
        || value.contains('"')
        || value.contains('\'')
        || value.is_empty()
        || value.starts_with(' ')
        || value.ends_with(' ')
        || value == "true"
        || value == "false"
        || value == "null"
        || value.parse::<f64>().is_ok()
        // Leading YAML indicators that change parsing
        || matches!(
            value.as_bytes().first(),
            Some(b'&' | b'*' | b'!' | b'|' | b'>' | b'%' | b'@' | b'`' | b'?' | b'-' | b'[' | b'{' | b',')
        )
        // Any control character (other than tab/newline already handled)
        || value.chars().any(|c| c.is_control() && c != '\t' && c != '\n');

    if needs_quote {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

/// Escape a value for systemd Environment directive
fn escape_systemd_value(value: &str) -> String {
    // Systemd uses double quotes, escape accordingly
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Check if a value needs quoting in .env format
fn needs_quoting(value: &str) -> bool {
    value.contains(' ')
        || value.contains('"')
        || value.contains('\'')
        || value.contains('#')
        || value.contains('\n')
        || value.contains('$')
        || value.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env(entries: Vec<(&str, &str)>) -> HashMap<String, String> {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_export_format_from_str() {
        assert_eq!("shell".parse::<ExportFormat>(), Ok(ExportFormat::Shell));
        assert_eq!("bash".parse::<ExportFormat>(), Ok(ExportFormat::Shell));
        assert_eq!("docker".parse::<ExportFormat>(), Ok(ExportFormat::Docker));
        assert_eq!(
            "dockerfile".parse::<ExportFormat>(),
            Ok(ExportFormat::Docker)
        );
        assert_eq!("k8s".parse::<ExportFormat>(), Ok(ExportFormat::K8s));
        assert_eq!("kubernetes".parse::<ExportFormat>(), Ok(ExportFormat::K8s));
        assert_eq!("configmap".parse::<ExportFormat>(), Ok(ExportFormat::K8s));
        assert_eq!("json".parse::<ExportFormat>(), Ok(ExportFormat::Json));
        assert_eq!("systemd".parse::<ExportFormat>(), Ok(ExportFormat::Systemd));
        assert_eq!("service".parse::<ExportFormat>(), Ok(ExportFormat::Systemd));
        assert_eq!("dotenv".parse::<ExportFormat>(), Ok(ExportFormat::Dotenv));
        assert_eq!("env".parse::<ExportFormat>(), Ok(ExportFormat::Dotenv));
        assert!("invalid".parse::<ExportFormat>().is_err());
    }

    #[test]
    fn test_export_shell() {
        let env = make_env(vec![("FOO", "bar"), ("BAZ", "qux")]);
        let baz = "BAZ".to_string();
        let foo = "FOO".to_string();
        let keys: Vec<&String> = vec![&baz, &foo];
        let result = export_shell(&keys, &env).unwrap();
        assert!(result.contains("#!/bin/sh"));
        assert!(result.contains("export BAZ=\"qux\""));
        assert!(result.contains("export FOO=\"bar\""));
    }

    #[test]
    fn test_export_shell_escapes() {
        let env = make_env(vec![("KEY", "value with \"quotes\" and $var")]);
        let key = "KEY".to_string();
        let keys: Vec<&String> = vec![&key];
        let result = export_shell(&keys, &env).unwrap();
        assert!(result.contains("\\\"quotes\\\""));
        assert!(result.contains("\\$var"));
    }

    #[test]
    fn test_export_docker() {
        let env = make_env(vec![("PORT", "3000"), ("NAME", "my app")]);
        let name = "NAME".to_string();
        let port = "PORT".to_string();
        let keys: Vec<&String> = vec![&name, &port];
        let result = export_docker(&keys, &env).unwrap();
        assert!(result.contains("ENV NAME=\"my app\""));
        assert!(result.contains("ENV PORT=3000"));
    }

    #[test]
    fn test_export_k8s() {
        let env = make_env(vec![("DATABASE_URL", "postgres://localhost/db")]);
        let db_url = "DATABASE_URL".to_string();
        let keys: Vec<&String> = vec![&db_url];
        let result = export_k8s(&keys, &env).unwrap();
        assert!(result.contains("apiVersion: v1"));
        assert!(result.contains("kind: ConfigMap"));
        assert!(result.contains("DATABASE_URL:"));
    }

    #[test]
    fn test_export_json() {
        let env = make_env(vec![("FOO", "bar")]);
        let foo = "FOO".to_string();
        let keys: Vec<&String> = vec![&foo];
        let result = export_json(&keys, &env).unwrap();
        assert!(result.contains("\"FOO\": \"bar\""));
    }

    #[test]
    fn test_export_systemd() {
        let env = make_env(vec![("FOO", "bar")]);
        let foo = "FOO".to_string();
        let keys: Vec<&String> = vec![&foo];
        let result = export_systemd(&keys, &env).unwrap();
        assert!(result.contains("Environment=\"FOO=bar\""));
    }

    #[test]
    fn test_export_dotenv() {
        let env = make_env(vec![("SIMPLE", "value"), ("COMPLEX", "has spaces")]);
        let complex = "COMPLEX".to_string();
        let simple = "SIMPLE".to_string();
        let keys: Vec<&String> = vec![&complex, &simple];
        let result = export_dotenv(&keys, &env).unwrap();
        assert!(result.contains("SIMPLE=value"));
        assert!(result.contains("COMPLEX=\"has spaces\""));
    }

    #[test]
    fn test_escape_yaml_special_values() {
        // Boolean-like values need quoting
        assert_eq!(escape_yaml_value("true"), "\"true\"");
        assert_eq!(escape_yaml_value("false"), "\"false\"");
        assert_eq!(escape_yaml_value("null"), "\"null\"");
        // Numbers need quoting
        assert_eq!(escape_yaml_value("123"), "\"123\"");
        assert_eq!(escape_yaml_value("3.14"), "\"3.14\"");
        // Normal strings don't need quoting
        assert_eq!(escape_yaml_value("normal"), "normal");
    }

    // =========================================================================
    // GitHub Secrets Export Tests (v0.3.8)
    // =========================================================================

    #[test]
    fn test_export_github_secrets_simple() {
        let env = make_env(vec![("API_KEY", "secret123"), ("PORT", "3000")]);
        let api_key = "API_KEY".to_string();
        let port = "PORT".to_string();
        let keys: Vec<&String> = vec![&api_key, &port];
        let result = export_github_secrets(&keys, &env).unwrap();

        assert!(result.contains("#!/bin/sh"), "Should have shebang");
        assert!(
            result.contains("gh secret set API_KEY --body 'secret123'"),
            "Should use --body for simple values"
        );
        assert!(
            result.contains("gh secret set PORT --body '3000'"),
            "Should include PORT"
        );
    }

    #[test]
    fn test_export_github_secrets_multiline() {
        let multiline_value = "line1\nline2\nline3";
        let env = make_env(vec![("PRIVATE_KEY", multiline_value)]);
        let key = "PRIVATE_KEY".to_string();
        let keys: Vec<&String> = vec![&key];
        let result = export_github_secrets(&keys, &env).unwrap();

        // Multiline values should use heredoc format
        assert!(
            result.contains("gh secret set PRIVATE_KEY << 'EOF'"),
            "Should use heredoc for multiline"
        );
        assert!(
            result.contains("line1\nline2\nline3"),
            "Should preserve newlines"
        );
        assert!(result.contains("EOF"), "Should close heredoc");
    }

    #[test]
    fn test_export_github_secrets_special_chars() {
        // Single quotes need special handling
        let env = make_env(vec![("CONFIG", "it's a test")]);
        let key = "CONFIG".to_string();
        let keys: Vec<&String> = vec![&key];
        let result = export_github_secrets(&keys, &env).unwrap();

        // Values with single quotes should use heredoc
        assert!(
            result.contains("gh secret set CONFIG << 'EOF'"),
            "Should use heredoc for quotes"
        );
    }

    #[test]
    fn test_export_github_secrets_escapes_backslashes() {
        let env = make_env(vec![("PATH_VAR", "C:\\Users\\test")]);
        let key = "PATH_VAR".to_string();
        let keys: Vec<&String> = vec![&key];
        let result = export_github_secrets(&keys, &env).unwrap();

        // Should escape backslashes
        assert!(
            result.contains("gh secret set PATH_VAR"),
            "Should include key"
        );
        assert!(
            result.contains("--body") || result.contains("EOF"),
            "Should have body or heredoc"
        );
    }

    #[test]
    fn test_export_github_secrets_header_comments() {
        let env = make_env(vec![("FOO", "bar")]);
        let foo = "FOO".to_string();
        let keys: Vec<&String> = vec![&foo];
        let result = export_github_secrets(&keys, &env).unwrap();

        assert!(result.contains("#!/bin/sh"), "Should have shebang");
        assert!(
            result.contains("# Generated by zenv"),
            "Should have generator comment"
        );
        assert!(
            result.contains("gh auth login"),
            "Should mention auth requirement"
        );
    }

    #[test]
    fn test_export_github_secrets_format_alias() {
        // Test all format aliases parse correctly
        assert_eq!(
            "github-secrets".parse::<ExportFormat>(),
            Ok(ExportFormat::GithubSecrets)
        );
        assert_eq!(
            "gh-secrets".parse::<ExportFormat>(),
            Ok(ExportFormat::GithubSecrets)
        );
        assert_eq!(
            "github".parse::<ExportFormat>(),
            Ok(ExportFormat::GithubSecrets)
        );
    }
}
