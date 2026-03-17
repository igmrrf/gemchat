use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::fs;

use super::{extract_field, resolve_safe_path, SafetyTier, Tool};

// ── ReadFile ──

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Reads the contents of a file within the workspace" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "path": { "type": "STRING", "description": "Relative file path" }
            },
            "required": ["path"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(path_str) = extract_field(&args, "path") else {
            return "Error: 'path' is required".into();
        };
        let path = match resolve_safe_path(working_dir, &path_str) {
            Ok(p) => p,
            Err(e) => return e,
        };
        match fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => format!("Error reading file: {}", e),
        }
    }
}

// ── CreateFile ──

pub struct CreateFile;

#[async_trait]
impl Tool for CreateFile {
    fn name(&self) -> &str { "create_file" }
    fn description(&self) -> &str { "Creates a new file with the given content within the workspace" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "path": { "type": "STRING", "description": "Relative file path" },
                "content": { "type": "STRING", "description": "File content" }
            },
            "required": ["path", "content"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(path_str) = extract_field(&args, "path") else {
            return "Error: 'path' is required".into();
        };
        let content = extract_field(&args, "content").unwrap_or_default();
        let path = match resolve_safe_path(working_dir, &path_str) {
            Ok(p) => p,
            Err(e) => return e,
        };

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }

        match fs::write(&path, content).await {
            Ok(_) => format!("Successfully created {}", path_str),
            Err(e) => format!("Error writing file: {}", e),
        }
    }
}

// ── UpdateFile ──

pub struct UpdateFile;

#[async_trait]
impl Tool for UpdateFile {
    fn name(&self) -> &str { "update_file" }
    fn description(&self) -> &str { "Updates an existing file by appending content within the workspace" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "path": { "type": "STRING", "description": "Relative file path" },
                "content": { "type": "STRING", "description": "Content to append" }
            },
            "required": ["path", "content"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(path_str) = extract_field(&args, "path") else {
            return "Error: 'path' is required".into();
        };
        let content = extract_field(&args, "content").unwrap_or_default();
        let path = match resolve_safe_path(working_dir, &path_str) {
            Ok(p) => p,
            Err(e) => return e,
        };

        use tokio::io::AsyncWriteExt;
        match fs::OpenOptions::new().append(true).open(&path).await {
            Ok(mut file) => match file.write_all(content.as_bytes()).await {
                Ok(_) => format!("Successfully updated {}", path_str),
                Err(e) => format!("Error writing: {}", e),
            },
            Err(e) => format!("Error opening file: {}", e),
        }
    }
}

// ── DeleteFile ──

pub struct DeleteFile;

#[async_trait]
impl Tool for DeleteFile {
    fn name(&self) -> &str { "delete_file" }
    fn description(&self) -> &str { "Deletes a file within the workspace" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "path": { "type": "STRING", "description": "Relative file path" }
            },
            "required": ["path"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(path_str) = extract_field(&args, "path") else {
            return "Error: 'path' is required".into();
        };
        let path = match resolve_safe_path(working_dir, &path_str) {
            Ok(p) => p,
            Err(e) => return e,
        };
        match fs::remove_file(&path).await {
            Ok(_) => format!("Successfully deleted {}", path_str),
            Err(e) => format!("Error deleting: {}", e),
        }
    }
}

// ── ListDirectory ──

pub struct ListDirectory;

#[async_trait]
impl Tool for ListDirectory {
    fn name(&self) -> &str { "list_directory" }
    fn description(&self) -> &str { "Lists files and subdirectories in a directory within the workspace" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "path": { "type": "STRING", "description": "Relative directory path (default: '.')" }
            },
            "required": []
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let path_str = extract_field(&args, "path").unwrap_or_else(|| ".".into());
        let path = match resolve_safe_path(working_dir, &path_str) {
            Ok(p) => p,
            Err(e) => return e,
        };

        match fs::read_dir(&path).await {
            Ok(mut entries) => {
                let mut items = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let file_type = if entry.path().is_dir() { "dir" } else { "file" };
                    items.push(format!("  [{file_type}] {name}"));
                }
                items.sort();
                if items.is_empty() {
                    "Directory is empty".into()
                } else {
                    items.join("\n")
                }
            }
            Err(e) => format!("Error listing directory: {}", e),
        }
    }
}
