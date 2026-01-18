use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use url::Url;

use regex::Regex;

use crate::envfile;
use crate::schema::{self, LoadOptions, Schema, VarSpec, VarType};
use crate::secrets;

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

pub fn run(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
    watch: bool,
) -> Result<(), String> {
    if watch {
        run_watch_mode(env_path, schema_path, allow_missing_env, detect_secrets, no_cache)
    } else {
        run_once(env_path, schema_path, allow_missing_env, detect_secrets, no_cache)
    }
}

/// Run validation once and exit
fn run_once(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
) -> Result<(), String> {
    let options = LoadOptions { no_cache };
    let schema = schema::load_schema_with_options(schema_path, &options).map_err(|e| e.to_string())?;

    let resolved_path = resolve_env_file(env_path);
    let (env_map, raw_content): (HashMap<String, String>, Option<String>) = match &resolved_path {
        Some(resolved) => {
            if resolved != env_path {
                eprintln!("Note: Using {} (fallback)\n", resolved);
            }
            let content = fs::read_to_string(resolved).map_err(|e| e.to_string())?;
            let map = envfile::parse_env_file(resolved).map_err(|e| e.to_string())?;
            (map, Some(content))
        }
        None if allow_missing_env => (HashMap::new(), None),
        None => return Err(missing_env_error(env_path)),
    };

    // Interpolate variable references (${VAR} and $VAR)
    let env_map = envfile::interpolate_env(env_map).map_err(|e| e.to_string())?;

    let errors = validate(&schema, &env_map);

    // Check for secrets if flag is set
    let secret_warnings = if detect_secrets {
        if let Some(content) = &raw_content {
            secrets::detect_secrets(&env_map, content)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let has_errors = !errors.is_empty();
    let has_warnings = !secret_warnings.is_empty();

    if has_errors {
        // Count unknown keys for the tip
        let unknown_count = errors
            .iter()
            .filter(|e| e.contains("not in schema"))
            .count();

        eprintln!("Error: zenv check failed:\n");
        for e in &errors {
            eprintln!("- {e}");
        }

        // Suggest how to fix unknown keys
        if unknown_count > 0 {
            eprintln!("\nTip: {} unknown key(s) found in .env but not in schema.", unknown_count);
            eprintln!("  To add them: zenv init --example .env --schema {} (creates new schema)", schema_path);
            eprintln!("  Or manually add them to your schema file.");
        }
    }

    if has_warnings {
        if has_errors {
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

    if has_errors {
        return Err("validation failed".into());
    }

    if has_warnings {
        println!("\nzenv: OK (with {} secret warning(s))", secret_warnings.len());
    } else {
        println!("zenv: OK");
    }
    Ok(())
}

/// Run validation in watch mode with intelligent delta detection
fn run_watch_mode(
    env_path: &str,
    schema_path: &str,
    allow_missing_env: bool,
    detect_secrets: bool,
    no_cache: bool,
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
    let options = LoadOptions { no_cache };
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
            secrets::detect_secrets(&env_map, content)
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

/// Detect changes between old and new environment maps
fn detect_changes(old: &HashMap<String, String>, new: &HashMap<String, String>) -> Vec<EnvChange> {
    let mut changes = Vec::new();

    let old_keys: HashSet<&String> = old.keys().collect();
    let new_keys: HashSet<&String> = new.keys().collect();

    // Added keys
    for key in new_keys.difference(&old_keys) {
        changes.push(EnvChange {
            key: (*key).clone(),
            change_type: ChangeType::Added,
            new_value: new.get(*key).cloned(),
        });
    }

    // Removed keys
    for key in old_keys.difference(&new_keys) {
        changes.push(EnvChange {
            key: (*key).clone(),
            change_type: ChangeType::Removed,
            new_value: None,
        });
    }

    // Modified keys
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

    // Sort by key for consistent output
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

        let secret_warnings = secrets::detect_secrets(env_map, raw_content);
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
fn validate_single_key(_key: &str, value: &str, spec: &VarSpec) -> Result<String, String> {
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
                    match Regex::new(pattern) {
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
                Err(_) => Err(format!("expected int, got '{}'", truncate_value(value))),
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
                Err(_) => Err(format!("expected float, got '{}'", truncate_value(value))),
            }
        }
        VarType::Bool => {
            let v = value.to_lowercase();
            if matches!(v.as_str(), "true" | "false" | "1" | "0" | "yes" | "no") {
                Ok(format!("bool: {}", v))
            } else {
                Err(format!("expected bool, got '{}'", truncate_value(value)))
            }
        }
        VarType::Url => {
            if Url::parse(value).is_ok() {
                Ok("url".to_string())
            } else {
                Err(format!("expected url, got '{}'", truncate_value(value)))
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
