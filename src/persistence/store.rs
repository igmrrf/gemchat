use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};



/// Serializable pipeline state for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRecord {
    pub id: String,
    pub task: String,
    pub status: PipelineRecordStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub working_dir: String,
    pub branch: Option<String>,
    pub steps_completed: Vec<String>,
    pub current_step: Option<String>,
    pub context_snapshot: HashMap<String, String>,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PipelineRecordStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// File-based session store for pipeline persistence.
///
/// Stores pipeline state as JSON files under `~/.local/share/gemchat/sessions/`.
/// Automatically cleans up sessions older than the configured TTL.
pub struct SessionStore {
    base_dir: PathBuf,
    ttl: Duration,
}

impl SessionStore {
    /// Create a new store with TTL from config.
    pub fn new(ttl_hours: u32) -> color_eyre::Result<Self> {
        let base_dir = Self::default_directory()?;
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self {
            base_dir,
            ttl: Duration::from_secs(u64::from(ttl_hours) * 3600),
        })
    }

    /// Create a store at a specific directory (for testing).
    pub fn with_dir(dir: PathBuf, ttl_hours: u32) -> color_eyre::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            base_dir: dir,
            ttl: Duration::from_secs(u64::from(ttl_hours) * 3600),
        })
    }

    /// Default storage directory.
    fn default_directory() -> color_eyre::Result<PathBuf> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| color_eyre::eyre::eyre!("Cannot determine local data directory"))?;
        Ok(data_dir.join("gemchat").join("sessions"))
    }

    /// Save a pipeline record.
    pub fn save_pipeline(&self, record: &PipelineRecord) -> color_eyre::Result<()> {
        let filename = format!("{}.json", record.id);
        let path = self.base_dir.join(filename);
        let json = serde_json::to_string_pretty(record)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a pipeline record by ID.
    pub fn load_pipeline(&self, pipeline_id: &str) -> color_eyre::Result<PipelineRecord> {
        let filename = format!("{}.json", pipeline_id);
        let path = self.base_dir.join(filename);
        let json = std::fs::read_to_string(&path)?;
        let record: PipelineRecord = serde_json::from_str(&json)?;
        Ok(record)
    }

    /// Load all active (non-expired) pipelines.
    pub fn load_active_pipelines(&self) -> color_eyre::Result<Vec<PipelineRecord>> {
        let mut records = Vec::new();
        let now = Utc::now();

        if !self.base_dir.exists() {
            return Ok(records);
        }

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(record) = serde_json::from_str::<PipelineRecord>(&json) {
                        let age = now.signed_duration_since(record.updated_at);
                        if age.num_seconds() < self.ttl.as_secs() as i64 {
                            records.push(record);
                        }
                    }
                }
            }
        }

        // Sort by most recently updated
        records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(records)
    }

    /// Remove expired sessions. Returns count of removed files.
    pub fn cleanup_expired(&self) -> color_eyre::Result<usize> {
        let mut removed = 0;
        let now = Utc::now();

        if !self.base_dir.exists() {
            return Ok(0);
        }

        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                let should_remove = match std::fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<PipelineRecord>(&json) {
                        Ok(record) => {
                            let age = now.signed_duration_since(record.updated_at);
                            age.num_seconds() >= self.ttl.as_secs() as i64
                        }
                        Err(_) => true, // Remove malformed files
                    },
                    Err(_) => true,
                };
                if should_remove {
                    std::fs::remove_file(&path)?;
                    removed += 1;
                }
            }
        }

        Ok(removed)
    }

    /// Delete a specific pipeline record.
    pub fn delete_pipeline(&self, pipeline_id: &str) -> color_eyre::Result<()> {
        let path = self.base_dir.join(format!("{}.json", pipeline_id));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Update the status of a pipeline.
    pub fn update_status(
        &self,
        pipeline_id: &str,
        status: PipelineRecordStatus,
    ) -> color_eyre::Result<()> {
        let mut record = self.load_pipeline(pipeline_id)?;
        record.status = status;
        record.updated_at = Utc::now();
        self.save_pipeline(&record)
    }

    /// Get the storage directory path.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}

/// Helper to create a new PipelineRecord from task info.
impl PipelineRecord {
    pub fn new(id: String, task: String, working_dir: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            task,
            status: PipelineRecordStatus::Running,
            created_at: now,
            updated_at: now,
            working_dir,
            branch: None,
            steps_completed: Vec::new(),
            current_step: None,
            context_snapshot: HashMap::new(),
            total_tokens: 0,
        }
    }

    /// Record that a step was completed.
    pub fn complete_step(&mut self, step_name: &str, tokens: i64) {
        self.steps_completed.push(step_name.to_string());
        self.current_step = None;
        self.total_tokens += tokens;
        self.updated_at = Utc::now();
    }

    /// Mark the current active step.
    pub fn start_step(&mut self, step_name: &str) {
        self.current_step = Some(step_name.to_string());
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    fn temp_store() -> (SessionStore, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::with_dir(tmp.path().to_path_buf(), 24).unwrap();
        (store, tmp)
    }

    #[test]
    fn test_save_and_load() {
        let (store, _tmp) = temp_store();
        let record = PipelineRecord::new(
            "test-001".into(),
            "Add login page".into(),
            "/tmp/project".into(),
        );
        store.save_pipeline(&record).unwrap();

        let loaded = store.load_pipeline("test-001").unwrap();
        assert_eq!(loaded.id, "test-001");
        assert_eq!(loaded.task, "Add login page");
        assert_eq!(loaded.status, PipelineRecordStatus::Running);
    }

    #[test]
    fn test_load_active_pipelines() {
        let (store, _tmp) = temp_store();

        for i in 0..3 {
            let record = PipelineRecord::new(
                format!("p-{}", i),
                format!("Task {}", i),
                "/tmp".into(),
            );
            store.save_pipeline(&record).unwrap();
        }

        let active = store.load_active_pipelines().unwrap();
        assert_eq!(active.len(), 3);
    }

    #[test]
    fn test_delete_pipeline() {
        let (store, _tmp) = temp_store();
        let record = PipelineRecord::new("del-me".into(), "temp".into(), "/tmp".into());
        store.save_pipeline(&record).unwrap();

        assert!(store.load_pipeline("del-me").is_ok());
        store.delete_pipeline("del-me").unwrap();
        assert!(store.load_pipeline("del-me").is_err());
    }

    #[test]
    fn test_update_status() {
        let (store, _tmp) = temp_store();
        let record = PipelineRecord::new("status-01".into(), "WIP".into(), "/tmp".into());
        store.save_pipeline(&record).unwrap();

        store.update_status("status-01", PipelineRecordStatus::Completed).unwrap();
        let loaded = store.load_pipeline("status-01").unwrap();
        assert_eq!(loaded.status, PipelineRecordStatus::Completed);
    }

    #[test]
    fn test_pipeline_record_steps() {
        let mut record = PipelineRecord::new("s-01".into(), "task".into(), "/tmp".into());
        record.start_step("planner");
        assert_eq!(record.current_step, Some("planner".into()));

        record.complete_step("planner", 100);
        assert_eq!(record.current_step, None);
        assert_eq!(record.steps_completed, vec!["planner"]);
        assert_eq!(record.total_tokens, 100);
    }
}
