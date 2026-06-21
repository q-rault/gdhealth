use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "gdhealth")]
#[command(about = "Code-health analysis for Godot/GDScript repositories")]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Graph(CommandOptions),
    Metrics(CommandOptions),
    Policy(CommandOptions),
    Health(CommandOptions),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
    Dot,
}

#[derive(Debug, Args)]
pub struct CommandOptions {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,

    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    #[arg(long)]
    pub output: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub include_tests: bool,

    #[arg(long)]
    pub fail_on: Option<String>,
}
