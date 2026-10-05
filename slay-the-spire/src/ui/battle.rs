// 战斗界面:上面是"角色区 | 敌人区",中间一条横线,下面是卡区.
// 角色区:30x10 的画像 + 一行增减益;卡区:能量 / 手牌 | 说明 / 牌堆小结.
// 敌人区按数量平分高度,每个敌人四行:血量 / 名字 / 缩进的动作 / 缩进的状态.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, Mode};
use crate::core::combat::Combat;
use crate::core::enemy::{EnemyFx, Intent};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_padded, truncate};

/// 角色区宽度:30 宽的画像 + 左右边框
const CHAR_W: u16 = 32;
/// 角色区高度:10 高的画像 + 一行增减益 + 上下边框
const CHAR_H: u16 = 13;
/// 画像高度
const ART_H: usize = 10;

/// 角色画像(30x10 的占位;以后换成真图只改这张表)
const HERO_ART: [&str; ART_H] = [
    r"                              ",
    r"            _____             ",
    r"           /     \            ",
    r"          | () () |           ",
    r"           \  ^  /            ",
    r"            |---|             ",
    r"        ___/     \___         ",
    r"       /   |     |   \        ",
    r"           |     |            ",
    r"          /_/   \_\           ",
];

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(c) = app.run.combat() else {
        put(buf, area.x, area.y, "(no combat)", theme::dim());
        return;
    };
    if area.height < 8 || area.width < 60 {
        return;
    }
    // 底部两行固定:信息行 + 命令栏
    let command = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let info = Rect::new(area.x, area.y + area.height - 2, area.width, 1);
    let main = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2));
    // 上排:角色区 | 敌人区;中间一条横线;下面是卡区.
    // 敌人多的时候上排要高一点,好让每个敌人都放得下框(每个要 4 行内容 + 2 行框)
    let need_h = c.enemies.len().max(1) as u16 * 6;
    // 卡区至少留 6 行
    let top_h = CHAR_H
        .max(need_h)
        .min(main.height.saturating_sub(7))
        .max(4);
    if top_h < 4 {
        // 太矮就只画卡区
        render_card_area(buf, main, app, c);
        render_info(buf, info, app, c);
        render_command(buf, command, app);
        return;
    }
    let top = Rect::new(main.x, main.y, main.width, top_h);
    render_character_box(buf, Rect::new(top.x, top.y, CHAR_W, top_h), c);
    let foe_x = top.x + CHAR_W + 1;
    render_enemies(
        buf,
        Rect::new(
            foe_x,
            top.y,
            top.width.saturating_sub(CHAR_W + 1),
            top_h,
        ),
        app,
        c,
    );
    crate::ui::hline(
        buf,
        main.x,
        main.y + top_h,
        main.width,
        crate::ui::BOX_H,
        theme::fg(theme::BORDER),
    );
    let card = Rect::new(
        main.x,
        main.y + top_h + 1,
        main.width,
        main.height.saturating_sub(top_h + 1),
    );
    render_card_area(buf, card, app, c);
    render_info(buf, info, app, c);
    render_command(buf, command, app);
}

// ---- 角色区 ----

/// 角色区:里面有 30x10 的画像,最下面一行是自己身上的增减益
fn render_character_box(buf: &mut Buffer, area: Rect, c: &Combat) {
    if area.width < 6 || area.height < 4 {
        return;
    }
    draw_box(buf, area, "you", theme::fg(theme::BORDER), theme::fg(theme::INFO));
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // 画像:占前面 ART_H 行,水平居中
    let art_rows = (inner.height as usize).saturating_sub(1).min(ART_H);
    for (i, line) in HERO_ART.iter().take(art_rows).enumerate() {
        let text = truncate(line, inner.width as usize);
        let w = display_width(&text);
        let x = inner.x + (inner.width as usize).saturating_sub(w) as u16 / 2;
        put(buf, x, inner.y + i as u16, &text, theme::fg(theme::FG));
    }
    // 增减益行
    let buff_y = inner.y + inner.height - 1;
    put_padded(buf, inner.x, buff_y, "", inner.width as usize, Style::default());
    let words: Vec<(String, Style)> = c
        .player
        .statuses
        .iter()
        .enumerate()
        .flat_map(|(i, (st, n))| {
            let sep = (i > 0).then(|| ("  ".to_string(), theme::fg(theme::FG)));
            let color = if st.is_debuff() { theme::DEBUFF } else { theme::BUFF };
            sep.into_iter()
                .chain([(format!("{} {}", st.short(), n), theme::fg(color))])
        })
        .collect();
    let len: usize = words.iter().map(|(t, _)| display_width(t)).sum();
    let mut cx = inner.x + (inner.width as usize).saturating_sub(len) as u16 / 2;
    for (t, st) in &words {
        put(buf, cx, buff_y, t, *st);
        cx += display_width(t) as u16;
    }
}

// ---- 卡区 ----

/// 卡区:第一行能量,中间"手牌 | 说明",最下面一行是牌堆小结
fn render_card_area(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.width < 8 || area.height < 3 {
        return;
    }
    draw_box(buf, area, "cards", theme::fg(theme::BORDER), theme::fg(theme::INFO));
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    if inner.width < 4 || inner.height < 3 {
        return;
    }
    // 能量行
    let energy = format!("{}/{} energy", c.energy, c.max_energy);
    put(
        buf,
        inner.x,
        inner.y,
        &energy,
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD),
    );
    // 手牌 | 说明
    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 2);
    if body.height > 0 {
        let split = crate::ui::split_list_detail(body, c.hand.len() as u16);
        crate::ui::draw_split_plain(buf, &split, theme::fg(theme::BORDER));
        let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
        let list = split.list;
        if list.width > 0 && list.height > 0 {
            let capacity = list.height as usize;
            let start = if c.hand.len() > capacity {
                sel.saturating_sub(capacity / 2).min(c.hand.len() - capacity)
            } else {
                0
            };
            for (i, card) in c.hand.iter().skip(start).take(capacity).enumerate() {
                let slot = start + i;
                let playable = c.blocked_reason(slot).is_none();
                crate::ui::put_card_line(
                    buf,
                    list.x,
                    list.y + i as u16,
                    list.width,
                    card,
                    slot == sel,
                    !playable,
                );
            }
        }
        if let Some(card) = c.hand.get(sel) {
            crate::ui::card_desc(buf, split.detail, card);
        }
    }
    // 牌堆小结
    let cy = inner.y + inner.height - 1;
    let w = inner.width as usize;
    let left = format!("draw {}", c.draw.len());
    let mid = format!("exhausted {}", c.exhaust.len());
    let right = format!("discard {}", c.discard.len());
    put_padded(buf, inner.x, cy, "", w, Style::default());
    put(buf, inner.x, cy, &left, theme::fg(theme::INFO));
    let mid_x = inner.x + (w.saturating_sub(display_width(&mid)) / 2) as u16;
    put(buf, mid_x, cy, &mid, theme::fg(theme::INFO));
    let right_x = inner.x + w.saturating_sub(display_width(&right)) as u16;
    put(buf, right_x, cy, &right, theme::fg(theme::INFO));
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
        render_enemy_block(buf, area, y0, slot_h, &lines, selected);
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
    // 本槽位放得下框才画;放不下就把槽位高度全给内容
    let framed = rows + 2 <= max_rows;
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
    if !framed {
        // 放不下框:只写内容,用满槽位
        for (r, line) in lines.iter().take(rows as usize).enumerate() {
            if y0 + r as u16 >= bottom {
                break;
            }
            put_padded(buf, x, y0 + r as u16, "", block_w, Style::default().bg(bg));
            let mut cx = x;
            for (text, style) in line {
                put(buf, cx, y0 + r as u16, text, style.bg(bg));
                cx += display_width(text) as u16;
            }
        }
        return;
    }
    let box_area = Rect::new(x.saturating_sub(1), y0, block_w as u16 + 2, rows + 2);
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

