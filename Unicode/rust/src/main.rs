use std::io::{self, stdout};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthChar;

// ── constants ──────────────────────────────────────────────────

const COLS: usize = 32;
const ROWS: usize = 32;
const PAGE_SIZE: u32 = (COLS * ROWS) as u32; // 1024
const MAX_CP: u32 = 0x110000; // U+0000 ..= U+10FFFF
const TOTAL_PAGES: u32 = MAX_CP / PAGE_SIZE; // 1088 exactly

// ── display mode ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Show the actual glyph for each code point.
    Glyph,
    /// Show 4-digit hex code for each code point (two visual
    /// sub-rows per logical row so it fits an 80-column terminal).
    Hex,
}

// ── character classification ──────────────────────────────────

/// Returns `true` when the code point is a defined, display-worthy
/// Unicode scalar (i.e. not a control, surrogate, or noncharacter).
fn is_defined(cp: u32) -> bool {
    if cp <= 0x001F {
        return false;
    }
    if cp == 0x007F {
        return false;
    }
    if (0x0080..=0x009F).contains(&cp) {
        return false;
    }
    if (0xD800..=0xDFFF).contains(&cp) {
        return false;
    }
    if (0xFDD0..=0xFDEF).contains(&cp) {
        return false;
    }
    let lo = cp & 0xFFFF;
    if lo == 0xFFFE || lo == 0xFFFF {
        return false;
    }
    true
}

/// Human-readable glyph for a code point (may use control pictures).
fn glyph(cp: u32) -> String {
    if cp <= 0x1F {
        char::from_u32(0x2400 + cp).unwrap().to_string()
    } else if cp == 0x7F {
        '\u{2421}'.to_string()
    } else {
        char::from_u32(cp)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "·".into())
    }
}

/// Build a cell string in glyph mode.  Always appends a space so
/// cells never merge — even when the terminal renders a character
/// narrower than unicode-width predicts.
fn glyph_cell(cp: u32) -> String {
    if let Some(ch) = char::from_u32(cp) {
        if ch.width() == Some(0) {
            return format!("\u{25CC}{ch} ");
        }
    }
    glyph(cp) + " "
}

/// Build a fixed-width (5-column) hex cell: "XXXX "
fn hex_cell(cp: u32) -> String {
    format!("{cp:04X} ")
}

/// Build the UTF-8 hex bytes string for a code point.
fn utf8_hex(cp: u32) -> String {
    match char::from_u32(cp) {
        Some(ch) => {
            let mut buf = [0u8; 4];
            let enc = ch.encode_utf8(&mut buf);
            enc.as_bytes()
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        }
        None => "—".into(),
    }
}

// ── cache ──────────────────────────────────────────────────────

/// Pre-built per-page data so rendering never allocates.
struct PageCache {
    /// Fixed-width cell strings for current page (row-major, ROWS × COLS).
    cells: Vec<Vec<String>>,
    /// Precomputed is_defined for each cell.
    defined: Vec<Vec<bool>>,
}

impl PageCache {
    fn build(page: u32) -> Self {
        let page_start = page * PAGE_SIZE;
        let mut cells = Vec::with_capacity(ROWS);
        let mut defined = Vec::with_capacity(ROWS);

        for row in 0..ROWS {
            let mut cell_row = Vec::with_capacity(COLS);
            let mut def_row = Vec::with_capacity(COLS);
            for col in 0..COLS {
                let cp = page_start + (row * COLS + col) as u32;
                cell_row.push(glyph_cell(cp));
                def_row.push(is_defined(cp));
            }
            cells.push(cell_row);
            defined.push(def_row);
        }

        Self { cells, defined }
    }

    /// Reuse allocations — rebuild in-place.
    fn rebuild(&mut self, page: u32) {
        let page_start = page * PAGE_SIZE;
        for row in 0..ROWS {
            for col in 0..COLS {
                let cp = page_start + (row * COLS + col) as u32;
                self.cells[row][col] = glyph_cell(cp);
                self.defined[row][col] = is_defined(cp);
            }
        }
    }
}

// ── app state ─────────────────────────────────────────────────

struct App {
    page: u32,
    cursor_col: usize,
    cursor_row: usize,
    mode: Mode,
    /// Precomputed: which pages have at least one defined char.
    valid_pages: Vec<bool>,
    /// Precomputed cell data for the current page.
    cache: PageCache,
    /// Only rebuild cache when page changes.
    cache_page: u32,
}

impl App {
    fn new() -> Self {
        // One-time: compute which pages are worth visiting.
        let valid_pages: Vec<bool> = (0..TOTAL_PAGES).map(|p| {
            let start = p * PAGE_SIZE;
            (start..start + PAGE_SIZE).any(is_defined)
        }).collect();

        let start = if valid_pages[0] { 0 } else {
            (1..TOTAL_PAGES).find(|&p| valid_pages[p as usize]).unwrap_or(0)
        };

        Self {
            page: start,
            cursor_col: 0,
            cursor_row: 0,
            mode: Mode::Glyph,
            valid_pages,
            cache: PageCache::build(start),
            cache_page: start,
        }
    }

    fn cursor_cp(&self) -> u32 {
        self.page * PAGE_SIZE + (self.cursor_row * COLS + self.cursor_col) as u32
    }

    fn next_valid_page(&self) -> u32 {
        for i in 1..TOTAL_PAGES {
            let p = (self.page + i) % TOTAL_PAGES;
            if self.valid_pages[p as usize] {
                return p;
            }
        }
        self.page
    }

    fn prev_valid_page(&self) -> u32 {
        for i in 1..TOTAL_PAGES {
            let p = if self.page >= i {
                self.page - i
            } else {
                TOTAL_PAGES - (i - self.page)
            };
            if self.valid_pages[p as usize] {
                return p;
            }
        }
        self.page
    }

    /// Ensure cache is current for self.page.
    fn ensure_cache(&mut self) {
        if self.cache_page != self.page {
            self.cache.rebuild(self.page);
            self.cache_page = self.page;
        }
    }
}

// ── rendering ─────────────────────────────────────────────────

fn render_glyph_grid(app: &App) -> Vec<Line<'_>> {
    let mut lines = Vec::with_capacity(ROWS);

    for row in 0..ROWS {
        let cell_row = &app.cache.cells[row];
        let def_row = &app.cache.defined[row];
        let mut spans = Vec::with_capacity(COLS);

        for col in 0..COLS {
            let style = if row == app.cursor_row && col == app.cursor_col {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if !def_row[col] {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            spans.push(Span::styled(cell_row[col].as_str(), style));
        }
        lines.push(Line::from(spans));
    }

    lines
}

fn render_hex_grid(app: &App) -> Vec<Line<'_>> {
    let page_start = app.page * PAGE_SIZE;
    let mut lines = Vec::with_capacity(ROWS * 2);

    for row in 0..ROWS {
        for sub in 0..2 {
            let start_col = sub * 16;
            let def_row = &app.cache.defined[row];
            let mut spans = Vec::with_capacity(16);

            for (i, &is_def) in def_row[start_col..start_col + 16].iter().enumerate() {
                let col = start_col + i;
                let cp = page_start + (row * COLS + col) as u32;
                let hex = hex_cell(cp);

                let style = if row == app.cursor_row && col == app.cursor_col {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if !is_def {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                };

                spans.push(Span::styled(hex, style));
            }
            lines.push(Line::from(spans));
        }
    }

    lines
}

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    // ── status bar ──────────────────────────────────────────
    let page_start = app.page * PAGE_SIZE;
    let page_end = page_start + PAGE_SIZE - 1;
    let cp = app.cursor_cp();
    let ch = char::from_u32(cp);
    let char_str = ch.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
    let mode_label = match app.mode {
        Mode::Glyph => "glyph",
        Mode::Hex => "hex",
    };

    let status = Line::from(vec![
        Span::styled(
            format!("Page {}/{}", app.page, TOTAL_PAGES),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(format!(
            "  U+{page_start:06X}–U+{page_end:06X}"
        )),
        Span::raw("  │  "),
        Span::styled(
            format!("U+{cp:06X}"),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(format!("  '{char_str}'")),
        Span::raw(format!("  UTF-8: {}", utf8_hex(cp))),
        Span::raw("  │  "),
        Span::styled(
            format!("[{mode_label}]"),
            Style::default().fg(Color::Magenta),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(status).block(Block::default().borders(Borders::NONE)),
        layout[0],
    );

    // ── grid ────────────────────────────────────────────────
    let grid_lines = match app.mode {
        Mode::Glyph => render_glyph_grid(app),
        Mode::Hex => render_hex_grid(app),
    };

    let grid = Paragraph::new(grid_lines).block(Block::default().borders(Borders::NONE));
    frame.render_widget(grid, layout[1]);

    // ── help bar ────────────────────────────────────────────
    let help = Line::from(vec![
        Span::styled(" h/j/k/l ", Style::default().bg(Color::DarkGray).fg(Color::White)),
        Span::raw(" move  "),
        Span::styled(" n/N ", Style::default().bg(Color::DarkGray).fg(Color::White)),
        Span::raw(" next/prev page  "),
        Span::styled(" d ", Style::default().bg(Color::DarkGray).fg(Color::White)),
        Span::raw(" toggle hex/glyph  "),
        Span::styled(" q ", Style::default().bg(Color::DarkGray).fg(Color::White)),
        Span::raw(" quit"),
    ]);

    frame.render_widget(
        Paragraph::new(help).block(Block::default().borders(Borders::NONE)),
        layout[2],
    );
}

// ── event loop ────────────────────────────────────────────────

fn run() -> io::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut app = App::new();

    loop {
        app.ensure_cache();
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => break,

                KeyCode::Char('h') => {
                    app.cursor_col = app.cursor_col.saturating_sub(1);
                }
                KeyCode::Char('j') => {
                    app.cursor_row = (app.cursor_row + 1).min(ROWS - 1);
                }
                KeyCode::Char('k') => {
                    app.cursor_row = app.cursor_row.saturating_sub(1);
                }
                KeyCode::Char('l') => {
                    app.cursor_col = (app.cursor_col + 1).min(COLS - 1);
                }

                KeyCode::Char('n') => {
                    app.page = app.next_valid_page();
                }
                KeyCode::Char('N') => {
                    app.page = app.prev_valid_page();
                }

                KeyCode::Char('d') => {
                    app.mode = match app.mode {
                        Mode::Glyph => Mode::Hex,
                        Mode::Hex => Mode::Glyph,
                    };
                }

                _ => {}
            }
        }
    }

    Ok(())
}

// ── entry point ───────────────────────────────────────────────

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let result = run();

    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;

    result
}

// ── tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_defined() {
        assert!(!is_defined(0x0000));
        assert!(!is_defined(0x001F));
        assert!(!is_defined(0x007F));
        assert!(!is_defined(0x0080));
        assert!(!is_defined(0x009F));
        assert!(!is_defined(0xD800));
        assert!(!is_defined(0xDFFF));
        assert!(!is_defined(0xFDD0));
        assert!(!is_defined(0xFDEF));
        assert!(!is_defined(0x01FFFE));
        assert!(!is_defined(0x01FFFF));
        assert!(!is_defined(0x10FFFE));
        assert!(!is_defined(0x10FFFF));

        assert!(is_defined(0x0020));
        assert!(is_defined(0x0041));
        assert!(is_defined(0x00E9));
        assert!(is_defined(0x4E2D));
        assert!(is_defined(0x1F600));
    }

    #[test]
    fn test_glyph_controls() {
        assert_eq!(glyph(0x0000), "\u{2400}");
        assert_eq!(glyph(0x0001), "\u{2401}");
        assert_eq!(glyph(0x007F), "\u{2421}");
    }

    #[test]
    fn test_glyph_normal() {
        assert_eq!(glyph(0x0041), "A");
        assert_eq!(glyph(0x4E2D), "中");
    }

    #[test]
    fn test_glyph_invalid() {
        assert_eq!(glyph(0xD800), "·");
    }

    #[test]
    fn test_glyph_cell_narrow() {
        assert_eq!(glyph_cell(0x0041), "A ");
    }

    #[test]
    fn test_glyph_cell_wide() {
        // CJK chars also get trailing space (always padded).
        assert_eq!(glyph_cell(0x4E2D), "\u{4E2D} ");
    }

    #[test]
    fn test_glyph_cell_invalid() {
        assert_eq!(glyph_cell(0xD800), "· ");
    }

    #[test]
    fn test_glyph_cell_ambiguous() {
        assert_eq!(glyph_cell(0x25FD), "\u{25FD} ");
        assert_eq!(glyph_cell(0x2605), "\u{2605} ");
        assert_eq!(glyph_cell(0x2648), "\u{2648} ");
        // Control char U+0001 → control picture ␁, padded
        assert_eq!(glyph_cell(0x0001), "\u{2401} ");
    }

    #[test]
    fn test_utf8_hex() {
        assert_eq!(utf8_hex(0x0041), "41");
        assert_eq!(utf8_hex(0x00E9), "C3 A9");
        assert_eq!(utf8_hex(0x4E2D), "E4 B8 AD");
        assert_eq!(utf8_hex(0x1F600), "F0 9F 98 80");
        assert_eq!(utf8_hex(0xD800), "—");
    }

    #[test]
    fn test_page_count() {
        assert_eq!(MAX_CP % PAGE_SIZE, 0);
        assert_eq!(TOTAL_PAGES, 1088);
    }

    #[test]
    fn test_valid_pages() {
        let vp: Vec<bool> = (0..TOTAL_PAGES).map(|p| {
            let start = p * PAGE_SIZE;
            (start..start + PAGE_SIZE).any(is_defined)
        }).collect();

        // Page 0 has ASCII, so it's valid
        assert!(vp[0]);
        // Surrogate pages 54, 55 are empty
        assert!(!vp[54]);
        assert!(!vp[55]);
        // Last page (Private Use) is valid
        assert!(vp[1087]);
    }

    #[test]
    fn test_app_navigation() {
        let app = App::new();
        // From page 53, next skips surrogates
        let mut a = App {
            page: 53,
            cursor_col: 0,
            cursor_row: 0,
            mode: Mode::Glyph,
            valid_pages: app.valid_pages.clone(),
            cache: PageCache::build(53),
            cache_page: 53,
        };
        assert_eq!(a.next_valid_page(), 56);
        a.page = 56;
        assert_eq!(a.prev_valid_page(), 53);
    }
}

