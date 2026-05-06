//! Scan source code for environment variable usage and compare against schema.
//!
//! This command walks a directory tree, detects env var access patterns in various
//! programming languages, and reports variables that are:
//! - Used in code but not defined in schema (potential bugs)
//! - Defined in schema but not used in code (potential dead config)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use ignore::WalkBuilder;
use regex::Regex;

use crate::errors::CliError;
use crate::schema::{self, LoadOptions, Schema};

/// Language-specific pattern for detecting environment variable access
struct EnvPattern {
    language: &'static str,
    pattern: Regex,
}

/// Cached regex patterns (compiled once, reused across calls)
static SCAN_PATTERNS: OnceLock<Vec<EnvPattern>> = OnceLock::new();

/// A detected environment variable usage in source code
#[derive(Debug, Clone)]
struct EnvUsage {
    var_name: String,
    file_path: String,
    line_number: usize,
    language: &'static str,
}

/// Results from scanning source code
struct ScanResults {
    /// All environment variables found in code, with their usage locations
    found_vars: HashMap<String, Vec<EnvUsage>>,
    /// Variables used in code but not defined in schema
    missing_from_schema: Vec<String>,
    /// Variables defined in schema but not found in code
    unused_in_code: Vec<String>,
    /// Total files scanned
    files_scanned: usize,
}

/// Run the scan command
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn run(
    path: &str,
    schema_path: &str,
    show_unused: bool,
    show_paths: bool,
    format: &str,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    // Load schema
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds,
    };
    let schema = schema::load_schema_with_options(schema_path, &options)
        .map_err(|e| CliError::Schema(format!("failed to load schema: {}", e)))?;

    // Get cached regex patterns (compiled once, reused for performance)
    let patterns = get_patterns();

    // Scan directory
    let scan_path = Path::new(path);
    if !scan_path.exists() {
        return Err(CliError::Input(format!("path does not exist: {}", path)));
    }

    let results = scan_directory(scan_path, patterns, &schema).map_err(CliError::Input)?;

    // Output results
    match format {
        "json" => print_json_output(&results, path, show_unused, show_paths, &schema),
        _ => print_text_output(&results, path, show_unused, show_paths, &schema),
    }

    // Exit with error if there are missing vars (for CI)
    if !results.missing_from_schema.is_empty() {
        return Err(CliError::Validation(format!(
            "{} environment variable(s) used in code but not in schema",
            results.missing_from_schema.len()
        )));
    }

    Ok(())
}

/// Get cached regex patterns (compiled once, reused for performance)
fn get_patterns() -> &'static Vec<EnvPattern> {
    SCAN_PATTERNS.get_or_init(build_patterns)
}

/// Build all language-specific regex patterns for env var detection
fn build_patterns() -> Vec<EnvPattern> {
    let pattern_defs: Vec<(&str, &str)> = vec![
        // Rust patterns
        ("rust", r#"std::env::var\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        ("rust", r#"env::var\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        ("rust", r#"env::var_os\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        ("rust", r#"dotenvy::var\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        // JavaScript/TypeScript patterns
        ("javascript", r#"process\.env\.([A-Z_][A-Z0-9_]*)"#),
        ("javascript", r#"process\.env\["([A-Z_][A-Z0-9_]*)"\]"#),
        ("javascript", r#"process\.env\['([A-Z_][A-Z0-9_]*)'\]"#),
        // Python patterns
        ("python", r#"os\.environ\["([A-Z_][A-Z0-9_]*)"\]"#),
        ("python", r#"os\.environ\['([A-Z_][A-Z0-9_]*)'\]"#),
        ("python", r#"os\.getenv\s*\(\s*["']([A-Z_][A-Z0-9_]*)["']"#),
        (
            "python",
            r#"os\.environ\.get\s*\(\s*["']([A-Z_][A-Z0-9_]*)["']"#,
        ),
        // Go patterns
        ("go", r#"os\.Getenv\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        ("go", r#"os\.LookupEnv\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        // PHP patterns
        ("php", r#"getenv\s*\(\s*["']([A-Z_][A-Z0-9_]*)["']\s*\)"#),
        ("php", r#"\$_ENV\["([A-Z_][A-Z0-9_]*)"\]"#),
        ("php", r#"\$_ENV\['([A-Z_][A-Z0-9_]*)'\]"#),
        ("php", r#"\$_SERVER\["([A-Z_][A-Z0-9_]*)"\]"#),
        ("php", r#"\$_SERVER\['([A-Z_][A-Z0-9_]*)'\]"#),
        // Ruby patterns
        ("ruby", r#"ENV\["([A-Z_][A-Z0-9_]*)"\]"#),
        ("ruby", r#"ENV\['([A-Z_][A-Z0-9_]*)'\]"#),
        ("ruby", r#"ENV\.fetch\s*\(\s*["']([A-Z_][A-Z0-9_]*)["']"#),
        // Shell patterns (bash, sh, zsh)
        ("shell", r#"\$\{([A-Z_][A-Z0-9_]*)\}"#),
        ("shell", r#"\$([A-Z_][A-Z0-9_]*)"#),
        // Java patterns
        ("java", r#"System\.getenv\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#),
        // C# / .NET patterns
        (
            "csharp",
            r#"Environment\.GetEnvironmentVariable\s*\(\s*"([A-Z_][A-Z0-9_]*)"\s*\)"#,
        ),
    ];

    pattern_defs
        .into_iter()
        .filter_map(|(lang, pat)| {
            Regex::new(pat).ok().map(|regex| EnvPattern {
                language: lang,
                pattern: regex,
            })
        })
        .collect()
}

/// Determine the language of a file based on its extension
fn detect_language(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    match ext.to_lowercase().as_str() {
        "rs" => Some("rust"),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some("javascript"),
        "py" | "pyw" => Some("python"),
        "go" => Some("go"),
        "php" => Some("php"),
        "rb" | "erb" => Some("ruby"),
        "sh" | "bash" | "zsh" | "fish" => Some("shell"),
        "java" | "kt" | "kts" => Some("java"),
        "cs" => Some("csharp"),
        _ => None,
    }
}

/// Check if a file should be scanned (is a recognized source file)
fn should_scan_file(path: &Path) -> bool {
    detect_language(path).is_some()
}

/// Scan a directory tree for environment variable usage
fn scan_directory(
    root: &Path,
    patterns: &[EnvPattern],
    schema: &Schema,
) -> Result<ScanResults, String> {
    let mut found_vars: HashMap<String, Vec<EnvUsage>> = HashMap::new();
    let mut files_scanned = 0;

    // Walk directory, respecting .gitignore
    let walker = WalkBuilder::new(root)
        .hidden(false) // Don't skip hidden files by default
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .build();

    for entry in walker.flatten() {
        let path = entry.path();

        // Skip directories
        if path.is_dir() {
            continue;
        }

        // Skip non-source files
        if !should_scan_file(path) {
            continue;
        }

        // Scan file
        if let Ok(usages) = scan_file(path, patterns) {
            files_scanned += 1;
            for usage in usages {
                found_vars
                    .entry(usage.var_name.clone())
                    .or_default()
                    .push(usage);
            }
        }
    }

    // Compare against schema
    let schema_keys: HashSet<&String> = schema.keys().collect();
    let found_keys: HashSet<&String> = found_vars.keys().collect();

    let missing_from_schema: Vec<String> = found_keys
        .difference(&schema_keys)
        .map(|s| (*s).clone())
        .collect();

    let unused_in_code: Vec<String> = schema_keys
        .difference(&found_keys)
        .map(|s| (*s).clone())
        .collect();

    Ok(ScanResults {
        found_vars,
        missing_from_schema,
        unused_in_code,
        files_scanned,
    })
}

/// Maximum source file size to scan (skip larger files to avoid OOM
/// on vendored/minified blobs).
const SCAN_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Scan a single file for environment variable usage
fn scan_file(path: &Path, patterns: &[EnvPattern]) -> Result<Vec<EnvUsage>, String> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > SCAN_MAX_FILE_BYTES {
            // Silently skip oversized files. (Loud warnings would spam scan output
            // on large monorepos with vendored bundles.)
            return Ok(Vec::new());
        }
    }
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;

    let file_lang = detect_language(path);
    let path_str = path.to_string_lossy().to_string();
    let mut usages = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        // Skip comment lines (basic heuristic)
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
            continue;
        }

        for pattern in patterns {
            // Optionally filter by file language for more accurate results
            // For now, we apply all patterns to allow cross-language detection

            for cap in pattern.pattern.captures_iter(line) {
                if let Some(var_match) = cap.get(1) {
                    let var_name = var_match.as_str().to_string();

                    // Skip common false positives
                    if is_common_false_positive(&var_name) {
                        continue;
                    }

                    usages.push(EnvUsage {
                        var_name,
                        file_path: path_str.clone(),
                        line_number: line_num + 1,
                        language: file_lang.unwrap_or(pattern.language),
                    });
                }
            }
        }
    }

    Ok(usages)
}

/// Check if a variable name is a common false positive
fn is_common_false_positive(var_name: &str) -> bool {
    // Common system/runtime vars that are not typically in user schemas
    matches!(
        var_name,
        "PATH"
            | "HOME"
            | "USER"
            | "SHELL"
            | "TERM"
            | "PWD"
            | "LANG"
            | "LC_ALL"
            | "TMPDIR"
            | "TMP"
            | "TEMP"
            | "HOSTNAME"
            | "LOGNAME"
            | "EDITOR"
            | "VISUAL"
    )
}

/// Print results in human-readable text format
fn print_text_output(
    results: &ScanResults,
    path: &str,
    show_unused: bool,
    show_paths: bool,
    schema: &Schema,
) {
    println!("Scanning {} ...", path);
    println!();
    println!(
        "Scanned {} files, found {} unique environment variables",
        results.files_scanned,
        results.found_vars.len()
    );
    println!();

    // Show all found variables with paths (if requested)
    if show_paths {
        println!(
            "FOUND IN CODE ({}) - all environment variables detected:",
            results.found_vars.len()
        );
        let mut sorted: Vec<_> = results.found_vars.keys().collect();
        sorted.sort();

        for var_name in sorted {
            let in_schema = schema.contains_key(var_name);
            let status = if in_schema {
                "in schema"
            } else {
                "NOT in schema"
            };
            if let Some(usages) = results.found_vars.get(var_name) {
                println!("  {} ({} usages, {})", var_name, usages.len(), status);
                for usage in usages {
                    println!("    {}:{}", usage.file_path, usage.line_number);
                }
            }
        }
        println!();
    }

    // Missing from schema (bugs)
    if !results.missing_from_schema.is_empty() {
        println!(
            "MISSING FROM SCHEMA ({}) - variables used in code but not defined:",
            results.missing_from_schema.len()
        );
        let mut sorted: Vec<_> = results.missing_from_schema.iter().collect();
        sorted.sort();

        for var_name in sorted {
            println!("  {}", var_name);
            if let Some(usages) = results.found_vars.get(var_name) {
                for usage in usages {
                    println!("    {}:{}", usage.file_path, usage.line_number);
                }
            }
        }
        println!();
    }

    // Unused in code
    if show_unused && !results.unused_in_code.is_empty() {
        println!(
            "UNUSED IN CODE ({}) - variables in schema but not found in source:",
            results.unused_in_code.len()
        );
        let mut sorted: Vec<_> = results.unused_in_code.iter().collect();
        sorted.sort();

        for var_name in sorted {
            let required = schema
                .get(var_name)
                .map(|spec| {
                    if spec.required {
                        "required"
                    } else {
                        "optional"
                    }
                })
                .unwrap_or("unknown");
            println!("  {} ({})", var_name, required);
        }
        println!();
    }

    // Summary
    let matched = results.found_vars.len() - results.missing_from_schema.len();
    println!("Summary:");
    println!("  Total variables found: {}", results.found_vars.len());
    println!("  Matched in schema: {}", matched);
    println!(
        "  Missing from schema: {}",
        results.missing_from_schema.len()
    );
    if show_unused {
        println!("  Unused in code: {}", results.unused_in_code.len());
    }

    if results.missing_from_schema.is_empty() {
        println!();
        println!("All environment variables are defined in schema.");
    }
}

/// Print results in JSON format (CI-friendly)
fn print_json_output(
    results: &ScanResults,
    path: &str,
    show_unused: bool,
    show_paths: bool,
    schema: &Schema,
) {
    let mut output = serde_json::json!({
        "scanned_path": path,
        "files_scanned": results.files_scanned,
        "total_vars_found": results.found_vars.len(),
        "matched_in_schema": results.found_vars.len() - results.missing_from_schema.len(),
        "missing_from_schema_count": results.missing_from_schema.len(),
    });

    // All found variables with paths (if requested)
    if show_paths {
        let found: Vec<serde_json::Value> = results
            .found_vars
            .iter()
            .map(|(var_name, usages)| {
                let usage_list: Vec<serde_json::Value> = usages
                    .iter()
                    .map(|usage| {
                        serde_json::json!({
                            "file": usage.file_path,
                            "line": usage.line_number,
                            "language": usage.language,
                        })
                    })
                    .collect();

                serde_json::json!({
                    "var_name": var_name,
                    "in_schema": schema.contains_key(var_name),
                    "usage_count": usages.len(),
                    "usages": usage_list,
                })
            })
            .collect();

        output["found_vars"] = serde_json::json!(found);
    }

    // Missing from schema details
    let missing: Vec<serde_json::Value> = results
        .missing_from_schema
        .iter()
        .map(|var_name| {
            let usages: Vec<serde_json::Value> = results
                .found_vars
                .get(var_name)
                .map(|u| {
                    u.iter()
                        .map(|usage| {
                            serde_json::json!({
                                "file": usage.file_path,
                                "line": usage.line_number,
                                "language": usage.language,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            serde_json::json!({
                "var_name": var_name,
                "usages": usages,
            })
        })
        .collect();

    output["missing_from_schema"] = serde_json::json!(missing);

    // Unused in code details (if requested)
    if show_unused {
        let unused: Vec<serde_json::Value> = results
            .unused_in_code
            .iter()
            .map(|var_name| {
                let spec = schema.get(var_name);
                serde_json::json!({
                    "var_name": var_name,
                    "required": spec.map(|s| s.required).unwrap_or(false),
                    "type": spec.map(|s| format!("{:?}", s.var_type)).unwrap_or_else(|| "unknown".to_string()),
                })
            })
            .collect();

        output["unused_in_code_count"] = serde_json::json!(results.unused_in_code.len());
        output["unused_in_code"] = serde_json::json!(unused);
    }

    output["success"] = serde_json::json!(results.missing_from_schema.is_empty());

    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_build_patterns() {
        let patterns = build_patterns();
        assert!(!patterns.is_empty());
        // Should have patterns for multiple languages
        let languages: HashSet<_> = patterns.iter().map(|p| p.language).collect();
        assert!(languages.contains(&"rust"));
        assert!(languages.contains(&"javascript"));
        assert!(languages.contains(&"python"));
    }

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language(Path::new("main.rs")), Some("rust"));
        assert_eq!(detect_language(Path::new("lib.rs")), Some("rust"));
    }

    #[test]
    fn test_detect_language_javascript() {
        assert_eq!(detect_language(Path::new("app.js")), Some("javascript"));
        assert_eq!(detect_language(Path::new("app.ts")), Some("javascript"));
        assert_eq!(detect_language(Path::new("app.tsx")), Some("javascript"));
    }

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language(Path::new("main.py")), Some("python"));
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language(Path::new("data.json")), None);
        assert_eq!(detect_language(Path::new("README.md")), None);
    }

    #[test]
    fn test_is_common_false_positive() {
        assert!(is_common_false_positive("PATH"));
        assert!(is_common_false_positive("HOME"));
        assert!(!is_common_false_positive("API_KEY"));
        assert!(!is_common_false_positive("DATABASE_URL"));
    }

    #[test]
    fn test_rust_pattern_matching() {
        let patterns = build_patterns();
        let content = r#"let key = std::env::var("API_KEY").unwrap();"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in Rust code");
    }

    #[test]
    fn test_javascript_pattern_matching() {
        let patterns = build_patterns();
        let content = r#"const url = process.env.DATABASE_URL;"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "DATABASE_URL" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect DATABASE_URL in JavaScript code");
    }

    #[test]
    fn test_python_pattern_matching() {
        let patterns = build_patterns();
        let content = r#"secret = os.getenv("SECRET_KEY")"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "SECRET_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect SECRET_KEY in Python code");
    }

    #[test]
    fn test_scan_file_rust() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");

        let content = r#"
fn main() {
    let api_key = std::env::var("API_KEY").unwrap();
    let db_url = env::var("DATABASE_URL").unwrap();
}
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(var_names.contains("API_KEY"));
        assert!(var_names.contains("DATABASE_URL"));
    }

    #[test]
    fn test_scan_file_javascript() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.js");

        let content = r#"
const config = {
    apiKey: process.env.API_KEY,
    dbUrl: process.env["DATABASE_URL"],
    secret: process.env['SECRET_KEY'],
};
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(var_names.contains("API_KEY"));
        assert!(var_names.contains("DATABASE_URL"));
        assert!(var_names.contains("SECRET_KEY"));
    }

    #[test]
    fn test_scan_directory() {
        let dir = tempdir().unwrap();

        // Create a Rust file
        let rs_file = dir.path().join("main.rs");
        fs::write(&rs_file, r#"let key = env::var("API_KEY").unwrap();"#).unwrap();

        // Create a schema
        let schema: Schema = [
            (
                "API_KEY".to_string(),
                crate::schema::VarSpec {
                    var_type: crate::schema::VarType::String,
                    required: true,
                    description: None,
                    values: None,
                    default: None,
                    validate: None,
                    secret: None,
                    ..Default::default()
                },
            ),
            (
                "UNUSED_VAR".to_string(),
                crate::schema::VarSpec {
                    var_type: crate::schema::VarType::String,
                    required: false,
                    description: None,
                    values: None,
                    default: None,
                    validate: None,
                    secret: None,
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect();

        let patterns = build_patterns();
        let results = scan_directory(dir.path(), &patterns, &schema).unwrap();

        assert!(results.found_vars.contains_key("API_KEY"));
        assert!(results.missing_from_schema.is_empty());
        assert!(results.unused_in_code.contains(&"UNUSED_VAR".to_string()));
    }

    // ====== Go Language Tests ======

    #[test]
    fn test_detect_language_go() {
        assert_eq!(detect_language(Path::new("main.go")), Some("go"));
        assert_eq!(detect_language(Path::new("handler.go")), Some("go"));
    }

    #[test]
    fn test_go_pattern_getenv() {
        let patterns = build_patterns();
        let content = r#"apiKey := os.Getenv("API_KEY")"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in Go os.Getenv");
    }

    #[test]
    fn test_go_pattern_lookupenv() {
        let patterns = build_patterns();
        let content = r#"value, exists := os.LookupEnv("DATABASE_URL")"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "DATABASE_URL" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect DATABASE_URL in Go os.LookupEnv");
    }

    #[test]
    fn test_scan_file_go() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("main.go");

        let content = r#"
package main

import "os"

func main() {
    apiKey := os.Getenv("API_KEY")
    dbUrl, _ := os.LookupEnv("DATABASE_URL")
}
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in Go file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in Go file"
        );
    }

    // ====== PHP Language Tests ======

    #[test]
    fn test_detect_language_php() {
        assert_eq!(detect_language(Path::new("index.php")), Some("php"));
        assert_eq!(detect_language(Path::new("config.php")), Some("php"));
    }

    #[test]
    fn test_php_pattern_getenv() {
        let patterns = build_patterns();
        let content = r#"$apiKey = getenv("API_KEY");"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in PHP getenv()");
    }

    #[test]
    fn test_php_pattern_env_superglobal() {
        let patterns = build_patterns();
        let content = r#"$dbUrl = $_ENV["DATABASE_URL"];"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "DATABASE_URL" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect DATABASE_URL in PHP $_ENV");
    }

    #[test]
    fn test_php_pattern_server_superglobal() {
        let patterns = build_patterns();
        let content = r#"$secret = $_SERVER["SECRET_KEY"];"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "SECRET_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect SECRET_KEY in PHP $_SERVER");
    }

    #[test]
    fn test_scan_file_php() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("config.php");

        let content = r#"<?php
$apiKey = getenv("API_KEY");
$dbUrl = $_ENV["DATABASE_URL"];
$secret = $_SERVER['SECRET_KEY'];
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in PHP file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in PHP file"
        );
        assert!(
            var_names.contains("SECRET_KEY"),
            "Should find SECRET_KEY in PHP file"
        );
    }

    // ====== Ruby Language Tests ======

    #[test]
    fn test_detect_language_ruby() {
        assert_eq!(detect_language(Path::new("app.rb")), Some("ruby"));
        assert_eq!(detect_language(Path::new("config.erb")), Some("ruby"));
    }

    #[test]
    fn test_ruby_pattern_env_bracket() {
        let patterns = build_patterns();
        let content = r#"api_key = ENV["API_KEY"]"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in Ruby ENV[]");
    }

    #[test]
    fn test_ruby_pattern_env_fetch() {
        let patterns = build_patterns();
        let content = r#"db_url = ENV.fetch("DATABASE_URL")"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "DATABASE_URL" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect DATABASE_URL in Ruby ENV.fetch");
    }

    #[test]
    fn test_scan_file_ruby() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("config.rb");

        let content = r#"
api_key = ENV["API_KEY"]
db_url = ENV['DATABASE_URL']
secret = ENV.fetch("SECRET_KEY")
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in Ruby file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in Ruby file"
        );
        assert!(
            var_names.contains("SECRET_KEY"),
            "Should find SECRET_KEY in Ruby file"
        );
    }

    // ====== Shell Language Tests ======

    #[test]
    fn test_detect_language_shell() {
        assert_eq!(detect_language(Path::new("script.sh")), Some("shell"));
        assert_eq!(detect_language(Path::new("run.bash")), Some("shell"));
        assert_eq!(detect_language(Path::new("init.zsh")), Some("shell"));
    }

    #[test]
    fn test_shell_pattern_braces() {
        let patterns = build_patterns();
        let content = r#"echo "${API_KEY}""#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in shell ${{VAR}} syntax");
    }

    #[test]
    fn test_shell_pattern_dollar() {
        let patterns = build_patterns();
        let content = r#"export VALUE=$DATABASE_URL"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "DATABASE_URL" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect DATABASE_URL in shell $VAR syntax");
    }

    #[test]
    fn test_scan_file_shell() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("deploy.sh");

        let content = r#"#!/bin/bash
echo "Deploying with ${API_KEY}"
export DB=$DATABASE_URL
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in shell file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in shell file"
        );
    }

    // ====== Java Language Tests ======

    #[test]
    fn test_detect_language_java() {
        assert_eq!(detect_language(Path::new("Main.java")), Some("java"));
        assert_eq!(detect_language(Path::new("Config.kt")), Some("java"));
    }

    #[test]
    fn test_java_pattern_getenv() {
        let patterns = build_patterns();
        let content = r#"String apiKey = System.getenv("API_KEY");"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(found, "Should detect API_KEY in Java System.getenv");
    }

    #[test]
    fn test_scan_file_java() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("Config.java");

        let content = r#"
public class Config {
    public static void main(String[] args) {
        String apiKey = System.getenv("API_KEY");
        String dbUrl = System.getenv("DATABASE_URL");
    }
}
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in Java file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in Java file"
        );
    }

    // ====== C# Language Tests ======

    #[test]
    fn test_detect_language_csharp() {
        assert_eq!(detect_language(Path::new("Program.cs")), Some("csharp"));
    }

    #[test]
    fn test_csharp_pattern_getenvironmentvariable() {
        let patterns = build_patterns();
        let content = r#"var apiKey = Environment.GetEnvironmentVariable("API_KEY");"#;

        let mut found = false;
        for pattern in &patterns {
            for cap in pattern.pattern.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    if m.as_str() == "API_KEY" {
                        found = true;
                    }
                }
            }
        }
        assert!(
            found,
            "Should detect API_KEY in C# Environment.GetEnvironmentVariable"
        );
    }

    #[test]
    fn test_scan_file_csharp() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("Program.cs");

        let content = r#"
using System;

class Program {
    static void Main() {
        var apiKey = Environment.GetEnvironmentVariable("API_KEY");
        var dbUrl = Environment.GetEnvironmentVariable("DATABASE_URL");
    }
}
"#;
        fs::write(&file_path, content).unwrap();

        let patterns = build_patterns();
        let usages = scan_file(&file_path, &patterns).unwrap();

        let var_names: HashSet<_> = usages.iter().map(|u| u.var_name.as_str()).collect();
        assert!(
            var_names.contains("API_KEY"),
            "Should find API_KEY in C# file"
        );
        assert!(
            var_names.contains("DATABASE_URL"),
            "Should find DATABASE_URL in C# file"
        );
    }
}
