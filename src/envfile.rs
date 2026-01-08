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
