use crate::schema::{self, VarType};

pub fn run(schema_path: &str) -> Result<(), String> {
    let schema = schema::load_schema(schema_path).map_err(|e| e.to_string())?;

    println!("# Environment Variables\n");

    let mut keys: Vec<_> = schema.keys().cloned().collect();
    keys.sort();

    for key in keys {
        let spec = &schema[&key];
        let ty = match spec.var_type {
            VarType::String => "string",
            VarType::Int => "int",
            VarType::Float => "float",
            VarType::Bool => "bool",
            VarType::Url => "url",
            VarType::Enum => "enum",
        };

        println!("## `{}`", key);
        println!("- Type: `{}`", ty);
        println!("- Required: `{}`", spec.required);

        if let Some(d) = &spec.default {
            println!("- Default: `{}`", d);
        }
        if let Some(vals) = &spec.values {
            println!("- Allowed: `{}`", vals.join(", "));
        }
        if let Some(desc) = &spec.description {
            println!("\n{}\n", desc.trim());
        } else {
            println!();
        }
    }

    Ok(())
}
