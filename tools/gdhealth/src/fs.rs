use crate::error::{GdHealthError, Result};
use std::fs;
use std::path::Path;

pub fn emit_output(output: Option<&Path>, content: &str) -> Result<()> {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| GdHealthError::WriteFile {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }

            fs::write(path, content).map_err(|source| GdHealthError::WriteFile {
                path: path.to_path_buf(),
                source,
            })
        }
        None => {
            println!("{content}");
            Ok(())
        }
    }
}
