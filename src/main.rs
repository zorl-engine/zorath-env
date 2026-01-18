mod envfile;
mod remote;
mod schema;
mod secrets;
mod commands;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser, Debug)]
#[command(name="zenv", version, about="Validate .env files with a schema and generate docs.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Validate .env against schema
    Check {
        #[arg(long, default_value = ".env")]
        env: String,
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
        /// If set, missing .env is allowed (schema still validated against defaults/required rules)
        #[arg(long, default_value_t = true)]
        allow_missing_env: bool,
        /// Detect potential secrets in .env file (API keys, passwords, tokens)
        #[arg(long, default_value_t = false)]
        detect_secrets: bool,
        /// Skip cache when fetching remote schemas
        #[arg(long, default_value_t = false)]
        no_cache: bool,
        /// Watch for file changes and re-run validation
        #[arg(long, default_value_t = false)]
        watch: bool,
    },

    /// Print docs for schema (markdown or json)
    Docs {
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
        /// Output format: markdown or json
        #[arg(long, default_value = "markdown")]
        format: String,
        /// Skip cache when fetching remote schemas
        #[arg(long, default_value_t = false)]
        no_cache: bool,
    },

    /// Create a starter schema from .env.example
    Init {
        #[arg(long, default_value = ".env.example")]
        example: String,
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
    },

    /// Show version and optionally check for updates
    Version {
        /// Check crates.io for newer version
        #[arg(long, default_value_t = false)]
        check_update: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell)
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Generate .env.example from schema
    Example {
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
        /// Output file path (defaults to stdout)
        #[arg(long)]
        output: Option<String>,
        /// Include default values in output
        #[arg(long, default_value_t = false)]
        include_defaults: bool,
        /// Skip cache when fetching remote schemas
        #[arg(long, default_value_t = false)]
        no_cache: bool,
    },

    /// Compare two .env files
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
        #[arg(long, default_value_t = false)]
        no_cache: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Check { env, schema, allow_missing_env, detect_secrets, no_cache, watch } => {
            commands::check::run(&env, &schema, allow_missing_env, detect_secrets, no_cache, watch)
        }
        Command::Docs { schema, format, no_cache } => commands::docs::run(&schema, &format, no_cache),
        Command::Init { example, schema } => commands::init::run(&example, &schema),
        Command::Version { check_update } => commands::version::run(check_update),
        Command::Completions { shell } => commands::completions::run(shell),
        Command::Example { schema, output, include_defaults, no_cache } => {
            commands::example::run(&schema, output.as_deref(), include_defaults, no_cache)
        }
        Command::Diff { env_a, env_b, schema, format, no_cache } => {
            commands::diff::run(&env_a, &env_b, schema.as_deref(), &format, no_cache)
        }
    };

    if let Err(e) = result {
        eprintln!("zenv error: {e}");
        std::process::exit(1);
    }
}
