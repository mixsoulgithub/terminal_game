// 战斗界面:敌人区 + 战斗日志 + 手牌区.
// 手牌是 5 列 2 行共 10 个速记格子(颜色区分含义:费用黄、伤害红、格挡蓝),
// 只有选中的那张在右侧详情区给出全名与完整描述;能量放在手牌旁边.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::core::card::{CardInstance, Cost, Effect};
use crate::core::combat::{Combat, LogKind};
use crate::core::enemy::Intent;
use crate::ui::theme;
use crate::ui::{display_width, fit, put, put_padded, truncate};

/// 手牌一行放几张,两行正好十张
pub const GRID_COLS: usize = 5;
/// 手牌格子数
pub const HAND_SLOTS: usize = GRID_COLS * 2;
/// 窄到这个宽度以下,详情就换到手牌下面单独一行
const NARROW: u16 = 64;

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(c) = app.run.combat() else {
        put(buf, area.x, area.y, "(no combat)", theme::dim());
        return;
    };
    if area.height < 4 {
        return;
    }
    // 布局自下而上切:手牌 -> 日志 -> 敌人;每一块都夹在实际可用高度里
    let hand_h = if area.width >= NARROW { 3 } else { 4 };
    let hand_h = hand_h.min(area.height.saturating_sub(1));
    let enemy_h = (if area.height >= 24 { 5 } else { 4 })
        .min(area.height.saturating_sub(hand_h + 1))
        .max(2);
    let log_h = area.height.saturating_sub(enemy_h + hand_h);

    render_enemies(buf, Rect::new(area.x, area.y, area.width, enemy_h), app, c);
    render_log(buf, Rect::new(area.x, area.y + enemy_h, area.width, log_h), c);
    render_hand(
        buf,
        Rect::new(area.x, area.y + area.height - hand_h, area.width, hand_h),
        app,
        c,
    );
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
        // 血量 / 上限 / 格挡:同样是数字,不用血条
        let hp = format!("{}/{}", e.hp, e.max_hp);
        let bx = put2(buf, x + 2, y, inner_w, &hp, theme::fg(theme::BAD));
        put2(
            buf,
            bx,
            y,
            inner_w.saturating_sub((bx - x - 2) as usize),
            &format!("/{}", e.block),
            theme::fg(theme::BLOCK),
        );
        y += 1;
        if y >= area.y + area.height - 1 {
            continue;
        }
        // 意图(最关心的一行,紧跟血量那行)
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
        for (st, v) in e.statuses.iter() {
            second.push_str(&format!("{} {} ", st.short(), v));
        }
        put(buf, x + 2, y, &fit(&second, inner_w), theme::fg(theme::BUFF));
    }
}

/// 从 x 开始写一段文本,返回下一个可写位置(用于同一行拼不同颜色)
fn put2(buf: &mut Buffer, x: u16, y: u16, width: usize, text: &str, style: Style) -> u16 {
    let t = truncate(text, width);
    put(buf, x, y, &t, style);
    x + display_width(&t) as u16
}

fn intent_text(c: &Combat, i: usize) -> (String, Color) {
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
    // 行数不够时贴着底部画,紧挨着手牌
    let pad = area.height as usize - lines.len();
    for (i, line) in lines.iter().enumerate() {
        let style = match line.kind {
            LogKind::Info => theme::dim(),
            LogKind::Player => theme::fg(theme::GOOD),
            LogKind::Enemy => theme::fg(theme::BAD),
        };
        let y = area.y + (pad + i) as u16;
        put_padded(buf, area.x, y, &format!("  {}", line.text), area.width as usize, style);
    }
}

// ---- 手牌 ----

/// 手牌区:左边 5x2 的速记格子,右边是选中那张的详情
fn render_hand(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.height < 2 {
        return;
    }
    let narrow = area.width < NARROW;
    let rows_h = 2u16;
    // 能量写在手牌区第一行,离手牌最近;后面跟一句当前能不能打出去的提示
    let energy = format!("{}/{}", c.energy, c.max_energy);
    let mut ex = area.x + 2;
    ex = put2(
        buf,
        ex,
        area.y,
        8,
        &energy,
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD),
    );
    ex = put2(buf, ex, area.y, 12, " energy", theme::dim());
    let reason = if c.hand.is_empty() {
        None
    } else {
        c.blocked_reason(app.hand_sel.min(c.hand.len() - 1))
    };
    let hint = match reason {
        Some(r) => format!("cannot play: {r}"),
        None if c.hand.is_empty() => "no cards - press e to end the turn".to_string(),
        None => match c.hand.get(app.hand_sel.min(c.hand.len() - 1)) {
            Some(card) if card.needs_target() => match c.enemies.get(app.target_sel) {
                Some(e) if e.alive() => format!("[enter] play on {}", e.name),
                _ => "[enter] play".to_string(),
            },
            _ => "[enter] play".to_string(),
        },
    };
    let hint_style = if reason.is_some() {
        theme::fg(theme::WARN)
    } else {
        theme::dim()
    };
    let _ = put2(buf, ex + 2, area.y, 40, &hint, hint_style);
    // 格子整体缩进两格,和顶栏/能量的左边界对齐
    let gx = area.x + 2;
    if narrow {
        // 窄屏:详情单独占最后一行的整宽
        let cell_w = ((area.width.saturating_sub(4)) / GRID_COLS as u16).clamp(6, 16);
        let grid = Rect::new(gx, area.y + 1, cell_w * GRID_COLS as u16, rows_h);
        render_cells(buf, grid, app, c, cell_w);
        let y = area.y + 1 + rows_h;
        if y < area.y + area.height {
            let text = selected_text(app, c);
            put_padded(buf, gx, y, &text, (area.width as usize).saturating_sub(5), theme::fg(theme::FG));
        }
        return;
    }
    // 宽屏:左格子右详情
    let cell_w = ((area.width * 45 / 100) / GRID_COLS as u16).clamp(6, 14);
    let grid_w = cell_w * GRID_COLS as u16;
    render_cells(buf, Rect::new(gx, area.y + 1, grid_w, rows_h), app, c, cell_w);
    let dx = area.x + grid_w + 3;
    let dw = (area.x + area.width).saturating_sub(dx) as usize;
    if dw >= 16 {
        render_detail(buf, dx, area.y, dw, area.height, app, c);
    }
}

fn render_cells(buf: &mut Buffer, area: Rect, app: &App, c: &Combat, cell_w: u16) {
    for slot in 0..HAND_SLOTS {
        let row = (slot / GRID_COLS) as u16;
        let col = (slot % GRID_COLS) as u16;
        let x = area.x + col * cell_w;
        let y = area.y + row;
        if y >= buf.area.height || x >= buf.area.width {
            continue;
        }
        let Some(card) = c.hand.get(slot) else {
            continue;
        };
        let selected = slot == app.hand_sel;
        let playable = c.blocked_reason(slot).is_none();
        draw_cell(buf, x, y, cell_w.saturating_sub(1), card, selected, playable);
    }
}

/// 一个速记格子:费用黄、伤害红、+格挡蓝、-生命红……
fn draw_cell(buf: &mut Buffer, x: u16, y: u16, w: u16, card: &CardInstance, selected: bool, playable: bool) {
    if w == 0 {
        return;
    }
    let bg = if selected { theme::SEL_BG } else { theme::BG };
    put_padded(buf, x, y, "", w as usize, Style::default().bg(bg).fg(theme::FG));
    let mut cx = x;
    for (text, color) in card_tokens(card) {
        if cx >= x + w {
            break;
        }
        let style = if !playable {
            theme::dim()
        } else if selected {
            Style::default()
                .fg(color)
                .bg(theme::SEL_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            theme::fg(color)
        };
        cx = put2(buf, cx, y, (x + w - cx) as usize, &text, style);
        if cx < x + w {
            cx = put2(buf, cx, y, (x + w - cx) as usize, " ", Style::default().bg(bg));
        }
    }
}

/// 把一张牌压成"费用 + 效果"的短记号,颜色和顶栏同一套
fn card_tokens(card: &CardInstance) -> Vec<(String, Color)> {
    let mut out: Vec<(String, Color)> = Vec::new();
    match card.cost() {
        Cost::Fixed(n) => out.push((n.to_string(), theme::ENERGY)),
        Cost::X => out.push(("X".to_string(), theme::ENERGY)),
        Cost::Unplayable => out.push(("-".to_string(), theme::DIM)),
    }
    for e in card.effects() {
        let tok = match *e {
            Effect::Damage { amount, times } => Some((
                if times > 1 {
                    format!("{amount}x{times}")
                } else {
                    amount.to_string()
                },
                theme::BAD,
            )),
            Effect::DamageAll { amount, times } => Some((
                if times > 1 {
                    format!("{amount}x{times}A")
                } else {
                    format!("{amount}A")
                },
                theme::BAD,
            )),
            Effect::DamageRandom { amount, times } => Some((format!("{amount}R{times}"), theme::BAD)),
            Effect::DamageEqualBlock => Some(("=B".to_string(), theme::BAD)),
            Effect::DamageWithBonus { amount, .. } => {
                Some(((amount + card.bonus).to_string(), theme::BAD))
            }
            Effect::DamagePerStrike { base, .. } => Some((format!("{base}+"), theme::BAD)),
            Effect::DamageStrengthMult { amount, .. } => Some((amount.to_string(), theme::BAD)),
            Effect::DamageIfVulnerable { amount, .. } => Some((amount.to_string(), theme::BAD)),
            Effect::DamageAndKillMaxHp { amount, .. } => Some((amount.to_string(), theme::BAD)),
            Effect::DamagePerExhausted { per } => Some((format!("{per}E"), theme::BAD)),
            Effect::DamageAllX { per } => Some((format!("{per}X"), theme::BAD)),
            Effect::Reaper { amount } => Some((format!("{amount}A"), theme::BAD)),
            Effect::Block { amount } => Some((format!("+{amount}"), theme::BLOCK)),
            Effect::DoubleBlock => Some(("+Bx2".to_string(), theme::BLOCK)),
            Effect::BlockPerExhausted { per } => Some((format!("+{per}E"), theme::BLOCK)),
            Effect::LoseHp { amount } => Some((format!("-{amount}"), theme::BAD)),
            Effect::GainEnergy { n } => Some((format!("e{n}"), theme::ENERGY)),
            Effect::Draw { n } => Some((format!("d{n}"), theme::INFO)),
            Effect::AddSelfStatus { status, n }
            | Effect::AddTargetStatus { status, n }
            | Effect::AddAllEnemiesStatus { status, n } => Some((
                format!("{n}{}", status.short()),
                if status.is_debuff() {
                    theme::DEBUFF
                } else {
                    theme::BUFF
                },
            )),
            Effect::DoubleSelfStatus(s) => Some((format!("{}x2", s.short()), theme::BUFF)),
            Effect::ExhaustHand
            | Effect::ExhaustRandomInHand { .. }
            | Effect::ExhaustNonAttacks { .. }
            | Effect::ExhaustSelf => Some(("exh".to_string(), theme::DIM)),
            Effect::AddCardToDraw { .. } | Effect::AddCardToDiscard { .. } => {
                Some(("add".to_string(), theme::DIM))
            }
            Effect::UpgradeRandomInHand { .. } => Some(("up".to_string(), theme::INFO)),
            Effect::BonusSelf { .. } => None,
        };
        if let Some(tok) = tok {
            out.push(tok);
        }
    }
    out
}

/// 选中那张牌的详情:名字 + 费用/类型 + 完整描述
fn render_detail(buf: &mut Buffer, x: u16, y: u16, w: usize, h: u16, app: &App, c: &Combat) {
    let Some(card) = c.hand.get(app.hand_sel.min(c.hand.len().saturating_sub(1))) else {
        return;
    };
    let cost = match card.cost() {
        Cost::Fixed(n) => n.to_string(),
        Cost::X => "X".to_string(),
        Cost::Unplayable => "-".to_string(),
    };
    let head = format!("{}  {}  cost {}", card.label(), card.kind().name(), cost);
    put_padded(
        buf,
        x,
        y,
        &head,
        w,
        Style::default()
            .fg(theme::card_color(card.kind(), card.rarity()))
            .add_modifier(Modifier::BOLD),
    );
    let text = card.display_text();
    let lines = wrap_text(&text, w, (h.saturating_sub(1)) as usize);
    for (i, line) in lines.iter().enumerate() {
        put_padded(buf, x, y + 1 + i as u16, line, w, theme::fg(theme::FG));
    }
}

/// 窄屏用的单行详情
fn selected_text(app: &App, c: &Combat) -> String {
    let Some(card) = c.hand.get(app.hand_sel.min(c.hand.len().saturating_sub(1))) else {
        return String::new();
    };
    format!("{}: {}", card.label(), card.display_text())
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
