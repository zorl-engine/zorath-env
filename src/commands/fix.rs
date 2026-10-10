use crate::envfile;
use crate::errors::CliError;
use crate::schema::{load_schema_with_options, LoadOptions, Schema, VarSpec};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::secrets::is_sensitive_key;

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
/// Reject `--env` paths that look like traversal attempts. zenv fix
/// mutates the file in place and drops a backup beside it -- without
/// this check, `zenv fix --env "../../etc/.env"` becomes a generic
/// file-rewrite primitive.
///
/// Allows: plain `.env`, `.env.local`, `.env.production` (any `.env*`
/// basename), and absolute paths (user explicitly chose the location).
///
/// Rejects: relative paths containing `..` components that resolve outside
/// CWD, and basenames that don't match `.env*` (defends against
/// `zenv fix --env /home/victim/.ssh/config`).
fn validate_env_path(env_path: &str) -> Result<(), CliError> {
    let path = Path::new(env_path);

    // Basename gate: must look like a .env* file.
    let basename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::Input(format!("invalid --env path: {}", env_path)))?;
    if !(basename == ".env" || basename.starts_with(".env.")) {
        return Err(CliError::Input(format!(
            "--env basename must match '.env' or '.env.*', got '{}'",
            basename
        )));
    }

    // Containment check ONLY for relative paths with `..` traversal.
    // Absolute paths are user-chosen and need no traversal containment.
    use std::path::Component;
    let has_parent_dir = path.components().any(|c| c == Component::ParentDir);
    if has_parent_dir && !path.is_absolute() {
        let cwd = std::env::current_dir()
            .map_err(|e| CliError::Input(format!("cannot read current directory: {}", e)))?;
        let cwd_canon = fs::canonicalize(&cwd).unwrap_or(cwd.clone());
        let resolved = cwd.join(path);
        let mut existing = resolved.as_path();
        while !existing.exists() {
            match existing.parent() {
                Some(p) => existing = p,
                None => break,
            }
        }
        if let Ok(existing_canon) = fs::canonicalize(existing) {
            if !existing_canon.starts_with(&cwd_canon) {
                return Err(CliError::Input(format!(
                    "--env path '{}' escapes current directory via '..' traversal",
                    env_path
                )));
            }
        }
    }
    Ok(())
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run(
    env_path: &str,
    schema_path: &str,
    remove_unknown: bool,
    dry_run: bool,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    validate_env_path(env_path)?;
    // Load schema
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds,
    };
    let schema = load_schema_with_options(schema_path, &options)
        .map_err(|e| CliError::Schema(e.to_string()))?;

    // Read original file content
    let original_content = if Path::new(env_path).exists() {
        fs::read_to_string(env_path)
            .map_err(|e| CliError::Input(format!("failed to read {}: {}", env_path, e)))?
    } else {
        String::new()
    };

    // Parse env file
    let env_map = envfile::interpolate_env(envfile::parse_env_str(&original_content))
        .map_err(|e| CliError::Input(e.to_string()))?;

    // Analyze for fixes
    let analysis = analyze_fixes(&env_map, &original_content, &schema, remove_unknown);

    // Check if there's anything to fix
    if analysis.fixable.is_empty() && analysis.unfixable.is_empty() {
        println!("zenv fix: nothing to fix");
        return Ok(());
    }

    // Print dry run or apply fixes
    if dry_run {
        print_dry_run(&analysis, env_path, &schema);
    } else {
        apply_fixes(env_path, &original_content, &analysis).map_err(CliError::Input)?;
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
    super::check::validate_value(key, value, spec)
        .into_iter()
        .next()
}

/// Build a map of key -> line number from original content
fn build_line_map(content: &str) -> HashMap<String, usize> {
    envfile::parse_env_str_detailed(content).line_numbers
}

/// Print what would be done in dry-run mode
fn print_dry_run(analysis: &FixAnalysis, env_path: &str, schema: &Schema) {
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
                        let display_value = if schema
                            .get(key)
                            .is_some_and(|spec| spec.secret == Some(true))
                            || is_sensitive_key(key)
                            || crate::secrets::value_looks_secret(value)
                        {
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
fn apply_fixes(
    env_path: &str,
    original_content: &str,
    analysis: &FixAnalysis,
) -> Result<(), String> {
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

    // Remove complete parser-recognized assignments, including duplicates and
    // multiline bodies. A KEY= fragment inside another value is not a key.
    let (_, entries) = envfile::parse_env_with_entries(original_content);
    let removed_lines: std::collections::HashSet<usize> = entries
        .iter()
        .filter(|entry| keys_to_remove.contains(&entry.key))
        .flat_map(|entry| entry.start_line..=entry.end_line)
        .collect();
    let mut new_lines: Vec<String> = Vec::new();
    for (index, line) in original_content.lines().enumerate() {
        if !removed_lines.contains(&(index + 1)) {
            new_lines.push(line.to_string());
        }
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
            let formatted = format!("{}={}", key, envfile::format_env_value(value));
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

    // Atomic write with mode preservation: stages to a PID-tagged temp file
    // in the same directory, tightens perms to match the original .env (or
    // 0600 for new files) BEFORE rename, then renames atomically over the
    // target. Survives SIGKILL between stages -- the original .env is
    // replaced as a single filesystem op, never observed empty by readers.
    crate::remote::write_atomic_preserve_mode(Path::new(env_path), final_content.as_bytes(), 0o600)
        .map_err(|e| format!("failed to atomically write {}: {}", env_path, e))?;

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

/// Create a backup without replacing existing files. Staging starts private;
/// Unix backups preserve the original mode (or use 0600 if it is missing).
fn create_backup(env_path: &str, content: &str) -> Result<String, String> {
    use std::io::Write;
    let path = Path::new(env_path);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let create = || -> std::io::Result<tempfile::NamedTempFile> {
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(content.as_bytes())?;
        file.as_file().sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = match fs::metadata(path) {
                Ok(meta) => meta.permissions().mode() & 0o777,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0o600,
                Err(e) => return Err(e),
            };
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(mode))?;
        }
        Ok(file)
    };
    let mut staged = create().map_err(|e| format!("failed to create backup: {}", e))?;
    for counter in 0..=100 {
        let backup = if counter == 0 {
            format!("{}.backup", env_path)
        } else {
            format!("{}.backup.{}", env_path, counter)
        };
        match staged.persist_noclobber(&backup) {
            Ok(_) => return Ok(backup),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => staged = e.file,
            Err(e) => return Err(format!("failed to create backup: {}", e.error)),
        }
    }
    Err("too many backup files exist".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{VarSpec, VarType};
    use tempfile::tempdir;

    #[test]
    fn concurrent_backups_do_not_overwrite_each_other() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "original").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let content = format!("VALUE={i}");
                    barrier.wait();
                    let backup = create_backup(path.to_str().unwrap(), &content).unwrap();
                    assert_eq!(fs::read_to_string(&backup).unwrap(), content);
                    backup
                })
            })
            .collect();
        let names: std::collections::HashSet<_> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(names.len(), 8);
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 9);
    }

    #[cfg(unix)]
    #[test]
    fn backup_does_not_follow_existing_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        let other = dir.path().join("other");
        fs::write(&path, "secret").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&other, "untouched").unwrap();
        symlink(&other, dir.path().join(".env.backup")).unwrap();
        let backup = create_backup(path.to_str().unwrap(), "secret").unwrap();
        assert!(backup.ends_with(".backup.1"));
        assert_eq!(fs::read_to_string(other).unwrap(), "untouched");
        assert_eq!(
            fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

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
        assert!(envfile::needs_env_quotes("hello world"));
        assert!(envfile::needs_env_quotes("value#comment"));
        assert!(envfile::needs_env_quotes(""));
        assert!(!envfile::needs_env_quotes("simple"));
        assert!(!envfile::needs_env_quotes("123"));
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
            values: Some(vec![
                "dev".to_string(),
                "staging".to_string(),
                "prod".to_string(),
            ]),
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
        assert!(envfile::needs_env_quotes("value with space"));
        assert!(envfile::needs_env_quotes("value\twith\ttab"));
        assert!(envfile::needs_env_quotes("value#with#hash"));
        // Equals signs don't need quoting - they're fine in values
        assert!(!envfile::needs_env_quotes("value=with=equals"));
        assert!(!envfile::needs_env_quotes("simplevalue123"));
    }

    #[test]
    fn test_analyze_multiple_missing_required() {
        let env_map = HashMap::new();
        let mut schema = Schema::new();
        schema.insert(
            "KEY1".to_string(),
            VarSpec {
                var_type: VarType::String,
                required: true,
                default: Some(serde_json::json!("val1")),
                ..Default::default()
            },
        );
        schema.insert(
            "KEY2".to_string(),
            VarSpec {
                var_type: VarType::String,
                required: true,
                default: Some(serde_json::json!("val2")),
                ..Default::default()
            },
        );

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
        fs::write(&schema_path, r#"{"KNOWN": {"type": "string"}}"#).unwrap();

        let env_path = dir.path().join(".env");
        fs::write(&env_path, "KNOWN=value\nUNKNOWN=should_be_removed\n").unwrap();

        // With remove_unknown = true
        let result = run(
            env_path.to_str().unwrap(),
            schema_path.to_str().unwrap(),
            true,  // remove_unknown
            false, // not dry_run
            false,
            None,
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
        schema.insert(
            "OPTIONAL".to_string(),
            VarSpec {
                var_type: VarType::String,
                required: false,
                ..Default::default()
            },
        );

        let analysis = analyze_fixes(&env_map, "", &schema, false);

        // Optional var without default should not create a fix
        assert!(analysis.fixable.is_empty());
    }
}
