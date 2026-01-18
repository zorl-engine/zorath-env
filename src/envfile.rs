use std::collections::{HashMap, HashSet};
use std::fs;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvError {
    #[error("failed to read env file: {0}")]
    Read(String),
    #[error("circular variable reference: {0}")]
    CircularRef(String),
}

/// .env parser with multiline and escape support:
/// - ignores blank lines and comments starting with '#'
/// - parses KEY=VALUE
/// - supports multiline values in quoted strings
/// - handles escape sequences in double-quoted strings: \", \\, \n, \t, \r
/// - single-quoted strings are literal (no escape processing)
pub fn parse_env_file(path: &str) -> Result<HashMap<String, String>, EnvError> {
    let content = fs::read_to_string(path).map_err(|e| EnvError::Read(e.to_string()))?;
    Ok(parse_env_str(&content))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ParseState {
    LineStart,
    InKey,
    AfterEquals,
    InUnquotedValue,
    InDoubleQuoted,
    InDoubleQuotedEscape,
    InSingleQuoted,
}

pub fn parse_env_str(content: &str) -> HashMap<String, String> {
    parse_env_str_with_warnings(content, true)
}

/// Parse .env content with optional duplicate key warnings
pub fn parse_env_str_with_warnings(content: &str, warn_duplicates: bool) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut state = ParseState::LineStart;
    let mut current_key = String::new();
    let mut current_value = String::new();
    let mut chars = content.chars().peekable();
    let mut line_number: usize = 1;
    let mut key_start_line: usize = 1;

    // Helper to insert with duplicate warning
    let insert_with_warning = |map: &mut HashMap<String, String>, key: String, value: String, line: usize| {
        if warn_duplicates && map.contains_key(&key) {
            eprintln!("warning: duplicate key '{}' at line {} (overwriting previous value)", key, line);
        }
        map.insert(key, value);
    };

    while let Some(ch) = chars.next() {
        // Track line numbers
        if ch == '\n' {
            line_number += 1;
        }

        match state {
            ParseState::LineStart => {
                if ch == '#' {
                    // Skip comment line
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\n' {
                            line_number += 1;
                            break;
                        }
                    }
                } else if ch == '\n' || ch == '\r' {
                    // Empty line, stay in LineStart
                } else if ch.is_whitespace() {
                    // Skip leading whitespace
                } else if ch == 'e' && chars.peek() == Some(&'x') {
                    // Check for "export " prefix
                    let rest: String = chars.clone().take(5).collect();
                    if rest == "xport" {
                        // Skip "xport"
                        for _ in 0..5 { chars.next(); }
                        // Skip space after export
                        if chars.peek() == Some(&' ') {
                            chars.next();
                        }
                        state = ParseState::LineStart;
                    } else {
                        current_key.push(ch);
                        state = ParseState::InKey;
                    }
                } else {
                    current_key.push(ch);
                    key_start_line = line_number;
                    state = ParseState::InKey;
                }
            }

            ParseState::InKey => {
                if ch == '=' {
                    state = ParseState::AfterEquals;
                } else if ch == '\n' || ch == '\r' {
                    // Key without value, ignore
                    current_key.clear();
                    state = ParseState::LineStart;
                } else if ch.is_whitespace() {
                    // Whitespace before = is trimmed
                } else {
                    current_key.push(ch);
                }
            }

            ParseState::AfterEquals => {
                if ch == '"' {
                    state = ParseState::InDoubleQuoted;
                } else if ch == '\'' {
                    state = ParseState::InSingleQuoted;
                } else if ch == '\n' || ch == '\r' {
                    // Empty value
                    let key = current_key.trim().to_string();
                    if !key.is_empty() {
                        insert_with_warning(&mut map, key, String::new(), key_start_line);
                    }
                    current_key.clear();
                    state = ParseState::LineStart;
                } else if ch.is_whitespace() {
                    // Skip whitespace after =
                } else {
                    current_value.push(ch);
                    state = ParseState::InUnquotedValue;
                }
            }

            ParseState::InUnquotedValue => {
                if ch == '\n' || ch == '\r' {
                    let key = current_key.trim().to_string();
                    let val = current_value.trim().to_string();
                    if !key.is_empty() {
                        insert_with_warning(&mut map, key, val, key_start_line);
                    }
                    current_key.clear();
                    current_value.clear();
                    state = ParseState::LineStart;
                } else if ch == '#' {
                    // Inline comment - end value here
                    let key = current_key.trim().to_string();
                    let val = current_value.trim().to_string();
                    if !key.is_empty() {
                        insert_with_warning(&mut map, key, val, key_start_line);
                    }
                    current_key.clear();
                    current_value.clear();
                    // Skip rest of line
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\n' {
                            line_number += 1;
                            break;
                        }
                    }
                    state = ParseState::LineStart;
                } else {
                    current_value.push(ch);
                }
            }

            ParseState::InDoubleQuoted => {
                if ch == '\\' {
                    state = ParseState::InDoubleQuotedEscape;
                } else if ch == '"' {
                    // End of double-quoted value
                    let key = current_key.trim().to_string();
                    if !key.is_empty() {
                        insert_with_warning(&mut map, key, current_value.clone(), key_start_line);
                    }
                    current_key.clear();
                    current_value.clear();
                    // Skip to end of line
                    while let Some(&c) = chars.peek() {
                        if c == '\n' || c == '\r' { break; }
                        chars.next();
                    }
                    state = ParseState::LineStart;
                } else {
                    // Include newlines in multiline values
                    current_value.push(ch);
                }
            }

            ParseState::InDoubleQuotedEscape => {
                match ch {
                    'n' => current_value.push('\n'),
                    'r' => current_value.push('\r'),
                    't' => current_value.push('\t'),
                    '\\' => current_value.push('\\'),
                    '"' => current_value.push('"'),
                    '\n' | '\r' => {
                        // Line continuation - skip the newline
                        if ch == '\r' && chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                    }
                    _ => {
                        // Unknown escape, keep as-is
                        current_value.push('\\');
                        current_value.push(ch);
                    }
                }
                state = ParseState::InDoubleQuoted;
            }

            ParseState::InSingleQuoted => {
                if ch == '\'' {
                    // End of single-quoted value (no escape processing)
                    let key = current_key.trim().to_string();
                    if !key.is_empty() {
                        insert_with_warning(&mut map, key, current_value.clone(), key_start_line);
                    }
                    current_key.clear();
                    current_value.clear();
                    // Skip to end of line
                    while let Some(&c) = chars.peek() {
                        if c == '\n' || c == '\r' { break; }
                        chars.next();
                    }
                    state = ParseState::LineStart;
                } else {
                    // Include everything literally, including newlines
                    current_value.push(ch);
                }
            }
        }
    }

    // Handle final value if file doesn't end with newline
    match state {
        ParseState::InUnquotedValue => {
            let key = current_key.trim().to_string();
            let val = current_value.trim().to_string();
            if !key.is_empty() {
                insert_with_warning(&mut map, key, val, key_start_line);
            }
        }
        ParseState::InDoubleQuoted | ParseState::InSingleQuoted => {
            // Unclosed quote - still save what we have
            let key = current_key.trim().to_string();
            if !key.is_empty() {
                insert_with_warning(&mut map, key, current_value, key_start_line);
            }
        }
        ParseState::AfterEquals => {
            let key = current_key.trim().to_string();
            if !key.is_empty() {
                insert_with_warning(&mut map, key, String::new(), key_start_line);
            }
        }
        _ => {}
    }

    map
}

/// Interpolate ${VAR} and $VAR references in a single value
fn interpolate_value(value: &str, env_map: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'{') {
                // ${VAR} syntax
                chars.next(); // consume '{'
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if let Some(val) = env_map.get(&var_name) {
                    result.push_str(val);
                } else {
                    // Keep unresolved reference as-is
                    result.push_str(&format!("${{{}}}", var_name));
                }
            } else if chars.peek().map(|c| c.is_alphabetic() || *c == '_').unwrap_or(false) {
                // $VAR syntax (bare word) - collect without consuming trailing char
                let mut var_name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        var_name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(val) = env_map.get(&var_name) {
                    result.push_str(val);
                } else {
                    // Keep unresolved reference as-is
                    result.push('$');
                    result.push_str(&var_name);
                }
            } else {
                // Lone $ or $followed-by-non-identifier
                result.push('$');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Check if a value contains variable references
fn has_var_refs(value: &str) -> bool {
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'{') {
                return true;
            }
            if chars.peek().map(|c| c.is_alphabetic() || *c == '_').unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// Extract variable names referenced in a value
fn extract_var_refs(value: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if !var_name.is_empty() {
                    refs.push(var_name);
                }
            } else if chars.peek().map(|c| c.is_alphabetic() || *c == '_').unwrap_or(false) {
                let mut var_name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        var_name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    refs.push(var_name);
                }
            }
        }
    }

    refs
}

/// Check if value has refs to keys that exist in the map (potential circular)
fn has_resolvable_refs(value: &str, keys: &HashSet<String>) -> bool {
    for var_name in extract_var_refs(value) {
        if keys.contains(&var_name) {
            return true;
        }
    }
    false
}

/// Interpolate all variable references in env_map
/// Returns error if circular reference detected
pub fn interpolate_env(env_map: HashMap<String, String>) -> Result<HashMap<String, String>, EnvError> {
    let mut result = env_map.clone();
    let keys: HashSet<String> = env_map.keys().cloned().collect();
    let max_iterations = env_map.len() + 1;

    for _ in 0..max_iterations {
        let mut changed = false;
        let snapshot = result.clone();

        for (_key, value) in result.iter_mut() {
            if has_var_refs(value) {
                let new_value = interpolate_value(value, &snapshot);
                if new_value != *value {
                    *value = new_value;
                    changed = true;
                }
            }
        }

        if !changed {
            // No changes - check if there are still refs to existing keys (circular)
            for (key, value) in result.iter() {
                if has_resolvable_refs(value, &keys) {
                    return Err(EnvError::CircularRef(key.clone()));
                }
            }
            return Ok(result);
        }
    }

    // Exhausted iterations - must be circular
    for (key, value) in result.iter() {
        if has_resolvable_refs(value, &keys) {
            return Err(EnvError::CircularRef(key.clone()));
        }
    }

    Ok(result)
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
    fn test_unclosed_double_quote() {
        // With new parser, unclosed quote captures everything after opening quote
        let input = "FOO=\"bar'";
        let result = parse_env_str(input);
        assert_eq!(result.get("FOO"), Some(&"bar'".to_string()));
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

    // Interpolation tests

    #[test]
    fn test_interpolate_brace_syntax() {
        let input = "BASE=/home/user\nPATH=${BASE}/bin";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("PATH"), Some(&"/home/user/bin".to_string()));
    }

    #[test]
    fn test_interpolate_bare_syntax() {
        let input = "USER=alice\nGREETING=Hello $USER!";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("GREETING"), Some(&"Hello alice!".to_string()));
    }

    #[test]
    fn test_interpolate_multiple_refs() {
        let input = "HOST=localhost\nPORT=3000\nURL=http://${HOST}:${PORT}/api";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("URL"), Some(&"http://localhost:3000/api".to_string()));
    }

    #[test]
    fn test_interpolate_chain() {
        let input = "A=1\nB=${A}\nC=${B}";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("C"), Some(&"1".to_string()));
    }

    #[test]
    fn test_interpolate_undefined_kept() {
        let input = "PATH=${UNDEFINED}/bin";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("PATH"), Some(&"${UNDEFINED}/bin".to_string()));
    }

    #[test]
    fn test_interpolate_circular_error() {
        let mut env = HashMap::new();
        env.insert("A".to_string(), "${B}".to_string());
        env.insert("B".to_string(), "${A}".to_string());
        let result = interpolate_env(env);
        assert!(result.is_err());
    }

    #[test]
    fn test_interpolate_no_refs() {
        let input = "FOO=bar\nBAZ=qux";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(result.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn test_interpolate_lone_dollar() {
        let input = "PRICE=$50";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("PRICE"), Some(&"$50".to_string()));
    }

    #[test]
    fn test_interpolate_underscore_var() {
        let input = "MY_VAR=hello\nOTHER=${MY_VAR}_world";
        let env = parse_env_str(input);
        let result = interpolate_env(env).unwrap();
        assert_eq!(result.get("OTHER"), Some(&"hello_world".to_string()));
    }

    // Multiline tests

    #[test]
    fn test_multiline_double_quoted() {
        let input = "KEY=\"line1\nline2\nline3\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"line1\nline2\nline3".to_string()));
    }

    #[test]
    fn test_multiline_single_quoted() {
        let input = "KEY='line1\nline2'";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"line1\nline2".to_string()));
    }

    #[test]
    fn test_multiline_preserves_internal_quotes() {
        let input = "KEY=\"he said 'hello'\nand left\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"he said 'hello'\nand left".to_string()));
    }

    // Escape sequence tests

    #[test]
    fn test_escape_newline() {
        let input = "MSG=\"line1\\nline2\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("MSG"), Some(&"line1\nline2".to_string()));
    }

    #[test]
    fn test_escape_tab() {
        let input = "MSG=\"col1\\tcol2\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("MSG"), Some(&"col1\tcol2".to_string()));
    }

    #[test]
    fn test_escape_carriage_return() {
        let input = "MSG=\"line1\\rline2\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("MSG"), Some(&"line1\rline2".to_string()));
    }

    #[test]
    fn test_escape_double_quote() {
        let input = r#"MSG="say \"hello\"""#;
        let result = parse_env_str(input);
        assert_eq!(result.get("MSG"), Some(&"say \"hello\"".to_string()));
    }

    #[test]
    fn test_escape_backslash() {
        let input = r#"PATH="C:\\Users\\name""#;
        let result = parse_env_str(input);
        assert_eq!(result.get("PATH"), Some(&"C:\\Users\\name".to_string()));
    }

    #[test]
    fn test_escape_unknown_kept() {
        let input = "MSG=\"test\\xvalue\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("MSG"), Some(&"test\\xvalue".to_string()));
    }

    #[test]
    fn test_single_quote_no_escape() {
        let input = r"KEY='back\\slash'";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"back\\\\slash".to_string()));
    }

    #[test]
    fn test_line_continuation() {
        let input = "KEY=\"part1\\\npart2\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"part1part2".to_string()));
    }

    #[test]
    fn test_unquoted_no_escape() {
        let input = "KEY=value\\nhere";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"value\\nhere".to_string()));
    }

    #[test]
    fn test_inline_comment() {
        let input = "KEY=value # this is a comment";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_hash_in_quoted_value() {
        let input = "KEY=\"value # not a comment\"";
        let result = parse_env_str(input);
        assert_eq!(result.get("KEY"), Some(&"value # not a comment".to_string()));
    }

    #[test]
    fn test_multiple_vars_with_multiline() {
        let input = "A=first\nB=\"multi\nline\"\nC=third";
        let result = parse_env_str(input);
        assert_eq!(result.get("A"), Some(&"first".to_string()));
        assert_eq!(result.get("B"), Some(&"multi\nline".to_string()));
        assert_eq!(result.get("C"), Some(&"third".to_string()));
    }

    // Duplicate key tests
    #[test]
    fn test_duplicate_key_last_value_wins() {
        let input = "FOO=first\nFOO=second";
        let result = parse_env_str_with_warnings(input, false);
        assert_eq!(result.get("FOO"), Some(&"second".to_string()));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_duplicate_key_multiple_times() {
        let input = "FOO=1\nFOO=2\nFOO=3";
        let result = parse_env_str_with_warnings(input, false);
        assert_eq!(result.get("FOO"), Some(&"3".to_string()));
    }

    #[test]
    fn test_duplicate_with_different_types() {
        let input = "KEY=unquoted\nKEY=\"quoted\"";
        let result = parse_env_str_with_warnings(input, false);
        assert_eq!(result.get("KEY"), Some(&"quoted".to_string()));
    }
}
