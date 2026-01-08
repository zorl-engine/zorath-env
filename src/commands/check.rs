use std::collections::HashMap;
use std::path::Path;

use url::Url;

use crate::envfile;
use crate::schema::{self, VarType};

pub fn run(env_path: &str, schema_path: &str, allow_missing_env: bool) -> Result<(), String> {
    let schema = schema::load_schema(schema_path).map_err(|e| e.to_string())?;

    let env_map: HashMap<String, String> = if Path::new(env_path).exists() {
        envfile::parse_env_file(env_path).map_err(|e| e.to_string())?
    } else if allow_missing_env {
        HashMap::new()
    } else {
        return Err(format!("env file not found: {env_path}"));
    };

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
            VarType::String => { /* always ok */ }

            VarType::Int => {
                if value.parse::<i64>().is_err() {
                    errors.push(format!("{key}: expected int, got '{value}'"));
                }
            }

            VarType::Float => {
                if value.parse::<f64>().is_err() {
                    errors.push(format!("{key}: expected float, got '{value}'"));
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

    if !errors.is_empty() {
        eprintln!("zenv check failed:\n");
        for e in errors {
            eprintln!("- {e}");
        }
        return Err("validation failed".into());
    }

    println!("zenv: OK");
    Ok(())
}
