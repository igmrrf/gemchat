# 💎 GemChat

GemChat is a powerful, multi-agent AI coding assistant built for the terminal. It leverages specialized AI agents (like Architects, Coders, and Researchers) to help you solve complex programming tasks safely and efficiently.

Built with **Rust**, **Ratatui**, and powered primarily by **Google Gemini**.

---

## ✨ Key Features

- **🤖 Multi-Agent Orchestration**: Decompose complex goals into sub-tasks handled by specialized agents.
- **🏗️ Safe Worktrees**: Every orchestration run creates an isolated Git worktree. Agents never mess up your main branch until you say so.
- **🛡️ Tiered Approval**: High-risk tool calls (like running shell commands or deleting files) require your explicit `y/n` confirmation in the TUI.
- **💾 Session Persistence**: Your chat history, token usage, and pipeline states are automatically saved and can be resumed later.
- **🔌 Multi-Provider Support**: Built-in support for Google Gemini, Anthropic Claude, and OpenAI.
- **🎨 Modern TUI**: Fast, responsive terminal interface with markdown rendering and syntax highlighting.

---

## 🚀 Getting Started

### 1. Prerequisites

- **Rust**: Ensure you have the Rust toolchain installed ([rustup.rs](https://rustup.rs/)).
- **Git**: Required for worktree management and source control tools.
- **API Key**: A Google Gemini API key is recommended. You can get one for free at [aistudio.google.com](https://aistudio.google.com/).

### 2. Installation

Clone the repository and enter the directory:

```bash
git clone https://github.com/thelazydo/gemchat
cd gemchat
```

### 3. Configuration

Set your API key as an environment variable:

```bash
export GEMINI_API_KEY="your_api_key_here"
```

*(Optional)* You can also set `CLAUDE_API_KEY` or `OPENAI_API_KEY` if you wish to use those providers.

### 4. Run

Launch the application:

```bash
cargo run
```

---

## 🎮 How to Use

### Terminal Modes
GemChat operates in two modes, similar to Vim:
- **Editing Mode (`i`)**: Type your messages here. Press **Enter** to send.
- **Normal Mode (`Esc`)**: Navigate history using `j`/`k`. Use shortcuts like `q` to quit.

### Powerful Commands
Type these directly into the chat:
- `/orchestrate <goal>`: Start a multi-agent plan to achieve a complex goal.
- `/plan <goal>`: Alias for orchestrate.
- `/sessions`: List your 10 most recent saved sessions.
- `/load <session_id>`: Resume a previous conversation.
- `/merge <pipeline_id>`: Squash-merge the changes from an orchestrated worktree back into your current branch.
- `/clear`: Reset the current chat and agent context.

### Safety & Approvals
When an agent wants to perform a "dangerous" action (like `run_command`), a **Magenta** dialog will appear. 
1. Press `Esc` to enter **Normal Mode**.
2. Press `y` to approve or `n` to deny the action.

---

## 🛠️ Tech Stack

- **UI**: [Ratatui](https://ratatui.rs/) & [Crossterm](https://github.com/crossterm-rs/crossterm)
- **Editor**: [tui-textarea](https://github.com/rhysd/tui-textarea)
- **Formatting**: [Syntect](https://github.com/trishume/syntect) (Syntax Highlighting)
- **Core**: [Tokio](https://tokio.rs/) (Async Runtime) & [Serde](https://serde.rs/) (Serialization)

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for more information (if available).

Developed with ❤️ for terminal power users.
