use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::envfile;
use crate::remote;
use crate::schema::{self, LoadOptions, SchemaFormat};

/// Health check result for a single item
struct HealthItem {
    name: String,
    status: HealthStatus,
    message: String,
    suggestion: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
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

pub fn run() -> Result<(), String> {
    println!("zenv doctor - Health Check\n");

    let mut items = Vec::new();

    // Load config (if exists)
    let config = Config::load().unwrap_or_default();

    // 1. Check schema file
    items.push(check_schema(&config));

    // 2. Check .env file
    items.push(check_env_file(&config));

    // 3. Check config file
    items.push(check_config_file());

    // 4. Check remote schema cache
    items.push(check_cache());

    // 5. Validation test (if both schema and env exist)
    if let Some(validation_result) = check_validation(&config) {
        items.push(validation_result);
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
        Err("doctor found issues".into())
    } else if has_warnings {
        println!("Health check completed with warnings.");
        Ok(())
    } else {
        println!("Health check passed. All systems operational.");
        Ok(())
    }
}

fn check_schema(config: &Config) -> HealthItem {
    let schema_path = config.schema_or("env.schema.json");

    // Check for common schema file locations
    let possible_paths = [
        schema_path.as_str(),
        "env.schema.json",
        "env.schema.yaml",
        "env.schema.yml",
        ".env.schema.json",
    ];

    for path in &possible_paths {
        if Path::new(path).exists() {
            // Try to parse it
            let options = LoadOptions {
                no_cache: true,
                verify_hash: None,
                ca_cert: None,
                rate_limit_seconds: None,
            };
            match schema::load_schema_with_options(path, &options) {
                Ok(schema) => {
                    let format = SchemaFormat::from_path(path);
                    return HealthItem {
                        name: "Schema".to_string(),
                        status: HealthStatus::Ok,
                        message: format!("Found {} ({} format, {} variables)", path, format.name(), schema.len()),
                        suggestion: None,
                    };
                }
                Err(e) => {
                    return HealthItem {
                        name: "Schema".to_string(),
                        status: HealthStatus::Error,
                        message: format!("Found {} but failed to parse", path),
                        suggestion: Some(format!("Fix schema error: {}", e)),
                    };
                }
            }
        }
    }

    HealthItem {
        name: "Schema".to_string(),
        status: HealthStatus::Warning,
        message: "No schema file found".to_string(),
        suggestion: Some("Run 'zenv init' to create one from .env.example".to_string()),
    }
}

fn check_env_file(config: &Config) -> HealthItem {
    let env_path = config.env_or(".env");

    // Check for common env file locations
    let possible_paths = [
        env_path.as_str(),
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ];

    for path in &possible_paths {
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
        status: HealthStatus::Warning,
        message: "No .env file found".to_string(),
        suggestion: Some("Create .env or use 'zenv example' to generate a template".to_string()),
    }
}

fn check_config_file() -> HealthItem {
    let config_paths = [".zenvrc", ".zenv.json"];

    for path in &config_paths {
        if Path::new(path).exists() {
            // Try to parse it
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    return HealthItem {
                        name: "Config".to_string(),
                        status: HealthStatus::Error,
                        message: format!("Found {} but couldn't read it", path),
                        suggestion: Some(format!("Fix read error: {}", e)),
                    };
                }
            };

            match serde_json::from_str::<Config>(&content) {
                Ok(_) => {
                    return HealthItem {
                        name: "Config".to_string(),
                        status: HealthStatus::Ok,
                        message: format!("Found {} (valid)", path),
                        suggestion: None,
                    };
                }
                Err(e) => {
                    return HealthItem {
                        name: "Config".to_string(),
                        status: HealthStatus::Error,
                        message: format!("Found {} but it's invalid JSON", path),
                        suggestion: Some(format!("Fix JSON error: {}", e)),
                    };
                }
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

    if !cache_dir.exists() {
        return HealthItem {
            name: "Cache".to_string(),
            status: HealthStatus::Ok,
            message: "No cache directory (remote schemas not used)".to_string(),
            suggestion: None,
        };
    }

    // Count cached schemas
    match fs::read_dir(&cache_dir) {
        Ok(entries) => {
            let count = entries.filter_map(|e| e.ok()).count();
            if count > 0 {
                HealthItem {
                    name: "Cache".to_string(),
                    status: HealthStatus::Ok,
                    message: format!("{} cached remote schema(s) at {}", count, cache_dir.display()),
                    suggestion: None,
                }
            } else {
                HealthItem {
                    name: "Cache".to_string(),
                    status: HealthStatus::Ok,
                    message: format!("Cache directory exists but is empty: {}", cache_dir.display()),
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

fn check_validation(config: &Config) -> Option<HealthItem> {
    let schema_path = config.schema_or("env.schema.json");
    let env_path = config.env_or(".env");

    // Both must exist for validation
    let schema_exists = Path::new(&schema_path).exists();
    let env_exists = find_env_file(&env_path).is_some();

    if !schema_exists || !env_exists {
        return None;
    }

    let resolved_env = find_env_file(&env_path)?;

    // Load schema
    let options = LoadOptions {
        no_cache: true,
        verify_hash: None,
        ca_cert: None,
        rate_limit_seconds: None,
    };
    let schema = match schema::load_schema_with_options(&schema_path, &options) {
        Ok(s) => s,
        Err(_) => return None,
    };

    // Load env
    let env_map = match envfile::parse_env_file(&resolved_env) {
        Ok(m) => m,
        Err(_) => return None,
    };

    // Interpolate
    let env_map = match envfile::interpolate_env(env_map) {
        Ok(m) => m,
        Err(_) => return None,
    };

    // Validate
    let errors = crate::commands::check::validate(&schema, &env_map);

    if errors.is_empty() {
        Some(HealthItem {
            name: "Validation".to_string(),
            status: HealthStatus::Ok,
            message: "Schema validation passed".to_string(),
            suggestion: None,
        })
    } else {
        let error_count = errors.len();
        Some(HealthItem {
            name: "Validation".to_string(),
            status: HealthStatus::Error,
            message: format!("{} validation error(s) found", error_count),
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
    fn test_health_status_symbol() {
        assert_eq!(HealthStatus::Ok.symbol(), "[OK]");
        assert_eq!(HealthStatus::Warning.symbol(), "[WARN]");
        assert_eq!(HealthStatus::Error.symbol(), "[ERROR]");
    }
}
