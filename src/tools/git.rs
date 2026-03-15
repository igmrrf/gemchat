use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use super::{extract_field, SafetyTier, Tool};

// ── GitDiff ──

pub struct GitDiff;

#[async_trait]
impl Tool for GitDiff {
    fn name(&self) -> &str { "git_diff" }
    fn description(&self) -> &str { "Shows git diff of current changes" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "staged": { "type": "BOOLEAN", "description": "Show staged changes only (default: false)" }
            },
            "required": []
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let staged = args.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut cmd = Command::new("git");
        cmd.arg("diff").current_dir(working_dir)
            .stdout(Stdio::piped()).stderr(Stdio::piped());
        if staged {
            cmd.arg("--cached");
        }

        match cmd.output().await {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.is_empty() { "No changes".into() } else { stdout.to_string() }
            }
            Err(e) => format!("git diff failed: {}", e),
        }
    }
}

// ── GitStatus ──

pub struct GitStatus;

#[async_trait]
impl Tool for GitStatus {
    fn name(&self) -> &str { "git_status" }
    fn description(&self) -> &str { "Shows git status (short format)" }
    fn parameters(&self) -> Value {
        json!({ "type": "OBJECT", "properties": {}, "required": [] })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, _args: Value, working_dir: &Path) -> String {
        match Command::new("git")
            .args(["status", "--short"])
            .current_dir(working_dir)
            .stdout(Stdio::piped()).stderr(Stdio::piped())
            .output().await
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.is_empty() { "Working tree clean".into() } else { stdout.to_string() }
            }
            Err(e) => format!("git status failed: {}", e),
        }
    }
}

// ── GitCommit ──

pub struct GitCommit;

#[async_trait]
impl Tool for GitCommit {
    fn name(&self) -> &str { "git_commit" }
    fn description(&self) -> &str { "Stages all changes and commits with a message" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "message": { "type": "STRING", "description": "Commit message" }
            },
            "required": ["message"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(message) = extract_field(&args, "message") else {
            return "Error: 'message' is required".into();
        };

        // Stage all
        let _ = Command::new("git")
            .args(["add", "-A"])
            .current_dir(working_dir)
            .output().await;

        // Commit
        match Command::new("git")
            .args(["commit", "-m", &message])
            .current_dir(working_dir)
            .stdout(Stdio::piped()).stderr(Stdio::piped())
            .output().await
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                format!("{stdout}\n{stderr}")
            }
            Err(e) => format!("git commit failed: {}", e),
        }
    }
}

// ── GitBranch ──

pub struct GitBranch;

#[async_trait]
impl Tool for GitBranch {
    fn name(&self) -> &str { "git_branch" }
    fn description(&self) -> &str { "Lists branches or creates a new branch" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "name": { "type": "STRING", "description": "Branch name to create (omit to list)" }
            },
            "required": []
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        if let Some(name) = extract_field(&args, "name") {
            // Create and checkout branch
            match Command::new("git")
                .args(["checkout", "-b", &name])
                .current_dir(working_dir)
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .output().await
            {
                Ok(out) => String::from_utf8_lossy(&out.stderr).to_string(),
                Err(e) => format!("git branch failed: {}", e),
            }
        } else {
            // List branches
            match Command::new("git")
                .args(["branch", "--list"])
                .current_dir(working_dir)
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .output().await
            {
                Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
                Err(e) => format!("git branch failed: {}", e),
            }
        }
    }
}
