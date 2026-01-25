# Contributing to zenv

Thank you for your interest in contributing to zenv (package: `zorath-env`, binary: `zenv`)! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## How to Contribute

### Reporting Bugs

Before submitting a bug report:

1. Check the [existing issues](https://github.com/zorl-engine/zorath-env/issues) to avoid duplicates
2. Use the latest version of zenv
3. Collect relevant information:
   - zenv version (`zenv --version`)
   - Operating system and version
   - Steps to reproduce
   - Expected vs actual behavior

[Open a bug report](https://github.com/zorl-engine/zorath-env/issues/new)

### Suggesting Features

Feature requests are welcome! Please:

1. Check existing issues for similar suggestions
2. Describe the problem your feature would solve
3. Explain your proposed solution
4. Consider if it fits zenv's scope (schema-based .env validation)

[Open a feature request](https://github.com/zorl-engine/zorath-env/issues/new)

## Development Setup

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))
- Git

### Clone and Build

```bash
git clone https://github.com/zorl-engine/zorath-env.git
cd zorath-env
cargo build
```

### Run Locally

```bash
cargo run -- check
cargo run -- docs
cargo run -- init
```

### Run Tests

```bash
cargo test
```

All 630 tests should pass.

### Check Code Quality

```bash
cargo fmt --check
cargo clippy
```

## Pull Request Process

### Before Submitting

1. Fork the repository
2. Create a feature branch from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```
3. Make your changes
4. Add tests for new functionality
5. Ensure all tests pass: `cargo test`
6. Run formatting: `cargo fmt`
7. Run linter: `cargo clippy`
8. Commit with clear messages

### Commit Messages

Use clear, descriptive commit messages:

```
Add validation rule for email type

- Implement email regex pattern
- Add tests for valid/invalid emails
- Update documentation
```

### Submitting

1. Push your branch to your fork
2. Open a PR against `main`
3. Fill out the PR template
4. Link any related issues

### PR Checklist

- [ ] Tests added for new functionality
- [ ] All tests passing (`cargo test`)
- [ ] Code formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation updated if needed
- [ ] Commit messages are clear

## Code Style

- Follow standard Rust conventions
- Use `cargo fmt` for formatting
- Address all `cargo clippy` warnings
- Keep functions focused and small
- Add comments for complex logic
- Use descriptive variable names

## Testing

### Running Tests

```bash
# All tests
cargo test

# Specific test file
cargo test --test schema

# Specific test
cargo test test_validate_int_min_max
```

### Writing Tests

- Add tests in the same file as the code (using `#[cfg(test)]` modules)
- Cover both success and failure cases
- Test edge cases
- Use descriptive test names

Example:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_int() {
        // Test implementation
    }

    #[test]
    fn test_parse_invalid_int_returns_error() {
        // Test implementation
    }
}
```

## Project Structure

```
src/
  main.rs          # CLI entry point
  schema.rs        # Schema parsing and types (JSON + YAML)
  envfile.rs       # .env file parsing
  remote.rs        # Remote schema fetching and caching
  secrets.rs       # Secret detection patterns
  config.rs        # .zenvrc config file support
  suggestions.rs   # "Did you mean?" suggestions
  presets.rs       # Framework presets (nextjs, rails, etc.)
  commands/
    mod.rs         # Command module declarations
    cache.rs       # zenv cache command
    check.rs       # zenv check command
    completions.rs # zenv completions command
    diff.rs        # zenv diff command
    docs.rs        # zenv docs command
    doctor.rs      # zenv doctor command
    example.rs     # zenv example command
    export.rs      # zenv export command
    fix.rs         # zenv fix command
    init.rs        # zenv init command
    scan.rs        # zenv scan command
    template.rs    # zenv template command
    version.rs     # zenv version command
```

## Questions?

- Read the [official documentation](https://zorl.cloud/zenv/docs)
- Check the [FAQ](https://github.com/zorl-engine/zorath-env/wiki/FAQ)
- Open an issue for questions

Thank you for contributing!
