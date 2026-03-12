use std::io::Stdout;

use clap::Parser;
use crossterm::{
    event::{self, EnableBracketedPaste, Event, KeyCode},
    terminal::EnterAlternateScreen,
};
use futures::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};
use serde::Serialize;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tui_textarea::TextArea;

use figure_8_bin::{
    Error,
    schemas::{
        BrowserCapability, Capabilities, ExecutionRequest, McpCapability, NegotiationResponse,
    },
};

const CAPABILITIES: &[&str] = &["Browser"];

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long, default_value = "ws://localhost:8080/stream")]
    url: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();

    let (mut probe, _) = tokio_tungstenite::connect_async(&args.url).await?;
    let _ = probe.close(None).await;

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
    )?;

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut list_state = ListState::default();
    list_state.select(Some(0));

    let screen = TuiState::CapabilitySelect {
        selected: vec![false; CAPABILITIES.len()],
        cursor: 0,
        list_state,
        error: None,
        mcp_servers: Vec::new(),
        mcp_input: None,
    };

    let result = run_tui(&mut terminal, screen, &args.url).await;

    crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::LeaveAlternateScreen,
    )?;
    crossterm::terminal::disable_raw_mode()?;

    result
}

#[derive(PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
enum Mode {
    Edit,
    Execute,
}

enum TuiState {
    CapabilitySelect {
        selected: Vec<bool>,
        cursor: usize,
        list_state: ListState,
        error: Option<String>,
        mcp_servers: Vec<String>,
        mcp_input: Option<Box<TextArea<'static>>>,
    },
    Editor {
        textarea: Box<TextArea<'static>>,
        mode: Mode,
        output: Vec<String>,
        ws_tx: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
        ws_rx: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    },
}

async fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut screen: TuiState,
    url: &str,
) -> Result<(), Error> {
    loop {
        terminal.draw(|f| match &mut screen {
            TuiState::CapabilitySelect {
                selected,
                list_state,
                error,
                mcp_servers,
                mcp_input,
                ..
            } => draw_capability_select(
                f,
                selected,
                mcp_servers,
                list_state,
                error.as_deref(),
                mcp_input.as_deref_mut(),
            ),
            TuiState::Editor {
                textarea,
                mode,
                output,
                ..
            } => draw_editor(f, textarea, mode, output),
        })?;

        match &mut screen {
            TuiState::CapabilitySelect {
                selected,
                cursor,
                list_state,
                error,
                mcp_servers,
                mcp_input,
            } => {
                if !event::poll(std::time::Duration::from_millis(50))? {
                    continue;
                }
                let Event::Key(key) = event::read()? else {
                    continue;
                };
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }

                if mcp_input.is_some() {
                    match key.code {
                        KeyCode::Enter => {
                            if let Some(input) = mcp_input.take() {
                                let server_url = input.lines().join("");
                                if !server_url.trim().is_empty() {
                                    mcp_servers.push(server_url.trim().to_string());
                                }
                                let total = CAPABILITIES.len() + mcp_servers.len() + 1;
                                *cursor = total - 1;
                                list_state.select(Some(*cursor));
                            }
                        }
                        KeyCode::Esc => {
                            *mcp_input = None;
                        }
                        _ => {
                            if let Some(input) = mcp_input.as_mut() {
                                input.input(Event::Key(key));
                            }
                        }
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Up => {
                            if *cursor > 0 {
                                *cursor -= 1;
                                list_state.select(Some(*cursor));
                            }
                        }
                        KeyCode::Down => {
                            let total = CAPABILITIES.len() + mcp_servers.len() + 1;
                            if *cursor + 1 < total {
                                *cursor += 1;
                                list_state.select(Some(*cursor));
                            }
                        }
                        KeyCode::Char(' ') => {
                            let add_idx = CAPABILITIES.len() + mcp_servers.len();
                            if *cursor < CAPABILITIES.len() {
                                selected[*cursor] = !selected[*cursor];
                            } else if *cursor < add_idx {
                                mcp_servers.remove(*cursor - CAPABILITIES.len());
                                let total = CAPABILITIES.len() + mcp_servers.len() + 1;
                                if *cursor >= total {
                                    *cursor = total - 1;
                                }
                                list_state.select(Some(*cursor));
                            } else {
                                let mut input = TextArea::default();
                                input.set_block(Block::bordered().title(" Server URL "));
                                input.set_cursor_line_style(Style::default());
                                *mcp_input = Some(Box::new(input));
                            }
                        }
                        KeyCode::Enter => {
                            *error = None;

                            let capabilities = Capabilities {
                                fs: None,
                                fetch: None,
                                browser: selected[0].then_some(BrowserCapability {}),
                                mcp: mcp_servers
                                    .iter()
                                    .map(|s| McpCapability { server: s.clone() })
                                    .collect(),
                            };

                            let (ws_stream, _) = tokio_tungstenite::connect_async(url).await?;

                            let (mut ws_tx, mut ws_rx) = ws_stream.split();

                            let negotiation = serde_json::to_string(&capabilities).unwrap();
                            ws_tx.send(Message::Text(negotiation.into())).await?;

                            let mut output = Vec::new();

                            if let Some(Ok(msg)) = ws_rx.next().await
                                && let Ok(text) = msg.into_text()
                                && let Ok(negotiation_response) =
                                    serde_json::from_str::<NegotiationResponse>(&text)
                            {
                                match negotiation_response {
                                    NegotiationResponse::Success { interface, .. } => {
                                        output.push("Connected successfully".to_string());
                                        output.push(interface);
                                    }
                                    NegotiationResponse::Error { message } => {
                                        *error = Some(message);
                                        continue;
                                    }
                                }
                            }

                            let mut textarea = TextArea::default();
                            textarea.set_block(Block::bordered().title(" Editor"));
                            textarea.set_cursor_line_style(Style::default());

                            screen = TuiState::Editor {
                                textarea: Box::new(textarea),
                                mode: Mode::Edit,
                                output,
                                ws_tx,
                                ws_rx,
                            };
                        }
                        _ => {}
                    }
                }
            }
            TuiState::Editor {
                textarea,
                mode,
                output,
                ws_tx,
                ws_rx,
            } => {
                tokio::select! {
                    _ = async {
                        loop {
                            if event::poll(std::time::Duration::from_millis(10)).unwrap() {
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    } => {
                        let ev = event::read()?;
                        let Event::Key(key) = ev else {
                            textarea.input(ev);
                            continue;
                        };
                        if key.kind != event::KeyEventKind::Press {
                            continue;
                        }

                        match mode {
                            Mode::Execute => {
                                match key.code {
                                    KeyCode::Char('q') => return Ok(()),
                                    KeyCode::Char('i') => *mode = Mode::Edit,
                                    KeyCode::Enter => {
                                        let code = textarea.lines().join("\n");

                                        if !code.trim().is_empty() {
                                            let json = serde_json::to_string(&ExecutionRequest { code }).unwrap();

                                            ws_tx.send(Message::Text(json.into())).await?;

                                            textarea.select_all();
                                            textarea.cut();
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Mode::Edit => {
                                match key.code {
                                    KeyCode::Esc => *mode = Mode::Execute,
                                    _ => { textarea.input(Event::Key(key)); },
                                }
                            }
                        }
                    }
                    Some(Ok(msg)) = ws_rx.next() => {
                        match msg {
                            Message::Text(text) => {
                                output.push(text.to_string());
                            }
                            Message::Close(_) => return Ok(()),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn draw_capability_select(
    f: &mut Frame,
    selected: &[bool],
    mcp_servers: &[String],
    list_state: &mut ListState,
    error: Option<&str>,
    mcp_input: Option<&mut TextArea>,
) {
    let error_height = if error.is_some() { 3 } else { 0 };
    let input_height: u16 = if mcp_input.is_some() { 3 } else { 0 };
    let total_items = CAPABILITIES.len() + mcp_servers.len() + 1;

    let [_, center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Max(total_items as u16 + 6 + error_height + input_height),
        Constraint::Fill(1),
    ])
    .areas(f.area());

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(50),
        Constraint::Fill(1),
    ])
    .areas(center);

    let [title_area, list_area, input_area, error_area, help_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(input_height),
        Constraint::Length(error_height),
        Constraint::Length(2),
    ])
    .areas(center);

    f.render_widget(
        Paragraph::new(" Select Capabilities ".bold().cyan()).centered(),
        title_area,
    );

    let mut items = Vec::new();

    for (i, name) in CAPABILITIES.iter().enumerate() {
        let check = if selected[i] { "[x]" } else { "[ ]" };
        items.push(ListItem::new(format!("  {} {}", check, name)));
    }

    for server in mcp_servers {
        items.push(ListItem::new(format!("  [mcp] {}", server)));
    }

    items.push(ListItem::new("  [+] Add MCP Server..."));

    let list = List::new(items)
        .block(Block::bordered())
        .highlight_style(Style::new().yellow().bold());

    f.render_stateful_widget(list, list_area, list_state);

    let is_input_mode = mcp_input.is_some();

    if let Some(input) = mcp_input {
        f.render_widget(&*input, input_area);
    }

    if let Some(err) = error {
        f.render_widget(
            Paragraph::new(err.red().bold())
                .wrap(Wrap { trim: false })
                .centered(),
            error_area,
        );
    }

    let help = if is_input_mode {
        Line::from(vec![
            "Enter".green(),
            ": add  |  ".into(),
            "Esc".green(),
            ": cancel".into(),
        ])
    } else {
        Line::from(vec![
            "Space".green(),
            ": toggle  |  ".into(),
            "Enter".green(),
            ": connect  |  ".into(),
            "q".green(),
            ": quit".into(),
        ])
    };

    f.render_widget(Paragraph::new(help).centered(), help_area);
}

fn draw_editor(f: &mut Frame, textarea: &mut TextArea, mode: &Mode, output: &[String]) {
    let [editor_area, output_area, status_area] = Layout::vertical([
        Constraint::Percentage(60),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(&*textarea, editor_area);

    let visible_height = output_area.height.saturating_sub(2) as usize;
    let content_width = output_area.width.saturating_sub(2) as usize;

    let output_lines = output
        .iter()
        .map(|s| format!("> {}", s).into())
        .collect::<Vec<Line>>();

    let wrapped_height: usize = output_lines
        .iter()
        .map(|line| line.width().div_ceil(content_width.max(1)).max(1))
        .sum();

    let scroll = wrapped_height.saturating_sub(visible_height) as u16;

    f.render_widget(
        Paragraph::new(output_lines)
            .block(Block::bordered().title(" Output "))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        output_area,
    );

    let mode_color = if *mode == Mode::Edit {
        Color::Green
    } else {
        Color::Yellow
    };

    let keybinds = match *mode {
        Mode::Edit => "Esc: execute".dark_gray(),
        Mode::Execute => "q: quit, i: edit, Enter: submit".dark_gray(),
    };

    f.render_widget(
        Line::from(vec![
            Span::from(format!(" {} ", serde_json::to_string(mode).unwrap()))
                .bold()
                .fg(mode_color),
            " | ".into(),
            keybinds,
        ]),
        status_area,
    );
}
