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
use crate::ui::{display_width, put, put_padded, truncate};

/// 画像/血条宽度
const IMG_W: usize = 20;
/// 画像高度
const ART_H: usize = 10;

/// 铁甲战士画像(20x10,由 assets/characters/tuned/ironclad-s4-auto-b1.30.png 转出)
const HERO_ART: [&str; ART_H] = [
    r"      *=&*-",
    r"     -&&&=/",
    r"  -/&&+/++&=/",
    r"--//*+////**/",
    r"    -/////--",
    r"   -++///++-",
    r" -/+/-  -//+/",
    r" *=+       -*=-",
    r" -*          -++",
    r" -            -/",
];

/// 画像每格的 RGB(由 ascii-image-converter -C 的输出解析而来,空格不画)
const HERO_FG: [[u32; 20]; ART_H] = [
    // 每格的 RGB(空格不画,颜色无所谓,留 0)
    [0x000000, 0x000000, 0x010101, 0x010101, 0x000000, 0x000000, 0x8b8b89, 0xc3c18b, 0xd7cb7a, 0xa3a267, 0x282715, 0x000000, 0x030302, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x000000, 0x000000, 0x000000, 0x000000, 0x0d0d09, 0x38362c, 0xcac7c0, 0xd7d4ae, 0xd5cd87, 0xc9b96a, 0x504d2c, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x191d1e, 0x1b1f20, 0x233033, 0x434641, 0xddd6b1, 0xdcd9b5, 0x6d6e64, 0x494847, 0x646667, 0x7d7d79, 0xccc9a0, 0xc4be92, 0x4e4d3f, 0x000000, 0x010101, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x2d3738, 0x2e3637, 0x34454a, 0x474d50, 0x8e9191, 0x646d6e, 0x515658, 0x565b5e, 0x585b5c, 0x5d5f5c, 0x81837a, 0x9b9684, 0x514d43, 0x000000, 0x020201, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x000000, 0x000000, 0x000000, 0x000000, 0x2d2b28, 0x62534d, 0x574c3f, 0x4d4734, 0x4a402e, 0x4e4133, 0x281f1e, 0x343935, 0x1d1c19, 0x000000, 0x010101, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x010102, 0x000000, 0x070303, 0x592624, 0xb15751, 0xb34944, 0x9a4240, 0x904b44, 0x9a4643, 0xad4644, 0xad4845, 0x4e1d1c, 0x000000, 0x000000, 0x010101, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x030302, 0x3f2a23, 0x98403b, 0xba4f4a, 0xab3e3b, 0x682423, 0x32100f, 0x2b0c0b, 0x481817, 0x782927, 0xa53f3b, 0xaa4440, 0x7e2b2a, 0x0d0202, 0x000000, 0x020101, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x000000, 0x8d8066, 0xceb596, 0x897162, 0x2c1513, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x0f0606, 0x411f1d, 0xb6856e, 0xb4a48a, 0x3b3b35, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000],
    [0x060606, 0x272826, 0x9d9e93, 0x1a1f1e, 0x000000, 0x020101, 0x010000, 0x010000, 0x020000, 0x010000, 0x000000, 0x000000, 0x131914, 0x353531, 0x7e786b, 0x6b675f, 0x050606, 0x000000, 0x000000, 0x000000],
    [0x0d0d0d, 0x2e2c29, 0x1a1918, 0x040304, 0x020000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x010000, 0x010000, 0x000000, 0x000000, 0x35322e, 0x4b4944, 0x020202, 0x000000, 0x000000, 0x000000],
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
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let card = c.hand.get(sel);
    // 底部:能量+小结线 / 手牌行 / 成本+类型线 / 说明两行 / 收尾线
    let desc_text = card.map(|x| x.display_text()).unwrap_or_default();
    let desc_lines = 2u16;
    let bottom_h = 3 + desc_lines + 1;
    let top_h = main.height.saturating_sub(bottom_h).max(4);
    let top = Rect::new(main.x, main.y, main.width, top_h);
    render_character(
        buf,
        top,
        c,
        app.battle_shake_offset(crate::core::combat::ShakeWho::Hero),
    );
    render_enemies(buf, top, app, c);

    let mut y = main.y + top_h;
    // 第一行:能量在左,牌堆数量在右,中间用 ─ 补满
    let right = format!(
        "in hand {}/undrawn {}/exhausted {}/discard {}",
        c.hand.len(),
        c.draw.len(),
        c.exhaust.len(),
        c.discard.len()
    );
    let rw = display_width(&right) as u16;
    put(buf, main.x, y, &crate::ui::BOX_H.to_string(), theme::fg(theme::BORDER));
    let ecolor = theme::energy_color(energy_key(app));
    let mut ex = crate::ui::put_cost_token(buf, main.x + 1, y, &c.energy.to_string(), ecolor, theme::FG);
    put(buf, ex, y, "/", theme::fg(theme::BORDER));
    ex += 1;
    ex = crate::ui::put_cost_token(buf, ex, y, &c.max_energy.to_string(), ecolor, theme::FG);
    put(buf, ex, y, " energy", theme::fg(theme::DIM));
    ex += display_width(" energy") as u16;
    let used0 = ex - main.x;
    let rx = (main.x + main.width).saturating_sub(rw).max(main.x + used0);
    crate::ui::hline(
        buf,
        main.x + used0,
        y,
        rx.saturating_sub(main.x + used0),
        crate::ui::BOX_H,
        theme::fg(theme::BORDER),
    );
    // 中间空出来的地方:有待选择的牌就写提示
    if let Some(hint) = app.select_hint() {
        let hw = display_width(&hint) as u16;
        let gap = rx.saturating_sub(main.x + used0);
        if hw < gap {
            put(
                buf,
                main.x + used0 + (gap - hw) / 2,
                y,
                &hint,
                theme::fg(theme::YELLOW),
            );
        }
    }
    put(buf, rx, y, &right, theme::fg(theme::INFO));
    y += 1;
    // 手牌一行
    put_cards_row(buf, y, main, app, c);
    y += 1;
    // 这一行:卡牌自己的费用在左,类型在右
    put(buf, main.x, y, &crate::ui::BOX_H.to_string(), theme::fg(theme::BORDER));
    let used = match card {
        Some(card) => {
            let (num, kind, digit) = crate::ui::card_cost_token(card);
            let ex = crate::ui::put_cost_token(buf, main.x + 1, y, &num, kind, digit);
            put(buf, ex, y, " energy", theme::fg(theme::DIM));
            1 + display_width(&num) as u16 + 2 + display_width(" energy") as u16
        }
        None => {
            put(buf, main.x + 1, y, "-", theme::dim());
            2
        }
    };
    crate::ui::hline(
        buf,
        main.x + used,
        y,
        main.width.saturating_sub(used),
        crate::ui::BOX_H,
        theme::fg(theme::BORDER),
    );
    if let Some(card) = card {
        // 类型居中,画在横线上面
        let kind = crate::ui::kind_label(card.kind());
        let kw = display_width(&kind) as u16;
        let kx = main.x + (main.width.saturating_sub(kw)) / 2;
        put(buf, kx, y, &kind, theme::fg(theme::FG));
    }
    y += 1;
    // 说明文本:固定两行,居中,伤害/格挡上色
    let words = crate::ui::desc_words(&desc_text, crate::ui::run_energy_color(&app.run));
    let mut drawn = 0u16;
    for line in crate::ui::wrap_words(&words, main.width as usize)
        .into_iter()
        .take(desc_lines as usize)
    {
        crate::ui::put_centered_words(buf, main.x, y, main.width as usize, &line);
        y += 1;
        drawn += 1;
    }
    y += desc_lines - drawn;
    // 收尾线
    crate::ui::hline(buf, main.x, y, main.width, crate::ui::BOX_H, theme::fg(theme::BORDER));

    render_info(buf, info, app, c);
    render_command(buf, command, app);
    // 战斗里也要有命令行补全提示
    crate::ui::command_hints(buf, command, app);
}

// ---- 角色区 ----

/// 靠屏幕左半边向下取整,右半边向上取整
fn round_by_side(v: f32, width: u16) -> u16 {
    if v < width as f32 / 2.0 {
        v.floor() as u16
    } else {
        v.ceil() as u16
    }
}

/// 角色:画像的重心放在屏幕 1/4 处,增减益跟着画像中心。
/// off 是抖动偏移(掉血往左退、出手往右冲)。
fn render_character(buf: &mut Buffer, area: Rect, c: &Combat, off: i32) {
    if area.width == 0 || area.height < 14 {
        return;
    }
    let art_rows = ART_H;
    let block_h = (art_rows + 1) as u16; // 画像 + 增减益(不再有血条)
    let voff = (area.height as usize).saturating_sub(block_h as usize) / 2;
    // 画像中心落在屏幕 1/4 处
    let center = area.x as f32 + area.width as f32 / 4.0;
    let cx = round_by_side(center - IMG_W as f32 / 2.0, area.width);
    let cx = (cx as i32 + off).max(area.x as i32) as u16;
    // 画像
    for (i, line) in HERO_ART.iter().enumerate() {
        let y = area.y + voff as u16 + i as u16;
        for (j, ch) in line.char_indices() {
            if ch == ' ' || j >= area.width as usize {
                continue;
            }
            let col = HERO_FG[i][j];
            let style = Style::default().fg(Color::Rgb(
                (col >> 16) as u8,
                ((col >> 8) & 0xff) as u8,
                (col & 0xff) as u8,
            ));
            put(buf, cx + j as u16, y, &ch.to_string(), style);
        }
    }
    let buff_y = area.y + voff as u16 + art_rows as u16;
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
    let mut bx = cx + (IMG_W.saturating_sub(len) / 2) as u16;
    for (t, st) in &words {
        put(buf, bx, buff_y, t, *st);
        bx += display_width(t) as u16;
    }
}

/// 一行手牌:行首一个 │,每张牌后面跟一个 │,宽度贴着内容
fn put_cards_row(buf: &mut Buffer, y: u16, area: Rect, app: &App, c: &Combat) {
    let n = c.hand.len();
    if n == 0 || area.width < 3 {
        return;
    }
    let bar = |buf: &mut Buffer, x: u16, ch: &str| {
        put(
            buf,
            x,
            y,
            ch,
            theme::fg(if ch == "│" { theme::BORDER } else { theme::SEL_FG }),
        );
    };
    // 每张牌占的格数(按内容),整行连竖线一起居中;选中的那张要多留两个方格放 []
    let sel = app.hand_sel.min(n - 1);
    let lengths: Vec<usize> = c
        .hand
        .iter()
        .map(|card| display_width(&crate::ui::cost_label(card)) + 1 + display_width(&card.label()))
        .collect();
    // {} 只画在光标那张;已经选中的(背景色)不画括号
    let marked = |k: usize| k == sel;
    let highlighted = |k: usize| k == sel || app.choice_sel.contains(&k);
    // 选择模式下,"能不能选"决定压暗而不是"能不能打"
    let choosable: Option<Vec<usize>> = app
        .run
        .combat()
        .filter(|c| c.choice.is_some())
        .map(|c| c.choice_candidates().into_iter().map(|(i, _)| i).collect());
    let widths: Vec<usize> = lengths.clone();
    // 分隔符:选中的牌把它左右两根竖线换成 [ 和 ](宽度不变)
    let bar_at = |before: usize| -> &'static str {
        if before < n && marked(before) {
            "{"
        } else if before > 0 && marked(before - 1) {
            "}"
        } else {
            "│"
        }
    };
    let total: usize = widths.iter().sum::<usize>() + n + 1;
    let mut cx = if total <= area.width as usize {
        area.x + ((area.width as usize - total) / 2) as u16
    } else {
        area.x
    };
    let last_x = area.x + area.width - 1;
    bar(buf, cx, bar_at(0));
    cx += 1;
    for (k, card) in c.hand.iter().enumerate() {
        let want = widths[k];
        let remain = last_x.saturating_sub(cx) as usize;
        if remain == 0 {
            break;
        }
        let w = want.min(remain.saturating_sub(1)).max(1);
        let dim = match &choosable {
            Some(list) => !list.contains(&k),
            None => c.blocked_reason(k).is_some(),
        };
        crate::ui::put_card_cell(buf, cx, y, w as u16, card, highlighted(k), dim);
        cx += w as u16;
        bar(buf, cx, bar_at(k + 1));
        cx += 1;
    }
}

// ---- 敌人区 ----

/// 敌人区:每个敌人平分一段高度,块贴右,纯文本不画框
/// 玩家能量括号用角色自己的颜色
fn energy_key(app: &App) -> &'static str {
    crate::core::roster::find(app.run.character)
        .map(|c| c.color)
        .unwrap_or("gray")
}

fn render_enemies(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if area.width < 8 || area.height == 0 || c.enemies.is_empty() {
        return;
    }
    let n = c.enemies.len();
    let gap = 4u16;
    let widths: Vec<u16> = (0..n).map(|i| enemy_block_w(c, i)).collect();
    let total: u16 = widths.iter().sum::<u16>() + gap * (n as u16 - 1);
    // 整组重心和角色的 1/4 对称,放在屏幕 3/4 处
    let mut x0 = area.x as f32 + area.width as f32 * 3.0 / 4.0 - total as f32 / 2.0;
    // 最右那个框的右边不能超过屏幕的 19/20
    let limit = area.x as f32 + area.width as f32 * 19.0 / 20.0;
    if x0 + total as f32 > limit {
        x0 = limit - total as f32;
    }
    let mut x = round_by_side(x0.max(area.x as f32), area.width);
    for (i, e) in c.enemies.iter().enumerate() {
        if x >= area.x + area.width {
            break;
        }
        let selected = app.target_sel == i && e.alive();
        let lines = enemy_lines(c, i);
        let w = widths[i].min((area.x + area.width).saturating_sub(x)).max(1);
        // 抖动:这个敌人在挨打时往右退、出手时往左冲
        let off = app.battle_shake_offset(crate::core::combat::ShakeWho::Enemy(i));
        let bx = (x as i32 + off).clamp(area.x as i32, (area.x + area.width) as i32 - 1) as u16;
        render_enemy_block(buf, Rect::new(bx, area.y, w, area.height), &lines, selected);
        x += widths[i] + gap;
    }
}

/// 敌人框的总宽(内容 + 左右边框)
fn enemy_block_w(c: &Combat, i: usize) -> u16 {
    let lines = enemy_lines(c, i);
    let w = lines
        .iter()
        .map(|l| l.iter().map(|(t, _)| display_width(t)).sum::<usize>())
        .max()
        .unwrap_or(0);
    w as u16 + 2
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
fn render_enemy_block(buf: &mut Buffer, slot: Rect, lines: &[Vec<(String, Style)>], selected: bool) {
    let line_w = |line: &Vec<(String, Style)>| -> usize {
        line.iter().map(|(t, _)| display_width(t)).sum()
    };
    let block_w = lines.iter().map(line_w).max().unwrap_or(0);
    if block_w == 0 || slot.width == 0 || slot.height == 0 {
        return;
    }
    let rows = lines.len() as u16;
    let bg = theme::BG;
    let box_w = block_w as u16 + 2;
    let box_h = rows + 2;
    // 槽位里居中;放不下框就只居中写内容
    let framed = box_w <= slot.width && box_h <= slot.height;
    let (x, y) = if framed {
        (
            slot.x + (slot.width - box_w) / 2 + 1,
            slot.y + (slot.height - box_h) / 2 + 1,
        )
    } else {
        let w = block_w.min(slot.width as usize) as u16;
        (
            slot.x + slot.width.saturating_sub(w) / 2,
            slot.y + slot.height.saturating_sub(rows) / 2,
        )
    };
    // 不画灰色的框,只有选中的那个敌人才点四个角
    if framed && selected {
        let bx = x - 1;
        let by = y - 1;
        let corner = theme::fg(theme::SEL_FG);
        put(buf, bx, by, "┌", corner);
        put(buf, bx + box_w - 1, by, "┐", corner);
        put(buf, bx, by + box_h - 1, "└", corner);
        put(buf, bx + box_w - 1, by + box_h - 1, "┘", corner);
    }
    for (r, line) in lines.iter().enumerate() {
        let yy = y + r as u16;
        if yy >= slot.y + slot.height {
            break;
        }
        put_padded(buf, x, yy, "", block_w, Style::default().bg(bg));
        let mut cx = x;
        for (text, style) in line {
            put(buf, cx, yy, text, style.bg(bg));
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
    // 提示要抖的时候整句话往右挪一格(和商店那套一致)
    let x = area.x + 2 + app.shake_nudge() as u16;
    put(buf, x, area.y, &truncate(&text, (area.width as usize).saturating_sub(2)), style);
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

