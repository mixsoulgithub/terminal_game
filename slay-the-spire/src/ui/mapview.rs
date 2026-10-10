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
use crate::ui::{display_width, put, put_padded, truncate};

/// 每层占的列数:1 列画节点,3 列画连线
pub const CELL_W: u16 = 4;

/// 每个岔路占的行数:1 行放节点,1 行留给斜线
pub const ROW_H: u16 = 2;

/// 右边放图例需要多少列
const LEGEND_W: u16 = 22;

/// 地图界面:横向铺开,第 1 层在左、Boss 在右
pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let run = &app.run;
    let total = run.map.total_floors();
    let visible = visible_floors(area.width as usize, total);
    let start = window_start(app, total, visible);
    // 7 个岔路各占 1 行,行间再留 1 行画斜线,整块就是 2*7-1=13 行,垂直居中.
    // 一个节点最多 3 个出度,且都落在下一层的相邻 3 列,所以斜边只跨一个列号,
    // 中间那一行放一个 - \ / 就够连上.
    let block_h = COLS as u16 * ROW_H - 1;
    let top = area.y + area.height.saturating_sub(block_h) / 2;
    let map_w = visible as u16 * CELL_W;
    // 图例靠最右停,纵向和地图一样居中
    let right_x = area.x + area.width.saturating_sub(LEGEND_W);
    let legend = legend_lines(run);
    let legend_shown = right_x >= area.x + map_w + 2 && area.height as usize > legend.len();
    // 地图横向居中:图例在时,可用的宽度要减掉图例那一块
    let usable = if legend_shown {
        area.width.saturating_sub(LEGEND_W)
    } else {
        area.width
    };
    let map_x = area.x + usable.saturating_sub(map_w) / 2;
    // 选中那条岔路之后的整片未来:换一个岔路,亮的就是另一片
    let future = chosen_future(app);
    let legend_y = area.y + area.height.saturating_sub(legend.len() as u16) / 2;
    for i in 0..visible {
        let f = start + i;
        if f >= total {
            break;
        }
        let x = map_x + (i as u16) * CELL_W;
        render_floor(
            buf,
            x,
            top,
            app,
            f,
            future.as_deref(),
            if legend_shown {
                Some((right_x, legend_y + legend.len() as u16))
            } else {
                None
            },
        );
        if f + 1 < total {
            render_edges(buf, x, top, app, f, future.as_deref());
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
    // 右边空着就摆图例(不再写"亮暗怎么读"那行)
    if legend_shown {
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
    let reach = app.run.travel_options();
    let chosen = *reach.get(app.map_sel.min(reach.len().saturating_sub(1)))?;
    Some(app.run.map.forward_reachable(chosen))
}

/// 没选中时是单个字符(燃烧精英多一个 *);选中时左右各铺一格,
/// 好让色块正好铺满三格
fn map_glyph(kind: NodeKind, burning: bool, wide: bool) -> &'static str {
    let k = if burning { NodeKind::Elite } else { kind };
    match k {
        NodeKind::Monster => {
            if wide {
                " e "
            } else {
                "e"
            }
        }
        NodeKind::Elite if burning => {
            if wide {
                " E*"
            } else {
                "E*"
            }
        }
        NodeKind::Elite => {
            if wide {
                " E "
            } else {
                "E"
            }
        }
        NodeKind::Event => {
            if wide {
                " ? "
            } else {
                "?"
            }
        }
        NodeKind::Rest => {
            if wide {
                " R "
            } else {
                "R"
            }
        }
        NodeKind::Shop => {
            if wide {
                " $ "
            } else {
                "$"
            }
        }
        NodeKind::Treasure => {
            if wide {
                " T "
            } else {
                "T"
            }
        }
        NodeKind::Boss => {
            if wide {
                " B "
            } else {
                "B"
            }
        }
    }
}

/// 图例:一个符号一行,颜色照搬地图上的用法;Boss 那行直接写它这一局的全名。
/// 标记统一占 3 格再空 1 格,这样每一行名字都从同一列起头
fn legend_lines(run: &Run) -> Vec<(String, Style)> {
    let row = |marker: &str, name: &str| format!("{marker:<3} {name}");
    let mut out: Vec<(String, Style)> = Vec::new();
    if run.wing_boots_charges() > 0 {
        out.push((
            row("", &format!("Wing Boots x{}", run.wing_boots_charges())),
            theme::fg(theme::GOLD),
        ));
    }
    out.extend(vec![
        (row("[]", "you"), Style::default().add_modifier(Modifier::BOLD)),
        (
            row("?", "Unknown"),
            theme::kind_style(NodeKind::Event),
        ),
        (row("$", "Merchant"), theme::kind_style(NodeKind::Shop)),
        (row("T", "Treasure"), theme::kind_style(NodeKind::Treasure)),
        (row(" R ", "Rest"), theme::kind_style(NodeKind::Rest)),
        (row("e", "Enemy"), theme::kind_style(NodeKind::Monster)),
        (row(" E ", "Elite"), theme::kind_style(NodeKind::Elite)),
        (row(" E*", "Burning"), theme::kind_style(NodeKind::Elite)),
        (
            row("B", run.boss_name()),
            theme::kind_style(NodeKind::Boss),
        ),
    ]);
    out
}

/// 一屏能放几层
pub fn visible_floors(width: usize, total: usize) -> usize {
    (width / CELL_W as usize).max(1).min(total)
}

/// 视野起点:app 里的 map_scroll 是"往前后看路"的结果
fn window_start(app: &App, total: usize, visible: usize) -> usize {
    app.map_scroll.min(total.saturating_sub(visible))
}

fn render_floor(
    buf: &mut Buffer,
    x: u16,
    y0: u16,
    app: &App,
    floor: usize,
    future: Option<&[bool]>,
    // 图例的位置:(左边, 结束行)——Boss 的名字不能压上去
    legend: Option<(u16, u16)>,
) {
    let run = &app.run;
    // 羽翼靴有电时,往后的任意节点都算候选(飞过去要花一次充能)
    let reach = run.travel_options();
    let reachable_here: Vec<usize> = reach
        .iter()
        .copied()
        .filter(|i| run.map.node(*i).floor == floor)
        .collect();
    let sel = app.map_sel.min(reachable_here.len().saturating_sub(1));
    for i in run.map.row(floor) {
        let node = run.map.node(*i);
        let y = y0 + (node.col as u16) * ROW_H;
        if y >= buf.area.height {
            continue;
        }
        let is_sel = reachable_here
            .iter()
            .position(|r| r == i)
            .map(|p| p == sel)
            .unwrap_or(false);
        let is_candidate = reach.contains(i);
        let on_path = future.map(|f| f[*i]).unwrap_or(false);
        if node.kind == NodeKind::Boss {
            // Boss:一整块和精英同色的底,纵向铺满整张地图(所有岔路),
            // 名字横排写在块里(黑字);右边要给图例让位
            let room = match legend {
                Some((lx, _)) => lx.saturating_sub(1).saturating_sub(x),
                _ => buf.area.width.saturating_sub(x),
            };
            let name = run.boss_name();
            let w = ((display_width(name) + 2) as u16).min(room);
            if w >= 3 {
                let mut style = theme::kind_chip(NodeKind::Boss);
                if is_sel {
                    style = style.add_modifier(Modifier::SLOW_BLINK);
                }
                // 只有一行:名字左右各留一格,底色也就铺满这一行
                let label = truncate(name, (w - 2) as usize);
                put_padded(buf, x, y, &format!(" {label} "), w as usize, style);
            } else {
                // 右边放不下整块就退回一行红字
                put(
                    buf,
                    x,
                    y,
                    &truncate(name, room as usize),
                    theme::kind_style(NodeKind::Boss),
                );
            }
            continue;
        }
        // 只有"已到达 / 选中 / 在选中那条路线上"这三种状态才铺底色,
        // 底色左右各铺一格(所以用宽字形);其余只有字色
        let burning = node.burning;
        let k = if burning { NodeKind::Elite } else { node.kind };
        let visited = run.path.contains(i);
        let lit = visited || is_sel || on_path;
        let plain = map_glyph(k, burning, false).to_string();
        let (px, text, style) = if lit && x > 0 {
            let style = if visited {
                theme::arrived_chip()
            } else {
                theme::kind_chip(k)
            };
            let style = if is_sel {
                style.add_modifier(Modifier::SLOW_BLINK)
            } else {
                style
            };
            (x - 1, map_glyph(k, burning, true).to_string(), style)
        } else if lit {
            // 最左那一列借不到左边的格子:不铺底,只把字色提亮
            (
                x,
                plain.clone(),
                theme::kind_style(k)
                    .fg(theme::SEL_FG)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_candidate {
            (
                x,
                plain.clone(),
                theme::kind_style(k).add_modifier(Modifier::SLOW_BLINK),
            )
        } else {
            (x, plain, theme::dim())
        };
        put(buf, px, y, &text, style);
    }
}

/// 连线的样式:选中那条路上的连线跟着亮(和房间同色),其余压暗
fn edge_style(on_path: bool) -> Style {
    if on_path {
        Style::default()
            .fg(theme::SEL_FG)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    }
}

/// 一条边该不该亮:子节点要在"选中那条路的未来"里,父节点也得在未来里
/// (或者它就是你现在站的那间,保证从当前位置出发那条边亮着).
/// 只看子节点的话,别的岔路汇进这条路时它自己的入边会被误点亮.
pub(crate) fn edge_lit(future: &[bool], pos: Option<usize>, parent: usize, child: usize) -> bool {
    future[child] && (future[parent] || Some(parent) == pos)
}

/// 连线:逐个"父节点"把它的每一条出边都画出来.
/// 一个父节点最多 3 条出边,且子节点只落在相邻的 3 列,所以每条斜边只跨一个
/// 列号,正好落在两行之间那一行的中间列;同列的子节点连成一条横线 "-".
/// 按父节点画才不会漏掉"多条路合并到同一个子节点"时的其它入边.
fn render_edges(
    buf: &mut Buffer,
    x: u16,
    y0: u16,
    app: &App,
    floor: usize,
    future: Option<&[bool]>,
) {
    let run = &app.run;
    // 中间那一列:父节点在 x,子节点在 x+CELL_W,取正中
    let col_x = x + CELL_W / 2;
    let row_of = |col: usize| y0 + (col as u16) * ROW_H;
    for parent in run.map.row(floor) {
        let p = run.map.node(*parent);
        let yp = row_of(p.col);
        for child in &p.next {
            let c = run.map.node(*child);
            // 只连相邻的下一层;Boss 那一层也要连过来
            if c.floor != floor + 1 {
                continue;
            }
            let yc = row_of(c.col);
            let on_path = future
                .map(|f| edge_lit(f, run.pos, *parent, *child))
                .unwrap_or(false);
            let style = edge_style(on_path);
            if yp == yc {
                if yp < buf.area.height {
                    put(buf, col_x, yp, &crate::ui::BOX_H.to_string(), style);
                }
                continue;
            }
            // 制表符里的斜线
            let ch = if yc > yp { '╲' } else { '╱' };
            if yp.abs_diff(yc) > ROW_H {
                // 跨了不止一层(只有通往 Boss 才会这样):只画挨着父节点的那一格,
                // 当作"Boss 就在隔壁";不铺一整列,也不会所有边都堆到 Boss 旁同一格
                let y = if yc > yp { yp + 1 } else { yp - 1 };
                if y < buf.area.height {
                    put(buf, col_x, y, &ch.to_string(), style);
                }
                continue;
            }
            for y in (yp.min(yc) + 1)..yp.max(yc) {
                if y < buf.area.height {
                    put(buf, col_x, y, &ch.to_string(), style);
                }
            }
        }
    }
}
