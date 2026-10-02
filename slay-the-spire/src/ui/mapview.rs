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
    let map_w = visible as u16 * CELL_W;
    let right_x = area.x + map_w + 2;
    let legend = legend_lines(run);
    let legend_shown = area.width >= map_w + LEGEND_W + 2 && area.height as usize > legend.len();
    // 选中那条岔路之后的整片未来:换一个岔路,亮的就是另一片
    let future = chosen_future(app);
    // 图例贴右边区域的上沿放,这样它不会和任何一行节点(尤其 Boss 那行)撞上
    let legend_y = if top >= area.y + 1 + legend.len() as u16 {
        top - legend.len() as u16
    } else {
        area.y + 1
    };
    for i in 0..visible {
        let f = start + i;
        if f >= total {
            break;
        }
        let x = area.x + (i as u16) * CELL_W;
        floor_label(buf, x, area.y, app, f);
        render_floor(
            buf,
            x,
            top,
            app,
            f,
            slot_h,
            future.as_deref(),
            if legend_shown {
                Some((right_x, legend_y + legend.len() as u16))
            } else {
                None
            },
        );
        if f + 1 < total {
            render_edges(buf, x, top, app, f, slot_h, future.as_deref());
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
    // 地图下面一行:亮暗和当前位置怎么读
    let hint = "bright = you can go there next    (x) = you are here";
    put(
        buf,
        area.x,
        area.y + area.height.saturating_sub(2),
        &truncate(hint, area.width as usize),
        theme::dim(),
    );
    if legend_shown {
        // 右边空着,图例就摆在那里
        for (i, (text, style)) in legend.iter().enumerate() {
            put(buf, right_x, legend_y + i as u16, text, *style);
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

/// 光标停着的那间房之后的整片未来(含它自己);还没上路时就是起点们的未来
fn chosen_future(app: &App) -> Option<Vec<bool>> {
    let reach = app.run.reachable();
    let chosen = *reach.get(app.map_sel.min(reach.len().saturating_sub(1)))?;
    Some(app.run.map.forward_reachable(chosen))
}

/// 图例:一个符号一行,颜色照搬地图上的用法;Boss 那行直接写它这一局的全名
fn legend_lines(run: &Run) -> Vec<(String, Style)> {
    vec![
        ("?  unknown".to_string(), theme::kind_style(NodeKind::Event)),
        ("$  merchant".to_string(), theme::kind_style(NodeKind::Shop)),
        ("T  treasure".to_string(), theme::kind_style(NodeKind::Treasure)),
        ("R  rest".to_string(), theme::kind_style(NodeKind::Rest)),
        ("E  enemy".to_string(), theme::kind_style(NodeKind::Monster)),
        ("E  elite".to_string(), theme::kind_style(NodeKind::Elite)),
        (
            format!("B  {}", run.boss_name()),
            theme::kind_style(NodeKind::Boss),
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

fn render_floor(
    buf: &mut Buffer,
    x: u16,
    y0: u16,
    app: &App,
    floor: usize,
    slot_h: u16,
    future: Option<&[bool]>,
    // 图例的位置:(左边, 结束行)——Boss 的名字不能压上去
    legend: Option<(u16, u16)>,
) {
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
        let is_candidate = reach.contains(i);
        let on_path = future.map(|f| f[*i]).unwrap_or(false);
        if node.kind == NodeKind::Boss {
            // 名字写在节点右边;只有这一行真的和图例同排时才让位
            let room = match legend {
                Some((lx, legend_bottom)) if y < legend_bottom => {
                    lx.saturating_sub(1).saturating_sub(x)
                }
                _ => buf.area.width.saturating_sub(x),
            };
            let style = if on_path {
                theme::kind_style(NodeKind::Boss)
            } else {
                theme::dim()
            };
            let name = run.boss_name();
            if room as usize >= crate::ui::display_width(name) {
                put(buf, x, y, &truncate(name, room as usize), style);
            } else {
                // 实在放不下名字就退回符号,图例那一行仍然写着全名
                put(buf, x, y, "[B]", style);
            }
            continue;
        }
        let sigil = node.kind.sigil();
        let kind_style = theme::kind_style(node.kind);
        let (text, style) = if is_cur {
            // 你现在在这里
            (
                format!("({sigil})"),
                Style::default().fg(theme::GOOD).add_modifier(Modifier::BOLD),
            )
        } else if is_sel {
            // 光标停着的那个岔路
            (
                format!("<{sigil}>"),
                kind_style.bg(theme::SEL_BG).add_modifier(Modifier::BOLD),
            )
        } else if on_path {
            // 选了它之后能走到的房间
            (format!("<{sigil}>"), kind_style.add_modifier(Modifier::BOLD))
        } else if is_candidate {
            // 别的岔路:可以选,但不是当前这条未来
            (format!("<{sigil}>"), kind_style)
        } else {
            // 现在走不到的一律灰掉
            (format!("[{sigil}]"), theme::dim())
        };
        put(buf, x, y, &text, style);
    }
}

/// 连线:逐个"子节点"画一条从父节点到它的斜线.
/// 一个父节点最多连三个子节点,只画第一条会把岔路藏起来;
/// 而按子节点画,朝上的走上一半行、朝下的走下一半行,彼此不会压到.
/// 父节点优先取"你现在站的那个",这样从当前位置出发的这条线一定画得出来.
fn render_edges(
    buf: &mut Buffer,
    x: u16,
    y0: u16,
    app: &App,
    floor: usize,
    slot_h: u16,
    future: Option<&[bool]>,
) {
    let run = &app.run;
    let upper = floor + 1;
    if upper >= run.map.total_floors() {
        return;
    }
    let col_x = x + CELL_W - 1;
    let row_of = |col: usize| y0 + (col as u16) * slot_h;
    for child in run.map.row(upper) {
        let node = run.map.node(*child);
        // Boss 房不连线:最后一层只有它,不需要箭头指过去
        if node.kind == NodeKind::Boss {
            continue;
        }
        let parent = node
            .prev
            .iter()
            .copied()
            .find(|p| Some(*p) == run.pos)
            .or_else(|| node.prev.first().copied());
        let Some(parent) = parent else {
            continue;
        };
        let yp = row_of(run.map.node(parent).col);
        let yc = row_of(node.col);
        // 这一步落在"选中那条岔路的未来"里才亮
        let on_path = future.map(|f| f[*child]).unwrap_or(false);
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
