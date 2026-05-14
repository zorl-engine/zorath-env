use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use zorath_env::commands;
use zorath_env::config::Config;

#[derive(Parser, Debug)]
#[command(
    name = "zenv",
    version,
    about = "Validate .env files with a schema and generate docs."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to config file (default: .zenvrc in current or parent directories)
    #[arg(long, global = true)]
    config: Option<String>,

    /// Show verbose diagnostic output
    #[arg(long, global = true, conflicts_with = "quiet", env = "ZENV_VERBOSE")]
    verbose: bool,

    /// Suppress all diagnostic output
    #[arg(long, global = true, conflicts_with = "verbose", env = "ZENV_QUIET")]
    quiet: bool,

    /// Disable colored output (also respects NO_COLOR env var and .zenvrc)
    #[arg(long, global = true, env = "NO_COLOR")]
    no_color: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Validate .env against schema
    #[command(after_help = "\
Examples:
  zenv check                            Validate using defaults
  zenv check --schema custom.json       Use custom schema
  zenv check --detect-secrets           Include secret detection
  zenv check --watch                    Watch for file changes
  zenv check --env .env.local           Validate specific env file
  zenv check --format json              JSON output for CI/CD
  zenv check --allow-missing-env        Schema-only validation (no .env required)

Security options for remote schemas:
  zenv check --schema https://... --verify-hash abc123...
  zenv check --schema https://... --ca-cert /path/to/ca.pem

Config file:
  Create .zenvrc in your project root to set defaults:
  {\"schema\": \"env.schema.json\", \"detect_secrets\": true}")]
    Check {
        /// Path to .env file (default: .env, or from .zenvrc)
        #[arg(long)]
        env: Option<String>,
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// If set, missing .env is allowed (schema still validated against defaults/required rules)
        #[arg(long)]
        allow_missing_env: Option<bool>,
        /// Detect potential secrets in .env file (API keys, passwords, tokens)
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        detect_secrets: Option<bool>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Watch for file changes and re-run validation
        #[arg(long, default_value_t = false)]
        watch: bool,
        /// Output format: text or json (default: text, or from .zenvrc)
        #[arg(long)]
        format: Option<String>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Generate documentation from schema
    #[command(after_help = "\
Examples:
  zenv docs                             Generate markdown docs
  zenv docs --format json               Generate JSON output
  zenv docs --schema https://...        Use remote schema
  zenv docs --schema https://... --verify-hash abc123...")]
    Docs {
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Output format: markdown or json (default: markdown, or from .zenvrc)
        #[arg(short = 'f', long)]
        format: Option<String>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Create a starter schema from .env.example or preset
    #[command(after_help = "\
Examples:
  zenv init                             Create schema from .env.example
  zenv init --example .env              Use .env as source
  zenv init --preset nextjs             Use Next.js preset
  zenv init --preset rails              Use Rails preset
  zenv init --list-presets              Show available presets

Available presets:
  nextjs, rails, django, fastapi, express, laravel")]
    Init {
        /// Source .env file to infer types from
        #[arg(long, default_value = ".env.example")]
        example: String,
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Use a framework preset (nextjs, rails, django, fastapi, express, laravel)
        #[arg(long)]
        preset: Option<String>,
        /// List available presets
        #[arg(long, default_value_t = false)]
        list_presets: bool,
    },

    /// Show version and optionally check for updates
    #[command(after_help = "\
Examples:
  zenv version                          Show current version
  zenv version --check-update           Check for newer version")]
    Version {
        /// Check crates.io for newer version
        #[arg(long, default_value_t = false)]
        check_update: bool,
    },

    /// Generate shell completions
    #[command(after_help = "\
Examples:
  zenv completions bash                 Generate bash completions
  zenv completions zsh                  Generate zsh completions
  zenv completions fish                 Generate fish completions
  zenv completions powershell           Generate PowerShell completions

Installation:
  bash:  zenv completions bash >> ~/.bashrc
  zsh:   zenv completions zsh >> ~/.zshrc
  fish:  zenv completions fish > ~/.config/fish/completions/zenv.fish")]
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell)
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Generate .env.example from schema
    #[command(after_help = "\
Examples:
  zenv example                          Print to stdout
  zenv example --output .env.example    Write to file
  zenv example --include-defaults       Include schema defaults
  zenv example --schema https://... --verify-hash abc123...")]
    Example {
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Output file path (defaults to stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// Include default values in output
        #[arg(long, default_value_t = false)]
        include_defaults: bool,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Compare two .env files
    #[command(after_help = "\
Examples:
  zenv diff .env.local .env.prod        Compare two env files
  zenv diff .env.dev .env --schema s.json   With schema validation
  zenv diff .env.a .env.b --format json     JSON output for CI

Remote schema with integrity verification:
  zenv diff .env.a .env.b --schema https://... --verify-hash abc123...")]
    Diff {
        /// First .env file
        env_a: String,
        /// Second .env file
        env_b: String,
        /// Optional schema to check compliance
        #[arg(long)]
        schema: Option<String>,
        /// Output format: text or json (default: text, or from .zenvrc)
        #[arg(long)]
        format: Option<String>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Auto-fix common .env issues
    #[command(after_help = "\
Examples:
  zenv fix --dry-run                    Preview fixes without changing files
  zenv fix                              Apply fixes (creates .env.backup)
  zenv fix --remove-unknown             Also remove keys not in schema

Remote schema with integrity verification:
  zenv fix --schema https://... --verify-hash abc123...

What it fixes:
  - Adds missing required variables (with schema defaults)
  - Removes unknown keys (with --remove-unknown)

What it reports but doesn't fix:
  - Invalid types (can't guess correct value)
  - Invalid enum values (needs human choice)")]
    Fix {
        /// Path to .env file (default: .env, or from .zenvrc)
        #[arg(long)]
        env: Option<String>,
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Remove keys not defined in schema
        #[arg(long, default_value_t = false)]
        remove_unknown: bool,
        /// Preview changes without modifying files
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Scan source code for environment variable usage
    #[command(after_help = "\
Examples:
  zenv scan                             Scan current directory
  zenv scan --path ./src                Scan specific directory
  zenv scan --show-unused               Show vars in schema but not in code
  zenv scan --show-paths                Show file:line for all found vars
  zenv scan --format json               JSON output for CI

Supported languages:
  JavaScript/TypeScript, Python, Go, Rust, PHP, Ruby, Java, C#, Kotlin")]
    Scan {
        /// Directory to scan
        #[arg(long, default_value = ".")]
        path: String,
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Show variables in schema but not found in code
        #[arg(long, default_value_t = false)]
        show_unused: bool,
        /// Show file:line paths for all found variables
        #[arg(long, default_value_t = false)]
        show_paths: bool,
        /// Output format: text or json (default: text, or from .zenvrc)
        #[arg(long)]
        format: Option<String>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Manage remote schema cache
    #[command(after_help = "\
Examples:
  zenv cache list                       List cached schemas
  zenv cache stats                      Show cache statistics
  zenv cache clear                      Clear all cached schemas
  zenv cache clear https://...          Clear specific URL
  zenv cache path                       Show cache directory")]
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Export .env to various formats
    #[command(after_help = "\
Examples:
  zenv export --format shell            Shell script (export FOO=\"bar\")
  zenv export --format docker           Dockerfile (ENV FOO=bar)
  zenv export --format k8s              Kubernetes ConfigMap YAML
  zenv export --format json             JSON object
  zenv export --format systemd          systemd Environment directives
  zenv export --format dotenv           Standard .env format
  zenv export --format github-secrets   GitHub CLI commands (gh secret set)

  zenv export --env .env.local          Export specific env file
  zenv export --schema s.json           Only export vars defined in schema
  zenv export -f shell -o setup.sh      Write to file

Aliases:
  shell: bash, sh
  docker: dockerfile
  k8s: kubernetes, configmap
  systemd: service
  dotenv: env
  github-secrets: gh-secrets, github")]
    Export {
        /// Path to .env file to export (default: .env, or from .zenvrc)
        #[arg(long)]
        env: Option<String>,
        /// Output format: shell, docker, k8s, json, systemd, dotenv, github-secrets (default: shell, or from .zenvrc)
        #[arg(short = 'f', long)]
        format: Option<String>,
        /// Optional schema to filter variables
        #[arg(long)]
        schema: Option<String>,
        /// Output file path (defaults to stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Run health check and diagnostics
    #[command(after_help = "\
Examples:
  zenv doctor                           Run full health check
  zenv doctor --schema custom.json      Use custom schema path
  zenv doctor --env .env.local          Use specific env file
  zenv doctor --schema https://... --verify-hash abc123...
  zenv doctor --schema https://... --ca-cert /path/to/ca.pem

Checks:
  - Schema file exists and is valid
  - .env file exists and parses correctly
  - Config file (.zenvrc) is valid JSON
  - Remote schema cache is accessible
  - Validation passes (if schema and env exist)

Each check shows:
  [OK]    - No issues
  [WARN]  - Non-critical issue
  [ERROR] - Critical issue that needs attention")]
    Doctor {
        /// Path to .env file (default: .env, or from .zenvrc)
        #[arg(long)]
        env: Option<String>,
        /// Path to schema file (default: env.schema.json, or from .zenvrc)
        #[arg(long)]
        schema: Option<String>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Verify remote schema integrity with SHA-256 hash
        #[arg(long)]
        verify_hash: Option<String>,
        /// Custom CA certificate for enterprise TLS (PEM format)
        #[arg(long)]
        ca_cert: Option<String>,
    },

    /// Generate CI/CD configuration templates
    #[command(after_help = "\
Examples:
  zenv template github              Output GitHub Actions workflow
  zenv template gitlab -o .gitlab-ci.yml  Write GitLab CI config to file
  zenv template circleci            Output CircleCI config
  zenv template --list              List available templates
  zenv template github --use-binary Use binary download (faster CI)

Aliases:
  github: gh, github-actions
  gitlab: gl, gitlab-ci
  circleci: circle")]
    Template {
        /// Template name (github, gitlab, circleci)
        #[arg(default_value = "github")]
        name: String,
        /// Output file path (defaults to stdout)
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// List available templates
        #[arg(long)]
        list: bool,
        /// Use binary download instead of cargo install (faster)
        #[arg(long)]
        use_binary: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CacheAction {
    /// List cached remote schemas
    List,
    /// Clear cached schemas (all or specific URL)
    Clear {
        /// URL to clear from cache (omit to clear all)
        url: Option<String>,
    },
    /// Print cache directory path
    Path,
    /// Show cache statistics (size, age, expiry)
    Stats,
}

/// Pure boolish-value test. Returns false for the standard falsy strings
/// (empty, "false", "0", "no", "n", "off") and true for anything else --
/// liberal acceptance matching the no-color.org spec ("set to a value
/// other than the empty string") and the convention most CLIs honor for
/// boolean env vars. Extracted from normalize_boolish_env so the spec
/// compliance is unit-testable without mutating real process env vars.
fn is_truthy_boolish(value: &str) -> bool {
    let trimmed = value.trim().to_ascii_lowercase();
    !(trimmed.is_empty()
        || trimmed == "false"
        || trimmed == "0"
        || trimmed == "no"
        || trimmed == "n"
        || trimmed == "off")
}

/// Normalize a boolish env var so both clap's strict bool parser AND
/// internal `env::var(name).is_err()` presence checks match user intent.
/// Truthy values (per `is_truthy_boolish`) are canonicalized to literal
/// "true" -- clap 4.x rejects "1" for `env=` on a SetTrue bool flag, but
/// accepts "true"; internal modules only check presence so the specific
/// value doesn't matter to them. Falsy values are REMOVED entirely so
/// presence checks correctly return Err. Without this, `ZENV_QUIET=0`
/// would be indistinguishable from `ZENV_QUIET=1` (both leave the var
/// SET).
fn normalize_boolish_env(name: &str) {
    if let Ok(val) = std::env::var(name) {
        if is_truthy_boolish(&val) {
            std::env::set_var(name, "true");
        } else {
            std::env::remove_var(name);
        }
    }
}

fn main() {
    // Pre-normalize boolish env vars BEFORE Cli::parse() so users can set
    // NO_COLOR=1, ZENV_QUIET=yes, ZENV_VERBOSE=on, etc. without hitting
    // clap's strict bool parser. See normalize_boolish_env for rationale.
    normalize_boolish_env("ZENV_QUIET");
    normalize_boolish_env("ZENV_VERBOSE");
    normalize_boolish_env("NO_COLOR");

    let cli = Cli::parse();

    // Set verbosity via env vars so modules can check without threading args
    if cli.quiet {
        std::env::set_var("ZENV_QUIET", "1");
    }
    if cli.verbose {
        std::env::set_var("ZENV_VERBOSE", "1");
    }

    // Load config from specified path or .zenvrc (if present)
    let config = Config::load_from(cli.config.as_deref()).unwrap_or_default();

    // Set NO_COLOR if flag or config requests it
    if cli.no_color || config.no_color_or(false) {
        std::env::set_var("NO_COLOR", "1");
    }

    // Pull rate_limit_seconds from .zenvrc once and pass to every command that
    // hits the network. None means "use the built-in default", so this is safe
    // even when the field is absent from the config.
    let rate_limit = config.rate_limit_seconds();

    let result = match cli.command {
        Command::Check {
            env,
            schema,
            allow_missing_env,
            detect_secrets,
            no_cache,
            watch,
            format,
            verify_hash,
            ca_cert,
        } => {
            // CLI args override config, config overrides defaults
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let allow_missing_env =
                allow_missing_env.unwrap_or_else(|| config.allow_missing_env_or(false));
            let detect_secrets = detect_secrets.unwrap_or_else(|| config.detect_secrets_or(false));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            let format = format.unwrap_or_else(|| config.format_or("text"));
            commands::check::run(
                &env,
                &schema,
                allow_missing_env,
                detect_secrets,
                no_cache,
                watch,
                &format,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Docs {
            schema,
            format,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            let format = format.unwrap_or_else(|| config.format_or("markdown"));
            commands::docs::run(
                &schema,
                &format,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Init {
            example,
            schema,
            preset,
            list_presets,
        } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            commands::init::run_with_options(&example, &schema, preset.as_deref(), list_presets)
        }
        Command::Version { check_update } => commands::version::run(check_update),
        Command::Completions { shell } => commands::completions::run(shell, &mut Cli::command()),
        Command::Example {
            schema,
            output,
            include_defaults,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::example::run(
                &schema,
                output.as_deref(),
                include_defaults,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Diff {
            env_a,
            env_b,
            schema,
            format,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            // For diff, schema is optional so we don't apply config default
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            let format = format.unwrap_or_else(|| config.format_or("text"));
            commands::diff::run(
                &env_a,
                &env_b,
                schema.as_deref(),
                &format,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Fix {
            env,
            schema,
            remove_unknown,
            dry_run,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::fix::run(
                &env,
                &schema,
                remove_unknown,
                dry_run,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Scan {
            path,
            schema,
            show_unused,
            show_paths,
            format,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            let format = format.unwrap_or_else(|| config.format_or("text"));
            commands::scan::run(
                &path,
                &schema,
                show_unused,
                show_paths,
                &format,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Cache { action } => match action {
            CacheAction::List => commands::cache::run_list(),
            CacheAction::Clear { url } => commands::cache::run_clear(url.as_deref()),
            CacheAction::Path => commands::cache::run_path(),
            CacheAction::Stats => commands::cache::run_stats(),
        },
        Command::Export {
            env,
            format,
            schema,
            output,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            let format = format.unwrap_or_else(|| config.format_or("shell"));
            commands::export::run(
                &env,
                schema.as_deref(),
                &format,
                output.as_deref(),
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Doctor {
            env,
            schema,
            no_cache,
            verify_hash,
            ca_cert,
        } => {
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::doctor::run(
                &env,
                &schema,
                no_cache,
                verify_hash.as_deref(),
                ca_cert.as_deref(),
                rate_limit,
            )
        }
        Command::Template {
            name,
            output,
            list,
            use_binary,
        } => commands::template::run(&name, output.as_deref(), list, use_binary),
    };

    if let Err(e) = result {
        eprintln!("zenv error: {e}");
        std::process::exit(e.exit_code());
    }
}

#[cfg(test)]
mod tests {
    use super::is_truthy_boolish;

    #[test]
    fn boolish_truthy_forms() {
        for v in [
            "1",
            "yes",
            "y",
            "on",
            "true",
            "enabled",
            "anything-non-empty",
            "TRUE",
            "Yes",
        ] {
            assert!(is_truthy_boolish(v), "{} should be truthy", v);
        }
    }

    #[test]
    fn boolish_falsy_forms() {
        // no-color.org compliance: empty string MUST be falsy. The other
        // falsy strings match the conventions users expect from boolean
        // env vars in modern CLIs.
        for v in [
            "", "0", "false", "no", "n", "off", "FALSE", "No", " 0 ", "  ",
        ] {
            assert!(
                !is_truthy_boolish(v),
                "{:?} should be falsy (no-color.org compliance)",
                v
            );
        }
    }
}
