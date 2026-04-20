# ADR 001: Multi-Agent Architecture Refactor

## Status
Proposed (Active Refactor)

## Context
The current implementation of `Pipeline` (in `src/pipeline/mod.rs`) is becoming a monolithic "God Object" that handles orchestration, tool execution, and provider communication for all agent roles. The agent logic itself is currently limited to system prompt switching within a single loop.

We have stubbed out individual agent modules in `src/agents/` (e.g., `coder.rs`, `architect.rs`), but they are not yet integrated.

## Decision
We will transition to a decentralized, trait-based multi-agent architecture.

### 1. The `Agent` Trait
Each agent will implement a common `Agent` trait to allow for uniform execution and composition.

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn role(&self) -> AgentRole;
    async fn process(&self, context: &PipelineContext, input: &str) -> Result<AgentOutput>;
}
```

### 2. Stateful Agents
Agents will maintain their own internal state or view of the `PipelineContext` to allow for specialized reasoning (e.g., the Coder focusing on file diffs while the Researcher focuses on search results).

### 3. Orchestration via "Plan-Act-Review"
The `Orchestrator` will move from a sequential JSON loop to a more dynamic model where it can re-plan based on agent feedback.

### 4. Bounded Asynchrony
All agent communication will use bounded channels (`tokio::sync::mpsc::channel`) to prevent memory exhaustion and provide backpressure to the AI providers.

## Consequences

### Positive
- **Maintainability:** Easier to add or modify specific agent behaviors without touching the core pipeline.
- **Scalability:** Enables complex multi-agent interactions like peer review or automated QA loops.
- **Reliability:** Bounded channels and structured error handling improve system stability.

### Negative
- **Complexity:** Higher initial boilerplate to implement the trait for each agent.
- **Overhead:** Slight performance cost due to additional abstraction layers and context cloning.
