use crate::schema::{load_schema_with_options, LoadOptions, VarSpec, VarType};
use std::fs;

/// Generate .env.example from schema
pub fn run(schema_path: &str, output_path: Option<&str>, include_defaults: bool, no_cache: bool) -> Result<(), String> {
    let options = LoadOptions { no_cache };
    let schema = load_schema_with_options(schema_path, &options).map_err(|e| e.to_string())?;

    // Sort keys alphabetically for consistent output
    let mut keys: Vec<&String> = schema.keys().collect();
    keys.sort();

    let mut output = String::new();

    for (i, key) in keys.iter().enumerate() {
        let spec = schema.get(*key).unwrap();

        // Add blank line between variables (not before first)
        if i > 0 {
            output.push('\n');
        }

        // Generate comments for this variable
        output.push_str(&generate_var_comments(key, spec));

        // Generate the variable line
        output.push_str(&generate_var_line(key, spec, include_defaults));
        output.push('\n');
    }

    // Output to file or stdout
    if let Some(path) = output_path {
        fs::write(path, &output).map_err(|e| format!("failed to write output: {}", e))?;
        println!("Generated: {}", path);
    } else {
        print!("{}", output);
    }

    Ok(())
}

/// Generate comment lines for a variable
fn generate_var_comments(_key: &str, spec: &VarSpec) -> String {
    let mut comments = String::new();

    // Description
    if let Some(ref desc) = spec.description {
        comments.push_str(&format!("# {}\n", desc));
    }

    // Type and required status
    let type_str = match spec.var_type {
        VarType::String => "string",
        VarType::Int => "int",
        VarType::Float => "float",
        VarType::Bool => "bool",
        VarType::Url => "url",
        VarType::Enum => "enum",
    };
    let required_str = if spec.required { "required" } else { "optional" };
    comments.push_str(&format!("# Type: {} ({})\n", type_str, required_str));

    // Enum values
    if let Some(ref values) = spec.values {
        comments.push_str(&format!("# Values: {}\n", values.join(", ")));
    }

    // Default value
    if let Some(ref default) = spec.default {
        let default_str = format_default_value(default);
        comments.push_str(&format!("# Default: {}\n", default_str));
    }

    // Validation rules
    if let Some(ref validate) = spec.validate {
        let mut rules = Vec::new();

        if let Some(min) = validate.min {
            rules.push(format!("min={}", min));
        }
        if let Some(max) = validate.max {
            rules.push(format!("max={}", max));
        }
        if let Some(min_value) = validate.min_value {
            rules.push(format!("min_value={}", min_value));
        }
        if let Some(max_value) = validate.max_value {
            rules.push(format!("max_value={}", max_value));
        }
        if let Some(min_length) = validate.min_length {
            rules.push(format!("min_length={}", min_length));
        }
        if let Some(max_length) = validate.max_length {
            rules.push(format!("max_length={}", max_length));
        }
        if let Some(ref pattern) = validate.pattern {
            rules.push(format!("pattern={}", pattern));
        }

        if !rules.is_empty() {
            comments.push_str(&format!("# Validation: {}\n", rules.join(", ")));
        }
    }

    comments
}

/// Format a default value for display
fn format_default_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

/// Generate the KEY=value line
fn generate_var_line(key: &str, spec: &VarSpec, include_defaults: bool) -> String {
    let value = if include_defaults {
        spec.default
            .as_ref()
            .map(format_default_value)
            .unwrap_or_else(|| generate_placeholder(key, spec))
    } else {
        generate_placeholder(key, spec)
    };

    format!("{}={}", key, value)
}

/// Generate a type-aware placeholder value
fn generate_placeholder(key: &str, spec: &VarSpec) -> String {
    let lower_key = key.to_lowercase();

    // Special cases based on key name
    if lower_key.contains("database") || lower_key.contains("db_url") {
        return "postgres://user:password@localhost:5432/dbname".to_string();
    }
    if lower_key.contains("redis") {
        return "redis://localhost:6379".to_string();
    }
    if lower_key.contains("mongo") {
        return "mongodb://localhost:27017/dbname".to_string();
    }
    if lower_key == "port" || lower_key.ends_with("_port") {
        return "3000".to_string();
    }
    if lower_key.contains("host") && !lower_key.contains("url") {
        return "localhost".to_string();
    }
    if lower_key.contains("api_key") || lower_key.contains("apikey") {
        return "your_api_key_here".to_string();
    }
    if lower_key.contains("secret") {
        return "your_secret_here".to_string();
    }
    if lower_key.contains("token") {
        return "your_token_here".to_string();
    }
    if lower_key.contains("password") || lower_key.contains("passwd") {
        return "your_password_here".to_string();
    }
    if lower_key == "node_env" || lower_key == "app_env" || lower_key == "environment" {
        return "development".to_string();
    }
    if lower_key.contains("email") {
        return "user@example.com".to_string();
    }
    if lower_key.contains("url") || lower_key.contains("uri") || lower_key.contains("endpoint") {
        return "https://api.example.com".to_string();
    }
    if lower_key.contains("timeout") {
        return "30000".to_string();
    }
    if lower_key.contains("path") || lower_key.contains("dir") {
        return "/path/to/directory".to_string();
    }

    // Type-based fallback
    match spec.var_type {
        VarType::String => String::new(),
        VarType::Int => "0".to_string(),
        VarType::Float => "0.0".to_string(),
        VarType::Bool => "false".to_string(),
        VarType::Url => "https://example.com".to_string(),
        VarType::Enum => {
            // Use first enum value as placeholder
            spec.values
                .as_ref()
                .and_then(|v| v.first().cloned())
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ValidationRule;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_generate_var_comments_with_description() {
        let spec = VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("A test variable".to_string()),
            values: None,
            default: None,
            validate: None,
        };
        let comments = generate_var_comments("TEST", &spec);
        assert!(comments.contains("# A test variable"));
        assert!(comments.contains("# Type: string (required)"));
    }

    #[test]
    fn test_generate_var_comments_optional() {
        let spec = VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        };
        let comments = generate_var_comments("TEST", &spec);
        assert!(comments.contains("# Type: int (optional)"));
    }

    #[test]
    fn test_generate_var_comments_enum_values() {
        let spec = VarSpec {
            var_type: VarType::Enum,
            required: true,
            description: None,
            values: Some(vec![
                "development".to_string(),
                "staging".to_string(),
                "production".to_string(),
            ]),
            default: None,
            validate: None,
        };
        let comments = generate_var_comments("NODE_ENV", &spec);
        assert!(comments.contains("# Values: development, staging, production"));
    }

    #[test]
    fn test_generate_var_comments_default_value() {
        let spec = VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: Some(serde_json::json!(3000)),
            validate: None,
        };
        let comments = generate_var_comments("PORT", &spec);
        assert!(comments.contains("# Default: 3000"));
    }

    #[test]
    fn test_generate_var_comments_validation_rules() {
        let spec = VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min: Some(1024),
                max: Some(65535),
                ..Default::default()
            }),
        };
        let comments = generate_var_comments("PORT", &spec);
        assert!(comments.contains("# Validation: min=1024, max=65535"));
    }

    #[test]
    fn test_generate_var_comments_string_validation() {
        let spec = VarSpec {
            var_type: VarType::String,
            required: true,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_length: Some(32),
                pattern: Some("^sk_".to_string()),
                ..Default::default()
            }),
        };
        let comments = generate_var_comments("API_KEY", &spec);
        assert!(comments.contains("min_length=32"));
        assert!(comments.contains("pattern=^sk_"));
    }

    #[test]
    fn test_generate_var_line_without_defaults() {
        let spec = VarSpec {
            var_type: VarType::String,
            required: true,
            description: None,
            values: None,
            default: Some(serde_json::json!("test")),
            validate: None,
        };
        let line = generate_var_line("FOO", &spec, false);
        assert_eq!(line, "FOO=");
    }

    #[test]
    fn test_generate_var_line_with_defaults() {
        let spec = VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: Some(serde_json::json!(3000)),
            validate: None,
        };
        let line = generate_var_line("PORT", &spec, true);
        assert_eq!(line, "PORT=3000");
    }

    #[test]
    fn test_generate_var_line_string_default() {
        let spec = VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: Some(serde_json::json!("development")),
            validate: None,
        };
        let line = generate_var_line("NODE_ENV", &spec, true);
        assert_eq!(line, "NODE_ENV=development");
    }

    #[test]
    fn test_generate_var_line_bool_default() {
        let spec = VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: None,
            values: None,
            default: Some(serde_json::json!(true)),
            validate: None,
        };
        let line = generate_var_line("DEBUG", &spec, true);
        assert_eq!(line, "DEBUG=true");
    }

    #[test]
    fn test_format_default_value_string() {
        let value = serde_json::json!("hello");
        assert_eq!(format_default_value(&value), "hello");
    }

    #[test]
    fn test_format_default_value_number() {
        let value = serde_json::json!(42);
        assert_eq!(format_default_value(&value), "42");
    }

    #[test]
    fn test_format_default_value_bool() {
        let value = serde_json::json!(true);
        assert_eq!(format_default_value(&value), "true");
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_format_default_value_float() {
        let value = serde_json::json!(3.14);
        assert_eq!(format_default_value(&value), "3.14");
    }

    #[test]
    fn test_run_with_schema_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{
                "PORT": {{"type": "int", "default": 3000, "description": "HTTP port"}},
                "DEBUG": {{"type": "bool", "default": false}}
            }}"#
        )
        .unwrap();

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test.env.example");

        let result = run(
            file.path().to_str().unwrap(),
            Some(output_path.to_str().unwrap()),
            true,
            false,
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("DEBUG=false"));
        assert!(content.contains("PORT=3000"));
        assert!(content.contains("# HTTP port"));
    }

    #[test]
    fn test_run_without_defaults() {
        // Use a non-standard port value to distinguish schema default from placeholder
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"PORT": {{"type": "int", "default": 8080}}}}"#
        )
        .unwrap();

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test.env.example");

        let result = run(
            file.path().to_str().unwrap(),
            Some(output_path.to_str().unwrap()),
            false,
            false,
        );
        assert!(result.is_ok());

        let content = fs::read_to_string(&output_path).unwrap();
        // Should use smart placeholder (3000 for port), not schema default (8080)
        assert!(content.contains("PORT=3000"));
        assert!(!content.contains("PORT=8080"));
    }

    #[test]
    fn test_output_sorted_alphabetically() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{
                "ZEBRA": {{"type": "string"}},
                "APPLE": {{"type": "string"}},
                "MANGO": {{"type": "string"}}
            }}"#
        )
        .unwrap();

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test.env.example");

        run(
            file.path().to_str().unwrap(),
            Some(output_path.to_str().unwrap()),
            false,
            false,
        )
        .unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        let apple_pos = content.find("APPLE=").unwrap();
        let mango_pos = content.find("MANGO=").unwrap();
        let zebra_pos = content.find("ZEBRA=").unwrap();

        assert!(apple_pos < mango_pos);
        assert!(mango_pos < zebra_pos);
    }

    #[test]
    fn test_all_types_documented() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{
                "STR": {{"type": "string"}},
                "INT": {{"type": "int"}},
                "FLT": {{"type": "float"}},
                "BLN": {{"type": "bool"}},
                "URL": {{"type": "url"}},
                "ENM": {{"type": "enum", "values": ["a", "b"]}}
            }}"#
        )
        .unwrap();

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test.env.example");

        run(
            file.path().to_str().unwrap(),
            Some(output_path.to_str().unwrap()),
            false,
            false,
        )
        .unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Type: string"));
        assert!(content.contains("# Type: int"));
        assert!(content.contains("# Type: float"));
        assert!(content.contains("# Type: bool"));
        assert!(content.contains("# Type: url"));
        assert!(content.contains("# Type: enum"));
    }

    #[test]
    fn test_invalid_schema_path() {
        let result = run("/nonexistent/path/schema.json", None, false, false);
        assert!(result.is_err());
    }
}
