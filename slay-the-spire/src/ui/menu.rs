// 非战斗界面:奖励、商店、营火、事件、宝箱、选牌、结算.
// 统一用"列表 + 详情"的样式,选中行整行反白.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::app::App;
use crate::core::card::CardInstance;
use crate::core::run::{RestOption, RewardSlot, ShopItem};
use crate::ui::theme;
use crate::ui::{display_width, draw_box, put, put_padded, truncate, wrap_text};

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
        .filter(|(_, s)| {
            matches!(
                s,
                RewardSlot::Relic
                    | RewardSlot::RelicChoice(_)
                    | RewardSlot::Potion(_)
                    | RewardSlot::EmeraldKey
            )
        })
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
            &format!("{}Gold  +${}", marker(sel), r.gold),
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
                card_box(
                    buf,
                    Rect::new(x, y, cw, card_h),
                    card,
                    r.index == si,
                    crate::ui::run_energy_color(&app.run),
                );
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
                    theme::relic_tier_color(d.tier),
                ),
                None => continue,
            },
            RewardSlot::RelicChoice(k) => match r.relic_choices.get(k) {
                Some(d) => (
                    format!("Relic  {}  ({})", d.name, d.desc),
                    theme::relic_tier_color(d.tier),
                ),
                None => continue,
            },
            RewardSlot::Potion(i) => match r.potions.get(i).copied() {
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
            RewardSlot::EmeraldKey => (
                "Emerald Key   from the burning elite".to_string(),
                theme::GOOD,
            ),
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
fn card_box(
    buf: &mut Buffer,
    rect: Rect,
    card: &CardInstance,
    selected: bool,
    energy: ratatui::style::Color,
) {
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
    crate::ui::card_desc(buf, inner, card, energy);
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
        // 买不成那一行会抖:只抖左边的名字,价钱和标记不动
        let off = app.shake_offset(idx);
        shop_row(
            buf,
            list.x,
            list.y + i as u16,
            list.width,
            item,
            idx == s.index,
            s.sold.get(idx).copied().unwrap_or(false),
            app.run.player.gold,
            off,
        );
    }
    // 说明
    let Some(item) = s.items.get(s.index) else {
        return;
    };
    let detail = split.detail;
    match item {
        ShopItem::Card(card, _) => {
            crate::ui::card_desc(buf, detail, card, crate::ui::run_energy_color(&app.run))
        }
        ShopItem::Relic(def, _) => put_centered_detail(
            buf,
            detail,
            def.name,
            theme::fg(theme::relic_tier_color(def.tier)),
            Some(def.tier.name()),
            def.desc,
        ),
        ShopItem::Potion(def, _) => put_centered_detail(
            buf,
            detail,
            &format!("({})", def.name),
            theme::fg(theme::corpus_color(def.rarity.name())),
            Some(def.rarity.name()),
            def.desc,
        ),
        ShopItem::Remove(_) => put_centered_detail(
            buf,
            detail,
            "Card Removal Service",
            theme::fg(theme::GOOD),
            None,
            "Remove a card from your deck permanently.",
        ),
    }
}

/// 商品行的名字部分:卡牌用战斗里那套(费用+牌名),其它就一行文字
fn shop_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    item: &ShopItem,
    selected: bool,
    sold: bool,
    gold: i32,
    off: i32,
) {
    if w == 0 {
        return;
    }
    let bg = if selected { theme::SEL_BG } else { theme::BG };
    let base = Style::default().bg(bg);
    put_padded(buf, x, y, "", w as usize, base);
    // 卖光了、或者钱不够:整行压暗,价钱前面挂个标记
    let affordable = gold >= item.price();
    let dead = sold || !affordable;
    let tag = if sold {
        "[sold out] "
    } else if !affordable {
        "[can't afford] "
    } else {
        ""
    };
    let price = format!("{}{:<4}", tag, format!("${}", item.price()));
    let field_w = display_width(&price).min(w as usize);
    // 抖动:整行(名字 + 标记 + 价钱)一起往右挪 shift 格,右边超出去的那截截掉
    let shift = off.max(0) as u16;
    let nx = x + shift;
    let name_w = (w as usize).saturating_sub(field_w + 1 + shift as usize);
    match item {
        ShopItem::Card(card, _) => {
            crate::ui::put_card_line(buf, nx, y, name_w as u16, card, selected, dead);
        }
        ShopItem::Relic(def, _) => {
            // 稀有度不用写出来,名字的颜色就是稀有度
            let style = if dead {
                theme::dim().bg(bg)
            } else {
                theme::fg(theme::relic_tier_color(def.tier)).bg(bg)
            };
            put(buf, nx, y, &truncate(def.name, name_w), style);
        }
        ShopItem::Potion(def, _) => {
            // 药水用 () 括起来
            let style = if dead {
                theme::dim().bg(bg)
            } else {
                theme::fg(theme::corpus_color(def.rarity.name())).bg(bg)
            };
            put(buf, nx, y, &truncate(&format!("({})", def.name), name_w), style);
        }
        ShopItem::Remove(_) => {
            let style = if dead { theme::dim().bg(bg) } else { theme::fg(theme::GOOD).bg(bg) };
            put(buf, nx, y, &truncate("Card Removal Service", name_w), style);
        }
    }
    let style = if dead { theme::dim().bg(bg) } else { theme::fg(theme::GOLD).bg(bg) };
    // 价钱按原位 + 抖动摆,右端超出行的部分截掉(只剩 4 格以下就直接截短)
    let price_x = x + (w as usize - field_w) as u16 + shift;
    let price_w = field_w.saturating_sub(shift as usize).max(1);
    put(buf, price_x, y, &truncate(&price, price_w), style);
}

/// 左对齐写几行说明,按宽度折行
/// 遗物/药水/服务的说明:名字一行、"[稀有度]"一行、介绍若干行,全部居中
fn put_centered_detail(
    buf: &mut Buffer,
    rect: Rect,
    title: &str,
    title_style: Style,
    rarity: Option<&'static str>,
    text: &str,
) {
    let w = rect.width as usize;
    if w == 0 || rect.height == 0 {
        return;
    }
    let bottom = rect.y + rect.height;
    let mut y = rect.y;
    crate::ui::put_centered(buf, rect.x, y, w, title, title_style);
    y += 1;
    if let Some(r) = rarity {
        if y >= bottom {
            return;
        }
        crate::ui::put_centered(buf, rect.x, y, w, &format!("[{}]", r), title_style);
        y += 1;
    }
    y += 1;
    for line in wrap_text(text, w, usize::MAX) {
        if y >= bottom {
            return;
        }
        crate::ui::put_centered(buf, rect.x, y, w, &line, theme::fg(theme::FG));
        y += 1;
    }
}


// ---- 营火 ----

fn rest(buf: &mut Buffer, area: Rect, app: &App) {
    let heal = app.run.player.max_hp * crate::core::run::REST_HEAL_PCT / 100;
    draw_box(
        buf,
        area,
        "rest site",
        theme::fg(theme::BORDER),
        theme::fg(theme::INFO),
    );
    let inner_w = (area.width as usize).saturating_sub(4);
    let inner_x = area.x + 2;
    if inner_w == 0 || area.height < 4 {
        return;
    }
    // 和事件界面一样:没有描述,只有居中的选项,选中靠底色.
    // 选项表由 Run 给(休息/锻造/回忆/举铁/删牌/挖宝)
    let mut choices: Vec<(String, Style)> = Vec::new();
    for opt in app.run.rest_options() {
        let (text, style) = match opt {
            RestOption::Rest => (
                format!("Rest   heal {heal} HP"),
                theme::fg(theme::GOOD),
            ),
            RestOption::Smith => (
                "Smith   upgrade a card".to_string(),
                theme::fg(theme::BLOCK),
            ),
            RestOption::Recall => (
                "Recall   take the Ruby Key".to_string(),
                theme::fg(theme::BAD),
            ),
            RestOption::Lift => (
                format!(
                    "Lift   +1 Strength next combat ({}/3)",
                    app.run.lifts()
                ),
                theme::fg(theme::GOLD),
            ),
            RestOption::Toke => (
                "Toke   remove a card".to_string(),
                theme::fg(theme::INFO),
            ),
            RestOption::Dig => (
                "Dig   dig up a relic".to_string(),
                theme::fg(theme::GOLD),
            ),
        };
        choices.push((text, style));
    }
    let n = choices.len() as u16;
    let mut y = area.y + (area.height.saturating_sub(n)) / 2;
    for (i, (text, base)) in choices.iter().enumerate() {
        if y >= area.y + area.height - 1 {
            break;
        }
        let selected = i == app.rest_index;
        let style = if selected { theme::selected() } else { *base };
        if selected {
            put_padded(buf, inner_x, y, "", inner_w, style);
        }
        crate::ui::put_centered_line(buf, inner_x, y, inner_w, text, style);
        y += 1;
    }
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
    // 提示语多行,每行居中;太长按词折行
    for line in st.def.body {
        for part in wrap_text(line, inner_w, 2) {
            if y >= area.y + area.height - 1 {
                break;
            }
            crate::ui::put_centered_line(buf, area.x + 2, y, inner_w, &part, theme::fg(theme::FG));
            y += 1;
        }
    }
    y += 1;
    match &st.result {
        Some(text) => {
            for part in wrap_text(text.as_str(), inner_w, 3) {
                if y >= area.y + area.height - 1 {
                    break;
                }
                crate::ui::put_centered_line(
                    buf,
                    area.x + 2,
                    y,
                    inner_w,
                    &part,
                    theme::fg(theme::GOOD),
                );
                y += 1;
            }
            if y < area.y + area.height - 1 {
                crate::ui::put_centered_line(
                    buf,
                    area.x + 2,
                    y,
                    inner_w,
                    "press enter or esc to continue",
                    theme::dim(),
                );
            }
        }
        None => {
            for i in 0..app.run.event_choice_count() {
                if y >= area.y + area.height - 1 {
                    break;
                }
                let Some((label, cost_gold, cost_hp)) = app.run.event_choice_row(i) else {
                    continue;
                };
                let selected = i == st.index;
                let available = app.run.event_choice_available(i);
                let mut cost = String::new();
                if cost_gold > 0 {
                    cost.push_str(&format!(" [${}]", cost_gold));
                }
                if cost_hp > 0 {
                    cost.push_str(&format!(" [{}HP]", cost_hp));
                }
                let text = format!("{label}{cost}");
                let style = if !available {
                    theme::dim()
                } else if selected {
                    theme::selected()
                } else {
                    theme::fg(theme::FG)
                };
                // 不带序号和 > ,整行居中;选中靠底色;长选项折行,选中时整块反白
                let lines = wrap_text(&text, inner_w, 3);
                for part in &lines {
                    if y >= area.y + area.height - 1 {
                        break;
                    }
                    if selected {
                        put_padded(buf, area.x + 2, y, "", inner_w, style);
                    }
                    crate::ui::put_centered_line(buf, area.x + 2, y, inner_w, part, style);
                    y += 1;
                }
            }
            // 翻牌棋盘的说明行:上一次翻出来的是什么
            if let Some(note) = app.run.event_note() {
                for part in wrap_text(note, inner_w, 2) {
                    if y >= area.y + area.height - 1 {
                        break;
                    }
                    crate::ui::put_centered_line(buf, area.x + 2, y, inner_w, &part, theme::dim());
                    y += 1;
                }
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
    let inner_x = area.x + 2;
    if inner_w == 0 {
        return;
    }
    // 全部居中
    let mut y = area.y + 2;
    crate::ui::put_centered_line(
        buf,
        inner_x,
        y,
        inner_w,
        "you open the chest...",
        theme::fg(theme::FG),
    );
    y += 2;
    match app.run.treasure {
        Some(relic) => {
            crate::ui::put_centered_line(
                buf,
                inner_x,
                y,
                inner_w,
                &format!("{}  [{}]", relic.name, relic.tier.name()),
                theme::fg(theme::relic_tier_color(relic.tier)),
            );
            if y + 1 < area.y + area.height - 1 {
                crate::ui::put_centered_line(
                    buf,
                    inner_x,
                    y + 1,
                    inner_w,
                    relic.desc,
                    theme::fg(theme::FG),
                );
            }
        }
        None => {
            crate::ui::put_centered_line(buf, inner_x, y, inner_w, "(empty)", theme::dim());
        }
    }
    if y + 3 < area.y + area.height - 1 {
        crate::ui::put_centered_line(
            buf,
            inner_x,
            y + 3,
            inner_w,
            "press enter to take it",
            theme::dim(),
        );
    }
    // 还没有蓝钥匙时,可以按 s 拿钥匙(遗物作废)
    if app.run.chest_sapphire_available() && y + 4 < area.y + area.height - 1 {
        crate::ui::put_centered_line(
            buf,
            inner_x,
            y + 4,
            inner_w,
            "press s to take the Sapphire Key (forfeit the relic)",
            theme::fg(theme::BLOCK),
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
        crate::ui::run_energy_color(&app.run),
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
    let fg = theme::fg(theme::FG);
    let dim = theme::dim();
    let lines: Vec<(String, Style)> = vec![
        (
            if victory {
                // 第四章打掉心脏和第三章"推门离开尖塔"是两种胜利
                if run.act >= 4 {
                    "The Corrupt Heart lies still. The climb is over."
                } else {
                    "You step through the door and leave the Spire behind."
                }
            } else {
                "The spire claims another climber."
            }
            .to_string(),
            fg,
        ),
        (String::new(), dim),
        (format!("seed          {}", run.seed), fg),
        (format!("act           {}", run.act), fg),
        (
            format!("floor         {}/{}", run.floor() + 1, run.map.total_floors()),
            fg,
        ),
        (format!("hp            {}/{}", run.player.hp, run.player.max_hp), fg),
        (
            format!("gold          ${}", run.player.gold),
            theme::fg(theme::GOLD),
        ),
        (format!("deck size     {}", run.player.deck.len()), fg),
        (format!("relics        {}", run.player.relics.len()), fg),
        (format!("fights        {}", run.stats.fights), fg),
        (format!("elites        {}", run.stats.elites), fg),
        (format!("bosses        {}", run.stats.bosses), fg),
        (format!("turns         {}", run.stats.turns), fg),
        (
            format!("damage dealt  {}", run.stats.damage_dealt),
            theme::fg(theme::BLOOD),
        ),
        (format!("potions used  {}", run.stats.potions_used), fg),
        (String::new(), dim),
        (
            "r: new run (same seed)    n: new run (new seed)    :q quit".to_string(),
            dim,
        ),
    ];
    let inner_w = (area.width as usize).saturating_sub(4);
    for (i, (line, style)) in lines.iter().enumerate() {
        let y = area.y + 2 + i as u16;
        if y >= area.y + area.height - 1 {
            break;
        }
        let style = *style;
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
