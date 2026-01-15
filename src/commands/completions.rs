use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io;

/// Generate shell completions for the specified shell
pub fn run(shell: Shell) -> Result<(), String> {
    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "zenv", &mut io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_completions_generate() {
        // Just verify it does not panic
        let mut cmd = crate::Cli::command();
        let mut output = Vec::new();
        generate(Shell::Bash, &mut cmd, "zenv", &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("complete"));
    }

    #[test]
    fn test_zsh_completions_generate() {
        let mut cmd = crate::Cli::command();
        let mut output = Vec::new();
        generate(Shell::Zsh, &mut cmd, "zenv", &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("compdef"));
    }

    #[test]
    fn test_fish_completions_generate() {
        let mut cmd = crate::Cli::command();
        let mut output = Vec::new();
        generate(Shell::Fish, &mut cmd, "zenv", &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("complete"));
    }

    #[test]
    fn test_powershell_completions_generate() {
        let mut cmd = crate::Cli::command();
        let mut output = Vec::new();
        generate(Shell::PowerShell, &mut cmd, "zenv", &mut output);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("zenv"));
        assert!(output_str.contains("Register-ArgumentCompleter"));
    }
}
