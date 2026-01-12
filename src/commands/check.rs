use std::collections::HashMap;
use std::path::Path;

use url::Url;

use crate::envfile;
use crate::schema::{self, Schema, VarType};

pub fn run(env_path: &str, schema_path: &str, allow_missing_env: bool) -> Result<(), String> {
    let schema = schema::load_schema(schema_path).map_err(|e| e.to_string())?;

    let env_map: HashMap<String, String> = if Path::new(env_path).exists() {
        envfile::parse_env_file(env_path).map_err(|e| e.to_string())?
    } else if allow_missing_env {
        HashMap::new()
    } else {
        return Err(format!("env file not found: {env_path}"));
    };

    let errors = validate(&schema, &env_map);

    if !errors.is_empty() {
        eprintln!("zenv check failed:\n");
        for e in &errors {
            eprintln!("- {e}");
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
            VarType::String => { /* always ok */ }

            VarType::Int => {
                if value.parse::<i64>().is_err() {
                    errors.push(format!("{key}: expected int, got '{value}'"));
                }
            }

            VarType::Float => {
                if value.parse::<f64>().is_err() {
                    errors.push(format!("{key}: expected float, got '{value}'"));
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
    use crate::schema::{VarSpec, VarType};

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
        }
    }

    fn int_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Int,
            required,
            description: None,
            values: None,
            default: None,
        }
    }

    fn float_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Float,
            required: false,
            description: None,
            values: None,
            default: None,
        }
    }

    fn bool_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: None,
            values: None,
            default: None,
        }
    }

    fn url_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Url,
            required: false,
            description: None,
            values: None,
            default: None,
        }
    }

    fn enum_spec(values: Vec<&str>) -> VarSpec {
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: None,
            values: Some(values.into_iter().map(String::from).collect()),
            default: None,
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
}
