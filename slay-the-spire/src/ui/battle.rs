// 战斗界面:敌人区 + 战斗日志 + 手牌 + 描述行.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::card::CardInstance;
use crate::core::combat::{Combat, LogKind};
use crate::core::enemy::Intent;
use crate::ui::theme;
use crate::ui::{display_width, fit, put, put_padded, truncate};

const CARD_W: usize = 18;
const CARD_H: u16 = 6;

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(c) = app.run.combat() else {
        put(buf, area.x, area.y, "(no combat)", theme::dim());
        return;
    };
    if area.height < 3 {
        return;
    }
    // 布局自下而上切:手牌 -> 描述 -> 日志 -> 敌人;每一块都夹在实际可用高度里
    let hand_h = CARD_H.min(area.height.saturating_sub(2));
    let enemy_h = if area.height >= 24 { 5 } else { 4 }.min(area.height.saturating_sub(hand_h));
    let desc_h = 2u16.min(area.height.saturating_sub(enemy_h + hand_h));
    let log_h = area.height.saturating_sub(enemy_h + desc_h + hand_h + 1);

    let enemy_area = Rect::new(area.x, area.y, area.width, enemy_h);
    let log_area = Rect::new(area.x, area.y + enemy_h, area.width, log_h);
    let hand_y = area.y + area.height - hand_h;
    let hand_area = Rect::new(area.x, hand_y, area.width, hand_h);
    let desc_area = Rect::new(area.x, hand_y - desc_h, area.width, desc_h);

    render_enemies(buf, enemy_area, app, c);
    render_log(buf, log_area, c);
    render_desc(buf, desc_area, app, c);
    render_hand(buf, hand_area, app, c);
}

fn render_enemies(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    let n = c.enemies.len().max(1);
    let gap = 1u16;
    let box_w = ((area.width.saturating_sub(gap * (n as u16 - 1))) / n as u16)
        .max(14)
        .min(38);
    for (i, e) in c.enemies.iter().enumerate() {
        let x = area.x + (box_w + gap) * i as u16;
        if x + box_w > area.x + area.width {
            break;
        }
        let selected = app.target_sel == i && e.alive();
        let border = if !e.alive() {
            theme::DIM
        } else if selected {
            theme::SEL_FG
        } else {
            theme::BORDER
        };
        let box_area = Rect::new(x, area.y, box_w, area.height);
        let title = if e.alive() { e.name.as_str() } else { "dead" };
        let title_style = if e.alive() {
            Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        crate::ui::draw_box(buf, box_area, title, theme::fg(border), title_style);
        let inner_w = (box_w as usize).saturating_sub(4);
        let mut y = area.y + 1;
        if !e.alive() {
            put(buf, x + 2, y, &fit("slain", inner_w), theme::dim());
            continue;
        }
        // 血条
        let ratio = e.hp_ratio();
        let hp = format!("HP [{}] {}/{}", theme::bar(ratio, 8), e.hp, e.max_hp);
        put(buf, x + 2, y, &fit(&hp, inner_w), theme::fg(theme::hp_color(ratio)));
        y += 1;
        if y >= area.y + area.height - 1 {
            continue;
        }
        // 意图(最关心的一行,紧跟血条)
        let (text, color) = intent_text(c, i);
        let marker = if selected { "> " } else { "  " };
        put(
            buf,
            x + 2,
            y,
            &fit(&format!("{marker}{text}"), inner_w),
            theme::fg(color).add_modifier(Modifier::BOLD),
        );
        y += 1;
        if y >= area.y + area.height - 1 {
            continue;
        }
        // 格挡与状态
        let mut second = String::new();
        if e.block > 0 {
            second.push_str(&format!("BLK {} ", e.block));
        }
        for (st, v) in e.statuses.iter() {
            second.push_str(&format!("{} {} ", st.short(), v));
        }
        put(
            buf,
            x + 2,
            y,
            &fit(&second, inner_w),
            theme::fg(theme::BLOCK),
        );
    }
}

fn intent_text(c: &Combat, i: usize) -> (String, ratatui::style::Color) {
    let intent = c.enemies[i].intent();
    let color = theme::intent_color(&intent);
    let text = match intent {
        Intent::Attack { .. } | Intent::AttackDebuff { .. } => {
            let (per, times) = c.predicted_damage(i);
            if times > 1 {
                format!("ATK {per} x{times}")
            } else {
                format!("ATK {per}")
            }
        }
        Intent::AttackDefend { .. } => {
            let (per, times) = c.predicted_damage(i);
            let blk = c.intent_block(i);
            if times > 1 {
                format!("ATK {per} x{times} DEF {blk}")
            } else {
                format!("ATK {per} DEF {blk}")
            }
        }
        Intent::Defend => format!("DEF {}", c.intent_block(i)),
        Intent::Buff => "BUFF".to_string(),
        Intent::Debuff => "DEBUFF".to_string(),
        Intent::Sleep => "SLEEP".to_string(),
        Intent::Unknown => "???".to_string(),
    };
    (text, color)
}

fn render_log(buf: &mut Buffer, area: Rect, c: &Combat) {
    if area.height == 0 {
        return;
    }
    let lines: Vec<&crate::core::combat::LogLine> = c
        .log
        .iter()
        .rev()
        .take(area.height as usize)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    for (i, line) in lines.iter().enumerate() {
        let style = match line.kind {
            LogKind::Info => theme::dim(),
            LogKind::Player => theme::fg(theme::GOOD),
            LogKind::Enemy => theme::fg(theme::BAD),
        };
        let y = area.y + i as u16;
        put_padded(buf, area.x, y, &format!("  {}", line.text), area.width as usize, style);
    }
}

/// 手牌;牌太多就显示一个窗口
pub fn render_hand(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if c.hand.is_empty() {
        put(
            buf,
            area.x + 2,
            area.y,
            "(hand is empty - press e to end the turn)",
            theme::dim(),
        );
        return;
    }
    let per = ((area.width + 1) / (CARD_W as u16 + 1)).max(1) as usize;
    let sel = app.hand_sel.min(c.hand.len() - 1);
    let start = if c.hand.len() <= per {
        0
    } else {
        sel.saturating_sub(per / 2).min(c.hand.len() - per)
    };
    if start > 0 {
        put(buf, area.x, area.y + CARD_H / 2, "<", theme::fg(theme::DIM));
    }
    for (slot, idx) in (start..c.hand.len().min(start + per)).enumerate() {
        let card = &c.hand[idx];
        let x = area.x + 1 + (slot as u16) * (CARD_W as u16 + 1);
        let w = (area.width as usize).saturating_sub(x as usize - area.x as usize);
        if w < 8 {
            break;
        }
        let w = w.min(CARD_W);
        let selected = idx == sel;
        let playable = c.blocked_reason(idx).is_none();
        draw_card(
            buf,
            Rect::new(x, area.y, w as u16, CARD_H),
            card,
            idx,
            selected,
            playable,
        );
    }
    if start + per < c.hand.len() {
        let x = area.x + area.width - 2;
        put(buf, x, area.y + CARD_H / 2, ">", theme::fg(theme::DIM));
    }
}

fn draw_card(
    buf: &mut Buffer,
    area: Rect,
    card: &CardInstance,
    idx: usize,
    selected: bool,
    playable: bool,
) {
    let w = area.width as usize;
    if w < 8 {
        return;
    }
    let inner = w.saturating_sub(4);
    let color = if playable {
        theme::card_color(card.kind(), card.rarity())
    } else {
        theme::DIM
    };
    let border = if selected { theme::SEL_FG } else { theme::BORDER };
    let top = if selected {
        format!("*{}+", "-".repeat(w.saturating_sub(2)))
    } else {
        format!("+{}+", "-".repeat(w.saturating_sub(2)))
    };
    put(buf, area.x, area.y, &top, theme::fg(border));
    let bottom = format!("+{}+", "-".repeat(w.saturating_sub(2)));
    put(buf, area.x, area.y + area.height - 1, &bottom, theme::fg(border));
    for y in area.y + 1..area.y + area.height - 1 {
        let style = if selected {
            theme::selected()
        } else {
            Style::default().bg(theme::BG)
        };
        put_padded(buf, area.x, y, "|", 1, theme::fg(border));
        put_padded(buf, area.x + area.width - 1, y, "|", 1, theme::fg(border));
        put_padded(buf, area.x + 1, y, "", inner, style);
    }
    // 名字行带序号
    let name = format!("{}. {}", idx + 1, card.label());
    put_padded(
        buf,
        area.x + 2,
        area.y + 1,
        &name,
        inner,
        if selected {
            theme::selected()
        } else {
            theme::fg(if playable { color } else { theme::DIM })
        },
    );
    // 类型与费用
    let cost = match card.cost() {
        crate::core::card::Cost::Fixed(n) => n.to_string(),
        crate::core::card::Cost::X => "X".to_string(),
        crate::core::card::Cost::Unplayable => "-".to_string(),
    };
    let kind = format!("{} c{}", card.kind().name(), cost);
    put_padded(
        buf,
        area.x + 2,
        area.y + 2,
        &kind,
        inner,
        if selected {
            theme::selected()
        } else {
            theme::dim()
        },
    );
    // 描述占两行
    let text = card.display_text();
    let lines = wrap_text(&text, inner, 2);
    for (i, line) in lines.iter().enumerate() {
        let y = area.y + 3 + i as u16;
        if y >= area.y + area.height - 1 {
            break;
        }
        put_padded(
            buf,
            area.x + 2,
            y,
            line,
            inner,
            if selected {
                theme::selected()
            } else {
                theme::fg(theme::FG)
            },
        );
    }
}

fn render_desc(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.height == 0 {
        return;
    }
    let w = area.width as usize;
    let Some(card) = c.hand.get(app.hand_sel.min(c.hand.len().saturating_sub(1))) else {
        return;
    };
    let text = card.display_text();
    put_padded(
        buf,
        area.x,
        area.y,
        &format!("  {}: {}", card.label(), text),
        w,
        theme::fg(theme::FG),
    );
    if area.height < 2 {
        return;
    }
    let reason = c.blocked_reason(app.hand_sel.min(c.hand.len() - 1));
    let target = if card.needs_target() {
        match c.enemies.get(app.target_sel) {
            Some(e) if e.alive() => format!("target: {}", e.name),
            _ => "target: none".to_string(),
        }
    } else {
        "target: none".to_string()
    };
    let hint = match reason {
        Some(r) => format!("  cannot play: {r}"),
        None => format!("  [enter] play   {target}"),
    };
    let style = if reason.is_some() {
        theme::fg(theme::WARN)
    } else {
        theme::dim()
    };
    put_padded(buf, area.x, area.y + 1, &hint, w, style);
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
            // 单个词太长,硬切
            let t = truncate(word, width);
            lines.push(t);
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
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }
    if lines.len() == max_lines {
        if let Some(last) = lines.last_mut() {
            if display_width(last) >= width {
                *last = truncate(last, width.saturating_sub(2)) + "..";
            }
        }
    }
    lines
}
