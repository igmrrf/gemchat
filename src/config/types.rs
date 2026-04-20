use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root configuration structure for gemchat.
/// Loaded from `~/.config/gemchat/config.toml` with sensible defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub models: ModelConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub qa: QaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_max_pipelines")]
    pub max_pipelines: u8,
    #[serde(default = "default_persistence_ttl")]
    pub persistence_ttl_hours: u32,
    #[serde(default = "default_auto_init_git")]
    pub auto_init_git: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    #[serde(default = "default_approval_tier")]
    pub default_tier: ApprovalTier,
    #[serde(default = "default_safe_tools")]
    pub safe_tools: Vec<String>,
    #[serde(default = "default_dangerous_tools")]
    pub dangerous_tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalTier {
    Tiered,
    Autonomous,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_model_pro")]
    pub orchestrator: String,
    #[serde(default = "default_model_pro")]
    pub planner: String,
    #[serde(default = "default_model_flash")]
    pub researcher: String,
    #[serde(default = "default_model_pro")]
    pub architect: String,
    #[serde(default = "default_model_pro")]
    pub coder: String,
    #[serde(default = "default_model_flash")]
    pub reviewer: String,
    #[serde(default = "default_model_flash")]
    pub qa: String,
    #[serde(default = "default_model_flash_lite")]
    pub executor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub gemini: ProviderEntry,
    #[serde(default)]
    pub claude: ProviderEntry,
    #[serde(default)]
    pub openai: ProviderEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct ProviderEntry {
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

// ── Defaults ──

fn default_max_pipelines() -> u8 { 4 }
fn default_persistence_ttl() -> u32 { 24 }
fn default_auto_init_git() -> String { "ask".into() }
fn default_approval_tier() -> ApprovalTier { ApprovalTier::Tiered }
fn default_true() -> bool { true }

fn default_model_pro() -> String { "gemini-2.5-flash".into() }
fn default_model_flash() -> String { "gemini-2.5-flash".into() }
fn default_model_flash_lite() -> String { "gemini-2.0-flash-lite".into() }

fn default_safe_tools() -> Vec<String> {
    vec![
        "search_google".into(),
        "read_file".into(),
        "list_directory".into(),
        "git_diff".into(),
        "git_status".into(),
    ]
}

fn default_dangerous_tools() -> Vec<String> {
    vec![
        "run_command".into(),
        "delete_file".into(),
        "git_commit".into(),
    ]
}

// ── Trait impls ──


impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            max_pipelines: default_max_pipelines(),
            persistence_ttl_hours: default_persistence_ttl(),
            auto_init_git: default_auto_init_git(),
        }
    }
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            default_tier: default_approval_tier(),
            safe_tools: default_safe_tools(),
            dangerous_tools: default_dangerous_tools(),
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            orchestrator: default_model_pro(),
            planner: default_model_pro(),
            researcher: default_model_flash(),
            architect: default_model_pro(),
            coder: default_model_pro(),
            reviewer: default_model_flash(),
            qa: default_model_flash(),
            executor: default_model_flash_lite(),
        }
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            gemini: ProviderEntry {
                api_key_env: "GEMINI_API_KEY".into(),
                extra: HashMap::new(),
            },
            claude: ProviderEntry {
                api_key_env: "ANTHROPIC_API_KEY".into(),
                extra: HashMap::new(),
            },
            openai: ProviderEntry {
                api_key_env: "OPENAI_API_KEY".into(),
                extra: HashMap::new(),
            },
        }
    }
}


impl Default for QaConfig {
    fn default() -> Self {
        Self { enabled: default_true() }
    }
}
