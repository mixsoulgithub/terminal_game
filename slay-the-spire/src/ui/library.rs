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
            Style::default().fg(theme::YELLOW).bg(theme::tab_color(g.color))
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
        let tag = format!("{:>2} ", item.tag);
        put(buf, list.x, y, &tag, style.fg(theme::DIM));
        put(
            buf,
            list.x + display_width(&tag) as u16,
            y,
            &truncate(item.name, list.width as usize - 4),
            style.fg(name_color),
        );
    }

    // 详情
    let Some(item): Option<&Item> = items.get(app.lib_sel) else {
        return;
    };
    let w = detail.width as usize;
    if w < 4 {
        return;
    }
    let bottom = detail.y + detail.height;
    let name_style = Style::default()
        .fg(theme::corpus_color(item.rarity_key))
        .bg(theme::BG)
        .add_modifier(Modifier::BOLD);
    put(buf, detail.x, detail.y, &truncate(item.name, w), name_style);
    let mut y = detail.y + 1;
    for text in [&item.sub, &item.origin] {
        if y >= bottom {
            return;
        }
        let color = if text == &item.sub {
            theme::DIM
        } else {
            theme::BORDER
        };
        put(
            buf,
            detail.x,
            y,
            &truncate(text, w),
            Style::default().fg(color).bg(theme::BG),
        );
        y += 1;
    }
    y += 1;
    y = put_paras(
        buf,
        bottom,
        detail.x,
        y,
        w,
        &item.text,
        Style::default().fg(theme::FG).bg(theme::BG),
    );
    if !item.text_up.is_empty() {
        y += 1;
        if y < bottom {
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
                bottom,
                detail.x,
                y,
                w,
                &item.text_up,
                Style::default().fg(theme::GOOD).bg(theme::BG),
            );
        }
    }
    y += 1;
    if y < bottom {
        let (mark, style) = if item.done {
            ("implemented", theme::fg(theme::GOOD))
        } else {
            ("not implemented yet", theme::fg(theme::WARN))
        };
        put(buf, detail.x, y, mark, style.bg(theme::BG));
    }
}
