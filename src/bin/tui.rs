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
    schemas::{Capabilities, ExecutionRequest, InstanceConfig, StreamResponse, SuccessResponse},
};

const CAPABILITIES: &[(&str, fn() -> Capabilities)] = &[("Browser", || Capabilities::Browser)];

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
                ..
            } => draw_capability_select(f, selected, list_state, error.as_deref()),
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

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Up => {
                        if *cursor > 0 {
                            *cursor -= 1;
                            list_state.select(Some(*cursor));
                        }
                    }
                    KeyCode::Down => {
                        if *cursor + 1 < CAPABILITIES.len() {
                            *cursor += 1;
                            list_state.select(Some(*cursor));
                        }
                    }
                    KeyCode::Char(' ') => {
                        selected[*cursor] = !selected[*cursor];
                    }
                    KeyCode::Enter => {
                        *error = None;

                        let capabilities = selected
                            .iter()
                            .enumerate()
                            .filter_map(|(i, s)| s.then_some((CAPABILITIES[i].1)()))
                            .collect::<Vec<_>>();

                        let config = InstanceConfig { capabilities };

                        let (ws_stream, _) = tokio_tungstenite::connect_async(url).await?;

                        let (mut ws_tx, mut ws_rx) = ws_stream.split();

                        let negotiation = serde_json::to_string(&config).unwrap();
                        ws_tx.send(Message::Text(negotiation.into())).await?;

                        let mut output = Vec::new();

                        if let Some(Ok(msg)) = ws_rx.next().await
                            && let Ok(text) = msg.into_text()
                            && let Ok(stream_response) =
                                serde_json::from_str::<StreamResponse>(&text)
                        {
                            match stream_response {
                                StreamResponse::Success(SuccessResponse::Negotiation {
                                    capabilities,
                                }) => {
                                    output.push(format!(
                                        "Negotiated capabilities: {:?}",
                                        capabilities
                                    ));
                                }
                                StreamResponse::Error { message } => {
                                    *error = Some(message);
                                    continue;
                                }
                                _ => return Ok(()),
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
    list_state: &mut ListState,
    error: Option<&str>,
) {
    let error_height = if error.is_some() { 3 } else { 0 };

    let [_, center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Max(CAPABILITIES.len() as u16 + 6 + error_height),
        Constraint::Fill(1),
    ])
    .areas(f.area());

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Percentage(50),
        Constraint::Fill(1),
    ])
    .areas(center);

    let [title_area, list_area, error_area, help_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(error_height),
        Constraint::Length(2),
    ])
    .areas(center);

    f.render_widget(
        Paragraph::new(" Select Capabilities ".bold().cyan()).centered(),
        title_area,
    );

    let items = CAPABILITIES
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let check = if selected[i] { "[x]" } else { "[ ]" };
            ListItem::new(format!("  {} {}", check, name))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::bordered())
        .highlight_style(Style::new().yellow().bold());

    f.render_stateful_widget(list, list_area, list_state);

    if let Some(err) = error {
        f.render_widget(
            Paragraph::new(err.red().bold())
                .wrap(Wrap { trim: false })
                .centered(),
            error_area,
        );
    }

    f.render_widget(
        Paragraph::new(Line::from(vec![
            "Space".green(),
            ": toggle  |  ".into(),
            "Enter".green(),
            ": connect  |  ".into(),
            "q".green(),
            ": quit".into(),
        ]))
        .centered(),
        help_area,
    );
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
