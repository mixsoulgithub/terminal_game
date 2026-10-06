// 渲染入口与画字符的小工具.
// 所有框线只用 ASCII,颜色承担全部视觉效果(和 neon 保持一致).
pub mod battle;
pub mod hud;
pub mod library;
pub mod start;
pub mod mapview;
pub mod menu;
pub mod overlay;
pub mod theme;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::Frame;

use crate::app::{App, Mode};

/// 在缓冲里写一行 ASCII 文本,越界自动截断
pub fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y >= buf.area.height || x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    let s = truncate(text, max);
    buf.set_string(x, y, &s, style);
}

/// 按显示宽度截断(全角字符算两格)
pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

/// 截断后右补空格
pub fn fit(s: &str, width: usize) -> String {
    let t = truncate(s, width);
    let w = display_width(&t);
    if w >= width {
        return t;
    }
    let mut out = t;
    out.push_str(&" ".repeat(width - w));
    out
}

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// 药水名里的 "Potion" 换成 ~,只在放不下时才用
pub fn potion_label(name: &str) -> String {
    name.replace("Potion", "~")
}

/// 卡牌费用记号:数字 / X / -
pub fn cost_label(card: &crate::core::card::CardInstance) -> String {
    match card.cost() {
        crate::core::card::Cost::Fixed(n) => n.to_string(),
        crate::core::card::Cost::X => "X".to_string(),
        crate::core::card::Cost::Unplayable => "-".to_string(),
    }
}

/// 按宽度折行,最多 max_lines 行;放不下的部分用 .. 收尾
pub fn wrap_text(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for word in s.split(' ') {
        let ww = display_width(word);
        if cur_w == 0 && ww > width {
            lines.push(truncate(word, width));
            if lines.len() == max_lines {
                break;
            }
            continue;
        }
        if cur_w + ww + 1 > width && cur_w > 0 {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
            if lines.len() == max_lines {
                break;
            }
        }
        if cur_w > 0 {
            cur.push(' ');
            cur_w += 1;
        }
        cur.push_str(word);
        cur_w += ww;
    }
    if lines.len() < max_lines && !cur.is_empty() {
        lines.push(cur);
    }
    lines.truncate(max_lines);
    lines
}

/// 在缓冲里写一行,右侧补空格到 width(用于选中行的底色铺满)
pub fn put_padded(buf: &mut Buffer, x: u16, y: u16, text: &str, width: usize, style: Style) {
    put(buf, x, y, &fit(text, width), style);
}

/// 方框用的制表符
pub const BOX_H: char = '─';
pub const BOX_V: char = '│';

/// 制表符方框
pub fn draw_box(buf: &mut Buffer, area: Rect, title: &str, style: Style, title_style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let w = area.width as usize;
    let top = format!("┌{}┐", BOX_H.to_string().repeat(w.saturating_sub(2)));
    put(buf, area.x, area.y, &top, style);
    let bottom = format!("└{}┘", BOX_H.to_string().repeat(w.saturating_sub(2)));
    put(buf, area.x, area.y + area.height - 1, &bottom, style);
    for y in area.y + 1..area.y + area.height - 1 {
        put(buf, area.x, y, &BOX_V.to_string(), style);
        put(buf, area.x + area.width - 1, y, &BOX_V.to_string(), style);
    }
    if !title.is_empty() && w > 6 {
        let t = format!(" {} ", truncate(title, w - 6));
        put(buf, area.x + 2, area.y, &t, title_style);
    }
}

/// 水平分隔线
pub fn hline(buf: &mut Buffer, x: u16, y: u16, width: u16, ch: char, style: Style) {
    if width == 0 {
        return;
    }
    let line: String = std::iter::repeat(ch).take(width as usize).collect();
    put(buf, x, y, &line, style);
}

/// 竖直分隔线
pub fn vline(buf: &mut Buffer, x: u16, y: u16, height: u16, ch: char, style: Style) {
    for i in 0..height {
        put(buf, x, y + i, &ch.to_string(), style);
    }
}

/// 居中写一行(按显示宽度居中)
pub fn put_centered_line(buf: &mut Buffer, x: u16, y: u16, w: usize, text: &str, style: Style) {
    put_centered(buf, x, y, w, text, style);
}

/// 居中写一行(按显示宽度居中)
fn put_centered(buf: &mut Buffer, x: u16, y: u16, w: usize, text: &str, style: Style) {
    let tw = display_width(text);
    if tw > w {
        put(buf, x, y, &truncate(text, w), style);
        return;
    }
    put(buf, x + ((w - tw) / 2) as u16, y, text, style);
}

/// 把描述按词切开:数字后面跟 damage/block 时分别上血色/蓝色
fn desc_words(text: &str) -> Vec<(String, Style)> {
    let words: Vec<&str> = text.split(' ').collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let next = words.get(i + 1).map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        let is_num = w.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);
        let color = if is_num && next.starts_with("damage") {
            Some(theme::BLOOD)
        } else if is_num && next.starts_with("block") {
            Some(theme::BLOCK)
        } else {
            None
        };
        let style = match color {
            Some(c) => theme::fg(c),
            None => theme::fg(theme::FG),
        };
        out.push((w.to_string(), style));
    }
    out
}

/// 按宽度把带样式的词折行
fn wrap_words(words: &[(String, Style)], width: usize) -> Vec<Vec<(String, Style)>> {
    let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
    let mut cur: Vec<(String, Style)> = Vec::new();
    let mut cur_w = 0usize;
    for (w, st) in words {
        let ww = display_width(w);
        let extra = if cur.is_empty() { 0 } else { 1 };
        if cur_w + ww + extra > width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        if !cur.is_empty() {
            cur.push((" ".to_string(), theme::fg(theme::FG)));
            cur_w += 1;
        }
        cur.push((w.clone(), *st));
        cur_w += ww;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// 居中写一行带样式的词
fn put_centered_words(buf: &mut Buffer, x: u16, y: u16, w: usize, words: &[(String, Style)]) {
    let len: usize = words.iter().map(|(t, _)| display_width(t)).sum();
    let mut cx = x + (w.saturating_sub(len) / 2) as u16;
    for (t, st) in words {
        put(buf, cx, y, t, *st);
        cx += display_width(t) as u16;
    }
}

/// 卡牌类型记号:攻击 <Attack>、技能 [Skill]、能力 (Power),其余就写名字
fn kind_label(kind: crate::core::card::CardType) -> String {
    use crate::core::card::CardType;
    match kind {
        CardType::Attack => format!("<{}>", kind.name()),
        CardType::Skill => format!("[{}]", kind.name()),
        CardType::Power => format!("({})", kind.name()),
        _ => kind.name().to_string(),
    }
}

/// 一张牌的说明主体:名字、类型、描述都居中(不含费用)
pub fn card_body(buf: &mut Buffer, rect: Rect, card: &crate::core::card::CardInstance) {
    let w = rect.width as usize;
    if w == 0 || rect.height == 0 {
        return;
    }
    let bottom = rect.y + rect.height;
    let mut y = rect.y;
    // 名字居中
    let name_style = Style::default()
        .fg(theme::card_color(card.rarity()))
        .add_modifier(ratatui::style::Modifier::BOLD);
    put_centered(buf, rect.x, y, w, &card.label(), name_style);
    y += 1;
    if y >= bottom {
        return;
    }
    // 类型居中,按类型加不同括号
    put_centered(buf, rect.x, y, w, &kind_label(card.kind()), theme::fg(theme::FG));
    y += 1;
    // 描述居中,按宽度折行;伤害数字上血色、格挡数字上蓝色
    let words = desc_words(&card.display_text());
    let max = (bottom - y) as usize;
    for line in wrap_words(&words, w).into_iter().take(max) {
        if y >= bottom {
            break;
        }
        put_centered_words(buf, rect.x, y, w, &line);
        y += 1;
    }
}

/// 一张牌的完整说明:费用靠左,剩下交给 card_body
pub fn card_desc(buf: &mut Buffer, rect: Rect, card: &crate::core::card::CardInstance) {
    let w = rect.width as usize;
    if w == 0 || rect.height == 0 {
        return;
    }
    // 费用左对齐,写成 (1) 并保持能量色;说明永远不压暗
    let cost_style =
        Style::default().fg(theme::ENERGY).add_modifier(ratatui::style::Modifier::BOLD);
    put(buf, rect.x, rect.y, &format!("({})", cost_label(card)), cost_style);
    card_body(
        buf,
        Rect::new(rect.x, rect.y + 1, rect.width, rect.height.saturating_sub(1)),
        card,
    );
}

/// "列表 + 描述" 的通用切分.
/// 盒子够宽(宽 > 高 * 1.5)时用横线上下分:列表在上、按内容定高(动态缩),描述占剩下的;
/// 否则用竖线左右分:各占一半,两侧各自按自己的宽度折行,不缩.
pub struct Split {
    pub list: Rect,
    pub detail: Rect,
    /// true = 上下分(分隔符 -),false = 左右分(分隔符 |)
    pub horizontal: bool,
}

/// 内容左右各留 1 格,分隔线仍然横跨整个 area.
pub(crate) fn split_list_detail(area: Rect, list_h: u16) -> Split {
    let x = area.x + 1;
    let w = area.width.saturating_sub(2);
    // 够宽(宽 > 高 * 1.5)左右分,否则上下分
    let horizontal = w as u32 <= area.height as u32 * 3 / 2;
    if horizontal {
        let lh = list_h.min(area.height.saturating_sub(2));
        Split {
            list: Rect::new(x, area.y, w, lh),
            detail: Rect::new(
                x,
                area.y + lh + 1,
                w,
                area.height.saturating_sub(lh + 1),
            ),
            horizontal: true,
        }
    } else {
        let lw = w / 2;
        Split {
            list: Rect::new(x, area.y, lw, area.height),
            detail: Rect::new(
                x + lw + 1,
                area.y,
                w.saturating_sub(lw + 1),
                area.height,
            ),
            horizontal: false,
        }
    }
}

/// 盒子内部的横分隔线:两头用 ├ ┤ 顶掉那一行的左右边框.
/// `inner` 是盒子内部(不含边框)的区域.
pub fn hsep(buf: &mut Buffer, inner: Rect, y: u16, style: Style) {
    if inner.width == 0 || inner.x == 0 {
        return;
    }
    put(buf, inner.x - 1, y, "├", style);
    hline(buf, inner.x, y, inner.width, BOX_H, style);
    put(buf, inner.x + inner.width, y, "┤", style);
}

/// 盒子内部的竖分隔线:x 是分隔线所在的列,上下用 ┬ ┴ 顶掉边框
pub fn vsep(buf: &mut Buffer, inner: Rect, x: u16, style: Style) {
    if inner.height == 0 || inner.y == 0 {
        return;
    }
    put(buf, x, inner.y - 1, "┬", style);
    vline(buf, x, inner.y, inner.height, BOX_V, style);
    put(buf, x, inner.y + inner.height, "┴", style);
}

/// 按切分结果画出中间那条分隔线(area 是盒子内部,线会接上盒子边框)
pub fn draw_split(buf: &mut Buffer, area: Rect, split: &Split, style: Style) {
    if split.horizontal {
        hsep(buf, area, split.detail.y.saturating_sub(1), style);
    } else {
        vsep(buf, area, split.detail.x.saturating_sub(1), style);
    }
}


/// 指定切分方向的对半切分:horizontal 为 true 时上下分(-),false 时左右分(|)
pub fn split_two_with(area: Rect, horizontal: bool) -> (Rect, Rect, bool) {
    if horizontal {
        let h = area.height / 2;
        (
            Rect::new(area.x, area.y, area.width, h),
            Rect::new(
                area.x,
                area.y + h + 1,
                area.width,
                area.height.saturating_sub(h + 1),
            ),
            true,
        )
    } else {
        let w = area.width / 2;
        (
            Rect::new(area.x, area.y, w, area.height),
            Rect::new(
                area.x + w + 1,
                area.y,
                area.width.saturating_sub(w + 1),
                area.height,
            ),
            false,
        )
    }
}

/// 一行卡牌列表:费用用能量色,牌名用牌自己的颜色;selected 时整行铺底色
pub fn put_card_line(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    card: &crate::core::card::CardInstance,
    selected: bool,
    dim: bool,
) {
    if w == 0 {
        return;
    }
    let bg = if selected { theme::SEL_BG } else { theme::BG };
    let base = Style::default().bg(bg);
    put_padded(buf, x, y, "", w as usize, base);
    let cost_style = if dim {
        theme::dim().bg(bg)
    } else {
        Style::default().fg(theme::ENERGY).bg(bg)
    };
    let name_style = if dim {
        theme::dim().bg(bg)
    } else {
        Style::default()
            .fg(theme::card_color(card.rarity()))
            .bg(bg)
    };
    let mut cx = x;
    let mut left = w as usize;
    let cost = truncate(&cost_label(card), left);
    put(buf, cx, y, &cost, cost_style);
    let cw = display_width(&cost);
    cx += cw as u16;
    left = left.saturating_sub(cw);
    if left == 0 {
        return;
    }
    put(buf, cx, y, " ", base);
    cx += 1;
    left -= 1;
    let name = truncate(&card.label(), left);
    put(buf, cx, y, &name, name_style);
}

/// 卡牌窗口里的一行
pub enum CardRow {
    /// 分组标题(战斗里各堆的名字)
    Header(String),
    Card {
        card: crate::core::card::CardInstance,
        /// 不可选的行会压暗,光标跳过
        selectable: bool,
    },
}

/// 卡牌窗口:左边(或上面)是卡牌列表,右边(或下面)是选中那张的说明.
/// `after` 有值时(升级),说明那半边再按同一规则对半切成"升级前 / 升级后".
pub fn card_window(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    rows: &[CardRow],
    sel: usize,
    after: Option<&crate::core::card::CardInstance>,
) {
    draw_box(buf, area, title, theme::fg(theme::SEL_FG), theme::fg(theme::INFO));
    if area.width < 4 || area.height < 4 {
        return;
    }
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let split = split_list_detail(inner, rows.len() as u16);
    draw_split(buf, inner, &split, theme::fg(theme::BORDER));
    // 列表
    let list = split.list;
    let lw = list.width;
    if lw == 0 || list.height == 0 {
        return;
    }
    let capacity = list.height as usize;
    let start = if rows.len() > capacity {
        sel.saturating_sub(capacity / 2).min(rows.len() - capacity)
    } else {
        0
    };
    for (i, row) in rows.iter().skip(start).take(capacity).enumerate() {
        let idx = start + i;
        let y = list.y + i as u16;
        match row {
            CardRow::Header(text) => {
                put_padded(buf, list.x, y, text, lw as usize, theme::fg(theme::INFO));
            }
            CardRow::Card { card, selectable } => {
                put_card_line(buf, list.x, y, lw, card, idx == sel, !selectable);
            }
        }
    }
    // 说明
    let Some(CardRow::Card { card, .. }) = rows.get(sel) else {
        return;
    };
    if let Some(after) = after {
        // 升级的"前 / 后"两份说明用和外面相反的方向切,免得两条同样的线并排
        let (first, second, horizontal) = split_two_with(split.detail, !split.horizontal);
        if horizontal {
            hline(
                buf,
                split.detail.x,
                second.y.saturating_sub(1),
                split.detail.width,
                BOX_H,
                theme::fg(theme::BORDER),
            );
        } else {
            vline(
                buf,
                second.x.saturating_sub(1),
                split.detail.y,
                split.detail.height,
                BOX_V,
                theme::fg(theme::BORDER),
            );
        }
        card_desc(buf, first, card);
        card_desc(buf, second, after);
    } else {
        card_desc(buf, split.detail, card);
    }
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let buf = f.buffer_mut();
    buf.set_style(area, Style::default().bg(theme::BG).fg(theme::FG));
    if area.width < 80 || area.height < 24 {
        // 界面按 80x24 起排:更小的终端只给一句提示,不硬挤
        let msg = format!(
            "spire needs at least 80x24 (now {}x{})",
            area.width, area.height
        );
        let x = area.x + (area.width.saturating_sub(display_width(&msg) as u16)) / 2;
        let y = area.y + area.height / 2;
        put(buf, x, y, &msg, theme::fg(theme::WARN));
        return;
    }
    // 开始界面/角色选择/图鉴占满整屏,没有顶栏、遗物行和状态栏
    match app.run.screen {
        crate::core::run::Screen::Title => {
            start::title(buf, area, app);
            return;
        }
        crate::core::run::Screen::CharSelect => {
            start::char_select(buf, area, app);
            return;
        }
        crate::core::run::Screen::Compendium => {
            start::compendium(buf, area, app);
            return;
        }
        crate::core::run::Screen::Library => {
            library::render(buf, area, app);
            return;
        }
        _ => {}
    }
    let hud_area = Rect::new(area.x, area.y, area.width, 1);
    let relic_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let potion_rect = hud::render(buf, hud_area, app);
    relic_bar(buf, relic_area, app);
    if app.run.screen == crate::core::run::Screen::Combat {
        // 遗物行和战斗区之间也隔一条线
        hline(buf, area.x, area.y + 2, area.width, BOX_H, theme::fg(theme::BORDER));
        // 战斗界面自带信息行和命令栏,顶栏以下整块都归它,不用全局底栏
        let body = Rect::new(area.x, area.y + 3, area.width, area.height.saturating_sub(3));
        battle::render(buf, body, app);
    } else {
        let status_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        let body = Rect::new(area.x, area.y + 2, area.width, area.height.saturating_sub(3));
        // 顶部隔开一条线,信息更清楚
        hline(buf, body.x, body.y, body.width, BOX_H, theme::fg(theme::BORDER));
        let body = Rect::new(body.x, body.y + 1, body.width, body.height.saturating_sub(1));
        match app.run.screen {
            crate::core::run::Screen::Map => mapview::render(buf, body, app),
            _ => menu::render(buf, body, app),
        };
        status(buf, status_area, app);
    }
    // 选药水时,顶栏药水区下面浮一个无边框说明
    potion_popup(buf, area, potion_rect, app);
    if let Some(ov) = app.overlay {
        overlay::render(buf, area, app, ov);
    }
}

/// 选药水时的浮窗:无边框,宽度就是顶栏药水区那一块,高度按说明自动
fn potion_popup(buf: &mut Buffer, area: Rect, potion_rect: Rect, app: &App) {
    let Some(sel) = app.potion_sel else {
        return;
    };
    let w = potion_rect.width as usize;
    if w == 0 || area.height < 3 {
        return;
    }
    let lines: Vec<String> = match app.run.player.potions.get(sel).and_then(|s| s.as_ref()) {
        Some(d) => {
            let mut v = vec![d.name.to_string()];
            v.extend(wrap_text(d.desc, w, usize::MAX));
            v
        }
        None => vec!["empty".to_string()],
    };
    let h = (lines.len() as u16).min(area.height.saturating_sub(2));
    if h == 0 {
        return;
    }
    let rect = Rect::new(potion_rect.x, area.y + 1, potion_rect.width, h);
    let bg = Style::default().bg(theme::SEL_BG);
    for y in rect.y..rect.y + rect.height {
        put_padded(buf, rect.x, y, "", w, bg);
    }
    for (i, line) in lines.iter().take(h as usize).enumerate() {
        put_padded(buf, rect.x, rect.y + i as u16, line, w, theme::fg(theme::FG).bg(theme::SEL_BG));
    }
}

/// 遗物行:只写名字,逗号加空格分开;名字按稀有度上色
fn relic_bar(buf: &mut Buffer, area: Rect, app: &App) {
    let relics = &app.run.player.relics;
    if relics.is_empty() {
        put(buf, area.x + 2, area.y, "no relics", theme::dim());
        return;
    }
    let mut x = area.x + 2;
    for (i, r) in relics.iter().enumerate() {
        if x >= area.x + area.width {
            break;
        }
        let text = if i == 0 {
            r.name.to_string()
        } else {
            format!(", {}", r.name)
        };
        put(buf, x, area.y, &text, theme::fg(theme::relic_color(r.rarity)));
        x += display_width(&text) as u16;
    }
}

/// 底栏:左边是模式与消息,右边是当前界面的按键提示
fn status(buf: &mut Buffer, area: Rect, app: &App) {
    let width = area.width as usize;
    let mode = format!("-- {} --", app.run.screen.name());
    if app.mode == Mode::Command {
        // 打字的时候整条底栏都让给命令行,不然提示会把输入挤掉
        let line = format!(":{}_", app.cmd);
        put_padded(
            buf,
            area.x,
            area.y,
            &truncate(&line, width),
            width,
            theme::selected(),
        );
        return;
    }
    let left = format!("{mode} {}", app.msg);
    let hints = app.key_hints();
    let hint_text: String = hints
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join("   ");
    // 消息优先于按键提示:提示放不下就截断它,别把"为什么按不动"吃掉
    let hint_w = display_width(&hint_text);
    let msg_w = display_width(&left);
    let left_w = msg_w
        .min(width.saturating_sub(1))
        .min((width / 2).max(width.saturating_sub(hint_w + 2)));
    put_padded(buf, area.x, area.y, &left, left_w, theme::selected());
    let hx = area.x + left_w as u16 + 1;
    if hx < area.x + area.width {
        put(buf, hx, area.y, &hint_text, theme::fg(theme::DIM));
    }
}


#[cfg(test)]
mod tests {
    //! 渲染层回归测试:每个界面都要能在几种终端尺寸下画出来,关键内容要真的出现在屏幕上.
    use super::*;
    use crate::app::{App, Mode};
    use crate::ui::overlay::Overlay;
    use crate::core::enemies;
    use crate::core::run::Screen;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen_text(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| render(f, app)).expect("draw");
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn app_in_combat(seed: u64, encounter: &'static str) -> App {
        let mut app = App::new(seed);
        let enc = enemies::encounter_def(encounter).expect("遭遇存在");
        app.run.debug_start_combat(enc);
        app.clamp();
        app
    }

    #[test]
    fn map_screen_shows_legend_and_boss() {
        let app = App::new(5);
        let text = screen_text(&app, 110, 40);
        assert!(text.contains("Merchant"), "地图缺少图例:\n{text}");
        assert!(text.contains("Elite"), "图例缺精英那一行:\n{text}");
        assert!(text.contains('B'), "缺少 Boss 节点");
        assert!(text.contains("$99"), "顶栏没画出来");
        assert!(text.contains("80/80"), "血量数字没画出来");
    }

    #[test]
    fn combat_screen_shows_every_enemy_and_hand() {
        let app = app_in_combat(7, "three_sentries");
        let text = screen_text(&app, 120, 36);
        for e in app.run.combat().unwrap().enemies.iter() {
            assert!(text.contains(&e.name), "敌人 {} 没画出来:\n{text}", e.name);
        }
        assert!(text.contains("3/3"), "没有能量显示");
        assert!(!text.contains("##"), "不该再出现血条");
        let name = app.run.combat().unwrap().hand[0].label();
        assert!(text.contains(&name), "手牌 {name} 没画出来:\n{text}");
    }

    #[test]
    fn full_hand_lists_ten_cards_with_cost() {
        let mut app = app_in_combat(7, "three_sentries");
        {
            let c = app.run.combat_mut().unwrap();
            c.hand = vec![crate::core::cards::card("strike"); 10];
        }
        // 终端给大一点,好让 10 张手牌都排得下
        let text = screen_text(&app, 160, 50);
        // 手牌一行一张,每行"费用 牌名"
        assert_eq!(
            text.matches("1 Strike").count(),
            10,
            "手牌应该正好十行速记:\n{text}"
        );
        // 手牌下面是说明:能量在分隔线左边、类型在右边,正文是描述
        assert!(text.contains("energy"), "缺能量:\n{text}");
        assert!(text.contains("<Attack>"), "缺类型(要带尖括号):\n{text}");
        assert!(text.contains("Deal 6 damage."), "缺描述:\n{text}");
    }

    #[test]
    fn upgrade_picker_previews_the_upgraded_card() {
        let mut app = App::new(21);
        // 先把牌组里塞一张 Strike,再打开营火的升级界面
        app.run.rest_smith();
        assert_eq!(app.run.screen, Screen::Pick);
        let text = screen_text(&app, 110, 30);
        assert!(
            text.contains("Strike+") && text.contains("Deal 9 damage."),
            "升级界面应该显示升级后的样子:\n{text}"
        );
    }

    #[test]
    fn history_window_shows_what_happened() {
        let mut app = app_in_combat(31, "jaw_worm_solo");
        app.run.sync_combat();
        // 走真实按键:打开时应该直接停在最新几条上
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('H'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.overlay, Some(Overlay::History));
        let text = screen_text(&app, 110, 34);
        assert!(
            text.contains("history  (j/k scroll, H or esc close)"),
            "历史窗口没打开:\n{text}"
        );
        assert!(
            text.contains("Combat begins") || text.contains("Turn 1"),
            "历史里应该有刚打完的那几步:\n{text}"
        );
        // 滚到顶再滚到底都不该空屏
        app.overlay_scroll = 0;
        let top = screen_text(&app, 110, 34);
        assert!(top.contains("entries in this run"), "滚到顶该看到条目数:\n{top}");
        assert!(top.contains("climb the spire"), "滚到顶该看到开局第一条:\n{top}");
    }

    #[test]
    fn overlays_render_their_content() {
        let mut app = App::new(9);
        app.overlay = Some(Overlay::Deck);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains("Strike"), "牌组界面没列出手牌:\n{text}");
        assert!(text.contains("Deal 6 damage."), "牌组界面缺卡牌说明:\n{text}");
        app.overlay = Some(Overlay::Help);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains(":q"), "帮助界面没列出 :q");
        app.overlay = Some(Overlay::Relics);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains("Burning Blood"), "遗物界面没列出起始遗物");
    }

    #[test]
    fn command_line_is_visible_while_typing() {
        let mut app = App::new(3);
        app.mode = Mode::Command;
        app.cmd = "seed".to_string();
        let text = screen_text(&app, 100, 30);
        assert!(text.contains(":seed"), "命令行没显示出来:\n{text}");
    }

    /// 把每个格子的"字符+样式"抓成字符串:只改颜色不改字符也能比出来
    fn style_snapshot(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| render(f, app)).expect("draw");
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                out.push_str(cell.symbol());
                out.push_str(&format!("{:?}", cell.style()));
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn map_highlight_follows_the_chosen_fork() {
        // 地图上第一层有很多岔路:换一个岔路,该亮的那片未来就不一样
        let mut app = App::new(5);
        app.term_size = (110, 34);
        let reach = app.run.reachable();
        assert!(reach.len() > 1, "第一层应该有多个岔路");
        let before = style_snapshot(&app, 110, 34);
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('j'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(app.map_sel, 1);
        let after = style_snapshot(&app, 110, 34);
        assert_ne!(before, after, "换个岔路,高亮的未来应该跟着变");

        // 高亮的是"选中的那间房之后的整片未来",包含 Boss,但不含刚走出来的那间
        let sel = app.run.reachable()[app.map_sel];
        let fwd = app.run.map.forward_reachable(sel);
        assert!(fwd[app.run.map.boss], "未来里应当包含 Boss");
        if let Some(pos) = app.run.pos {
            assert!(!fwd[pos], "刚走出来的房间不该算进未来");
        }
    }

    #[test]
    fn map_edge_lights_only_when_its_parent_is_on_the_route() {
        // a - b
        //   /
        // c    : a,b 在选中那条路的未来里,c 不在
        let future = [true, true, false];
        assert!(crate::ui::mapview::edge_lit(&future, None, 0, 1), "a->b 应该亮");
        assert!(
            !crate::ui::mapview::edge_lit(&future, None, 2, 1),
            "c 汇进 b,但 c 不在路上,c->b 不该亮"
        );
        // 站在 c 上时,从当前位置出发那条要亮
        assert!(crate::ui::mapview::edge_lit(&future, Some(2), 2, 1));
    }

    #[test]
    fn minimum_terminal_is_80x24() {
        let app = App::new(5);
        // 80x24 要能完整画出来
        let text = screen_text(&app, 80, 24);
        assert!(text.contains("Merchant"), "80x24 下地图应该正常显示:\n{text}");
        let combat = app_in_combat(5, "three_sentries");
        let text = screen_text(&combat, 80, 24);
        assert!(text.contains("energy"), "80x24 下战斗界面应该正常:\n{text}");
        assert!(text.contains("Strike") || text.contains("Defend"), "手牌要画出来");
        // 比这小就只给一句提示
        let small = screen_text(&app, 79, 23);
        assert!(small.contains("needs at least 80x24"), "小终端应给出提示:\n{small}");
    }

    #[test]
    fn every_screen_survives_tiny_and_huge_terminals() {
        let mut app = App::new(11);
        let screens = [
            Screen::Map,
            Screen::Combat,
            Screen::Reward,
            Screen::Rest,
            Screen::Shop,
            Screen::Event,
            Screen::Treasure,
            Screen::Pick,
            Screen::Victory,
            Screen::Death,
        ];
        for size in [(24u16, 6u16), (40, 12), (80, 24), (200, 60)] {
            for s in screens.iter() {
                app.run.screen = *s;
                let _ = screen_text(&app, size.0, size.1);
            }
        }
        // 真数据下的战斗界面也要能画
        let combat = app_in_combat(13, "three_sentries");
        for size in [(24u16, 6u16), (60, 18), (140, 50)] {
            let _ = screen_text(&combat, size.0, size.1);
        }
        // 叠加层在各种尺寸下也要能画
        for ov in [
            Overlay::Deck,
            Overlay::Map,
            Overlay::Relics,
            Overlay::Potions,
            Overlay::Help,
        ] {
            app.overlay = Some(ov);
            for size in [(24u16, 6u16), (60, 20), (160, 50)] {
                let _ = screen_text(&app, size.0, size.1);
            }
        }
        app.overlay = None;
        // 极端窄屏只画一行提示,不能 panic
        let _ = screen_text(&app, 10, 4);
        let _ = screen_text(&app, 1, 1);
    }
}












