// TUI: 字形墙（默认）+ `/` 过滤列表 + `?` 墙上查找 + `:` 命令行。
//
// 视图:
//   Wall  字形墙: 只铺 emoji / nerd 图标 / 画图用符号, h/j/k/l 移动
//   List  `/` 打开的结果列表: 输入即过滤(fzf 式), 回车跳到墙上的对应字形
//   Help  `:help` 打开的按键浮层
// 三种命令行前缀沿用 vim: `/` 过滤, `?` 查找(回车重复上一个模式), `:` 命令。

use std::io::{self, stdout};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthStr;

use crate::data::{self, Entry, Index};
use crate::search;

/// `:help` 浮层的三段说明。整体行数控制在 22 行内，
/// 24 行的终端（编辑器里常见的分屏高度）也放得下。
const HELP_KEYS: &[(&str, &str)] = &[
    ("h j k l", "move by one glyph / one row"),
    ("g / G", "first / last glyph"),
    ("PgUp/PgDn", "half a screen (Ctrl+U / Ctrl+D too)"),
    ("/", "search every codepoint: fzf style list"),
    ("?", "find: next glyph on the wall, empty Enter repeats"),
    (":", "command line (:help, :q)"),
    ("q", "quit"),
];

const HELP_LIST: &[(&str, &str)] = &[
    ("Up/Down", "move the selection while typing (Ctrl+N/P, PgUp/PgDn)"),
    ("Enter", "jump to it on the wall (off-wall codepoints stay here)"),
    ("Esc", "back to the wall, drops the filter"),
    ("Ctrl+U / W", "clear the line / delete the last word"),
];

/// 列表里字形列的特殊情况。
const HELP_LEGEND: &[(&str, &str)] = &[
    ("dim/dot", "control or noncharacter: shown as a control picture or a dot"),
    ("invisible", "zero width marks; the name column is the way to tell"),
    ("private use", "nerd font icons; needs a patched font"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Wall,
    List,
    Help,
}

/// 字形墙每个字形占的列数：最宽 2 列的字形（emoji）+ 至少 1 列间隔。
const WALL_PITCH: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    Filter,
    Find,
    Command,
}

impl PromptKind {
    fn sigil(self) -> char {
        match self {
            PromptKind::Filter => '/',
            PromptKind::Find => '?',
            PromptKind::Command => ':',
        }
    }

    fn label(self) -> &'static str {
        match self {
            PromptKind::Filter => "filter",
            PromptKind::Find => "find",
            PromptKind::Command => "command",
        }
    }
}

/// 命令行输入。Vec<char> 让光标就是一个索引，插入/删除最多几十个字符，够用。
struct Prompt {
    kind: PromptKind,
    chars: Vec<char>,
    cursor: usize,
}

impl Prompt {
    fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    fn delete_word(&mut self) {
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }
}

// --- 应用状态

struct App {
    index: Index,
    /// 字形墙收录的 entries 下标（emoji + nerd 图标 + 画图/游戏常用区段）。
    wall: Vec<usize>,
    /// entries 下标 -> 墙上的位置，列表里回车时用它回墙定位。
    wall_of_entry: Vec<u32>,
    wall_cursor: usize,
    wall_top: usize,
    /// 墙上一行放几个，随终端宽度变。
    wall_per_row: usize,
    view: View,
    /// Help 浮层关掉之后回到哪个视图。
    help_return: View,
    prompt: Option<Prompt>,
    /// 已提交的过滤词；`/` 用它预填，空过滤词表示显示全部候选。
    filter: String,
    /// 当前过滤结果（entries 下标）。
    hits: Vec<usize>,
    list_cursor: usize,
    list_top: usize,
    /// 上一次 `?` 的模式；`?` 后直接回车就是重复查找。
    find: String,
    status: String,
    visible_rows: usize,
    quit: bool,
}

impl App {
    fn new(index: Index) -> Self {
        let wall = index.wall();
        let mut wall_of_entry = vec![u32::MAX; index.entries.len()];
        for (pos, &idx) in wall.iter().enumerate() {
            wall_of_entry[idx] = pos as u32;
        }

        Self {
            index,
            wall,
            wall_of_entry,
            wall_cursor: 0,
            wall_top: 0,
            wall_per_row: 1,
            view: View::Wall,
            help_return: View::Wall,
            prompt: None,
            filter: String::new(),
            hits: Vec::new(),
            list_cursor: 0,
            list_top: 0,
            find: String::new(),
            status: String::new(),
            visible_rows: 0,
            quit: false,
        }
    }

    fn apply_filter(&mut self) {
        self.hits = search::hits(&self.index, &self.filter);
        self.list_cursor = 0;
        self.list_top = 0;
    }

    fn fix_list_scroll(&mut self) {
        self.list_top = scroll_top(
            self.list_cursor,
            self.list_top,
            self.hits.len(),
            self.visible_rows,
        );
    }

    fn open_prompt(&mut self, kind: PromptKind) {
        let seed = match kind {
            PromptKind::Filter => self.filter.clone(),
            PromptKind::Find => self.find.clone(),
            PromptKind::Command => String::new(),
        };

        if kind == PromptKind::Filter {
            // 空过滤词 = 全部候选，所以按 / 立刻就能看到完整列表
            self.apply_filter();
            self.view = View::List;
        }

        let chars: Vec<char> = seed.chars().collect();
        self.prompt = Some(Prompt {
            kind,
            cursor: chars.len(),
            chars,
        });
    }

    fn cancel_prompt(&mut self) {
        let kind = self.prompt.take().map(|p| p.kind);
        if kind == Some(PromptKind::Filter) {
            self.filter.clear();
            self.hits.clear();
            self.list_cursor = 0;
            self.list_top = 0;
            self.view = View::Wall;
        }
    }

    fn submit_prompt(&mut self, kind: PromptKind, text: String) {
        match kind {
            PromptKind::Find => {
                // 空输入 = 重复上一次模式，等价于 vim 的 n
                let pattern = if text.is_empty() { self.find.clone() } else { text };
                if pattern.trim().is_empty() {
                    self.status = "find: empty pattern".into();
                    return;
                }
                self.find = pattern;
                self.find_in_wall();
            }
            PromptKind::Command => self.run_command(&text),
            // 过滤行不经过这里: 它自己吞 Enter 并处理列表选择
            PromptKind::Filter => {}
        }
    }

    fn run_command(&mut self, raw: &str) {
        let (name, _rest) = match raw.trim().split_once(char::is_whitespace) {
            Some((name, rest)) => (name, rest.trim()),
            None => (raw.trim(), ""),
        };

        match name {
            "help" | "h" | "?" => {
                self.help_return = self.view;
                self.view = View::Help;
            }
            "wall" => {
                self.view = View::Wall;
                self.help_return = View::Wall;
                self.status = "glyph wall".into();
            }
            "q" | "q!" | "quit" => self.quit = true,
            "" => {}
            other => self.status = format!("unknown command: :{other}"),
        }
    }

    /// 回车：字形在墙上就回墙定位并返回 true；不在墙上就留在列表里继续挑，返回 false。
    /// 调用方只在返回 true 时关闭过滤行，这样列表视图永远带着自己的输入行。
    fn commit_selection(&mut self) -> bool {
        let Some(&idx) = self.hits.get(self.list_cursor) else {
            return false;
        };
        let (cp, label) = {
            let entry = &self.index.entries[idx];
            (entry.cp, entry.label.clone())
        };

        // 墙里有这个字形就回墙上定位；不在墙上（CJK、其他文字等）就留在列表里，
        // 行本身已经给出码点和名字
        let wall_pos = self.wall_of_entry.get(idx).copied().unwrap_or(u32::MAX);
        if wall_pos != u32::MAX {
            self.wall_cursor = wall_pos as usize;
            self.view = View::Wall;
            self.status = format!("jumped to {label} U+{cp:04X}");
            true
        } else {
            self.status = format!("U+{cp:04X} {label} is not on the wall");
            false
        }
    }

    fn wall_move(&mut self, delta: isize) {
        if self.wall.is_empty() {
            return;
        }
        let last = self.wall.len() - 1;
        let target = self.wall_cursor as isize + delta;
        self.wall_cursor = target.clamp(0, last as isize) as usize;
    }

    fn fix_wall_scroll(&mut self) {
        let per_row = self.wall_per_row.max(1);
        let rows = self.wall.len().div_ceil(per_row);
        let cursor_row = self.wall_cursor / per_row;
        self.wall_top = scroll_top(cursor_row, self.wall_top, rows, self.visible_rows);
    }

    /// 在墙上找下一个匹配的字形（用与搜索同一套规范化文本），走到末尾回绕。
    fn find_in_wall(&mut self) {
        let pattern = self.find.trim().to_lowercase();
        if self.wall.is_empty() {
            self.status = "find: the wall is empty".into();
            return;
        }

        let start = self.wall_cursor + 1;
        let total = self.wall.len();
        let mut found = None;
        for step in 0..total {
            let pos = (start + step) % total;
            if search::find_ci(&self.index.entries[self.wall[pos]].search, &pattern).is_some() {
                found = Some(pos);
                break;
            }
        }

        match found {
            Some(pos) => {
                let entry = &self.index.entries[self.wall[pos]];
                let (label, cp) = (entry.label.clone(), entry.cp);
                let wrapped = pos < start;
                self.wall_cursor = pos;
                self.status = format!(
                    "find {}: {label} U+{cp:04X}{}",
                    self.find,
                    if wrapped { " (wrapped)" } else { "" }
                );
            }
            None => self.status = format!("find {}: not found", self.find),
        }
    }

    fn wall_key(&mut self, key: KeyEvent) {
        let per_row = self.wall_per_row.max(1);
        // 半屏翻页：整屏在大终端里一次能跳过五千多个字形（188x89 下），
        // 半个屏幕还能保留一半重叠，扫过去不容易看丢
        let page = wall_page_step(per_row, self.visible_rows);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('q') => self.quit = true,

            KeyCode::Char('h') | KeyCode::Left => self.wall_move(-1),
            KeyCode::Char('l') | KeyCode::Right => self.wall_move(1),
            KeyCode::Char('j') | KeyCode::Down => self.wall_move(per_row as isize),
            KeyCode::Char('k') | KeyCode::Up => self.wall_move(-(per_row as isize)),
            KeyCode::Char('g') | KeyCode::Home => self.wall_cursor = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.wall_cursor = self.wall.len().saturating_sub(1);
            }
            KeyCode::PageDown => self.wall_move(page),
            KeyCode::PageUp => self.wall_move(-page),
            KeyCode::Char('d') if ctrl => self.wall_move(page),
            KeyCode::Char('u') if ctrl => self.wall_move(-page),

            KeyCode::Char('/') => self.open_prompt(PromptKind::Filter),
            KeyCode::Char('?') => self.open_prompt(PromptKind::Find),
            KeyCode::Char(':') => self.open_prompt(PromptKind::Command),
            _ => {}
        }
    }

    fn list_move(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let last = self.hits.len() - 1;
        let target = self.list_cursor as isize + delta;
        self.list_cursor = target.clamp(0, last as isize) as usize;
    }

    // --- 按键

    fn on_key(&mut self, key: KeyEvent) {
        match self.prompt.as_ref().map(|p| p.kind) {
            // 过滤行同时也是列表的输入框: 字母进查询, 上下键/^N/^P 动选择
            Some(PromptKind::Filter) => self.filter_key(key),
            Some(kind) => self.prompt_key(kind, key),
            None => match self.view {
                View::Help => self.view = self.help_return,
                View::Wall => self.wall_key(key),
                // 列表视图永远带着过滤行，走不到这里
                View::List => {}
            },
        }
    }

    /// 过滤行: 编辑查询词, 选择用 Up/Down 或 Ctrl+N/P 移动（j/k 是查询字符）。
    fn filter_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.visible_rows.max(1) as isize;

        match key.code {
            KeyCode::Esc => self.cancel_prompt(),
            KeyCode::Enter => {
                if self.commit_selection() {
                    self.prompt = None;
                }
            }
            KeyCode::Up => self.list_move(-1),
            KeyCode::Down => self.list_move(1),
            KeyCode::PageUp => self.list_move(-page),
            KeyCode::PageDown => self.list_move(page),
            KeyCode::Char('n') if ctrl => self.list_move(1),
            KeyCode::Char('p') if ctrl => self.list_move(-1),
            other => self.prompt_edit(PromptKind::Filter, KeyEvent { code: other, ..key }),
        }
    }

    fn prompt_key(&mut self, kind: PromptKind, key: KeyEvent) {
        self.prompt_edit(kind, key);
        if self.prompt.is_none() {
            return;
        }

        match key.code {
            KeyCode::Esc => self.cancel_prompt(),
            KeyCode::Enter => {
                let text = self.prompt.as_ref().map(Prompt::text).unwrap_or_default();
                self.prompt = None;
                self.submit_prompt(kind, text);
            }
            _ => {}
        }
    }

    /// 纯文本编辑部分，过滤行与命令行共用。
    fn prompt_edit(&mut self, kind: PromptKind, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Backspace => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.backspace();
                }
                self.after_prompt_edit(kind);
            }
            KeyCode::Delete => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.delete();
                }
                self.after_prompt_edit(kind);
            }
            KeyCode::Left => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.cursor = prompt.cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.cursor = (prompt.cursor + 1).min(prompt.chars.len());
                }
            }
            KeyCode::Home => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.cursor = 0;
                }
            }
            KeyCode::End => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.cursor = prompt.chars.len();
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.chars.clear();
                    prompt.cursor = 0;
                }
                self.after_prompt_edit(kind);
            }
            KeyCode::Char('w') if ctrl => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.delete_word();
                }
                self.after_prompt_edit(kind);
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.insert(c);
                }
                self.after_prompt_edit(kind);
            }
            _ => {}
        }
    }

    /// 过滤词是实时的：每敲一个字符就重算候选。
    fn after_prompt_edit(&mut self, kind: PromptKind) {
        if kind == PromptKind::Filter {
            self.filter = self.prompt.as_ref().map(Prompt::text).unwrap_or_default();
            self.apply_filter();
        }
    }

}

/// 墙上翻页的步长：半个屏幕，至少一个字形。
fn wall_page_step(per_row: usize, visible_rows: usize) -> isize {
    ((per_row.max(1) * visible_rows.max(1)) / 2).max(1) as isize
}

/// 让 list_cursor 始终落在 [top, top+height) 里；返回修正后的 top。
fn scroll_top(cursor: usize, top: usize, len: usize, height: usize) -> usize {
    if len == 0 || height == 0 {
        return 0;
    }
    let mut top = top;
    if cursor < top {
        top = cursor;
    } else if cursor >= top + height {
        top = cursor + 1 - height;
    }
    if top + height > len {
        top = len.saturating_sub(height);
    }
    top
}

// --- 渲染

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    app.visible_rows = chunks[1].height as usize;
    app.wall_per_row = (chunks[1].width as usize / WALL_PITCH).max(1);
    app.fix_list_scroll();
    app.fix_wall_scroll();

    render_status(frame, app, chunks[0]);

    // Help 浮层盖在进入前的视图上，不额外切换布局
    let base = match app.view {
        View::Help => app.help_return,
        view => view,
    };
    match base {
        View::List => render_list(frame, app, chunks[1]),
        _ => render_wall(frame, app, chunks[1]),
    }

    render_bottom(frame, app, chunks[2]);

    if app.view == View::Help {
        render_help(frame, area);
    }
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();

    // Help 浮层盖在底层视图上，状态行也跟着显示底层视图的信息
    let base = match app.view {
        View::Help => app.help_return,
        view => view,
    };

    match base {
        // base 已经把 Help 折成 help_return，这里只是让 match 完备
        View::Help => {}
        View::Wall => {
            let per_row = app.wall_per_row.max(1);
            let total = app.wall.len();
            spans.push(Span::styled(
                format!("wall {}/{}", app.wall_cursor + 1, total),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(format!(
                "  row {}/{}",
                app.wall_cursor / per_row + 1,
                total.div_ceil(per_row)
            )));

            if let Some(&idx) = app.wall.get(app.wall_cursor) {
                let entry = &app.index.entries[idx];
                spans.push(Span::raw("  |  "));
                spans.push(Span::styled(
                    format!("U+{:04X}", entry.cp),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::raw(format!("  '{}'", entry.glyph)));
                spans.push(Span::styled(
                    format!("  {}", entry.label),
                    Style::default().fg(Color::Green),
                ));
                spans.push(Span::styled(
                    format!("  [{}]", entry.source.tag()),
                    Style::default().fg(Color::Magenta),
                ));
            }
        }
        View::List => {
            spans.push(Span::styled(
                format!("filter /{}", app.filter),
                Style::default().fg(Color::Cyan),
            ));
            spans.push(Span::raw(format!("  {} hits", app.hits.len())));
            if !app.hits.is_empty() {
                spans.push(Span::raw(format!(
                    "  sel {}/{}",
                    app.list_cursor + 1,
                    app.hits.len()
                )));
            }
            spans.push(Span::raw("  |  Enter=jump  Esc=back"));
        }
    }

    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("  |  {}", app.status),
            Style::default().fg(Color::DarkGray),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_list(frame: &mut Frame, app: &App, area: Rect) {
    if app.hits.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no match",
                Style::default().fg(Color::DarkGray),
            ))),
            area,
        );
        return;
    }

    let width = area.width as usize;
    let mut lines = Vec::with_capacity(area.height as usize);

    for row in 0..area.height as usize {
        let Some(&idx) = app.hits.get(app.list_top + row) else {
            break;
        };
        let selected = app.list_top + row == app.list_cursor;
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            list_row(&app.index.entries[idx], selected, width),
            style,
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// 字形墙：定宽列距铺开（emoji 占 2 列，其他 1 列），选中格反白。
/// 墙上全是能直接显示的字形，不需要处理控制符和未分配码点（那些只在列表里出现）。
fn render_wall(frame: &mut Frame, app: &App, area: Rect) {
    let per_row = app.wall_per_row.max(1);
    let mut lines = Vec::with_capacity(area.height as usize);

    for row in app.wall_top..app.wall_top + area.height as usize {
        let start = row * per_row;
        if start >= app.wall.len() {
            break;
        }
        let end = (start + per_row).min(app.wall.len());
        let mut spans = Vec::with_capacity((end - start) * 2);

        for pos in start..end {
            let glyph = app.index.entries[app.wall[pos]].glyph.as_str();
            let style = if pos == app.wall_cursor {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            spans.push(Span::styled(glyph, style));

            // 补到固定列距，行才对齐；emoji 宽 2 只需补 1 列
            let pad = WALL_PITCH.saturating_sub(UnicodeWidthStr::width(glyph));
            if pad > 0 {
                spans.push(Span::styled(" ".repeat(pad), style));
            }
        }

        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// 一行: 标记 字形 码点 名字 [来源] 提示。
fn list_row(entry: &Entry, selected: bool, width: usize) -> String {
    let mut text = format!(
        "{} {} U+{:04X}  {}  [{}]",
        if selected { '>' } else { ' ' },
        data::glyph_cell(&entry.glyph),
        entry.cp,
        entry.label,
        entry.source.tag()
    );
    if !entry.hint.is_empty() {
        text.push_str("  ");
        text.push_str(&entry.hint);
    }
    truncate_width(&text, width)
}

/// 按显示宽度截断：超宽的行会被终端折行，把整个布局挤歪。
/// 用 unicode-width 的字符串宽度（emoji ZWJ 序列按一个整体算），
/// 不能逐字符相加 —— 那样会把 ZWJ 序列算成 6 列，比终端实际占位宽。
fn truncate_width(text: &str, max: usize) -> String {
    if UnicodeWidthStr::width(text) <= max {
        return text.to_string();
    }

    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        let next = index + ch.len_utf8();
        if UnicodeWidthStr::width(&text[..next]) > max {
            break;
        }
        end = next;
    }
    text[..end].to_string()
}

fn render_bottom(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(prompt) = &app.prompt {
        let before: String = prompt.chars[..prompt.cursor].iter().collect();
        let after: String = prompt.chars[prompt.cursor.min(prompt.chars.len())..]
            .iter()
            .skip(1)
            .collect();
        let cursor_char = prompt.chars.get(prompt.cursor).copied().unwrap_or(' ');

        let spans = vec![
            Span::styled(
                prompt.kind.sigil().to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(before),
            Span::styled(
                cursor_char.to_string(),
                Style::default().add_modifier(Modifier::REVERSED),
            ),
            Span::raw(after),
            Span::styled(
                format!("   ({})", prompt.kind.label()),
                Style::default().fg(Color::DarkGray),
            ),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    let base = match app.view {
        View::Help => app.help_return,
        view => view,
    };

    let pairs: &[(&str, &str)] = match base {
        View::Wall => &[
            ("h/j/k/l", "move"),
            ("g/G", "first/last"),
            ("PgUp/Dn", "page"),
            ("/", "search"),
            ("?", "find"),
            (":", "cmd"),
        ],
        View::List => &[
            ("Up/Down", "move"),
            ("Enter", "jump"),
            ("Esc", "back"),
            (":", "cmd"),
        ],
        _ => &[],
    };

    let mut spans = Vec::new();
    for (keys, desc) in pairs {
        spans.push(Span::styled(
            format!(" {keys} "),
            Style::default().bg(Color::DarkGray).fg(Color::White),
        ));
        spans.push(Span::raw(format!(" {desc}  ")));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    for (title, table) in [
        ("keys", HELP_KEYS),
        ("list and prompt (/ result)", HELP_LIST),
        ("marks", HELP_LEGEND),
    ] {
        lines.push(Line::from(Span::styled(
            format!("{title}:"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for (keys, desc) in table {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {keys:<13}"),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(*desc),
            ]));
        }
    }
    lines.push(Line::from(Span::styled(
        "any key closes this help",
        Style::default().fg(Color::DarkGray),
    )));

    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 66.min(area.width.saturating_sub(2));
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" keys ")
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

// --- 事件循环

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| ui(frame, app))?;

        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key),
            _ => {}
        }
    }
    Ok(())
}

/// 进入 TUI。数据在进终端前已经加载好，这里只做终端生命周期和主循环。
pub(crate) fn run(index: Index) -> io::Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(index);
    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> Index {
        use crate::data::{Entry, Source};
        let make = |glyph: &str, cp: u32, label: &str, source: Source| Entry {
            glyph: glyph.into(),
            cp,
            label: label.into(),
            label_key_len: label.len() as u32,
            hint: String::new(),
            search: label.to_lowercase(),
            source,
        };
        Index::new(vec![
                make("\u{1F602}", 0x1F602, ":joy:", Source::Emoji),
                make("\u{1F604}", 0x1F604, ":smile:", Source::Emoji),
                make("\u{1F923}", 0x1F923, ":rofl:", Source::Emoji),
                make("a", 0x41, "LATIN CAPITAL LETTER A", Source::Unicode),
                make("-", 0x2500, "BOX DRAWINGS LIGHT HORIZONTAL", Source::Unicode),
                make("\u{2801}", 0x2801, "BRAILLE PATTERN DOTS-1", Source::Unicode),
        ])
    }

    #[test]
    fn wall_cursor_moves_by_row_and_clamps() {
        let mut app = App::new(sample_index());
        // 墙上只有 emoji 与线框/braille，ASCII 不进墙
        assert_eq!(app.wall.len(), 5);
        app.wall_per_row = 2;
        app.wall_move(2);
        assert_eq!(app.wall_cursor, 2, "one row down");
        app.wall_move(-2);
        assert_eq!(app.wall_cursor, 0, "one row up");
        app.wall_move(-5);
        assert_eq!(app.wall_cursor, 0, "clamped at the start");
        app.wall_move(99);
        assert_eq!(app.wall_cursor, app.wall.len() - 1);
    }

    #[test]
    fn enter_lands_on_the_wall_or_stays_in_the_list() {
        let mut app = App::new(sample_index());

        // emoji 在墙上 -> 回墙并定位
        app.hits = vec![0];
        assert!(app.commit_selection(), "回墙时返回 true，调用方才会关掉过滤行");
        assert_eq!(app.view, View::Wall);
        assert_eq!(app.wall_cursor, 0);

        // ASCII 不在墙上 -> 留在列表，状态行说明它不在墙上
        let ascii = app
            .index
            .entries
            .iter()
            .position(|entry| entry.cp == 0x41)
            .expect("sample index contains ASCII");
        app.view = View::List;
        app.hits = vec![ascii];
        assert!(!app.commit_selection(), "不在墙上时留在列表里");
        assert_eq!(app.view, View::List);
        assert!(app.status.contains("not on the wall"), "{}", app.status);
    }

    #[test]
    fn page_step_is_half_a_screen() {
        // 188x89 的终端：per_row 62，可见 87 行 -> 一次 2697 个字形
        assert_eq!(wall_page_step(62, 87), 2697);
        assert_eq!(wall_page_step(26, 22), 286);
        // 退化情况不能变成 0（那样翻页就没反应了）
        assert_eq!(wall_page_step(0, 0), 1);
        assert_eq!(wall_page_step(1, 1), 1);
    }

    #[test]
    fn wall_scroll_keeps_the_cursor_row_visible() {
        let mut app = App::new(sample_index());
        app.wall_per_row = 3;
        app.visible_rows = 1;
        app.wall_cursor = app.wall.len() - 1;
        app.fix_wall_scroll();
        assert_eq!(app.wall_top, (app.wall.len() - 1) / 3);
    }

    #[test]
    fn scroll_keeps_cursor_visible() {
        // 光标在视口上方 -> 顶部跟着上移
        assert_eq!(scroll_top(3, 10, 100, 5), 3);
        // 光标越过视口底部 -> 只滚到刚好露出
        assert_eq!(scroll_top(10, 0, 100, 5), 6);
        // 接近末尾时不能滚过头
        assert_eq!(scroll_top(99, 90, 100, 5), 95);
        assert_eq!(scroll_top(0, 7, 3, 5), 0);
    }

    #[test]
    fn truncate_width_respects_wide_glyphs() {
        assert_eq!(truncate_width("abc", 2), "ab");
        assert_eq!(truncate_width("abc", 10), "abc");
        assert_eq!(truncate_width("\u{4E2D}\u{6587}", 3), "\u{4E2D}");
        assert_eq!(truncate_width("", 4), "");
    }

    #[test]
    fn truncate_width_treats_zwj_sequences_as_one_glyph() {
        // 家庭 emoji 是一个零宽连接序列，占 2 列；逐字符相加会算成 8 列
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(UnicodeWidthStr::width(family), 2);
        assert_eq!(truncate_width(family, 2), family);
        assert_eq!(truncate_width(family, 1), "");
        assert_eq!(truncate_width(&format!("{family}x"), 3), format!("{family}x"));
    }

    #[test]
    fn prompt_editing_follows_the_cursor() {
        let mut prompt = Prompt {
            kind: PromptKind::Command,
            chars: Vec::new(),
            cursor: 0,
        };
        for c in "help".chars() {
            prompt.insert(c);
        }
        prompt.cursor = 2;
        prompt.backspace();
        assert_eq!(prompt.text(), "hlp");
        prompt.cursor = 0;
        prompt.delete();
        assert_eq!(prompt.text(), "lp");

        let mut prompt = Prompt {
            kind: PromptKind::Filter,
            chars: "joy laugh".chars().collect(),
            cursor: 9,
        };
        prompt.delete_word();
        assert_eq!(prompt.text(), "joy ");
    }

    #[test]
    fn command_line_handles_help_and_quit() {
        let mut app = App::new(Index::new(Vec::new()));

        app.run_command("help");
        assert_eq!(app.view, View::Help);

        app.run_command("nope");
        assert!(app.status.contains("unknown command"), "{}", app.status);

        app.run_command("q");
        assert!(app.quit);
    }
}
