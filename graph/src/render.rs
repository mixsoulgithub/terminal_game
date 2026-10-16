//! 投影与上屏:画布坐标 -> 字符格 -> ratatui buffer.
//! 节点字形、标签摆放、图例、状态栏、键位速查都在这儿.

use std::collections::{HashMap, HashSet};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::View;
use crate::canvas::{arrive_hor, box_char, Canvas};
use crate::lang::Lang;
use crate::model::Graph;
use crate::scene::Scene;

/// 每行显示宽度(全角算两格)
pub fn width(s: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(s)
}

/// 画布坐标 -> 字符格的投影
#[derive(Clone, Copy)]
pub struct Proj {
    /// 每列多少画布单位
    pub scale: f64,
    /// 视图中心(画布坐标)
    cx: f64,
    cy: f64,
    /// 一行的物理高度相当于几列(真实地理 2,布局算出来的 1)
    aspect: f64,
    /// y 是向上的(输入坐标)还是向下的(布局算出来的已经是屏幕方向)
    flip_y: bool,
    /// 中心落在哪一格
    x0: i32,
    y0: i32,
}

impl Proj {
    pub fn cell(&self, p: (f64, f64)) -> (i32, i32) {
        let dx = ((p.0 - self.cx) / self.scale).round() as i32;
        let dy = ((p.1 - self.cy) / (self.scale * self.aspect)).round() as i32;
        (
            self.x0 + dx,
            if self.flip_y {
                self.y0 - dy
            } else {
                self.y0 + dy
            },
        )
    }
}

/// 一块区域怎么分:地图区、图例区,以及这一帧的投影
#[derive(Clone, Copy)]
pub struct Panels {
    pub map: Rect,
    pub legend: Rect,
    pub proj: Proj,
}

/// 画面高度只有 1 行时状态栏就没了,别把地图也吃掉
fn body_of(area: Rect) -> Rect {
    let status = 1.min(area.height);
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(status),
    }
}

/// 一行的物理高度相当于几列:场景没给或者给了个坏值时按 1 算,免得后面除出 NaN
fn aspect_of(sc: &Scene) -> f64 {
    if sc.aspect.is_finite() && sc.aspect > 0.0 {
        sc.aspect
    } else {
        1.0
    }
}

/// 铺满整个区域时,每列对应多少画布单位
pub fn fit_scale(sc: &Scene, w: u16, h: u16) -> f64 {
    let (wd, hd) = sc.size();
    let cols = (w as f64 - 4.0).max(8.0);
    let rows = (h as f64 - 2.0).max(4.0) * aspect_of(sc);
    (wd / cols).max(hd / rows)
}

/// 把视图中心按**屏幕格**平移(dcols 列、drows 行,正数向右/向下),返回新的画布坐标中心.
/// 滚轮平移就靠它:先换算中心的格坐标,挪几格,再换回画布坐标
pub fn pan_center(proj: &Proj, from: (f64, f64), dcols: i32, drows: i32) -> (f64, f64) {
    let (x, y) = proj.cell(from);
    let (nx, ny) = (x + dcols, y + drows);
    let wx = proj.cx + (nx - proj.x0) as f64 * proj.scale;
    let wy = if proj.flip_y {
        proj.cy - (ny - proj.y0) as f64 * proj.scale * proj.aspect
    } else {
        proj.cy + (ny - proj.y0) as f64 * proj.scale * proj.aspect
    };
    (wx, wy)
}

/// 图例占的宽度(放不下就是 0).只算有名字的分组:没名字的分组本来也不画
fn legend_width(g: &Graph, area_w: u16, lang: Lang) -> u16 {
    let text = g
        .groups
        .iter()
        .filter(|x| !x.label.trim().is_empty())
        .map(|x| width(&x.label))
        .chain([
            width(lang.pick("分组", "groups")),
            width(lang.pick("○ 节点", "○ node")),
            width(lang.pick("◎ 枢纽", "◎ hub")),
            width(lang.pick("> 有向边", "> directed")),
        ])
        .max()
        .unwrap_or(0) as u16;
    let need = text + 8;
    if area_w as u32 > need as u32 * 2 + 40 {
        need
    } else {
        0
    }
}

/// 这一帧的区域划分:地图区、图例区,以及按视图中心/缩放算出来的投影
pub fn panels(g: &Graph, sc: &Scene, area: Rect, view: &View) -> Panels {
    let body = body_of(area);
    let legend_w = legend_width(g, body.width, view.lang);
    let map = Rect {
        width: body.width.saturating_sub(legend_w),
        ..body
    };
    let legend = if legend_w > 2 {
        Rect::new(body.x + body.width - legend_w + 2, body.y, legend_w - 2, body.height)
    } else {
        Rect::new(body.x + body.width, body.y, 0, 0)
    };
    let aspect = aspect_of(sc);
    let scale = fit_scale(sc, map.width, map.height) / view.zoom.max(0.2);
    let (cx, cy) = view.center.unwrap_or_else(|| sc.center());
    // 把中心对齐到字符格:一行的画布高度是 scale*aspect.
    // 相机的小数位置只让整幅图按整数格整体平移(鼠标拖动也是这个路子),
    // 不然有的节点先跳一格、有的后跳,边在移动过程中一直变形
    let (cx, cy) = (
        (cx / scale).round() * scale,
        (cy / (scale * aspect)).round() * scale * aspect,
    );
    Panels {
        map,
        legend,
        proj: Proj {
            scale,
            cx,
            cy,
            aspect,
            flip_y: sc.flip_y,
            x0: map.x as i32 + map.width as i32 / 2,
            y0: map.y as i32 + map.height as i32 / 2,
        },
    }
}

/// 有向边在画面上的箭头:按 (小索引, 大索引, 分组) 找,值表示箭头画在哪几端
/// (画在小索引那一端, 画在大索引那一端).
/// 有向边按有序端点去重,所以 a -> b 与 b -> a 会合到同一个键上,两位各自记一笔
fn head_table(g: &Graph) -> HashMap<(usize, usize, usize), (bool, bool)> {
    let mut out: HashMap<(usize, usize, usize), (bool, bool)> = HashMap::new();
    for e in g.edges.iter().filter(|e| e.directed) {
        let (lo, hi) = if e.a < e.b { (e.a, e.b) } else { (e.b, e.a) };
        let slot = out.entry((lo, hi, e.group)).or_insert((false, false));
        if e.b == lo {
            slot.0 = true; // 箭头在小索引那一端
        } else {
            slot.1 = true;
        }
    }
    out
}

/// 按强度把颜色往红里拉:t = 0 是原色,t = 1 是纯红.
/// view.pulse 就是这里的 t(曲线由主循环按时间算好),所以这里只做插值
fn pulse_color(c: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::Rgb(
        mix(c.0, BLINK_RED.0),
        mix(c.1, BLINK_RED.1),
        mix(c.2, BLINK_RED.2),
    )
}

/// 压暗用的灰
const DIM_COLOR: Color = Color::Rgb(66, 74, 96);
/// 亮白:枢纽与光标节点平时用的那个
const BRIGHT: (u8, u8, u8) = (246, 248, 252);
/// 闪到最红的那一档
const BLINK_RED: (u8, u8, u8) = (255, 62, 56);
/// 普通节点名的灰
const LABEL: (u8, u8, u8) = (150, 158, 180);
/// 浮层与提示用的底色
const PANEL_BG: (u8, u8, u8) = (24, 28, 44);

/// 节点字形:枢纽是靶心,普通节点是空心圆
const GLYPH_HUB: char = '◎';
const GLYPH_NODE: char = '○';
/// 光标那一个画成实心的,一眼就能认出"我在这儿"
const GLYPH_HUB_CURSOR: char = '◉';
const GLYPH_NODE_CURSOR: char = '●';

fn bright_style() -> Style {
    Style::default()
        .fg(Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2))
        .add_modifier(Modifier::BOLD)
}

fn dark_style() -> Style {
    Style::default().fg(Color::Rgb(70, 78, 100))
}

fn dim_style() -> Style {
    Style::default().fg(DIM_COLOR)
}

fn panel_bg() -> Style {
    Style::default().bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2))
}

/// 画一帧.浮层(候选列表)在最上面,画面本身在 render_frame 里
pub fn render(buf: &mut Buffer, area: Rect, g: &Graph, sc: &Scene, view: &View) -> Panels {
    let p = render_frame(buf, area, g, sc, view);
    if !view.help {
        render_choice(buf, area, g, view);
    }
    p
}

/// 画面本身:地图 + 标签 + 图例 + 状态行
fn render_frame(buf: &mut Buffer, area: Rect, g: &Graph, sc: &Scene, view: &View) -> Panels {
    let p = panels(g, sc, area, view);
    if view.help {
        render_help(buf, area, help_keys(view.lang), view.lang);
        return p;
    }
    if area.width == 0 || area.height == 0 || g.nodes.is_empty() {
        return p;
    }
    let (map, proj) = (p.map, p.proj);
    let cursor = view.cursor.min(g.nodes.len() - 1);
    // 颜色冲突时谁说了算:聚焦了某几个分组就一直归它们(光标挪到别的分组上也不变),
    // 没聚焦时才是光标节点的那几个分组
    let mut priority = vec![false; g.groups.len()];
    let who = if view.focus.is_empty() {
        &g.node_groups[cursor]
    } else {
        &view.focus
    };
    for &gr in who {
        if gr < priority.len() {
            priority[gr] = true;
        }
    }
    // 聚焦:只留这几个分组的颜色,别的压暗
    let focus: u64 = view
        .focus
        .iter()
        .filter(|gr| **gr < 64)
        .fold(0u64, |m, gr| m | (1 << gr));
    let mut cv = Canvas::new(
        (map.x as i32, map.y as i32, map.width as i32, map.height as i32),
        priority,
        focus,
    );
    // 先记下所有节点的格子:画边时要避开,免得从节点身上穿过去
    for p0 in sc.pos.iter().take(g.nodes.len()) {
        let c = proj.cell(*p0);
        if cv.idx(c.0, c.1).is_some() {
            cv.node_cells.insert(c);
        }
    }
    // 1. 连边:按分组一条条走,拐弯由方向位合成 ╭╮╯╰
    let heads = head_table(g);
    for (gr, path) in g.series() {
        let cells: Vec<(i32, i32)> = path.iter().map(|&n| proj.cell(sc.pos[n])).collect();
        for k in 0..cells.len().saturating_sub(1) {
            // 下一个节点决定这一段该"竖着进"还是"横着进"
            let arrive = if k + 2 < cells.len() {
                arrive_hor(cells[k], cells[k + 1], cells[k + 2])
            } else {
                None
            };
            // 有向边:箭头画在箭头那一端(series 里的先后顺序不一定就是箭头方向).
            // 同一个分组里两个方向都有时,这一段两端各画一个
            let (u, v) = (path[k], path[k + 1]);
            let (lo, hi) = if u < v { (u, v) } else { (v, u) };
            let heads = heads
                .get(&(lo, hi, *gr))
                .map(|&(at_lo, at_hi)| {
                    if u == lo {
                        (at_lo, at_hi)
                    } else {
                        (at_hi, at_lo)
                    }
                })
                .unwrap_or((false, false));
            cv.edge(
                cells[k],
                cells[k + 1],
                *gr,
                g.group_color(*gr),
                arrive,
                heads,
            );
        }
    }
    // 2. 节点:一格也只有一个颜色,同样让光标那条分组先挑
    for i in 0..g.nodes.len() {
        let (x, y) = proj.cell(sc.pos[i]);
        let groups = &g.node_groups[i];
        let gr = groups
            .iter()
            .copied()
            .find(|gr| *gr < cv.priority.len() && cv.priority[*gr])
            .or_else(|| groups.first().copied());
        let mask = groups
            .iter()
            .filter(|gr| **gr < 64)
            .fold(0u64, |m, gr| m | (1 << gr));
        let fg = gr.map(|gr| g.group_color(gr)).unwrap_or(BRIGHT);
        cv.node_at(x, y, g.hub(i), mask, gr, fg);
    }
    let cursor_cell = proj.cell(sc.pos[cursor]);
    blit(buf, &cv, bright_style(), Some(cursor_cell), view.pulse);
    // 间距够宽(相邻节点拉开到 4 格以上)就自动把节点名标出来,
    // 不然图上一堆小圈看不出哪个是哪个
    let labels = view.labels || median_edge_cells(g, sc, &proj) >= 4.5;
    // 3. 节点名标注(画在最上层,只写在空位上):端点一直标,间距宽了或按了 b 就全标.
    //    顺序上先给"端点/枢纽"找位置,剩下的节点再捡空位
    let mut order: Vec<usize> = (0..g.nodes.len()).collect();
    order.sort_by_key(|&i| {
        let terminus = g.adj[i].len() == 1;
        !(g.hub(i) || terminus)
    });
    let mut used: HashSet<(i32, i32)> = HashSet::new();
    for i in order {
        let terminus = g.adj[i].len() == 1;
        if !labels && !terminus {
            continue;
        }
        let (x, y) = proj.cell(sc.pos[i]);
        let text = &g.nodes[i].label;
        let w = width(text) as i32;
        if text.is_empty() || w == 0 {
            continue;
        }
        // 四个候选位:右边、左边、上面居中、下面居中
        let mut slots = [
            (x + 2, y),
            (x - w - 1, y),
            (x - w / 2, y - 1),
            (x - w / 2, y + 1),
        ];
        // 端点:先试"背着邻居"的那个方向,标签就像从边的末端伸出去
        if terminus {
            if let Some(&(nbr, _)) = g.adj[i].first() {
                let n = proj.cell(sc.pos[nbr]);
                let (dx, dy) = ((x - n.0).signum(), (y - n.1).signum());
                slots.sort_by_key(|(lx, ly)| {
                    let (ox, oy) = ((lx - x).signum(), (ly - y).signum());
                    // 与"背离邻居"方向一致的最靠前
                    (ox != dx || oy != dy) as u8
                });
            }
        }
        let fits = |lx: i32, ly: i32| -> bool {
            if lx < map.x as i32
                || ly < map.y as i32
                || lx + w > (map.x + map.width) as i32
                || ly >= (map.y + map.height) as i32
            {
                return false;
            }
            !(0..w).any(|k| cv.at(lx + k, ly) != 0 || used.contains(&(lx + k, ly)))
        };
        let Some(&(lx, ly)) = slots.iter().find(|&&(lx, ly)| fits(lx, ly)) else {
            continue;
        };
        // 占住这一行和上下各一行:标签之间留出空当,不糊成一片
        for k in 0..w + 1 {
            used.insert((lx + k, ly));
            used.insert((lx + k, ly + 1));
            used.insert((lx + k, ly - 1));
        }
        let on_focus = focus == 0
            || g.node_groups[i]
                .iter()
                .any(|gr| *gr < 64 && focus & (1 << gr) != 0);
        let style = if !on_focus {
            dim_style()
        } else if g.hub(i) || terminus {
            bright_style()
        } else {
            Style::default().fg(Color::Rgb(LABEL.0, LABEL.1, LABEL.2))
        };
        put(buf, lx as u16, ly as u16, text, style);
    }
    // 4. 图例:分组色块 + 名字,再说明三种字形
    if p.legend.width > 0 {
        let lx = p.legend.x;
        let lit = |gr: usize| focus == 0 || (gr < 64 && focus & (1 << gr) != 0);
        let shown: Vec<usize> = (0..g.groups.len())
            .filter(|&gr| !g.groups[gr].label.trim().is_empty())
            .collect();
        let rows = shown.len() as u16 + 5;
        let mut ly = p.legend.y + p.legend.height.saturating_sub(rows) / 2;
        put(buf, lx, ly, view.lang.pick("分组", "groups"), dark_style());
        ly += 1;
        for &gr in &shown {
            let (r, gg, b) = g.group_color(gr);
            let style = if lit(gr) {
                Style::default().fg(Color::Rgb(r, gg, b))
            } else {
                dim_style()
            };
            put(buf, lx, ly, &format!("── {}", g.groups[gr].label), style);
            ly += 1;
        }
        ly += 1;
        let plain = if focus != 0 { dim_style() } else { dark_style() };
        put(buf, lx, ly, view.lang.pick("○ 节点", "○ node"), plain);
        put(buf, lx, ly + 1, view.lang.pick("◎ 枢纽", "◎ hub"), bright_style());
        put(
            buf,
            lx,
            ly + 2,
            view.lang.pick("> 有向边", "> directed"),
            plain,
        );
    }
    // 5. 状态行(搜索中就是输入行)
    let status_y = area.y + area.height - 1;
    if let Some(q) = &view.query {
        let prompt = format!("/{q}");
        put(
            buf,
            area.x,
            status_y,
            &prompt,
            Style::default().fg(Color::Rgb(238, 226, 104)),
        );
        let x = area.x + width(&prompt) as u16;
        let what = if q.trim().is_empty() {
            view.lang
                .pick(
                    "节点名 / 别名(比如 wlgc),或分组号",
                    "node name / alias (e.g. wlgc), or a group number",
                )
                .to_string()
        } else if q.trim().chars().all(|c| c.is_ascii_digit()) {
            let hit_groups = crate::search::groups(g, q.trim());
            if hit_groups.is_empty() {
                view.lang.pick("没有这个分组", "no such group").to_string()
            } else {
                let names = hit_groups
                    .iter()
                    .map(|gr| g.groups[*gr].label.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let n = view.hits.len();
                view.lang.pick_owned(
                    format!("聚焦 {names}:{n} 个节点"),
                    format!("focus {names}: {n} nodes"),
                )
            }
        } else if view.hits.is_empty() {
            view.lang.pick("没找到", "no match").to_string()
        } else {
            let at = view.hit_at.min(view.hits.len() - 1);
            let cur = view.hits[at];
            let more = if view.hits.len() > 1 {
                view.lang.pick(",n 下一个", ", n for next")
            } else {
                ""
            };
            view.lang.pick_owned(
                format!(
                    "{} 处匹配:{}{more}",
                    view.hits.len(),
                    g.nodes[cur].label
                ),
                format!(
                    "{} match(es): {}{more}",
                    view.hits.len(),
                    g.nodes[cur].label
                ),
            )
        };
        put(buf, x + 2, status_y, &what, dark_style());
        let tip = view
            .lang
            .pick("Enter 确定  Esc 取消", "Enter to confirm, Esc to cancel");
        let tw = width(tip) as u16;
        if area.width > tw + 4 {
            put(buf, area.x + area.width - tw, status_y, tip, dark_style());
        }
        return p;
    }
    let mut x = area.x;
    // ":" 命令行与命令回显是操作反馈,必须看得见;默认那串键位提示放不下就不画.
    // 所以先算提示行要多宽,再决定左边那些次要信息什么时候停手
    let hint_is_note = view.cmd.is_some() || view.note.is_some();
    let hint = hint_text(view);
    let hw = width(&hint) as u16;
    let limit = if hint_is_note {
        area.x + area.width - (hw + 4).min(area.width / 2 + 8)
    } else {
        area.x + area.width
    };
    x += draw(
        buf,
        x,
        status_y,
        &view.lang.pick_owned(
            format!(
                "节点 {}  边 {}  分组 {}  ",
                g.nodes.len(),
                g.edges.len(),
                g.groups.len()
            ),
            format!(
                "{} nodes  {} edges  {} groups  ",
                g.nodes.len(),
                g.edges.len(),
                g.groups.len()
            ),
        ),
        dark_style(),
    );
    x += draw(buf, x, status_y, view.layout.label(view.lang), bright_style());
    // 投影之后有几个节点跟别人挤在同一格里:窗口装不下,或者输入坐标本身就重合时,
    // 这是用户唯一能得到的提示 —— 不然画面上就是默默少了几个节点
    let stacked = {
        let mut seen = std::collections::HashSet::new();
        let mut bad = 0;
        for q in &sc.pos {
            let (cx, cy) = p.proj.cell(*q);
            let inside = cx >= p.map.x as i32
                && cx < (p.map.x + p.map.width) as i32
                && cy >= p.map.y as i32
                && cy < (p.map.y + p.map.height) as i32;
            if inside && !seen.insert((cx, cy)) {
                bad += 1;
            }
        }
        bad
    };
    if stacked > 0 {
        x += draw(
            buf,
            x,
            status_y,
            &view
                .lang
                .pick_owned(format!("  挤 {stacked}"), format!("  {stacked} stacked")),
            Style::default().fg(Color::Rgb(238, 226, 104)),
        );
    }
    x += draw(buf, x, status_y, view.lang.pick("  光标 ", "  cursor "), dark_style());
    x += draw(buf, x, status_y, &g.nodes[cursor].label, bright_style());
    for gr in &g.node_groups[cursor] {
        let (r, gg, b) = g.group_color(*gr);
        let label = &g.groups[*gr].label;
        if label.is_empty() || x >= limit {
            continue;
        }
        x += draw(buf, x, status_y, "  ", dark_style());
        x += draw(buf, x, status_y, label, Style::default().fg(Color::Rgb(r, gg, b)));
    }
    if !view.focus.is_empty() && x < limit {
        let names = view
            .focus
            .iter()
            .filter(|gr| **gr < g.groups.len())
            .map(|gr| g.groups[*gr].label.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        x += draw(
            buf,
            x,
            status_y,
            &view.lang.pick_owned(format!("  聚焦 {names}"), format!("  focus {names}")),
            dark_style(),
        );
    }
    // 已经跳过的搜索命中:输入行没了,命中数还留着,n / N 还能接着翻
    if !view.hits.is_empty() && x < limit {
        let at = view.hit_at.min(view.hits.len() - 1) + 1;
        x += draw(
            buf,
            x,
            status_y,
            &view.lang.pick_owned(
                format!("  命中 {at}/{}", view.hits.len()),
                format!("  hit {at}/{}", view.hits.len()),
            ),
            dark_style(),
        );
    }
    let hint = hint_text(view);
    let hw = width(&hint) as u16;
    let right = area.x + area.width;
    if right > hw + 4 && x + 2 < right - hw {
        put(buf, right - hw, status_y, &hint, dark_style());
    } else if hint_is_note {
        // 提示行放不下:从左边的信息后面接上,截到右边界
        let room = right.saturating_sub(x + 2) as usize;
        if room >= 8 {
            let text = truncate_to(&hint, room);
            put(buf, x + 2, status_y, &text, dark_style());
        }
    }
    p
}

/// 状态行右边那串提示:":" 命令行 > 命令回显 > 键位提示
fn hint_text(view: &View) -> String {
    if let Some(c) = &view.cmd {
        return format!(":{c}");
    }
    if let Some(n) = &view.note {
        return n.clone();
    }
    view.lang.pick_owned(
        format!(
            "x{:.1}  hjkl 移动  +/- 缩放  空格 居中  0 复位  / 搜索  b 节点名  r 布局  ? 键位  q 退出",
            view.zoom
        ),
        format!(
            "x{:.1}  hjkl move  +/- zoom  space centre  0 reset  / search  b labels  r layout  ? keys  q quit",
            view.zoom
        ),
    )
}

/// 屏幕格上的节点(点哪儿跳哪儿):把地图区里离点击点最近的那个节点给出来.
/// 不设"最大距离":点哪儿都会把光标挪过去,单击必定有反应
pub fn node_at(sc: &Scene, p: &Panels, area: Rect, col: u16, row: u16) -> Option<usize> {
    let (c, r) = (col as i32, row as i32);
    let inside = |a: Rect| {
        c >= a.x as i32 && c < (a.x + a.width) as i32 && r >= a.y as i32 && r < (a.y + a.height) as i32
    };
    // 画面之外(比如状态行)的点击不算,图例上的也不算
    if !inside(area) || !inside(p.map) {
        return None;
    }
    let mut best: Option<(i64, usize)> = None;
    for (i, q) in sc.pos.iter().enumerate() {
        let (x, y) = p.proj.cell(*q);
        let d = ((x - c) as i64).pow(2) + ((y - r) as i64).pow(2);
        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

/// 键位速查:中文
const HELP_KEYS_ZH: &[(&str, &str)] = &[
    ("h j k l / 方向键", "往那个方向挑最近、最正的那个节点(5l = 连挪 5 个)"),
    ("H K / J L", "只走图上相邻的那一个(这个分组的上一段 / 下一段)"),
    ("Ctrl-d / Ctrl-u", "半屏往下 / 往上跳一个节点(远距离旅行)"),
    ("Ctrl-o / Ctrl-i", "回到上一个 / 下一个跳过的节点"),
    ("n / N", "下一个 / 上一个搜索命中"),
    ("/", "搜节点名、别名、首字母;纯数字 = 聚焦那个分组"),
    ("V", "聚焦当前节点所在的分组(再按一次取消)"),
    ("f", "把聚焦的分组(没聚焦就是当前节点的分组)铺满画面"),
    ("z / 空格", "把当前节点挪到画面正中"),
    ("+ - / 0", "放大 / 缩小 / 复位"),
    ("b", "开关节点名标注"),
    ("r", "循环切换布局"),
    ("鼠标左键", "点哪个节点就跳到哪个节点;拖动平移,滚轮缩放,Ctrl+滚轮平移"),
    (":", "命令行:layout <名字> 换布局,cycle <倍数> 同组档位允许远多少,side <倍数> 横向偏斜算几倍距离,lang <zh|en> 界面语言"),
    ("?", "键位速查(任意键关掉)"),
    ("q / esc", "退出"),
];

/// 键位速查:英文
const HELP_KEYS_EN: &[(&str, &str)] = &[
    ("h j k l / arrows", "nearest node on screen in that direction (5l = move 5 nodes)"),
    ("H K / J L", "one node along the group (previous / next segment)"),
    ("Ctrl-d / Ctrl-u", "jump half a screen down / up (long trips)"),
    ("Ctrl-o / Ctrl-i", "back to the previous / next node you jumped to"),
    ("n / N", "next / previous search hit"),
    ("/", "search name / alias / initials; a bare number focuses that group"),
    ("V", "focus the group(s) through this node (again to clear)"),
    ("f", "fit the focused group (or this node's group) to the screen"),
    ("z / space", "centre this node"),
    ("+ - / 0", "zoom in / out / reset"),
    ("b", "toggle node labels"),
    ("r", "cycle the layout"),
    ("mouse left", "click a node to jump to it; drag pans, wheel zooms, Ctrl+wheel pans"),
    (":", "command line: layout <name> | side <x> | cycle <x> | lang <zh|en>"),
    ("?", "key reference (any key closes it)"),
    ("q / esc", "quit"),
];

/// 当前语言的键位表
pub fn help_keys(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::Zh => HELP_KEYS_ZH,
        Lang::En => HELP_KEYS_EN,
    }
}

/// 键位速查浮层
pub fn render_help(buf: &mut Buffer, area: Rect, lines: &[(&str, &str)], lang: Lang) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let title = lang.pick("键位速查", "key reference");
    let w = lines
        .iter()
        .map(|(k, v)| width(k) + width(v) + 4)
        .chain([width(title) + 6])
        .max()
        .unwrap_or(30)
        .min(area.width as usize) as u16;
    let h = (lines.len() as u16 + 4).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    // 铺底:把这块刷成面板色,免得和底下的图糊在一起
    for yy in y..(y + h).min(area.y + area.height) {
        for xx in x..(x + w).min(area.x + area.width) {
            buf[(xx, yy)].set_char(' ').set_style(panel_bg());
        }
    }
    let bright = bright_style().patch(panel_bg());
    let dim = Style::default()
        .fg(Color::Rgb(170, 178, 200))
        .bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2));
    put(buf, x + 2, y + 1, title, bright);
    for (i, (k, v)) in lines.iter().enumerate() {
        let yy = y + 2 + i as u16;
        if yy >= area.y + area.height {
            break;
        }
        put(buf, x + 2, yy, k, bright);
        put(buf, x + 2 + width(k).max(6) as u16 + 2, yy, v, dim);
    }
    put(
        buf,
        x + 2,
        y + h.saturating_sub(2),
        lang.pick("任意键关掉", "any key closes this"),
        dim,
    );
}

/// "沿线走一个"的候选浮层:列出 (节点, 分组),高亮 view.choice.at.
/// 没在挑候选就什么都不画
pub fn render_choice(buf: &mut Buffer, area: Rect, g: &Graph, view: &View) {
    let Some(choice) = &view.choice else { return };
    if choice.items.is_empty() || area.width == 0 || area.height == 0 {
        return;
    }
    let title = view.lang.pick("沿哪个分组走?", "which group to step along?");
    let hint = view
        .lang
        .pick("数字 / j k 选,回车走,esc 取消", "digits / j k to pick, Enter to go, Esc to cancel");
    let texts: Vec<String> = choice
        .items
        .iter()
        .enumerate()
        .map(|(i, (node, _))| {
            let mark = if i == choice.at { ">" } else { " " };
            let name = g
                .nodes
                .get(*node)
                .map(|n| n.label.as_str())
                .unwrap_or("");
            format!("{mark} {} {}", i + 1, name)
        })
        .collect();
    let w = texts
        .iter()
        .map(|t| width(t))
        .chain([
            width(title),
            width(hint),
            choice
                .items
                .iter()
                .map(|(_, gr)| {
                    g.groups
                        .get(*gr)
                        .map(|x| width(&x.label) + 8)
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0),
        ])
        .max()
        .unwrap_or(20)
        .min(area.width as usize) as u16;
    let h = (texts.len() as u16 + 4).min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    for yy in y..(y + h).min(area.y + area.height) {
        for xx in x..(x + w).min(area.x + area.width) {
            buf[(xx, yy)].set_char(' ').set_style(panel_bg());
        }
    }
    let bright = bright_style().patch(panel_bg());
    let dim = Style::default()
        .fg(Color::Rgb(170, 178, 200))
        .bg(Color::Rgb(PANEL_BG.0, PANEL_BG.1, PANEL_BG.2));
    put(buf, x + 2, y + 1, title, bright);
    for (i, t) in texts.iter().enumerate() {
        let yy = y + 2 + i as u16;
        if yy >= y + h.saturating_sub(1) || yy >= area.y + area.height {
            break;
        }
        let style = if i == choice.at { bright } else { dim };
        put(buf, x + 2, yy, t, style);
        // 右边标一下是哪一组带过来的
        if let Some((_, gr)) = choice.items.get(i) {
            let label = g.groups.get(*gr).map(|x| x.label.as_str()).unwrap_or("");
            if !label.is_empty() {
                let lx = (x + w).saturating_sub(2 + width(label) as u16);
                if lx > x.saturating_add(width(t) as u16) {
                    put(buf, lx, yy, label, dim);
                }
            }
        }
    }
    put(buf, x + 2, y + h.saturating_sub(2), hint, dim);
}

/// 把画布扫一遍落到 ratatui buffer 上
fn blit(buf: &mut Buffer, cv: &Canvas, bright: Style, cursor: Option<(i32, i32)>, pulse: f64) {
    for y in cv.y0..cv.y0 + cv.h {
        for x in cv.x0..cv.x0 + cv.w {
            let Some(i) = cv.idx(x, y) else { continue };
            if cv.node[i] == 0 && cv.mask[i] == 0 {
                continue;
            }
            let (r, gg, b) = cv.fg[i];
            let mut ch = match cv.node[i] {
                2 => GLYPH_HUB,
                1 => GLYPH_NODE,
                // 非节点处:如果别的分组也打这儿过,就按"占住这一格的那一组"自己的形状画,
                // 后来的那一组断一格 —— 不然满屏 ┼ ├ ┬,看起来到处是枢纽,其实那儿什么都没有
                _ if cv.mask[i].count_ones() >= 3 && cv.owner_mask[i] != cv.mask[i] => {
                    box_char(cv.owner_mask[i])
                }
                _ => box_char(cv.mask[i]),
            };
            // 有向边的箭头盖在笔画上
            if cv.node[i] == 0 {
                if let Some(a) = cv.arrow[i] {
                    ch = a;
                }
            }
            let mut style = Style::default().fg(Color::Rgb(r, gg, b));
            // 枢纽平时是亮白的,一眼能认出来;但如果它也在光标那条分组上,
            // 就用那一组的颜色,这样顺着颜色能把整条路径连起来看
            if cv.node[i] == 2 && !cv.mine[i] {
                style = bright;
            }
            // 聚焦某几个分组时,别的分组(节点也好笔画也好)全压暗,
            // 只留聚焦那几个分组的颜色
            let dimmed = if cv.focus == 0 {
                false
            } else if cv.node[i] != 0 {
                // 节点格:看这一个节点自己在不在聚焦的分组上(别的分组只是从这儿路过)
                cv.node_groups[i] & cv.focus == 0
            } else {
                cv.groups_here[i] & cv.focus == 0
            };
            if dimmed {
                style = dim_style();
            }
            if cursor == Some((x, y)) {
                // 当前节点:实心字形 + 平时那个亮白 <-> 红色,按呼吸节奏插值.
                // 背景高亮和加粗保持不变,这样闪到哪一档都看得见它在哪
                ch = match cv.node[i] {
                    2 => GLYPH_HUB_CURSOR,
                    1 => GLYPH_NODE_CURSOR,
                    _ => ch,
                };
                style = Style::default()
                    .fg(pulse_color(BRIGHT, pulse))
                    .bg(Color::Rgb(46, 56, 96))
                    .add_modifier(Modifier::BOLD);
            }
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}

/// 相邻两个节点在屏幕上大概隔几格(用来决定要不要自动标节点名)
fn median_edge_cells(g: &Graph, sc: &Scene, proj: &Proj) -> f64 {
    if g.edges.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f64> = g
        .edges
        .iter()
        .map(|e| {
            let a = proj.cell(sc.pos[e.a]);
            let b = proj.cell(sc.pos[e.b]);
            (((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64).sqrt()
        })
        .collect();
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// 写一段文字,返回实际占用的列数
fn draw(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) -> u16 {
    if x >= buf.area.width || y >= buf.area.height {
        return 0;
    }
    let max = (buf.area.width - x) as usize;
    let s = truncate_to(text, max);
    let w = width(&s) as u16;
    buf.set_string(x, y, &s, style);
    w
}

/// 写一段文字,超出画面就丢掉(bounds 一律挡住,别让浮层把自己写出界)
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if x >= buf.area.width || y >= buf.area.height {
        return;
    }
    let max = (buf.area.width - x) as usize;
    let s = truncate_to(text, max);
    buf.set_string(x, y, &s, style);
}

/// 按显示宽度截断
fn truncate_to(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Choice;

    /// 十字交叉:一个分组横着、一个分组竖着,交点在格子中央(不是节点)
    fn crossing() -> Graph {
        let mut b = Graph::builder();
        let a1 = b.node("甲", Some((0.0, 0.0)));
        let a2 = b.node("乙", Some((2000.0, 0.0)));
        let b1 = b.node("丙", Some((1000.0, 1000.0)));
        let b2 = b.node("丁", Some((1000.0, -1000.0)));
        let gx = b.group("东西", Some((255, 0, 0)));
        let gy = b.group("南北", Some((0, 255, 0)));
        b.edge(a1, a2, gx, false);
        b.edge(b1, b2, gy, false);
        b.build().expect("样例图")
    }

    /// 名字长、又挤在一小片地方:标注得避开节点和彼此
    fn crowded() -> Graph {
        let mut b = Graph::builder();
        let gr = b.group("网格", Some((120, 200, 255)));
        let mut n = Vec::new();
        for k in 0..6 {
            let x = (k % 3) as f64 * 300.0;
            let y = (k / 3) as f64 * 300.0;
            n.push(b.node(format!("很长的名字{k}"), Some((x, y))));
        }
        for (u, v) in [(0, 1), (1, 2), (3, 4), (4, 5), (0, 3), (1, 4), (2, 5)] {
            b.edge(n[u], n[v], gr, false);
        }
        b.build().expect("样例图")
    }

    /// 输入坐标那套(y 向上)的投影
    fn scene_of(g: &Graph) -> Scene {
        Scene::from_pos((0..g.nodes.len()).map(|i| g.pos(i)).collect(), 2.0, true)
    }

    /// 把画面读成文本:全角字占的第二格要跨过去
    fn screen(buf: &Buffer) -> Vec<String> {
        let a = buf.area;
        (a.y..a.y + a.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = a.x;
                while x < a.x + a.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += width(sym).max(1) as u16;
                }
                line
            })
            .collect()
    }

    /// 非节点处的交叉:交叉格照"先画的那一组"自己的形状画(直通的那一笔),
    /// 另一组在这里断一格 —— 不能画成 ┼,不然看起来像这里有枢纽.
    /// 这个形状**不跟光标走**:挪光标只换颜色,不然整段边的字形会跟着光标变,像在变形.
    /// 颜色仍旧跟着光标那一组,从当前节点顺着颜色就能找到自己那一组
    #[test]
    fn 交叉格只画先到那条线的形状() {
        let g = crossing();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 24);
        let p = panels(&g, &sc, area, &View::default());
        let (x, y) = (p.proj.cell((1000.0, 0.0)).0, p.proj.cell((0.0, 0.0)).1);
        let mut shapes = Vec::new();
        for (cursor, want) in [(0usize, Color::Rgb(255, 0, 0)), (2usize, Color::Rgb(0, 255, 0))] {
            let mut view = View::default();
            view.cursor = cursor;
            let mut buf = Buffer::empty(area);
            render(&mut buf, area, &g, &sc, &view);
            let cell = &buf[(x as u16, y as u16)];
            assert_eq!(cell.symbol(), "─", "交叉格该是先画那一组的直笔画");
            assert_eq!(
                cell.fg, want,
                "光标在节点 {cursor} 上时,交叉格该用他那一组的颜色"
            );
            shapes.push(cell.symbol().to_string());
        }
        assert_eq!(shapes[0], shapes[1], "挪光标不该改交叉格的字形");
    }

    /// 整幅图要能塞进画布:四个角上的节点都得在地图区里
    #[test]
    fn 整幅图能塞进画布() {
        let g = crossing();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 24);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &g, &sc, &View::default());
        for i in 0..g.nodes.len() {
            let (x, y) = p.proj.cell(sc.pos[i]);
            assert!(
                x >= p.map.x as i32 && x < (p.map.x + p.map.width) as i32,
                "节点 {} 出了画面",
                g.nodes[i].label
            );
            assert!(
                y >= p.map.y as i32 && y < (p.map.y + p.map.height) as i32,
                "节点 {} 出了画面",
                g.nodes[i].label
            );
        }
    }

    /// 节点名标注只写在空位上:标出来之后,每个节点自己的那一格还是节点字形
    #[test]
    fn 节点名标注不压节点() {
        let g = crowded();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 40, 14);
        let mut view = View::default();
        view.labels = true;
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &g, &sc, &view);
        for i in 0..g.nodes.len() {
            let (x, y) = p.proj.cell(sc.pos[i]);
            let sym = buf[(x as u16, y as u16)].symbol();
            assert!(
                matches!(sym, "○" | "◎" | "●" | "◉"),
                "节点 {} 那一格被标成了 {sym:?}",
                g.nodes[i].label
            );
        }
        let text = screen(&buf).join("\n");
        let shown = (0..g.nodes.len()).any(|i| text.contains(&g.nodes[i].label));
        assert!(shown, "地方够宽,总得标出一个名字来:\n{text}");
    }

    /// 有向边:箭头画在终点的前一格,朝着终点,颜色跟着这一组
    #[test]
    fn 有向边箭头画在终点那一格() {
        let arrow_of = |a_first: bool| {
            let mut b = Graph::builder();
            let u = b.node("西", Some((0.0, 0.0)));
            let v = b.node("东", Some((2000.0, 0.0)));
            let gr = b.group("依赖", Some((76, 142, 248)));
            b.edge(if a_first { u } else { v }, if a_first { v } else { u }, gr, true);
            let g = b.build().expect("样例图");
            let sc = scene_of(&g);
            let area = Rect::new(0, 0, 60, 24);
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &g, &sc, &View::default());
            let (xa, ya) = p.proj.cell(sc.pos[0]);
            let (xb, yb) = p.proj.cell(sc.pos[1]);
            assert_eq!(ya, yb, "这是一条直线边");
            // 终点是 sc.pos[1] 的那一端:箭头就在它前面一格
            let (head, tail) = if a_first { (xb, xa) } else { (xa, xb) };
            let d = (head - tail).signum();
            let cell = &buf[((head - d) as u16, yb as u16)];
            (cell.symbol().to_string(), cell.fg)
        };
        let (right, fg) = arrow_of(true);
        assert_eq!(right, ">", "从西往东:箭头朝东");
        assert_eq!(fg, Color::Rgb(76, 142, 248));
        let (left, _) = arrow_of(false);
        assert_eq!(left, "<", "从东往西:箭头朝西");
    }

    /// 同一个分组里 a -> b 与 b -> a 是两条独立的边,画出来这一段两端都有箭头。
    /// 依赖图里的双向依赖(store <-> view)就是样子
    #[test]
    fn 双向依赖两端都有箭头() {
        let mut b = Graph::builder();
        let u = b.node("视图", Some((0.0, 0.0)));
        let v = b.node("状态", Some((2000.0, 0.0)));
        let gr = b.group("依赖", Some((76, 142, 248)));
        b.edge(u, v, gr, true);
        b.edge(v, u, gr, true);
        let g = b.build().expect("样例图");
        assert_eq!(g.edges.len(), 2, "有向边按有序端点去重,两个方向是两条边");
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 24);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &g, &sc, &View::default());
        let (xa, ya) = p.proj.cell(sc.pos[0]);
        let (xb, yb) = p.proj.cell(sc.pos[1]);
        assert_eq!(ya, yb, "这是一条直线边");
        assert_ne!(xa, xb);
        let d = (xb - xa).signum();
        assert_eq!(buf[((xb - d) as u16, yb as u16)].symbol(), ">", "去程指向东");
        assert_eq!(
            buf[((xa + d) as u16, ya as u16)].symbol(),
            "<",
            "回程指向西"
        );
    }

    /// 光标那个节点是实心的,并按 view.pulse 在亮白和红之间插值
    #[test]
    fn 光标节点实心并按脉冲闪红() {
        let g = crossing();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 24);
        let cell_of = |pulse: f64| {
            let mut view = View::default();
            view.cursor = 2; // 丙:在南北那一组上
            view.pulse = pulse;
            let mut buf = Buffer::empty(area);
            let p = render(&mut buf, area, &g, &sc, &view);
            let (x, y) = p.proj.cell(sc.pos[2]);
            let cell = &buf[(x as u16, y as u16)];
            (cell.fg, cell.symbol().to_string())
        };
        let (soft, ch) = cell_of(0.0);
        assert_eq!(ch, "●", "普通节点该画成实心");
        assert_eq!(soft, Color::Rgb(BRIGHT.0, BRIGHT.1, BRIGHT.2));
        let (red, _) = cell_of(1.0);
        assert_eq!(red, Color::Rgb(BLINK_RED.0, BLINK_RED.1, BLINK_RED.2));
        let chan = |c: Color| match c {
            Color::Rgb(r, _, _) => r as i32,
            _ => -1,
        };
        let (half, _) = cell_of(0.5);
        assert!(
            chan(half) > chan(soft) && chan(half) < chan(red),
            "中间的档位该在两头之间"
        );
    }

    /// 图例:有名字的分组画一行色块 + 名字,再说明三种字形;没名字的分组不画
    #[test]
    fn 图例画分组名与字形说明() {
        let mut b = Graph::builder();
        let u = b.node("甲", Some((0.0, 0.0)));
        let v = b.node("乙", Some((3000.0, 0.0)));
        let named = b.group("主线", Some((255, 0, 0)));
        let blank = b.group("", Some((0, 255, 0)));
        b.edge(u, v, named, false);
        b.edge(u, v, blank, false);
        let g = b.build().expect("样例图");
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        let p = render(&mut buf, area, &g, &sc, &View::default());
        assert!(p.legend.width > 0, "这么宽该放得下图例");
        // 全角字占的第二格是空的,拼的时候要跨过去
        let legend = (p.legend.y..p.legend.y + p.legend.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = p.legend.x;
                while x < p.legend.x + p.legend.width {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += width(sym).max(1) as u16;
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(legend.contains("分组"), "图例该有标题:\n{legend}");
        assert!(legend.contains("主线"), "有名字的分组要画出来:\n{legend}");
        assert!(legend.contains("○ 节点") && legend.contains("◎ 枢纽"), "{legend}");
        assert!(legend.contains("> 有向边"), "{legend}");
        let bars = (p.legend.y..p.legend.y + p.legend.height)
            .flat_map(|y| (p.legend.x..p.legend.x + p.legend.width).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(x, y)].symbol() == "─")
            .count();
        assert_eq!(bars, 2, "只有有名字的那一组才画色块");
    }

    /// 状态栏:节点数、边数、分组数、当前布局、光标节点名,而且要跟着界面语言走
    #[test]
    fn 状态栏报规模与光标() {
        let g = crowded();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &View::default());
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("节点 6"), "该报节点数:{last}");
        assert!(last.contains("边 7"), "该报边数:{last}");
        assert!(last.contains("分组 1"), "该报分组数:{last}");
        assert!(last.contains("光标 很长的名字0"), "该报光标节点名:{last}");
        assert!(last.contains("hjkl 移动"), "该报键位提示:{last}");
        // 换英文:状态栏那一套提示文字都该跟着换
        let mut view = View::default();
        view.lang = Lang::En;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("6 nodes"), "{last}");
        assert!(last.contains("cursor "), "{last}");
        assert!(last.contains("hjkl move"), "{last}");
        assert!(!last.contains("光标"), "英文状态栏里不该有中文:{last}");
    }

    /// 投影之后跟别人挤在同一格的节点要说出来:窗口装不下或者输入坐标重合时,
    /// 这是画面上唯一能看出来的线索(不然就是默默少画几个节点)
    #[test]
    fn 挤在一起的节点报数() {
        let mut b = Graph::builder();
        let gr = b.group("线", Some((120, 200, 255)));
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((4.0, 0.0)));
        let d = b.node("c", Some((400.0, 0.0)));
        b.edge(a, c, gr, false);
        b.edge(a, d, gr, false);
        let g = b.build().expect("样例图");
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &View::default());
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("挤 1"), "两个节点落在同一格该报出来:{last}");

        // 画面够大时不该有这条提示
        let area = Rect::new(0, 0, 200, 50);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &View::default());
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(!last.contains("挤"), "画得下就不该报:{last}");

        // 英文界面跟着换(英文更长,给到 52 列才放得下;宽度再小一点 b 就不与 a 重叠了)
        let mut view = View::default();
        view.lang = Lang::En;
        let area = Rect::new(0, 0, 52, 12);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("1 stacked"), "{last}");
    }

    /// 搜索中的输入行:词、命中数、提示都在
    #[test]
    fn 搜索行报命中数() {
        let g = crowded();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 200, 50);
        let mut view = View::default();
        view.query = Some("很长".to_string());
        view.hits = vec![0, 1, 2];
        view.hit_at = 1;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("/很长"), "该显示搜索词:{last}");
        assert!(last.contains("3 处匹配"), "该显示命中数:{last}");
        assert!(last.contains("很长的名字1"), "该显示当前命中:{last}");
        // 命令行缓冲与 note 走同一行
        let mut view = View::default();
        view.cmd = Some("lang en".to_string());
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains(":lang en"), "该显示命令行缓冲:{last}");
        let mut view = View::default();
        view.note = Some("换了布局".to_string());
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let last = screen(&buf).pop().expect("有最后一行");
        assert!(last.contains("换了布局"), "该显示 note:{last}");
    }

    /// 帮助浮层:画的是键位速查,不是地图;英文那份不许留中文
    #[test]
    fn 帮助浮层盖住地图() {
        let g = crowded();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 90, 30);
        let mut view = View::default();
        view.help = true;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let text = screen(&buf).join("\n");
        assert!(text.contains("键位速查"), "该有标题:\n{text}");
        assert!(text.contains("任意键关掉"), "该提示怎么关:\n{text}");
        assert!(text.contains("半屏往下"), "该列出半屏跳:\n{text}");
        assert!(!text.contains('○'), "浮层打开时不该还在画地图:\n{text}");
        let mut view = View::default();
        view.help = true;
        view.lang = Lang::En;
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let text = screen(&buf).join("\n");
        assert!(text.contains("key reference"), "{text}");
        assert!(
            !text.chars().any(|c| !c.is_ascii()),
            "英文速查里不该有非 ASCII:\n{text}"
        );
    }

    /// 候选浮层:列出 (节点, 分组),高亮 view.choice.at
    #[test]
    fn 候选浮层列出节点与分组() {
        let g = crowded();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 20);
        let mut view = View::default();
        view.choice = Some(Choice {
            items: vec![(0, 0), (1, 0)],
            at: 1,
        });
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &g, &sc, &view);
        let text = screen(&buf).join("\n");
        assert!(text.contains("沿哪个分组走?"), "该有标题:\n{text}");
        assert!(text.contains("很长的名字0") && text.contains("很长的名字1"), "{text}");
        assert!(text.contains("> 2 很长的名字1"), "选中的那行要有标记:\n{text}");
        assert!(text.contains("网格"), "该标出是哪一组带过来的:\n{text}");
    }

    /// 点哪个节点就选哪个;状态行和图例上的点击不算
    #[test]
    fn 点哪个节点就选哪个() {
        let g = crossing();
        let sc = scene_of(&g);
        let area = Rect::new(0, 0, 60, 24);
        let p = panels(&g, &sc, area, &View::default());
        for i in 0..g.nodes.len() {
            let (x, y) = p.proj.cell(sc.pos[i]);
            assert_eq!(
                node_at(&sc, &p, area, x as u16, y as u16),
                Some(i),
                "点在 {} 自己的格子上该选它",
                g.nodes[i].label
            );
        }
        assert_eq!(
            node_at(&sc, &p, area, 5, area.height - 1),
            None,
            "状态行上的点击该忽略"
        );
        // 图例上的点击也不算
        let wide = Rect::new(0, 0, 200, 50);
        let pw = panels(&g, &sc, wide, &View::default());
        assert!(pw.legend.width > 0);
        assert_eq!(
            node_at(&sc, &pw, wide, pw.legend.x, pw.legend.y + 1),
            None,
            "图例上的点击该忽略"
        );
        // 空白角落:仍旧有反应,选的是离它最近的那一个节点
        let near = node_at(&sc, &pw, wide, 1, 1).expect("地图区里点哪儿都该有节点");
        let want = (0..sc.pos.len())
            .min_by_key(|&i| {
                let (x, y) = pw.proj.cell(sc.pos[i]);
                ((x - 1) as i64).pow(2) + ((y - 1) as i64).pow(2)
            })
            .unwrap();
        assert_eq!(near, want, "该选离点击点最近的那一个");
    }

    /// 滚轮平移按屏幕格算:挪几格,中心就落在原来那一格加几格上
    #[test]
    fn 平移按屏幕格算() {
        let g = crossing();
        let sc = scene_of(&g);
        let p = panels(&g, &sc, Rect::new(0, 0, 60, 24), &View::default());
        let from = (500.0, 300.0);
        let (c0, r0) = p.proj.cell(from);
        let to = pan_center(&p.proj, from, 3, -2);
        assert_eq!(p.proj.cell(to), (c0 + 3, r0 - 2));
    }

    /// 窗口小到几乎没有:照画不炸,状态行与浮层都不越界
    #[test]
    fn 极小画面也不炸() {
        let g = crossing();
        let sc = scene_of(&g);
        for (w, h) in [(0u16, 0u16), (1, 1), (1, 5), (5, 1), (3, 3), (8, 4), (12, 3)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            render(&mut buf, area, &g, &sc, &View::default());
            let mut view = View::default();
            view.query = Some("甲".to_string());
            view.hits = vec![0];
            render(&mut buf, area, &g, &sc, &view);
            let mut view = View::default();
            view.help = true;
            render(&mut buf, area, &g, &sc, &view);
            let mut view = View::default();
            view.choice = Some(Choice {
                items: vec![(0, 0), (1, 1)],
                at: 9,
            });
            render(&mut buf, area, &g, &sc, &view);
            let p = panels(&g, &sc, area, &View::default());
            let hit = node_at(&sc, &p, area, 0, 0);
            if w == 0 || h <= 1 {
                assert_eq!(hit, None, "{w}x{h} 上没有可点的节点");
            }
        }
    }

    /// 铺满整个区域:缩放够小,整幅图才塞得下
    #[test]
    fn 铺满时画布刚好装下整幅图() {
        let sc = Scene::from_pos(vec![(0.0, 0.0), (1000.0, 400.0)], 1.0, false);
        let (wd, hd) = sc.size();
        let scale = fit_scale(&sc, 60, 24);
        assert!(wd / scale <= 60.0 - 4.0 + 1.0, "横着放得下");
        assert!(hd / (scale * sc.aspect) <= 24.0 - 2.0 + 1.0, "竖着也放得下");
        // 区域越大每列分到的画布单位越少(也就是看得越细)
        assert!(fit_scale(&sc, 200, 60) < scale);
    }
}
