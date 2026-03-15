pub mod merge;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use uuid::Uuid;

/// Unique identifier for a pipeline instance.
pub type PipelineId = String;

/// Information about a single git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub pipeline_id: PipelineId,
    pub branch: String,
    pub path: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Manages git worktrees for parallel pipeline execution.
///
/// Each pipeline gets its own worktree so multiple pipelines can
/// modify files independently without conflicts.
pub struct WorktreeManager {
    /// Root of the main git repository
    repo_root: PathBuf,
    /// Directory where worktrees are created
    worktree_base: PathBuf,
    /// Active worktrees by pipeline ID
    worktrees: HashMap<PipelineId, WorktreeInfo>,
}

impl WorktreeManager {
    /// Create a new WorktreeManager anchored at the given repo root.
    pub fn new(repo_root: PathBuf) -> Self {
        let worktree_base = repo_root.join(".gemchat-worktrees");
        Self {
            repo_root,
            worktree_base,
            worktrees: HashMap::new(),
        }
    }

    /// Check if the repo root is a valid git repository.
    pub fn is_git_repo(&self) -> bool {
        self.repo_root.join(".git").exists()
    }

    /// Initialize a git repo if needed, based on config policy.
    pub fn init_repo_if_needed(&self, mode: &str) -> color_eyre::Result<bool> {
        if self.is_git_repo() {
            return Ok(false); // Already initialized
        }

        match mode {
            "always" => {
                self.run_git(&["init"])?;
                Ok(true)
            }
            "never" => Err(color_eyre::eyre::eyre!(
                "Directory is not a git repo and auto_init_git is 'never'"
            )),
            // "ask" — caller should prompt user first
            _ => Err(color_eyre::eyre::eyre!(
                "Directory is not a git repo. Run `git init` first or set auto_init_git='always'"
            )),
        }
    }

    /// Create a new worktree for a pipeline.
    ///
    /// Creates a new branch and worktree directory under `.gemchat-worktrees/`.
    pub fn create_worktree(
        &mut self,
        pipeline_id: &str,
        task_hint: &str,
    ) -> color_eyre::Result<PathBuf> {
        if !self.is_git_repo() {
            return Err(color_eyre::eyre::eyre!(
                "Cannot create worktree: not a git repository"
            ));
        }

        // Generate branch name from task hint
        let branch = self.generate_branch_name(pipeline_id, task_hint);
        let worktree_path = self.worktree_base.join(format!("pipeline-{}", pipeline_id));

        // Create the worktree base directory
        std::fs::create_dir_all(&self.worktree_base)?;

        // Create worktree with new branch
        self.run_git(&[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree_path.to_str().unwrap_or(""),
        ])?;

        let info = WorktreeInfo {
            pipeline_id: pipeline_id.to_string(),
            branch: branch.clone(),
            path: worktree_path.clone(),
            created_at: chrono::Utc::now(),
        };

        self.worktrees.insert(pipeline_id.to_string(), info);

        Ok(worktree_path)
    }

    /// Remove a worktree and optionally its branch.
    pub fn remove_worktree(
        &mut self,
        pipeline_id: &str,
        delete_branch: bool,
    ) -> color_eyre::Result<()> {
        let info = self
            .worktrees
            .remove(pipeline_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("No worktree for pipeline {}", pipeline_id))?;

        // Remove the worktree
        self.run_git(&["worktree", "remove", "--force", info.path.to_str().unwrap_or("")])?;

        // Optionally delete the branch
        if delete_branch {
            let _ = self.run_git(&["branch", "-D", &info.branch]);
        }

        Ok(())
    }

    /// Get the working directory for a pipeline.
    ///
    /// Returns the worktree path if one exists, otherwise the repo root.
    pub fn working_dir_for(&self, pipeline_id: &str) -> PathBuf {
        self.worktrees
            .get(pipeline_id)
            .map(|info| info.path.clone())
            .unwrap_or_else(|| self.repo_root.clone())
    }

    /// List all active worktrees.
    pub fn list_worktrees(&self) -> Vec<&WorktreeInfo> {
        self.worktrees.values().collect()
    }

    /// Get worktree info for a pipeline.
    pub fn get_worktree(&self, pipeline_id: &str) -> Option<&WorktreeInfo> {
        self.worktrees.get(pipeline_id)
    }

    /// Prune dead worktrees (e.g. after crash).
    pub fn prune(&self) -> color_eyre::Result<()> {
        self.run_git(&["worktree", "prune"])?;
        Ok(())
    }

    /// Generate a unique pipeline ID.
    pub fn new_pipeline_id() -> PipelineId {
        Uuid::new_v4().to_string()[..8].to_string()
    }

    // ── Internal helpers ──

    fn generate_branch_name(&self, pipeline_id: &str, task_hint: &str) -> String {
        let sanitized: String = task_hint
            .chars()
            .take(30)
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string();

        let label = if sanitized.is_empty() {
            "task".to_string()
        } else {
            sanitized
        };

        format!("gemchat/{}/{}", label, &pipeline_id[..8.min(pipeline_id.len())])
    }

    fn run_git(&self, args: &[&str]) -> color_eyre::Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo_root)
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(color_eyre::eyre::eyre!("git {} failed: {}", args.join(" "), stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_pipeline_id() {
        let id = WorktreeManager::new_pipeline_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn test_branch_name_generation() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/test-repo"));
        let branch = mgr.generate_branch_name("abcd1234", "Add user authentication");
        assert!(branch.starts_with("gemchat/"));
        assert!(branch.contains("add-user-authentication"));
        assert!(branch.ends_with("abcd1234"));
    }

    #[test]
    fn test_branch_name_sanitization() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/test-repo"));
        let branch = mgr.generate_branch_name("xyz99999", "Fix $pecial Ch@rs!!!");
        assert!(!branch.contains('$'));
        assert!(!branch.contains('@'));
        assert!(!branch.contains('!'));
    }

    #[test]
    fn test_working_dir_defaults_to_repo_root() {
        let mgr = WorktreeManager::new(PathBuf::from("/tmp/my-repo"));
        let dir = mgr.working_dir_for("nonexistent");
        assert_eq!(dir, PathBuf::from("/tmp/my-repo"));
    }
}
