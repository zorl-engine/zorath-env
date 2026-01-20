use std::collections::HashMap;
use std::fs;

use crate::envfile;
use crate::schema::{self, LoadOptions};

/// Supported export formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Shell,      // export FOO="bar"
    Docker,     // ENV FOO=bar
    K8s,        // ConfigMap YAML
    Json,       // JSON object
    Systemd,    // Environment=FOO=bar
    Dotenv,     // FOO=bar (standard .env)
}

impl ExportFormat {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "shell" | "bash" | "sh" => Some(ExportFormat::Shell),
            "docker" | "dockerfile" => Some(ExportFormat::Docker),
            "k8s" | "kubernetes" | "configmap" => Some(ExportFormat::K8s),
            "json" => Some(ExportFormat::Json),
            "systemd" | "service" => Some(ExportFormat::Systemd),
            "dotenv" | "env" => Some(ExportFormat::Dotenv),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ExportFormat::Shell => "shell",
            ExportFormat::Docker => "docker",
            ExportFormat::K8s => "k8s",
            ExportFormat::Json => "json",
            ExportFormat::Systemd => "systemd",
            ExportFormat::Dotenv => "dotenv",
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
pub fn export_to_string(env_map: &HashMap<String, String>, format: ExportFormat) -> Result<String, String> {
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
    }
}

pub fn run(
    env_path: &str,
    schema_path: Option<&str>,
    format: &str,
    output: Option<&str>,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
) -> Result<(), String> {
    // Parse format
    let export_format = ExportFormat::from_str(format)
        .ok_or_else(|| format!("Unknown format '{}'. Valid formats: shell, docker, k8s, json, systemd, dotenv", format))?;

    // Load and parse env file
    let content = fs::read_to_string(env_path)
        .map_err(|e| format!("Failed to read {}: {}", env_path, e))?;
    let env_map = envfile::parse_env_file(env_path)
        .map_err(|e| format!("Failed to parse {}: {}", env_path, e))?;

    // Interpolate variables
    let env_map = envfile::interpolate_env(env_map)
        .map_err(|e| format!("Interpolation error: {}", e))?;

    // Optionally validate against schema
    if let Some(schema_path) = schema_path {
        let options = LoadOptions {
            no_cache,
            verify_hash: verify_hash.map(|s| s.to_string()),
            ca_cert: ca_cert.map(|s| s.to_string()),
            rate_limit_seconds: None,
        };
        let schema = schema::load_schema_with_options(schema_path, &options)
            .map_err(|e| e.to_string())?;

        // Filter to only include keys that are in the schema
        let filtered: HashMap<String, String> = env_map
            .into_iter()
            .filter(|(k, _)| schema.contains_key(k))
            .collect();

        let result = export(&filtered, export_format, &content)?;
        output_result(&result, output)
    } else {
        let result = export(&env_map, export_format, &content)?;
        output_result(&result, output)
    }
}

fn output_result(result: &str, output: Option<&str>) -> Result<(), String> {
    match output {
        Some(path) => {
            fs::write(path, result)
                .map_err(|e| format!("Failed to write {}: {}", path, e))?;
            eprintln!("Exported to {}", path);
            Ok(())
        }
        None => {
            println!("{}", result);
            Ok(())
        }
    }
}

fn export(env_map: &HashMap<String, String>, format: ExportFormat, _raw_content: &str) -> Result<String, String> {
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
    }
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
    serde_json::to_string_pretty(&map)
        .map_err(|e| format!("JSON serialization error: {}", e))
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
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

/// Escape a value for YAML
fn escape_yaml_value(value: &str) -> String {
    // YAML needs quoting for certain characters
    if value.contains(':') || value.contains('#') || value.contains('\n')
        || value.contains('"') || value.contains('\'') || value.is_empty()
        || value.starts_with(' ') || value.ends_with(' ')
        || value == "true" || value == "false" || value == "null"
        || value.parse::<f64>().is_ok()
    {
        // Use double quotes and escape
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
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
        entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_export_format_from_str() {
        assert_eq!(ExportFormat::from_str("shell"), Some(ExportFormat::Shell));
        assert_eq!(ExportFormat::from_str("bash"), Some(ExportFormat::Shell));
        assert_eq!(ExportFormat::from_str("docker"), Some(ExportFormat::Docker));
        assert_eq!(ExportFormat::from_str("dockerfile"), Some(ExportFormat::Docker));
        assert_eq!(ExportFormat::from_str("k8s"), Some(ExportFormat::K8s));
        assert_eq!(ExportFormat::from_str("kubernetes"), Some(ExportFormat::K8s));
        assert_eq!(ExportFormat::from_str("configmap"), Some(ExportFormat::K8s));
        assert_eq!(ExportFormat::from_str("json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::from_str("systemd"), Some(ExportFormat::Systemd));
        assert_eq!(ExportFormat::from_str("service"), Some(ExportFormat::Systemd));
        assert_eq!(ExportFormat::from_str("dotenv"), Some(ExportFormat::Dotenv));
        assert_eq!(ExportFormat::from_str("env"), Some(ExportFormat::Dotenv));
        assert_eq!(ExportFormat::from_str("invalid"), None);
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
}
