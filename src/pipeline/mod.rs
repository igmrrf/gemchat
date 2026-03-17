pub mod context;
pub mod step;

use std::path::PathBuf;

use tokio::sync::{mpsc, RwLock};

use crate::agents::AgentRole;
use crate::config::AppConfig;
use crate::provider::{self, AiProvider, AiUpdate, ChatMessage};
use crate::tools::ToolRegistry;

use context::PipelineContext;
use step::{StepResult, StepStatus};

use crate::worktree::WorktreeManager;

/// The pipeline coordinator routes messages to agents, manages context,
/// and handles tool call loops.
pub struct Pipeline {
    pub config: AppConfig,
    pub context: RwLock<PipelineContext>,
    pub tool_registry: ToolRegistry,
    pub working_dir: PathBuf,
    pub worktree_manager: RwLock<WorktreeManager>,
    provider: Box<dyn AiProvider>,
}

impl Pipeline {
    /// Create a new pipeline with the given config and model.
    pub fn new(config: AppConfig, model: &str, working_dir: PathBuf) -> color_eyre::Result<Self> {
        let provider = provider::create_provider(model, &config)?;
        let worktree_manager = WorktreeManager::new(working_dir.clone());
        Ok(Self {
            config,
            context: RwLock::new(PipelineContext::new()),
            tool_registry: ToolRegistry::new(),
            working_dir,
            worktree_manager: RwLock::new(worktree_manager),
            provider,
        })
    }

    /// Expose the tool registry for UI tool calls.
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Stream an agent's execution, handling tools internally and sending
    /// status updates back through a channel.
    pub async fn stream_agent(
        &self,
        role: AgentRole,
        user_message: &str,
        working_dir: &PathBuf,
        tx_out: mpsc::UnboundedSender<AiUpdate>,
    ) {
        let system_prompt = role.system_prompt();
        let allowed_tools = role.allowed_tools();
        let tools = self.tool_registry.tool_definitions_for(allowed_tools);

        // Retrieve existing history from context
        let mut messages = self.context.read().await.get_messages();

        // If history is empty, add system prompt first
        if messages.is_empty() {
             messages.push(ChatMessage {
                role: "user".into(), // Some providers require user role for system context if they don't support 'system'
                content: format!("[System] {}", system_prompt),
            });
            messages.push(ChatMessage {
                role: "model".into(),
                content: "I understand my role and instructions. How can I help you?".into(),
            });
        }

        // Add the new user message
        messages.push(ChatMessage {
            role: "user".into(),
            content: user_message.to_string(),
        });

        // Add context from recent agent outputs (if any) as a reminder
        for ctx in self.context.read().await.recent_outputs(3) {
            messages.push(ChatMessage {
                role: "user".into(), // Use 'user' to inject context info
                content: format!("[Context from {}] {}", ctx.0, ctx.1),
            });
        }

        let max_iterations = 10;
        for _iteration in 0..max_iterations {
            let (tx, mut rx) = mpsc::unbounded_channel();
            self.provider.stream_response(&messages, &tools, tx).await;

            let mut chunk_text = String::new();
            let mut pending_tool_call: Option<(String, String)> = None;

            while let Some(update) = rx.recv().await {
                match update {
                    AiUpdate::Content(text) => {
                        chunk_text.push_str(&text);
                        let _ = tx_out.send(AiUpdate::Content(text));
                    }
                    AiUpdate::ToolCall { name, args } => {
                        pending_tool_call = Some((name, args));
                    }
                    AiUpdate::Usage(u) => {
                        let _ = tx_out.send(AiUpdate::Usage(u));
                    }
                    AiUpdate::Error(e) => {
                        let _ = tx_out.send(AiUpdate::Error(e));
                        return;
                    }
                    AiUpdate::Finished => break,
                    AiUpdate::ToolResult { .. } => {} // Should not happen here
                    AiUpdate::PendingApproval { .. } => {} // Should not happen here
                    AiUpdate::RequestInput { .. } => {} // Should not happen here
                }
            }

            if let Some((tool_name, mut tool_args)) = pending_tool_call {
                let _ = tx_out.send(AiUpdate::ToolCall {
                    name: tool_name.clone(),
                    args: tool_args.clone(),
                });

                let needs_approval = self
                    .tool_registry
                    .needs_approval(&tool_name, &self.config.approval);

                if needs_approval {
                    let (app_tx, app_rx) = tokio::sync::oneshot::channel();
                    let _ = tx_out.send(AiUpdate::PendingApproval {
                        name: tool_name.clone(),
                        args: tool_args.clone(),
                        tx: app_tx,
                    });

                    // Wait for user approval
                    match app_rx.await {
                        Ok((true, updated_args)) => {
                            // Proceed
                            if let Some(new_args) = updated_args {
                                tool_args = new_args;
                            }
                        }
                        _ => {
                            // Denied
                            let result = "Tool execution denied by user.".to_string();
                            let _ = tx_out.send(AiUpdate::ToolResult {
                                name: tool_name.clone(),
                                result: result.clone(),
                            });
                            
                            // Append to local messages for loop iteration
                            messages.push(ChatMessage {
                                role: "model".into(),
                                content: format!("{}\n[Tool Call: {}({})]", chunk_text, tool_name, tool_args),
                            });
                            messages.push(ChatMessage {
                                role: "user".into(),
                                content: format!("[Tool Result for '{}']:\n{}", tool_name, result),
                            });
                            continue;
                        }
                    }
                }

                let result = if self.tool_registry.requires_input(&tool_name, &tool_args) {
                    let (in_tx, in_rx) = tokio::sync::oneshot::channel();
                    let _ = tx_out.send(AiUpdate::RequestInput {
                        name: tool_name.clone(),
                        args: tool_args.clone(),
                        tx: in_tx,
                    });
                    in_rx.await.unwrap_or_else(|_| "Error: Interactive input failed".into())
                } else {
                    self.tool_registry
                        .execute(&tool_name, &tool_args, working_dir)
                        .await
                };


                let _ = tx_out.send(AiUpdate::ToolResult {
                    name: tool_name.clone(),
                    result: result.clone(),
                });

                // Append to local messages for loop iteration
                messages.push(ChatMessage {
                    role: "model".into(),
                    content: format!("{}\n[Tool Call: {}({})]", chunk_text, tool_name, tool_args),
                });
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: format!("[Tool Result for '{}']:\n{}", tool_name, result),
                });
                continue;
            }

            // No tool call or tool finished, final response content is in chunk_text
            // Store the final interaction in PipelineContext
            {
                let mut ctx = self.context.write().await;
                ctx.add_message(ChatMessage {
                    role: "user".into(),
                    content: user_message.to_string(),
                });
                ctx.add_message(ChatMessage {
                    role: "model".into(),
                    content: chunk_text.clone(),
                });
            }
            break;
        }
        let _ = tx_out.send(AiUpdate::Finished);
    }

    /// Execute a single agent step: send messages to the AI, handle tool calls
    /// in a loop, and return the final text response.
    pub async fn execute_agent(
        &self,
        role: AgentRole,
        user_message: &str,
        working_dir: Option<PathBuf>,
    ) -> StepResult {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let role_clone = role;
        let dir = working_dir.unwrap_or_else(|| self.working_dir.clone());
        
        let handle = tokio::spawn(async move {
            let mut inner_output = String::new();
            let mut inner_tool_calls = Vec::new();
            let mut inner_usage = None;
            let mut last_tool_call: Option<(String, String)> = None;

            while let Some(update) = rx.recv().await {
                match update {
                    AiUpdate::Content(c) => inner_output.push_str(&c),
                    AiUpdate::ToolCall { name, args } => {
                        last_tool_call = Some((name, args));
                    }
                    AiUpdate::ToolResult { name, result } => {
                        if let Some((t_name, t_args)) = last_tool_call.take() {
                            inner_tool_calls.push(step::ToolCallRecord {
                                name: t_name,
                                args: t_args,
                                result,
                            });
                        }
                    }
                    AiUpdate::Usage(u) => inner_usage = Some(u),
                    AiUpdate::Error(e) => return Err(e),
                    AiUpdate::Finished => break,
                    AiUpdate::PendingApproval { .. } => {} 
                    AiUpdate::RequestInput { .. } => {}
                }
            }
            Ok((inner_output, inner_tool_calls, inner_usage))
        });

        self.stream_agent(role, user_message, &dir, tx).await;

        match handle.await.unwrap() {
            Ok((output, calls, u)) => StepResult {
                role: role_clone,
                status: StepStatus::Completed,
                output,
                tool_calls: calls,
                usage: u,
            },
            Err(e) => StepResult {
                role: role_clone,
                status: StepStatus::Failed,
                output: format!("Error: {}", e),
                tool_calls: Vec::new(),
                usage: None,
            },
        }
    }


    /// Simple single-agent chat (for Phase 1 compatibility).
    /// Sends user message to the Coder agent and returns the response.
    pub async fn chat(&self, user_message: &str) -> String {
        let result = self.execute_agent(AgentRole::Coder, user_message, None).await;

        // Store in context
        self.context.write().await.add_output(
            result.role.as_str().to_string(),
            result.output.clone(),
        );

        result.output
    }

    /// Multi-agent pipeline: orchestrator decomposes, then agents execute.
    /// For Phase 1, this is a simplified version that runs sequentially.
    pub async fn multi_agent(&self, user_message: &str) -> Vec<StepResult> {
        let mut results = Vec::new();

        // Step 1: Planner analyzes the task
        let plan_result = self
            .execute_agent(AgentRole::Planner, user_message, None)
            .await;
        self.context.write().await
            .add_output("planner".into(), plan_result.output.clone());
        results.push(plan_result);

        // Step 2: Coder implements
        let code_prompt = format!(
            "Based on this plan:\n{}\n\nImplement the changes.",
            self.context.read().await.get_output("planner").unwrap_or_default()
        );
        let code_result = self
            .execute_agent(AgentRole::Coder, &code_prompt, None)
            .await;
        self.context.write().await
            .add_output("coder".into(), code_result.output.clone());
        results.push(code_result);

        // Step 3: Reviewer checks
        let review_prompt = format!(
            "Review these changes:\n{}",
            self.context.read().await.get_output("coder").unwrap_or_default()
        );
        let review_result = self
            .execute_agent(AgentRole::Reviewer, &review_prompt, None)
            .await;
        self.context.write().await
            .add_output("reviewer".into(), review_result.output.clone());
        results.push(review_result);

        results
    }

    /// Orchestrate a task by decomposing it into sub-tasks and executing them.
    pub async fn orchestrate(&self, user_message: &str, tx_out: mpsc::UnboundedSender<AiUpdate>) {
        let pipeline_id = crate::worktree::WorktreeManager::new_pipeline_id();
        let _ = tx_out.send(AiUpdate::Content(format!("🎯 Orchestrating task (ID: {})...\n", pipeline_id)));

        // Create worktree for this orchestration
        let worktree_dir = {
            let mut wtm = self.worktree_manager.write().await;
            match wtm.create_worktree(&pipeline_id, user_message) {
                Ok(path) => {
                    let _ = tx_out.send(AiUpdate::Content(format!("🛠️ Isolated worktree created at: {}\n", path.display())));
                    path
                }
                Err(e) => {
                    let _ = tx_out.send(AiUpdate::Content(format!("⚠️ Failed to create worktree: {}. Using repo root.\n", e)));
                    self.working_dir.clone()
                }
            }
        };

        // 1. Decompose task using Orchestrator agent
        let decomposition_prompt = format!(
            "Decompose the following user request into a list of sub-tasks. \
            Output ONLY a JSON array of objects with 'id', 'agent' (role), and 'description'. \
            Available agents: planner, researcher, architect, coder, reviewer, qa, executor.\n\n\
            Request: {}",
            user_message
        );

        let decomposition_result = self
            .execute_agent(AgentRole::Orchestrator, &decomposition_prompt, Some(worktree_dir.clone()))
            .await;

        if decomposition_result.status == StepStatus::Failed {
            let _ = tx_out.send(AiUpdate::Error(format!(
                "Orchestration failed: {}",
                decomposition_result.output
            )));
            return;
        }

        let _ = tx_out.send(AiUpdate::Content(format!(
            "✅ Task decomposed: \n{}\n\n",
            decomposition_result.output
        )));

        // Try to parse the plan
        let plan_json = decomposition_result.output.trim();
        // Basic extraction if AI wraps in code blocks
        let plan_json = if plan_json.starts_with("```json") {
            plan_json.trim_start_matches("```json").trim_end_matches("```")
        } else if plan_json.starts_with("```") {
            plan_json.trim_start_matches("```").trim_end_matches("```")
        } else {
            plan_json
        };

        let plan: Vec<serde_json::Value> = match serde_json::from_str(plan_json) {
            Ok(p) => p,
            Err(_) => {
                let _ = tx_out.send(AiUpdate::Content(
                    "⚠️ Could not parse structured plan. Falling back to sequential execution.\n"
                        .into(),
                ));
                // Fallback to a simple sequence
                vec![
                    serde_json::json!({"agent": "planner", "description": "Create a plan"}),
                    serde_json::json!({"agent": "coder", "description": "Implement the changes"}),
                    serde_json::json!({"agent": "reviewer", "description": "Review the code"}),
                ]
            }
        };

        // 2. Execute sub-tasks
        for task in plan {
            let agent_str = task["agent"].as_str().unwrap_or("coder");
            let description = task["description"].as_str().unwrap_or("");

            let role = match agent_str {
                "planner" => AgentRole::Planner,
                "researcher" => AgentRole::Researcher,
                "architect" => AgentRole::Architect,
                "coder" => AgentRole::Coder,
                "reviewer" => AgentRole::Reviewer,
                "qa" => AgentRole::Qa,
                "executor" => AgentRole::Executor,
                _ => AgentRole::Coder,
            };

            let _ = tx_out.send(AiUpdate::Content(format!(
                "\n--- 🤖 Executing {}: {} ---\n",
                role.display_name(),
                description
            )));

            // Use context from previous steps
            let task_prompt = format!(
                "Goal: {}\n\nInstructions: Perform your assigned task based on the goal above. Use the provided context from previous agents if available.",
                description
            );

            // Execute and capture output
            let step_result = self.execute_agent(role, &task_prompt, Some(worktree_dir.clone())).await;
            
            // Forward content to UI
            let _ = tx_out.send(AiUpdate::Content(step_result.output.clone()));
            
            // Forward usage and tool info
            if let Some(u) = step_result.usage {
                let _ = tx_out.send(AiUpdate::Usage(u));
            }
            for call in step_result.tool_calls {
                let _ = tx_out.send(AiUpdate::ToolCall { name: call.name.clone(), args: call.args });
                let _ = tx_out.send(AiUpdate::ToolResult { name: call.name, result: call.result });
            }

            // Store result in context for next steps
            self.context.write().await.add_output(agent_str.to_string(), step_result.output);
        }


        let _ = tx_out.send(AiUpdate::Content("\n🏁 Orchestration complete.\n".into()));
        let _ = tx_out.send(AiUpdate::Finished);
    }

    /// Merge a worktree back into the main branch.
    pub async fn merge_worktree(&self, pipeline_id: &str, tx_out: mpsc::UnboundedSender<AiUpdate>) {
        let wtm = self.worktree_manager.read().await;
        if let Some(info) = wtm.get_worktree(pipeline_id) {
            let _ = tx_out.send(AiUpdate::Content(format!("🔄 Merging worktree '{}' (branch: {})...\n", pipeline_id, info.branch)));
            
            match crate::worktree::merge::merge_branch(
                &self.working_dir,
                &info.branch,
                "main", // Assuming main, could be dynamic
                crate::worktree::merge::MergeStrategy::Squash,
            ) {
                Ok(result) => {
                    if result.success {
                        let _ = tx_out.send(AiUpdate::Content(format!("✅ Merge successful:\n{}\n", result.message)));
                    } else {
                        let _ = tx_out.send(AiUpdate::Content(format!("❌ Merge conflicts:\n{}\nConflicting files:\n{:#?}\n", result.message, result.conflicting_files)));
                    }
                }
                Err(e) => {
                    let _ = tx_out.send(AiUpdate::Error(format!("Failed to execute merge: {}", e)));
                }
            }
        } else {
            let _ = tx_out.send(AiUpdate::Error(format!("Worktree '{}' not found. Ensure the ID is correct.", pipeline_id)));
        }
        let _ = tx_out.send(AiUpdate::Finished);
    }
}
