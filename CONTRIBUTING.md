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

- Latest stable Rust ([1.99.0](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/) as of October 10, 2026); update with `rustup update stable`. CI also checks the supported minimum, Rust 1.85. Install via [rustup](https://rustup.rs/).
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

All tests should pass. CI also checks the minimum supported Rust version.

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
cargo test --test workflows

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
    mcp.rs         # stdio MCP tools, resources, and prompts
    scan.rs        # zenv scan command
    template.rs    # zenv template command
    version.rs     # zenv version command
```

## Preparing a release

Use Python 3.11 or newer for release tooling:

```bash
python scripts/release.py --check
python scripts/test_release.py
node --test scripts/test_sync_website.js
# Example next patch version; choose the intended unpublished version.
python scripts/release.py 0.3.14
```

The final command prepares a version bump in Cargo.toml, Cargo.lock, and the Action fallback. It does not publish, tag, commit, or change dependency versions. Update CHANGELOG.md and review the diff before creating a release tag. Tag builds must pass the reusable cross-platform CI workflow, tag/version agreement, and a locked package dry run before release artifacts or the crate are published. `cargo release` runs the locked release-profile tests.

The website lives in the separate private repository `Rex0Lux/zorl` on `master`. Its **Sync zenv release** workflow reads the public CLI release and verifies matching, non-yanked crates.io publication before updating metadata. It runs hourly at minute 17 with the website repository's own `GITHUB_TOKEN`; no cross-repository secret is required. GitHub can delay scheduled runs. For immediate synchronization after publishing, use the website account:

```bash
gh workflow run sync-zenv.yml --repo Rex0Lux/zorl --ref master
```

The workflow validates both destination files, refuses downgrades, commits only zenv metadata, and verifies the live version after Vercel deployment. Repeated runs without changes do not create commits. Feature descriptions, counts, examples, and privacy statements still require a separate content review. GitHub releases created with `GITHUB_TOKEN` do not trigger another release-event workflow ([GitHub documentation](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)), so do not rely on that event to update the separate website.

After website synchronization, use the CLI repository's read-only **Verify Website** workflow, or run `node scripts/verify-website.js` locally. It checks `/zenv`, `/zenv/docs`, and the zenv section of `/llms.txt` against the version in Cargo.toml. Set `EXPECTED_VERSION` to verify another published stable version. The older `scripts/sync-website.js` helper remains available for an explicitly reviewed local website checkout.

## Maintaining repository docs and the wiki

The CLI repository is `zorl-engine/zorath-env` on `main`. The wiki is a separate Git repository on `master`; updating the README does not update the wiki:

```bash
git clone https://github.com/zorl-engine/zorath-env.wiki.git
cd zorath-env.wiki
```

For each release, review all seven pages: Home, Installation, Quick-Start, Schema-Format, Commands, CI-Integration, and FAQ. Keep `_Sidebar.md` links in sync. Check examples and flags against the published binary, execute the quick start in a temporary directory, and validate schema/config examples against current source. Keep historical test records clearly labeled in `TESTED.md`; never reuse old results as a fresh test pass. Review and commit the wiki changes, then push its `master` branch with an account authorized for the CLI repository.

The v0.3.12 and v0.3.13 release runs published successfully but failed during the old cross-repository website checkout. Those historical runs retain their original workflows. The website-owned synchronization above replaces that credential dependency; no secrets were changed. Do not treat a published crate as proof that the website or wiki was updated.

## Questions and documentation

- Read the [official documentation](https://zorl.cloud/zenv/docs)
- Check the [FAQ](https://github.com/zorl-engine/zorath-env/wiki/FAQ)
- Open an issue for questions

Thank you for contributing!
