use std::path::Path;
use std::process::Command;

/// Result of a merge attempt.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub success: bool,
    pub branch: String,
    pub target: String,
    pub message: String,
    pub conflicting_files: Vec<String>,
}

/// Merge strategies for combining worktree branches back to the main branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Standard merge with a merge commit
    NoFastForward,
    /// Squash all changes into a single commit
    Squash,
    /// Rebase onto target then fast-forward
    Rebase,
}

impl MergeStrategy {
    pub fn as_str(&self) -> &str {
        match self {
            Self::NoFastForward => "no-ff",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

/// Attempt to merge a pipeline branch into the target branch.
pub fn merge_branch(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
    strategy: MergeStrategy,
) -> color_eyre::Result<MergeResult> {
    // First checkout the target branch
    run_git(repo_root, &["checkout", target_branch])?;

    let result = match strategy {
        MergeStrategy::NoFastForward => {
            let msg = format!("Merge gemchat pipeline branch '{}'", source_branch);
            run_git(repo_root, &["merge", "--no-ff", "-m", &msg, source_branch])
        }
        MergeStrategy::Squash => {
            let squash_result = run_git(repo_root, &["merge", "--squash", source_branch]);
            if squash_result.is_ok() {
                let msg = format!("Squashed merge from gemchat pipeline '{}'", source_branch);
                run_git(repo_root, &["commit", "-m", &msg])
            } else {
                squash_result
            }
        }
        MergeStrategy::Rebase => {
            // Checkout source, rebase onto target, then ff-merge
            run_git(repo_root, &["checkout", source_branch])?;
            let rebase_result = run_git(repo_root, &["rebase", target_branch]);
            if rebase_result.is_ok() {
                run_git(repo_root, &["checkout", target_branch])?;
                run_git(repo_root, &["merge", "--ff-only", source_branch])
            } else {
                // Abort rebase on failure
                let _ = run_git(repo_root, &["rebase", "--abort"]);
                rebase_result
            }
        }
    };

    match result {
        Ok(output) => Ok(MergeResult {
            success: true,
            branch: source_branch.to_string(),
            target: target_branch.to_string(),
            message: output,
            conflicting_files: vec![],
        }),
        Err(e) => {
            // Try to get list of conflicting files
            let conflicts = get_conflicting_files(repo_root);

            // Abort the failed merge
            let _ = run_git(repo_root, &["merge", "--abort"]);

            Ok(MergeResult {
                success: false,
                branch: source_branch.to_string(),
                target: target_branch.to_string(),
                message: e.to_string(),
                conflicting_files: conflicts,
            })
        }
    }
}

/// Get the diff stat between two branches.
pub fn diff_stat(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> color_eyre::Result<String> {
    run_git(
        repo_root,
        &["diff", "--stat", &format!("{}...{}", target_branch, source_branch)],
    )
}

/// Check if a branch can be fast-forward merged.
pub fn can_fast_forward(
    repo_root: &Path,
    source_branch: &str,
    target_branch: &str,
) -> bool {
    run_git(
        repo_root,
        &["merge-base", "--is-ancestor", target_branch, source_branch],
    )
    .is_ok()
}

// ── Internal helpers ──

fn run_git(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(color_eyre::eyre::eyre!("git {} failed: {}", args.join(" "), stderr))
    }
}

fn get_conflicting_files(repo_root: &Path) -> Vec<String> {
    run_git(repo_root, &["diff", "--name-only", "--diff-filter=U"])
        .map(|output| {
            output
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_strategy_as_str() {
        assert_eq!(MergeStrategy::NoFastForward.as_str(), "no-ff");
        assert_eq!(MergeStrategy::Squash.as_str(), "squash");
        assert_eq!(MergeStrategy::Rebase.as_str(), "rebase");
    }
}
