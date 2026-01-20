use clap::Command;
use clap_complete::{generate, Shell};
use std::io;

/// Generate shell completions for the specified shell.
///
/// Takes a clap Command instance from the caller (main.rs provides Cli::command()).
/// This decouples the library from the CLI definition.
pub fn run(shell: Shell, cmd: &mut Command) -> Result<(), String> {
    generate(shell, cmd, "zenv", &mut io::stdout());
    Ok(())
}

/// Generate completions to a writer (for testing).
pub fn generate_to_writer<W: io::Write>(shell: Shell, cmd: &mut Command, writer: &mut W) {
    generate(shell, cmd, "zenv", writer);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a mock command for testing (simulates the real CLI structure)
    fn mock_command() -> Command {
        Command::new("zenv")
            .subcommand(Command::new("check"))
            .subcommand(Command::new("docs"))
            .subcommand(Command::new("init"))
            .subcommand(Command::new("example"))
            .subcommand(Command::new("diff"))
            .subcommand(Command::new("fix"))
            .subcommand(Command::new("scan"))
            .subcommand(Command::new("cache"))
            .subcommand(Command::new("doctor"))
            .subcommand(Command::new("export"))
            .subcommand(Command::new("completions"))
            .subcommand(Command::new("version"))
    }

    #[test]
    fn test_bash_completions_generate() {
        let mut cmd = mock_command();
        let mut output = Vec::new();
        generate_to_writer(Shell::Bash, &mut cmd, &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("complete"));
    }

    #[test]
    fn test_zsh_completions_generate() {
        let mut cmd = mock_command();
        let mut output = Vec::new();
        generate_to_writer(Shell::Zsh, &mut cmd, &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("compdef"));
    }

    #[test]
    fn test_fish_completions_generate() {
        let mut cmd = mock_command();
        let mut output = Vec::new();
        generate_to_writer(Shell::Fish, &mut cmd, &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("complete"));
    }

    #[test]
    fn test_powershell_completions_generate() {
        let mut cmd = mock_command();
        let mut output = Vec::new();
        generate_to_writer(Shell::PowerShell, &mut cmd, &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("Register-ArgumentCompleter"));
    }
}
