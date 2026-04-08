//! Plain terminal output and the ratatui TUI.

use std::{io, time::{Duration, Instant}};

use anyhow::{Context, Result};
use arboard::Clipboard;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::Title, Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};

use crate::{classify::{ContextKind, ErrorBlock, Severity}, palette};

pub fn render_plain(blocks: &[ErrorBlock], show_warnings: bool) {
    let n_errors   = blocks.iter().filter(|b| b.severity.is_error()).count();
    let n_warnings = blocks.iter().filter(|b| b.severity == Severity::Warning).count();

    let shown: Vec<_> = blocks.iter().filter(|b| {
        b.severity.is_error() || (show_warnings && !b.severity.is_error())
    }).collect();

    if shown.is_empty() {
        if n_warnings == 0 {
            println!("bx: no errors or warnings found.");
        } else {
            println!("bx: no errors. {} warning(s) — pass --warnings to show them.", n_warnings);
        }
        return;
    }

    let divider = "─".repeat(80);
    println!("{}", divider);
    println!("bx — {} error(s), {} warning(s)", n_errors, n_warnings);
    println!("{}", divider);

    for block in &shown {
        println!(" [{}]  {}", block.severity.label(), block.trigger.trim_end());
        for (kind, line) in &block.context {
            let prefix = match kind {
                ContextKind::Note    => "  >> ",
                ContextKind::Context => "     ",
            };
            println!("{}{}", prefix, line.trim_end());
        }
        println!();
    }

    println!("{}", divider);
    print!("{} error(s)", n_errors);
    if n_warnings > 0 {
        let suffix = if show_warnings { "" } else { " (--warnings to show)" };
        print!("  ·  {} warning(s){}", n_warnings, suffix);
    }
    println!();
}

#[derive(PartialEq, Eq)]
enum Focus { List, Detail }

struct App {
    blocks:        Vec<ErrorBlock>,
    list_state:    ListState,
    detail_scroll: u16,
    focus:         Focus,
    last_g:        Option<Instant>,
    notify:        Option<String>,
    clipboard:     Option<Clipboard>,
}

impl App {
    fn new(blocks: Vec<ErrorBlock>) -> Self {
        let mut list_state = ListState::default();
        if !blocks.is_empty() {
            list_state.select(Some(0));
        }
        // Clipboard is initialized once here — on some platforms (X11) it
        // spawns a background thread, so doing it per-keypress would be wrong.
        Self {
            blocks,
            list_state,
            detail_scroll: 0,
            focus: Focus::List,
            last_g: None,
            notify: None,
            clipboard: Clipboard::new().ok(),
        }
    }

    fn selected(&self) -> usize { self.list_state.selected().unwrap_or(0) }

    fn move_down(&mut self) {
        let n = self.blocks.len();
        if n == 0 { return; }
        self.list_state.select(Some((self.selected() + 1).min(n - 1)));
        self.detail_scroll = 0;
    }

    fn move_up(&mut self) {
        if self.blocks.is_empty() { return; }
        self.list_state.select(Some(self.selected().saturating_sub(1)));
        self.detail_scroll = 0;
    }

    fn jump_first(&mut self) {
        if !self.blocks.is_empty() {
            self.list_state.select(Some(0));
            self.detail_scroll = 0;
        }
    }

    fn jump_last(&mut self) {
        if !self.blocks.is_empty() {
            self.list_state.select(Some(self.blocks.len() - 1));
            self.detail_scroll = 0;
        }
    }

    fn yank(&mut self) {
        let text = self.blocks[self.selected()].full_text();
        if let Some(cb) = &mut self.clipboard {
            match cb.set_text(text) {
                Ok(_)  => self.notify = Some(" yanked ".into()),
                Err(e) => self.notify = Some(format!(" yank failed: {} ", e)),
            }
        } else {
            self.notify = Some(" clipboard unavailable ".into());
        }
    }
}

pub fn render_tui(blocks: Vec<ErrorBlock>) -> Result<()> {
    if blocks.is_empty() {
        println!("bx: no errors found.");
        return Ok(());
    }

    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;

    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend).context("failed to create terminal")?;
    let mut app  = App::new(blocks);
    let result   = run_loop(&mut term, &mut app);

    // Restore terminal regardless of whether the loop errored
    disable_raw_mode().ok();
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();
    term.show_cursor().ok();

    result
}

fn run_loop<B: ratatui::backend::Backend>(term: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        term.draw(|f| draw(f, app)).context("draw failed")?;

        if !event::poll(Duration::from_millis(50)).context("event poll failed")? {
            continue;
        }

        let ev = event::read().context("event read failed")?;

        if let Event::Key(key) = ev {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(());
            }

            app.notify = None;

            match app.focus {
                Focus::List => match key.code {
                    KeyCode::Char('q')                 => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => { app.move_down(); app.last_g = None; }
                    KeyCode::Char('k') | KeyCode::Up   => { app.move_up();   app.last_g = None; }
                    KeyCode::Char('G')                 => { app.jump_last(); app.last_g = None; }
                    KeyCode::Char('g') => {
                        let double_g = app.last_g
                            .map_or(false, |t| t.elapsed() < Duration::from_millis(500));
                        if double_g { app.jump_first(); app.last_g = None; }
                        else        { app.last_g = Some(Instant::now()); }
                    }
                    KeyCode::Char('y') => { app.yank(); app.last_g = None; }
                    KeyCode::Enter     => { app.focus = Focus::Detail; app.last_g = None; }
                    _                  => { app.last_g = None; }
                },
                Focus::Detail => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => { app.focus = Focus::List; app.detail_scroll = 0; }
                    KeyCode::Char('j') | KeyCode::Down => { app.detail_scroll = app.detail_scroll.saturating_add(3); }
                    KeyCode::Char('k') | KeyCode::Up   => { app.detail_scroll = app.detail_scroll.saturating_sub(3); }
                    KeyCode::Char('g') => { app.detail_scroll = 0; }
                    KeyCode::Char('G') => { app.detail_scroll = app.detail_scroll.saturating_add(999); }
                    _ => {}
                },
            }
        }
    }
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(f.size());

    let list_border_color = if app.focus == Focus::List { palette::SLATE } else { palette::DIM };
    let selected_idx = app.selected();
    let n = app.blocks.len();

    let items: Vec<ListItem> = app.blocks.iter().enumerate().map(|(i, b)| {
        let marker = if i == selected_idx { "▶ " } else { "  " };
        let style  = if i == selected_idx {
            Style::default().fg(b.severity.color()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(b.severity.color())
        };
        let text = format!(
            "{} [{}]  {}",
            marker,
            b.severity.label(),
            b.trigger.trim_end().chars().take(120).collect::<String>()
        );
        ListItem::new(Line::from(Span::styled(text, style)))
    }).collect();

    let status = format!(
        "  {}/{}   [j/k move  gg/G jump  y yank  Enter detail  q quit]",
        selected_idx + 1, n
    );

    let notify_span = match &app.notify {
        Some(msg) => Span::styled(msg.clone(), Style::default().fg(palette::SLATE).add_modifier(Modifier::BOLD)),
        None      => Span::raw(""),
    };

    let list_widget = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(list_border_color))
                .title(Span::styled(" bx ", Style::default().fg(palette::RUST).add_modifier(Modifier::BOLD)))
                .title(Title::from(notify_span).alignment(Alignment::Right))
                .title_bottom(Span::styled(status, Style::default().fg(palette::MUTED)))
        )
        .highlight_style(Style::default());

    f.render_stateful_widget(list_widget, chunks[0], &mut app.list_state);

    let detail_border_color = if app.focus == Focus::Detail { palette::SLATE } else { palette::DIM };
    let detail_title = if app.focus == Focus::Detail {
        " detail — hjkl scroll  Esc back "
    } else {
        " detail — Enter to focus "
    };

    let detail_lines = if app.blocks.is_empty() {
        vec![]
    } else {
        app.blocks[selected_idx].detail_lines()
    };

    let detail_widget = Paragraph::new(detail_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(detail_border_color))
                .title(Span::styled(detail_title, Style::default().fg(palette::MUTED)))
        )
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    f.render_widget(detail_widget, chunks[1]);
}
