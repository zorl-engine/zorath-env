use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use url::Url;

use regex::Regex;

use crate::envfile;
use crate::schema::{self, LoadOptions, Schema, Severity, VarSpec, VarType};
use crate::secrets;
use crate::suggestions;

/// JSON output structure for check command
#[derive(Serialize)]
struct CheckResult {
    valid: bool,
    errors: Vec<CheckIssue>,
    warnings: Vec<CheckIssue>,
    duplicate_warnings: Vec<DuplicateWarning>,
    secret_warnings: Vec<SecretWarning>,
    stats: CheckStats,
}

#[derive(Serialize)]
struct CheckIssue {
    key: String,
    message: String,
    issue_type: String,
}

#[derive(Serialize)]
struct DuplicateWarning {
    key: String,
    line: usize,
    previous_line: usize,
}

#[derive(Serialize)]
struct SecretWarning {
    key: String,
    message: String,
    line: usize,
}

#[derive(Serialize)]
struct CheckStats {
    total_variables: usize,
    schema_variables: usize,
    errors_count: usize,
    warnings_count: usize,
    duplicate_warnings_count: usize,
    secret_warnings_count: usize,
}

/// A validation issue with its severity
struct ValidationIssue {
    key: String,
    message: String,
    severity: Severity,
}

/// Convert validation errors to issues with severity from schema
fn errors_to_issues(errors: Vec<String>, schema: &Schema) -> Vec<ValidationIssue> {
    errors.into_iter().map(|e| {
        let parts: Vec<&str> = e.splitn(2, ": ").collect();
        let key = parts.first().unwrap_or(&"").to_string();

        // Get severity from schema, default to Error
        let severity = schema.get(&key)
            .map(|spec| spec.severity)
            .unwrap_or(Severity::Error);

        ValidationIssue {
            key,
            message: e,
            severity,
        }
    }).collect()
}

// Pre-compiled regex patterns for built-in types (cached with OnceLock)
// Made public for reuse in fix.rs
pub fn uuid_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$").unwrap()
    })
}

pub fn email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // RFC 5322 simplified - covers most common email formats
        Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap()
    })
}

pub fn ipv4_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$").unwrap()
    })
}

pub fn semver_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Matches: 1.0.0, 2.1.3-beta.1, 1.0.0+build.123, 1.0.0-alpha+001
        Regex::new(r"^\d+\.\d+\.\d+(-[a-zA-Z0-9]+(\.[a-zA-Z0-9]+)*)?(\+[a-zA-Z0-9]+(\.[a-zA-Z0-9]+)*)?$").unwrap()
    })
}

pub fn ipv6_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Simplified IPv6 validation - 8 groups of 1-4 hex digits separated by colons
        // Also supports :: for consecutive zero groups
        Regex::new(r"^([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}$|^(([0-9a-fA-F]{1,4}:)*)?::(([0-9a-fA-F]{1,4}:)*[0-9a-fA-F]{1,4})?$").unwrap()
    })
}

pub fn date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // ISO 8601 date format: YYYY-MM-DD
        Regex::new(r"^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$").unwrap()
    })
}

pub fn hostname_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // RFC 1123 hostname: labels separated by dots, each 1-63 chars, total max 253
        // Labels: alphanumeric, can contain hyphens (not at start/end)
        Regex::new(r"^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)*[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap()
    })
}

// Cache for user-defined regex patterns (from schema validate.pattern)
use std::cell::RefCell;
thread_local! {
    static PATTERN_CACHE: RefCell<HashMap<String, Result<Regex, String>>> = RefCell::new(HashMap::new());
}

/// Get or compile a regex pattern with caching
fn get_cached_regex(pattern: &str) -> Result<Regex, String> {
    PATTERN_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(result) = cache.get(pattern) {
            return result.clone();
        }
        let result = Regex::new(pattern).map_err(|e| e.to_string());
        cache.insert(pattern.to_string(), result.clone());
        result
    })
}

/// Validate IPv4 octets are in valid range (0-255)
fn is_valid_ipv4(value: &str) -> bool {
    if let Some(caps) = ipv4_regex().captures(value) {
        for i in 1..=4 {
            if let Some(m) = caps.get(i) {
                if let Ok(octet) = m.as_str().parse::<u16>() {
                    if octet > 255 {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        true
    } else {
        false
    }
}

/// Check if a key name suggests it contains sensitive data
pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    let sensitive_patterns = [
        "password", "passwd", "secret", "token", "api_key", "apikey",
        "private_key", "privatekey", "auth", "credential", "jwt",
        "bearer", "access_key", "accesskey", "secret_key", "secretkey",
        "encryption_key", "encryptionkey", "signing_key", "signingkey",
    ];

    for pattern in sensitive_patterns {
        if lower.contains(pattern) {
            return true;
        }
    }

    // Also check for common suffixes
    lower.ends_with("_key") || lower.ends_with("_token") || lower.ends_with("_secret")
}

/// Mask sensitive values for safe display (truncates non-sensitive values)
pub fn mask_value(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        "***MASKED***".to_string()
    } else {
        truncate_value(value)
    }
}

/// State tracked between watch iterations for delta detection
struct WatchState {
    /// Hash of raw .env file content
    content_hash: u64,
    /// Parsed environment variables
    env_map: HashMap<String, String>,
    /// Hash of schema content (for detecting schema changes)
    schema_hash: u64,
}

/// Type of change detected for a variable
#[derive(Debug, Clone, PartialEq)]
enum ChangeType {
    Added,
    Removed,
    Modified { old_value: String },
}

/// A detected change in the environment
struct EnvChange {
    key: String,
    change_type: ChangeType,
    new_value: Option<String>,
}

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

#[allow(clippy::too_many_arguments)]
pub fn run(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
    watch: bool,
    format: &str,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
) -> Result<(), String> {
    if watch {
        if format == "json" {
            return Err("JSON format is not supported in watch mode".into());
        }
        run_watch_mode(env_path, schema_path, allow_missing_env, detect_secrets, no_cache, verify_hash, ca_cert)
    } else {
        run_once(env_path, schema_path, allow_missing_env, detect_secrets, no_cache, format, verify_hash, ca_cert)
    }
}

/// Run validation once and exit
#[allow(clippy::too_many_arguments)]
fn run_once(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
    format: &str,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
) -> Result<(), String> {
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds: None,
    };
    let schema = schema::load_schema_with_options(schema_path, &options).map_err(|e| e.to_string())?;

    let resolved_path = resolve_env_file(env_path);
    let (env_map, raw_content, duplicates): (HashMap<String, String>, Option<String>, Vec<envfile::DuplicateKey>) = match &resolved_path {
        Some(resolved) => {
            if resolved != env_path && format != "json" {
                eprintln!("Note: Using {} (fallback)\n", resolved);
            }
            let content = fs::read_to_string(resolved).map_err(|e| e.to_string())?;
            let parse_result = envfile::parse_env_file_detailed(resolved).map_err(|e| e.to_string())?;
            (parse_result.values, Some(content), parse_result.duplicates)
        }
        None if allow_missing_env => {
            // When env file is missing and flag is set, validate schema only (no env values)
            if format == "json" {
                let result = CheckResult {
                    valid: true,
                    stats: CheckStats {
                        total_variables: 0,
                        schema_variables: schema.len(),
                        errors_count: 0,
                        warnings_count: 0,
                        duplicate_warnings_count: 0,
                        secret_warnings_count: 0,
                    },
                    errors: vec![],
                    warnings: vec![],
                    duplicate_warnings: vec![],
                    secret_warnings: vec![],
                };
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
            } else {
                println!("zenv: OK (schema validated, no .env file)");
                println!("  Schema: {} variables defined", schema.len());
            }
            return Ok(());
        }
        None => return Err(missing_env_error(env_path)),
    };

    // Interpolate variable references (${VAR} and $VAR)
    let env_map = envfile::interpolate_env(env_map).map_err(|e| e.to_string())?;

    let raw_errors = validate(&schema, &env_map);

    // Convert to issues with severity
    let issues = errors_to_issues(raw_errors, &schema);

    // Separate errors (exit code 1) from warnings (reported but don't fail)
    let errors: Vec<&ValidationIssue> = issues.iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    let schema_warnings: Vec<&ValidationIssue> = issues.iter()
        .filter(|i| i.severity == Severity::Warning)
        .collect();

    // Check for secrets if flag is set
    let secret_warnings = if detect_secrets {
        if let Some(content) = &raw_content {
            secrets::detect_secrets(&env_map, content, Some(&schema))
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let has_errors = !errors.is_empty();
    let has_schema_warnings = !schema_warnings.is_empty();
    let has_duplicate_warnings = !duplicates.is_empty();
    let has_secret_warnings = !secret_warnings.is_empty();

    // JSON output mode
    if format == "json" {
        let check_errors: Vec<CheckIssue> = errors.iter().map(|e| {
            let (_, message, issue_type) = parse_error_message(&e.message);
            CheckIssue { key: e.key.clone(), message, issue_type }
        }).collect();

        let check_warnings: Vec<CheckIssue> = schema_warnings.iter().map(|w| {
            let (_, message, issue_type) = parse_error_message(&w.message);
            CheckIssue { key: w.key.clone(), message, issue_type }
        }).collect();

        let check_duplicate_warnings: Vec<DuplicateWarning> = duplicates.iter().map(|d| {
            DuplicateWarning {
                key: d.key.clone(),
                line: d.line,
                previous_line: d.previous_line,
            }
        }).collect();

        let check_secret_warnings: Vec<SecretWarning> = secret_warnings.iter().map(|w| {
            SecretWarning {
                key: w.key.clone(),
                message: w.reason.clone(),
                line: w.line,
            }
        }).collect();

        let result = CheckResult {
            valid: !has_errors,
            stats: CheckStats {
                total_variables: env_map.len(),
                schema_variables: schema.len(),
                errors_count: check_errors.len(),
                warnings_count: check_warnings.len(),
                duplicate_warnings_count: check_duplicate_warnings.len(),
                secret_warnings_count: check_secret_warnings.len(),
            },
            errors: check_errors,
            warnings: check_warnings,
            duplicate_warnings: check_duplicate_warnings,
            secret_warnings: check_secret_warnings,
        };

        println!("{}", serde_json::to_string_pretty(&result).unwrap());

        if has_errors {
            return Err("validation failed".into());
        }
        return Ok(());
    }

    // Text output mode (default)
    if has_errors {
        // Count unknown keys for the tip
        let unknown_count = errors
            .iter()
            .filter(|e| e.message.contains("not in schema"))
            .count();

        eprintln!("Error: zenv check failed:\n");
        for e in &errors {
            eprintln!("- {}", e.message);
        }

        // Suggest how to fix unknown keys
        if unknown_count > 0 {
            eprintln!("\nTip: {} unknown key(s) found in .env but not in schema.", unknown_count);
            eprintln!("  To add them: zenv init --example .env --schema {} (creates new schema)", schema_path);
            eprintln!("  Or manually add them to your schema file.");
        }

        // Count missing required keys for fix suggestion
        let missing_count = errors
            .iter()
            .filter(|e| e.message.contains("missing and required"))
            .count();

        // Suggest fix command when there are auto-fixable issues
        if missing_count > 0 || unknown_count > 0 {
            eprintln!();
            eprintln!("Tip: Run `zenv fix --dry-run --schema {} --env {}` to preview auto-fixes.", schema_path, env_path);
            if unknown_count > 0 {
                eprintln!("  Add `--remove-unknown` to also remove keys not in schema.");
            }
        }
    }

    // Show schema warnings (severity: warning)
    if has_schema_warnings {
        if has_errors {
            eprintln!();
        }
        eprintln!("Warning: Schema validation warnings:\n");
        for w in &schema_warnings {
            eprintln!("- {}", w.message);
        }
    }

    if has_secret_warnings {
        if has_errors || has_schema_warnings {
            eprintln!();
        }
        eprintln!("Warning: Potential secrets detected:\n");
        for warning in &secret_warnings {
            if warning.line > 0 {
                eprintln!("- {} (line {}): {}", warning.key, warning.line, warning.reason);
            } else {
                eprintln!("- {}: {}", warning.key, warning.reason);
            }
        }
        eprintln!("\nThese values may be real secrets. Consider using placeholders in committed files.");
        eprintln!("Use `zenv example --schema {}` to generate safe placeholders.", schema_path);
    }

    // Show duplicate key warnings
    if has_duplicate_warnings {
        if has_errors || has_schema_warnings || has_secret_warnings {
            eprintln!();
        }
        eprintln!("Warning: Duplicate keys detected:\n");
        for dup in &duplicates {
            eprintln!("- {} (line {}) overwrites previous definition at line {}", dup.key, dup.line, dup.previous_line);
        }
        eprintln!("\nDuplicate keys can cause silent overwrites. Consider removing duplicates.");
    }

    if has_errors {
        return Err("validation failed".into());
    }

    // Build success message
    let warning_count = schema_warnings.len() + secret_warnings.len() + duplicates.len();
    if warning_count > 0 {
        println!("\nzenv: OK (with {} warning(s))", warning_count);
    } else {
        println!("zenv: OK");
    }
    Ok(())
}

/// Parse an error message into structured components
fn parse_error_message(error: &str) -> (String, String, String) {
    // Format: "KEY: message" or "KEY: message\n  suggestion"
    let parts: Vec<&str> = error.splitn(2, ": ").collect();
    if parts.len() == 2 {
        let key = parts[0].to_string();
        let message = parts[1].lines().next().unwrap_or(parts[1]).to_string();

        // Determine error type from message content
        let error_type = if message.contains("missing (required)") {
            "missing_required"
        } else if message.contains("not in schema") {
            "unknown_key"
        } else if message.contains("expected") {
            "type_mismatch"
        } else if message.contains("less than minimum") || message.contains("exceeds maximum") {
            "validation_rule"
        } else if message.contains("does not match pattern") {
            "pattern_mismatch"
        } else if message.contains("length") {
            "length_violation"
        } else {
            "validation_error"
        };

        (key, message, error_type.to_string())
    } else {
        ("unknown".to_string(), error.to_string(), "validation_error".to_string())
    }
}

/// Run validation in watch mode with intelligent delta detection
fn run_watch_mode(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
) -> Result<(), String> {
    // Check if schema is a remote URL - can't watch remote schemas
    let is_remote_schema = schema_path.starts_with("http://") || schema_path.starts_with("https://");

    // Collect files to watch
    let mut watch_paths: Vec<String> = Vec::new();

    // Add env file(s) to watch
    if let Some(resolved) = resolve_env_file(env_path) {
        watch_paths.push(resolved);
    } else if Path::new(env_path).exists() {
        watch_paths.push(env_path.to_string());
    } else if env_path == ".env" {
        // Watch all fallback paths even if they don't exist yet
        watch_paths.push(".env".to_string());
        for fallback in ENV_FALLBACKS {
            watch_paths.push((*fallback).to_string());
        }
    } else {
        watch_paths.push(env_path.to_string());
    }

    // Add schema to watch (only if local file)
    if !is_remote_schema {
        watch_paths.push(schema_path.to_string());
    }

    // Print header
    println!("zenv watch v{}\n", env!("CARGO_PKG_VERSION"));
    let watch_display: Vec<&str> = watch_paths.iter().map(|s| s.as_str()).collect();
    println!("[watching] {}", watch_display.join(", "));
    if is_remote_schema {
        println!("[note] Remote schema will not be watched for changes");
    }

    // Load schema for initial validation
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds: None,
    };
    let schema = schema::load_schema_with_options(schema_path, &options)
        .map_err(|e| format!("Schema error: {}", e))?;

    // Run initial validation and capture state
    let mut state = run_initial_validation(env_path, &schema, allow_missing_env, detect_secrets, schema_path)?;

    // Set up file watcher
    let (tx, rx) = mpsc::channel();

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    )
    .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    // Watch each path
    for path in &watch_paths {
        let p = Path::new(path);
        let watch_target = if p.exists() {
            p.to_path_buf()
        } else if let Some(parent) = p.parent() {
            if parent.as_os_str().is_empty() {
                Path::new(".").to_path_buf()
            } else {
                parent.to_path_buf()
            }
        } else {
            Path::new(".").to_path_buf()
        };

        if watch_target.exists() {
            let _ = watcher.watch(&watch_target, RecursiveMode::NonRecursive);
        }
    }

    // Debounce settings
    let debounce_duration = Duration::from_millis(150);
    let mut last_event_time = Instant::now() - debounce_duration;

    println!("\nPress Ctrl+C to stop watching.\n");

    // Event loop with delta detection
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(_event) => {
                let now = Instant::now();
                if now.duration_since(last_event_time) >= debounce_duration {
                    last_event_time = now;

                    // Check for schema changes first (if local)
                    if !is_remote_schema {
                        if let Ok(schema_content) = fs::read_to_string(schema_path) {
                            let new_schema_hash = compute_hash(&schema_content);
                            if new_schema_hash != state.schema_hash {
                                // Schema changed - reload and revalidate everything
                                let timestamp = local_timestamp();
                                println!("[{}] ~ schema changed", timestamp);

                                match schema::load_schema_with_options(schema_path, &options) {
                                    Ok(new_schema) => {
                                        state.schema_hash = new_schema_hash;
                                        // Full revalidation with new schema
                                        state = match run_initial_validation(env_path, &new_schema, allow_missing_env, detect_secrets, schema_path) {
                                            Ok(s) => s,
                                            Err(e) => {
                                                eprintln!("           {}", e);
                                                print_bell();
                                                continue;
                                            }
                                        };
                                    }
                                    Err(e) => {
                                        print_schema_error(&e);
                                        print_bell();
                                    }
                                }
                                continue;
                            }
                        }
                    }

                    // Check for env file changes with delta detection
                    let resolved_path = resolve_env_file(env_path);
                    if let Some(ref resolved) = resolved_path {
                        if let Ok(content) = fs::read_to_string(resolved) {
                            let new_hash = compute_hash(&content);

                            // Skip if content unchanged (editor touch without changes)
                            if new_hash == state.content_hash {
                                continue;
                            }

                            // Parse new env file
                            match envfile::parse_env_file(resolved) {
                                Ok(new_env_raw) => {
                                    match envfile::interpolate_env(new_env_raw) {
                                        Ok(new_env) => {
                                            // Reload schema for validation
                                            let schema = match schema::load_schema_with_options(schema_path, &options) {
                                                Ok(s) => s,
                                                Err(e) => {
                                                    print_schema_error(&e);
                                                    print_bell();
                                                    continue;
                                                }
                                            };

                                            // Detect changes
                                            let changes = detect_changes(&state.env_map, &new_env);

                                            if changes.is_empty() {
                                                // No logical changes (whitespace only?)
                                                state.content_hash = new_hash;
                                                continue;
                                            }

                                            // Validate changed variables and print results
                                            let had_errors = print_delta_validation(
                                                &changes,
                                                &new_env,
                                                &schema,
                                                detect_secrets,
                                                &content,
                                            );

                                            if had_errors {
                                                print_bell();
                                            }

                                            // Update state
                                            state.content_hash = new_hash;
                                            state.env_map = new_env;
                                        }
                                        Err(e) => {
                                            let timestamp = local_timestamp();
                                            eprintln!("[{}] Interpolation error: {}", timestamp, e);
                                            print_bell();
                                        }
                                    }
                                }
                                Err(e) => {
                                    let timestamp = local_timestamp();
                                    eprintln!("[{}] Parse error: {}", timestamp, e);
                                    print_bell();
                                }
                            }
                        }
                    } else if !allow_missing_env {
                        let timestamp = local_timestamp();
                        eprintln!("[{}] Env file not found: {}", timestamp, env_path);
                        print_bell();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No event, continue waiting
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("File watcher disconnected".into());
            }
        }
    }
}

/// Run initial validation and return state for delta tracking
fn run_initial_validation(
    env_path: &str,
    schema: &Schema,
    allow_missing_env: bool,
    detect_secrets: bool,
    schema_path: &str,
) -> Result<WatchState, String> {
    let timestamp = local_timestamp();

    // Read schema content for hash
    let schema_hash = if let Ok(content) = fs::read_to_string(schema_path) {
        compute_hash(&content)
    } else {
        0 // Remote schema or missing
    };

    // Load env file
    let resolved_path = resolve_env_file(env_path);
    let (env_map, raw_content, content_hash): (HashMap<String, String>, Option<String>, u64) =
        match &resolved_path {
            Some(resolved) => {
                let content = fs::read_to_string(resolved).map_err(|e| e.to_string())?;
                let hash = compute_hash(&content);
                let map = envfile::parse_env_file(resolved).map_err(|e| e.to_string())?;
                (map, Some(content), hash)
            }
            None if allow_missing_env => (HashMap::new(), None, 0),
            None => return Err(missing_env_error(env_path)),
        };

    // Interpolate
    let env_map = envfile::interpolate_env(env_map).map_err(|e| e.to_string())?;

    // Validate all
    let errors = validate(schema, &env_map);

    // Check for secrets
    let secret_warnings = if detect_secrets {
        if let Some(content) = &raw_content {
            secrets::detect_secrets(&env_map, content, Some(schema))
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let has_errors = !errors.is_empty();
    let has_warnings = !secret_warnings.is_empty();
    let var_count = env_map.len();

    if has_errors {
        eprintln!("[{}] Initial: FAILED ({} variables)", timestamp, var_count);
        for e in &errors {
            eprintln!("           - {}", e);
        }
        print_bell();
    } else if has_warnings {
        println!("[{}] Initial: OK ({} variables, {} secret warning(s))", timestamp, var_count, secret_warnings.len());
        for warning in &secret_warnings {
            eprintln!("           - {}: {}", warning.key, warning.reason);
        }
    } else {
        println!("[{}] Initial: OK ({} variables)", timestamp, var_count);
    }

    Ok(WatchState {
        content_hash,
        env_map,
        schema_hash,
    })
}

/// Detect changes between old and new environment maps.
///
/// # Algorithm
/// Uses HashSet set operations for O(n) complexity instead of O(n^2) nested loops.
/// Memory tradeoff: creates two HashSets of key references for efficient comparison.
///
/// # Change Detection
/// - **Added**: Keys in `new` but not in `old` (new_keys.difference(old_keys))
/// - **Removed**: Keys in `old` but not in `new` (old_keys.difference(new_keys))
/// - **Modified**: Keys in both with different values (old_keys.intersection(new_keys))
///
/// # Output
/// Returns sorted Vec<EnvChange> for consistent, reproducible output in watch mode.
/// Sorting is O(n log n) but acceptable for typical .env sizes (<100 variables).
fn detect_changes(old: &HashMap<String, String>, new: &HashMap<String, String>) -> Vec<EnvChange> {
    let mut changes = Vec::new();

    // Build HashSets for O(n) set operations instead of O(n^2) nested iteration
    let old_keys: HashSet<&String> = old.keys().collect();
    let new_keys: HashSet<&String> = new.keys().collect();

    // Added: keys present in new state but absent from old state
    for key in new_keys.difference(&old_keys) {
        changes.push(EnvChange {
            key: (*key).clone(),
            change_type: ChangeType::Added,
            new_value: new.get(*key).cloned(),
        });
    }

    // Removed: keys present in old state but absent from new state
    for key in old_keys.difference(&new_keys) {
        changes.push(EnvChange {
            key: (*key).clone(),
            change_type: ChangeType::Removed,
            new_value: None,
        });
    }

    // Modified: keys in both states with different values (string equality comparison)
    for key in old_keys.intersection(&new_keys) {
        let old_val = old.get(*key).unwrap();
        let new_val = new.get(*key).unwrap();
        if old_val != new_val {
            changes.push(EnvChange {
                key: (*key).clone(),
                change_type: ChangeType::Modified { old_value: old_val.clone() },
                new_value: Some(new_val.clone()),
            });
        }
    }

    // Sort alphabetically for consistent output order (aids testing and user experience)
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    changes
}

/// Print delta validation results for changed variables
fn print_delta_validation(
    changes: &[EnvChange],
    env_map: &HashMap<String, String>,
    schema: &Schema,
    detect_secrets: bool,
    raw_content: &str,
) -> bool {
    let timestamp = local_timestamp();
    let mut had_errors = false;

    for change in changes {
        let symbol = match &change.change_type {
            ChangeType::Added => "+",
            ChangeType::Removed => "-",
            ChangeType::Modified { .. } => "~",
        };

        // Build change description
        let change_desc = match &change.change_type {
            ChangeType::Added => {
                let val = truncate_value(change.new_value.as_deref().unwrap_or(""));
                format!("{} {} = \"{}\"", symbol, change.key, val)
            }
            ChangeType::Removed => {
                format!("{} {} (removed)", symbol, change.key)
            }
            ChangeType::Modified { old_value } => {
                let old = truncate_value(old_value);
                let new = truncate_value(change.new_value.as_deref().unwrap_or(""));
                format!("{} {}: \"{}\" -> \"{}\"", symbol, change.key, old, new)
            }
        };

        // Validate this specific key
        let validation_result = if change.change_type == ChangeType::Removed {
            // Check if removed key was required
            if let Some(spec) = schema.get(&change.key) {
                if spec.required && spec.default.is_none() {
                    Some(format!("FAILED: {} is required", change.key))
                } else {
                    Some("OK: optional variable removed".to_string())
                }
            } else {
                Some("OK: unknown variable removed".to_string())
            }
        } else {
            // Validate the key against schema
            match schema.get(&change.key) {
                Some(spec) => {
                    let value = change.new_value.as_deref().unwrap_or("");
                    match validate_single_key(&change.key, value, spec) {
                        Ok(type_info) => Some(format!("OK ({})", type_info)),
                        Err(e) => {
                            had_errors = true;
                            Some(format!("FAILED: {}", e))
                        }
                    }
                }
                None => {
                    // Key not in schema
                    had_errors = true;
                    Some("WARNING: not in schema".to_string())
                }
            }
        };

        // Print the change and validation result
        print!("[{}] {}", timestamp, change_desc);
        if let Some(result) = validation_result {
            if result.starts_with("FAILED") || result.starts_with("WARNING") {
                eprintln!();
                eprintln!("           {}", result);
            } else {
                println!();
                println!("           {}", result);
            }
        } else {
            println!();
        }
    }

    // Check for secrets on added/modified values
    if detect_secrets {
        let changed_keys: HashSet<String> = changes.iter()
            .filter(|c| c.change_type != ChangeType::Removed)
            .map(|c| c.key.clone())
            .collect();

        let secret_warnings = secrets::detect_secrets(env_map, raw_content, Some(schema));
        for warning in secret_warnings {
            if changed_keys.contains(&warning.key) {
                eprintln!("[{}] ! {}: potential secret detected", timestamp, warning.key);
                eprintln!("           {}", warning.reason);
            }
        }
    }

    had_errors
}

/// Validate a single key-value pair against its spec
fn validate_single_key(key: &str, value: &str, spec: &VarSpec) -> Result<String, String> {
    match spec.var_type {
        VarType::String => {
            if let Some(ref rules) = spec.validate {
                if let Some(min_len) = rules.min_length {
                    if value.len() < min_len {
                        return Err(format!("length {} < minimum {}", value.len(), min_len));
                    }
                }
                if let Some(max_len) = rules.max_length {
                    if value.len() > max_len {
                        return Err(format!("length {} > maximum {}", value.len(), max_len));
                    }
                }
                if let Some(ref pattern) = rules.pattern {
                    match get_cached_regex(pattern) {
                        Ok(re) => {
                            if !re.is_match(value) {
                                return Err(format!("does not match pattern '{}'", pattern));
                            }
                        }
                        Err(e) => {
                            return Err(format!("invalid regex: {}", e));
                        }
                    }
                }
            }
            Ok("string".to_string())
        }
        VarType::Int => {
            match value.parse::<i64>() {
                Ok(n) => {
                    if let Some(ref rules) = spec.validate {
                        if let Some(min) = rules.min {
                            if n < min {
                                return Err(format!("value {} < minimum {}", n, min));
                            }
                        }
                        if let Some(max) = rules.max {
                            if n > max {
                                return Err(format!("value {} > maximum {}", n, max));
                            }
                        }
                    }
                    Ok(format!("int: {}", n))
                }
                Err(_) => Err(format!("expected int, got '{}'", mask_value(key, value))),
            }
        }
        VarType::Float => {
            match value.parse::<f64>() {
                Ok(n) => {
                    if let Some(ref rules) = spec.validate {
                        if let Some(min_val) = rules.min_value {
                            if n < min_val {
                                return Err(format!("value {} < minimum {}", n, min_val));
                            }
                        }
                        if let Some(max_val) = rules.max_value {
                            if n > max_val {
                                return Err(format!("value {} > maximum {}", n, max_val));
                            }
                        }
                    }
                    Ok(format!("float: {}", n))
                }
                Err(_) => Err(format!("expected float, got '{}'", mask_value(key, value))),
            }
        }
        VarType::Bool => {
            let v = value.to_lowercase();
            if matches!(v.as_str(), "true" | "false" | "1" | "0" | "yes" | "no") {
                Ok(format!("bool: {}", v))
            } else {
                Err(format!("expected bool, got '{}'", mask_value(key, value)))
            }
        }
        VarType::Url => {
            if Url::parse(value).is_ok() {
                Ok("url".to_string())
            } else {
                Err(format!("expected url, got '{}'", mask_value(key, value)))
            }
        }
        VarType::Enum => {
            match spec.values.as_ref() {
                Some(allowed) => {
                    if allowed.iter().any(|v| v == value) {
                        Ok(format!("enum: {}", value))
                    } else {
                        Err(format!("expected one of {:?}", allowed))
                    }
                }
                None => Err("enum type missing 'values' in schema".to_string()),
            }
        }
        VarType::Uuid => {
            if uuid_regex().is_match(value) {
                Ok("uuid".to_string())
            } else {
                Err(format!("expected uuid (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx), got '{}'", mask_value(key, value)))
            }
        }
        VarType::Email => {
            if email_regex().is_match(value) {
                Ok("email".to_string())
            } else {
                Err(format!("expected email (user@domain.tld), got '{}'", mask_value(key, value)))
            }
        }
        VarType::Ipv4 => {
            if is_valid_ipv4(value) {
                Ok("ipv4".to_string())
            } else {
                Err(format!("expected ipv4 (x.x.x.x where x is 0-255), got '{}'", mask_value(key, value)))
            }
        }
        VarType::Semver => {
            if semver_regex().is_match(value) {
                Ok("semver".to_string())
            } else {
                Err(format!("expected semver (x.y.z[-prerelease][+build]), got '{}'", mask_value(key, value)))
            }
        }
        VarType::Ipv6 => {
            if ipv6_regex().is_match(value) {
                Ok("ipv6".to_string())
            } else {
                Err(format!("expected ipv6 address, got '{}'", mask_value(key, value)))
            }
        }
        VarType::Port => {
            match value.parse::<u16>() {
                Ok(port) if port >= 1 => Ok(format!("port: {}", port)),
                Ok(_) => Err("port must be between 1 and 65535".to_string()),
                Err(_) => Err(format!("expected port (1-65535), got '{}'", mask_value(key, value))),
            }
        }
        VarType::Date => {
            if date_regex().is_match(value) {
                Ok("date".to_string())
            } else {
                Err(format!("expected date (YYYY-MM-DD), got '{}'", mask_value(key, value)))
            }
        }
        VarType::Hostname => {
            if value.len() <= 253 && hostname_regex().is_match(value) {
                Ok("hostname".to_string())
            } else {
                Err(format!("expected hostname (RFC 1123), got '{}'", mask_value(key, value)))
            }
        }
    }
}

/// Truncate a value for display (max 30 chars)
fn truncate_value(value: &str) -> String {
    if value.len() <= 30 {
        value.replace('\n', "\\n")
    } else {
        format!("{}...", &value[..27].replace('\n', "\\n"))
    }
}

/// Compute a simple hash of a string for change detection
fn compute_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Print terminal bell character (beep on error)
fn print_bell() {
    eprint!("\x07");
}

/// Print a schema error with helpful context
fn print_schema_error(error: &dyn std::fmt::Display) {
    let timestamp = local_timestamp();
    let error_str = error.to_string();

    eprintln!("[{}] Schema error:", timestamp);

    // Parse the error for better formatting
    if error_str.contains("line ") && error_str.contains("column ") {
        // JSON parse error with location
        eprintln!("           {}", error_str);
        eprintln!();
        eprintln!("           Tip: Check for trailing commas, missing quotes, or invalid JSON syntax.");
    } else if error_str.contains("No such file") || error_str.contains("cannot find") || error_str.contains("not found") {
        eprintln!("           File not found: {}", error_str);
        eprintln!();
        eprintln!("           Tip: Create a schema with: zenv init --example .env.example");
    } else if error_str.contains("invalid type") || error_str.contains("unknown variant") {
        eprintln!("           {}", error_str);
        eprintln!();
        eprintln!("           Tip: Valid types are: string, int, float, bool, url, enum");
    } else {
        // Generic error
        eprintln!("           {}", error_str);
    }
}

/// Get local timestamp in HH:MM:SS format using OS APIs
#[cfg(windows)]
fn local_timestamp() -> String {
    use std::mem::MaybeUninit;

    #[repr(C)]
    struct SystemTime {
        w_year: u16,
        w_month: u16,
        w_day_of_week: u16,
        w_day: u16,
        w_hour: u16,
        w_minute: u16,
        w_second: u16,
        w_milliseconds: u16,
    }

    extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTime);
    }

    let mut st = MaybeUninit::<SystemTime>::uninit();
    unsafe {
        GetLocalTime(st.as_mut_ptr());
        let st = st.assume_init();
        format!("{:02}:{:02}:{:02}", st.w_hour, st.w_minute, st.w_second)
    }
}

#[cfg(not(windows))]
fn local_timestamp() -> String {
    use std::time::SystemTime;

    // On Unix, we could use libc::localtime_r for proper local time
    // For simplicity, falling back to UTC with note
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let total_secs = now.as_secs();
    let secs_in_day = total_secs % 86400;
    let hours = (secs_in_day / 3600) % 24;
    let minutes = (secs_in_day % 3600) / 60;
    let seconds = secs_in_day % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
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
                        match get_cached_regex(pattern) {
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
                        errors.push(format!("{key}: expected int, got '{}'", mask_value(key, value)));
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
                        errors.push(format!("{key}: expected float, got '{}'", mask_value(key, value)));
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
                    errors.push(format!("{key}: expected bool (true/false/1/0/yes/no), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Url => {
                if Url::parse(value).is_err() {
                    errors.push(format!("{key}: expected url, got '{}'", mask_value(key, value)));
                }
            }

            VarType::Enum => {
                match spec.values.as_ref() {
                    None => {
                        errors.push(format!("{key}: enum type missing 'values' field in schema"));
                    }
                    Some(allowed) => {
                        if !allowed.iter().any(|v| v == value) {
                            let mut error_msg = format!("{key}: expected one of {:?}, got '{}'", allowed, mask_value(key, value));
                            // Add "Did you mean?" suggestion
                            if let Some(suggestion) = suggestions::suggest_enum_value(value, allowed.iter()) {
                                error_msg.push_str(&format!("\n  {}", suggestion));
                            }
                            errors.push(error_msg);
                        }
                    }
                }
            }

            VarType::Uuid => {
                if !uuid_regex().is_match(value) {
                    errors.push(format!("{key}: expected uuid (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Email => {
                if !email_regex().is_match(value) {
                    errors.push(format!("{key}: expected email (user@domain.tld), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Ipv4 => {
                if !is_valid_ipv4(value) {
                    errors.push(format!("{key}: expected ipv4 (x.x.x.x where x is 0-255), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Semver => {
                if !semver_regex().is_match(value) {
                    errors.push(format!("{key}: expected semver (x.y.z[-prerelease][+build]), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Ipv6 => {
                if !ipv6_regex().is_match(value) {
                    errors.push(format!("{key}: expected ipv6 address, got '{}'", mask_value(key, value)));
                }
            }

            VarType::Port => {
                match value.parse::<u16>() {
                    Ok(port) if port >= 1 => {}
                    Ok(_) => {
                        errors.push(format!("{key}: port must be between 1 and 65535"));
                    }
                    Err(_) => {
                        errors.push(format!("{key}: expected port (1-65535), got '{}'", mask_value(key, value)));
                    }
                }
            }

            VarType::Date => {
                if !date_regex().is_match(value) {
                    errors.push(format!("{key}: expected date (YYYY-MM-DD), got '{}'", mask_value(key, value)));
                }
            }

            VarType::Hostname => {
                if value.len() > 253 || !hostname_regex().is_match(value) {
                    errors.push(format!("{key}: expected hostname (RFC 1123), got '{}'", mask_value(key, value)));
                }
            }
        }
    }

    // warn on unknown keys (present in env but not in schema)
    for k in env_map.keys() {
        if !schema.contains_key(k) {
            let mut error_msg = format!("{k}: not in schema (unknown key)");
            // Add "Did you mean?" suggestion
            if let Some(suggestion) = suggestions::suggest_variable_name(k, schema.keys()) {
                error_msg.push_str(&format!("\n  {}", suggestion));
            }
            errors.push(error_msg);
        }
    }

    errors
}

/// Validate environment file against schema file (convenience function)
///
/// This is a convenience wrapper that handles file loading and parsing.
/// Useful for simple validation without needing to manually load files.
///
/// # Arguments
/// * `env_path` - Path to the .env file
/// * `schema_path` - Path to the schema file (JSON or YAML)
/// * `opts` - Schema loading options (caching, hash verification, etc.)
///
/// # Returns
/// * `Ok(Vec<String>)` - Empty vec if valid, otherwise contains error messages
/// * `Err(String)` - If schema or env file could not be loaded
///
/// # Example
/// ```ignore
/// let opts = LoadOptions::default();
/// let errors = validate_files(".env", "env.schema.json", &opts)?;
/// if errors.is_empty() {
///     println!("Valid!");
/// }
/// ```
pub fn validate_files(
    env_path: &str,
    schema_path: &str,
    opts: &LoadOptions,
) -> Result<Vec<String>, String> {
    // Load schema
    let schema_result = schema::load_schema_with_options(schema_path, opts);
    let loaded_schema = schema_result.map_err(|e| e.to_string())?;

    // Parse env file
    let env_map = envfile::parse_env_file(env_path)
        .map_err(|e| format!("Failed to parse {}: {}", env_path, e))?;

    // Interpolate variables
    let env_map = envfile::interpolate_env(env_map)
        .map_err(|e| format!("Interpolation error: {}", e))?;

    // Validate and return errors
    Ok(validate(&loaded_schema, &env_map))
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
            ..Default::default()
        }
    }

    fn int_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Int,
            required,
            ..Default::default()
        }
    }

    fn float_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Float,
            ..Default::default()
        }
    }

    fn bool_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Bool,
            ..Default::default()
        }
    }

    fn url_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Url,
            ..Default::default()
        }
    }

    fn enum_spec(values: Vec<&str>) -> VarSpec {
        VarSpec {
            var_type: VarType::Enum,
            values: Some(values.into_iter().map(String::from).collect()),
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            validate: Some(ValidationRule {
                min: Some(1024),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("PORT", "3000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_min_invalid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            validate: Some(ValidationRule {
                min: Some(1024),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                max: Some(65535),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("PORT", "8080")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_int_max_invalid() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            validate: Some(ValidationRule {
                max: Some(65535),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                min: Some(1024),
                max: Some(65535),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                min_value: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("RATE", "0.5")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_min_value_invalid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            validate: Some(ValidationRule {
                min_value: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                max_value: Some(1.0),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("RATE", "0.75")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_float_max_value_invalid() {
        let schema = make_schema(vec![("RATE", VarSpec {
            var_type: VarType::Float,
            validate: Some(ValidationRule {
                max_value: Some(1.0),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                min_length: Some(8),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("API_KEY", "abcdefghij")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_min_length_invalid() {
        let schema = make_schema(vec![("API_KEY", VarSpec {
            var_type: VarType::String,
            validate: Some(ValidationRule {
                min_length: Some(8),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                max_length: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("CODE", "ABC123")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_max_length_invalid() {
        let schema = make_schema(vec![("CODE", VarSpec {
            var_type: VarType::String,
            validate: Some(ValidationRule {
                max_length: Some(5),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                pattern: Some(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("EMAIL", "user@example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_pattern_invalid() {
        let schema = make_schema(vec![("EMAIL", VarSpec {
            var_type: VarType::String,
            validate: Some(ValidationRule {
                pattern: Some(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                pattern: Some(r"^v\d+\.\d+\.\d+$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("VERSION", "v1.2.3")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_pattern_invalid_regex() {
        let schema = make_schema(vec![("FOO", VarSpec {
            var_type: VarType::String,
            validate: Some(ValidationRule {
                pattern: Some(r"[invalid".to_string()),
                ..Default::default()
            }),
            ..Default::default()
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
            validate: Some(ValidationRule {
                min_length: Some(36),
                max_length: Some(36),
                pattern: Some(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("UUID", "550e8400-e29b-41d4-a716-446655440000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_string_multiple_validation_failures() {
        let schema = make_schema(vec![("CODE", VarSpec {
            var_type: VarType::String,
            validate: Some(ValidationRule {
                min_length: Some(10),
                pattern: Some(r"^[A-Z]+$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        })]);
        let env = make_env(vec![("CODE", "abc")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 2); // too short AND wrong pattern
    }

    // UUID type tests

    fn uuid_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Uuid,
            ..Default::default()
        }
    }

    #[test]
    fn test_uuid_type_valid() {
        let schema = make_schema(vec![("SESSION_ID", uuid_spec())]);
        let env = make_env(vec![("SESSION_ID", "550e8400-e29b-41d4-a716-446655440000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_uuid_type_valid_uppercase() {
        let schema = make_schema(vec![("SESSION_ID", uuid_spec())]);
        let env = make_env(vec![("SESSION_ID", "550E8400-E29B-41D4-A716-446655440000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_uuid_type_invalid_format() {
        let schema = make_schema(vec![("SESSION_ID", uuid_spec())]);
        let env = make_env(vec![("SESSION_ID", "not-a-uuid")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected uuid"));
    }

    #[test]
    fn test_uuid_type_invalid_no_dashes() {
        let schema = make_schema(vec![("SESSION_ID", uuid_spec())]);
        let env = make_env(vec![("SESSION_ID", "550e8400e29b41d4a716446655440000")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected uuid"));
    }

    // Email type tests

    fn email_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Email,
            ..Default::default()
        }
    }

    #[test]
    fn test_email_type_valid() {
        let schema = make_schema(vec![("ADMIN_EMAIL", email_spec())]);
        let env = make_env(vec![("ADMIN_EMAIL", "user@example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_email_type_valid_subdomain() {
        let schema = make_schema(vec![("ADMIN_EMAIL", email_spec())]);
        let env = make_env(vec![("ADMIN_EMAIL", "user@mail.example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_email_type_valid_plus() {
        let schema = make_schema(vec![("ADMIN_EMAIL", email_spec())]);
        let env = make_env(vec![("ADMIN_EMAIL", "user+tag@example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_email_type_invalid_no_at() {
        let schema = make_schema(vec![("ADMIN_EMAIL", email_spec())]);
        let env = make_env(vec![("ADMIN_EMAIL", "userexample.com")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected email"));
    }

    #[test]
    fn test_email_type_invalid_no_domain() {
        let schema = make_schema(vec![("ADMIN_EMAIL", email_spec())]);
        let env = make_env(vec![("ADMIN_EMAIL", "user@")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected email"));
    }

    // IPv4 type tests

    fn ipv4_spec() -> VarSpec {
        VarSpec {
            var_type: VarType::Ipv4,
            ..Default::default()
        }
    }

    #[test]
    fn test_ipv4_type_valid() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "192.168.1.1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv4_type_valid_localhost() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "127.0.0.1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv4_type_valid_all_zeros() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "0.0.0.0")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv4_type_valid_max() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "255.255.255.255")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv4_type_invalid_octet_too_large() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "256.168.1.1")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected ipv4"));
    }

    #[test]
    fn test_ipv4_type_invalid_format() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "192.168.1")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected ipv4"));
    }

    #[test]
    fn test_ipv4_type_invalid_text() {
        let schema = make_schema(vec![("BIND_ADDRESS", ipv4_spec())]);
        let env = make_env(vec![("BIND_ADDRESS", "localhost")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected ipv4"));
    }

    // Suggestions tests

    #[test]
    fn test_unknown_key_with_suggestion() {
        let schema = make_schema(vec![
            ("DATABASE_URL", string_spec(false)),
            ("PORT", int_spec(false)),
        ]);
        let env = make_env(vec![("DATABSE_URL", "test")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("not in schema"));
        assert!(errors[0].contains("Did you mean"));
        assert!(errors[0].contains("DATABASE_URL"));
    }

    #[test]
    fn test_enum_with_suggestion() {
        let schema = make_schema(vec![("NODE_ENV", enum_spec(vec!["development", "staging", "production"]))]);
        let env = make_env(vec![("NODE_ENV", "dev")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected one of"));
        assert!(errors[0].contains("Did you mean"));
        assert!(errors[0].contains("development"));
    }

    // Secret masking tests

    #[test]
    fn test_is_sensitive_key_password() {
        assert!(is_sensitive_key("DATABASE_PASSWORD"));
        assert!(is_sensitive_key("DB_PASSWD"));
        assert!(is_sensitive_key("user_password"));
    }

    #[test]
    fn test_is_sensitive_key_secret() {
        assert!(is_sensitive_key("JWT_SECRET"));
        assert!(is_sensitive_key("APP_SECRET"));
        assert!(is_sensitive_key("client_secret"));
    }

    #[test]
    fn test_is_sensitive_key_token() {
        assert!(is_sensitive_key("AUTH_TOKEN"));
        assert!(is_sensitive_key("ACCESS_TOKEN"));
        assert!(is_sensitive_key("BEARER_TOKEN"));
    }

    #[test]
    fn test_is_sensitive_key_api_key() {
        assert!(is_sensitive_key("STRIPE_API_KEY"));
        assert!(is_sensitive_key("GITHUB_APIKEY"));
        assert!(is_sensitive_key("AWS_ACCESS_KEY"));
    }

    #[test]
    fn test_is_sensitive_key_suffix() {
        assert!(is_sensitive_key("ENCRYPTION_KEY"));
        assert!(is_sensitive_key("SIGNING_KEY"));
        assert!(is_sensitive_key("MY_CUSTOM_TOKEN"));
        assert!(is_sensitive_key("APP_SECRET"));
    }

    #[test]
    fn test_is_sensitive_key_not_sensitive() {
        assert!(!is_sensitive_key("DATABASE_URL"));
        assert!(!is_sensitive_key("PORT"));
        assert!(!is_sensitive_key("NODE_ENV"));
        assert!(!is_sensitive_key("DEBUG"));
    }

    #[test]
    fn test_mask_value_sensitive() {
        assert_eq!(mask_value("API_KEY", "sk_live_abc123"), "***MASKED***");
        assert_eq!(mask_value("JWT_SECRET", "mysupersecret"), "***MASKED***");
        assert_eq!(mask_value("DATABASE_PASSWORD", "hunter2"), "***MASKED***");
    }

    #[test]
    fn test_mask_value_not_sensitive() {
        assert_eq!(mask_value("PORT", "3000"), "3000");
        assert_eq!(mask_value("NODE_ENV", "development"), "development");
        assert_eq!(mask_value("DEBUG", "true"), "true");
    }

    #[test]
    fn test_secret_masking_in_error() {
        let schema = make_schema(vec![("API_SECRET", int_spec(true))]);
        let env = make_env(vec![("API_SECRET", "not_an_int_but_secret")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        // Should NOT contain the actual value
        assert!(!errors[0].contains("not_an_int_but_secret"));
        // Should contain the masked indicator
        assert!(errors[0].contains("***MASKED***"));
    }

    #[test]
    fn test_non_secret_shows_value_in_error() {
        let schema = make_schema(vec![("PORT", int_spec(true))]);
        let env = make_env(vec![("PORT", "not_a_number")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        // Should contain the actual value since PORT is not sensitive
        assert!(errors[0].contains("not_a_number"));
    }

    // Semver type tests
    fn semver_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Semver,
            required,
            ..Default::default()
        }
    }

    #[test]
    fn test_semver_valid_simple() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "1.0.0")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semver_valid_with_prerelease() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "2.1.3-beta.1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semver_valid_with_build() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "1.0.0+build.123")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semver_valid_with_prerelease_and_build() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "3.2.1-alpha.2+build.456")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semver_valid_zero_version() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "0.0.0")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_semver_invalid_missing_patch() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "1.0")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected semver"));
    }

    #[test]
    fn test_semver_invalid_text() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "latest")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected semver"));
    }

    #[test]
    fn test_semver_invalid_v_prefix() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "v1.0.0")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected semver"));
    }

    #[test]
    fn test_semver_invalid_extra_parts() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "1.0.0.0")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected semver"));
    }

    // =========================================================================
    // IPv6 Type Tests
    // =========================================================================

    fn ipv6_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Ipv6,
            required,
            ..Default::default()
        }
    }

    #[test]
    fn test_ipv6_type_valid_full() {
        let schema = make_schema(vec![("IP", ipv6_spec(true))]);
        let env = make_env(vec![("IP", "2001:0db8:85a3:0000:0000:8a2e:0370:7334")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv6_type_valid_uppercase() {
        let schema = make_schema(vec![("IP", ipv6_spec(true))]);
        let env = make_env(vec![("IP", "2001:0DB8:85A3:0000:0000:8A2E:0370:7335")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv6_type_valid_all_zeros() {
        let schema = make_schema(vec![("IP", ipv6_spec(true))]);
        let env = make_env(vec![("IP", "0000:0000:0000:0000:0000:0000:0000:0000")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_ipv6_type_invalid() {
        let schema = make_schema(vec![("IP", ipv6_spec(true))]);
        let env = make_env(vec![("IP", "not-an-ipv6")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected ipv6"));
    }

    // =========================================================================
    // Port Type Tests
    // =========================================================================

    fn port_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Port,
            required,
            ..Default::default()
        }
    }

    #[test]
    fn test_port_type_valid_standard() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "8080")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_port_type_valid_min() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_port_type_valid_max() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "65535")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_port_type_invalid_zero() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "0")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("port") && errors[0].contains("between"));
    }

    #[test]
    fn test_port_type_invalid_too_high() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "65536")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("port"));
    }

    #[test]
    fn test_port_type_invalid_text() {
        let schema = make_schema(vec![("PORT", port_spec(true))]);
        let env = make_env(vec![("PORT", "http")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
    }

    // =========================================================================
    // Date Type Tests
    // =========================================================================

    fn date_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Date,
            required,
            ..Default::default()
        }
    }

    #[test]
    fn test_date_type_valid_standard() {
        let schema = make_schema(vec![("EXPIRY", date_spec(true))]);
        let env = make_env(vec![("EXPIRY", "2024-01-15")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_date_type_valid_leap_year() {
        let schema = make_schema(vec![("DATE", date_spec(true))]);
        let env = make_env(vec![("DATE", "2024-02-29")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_date_type_invalid_format() {
        let schema = make_schema(vec![("DATE", date_spec(true))]);
        let env = make_env(vec![("DATE", "01/15/2024")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected date"));
    }

    #[test]
    fn test_date_type_invalid_month() {
        let schema = make_schema(vec![("DATE", date_spec(true))]);
        let env = make_env(vec![("DATE", "2024-13-01")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
    }

    // =========================================================================
    // Hostname Type Tests
    // =========================================================================

    fn hostname_spec(required: bool) -> VarSpec {
        VarSpec {
            var_type: VarType::Hostname,
            required,
            ..Default::default()
        }
    }

    #[test]
    fn test_hostname_type_valid_simple() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "localhost")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_hostname_type_valid_domain() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "api.example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_hostname_type_valid_subdomain() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "dev.api.example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_hostname_type_invalid_starting_dash() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "-invalid.com")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected hostname"));
    }

    #[test]
    fn test_hostname_type_invalid_spaces() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "invalid host.com")]);
        let errors = validate(&schema, &env);
        assert_eq!(errors.len(), 1);
    }

    // Test for invalid flag combination: --watch with --format json
    #[test]
    fn test_watch_json_combination_rejected() {
        // Create temp files for testing
        let temp_dir = std::env::temp_dir();
        let schema_path = temp_dir.join("test_schema.json");
        let env_path = temp_dir.join("test.env");

        // Write minimal schema
        std::fs::write(&schema_path, r#"{"FOO": {"type": "string"}}"#).unwrap();
        std::fs::write(&env_path, "FOO=bar").unwrap();

        // Test that watch + json combination returns error
        let result = super::run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            false,  // allow_missing_env
            false,  // detect_secrets
            true,   // no_cache
            true,   // watch = true
            "json", // format = json (invalid with watch)
            None,   // verify_hash
            None,   // ca_cert
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JSON format is not supported in watch mode"));

        // Cleanup
        let _ = std::fs::remove_file(&schema_path);
        let _ = std::fs::remove_file(&env_path);
    }

    // =========================================================================
    // Severity Level Tests (v0.3.5)
    // =========================================================================

    #[test]
    fn test_severity_default_is_error() {
        // VarSpec without severity should default to Error
        let spec = VarSpec {
            var_type: VarType::String,
            required: true,
            ..Default::default()
        };
        assert_eq!(spec.severity, crate::schema::Severity::Error);
    }

    #[test]
    fn test_severity_warning_in_issues() {
        // Test that severity is correctly converted from schema
        let mut schema = Schema::new();
        let spec = VarSpec {
            var_type: VarType::Int,
            required: true,
            severity: crate::schema::Severity::Warning,
            ..Default::default()
        };
        schema.insert("PORT".to_string(), spec);

        let mut env_map = std::collections::HashMap::new();
        env_map.insert("PORT".to_string(), "not_a_number".to_string());

        let errors = validate(&schema, &env_map);
        assert!(!errors.is_empty(), "Should have validation error");

        // Convert to issues and check severity (errors_to_issues takes ownership)
        let issues = errors_to_issues(errors, &schema);
        assert!(!issues.is_empty());
        assert_eq!(issues[0].severity, crate::schema::Severity::Warning);
    }

    #[test]
    fn test_severity_error_in_issues() {
        // Test default error severity
        let mut schema = Schema::new();
        let spec = VarSpec {
            var_type: VarType::Int,
            required: true,
            ..Default::default()
        };
        schema.insert("PORT".to_string(), spec);

        let mut env_map = std::collections::HashMap::new();
        env_map.insert("PORT".to_string(), "invalid".to_string());

        let errors = validate(&schema, &env_map);
        let issues = errors_to_issues(errors, &schema);
        assert!(!issues.is_empty());
        assert_eq!(issues[0].severity, crate::schema::Severity::Error);
    }

    #[test]
    fn test_severity_mixed_warning_and_error() {
        // Schema with both warning and error severity fields
        let mut schema = Schema::new();

        let warn_spec = VarSpec {
            var_type: VarType::Int,
            required: true,
            severity: crate::schema::Severity::Warning,
            ..Default::default()
        };
        schema.insert("WARN_PORT".to_string(), warn_spec);

        let error_spec = VarSpec {
            var_type: VarType::Int,
            required: true,
            ..Default::default()
        };
        schema.insert("ERROR_PORT".to_string(), error_spec);

        let mut env_map = std::collections::HashMap::new();
        env_map.insert("WARN_PORT".to_string(), "invalid".to_string());
        env_map.insert("ERROR_PORT".to_string(), "invalid".to_string());

        let errors = validate(&schema, &env_map);
        let issues = errors_to_issues(errors, &schema);

        // Should have both warnings and errors
        let warnings: Vec<_> = issues.iter()
            .filter(|i| i.severity == crate::schema::Severity::Warning)
            .collect();
        let errors_list: Vec<_> = issues.iter()
            .filter(|i| i.severity == crate::schema::Severity::Error)
            .collect();

        assert!(!warnings.is_empty(), "Should have warnings");
        assert!(!errors_list.is_empty(), "Should have errors");
    }

    // =========================================================================
    // Watch Mode Tests (v0.3.4) - Additional validation
    // =========================================================================

    #[test]
    fn test_watch_state_content_hash_changes() {
        // Test that WatchState tracks content changes
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_content(content: &str) -> u64 {
            let mut hasher = DefaultHasher::new();
            content.hash(&mut hasher);
            hasher.finish()
        }

        let content1 = "FOO=bar\n";
        let content2 = "FOO=baz\n";

        let hash1 = hash_content(content1);
        let hash2 = hash_content(content2);

        assert_ne!(hash1, hash2, "Different content should produce different hashes");
    }

    #[test]
    fn test_detect_changes_added_key() {
        let old: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
            ("BAZ".to_string(), "qux".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        assert!(!changes.is_empty(), "Should detect added key");
        // Check that one of the changes is for the added key "BAZ"
        assert_eq!(changes.len(), 1, "Should have exactly one change");
    }

    #[test]
    fn test_detect_changes_removed_key() {
        let old: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
            ("BAZ".to_string(), "qux".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        assert!(!changes.is_empty(), "Should detect removed key");
        assert_eq!(changes.len(), 1, "Should have exactly one change");
    }

    #[test]
    fn test_detect_changes_modified_key() {
        let old: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "baz".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        assert!(!changes.is_empty(), "Should detect modified key");
        assert_eq!(changes.len(), 1, "Should have exactly one change");
    }

    #[test]
    fn test_detect_changes_no_changes() {
        let old: std::collections::HashMap<String, String> = [
            ("FOO".to_string(), "bar".to_string()),
        ].into_iter().collect();

        let new = old.clone();

        let changes = detect_changes(&old, &new);
        assert!(changes.is_empty(), "Should detect no changes for identical maps");
    }

    #[test]
    fn test_detect_changes_multiple_changes() {
        let old: std::collections::HashMap<String, String> = [
            ("KEEP".to_string(), "same".to_string()),
            ("REMOVE".to_string(), "gone".to_string()),
            ("MODIFY".to_string(), "old".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("KEEP".to_string(), "same".to_string()),
            ("ADD".to_string(), "new".to_string()),
            ("MODIFY".to_string(), "new".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        // Should have: 1 removed, 1 added, 1 modified = 3 changes
        assert_eq!(changes.len(), 3, "Should detect all three types of changes");
    }

    // ====== Additional Watch Mode Tests ======

    #[test]
    fn test_watch_state_empty_maps() {
        let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let changes = detect_changes(&empty, &empty);
        assert!(changes.is_empty(), "Empty maps should have no changes");
    }

    #[test]
    fn test_watch_state_add_first_key() {
        let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let one_key: std::collections::HashMap<String, String> = [
            ("FIRST".to_string(), "value".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&empty, &one_key);
        assert_eq!(changes.len(), 1, "Adding first key should be one change");
    }

    #[test]
    fn test_watch_state_remove_last_key() {
        let one_key: std::collections::HashMap<String, String> = [
            ("LAST".to_string(), "value".to_string()),
        ].into_iter().collect();
        let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        let changes = detect_changes(&one_key, &empty);
        assert_eq!(changes.len(), 1, "Removing last key should be one change");
    }

    #[test]
    fn test_watch_state_value_whitespace_change() {
        let old: std::collections::HashMap<String, String> = [
            ("KEY".to_string(), "value".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("KEY".to_string(), "value ".to_string()), // trailing space
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        assert_eq!(changes.len(), 1, "Whitespace change should be detected");
    }

    #[test]
    fn test_watch_state_case_sensitive_keys() {
        let old: std::collections::HashMap<String, String> = [
            ("KEY".to_string(), "value".to_string()),
        ].into_iter().collect();

        let new: std::collections::HashMap<String, String> = [
            ("key".to_string(), "value".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&old, &new);
        // KEY removed, key added = 2 changes
        assert_eq!(changes.len(), 2, "Different case keys are different");
    }

    #[test]
    fn test_watch_state_empty_value() {
        let with_value: std::collections::HashMap<String, String> = [
            ("KEY".to_string(), "value".to_string()),
        ].into_iter().collect();

        let empty_value: std::collections::HashMap<String, String> = [
            ("KEY".to_string(), "".to_string()),
        ].into_iter().collect();

        let changes = detect_changes(&with_value, &empty_value);
        assert_eq!(changes.len(), 1, "Value to empty should be a change");
    }

    // ====== Type Validation Edge Cases ======

    #[test]
    fn test_date_format_various_valid() {
        // Regex validates format YYYY-MM-DD, not calendar logic
        let schema = make_schema(vec![("DATE", date_spec(true))]);
        let env = make_env(vec![("DATE", "2023-01-01")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty(), "Valid date format should pass");
    }

    #[test]
    fn test_date_format_with_invalid_separator() {
        // Using slashes instead of dashes
        let schema = make_schema(vec![("DATE", date_spec(true))]);
        let env = make_env(vec![("DATE", "2024/04/15")]);
        let errors = validate(&schema, &env);
        assert!(!errors.is_empty(), "Invalid separator should fail");
    }

    #[test]
    fn test_hostname_with_numbers() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "server01.example.com")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty(), "Hostname with numbers should be valid");
    }

    #[test]
    fn test_hostname_single_label() {
        let schema = make_schema(vec![("HOST", hostname_spec(true))]);
        let env = make_env(vec![("HOST", "myserver")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty(), "Single label hostname should be valid");
    }

    #[test]
    fn test_semver_prerelease_alpha() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "1.0.0-alpha")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty(), "Semver with alpha prerelease should be valid");
    }

    #[test]
    fn test_semver_prerelease_with_numbers() {
        let schema = make_schema(vec![("VERSION", semver_spec(true))]);
        let env = make_env(vec![("VERSION", "2.1.0-beta.1")]);
        let errors = validate(&schema, &env);
        assert!(errors.is_empty(), "Semver with numbered prerelease should be valid");
    }
}
