use crate::cli::CommandOptions;
use crate::config::Config;
use crate::error::Result;
use crate::model::{OutputMetadata, PolicyFinding, PolicyReport};

pub fn evaluate_policy(_config: &Config, options: &CommandOptions) -> Result<PolicyReport> {
    Ok(PolicyReport {
        metadata: OutputMetadata::default(),
        root: options.root.to_string_lossy().into_owned(),
        findings: Vec::<PolicyFinding>::new(),
    })
}
