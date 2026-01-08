use std::path::Path;

use crate::envfile;
use crate::schema::{self, Schema, VarSpec, VarType};

pub fn run(example_path: &str, schema_path: &str) -> Result<(), String> {
    if !Path::new(example_path).exists() {
        return Err(format!("example file not found: {example_path}"));
    }

    if Path::new(schema_path).exists() {
        eprintln!("warning: overwriting existing {schema_path}");
    }

    let env = envfile::parse_env_file(example_path).map_err(|e| e.to_string())?;

    let mut schema_map: Schema = Schema::new();

    for (k, v) in env {
        let inferred = infer_type(&v);

        schema_map.insert(k, VarSpec {
            var_type: inferred,
            required: true,
            description: Some("TODO: describe this variable".into()),
            values: None,
            default: None,
        });
    }

    schema::save_schema(schema_path, &schema_map).map_err(|e| e.to_string())?;
    println!("zenv: wrote schema to {schema_path}");
    Ok(())
}

fn infer_type(v: &str) -> VarType {
    let lv = v.trim().to_lowercase();

    if lv == "true" || lv == "false" || lv == "1" || lv == "0" || lv == "yes" || lv == "no" {
        return VarType::Bool;
    }
    if v.parse::<i64>().is_ok() {
        return VarType::Int;
    }
    if v.parse::<f64>().is_ok() {
        return VarType::Float;
    }
    if url::Url::parse(v).is_ok() {
        return VarType::Url;
    }
    VarType::String
}
