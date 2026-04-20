pub mod context;
pub mod step;

use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::{mpsc, RwLock};

use crate::agents::{AgentRole, Agent, skill::SkillRegistry};
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
    pub skill_registry: SkillRegistry,
    pub agents: HashMap<AgentRole, Box<dyn Agent>>,
    pub working_dir: PathBuf,
    pub worktree_manager: RwLock<WorktreeManager>,
    provider: Box<dyn AiProvider>,
}

impl Pipeline {
    /// Create a new pipeline with the given config and model.
    pub fn new(config: AppConfig, model: &str, working_dir: PathBuf) -> color_eyre::Result<Self> {
        let provider = provider::create_provider(model, &config)?;
        let worktree_manager = WorktreeManager::new(working_dir.clone());

        let mut agents: HashMap<AgentRole, Box<dyn Agent>> = HashMap::new();
        agents.insert(AgentRole::Planner, Box::new(crate::agents::planner::PlannerAgent));
        agents.insert(AgentRole::Coder, Box::new(crate::agents::coder::CoderAgent));
        agents.insert(AgentRole::Orchestrator, Box::new(crate::agents::orchestrator::OrchestratorAgent));
        agents.insert(AgentRole::Architect, Box::new(crate::agents::architect::ArchitectAgent));
        agents.insert(AgentRole::Researcher, Box::new(crate::agents::researcher::ResearcherAgent));
        agents.insert(AgentRole::Reviewer, Box::new(crate::agents::reviewer::ReviewerAgent));
        agents.insert(AgentRole::Qa, Box::new(crate::agents::qa::QaAgent));
        agents.insert(AgentRole::Executor, Box::new(crate::agents::executor::ExecutorAgent));

        Ok(Self {
            config,
            context: RwLock::new(PipelineContext::new()),
            tool_registry: ToolRegistry::new(),
            skill_registry: SkillRegistry::new(),
            agents,
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
        working_dir: &std::path::Path,
        tx_out: mpsc::Sender<AiUpdate>,
    ) {
        let mut system_prompt = role.system_prompt().to_string();
        let mut allowed_tools: Vec<String> = role.allowed_tools().iter().map(|s| s.to_string()).collect();

        // Apply default skills for the role
        for skill_name in role.default_skills() {
            if let Some(skill) = self.skill_registry.get(&skill_name) {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&format!("[Skill: {}]\n{}", skill.name(), skill.instructions()));
                for tool in skill.provided_tools() {
                    if !allowed_tools.contains(&tool) {
                        allowed_tools.push(tool);
                    }
                }
            }
        }

        let tools = self.tool_registry.tool_definitions_for_vec(&allowed_tools);

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

        // Automatic history summarization if too long
        if messages.len() > 20 {
            let _ = tx_out.send(AiUpdate::Content("📝 Summarizing long context...\n".into())).await;
            if let Ok(summary) = self.summarize_history(&messages).await {
                let mut ctx = self.context.write().await;
                ctx.clear(); // Clear all and replace with summary
                ctx.add_message(ChatMessage {
                    role: "system".into(),
                    content: format!("Summary of previous context: {}", summary),
                });
                // Re-fetch messages after summarization
                messages = ctx.get_messages();
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: user_message.to_string(),
                });
            }
        }

        let max_iterations = 10;
        for _iteration in 0..max_iterations {
            let (tx, mut rx) = mpsc::channel(10);
            self.provider.stream_response(&messages, &tools, tx).await;

            let mut chunk_text = String::new();
            let mut pending_tool_call: Option<(String, String)> = None;

            while let Some(update) = rx.recv().await {
                match update {
                    AiUpdate::Content(text) => {
                        chunk_text.push_str(&text);
                        let _ = tx_out.send(AiUpdate::Content(text)).await;
                    }
                    AiUpdate::ToolCall { name, args } => {
                        pending_tool_call = Some((name, args));
                    }
                    AiUpdate::Usage(u) => {
                        let _ = tx_out.send(AiUpdate::Usage(u)).await;
                    }
                    AiUpdate::Error(e) => {
                        let _ = tx_out.send(AiUpdate::Error(e)).await;
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
                }).await;

                let needs_approval = self
                    .tool_registry
                    .needs_approval(&tool_name, &self.config.approval);

                if needs_approval {
                    let (app_tx, app_rx) = tokio::sync::oneshot::channel();
                    let _ = tx_out.send(AiUpdate::PendingApproval {
                        name: tool_name.clone(),
                        args: tool_args.clone(),
                        tx: app_tx,
                    }).await;

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
                            }).await;
                            
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
                    }).await;
                    in_rx.await.unwrap_or_else(|_| "Error: Interactive input failed".into())
                } else {
                    self.tool_registry
                        .execute(&tool_name, &tool_args, working_dir)
                        .await
                };


                let _ = tx_out.send(AiUpdate::ToolResult {
                    name: tool_name.clone(),
                    result: result.clone(),
                }).await;

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
        let _ = tx_out.send(AiUpdate::Finished).await;
    }

    /// Internal helper to summarize chat history when it gets too long.
    async fn summarize_history(&self, messages: &[ChatMessage]) -> color_eyre::Result<String> {
        let (tx, mut rx) = mpsc::channel(10);
        let summary_prompt = vec![ChatMessage {
            role: "user".into(),
            content: format!(
                "Summarize the following chat history concisely, preserving key technical decisions, \
                file paths, and tool results:\n\n{:?}",
                messages
            ),
        }];

        self.provider.stream_response(&summary_prompt, &[], tx).await;

        let mut summary = String::new();
        while let Some(update) = rx.recv().await {
            if let AiUpdate::Content(c) = update {
                summary.push_str(&c);
            }
        }
        Ok(summary)
    }

    /// Execute a single agent step: send messages to the AI, handle tool calls
    /// in a loop, and return the final text response.
    pub async fn execute_agent(
        &self,
        role: AgentRole,
        user_message: &str,
        working_dir: Option<PathBuf>,
    ) -> StepResult {
        let (tx, mut _rx) = mpsc::channel::<AiUpdate>(100);
        self.execute_agent_with_streaming(role, user_message, working_dir, tx).await
    }

    /// Combined execution and streaming for agents.
    pub async fn execute_agent_with_streaming(
        &self,
        role: AgentRole,
        user_message: &str,
        working_dir: Option<PathBuf>,
        tx_out: mpsc::Sender<AiUpdate>,
    ) -> StepResult {
        let (tx, mut rx) = mpsc::channel::<AiUpdate>(100);
        let role_clone = role;
        let dir = working_dir.unwrap_or_else(|| self.working_dir.clone());
        
        let tx_out_clone = tx_out.clone();
        let handle: tokio::task::JoinHandle<Result<(String, Vec<step::ToolCallRecord>, Option<crate::provider::Usage>), String>> = tokio::spawn(async move {
            let mut inner_output = String::new();
            let mut inner_tool_calls = Vec::new();
            let mut inner_usage = None;
            let mut last_tool_call: Option<(String, String)> = None;

            while let Some(update) = rx.recv().await {
                let mut should_break = false;
                match &update {
                    AiUpdate::Content(c) => inner_output.push_str(c),
                    AiUpdate::ToolCall { name, args } => {
                        last_tool_call = Some((name.clone(), args.clone()));
                    }
                    AiUpdate::ToolResult { result, .. } => {
                        if let Some((t_name, t_args)) = last_tool_call.take() {
                            inner_tool_calls.push(step::ToolCallRecord {
                                name: t_name,
                                args: t_args,
                                result: result.clone(),
                            });
                        }
                    }
                    AiUpdate::Usage(u) => inner_usage = Some(u.clone()),
                    AiUpdate::Error(_) => {
                        should_break = true;
                    }
                    AiUpdate::Finished => {
                        should_break = true;
                    }
                    _ => {}
                }

                // Forward to UI (moves update)
                let _ = tx_out_clone.send(update).await;
                if should_break {
                    break;
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


    /// Simple single-agent chat.
    /// Sends user message to the Coder agent and returns the response.
    pub async fn chat(&self, user_message: &str) -> String {
        let result = if let Some(agent) = self.agents.get(&AgentRole::Coder) {
            let (tx, mut _rx) = mpsc::channel(100);
            agent.process(self, user_message, None, tx).await
        } else {
            self.execute_agent(AgentRole::Coder, user_message, None).await
        };

        // Store in context
        self.context.write().await.add_output(
            result.role.as_str().to_string(),
            result.output.clone(),
        );

        result.output
    }

    /// Multi-agent pipeline: orchestrator decomposes, then agents execute.
    pub async fn multi_agent(&self, user_message: &str) -> Vec<StepResult> {
        let mut results: Vec<StepResult> = Vec::new();
        let roles = [AgentRole::Planner, AgentRole::Coder, AgentRole::Reviewer];

        for role in roles {
            let prompt = if results.is_empty() {
                user_message.to_string()
            } else {
                let prev_output = results.last().unwrap().output.clone();
                match role {
                    AgentRole::Coder => format!("Based on this plan:\n{}\n\nImplement the changes.", prev_output),
                    AgentRole::Reviewer => format!("Review these changes:\n{}", prev_output),
                    _ => user_message.to_string(),
                }
            };

            let (tx, mut _rx) = mpsc::channel(100);
            let result = if let Some(agent) = self.agents.get(&role) {
                agent.process(self, &prompt, None, tx).await
            } else {
                self.execute_agent(role, &prompt, None).await
            };

            self.context.write().await
                .add_output(role.as_str().to_string(), result.output.clone());
            results.push(result);
        }

        results
    }

    /// Orchestrate a task by decomposing it into sub-tasks and executing them.
    pub async fn orchestrate(&self, user_message: &str, tx_out: mpsc::Sender<AiUpdate>) {
        let pipeline_id = crate::worktree::WorktreeManager::new_pipeline_id();
        let _ = tx_out.send(AiUpdate::Content(format!("🎯 Orchestrating task (ID: {})...\n", pipeline_id))).await;

        // Create worktree for this orchestration
        let worktree_dir = {
            let mut wtm = self.worktree_manager.write().await;
            match wtm.create_worktree(&pipeline_id, user_message) {
                Ok(path) => {
                    let _ = tx_out.send(AiUpdate::Content(format!("🛠️ Isolated worktree created at: {}\n", path.display()))).await;
                    path
                }
                Err(e) => {
                    let _ = tx_out.send(AiUpdate::Content(format!("⚠️ Failed to create worktree: {}. Using repo root.\n", e))).await;
                    self.working_dir.clone()
                }
            }
        };

        // 1. Decompose task using Orchestrator agent
        let orchestrator = crate::agents::orchestrator::OrchestratorAgent;
        let plan = match orchestrator.decompose(self, user_message, Some(worktree_dir.clone()), tx_out.clone()).await {
            Ok(p) => p,
            Err(e) => {
                let _ = tx_out.send(AiUpdate::Content(format!("⚠️ Failed to parse structured plan: {}. Falling back to sequential execution.\n", e))).await;
                // Fallback plan
                crate::agents::orchestrator::ExecutionPlan {
                    task_summary: "Sequential execution".into(),
                    steps: vec![
                        crate::agents::orchestrator::PlanStep { id: 1, agent: AgentRole::Planner, description: "Create a plan".into(), depends_on: vec![] },
                        crate::agents::orchestrator::PlanStep { id: 2, agent: AgentRole::Coder, description: "Implement the changes".into(), depends_on: vec![1] },
                        crate::agents::orchestrator::PlanStep { id: 3, agent: AgentRole::Reviewer, description: "Review the code".into(), depends_on: vec![2] },
                    ]
                }
            }
        };

        let _ = tx_out.send(AiUpdate::Content(format!("✅ Plan established: {}\n", plan.task_summary))).await;

        // 2. Execute sub-tasks
        let mut completed_ids = Vec::new();
        for step in plan.steps {
            let role = step.agent;
            let description = step.description;

            let _ = tx_out.send(AiUpdate::Content(format!(
                "\n--- 🤖 Step {}: {} ({}) ---\n",
                step.id,
                role.display_name(),
                description
            ))).await;

            // Use context from previous steps
            let task_prompt = format!(
                "Goal: {}\n\nInstructions: Perform your assigned task based on the goal above. Use the provided context from previous agents if available.",
                description
            );

            // Execute and capture output
            let step_result = if let Some(agent) = self.agents.get(&role) {
                agent.process(self, &task_prompt, Some(worktree_dir.clone()), tx_out.clone()).await
            } else {
                self.execute_agent_with_streaming(role, &task_prompt, Some(worktree_dir.clone()), tx_out.clone()).await
            };
            
            // Store result in context for next steps
            self.context.write().await.add_output(format!("{}_{}", role.as_str(), step.id), step_result.output);
            completed_ids.push(step.id);
        }


        let _ = tx_out.send(AiUpdate::Content("\n🏁 Orchestration complete.\n".into())).await;
        let _ = tx_out.send(AiUpdate::Finished).await;
    }

    /// Merge a worktree back into the main branch.
    pub async fn merge_worktree(&self, pipeline_id: &str, tx_out: mpsc::Sender<AiUpdate>) {
        let wtm = self.worktree_manager.read().await;
        if let Some(info) = wtm.get_worktree(pipeline_id) {
            let _ = tx_out.send(AiUpdate::Content(format!("🔄 Merging worktree '{}' (branch: {})...\n", pipeline_id, info.branch))).await;
            
            match crate::worktree::merge::merge_branch(
                &self.working_dir,
                &info.branch,
                "main", // Assuming main, could be dynamic
                crate::worktree::merge::MergeStrategy::Squash,
            ) {
                Ok(result) => {
                    if result.success {
                        let _ = tx_out.send(AiUpdate::Content(format!("✅ Merge successful:\n{}\n", result.message))).await;
                    } else {
                        let _ = tx_out.send(AiUpdate::Content(format!("❌ Merge conflicts:\n{}\nConflicting files:\n{:#?}\n", result.message, result.conflicting_files))).await;
                    }
                }
                Err(e) => {
                    let _ = tx_out.send(AiUpdate::Error(format!("Failed to execute merge: {}", e))).await;
                }
            }
        } else {
            let _ = tx_out.send(AiUpdate::Error(format!("Worktree '{}' not found. Ensure the ID is correct.", pipeline_id))).await;
        }
        let _ = tx_out.send(AiUpdate::Finished).await;
    }
}
