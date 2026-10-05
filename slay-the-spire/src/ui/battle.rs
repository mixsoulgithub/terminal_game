// 战斗界面:纯文本优先,不拿方框占地方.
// 从上到下:遗物行 / 左角色区(带边框,边上是能量) | 右敌人区 / 信息行 / 命令栏.
// 角色区里是 10 行手牌速记(费用 + 牌名)、一条分隔线、再是选中那张的详情.
// 敌人区按敌人数量平分高度,每个敌人四行:血量 / 名字 / 缩进的动作 / 缩进的状态.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, Mode};
use crate::core::combat::Combat;
use crate::core::enemy::{EnemyFx, Intent};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_padded, truncate};

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(c) = app.run.combat() else {
        put(buf, area.x, area.y, "(no combat)", theme::dim());
        return;
    };
    if area.height < 6 || area.width < 40 {
        return;
    }
    // 底部两行固定:信息行 + 命令栏
    let command = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let info = Rect::new(area.x, area.y + area.height - 2, area.width, 1);
    let main = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2));
    render_main(buf, main, app, c);
    render_info(buf, info, app, c);
    render_command(buf, command, app);
}

/// 主区:左边角色区(带框),右边敌人区
fn render_main(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.height < 4 || area.width < 24 {
        return;
    }
    let char_w = (area.width * 45 / 100).clamp(24, area.width.saturating_sub(18));
    let char_area = Rect::new(area.x, area.y, char_w, area.height);
    let foe_x = area.x + char_w + 1;
    let foe_area = Rect::new(foe_x, area.y, (area.x + area.width).saturating_sub(foe_x), area.height);
    render_character(buf, char_area, app, c);
    render_enemies(buf, foe_area, app, c);
}

// ---- 角色区 ----

/// 角色区:边框上写着能量,里面是手牌速记 + 分隔线 + 选中牌的详情
fn render_character(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.width < 8 || area.height < 4 {
        return;
    }
    let energy = format!("{}/{} energy", c.energy, c.max_energy);
    draw_box(
        buf,
        area,
        &energy,
        theme::fg(theme::BORDER),
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD),
    );
    let iy = area.y + 1;
    let inner = Rect::new(
        area.x + 1,
        iy,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.width == 0 || inner.height < 3 {
        return;
    }
    // 最下面两行:一行增减益,一行牌堆小结,都不加分隔线
    let body = Rect::new(inner.x, inner.y, inner.width, inner.height - 2);
    let buff_y = inner.y + inner.height - 2;
    let summary_y = inner.y + inner.height - 1;
    // 手牌 + 描述:卡区固定用横线上下分(列表在上、说明在下)
    let split = crate::ui::split_list_detail_h(body, c.hand.len() as u16);
    crate::ui::draw_split(buf, body, &split, theme::fg(theme::BORDER));
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let list = split.list;
    let rows = list.height as usize;
    let start = if c.hand.len() > rows {
        sel.saturating_sub(rows / 2).min(c.hand.len() - rows)
    } else {
        0
    };
    for (i, card) in c.hand.iter().skip(start).take(rows).enumerate() {
        let slot = start + i;
        let playable = c.blocked_reason(slot).is_none();
        crate::ui::put_card_line(buf, list.x, list.y + i as u16, list.width, card, slot == sel, !playable);
    }
    // 描述:牌名 + 类型 + 完整描述(能量已经写在边框上了,不再重复)
    let Some(card) = c.hand.get(sel) else {
        return;
    };
    crate::ui::card_desc(buf, split.detail, card);
    // 描述与增减益之间横一条分隔线,两头接上框
    crate::ui::hsep(buf, inner, buff_y.saturating_sub(1), theme::fg(theme::BORDER));
    // 增减益行:自己身上的 buff/debuff
    let status_words: Vec<(String, Style)> = c
        .player
        .statuses
        .iter()
        .enumerate()
        .flat_map(|(i, (st, n))| {
            let sep = (i > 0).then(|| (" ".to_string(), theme::fg(theme::FG)));
            let color = if st.is_debuff() { theme::DEBUFF } else { theme::BUFF };
            sep.into_iter()
                .chain([(format!("{} {}", st.short(), n), theme::fg(color))])
        })
        .collect();
    if !status_words.is_empty() {
        let words = &status_words;
        let len: usize = words.iter().map(|(t, _)| display_width(t)).sum();
        let x = inner.x + (inner.width as usize).saturating_sub(len) as u16 / 2;
        put_padded(buf, inner.x, buff_y, "", inner.width as usize, Style::default());
        let mut cx = x;
        for (t, st) in words {
            put(buf, cx, buff_y, t, *st);
            cx += display_width(t) as u16;
        }
    }
    // 底行:draw / exhausted / discard
    let w = inner.width as usize;
    let left = format!("draw {}", c.draw.len());
    let mid = format!("exhausted {}", c.exhaust.len());
    let right = format!("discard {}", c.discard.len());
    put_padded(buf, inner.x, summary_y, "", w, Style::default());
    put(buf, inner.x, summary_y, &left, theme::fg(theme::INFO));
    let mid_x = inner.x + (w.saturating_sub(crate::ui::display_width(&mid)) / 2) as u16;
    put(buf, mid_x, summary_y, &mid, theme::fg(theme::INFO));
    let right_x = inner.x + w.saturating_sub(crate::ui::display_width(&right)) as u16;
    put(buf, right_x, summary_y, &right, theme::fg(theme::INFO));
}

// ---- 敌人区 ----

/// 敌人区:每个敌人平分一段高度,块贴右,纯文本不画框
fn render_enemies(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.width < 8 || area.height == 0 {
        return;
    }
    let n = c.enemies.len().max(1);
    let slot_h = (area.height as usize / n).max(1) as u16;
    for (i, e) in c.enemies.iter().enumerate() {
        let y0 = area.y + i as u16 * slot_h;
        if y0 >= area.y + area.height {
            break;
        }
        let selected = app.target_sel == i && e.alive();
        let lines = enemy_lines(c, i);
        // 框上下各占一行,内容最多放 slot_h - 2 行
        render_enemy_block(buf, area, y0, slot_h.saturating_sub(2), &lines, selected);
    }
}

/// 一个敌人的四行:血量/上限/格挡、名字、本回合动作、身上的状态.
/// 动作与状态相对名字缩进两格.
fn enemy_lines(c: &Combat, i: usize) -> Vec<Vec<(String, Style)>> {
    let e = &c.enemies[i];
    if !e.alive() {
        return vec![
            vec![("0/0/0".to_string(), theme::dim())],
            vec![("dead".to_string(), theme::dim())],
        ];
    }
    // 第一行:血量/上限/格挡
    let hp_line = vec![
        (format!("{}/{}", e.hp, e.max_hp), theme::fg(theme::BAD)),
        ("/".to_string(), theme::dim()),
        (e.block.to_string(), theme::fg(theme::BLOCK)),
    ];
    // 第二行:名字
    let name_line = vec![(
        e.name.clone(),
        Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
    )];
    // 第三行:本回合动作
    let mut action: Vec<(String, Style)> = vec![("  ".to_string(), theme::fg(theme::FG))];
    for (j, (text, color)) in enemy_action_tokens(c, i).into_iter().enumerate() {
        if j > 0 {
            action.push((" ".to_string(), theme::fg(theme::FG)));
        }
        action.push((text, theme::fg(color).add_modifier(Modifier::BOLD)));
    }
    // 第四行:身上的增减益
    let mut status: Vec<(String, Style)> = vec![("  ".to_string(), theme::fg(theme::FG))];
    for (j, (st, n)) in e.statuses.iter().enumerate() {
        if j > 0 {
            status.push((" ".to_string(), theme::fg(theme::FG)));
        }
        let color = if st.is_debuff() { theme::DEBUFF } else { theme::BUFF };
        status.push((format!("{} {}", st.short(), n), theme::fg(color)));
    }
    vec![hp_line, name_line, action, status]
}

/// 本回合动作记号:给玩家的减益病绿、自身增益紫、攻击红(带加减益后的实际值)、格挡蓝
fn enemy_action_tokens(c: &Combat, i: usize) -> Vec<(String, Color)> {
    let e = &c.enemies[i];
    if e.intent() == Intent::Sleep {
        return vec![("SLEEP".to_string(), theme::DIM)];
    }
    let Some(mv) = e.def.moves.get(e.next_move) else {
        return vec![("???".to_string(), theme::DIM)];
    };
    let (mut attack, mut block, mut buff, mut debuff) = (false, 0, false, false);
    for fx in mv.effects {
        match fx {
            EnemyFx::Attack { .. } => attack = true,
            EnemyFx::Block { amount } => block += *amount,
            EnemyFx::GainStatus { .. } => buff = true,
            EnemyFx::PlayerStatus { .. } => debuff = true,
        }
    }
    let mut out: Vec<(String, Color)> = Vec::new();
    if debuff {
        out.push(("DEBUFF".to_string(), theme::DEBUFF));
    }
    if buff {
        out.push(("BUFF".to_string(), theme::BUFF));
    }
    if attack {
        let (per, times) = c.predicted_damage(i);
        out.push((
            if times > 1 {
                format!("ATTACK {per}x{times}")
            } else {
                format!("ATTACK {per}")
            },
            theme::BAD,
        ));
    }
    if block > 0 {
        out.push((format!("BLOCK {}", c.intent_block(i)), theme::BLOCK));
    }
    if out.is_empty() {
        out.push(("???".to_string(), theme::DIM));
    }
    out
}

/// 把一个敌人的几行贴右画出来;整块宽度取最长一行,选中时铺底色
fn render_enemy_block(
    buf: &mut Buffer,
    area: Rect,
    y0: u16,
    max_rows: u16,
    lines: &[Vec<(String, Style)>],
    selected: bool,
) {
    let line_w = |line: &Vec<(String, Style)>| -> usize {
        line.iter().map(|(t, _)| display_width(t)).sum()
    };
    let block_w = lines.iter().map(line_w).max().unwrap_or(0);
    if block_w == 0 {
        return;
    }
    let rows = lines.len().min(max_rows as usize) as u16;
    if rows == 0 {
        return;
    }
    // 敌人区占满右边的剩余宽度;内容放在区中间一个 3/4 宽(向上取整)的框里,
    // 框的左右各留至少一格,不让文字贴到边上.
    let w = area.width as usize;
    let region_w = ((w * 3 + 3) / 4).min(w.saturating_sub(4)).max(1);
    let region_x = area.x + ((w - region_w) / 2) as u16;
    let centered = region_x + (region_w.saturating_sub(block_w) / 2) as u16;
    let min_x = area.x + 2;
    let max_x = (area.x + area.width)
        .saturating_sub(2 + block_w as u16)
        .max(min_x);
    let x = centered.clamp(min_x, max_x);
    let bg = theme::BG;
    let bottom = area.y + area.height;
    // 每个敌人都有框;选中的那个只把四个角点亮,边还是暗的
    let box_area = Rect::new(
        x.saturating_sub(1),
        y0,
        block_w as u16 + 2,
        rows + 2,
    );
    if box_area.y + box_area.height > bottom {
        // 放不下就退回不画框,只写内容
        for (r, line) in lines.iter().take(rows as usize).enumerate() {
            let mut cx = x;
            for (text, style) in line {
                put(buf, cx, y0 + r as u16, text, style.bg(bg));
                cx += display_width(text) as u16;
            }
        }
        return;
    }
    let edge = theme::fg(theme::BORDER);
    let corner = theme::fg(if selected { theme::SEL_FG } else { theme::BORDER });
    let (bx, by) = (box_area.x, box_area.y);
    let (bw, bh) = (box_area.width, box_area.height);
    if bw >= 2 && bh >= 2 {
        let top = format!(
            "┌{}┐",
            crate::ui::BOX_H.to_string().repeat((bw - 2) as usize)
        );
        let bottom = format!(
            "└{}┘",
            crate::ui::BOX_H.to_string().repeat((bw - 2) as usize)
        );
        put(buf, bx, by, &top, edge);
        put(buf, bx, by + bh - 1, &bottom, edge);
        for y in by + 1..by + bh - 1 {
            let v = crate::ui::BOX_V.to_string();
            put(buf, bx, y, &v, edge);
            put(buf, bx + bw - 1, y, &v, edge);
        }
        // 四角单独上色
        put(buf, bx, by, "┌", corner);
        put(buf, bx + bw - 1, by, "┐", corner);
        put(buf, bx, by + bh - 1, "└", corner);
        put(buf, bx + bw - 1, by + bh - 1, "┘", corner);
    }
    for (r, line) in lines.iter().take(rows as usize).enumerate() {
        let y = y0 + 1 + r as u16;
        put_padded(buf, x, y, "", block_w, Style::default().bg(bg));
        let mut cx = x;
        for (text, style) in line {
            put(buf, cx, y, text, style.bg(bg));
            cx += display_width(text) as u16;
        }
    }
}

// ---- 底部两栏 ----

/// 信息行:先说打不出去的原因,再顺带一句当前操作
fn render_info(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let reason = if c.hand.is_empty() {
        None
    } else {
        c.blocked_reason(sel)
    };
    let (text, style) = match reason {
        Some(r) => (format!("cannot play: {r}"), theme::fg(theme::WARN)),
        None if !app.msg.is_empty() => (app.msg.clone(), theme::fg(theme::FG)),
        None if c.hand.is_empty() => (
            "no cards - press e to end the turn".to_string(),
            theme::dim(),
        ),
        None => match c.hand.get(sel) {
            Some(card) if card.needs_target() => match c.enemies.get(app.target_sel) {
                Some(e) if e.alive() => (format!("[enter] play on {}", e.name), theme::dim()),
                _ => ("[enter] play".to_string(), theme::dim()),
            },
            _ => ("[enter] play".to_string(), theme::dim()),
        },
    };
    put(buf, area.x + 2, area.y, &truncate(&text, (area.width as usize).saturating_sub(2)), style);
}

/// 命令栏:平时只写 "? help";按 : 进入命令行时整条让给输入
fn render_command(buf: &mut Buffer, area: Rect, app: &App) {
    if app.mode == Mode::Command {
        let line = format!(":{}_", app.cmd);
        put_padded(buf, area.x, area.y, &line, area.width as usize, theme::selected());
        return;
    }
    put(buf, area.x + 2, area.y, "? help", theme::dim());
}

