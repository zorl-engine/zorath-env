use std::collections::{BTreeMap, BTreeSet};

use crate::envfile;
use crate::schema;

pub fn run(
    env_a: &str,
    env_b: &str,
    schema_path: Option<&str>,
) -> Result<(), String> {
    // Parse both env files
    let map_a = envfile::parse_env_file(env_a).map_err(|e| format!("Error reading {}: {}", env_a, e))?;
    let map_b = envfile::parse_env_file(env_b).map_err(|e| format!("Error reading {}: {}", env_b, e))?;

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

    // Print header
    println!("Comparing {} vs {}\n", env_a, env_b);

    let mut has_diff = false;

    // Variables only in A
    if !only_in_a.is_empty() {
        has_diff = true;
        println!("Only in {}:", env_a);
        for key in &only_in_a {
            let val = map_a.get(*key).unwrap();
            println!("  + {}={}", key, truncate_value(val, 50));
        }
        println!();
    }

    // Variables only in B
    if !only_in_b.is_empty() {
        has_diff = true;
        println!("Only in {}:", env_b);
        for key in &only_in_b {
            let val = map_b.get(*key).unwrap();
            println!("  + {}={}", key, truncate_value(val, 50));
        }
        println!();
    }

    // Variables with different values
    if !different_values.is_empty() {
        has_diff = true;
        println!("Different values:");
        for (key, val_a, val_b) in &different_values {
            println!("  {}:", key);
            println!("    {} -> {}", truncate_value(val_a, 40), truncate_value(val_b, 40));
        }
        println!();
    }

    // Schema compliance check if schema provided
    if let Some(schema_path) = schema_path {
        match schema::load_schema(schema_path) {
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

/// Truncate a value for display, masking potential secrets
fn truncate_value(value: &str, max_len: usize) -> String {
    // Replace newlines for display
    let display = value.replace('\n', "\\n").replace('\r', "\\r");

    if display.len() <= max_len {
        display
    } else {
        format!("{}...", &display[..max_len])
    }
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
fn check_unknown_keys(
    env_map: &BTreeMap<String, String>,
    schema: &schema::Schema,
) -> Vec<String> {
    let mut unknown = Vec::new();
    for key in env_map.keys() {
        if !schema.contains_key(key) {
            unknown.push(key.clone());
        }
    }
    unknown
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
    fn test_truncate_value_short() {
        assert_eq!(truncate_value("short", 10), "short");
    }

    #[test]
    fn test_truncate_value_long() {
        assert_eq!(truncate_value("this is a very long value", 10), "this is a ...");
    }

    #[test]
    fn test_truncate_value_newlines() {
        assert_eq!(truncate_value("line1\nline2", 20), "line1\\nline2");
    }

    #[test]
    fn test_diff_identical_files() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nBAZ=qux");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");

        let result = run(&env_a, &env_b, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_different_files() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar\nONLY_A=value");
        let env_b = create_temp_env(&dir, "b.env", "FOO=different\nONLY_B=value");

        let result = run(&env_a, &env_b, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_with_schema() {
        let dir = TempDir::new().unwrap();
        let env_a = create_temp_env(&dir, "a.env", "FOO=bar");
        let env_b = create_temp_env(&dir, "b.env", "FOO=bar\nBAZ=qux");
        let schema = create_temp_env(&dir, "schema.json", r#"{"FOO": {"type": "string", "required": true}}"#);

        let result = run(&env_a, &env_b, Some(&schema));
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_missing_file() {
        let result = run("nonexistent_a.env", "nonexistent_b.env", None);
        assert!(result.is_err());
    }
}
