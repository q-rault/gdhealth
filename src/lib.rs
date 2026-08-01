pub mod analysis;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod error;
pub mod fs;
pub mod git;
pub mod graph;
pub mod health;
pub mod isolate;
pub mod metrics;
pub mod model;
pub mod output;
pub mod parser;
pub mod policy;

use crate::cli::{Cli, Command, OutputFormat};
use crate::config::Config;
use crate::error::Result;
use crate::model::SCHEMA_VERSION;
use clap::Parser;

fn try_handle_probe(args: &[std::ffi::OsString]) -> bool {
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("--schema-version") | Some("schema-version") => {
            println!("{SCHEMA_VERSION}");
            true
        }
        _ => false,
    }
}

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if try_handle_probe(&args) {
        return Ok(());
    }

    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;

    match cli.command {
        Command::Graph(options) => match options.format {
            OutputFormat::Json => {
                let report = graph::build_graph(&config, &options)?;
                let rendered = output::json::render(&report)?;
                fs::emit_output(options.output.as_deref(), &rendered)
            }
            OutputFormat::Dot => {
                let report = graph::build_graph(&config, &options)?;
                let rendered = output::dot::render_graph(&report);
                fs::emit_output(options.output.as_deref(), &rendered)
            }
            OutputFormat::Text => {
                let report = graph::build_graph(&config, &options)?;
                let rendered = output::text::render_graph(&report);
                fs::emit_output(options.output.as_deref(), &rendered)
            }
            OutputFormat::EmergeJson => {
                let graph = graph::build_graph(&config, &options)?;
                let metrics = metrics::compute_metrics(&config, &options)?;
                let rendered = output::browser::render_emerge_json(&graph, &metrics, &[], 0)?;
                fs::emit_output(options.output.as_deref(), &rendered)
            }
            OutputFormat::EmergeHtml => {
                let graph = graph::build_graph(&config, &options)?;
                let metrics = metrics::compute_metrics(&config, &options)?;
                output::browser::emit_emerge_html(
                    options.root.as_path(),
                    options.output.as_deref(),
                    &graph,
                    &metrics,
                    &[],
                    0,
                )
            }
        },
        Command::Metrics(options) => {
            let report = metrics::compute_metrics(&config, &options)?;
            let rendered = match options.format {
                OutputFormat::Json => output::json::render(&report)?,
                OutputFormat::Dot | OutputFormat::EmergeJson | OutputFormat::EmergeHtml => {
                    return Err(crate::error::GdHealthError::UnsupportedFormat(
                        "metrics does not support dot or browser output".to_string(),
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
                OutputFormat::Dot | OutputFormat::EmergeJson | OutputFormat::EmergeHtml => {
                    return Err(crate::error::GdHealthError::UnsupportedFormat(
                        "policy does not support dot or browser output".to_string(),
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
                    OutputFormat::Dot | OutputFormat::EmergeJson | OutputFormat::EmergeHtml => {
                        return Err(crate::error::GdHealthError::UnsupportedFormat(
                            "health does not support dot or browser output".to_string(),
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
        Command::Risk(options) => {
            let common = &options.common;
            let risk = crate::git::risk::score_revspec(common.root.as_path(), &options.revspec)
                .map_err(|message| crate::error::GdHealthError::Git { message })?;

            let rendered = match common.format {
                OutputFormat::Json => output::json::render(&risk)?,
                OutputFormat::Dot | OutputFormat::EmergeJson | OutputFormat::EmergeHtml => {
                    return Err(crate::error::GdHealthError::UnsupportedFormat(
                        "risk does not support dot or browser output".to_string(),
                    ));
                }
                OutputFormat::Text => output::text::render_risk(&risk),
            };

            fs::emit_output(common.output.as_deref(), &rendered)
        }
    }
}
