// 非战斗界面:奖励、商店、营火、事件、宝箱、选牌、结算.
// 统一用"列表 + 详情"的样式,选中行整行反白.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::app::App;
use crate::core::card::CardInstance;
use crate::core::run::{RewardSlot, ShopItem};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, fit, put, put_padded, truncate, wrap_text};

struct Row {
    text: String,
    style: Style,
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    match app.run.screen {
        crate::core::run::Screen::Reward => reward(buf, area, app),
        crate::core::run::Screen::Shop => shop(buf, area, app),
        crate::core::run::Screen::Rest => rest(buf, area, app),
        crate::core::run::Screen::Event => event(buf, area, app),
        crate::core::run::Screen::Treasure => treasure(buf, area, app),
        crate::core::run::Screen::Pick => pick(buf, area, app),
        crate::core::run::Screen::Victory => summary(buf, area, app, true),
        crate::core::run::Screen::Death => summary(buf, area, app, false),
        _ => {}
    }
}

/// 列表框:标题 + 选中行 + 描述.
/// 宽盒子用 --- 上下分(列表按内容定高),窄盒子用 | 左右分(两侧各自折行).
fn draw_list(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    rows: &[Row],
    sel: usize,
    details: &[String],
) {
    draw_box(buf, area, title, theme::fg(theme::BORDER), theme::fg(theme::INFO));
    if area.width < 4 || area.height < 4 {
        return;
    }
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let split = crate::ui::split_list_detail(inner, rows.len() as u16);
    crate::ui::draw_split(buf, inner, &split, theme::fg(theme::BORDER));
    // 列表
    let list = split.list;
    let lw = list.width as usize;
    if lw == 0 || list.height == 0 {
        return;
    }
    let capacity = list.height as usize;
    let start = if rows.len() > capacity {
        sel.saturating_sub(capacity / 2).min(rows.len() - capacity)
    } else {
        0
    };
    let mut y = list.y;
    let bottom = list.y + list.height;
    for (i, row) in rows.iter().skip(start).enumerate() {
        let idx = start + i;
        let selected = idx == sel;
        let style = if selected { theme::selected() } else { row.style };
        let marker = if selected { "> " } else { "  " };
        let text = format!("{marker}{}", row.text);
        // 窄盒子左右分的时候,这半边要正常折行;宽盒子一行一条
        let lines = if split.horizontal {
            vec![truncate(&text, lw)]
        } else {
            wrap_text(&text, lw, usize::MAX)
        };
        for line in lines {
            if y >= bottom {
                break;
            }
            put_padded(buf, list.x, y, &line, lw, style);
            y += 1;
        }
        if y >= bottom {
            break;
        }
    }
    // 描述
    let detail = split.detail;
    let dw = detail.width as usize;
    if dw == 0 || detail.height == 0 {
        return;
    }
    let mut y = detail.y;
    let bottom = detail.y + detail.height;
    for line in details {
        for wrapped in wrap_text(line, dw, usize::MAX) {
            if y >= bottom {
                return;
            }
            put_padded(buf, detail.x, y, &wrapped, dw, theme::fg(theme::FG));
            y += 1;
        }
    }
}

// ---- 奖励 ----

fn reward(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(r) = &app.run.reward else {
        return;
    };
    if area.height < 6 || area.width < 40 {
        return;
    }
    let slots = app.run.reward_slots();
    // 按 reward_slots 的顺序把每类槽位挑出来,选择下标才能对上
    let gold = slots.iter().position(|s| matches!(s, RewardSlot::Gold));
    let cards: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, RewardSlot::Card(_)))
        .map(|(i, _)| i)
        .collect();
    let others: Vec<usize> = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, RewardSlot::Relic | RewardSlot::Potion))
        .map(|(i, _)| i)
        .collect();

    let mut y = area.y;
    let bottom = area.y + area.height;
    // 金币行
    if let Some(gi) = gold {
        let sel = r.index == gi;
        let style = if sel { theme::selected() } else { theme::fg(theme::GOLD) };
        put_padded(
            buf,
            area.x + 2,
            y,
            &format!("{}Gold  +{}", marker(sel), r.gold),
            (area.width as usize).saturating_sub(4),
            style,
        );
        y += 1;
    }
    // 卡牌段:直接横排三个卡框,外面不套框.
    // 高度取"最长描述折行后需要的行数 + 名字 1 行 + 上下边框 2 行",不足 10 行则撑到 10 行.
    // 牌被拿走之后这一段整体消失,后面的奖励自然顶上来,相邻摆放.
    if !cards.is_empty() {
        let n = cards.len() as u16;
        let gap = 1u16;
        let cw = area.width.saturating_sub(gap * (n - 1)) / n;
        let text_w = (cw as usize).saturating_sub(4);
        let longest = cards
            .iter()
            .map(|&si| match slots[si] {
                RewardSlot::Card(ci) => {
                    wrap_text(&r.cards[ci].display_text(), text_w, usize::MAX).len()
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        // 高度按内容:费用 1 行 + 名字 1 行 + 类型 1 行 + 最长描述折行行数 + 上下边框 2 行
        let want_h = longest as u16 + 5;
        let card_h = want_h.min(bottom.saturating_sub(y));
        if card_h >= 3 && cw >= 6 {
            for (k, &si) in cards.iter().enumerate() {
                let card = match slots[si] {
                    RewardSlot::Card(ci) => &r.cards[ci],
                    _ => continue,
                };
                let x = area.x + k as u16 * (cw + gap);
                card_box(buf, Rect::new(x, y, cw, card_h), card, r.index == si);
            }
        }
        y += card_h;
    }
    // 遗物与药水:紧跟在上一个奖励下面
    for &i in others.iter() {
        if y >= bottom {
            break;
        }
        let sel = r.index == i;
        let (text, color) = match slots[i] {
            RewardSlot::Relic => match r.relic {
                Some(d) => (
                    format!("Relic  {}  ({})", d.name, d.desc),
                    theme::RELIC,
                ),
                None => continue,
            },
            RewardSlot::Potion => match r.potion {
                Some(d) => {
                    // 放得下就写全名,放不下才把 "Potion" 缩成 "~"
                    let full = format!("Potion  {}  ({})", d.name, d.desc);
                    let avail = (area.width as usize).saturating_sub(6);
                    let text = if display_width(&full) <= avail {
                        full
                    } else {
                        format!(
                            "Potion  {}  ({})",
                            crate::ui::potion_label(d.name),
                            d.desc
                        )
                    };
                    (text, theme::BUFF)
                }
                None => continue,
            },
            _ => continue,
        };
        let style = if sel { theme::selected() } else { theme::fg(color) };
        put_padded(
            buf,
            area.x + 2,
            y,
            &format!("{}{text}", marker(sel)),
            (area.width as usize).saturating_sub(4),
            style,
        );
        y += 1;
    }
}

fn marker(selected: bool) -> &'static str {
    if selected {
        "> "
    } else {
        "  "
    }
}

/// 一张卡的小框:框里用和战斗一样的说明版式(费用行 + 名字/类型/描述居中)
fn card_box(buf: &mut Buffer, rect: Rect, card: &CardInstance, selected: bool) {
    if rect.width < 6 || rect.height < 3 {
        return;
    }
    let border = if selected {
        theme::fg(theme::SEL_FG)
    } else {
        theme::fg(theme::BORDER)
    };
    draw_box(buf, rect, "", border, theme::fg(theme::INFO));
    let inner = Rect::new(
        rect.x + 1,
        rect.y + 1,
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    );
    if selected {
        for y in inner.y..inner.y + inner.height {
            put_padded(
                buf,
                inner.x,
                y,
                "",
                inner.width as usize,
                Style::default().bg(theme::SEL_BG),
            );
        }
    }
    crate::ui::card_desc(buf, inner, card);
}

// ---- 商店 ----

fn shop(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(s) = &app.run.shop else {
        return;
    };
    if area.width < 4 || area.height < 4 {
        return;
    }
    draw_box(buf, area, "shop", theme::fg(theme::BORDER), theme::fg(theme::INFO));
    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let split = crate::ui::split_list_detail(inner, s.items.len() as u16);
    crate::ui::draw_split(buf, inner, &split, theme::fg(theme::BORDER));
    // 商品列表:卡牌那行和战斗里的手牌一样,价钱靠右
    let list = split.list;
    if list.width == 0 || list.height == 0 {
        return;
    }
    let capacity = list.height as usize;
    let start = if s.items.len() > capacity {
        s.index.saturating_sub(capacity / 2).min(s.items.len() - capacity)
    } else {
        0
    };
    for (i, item) in s.items.iter().skip(start).take(capacity).enumerate() {
        let idx = start + i;
        shop_row(
            buf,
            list.x,
            list.y + i as u16,
            list.width,
            item,
            idx == s.index,
            s.sold.get(idx).copied().unwrap_or(false),
        );
    }
    // 说明
    let Some(item) = s.items.get(s.index) else {
        return;
    };
    let detail = split.detail;
    match item {
        ShopItem::Card(card, _) => crate::ui::card_desc(buf, detail, card),
        ShopItem::Relic(def, _) => put_lines(buf, detail, &[def.name.to_string(), def.desc.to_string()]),
        ShopItem::Potion(def, _) => put_lines(buf, detail, &[def.name.to_string(), def.desc.to_string()]),
        ShopItem::Remove(_) => put_lines(
            buf,
            detail,
            &[
                "Card Removal Service".to_string(),
                "Remove a card from your deck permanently.".to_string(),
            ],
        ),
    }
}

/// 商品行的名字部分:卡牌用战斗里那套(费用+牌名),其它就一行文字
fn shop_row(buf: &mut Buffer, x: u16, y: u16, w: u16, item: &ShopItem, selected: bool, sold: bool) {
    if w == 0 {
        return;
    }
    let bg = if selected { theme::SEL_BG } else { theme::BG };
    let base = Style::default().bg(bg);
    put_padded(buf, x, y, "", w as usize, base);
    // 价钱最多 $999,固定 4 格,靠右摆
    let price = format!("{:<4}", format!("${}", item.price()));
    let field_w = display_width(&price).min(w as usize);
    let name_w = (w as usize).saturating_sub(field_w + 1);
    match item {
        ShopItem::Card(card, _) => {
            crate::ui::put_card_line(buf, x, y, name_w as u16, card, selected, sold);
        }
        ShopItem::Relic(def, _) => {
            let style = if sold { theme::dim().bg(bg) } else { theme::fg(theme::RELIC).bg(bg) };
            put(buf, x, y, &truncate(&format!("{} [{}]", def.name, def.rarity.name()), name_w), style);
        }
        ShopItem::Potion(def, _) => {
            let style = if sold { theme::dim().bg(bg) } else { theme::fg(theme::BUFF).bg(bg) };
            put(buf, x, y, &truncate(&def.name, name_w), style);
        }
        ShopItem::Remove(_) => {
            let style = if sold { theme::dim().bg(bg) } else { theme::fg(theme::GOOD).bg(bg) };
            put(buf, x, y, &truncate("Card Removal Service", name_w), style);
        }
    }
    let style = if sold { theme::dim().bg(bg) } else { theme::fg(theme::GOLD).bg(bg) };
    put(buf, x + (w as usize - field_w) as u16, y, &price, style);
}

/// 左对齐写几行说明,按宽度折行
fn put_lines(buf: &mut Buffer, rect: Rect, lines: &[String]) {
    let w = rect.width as usize;
    if w == 0 {
        return;
    }
    let bottom = rect.y + rect.height;
    let mut y = rect.y;
    for line in lines {
        for wrapped in wrap_text(line, w, usize::MAX) {
            if y >= bottom {
                return;
            }
            put_padded(buf, rect.x, y, &wrapped, w, theme::fg(theme::FG));
            y += 1;
        }
    }
}

// ---- 营火 ----

fn rest(buf: &mut Buffer, area: Rect, app: &App) {
    let heal = app.run.player.max_hp * crate::core::run::REST_HEAL_PCT / 100;
    let rows = vec![
        Row {
            text: format!("1) Rest  heal {heal} HP"),
            style: theme::fg(theme::GOOD),
        },
        Row {
            text: "2) Smith  upgrade a card".to_string(),
            style: theme::fg(theme::BLOCK),
        },
    ];
    let details = vec![
        format!(
            "HP {}/{}   max HP {}",
            app.run.player.hp, app.run.player.max_hp, app.run.player.max_hp
        ),
        "Rest sites are the only place to heal or upgrade.".to_string(),
    ];
    draw_list(buf, area, "rest site", &rows, app.rest_index, &details);
}

// ---- 事件 ----

fn event(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(st) = &app.run.event else {
        return;
    };
    draw_box(
        buf,
        area,
        st.def.name,
        theme::fg(theme::BORDER),
        theme::fg(theme::INFO),
    );
    let inner_w = (area.width as usize).saturating_sub(4);
    let mut y = area.y + 2;
    for line in st.def.body {
        if y >= area.y + area.height - 1 {
            break;
        }
        put_padded(
            buf,
            area.x + 2,
            y,
            &truncate(line, inner_w),
            inner_w,
            theme::fg(theme::FG),
        );
        y += 1;
    }
    y += 1;
    match st.result {
        Some(text) => {
            if y < area.y + area.height - 1 {
                put_padded(
                    buf,
                    area.x + 2,
                    y,
                    &format!(">> {}", truncate(text, inner_w.saturating_sub(3))),
                    inner_w,
                    theme::fg(theme::GOOD),
                );
            }
            if y + 1 < area.y + area.height - 1 {
                put_padded(
                    buf,
                    area.x + 2,
                    y + 1,
                    "   press enter or esc to continue",
                    inner_w,
                    theme::dim(),
                );
            }
        }
        None => {
            for (i, choice) in st.def.choices.iter().enumerate() {
                if y >= area.y + area.height - 1 {
                    break;
                }
                let selected = i == st.index;
                let available = app.run.event_choice_available(i);
                let mut cost = String::new();
                if choice.cost_gold > 0 {
                    cost.push_str(&format!(" [{}g]", choice.cost_gold));
                }
                if choice.cost_hp > 0 {
                    cost.push_str(&format!(" [{}HP]", choice.cost_hp));
                }
                let text = format!("{}) {}{cost}", i + 1, choice.label);
                let style = if !available {
                    theme::dim()
                } else if selected {
                    theme::selected()
                } else {
                    theme::fg(theme::FG)
                };
                let marker = if selected { "> " } else { "  " };
                let line = format!("{marker}{text}");
                put_padded(
                    buf,
                    area.x + 2,
                    y,
                    &truncate(&fit(&line, inner_w), inner_w),
                    inner_w,
                    style,
                );
                y += 1;
            }
        }
    }
}

// ---- 宝箱 ----

fn treasure(buf: &mut Buffer, area: Rect, app: &App) {
    draw_box(
        buf,
        area,
        "treasure chest",
        theme::fg(theme::BORDER),
        theme::fg(theme::GOLD),
    );
    let inner_w = (area.width as usize).saturating_sub(4);
    let mut y = area.y + 2;
    put_padded(buf, area.x + 2, y, "you open the chest...", inner_w, theme::fg(theme::FG));
    y += 2;
    match app.run.treasure {
        Some(relic) => {
            put_padded(
                buf,
                area.x + 2,
                y,
                &format!("  {}  [{}]", relic.name, relic.rarity.name()),
                inner_w,
                theme::fg(theme::RELIC),
            );
            if y + 1 < area.y + area.height - 1 {
                put_padded(
                    buf,
                    area.x + 2,
                    y + 1,
                    &truncate(relic.desc, inner_w),
                    inner_w,
                    theme::fg(theme::FG),
                );
            }
        }
        None => {
            put_padded(buf, area.x + 2, y, "  (empty)", inner_w, theme::dim());
        }
    }
    if y + 3 < area.y + area.height - 1 {
        put_padded(
            buf,
            area.x + 2,
            y + 3,
            "press enter to take it",
            inner_w,
            theme::dim(),
        );
    }
}

// ---- 选牌 ----

fn pick(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(p) = &app.run.picker else {
        return;
    };
    let cands = app.run.picker_candidates();
    let deck = &app.run.player.deck;
    // 整副牌都列出来(和看牌组的窗口一样),只有候选的那些可选
    let rows: Vec<crate::ui::CardRow> = deck
        .iter()
        .enumerate()
        .map(|(i, card)| crate::ui::CardRow::Card {
            card: card.clone(),
            selectable: cands.contains(&i),
        })
        .collect();
    let sel = cands.get(p.index).copied().unwrap_or(0);
    // 只有升级才需要"升级后"那一份说明
    let after = if p.purpose == crate::core::run::PickPurpose::Upgrade {
        cands.get(p.index).map(|i| {
            let mut up = deck[*i].clone();
            up.upgrade();
            up
        })
    } else {
        None
    };
    crate::ui::card_window(
        buf,
        area,
        p.purpose.title(),
        &rows,
        sel,
        after.as_ref(),
    );
}

// ---- 胜负结算 ----

fn summary(buf: &mut Buffer, area: Rect, app: &App, victory: bool) {
    let run = &app.run;
    let (title, style) = if victory {
        ("victory", theme::fg(theme::GOOD))
    } else {
        ("you died", theme::fg(theme::BAD))
    };
    draw_box(buf, area, title, theme::fg(theme::BORDER), style);
    let lines = vec![
        format!(
            "{}",
            if victory {
                "You climbed the spire and struck it down."
            } else {
                "The spire claims another climber."
            }
        ),
        String::new(),
        format!("seed          {}", run.seed),
        format!("floor         {}/{}", run.floor() + 1, run.map.total_floors()),
        format!("hp            {}/{}", run.player.hp, run.player.max_hp),
        format!("gold          {}", run.player.gold),
        format!("deck size     {}", run.player.deck.len()),
        format!("relics        {}", run.player.relics.len()),
        format!("fights        {}", run.stats.fights),
        format!("elites        {}", run.stats.elites),
        format!("bosses        {}", run.stats.bosses),
        format!("turns         {}", run.stats.turns),
        format!("damage dealt  {}", run.stats.damage_dealt),
        format!("potions used  {}", run.stats.potions_used),
        String::new(),
        "r: new run (same seed)    n: new run (new seed)    :q quit".to_string(),
    ];
    let inner_w = (area.width as usize).saturating_sub(4);
    for (i, line) in lines.iter().enumerate() {
        let y = area.y + 2 + i as u16;
        if y >= area.y + area.height - 1 {
            break;
        }
        let style = if line.is_empty() {
            theme::dim()
        } else {
            theme::fg(theme::FG)
        };
        put_padded(
            buf,
            area.x + 2,
            y,
            &truncate(line, inner_w),
            inner_w,
            style,
        );
    }
}
