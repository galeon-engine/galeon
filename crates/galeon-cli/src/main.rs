// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use clap::{Parser, Subcommand, ValueEnum};

mod new;
mod templates;

#[derive(Parser)]
#[command(name = "galeon", about = "Galeon Engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Scaffold a new Galeon game project
    New {
        /// Project name
        name: String,
        /// Project preset
        #[arg(long, default_value = "server-authoritative")]
        preset: Preset,
    },
}

#[derive(Clone, ValueEnum)]
pub enum Preset {
    ServerAuthoritative,
    LocalFirst,
    Hybrid,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        CliCommand::New { name, preset } => {
            if let Err(e) = new::scaffold(std::path::Path::new("."), &name, &preset) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
            println!("created project `{name}`");
        }
    }
}
