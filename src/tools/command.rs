use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use super::{extract_field, SafetyTier, Tool};

pub struct RunCommand;

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &str { "run_command" }
    fn description(&self) -> &str { 
        "Executes a terminal command via sh -c. IMPORTANT: If the command is known to be interactive \
         (e.g. npx create-next-app, npm init, git commit without -m, ssh, etc), you MUST set 'interactive: true'." 
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "command": { "type": "STRING", "description": "The command to run" },
                "interactive": { "type": "BOOLEAN", "description": "Set to true if command requires user input (e.g. git commit without -m, ssh, etc)" }
            },
            "required": ["command"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    fn requires_input(&self, args: &Value) -> bool {
        args.get("interactive").and_then(|v| v.as_bool()).unwrap_or(false)
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(cmd) = extract_field(&args, "command") else {
            return "Error: 'command' is required".into();
        };

        let output_task = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match output_task {
            Ok(c) => c,
            Err(e) => return format!("Failed to spawn command: {}", e),
        };

        let timeout = tokio::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => {
                let stdout = if let Some(mut out) = child.stdout.take() {
                    let mut buf = Vec::new();
                    use tokio::io::AsyncReadExt;
                    let _ = out.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                } else { String::new() };
                
                let stderr = if let Some(mut err) = child.stderr.take() {
                    let mut buf = Vec::new();
                    use tokio::io::AsyncReadExt;
                    let _ = err.read_to_end(&mut buf).await;
                    String::from_utf8_lossy(&buf).to_string()
                } else { String::new() };

                let status_str = if status.success() { "OK" } else { "FAILED" };
                format!("[{status_str}]\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}")
            }
            Ok(Err(e)) => format!("Command execution failed: {}", e),
            Err(_) => {
                let _ = child.kill().await;
                "Error: Command timed out after 30s. If this command is interactive (requires input/selection), please run it with 'interactive: true'.".to_string()
            }
        }
    }
}
