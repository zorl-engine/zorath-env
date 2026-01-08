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
