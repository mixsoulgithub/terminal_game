// 地图界面:横向铺开,第 1 层在左、Boss 在右;纵向是同一层的不同岔路.
// 只有"现在真能走的路"(当前节点、下一步的可选节点、以及它们之间的连线)是亮的,
// 其余一律灰掉;右边空出来的地方放图例.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::map::{NodeKind, COLS};
use crate::core::run::Run;
use crate::ui::theme;
use crate::ui::{put, truncate};

/// 每层占的列数:3 列画节点,1 列画连线
pub const CELL_W: u16 = 4;

/// 右边放图例需要多少列
const LEGEND_W: u16 = 22;

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
        put(buf, area.x, area.y + area.height.saturating_sub(2), &tip, theme::fg(theme::WARN));
    }
    let legend = legend_lines(run);
    let map_w = visible as u16 * CELL_W;
    let right_x = area.x + map_w + 2;
    if area.width >= map_w + LEGEND_W + 2 && area.height as usize > legend.len() {
        // 右边空着,图例就摆在那里
        for (i, (text, style)) in legend.iter().enumerate() {
            put(buf, right_x, top + i as u16, text, *style);
        }
    } else {
        let line: String = legend
            .iter()
            .map(|(t, _)| t.trim().split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("  ");
        put(
            buf,
            area.x,
            area.y + area.height.saturating_sub(1),
            &truncate(&line, area.width as usize),
            theme::dim(),
        );
    }
}

/// 图例:每个符号是什么,颜色照搬地图上的用法
fn legend_lines(run: &Run) -> Vec<(String, Style)> {
    vec![
        ("M  monster".to_string(), theme::kind_style(NodeKind::Monster)),
        ("E  elite".to_string(), theme::kind_style(NodeKind::Elite)),
        ("?  event".to_string(), theme::kind_style(NodeKind::Event)),
        ("R  rest".to_string(), theme::kind_style(NodeKind::Rest)),
        ("$  shop".to_string(), theme::kind_style(NodeKind::Shop)),
        ("T  treasure".to_string(), theme::kind_style(NodeKind::Treasure)),
        (
            format!("B  {}", run.boss_name()),
            theme::kind_style(NodeKind::Boss),
        ),
        (
            "bright = you can go there next".to_string(),
            theme::fg(theme::SEL_FG),
        ),
        (
            "(...) = you are here".to_string(),
            theme::fg(theme::GOOD),
        ),
    ]
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
        let is_sel = reachable_here
            .iter()
            .position(|r| r == i)
            .map(|p| p == sel)
            .unwrap_or(false);
        let is_next = reach.contains(i);
        // Boss 直接写名字,其余用符号
        if node.kind == NodeKind::Boss {
            let x = if is_cur { x + 1 } else { x };
            let name = truncate(run.boss_name(), 24);
            let style = if is_next {
                theme::kind_style(NodeKind::Boss)
            } else {
                theme::dim()
            };
            put(buf, x, y, &name, style);
            continue;
        }
        let sigil = node.kind.sigil();
        let (text, style) = if is_cur {
            (
                format!("({sigil})"),
                Style::default().fg(theme::GOOD).add_modifier(Modifier::BOLD),
            )
        } else if is_sel {
            (
                format!("<{sigil}>"),
                theme::kind_style(node.kind).bg(theme::SEL_BG).add_modifier(Modifier::BOLD),
            )
        } else if is_next {
            (
                format!("<{sigil}>"),
                theme::kind_style(node.kind).add_modifier(Modifier::BOLD),
            )
        } else {
            // 现在走不到的节点一律灰掉
            (format!("[{sigil}]"), theme::dim())
        };
        put(buf, x, y, &text, style);
    }
}

/// 连线:逐个"子节点"画一条从父节点到它的斜线.
/// 一个父节点最多连三个子节点,只画第一条会把岔路藏起来;
/// 而按子节点画,朝上的走上一半行、朝下的走下一半行,彼此不会压到.
fn render_edges(buf: &mut Buffer, x: u16, y0: u16, run: &Run, floor: usize, slot_h: u16) {
    let reach = run.reachable();
    let upper = floor + 1;
    if upper >= run.map.total_floors() {
        return;
    }
    let col_x = x + CELL_W - 1;
    let row_of = |col: usize| y0 + (col as u16) * slot_h;
    for child in run.map.row(upper) {
        let node = run.map.node(*child);
        let Some(parent) = node.prev.first() else {
            continue;
        };
        let yp = row_of(run.map.node(*parent).col);
        let yc = row_of(node.col);
        // 这条线属于"现在能走的路"才亮:要么从当前节点出发,要么终点是下一步可选
        let on_path = reach.contains(parent) || reach.contains(child);
        let style = if on_path {
            Style::default().fg(theme::SEL_FG).add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        if yp == yc {
            if yp < buf.area.height {
                put(buf, col_x, yp, "-", style);
            }
            continue;
        }
        let ch = if yc > yp { '\\' } else { '/' };
        for y in (yp.min(yc) + 1)..yp.max(yc) {
            if y < buf.area.height {
                put(buf, col_x, y, &ch.to_string(), style);
            }
        }
    }
}
