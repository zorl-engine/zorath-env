use thiserror::Error;

/// Structured CLI error with exit code mapping.
///
/// Maps error categories to specific exit codes for CI/CD integration:
/// - Exit 1: Validation failures (schema rules violated)
/// - Exit 2: Input/file errors (not found, failed to read/write)
/// - Exit 3: Schema errors (invalid schema, parse failure)
#[derive(Error, Debug)]
pub enum CliError {
    /// Validation failures (exit code 1)
    #[error("{0}")]
    Validation(String),

    /// Input/file errors (exit code 2)
    #[error("{0}")]
    Input(String),

    /// Schema errors (exit code 3)
    #[error("{0}")]
    Schema(String),
}

impl CliError {
    /// Map error variant to process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Validation(_) => 1,
            CliError::Input(_) => 2,
            CliError::Schema(_) => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(CliError::Validation("x".into()).exit_code(), 1);
        assert_eq!(CliError::Input("x".into()).exit_code(), 2);
        assert_eq!(CliError::Schema("x".into()).exit_code(), 3);
    }

    #[test]
    fn test_display() {
        let e = CliError::Validation("validation failed".into());
        assert_eq!(e.to_string(), "validation failed");

        let e = CliError::Input("file not found".into());
        assert_eq!(e.to_string(), "file not found");

        let e = CliError::Schema("invalid json".into());
        assert_eq!(e.to_string(), "invalid json");
    }

    #[test]
    fn test_debug() {
        let e = CliError::Validation("test".into());
        let debug = format!("{:?}", e);
        assert!(debug.contains("Validation"));
    }
}
