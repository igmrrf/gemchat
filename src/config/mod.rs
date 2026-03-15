pub mod types;

pub use types::*;

use color_eyre::Result;
use std::path::PathBuf;

/// Returns the config directory: `~/.config/gemchat/`
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gemchat")
}

/// Returns the data directory: `~/.local/share/gemchat/`
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gemchat")
}

/// Load config from `~/.config/gemchat/config.toml`.
/// Falls back to defaults if the file doesn't exist.
pub fn load_config() -> Result<AppConfig> {
    let path = config_dir().join("config.toml");

    if path.exists() {
        let contents = std::fs::read_to_string(&path)?;
        let config: AppConfig = toml::from_str(&contents)?;
        validate_config(&config)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

/// Write default config to disk for the user to customize.
pub fn write_default_config() -> Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let default = AppConfig::default();
    let contents = toml::to_string_pretty(&default)?;
    std::fs::write(&path, contents)?;
    Ok(path)
}

fn validate_config(config: &AppConfig) -> Result<()> {
    if config.general.max_pipelines < 1 || config.general.max_pipelines > 10 {
        return Err(color_eyre::eyre::eyre!(
            "max_pipelines must be between 1 and 10, got {}",
            config.general.max_pipelines
        ));
    }
    Ok(())
}

/// Resolve the model for a given agent role from config.
pub fn model_for_role(config: &AppConfig, role: &str) -> String {
    match role {
        "orchestrator" => config.models.orchestrator.clone(),
        "planner" => config.models.planner.clone(),
        "researcher" => config.models.researcher.clone(),
        "architect" => config.models.architect.clone(),
        "coder" => config.models.coder.clone(),
        "reviewer" => config.models.reviewer.clone(),
        "qa" => config.models.qa.clone(),
        "executor" => config.models.executor.clone(),
        _ => config.models.coder.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.general.max_pipelines, 4);
        assert_eq!(config.general.persistence_ttl_hours, 24);
        assert_eq!(config.approval.default_tier, ApprovalTier::Tiered);
        assert!(config.approval.safe_tools.contains(&"read_file".to_string()));
        assert!(config.approval.dangerous_tools.contains(&"run_command".to_string()));
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = r#"
[general]
max_pipelines = 2
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.max_pipelines, 2);
        // Other fields should be defaults
        assert_eq!(config.models.coder, "gemini-2.5-flash");
    }

    #[test]
    fn test_validation_rejects_bad_pipelines() {
        let mut config = AppConfig::default();
        config.general.max_pipelines = 99;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_model_for_role() {
        let config = AppConfig::default();
        assert_eq!(model_for_role(&config, "researcher"), "gemini-2.5-flash");
        assert_eq!(model_for_role(&config, "executor"), "gemini-2.0-flash-lite");
    }
}
