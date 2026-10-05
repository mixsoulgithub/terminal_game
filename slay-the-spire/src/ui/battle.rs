// 战斗界面:上面是"说明区(带框) | 角色区 | 敌人区",中间一条横线,下面是卡牌列表.
// 角色区:30x10 的画像 + 一行增减益,不画框;卡牌列表:能量 / 一行手牌(│ 分隔)/ 牌堆小结.
// 敌人区按数量平分高度,每个敌人四行:血量 / 名字 / 缩进的动作 / 缩进的状态.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, Mode};
use crate::core::combat::Combat;
use crate::core::enemy::{EnemyFx, Intent};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_padded, truncate};

/// 角色区宽度:画像 30 宽
const CHAR_W: u16 = 30;
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
    // 上排:说明 | 角色 | 敌人;中间一条"能量线";下面两行:手牌 / 牌堆小结
    let top_h = main.height.saturating_sub(3).max(4);
    let top = Rect::new(main.x, main.y, main.width, top_h);
    let char_w = CHAR_W.min(top.width.saturating_sub(20));
    // 说明区占整宽的 1/4(向下取整,含边框),同时给角色和敌人各留出位置
    let desc_w = (top.width / 4)
        .max(10)
        .min(top.width.saturating_sub(char_w + 2 + 10));
    let desc = Rect::new(top.x, top.y, desc_w, top_h);
    let ch = Rect::new(desc.x + desc_w + 1, top.y, char_w, top_h);
    let foe_x = ch.x + char_w + 1;
    let foe = Rect::new(
        foe_x,
        top.y,
        (top.x + top.width).saturating_sub(foe_x),
        top_h,
    );
    render_desc_box(buf, desc, app, c);
    render_character(buf, ch, c);
    render_enemies(buf, foe, app, c);
    // 第一行:线里嵌着能量和牌堆小结,形如 -3/3 energy-----draw 5---exhausted 0---discard 0
    let sep_y = main.y + top_h;
    let energy = format!("{}/{} energy", c.energy, c.max_energy);
    let right = format!(
        "draw {}{}exhausted {}{}discard {}",
        c.draw.len(),
        crate::ui::BOX_H.to_string().repeat(3),
        c.exhaust.len(),
        crate::ui::BOX_H.to_string().repeat(3),
        c.discard.len()
    );
    let rw = display_width(&right) as u16;
    put(buf, main.x, sep_y, &crate::ui::BOX_H.to_string(), theme::fg(theme::BORDER));
    put(
        buf,
        main.x + 1,
        sep_y,
        &energy,
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD),
    );
    let used = 1 + display_width(&energy) as u16;
    let rx = (main.x + main.width).saturating_sub(rw).max(main.x + used);
    crate::ui::hline(
        buf,
        main.x + used,
        sep_y,
        rx.saturating_sub(main.x + used),
        crate::ui::BOX_H,
        theme::fg(theme::BORDER),
    );
    put(buf, rx, sep_y, &right, theme::fg(theme::INFO));
    // 第二行:一行手牌,两头 │ 框起来
    let cards_y = sep_y + 1;
    put_cards_row(buf, cards_y, main, app, c);
    // 第三行:收尾的横线
    crate::ui::hline(
        buf,
        main.x,
        sep_y + 2,
        main.width,
        crate::ui::BOX_H,
        theme::fg(theme::BORDER),
    );
    render_info(buf, info, app, c);
    render_command(buf, command, app);
}

// ---- 说明区 ----

/// 说明区:选中那张牌的说明,带框
fn render_desc_box(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.width < 6 || area.height < 3 {
        return;
    }
    draw_box(buf, area, "card", theme::fg(theme::BORDER), theme::fg(theme::INFO));
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let Some(card) = c.hand.get(sel) else {
        return;
    };
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    crate::ui::card_desc(buf, inner, card);
}

// ---- 角色区 ----

/// 角色区:30x10 的画像 + 一行增减益,不画框
fn render_character(buf: &mut Buffer, area: Rect, c: &Combat) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    // 画像 + 增减益行整块在区域里垂直居中(上下就是遗物行和能量线)
    let art_rows = ART_H.min(area.height as usize);
    let block_h = art_rows + 1;
    let off = (area.height as usize).saturating_sub(block_h) / 2;
    for (i, line) in HERO_ART.iter().take(art_rows).enumerate() {
        let text = truncate(line, area.width as usize);
        let x = area.x + (area.width as usize).saturating_sub(display_width(&text)) as u16 / 2;
        put(buf, x, area.y + off as u16 + i as u16, &text, theme::fg(theme::FG));
    }
    // 增减益行
    let buff_y = area.y + off as u16 + art_rows as u16;
    if buff_y >= area.y + area.height {
        return;
    }
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
    let mut cx = area.x + (area.width as usize).saturating_sub(len) as u16 / 2;
    for (t, st) in &words {
        put(buf, cx, buff_y, t, *st);
        cx += display_width(t) as u16;
    }
}

// ---- 卡牌列表 ----

/// 一行手牌:两头 │ 框住,每张等宽,名字放不下就截断
fn put_cards_row(buf: &mut Buffer, y: u16, area: Rect, app: &App, c: &Combat) {
    let n = c.hand.len();
    if n == 0 || area.width < 3 {
        return;
    }
    let bar = |buf: &mut Buffer, x: u16| {
        put(buf, x, y, &crate::ui::BOX_V.to_string(), theme::fg(theme::BORDER));
    };
    bar(buf, area.x);
    // 每格贴着文字宽度,放不下就截断名字
    let sel = app.hand_sel.min(n - 1);
    let mut cx = area.x + 1;
    let last_x = area.x + area.width - 1;
    for (k, card) in c.hand.iter().enumerate() {
        let want = display_width(&crate::ui::cost_label(card)) + 1 + display_width(&card.label());
        let remain = last_x.saturating_sub(cx) as usize;
        if remain == 0 {
            break;
        }
        let w = want.min(remain.saturating_sub(1)).max(1);
        let playable = c.blocked_reason(k).is_none();
        crate::ui::put_card_line(buf, cx, y, w as u16, card, k == sel, !playable);
        cx += w as u16;
        bar(buf, cx);
        cx += 1;
    }
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

