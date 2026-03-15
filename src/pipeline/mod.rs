pub mod context;
pub mod step;

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::agents::AgentRole;
use crate::config::AppConfig;
use crate::provider::{self, AiProvider, AiUpdate, ChatMessage};
use crate::tools::ToolRegistry;

use context::PipelineContext;
use step::{StepResult, StepStatus};

/// The pipeline coordinator routes messages to agents, manages context,
/// and handles tool call loops.
pub struct Pipeline {
    pub config: AppConfig,
    pub context: PipelineContext,
    pub tool_registry: ToolRegistry,
    pub working_dir: PathBuf,
    provider: Box<dyn AiProvider>,
}

impl Pipeline {
    /// Create a new pipeline with the given config and model.
    pub fn new(config: AppConfig, model: &str, working_dir: PathBuf) -> color_eyre::Result<Self> {
        let provider = provider::create_provider(model, &config)?;
        Ok(Self {
            config,
            context: PipelineContext::new(),
            tool_registry: ToolRegistry::new(),
            working_dir,
            provider,
        })
    }

    /// Execute a single agent step: send messages to the AI, handle tool calls
    /// in a loop, and return the final text response.
    pub async fn execute_agent(
        &self,
        role: AgentRole,
        user_message: &str,
    ) -> StepResult {
        let system_prompt = role.system_prompt();
        let allowed_tools = role.allowed_tools();
        let tools = self.tool_registry.tool_definitions_for(allowed_tools);

        // Build messages: system + context + user
        let mut messages = vec![
            ChatMessage {
                role: "user".into(),
                content: format!("[System] {}\n\nUser request: {}", system_prompt, user_message),
            },
        ];

        // Add any relevant context from previous steps
        for ctx in self.context.recent_outputs(3) {
            messages.push(ChatMessage {
                role: "model".into(),
                content: format!("[Context from {}] {}", ctx.0, ctx.1),
            });
        }

        // Agentic tool-call loop (max iterations to prevent infinite loops)
        let max_iterations = 10;
        let mut full_response = String::new();
        let mut tool_calls_made = Vec::new();

        for _iteration in 0..max_iterations {
            let (tx, mut rx) = mpsc::unbounded_channel();
            self.provider.stream_response(&messages, &tools, tx).await;

            let mut chunk_text = String::new();
            let mut pending_tool_call: Option<(String, String)> = None;
            let mut usage = None;

            while let Some(update) = rx.recv().await {
                match update {
                    AiUpdate::Content(text) => {
                        chunk_text.push_str(&text);
                    }
                    AiUpdate::ToolCall { name, args } => {
                        pending_tool_call = Some((name, args));
                    }
                    AiUpdate::Usage(u) => {
                        usage = Some(u);
                    }
                    AiUpdate::Error(e) => {
                        return StepResult {
                            role,
                            status: StepStatus::Failed,
                            output: format!("Error: {}", e),
                            tool_calls: tool_calls_made,
                            usage,
                        };
                    }
                    AiUpdate::Finished => break,
                }
            }

            full_response.push_str(&chunk_text);

            // If there's a tool call, execute it and loop
            if let Some((tool_name, tool_args)) = pending_tool_call {
                // Check approval
                let needs_approval = self
                    .tool_registry
                    .needs_approval(&tool_name, self.config.approval.default_tier);

                if needs_approval {
                    // In Phase 1, auto-approve with a note
                    // Phase 2 will integrate TUI approval dialog
                    full_response.push_str(&format!(
                        "\n[Auto-approved tool: {} (tiered approval pending TUI)]\n",
                        tool_name
                    ));
                }

                let result = self
                    .tool_registry
                    .execute(&tool_name, &tool_args, &self.working_dir)
                    .await;

                tool_calls_made.push(step::ToolCallRecord {
                    name: tool_name.clone(),
                    args: tool_args,
                    result: result.clone(),
                });

                // Add tool result to conversation for next iteration
                messages.push(ChatMessage {
                    role: "model".into(),
                    content: chunk_text,
                });
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "[Tool Result for '{}']:\n{}",
                        tool_name, result
                    ),
                });

                continue; // Loop for next AI response
            }

            // No tool call — agent is done
            return StepResult {
                role,
                status: StepStatus::Completed,
                output: full_response,
                tool_calls: tool_calls_made,
                usage,
            };
        }

        // Max iterations reached
        StepResult {
            role,
            status: StepStatus::Completed,
            output: full_response,
            tool_calls: tool_calls_made,
            usage: None,
        }
    }

    /// Simple single-agent chat (for Phase 1 compatibility).
    /// Sends user message to the Coder agent and returns the response.
    pub async fn chat(&mut self, user_message: &str) -> String {
        let result = self.execute_agent(AgentRole::Coder, user_message).await;

        // Store in context
        self.context.add_output(
            result.role.as_str().to_string(),
            result.output.clone(),
        );

        result.output
    }

    /// Multi-agent pipeline: orchestrator decomposes, then agents execute.
    /// For Phase 1, this is a simplified version that runs sequentially.
    pub async fn multi_agent(&mut self, user_message: &str) -> Vec<StepResult> {
        let mut results = Vec::new();

        // Step 1: Planner analyzes the task
        let plan_result = self
            .execute_agent(AgentRole::Planner, user_message)
            .await;
        self.context
            .add_output("planner".into(), plan_result.output.clone());
        results.push(plan_result);

        // Step 2: Coder implements
        let code_prompt = format!(
            "Based on this plan:\n{}\n\nImplement the changes.",
            self.context.get_output("planner").unwrap_or_default()
        );
        let code_result = self
            .execute_agent(AgentRole::Coder, &code_prompt)
            .await;
        self.context
            .add_output("coder".into(), code_result.output.clone());
        results.push(code_result);

        // Step 3: Reviewer checks
        let review_prompt = format!(
            "Review these changes:\n{}",
            self.context.get_output("coder").unwrap_or_default()
        );
        let review_result = self
            .execute_agent(AgentRole::Reviewer, &review_prompt)
            .await;
        self.context
            .add_output("reviewer".into(), review_result.output.clone());
        results.push(review_result);

        results
    }
}
