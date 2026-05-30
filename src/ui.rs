use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, EditMode, InputMode, View};
use crate::header;

// Phosphor-green palette
const GREEN_DIM: Color = Color::Rgb(0, 128, 100);
const GREEN_BRIGHT: Color = Color::Rgb(0, 255, 200);
const CYAN: Color = Color::Rgb(0, 200, 200);
const YELLOW: Color = Color::Rgb(255, 230, 0);
const DIM: Color = Color::Rgb(60, 60, 60);
const BG: Color = Color::Rgb(8, 8, 8);
const HEADER_BG: Color = Color::Rgb(15, 30, 15);
const STATUS_BG: Color = Color::Rgb(20, 40, 20);

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Layout: info bar (3) | main content (rest) | status bar (3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // file info bar
            Constraint::Min(5),    // main content
            Constraint::Length(3), // status bar
        ])
        .split(size);

    draw_info_bar(f, app, chunks[0]);

    // Update visible lines based on actual terminal size
    app.visible_lines = chunks[1].height.saturating_sub(2) as usize;

    match app.view {
        View::Hex => draw_hex_view(f, app, chunks[1]),
        View::Info => draw_header_info(f, app, chunks[1]),
    }

    draw_status_bar(f, app, chunks[2]);
}

fn draw_info_bar(f: &mut Frame, app: &App, area: Rect) {
    let modified_indicator = if app.modified { " [MODIFIED]" } else { "" };
    let readonly_indicator = if app.editor.readonly { " [RO]" } else { "" };

    let sel_info = if let Some((lo, hi)) = app.selection_range() {
        format!("  Sel: 0x{:X}-0x{:X} ({} bytes)", lo, hi, hi - lo + 1)
    } else {
        String::new()
    };

    let info_text = format!(
        " {} | Size: {} | Offset: 0x{:08X} ({}) | Mode: {:?}{}{}{}",
        app.filename(),
        format_size(app.file_size()),
        app.cursor,
        app.cursor,
        app.edit_mode,
        modified_indicator,
        readonly_indicator,
        sel_info,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN_DIM))
        .style(Style::default().bg(HEADER_BG));

    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        info_text,
        Style::default().fg(GREEN_BRIGHT),
    )]))
    .block(block);

    f.render_widget(paragraph, area);
}

fn draw_hex_view(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN_DIM))
        .title(Span::styled(
            " Hex Forge ",
            Style::default()
                .fg(GREEN_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_lines = inner.height as usize;
    let selection = app.selection_range();

    let mut lines: Vec<Line> = Vec::with_capacity(visible_lines);

    for line_idx in 0..visible_lines {
        let line_num = app.scroll_offset + line_idx;
        let base_offset = line_num * 16;

        if base_offset >= app.file_size() {
            lines.push(Line::from(Span::styled("~", Style::default().fg(DIM))));
            continue;
        }

        let mut spans: Vec<Span> = Vec::new();

        // Offset column
        spans.push(Span::styled(
            format!("{:08X}  ", base_offset),
            Style::default().fg(GREEN_DIM),
        ));

        // Hex bytes
        for col in 0..16 {
            let offset = base_offset + col;
            if offset < app.file_size() {
                let byte = app.editor.read_byte(offset).unwrap_or(0);
                let is_cursor = offset == app.cursor;
                let is_modified = app.editor.is_modified(offset);
                let is_selected = selection
                    .map(|(lo, hi)| offset >= lo && offset <= hi)
                    .unwrap_or(false);
                let is_null = byte == 0;

                let mut style = if is_cursor && app.edit_mode == EditMode::Hex {
                    Style::default()
                        .fg(Color::Black)
                        .bg(GREEN_BRIGHT)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Black).bg(CYAN)
                } else if is_modified {
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
                } else if is_null {
                    Style::default().fg(DIM)
                } else {
                    Style::default().fg(GREEN_BRIGHT)
                };

                // Show pending nibble
                let hex_str = if let (true, Some(high)) = (is_cursor, app.hex_nibble) {
                    style = Style::default()
                        .fg(Color::Black)
                        .bg(YELLOW)
                        .add_modifier(Modifier::BOLD);
                    format!("{:X}_", high)
                } else {
                    format!("{:02X}", byte)
                };

                spans.push(Span::styled(hex_str, style));
            } else {
                spans.push(Span::styled("  ", Style::default().fg(DIM)));
            }

            // Separator between bytes
            if col == 7 {
                spans.push(Span::styled("  ", Style::default().fg(DIM)));
            } else {
                spans.push(Span::styled(" ", Style::default().fg(DIM)));
            }
        }

        // Separator
        spans.push(Span::styled(" |", Style::default().fg(GREEN_DIM)));

        // ASCII column
        for col in 0..16 {
            let offset = base_offset + col;
            if offset < app.file_size() {
                let byte = app.editor.read_byte(offset).unwrap_or(0);
                let is_cursor = offset == app.cursor;
                let is_modified = app.editor.is_modified(offset);
                let is_selected = selection
                    .map(|(lo, hi)| offset >= lo && offset <= hi)
                    .unwrap_or(false);

                let ch = if (0x20..0x7f).contains(&byte) {
                    byte as char
                } else {
                    '.'
                };

                let style = if is_cursor && app.edit_mode == EditMode::Ascii {
                    Style::default()
                        .fg(Color::Black)
                        .bg(CYAN)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::Black).bg(CYAN)
                } else if is_modified {
                    Style::default().fg(YELLOW)
                } else if byte == 0 {
                    Style::default().fg(DIM)
                } else {
                    Style::default().fg(CYAN)
                };

                spans.push(Span::styled(format!("{}", ch), style));
            } else {
                spans.push(Span::styled(" ", Style::default().fg(DIM)));
            }
        }

        spans.push(Span::styled("|", Style::default().fg(GREEN_DIM)));

        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn draw_header_info(f: &mut Frame, app: &App, area: Rect) {
    let info = header::parse_header(&app.editor);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN_DIM))
        .title(Span::styled(
            format!(" {} — Header Info (F1 to return) ", info.format_name),
            Style::default()
                .fg(GREEN_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("Format: {}", info.format_name),
        Style::default()
            .fg(GREEN_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for (key, value) in &info.fields {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<24} ", key),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(value.clone(), Style::default().fg(GREEN_BRIGHT)),
        ]));
    }

    // Scroll support for info view
    let total = lines.len();
    let visible = inner.height as usize;
    if app.info_scroll > total.saturating_sub(visible) {
        // clamp (though we don't have scroll input for info yet)
    }

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.info_scroll as u16, 0));
    f.render_widget(paragraph, inner);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN_DIM))
        .style(Style::default().bg(STATUS_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let status_line = match app.input_mode {
        InputMode::GotoOffset => {
            let msg = app.status_msg.as_deref().unwrap_or("Goto offset:");
            Line::from(vec![
                Span::styled(format!("{} ", msg), Style::default().fg(GREEN_BRIGHT)),
                Span::styled(
                    app.input_buf.clone(),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "_",
                    Style::default()
                        .fg(YELLOW)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        }
        InputMode::Search => {
            let msg = app.status_msg.as_deref().unwrap_or("Search:");
            Line::from(vec![
                Span::styled(format!("{} ", msg), Style::default().fg(GREEN_BRIGHT)),
                Span::styled(
                    app.input_buf.clone(),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "_",
                    Style::default()
                        .fg(YELLOW)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
            ])
        }
        InputMode::QuitConfirm => {
            let msg = app.status_msg.as_deref().unwrap_or("Quit? (Y/n)");
            Line::from(Span::styled(
                msg.to_string(),
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ))
        }
        InputMode::Normal => {
            if let Some(ref msg) = app.status_msg {
                Line::from(Span::styled(msg.clone(), Style::default().fg(GREEN_BRIGHT)))
            } else {
                let help = " Ctrl+Q:Quit  Ctrl+S:Save  Ctrl+G:Goto  Ctrl+F:Find  /:Search  n:Next  Tab:Mode  F1:Info  v:Select  y:Copy";
                Line::from(Span::styled(help, Style::default().fg(GREEN_DIM)))
            }
        }
    };

    let paragraph = Paragraph::new(status_line);
    f.render_widget(paragraph, inner);
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
