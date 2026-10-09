// 开始界面:标题(continue/new game/compendium/quit)、角色选择、图鉴子菜单.
// 这三个屏幕占满整屏,不画顶栏和遗物行.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::compendium::{self, Library};
use crate::core::roster;
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_centered, truncate, wrap_text};

/// 塔的图案,画在标题上方(用地图一样的 ╱╲)
const SPIRE: [&str; 4] = ["      ╱╲", "     ╱  ╲", "    ╱ ╱╲ ╲", "   ╱_╱  ╲_╲"];

fn centered_x(width: u16, w: usize) -> u16 {
    ((width as usize).saturating_sub(w) / 2) as u16
}

pub fn title(buf: &mut Buffer, area: Rect, app: &App) {
    let entries = app.title_entries();
    let entries_h = entries.len();
    let rows: Vec<(&str, bool, &str)> = entries.clone();
    // 需要的高度:塔 4 行 + 空行 + 框(2 + 条目 + 1)
    let box_h = entries_h as u16 + 3;
    let need = 4 + 1 + box_h;
    let w = 54u16.min(area.width.saturating_sub(2));
    let x = area.x + centered_x(area.width, w as usize);
    let mut y = area.y + area.height.saturating_sub(need) / 2;
    if area.height > need + 2 {
        for line in SPIRE {
            put(
                buf,
                x + centered_x(w, display_width(line)),
                y,
                line,
                Style::default().fg(theme::BLOOD).bg(theme::BG),
            );
            y += 1;
        }
        y += 1;
    }
    let rect = Rect::new(x, y, w, box_h.min(area.height.saturating_sub(y - area.y)));
    draw_box(
        buf,
        rect,
        "slay the spire",
        theme::fg(theme::SEL_FG),
        theme::fg(theme::INFO),
    );
    for (i, (label, ok, _)) in rows.iter().enumerate() {
        if i >= entries_h {
            break;
        }
        let ly = rect.y + 1 + i as u16;
        if ly >= rect.y + rect.height.saturating_sub(1) {
            break;
        }
        let selected = i == app.title_sel;
        let text = truncate(label, rect.width as usize - 8);
        let style = if selected {
            theme::selected()
        } else if *ok {
            Style::default().fg(theme::FG).bg(theme::BG)
        } else {
            Style::default().fg(theme::DIM).bg(theme::BG)
        };
        let lx = rect.x + 4;
        if selected {
            // 选中行整行反白
            for xx in rect.x + 1..rect.x + rect.width - 1 {
                put(buf, xx, ly, " ", Style::default().bg(theme::SEL_BG));
            }
        }
        put(buf, lx, ly, &text, style);
    }
    // 底部一行:种子和存档状态
    let foot = format!("seed {}", app.run.seed);
    if rect.height >= entries_h as u16 + 3 {
        put(
            buf,
            rect.x + 2,
            rect.y + rect.height - 2,
            &truncate(&foot, rect.width as usize - 4),
            Style::default().fg(theme::DIM).bg(theme::BG),
        );
    }
}

pub fn char_select(buf: &mut Buffer, area: Rect, app: &App) {
    let chars = roster::all();
    let box_h = chars.len() as u16 + 5;
    let w = 62u16.min(area.width.saturating_sub(2));
    let h = box_h.min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + centered_x(area.width, w as usize),
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    draw_box(
        buf,
        rect,
        "choose a character",
        theme::fg(theme::SEL_FG),
        theme::fg(theme::INFO),
    );
    for (i, ch) in chars.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y >= rect.y + rect.height - 1 {
            break;
        }
        let selected = i == app.char_sel;
        let playable = roster::playable(ch);
        let deck_n: u32 = ch.deck.iter().map(|(_, n)| *n as u32).sum();
        let line = format!(
            "{:<10} {:>3} HP   {:<20} {} cards",
            ch.name, ch.max_hp, ch.relic_name, deck_n
        );
        let style = if selected {
            theme::selected()
        } else if playable {
            Style::default().fg(theme::FG).bg(theme::BG)
        } else {
            Style::default().fg(theme::DIM).bg(theme::BG)
        };
        if selected {
            for xx in rect.x + 1..rect.x + rect.width - 1 {
                put(buf, xx, y, " ", Style::default().bg(theme::SEL_BG));
            }
        }
        put(
            buf,
            rect.x + 3,
            y,
            &truncate(&line, rect.width as usize - 5),
            style.fg(if selected {
                theme::SEL_FG
            } else {
                theme::role_color(ch.color)
            }),
        );
    }
    // 底部:飞升等级(可调) + 当前角色能不能玩
    let asc_y = rect.y + rect.height - 3;
    put(
        buf,
        rect.x + 2,
        asc_y,
        &truncate(
            &format!(
                "Ascension {}: {}   [a/A to change]",
                app.ascension,
                crate::core::ascension::label(app.ascension)
            ),
            rect.width as usize - 4,
        ),
        Style::default()
            .fg(if app.ascension > 0 { theme::WARN } else { theme::FG })
            .bg(theme::BG),
    );
    let ch = roster::by_index(app.char_sel);
    let note = match roster::blocked_reason(ch) {
        None => format!("{} is ready", ch.name),
        Some(reason) => format!("{} not playable yet: {}", ch.name, reason),
    };
    let y = rect.y + rect.height - 2;
    for (i, line) in wrap_text(&note, rect.width as usize - 4, 2).iter().enumerate() {
        if y + i as u16 >= rect.y + rect.height - 1 {
            break;
        }
        put(
            buf,
            rect.x + 2,
            y + i as u16,
            line,
            Style::default().fg(theme::WARN).bg(theme::BG),
        );
    }
}

pub fn compendium(buf: &mut Buffer, area: Rect, app: &App) {
    let libs = Library::ALL;
    let box_h = libs.len() as u16 + 4;
    let w = 66u16.min(area.width.saturating_sub(2));
    let h = box_h.min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + centered_x(area.width, w as usize),
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    draw_box(
        buf,
        rect,
        "compendium",
        theme::fg(theme::SEL_FG),
        theme::fg(theme::INFO),
    );
    for (i, lib) in libs.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y >= rect.y + rect.height - 1 {
            break;
        }
        let (done, total) = compendium::progress(*lib);
        let selected = i == app.comp_sel;
        let line = format!("{:<20} {:>4}/{:<4} implemented", lib.title(), done, total);
        if selected {
            for xx in rect.x + 1..rect.x + rect.width - 1 {
                put(buf, xx, y, " ", Style::default().bg(theme::SEL_BG));
            }
        }
        let style = if selected {
            theme::selected()
        } else {
            Style::default().fg(theme::FG).bg(theme::BG)
        };
        put(
            buf,
            rect.x + 3,
            y,
            &truncate(&line, rect.width as usize - 5),
            style,
        );
    }
    let other = format!(
        "cards {} / relics {} / potions {} / events {} / enemies {}",
        compendium::progress(Library::Cards).1,
        compendium::progress(Library::Relics).1,
        compendium::progress(Library::Potions).1,
        compendium::progress(Library::Events).1,
        compendium::progress(Library::Enemies).1
    );
    put_centered(
        buf,
        rect.x + 1,
        rect.y + rect.height - 2,
        rect.width as usize - 2,
        &truncate(&other, rect.width as usize - 2),
        Style::default().fg(theme::DIM).bg(theme::BG),
    );
    let _ = Modifier::BOLD;
}
