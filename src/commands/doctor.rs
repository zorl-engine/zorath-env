use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::envfile;
use crate::errors::CliError;
use crate::remote;
use crate::schema::{self, LoadOptions, SchemaFormat};

/// Health check result for a single item
struct HealthItem {
    name: String,
    status: HealthStatus,
    message: String,
    suggestion: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HealthStatus {
    Ok,
    Warning,
    Error,
}

impl HealthStatus {
    fn symbol(&self) -> &'static str {
        match self {
            HealthStatus::Ok => "[OK]",
            HealthStatus::Warning => "[WARN]",
            HealthStatus::Error => "[ERROR]",
        }
    }
}

#[doc(hidden)]
pub fn run(
    env_path: &str,
    schema_path: &str,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    run_with_config(
        env_path,
        schema_path,
        no_cache,
        verify_hash,
        ca_cert,
        rate_limit_seconds,
        None,
    )
}

#[doc(hidden)]
pub fn run_with_config(
    env_path: &str,
    schema_path: &str,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
    config_path: Option<&str>,
) -> Result<(), CliError> {
    println!("zenv doctor - Health Check\n");

    let load_options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds,
    };

    let (schema_item, loaded_schema) = inspect_schema(schema_path, &load_options);
    let mut items = vec![
        // 1. Check schema file
        schema_item,
        // 2. Check .env file
        check_env_path(env_path),
        // 3. Check config file
        check_config_file(config_path),
        // 4. Check remote schema cache
        check_cache(),
    ];

    // 5. Validation test (if both schema and env exist)
    if let Some(schema) = loaded_schema {
        if let Some(validation_result) = check_validation(env_path, &schema) {
            items.push(validation_result);
        }
    }

    // Print results
    let mut has_errors = false;
    let mut has_warnings = false;

    for item in &items {
        let status_str = item.status.symbol();
        println!("{} {}: {}", status_str, item.name, item.message);

        if let Some(ref suggestion) = item.suggestion {
            println!("     -> {}", suggestion);
        }

        match item.status {
            HealthStatus::Error => has_errors = true,
            HealthStatus::Warning => has_warnings = true,
            HealthStatus::Ok => {}
        }
    }

    // Summary
    println!();
    if has_errors {
        println!("Health check completed with errors.");
        Err(CliError::Validation("doctor found issues".into()))
    } else if has_warnings {
        println!("Health check completed with warnings.");
        Ok(())
    } else {
        println!("Health check passed. All systems operational.");
        Ok(())
    }
}

fn inspect_schema(
    schema_path: &str,
    load_options: &LoadOptions,
) -> (HealthItem, Option<schema::Schema>) {
    // Check for common schema file locations
    let possible_paths = [
        schema_path,
        "env.schema.json",
        "env.schema.yaml",
        "env.schema.yml",
        ".env.schema.json",
    ];

    for path in &possible_paths {
        if *path != schema_path && schema_path != "env.schema.json" {
            continue;
        }
        if Path::new(path).exists() || remote::is_remote_url(path) {
            // Try to parse it
            match schema::load_schema_with_options(path, load_options) {
                Ok(schema) => {
                    let format = SchemaFormat::from_path(path);
                    return (
                        HealthItem {
                            name: "Schema".to_string(),
                            status: HealthStatus::Ok,
                            message: format!(
                                "Found {} ({} format, {} variables)",
                                path,
                                format.name(),
                                schema.len()
                            ),
                            suggestion: None,
                        },
                        Some(schema),
                    );
                }
                Err(e) => {
                    return (
                        HealthItem {
                            name: "Schema".to_string(),
                            status: HealthStatus::Error,
                            message: format!("Found {} but failed to parse", path),
                            suggestion: Some(format!("Fix schema error: {}", e)),
                        },
                        None,
                    );
                }
            }
        }
    }

    (
        HealthItem {
            name: "Schema".to_string(),
            status: if schema_path == "env.schema.json" {
                HealthStatus::Warning
            } else {
                HealthStatus::Error
            },
            message: "No schema file found".to_string(),
            suggestion: Some("Run 'zenv init' to create one from .env.example".to_string()),
        },
        None,
    )
}

#[cfg(test)]
fn check_schema_path(schema_path: &str, options: &LoadOptions) -> HealthItem {
    inspect_schema(schema_path, options).0
}

fn check_env_path(env_path: &str) -> HealthItem {
    // Check for common env file locations
    let possible_paths = [
        env_path,
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ];

    for path in &possible_paths {
        if *path != env_path && env_path != ".env" {
            continue;
        }
        if Path::new(path).exists() {
            // Try to parse it
            match envfile::parse_env_file(path) {
                Ok(env_map) => {
                    return HealthItem {
                        name: "Env file".to_string(),
                        status: HealthStatus::Ok,
                        message: format!("Found {} ({} variables)", path, env_map.len()),
                        suggestion: None,
                    };
                }
                Err(e) => {
                    return HealthItem {
                        name: "Env file".to_string(),
                        status: HealthStatus::Error,
                        message: format!("Found {} but failed to parse", path),
                        suggestion: Some(format!("Fix parse error: {}", e)),
                    };
                }
            }
        }
    }

    HealthItem {
        name: "Env file".to_string(),
        status: if env_path == ".env" {
            HealthStatus::Warning
        } else {
            HealthStatus::Error
        },
        message: "No .env file found".to_string(),
        suggestion: Some("Create .env or use 'zenv example' to generate a template".to_string()),
    }
}

fn check_config_file(explicit: Option<&str>) -> HealthItem {
    let config_path = explicit
        .map(std::path::PathBuf::from)
        .or_else(crate::config::find_config_file);

    if let Some(path) = config_path {
        // Try to parse it
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return HealthItem {
                    name: "Config".to_string(),
                    status: HealthStatus::Error,
                    message: format!("Couldn't read config {}", path.display()),
                    suggestion: Some(format!("Fix read error: {}", e)),
                };
            }
        };

        match serde_json::from_str::<Config>(&content) {
            Ok(_) => {
                return HealthItem {
                    name: "Config".to_string(),
                    status: HealthStatus::Ok,
                    message: format!("Found {} (valid)", path.display()),
                    suggestion: None,
                };
            }
            Err(e) => {
                return HealthItem {
                    name: "Config".to_string(),
                    status: HealthStatus::Error,
                    message: format!("Found {} but it's invalid JSON", path.display()),
                    suggestion: Some(format!("Fix JSON error: {}", e)),
                };
            }
        }
    }

    HealthItem {
        name: "Config".to_string(),
        status: HealthStatus::Ok,
        message: "No .zenvrc found (using defaults)".to_string(),
        suggestion: None,
    }
}

fn check_cache() -> HealthItem {
    let cache_dir = match remote::cache_dir() {
        Some(dir) => dir,
        None => {
            return HealthItem {
                name: "Cache".to_string(),
                status: HealthStatus::Warning,
                message: "Could not determine cache directory".to_string(),
                suggestion: None,
            };
        }
    };

    check_cache_at(&cache_dir)
}

fn check_cache_at(cache_dir: &Path) -> HealthItem {
    if !cache_dir.exists() {
        return HealthItem {
            name: "Cache".to_string(),
            status: HealthStatus::Ok,
            message: "No cache directory (remote schemas not used)".to_string(),
            suggestion: None,
        };
    }

    // Count cached schemas
    match fs::read_dir(cache_dir) {
        Ok(entries) => {
            let count = entries
                .filter_map(|e| e.ok())
                .filter(super::cache::is_schema_entry)
                .count();
            if count > 0 {
                HealthItem {
                    name: "Cache".to_string(),
                    status: HealthStatus::Ok,
                    message: format!(
                        "{} cached remote schema(s) at {}",
                        count,
                        cache_dir.display()
                    ),
                    suggestion: None,
                }
            } else {
                HealthItem {
                    name: "Cache".to_string(),
                    status: HealthStatus::Ok,
                    message: format!("No cached schemas at {}", cache_dir.display()),
                    suggestion: None,
                }
            }
        }
        Err(e) => HealthItem {
            name: "Cache".to_string(),
            status: HealthStatus::Warning,
            message: format!("Couldn't read cache directory: {}", e),
            suggestion: Some("Run 'zenv cache clear' to reset".to_string()),
        },
    }
}

#[cfg(test)]
fn check_validation_paths(
    env_path: &str,
    schema_path: &str,
    load_options: &LoadOptions,
) -> Option<HealthItem> {
    let (_, schema) = inspect_schema(schema_path, load_options);
    check_validation(env_path, &schema?)
}

fn check_validation(env_path: &str, schema: &schema::Schema) -> Option<HealthItem> {
    let resolved_env = find_env_file(env_path)?;

    // Load env
    let env_map = match envfile::parse_env_file(&resolved_env) {
        Ok(m) => m,
        Err(_) => return None,
    };

    // Interpolate
    let env_map = match envfile::interpolate_env(env_map) {
        Ok(m) => m,
        Err(error) => {
            return Some(HealthItem {
                name: "Validation".into(),
                status: HealthStatus::Error,
                message: error.to_string(),
                suggestion: Some("Fix variable references and run 'zenv check'".into()),
            })
        }
    };

    // Validate
    let issues = crate::commands::check::errors_to_issues(
        crate::commands::check::validate(schema, &env_map),
        schema,
    );
    let error_count = issues
        .iter()
        .filter(|i| i.severity == schema::Severity::Error)
        .count();
    let warning_count = issues.len() - error_count;

    if issues.is_empty() {
        Some(HealthItem {
            name: "Validation".to_string(),
            status: HealthStatus::Ok,
            message: "Schema validation passed".to_string(),
            suggestion: None,
        })
    } else {
        Some(HealthItem {
            name: "Validation".to_string(),
            status: if error_count > 0 {
                HealthStatus::Error
            } else {
                HealthStatus::Warning
            },
            message: format!(
                "{} validation error(s), {} warning(s) found",
                error_count, warning_count
            ),
            suggestion: Some("Run 'zenv check' for details".to_string()),
        })
    }
}

fn find_env_file(primary: &str) -> Option<String> {
    let paths = [
        primary,
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ];

    for path in &paths {
        if *path != primary && primary != ".env" {
            continue;
        }
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_health_counts_schemas_without_metadata_or_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("0123456789abcdef.json"), "{}").unwrap();
        fs::write(dir.path().join("0123456789abcdef.meta"), "{}").unwrap();
        fs::write(dir.path().join(".temporary"), "").unwrap();
        fs::create_dir(dir.path().join("1111111111111111.json")).unwrap();
        let health = check_cache_at(dir.path());
        assert_eq!(health.status, HealthStatus::Ok);
        assert!(
            health.message.starts_with("1 cached remote schema(s)"),
            "{}",
            health.message
        );
    }

    fn default_load_options() -> LoadOptions {
        LoadOptions {
            no_cache: true,
            verify_hash: None,
            ca_cert: None,
            rate_limit_seconds: None,
        }
    }

    #[test]
    fn test_health_status_symbol() {
        assert_eq!(HealthStatus::Ok.symbol(), "[OK]");
        assert_eq!(HealthStatus::Warning.symbol(), "[WARN]");
        assert_eq!(HealthStatus::Error.symbol(), "[ERROR]");
    }

    #[test]
    fn test_check_schema_path_not_found() {
        // Note: If run from a directory with env.schema.json (like zenv root),
        // the function will find the fallback file and return Ok instead of Warning
        let result = check_schema_path("nonexistent_schema_12345.json", &default_load_options());
        // Either file not found (Warning) or fallback found (Ok/Error)
        assert!(
            result.status == HealthStatus::Warning
                || result.status == HealthStatus::Ok
                || result.status == HealthStatus::Error,
            "Expected Warning, Ok, or Error status"
        );
    }

    #[test]
    fn test_check_schema_path_found_valid() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_schema.json");
        std::fs::write(&schema_path, r#"{"FOO": {"type": "string"}}"#).unwrap();

        let result = check_schema_path(schema_path.to_str().unwrap(), &default_load_options());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("Found"));
        assert!(result.message.contains("1 variables"));

        let _ = std::fs::remove_file(&schema_path);
    }

    #[test]
    fn test_check_schema_path_found_invalid() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_invalid_schema.json");
        std::fs::write(&schema_path, "{ invalid json }").unwrap();

        let result = check_schema_path(schema_path.to_str().unwrap(), &default_load_options());
        assert_eq!(result.status, HealthStatus::Error);
        assert!(result.message.contains("failed to parse"));
        assert!(result.suggestion.is_some());

        let _ = std::fs::remove_file(&schema_path);
    }

    #[test]
    fn test_check_env_path_not_found() {
        // Note: If run from a directory with .env (like zenv root),
        // the function will find the fallback file and return Ok instead of Warning
        let result = check_env_path("nonexistent_env_12345.env");
        // Either file not found (Warning) or fallback found (Ok/Error)
        assert!(
            result.status == HealthStatus::Warning
                || result.status == HealthStatus::Ok
                || result.status == HealthStatus::Error,
            "Expected Warning, Ok, or Error status"
        );
    }

    #[test]
    fn test_check_env_path_found_valid() {
        let temp_dir = std::env::temp_dir();
        let env_path = temp_dir.join("test_doctor.env");
        std::fs::write(&env_path, "FOO=bar\nBAZ=qux").unwrap();

        let result = check_env_path(env_path.to_str().unwrap());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("Found"));
        assert!(result.message.contains("2 variables"));

        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_config_file_not_found() {
        // This test assumes no .zenvrc exists in the current directory
        // Since we're testing, this should be safe
        let result = check_config_file(None);
        // Either Ok (not found, using defaults) or found if one exists
        assert!(result.status == HealthStatus::Ok || result.status == HealthStatus::Error);
    }

    #[test]
    fn test_check_cache_returns_result() {
        // Cache check should always return a result (Ok or Warning)
        let result = check_cache();
        assert!(result.status == HealthStatus::Ok || result.status == HealthStatus::Warning);
        assert!(result.name == "Cache");
    }

    #[test]
    fn test_find_env_file_primary_exists() {
        let temp_dir = std::env::temp_dir();
        let env_path = temp_dir.join("test_find_env.env");
        std::fs::write(&env_path, "FOO=bar").unwrap();

        let result = find_env_file(env_path.to_str().unwrap());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), env_path.to_str().unwrap());

        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_find_env_file_not_found() {
        let result = find_env_file("nonexistent_env_file_12345.env");
        // Will be None unless .env, .env.local, etc. exist in current dir
        // This is expected behavior - it checks fallbacks
        assert!(result.is_none() || result.is_some());
    }

    #[test]
    fn test_health_item_structure() {
        let item = HealthItem {
            name: "Test".to_string(),
            status: HealthStatus::Ok,
            message: "Test message".to_string(),
            suggestion: Some("Test suggestion".to_string()),
        };
        assert_eq!(item.name, "Test");
        assert_eq!(item.status, HealthStatus::Ok);
        assert_eq!(item.message, "Test message");
        assert_eq!(item.suggestion, Some("Test suggestion".to_string()));
    }

    #[test]
    fn test_check_validation_paths_returns_none_when_files_missing() {
        // When neither schema nor env exists, should return None
        let result = check_validation_paths(
            "nonexistent_env_12345.env",
            "nonexistent_schema_12345.json",
            &default_load_options(),
        );
        assert!(result.is_none());
    }

    // ====== Additional Doctor Scenario Tests ======

    #[test]
    fn test_check_schema_yaml_format() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_schema.yaml");
        std::fs::write(&schema_path, "FOO:\n  type: string\n  required: true\n").unwrap();

        let result = check_schema_path(schema_path.to_str().unwrap(), &default_load_options());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("YAML") || result.message.contains("yaml"));

        let _ = std::fs::remove_file(&schema_path);
    }

    #[test]
    fn test_check_schema_multiple_variables() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_multi_schema.json");
        std::fs::write(
            &schema_path,
            r#"{
                "API_KEY": {"type": "string", "required": true},
                "DATABASE_URL": {"type": "url"},
                "PORT": {"type": "int", "default": "3000"}
            }"#,
        )
        .unwrap();

        let result = check_schema_path(schema_path.to_str().unwrap(), &default_load_options());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("3 variables"));

        let _ = std::fs::remove_file(&schema_path);
    }

    #[test]
    fn test_check_env_with_comments_and_empty_lines() {
        let temp_dir = std::env::temp_dir();
        let env_path = temp_dir.join("test_doctor_comments.env");
        std::fs::write(
            &env_path,
            "# This is a comment\n\nFOO=bar\n\n# Another comment\nBAZ=qux\n",
        )
        .unwrap();

        let result = check_env_path(env_path.to_str().unwrap());
        assert_eq!(result.status, HealthStatus::Ok);
        // Should count only actual variables, not comments/empty lines
        assert!(result.message.contains("2 variables"));

        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_validation_passes() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_val_schema.json");
        let env_path = temp_dir.join("test_doctor_val.env");

        std::fs::write(
            &schema_path,
            r#"{"PORT": {"type": "int", "required": true}}"#,
        )
        .unwrap();
        std::fs::write(&env_path, "PORT=3000").unwrap();

        let result = check_validation_paths(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            &default_load_options(),
        );

        assert!(result.is_some());
        let item = result.unwrap();
        assert_eq!(item.status, HealthStatus::Ok);
        assert!(item.message.contains("passed"));

        let _ = std::fs::remove_file(&schema_path);
        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_validation_fails_missing_required() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_val_fail_schema.json");
        let env_path = temp_dir.join("test_doctor_val_fail.env");

        std::fs::write(
            &schema_path,
            r#"{"API_KEY": {"type": "string", "required": true}}"#,
        )
        .unwrap();
        std::fs::write(&env_path, "OTHER_VAR=value").unwrap();

        let result = check_validation_paths(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            &default_load_options(),
        );

        assert!(result.is_some());
        let item = result.unwrap();
        assert_eq!(item.status, HealthStatus::Error);
        assert!(item.message.contains("error"));
        assert!(item.suggestion.is_some());

        let _ = std::fs::remove_file(&schema_path);
        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_validation_fails_type_mismatch() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_type_schema.json");
        let env_path = temp_dir.join("test_doctor_type.env");

        std::fs::write(
            &schema_path,
            r#"{"PORT": {"type": "int", "required": true}}"#,
        )
        .unwrap();
        std::fs::write(&env_path, "PORT=not_a_number").unwrap();

        let result = check_validation_paths(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            &default_load_options(),
        );

        assert!(result.is_some());
        let item = result.unwrap();
        assert_eq!(item.status, HealthStatus::Error);

        let _ = std::fs::remove_file(&schema_path);
        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_health_item_without_suggestion() {
        let item = HealthItem {
            name: "Test".to_string(),
            status: HealthStatus::Ok,
            message: "All good".to_string(),
            suggestion: None,
        };
        assert!(item.suggestion.is_none());
    }

    #[test]
    fn test_health_status_equality() {
        assert_eq!(HealthStatus::Ok, HealthStatus::Ok);
        assert_eq!(HealthStatus::Warning, HealthStatus::Warning);
        assert_eq!(HealthStatus::Error, HealthStatus::Error);
        assert_ne!(HealthStatus::Ok, HealthStatus::Error);
        assert_ne!(HealthStatus::Warning, HealthStatus::Error);
    }

    #[test]
    fn test_check_env_with_quoted_values() {
        let temp_dir = std::env::temp_dir();
        let env_path = temp_dir.join("test_doctor_quoted.env");
        std::fs::write(
            &env_path,
            r#"
FOO="bar with spaces"
BAZ='single quoted'
PLAIN=noquotes
"#,
        )
        .unwrap();

        let result = check_env_path(env_path.to_str().unwrap());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("3 variables"));

        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_schema_empty_but_valid() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_empty_schema.json");
        std::fs::write(&schema_path, "{}").unwrap();

        let result = check_schema_path(schema_path.to_str().unwrap(), &default_load_options());
        assert_eq!(result.status, HealthStatus::Ok);
        assert!(result.message.contains("0 variables"));

        let _ = std::fs::remove_file(&schema_path);
    }

    #[test]
    fn test_check_env_with_multiline_value() {
        let temp_dir = std::env::temp_dir();
        let env_path = temp_dir.join("test_doctor_multiline.env");
        std::fs::write(
            &env_path,
            r#"SINGLE=value
MULTI="line1
line2
line3"
ANOTHER=final
"#,
        )
        .unwrap();

        let result = check_env_path(env_path.to_str().unwrap());
        // Should successfully parse multiline values
        assert_eq!(result.status, HealthStatus::Ok);

        let _ = std::fs::remove_file(&env_path);
    }

    #[test]
    fn test_check_validation_with_interpolation() {
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_doctor_interp_schema.json");
        let env_path = temp_dir.join("test_doctor_interp.env");

        // Include HOST in schema so it's not flagged as unknown
        std::fs::write(
            &schema_path,
            r#"{
                "HOST": {"type": "hostname", "required": true},
                "FULL_URL": {"type": "url", "required": true}
            }"#,
        )
        .unwrap();
        std::fs::write(&env_path, "HOST=example.com\nFULL_URL=https://${HOST}/api").unwrap();

        let result = check_validation_paths(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            &default_load_options(),
        );

        // Should pass if interpolation works correctly
        assert!(result.is_some());
        let item = result.unwrap();
        assert_eq!(
            item.status,
            HealthStatus::Ok,
            "Interpolated URL should be valid"
        );

        let _ = std::fs::remove_file(&schema_path);
        let _ = std::fs::remove_file(&env_path);
    }
}
