// 图鉴:一行标签页,下面是"列表 + 详情".
// 数据来自 core::compendium(语料),这里只负责画.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::compendium::{self, Group, Item};
use crate::ui::theme;
use crate::ui::{
    display_width, draw_box, hsep, put, put_centered, put_padded, truncate,
    vsep,
};

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
    // 事件册:列表上方压一行总数统计
    let content = if let Some(note) = compendium::header_note(lib) {
        if content.height == 0 {
            return;
        }
        put(
            buf,
            content.x,
            content.y,
            &truncate(&note, content.width as usize),
            Style::default().fg(theme::INFO),
        );
        Rect::new(
            content.x,
            content.y + 1,
            content.width,
            content.height.saturating_sub(1),
        )
    } else {
        content
    };
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
                theme::FG,
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

/// 详情里的一块,和升级界面的说明一个版式:
/// 费用在左,名字/类型/正文居中(数字照样上色)
/// 升级后的费用比原来小?("3" > "2" 这种)
fn cost_num_less(up: &str, base: &str) -> bool {
    match (up.parse::<i32>(), base.parse::<i32>()) {
        (Ok(u), Ok(b)) => u < b,
        _ => false,
    }
}

fn put_card_block(buf: &mut Buffer, rect: Rect, item: &Item, upgraded: bool) {
    let w = rect.width as usize;
    if w < 4 || rect.height == 0 {
        return;
    }
    let bottom = rect.y + rect.height;
    let (cost, name) = if upgraded {
        (item.tag_up.as_str(), format!("{}+", item.name))
    } else {
        (item.tag.as_str(), item.name.to_string())
    };
    if item.show_cost {
        // 升级后费用变便宜就绿,否则默认色
        let digit = if cost_num_less(item.tag_up.as_str(), item.tag.as_str()) {
            theme::GOOD
        } else {
            theme::FG
        };
        crate::ui::put_cost_token(buf, rect.x, rect.y, cost, theme::energy_color(item.color_key), digit);
    }
    let name_style = Style::default()
        .fg(theme::corpus_color(item.rarity_key))
        .bg(theme::BG)
        .add_modifier(Modifier::BOLD);
    let mut y = rect.y;
    crate::ui::put_centered_label(buf, rect.x, y, w, &name, name_style);
    y += 1;
    if y >= bottom {
        return;
    }
    // 卡牌的类型带括号(<Attack> 这种),遗物/药水就用副标题
    let kind_line = if item.show_cost {
        crate::ui::kind_label_str(item.kind)
    } else {
        item.sub.clone()
    };
    put_centered(buf, rect.x, y, w, &kind_line, theme::fg(theme::FG));
    y += 1;
    // 正文居中折行,伤害/格挡数字上色;语料的正文本身带换行,按行分开折
    let text = if upgraded { &item.text_up } else { &item.text };
    for para in text.split('\n') {
        let words = crate::ui::desc_words(para, theme::energy_color(item.color_key));
        for line in crate::ui::wrap_words(&words, w) {
            if y >= bottom {
                return;
            }
            crate::ui::put_centered_words(buf, rect.x, y, w, &line);
            y += 1;
        }
    }
}
