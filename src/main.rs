mod envfile;
mod schema;
mod commands;

use clap::{Parser, Subcommand};

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
    },

    /// Print Markdown docs for schema
    Docs {
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
    },

    /// Create a starter schema from .env.example
    Init {
        #[arg(long, default_value = ".env.example")]
        example: String,
        #[arg(long, default_value = "env.schema.json")]
        schema: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Check { env, schema, allow_missing_env } => {
            commands::check::run(&env, &schema, allow_missing_env)
        }
        Command::Docs { schema } => commands::docs::run(&schema),
        Command::Init { example, schema } => commands::init::run(&example, &schema),
    };

    if let Err(e) = result {
        eprintln!("zenv error: {e}");
        std::process::exit(1);
    }
}
