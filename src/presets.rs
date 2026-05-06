use crate::schema::{Schema, ValidationRule, VarSpec, VarType};

/// Available framework presets
pub const AVAILABLE_PRESETS: &[&str] =
    &["nextjs", "rails", "django", "fastapi", "express", "laravel"];

/// Get a preset schema by name
pub fn get_preset(name: &str) -> Option<Schema> {
    match name.to_lowercase().as_str() {
        "nextjs" => Some(nextjs_preset()),
        "rails" => Some(rails_preset()),
        "django" => Some(django_preset()),
        "fastapi" => Some(fastapi_preset()),
        "express" => Some(express_preset()),
        "laravel" => Some(laravel_preset()),
        _ => None,
    }
}

/// Next.js preset
fn nextjs_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "NODE_ENV".to_string(),
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: Some("Node.js environment".to_string()),
            values: Some(vec![
                "development".to_string(),
                "production".to_string(),
                "test".to_string(),
            ]),
            default: Some(serde_json::json!("development")),
            ..Default::default()
        },
    );

    schema.insert(
        "NEXT_PUBLIC_APP_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Public application URL".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "DATABASE_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Database connection string".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "NEXTAUTH_SECRET".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("NextAuth.js secret for JWT signing".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(32),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "NEXTAUTH_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: false,
            description: Some(
                "NextAuth.js callback URL (defaults to NEXT_PUBLIC_APP_URL)".to_string(),
            ),
            ..Default::default()
        },
    );

    schema
}

/// Ruby on Rails preset
fn rails_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "RAILS_ENV".to_string(),
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: Some("Rails environment".to_string()),
            values: Some(vec![
                "development".to_string(),
                "production".to_string(),
                "test".to_string(),
            ]),
            default: Some(serde_json::json!("development")),
            ..Default::default()
        },
    );

    schema.insert(
        "DATABASE_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Database connection string".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "SECRET_KEY_BASE".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Rails secret key for session encryption".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(64),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "REDIS_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: false,
            description: Some("Redis connection string for caching/jobs".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "RAILS_MASTER_KEY".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("Rails credentials master key".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(32),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema
}

/// Django preset
fn django_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "DJANGO_SECRET_KEY".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Django secret key for cryptographic signing".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(50),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "DEBUG".to_string(),
        VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: Some("Enable Django debug mode".to_string()),
            default: Some(serde_json::json!(false)),
            ..Default::default()
        },
    );

    schema.insert(
        "DATABASE_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Database connection string".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "ALLOWED_HOSTS".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Comma-separated list of allowed hosts".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "DJANGO_SETTINGS_MODULE".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("Python path to Django settings module".to_string()),
            ..Default::default()
        },
    );

    schema
}

/// FastAPI preset
fn fastapi_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "DATABASE_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Database connection string".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "SECRET_KEY".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Secret key for JWT tokens".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(32),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "DEBUG".to_string(),
        VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: Some("Enable debug mode".to_string()),
            default: Some(serde_json::json!(false)),
            ..Default::default()
        },
    );

    schema.insert(
        "CORS_ORIGINS".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("Comma-separated list of allowed CORS origins".to_string()),
            default: Some(serde_json::json!("http://localhost:3000")),
            ..Default::default()
        },
    );

    schema.insert(
        "API_PREFIX".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("API route prefix".to_string()),
            default: Some(serde_json::json!("/api/v1")),
            ..Default::default()
        },
    );

    schema
}

/// Express.js preset
fn express_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "NODE_ENV".to_string(),
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: Some("Node.js environment".to_string()),
            values: Some(vec![
                "development".to_string(),
                "production".to_string(),
                "test".to_string(),
            ]),
            default: Some(serde_json::json!("development")),
            ..Default::default()
        },
    );

    schema.insert(
        "PORT".to_string(),
        VarSpec {
            var_type: VarType::Int,
            required: false,
            description: Some("Server port".to_string()),
            default: Some(serde_json::json!(3000)),
            validate: Some(ValidationRule {
                min: Some(1),
                max: Some(65535),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "DATABASE_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Database connection string".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "SESSION_SECRET".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Express session secret".to_string()),
            validate: Some(ValidationRule {
                min_length: Some(32),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "CORS_ORIGIN".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("Allowed CORS origin".to_string()),
            default: Some(serde_json::json!("*")),
            ..Default::default()
        },
    );

    schema
}

/// Laravel preset
fn laravel_preset() -> Schema {
    let mut schema = Schema::new();

    schema.insert(
        "APP_ENV".to_string(),
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: Some("Application environment".to_string()),
            values: Some(vec![
                "local".to_string(),
                "staging".to_string(),
                "production".to_string(),
            ]),
            default: Some(serde_json::json!("local")),
            ..Default::default()
        },
    );

    schema.insert(
        "APP_KEY".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Laravel application key (base64:...)".to_string()),
            validate: Some(ValidationRule {
                pattern: Some("^base64:.+$".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    schema.insert(
        "APP_DEBUG".to_string(),
        VarSpec {
            var_type: VarType::Bool,
            required: false,
            description: Some("Enable debug mode".to_string()),
            default: Some(serde_json::json!(false)),
            ..Default::default()
        },
    );

    schema.insert(
        "APP_URL".to_string(),
        VarSpec {
            var_type: VarType::Url,
            required: true,
            description: Some("Application URL".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "DB_CONNECTION".to_string(),
        VarSpec {
            var_type: VarType::Enum,
            required: false,
            description: Some("Database driver".to_string()),
            values: Some(vec![
                "mysql".to_string(),
                "pgsql".to_string(),
                "sqlite".to_string(),
                "sqlsrv".to_string(),
            ]),
            default: Some(serde_json::json!("mysql")),
            ..Default::default()
        },
    );

    schema.insert(
        "DB_HOST".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: false,
            description: Some("Database host".to_string()),
            default: Some(serde_json::json!("127.0.0.1")),
            ..Default::default()
        },
    );

    schema.insert(
        "DB_DATABASE".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Database name".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "DB_USERNAME".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Database username".to_string()),
            ..Default::default()
        },
    );

    schema.insert(
        "DB_PASSWORD".to_string(),
        VarSpec {
            var_type: VarType::String,
            required: true,
            description: Some("Database password".to_string()),
            ..Default::default()
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_presets() {
        assert!(AVAILABLE_PRESETS.contains(&"nextjs"));
        assert!(AVAILABLE_PRESETS.contains(&"rails"));
        assert!(AVAILABLE_PRESETS.contains(&"django"));
        assert!(AVAILABLE_PRESETS.contains(&"fastapi"));
        assert!(AVAILABLE_PRESETS.contains(&"express"));
        assert!(AVAILABLE_PRESETS.contains(&"laravel"));
    }

    #[test]
    fn test_get_preset_nextjs() {
        let schema = get_preset("nextjs").unwrap();
        assert!(schema.contains_key("NODE_ENV"));
        assert!(schema.contains_key("DATABASE_URL"));
        assert!(schema.contains_key("NEXTAUTH_SECRET"));
    }

    #[test]
    fn test_get_preset_rails() {
        let schema = get_preset("rails").unwrap();
        assert!(schema.contains_key("RAILS_ENV"));
        assert!(schema.contains_key("SECRET_KEY_BASE"));
    }

    #[test]
    fn test_get_preset_django() {
        let schema = get_preset("django").unwrap();
        assert!(schema.contains_key("DJANGO_SECRET_KEY"));
        assert!(schema.contains_key("DEBUG"));
        assert!(schema.contains_key("ALLOWED_HOSTS"));
    }

    #[test]
    fn test_get_preset_fastapi() {
        let schema = get_preset("fastapi").unwrap();
        assert!(schema.contains_key("SECRET_KEY"));
        assert!(schema.contains_key("CORS_ORIGINS"));
    }

    #[test]
    fn test_get_preset_express() {
        let schema = get_preset("express").unwrap();
        assert!(schema.contains_key("PORT"));
        assert!(schema.contains_key("SESSION_SECRET"));
    }

    #[test]
    fn test_get_preset_laravel() {
        let schema = get_preset("laravel").unwrap();
        assert!(schema.contains_key("APP_KEY"));
        assert!(schema.contains_key("APP_ENV"));
        assert!(schema.contains_key("DB_CONNECTION"));
    }

    #[test]
    fn test_get_preset_case_insensitive() {
        assert!(get_preset("NextJS").is_some());
        assert!(get_preset("RAILS").is_some());
        assert!(get_preset("Django").is_some());
    }

    #[test]
    fn test_get_preset_invalid() {
        assert!(get_preset("invalid").is_none());
        assert!(get_preset("").is_none());
    }

    #[test]
    fn test_preset_has_descriptions() {
        let schema = get_preset("nextjs").unwrap();
        for (_, spec) in schema.iter() {
            assert!(
                spec.description.is_some(),
                "All preset vars should have descriptions"
            );
        }
    }
}
