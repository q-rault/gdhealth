use std::path::Path;
use std::process::Command;

/// Errors from shelling out to the system `git` binary. Kept local to the
/// `git` module — `git::collect` maps every variant to
/// `GitIntelligenceReport::unavailable(reason)` rather than letting it
/// propagate as a `GdHealthError`, so a missing git binary or a directory
/// that isn't a repository never fails the whole command.
#[derive(Debug)]
pub enum GitError {
    BinaryNotFound,
    NotARepository,
    CommandFailed { args: Vec<String>, stderr: String },
    Utf8 { args: Vec<String> },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryNotFound => write!(f, "git binary not found on PATH"),
            Self::NotARepository => write!(f, "not a git repository"),
            Self::CommandFailed { args, stderr } => {
                write!(f, "git {} failed: {}", args.join(" "), stderr.trim())
            }
            Self::Utf8 { args } => write!(f, "git {} produced non-UTF-8 output", args.join(" ")),
        }
    }
}

pub fn run_git(root: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let output = Command::new("git").arg("-C").arg(root).args(args).output();

    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(GitError::BinaryNotFound);
        }
        Err(error) => {
            return Err(GitError::CommandFailed {
                args: owned_args(args),
                stderr: error.to_string(),
            });
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if stderr.to_ascii_lowercase().contains("not a git repository") {
            return Err(GitError::NotARepository);
        }
        return Err(GitError::CommandFailed {
            args: owned_args(args),
            stderr,
        });
    }

    Ok(output.stdout)
}

pub fn run_git_text(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let bytes = run_git(root, args)?;
    String::from_utf8(bytes).map_err(|_| GitError::Utf8 {
        args: owned_args(args),
    })
}

fn owned_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}
