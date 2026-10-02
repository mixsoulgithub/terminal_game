// 地图界面:横向铺开,第 1 层在左、Boss 在右;纵向是同一层的不同岔路.
// 一屏装不下整条路时用 h/l 往前/往后看,层号在最上面一行.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::map::COLS;
use crate::core::run::Run;
use crate::ui::theme;
use crate::ui::{put, truncate};

/// 每层占的列数:3 列画节点,1 列画连线
pub const CELL_W: u16 = 4;

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let run = &app.run;
    let total = run.map.total_floors();
    let visible = visible_floors(area.width as usize, total);
    let start = window_start(app, total, visible);
    // 每个岔路占几行:按可用高度摊开,连线的斜线才有地方画
    let avail = area.height.saturating_sub(2);
    let slot_h = (avail / COLS as u16).clamp(1, 4);
    let block_h = COLS as u16 * slot_h;
    let top = area.y + 1 + avail.saturating_sub(block_h) / 2;
    for i in 0..visible {
        let f = start + i;
        if f >= total {
            break;
        }
        let x = area.x + (i as u16) * CELL_W;
        floor_label(buf, x, area.y, app, f);
        render_floor(buf, x, top, app, f, slot_h);
        if f + 1 < total {
            render_edges(buf, x, top, run, f, slot_h);
        }
    }
    // 视野两头还有内容就给一句提示
    let mut tip = String::new();
    if start > 0 {
        tip.push_str("(h) back  ");
    }
    if start + visible < total {
        tip.push_str(&format!("(l) {} more floors", total - start - visible));
    }
    if !tip.is_empty() {
        put(buf, area.x, area.y + area.height - 2, &tip, theme::fg(theme::WARN));
    }
    let legend = "M monster  E elite  ? event  R rest  $ shop  T treasure  B boss   h/l look along the road  j/k pick a fork";
    put(
        buf,
        area.x,
        area.y + area.height.saturating_sub(1),
        &truncate(legend, area.width as usize),
        theme::dim(),
    );
}

/// 一屏能放几层
pub fn visible_floors(width: usize, total: usize) -> usize {
    (width / CELL_W as usize).max(1).min(total)
}

/// 视野起点:app 里的 map_scroll 是"往前后看路"的结果
fn window_start(app: &App, total: usize, visible: usize) -> usize {
    app.map_scroll.min(total.saturating_sub(visible))
}

fn floor_label(buf: &mut Buffer, x: u16, y: u16, app: &App, floor: usize) {
    let run = &app.run;
    let here = run.pos.is_some() && run.floor() == floor;
    let style = if here {
        Style::default().fg(theme::GOOD).add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    put(buf, x, y, &format!("{:>2} ", floor + 1), style);
}

fn render_floor(buf: &mut Buffer, x: u16, y0: u16, app: &App, floor: usize, slot_h: u16) {
    let run = &app.run;
    let reach = run.reachable();
    let reachable_here: Vec<usize> = reach
        .iter()
        .copied()
        .filter(|i| run.map.node(*i).floor == floor)
        .collect();
    let sel = app.map_sel.min(reachable_here.len().saturating_sub(1));
    for i in run.map.row(floor) {
        let node = run.map.node(*i);
        let y = y0 + (node.col as u16) * slot_h;
        if y >= buf.area.height {
            continue;
        }
        let is_cur = run.pos == Some(*i);
        let is_reach = reach.contains(i);
        let is_sel = is_reach
            && reachable_here
                .iter()
                .position(|r| r == i)
                .map(|p| p == sel)
                .unwrap_or(false);
        let sigil = node.kind.sigil();
        let (text, style) = if is_cur {
            (
                format!("({sigil})"),
                Style::default().fg(theme::GOOD).add_modifier(Modifier::BOLD),
            )
        } else if is_sel {
            (
                format!("<{sigil}>"),
                Style::default()
                    .fg(theme::SEL_FG)
                    .bg(theme::SEL_BG)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_reach {
            (
                format!("<{sigil}>"),
                Style::default()
                    .fg(theme::kind_color(node.kind))
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (format!("[{sigil}]"), theme::fg(theme::kind_color(node.kind)))
        };
        put(buf, x, y, &text, style);
    }
}

/// 连线:写在两层之间那一列上,从父节点那行一路斜到子节点那行
fn render_edges(buf: &mut Buffer, x: u16, y0: u16, run: &Run, floor: usize, slot_h: u16) {
    let reach = run.reachable();
    for i in run.map.row(floor) {
        let node = run.map.node(*i);
        let Some(child) = node.next.first() else {
            continue;
        };
        let c = run.map.node(*child);
        let yp = y0 + (node.col as u16) * slot_h;
        let yc = y0 + (c.col as u16) * slot_h;
        let style = if reach.contains(i) {
            Style::default().fg(theme::SEL_FG).add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        let col = x + CELL_W - 1;
        if yp == yc {
            if yp < buf.area.height {
                put(buf, col, yp, "-", style);
            }
            continue;
        }
        let ch = if yc > yp { '\\' } else { '/' };
        for y in (yp.min(yc) + 1)..yp.max(yc) {
            if y < buf.area.height {
                put(buf, col, y, &ch.to_string(), style);
            }
        }
    }
}
