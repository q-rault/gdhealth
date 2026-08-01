use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "gdhealth")]
#[command(version = crate::model::TOOL_VERSION)]
#[command(about = "Code-health analysis for Godot/GDScript repositories")]
#[command(long_about = "gdhealth extracts dependency graphs, computes complexity metrics, evaluates \
    architecture policies, and bundles a health summary for CI and code review.\n\n\
    Run `gdhealth health --root .` to generate the full report bundle at reports/code-health/.")]
pub struct Cli {
    #[arg(
        long,
        help = "Path to gdhealth.toml (default: gdhealth.toml in the current directory)"
    )]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Extract and render the file dependency graph.
    ///
    /// Supports --format: json | text | dot | emerge-json | emerge-html
    /// Default output: stdout. Use --output <file> to write to a file.
    /// Exit 0 always (use `policy --fail-on` or `health --fail-on` for CI enforcement).
    Graph(CommandOptions),

    /// Compute file and function-level complexity metrics.
    ///
    /// Supports --format: json | text
    /// Default output: stdout. Use --output <file> to write to a file.
    /// Exit 0 always (use `policy --fail-on` or `health --fail-on` for CI enforcement).
    Metrics(CommandOptions),

    /// Evaluate architecture and complexity policies against the codebase.
    ///
    /// Supports --format: json | text
    /// Default output: stdout. Use --output <file> to write to a file.
    /// Use --fail-on critical|warning|info to exit 1 when findings reach the threshold.
    Policy(CommandOptions),

    /// Bundle a full health report into reports/code-health/ (recommended for most users and CI).
    ///
    /// Writes: graph.json, graph.dot, metrics.json, policy.json, health.json, summary.txt
    /// When --output is specified, writes a single file in the requested format instead.
    /// Supports --format: json | text (only meaningful when --output is set)
    /// Use --fail-on critical|warning|info to exit 1 when findings reach the threshold.
    Health(CommandOptions),

    /// Score a git commit or base..head range for defect risk from diff shape alone.
    ///
    /// Supports --format: json | text. Default output: stdout.
    /// Uses repowise's published calibration coefficients as a placeholder —
    /// NOT calibrated for GDScript. Treat the score as directional, not absolute.
    Risk(RiskOptions),
}

#[derive(Debug, Args)]
pub struct RiskOptions {
    #[command(flatten)]
    pub common: CommandOptions,

    #[arg(
        default_value = "HEAD",
        help = "Commit or base..head range to score, e.g. HEAD or HEAD~5..HEAD"
    )]
    pub revspec: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
    Dot,
    EmergeJson,
    EmergeHtml,
}

#[derive(Debug, Args)]
pub struct CommandOptions {
    #[arg(
        long,
        default_value = ".",
        help = "Root directory of the GDScript project to analyze"
    )]
    pub root: PathBuf,

    #[arg(
        long,
        value_enum,
        default_value = "text",
        help = "Output format. Not all formats are supported by all commands (see subcommand help)"
    )]
    pub format: OutputFormat,

    #[arg(
        long,
        help = "Write output to a file instead of stdout. For `health` without this flag, writes the full bundle to reports/code-health/"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = false,
        help = "Include files matched by the `test` group in analysis"
    )]
    pub include_tests: bool,

    #[arg(
        long,
        help = "Exit 1 if any finding is at or above this severity. Values: info, warning, critical"
    )]
    pub fail_on: Option<String>,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip git-derived signals (churn, ownership, co-change) even if [git] enabled = true in config"
    )]
    pub no_git: bool,
}
