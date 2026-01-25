use crate::envfile;
use crate::schema::{load_schema_with_options, LoadOptions, Schema, VarSpec};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Import cached regex functions and utilities from check module
use super::check::{
    date_regex, email_regex, hostname_regex, ipv4_regex, ipv6_regex, is_sensitive_key,
    semver_regex, uuid_regex,
};

/// Actions that can be automatically fixed
#[derive(Debug, Clone)]
enum FixAction {
    /// Add a missing required variable
    AddMissing {
        key: String,
        value: String,
        has_default: bool,
    },
    /// Remove an unknown key (not in schema)
    RemoveUnknown { key: String, line_num: usize },
}

/// Result of analyzing an env file for fixes
struct FixAnalysis {
    /// Actions that can be automatically applied
    fixable: Vec<FixAction>,
    /// Issues that require manual intervention
    unfixable: Vec<String>,
}

/// Run the fix command
pub fn run(
    env_path: &str,
    schema_path: &str,
    remove_unknown: bool,
    dry_run: bool,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
) -> Result<(), String> {
    // Load schema
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds: None,
    };
    let schema = load_schema_with_options(schema_path, &options).map_err(|e| e.to_string())?;

    // Read original file content
    let original_content = if Path::new(env_path).exists() {
        fs::read_to_string(env_path).map_err(|e| format!("failed to read {}: {}", env_path, e))?
    } else {
        String::new()
    };

    // Parse env file
    let env_map = envfile::parse_env_str(&original_content);

    // Analyze for fixes
    let analysis = analyze_fixes(&env_map, &original_content, &schema, remove_unknown);

    // Check if there's anything to fix
    if analysis.fixable.is_empty() && analysis.unfixable.is_empty() {
        println!("zenv fix: nothing to fix");
        return Ok(());
    }

    // Print dry run or apply fixes
    if dry_run {
        print_dry_run(&analysis, env_path);
    } else {
        apply_fixes(env_path, &original_content, &analysis)?;
    }

    // Always print unfixable issues
    if !analysis.unfixable.is_empty() {
        println!();
        println!("Unfixable (manual intervention required):");
        for issue in &analysis.unfixable {
            println!("  - {}", issue);
        }
    }

    Ok(())
}

/// Analyze the env file and schema to determine what can be fixed
fn analyze_fixes(
    env_map: &HashMap<String, String>,
    original_content: &str,
    schema: &Schema,
    remove_unknown: bool,
) -> FixAnalysis {
    let mut fixable = Vec::new();
    let mut unfixable = Vec::new();

    // Check for missing required variables
    for (key, spec) in schema.iter() {
        if !env_map.contains_key(key) {
            if spec.required {
                let (value, has_default) = get_default_value(spec);
                fixable.push(FixAction::AddMissing {
                    key: key.clone(),
                    value,
                    has_default,
                });
            }
        } else {
            // Variable exists - check for type/validation errors
            let value = env_map.get(key).unwrap();
            if let Some(error) = check_value_error(key, value, spec) {
                unfixable.push(error);
            }
        }
    }

    // Check for unknown keys (if --remove-unknown)
    if remove_unknown {
        let line_map = build_line_map(original_content);
        for key in env_map.keys() {
            if !schema.contains_key(key) {
                let line_num = line_map.get(key).copied().unwrap_or(0);
                fixable.push(FixAction::RemoveUnknown {
                    key: key.clone(),
                    line_num,
                });
            }
        }
    }

    // Sort fixable actions for consistent output
    fixable.sort_by(|a, b| {
        let key_a = match a {
            FixAction::AddMissing { key, .. } => key,
            FixAction::RemoveUnknown { key, .. } => key,
        };
        let key_b = match b {
            FixAction::AddMissing { key, .. } => key,
            FixAction::RemoveUnknown { key, .. } => key,
        };
        key_a.cmp(key_b)
    });

    FixAnalysis { fixable, unfixable }
}

/// Get the default value for a variable, or empty string if none
fn get_default_value(spec: &VarSpec) -> (String, bool) {
    match &spec.default {
        Some(value) => {
            let str_value = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };
            (str_value, true)
        }
        None => (String::new(), false),
    }
}

/// Check if a value has validation errors (returns error message if so)
fn check_value_error(key: &str, value: &str, spec: &VarSpec) -> Option<String> {
    use crate::schema::VarType;

    match spec.var_type {
        VarType::Int => {
            if value.parse::<i64>().is_err() {
                return Some(format!("{}: expected int, got '{}'", key, value));
            }
        }
        VarType::Float => {
            if value.parse::<f64>().is_err() {
                return Some(format!("{}: expected float, got '{}'", key, value));
            }
        }
        VarType::Bool => {
            let lower = value.to_lowercase();
            if !matches!(
                lower.as_str(),
                "true" | "false" | "1" | "0" | "yes" | "no"
            ) {
                return Some(format!(
                    "{}: expected bool (true/false/1/0/yes/no), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Url => {
            if url::Url::parse(value).is_err() {
                return Some(format!("{}: expected url, got '{}'", key, value));
            }
        }
        VarType::Enum => {
            if let Some(ref values) = spec.values {
                if !values.contains(&value.to_string()) {
                    return Some(format!(
                        "{}: expected one of [{}], got '{}'",
                        key,
                        values.join(", "),
                        value
                    ));
                }
            }
        }
        VarType::String => {
            // String type - check validation rules if present
            if let Some(ref validate) = spec.validate {
                if let Some(min_len) = validate.min_length {
                    if value.len() < min_len {
                        return Some(format!(
                            "{}: length {} is less than minimum {}",
                            key,
                            value.len(),
                            min_len
                        ));
                    }
                }
                if let Some(max_len) = validate.max_length {
                    if value.len() > max_len {
                        return Some(format!(
                            "{}: length {} exceeds maximum {}",
                            key,
                            value.len(),
                            max_len
                        ));
                    }
                }
                if let Some(ref pattern) = validate.pattern {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        if !re.is_match(value) {
                            return Some(format!(
                                "{}: value '{}' does not match pattern '{}'",
                                key, value, pattern
                            ));
                        }
                    }
                }
            }
        }
        VarType::Uuid => {
            // UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (using cached regex)
            if !uuid_regex().is_match(value) {
                return Some(format!(
                    "{}: expected uuid (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Email => {
            // Simplified RFC 5322 email (using cached regex)
            if !email_regex().is_match(value) {
                return Some(format!(
                    "{}: expected email (user@domain.tld), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Ipv4 => {
            // IPv4 format with octet range check (using cached regex)
            if let Some(caps) = ipv4_regex().captures(value) {
                for i in 1..=4 {
                    if let Some(m) = caps.get(i) {
                        if let Ok(octet) = m.as_str().parse::<u16>() {
                            if octet > 255 {
                                return Some(format!(
                                    "{}: expected ipv4 (x.x.x.x where x is 0-255), got '{}'",
                                    key, value
                                ));
                            }
                        }
                    }
                }
            } else {
                return Some(format!(
                    "{}: expected ipv4 (x.x.x.x where x is 0-255), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Semver => {
            // Semantic versioning format: x.y.z[-prerelease][+build] (using cached regex)
            if !semver_regex().is_match(value) {
                return Some(format!(
                    "{}: expected semver (x.y.z[-prerelease][+build]), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Ipv6 => {
            // IPv6 format (using cached regex)
            if !ipv6_regex().is_match(value) {
                return Some(format!(
                    "{}: expected ipv6 address, got '{}'",
                    key, value
                ));
            }
        }
        VarType::Port => {
            // Port: 1-65535
            match value.parse::<u16>() {
                Ok(port) if port >= 1 => {}
                Ok(_) => {
                    return Some(format!("{}: port must be between 1 and 65535", key));
                }
                Err(_) => {
                    return Some(format!("{}: expected port (1-65535), got '{}'", key, value));
                }
            }
        }
        VarType::Date => {
            // ISO 8601 date format: YYYY-MM-DD (using cached regex)
            if !date_regex().is_match(value) {
                return Some(format!(
                    "{}: expected date (YYYY-MM-DD), got '{}'",
                    key, value
                ));
            }
        }
        VarType::Hostname => {
            // RFC 1123 hostname (using cached regex)
            if value.len() > 253 || !hostname_regex().is_match(value) {
                return Some(format!(
                    "{}: expected hostname (RFC 1123), got '{}'",
                    key, value
                ));
            }
        }
    }

    // Check numeric validation rules
    if let Some(ref validate) = spec.validate {
        if let Ok(n) = value.parse::<i64>() {
            if let Some(min) = validate.min {
                if n < min {
                    return Some(format!("{}: value {} is less than minimum {}", key, n, min));
                }
            }
            if let Some(max) = validate.max {
                if n > max {
                    return Some(format!("{}: value {} exceeds maximum {}", key, n, max));
                }
            }
        }
        if let Ok(f) = value.parse::<f64>() {
            if let Some(min_value) = validate.min_value {
                if f < min_value {
                    return Some(format!(
                        "{}: value {} is less than minimum {}",
                        key, f, min_value
                    ));
                }
            }
            if let Some(max_value) = validate.max_value {
                if f > max_value {
                    return Some(format!(
                        "{}: value {} exceeds maximum {}",
                        key, f, max_value
                    ));
                }
            }
        }
    }

    None
}

/// Build a map of key -> line number from original content
fn build_line_map(content: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Handle export prefix
        let line_content = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some(eq_pos) = line_content.find('=') {
            let key = line_content[..eq_pos].trim();
            map.insert(key.to_string(), line_num + 1);
        }
    }
    map
}

/// Print what would be done in dry-run mode
fn print_dry_run(analysis: &FixAnalysis, env_path: &str) {
    println!("zenv fix (dry run)");
    println!();

    if !analysis.fixable.is_empty() {
        let adds: Vec<_> = analysis
            .fixable
            .iter()
            .filter(|a| matches!(a, FixAction::AddMissing { .. }))
            .collect();
        let removes: Vec<_> = analysis
            .fixable
            .iter()
            .filter(|a| matches!(a, FixAction::RemoveUnknown { .. }))
            .collect();

        if !adds.is_empty() {
            println!("Will add:");
            for action in adds {
                if let FixAction::AddMissing {
                    key,
                    value,
                    has_default,
                } = action
                {
                    if *has_default {
                        // Mask sensitive values for safe display
                        let display_value = if is_sensitive_key(key) {
                            "***MASKED***".to_string()
                        } else {
                            value.clone()
                        };
                        println!("  + {}={} (schema default)", key, display_value);
                    } else {
                        println!("  + {}= (empty placeholder)", key);
                    }
                }
            }
            println!();
        }

        if !removes.is_empty() {
            println!("Will remove:");
            for action in removes {
                if let FixAction::RemoveUnknown { key, line_num } = action {
                    println!("  - {} (line {})", key, line_num);
                }
            }
            println!();
        }
    }

    println!("Run without --dry-run to apply fixes.");
    println!("Backup will be created at: {}.backup", env_path);
}

/// Apply fixes to the env file
fn apply_fixes(env_path: &str, original_content: &str, analysis: &FixAnalysis) -> Result<(), String> {
    if analysis.fixable.is_empty() {
        return Ok(());
    }

    // Create backup first
    let backup_path = create_backup(env_path, original_content)?;
    println!("Created backup: {}", backup_path);

    // Build set of keys to remove
    let keys_to_remove: std::collections::HashSet<String> = analysis
        .fixable
        .iter()
        .filter_map(|a| {
            if let FixAction::RemoveUnknown { key, .. } = a {
                Some(key.clone())
            } else {
                None
            }
        })
        .collect();

    // Process original content line by line, removing marked keys
    let mut new_lines: Vec<String> = Vec::new();
    for line in original_content.lines() {
        let trimmed = line.trim();

        // Keep empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            continue;
        }

        // Check if this line should be removed
        let line_content = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        if let Some(eq_pos) = line_content.find('=') {
            let key = line_content[..eq_pos].trim();
            if keys_to_remove.contains(key) {
                continue; // Skip this line
            }
        }

        new_lines.push(line.to_string());
    }

    // Add missing variables at the end
    let adds: Vec<_> = analysis
        .fixable
        .iter()
        .filter_map(|a| {
            if let FixAction::AddMissing { key, value, .. } = a {
                Some((key, value))
            } else {
                None
            }
        })
        .collect();

    if !adds.is_empty() {
        // Add blank line before new variables if file doesn't end with one
        if !new_lines.is_empty() && !new_lines.last().map(|l| l.is_empty()).unwrap_or(true) {
            new_lines.push(String::new());
        }

        // Add comment header for auto-added variables
        new_lines.push("# Added by zenv fix".to_string());

        for (key, value) in &adds {
            let formatted = if value.is_empty() || needs_quotes(value) {
                format!("{}=\"{}\"", key, value)
            } else {
                format!("{}={}", key, value)
            };
            new_lines.push(formatted);
        }
    }

    // Write new content
    let new_content = new_lines.join("\n");
    // Ensure file ends with newline
    let final_content = if new_content.ends_with('\n') {
        new_content
    } else {
        format!("{}\n", new_content)
    };

    fs::write(env_path, &final_content)
        .map_err(|e| format!("failed to write {}: {}", env_path, e))?;

    // Print summary
    println!();
    println!("zenv fix: applied {} fix(es)", analysis.fixable.len());

    let added_count = analysis
        .fixable
        .iter()
        .filter(|a| matches!(a, FixAction::AddMissing { .. }))
        .count();
    let removed_count = analysis
        .fixable
        .iter()
        .filter(|a| matches!(a, FixAction::RemoveUnknown { .. }))
        .count();

    if added_count > 0 {
        println!("  + {} variable(s) added", added_count);
    }
    if removed_count > 0 {
        println!("  - {} variable(s) removed", removed_count);
    }

    Ok(())
}

/// Create a backup of the original file
fn create_backup(env_path: &str, content: &str) -> Result<String, String> {
    let base_backup = format!("{}.backup", env_path);

    // Find available backup filename
    let backup_path = if !Path::new(&base_backup).exists() {
        base_backup
    } else {
        let mut counter = 1;
        loop {
            let numbered = format!("{}.backup.{}", env_path, counter);
            if !Path::new(&numbered).exists() {
                break numbered;
            }
            counter += 1;
            if counter > 100 {
                return Err("too many backup files exist".to_string());
            }
        }
    };

    fs::write(&backup_path, content)
        .map_err(|e| format!("failed to create backup: {}", e))?;

    Ok(backup_path)
}

/// Check if a value needs quotes
fn needs_quotes(value: &str) -> bool {
    value.contains(' ')
        || value.contains('#')
        || value.contains('"')
        || value.contains('\'')
        || value.contains('\n')
        || value.contains('\t')
        || value.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{VarSpec, VarType};
    use tempfile::tempdir;

    fn make_schema(vars: Vec<(&str, bool, Option<&str>)>) -> Schema {
        let mut schema = Schema::new();
        for (key, required, default) in vars {
            schema.insert(
                key.to_string(),
                VarSpec {
                    var_type: VarType::String,
                    required,
                    default: default.map(|d| serde_json::json!(d)),
                    ..Default::default()
                },
            );
        }
        schema
    }

    #[test]
    fn test_analyze_missing_required_with_default() {
        let env_map = HashMap::new();
        let schema = make_schema(vec![("API_KEY", true, Some("default_key"))]);

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        assert_eq!(analysis.fixable.len(), 1);
        if let FixAction::AddMissing {
            key,
            value,
            has_default,
        } = &analysis.fixable[0]
        {
            assert_eq!(key, "API_KEY");
            assert_eq!(value, "default_key");
            assert!(*has_default);
        } else {
            panic!("Expected AddMissing action");
        }
    }

    #[test]
    fn test_analyze_missing_required_no_default() {
        let env_map = HashMap::new();
        let schema = make_schema(vec![("SECRET", true, None)]);

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        assert_eq!(analysis.fixable.len(), 1);
        if let FixAction::AddMissing {
            key,
            value,
            has_default,
        } = &analysis.fixable[0]
        {
            assert_eq!(key, "SECRET");
            assert_eq!(value, "");
            assert!(!*has_default);
        } else {
            panic!("Expected AddMissing action");
        }
    }

    #[test]
    fn test_analyze_optional_not_added() {
        let env_map = HashMap::new();
        let schema = make_schema(vec![("OPTIONAL_VAR", false, Some("default"))]);

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        assert!(analysis.fixable.is_empty());
    }

    #[test]
    fn test_analyze_unknown_key_with_flag() {
        let mut env_map = HashMap::new();
        env_map.insert("UNKNOWN".to_string(), "value".to_string());
        let content = "UNKNOWN=value";
        let schema = make_schema(vec![]);

        let analysis = analyze_fixes(&env_map, content, &schema, true);

        assert_eq!(analysis.fixable.len(), 1);
        if let FixAction::RemoveUnknown { key, .. } = &analysis.fixable[0] {
            assert_eq!(key, "UNKNOWN");
        } else {
            panic!("Expected RemoveUnknown action");
        }
    }

    #[test]
    fn test_analyze_unknown_key_without_flag() {
        let mut env_map = HashMap::new();
        env_map.insert("UNKNOWN".to_string(), "value".to_string());
        let schema = make_schema(vec![]);

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        assert!(analysis.fixable.is_empty());
    }

    #[test]
    fn test_check_value_error_int() {
        let spec = VarSpec {
            var_type: VarType::Int,
            ..Default::default()
        };

        assert!(check_value_error("PORT", "8080", &spec).is_none());
        assert!(check_value_error("PORT", "abc", &spec).is_some());
    }

    #[test]
    fn test_check_value_error_bool() {
        let spec = VarSpec {
            var_type: VarType::Bool,
            ..Default::default()
        };

        assert!(check_value_error("DEBUG", "true", &spec).is_none());
        assert!(check_value_error("DEBUG", "false", &spec).is_none());
        assert!(check_value_error("DEBUG", "1", &spec).is_none());
        assert!(check_value_error("DEBUG", "maybe", &spec).is_some());
    }

    #[test]
    fn test_build_line_map() {
        let content = "# Comment\nFOO=bar\n\nBAZ=qux";
        let map = build_line_map(content);

        assert_eq!(map.get("FOO"), Some(&2));
        assert_eq!(map.get("BAZ"), Some(&4));
    }

    #[test]
    fn test_needs_quotes() {
        assert!(needs_quotes("hello world"));
        assert!(needs_quotes("value#comment"));
        assert!(needs_quotes(""));
        assert!(!needs_quotes("simple"));
        assert!(!needs_quotes("123"));
    }

    #[test]
    fn test_backup_creation() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "FOO=bar").unwrap();

        let backup = create_backup(env_path.to_str().unwrap(), "FOO=bar").unwrap();
        assert!(Path::new(&backup).exists());
        assert!(backup.ends_with(".backup"));
    }

    #[test]
    fn test_backup_numbered() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "FOO=bar").unwrap();

        // Create first backup
        let backup1 = create_backup(env_path.to_str().unwrap(), "FOO=bar").unwrap();
        // Create second backup
        let backup2 = create_backup(env_path.to_str().unwrap(), "FOO=bar").unwrap();

        assert!(backup1.ends_with(".backup"));
        assert!(backup2.ends_with(".backup.1"));
    }

    #[test]
    fn test_run_dry_run() {
        let dir = tempdir().unwrap();

        let schema_path = dir.path().join("schema.json");
        fs::write(
            &schema_path,
            r#"{"API_KEY": {"type": "string", "required": true, "default": "test_key"}}"#,
        )
        .unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "").unwrap();

        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            false,
            true, // dry_run
            false,
            None,
            None,
        );

        assert!(result.is_ok());
        // File should be unchanged in dry-run
        assert_eq!(fs::read_to_string(&env_path).unwrap(), "");
    }

    #[test]
    fn test_run_applies_fixes() {
        let dir = tempdir().unwrap();

        let schema_path = dir.path().join("schema.json");
        fs::write(
            &schema_path,
            r#"{"API_KEY": {"type": "string", "required": true, "default": "test_key"}}"#,
        )
        .unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "EXISTING=value\n").unwrap();

        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            false,
            false, // not dry_run
            false,
            None,
            None,
        );

        assert!(result.is_ok());

        let content = fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("API_KEY=test_key"));
        assert!(content.contains("EXISTING=value"));

        // Backup should exist
        let backup_path = format!("{}.backup", env_path.to_str().unwrap());
        assert!(Path::new(&backup_path).exists());
    }

    #[test]
    fn test_check_value_error_url() {
        let spec = VarSpec {
            var_type: VarType::Url,
            ..Default::default()
        };

        assert!(check_value_error("API_URL", "https://example.com", &spec).is_none());
        assert!(check_value_error("API_URL", "http://localhost:8080", &spec).is_none());
        assert!(check_value_error("API_URL", "not-a-url", &spec).is_some());
    }

    #[test]
    fn test_check_value_error_enum() {
        let spec = VarSpec {
            var_type: VarType::Enum,
            values: Some(vec!["dev".to_string(), "staging".to_string(), "prod".to_string()]),
            ..Default::default()
        };

        assert!(check_value_error("ENV", "dev", &spec).is_none());
        assert!(check_value_error("ENV", "staging", &spec).is_none());
        assert!(check_value_error("ENV", "invalid", &spec).is_some());
    }

    #[test]
    fn test_check_value_error_float() {
        let spec = VarSpec {
            var_type: VarType::Float,
            ..Default::default()
        };

        assert!(check_value_error("RATE", "3.14", &spec).is_none());
        assert!(check_value_error("RATE", "-1.5", &spec).is_none());
        assert!(check_value_error("RATE", "not-a-float", &spec).is_some());
    }

    #[test]
    fn test_build_line_map_with_multiline() {
        let content = "FOO=bar\nMULTI=\"line1\nline2\"\nBAZ=qux";
        let map = build_line_map(content);

        assert_eq!(map.get("FOO"), Some(&1));
        assert_eq!(map.get("MULTI"), Some(&2));
        assert_eq!(map.get("BAZ"), Some(&4));
    }

    #[test]
    fn test_build_line_map_with_export() {
        let content = "export FOO=bar\nexport BAZ=qux";
        let map = build_line_map(content);

        assert_eq!(map.get("FOO"), Some(&1));
        assert_eq!(map.get("BAZ"), Some(&2));
    }

    #[test]
    fn test_needs_quotes_with_special_chars() {
        assert!(needs_quotes("value with space"));
        assert!(needs_quotes("value\twith\ttab"));
        assert!(needs_quotes("value#with#hash"));
        // Equals signs don't need quoting - they're fine in values
        assert!(!needs_quotes("value=with=equals"));
        assert!(!needs_quotes("simplevalue123"));
    }

    #[test]
    fn test_analyze_multiple_missing_required() {
        let env_map = HashMap::new();
        let mut schema = Schema::new();
        schema.insert("KEY1".to_string(), VarSpec {
            var_type: VarType::String,
            required: true,
            default: Some(serde_json::json!("val1")),
            ..Default::default()
        });
        schema.insert("KEY2".to_string(), VarSpec {
            var_type: VarType::String,
            required: true,
            default: Some(serde_json::json!("val2")),
            ..Default::default()
        });

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        assert_eq!(analysis.fixable.len(), 2);
    }

    // ====== Fix Command Edge Cases ======

    #[test]
    fn test_fix_with_sensitive_key_default() {
        // Test that sensitive keys like API_KEY work correctly
        let dir = tempdir().unwrap();

        let schema_path = dir.path().join("schema.json");
        fs::write(
            &schema_path,
            r#"{"API_SECRET": {"type": "string", "required": true, "default": "secret_value_123"}}"#,
        )
        .unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "").unwrap();

        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            false,
            true, // dry_run
            false,
            None,
            None,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_fix_preserves_existing_lines() {
        let dir = tempdir().unwrap();

        let schema_path = dir.path().join("schema.json");
        fs::write(
            &schema_path,
            r#"{
                "EXISTING": {"type": "string"},
                "NEW_KEY": {"type": "string", "required": true, "default": "new_value"}
            }"#,
        )
        .unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "EXISTING=keep_this\n# Comment to preserve\n").unwrap();

        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            false,
            false, // not dry_run
            false,
            None,
            None,
        );

        assert!(result.is_ok());

        let content = fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("EXISTING=keep_this"));
        assert!(content.contains("NEW_KEY=new_value"));
    }

    #[test]
    fn test_fix_remove_unknown_flag() {
        let dir = tempdir().unwrap();

        let schema_path = dir.path().join("schema.json");
        fs::write(
            &schema_path,
            r#"{"KNOWN": {"type": "string"}}"#,
        )
        .unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "KNOWN=value\nUNKNOWN=should_be_removed\n").unwrap();

        // With remove_unknown = true
        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            true, // remove_unknown
            false, // not dry_run
            false,
            None,
            None,
        );

        assert!(result.is_ok());

        let content = fs::read_to_string(&env_path).unwrap();
        assert!(content.contains("KNOWN=value"));
        assert!(!content.contains("UNKNOWN"));
    }

    #[test]
    fn test_fix_analysis_empty_env_file() {
        let env_map = HashMap::new();
        let mut schema = Schema::new();
        schema.insert("OPTIONAL".to_string(), VarSpec {
            var_type: VarType::String,
            required: false,
            ..Default::default()
        });

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        // Optional var without default should not create a fix
        assert!(analysis.fixable.is_empty());
    }
}
