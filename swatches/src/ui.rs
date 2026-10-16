// ui.rs:把状态画到帧缓冲上.
// 色块直接改 Buffer 单元格的底色,不走 Widget;只有帮助浮层用现成的 Paragraph 排版.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::palette::{ALL, SWATCH_H, SWATCH_W};
use crate::State;

// 只用 ASCII 画框,颜色交给色块
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

const HELP: &[&str] = &[
    "  h / l         move left / right  j / k       move up / down",
    "  arrows        same as hjkl       0 / $       start / end of row",
    "  g then g      top of column      G           bottom of column",
    "  y then y      copy the hex code  ?           toggle this help",
    "  tab / S-tab   next / previous palette",
    "  1 .. 5        pick a palette directly",
    "  q / esc       quit",
    "",
    "  1 hue/value   2 hue/sat   3 ansi16   4 ansi256   5 gray",
];

// 帮助框的内宽:上面每行都得短于它,否则右边会被切掉
const HELP_WIDTH: usize = 66;

pub struct Regions {
    pub header: Rect,
    pub body: Rect,
    pub info: Rect,
    pub status: Rect,
}

// 顶栏 1 行 + 底栏 1 行 + 信息栏 3 行,剩下全是色卡
pub fn regions(area: Rect) -> Option<Regions> {
    if area.width < 24 || area.height < 8 {
        return None;
    }
    Some(Regions {
        header: Rect::new(area.x, area.y, area.width, 1),
        body: Rect::new(area.x, area.y + 1, area.width, area.height - 5),
        info: Rect::new(area.x, area.y + area.height - 4, area.width, 3),
        status: Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    })
}

pub fn render(f: &mut Frame, st: &State) {
    let area = f.area();
    {
        let buf = f.buffer_mut();
        buf.set_style(area, Style::default());
        match regions(area) {
            Some(reg) => {
                draw_header(buf, st, reg.header);
                draw_grid(buf, st, reg.body);
                draw_info(buf, st, reg.info);
                draw_status(buf, st, reg.status);
            }
            None => {
                put(buf, area.x, area.y, "terminal too small", Style::default());
            }
        }
    }
    if st.help {
        draw_help(f, area);
    }
}

fn draw_header(buf: &mut Buffer, st: &State, area: Rect) {
    let mut x = put(
        buf,
        area.x,
        area.y,
        "swatches",
        Style::default().add_modifier(Modifier::BOLD),
    );
    x = put(buf, x, area.y, "  ", Style::default());
    for (i, k) in ALL.iter().enumerate() {
        let style = if i == st.kind {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        x = put(buf, x, area.y, &format!(" {}:{} ", i + 1, k.name()), style);
    }
    let hint = "hjkl move   yy copy   ? help   q quit";
    let hx = area.right().saturating_sub(hint.len() as u16 + 1);
    if hx > x {
        put(
            buf,
            hx,
            area.y,
            hint,
            Style::default().add_modifier(Modifier::DIM),
        );
    }
}

fn draw_grid(buf: &mut Buffer, st: &State, area: Rect) {
    let g = &st.grid;
    for r in 0..g.rows {
        let y = area.y + (r * SWATCH_H) as u16;
        if y >= area.bottom() {
            break;
        }
        for c in 0..g.cols {
            let idx = g.index(r, c);
            let Some(color) = g.at(idx) else { continue };
            let x = area.x + (c * SWATCH_W) as u16;
            if x >= area.right() {
                break;
            }
            let bg = Color::Rgb(color.r, color.g, color.b);
            if idx == st.cursor {
                // 光标那格用黑/白方括号套住,底色一点没挡住
                let ink = color.ink();
                let style = Style::default()
                    .fg(Color::Rgb(ink.r, ink.g, ink.b))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD);
                for (i, ch) in ['[', ']'].into_iter().enumerate() {
                    let cx = x + i as u16;
                    if cx < area.right() {
                        buf[(cx, y)].set_char(ch).set_style(style);
                    }
                }
            } else {
                // 空格的底色就是颜色本身
                let style = Style::default().fg(bg).bg(bg);
                for i in 0..SWATCH_W as u16 {
                    let cx = x + i;
                    if cx < area.right() {
                        buf[(cx, y)].set_char(' ').set_style(style);
                    }
                }
            }
        }
    }
}

fn draw_info(buf: &mut Buffer, st: &State, area: Rect) {
    let Some(c) = st.grid.at(st.cursor) else {
        return;
    };
    let bg = Color::Rgb(c.r, c.g, c.b);
    // 左边一块大色卡,hex 写在里面
    let pw = 11u16.min(area.width.saturating_sub(2));
    for y in area.y..area.bottom() {
        for x in area.x + 1..area.x + 1 + pw {
            if x < area.right() {
                buf[(x, y)].set_style(Style::default().bg(bg));
            }
        }
    }
    let hex = c.hex();
    let ink = c.ink();
    let hx = area.x + 1 + (pw.saturating_sub(hex.len() as u16)) / 2;
    put(
        buf,
        hx,
        area.y + area.height / 2,
        &hex,
        Style::default()
            .fg(Color::Rgb(ink.r, ink.g, ink.b))
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    );

    let dx = area.x + 2 + pw;
    let (h, s, l) = c.hsl();
    let (a16, name) = c.ansi16();
    let (row, col) = st.grid.point(st.cursor);
    put(
        buf,
        dx,
        area.y,
        &format!(
            "rgb({}, {}, {})   hsl({}, {}%, {}%)",
            c.r, c.g, c.b, h, s, l
        ),
        Style::default(),
    );
    put(
        buf,
        dx,
        area.y + 1,
        &format!(
            "ansi256 {}   classic {} ({})   sgr {}",
            c.ansi256(),
            name,
            a16,
            c.sgr()
        ),
        Style::default().add_modifier(Modifier::DIM),
    );
    put(
        buf,
        dx,
        area.y + 2,
        &format!(
            "row {} col {}   cell {}/{}   palette {}",
            row,
            col,
            st.cursor + 1,
            st.grid.cells.len(),
            ALL[st.kind].name()
        ),
        Style::default().add_modifier(Modifier::DIM),
    );
}

fn draw_status(buf: &mut Buffer, st: &State, area: Rect) {
    let style = if st.status.is_empty() {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    let text = if st.status.is_empty() {
        "ready"
    } else {
        &st.status
    };
    put(buf, area.x, area.y, text, style);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let w = (HELP_WIDTH as u16 + 2).min(area.width.saturating_sub(2));
    let h = (HELP.len() as u16 + 2).min(area.height.saturating_sub(2));
    let r = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    let lines: Vec<Line> = HELP.iter().map(|s| Line::from(*s)).collect();
    let block = Block::default().borders(Borders::ALL).border_set(ASCII);
    f.render_widget(Paragraph::new(lines).block(block), r);
}

// 写串并返回新的 x,超出右边界由 Buffer::set_stringn 自己裁掉
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) -> u16 {
    buf.set_stringn(x, y, text, usize::MAX, style).0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_at(rows: usize, cols: usize) -> State {
        let mut st = State::new(0);
        st.rebuild(rows, cols);
        st
    }

    #[test]
    fn every_help_line_fits_inside_the_box() {
        for line in HELP {
            assert!(
                line.chars().count() <= HELP_WIDTH,
                "help line is too wide for the box: {line:?}"
            );
        }
    }

    #[test]
    fn regions_leave_room_for_the_furniture() {
        let area = Rect::new(0, 0, 100, 30);
        let r = regions(area).expect("100x30 is usable");
        assert_eq!(r.header, Rect::new(0, 0, 100, 1));
        assert_eq!(r.body, Rect::new(0, 1, 100, 25));
        assert_eq!(r.info, Rect::new(0, 26, 100, 3));
        assert_eq!(r.status, Rect::new(0, 29, 100, 1));
    }

    #[test]
    fn regions_refuse_a_tiny_window() {
        assert!(regions(Rect::new(0, 0, 20, 30)).is_none());
        assert!(regions(Rect::new(0, 0, 100, 5)).is_none());
    }

    #[test]
    fn grid_cells_land_on_two_cell_columns() {
        let st = state_at(4, 10);
        // 10 块色卡铺满 20 格,每块宽 2
        let last = st.grid.index(3, 9);
        assert_eq!(st.grid.point(last), (3, 9));
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 9));
        draw_grid(&mut buf, &st, Rect::new(0, 1, 20, 4));
        let c = st.grid.at(last).unwrap();
        assert_eq!(buf[(18, 4)].bg, Color::Rgb(c.r, c.g, c.b));
        assert_eq!(buf[(19, 4)].bg, Color::Rgb(c.r, c.g, c.b));
    }

    #[test]
    fn the_cursor_is_drawn_as_brackets_over_the_color() {
        let mut st = state_at(4, 10);
        st.cursor = st.grid.index(2, 6);
        let c = st.grid.at(st.cursor).unwrap();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 9));
        draw_grid(&mut buf, &st, Rect::new(0, 1, 20, 4));
        let x = st.grid.point(st.cursor).1 * SWATCH_W;
        assert_eq!(buf[(x as u16, 3)].symbol(), "[");
        assert_eq!(buf[(x as u16 + 1, 3)].symbol(), "]");
        assert_eq!(buf[(x as u16, 3)].bg, Color::Rgb(c.r, c.g, c.b));
        let ink = c.ink();
        assert_eq!(buf[(x as u16, 3)].fg, Color::Rgb(ink.r, ink.g, ink.b));
    }

    #[test]
    fn info_panel_prints_the_hex_inside_the_preview() {
        let mut st = state_at(4, 10);
        st.cursor = st.grid.index(0, 0);
        let c = st.grid.at(st.cursor).unwrap();
        let hex = c.hex();
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 9));
        draw_info(&mut buf, &st, Rect::new(0, 5, 60, 3));
        let row: String = (1..12).map(|x| buf[(x, 6)].symbol().to_string()).collect();
        assert!(row.contains(&hex), "preview row was {row:?}");
        assert_eq!(buf[(1, 6)].bg, Color::Rgb(c.r, c.g, c.b));
    }

    #[test]
    fn every_palette_survives_a_realistic_terminal() {
        for (k, kind) in ALL.iter().enumerate() {
            let mut st = State::new(k);
            st.rebuild(25, 40);
            assert!(
                st.grid.at(st.cursor).is_some(),
                "{} has no swatch",
                kind.name()
            );
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 31));
            render_into(&mut buf, &st);
        }
    }

    // 直接跑一遍绘图路径,确认没有越界 panic
    fn render_into(buf: &mut Buffer, st: &State) {
        let area = buf.area;
        let reg = regions(area).expect("usable");
        draw_header(buf, st, reg.header);
        draw_grid(buf, st, reg.body);
        draw_info(buf, st, reg.info);
        draw_status(buf, st, reg.status);
    }

    // 色块的底色必须真的以 24 位真彩写出去,不能被量化成调色板编号
    #[test]
    fn swatch_colors_reach_the_wire_as_truecolor() {
        use ratatui::backend::{Backend, CrosstermBackend};
        use ratatui::{Terminal, TerminalOptions, Viewport};

        // 本机环境可能带着 NO_COLOR,crossterm 会因此一个颜色都不写;这条测试看的是字节,
        // 所以显式把颜色打开
        crossterm::style::Colored::set_ansi_color_disabled(false);

        let mut st = State::new(0);
        let area = Rect::new(0, 0, 24, 9);
        st.rebuild(2, 10);
        let mut out: Vec<u8> = Vec::new();
        {
            let backend = CrosstermBackend::new(&mut out);
            let mut term = Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Fixed(area),
                },
            )
            .expect("a fixed viewport needs no tty");
            term.draw(|f| render(f, &st)).expect("draw");
            term.backend_mut().flush().expect("flush");
        }
        let text = String::from_utf8_lossy(&out);
        let c = st.grid.at(st.grid.index(0, 0)).unwrap();
        let want = format!("48;2;{};{};{}", c.r, c.g, c.b);
        assert!(text.contains(&want), "no truecolor background in {text:?}");
    }
}
