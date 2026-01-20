use std::path::Path;

use crate::envfile;
use crate::presets;
use crate::schema::{self, Schema, VarSpec, VarType};

pub fn run(example_path: &str, schema_path: &str, preset: Option<&str>) -> Result<(), String> {
    // Start with preset schema if provided
    let mut schema_map: Schema = if let Some(preset_name) = preset {
        match presets::get_preset(preset_name) {
            Some(preset_schema) => {
                println!("zenv: using '{}' preset", preset_name);
                preset_schema
            }
            None => {
                return Err(format!(
                    "unknown preset '{}'. Available: {}",
                    preset_name,
                    presets::list_presets().join(", ")
                ));
            }
        }
    } else {
        Schema::new()
    };

    // If example file exists, merge inferred types (inferred takes precedence for existing keys)
    if Path::new(example_path).exists() {
        let env = envfile::parse_env_file(example_path).map_err(|e| e.to_string())?;

        for (k, v) in env {
            // Only add if not already in preset
            schema_map.entry(k.clone()).or_insert_with(|| {
                let inferred = infer_type(&v);
                let description = infer_description(&k, &inferred);

                VarSpec {
                    var_type: inferred,
                    required: true,
                    description: Some(description),
                    values: None,
                    default: None,
                    validate: None,
                    secret: None,
                    ..Default::default()
                }
            });
        }
    } else if preset.is_none() {
        // No preset and no example file
        return Err(format!("example file not found: {example_path}"));
    }

    if Path::new(schema_path).exists() {
        eprintln!("warning: overwriting existing {schema_path}");
    }

    schema::save_schema(schema_path, &schema_map).map_err(|e| e.to_string())?;
    println!("zenv: wrote schema to {schema_path} ({} variables)", schema_map.len());
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

/// Infer a description from the variable name and type
fn infer_description(key: &str, var_type: &VarType) -> String {
    let lower_key = key.to_lowercase();

    // Database-related
    if lower_key.contains("database") || lower_key.contains("db_") || lower_key == "db" {
        return "Database connection string".to_string();
    }
    if lower_key.contains("redis") {
        return "Redis connection URL".to_string();
    }
    if lower_key.contains("mongo") {
        return "MongoDB connection URL".to_string();
    }

    // URL patterns
    if lower_key.ends_with("_url") || lower_key.ends_with("_uri") {
        let service = extract_service_name(key);
        return format!("{} endpoint URL", service);
    }
    if lower_key.ends_with("_endpoint") {
        let service = extract_service_name(key);
        return format!("{} API endpoint", service);
    }

    // Authentication
    if lower_key.contains("api_key") || lower_key.contains("apikey") {
        let service = extract_service_name(key);
        return format!("{} API key", service);
    }
    if lower_key.contains("secret_key") || lower_key.contains("secretkey") {
        let service = extract_service_name(key);
        return format!("{} secret key", service);
    }
    if lower_key.contains("access_key") {
        let service = extract_service_name(key);
        return format!("{} access key", service);
    }
    if lower_key.contains("_token") || lower_key.ends_with("token") {
        let service = extract_service_name(key);
        return format!("{} authentication token", service);
    }
    if lower_key.contains("password") || lower_key.contains("passwd") {
        return "Password credential".to_string();
    }
    if lower_key.contains("_secret") || lower_key.ends_with("secret") {
        let service = extract_service_name(key);
        return format!("{} secret", service);
    }

    // Environment
    if lower_key == "node_env" || lower_key == "app_env" || lower_key == "environment" || lower_key == "env" {
        return "Application environment (e.g., development, staging, production)".to_string();
    }

    // Common patterns
    if lower_key.contains("port") {
        return "Port number for network service".to_string();
    }
    if lower_key.contains("host") && !lower_key.contains("_url") {
        return "Hostname or IP address".to_string();
    }
    if lower_key.contains("timeout") {
        return "Timeout duration in milliseconds".to_string();
    }
    if lower_key.contains("max_") || lower_key.contains("_max") {
        return format!("Maximum {} limit", key.replace('_', " ").to_lowercase());
    }
    if lower_key.contains("min_") || lower_key.contains("_min") {
        return format!("Minimum {} threshold", key.replace('_', " ").to_lowercase());
    }
    if lower_key.contains("enable") || lower_key.contains("disable") {
        return format!("Toggle for {}", key.replace('_', " ").to_lowercase());
    }
    if lower_key.starts_with("debug") {
        return "Enable debug mode".to_string();
    }
    if lower_key.contains("log_level") || lower_key == "loglevel" {
        return "Logging verbosity level".to_string();
    }
    if lower_key.contains("path") || lower_key.contains("dir") || lower_key.contains("directory") {
        return "File system path".to_string();
    }

    // Type-based fallback
    match var_type {
        VarType::Url => format!("{} URL", humanize_key(key)),
        VarType::Int => format!("{} (integer)", humanize_key(key)),
        VarType::Float => format!("{} (decimal)", humanize_key(key)),
        VarType::Bool => format!("{} flag", humanize_key(key)),
        _ => format!("{} configuration", humanize_key(key)),
    }
}

/// Extract service name from a key like STRIPE_API_KEY -> Stripe
fn extract_service_name(key: &str) -> String {
    let parts: Vec<&str> = key.split('_').collect();
    if parts.is_empty() {
        return key.to_string();
    }

    // Common suffixes to strip
    let suffixes = ["API", "KEY", "SECRET", "TOKEN", "URL", "URI", "ENDPOINT", "HOST", "PORT"];

    // Find the first part that's not a common suffix
    for part in &parts {
        let upper = part.to_uppercase();
        if !suffixes.contains(&upper.as_str()) && !part.is_empty() {
            // Capitalize first letter, lowercase rest
            let mut chars = part.chars();
            return match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            };
        }
    }

    humanize_key(key)
}

/// Convert KEY_NAME to "Key name"
fn humanize_key(key: &str) -> String {
    let result = key
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ");

    if result.is_empty() {
        key.to_string()
    } else {
        result
    }
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

    // Description inference tests
    #[test]
    fn test_infer_description_database_url() {
        let desc = infer_description("DATABASE_URL", &VarType::Url);
        assert_eq!(desc, "Database connection string");
    }

    #[test]
    fn test_infer_description_api_key() {
        let desc = infer_description("STRIPE_API_KEY", &VarType::String);
        assert_eq!(desc, "Stripe API key");
    }

    #[test]
    fn test_infer_description_port() {
        let desc = infer_description("PORT", &VarType::Int);
        assert_eq!(desc, "Port number for network service");
    }

    #[test]
    fn test_infer_description_node_env() {
        let desc = infer_description("NODE_ENV", &VarType::String);
        assert_eq!(desc, "Application environment (e.g., development, staging, production)");
    }

    #[test]
    fn test_infer_description_generic_url() {
        let desc = infer_description("WEBHOOK_URL", &VarType::Url);
        assert_eq!(desc, "Webhook endpoint URL");
    }

    #[test]
    fn test_humanize_key() {
        assert_eq!(humanize_key("DATABASE_URL"), "Database Url");
        assert_eq!(humanize_key("API_KEY"), "Api Key");
        assert_eq!(humanize_key("PORT"), "Port");
    }

    #[test]
    fn test_extract_service_name() {
        assert_eq!(extract_service_name("STRIPE_API_KEY"), "Stripe");
        assert_eq!(extract_service_name("AWS_SECRET_KEY"), "Aws");
        assert_eq!(extract_service_name("GITHUB_TOKEN"), "Github");
    }
}
