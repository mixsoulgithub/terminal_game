// 图鉴:一行标签页,下面是"列表 + 详情".
// 数据来自 core::compendium(语料),这里只负责画.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::compendium::{self, Group, Item};
use crate::ui::theme;
use crate::ui::{
    display_width, draw_box, hsep, put, put_padded, truncate, vsep, wrap_text,
};

/// 把一段带换行的文本按行宽写下去,返回写完后的 y
fn put_paras(
    buf: &mut Buffer,
    bottom: u16,
    x: u16,
    mut y: u16,
    w: usize,
    text: &str,
    style: Style,
) -> u16 {
    for para in text.split('\n') {
        for line in wrap_text(para, w, usize::MAX) {
            if y >= bottom {
                return y;
            }
            put(buf, x, y, &line, style);
            y += 1;
        }
    }
    y
}

/// 标签页那一行:选中的字是黄的、底色是本色;没选中的字灰、底色掺灰
fn tabs(buf: &mut Buffer, row: Rect, gs: &[Group], cur: usize) {
    let mut x = row.x;
    for (i, g) in gs.iter().enumerate() {
        let label = format!(" {} ", g.name);
        let w = display_width(&label) as u16;
        if x + w > row.x + row.width {
            break;
        }
        let style = if i == cur {
            Style::default().fg(theme::tab_fg(g.color)).bg(theme::tab_color(g.color))
        } else {
            Style::default().fg(theme::DIM).bg(theme::tab_bg(g.color))
        };
        put(buf, x, row.y, &label, style);
        x += w;
    }
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let lib = app.library;
    if area.width < 40 || area.height < 6 {
        return;
    }
    let gs = compendium::groups(lib);
    let tab = app.lib_tab % gs.len().max(1);
    let items = compendium::items(lib, tab);
    let (done, total) = compendium::progress(lib);
    let title = format!("{}   {}/{} implemented", lib.title(), done, total);
    draw_box(
        buf,
        area,
        &title,
        theme::fg(theme::SEL_FG),
        theme::fg(theme::INFO),
    );
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.width < 20 || inner.height < 4 {
        return;
    }
    // 第 1 行标签页,第 2 行分隔线,下面是内容
    tabs(buf, Rect::new(inner.x, inner.y, inner.width, 1), &gs, tab);
    let (tdone, ttotal) = compendium::tab_progress(lib, tab);
    let count = format!("{}/{}", tdone, ttotal);
    let cw = display_width(&count) as u16;
    if inner.width > cw + 2 {
        put(
            buf,
            inner.x + inner.width - cw - 1,
            inner.y,
            &count,
            Style::default().fg(theme::DIM).bg(theme::BG),
        );
    }
    let content = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let border = theme::fg(theme::BORDER);
    hsep(buf, inner, inner.y + 1, border);
    if content.width < 20 || content.height == 0 {
        return;
    }
    // 左边列表,右边详情,中间一条竖线
    let list_w = (content.width as usize * 55 / 100).clamp(16, 44) as u16;
    let list = Rect::new(content.x, content.y, list_w, content.height);
    let detail = Rect::new(
        content.x + list_w + 1,
        content.y,
        content.width.saturating_sub(list_w + 1),
        content.height,
    );
    vsep(buf, content, content.x + list_w, border);

    // 滚动:光标始终在窗口里
    let cap = list.height as usize;
    let first = if items.len() > cap {
        app.lib_sel.saturating_sub(cap / 2).min(items.len() - cap)
    } else {
        0
    };
    for (i, item) in items.iter().enumerate().skip(first).take(cap) {
        let y = list.y + (i - first) as u16;
        let selected = i == app.lib_sel;
        let style = if selected {
            theme::selected()
        } else if item.done {
            Style::default().fg(theme::FG).bg(theme::BG)
        } else {
            Style::default().fg(theme::DIM).bg(theme::BG)
        };
        let name_color = if selected {
            theme::SEL_FG
        } else {
            theme::corpus_color(item.rarity_key)
        };
        put_padded(buf, list.x, y, "", list.width as usize, style);
        // 左边:费用 + 名字
        let mut x = list.x;
        if !item.show_cost {
            let tag = format!("{:>2} ", item.tag);
            put(buf, x, y, &tag, style.fg(theme::DIM));
            x += display_width(&tag) as u16;
        } else {
            x = crate::ui::put_cost_token(
                buf,
                x,
                y,
                &item.tag,
                if selected {
                    theme::energy_color(item.color_key)
                } else {
                    theme::DIM
                },
            );
            put(buf, x, y, " ", style);
            x += 1;
        }
        let right_w = if item.target_tag.is_empty() {
            0
        } else {
            display_width(&item.target_tag)
        };
        let name_w = (list.width as usize).saturating_sub((x - list.x) as usize + right_w + 1);
        put(
            buf,
            x,
            y,
            &truncate(item.name, name_w.max(1)),
            style.fg(name_color),
        );
        // 中间:没实现
        if !item.done {
            let mid = "(not implemented)";
            let mw = display_width(mid);
            let mx = list.x + ((list.width as usize).saturating_sub(mw) / 2) as u16;
            if mx > x + display_width(item.name) as u16 {
                put(
                    buf,
                    mx,
                    y,
                    mid,
                    if selected {
                        theme::selected()
                    } else {
                        Style::default().fg(theme::WARN).bg(theme::BG)
                    },
                );
            }
        }
        // 右边:目标
        if !item.target_tag.is_empty() {
            let tw = display_width(&item.target_tag) as u16;
            let tx = list.x + list.width.saturating_sub(tw + 1);
            put(
                buf,
                tx,
                y,
                &item.target_tag,
                if selected {
                    theme::selected()
                } else {
                    Style::default().fg(theme::DIM).bg(theme::BG)
                },
            );
        }
    }

    // 详情:和升级界面一样,上面本体、下面升过级的(没有升级就只画一块)
    let Some(item) = items.get(app.lib_sel) else {
        return;
    };
    if item.text_up.is_empty() {
        put_card_block(buf, detail, item, false);
    } else {
        let (first, second, horizontal) = crate::ui::split_two_with(detail, true);
        if horizontal {
            hsep(
                buf,
                detail,
                second.y.saturating_sub(1),
                theme::fg(theme::BORDER),
            );
        }
        put_card_block(buf, first, item, false);
        put_card_block(buf, second, item, true);
    }
}

/// 详情里的一块:费用 + 名字 / 类型 / 语料字段 / 正文
fn put_card_block(buf: &mut Buffer, rect: Rect, item: &Item, upgraded: bool) {
    let w = rect.width as usize;
    if w < 4 || rect.height == 0 {
        return;
    }
    let bottom = rect.y + rect.height;
    let mut y = rect.y;
    let (cost, name) = if upgraded {
        (item.tag_up.as_str(), format!("{}+", item.name))
    } else {
        (item.tag.as_str(), item.name.to_string())
    };
    let mut x = rect.x;
    if item.show_cost {
        x = crate::ui::put_cost_token(buf, x, y, cost, theme::energy_color(item.color_key));
        put(buf, x, y, " ", Style::default().bg(theme::BG));
        x += 1;
    }
    let name_style = Style::default()
        .fg(theme::corpus_color(item.rarity_key))
        .bg(theme::BG)
        .add_modifier(Modifier::BOLD);
    put(
        buf,
        x,
        y,
        &truncate(&name, w.saturating_sub((x - rect.x) as usize)),
        name_style,
    );
    y += 1;
    // 语料字段只在上半块写一次,下半块(升级后)不重复
    let mut lines: Vec<(&str, ratatui::style::Color)> = vec![(item.sub.as_str(), theme::DIM)];
    if !upgraded {
        lines.push((item.origin.as_str(), theme::BORDER));
    }
    for (text, color) in lines {
        if y >= bottom {
            return;
        }
        put(
            buf,
            rect.x,
            y,
            &truncate(text, w),
            Style::default().fg(color).bg(theme::BG),
        );
        y += 1;
    }
    y += 1;
    let (text, style) = if upgraded {
        (
            &item.text_up,
            Style::default().fg(theme::GOOD).bg(theme::BG),
        )
    } else {
        (&item.text, Style::default().fg(theme::FG).bg(theme::BG))
    };
    put_paras(buf, bottom, rect.x, y, w, text, style);
}

