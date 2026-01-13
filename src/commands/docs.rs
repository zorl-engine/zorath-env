use crate::schema::{self, Schema, VarType};

pub fn run(schema_path: &str) -> Result<(), String> {
    let schema = schema::load_schema(schema_path).map_err(|e| e.to_string())?;
    let output = generate_docs(&schema);
    print!("{}", output);
    Ok(())
}

/// Generate markdown documentation from schema
pub fn generate_docs(schema: &Schema) -> String {
    let mut output = String::from("# Environment Variables\n\n");

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

        output.push_str(&format!("## `{}`\n", key));
        output.push_str(&format!("- Type: `{}`\n", ty));
        output.push_str(&format!("- Required: `{}`\n", spec.required));

        if let Some(d) = &spec.default {
            output.push_str(&format!("- Default: `{}`\n", d));
        }
        if let Some(vals) = &spec.values {
            output.push_str(&format!("- Allowed: `{}`\n", vals.join(", ")));
        }
        if let Some(desc) = &spec.description {
            output.push_str(&format!("\n{}\n\n", desc.trim()));
        } else {
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{VarSpec, VarType};

    fn make_schema(entries: Vec<(&str, VarSpec)>) -> Schema {
        entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn test_header_present() {
        let schema = make_schema(vec![]);
        let output = generate_docs(&schema);
        assert!(output.starts_with("# Environment Variables"));
    }

    #[test]
    fn test_variable_header_format() {
        let schema = make_schema(vec![("FOO", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("## `FOO`"));
    }

    #[test]
    fn test_type_displayed() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("- Type: `int`"));
    }

    #[test]
    fn test_all_types_displayed() {
        let types = vec![
            ("S", VarType::String, "string"),
            ("I", VarType::Int, "int"),
            ("F", VarType::Float, "float"),
            ("B", VarType::Bool, "bool"),
            ("U", VarType::Url, "url"),
            ("E", VarType::Enum, "enum"),
        ];
        for (name, var_type, expected) in types {
            let schema = make_schema(vec![(name, VarSpec {
                var_type,
                required: false,
                description: None,
                values: None,
                default: None,
                validate: None,
            })]);
            let output = generate_docs(&schema);
            assert!(output.contains(&format!("- Type: `{}`", expected)));
        }
    }

    #[test]
    fn test_required_true() {
        let schema = make_schema(vec![("FOO", VarSpec {
            var_type: VarType::String,
            required: true,
            description: None,
            values: None,
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("- Required: `true`"));
    }

    #[test]
    fn test_required_false() {
        let schema = make_schema(vec![("FOO", VarSpec {
            var_type: VarType::String,
            required: false,
            description: None,
            values: None,
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("- Required: `false`"));
    }

    #[test]
    fn test_default_displayed() {
        let schema = make_schema(vec![("PORT", VarSpec {
            var_type: VarType::Int,
            required: false,
            description: None,
            values: None,
            default: Some(serde_json::json!(3000)),
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("- Default: `3000`"));
    }

    #[test]
    fn test_enum_values_displayed() {
        let schema = make_schema(vec![("ENV", VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: None,
            values: Some(vec!["dev".into(), "prod".into()]),
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("- Allowed: `dev, prod`"));
    }

    #[test]
    fn test_description_displayed() {
        let schema = make_schema(vec![("API_KEY", VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Your API key for authentication".into()),
            values: None,
            default: None,
            validate: None,
        })]);
        let output = generate_docs(&schema);
        assert!(output.contains("Your API key for authentication"));
    }

    #[test]
    fn test_keys_sorted_alphabetically() {
        let schema = make_schema(vec![
            ("ZEBRA", VarSpec { var_type: VarType::String, required: false, description: None, values: None, default: None, validate: None }),
            ("ALPHA", VarSpec { var_type: VarType::String, required: false, description: None, values: None, default: None, validate: None }),
            ("MIDDLE", VarSpec { var_type: VarType::String, required: false, description: None, values: None, default: None, validate: None }),
        ]);
        let output = generate_docs(&schema);
        let alpha_pos = output.find("## `ALPHA`").unwrap();
        let middle_pos = output.find("## `MIDDLE`").unwrap();
        let zebra_pos = output.find("## `ZEBRA`").unwrap();
        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zebra_pos);
    }

    #[test]
    fn test_empty_schema() {
        let schema = make_schema(vec![]);
        let output = generate_docs(&schema);
        assert_eq!(output, "# Environment Variables\n\n");
    }
}
