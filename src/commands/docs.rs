use crate::errors::CliError;
use crate::schema::{self, LoadOptions, Schema, VarType};

/// Generate documentation from a Schema (library function)
///
/// This is the unified library function that generates documentation
/// in the specified format. No file I/O.
///
/// # Arguments
/// * `schema` - The loaded Schema to document
/// * `format` - Output format: "markdown", "md", or "json"
///
/// # Returns
/// The generated documentation as a String, or an error for unknown format
pub fn generate(schema: &Schema, format: &str) -> Result<String, String> {
    match format {
        "markdown" | "md" => Ok(generate_markdown(schema)),
        "json" => generate_json(schema),
        _ => Err(format!(
            "unknown format '{}'. Use 'markdown' or 'json'",
            format
        )),
    }
}

/// Run the docs command (CLI function)
#[doc(hidden)]
pub fn run(
    schema_path: &str,
    format: &str,
    no_cache: bool,
    verify_hash: Option<&str>,
    ca_cert: Option<&str>,
    rate_limit_seconds: Option<u64>,
) -> Result<(), CliError> {
    let options = LoadOptions {
        no_cache,
        verify_hash: verify_hash.map(|s| s.to_string()),
        ca_cert: ca_cert.map(|s| s.to_string()),
        rate_limit_seconds,
    };
    let schema = schema::load_schema_with_options(schema_path, &options)
        .map_err(|e| CliError::Schema(e.to_string()))?;

    // Use the library function
    let output = generate(&schema, format).map_err(CliError::Input)?;

    print!("{}", output);
    Ok(())
}

/// Generate JSON documentation from schema
pub fn generate_json(schema: &Schema) -> Result<String, String> {
    serde_json::to_string_pretty(schema).map_err(|e| e.to_string())
}

/// Generate markdown documentation from schema
pub fn generate_markdown(schema: &Schema) -> String {
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
            VarType::Uuid => "uuid",
            VarType::Email => "email",
            VarType::Ipv4 => "ipv4",
            VarType::Ipv6 => "ipv6",
            VarType::Semver => "semver",
            VarType::Port => "port",
            VarType::Date => "date",
            VarType::Hostname => "hostname",
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
        // Show validation constraints
        if let Some(ref rules) = spec.validate {
            let mut constraints = Vec::new();
            if let Some(min) = rules.min {
                constraints.push(format!("min: {}", min));
            }
            if let Some(max) = rules.max {
                constraints.push(format!("max: {}", max));
            }
            if let Some(min_value) = rules.min_value {
                constraints.push(format!("min_value: {}", min_value));
            }
            if let Some(max_value) = rules.max_value {
                constraints.push(format!("max_value: {}", max_value));
            }
            if let Some(min_length) = rules.min_length {
                constraints.push(format!("min_length: {}", min_length));
            }
            if let Some(max_length) = rules.max_length {
                constraints.push(format!("max_length: {}", max_length));
            }
            if let Some(ref pattern) = rules.pattern {
                constraints.push(format!("pattern: {}", pattern));
            }
            if !constraints.is_empty() {
                output.push_str(&format!("- Constraints: `{}`\n", constraints.join(", ")));
            }
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
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn test_header_present() {
        let schema = make_schema(vec![]);
        let output = generate_markdown(&schema);
        assert!(output.starts_with("# Environment Variables"));
    }

    #[test]
    fn test_variable_header_format() {
        let schema = make_schema(vec![(
            "FOO",
            VarSpec {
                var_type: VarType::String,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("## `FOO`"));
    }

    #[test]
    fn test_type_displayed() {
        let schema = make_schema(vec![(
            "PORT",
            VarSpec {
                var_type: VarType::Int,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
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
            let schema = make_schema(vec![(
                name,
                VarSpec {
                    var_type,
                    required: false,
                    description: None,
                    values: None,
                    ..Default::default()
                },
            )]);
            let output = generate_markdown(&schema);
            assert!(output.contains(&format!("- Type: `{}`", expected)));
        }
    }

    #[test]
    fn test_required_true() {
        let schema = make_schema(vec![(
            "FOO",
            VarSpec {
                var_type: VarType::String,
                required: true,
                description: None,
                values: None,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("- Required: `true`"));
    }

    #[test]
    fn test_required_false() {
        let schema = make_schema(vec![(
            "FOO",
            VarSpec {
                var_type: VarType::String,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("- Required: `false`"));
    }

    #[test]
    fn test_default_displayed() {
        let schema = make_schema(vec![(
            "PORT",
            VarSpec {
                var_type: VarType::Int,
                default: Some(serde_json::json!(3000)),
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("- Default: `3000`"));
    }

    #[test]
    fn test_enum_values_displayed() {
        let schema = make_schema(vec![(
            "ENV",
            VarSpec {
                var_type: VarType::Enum,
                required: false,
                description: None,
                values: Some(vec!["dev".into(), "prod".into()]),
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("- Allowed: `dev, prod`"));
    }

    #[test]
    fn test_description_displayed() {
        let schema = make_schema(vec![(
            "API_KEY",
            VarSpec {
                var_type: VarType::String,
                required: true,
                description: Some("Your API key for authentication".into()),
                values: None,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("Your API key for authentication"));
    }

    #[test]
    fn test_keys_sorted_alphabetically() {
        let schema = make_schema(vec![
            (
                "ZEBRA",
                VarSpec {
                    var_type: VarType::String,
                    ..Default::default()
                },
            ),
            (
                "ALPHA",
                VarSpec {
                    var_type: VarType::String,
                    ..Default::default()
                },
            ),
            (
                "MIDDLE",
                VarSpec {
                    var_type: VarType::String,
                    ..Default::default()
                },
            ),
        ]);
        let output = generate_markdown(&schema);
        let alpha_pos = output.find("## `ALPHA`").unwrap();
        let middle_pos = output.find("## `MIDDLE`").unwrap();
        let zebra_pos = output.find("## `ZEBRA`").unwrap();
        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zebra_pos);
    }

    #[test]
    fn test_empty_schema() {
        let schema = make_schema(vec![]);
        let output = generate_markdown(&schema);
        assert_eq!(output, "# Environment Variables\n\n");
    }

    #[test]
    fn test_json_output_valid() {
        let schema = make_schema(vec![(
            "PORT",
            VarSpec {
                var_type: VarType::Int,
                required: true,
                description: Some("Server port".into()),
                default: Some(serde_json::json!(3000)),
                ..Default::default()
            },
        )]);
        let output = generate_json(&schema).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.get("PORT").is_some());
    }

    #[test]
    fn test_json_contains_all_fields() {
        let schema = make_schema(vec![(
            "API_KEY",
            VarSpec {
                var_type: VarType::String,
                required: true,
                description: Some("API key".into()),
                values: None,
                ..Default::default()
            },
        )]);
        let output = generate_json(&schema).unwrap();
        assert!(output.contains("\"type\""));
        assert!(output.contains("\"required\""));
        assert!(output.contains("\"description\""));
    }

    #[test]
    fn test_json_empty_schema() {
        let schema = make_schema(vec![]);
        let output = generate_json(&schema).unwrap();
        assert_eq!(output, "{}");
    }

    #[test]
    fn test_validation_rules_displayed() {
        use crate::schema::ValidationRule;
        let schema = make_schema(vec![(
            "PORT",
            VarSpec {
                var_type: VarType::Int,
                required: true,
                validate: Some(ValidationRule {
                    min: Some(1024),
                    max: Some(65535),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("- Constraints: `min: 1024, max: 65535`"));
    }

    #[test]
    fn test_validation_rules_string_constraints() {
        use crate::schema::ValidationRule;
        let schema = make_schema(vec![(
            "API_KEY",
            VarSpec {
                var_type: VarType::String,
                required: true,
                validate: Some(ValidationRule {
                    min_length: Some(32),
                    pattern: Some("^sk_".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(output.contains("min_length: 32"));
        assert!(output.contains("pattern: ^sk_"));
    }

    #[test]
    fn test_no_constraints_when_validate_none() {
        let schema = make_schema(vec![(
            "FOO",
            VarSpec {
                var_type: VarType::String,
                ..Default::default()
            },
        )]);
        let output = generate_markdown(&schema);
        assert!(!output.contains("Constraints"));
    }
}
