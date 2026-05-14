use std::collections::{BTreeMap, BTreeSet};

use crate::envfile;
use crate::errors::CliError;
use crate::schema::{self, LoadOptions};
use crate::secrets::{is_sensitive_key, truncate_value_for_display, value_looks_secret};
use crate::suggestions::find_closest_match;
use serde::Serialize;

/// Mask sensitive values for the JSON diff output. Returns the literal
/// `***MASKED***` for keys that look sensitive OR values that look secret
/// (URL with embedded password, Slack/Discord webhook URL); otherwise
/// returns the full value untruncated. JSON consumers are programmatic
/// (CI scripts, jq pipelines) and rely on the full value for non-sensitive
/// keys -- truncation belongs to the text output path's
/// `truncate_value_for_display`. The value-aware check closes the gap
/// where the old key-only version leaked DATABASE_URL-style passwords
/// behind innocuous key names.
fn mask_for_diff(key: &str, value: &str) -> String {
    if is_sensitive_key(key) || value_looks_secret(value) {
        "***MASKED***".to_string()
    } else {
        value.to_string()
    }
}

/// JSON output structure for diff command
#[derive(Serialize)]
struct DiffOutput {
    file_a: String,
    file_b: String,
    only_in_a: Vec<KeyValue>,
    only_in_b: Vec<KeyValue>,
    different_values: Vec<ValueDiff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_compliance: Option<SchemaCompliance>,
    identical: bool,
}

#[derive(Serialize)]
struct KeyValue {
    key: String,
    value: String,
}

#[derive(Serialize)]
struct ValueDiff {
    key: String,
    value_a: String,
    value_b: String,
}

#[derive(Serialize)]
struct SchemaCompliance {
    schema_path: String,
    file_a: FileCompliance,
    file_b: FileCompliance,
}

#[derive(Serialize)]
struct FileCompliance {
    missing_required: Vec<String>,
    unknown_keys: Vec<String>,
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run(
    env_a: &str,
    env_b: &str,
    schema_path: Option<&str>,
    format: &str,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    // Parse both env files
    let map_a = envfile::parse_env_file(env_a)
        .map_err(|e| CliError::Input(format!("Error reading {}: {}", env_a, e)))?;
    let map_b = envfile::parse_env_file(env_b)
        .map_err(|e| CliError::Input(format!("Error reading {}: {}", env_b, e)))?;

    // Convert to BTreeMap for sorted output
    let map_a: BTreeMap<String, String> = map_a.into_iter().collect();
    let map_b: BTreeMap<String, String> = map_b.into_iter().collect();

    let keys_a: BTreeSet<&String> = map_a.keys().collect();
    let keys_b: BTreeSet<&String> = map_b.keys().collect();

    // Find differences
    let only_in_a: Vec<&String> = keys_a.difference(&keys_b).copied().collect();
    let only_in_b: Vec<&String> = keys_b.difference(&keys_a).copied().collect();
    let in_both: Vec<&String> = keys_a.intersection(&keys_b).copied().collect();

    // Find values that differ
    let mut different_values: Vec<(&String, &String, &String)> = Vec::new();
    for key in &in_both {
        let val_a = map_a.get(*key).unwrap();
        let val_b = map_b.get(*key).unwrap();
        if val_a != val_b {
            different_values.push((key, val_a, val_b));
        }
    }

    // Handle JSON output format
    if format == "json" {
        return output_json(
            env_a,
            env_b,
            &map_a,
            &map_b,
            &only_in_a,
            &only_in_b,
            &different_values,
            schema_path,
            no_cache,
            verify_hash,
            ca_cert,
            rate_limit_seconds,
        )
        .map_err(CliError::Input);
    }

    if format != "text" {
        return Err(CliError::Input(format!(
            "unknown format '{}'. Use 'text' or 'json'",
            format
        )));
    }

    // Print header
    println!("Comparing {} vs {}\n", env_a, env_b);

    let mut has_diff = false;

    // Variables only in A
    if !only_in_a.is_empty() {
        has_diff = true;
        println!("Only in {}:", env_a);
        for key in &only_in_a {
            let val = map_a.get(*key).unwrap();
            println!("  + {}={}", key, truncate_value_for_display(key, val, 50));
        }
        println!();
    }

    // Variables only in B
    if !only_in_b.is_empty() {
        has_diff = true;
        println!("Only in {}:", env_b);
        for key in &only_in_b {
            let val = map_b.get(*key).unwrap();
            println!("  + {}={}", key, truncate_value_for_display(key, val, 50));
        }
        println!();
    }

    // Check for potential typos between only_in_a and only_in_b
    if !only_in_a.is_empty() && !only_in_b.is_empty() {
        let mut typo_suggestions: Vec<(String, String)> = Vec::new();

        // For each key in A, check if there's a similar key in B
        let keys_b_strings: Vec<String> = only_in_b.iter().map(|k| (*k).clone()).collect();
        for key_a in &only_in_a {
            if let Some((match_key, distance)) =
                find_closest_match(key_a, keys_b_strings.iter().map(|s| s.as_str()), 3)
            {
                // Only suggest if the distance is small relative to key length
                if distance <= key_a.len() / 3 + 1 {
                    typo_suggestions.push(((*key_a).clone(), match_key.to_string()));
                }
            }
        }

        if !typo_suggestions.is_empty() {
            println!("Possible typos:");
            for (key_a, key_b) in &typo_suggestions {
                println!("  {} (in {}) <-> {} (in {})", key_a, env_a, key_b, env_b);
            }
            println!();
        }
    }

    // Variables with different values
    if !different_values.is_empty() {
        has_diff = true;
        println!("Different values:");
        for (key, val_a, val_b) in &different_values {
            println!("  {}:", key);
            println!(
                "    {} -> {}",
                truncate_value_for_display(key, val_a, 40),
                truncate_value_for_display(key, val_b, 40)
            );
        }
        println!();
    }

    // Schema compliance check if schema provided
    if let Some(schema_path) = schema_path {
        let options = LoadOptions {
            no_cache,
            verify_hash: verify_hash.map(|s| s.to_string()),
            ca_cert: ca_cert.map(|s| s.to_string()),
            rate_limit_seconds,
        };
        match schema::load_schema_with_options(schema_path, &options) {
            Ok(schema) => {
                println!("Schema compliance ({}):", schema_path);

                // Check A against schema
                let missing_a = check_missing_required(&map_a, &schema);
                let unknown_a = check_unknown_keys(&map_a, &schema);

                // Check B against schema
                let missing_b = check_missing_required(&map_b, &schema);
                let unknown_b = check_unknown_keys(&map_b, &schema);

                // Report A
                print!("  {}: ", env_a);
                if missing_a.is_empty() && unknown_a.is_empty() {
                    println!("OK");
                } else {
                    let mut issues = Vec::new();
                    if !missing_a.is_empty() {
                        issues.push(format!("{} missing required", missing_a.len()));
                    }
                    if !unknown_a.is_empty() {
                        issues.push(format!("{} unknown", unknown_a.len()));
                    }
                    println!("{}", issues.join(", "));
                }

                // Report B
                print!("  {}: ", env_b);
                if missing_b.is_empty() && unknown_b.is_empty() {
                    println!("OK");
                } else {
                    let mut issues = Vec::new();
                    if !missing_b.is_empty() {
                        issues.push(format!("{} missing required", missing_b.len()));
                    }
                    if !unknown_b.is_empty() {
                        issues.push(format!("{} unknown", unknown_b.len()));
                    }
                    println!("{}", issues.join(", "));
                }
                println!();
            }
            Err(e) => {
                eprintln!("Warning: Could not load schema: {}", e);
            }
        }
    }

    if !has_diff {
        println!("Files are identical.");
    }

    Ok(())
}

/// Check for missing required variables
fn check_missing_required(
    env_map: &BTreeMap<String, String>,
    schema: &schema::Schema,
) -> Vec<String> {
    let mut missing = Vec::new();
    for (key, spec) in schema.iter() {
        if spec.required && spec.default.is_none() && !env_map.contains_key(key) {
            missing.push(key.clone());
        }
    }
    missing
}

/// Check for unknown keys not in schema
fn check_unknown_keys(env_map: &BTreeMap<String, String>, schema: &schema::Schema) -> Vec<String> {
    let mut unknown = Vec::new();
    for key in env_map.keys() {
        if !schema.contains_key(key) {
            unknown.push(key.clone());
        }
    }
    unknown
}

/// Output diff results as JSON
#[allow(clippy::too_many_arguments)]
fn output_json(
    env_a: &str,
    env_b: &str,
    map_a: &BTreeMap<String, String>,
    map_b: &BTreeMap<String, String>,
    only_in_a: &[&String],
    only_in_b: &[&String],
    different_values: &[(&String, &String, &String)],
    schema_path: Option<&str>,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), String> {
    let has_diff = !only_in_a.is_empty() || !only_in_b.is_empty() || !different_values.is_empty();

    // Build schema compliance if schema provided
    let schema_compliance = if let Some(schema_path) = schema_path {
        let options = LoadOptions {
            no_cache,
            verify_hash: verify_hash.map(|s| s.to_string()),
            ca_cert: ca_cert.map(|s| s.to_string()),
            rate_limit_seconds,
        };
        match schema::load_schema_with_options(schema_path, &options) {
            Ok(schema) => Some(SchemaCompliance {
                schema_path: schema_path.to_string(),
                file_a: FileCompliance {
                    missing_required: check_missing_required(map_a, &schema),
                    unknown_keys: check_unknown_keys(map_a, &schema),
                },
                file_b: FileCompliance {
                    missing_required: check_missing_required(map_b, &schema),
                    unknown_keys: check_unknown_keys(map_b, &schema),
                },
            }),
            Err(_) => None,
        }
    } else {
        None
    };

    let output = DiffOutput {
        file_a: env_a.to_string(),
        file_b: env_b.to_string(),
        only_in_a: only_in_a
            .iter()
            .map(|k| KeyValue {
                key: (*k).clone(),
                value: mask_for_diff(k, map_a.get(*k).unwrap()),
            })
            .collect(),
        only_in_b: only_in_b
            .iter()
            .map(|k| KeyValue {
                key: (*k).clone(),
                value: mask_for_diff(k, map_b.get(*k).unwrap()),
            })
            .collect(),
        different_values: different_values
            .iter()
            .map(|(k, va, vb)| ValueDiff {
                key: (*k).clone(),
                value_a: mask_for_diff(k, va),
                value_b: mask_for_diff(k, vb),
            })
            .collect(),
        schema_compliance,
        identical: !has_diff,
    };

    let json = serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?;
    println!("{}", json);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_env(dir: &TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_mask_for_diff_masks_sensitive() {
        assert_eq!(mask_for_diff("API_KEY", "sk_live_abc"), "***MASKED***");
        assert_eq!(mask_for_diff("PORT", "3000"), "3000");
    }

    #[test]
    fn test_mask_for_diff_masks_url_password_with_innocuous_key() {
        // Regression guard for the C1 finding in audit-2026-05-14: the JSON
        // output path used to mask only on is_sensitive_key, leaking embedded
        // URL passwords when the key name was innocuous (e.g. plain FOO
        // holding a postgres connection string).
        assert_eq!(
            mask_for_diff("FOO", "postgres://user:hunter2@host/db"),
            "***MASKED***"
        );
        // Use a non-placeholder password -- `contains_url_password` deliberately
        // skips `password`/`pass`/`secret`/`xxx*`/`example*`/`changeme*`/`your*`
        // to avoid false positives on example .env files. Real-world stolen
        // creds rarely look like that, so the value-aware mask must still fire
        // here.
        assert_eq!(
            mask_for_diff("CONFIG", "mysql://admin:Tr0ub4dor!@db.internal:3306/app"),
            "***MASKED***"
        );
    }

    #[test]
    fn test_mask_for_diff_masks_slack_webhook_with_innocuous_key() {
        assert_eq!(
            mask_for_diff(
                "HOOK",
                "https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXX"
            ),
            "***MASKED***"
        );
    }

    #[test]
    fn test_mask_for_diff_emits_full_value_for_safe_input() {
        // Non-sensitive key, non-secret-shaped value must round-trip unchanged
        // -- JSON consumers (CI scripts, jq pipelines) rely on the full value
        // for programmatic use; truncation belongs to the text path only.
        let long = "a-fairly-long-but-not-secret-config-value-that-must-not-be-truncated-in-json";
        assert_eq!(mask_for_diff("DESCRIPTION", long), long);
    }

    #[test]
    fn test_diff_identical_files() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nBAZ=qux");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_different_files() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nONLY_A=value");
        let env_b = create_temp_env(&dir, "b.env", "FOO=different\nONLY_B=value");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_with_schema() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");
        let schema = create_temp_env(
            &dir,
            "schema.json",
            r#"{"FOO": {"type": "string", "required": true}}"#,
        );

        let result = run(
            &env_a,
            &env_b,
            Some(&schema),
            "text",
            false,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_missing_file() {
        let result = run(
            "nonexistent_a.env",
            "nonexistent_b.env",
            None,
            "text",
            false,
            None,
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_diff_json_format() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nONLY_A=value");
        let env_b = create_temp_env(&dir, "b.env", "FOO=different\nONLY_B=value");

        let result = run(&env_a, &env_b, None, "json", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_json_with_schema() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");
        let schema = create_temp_env(
            &dir,
            "schema.json",
            r#"{"FOO": {"type": "string", "required": true}}"#,
        );

        let result = run(
            &env_a,
            &env_b,
            Some(&schema),
            "json",
            false,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_invalid_format() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar");

        let result = run(&env_a, &env_b, None, "xml", false, None, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown format"));
    }

    #[test]
    fn test_diff_empty_files() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "");
        let env_b = create_temp_env(&dir, "b.env", "");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_one_empty_one_populated() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_multiline_values() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=\"line1\nline2\"");
        let env_b = create_temp_env(&dir, "b.env", "FOO=\"line1\nline2\nline3\"");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_with_comments_and_blank_lines() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(
            &dir,
            "a.env",
            "# Comment\nFOO=bar\n\n# Another comment\nBAZ=qux",
        );
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    // ====== Typo Detection Tests ======

    #[test]
    fn test_diff_with_typo_detection() {
        let dir = TempDir::new().unwrap();
        // API_KEY vs APY_KEY - typo (1 edit distance)
        let env_a = create_temp_env(&dir, "a.env", "API_KEY=secret123");
        let env_b = create_temp_env(&dir, "b.env", "APY_KEY=secret123");

        // Run should succeed and internally detect the typo
        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_no_typos_completely_different() {
        let dir = TempDir::new().unwrap();
        // Completely different keys - no typo suggestion
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar");
        let env_b = create_temp_env(&dir, "b.env", "COMPLETELY_DIFFERENT=value");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_multiple_similar_keys() {
        let dir = TempDir::new().unwrap();
        // Multiple similar keys
        let env_a = create_temp_env(&dir, "a.env", "DATABASE_URL=db1\nDATABASE_HOST=host1");
        let env_b = create_temp_env(&dir, "b.env", "DATABSE_URL=db2\nDATABSE_HOST=host2");

        let result = run(&env_a, &env_b, None, "text", false, None, None, None);
        assert!(result.is_ok());
    }

    // ====== Schema Compliance Helper Tests ======

    #[test]
    fn test_check_missing_required() {
        use crate::schema::{Schema, VarSpec, VarType};

        let mut schema = Schema::new();
        let spec = VarSpec {
            var_type: VarType::String,
            required: true,
            ..Default::default()
        };
        schema.insert("REQUIRED_VAR".to_string(), spec);

        let mut env_map: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        env_map.insert("OTHER_VAR".to_string(), "value".to_string());

        let missing = check_missing_required(&env_map, &schema);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&"REQUIRED_VAR".to_string()));
    }

    #[test]
    fn test_check_unknown_keys() {
        use crate::schema::{Schema, VarSpec, VarType};

        let mut schema = Schema::new();
        let spec = VarSpec {
            var_type: VarType::String,
            ..Default::default()
        };
        schema.insert("KNOWN_VAR".to_string(), spec);

        let mut env_map: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        env_map.insert("KNOWN_VAR".to_string(), "value".to_string());
        env_map.insert("UNKNOWN_VAR".to_string(), "value".to_string());

        let unknown = check_unknown_keys(&env_map, &schema);
        assert_eq!(unknown.len(), 1);
        assert!(unknown.contains(&"UNKNOWN_VAR".to_string()));
    }

    #[test]
    fn test_diff_schema_both_files_compliant() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "PORT=3000\nDEBUG=true");
        let env_b = create_temp_env(&dir, "b.env", "PORT=8080\nDEBUG=false");
        let schema = create_temp_env(
            &dir,
            "schema.json",
            r#"{
            "PORT": {"type": "int", "required": true},
            "DEBUG": {"type": "bool", "required": true}
        }"#,
        );

        let result = run(
            &env_a,
            &env_b,
            Some(&schema),
            "text",
            false,
            None,
            None,
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_json_output_structure() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nONLY_A=value");
        let env_b = create_temp_env(&dir, "b.env", "FOO=different\nONLY_B=value");

        let result = run(&env_a, &env_b, None, "json", false, None, None, None);
        assert!(result.is_ok());
    }
}
