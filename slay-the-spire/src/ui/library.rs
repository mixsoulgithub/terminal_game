// 图鉴:全量卡牌/遗物/药水的列表 + 选中项详情.
// 数据来自 core::compendium(语料),这里只负责画.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::compendium;
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_padded, truncate, vline, wrap_text, BOX_V};

/// 把一段带换行的文本按行宽写下去,返回写完后的 y
fn put_paras(
    buf: &mut Buffer,
    detail: Rect,
    x: u16,
    mut y: u16,
    w: usize,
    text: &str,
    style: Style,
) -> u16 {
    for para in text.split('\n') {
        for line in wrap_text(para, w, usize::MAX) {
            if y >= detail.y + detail.height {
                return y;
            }
            put(buf, x, y, &line, style);
            y += 1;
        }
    }
    y
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let lib = app.library;
    if area.width < 40 || area.height < 6 {
        return;
    }
    let items = compendium::items(lib);
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
    if inner.width < 20 || inner.height == 0 {
        return;
    }
    // 左边列表,右边详情,中间一条竖线
    let list_w = (inner.width as usize * 55 / 100).clamp(18, 46) as u16;
    let list = Rect::new(inner.x, inner.y, list_w, inner.height);
    let detail = Rect::new(
        inner.x + list_w + 1,
        inner.y,
        inner.width.saturating_sub(list_w + 1),
        inner.height,
    );
    vline(
        buf,
        inner.x + list_w,
        inner.y,
        inner.height,
        BOX_V,
        theme::fg(theme::BORDER),
    );

    // 滚动:光标始终在窗口里
    let cap = list.height as usize;
    let first = if items.len() > cap {
        app.lib_sel.saturating_sub(cap / 2).min(items.len() - cap)
    } else {
        0
    };
    for (i, item) in items.iter().enumerate().skip(first).take(cap) {
        let y = list.y + (i - first) as u16;
        if item.header {
            put_padded(buf, list.x, y, "", list.width as usize, Style::default().bg(theme::BG));
            put(
                buf,
                list.x,
                y,
                item.name,
                Style::default()
                    .fg(theme::INFO)
                    .bg(theme::BG)
                    .add_modifier(Modifier::BOLD),
            );
            continue;
        }
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
        let tag = format!("{:>2} ", item.tag);
        put(buf, list.x, y, &tag, style.fg(theme::DIM));
        put(
            buf,
            list.x + display_width(&tag) as u16,
            y,
            &truncate(&item.name.to_string(), list.width as usize - 4),
            style.fg(name_color),
        );
    }

    // 详情
    let Some(item) = items.get(app.lib_sel).filter(|i| !i.header) else {
        return;
    };
    let mut y = detail.y;
    let w = detail.width as usize;
    let name_style = Style::default()
        .fg(theme::corpus_color(item.rarity_key))
        .bg(theme::BG)
        .add_modifier(Modifier::BOLD);
    put(buf, detail.x, y, &truncate(item.name, w), name_style);
    y += 1;
    if y >= detail.y + detail.height {
        return;
    }
    put(
        buf,
        detail.x,
        y,
        &truncate(&item.sub, w),
        Style::default().fg(theme::DIM).bg(theme::BG),
    );
    y += 1;
    if y >= detail.y + detail.height {
        return;
    }
    // 语料里的原始字段,方便和 slay-the-cli 对账
    put(
        buf,
        detail.x,
        y,
        &truncate(&item.origin, w),
        Style::default().fg(theme::BORDER).bg(theme::BG),
    );
    y += 2;
    y = put_paras(
        buf,
        detail,
        detail.x,
        y,
        w,
        &item.text,
        Style::default().fg(theme::FG).bg(theme::BG),
    );
    if !item.text_up.is_empty() {
        y += 1;
        if y >= detail.y + detail.height {
            return;
        }
        put(
            buf,
            detail.x,
            y,
            "upgraded",
            Style::default().fg(theme::INFO).bg(theme::BG),
        );
        y += 1;
        y = put_paras(
            buf,
            detail,
            detail.x,
            y,
            w,
            &item.text_up,
            Style::default().fg(theme::GOOD).bg(theme::BG),
        );
    }
    y += 1;
    if y < detail.y + detail.height {
        let (mark, style) = if item.done {
            ("implemented", theme::fg(theme::GOOD))
        } else {
            ("not implemented yet", theme::fg(theme::WARN))
        };
        put(buf, detail.x, y, mark, style.bg(theme::BG));
    }
}
