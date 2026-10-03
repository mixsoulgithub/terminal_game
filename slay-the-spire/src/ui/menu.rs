// 非战斗界面:奖励、商店、营火、事件、宝箱、选牌、结算.
// 统一用"列表 + 详情"的样式,选中行整行反白.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::core::card::CardInstance;
use crate::core::potions::PotionDef;
use crate::core::relics::RelicDef;
use crate::core::run::{RewardSlot, ShopItem};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, fit, hline, put, put_padded, truncate, wrap_text};

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

/// 列表框:标题 + 选中行 + 底部详情
fn draw_list(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    rows: &[Row],
    sel: usize,
    details: &[String],
) {
    draw_box(buf, area, title, theme::fg(theme::BORDER), theme::fg(theme::INFO));
    let inner_w = (area.width as usize).saturating_sub(4);
    let detail_h = if area.height > 10 && !details.is_empty() {
        (details.len() as u16 + 1).min(area.height / 3)
    } else {
        0
    };
    let list_h = area.height.saturating_sub(2 + detail_h);
    let start = if rows.len() > list_h as usize {
        sel.saturating_sub(list_h as usize / 2)
            .min(rows.len() - list_h as usize)
    } else {
        0
    };
    for (i, row) in rows.iter().skip(start).take(list_h as usize).enumerate() {
        let y = area.y + 1 + i as u16;
        let idx = start + i;
        let selected = idx == sel;
        let style = if selected {
            theme::selected()
        } else {
            row.style
        };
        let marker = if selected { "> " } else { "  " };
        put_padded(
            buf,
            area.x + 2,
            y,
            &format!("{marker}{}", row.text),
            inner_w,
            style,
        );
    }
    if detail_h > 0 {
        let y0 = area.y + area.height - 1 - detail_h;
        hline(buf, area.x + 1, y0, area.width - 2, '-', theme::fg(theme::BORDER));
        for (i, line) in details.iter().take(detail_h as usize).enumerate() {
            put_padded(
                buf,
                area.x + 2,
                y0 + 1 + i as u16,
                &format!("  {line}"),
                inner_w,
                theme::fg(theme::FG),
            );
        }
    }
}

fn card_row(idx: usize, card: &CardInstance) -> Row {
    let cost = match card.cost() {
        crate::core::card::Cost::Fixed(n) => n.to_string(),
        crate::core::card::Cost::X => "X".to_string(),
        crate::core::card::Cost::Unplayable => "-".to_string(),
    };
    Row {
        text: format!(
            "{}) {}  cost {}  {}",
            idx + 1,
            card.label(),
            cost,
            card.kind().name()
        ),
        style: theme::fg(theme::card_color(card.kind(), card.rarity())),
    }
}

fn card_details(card: &CardInstance) -> Vec<String> {
    vec![
        format!("{} ({})", card.label(), card.rarity().name()),
        card.display_text(),
    ]
}

fn relic_row(idx: usize, r: &RelicDef, price: Option<i32>) -> Row {
    let price = price.map(|p| format!("  {p}g")).unwrap_or_default();
    Row {
        text: format!("{}) {}  [{}]{}", idx + 1, r.name, r.rarity.name(), price),
        style: theme::fg(theme::RELIC),
    }
}

fn potion_row(idx: usize, p: &PotionDef, price: Option<i32>) -> Row {
    let price = price.map(|p| format!("  {p}g")).unwrap_or_default();
    Row {
        text: format!("{}) {}{}", idx + 1, p.name, price),
        style: theme::fg(theme::BUFF),
    }
}

fn relic_details(r: &RelicDef) -> Vec<String> {
    vec![format!("{} [{}]", r.name, r.rarity.name()), r.desc.to_string()]
}

fn potion_details(p: &PotionDef) -> Vec<String> {
    vec![p.name.to_string(), p.desc.to_string()]
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
    // 选牌框:里面横排每张候选卡各一个卡框
    if !cards.is_empty() {
        let card_h = (area.y + area.height).saturating_sub(y + others.len() as u16);
        if card_h >= 4 {
            render_card_pick(buf, Rect::new(area.x, y, area.width, card_h), app, &slots, &cards);
        }
        y += card_h;
    }
    // 遗物与药水各占一行
    for i in others {
        if y >= area.y + area.height {
            break;
        }
        let sel = r.index == i;
        let (text, color) = match slots[i] {
            RewardSlot::Relic => match r.relic {
                Some(d) => (format!("Relic  {}  [{}]", d.name, d.rarity.name()), theme::RELIC),
                None => continue,
            },
            RewardSlot::Potion => match r.potion {
                Some(d) => (format!("Potion  {}", d.name), theme::BUFF),
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

/// 选牌框:一个外框,里面横排每张候选卡各一个卡框
fn render_card_pick(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    slots: &[RewardSlot],
    cards: &[usize],
) {
    let r = app.run.reward.as_ref().expect("reward");
    draw_box(
        buf,
        area,
        "pick a card",
        theme::fg(theme::BORDER),
        theme::fg(theme::INFO),
    );
    let n = cards.len() as u16;
    let inner_x = area.x + 1;
    let inner_y = area.y + 1;
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);
    let gap = 1u16;
    let cw = inner_w.saturating_sub(gap * (n - 1)) / n;
    if cw < 6 || inner_h < 3 {
        return;
    }
    for (k, &si) in cards.iter().enumerate() {
        let card = match slots[si] {
            RewardSlot::Card(ci) => &r.cards[ci],
            _ => continue,
        };
        let x = inner_x + k as u16 * (cw + gap);
        let rect = Rect::new(x, inner_y, cw, inner_h);
        card_box(buf, rect, card, r.index == si);
    }
}

/// 一张卡的小框:费用写在框的边上,牌名居中,下面折行写描述
fn card_box(buf: &mut Buffer, rect: Rect, card: &CardInstance, selected: bool) {
    if rect.width < 6 || rect.height < 3 {
        return;
    }
    let border = if selected {
        theme::fg(theme::SEL_FG)
    } else {
        theme::fg(theme::BORDER)
    };
    let title = if selected {
        Style::default().fg(theme::SEL_FG).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::ENERGY).add_modifier(Modifier::BOLD)
    };
    draw_box(buf, rect, &crate::ui::cost_label(card), border, title);
    let inner_w = (rect.width as usize).saturating_sub(2);
    if inner_w == 0 {
        return;
    }
    // 牌名居中
    let row_bg = if selected { theme::SEL_BG } else { theme::BG };
    put_padded(
        buf,
        rect.x + 1,
        rect.y + 1,
        "",
        inner_w,
        Style::default().bg(row_bg),
    );
    let name = card.label();
    let nx = rect.x + 1 + (inner_w.saturating_sub(display_width(&name)) / 2) as u16;
    let name_style = if selected {
        theme::selected()
    } else {
        theme::fg(theme::card_color(card.kind(), card.rarity()))
    };
    put(buf, nx, rect.y + 1, &name, name_style);
    // 描述
    let max_lines = (rect.y + rect.height - 1)
        .saturating_sub(rect.y + 2) as usize;
    let text = card.display_text();
    for (i, line) in wrap_text(&text, inner_w.saturating_sub(2), max_lines)
        .iter()
        .enumerate()
    {
        put_padded(
            buf,
            rect.x + 1,
            rect.y + 2 + i as u16,
            &format!(" {line}"),
            inner_w,
            theme::fg(theme::FG),
        );
    }
}

// ---- 商店 ----

fn shop(buf: &mut Buffer, area: Rect, app: &App) {
    let Some(s) = &app.run.shop else {
        return;
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    for (i, item) in s.items.iter().enumerate() {
        let sold = s.sold.get(i).copied().unwrap_or(false);
        let mut row = match item {
            ShopItem::Card(card, price) => {
                let mut row = card_row(i, card);
                row.text.push_str(&format!("  {price}g"));
                row
            }
            ShopItem::Relic(def, price) => relic_row(i, def, Some(*price)),
            ShopItem::Potion(def, price) => potion_row(i, def, Some(*price)),
            ShopItem::Remove(price) => Row {
                text: format!("{}) Card Removal Service  {price}g", i + 1),
                style: theme::fg(theme::GOOD),
            },
        };
        if sold {
            row.text.push_str("  [SOLD]");
            row.style = theme::dim();
        }
        rows.push(row);
        if i == s.index {
            details = match item {
                ShopItem::Card(card, _) => card_details(card),
                ShopItem::Relic(def, _) => relic_details(def),
                ShopItem::Potion(def, _) => potion_details(def),
                ShopItem::Remove(_) => vec![
                    "Card Removal Service".to_string(),
                    "Remove a card from your deck permanently.".to_string(),
                ],
            };
        }
    }
    draw_list(
        buf,
        area,
        &format!("shop  (gold {})", app.run.player.gold),
        &rows,
        s.index,
        &details,
    );
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
    let upgrading = p.purpose == crate::core::run::PickPurpose::Upgrade;
    let mut rows: Vec<Row> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    for (slot, deck_idx) in cands.iter().enumerate() {
        let card = &app.run.player.deck[*deck_idx];
        // 升级时直接给升级后的样子:名字、费用、描述都是升级后的
        let shown = if upgrading {
            let mut preview = card.clone();
            preview.upgrade();
            preview
        } else {
            card.clone()
        };
        rows.push(card_row(slot, &shown));
        if slot == p.index {
            details = card_details(&shown);
        }
    }
    if rows.is_empty() {
        rows.push(Row {
            text: "(nothing to choose)".to_string(),
            style: theme::dim(),
        });
    }
    draw_list(buf, area, p.purpose.title(), &rows, p.index, &details);
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
