# 💎 GemChat: Agent Handoff Document

This document provides a comprehensive technical overview of the GemChat project for AI agents and developers. It covers the architecture, core components, key workflows, and recent improvements.

---

## 🏗️ Project Overview

GemChat is a terminal-based, multi-agent AI coding assistant built in Rust. It enables complex task orchestration by decomposing user goals into sub-tasks handled by specialized agents (Architect, Coder, Researcher, etc.) while maintaining safety through isolated Git worktrees and a tiered tool approval system.

### Core Tech Stack
- **Language**: Rust (Edition 2024)
- **UI Framework**: `ratatui` (TUI), `crossterm` (backend), `tui-textarea` (input)
- **Async Runtime**: `tokio`
- **AI Integration**: Custom provider system supporting Google Gemini, Anthropic Claude, and OpenAI.
- **PTY Support**: `portable-pty` for embedded terminal interactions.
- **Formatting**: `syntect` for syntax highlighting in the chat.

---

## 🗺️ High-Level Architecture

The application follows a central coordinator pattern:

1.  **TUI (`src/main.rs`)**: Manages the user interface, event loop, and state. It communicates with the `Pipeline` via asynchronous channels (`mpsc`).
2.  **Pipeline (`src/pipeline/mod.rs`)**: The "brain" of the application. It manages `PipelineContext`, `WorktreeManager`, and orchestrates calls to `AiProvider`.
3.  **Agents (`src/agents/`)**: Definitions for specialized roles. Each agent has a unique system prompt and a set of allowed tools.
4.  **Tools (`src/tools/`)**: Modular capabilities (e.g., `read_file`, `run_command`, `git_commit`) that agents can invoke.
5.  **Providers (`src/provider/`)**: Unified interface for different LLM APIs.
6.  **Worktree Manager (`src/worktree/`)**: Handles isolation by creating temporary Git worktrees for orchestration tasks.

---

## 🔑 Key Components & Modules

### `src/main.rs` (The UI & Event Loop)
- **`App` Struct**: Holds the entire UI state, including message history, input mode, terminal state, and pipeline reference.
- **`Action` Enum**: Centralized message-passing system. TUI events (like `UserInput`) and AI updates (like `ToolCall` or `RequestInput`) are all processed through this enum in the main loop.
- **Input Modes**:
    - `Normal`: Navigation and shortcuts (shortcuts: `i` for Edit, `t` for Terminal, `x` for Close Terminal).
    - `Editing`: Chat input using `tui-textarea`.
    - `Terminal`: Direct interaction with the embedded PTY.

### `src/pipeline/mod.rs` (The Orchestrator)
- **`stream_agent`**: The core loop for a single agent interaction. It dynamically builds the system prompt by combining the `AgentRole` prompt with instructions from active `Skills`.
- **`orchestrate`**: Decomposes a task using the `Orchestrator` agent and then iterates through the generated plan, spawning the appropriate agents in sequence.

### `src/app_terminal.rs` (Embedded Terminal)
- Manages an embedded pseudo-terminal (PTY) using `portable-pty`.
- **`EmbeddedTerminal`**: Stores a persistent PTY writer (wrapped in `Mutex`), vt100 parser, and child process handle. Persistence of the writer is critical for continuous interaction.
- **VT100 Rendering**: Manually renders the parser's screen state into the `ratatui` buffer in `main.rs`.

---

## 🔄 Important Workflows

### 1. Multi-Agent Orchestration (`/orchestrate`)
1.  **Worktree Creation**: A new Git worktree is created in `.gemchat-worktrees/` for the specific task.
2.  **Decomposition**: The `Orchestrator` agent receives the goal and returns a JSON plan of sub-tasks.
3.  **Execution Loop**: The pipeline iterates through the plan, sharing output context with subsequent agents via `PipelineContext`.
4.  **Finality**: The user can `/merge` the worktree changes back to the main branch if satisfied.

### 2. Tool Approval System
- Tools are categorized into `SafetyTier::Safe` (read-only) and `SafetyTier::Dangerous` (write/exec).
- High-risk tool calls trigger a confirmation dialog in the TUI. Users can approve (`y`), deny (`n`), or run interactively (`i`).

### 3. Interactive Tool Execution (`run_command` with `interactive: true`)
- When an agent sets `interactive: true`, the TUI spawns an `EmbeddedTerminal`.
- The UI switches to `InputMode::Terminal`, forwarding all keystrokes (including `Ctrl-C`, `Tab`, etc.) to the PTY writer.
- The pipeline waits on a `oneshot` channel until the terminal process exits or is manually closed, at which point the final screen state is returned to the AI.

---

## 🛠️ Agent Skills (New in v0.3.0)

Skills are pluggable modules that augment agents with specialized knowledge and tools. They allow for a more modular and extensible architecture than fixed roles.

- **`Skill` Trait**: Defines `name`, `instructions` (system prompt addition), and `provided_tools`.
- **Dynamic & Default Assignment**: 
    - `AgentRole` can define `default_skills()`. For example, `Coder` defaults to `rust_expert`.
    - Skills can also be assigned dynamically during orchestration.
- **Example Skills**:
    - `rust_expert`: Idiomatic Rust patterns and cargo toolchain integration.
    - `tdd`: Guides agents to write failing tests before implementation.
    - `web_expert`: Performance and accessibility for modern frontend frameworks.

---

## 🚀 Recent Fixes & Improvements (v0.2.0)

- **Fixed Terminal Interaction**: Resolved a bug where the PTY writer was dropped prematurely; it is now stored persistently in `EmbeddedTerminal`.
- **Pipeline Synchronization**: Improved `Action::RequestInput` handling to ensure the AI agent correctly waits for terminal interaction via `oneshot` channels.
- **Process Exit Monitoring**: The main loop now checks child process status on every `Tick`, enabling automatic terminal closure upon command completion.
- **Enhanced Keyboard Support**: Added support for control sequences (`Ctrl-C`, `Ctrl-D`, `Ctrl-L`, `Ctrl-Z`) and `Alt/Meta` keys in the embedded terminal.

---

## 🛠️ Extension Guide

### Adding a New Tool
1.  Create a struct in `src/tools/` implementing the `Tool` trait.
2.  Register it in `ToolRegistry::new()` in `src/tools/mod.rs`.

### Adding a New Skill
1.  Define the skill struct in `src/agents/skill.rs`.
2.  Register it in `SkillRegistry::new()`.
3.  Optionally assign it as a default to an `AgentRole` in `src/agents/mod.rs`.

---

## 📋 Roadmap & Future Ideas
- [ ] **Parallel Execution**: Allow independent sub-tasks in an orchestration plan to run concurrently.
- [ ] **Better Diff Visualization**: Implement a dedicated side-by-side diff viewer for worktree changes.
- [ ] **Plugin System**: Allow external WASM-based tools to be loaded dynamically.
- [ ] **LSP Integration**: Hook into Language Servers for better code awareness during agent execution.

---
*Maintained by GemChat Core Team.*
