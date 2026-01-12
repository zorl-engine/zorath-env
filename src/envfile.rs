use std::collections::HashMap;
use std::fs;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvError {
    #[error("failed to read env file: {0}")]
    Read(String),
}

/// Very small .env parser:
/// - ignores blank lines and comments starting with '#'
/// - parses KEY=VALUE
/// - strips optional surrounding quotes from VALUE
pub fn parse_env_file(path: &str) -> Result<HashMap<String, String>, EnvError> {
    let content = fs::read_to_string(path).map_err(|e| EnvError::Read(e.to_string()))?;
    Ok(parse_env_str(&content))
}

pub fn parse_env_str(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }

        // allow "export KEY=VALUE"
        let line = line.strip_prefix("export ").unwrap_or(line);

        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_string();
            let mut val = v.trim().to_string();

            // strip surrounding quotes
            if (val.starts_with('"') && val.ends_with('"')) || (val.starts_with('\'') && val.ends_with('\'')) {
                val = val[1..val.len()-1].to_string();
            }

            map.insert(key, val);
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_key_value() {
        let input = "FOO=bar";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_multiple_key_values() {
        let input = "FOO=bar\nBAZ=qux";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(result.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_ignores_comments() {
        let input = "# this is a comment\nFOO=bar\n# another comment";
        let result = parse_env_str(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_ignores_blank_lines() {
        let input = "\n\nFOO=bar\n\n\nBAZ=qux\n";
        let result = parse_env_str(input);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_export_prefix() {
        let input = "export FOO=bar";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_strips_double_quotes() {
        let input = "FOO=\"bar baz\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar baz".to_string()));
    }

    #[test]
    fn test_strips_single_quotes() {
        let input = "FOO='bar baz'";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar baz".to_string()));
    }

    #[test]
    fn test_preserves_mismatched_quotes() {
        let input = "FOO=\"bar'";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"\"bar'".to_string()));
    }

    #[test]
    fn test_empty_value() {
        let input = "FOO=";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"".to_string()));
    }

    #[test]
    fn test_value_with_equals() {
        let input = "DATABASE_URL=postgres://user:pass@host/db?foo=bar";
        let result = parse_env_str(input);
        assert_eq!(result.get("DATABASE_URL"), Some(&"postgres://user:pass@host/db?foo=bar".to_string()));
    }

    #[test]
    fn test_trims_whitespace() {
        let input = "  FOO  =  bar  ";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_empty_input() {
        let input = "";
        let result = parse_env_str(input);
        assert!(result.is_empty());
    }
}
