use std::path::Path;

use crate::envfile;
use crate::schema::{self, Schema, VarSpec, VarType};

pub fn run(example_path: &str, schema_path: &str) -> Result<(), String> {
    if !Path::new(example_path).exists() {
        return Err(format!("example file not found: {example_path}"));
    }

    if Path::new(schema_path).exists() {
        eprintln!("warning: overwriting existing {schema_path}");
    }

    let env = envfile::parse_env_file(example_path).map_err(|e| e.to_string())?;

    let mut schema_map: Schema = Schema::new();

    for (k, v) in env {
        let inferred = infer_type(&v);

        schema_map.insert(k, VarSpec {
            var_type: inferred,
            required: true,
            description: Some("TODO: describe this variable".into()),
            values: None,
            default: None,
        });
    }

    schema::save_schema(schema_path, &schema_map).map_err(|e| e.to_string())?;
    println!("zenv: wrote schema to {schema_path}");
    Ok(())
}

fn infer_type(v: &str) -> VarType {
    let lv = v.trim().to_lowercase();

    if lv == "true" || lv == "false" || lv == "1" || lv == "0" || lv == "yes" || lv == "no" {
        return VarType::Bool;
    }
    if v.parse::<i64>().is_ok() {
        return VarType::Int;
    }
    if v.parse::<f64>().is_ok() {
        return VarType::Float;
    }
    if url::Url::parse(v).is_ok() {
        return VarType::Url;
    }
    VarType::String
}

#[cfg(test)]
mod tests {
    use super::*;

    // Bool inference tests
    #[test]
    fn test_infer_bool_true() {
        assert!(matches!(infer_type("true"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_false() {
        assert!(matches!(infer_type("false"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_one() {
        assert!(matches!(infer_type("1"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_zero() {
        assert!(matches!(infer_type("0"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_yes() {
        assert!(matches!(infer_type("yes"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_no() {
        assert!(matches!(infer_type("no"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_case_insensitive() {
        assert!(matches!(infer_type("TRUE"), VarType::Bool));
        assert!(matches!(infer_type("False"), VarType::Bool));
        assert!(matches!(infer_type("YES"), VarType::Bool));
    }

    #[test]
    fn test_infer_bool_with_whitespace() {
        assert!(matches!(infer_type("  true  "), VarType::Bool));
    }

    // Int inference tests
    #[test]
    fn test_infer_int_positive() {
        assert!(matches!(infer_type("42"), VarType::Int));
    }

    #[test]
    fn test_infer_int_negative() {
        assert!(matches!(infer_type("-42"), VarType::Int));
    }

    #[test]
    fn test_infer_int_large() {
        assert!(matches!(infer_type("9999999999"), VarType::Int));
    }

    #[test]
    fn test_infer_int_port() {
        assert!(matches!(infer_type("3000"), VarType::Int));
    }

    // Float inference tests
    #[test]
    fn test_infer_float_decimal() {
        assert!(matches!(infer_type("3.14"), VarType::Float));
    }

    #[test]
    fn test_infer_float_negative() {
        assert!(matches!(infer_type("-0.5"), VarType::Float));
    }

    #[test]
    fn test_infer_float_scientific() {
        assert!(matches!(infer_type("1.5e10"), VarType::Float));
    }

    // URL inference tests
    #[test]
    fn test_infer_url_https() {
        assert!(matches!(infer_type("https://example.com"), VarType::Url));
    }

    #[test]
    fn test_infer_url_http() {
        assert!(matches!(infer_type("http://localhost:3000"), VarType::Url));
    }

    #[test]
    fn test_infer_url_postgres() {
        assert!(matches!(infer_type("postgres://user:pass@host/db"), VarType::Url));
    }

    #[test]
    fn test_infer_url_redis() {
        assert!(matches!(infer_type("redis://localhost:6379"), VarType::Url));
    }

    // String inference tests (default)
    #[test]
    fn test_infer_string_plain_text() {
        assert!(matches!(infer_type("hello world"), VarType::String));
    }

    #[test]
    fn test_infer_string_env_name() {
        assert!(matches!(infer_type("development"), VarType::String));
    }

    #[test]
    fn test_infer_string_api_key() {
        assert!(matches!(infer_type("sk_test_abc123xyz"), VarType::String));
    }

    #[test]
    fn test_infer_string_path() {
        assert!(matches!(infer_type("/var/log/app.log"), VarType::String));
    }

    #[test]
    fn test_infer_string_empty() {
        assert!(matches!(infer_type(""), VarType::String));
    }

    // Edge cases - priority order verification
    #[test]
    fn test_bool_takes_priority_over_int() {
        // "1" and "0" should be Bool, not Int
        assert!(matches!(infer_type("1"), VarType::Bool));
        assert!(matches!(infer_type("0"), VarType::Bool));
    }

    #[test]
    fn test_int_takes_priority_over_float() {
        // "42" parses as both i64 and f64, but should be Int
        assert!(matches!(infer_type("42"), VarType::Int));
    }
}
