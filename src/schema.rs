use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SchemaError {
    #[error("failed to read schema file: {0}")]
    Read(String),
    #[error("invalid schema json: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VarType {
    String,
    Int,
    Float,
    Bool,
    Url,
    Enum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarSpec {
    #[serde(rename = "type")]
    pub var_type: VarType,

    #[serde(default)]
    pub required: bool,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub values: Option<Vec<String>>, // for enum

    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

pub type Schema = HashMap<String, VarSpec>;

pub fn load_schema(path: &str) -> Result<Schema, SchemaError> {
    let content = fs::read_to_string(path).map_err(|e| SchemaError::Read(e.to_string()))?;
    serde_json::from_str::<Schema>(&content).map_err(|e| SchemaError::Parse(e.to_string()))
}

pub fn save_schema(path: &str, schema: &Schema) -> Result<(), SchemaError> {
    let json = serde_json::to_string_pretty(schema).map_err(|e| SchemaError::Parse(e.to_string()))?;
    fs::write(path, json).map_err(|e| SchemaError::Read(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string_type() {
        let json = r#"{"FOO": {"type": "string", "required": true}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert!(matches!(spec.var_type, VarType::String));
        assert!(spec.required);
    }

    #[test]
    fn test_parse_int_type() {
        let json = r#"{"PORT": {"type": "int", "required": false, "default": 3000}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("PORT").unwrap();
        assert!(matches!(spec.var_type, VarType::Int));
        assert!(!spec.required);
        assert_eq!(spec.default, Some(serde_json::json!(3000)));
    }

    #[test]
    fn test_parse_float_type() {
        let json = r#"{"RATE": {"type": "float"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("RATE").unwrap();
        assert!(matches!(spec.var_type, VarType::Float));
    }

    #[test]
    fn test_parse_bool_type() {
        let json = r#"{"DEBUG": {"type": "bool", "default": false}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("DEBUG").unwrap();
        assert!(matches!(spec.var_type, VarType::Bool));
        assert_eq!(spec.default, Some(serde_json::json!(false)));
    }

    #[test]
    fn test_parse_url_type() {
        let json = r#"{"API_URL": {"type": "url", "required": true}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("API_URL").unwrap();
        assert!(matches!(spec.var_type, VarType::Url));
    }

    #[test]
    fn test_parse_enum_type() {
        let json = r#"{"NODE_ENV": {"type": "enum", "values": ["dev", "staging", "prod"]}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("NODE_ENV").unwrap();
        assert!(matches!(spec.var_type, VarType::Enum));
        assert_eq!(spec.values, Some(vec!["dev".to_string(), "staging".to_string(), "prod".to_string()]));
    }

    #[test]
    fn test_parse_description() {
        let json = r#"{"FOO": {"type": "string", "description": "A test variable"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert_eq!(spec.description, Some("A test variable".to_string()));
    }

    #[test]
    fn test_parse_multiple_vars() {
        let json = r#"{
            "FOO": {"type": "string"},
            "BAR": {"type": "int"},
            "BAZ": {"type": "bool"}
        }"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.len(), 3);
    }

    #[test]
    fn test_invalid_json_error() {
        let json = r#"{"FOO": {"type": "string""#;
        let result: Result<Schema, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_type_error() {
        let json = r#"{"FOO": {"type": "invalid_type"}}"#;
        let result: Result<Schema, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_required_defaults_to_false() {
        let json = r#"{"FOO": {"type": "string"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let spec = schema.get("FOO").unwrap();
        assert!(!spec.required);
    }

    #[test]
    fn test_roundtrip_serialization() {
        let json = r#"{"FOO":{"type":"string","required":true,"description":"Test"}}"#;
        let schema: Schema = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&schema).unwrap();
        let reparsed: Schema = serde_json::from_str(&serialized).unwrap();
        assert_eq!(schema.len(), reparsed.len());
    }
}
