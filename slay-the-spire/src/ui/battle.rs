// 战斗界面:上面是"说明区(带框) | 角色区 | 敌人区",中间一条横线,下面是卡牌列表.
// 角色区:20x10 的画像 + 两行增减益徽章,不画框;卡牌列表:能量 / 一行手牌(│ 分隔)/ 牌堆小结.
// 敌人区按数量平分高度,每个敌人五行:意图 / 格挡-血量 / 名字 / 状态图标 / 状态数值.
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
    // 底部固定一行:左边是提示/原因,右边是 "? help"(按 : 时整条让给命令行)
    let status = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let main = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let card = c.hand.get(sel);
    // 底部:能量+小结线 / 手牌行 / 成本+类型线 / 说明两行 / 收尾线
    let desc_text = card.map(|x| x.display_text()).unwrap_or_default();
    let desc_lines = 2u16;
    // 手牌放不下一行就用两行(行数由同一个分行函数决定)
    let hand_lines = hand_row_layout(c, main.width as usize, 2).len() as u16;
    let bottom_h = 2 + hand_lines + 1 + desc_lines + 1;
    let top_h = main.height.saturating_sub(bottom_h).max(4);
    let top = Rect::new(main.x, main.y, main.width, top_h);
    render_character(
        buf,
        top,
        c,
        app.battle_shake_offset(crate::core::combat::ShakeWho::Hero),
    );
    render_enemies(buf, top, app, c);

    // 浩劫链播报:叠在战斗区上方,一层一行(缩进表示嵌套)
    for (i, (depth, label, _)) in app.play_banners.iter().enumerate() {
        let y = main.y + i as u16;
        if y >= main.y + top_h {
            break;
        }
        let text = format!("Havoc plays {label}");
        let w = display_width(&text) + 4 + (depth.saturating_sub(1) as usize) * 2;
        if w as u16 + 4 >= main.width {
            break;
        }
        let x = main.x + (main.width - w as u16) / 2;
        put_padded(
            buf,
            x,
            y,
            "",
            w,
            Style::default().bg(theme::SEL_BG),
        );
        put(
            buf,
            x + 2,
            y,
            &text,
            Style::default()
                .fg(theme::YELLOW)
                .bg(theme::SEL_BG)
                .add_modifier(Modifier::BOLD),
        );
    }
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
    // 手牌(一到两行)
    put_cards_row(buf, y, main, app, c, hand_lines);
    y += hand_lines;
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

    render_status(buf, status, app, c);
    // 战斗里也要有命令行补全提示
    crate::ui::command_hints(buf, status, app);
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
    // 只要塞得下"画像 + 两行徽章"就画;14 是早先留下的余量,没有别的含义
    if area.width == 0 || area.height < (ART_H + 2) as u16 {
        return;
    }
    let art_rows = ART_H;
    let block_h = (art_rows + 2) as u16; // 画像 + 两行徽章
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
    // 增减益也是两行徽章(上行图标、下行数值,每格 3 列),在画像下面居中
    let (icons, values) = status_badge_lines(&c.player.statuses);
    for (r, line) in [&icons, &values].into_iter().enumerate() {
        let len: usize = line.iter().map(|(t, _)| display_width(t)).sum();
        let mut bx = cx + (IMG_W.saturating_sub(len) / 2) as u16;
        for (t, st) in line {
            put(buf, bx, buff_y + r as u16, t, *st);
            bx += display_width(t) as u16;
        }
    }
}

/// 一行手牌:行首一个 │,每张牌后面跟一个 │,宽度贴着内容
/// 手牌分几行(每张牌不跨行):能一行就一行,否则两行均分,
/// 且第一行不短于第二行(长度接近时优先让第一行更长)
fn hand_row_layout(c: &Combat, width: usize, lines_max: u16) -> Vec<Vec<usize>> {
    let n = c.hand.len();
    if n == 0 {
        return vec![Vec::new()];
    }
    let card_w = |card: &crate::core::card::CardInstance| -> usize {
        display_width(&crate::ui::cost_label(card)) + 1 + display_width(&card.label())
    };
    // 一行:行首竖线 + 每格(内容 + 尾巴竖线)
    let row_w = |range: &[usize]| -> usize {
        range.iter().map(|k| card_w(&c.hand[*k]) + 2).sum::<usize>() + 1
    };
    let all: Vec<usize> = (0..n).collect();
    if lines_max < 2 || row_w(&all) <= width {
        return vec![all];
    }
    // 两行:挑差距最小的一刀,第一行短于第二行的方案加重罚
    let mut best: Option<(i32, usize)> = None;
    for k in 1..n {
        let (w1, w2) = (row_w(&all[..k]), row_w(&all[k..]));
        if w1 > width || w2 > width {
            continue;
        }
        let score = if w2 > w1 {
            1000 + (w2 - w1) as i32
        } else {
            (w1 - w2) as i32
        };
        if best.map_or(true, |(b, _)| score < b) {
            best = Some((score, k));
        }
    }
    if let Some((_, k)) = best {
        return vec![all[..k].to_vec(), all[k..].to_vec()];
    }
    // 怎么切都塞不下(牌大到一行一张都放不下):退回贪心装填
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 1usize;
    for k in 0..n {
        let cell = card_w(&c.hand[k]) + 2;
        if !rows.last().unwrap().is_empty() && used + cell > width {
            if rows.len() as u16 >= lines_max {
                break;
            }
            rows.push(Vec::new());
            used = 1;
        }
        rows.last_mut().unwrap().push(k);
        used += cell;
    }
    rows
}

fn put_cards_row(buf: &mut Buffer, y: u16, area: Rect, app: &App, c: &Combat, lines_max: u16) {
    let n = c.hand.len();
    if n == 0 || area.width < 3 {
        return;
    }
    let sel = app.hand_sel.min(n - 1);
    // {} 只画在光标那张;已经选中的(背景色)不画括号
    let marked = |k: usize| k == sel;
    let highlighted = |k: usize| k == sel || app.choice_sel.contains(&k);
    // 选择模式下,"能不能选"决定压暗而不是"能不能打"
    let choosable: Option<Vec<usize>> = app
        .run
        .combat()
        .filter(|c| c.choice.is_some())
        .map(|c| c.choice_candidates().into_iter().map(|(i, _)| i).collect());
    // 分隔符:选中的牌把它左右两根竖线换成 { 和 }(宽度不变)
    let bar_at = |before: usize| -> &'static str {
        if before < n && marked(before) {
            "{"
        } else if before > 0 && marked(before - 1) {
            "}"
        } else {
            "│"
        }
    };
    let width_of = |card: &crate::core::card::CardInstance| -> usize {
        display_width(&crate::ui::cost_label(card)) + 1 + display_width(&card.label())
    };
    for (r, cards) in hand_row_layout(c, area.width as usize, lines_max.max(1))
        .iter()
        .enumerate()
    {
        if cards.is_empty() {
            continue;
        }
        let y = y + r as u16;
        // 这一行连竖线一起居中
        let total: usize = cards.iter().map(|k| width_of(&c.hand[*k])).sum::<usize>()
            + cards.len()
            + 1;
        let mut cx = if total <= area.width as usize {
            area.x + ((area.width as usize - total) / 2) as u16
        } else {
            area.x
        };
        let last_x = area.x + area.width - 1;
        let bar = |buf: &mut Buffer, x: u16, ch: &str| {
            put(
                buf,
                x,
                y,
                ch,
                theme::fg(if ch == "│" { theme::BORDER } else { theme::SEL_FG }),
            );
        };
        // 行首只看本行第一张:上一行末尾那张的 } 已经在自己行尾画过了
        let head = if marked(cards[0]) { "{" } else { "│" };
        bar(buf, cx, head);
        cx += 1;
        for (i, k) in cards.iter().enumerate() {
            let card = &c.hand[*k];
            let want = width_of(card);
            let remain = last_x.saturating_sub(cx) as usize;
            if remain == 0 {
                break;
            }
            let w = want.min(remain.saturating_sub(1)).max(1);
            let dim = match &choosable {
                Some(list) => !list.contains(k),
                None => c.blocked_reason(*k).is_some(),
            };
            crate::ui::put_card_cell(buf, cx, y, w as u16, card, highlighted(*k), dim);
            cx += w as u16;
            let ch = if i + 1 == cards.len() {
                if marked(*k) {
                    "}"
                } else {
                    "│"
                }
            } else {
                bar_at(k + 1)
            };
            bar(buf, cx, ch);
            cx += 1;
        }
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
    // 符文穹顶:看不到敌人的下一步意图(原版语义)
    let hide_intents = app.run.has_relic("runic_dome");
    let n = c.enemies.len();
    let gap = 4u16;
    let widths: Vec<u16> = (0..n).map(|i| enemy_block_w(c, i, hide_intents)).collect();
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
        let lines = enemy_lines(c, i, hide_intents);
        let w = widths[i].min((area.x + area.width).saturating_sub(x)).max(1);
        // 抖动:这个敌人在挨打时往右退、出手时往左冲
        let off = app.battle_shake_offset(crate::core::combat::ShakeWho::Enemy(i));
        let bx = (x as i32 + off).clamp(area.x as i32, (area.x + area.width) as i32 - 1) as u16;
        render_enemy_block(buf, Rect::new(bx, area.y, w, area.height), &lines, selected);
        x += widths[i] + gap;
    }
}

/// 敌人框的总宽(内容 + 左右边框)
fn enemy_block_w(c: &Combat, i: usize, hide_intents: bool) -> u16 {
    let lines = enemy_lines(c, i, hide_intents);
    let w = lines
        .iter()
        .map(|l| l.iter().map(|(t, _)| display_width(t)).sum::<usize>())
        .max()
        .unwrap_or(0);
    w as u16 + 2
}

/// 一个敌人的五行(自上而下):意图 / 血量(格挡/当前/上限)/ 名字 / 状态图标 / 状态数值.
/// 意图用记号语法(见 intent_tokens);状态两行是 3 列徽章(见 ui::badge),左右都不加缩进,
/// 靠每格自带的右对齐补位.
fn enemy_lines(c: &Combat, i: usize, hide_intents: bool) -> Vec<Vec<(String, Style)>> {
    let e = &c.enemies[i];
    if !e.alive() {
        return vec![
            vec![("".to_string(), theme::dim())],
            vec![("0/0/0".to_string(), theme::dim())],
            vec![("dead".to_string(), theme::dim())],
            vec![("".to_string(), theme::dim())],
            vec![("".to_string(), theme::dim())],
        ];
    }
    // 第一行:本回合意图
    let intent = enemy_intent_line(c, i, hide_intents);
    // 第二行:格挡/当前/上限(格挡蓝、血量与上限红)
    let blood = vec![
        (e.block.to_string(), theme::fg(theme::BLOCK)),
        ("/".to_string(), theme::dim()),
        (e.hp.to_string(), theme::fg(theme::BAD)),
        ("/".to_string(), theme::dim()),
        (e.max_hp.to_string(), theme::fg(theme::BAD)),
    ];
    // 第三行:名字
    let name_line = vec![(
        e.name.clone(),
        Style::default().fg(theme::BAD).add_modifier(Modifier::BOLD),
    )];
    // 第四、五行:状态徽章的图标行与数值行
    let (icons, values) = status_badge_lines(&e.statuses);
    vec![intent, blood, name_line, icons, values]
}

/// 身上的增减益渲染成两行徽章:上行图标(emoji 自带色 / nf 用配色键)、下行数值
/// (减益绿、增益紫),每格 3 列右对齐,格间空 1 格.
fn status_badge_lines(
    statuses: &crate::core::status::Statuses,
) -> (Vec<(String, Style)>, Vec<(String, Style)>) {
    let mut icons: Vec<(String, Style)> = Vec::new();
    let mut values: Vec<(String, Style)> = Vec::new();
    for (j, (st, n)) in statuses.entries().enumerate() {
        if j > 0 {
            icons.push((" ".to_string(), theme::fg(theme::FG)));
            values.push((" ".to_string(), theme::fg(theme::FG)));
        }
        let (icon, value) = crate::ui::badge(st.glyph(), *n);
        let color = if st.is_debuff() { theme::DEBUFF } else { theme::BUFF };
        icons.push((icon, crate::ui::status_icon_style(*st)));
        values.push((value, theme::fg(color)));
    }
    (icons, values)
}

/// 意图行的上下文:把敌人的这一步摊成记号需要的几个数,交给 intent_tokens 排版.
fn enemy_intent_line(c: &Combat, i: usize, hide_intents: bool) -> Vec<(String, Style)> {
    let e = &c.enemies[i];
    if !e.up() {
        return vec![("--".to_string(), theme::dim())];
    }
    if hide_intents {
        // 符文穹顶:整行留空,但仍占一行(行数固定)
        return vec![("".to_string(), theme::dim())];
    }
    let mv = &e.def.moves[e.next_move];
    let fx = crate::core::ascension::effects(e.def.id, mv.name, mv.effects, e.asc);
    // 攻击伤害用实算值(含增减益与随回合成长);它也是"按伤害给格挡"那类法术的参考值
    let (per, times): (i32, u8) = if e.intent().attacks() {
        c.predicted_damage(i)
    } else {
        (0, 0)
    };
    let mut block = c.intent_block(i);
    if fx.iter().any(|f| matches!(f, EnemyFx::BlockFromDamage)) {
        block += per * times as i32;
    }
    intent_tokens(e.intent(), &fx, per, times, block)
        .into_iter()
        .enumerate()
        .flat_map(|(j, (text, color))| {
            let mut v: Vec<(String, Style)> = Vec::new();
            if j > 0 {
                v.push((" ".to_string(), theme::fg(theme::FG)));
            }
            v.push((text, theme::fg(color).add_modifier(Modifier::BOLD)));
            v
        })
        .collect()
}

/// 意图记号(纯函数,便于直接测):
/// - 攻击 `!伤害` / `!伤害x次数`(红)
/// - 格挡 `(数值)`(蓝)
/// - 减益 `@缩写数值`(绿)
/// - 自身增益 `💭缩写数值`(紫)
/// - 睡觉 `💤`、眩晕 `💫`、逃跑 `>>`、未知 `?`
/// 什么都不匹配就退回 `?`.
fn intent_tokens(
    intent: Intent,
    fx: &[EnemyFx],
    per: i32,
    times: u8,
    block: i32,
) -> Vec<(String, Color)> {
    match intent {
        Intent::Sleep => return vec![("\u{1F4A4}".to_string(), theme::DIM)],
        Intent::Stun => return vec![("\u{1F4AB}".to_string(), theme::DIM)],
        Intent::Escape => return vec!["\u{1F3C3}".to_string()].into_iter().map(|t| (t, theme::DEBUFF)).collect(),
        Intent::Unknown => return vec![("?".to_string(), theme::DIM)],
        _ => {}
    }
    let mut out: Vec<(String, Color)> = Vec::new();
    if intent.attacks() {
        out.push((
            if times > 1 {
                format!("!{per}x{times}")
            } else {
                format!("!{per}")
            },
            theme::BAD,
        ));
    }
    if block > 0 {
        out.push((format!("({block})"), theme::BLOCK));
    }
    for f in fx {
        match f {
            // 打到玩家身上的:减益用绿、非减益用默认色
            EnemyFx::PlayerStatus { status, n } => {
                let color = if status.is_debuff() { theme::DEBUFF } else { theme::FG };
                out.push((format!("@{}{}", status.short(), n), color));
            }
            // 给自己/同伴上的:算"策略",减益仍标绿
            EnemyFx::GainStatus { status, n, .. } => {
                let color = if status.is_debuff() { theme::DEBUFF } else { theme::BUFF };
                out.push((format!("\u{1F4AD}{}{}", status.short(), n), color));
            }
            _ => {}
        }
    }
    if out.is_empty() {
        out.push(("?".to_string(), theme::DIM));
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

// ---- 底部一行 ----

/// 底部提示文字:先说打不出去的原因,再顺带一句当前操作
fn info_text(app: &App, c: &Combat) -> (String, Style) {
    let sel = app.hand_sel.min(c.hand.len().saturating_sub(1));
    let reason = if c.hand.is_empty() {
        None
    } else {
        c.blocked_reason(sel)
    };
    // 选牌模式:只显示"怎么选",不再掺打牌提示与打不出的原因
    let choice_source = c.choice.as_ref().map(|ch| ch.source);
    if let Some(source) = choice_source {
        if !app.msg.is_empty() {
            (app.msg.clone(), theme::fg(theme::WARN))
        } else if source == crate::core::combat::ChoiceSource::Hand {
            (
                "space to choose, it applies right away, esc to cancel.".to_string(),
                theme::dim(),
            )
        } else {
            (
                "space/enter to pick, it applies right away, esc to cancel.".to_string(),
                theme::dim(),
            )
        }
    } else {
        match reason {
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
        }
    }
}

/// 底部那一行:平时左边写提示、右边写 "? help";按 : 后整条让给命令行
/// (补全提示由 ui::command_hints 叠在这一行上面)
fn render_status(buf: &mut Buffer, area: Rect, app: &App, c: &Combat) {
    if app.mode == Mode::Command {
        let line = format!(":{}_", app.cmd);
        put_padded(buf, area.x, area.y, &line, area.width as usize, theme::selected());
        return;
    }
    let (text, style) = info_text(app, c);
    // 右边给 "? help" 留 6 列 + 两侧各 2 列;剩不下就不写它
    let help = "? help";
    let room = (area.width as usize).saturating_sub(2);
    let reserve = help.len() + 3;
    let (text_w, show_help) = if room > reserve {
        (room - reserve, true)
    } else {
        (room, false)
    };
    // 提示要抖的时候整句话往右挪一格(和商店那套一致)
    let x = area.x + 2 + app.shake_nudge() as u16;
    put(buf, x, area.y, &truncate(&text, text_w), style);
    if show_help {
        put(buf, area.x + area.width - (help.len() as u16) - 2, area.y, help, theme::dim());
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::run::Run;

    fn row_text(line: &[(String, Style)]) -> String {
        line.iter().map(|(t, _)| t.as_str()).collect()
    }

    /// 符文穹顶:意图行留空(看不到敌人意图),别的行照常
    #[test]
    fn runic_dome_hides_enemy_intents() {
        let mut r = Run::new(3);
        r.debug_add_relic("runic_dome").unwrap();
        r.debug_start_combat(crate::core::enemies::resolve("cultist_solo"));
        let c = r.combat.as_ref().expect("进战斗");
        let shown = enemy_intent_line(c, 0, false);
        assert!(!row_text(&shown).trim().is_empty(), "没有穹顶时看得到意图记号");
        let lines = enemy_lines(c, 0, true);
        assert_eq!(row_text(&lines[0]).trim(), "", "符文穹顶下意图行留空");
        // 名字与血量行还在,敌人照常行动(只是看不到)
        assert!(!row_text(&lines[2]).trim().is_empty(), "名字行照常");
        assert!(row_text(&lines[1]).contains('/'), "血量行照常");
    }

    /// 行序固定为:意图 / 格挡-血量 / 名字 / 状态图标 / 状态数值
    #[test]
    fn enemy_lines_order_is_intent_blood_name_badges() {
        let mut r = Run::new(3);
        r.debug_start_combat(crate::core::enemies::resolve("cultist_solo"));
        let c = r.combat.as_ref().expect("进战斗");
        let lines = enemy_lines(c, 0, false);
        assert_eq!(lines.len(), 5, "五行");
        let e = &c.enemies[0];
        assert_eq!(
            row_text(&lines[1]),
            format!("{}/{}/{}", e.block, e.hp, e.max_hp),
            "血液行是 格挡/当前/上限"
        );
        assert_eq!(row_text(&lines[2]), e.name, "第三行是名字");
        // 标签行只有"图标"和"数值"两种内容,且两行等宽(每格 3 列)
        let w = |l: &[(String, Style)]| -> usize {
            l.iter().map(|(t, _)| display_width(t)).sum()
        };
        assert_eq!(w(&lines[3]), w(&lines[4]), "图标行与数值行等宽");
    }

    /// 意图记号:攻击 !N / !NxM、格挡 (N)、减益 @缩写N、自身增益 💭缩写N
    #[test]
    fn intent_tokens_follow_the_symbol_spec() {
        let tok = |t: &[(String, Color)]| -> String {
            t.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join(" ")
        };
        let dmg = intent_tokens(Intent::Attack { damage: 25, times: 1 }, &[], 25, 1, 0);
        assert_eq!(tok(&dmg), "!25");
        assert_eq!(dmg[0].1, theme::BAD, "攻击是红的");
        assert_eq!(tok(&intent_tokens(Intent::Attack { damage: 3, times: 3 }, &[], 3, 3, 0)), "!3x3");
        let def = intent_tokens(Intent::Defend, &[], 0, 0, 23);
        assert_eq!(tok(&def), "(23)");
        assert_eq!(def[0].1, theme::BLOCK, "格挡是蓝的");
        let db = intent_tokens(
            Intent::Debuff,
            &[EnemyFx::PlayerStatus { status: crate::core::status::Status::Weak, n: 2 }],
            0,
            0,
            0,
        );
        assert_eq!(tok(&db), "@WEAK2");
        assert_eq!(db[0].1, theme::DEBUFF, "减益是绿的");
        let st = intent_tokens(
            Intent::Buff,
            &[EnemyFx::GainStatus { status: crate::core::status::Status::Strength, n: 3, scope: crate::core::enemy::Scope::SelfOnly }],
            0,
            0,
            0,
        );
        assert_eq!(tok(&st), "\u{1F4AD}STR3", "自身增益用思考气泡");
        // 攻击+格挡+减益同时出现时按 攻击 / 格挡 / 减益 的顺序排
        let mix = intent_tokens(
            Intent::AttackDebuff { damage: 7, times: 2 },
            &[EnemyFx::PlayerStatus { status: crate::core::status::Status::Vulnerable, n: 1 }],
            7,
            2,
            5,
        );
        assert_eq!(tok(&mix), "!7x2 (5) @VULN1");
        // 特殊意图
        assert_eq!(tok(&intent_tokens(Intent::Sleep, &[], 0, 0, 0)), "\u{1F4A4}");
        assert_eq!(tok(&intent_tokens(Intent::Stun, &[], 0, 0, 0)), "\u{1F4AB}");
        assert_eq!(
            tok(&intent_tokens(Intent::Escape, &[], 0, 0, 0)),
            "\u{1F3C3}",
            "逃跑用跑步小人(盗贼)"
        );
        assert_eq!(display_width("\u{1F3C3}"), 2, "🏃 是双宽,框宽才对得上");
        assert_eq!(tok(&intent_tokens(Intent::Unknown, &[], 0, 0, 0)), "?");
        assert_eq!(tok(&intent_tokens(Intent::Defend, &[], 0, 0, 0)), "?", "没数就退回 ?");
    }

    /// 状态两行徽章:每格 3 列右对齐,格间 1 格,数值带符号
    #[test]
    fn status_badge_lines_pair_icons_with_values() {
        let mut st = crate::core::status::Statuses::new();
        st.add(crate::core::status::Status::Strength, 2);
        st.add(crate::core::status::Status::Dexterity, 3);
        let (icons, values) = status_badge_lines(&st);
        assert_eq!(row_text(&icons), " \u{1F4AA}  \u{1F45F}", "图标行");
        assert_eq!(row_text(&values), " +2  +3", "数值行");
    }
}
