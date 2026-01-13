use std::collections::HashMap;
use std::path::Path;

use url::Url;

use regex::Regex;

use crate::envfile;
use crate::schema::{self, Schema, VarType};

/// Fallback paths to check when primary env file doesn't exist
const ENV_FALLBACKS: &[&str] = &[
    ".env.local",
    ".env.development",
    ".env.development.local",
];

/// Try to find an env file, checking fallbacks if primary doesn't exist
fn resolve_env_file(primary: &str) -> Option<String> {
    if Path::new(primary).exists() {
        return Some(primary.to_string());
    }

    // Only try fallbacks if user specified the default ".env"
    if primary == ".env" {
        for fallback in ENV_FALLBACKS {
            if Path::new(fallback).exists() {
                return Some((*fallback).to_string());
            }
        }
    }

    None
}

/// Build a helpful error message listing checked paths
fn missing_env_error(primary: &str) -> String {
    let mut msg = format!("Error: env file not found\n\nChecked:\n  - {primary} (not found)");

    if primary == ".env" {
        for fallback in ENV_FALLBACKS {
            let status = if Path::new(fallback).exists() {
                "exists"
            } else {
                "not found"
            };
            msg.push_str(&format!("\n  - {fallback} ({status})"));
        }
    }

    msg.push_str("\n\nTip: Use --env to specify a path, e.g.: zenv check --env .env.local");
    msg
}

pub fn run(env_path: &str, schema_path: &str, allow_missing_env: bool) -> Result<(), String> {
    let schema = schema::load_schema(schema_path).map_err(|e| e.to_string())?;

    let env_map: HashMap<String, String> = match resolve_env_file(env_path) {
        Some(resolved) => {
            if resolved != env_path {
                eprintln!("Note: Using {} (fallback)\n", resolved);
            }
            envfile::parse_env_file(&resolved).map_err(|e| e.to_string())?
        }
        None if allow_missing_env => HashMap::new(),
        None => return Err(missing_env_error(env_path)),
    };

    // Interpolate variable references (${VAR} and $VAR)
    let env_map = envfile::interpolate_env(env_map).map_err(|e| e.to_string())?;

    let errors = validate(&schema, &env_map);

    if !errors.is_empty() {
        // Count unknown keys for the tip
        let unknown_count = errors
            .iter()
            .filter(|e| e.contains("not in schema"))
            .count();

        eprintln!("Error: zenv check failed:\n");
        for e in &errors {
            eprintln!("- {e}");
        }

        // Suggest init --merge if there are unknown keys
        if unknown_count > 0 {
            eprintln!(
                "\nTip: {} unknown key(s) found. Consider updating your schema.",
                unknown_count
            );
        }

        return Err("validation failed".into());
    }

    println!("zenv: OK");
    Ok(())
}

/// Validate env_map against schema, returns list of error messages
pub fn validate(schema: &Schema, env_map: &HashMap<String, String>) -> Vec<String> {
    let mut errors: Vec<String> = vec![];

    for (key, spec) in schema.iter() {
        let value_opt = env_map.get(key);

        if value_opt.is_none() {
            if spec.required && spec.default.is_none() {
                errors.push(format!("{key}: missing (required)"));
            }
            continue;
        }

        let value = value_opt.unwrap();

        match spec.var_type {
            VarType::String => {
                // Apply string validation rules
                if let Some(ref rules) = spec.validate {
                    if let Some(min_len) = rules.min_length {
                        if value.len() < min_len {
                            errors.push(format!("{key}: length {} is less than minimum {}", value.len(), min_len));
                        }
                    }
                    if let Some(max_len) = rules.max_length {
                        if value.len() > max_len {
                            errors.push(format!("{key}: length {} exceeds maximum {}", value.len(), max_len));
                        }
                    }
                    if let Some(ref pattern) = rules.pattern {
                        match Regex::new(pattern) {
                            Ok(re) => {
                                if !re.is_match(value) {
                                    errors.push(format!("{key}: value '{value}' does not match pattern '{pattern}'"));
                                }
                            }
                            Err(e) => {
                                errors.push(format!("{key}: invalid regex pattern '{pattern}': {e}"));
                            }
                        }
                    }
                }
            }

            VarType::Int => {
                match value.parse::<i64>() {
                    Err(_) => {
                        errors.push(format!("{key}: expected int, got '{value}'"));
                    }
                    Ok(n) => {
                        // Apply int validation rules
                        if let Some(ref rules) = spec.validate {
                            if let Some(min) = rules.min {
                                if n < min {
                                    errors.push(format!("{key}: value {n} is less than minimum {min}"));
                                }
                            }
                            if let Some(max) = rules.max {
                                if n > max {
                                    errors.push(format!("{key}: value {n} exceeds maximum {max}"));
                                }
                            }
                        }
                    }
                }
            }

            VarType::Float => {
                match value.parse::<f64>() {
                    Err(_) => {
                        errors.push(format!("{key}: expected float, got '{value}'"));
                    }
                    Ok(n) => {
                        // Apply float validation rules
                        if let Some(ref rules) = spec.validate {
                            if let Some(min_val) = rules.min_value {
                                if n < min_val {
                                    errors.push(format!("{key}: value {n} is less than minimum {min_val}"));
                                }
                            }
                            if let Some(max_val) = rules.max_value {
                                if n > max_val {
                                    errors.push(format!("{key}: value {n} exceeds maximum {max_val}"));
                                }
                            }
                        }
                    }
                }
            }

            VarType::Bool => {
                let v = value.to_lowercase();
                let ok = matches!(v.as_str(), "true" | "false" | "1" | "0" | "yes" | "no");
                if !ok {
                    errors.push(format!("{key}: expected bool (true/false/1/0/yes/no), got '{value}'"));
                }
            }

            VarType::Url => {
                if Url::parse(value).is_err() {
                    errors.push(format!("{key}: expected url, got '{value}'"));
                }
            }

            VarType::Enum => {
                match spec.values.as_ref() {
                    None => {
                        errors.push(format!("{key}: enum type missing 'values' field in schema"));
                    }
                    Some(allowed) => {
                        if !allowed.iter().any(|v| v == value) {
                            errors.push(format!("{key}: expected one of {:?}, got '{value}'", allowed));
                        }
                    }
                }
            }
        }
    }

    // warn on unknown keys (present in env but not in schema)
    for k in env_map.keys() {
        if !schema.contains_key(k) {
            errors.push(format!("{k}: not in schema (unknown key)"));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ValidationRule, VarSpec, VarType};

    fn make_schema(entries: Vec<(&str, VarSpec)>) -> Schema {
        entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn make_env(entries: Vec<(&str, &str)>) -> HashMap<String, String> {
        entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn string_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::String,
            required,
            description: None,
            values: None,
            default: None,
            validate: None,
        }
    }

    fn int_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Int,
            required,
            description: None,
            values: None,
            default: None,
            validate: None,
        }
    }

    fn float_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        }
    }

    fn bool_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        }
    }

    fn url_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Url,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        }
    }

    fn enum_spec(values: Vec<&str>) -> VarSpec {
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: None,
            values: Some(values.into_iter().map(String::from).collect()),
            default: None,
            validate: None,
        }
    }

    // String type tests
    #[test]
    fn test_string_type_always_passes() {
        let schema = make_schema(vec![("FOO", string_spec(false))]);
        let env = make_env(vec![("FOO", "anything goes here!")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    // Int type tests
    #[test]
    fn test_int_type_valid() {
        let schema = make_schema(vec![("PORT", int_spec(false))]);
        let env = make_env(vec![("PORT", "3000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_type_negative() {
        let schema = make_schema(vec![("NUM", int_spec(false))]);
        let env = make_env(vec![("NUM", "-42")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_type_invalid() {
        let schema = make_schema(vec![("PORT", int_spec(false))]);
        let env = make_env(vec![("PORT", "not_a_number")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected int"));
    }

    #[test]
    fn test_int_type_float_invalid() {
        let schema = make_schema(vec![("PORT", int_spec(false))]);
        let env = make_env(vec![("PORT", "3.14")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
    }

    // Float type tests
    #[test]
    fn test_float_type_valid() {
        let schema = make_schema(vec![("RATE", float_spec())]);
        let env = make_env(vec![("RATE", "3.14")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_type_int_valid() {
        let schema = make_schema(vec![("RATE", float_spec())]);
        let env = make_env(vec![("RATE", "42")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_type_invalid() {
        let schema = make_schema(vec![("RATE", float_spec())]);
        let env = make_env(vec![("RATE", "not_a_float")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected float"));
    }

    // Bool type tests
    #[test]
    fn test_bool_type_true() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "true")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_false() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "false")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_one() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_zero() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "0")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_yes() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "yes")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_no() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "no")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_case_insensitive() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "TRUE")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_bool_type_invalid() {
        let schema = make_schema(vec![("DEBUG", bool_spec())]);
        let env = make_env(vec![("DEBUG", "maybe")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected bool"));
    }

    // URL type tests
    #[test]
    fn test_url_type_valid_https() {
        let schema = make_schema(vec![("API", url_spec())]);
        let env = make_env(vec![("API", "https://example.com/api")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_url_type_valid_postgres() {
        let schema = make_schema(vec![("DB", url_spec())]);
        let env = make_env(vec![("DB", "postgres://user:pass@localhost/db")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_url_type_invalid() {
        let schema = make_schema(vec![("API", url_spec())]);
        let env = make_env(vec![("API", "not a url")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected url"));
    }

    // Enum type tests
    #[test]
    fn test_enum_type_valid() {
        let schema = make_schema(vec![("ENV", enum_spec(vec!["dev", "staging", "prod"]))]);
        let env = make_env(vec![("ENV", "staging")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_enum_type_invalid() {
        let schema = make_schema(vec![("ENV", enum_spec(vec!["dev", "staging", "prod"]))]);
        let env = make_env(vec![("ENV", "test")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected one of"));
    }

    #[test]
    fn test_enum_type_missing_values() {
        let schema = make_schema(vec![("ENV", VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        })]);
        let env = make_env(vec![("ENV", "dev")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing 'values' field"));
    }

    // Required field tests
    #[test]
    fn test_required_missing() {
        let schema = make_schema(vec![("API_KEY", string_spec(true))]);
        let env = make_env(vec![]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing (required)"));
    }

    #[test]
    fn test_required_present() {
        let schema = make_schema(vec![("API_KEY", string_spec(true))]);
        let env = make_env(vec![("API_KEY", "secret123")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_optional_missing_ok() {
        let schema = make_schema(vec![("DEBUG", string_spec(false))]);
        let env = make_env(vec![]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_required_with_default_ok() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: true,
            description: None,
            values: None,
            default: Some(serde_json::json!(3000)),
            validate: None,
        })]);
        let env = make_env(vec![]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    // Unknown key tests
    #[test]
    fn test_unknown_key_detected() {
        let schema = make_schema(vec![("FOO", string_spec(false))]);
        let env = make_env(vec![("FOO", "bar"), ("UNKNOWN", "value")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("not in schema"));
    }

    #[test]
    fn test_multiple_errors_accumulated() {
        let schema = make_schema(vec![
            ("REQUIRED", string_spec(true)),
            ("PORT", int_spec(false)),
        ]);
        let env = make_env(vec![("PORT", "not_int"), ("EXTRA", "val")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 3); // missing required, invalid int, unknown key
    }

    #[test]
    fn test_empty_schema_empty_env() {
        let schema = make_schema(vec![]);
        let env = make_env(vec![]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    // Validation rule tests - Int min/max

    #[test]
    fn test_int_min_valid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min: Some(1024),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("PORT", "3000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_min_invalid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min: Some(1024),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("PORT", "80")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("less than minimum"));
    }

    #[test]
    fn test_int_max_valid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max: Some(65535),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("PORT", "8080")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_max_invalid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max: Some(65535),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("PORT", "70000")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("exceeds maximum"));
    }

    #[test]
    fn test_int_min_max_range_valid() {
        let schema = make_schema(vec![("PORT", VarSpec {
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
        })]);
        let env = make_env(vec![("PORT", "8080")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    // Validation rule tests - Float min_value/max_value

    #[test]
    fn test_float_min_value_valid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_value: Some(0.0),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("RATE", "0.5")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_min_value_invalid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_value: Some(0.0),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("RATE", "-0.5")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("less than minimum"));
    }

    #[test]
    fn test_float_max_value_valid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max_value: Some(1.0),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("RATE", "0.75")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_max_value_invalid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max_value: Some(1.0),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("RATE", "1.5")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("exceeds maximum"));
    }

    // Validation rule tests - String min_length/max_length

    #[test]
    fn test_string_min_length_valid() {
        let schema = make_schema(vec![("API_KEY", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_length: Some(8),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("API_KEY", "abcdefghij")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_min_length_invalid() {
        let schema = make_schema(vec![("API_KEY", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_length: Some(8),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("API_KEY", "short")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("less than minimum"));
    }

    #[test]
    fn test_string_max_length_valid() {
        let schema = make_schema(vec![("CODE", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max_length: Some(10),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("CODE", "ABC123")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_max_length_invalid() {
        let schema = make_schema(vec![("CODE", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                max_length: Some(5),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("CODE", "TOOLONG123")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("exceeds maximum"));
    }

    // Validation rule tests - String pattern (regex)

    #[test]
    fn test_string_pattern_valid() {
        let schema = make_schema(vec![("EMAIL", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                pattern: Some(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("EMAIL", "user@example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_pattern_invalid() {
        let schema = make_schema(vec![("EMAIL", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                pattern: Some(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("EMAIL", "not-an-email")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("does not match pattern"));
    }

    #[test]
    fn test_string_pattern_simple_valid() {
        let schema = make_schema(vec![("VERSION", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                pattern: Some(r"^v\d+\.\d+\.\d+$".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("VERSION", "v1.2.3")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_pattern_invalid_regex() {
        let schema = make_schema(vec![("FOO", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                pattern: Some(r"[invalid".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("FOO", "bar")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("invalid regex"));
    }

    // Combined validation rules

    #[test]
    fn test_string_length_and_pattern_valid() {
        let schema = make_schema(vec![("UUID", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_length: Some(36),
                max_length: Some(36),
                pattern: Some(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("UUID", "550e8400-e29b-41d4-a716-446655440000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_multiple_validation_failures() {
        let schema = make_schema(vec![("CODE", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: Some(ValidationRule {
                min_length: Some(10),
                pattern: Some(r"^[A-Z]+$".to_string()),
                ..Default::default()
            }),
        })]);
        let env = make_env(vec![("CODE", "abc")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 2); // too short AND wrong pattern
    }
}
