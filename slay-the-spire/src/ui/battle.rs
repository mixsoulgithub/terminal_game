// 战斗界面:上面是敌人,下面是手牌(战斗日志挪到 H 的历史窗口里).
// 手牌是 5 列 2 行共 10 张牌,每张占两行:第一行牌名,第二行费用与效果速记;
// 只有选中的那张在右侧详情区给出全名与完整描述;能量放在手牌旁边.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::App;
use crate::core::card::{CardInstance, Cost, Effect};
use crate::core::combat::Combat;
use crate::core::enemy::Intent;
use crate::ui::theme;
use crate::ui::{display_width, fit, put, put_padded, truncate};

/// 手牌一行放几张,两行正好十张
pub const GRID_COLS: usize = 5;
/// 手牌格子数
pub const HAND_SLOTS: usize = GRID_COLS * 2;
/// 每张牌的固定格宽(算上右边的间隔):排面不跟着牌名长短跳
const CARD_W: u16 = 8;
/// 敌人框高度:边框 + 血量 + 意图 + 状态
const ENEMY_BOX_H: u16 = 5;
/// 每张牌占几行:牌名一行 + 速记一行
const CARD_LINES: u16 = 2;

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(c) = app.run.combat() else {
        put(buf, area.x, area.y, "(no combat)", theme::dim());
        return;
    };
    if area.height < HAND_H + 3 {
        return;
    }
    // 手牌区固定在底部,剩下的全给"战场":敌人框在战场里垂直居中
    let hand_area = Rect::new(
        area.x,
        area.y + area.height - HAND_H,
        area.width,
        HAND_H,
    );
    let field = Rect::new(area.x, area.y, area.width, area.height - HAND_H);
    render_enemies(buf, field, app, c);
    render_hand(buf, hand_area, app, c);
}

/// 手牌区高度:能量行 + 两行卡片 × 每张两行
const HAND_H: u16 = 1 + 2 * CARD_LINES;

fn render_enemies(buf: &mut Buffer, field: Rect, app: &App, c: &Combat) {
    let n = c.enemies.len().max(1);
    let gap = 1u16;
    let box_h = ENEMY_BOX_H.min(field.height);
    // 战场里垂直居中,敌人别贴着上沿
    let area = Rect::new(
        field.x,
        field.y + field.height.saturating_sub(box_h) / 2,
        field.width,
        box_h,
    );
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

// ---- 手牌 ----

/// 手牌区:左边 5 列 2 行的牌,右边是选中那张的详情
fn render_hand(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.height < HAND_H {
        return;
    }
    let gx = area.x + 2;
    // 格子宽度固定,不跟着牌名长短跳
    let grid_w = CARD_W * GRID_COLS as u16;
    // 能量写在手牌区第一行,离手牌最近;后面跟一句当前能不能打出去的提示
    let energy = format!("{}/{}", c.energy, c.max_energy);
    let mut ex = gx;
    ex = put2(
        buf,
        ex,
        area.y,
        8,
        &energy,
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD),
    );
    ex = put2(buf, ex, area.y, 12, " energy", theme::dim());
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let reason = if c.hand.is_empty() {
        None
    } else {
        c.blocked_reason(sel)
    };
    let hint = match reason {
        Some(r) => format!("cannot play: {r}"),
        None if c.hand.is_empty() => "no cards - press e to end the turn".to_string(),
        None => match c.hand.get(sel) {
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
    // 提示写在能量后面,宽度到详情列为止
    let hint_room = (gx + grid_w + 1).saturating_sub(ex + 2) as usize;
    let _ = put2(buf, ex + 2, area.y, hint_room, &hint, hint_style);

    render_cards(
        buf,
        Rect::new(gx, area.y + 1, grid_w, CARD_LINES * 2),
        app,
        c,
    );
    let dx = area.x + grid_w + 3;
    let dw = (area.x + area.width).saturating_sub(dx) as usize;
    if dw >= 16 {
        render_detail(buf, dx, area.y, dw, area.height, app, c);
    }
}

fn render_cards(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    for slot in 0..HAND_SLOTS {
        let col = (slot % GRID_COLS) as u16;
        let row = (slot / GRID_COLS) as u16;
        let x = area.x + col * CARD_W;
        let y = area.y + row * CARD_LINES;
        if y >= buf.area.height || x >= buf.area.width {
            continue;
        }
        let Some(card) = c.hand.get(slot) else {
            continue;
        };
        let selected = slot == app.hand_sel;
        let playable = c.blocked_reason(slot).is_none();
        draw_card(buf, x, y, CARD_W, card, selected, playable);
    }
}

/// 一张牌占两行:第一行牌名,第二行费用与效果速记.
/// 打不出去的整张压暗,等于告诉你能量不够;格宽固定,排面不会跳.
fn draw_card(buf: &mut Buffer, x: u16, y: u16, w: u16, card: &CardInstance, selected: bool, playable: bool) {
    if w == 0 {
        return;
    }
    let bg = if selected { theme::SEL_BG } else { theme::BG };
    let blank = Style::default().bg(bg).fg(theme::FG);
    put_padded(buf, x, y, "", (w - 1) as usize, blank);
    put_padded(buf, x, y + 1, "", (w - 1) as usize, blank);
    let name_style = if !playable {
        theme::dim().bg(bg)
    } else {
        theme::fg(theme::card_color(card.kind(), card.rarity())).bg(bg)
    };
    let inner = (w - 1) as usize;
    let cx = put2(buf, x, y, inner, &format!(" {}", card.label()), name_style);
    if selected {
        put2(buf, cx, y, (x + w - cx) as usize, " <", theme::selected());
    }
    // 第二行:费用黄、伤害红、+格挡蓝、-生命红……前面空一格和牌名对齐
    let mut cx = put2(buf, x, y + 1, 1, " ", blank);
    for (text, color) in card_tokens(card) {
        if cx >= x + w - 1 {
            break;
        }
        let style = if !playable {
            theme::dim().bg(bg)
        } else {
            theme::fg(color).bg(bg)
        };
        cx = put2(buf, cx, y + 1, (x + w - 1 - cx) as usize, &text, style);
        if cx < x + w - 1 {
            cx = put2(buf, cx, y + 1, (x + w - 1 - cx) as usize, " ", blank);
        }
    }
}

/// 把一张牌压成"费用 + 效果"的短记号(颜色和顶栏同一套)
fn card_tokens(card: &CardInstance) -> Vec<(String, ratatui::style::Color)> {
    let mut out: Vec<(String, ratatui::style::Color)> = Vec::new();
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
