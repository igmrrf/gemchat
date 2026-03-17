use clap::Parser;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tui_textarea::TextArea;

// Legacy module — kept for backward compatibility with existing AI streaming
mod ai;

// New multi-agent modules
mod agents;
mod cli;
mod config;
mod persistence;
mod pipeline;
mod provider;
mod tools;
mod worktree;

use crate::cli::Cli;
use crate::pipeline::Pipeline;
use crate::agents::AgentRole;
use crate::provider::AiUpdate;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

enum Action {
    UserInput(KeyEvent),
    SendMessage(String),
    AiResponseStart,
    AiResponseChunk(String),
    AiResponseError(String),
    AiResponseFinish,
    UpdateUsage(crate::provider::Usage),
    ToolCall { name: String, args: String },
    ToolResult { name: String, result: String },
    PendingApproval {
        name: String,
        args: String,
        tx: tokio::sync::oneshot::Sender<bool>,
    },
    ApproveTool(bool),
    ListSessions,
    LoadSession(String),
    Tick,
    Quit,
}

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: String,
    content: String,
}

struct App<'a> {
    textarea: TextArea<'a>,
    messages: Vec<Message>,
    should_quit: bool,
    action_tx: mpsc::UnboundedSender<Action>,
    is_loading: bool,
    spinner_index: usize,
    input_mode: InputMode,
    list_state: ListState,
    should_auto_scroll: bool,
    ps: SyntaxSet,
    ts: ThemeSet,

    // Stats
    total_prompt_tokens: i32,
    total_response_tokens: i32,

    pipeline: std::sync::Arc<Pipeline>,

    // Approval state
    pending_approval: Option<(String, String)>,
    approval_tx: Option<tokio::sync::oneshot::Sender<bool>>,

    // Persistence
    session_id: String,
    store: crate::persistence::SessionStore,
}

impl<'a> App<'a> {
    fn new(action_tx: mpsc::UnboundedSender<Action>, pipeline: Pipeline, store: crate::persistence::SessionStore) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_block(Block::default().borders(Borders::ALL).title("Input"));
        textarea.set_placeholder_text("Type message... (Enter to send, Esc to quit)");

        let session_id = uuid::Uuid::new_v4().to_string();

        Self {
            textarea,
            messages: vec![
                Message {
                    role: "System".into(),
                    content: "Welcome to GemChat!".into(),
                },
            ],
            should_quit: false,
            action_tx,
            is_loading: false,
            spinner_index: 0,
            input_mode: InputMode::Editing,
            list_state: ListState::default(),
            should_auto_scroll: true,
            ps: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
            total_prompt_tokens: 0,
            total_response_tokens: 0,
            pipeline: std::sync::Arc::new(pipeline),
            pending_approval: None,
            approval_tx: None,
            session_id,
            store,
        }
    }

    fn update(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Tick => {
                if self.is_loading {
                    self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
                }
            }
            Action::UserInput(key) => {
                match self.input_mode {
                    InputMode::Editing => {
                        match key.code {
                            KeyCode::Esc => {
                                self.input_mode = InputMode::Normal;
                            }
                            KeyCode::Enter => {
                                let input = self.textarea.lines().join("\n");
                                if !input.trim().is_empty() {
                                    self.messages.push(Message {
                                        role: "You".into(),
                                        content: input.clone(),
                                    });
                                    self.should_auto_scroll = true; // Snap to bottom on send
                                    let _ = self.action_tx.send(Action::SendMessage(input));

                                    let mut new_textarea = TextArea::default();
                                    new_textarea.set_block(self.textarea.block().cloned().unwrap());
                                    new_textarea.set_placeholder_text(
                                        "Type message... (Enter to send, Esc to quit)",
                                    );
                                    self.textarea = new_textarea;
                                }
                            }
                            _ => {
                                self.textarea.input(key);
                            }
                        }
                    }
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => self.should_quit = true,
                        KeyCode::Char('i') => self.input_mode = InputMode::Editing,
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.scroll_down();
                            self.should_auto_scroll = false;
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.scroll_up();
                            self.should_auto_scroll = false;
                        }
                        KeyCode::Char('G') => {
                            self.should_auto_scroll = true;
                            self.scroll_to_bottom();
                        }
                        KeyCode::Char('c') => {
                            self.messages.clear();
                            self.should_auto_scroll = true;
                            let _ = self.action_tx.send(Action::SendMessage("/clear".into()));
                        }
                        KeyCode::Char('y') if self.pending_approval.is_some() => {
                            let _ = self.action_tx.send(Action::ApproveTool(true));
                        }
                        KeyCode::Char('n') if self.pending_approval.is_some() => {
                            let _ = self.action_tx.send(Action::ApproveTool(false));
                        }
                        _ => {}
                    },
                }
            }
            Action::SendMessage(text) => {
                if text == "/sessions" {
                    let _ = self.action_tx.send(Action::ListSessions);
                    return Ok(());
                } else if text.starts_with("/load ") {
                    let session_id = text.trim_start_matches("/load").trim().to_string();
                    let _ = self.action_tx.send(Action::LoadSession(session_id));
                    return Ok(());
                }

                self.is_loading = true;
                self.spinner_index = 0;
                self.auto_save();

                let tx = self.action_tx.clone();
                let pipeline = self.pipeline.clone();
                
                tokio::spawn(async move {
                    let (ai_tx, mut ai_rx) = mpsc::unbounded_channel();

                    if text.starts_with("/orchestrate") || text.starts_with("/plan") {
                        let prompt = if text.starts_with("/orchestrate") {
                            text.trim_start_matches("/orchestrate").trim().to_string()
                        } else {
                            text.trim_start_matches("/plan").trim().to_string()
                        };
                        
                        tokio::spawn(async move {
                            pipeline.orchestrate(&prompt, ai_tx).await;
                        });
                    } else if text == "/clear" {
                        let _ = pipeline.context.write().await.clear();
                        let _ = ai_tx.send(AiUpdate::Content("🧹 Context cleared.".into()));
                        let _ = ai_tx.send(AiUpdate::Finished);
                    } else if text.starts_with("/merge") {
                        let pipeline_id = text.trim_start_matches("/merge").trim().to_string();
                        if pipeline_id.is_empty() {
                            let _ = ai_tx.send(AiUpdate::Error("Usage: /merge <pipeline_id>".into()));
                        } else {
                            tokio::spawn(async move {
                                pipeline.merge_worktree(&pipeline_id, ai_tx).await;
                            });
                        }
                    } else {
                        tokio::spawn(async move {
                            pipeline.stream_agent(AgentRole::Coder, &text, &pipeline.working_dir, ai_tx).await;
                        });
                    }

                    let _ = tx.send(Action::AiResponseStart);

                    while let Some(update) = ai_rx.recv().await {
                        match update {
                            AiUpdate::Content(s) => {
                                let _ = tx.send(Action::AiResponseChunk(s));
                            }
                            AiUpdate::Usage(usage) => {
                                let _ = tx.send(Action::UpdateUsage(usage));
                            }
                            AiUpdate::Error(e) => {
                                let _ = tx.send(Action::AiResponseError(e));
                            }
                            AiUpdate::ToolCall { name, args } => {
                                let _ = tx.send(Action::ToolCall { name, args });
                            }
                            AiUpdate::PendingApproval { name, args, tx: app_tx } => {
                                let _ = tx.send(Action::PendingApproval { name, args, tx: app_tx });
                            }
                            AiUpdate::ToolResult { name, result } => {
                                let _ = tx.send(Action::ToolResult { name, result });
                            }
                            AiUpdate::Finished => {
                                let _ = tx.send(Action::AiResponseFinish);
                                break;
                            }
                        }
                    }
                });
            }
            Action::AiResponseStart => {
                self.messages.push(Message {
                    role: "AI".into(),
                    content: String::new(),
                });
                if self.should_auto_scroll {
                    self.scroll_to_bottom();
                }
            }
            Action::AiResponseChunk(chunk) => {
                if let Some(last_msg) = self.messages.last_mut() {
                    if last_msg.role == "AI" {
                        last_msg.content.push_str(&chunk);
                    }
                }
            }
            Action::UpdateUsage(usage) => {
                self.total_prompt_tokens += usage.prompt_tokens;
                self.total_response_tokens += usage.response_tokens;
                self.auto_save();
            }
            Action::AiResponseError(err) => {
                self.messages.push(Message {
                    role: "Error".into(),
                    content: err,
                });
                self.is_loading = false;
                self.auto_save();
            }
            Action::AiResponseFinish => {
                self.is_loading = false;
                self.auto_save();
            }

            Action::ToolCall { name, args: _ } => {
                self.messages.push(Message {
                    role: "System".into(),
                    content: format!("Executing tool: `{}`", name),
                });
                if self.should_auto_scroll {
                    self.scroll_to_bottom();
                }
                self.auto_save();
            }
            Action::ToolResult { name, result } => {
                self.messages.push(Message {
                    role: "Tool Result".into(),
                    content: format!("**{}**\n```text\n{}\n```", name, result),
                });
                if self.should_auto_scroll {
                    self.scroll_to_bottom();
                }
                self.auto_save();
            }
            Action::PendingApproval { name, args, tx } => {
                self.pending_approval = Some((name, args));
                self.auto_save();
                self.approval_tx = Some(tx);
            }
            Action::ApproveTool(approved) => {
                if let Some(tx) = self.approval_tx.take() {
                    let _ = tx.send(approved);
                }
                self.pending_approval = None;
                self.auto_save();
            }
            Action::ListSessions => {
                match self.store.load_active_pipelines() {
                    Ok(records) => {
                        let mut msg = String::from("### Recent Sessions\n\n");
                        if records.is_empty() {
                            msg.push_str("No active sessions found.\n");
                        } else {
                            for r in records.iter().take(10) {
                                msg.push_str(&format!("- **{}** ({}): {} tokens\n", r.id, r.updated_at.format("%Y-%m-%d %H:%M"), r.total_tokens));
                            }
                            msg.push_str("\n*Use `/load <id>` to resume a session.*");
                        }
                        self.messages.push(Message {
                            role: "System".into(),
                            content: msg,
                        });
                        if self.should_auto_scroll {
                            self.scroll_to_bottom();
                        }
                    }
                    Err(e) => {
                        self.messages.push(Message {
                            role: "Error".into(),
                            content: format!("Failed to list sessions: {}", e),
                        });
                    }
                }
            }
            Action::LoadSession(id) => {
                match self.store.load_pipeline(&id) {
                    Ok(record) => {
                        self.session_id = record.id.clone();
                        let mut loaded_msgs = Vec::new();
                        for val in record.messages {
                            if let Ok(msg) = serde_json::from_value(val) {
                                loaded_msgs.push(msg);
                            }
                        }
                        self.messages = loaded_msgs;
                        self.messages.push(Message {
                            role: "System".into(),
                            content: format!("✅ Session {} loaded successfully.", id),
                        });
                        if self.should_auto_scroll {
                            self.scroll_to_bottom();
                        }
                    }
                    Err(e) => {
                        self.messages.push(Message {
                            role: "Error".into(),
                            content: format!("Failed to load session '{}': {}", id, e),
                        });
                    }
                }
            }
        }
        Ok(())
    }


    fn auto_save(&self) {
        let mut record = crate::persistence::store::PipelineRecord::new(
            self.session_id.clone(),
            "GemChat Session".into(),
            self.pipeline.working_dir.to_string_lossy().to_string(),
        );

        record.total_tokens = (self.total_prompt_tokens + self.total_response_tokens) as i64;
        record.messages = self
            .messages
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect();

        // Capture context snapshot if possible
        // Since it's async, we might skip it or use a simplified approach
        
        let _ = self.store.save_pipeline(&record);
    }

    fn scroll_up(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    0
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn scroll_down(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.total_list_items() - 1 {
                    i
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn scroll_to_bottom(&mut self) {
        let count = self.total_list_items();
        if count > 0 {
            self.list_state.select(Some(count - 1));
        }
    }

    fn total_list_items(&self) -> usize {
        let mut count = 0;
        for msg in &self.messages {
            count += 1; // Header
            count += parse_markdown(&msg.content, &self.ps, &self.ts).len(); // Content lines
            count += 1; // Spacer
        }
        count
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Main Layout: Left Sidebar (25 chars) | Right Main (Min 0)
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Length(25), Constraint::Min(0)])
            .split(frame.area());

        // Sidebar
        let sidebar_area = main_layout[0];
        let main_area = main_layout[1];

        self.draw_sidebar(frame, sidebar_area);
        self.draw_main_chat(frame, main_area);
    }

    fn draw_sidebar(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let sidebar_block = Block::default()
            .borders(Borders::ALL)
            .title("Sidebar")
            .style(Style::default().fg(Color::Cyan));

        let inner_area = sidebar_block.inner(area);
        frame.render_widget(sidebar_block, area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(10), // Stats
                Constraint::Min(0),     // Keybindings
            ])
            .split(inner_area);

        // Stats
        let stats_text = vec![
            Line::from(Span::styled(
                "Model:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("Gemini 3 Flash"),
            Line::from(""),
            Line::from(Span::styled(
                "Tokens:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("Prompt: {}", self.total_prompt_tokens)),
            Line::from(format!("Resp:   {}", self.total_response_tokens)),
            Line::from(format!(
                "Total:  {}",
                self.total_prompt_tokens + self.total_response_tokens
            )),
        ];
        frame.render_widget(Paragraph::new(stats_text), layout[0]);

        // Keybindings
        let help_text = vec![
            Line::from(Span::styled(
                "Keys:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from("Esc: Normal Mode"),
            Line::from("i:   Edit Mode"),
            Line::from("Ent: Send"),
            Line::from("j/k: Scroll"),
            Line::from("G:   Bottom"),
            Line::from("c:   Clear"),
            Line::from("q:   Quit"),
        ];
        frame.render_widget(Paragraph::new(help_text), layout[1]);
    }

    fn draw_main_chat(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Min(1),    // Messages area
                Constraint::Length(3), // Input area
            ])
            .split(area);

        let mut list_items = Vec::new();
        for (i, msg) in self.messages.iter().enumerate() {
            let content_lines = parse_markdown(&msg.content, &self.ps, &self.ts);

            let mut role_spans = vec![Span::styled(
                format!("{}: ", msg.role),
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(match msg.role.as_str() {
                        "You" => Color::Blue,
                        "AI" => Color::Green,
                        "Error" => Color::Red,
                        _ => Color::Yellow,
                    }),
            )];

            if self.is_loading && i == self.messages.len() - 1 && msg.role == "AI" {
                role_spans.push(Span::styled(
                    format!(" {} ", SPINNER_FRAMES[self.spinner_index]),
                    Style::default().fg(Color::Yellow),
                ));
            }

            let header = Line::from(role_spans);
            list_items.push(ListItem::new(header));

            for line in content_lines {
                list_items.push(ListItem::new(line));
            }
            list_items.push(ListItem::new(Line::from(""))); // Spacer
        }

        if self.should_auto_scroll {
            if !list_items.is_empty() {
                self.list_state.select(Some(list_items.len() - 1));
            }
        }

        let title = match self.input_mode {
            InputMode::Editing => "Chat (Editing)",
            InputMode::Normal => "Chat (Normal)",
        };

        let messages_list = List::new(list_items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(messages_list, layout[0], &mut self.list_state);

        let input_block_style = match self.input_mode {
            InputMode::Editing => Style::default().fg(Color::Yellow),
            InputMode::Normal => Style::default().fg(Color::DarkGray),
        };

        let mut textarea = self.textarea.clone();
        let title = if let Some((name, _)) = &self.pending_approval {
            format!("Tool Approval: {} (y/n)", name)
        } else {
            "Input".to_string()
        };

        let block_style = if self.pending_approval.is_some() {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            input_block_style
        };

        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(block_style),
        );

        if let Some((name, args)) = &self.pending_approval {
             textarea.set_placeholder_text(format!("Approve tool '{}' with args '{}'? (y)es / (n)o", name, args));
        }

        frame.render_widget(&textarea, layout[1]);
    }
}

// Markdown Parser with Syntax Highlighting
fn parse_markdown<'a>(text: &'a str, ps: &SyntaxSet, ts: &ThemeSet) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut current_lang = String::new();
    let mut code_block_content = String::new();

    for line in text.lines() {
        if line.trim().starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;

                // Highlight accumulated code
                let syntax = ps
                    .find_syntax_by_token(&current_lang)
                    .unwrap_or_else(|| ps.find_syntax_plain_text());

                // Use a dark theme for better contrast on terminals usually
                let theme = &ts.themes["base16-ocean.dark"];
                let mut h = HighlightLines::new(syntax, theme);

                for code_line in LinesWithEndings::from(&code_block_content) {
                    let ranges: Vec<(syntect::highlighting::Style, &str)> =
                        h.highlight_line(code_line, ps).unwrap_or_default();
                    let spans: Vec<Span> = ranges
                        .into_iter()
                        .map(|(style, content)| {
                            Span::styled(content.to_string(), translate_style(style))
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }

                // Add closing fence (optional, maybe dim it)
                lines.push(Line::from(Span::styled(
                    "```",
                    Style::default().fg(Color::DarkGray),
                )));

                code_block_content.clear();
            } else {
                // Start of code block
                in_code_block = true;
                current_lang = line.trim().trim_start_matches("```").to_string();
                lines.push(Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else if in_code_block {
            code_block_content.push_str(line);
            code_block_content.push('\n');
        } else {
            let parts = parse_inline_styles(line);
            lines.push(Line::from(parts));
        }
    }

    // Handle unclosed code blocks (during streaming)
    if in_code_block && !code_block_content.is_empty() {
        let syntax = ps
            .find_syntax_by_token(&current_lang)
            .unwrap_or_else(|| ps.find_syntax_plain_text());
        let theme = &ts.themes["base16-ocean.dark"];
        let mut h = HighlightLines::new(syntax, theme);

        for code_line in LinesWithEndings::from(&code_block_content) {
            let ranges: Vec<(syntect::highlighting::Style, &str)> =
                h.highlight_line(code_line, ps).unwrap_or_default();
            let spans: Vec<Span> = ranges
                .into_iter()
                .map(|(style, content)| Span::styled(content.to_string(), translate_style(style)))
                .collect();
            lines.push(Line::from(spans));
        }
    }

    lines
}

fn translate_style(style: syntect::highlighting::Style) -> Style {
    Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ))
}

fn parse_inline_styles(line: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut chars = line.chars().peekable();
    let mut is_bold = false;

    while let Some(c) = chars.next() {
        if c == '*' && chars.peek() == Some(&'*') {
            chars.next(); // consume second *
            if !current_text.is_empty() {
                spans.push(if is_bold {
                    Span::styled(
                        current_text.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(current_text.clone())
                });
                current_text.clear();
            }
            is_bold = !is_bold;
        } else {
            current_text.push(c);
        }
    }
    if !current_text.is_empty() {
        spans.push(if is_bold {
            Span::styled(current_text, Style::default().add_modifier(Modifier::BOLD))
        } else {
            Span::raw(current_text)
        });
    }
    spans
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    dotenvy::dotenv().ok();

    let _cli = Cli::parse();

    let terminal = ratatui::init();
    let result = run(terminal).await;
    ratatui::restore();
    result
}

async fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // Initialize pipeline
    let config = crate::config::load_config()?;
    let model = crate::config::model_for_role(&config, "coder");
    let working_dir = std::env::current_dir()?;
    let store = crate::persistence::SessionStore::new(config.general.persistence_ttl_hours)?;
    let pipeline = Pipeline::new(config, &model, working_dir)?;
    
    let mut app = App::new(tx.clone(), pipeline, store);

    // Tick task
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if tick_tx.send(Action::Tick).is_err() {
                break;
            }
        }
    });

    let input_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    if input_tx.send(Action::UserInput(key)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        terminal.draw(|frame| app.draw(frame))?;

        if let Some(action) = rx.recv().await {
            app.update(action)?;
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
