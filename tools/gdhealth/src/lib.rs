pub mod cli;
pub mod config;
pub mod discovery;
pub mod error;
pub mod fs;
pub mod graph;
pub mod health;
pub mod metrics;
pub mod model;
pub mod output;
pub mod parser;
pub mod policy;

use crate::cli::{Cli, Command, OutputFormat};
use crate::config::Config;
use crate::error::Result;
use clap::Parser;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;

    match cli.command {
        Command::Graph(options) => {
            let report = graph::build_graph(&config, &options)?;
            let rendered = match options.format {
                OutputFormat::Json => output::json::render(&report)?,
                OutputFormat::Dot => output::dot::render_graph(&report),
                OutputFormat::Text => output::text::render_graph(&report),
            };

            fs::emit_output(options.output.as_deref(), &rendered)
        }
        Command::Metrics(options) => {
            let report = metrics::compute_metrics(&config, &options)?;
            let rendered = match options.format {
                OutputFormat::Json => output::json::render(&report)?,
                OutputFormat::Dot => {
                    return Err(crate::error::GdHealthError::UnsupportedFormat(
                        "metrics does not support dot output".to_string(),
                    ));
                }
                OutputFormat::Text => output::text::render_metrics(&report),
            };

            fs::emit_output(options.output.as_deref(), &rendered)
        }
        Command::Policy(options) => {
            let report = policy::evaluate_policy(&config, &options)?;
            let rendered = match options.format {
                OutputFormat::Json => output::json::render(&report)?,
                OutputFormat::Dot => {
                    return Err(crate::error::GdHealthError::UnsupportedFormat(
                        "policy does not support dot output".to_string(),
                    ));
                }
                OutputFormat::Text => output::text::render_policy(&report),
            };

            fs::emit_output(options.output.as_deref(), &rendered)?;
            policy::enforce_fail_on(&report.findings, options.fail_on.as_deref())
        }
        Command::Health(options) => {
            let artifacts = health::build_health_artifacts(&config, &options)?;

            if let Some(output_path) = options.output.as_deref() {
                let rendered = match options.format {
                    OutputFormat::Json => output::json::render(&artifacts.health)?,
                    OutputFormat::Dot => {
                        return Err(crate::error::GdHealthError::UnsupportedFormat(
                            "health does not support dot output".to_string(),
                        ));
                    }
                    OutputFormat::Text => output::text::render_health(&artifacts.health),
                };

                fs::emit_output(Some(output_path), &rendered)?;
            } else {
                let bundle_dir = health::default_bundle_dir(&options.root);
                health::emit_report_bundle(&bundle_dir, &artifacts)?;
            }

            policy::enforce_fail_on(
                &artifacts.health.policy_findings,
                options.fail_on.as_deref(),
            )
        }
    }
}
