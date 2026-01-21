use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use zorath_env::config::Config;
use zorath_env::commands;
use zorath_env::presets;

#[derive(Parser, Debug)]
#[command(name="zenv", version, about="Validate .env files with a schema and generate docs.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Validate .env against schema
    #[command(after_help = "\
Examples:
  zenv check                            Validate using defaults
  zenv check --schema custom.json       Use custom schema
  zenv check --detect-secrets true      Include secret detection
  zenv check --watch                    Watch for file changes
  zenv check --env .env.local           Validate specific env file
  zenv check --format json              JSON output for CI/CD

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
        #[arg(long)]
        detect_secrets: Option<bool>,
        /// Skip cache when fetching remote schemas
        #[arg(long)]
        no_cache: Option<bool>,
        /// Watch for file changes and re-run validation
        #[arg(long, default_value_t = false)]
        watch: bool,
        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
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
        /// Output format: markdown or json
        #[arg(long, default_value = "markdown")]
        format: String,
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
        #[arg(long)]
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
  zenv diff .env.a .env.b --format json     JSON output for CI")]
    Diff {
        /// First .env file
        env_a: String,
        /// Second .env file
        env_b: String,
        /// Optional schema to check compliance
        #[arg(long)]
        schema: Option<String>,
        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
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
        /// Output format: text or json
        #[arg(long, default_value = "text")]
        format: String,
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
  zenv export .env --format shell       Shell script (export FOO=\"bar\")
  zenv export .env --format docker      Dockerfile (ENV FOO=bar)
  zenv export .env --format k8s         Kubernetes ConfigMap YAML
  zenv export .env --format json        JSON object
  zenv export .env --format systemd     systemd Environment directives
  zenv export .env --format dotenv      Standard .env format

  zenv export .env --schema s.json      Only export vars defined in schema
  zenv export .env -f shell -o setup.sh Write to file

Aliases:
  shell: bash, sh
  docker: dockerfile
  k8s: kubernetes, configmap
  systemd: service
  dotenv: env")]
    Export {
        /// Path to .env file to export
        env: String,
        /// Output format (shell, docker, k8s, json, systemd, dotenv)
        #[arg(short = 'f', long, default_value = "shell")]
        format: String,
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
    Doctor,

    /// Generate CI/CD configuration templates
    #[command(after_help = "\
Examples:
  zenv template github              Output GitHub Actions workflow
  zenv template gitlab -o .gitlab-ci.yml  Write GitLab CI config to file
  zenv template circleci            Output CircleCI config
  zenv template --list              List available templates

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
}

fn main() {
    let cli = Cli::parse();

    // Load config from .zenvrc (if present)
    let config = Config::load().unwrap_or_default();

    let result = match cli.command {
        Command::Check { env, schema, allow_missing_env, detect_secrets, no_cache, watch, format, verify_hash, ca_cert } => {
            // CLI args override config, config overrides defaults
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let allow_missing_env = allow_missing_env.unwrap_or_else(|| config.allow_missing_env_or(true));
            let detect_secrets = detect_secrets.unwrap_or_else(|| config.detect_secrets_or(false));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::check::run(&env, &schema, allow_missing_env, detect_secrets, no_cache, watch, &format, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Docs { schema, format, no_cache, verify_hash, ca_cert } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::docs::run(&schema, &format, no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Init { example, schema, preset, list_presets } => {
            if list_presets {
                println!("Available presets:");
                for name in presets::list_presets() {
                    println!("  {}", name);
                }
                Ok(())
            } else {
                let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
                commands::init::run(&example, &schema, preset.as_deref())
            }
        }
        Command::Version { check_update } => commands::version::run(check_update),
        Command::Completions { shell } => commands::completions::run(shell, &mut Cli::command()),
        Command::Example { schema, output, include_defaults, no_cache, verify_hash, ca_cert } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::example::run(&schema, output.as_deref(), include_defaults, no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Diff { env_a, env_b, schema, format, no_cache, verify_hash, ca_cert } => {
            // For diff, schema is optional so we don't apply config default
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::diff::run(&env_a, &env_b, schema.as_deref(), &format, no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Fix { env, schema, remove_unknown, dry_run, no_cache, verify_hash, ca_cert } => {
            let env = env.unwrap_or_else(|| config.env_or(".env"));
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::fix::run(&env, &schema, remove_unknown, dry_run, no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Scan { path, schema, show_unused, show_paths, format, no_cache, verify_hash, ca_cert } => {
            let schema = schema.unwrap_or_else(|| config.schema_or("env.schema.json"));
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::scan::run(&path, &schema, show_unused, show_paths, &format, no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Cache { action } => match action {
            CacheAction::List => commands::cache::run_list(),
            CacheAction::Clear { url } => commands::cache::run_clear(url.as_deref()),
            CacheAction::Path => commands::cache::run_path(),
        },
        Command::Export { env, format, schema, output, no_cache, verify_hash, ca_cert } => {
            let no_cache = no_cache.unwrap_or_else(|| config.no_cache_or(false));
            let verify_hash = verify_hash.or_else(|| config.verify_hash());
            let ca_cert = ca_cert.or_else(|| config.ca_cert());
            commands::export::run(&env, schema.as_deref(), &format, output.as_deref(), no_cache, verify_hash.as_deref(), ca_cert.as_deref())
        }
        Command::Doctor => commands::doctor::run(),
        Command::Template { name, output, list } => {
            commands::template::run(&name, output.as_deref(), list)
        }
    };

    if let Err(e) = result {
        eprintln!("zenv error: {e}");
        std::process::exit(1);
    }
}
