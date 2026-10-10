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
use crate::ui::{draw_box, put, put_padded, truncate, BOX_H, BOX_V};

/// 每层占的列数:1 列画节点,3 列画连线
pub const CELL_W: u16 = 4;

/// 每个岔路占的行数:1 行放节点,1 行留给斜线
pub const ROW_H: u16 = 2;

/// 右边放图例需要多少列
const LEGEND_W: u16 = 22;

/// 地图有几种画法(:ui map 1..5)
pub const UI_COUNT: u8 = 5;

/// 按 app.map_ui 挑一种画法;默认是 1 号(符号)
pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    match app.map_ui {
        2 => render_grid(buf, area, app),
        3 => render_rails(buf, area, app),
        4 => render_slide(buf, area, app),
        5 => render_icons(buf, area, app),
        _ => render_sigils(buf, area, app),
    }
}

/// 画法 1:符号
pub fn render_sigils(buf: &mut Buffer, area: Rect, app: &App) {
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

/// 地图上的房间记号。精英:红底铺三格(左右各多一格);燃烧精英在右边多一个 *;
/// 休息与商店用 emoji(各占两格,终端按显示宽度算)
fn map_glyph_text(kind: NodeKind, burning: bool) -> (&'static str, u16) {
    match kind {
        NodeKind::Rest => ("🔥", 0),
        NodeKind::Shop => ("💰", 0),
        NodeKind::Elite if burning => (" E*", 1),
        NodeKind::Elite => (" E ", 1),
        NodeKind::Monster => ("e", 0),
        NodeKind::Event => ("?", 0),
        NodeKind::Treasure => ("T", 0),
        NodeKind::Boss => ("B", 0),
    }
}

/// 房间的配色。精英(含燃烧精英)统一红底;其它照类型
fn map_glyph_style(kind: NodeKind) -> Style {
    if kind == NodeKind::Elite {
        theme::kind_style(NodeKind::Elite)
    } else {
        theme::kind_style(kind)
    }
}

/// 图例:一个符号一行,颜色照搬地图上的用法;Boss 那行直接写它这一局的全名
fn legend_lines(run: &Run) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    if run.wing_boots_charges() > 0 {
        out.push((
            format!("Wing Boots  x{}", run.wing_boots_charges()),
            theme::fg(theme::GOLD),
        ));
    }
    out.extend(vec![
        ("[] you".to_string(), Style::default().add_modifier(Modifier::BOLD)),
        ("?  Unknown".to_string(), theme::kind_style(NodeKind::Event)),
        ("💰  Merchant".to_string(), theme::kind_style(NodeKind::Shop)),
        ("T  Treasure".to_string(), theme::kind_style(NodeKind::Treasure)),
        ("🔥  Rest".to_string(), theme::kind_style(NodeKind::Rest)),
        ("e  Enemy".to_string(), theme::kind_style(NodeKind::Monster)),
        (" E  Elite".to_string(), theme::kind_style(NodeKind::Elite)),
        (" E* Burning".to_string(), theme::kind_style(NodeKind::Elite)),
        (
            format!("B  {}", run.boss_name()),
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
            let label = if is_sel {
                format!("[{name}]")
            } else {
                name.to_string()
            };
            if room as usize >= crate::ui::display_width(&label) {
                put(buf, x, y, &truncate(&label, room as usize), style);
            } else {
                // 实在放不下名字就退回符号,图例那一行仍然写着全名
                put(buf, x, y, "[B]", style);
            }
            continue;
        }
        // 记号:精英红底铺三格(左右各多一格),燃烧精英右边多一个 *;休息/商店是 emoji
        let (glyph, indent) = map_glyph_text(node.kind, node.burning);
        let kind_style = map_glyph_style(node.kind);
        let visited = run.path.contains(i);
        // 选中的房间用 [ ] 框出来:符号仍然落在本列,方括号借用左右各一格
        let sel_text = if node.kind == NodeKind::Elite {
            // 精英本来就铺了三格,选中时不再往两边撑
            if node.burning {
                "[E*]".to_string()
            } else {
                "[E]".to_string()
            }
        } else {
            format!("[{glyph}]")
        };
        let (px, text, style) = if is_sel && x > indent {
            (
                if node.kind == NodeKind::Elite {
                    x.saturating_sub(1)
                } else {
                    x - 1 - indent
                },
                sel_text,
                kind_style.add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK),
            )
        } else if visited {
            // 走过的房间(含现在这间)统一绿底
            (
                x.saturating_sub(indent),
                glyph.to_string(),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::GOOD)
                    .add_modifier(Modifier::BOLD),
            )
        } else if is_sel {
            // 最左那一列没法往左借格子,退回底色
            (
                x.saturating_sub(indent),
                glyph.to_string(),
                kind_style.bg(theme::SEL_BG).add_modifier(Modifier::BOLD),
            )
        } else if on_path {
            // 选了它之后能走到的房间
            (
                x.saturating_sub(indent),
                glyph.to_string(),
                kind_style.add_modifier(Modifier::BOLD),
            )
        } else if is_candidate {
            // 别的岔路:现在就能选,跟着一起闪
            (
                x.saturating_sub(indent),
                glyph.to_string(),
                kind_style.add_modifier(Modifier::SLOW_BLINK),
            )
        } else {
            // 现在走不到的一律灰掉
            (
                x.saturating_sub(indent),
                glyph.to_string(),
                theme::dim(),
            )
        };
        put(buf, px, y, &text, style);
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
            let style = if on_path {
                Style::default().fg(theme::SEL_FG).add_modifier(Modifier::BOLD)
            } else {
                theme::dim()
            };
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

// ---- 后面几种画法共用的状态与贴图 ----

/// 一屏地图共用的状态:可达 / 选中 / 未来 一次算好
struct MapView<'a> {
    run: &'a Run,
    total: usize,
    scroll: usize,
    reach: Vec<usize>,
    sel: usize,
    future: Option<Vec<bool>>,
}

impl<'a> MapView<'a> {
    fn new(app: &'a App) -> MapView<'a> {
        let reach = app.run.travel_options();
        let sel = app.map_sel.min(reach.len().saturating_sub(1));
        MapView {
            run: &app.run,
            total: app.run.map.total_floors(),
            scroll: app.map_scroll,
            reach,
            sel,
            future: chosen_future(app),
        }
    }

    fn is_sel(&self, i: usize) -> bool {
        !self.reach.is_empty() && self.reach[self.sel] == i
    }

    fn is_reach(&self, i: usize) -> bool {
        self.reach.contains(&i)
    }

    fn on_future(&self, i: usize) -> bool {
        self.future.as_ref().map(|f| f[i]).unwrap_or(false)
    }

    fn visited(&self, i: usize) -> bool {
        self.run.path.contains(&i)
    }

    /// 房间在几种画法里的统一配色:走过 > 选中 > 未来 > 候选 > 走不到
    fn style(&self, i: usize) -> Style {
        let n = self.run.map.node(i);
        let base = if n.burning {
            theme::burning_style()
        } else {
            theme::kind_style(n.kind)
        };
        if self.visited(i) {
            Style::default()
                .fg(theme::BG)
                .bg(theme::GOOD)
                .add_modifier(Modifier::BOLD)
        } else if self.is_sel(i) {
            base.bg(theme::SEL_BG)
                .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
        } else if self.on_future(i) {
            base.add_modifier(Modifier::BOLD)
        } else if self.is_reach(i) {
            base.add_modifier(Modifier::SLOW_BLINK)
        } else {
            theme::dim()
        }
    }

    /// 一屏放几层、从哪层开始(和 1 号画法同一套视野规则)
    fn window(&self, area: Rect, cell_w: u16) -> (usize, usize) {
        let visible = ((area.width as usize / cell_w.max(1) as usize).max(1)).min(self.total);
        let start = self.scroll.min(self.total.saturating_sub(visible));
        (start, visible)
    }

    /// 这一层这个岔路上的房间(没有就是 None)
    fn node_at(&self, floor: usize, col: usize) -> Option<usize> {
        self.run
            .map
            .row(floor)
            .iter()
            .copied()
            .find(|i| self.run.map.node(*i).col == col)
    }

    /// 一条边亮不亮:两头的房间都要在"选中那条路的未来"里
    fn edge_lit_here(&self, parent: usize, child: usize) -> bool {
        match self.future.as_deref() {
            Some(f) => edge_lit(f, self.run.pos, parent, child),
            None => false,
        }
    }
}

/// 边的样式:亮的用选中色加粗,其余压暗
fn edge_style(lit: bool) -> Style {
    if lit {
        Style::default()
            .fg(theme::SEL_FG)
            .add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    }
}

/// 画法 5 用的宽图标(固定 3 列)
fn icon3(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Monster => "<e>",
        NodeKind::Elite => "<E>",
        NodeKind::Event => "{?}",
        NodeKind::Rest => "(R)",
        NodeKind::Shop => "[$]",
        NodeKind::Treasure => "<T>",
        NodeKind::Boss => "/B\\",
    }
}

/// 画法 2:格子表.每层一列、每个岔路一行,房间摆在格子里,连线画在两层之间的格线列上.
pub fn render_grid(buf: &mut Buffer, area: Rect, app: &App) {
    let v = MapView::new(app);
    let rows = COLS as u16;
    let cell_w: u16 = 4; // 三列内容 + 一列当连线/格线
    let need = rows * 2 + 2; // 层号一行 + 每个岔路"一格 + 一条格线"
    if area.width < cell_w * 3 + 2 || area.height < need {
        render_sigils(buf, area, app);
        return;
    }
    let (start, visible) = v.window(area, cell_w);
    let w = visible as u16 * cell_w;
    let left = area.x + area.width.saturating_sub(w + 1) / 2;
    let top = area.y + area.height.saturating_sub(need) / 2;
    let div = Style::default().fg(theme::BORDER);
    // 层号
    for i in 0..visible {
        let x = left + 1 + i as u16 * cell_w;
        put(buf, x, top, &truncate(&format!("{}", start + i), 2), theme::dim());
    }
    for r in 0..rows {
        let y = top + 1 + r * 2;
        put(buf, left, y, &BOX_V.to_string(), div);
        for i in 0..visible {
            let x = left + 1 + i as u16 * cell_w;
            let here = v.node_at(start + i, r as usize);
            let text = match here {
                Some(i2) if v.is_sel(i2) => {
                    format!("[{}]", v.run.map.node(i2).kind.sigil())
                }
                Some(i2) => format!(" {} ", v.run.map.node(i2).kind.sigil()),
                None => "   ".to_string(),
            };
            let style = here.map(|i2| v.style(i2)).unwrap_or_else(theme::dim);
            put(buf, x, y, &text, style);
            // 格子右边那一列:连到下一层就画连线,否则当格线
            let sx = x + 3;
            let link = if i + 1 < visible {
                match (here, v.node_at(start + i + 1, r as usize)) {
                    (Some(p), Some(c)) if v.run.map.node(p).next.contains(&c) => Some((p, c)),
                    _ => None,
                }
            } else {
                None
            };
            match link {
                Some((p, c)) => {
                    let child_col = v.run.map.node(c).col;
                    let ch = if child_col == r as usize {
                        BOX_H
                    } else if child_col > r as usize {
                        '╲'
                    } else {
                        '╱'
                    };
                    put(buf, sx, y, &ch.to_string(), edge_style(v.edge_lit_here(p, c)));
                }
                None => put(buf, sx, y, &BOX_V.to_string(), div),
            }
        }
        put(buf, left + w, y, &BOX_V.to_string(), div);
        // 岔路之间的格线:最后一行当下边框
        let y_div = y + 1;
        let last = r + 1 == rows;
        let (l, mid, rr) = if last {
            ('└', '┴', '┘')
        } else {
            ('├', '┼', '┤')
        };
        put(buf, left, y_div, &l.to_string(), div);
        for i in 0..visible {
            let x = left + 1 + i as u16 * cell_w;
            put(buf, x, y_div, &BOX_H.to_string().repeat(3), div);
            put(
                buf,
                x + 3,
                y_div,
                &(if i + 1 == visible { rr } else { mid }).to_string(),
                div,
            );
        }
    }
}

/// 画法 3:轨道.每个岔路一行,横着串起每层的房间,像一张列车时刻表.
pub fn render_rails(buf: &mut Buffer, area: Rect, app: &App) {
    let v = MapView::new(app);
    let rows = COLS as u16;
    let slot: u16 = 4; // 三列内容 + 一列连线
    let need = rows + 2;
    if area.width < slot * 4 + 8 || area.height < need {
        render_sigils(buf, area, app);
        return;
    }
    let (start, visible) = v.window(area, slot);
    let gx = area.x + 6;
    let top = area.y + area.height.saturating_sub(need) / 2;
    put(buf, area.x, top, "br", theme::dim());
    for i in 0..visible {
        let x = gx + i as u16 * slot;
        put(buf, x, top, &truncate(&format!("{}", start + i), 2), theme::dim());
    }
    for r in 0..rows {
        let y = top + 1 + r;
        put(buf, area.x, y, &format!("{r}"), theme::dim());
        put(buf, area.x + 2, y, &BOX_V.to_string(), theme::dim());
        for i in 0..visible {
            let x = gx + i as u16 * slot;
            match v.node_at(start + i, r as usize) {
                Some(i2) => {
                    let n = v.run.map.node(i2);
                    let text = if v.is_sel(i2) {
                        format!("[{}]", n.kind.sigil())
                    } else {
                        format!(" {} ", n.kind.sigil())
                    };
                    put(buf, x, y, &text, v.style(i2));
                    // 同一轨道上连着下一层就画一段横杠
                    if i + 1 < visible {
                        if let Some(next) = v.node_at(start + i + 1, r as usize) {
                            if n.next.contains(&next) {
                                put(
                                    buf,
                                    x + 3,
                                    y,
                                    &BOX_H.to_string(),
                                    edge_style(v.edge_lit_here(i2, next)),
                                );
                            }
                        }
                    }
                }
                None => put(buf, x, y, " · ", theme::dim()),
            }
        }
    }
}

/// 画法 4:一页幻灯片.不铺整张图,只讲"你在哪、下一步能去哪"和一条路线摘要.
pub fn render_slide(buf: &mut Buffer, area: Rect, app: &App) {
    let v = MapView::new(app);
    if area.width < 28 || area.height < 9 {
        render_sigils(buf, area, app);
        return;
    }
    let w = area.width.min(74);
    let h = area.height.min(13);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let bg = Style::default().fg(theme::FG).bg(theme::BG);
    for y in rect.y..rect.y + h {
        put_padded(buf, rect.x, y, "", w as usize, bg);
    }
    let title = format!(" act {} - map ", v.run.act);
    draw_box(
        buf,
        rect,
        &title,
        Style::default().fg(theme::BORDER).bg(theme::BG),
        Style::default().fg(theme::GOLD).bg(theme::BG),
    );
    let inner = (w - 4) as usize;
    // 第一行:层数与你在哪
    let floor_now = v.run.pos.map(|i| v.run.map.node(i).floor).unwrap_or(0);
    put_padded(
        buf,
        rect.x + 2,
        rect.y + 1,
        &format!("floor {floor_now} / {}", v.total),
        inner,
        bg,
    );
    let here = match v.run.pos {
        Some(i) => {
            let n = v.run.map.node(i);
            format!("you: {} {}", icon3(n.kind), n.kind.name())
        }
        None => "you: not on the path yet".to_string(),
    };
    put_padded(
        buf,
        rect.x + 2 + 22,
        rect.y + 1,
        &here,
        inner.saturating_sub(22),
        Style::default().fg(theme::GOOD).bg(theme::BG),
    );
    // 路线摘要:走过的 + 之后能到的
    put(buf, rect.x + 2, rect.y + 3, "route", theme::dim());
    let mut x = rect.x + 10;
    for f in 0..v.total {
        let hit = v
            .run
            .map
            .row(f)
            .iter()
            .copied()
            .find(|i| v.visited(*i) || v.on_future(*i));
        let Some(i) = hit else { continue };
        let n = v.run.map.node(i);
        let text = if v.run.pos == Some(i) {
            format!("[{}]", n.kind.sigil())
        } else {
            format!(" {} ", n.kind.sigil())
        };
        if x + 3 > rect.x + rect.width {
            break;
        }
        put(buf, x, rect.y + 3, &text, v.style(i));
        x += 3;
        if f + 1 < v.total {
            x += 1;
        }
    }
    // 下一步能去哪
    put(buf, rect.x + 2, rect.y + 5, "next", theme::dim());
    for (k, i) in v.reach.iter().enumerate().take(4) {
        let y = rect.y + 6 + k as u16;
        if y >= rect.y + h - 2 {
            break;
        }
        let n = v.run.map.node(*i);
        let mark = if k == v.sel { ">" } else { " " };
        let line = format!(
            "{mark} {} {:<8} floor {}  branch {}",
            icon3(n.kind),
            n.kind.name(),
            n.floor,
            n.col
        );
        put_padded(
            buf,
            rect.x + 2,
            y,
            &line,
            inner,
            if k == v.sel {
                Style::default()
                    .fg(theme::FG)
                    .bg(theme::SEL_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                v.style(*i)
            },
        );
    }
    put_padded(
        buf,
        rect.x + 2,
        rect.y + h - 2,
        "j/k pick   enter go   h/l look   esc close",
        inner,
        theme::dim().bg(theme::BG),
    );
}

/// 画法 5:宽图标.房间画成 3 列的图标,每个岔路占两行(图标行 + 连线行).
pub fn render_icons(buf: &mut Buffer, area: Rect, app: &App) {
    let v = MapView::new(app);
    let rows = COLS as u16;
    let slot: u16 = 5; // 三列图标 + 一列连线 + 一列空隙
    let need = rows * 2 + 3; // 标尺 + 每个岔路两行 + 图例一行
    if area.width < slot * 3 || area.height < need {
        render_sigils(buf, area, app);
        return;
    }
    let (start, visible) = v.window(area, slot);
    let w = visible as u16 * slot;
    let left = area.x + area.width.saturating_sub(w) / 2;
    let top = area.y + area.height.saturating_sub(need) / 2 + 1;
    for i in 0..visible {
        let x = left + i as u16 * slot;
        put(buf, x, top, &truncate(&format!("{}", start + i), 2), theme::dim());
    }
    for r in 0..rows {
        let y = top + 1 + r * 2;
        for i in 0..visible {
            let x = left + i as u16 * slot;
            match v.node_at(start + i, r as usize) {
                Some(i2) => {
                    let n = v.run.map.node(i2);
                    put(buf, x, y, icon3(n.kind), v.style(i2));
                    if i + 1 < visible {
                        if let Some(c) = v.node_at(start + i + 1, r as usize) {
                            if n.next.contains(&c) {
                                put(buf, x + 3, y, &BOX_H.to_string(), edge_style(v.edge_lit_here(i2, c)));
                            }
                        }
                    }
                    // 选中/当前这间在下一行指一个箭头
                    if v.is_sel(i2) || v.run.pos == Some(i2) {
                        put(buf, x + 1, y + 1, "^", theme::fg(theme::GOLD));
                    } else if i + 1 < visible {
                        // 斜着连到下一层的另一条岔路
                        if let Some(c) = (0..rows)
                            .filter_map(|rr| v.node_at(start + i + 1, rr as usize))
                            .find(|c| n.next.contains(c))
                        {
                            let cc = v.run.map.node(c).col;
                            let ch = if cc > r as usize { '╲' } else { '╱' };
                            put(buf, x + 2, y + 1, &ch.to_string(), edge_style(v.edge_lit_here(i2, c)));
                        }
                    }
                }
                None => put(buf, x, y, "   ", theme::dim()),
            }
        }
    }
    let legend = "<e> enemy  <E> elite  {?} event  (R) rest  [$] shop  <T> treasure  /B\\ boss";
    put(
        buf,
        left,
        top + need - 1,
        &truncate(legend, area.width.saturating_sub(left - area.x) as usize),
        theme::dim(),
    );
}
