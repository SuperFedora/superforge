// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::catalog::ForgeState;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, Clear, Gauge, List, ListItem, Paragraph},
};
use std::io;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

pub fn run_tui() -> i32 {
    let mut st = ForgeState::load();
    if enable_raw_mode().is_err() {
        return 1;
    }
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnterAlternateScreen);
    let Ok(mut terminal) = Terminal::new(CrosstermBackend::new(stdout)) else {
        let _ = disable_raw_mode();
        return 1;
    };

    let mut sel = 0usize;
    let mut webapp_input: Option<String> = None;
    let mut status_msg = format!("SuperForge v0.1.0 Ready (-march={}, {} threads, linker={})", st.hw.march_target, st.hw.cpu_jobs, st.hw.linker);
    let busy = Arc::new(AtomicBool::new(false));
    let mut last_tick = Instant::now();
    let mut tick_count = 0u64;

    loop {
        let _ = terminal.draw(|f| render_ui(f, &st, sel, webapp_input.as_deref(), &status_msg));

        let timeout = Duration::from_millis(250).saturating_sub(last_tick.elapsed());
        if event::poll(timeout).unwrap_or(false)
            && let Ok(Event::Key(key)) = event::read()
            && key.kind == KeyEventKind::Press
        {
            if let Some(ref mut buf) = webapp_input {
                match key.code {
                    KeyCode::Esc => webapp_input = None,
                    KeyCode::Backspace => { buf.pop(); }
                    KeyCode::Enter => {
                        let url = buf.trim().to_string();
                        webapp_input = None;
                        if !url.is_empty() {
                            status_msg = format!("Installing WebApp: {url}");
                            spawn_bg(&st, &busy, vec!["webapp".into(), url]);
                        }
                    }
                    KeyCode::Char(c) => buf.push(c),
                    _ => {}
                }
                continue;
            }

            let total = (st.items.len() + st.webapps.len()).max(1);
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1) % total,
                KeyCode::Up | KeyCode::Char('k') => sel = (sel + total - 1) % total,
                KeyCode::Char('u') => {
                    status_msg = "Running 4-stage SuperForge update...".into();
                    spawn_bg(&st, &busy, vec!["update".into()]);
                }
                KeyCode::Char('w') => webapp_input = Some(String::new()),
                KeyCode::Enter | KeyCode::Char('i') => {
                    if sel < st.items.len() {
                        let name = st.items[sel].name.clone();
                        status_msg = format!("Forging {name} (-march=native)...");
                        spawn_bg(&st, &busy, vec![name]);
                    } else if let Some((aid, _, _, _)) = st.webapps.get(sel - st.items.len()) {
                        let _ = Command::new(aid).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
                    }
                }
                KeyCode::Char('x') | KeyCode::Delete => {
                    let target = if sel < st.items.len() {
                        st.items[sel].name.clone()
                    } else if let Some((aid, _, _, _)) = st.webapps.get(sel - st.items.len()) {
                        aid.clone()
                    } else {
                        continue;
                    };
                    status_msg = format!("Removing {target}...");
                    spawn_bg(&st, &busy, vec!["remove".into(), target]);
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= Duration::from_millis(250) {
            tick_count += 1;
            st.refresh_fast();
            if tick_count.is_multiple_of(4) {
                st.refresh();
            }
            last_tick = Instant::now();
        }
    }

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    0
}

fn spawn_bg(st: &ForgeState, busy: &Arc<AtomicBool>, args: Vec<String>) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let runner = st.forge_root.join("lib/forge-runner.sh");
    let root = st.forge_root.clone();
    let flag = Arc::clone(busy);
    thread::spawn(move || {
        let _ = Command::new(runner).env("FORGE_ROOT", root).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
        flag.store(false, Ordering::SeqCst);
    });
}

fn render_ui(f: &mut Frame, st: &ForgeState, selected: usize, webapp_prompt: Option<&str>, status_msg: &str) {
    let pri = st.primary_color();
    let sec = st.secondary_color();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(12), Constraint::Length(3)])
        .split(f.area());

    let (pct, step_lbl, g_col) = if st.live.active {
        (st.live.pct.min(100), format!("ACTIVE [{}]: {} ({}%)", st.live.target, st.live.step, st.live.pct), Color::Yellow)
    } else {
        let forged_cnt = st.items.iter().filter(|i| i.installed).count();
        let total_cnt = st.items.len().max(1);
        let p = ((forged_cnt * 100) / total_cnt).min(100) as u16;
        (p, format!("IDLE — {forged_cnt}/{total_cnt} Native Packages Forged (-march={}) | {} Firefox WebApps  [Press 'u' for 4-Stage Update]", st.hw.march_target, st.webapps.len()), pri)
    };

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(g_col)).title(Span::styled(" SUPERFORGE 4-STAGE PIPELINE ([1/4] DNF > [2/4] GIT > [3/4] LTO COMPILE > [4/4] WEBAPPS) ", Style::default().fg(g_col).add_modifier(Modifier::BOLD))))
        .gauge_style(Style::default().fg(g_col).bg(Color::DarkGray))
        .percent(pct)
        .label(Span::styled(step_lbl, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
    f.render_widget(gauge, rows[0]);

    let cols = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[1]);
    let mut items: Vec<ListItem> = st
        .items
        .iter()
        .enumerate()
        .map(|(idx, it)| {
            let is_sel = idx == selected;
            let badge = if it.installed { "[FORGED]" } else { "[AVAIL ]" };
            let b_col = if it.installed { Color::Green } else { Color::DarkGray };
            let sz = if it.size_kb > 0 { format!("{:>5} KB", it.size_kb) } else { "    --  ".into() };
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { " > " } else { "   " }, Style::default().fg(pri).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{badge:<9}"), Style::default().fg(b_col).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<13}", it.name), Style::default().fg(if is_sel { Color::Black } else { Color::White }).bg(if is_sel { pri } else { Color::Reset }).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {:<6} ", it.kind), Style::default().fg(sec)),
                Span::styled(format!("{sz}  "), Style::default().fg(Color::Cyan)),
                Span::styled(&it.desc, Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let base = st.items.len();
    for (w_idx, (aid, name, _, url)) in st.webapps.iter().enumerate() {
        let is_sel = base + w_idx == selected;
        items.push(ListItem::new(Line::from(vec![
            Span::styled(if is_sel { " > " } else { "   " }, Style::default().fg(pri).add_modifier(Modifier::BOLD)),
            Span::styled("[WEBAPP] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<13}", name.chars().take(13).collect::<String>()), Style::default().fg(if is_sel { Color::Black } else { Color::White }).bg(if is_sel { pri } else { Color::Reset }).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {:<16} ", aid), Style::default().fg(sec)),
            Span::styled(url, Style::default().fg(Color::Gray)),
        ])));
    }

    let list = List::new(items).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(pri)).title(Span::styled(" NATIVE RECIPES & FIREFOX WEBAPPS (Enter=Build/Launch, w=Add WebApp, x=Remove) ", Style::default().fg(pri).add_modifier(Modifier::BOLD))));
    f.render_widget(list, cols[0]);

    let size_bars: Vec<(&str, u64)> = st.items.iter().map(|i| (i.name.as_str(), (i.size_kb as f64).sqrt() as u64)).collect();
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(sec)).title(Span::styled(" FORGED BINARY FOOTPRINT CHART (sqrt KB) ", Style::default().fg(sec).add_modifier(Modifier::BOLD))))
        .data(&size_bars)
        .bar_width(6)
        .bar_gap(2)
        .bar_style(Style::default().fg(sec))
        .value_style(Style::default().fg(Color::Black).bg(pri).add_modifier(Modifier::BOLD))
        .label_style(Style::default().fg(Color::White));
    f.render_widget(chart, cols[1]);

    let ftr = Paragraph::new(Line::from(vec![
        Span::styled(" KEYS ", Style::default().fg(Color::Black).bg(sec).add_modifier(Modifier::BOLD)),
        Span::raw(" [Enter/i] Build/Launch  [u] 4-Stage Update  [w] Add WebApp  [x] Remove  [q] Quit  | "),
        Span::styled(status_msg, Style::default().fg(pri).add_modifier(Modifier::BOLD)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(sec)));
    f.render_widget(ftr, rows[2]);

    if let Some(input) = webapp_prompt {
        let popup = Rect { x: f.area().width.saturating_sub(68) / 2, y: f.area().height / 2, width: 68.min(f.area().width), height: 5 };
        f.render_widget(Clear, popup);
        let blk = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).title(Span::styled(" INSTALL CHROMELESS FIREFOX WEBAPP (Enter URL or url|Name, Esc=Cancel) ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        f.render_widget(Paragraph::new(format!(" > {input}_")).block(blk), popup);
    }
}
