// 地图界面:从下往上爬的 ASCII 地图,一次画 2 行一层(节点行 + 连接行).
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::run::Run;
use crate::ui::theme;
use crate::ui::{put, truncate};

/// 每列占 4 格
const CELL: usize = 4;

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let run = &app.run;
    let cols = run.map.row(0).len().max(1);
    let _ = cols;
    let map_w = crate::core::map::COLS * CELL;
    if area.width as usize <= map_w + 6 {
        // 太窄就退化成列表
        render_compact(buf, area, app);
        return;
    }
    let x0 = area.x + 5 + (((area.width as usize - map_w - 6) / 2) as u16);
    let total = run.map.total_floors();
    let cur_floor = if run.pos.is_some() { run.floor() } else { 0 };
    let win = (area.height as usize / 2).max(3).min(total);
    let lo = if total <= win {
        0
    } else {
        cur_floor
            .saturating_sub(win / 2)
            .min(total.saturating_sub(win))
    };
    let hi = (lo + win - 1).min(total - 1);
    for f in (lo..=hi).rev() {
        let y = area.y + ((hi - f) as u16) * 2;
        if y >= area.y + area.height {
            break;
        }
        render_floor(buf, x0, y, app, f);
        if f > lo && y + 1 < area.y + area.height {
            render_connector(buf, x0, y + 1, run, f - 1);
        }
    }
    // 左下角图例
    let legend = "M monster  E elite  ? event  R rest  $ shop  T treasure  B boss";
    put(
        buf,
        area.x,
        area.y + area.height.saturating_sub(1),
        &truncate(legend, area.width as usize),
        theme::dim(),
    );
}

fn render_floor(buf: &mut Buffer, x0: u16, y: u16, app: &App, floor: usize) {
    let run = &app.run;
    let label = format!("F{:>2} ", floor + 1);
    put(buf, x0.saturating_sub(5), y, &label, theme::fg(theme::INFO));
    let reach = run.reachable();
    let reachable_here: Vec<usize> = reach
        .iter()
        .copied()
        .filter(|i| run.map.node(*i).floor == floor)
        .collect();
    for i in run.map.row(floor) {
        let node = run.map.node(*i);
        let x = x0 + (node.col * CELL) as u16;
        let is_cur = run.pos == Some(*i);
        let is_reach = reach.contains(i);
        let is_sel = reachable_here
            .iter()
            .position(|r| r == i)
            .map(|p| p == app.map_sel.min(reachable_here.len().saturating_sub(1)))
            .unwrap_or(false);
        let sigil = node.kind.sigil();
        let (text, style) = if is_cur {
            (
                format!("({sigil})"),
                Style::default()
                    .fg(theme::GOOD)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_reach {
            if is_sel {
                (
                    format!("<{sigil}>"),
                    Style::default()
                        .fg(theme::SEL_FG)
                        .bg(theme::SEL_BG)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    format!("<{sigil}>"),
                    Style::default()
                        .fg(theme::kind_color(node.kind))
                        .add_modifier(Modifier::BOLD),
                )
            }
        } else {
            (format!("[{sigil}]"), theme::fg(theme::kind_color(node.kind)))
        };
        put(buf, x, y, &text, style);
    }
}

/// 连接行:每个下层节点往上看,画它第一条上行边的走向
fn render_connector(buf: &mut Buffer, x0: u16, y: u16, run: &Run, lower_floor: usize) {
    let reach = run.reachable();
    for i in run.map.row(lower_floor) {
        let node = run.map.node(*i);
        let Some(child) = node.next.first() else {
            continue;
        };
        let c = run.map.node(*child);
        let ch = if c.col < node.col {
            '\\'
        } else if c.col > node.col {
            '/'
        } else {
            '|'
        };
        let style = if reach.contains(i) {
            Style::default()
                .fg(theme::SEL_FG)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        put(buf, x0 + (node.col * CELL + 1) as u16, y, &ch.to_string(), style);
    }
}

/// 窄终端下的退化视图:一行一层
fn render_compact(buf: &mut Buffer, area: Rect, app: &App) {
    let run = &app.run;
    let mut y = area.y;
    for f in (0..run.map.total_floors()).rev() {
        if y >= area.y + area.height {
            break;
        }
        let mut line = String::new();
        for i in run.map.row(f) {
            let node = run.map.node(*i);
            let cur = run.pos == Some(*i);
            line.push_str(&format!(
                "{} ",
                if cur {
                    format!("({})", node.kind.sigil())
                } else {
                    format!("[{}]", node.kind.sigil())
                }
            ));
        }
        put(
            buf,
            area.x,
            y,
            &truncate(&format!("F{:>2} {}", f + 1, line), area.width as usize),
            theme::fg(theme::FG),
        );
        y += 1;
    }
}
