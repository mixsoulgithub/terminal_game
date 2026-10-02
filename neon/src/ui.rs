// 该文件把 Pager 的状态画到帧缓冲上.
// 策略:先用普通 Paragraph 写出字符,再逐单元格改颜色.
// 只改需要改的单元格,ratatui 的 Buffer diff 会把未变化的格子变成零输出字节.
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::doc::char_width;
use crate::neon::{Flow, BAR_TEXT, BG, HL_BG, HL_CUR_BG, HL_CUR_FG, HL_FG};
use crate::pager::Pager;

// 只用 ASCII 画框,霓虹感由颜色承担
const ASCII: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

// 假光标:不移动物理光标,自己在提示符位置画一个实心块.
// 动画每帧都在重绘,物理光标会跟着 diff 的落点乱跳,所以干脆不用它.
const CURSOR_BG: Color = Color::Rgb(238, 244, 255);
const CURSOR_FG: Color = Color::Rgb(8, 9, 16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    SearchFwd,
    SearchBack,
    Filter,
}

impl PromptKind {
    fn prefix(self) -> char {
        match self {
            PromptKind::SearchFwd => '/',
            PromptKind::SearchBack => '?',
            PromptKind::Filter => '&',
        }
    }
}

pub struct Prompt {
    pub text: String,
    pub kind: PromptKind,
}

const HELP: &[&str] = &[
    "  j / down     scroll one line        k / up       scroll one line up",
    "  space / f    scroll one page        b            scroll one page up",
    "  d / u        half page              pgdn / pgup  page",
    "  g / home     go to top              G / end      go to bottom",
    "  /            search forward         ?            search backward",
    "  n / N        next / previous match  esc          clear search / quit",
    "  m            toggle line wrap       l            toggle line numbers",
    "  a            toggle the color flow  r            tint every glyph",
    "  + / -        faster / slower        h            toggle this help",
    "  q            quit                   mouse wheel  scroll",
];

pub fn render(f: &mut Frame, p: &Pager, flow: &Flow, prompt: Option<&Prompt>) {
    let area = f.area();
    // 只留正文与底栏;没有任何信息画在顶行
    if area.width < 4 || area.height < 2 {
        return;
    }
    let body_area = Rect::new(area.x, area.y, area.width, area.height - 1);
    let status_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);

    // 全程不碰物理光标,ratatui 会在每帧结束后把它藏起来
    let buf = f.buffer_mut();
    buf.set_style(area, Style::default().bg(BG));
    render_body(buf, p, flow, body_area);
    render_status(buf, p, flow, prompt, status_area);
    if p.help {
        render_help(buf, area);
    }
}

// 正文:先出字符,再上色.空白格子不参与流动,否则每帧都会产生大量无用的重绘字节
fn render_body(buf: &mut Buffer, p: &Pager, flow: &Flow, area: Rect) {
    let view = p.view();
    if view.is_empty() {
        if area.height > 0 {
            let hint = if p.doc.len() == 0 { "(empty)" } else { "" };
            Paragraph::new(hint).render(area, buf);
            for x in 0..area.width {
                let cell = &mut buf[(area.x + x, area.y)];
                if !is_blank(cell) {
                    set_fg(cell, Color::Rgb(120, 128, 150));
                }
            }
        }
        return;
    }
    // 正文必须画在行号列右边,否则会被行号盖掉开头几个字符
    let text_x = area.x + p.text_x as u16;
    let text_w = area.width.saturating_sub(p.text_x as u16);
    let h = view.len() as u16;
    let lines: Vec<Line> = view.iter().map(|r| Line::raw(p.doc.text(*r))).collect();
    Paragraph::new(Text::from(lines)).render(
        Rect::new(text_x, area.y, text_w, area.height),
        buf,
    );

    if p.gutter > 0 {
        let gutter_w = (p.text_x as u16).saturating_sub(1);
        let garea = Rect::new(area.x, area.y, gutter_w, area.height);
        let mut nums = String::new();
        for (i, r) in view.iter().enumerate() {
            if i > 0 {
                nums.push('\n');
            }
            // 折行产生的续行不重复编号,和 less -N 一致
            if r.b0 == 0 {
                nums.push_str(&(r.line + 1).to_string());
            }
        }
        Paragraph::new(nums)
            .alignment(Alignment::Right)
            .render(garea, buf);
        for y in 0..h {
            for x in 0..gutter_w {
                let cell = &mut buf[(garea.x + x, garea.y + y)];
                if !is_blank(cell) {
                    set_fg(cell, flow.gutter_fg(y, h));
                }
            }
        }
    }

    for (y, row) in view.iter().enumerate() {
        let ranges = p.match_ranges(row);
        for x in 0..text_w {
            let cell = &mut buf[(text_x + x, area.y + y as u16)];
            let hit = ranges.iter().find(|(a, b, _)| x >= *a && x < *b);
            match hit {
                Some((_, _, true)) => set_cell(cell, HL_CUR_FG, Some(HL_CUR_BG)),
                Some((_, _, false)) => set_cell(cell, HL_FG, Some(HL_BG)),
                None => {
                    // 宽字符的续格没有字形,只保证底色正确
                    if is_blank(cell) {
                        if cell.bg != BG {
                            cell.set_bg(BG);
                        }
                        if cell.fg != Color::Reset {
                            cell.set_fg(Color::Reset);
                        }
                    } else {
                        set_fg(cell, flow.body_fg(x, y as u16, text_w, h, p.rainbow));
                    }
                }
            }
        }
    }
}

// 底栏:名字、过滤、提示消息走在左边,行数与进度、开关指示靠右;整行底色流动,
// 文字压深色;输入提示符的位置画一个实心块当光标
fn render_status(buf: &mut Buffer, p: &Pager, flow: &Flow, prompt: Option<&Prompt>, area: Rect) {
    let width = area.width as usize;
    let (text, caret) = match prompt {
        Some(pr) => {
            let s = format!(" {}{}", pr.kind.prefix(), pr.text);
            let caret = text_width(&s).min(width.saturating_sub(1)) as u16;
            (s, Some(caret))
        }
        None => {
            let mut left = format!(" {} ", p.doc.name);
            if let Some(f) = p.filter() {
                left.push_str(&format!("[{f}] "));
            }
            if !p.status.is_empty() {
                left.push_str(&format!(" {}  ", p.status));
            }
            let badges: String = [
                badge(p.wrap, 'm'),
                badge(p.word_wrap, 'w'),
                badge(p.numbers, 'l'),
                badge(p.rainbow, 'r'),
                badge(flow.enabled, 'a'),
                badge(p.follow, 'f'),
            ]
            .into_iter()
            .collect();
            let right = format!("{} lines  {:>3}%  {badges} ", p.doc.len(), p.percent());
            let (lw, rw) = (text_width(&left), text_width(&right));
            let text = if lw + rw <= width {
                format!("{left}{}{right}", " ".repeat(width - lw - rw))
            } else if rw < width {
                // 名字太长时先切名字,进度与开关始终留着
                format!("{}{right}", fit(&left, width - rw))
            } else {
                left
            };
            (text, None)
        }
    };
    let text = fit(&text, width);
    Paragraph::new(text).render(area, buf);
    for x in 0..area.width {
        let cell = &mut buf[(area.x + x, area.y)];
        set_cell(cell, bar_fg(), Some(flow.bar_bg(x, area.width)));
    }
    if let Some(x) = caret {
        let cell = &mut buf[(area.x + x, area.y)];
        set_cell(cell, CURSOR_FG, Some(CURSOR_BG));
    }
}

fn render_help(buf: &mut Buffer, area: Rect) {
    let w = (HELP.iter().map(|l| text_width(l)).max().unwrap_or(0) + 4) as u16;
    let h = (HELP.len() + 2) as u16;
    if w > area.width || h > area.height {
        return;
    }
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    Clear.render(rect, buf);
    buf.set_style(
        Rect::new(rect.x + 1, rect.y + 1, w - 2, h - 2),
        Style::default().bg(BG),
    );
    let body: Vec<Line> = HELP.iter().map(|l| Line::raw(*l)).collect();
    Paragraph::new(Text::from(body))
        .style(Style::default().fg(Color::Rgb(216, 222, 240)).bg(BG))
        .block(
            Block::bordered()
                .border_set(ASCII)
                .border_style(Style::default().fg(Color::Rgb(120, 230, 255)))
                .style(Style::default().bg(BG))
                .title(" keys "),
        )
        .render(rect, buf);
}

fn badge(on: bool, ch: char) -> char {
    if on {
        ch.to_ascii_uppercase()
    } else {
        '-'
    }
}

fn bar_fg() -> Color {
    Color::Rgb(BAR_TEXT.0, BAR_TEXT.1, BAR_TEXT.2)
}

fn is_blank(cell: &ratatui::buffer::Cell) -> bool {
    let s = cell.symbol();
    s.is_empty() || s == " "
}

fn set_fg(cell: &mut ratatui::buffer::Cell, fg: Color) {
    cell.set_style(Style::default().fg(fg));
}

// 同时写前景与背景,避免上一帧的高亮残留
fn set_cell(cell: &mut ratatui::buffer::Cell, fg: Color, bg: Option<Color>) {
    cell.set_style(Style::default().fg(fg).bg(bg.unwrap_or(Color::Reset)));
}

pub fn text_width(s: &str) -> usize {
    s.chars().map(|c| char_width(c).max(1)).sum()
}

// 按显示宽度截断并补齐到 w 列
fn fit(s: &str, w: usize) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut used = 0;
    for ch in s.chars() {
        let cw = char_width(ch).max(1);
        if used + cw > w {
            break;
        }
        out.push(ch);
        used += cw;
    }
    while used < w {
        out.push(' ');
        used += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::pager::{Options, Pager};
    use ratatui::backend::{Backend, TestBackend};
    use ratatui::Terminal;

    fn setup(text: &str, numbers: bool, w: u16, h: u16) -> (Pager, Flow) {
        let mut p = Pager::new(
            Doc::new("t".into(), text),
            Options {
                wrap: true,
                word_wrap: false,
                numbers,
                rainbow: false,
                follow: false,
            },
        );
        p.ensure_layout(w, h);
        (p, Flow::new(1.0))
    }

    // 直接断言真实画出来的缓冲,而不是终端字节流
    fn draw(p: &Pager, flow: &Flow, w: u16, h: u16) -> Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render(f, p, flow, None)).unwrap();
        term.backend().buffer().clone()
    }

    fn row(buf: &Buffer, y: u16, w: u16) -> String {
        (0..w)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn fit_truncates_by_display_width() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 3), "abc");
        // 中文占两格,不能只截一半
        assert_eq!(fit("霓虹灯", 5), "霓虹 ");
        assert_eq!(fit("霓虹灯", 6), "霓虹灯");
    }

    #[test]
    fn badge_marks_state() {
        assert_eq!(badge(true, 'w'), 'W');
        assert_eq!(badge(false, 'w'), '-');
    }

    #[test]
    fn the_bottom_bar_carries_all_the_info() {
        let (p, f) = setup("alpha\nbeta\n", true, 40, 5);
        let buf = draw(&p, &f, 40, 5);
        // 顶行就是正文本身,没有任何信息条
        assert_eq!(row(&buf, 0, 40), " 1 alpha");
        assert_eq!(row(&buf, 1, 40), " 2 beta");
        let bar = row(&buf, 4, 40);
        assert!(bar.starts_with(" t "), "got {bar:?}");
        assert!(bar.contains("2 lines"), "got {bar:?}");
        // 折行 / 按词折行 / 行号 / 彩虹 / 动画 / 跟随
        assert!(bar.ends_with("100%  M-L-A-"), "got {bar:?}");
    }

    #[test]
    fn a_long_name_is_cut_before_the_progress_and_badges() {
        let long = "x".repeat(60);
        let mut p = Pager::new(
            Doc::new(long.clone(), "body\n"),
            Options {
                wrap: true,
                word_wrap: false,
                numbers: false,
                rainbow: false,
                follow: false,
            },
        );
        p.ensure_layout(40, 4);
        let f = Flow::new(1.0);
        let buf = draw(&p, &f, 40, 4);
        let bar = row(&buf, 3, 40);
        assert!(bar.starts_with(" x"), "got {bar:?}");
        assert!(bar.contains("1 lines"), "got {bar:?}");
        assert!(bar.ends_with("100%  M---A-"), "got {bar:?}");
        assert!(!bar.contains(&long), "名字应当被截断: {bar:?}");
    }

    #[test]
    fn status_shows_the_active_filter() {
        let (mut p, f) = setup("keep me\ndrop me\n", false, 70, 5);
        p.set_filter("keep");
        let buf = draw(&p, &f, 70, 5);
        let bar = row(&buf, 4, 70);
        assert!(bar.contains("[keep]"), "got {bar:?}");
        assert!(bar.contains("1 of 2 lines match keep"), "got {bar:?}");
        assert!(bar.ends_with("100%  M---A-"), "got {bar:?}");
    }

    #[test]
    fn prompt_draws_a_block_cursor_without_touching_the_physical_one() {
        let (p, f) = setup("hello\n", false, 20, 5);
        let prompt = Prompt {
            text: "he".into(),
            kind: PromptKind::SearchFwd,
        };
        let mut term = Terminal::new(TestBackend::new(20, 5)).unwrap();
        term.hide_cursor().unwrap(); // 进入界面时就是这么隐藏的
        let before = term.backend_mut().get_cursor_position().unwrap();
        term.draw(|fr| render(fr, &p, &f, Some(&prompt))).unwrap();
        let buf = term.backend().buffer();
        // 提示符 " /he" 占 0..4 列,实心块落在第 4 列
        assert_eq!(buf[(1, 4)].symbol(), "/");
        assert_eq!(buf[(2, 4)].symbol(), "h");
        assert_eq!(buf[(3, 4)].symbol(), "e");
        assert_eq!(buf[(4, 4)].bg, CURSOR_BG);
        assert_eq!(buf[(4, 4)].fg, CURSOR_FG);
        assert_ne!(buf[(5, 4)].bg, CURSOR_BG);
        // 物理光标既没被显示,也没被挪到提示符处:这就是"光标乱飞"的根治办法
        assert!(!term.backend().cursor_visible());
        assert_eq!(term.backend_mut().get_cursor_position().unwrap(), before);
    }

    #[test]
    fn filter_prompt_uses_the_ampersand() {
        let (p, f) = setup("hello\n", false, 20, 5);
        let prompt = Prompt {
            text: "he".into(),
            kind: PromptKind::Filter,
        };
        let mut term = Terminal::new(TestBackend::new(20, 5)).unwrap();
        term.draw(|fr| render(fr, &p, &f, Some(&prompt))).unwrap();
        assert_eq!(term.backend().buffer()[(1, 4)].symbol(), "&");
    }

    #[test]
    fn text_starts_after_the_gutter() {
        let (p, f) = setup("alpha\nbeta\ngamma\n", true, 40, 5);
        let buf = draw(&p, &f, 40, 5);
        // 两位行号 + 一格间隔,正文从第 3 列开始,不会被行号盖掉
        assert_eq!(row(&buf, 0, 40), " 1 alpha");
        assert_eq!(row(&buf, 1, 40), " 2 beta");
        assert_eq!(row(&buf, 2, 40), " 3 gamma");
    }

    #[test]
    fn without_numbers_the_text_starts_at_column_zero() {
        let (p, f) = setup("alpha\n", false, 40, 5);
        let buf = draw(&p, &f, 40, 5);
        assert_eq!(row(&buf, 0, 40), "alpha");
    }

    #[test]
    fn wrapped_continuation_rows_are_not_numbered() {
        // 正文宽 11,首行折成两段
        let (p, f) = setup("0123456789abcdefghij\nlast\n", true, 14, 5);
        let buf = draw(&p, &f, 14, 5);
        assert_eq!(row(&buf, 0, 14), " 1 0123456789a");
        assert_eq!(row(&buf, 1, 14), "   bcdefghij");
        assert_eq!(row(&buf, 2, 14), " 2 last");
    }

    #[test]
    fn wide_chars_take_two_cells() {
        let (p, f) = setup("霓虹\n", true, 12, 5);
        let buf = draw(&p, &f, 12, 5);
        assert_eq!(buf[(3, 0)].symbol(), "霓");
        assert_eq!(buf[(5, 0)].symbol(), "虹");
        assert!(!is_blank(&buf[(3, 0)]));
        // 宽字符吃掉两格:第 4 格是它的第二格,第 6 格才是行尾空白
        assert!(is_blank(&buf[(4, 0)]));
        assert!(is_blank(&buf[(6, 0)]));
    }

    #[test]
    fn flow_changes_colors_but_never_glyphs() {
        let (p, mut f) = setup("hello\nworld\n", false, 40, 5);
        let a = draw(&p, &f, 40, 5);
        f.tick(0.5);
        let b = draw(&p, &f, 40, 5);
        let glyphs = |buf: &Buffer| (0..5u16).map(|y| row(buf, y, 40)).collect::<Vec<_>>();
        assert_eq!(glyphs(&a), glyphs(&b));
        // 底栏底色与正文光带都在流动
        let bar_bg = |buf: &Buffer| (0..40u16).map(|x| buf[(x, 4)].bg).collect::<Vec<_>>();
        assert_ne!(bar_bg(&a), bar_bg(&b));
        let body_fg = |buf: &Buffer| (0..40u16).map(|x| buf[(x, 0)].fg).collect::<Vec<_>>();
        assert_ne!(body_fg(&a), body_fg(&b));
    }

    #[test]
    fn search_highlights_the_matched_cells_only() {
        let (mut p, f) = setup("plain needle here\n", false, 40, 5);
        p.search("needle", false);
        let buf = draw(&p, &f, 40, 5);
        for x in 6..12 {
            assert_eq!(buf[(x, 0)].bg, HL_CUR_BG, "column {x} should be highlighted");
            assert_eq!(buf[(x, 0)].fg, HL_CUR_FG);
        }
        assert_eq!(buf[(5, 0)].bg, BG);
        assert_eq!(buf[(12, 0)].bg, BG);
    }

    #[test]
    fn empty_document_shows_a_placeholder() {
        let (p, f) = setup("", false, 40, 5);
        let buf = draw(&p, &f, 40, 5);
        assert!(row(&buf, 0, 40).starts_with("(empty)"));
    }

    #[test]
    fn tiny_terminal_renders_nothing_without_panicking() {
        let (p, f) = setup("a\n", false, 3, 2);
        let _ = draw(&p, &f, 3, 2);
    }

    #[test]
    fn help_box_fits_the_smallest_supported_terminal() {
        // 40x12 的终端放不下帮助框,此时应静默跳过而不是 panic
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 12));
        render_help(&mut buf, Rect::new(0, 0, 40, 12));
        let mut big = Buffer::empty(Rect::new(0, 0, 120, 40));
        render_help(&mut big, Rect::new(0, 0, 120, 40));
        // 帮助框画在中间,左上角应出现 ASCII 边框
        assert_eq!(big[(0, 0)].symbol(), " ");
        let any_border = (0..40).any(|y| (0..120).any(|x| big[(x, y)].symbol() == "+"));
        assert!(any_border);
    }
}
