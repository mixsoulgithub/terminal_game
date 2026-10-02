// 顶栏:一行的玩家状态.战斗里额外显示能量、牌堆计数与自身状态.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::ui::theme;
use crate::ui::{display_width, put, truncate};

fn segment(buf: &mut Buffer, x: &mut u16, y: u16, limit: u16, text: &str, style: Style) {
    if *x >= limit {
        return;
    }
    let w = (limit - *x) as usize;
    let t = truncate(text, w);
    put(buf, *x, y, &t, style);
    *x += display_width(&t) as u16 + 2;
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let run = &app.run;
    let p = &run.player;
    let y = area.y;
    let mut x = area.x;
    let limit = area.x + area.width;

    let hp_ratio = if p.max_hp > 0 {
        p.hp as f32 / p.max_hp as f32
    } else {
        0.0
    };
    let hp_text = format!("HP [{}] {}/{}", theme::bar(hp_ratio, 12), p.hp, p.max_hp);
    segment(
        buf,
        &mut x,
        y,
        limit,
        &hp_text,
        theme::fg(theme::hp_color(hp_ratio)),
    );

    if let Some(c) = run.combat() {
        segment(
            buf,
            &mut x,
            y,
            limit,
            &format!("EN {}/{}", c.energy, c.max_energy),
            Style::default()
                .fg(theme::ENERGY)
                .add_modifier(Modifier::BOLD),
        );
        if c.player.block > 0 {
            segment(
                buf,
                &mut x,
                y,
                limit,
                &format!("BLK {}", c.player.block),
                theme::fg(theme::BLOCK),
            );
        }
    }

    segment(
        buf,
        &mut x,
        y,
        limit,
        &format!("GOLD {}", p.gold),
        theme::fg(theme::GOLD),
    );
    let floor = if run.pos.is_some() { run.floor() + 1 } else { 0 };
    segment(
        buf,
        &mut x,
        y,
        limit,
        &format!("F{}/{}", floor, run.map.total_floors()),
        theme::fg(theme::INFO),
    );
    let potions = p.potions.iter().flatten().count();
    segment(
        buf,
        &mut x,
        y,
        limit,
        &format!("DECK {} POT {} RELIC {}", p.deck.len(), potions, p.relics.len()),
        theme::dim(),
    );
    if let Some(c) = run.combat() {
        segment(
            buf,
            &mut x,
            y,
            limit,
            &format!(
                "DRAW {} DISC {} EXH {}",
                c.draw.len(),
                c.discard.len(),
                c.exhaust.len()
            ),
            theme::dim(),
        );
    }
    if let Some(c) = run.combat() {
        let mut s = String::new();
        for (st, n) in c.player.statuses.iter() {
            s.push_str(&format!("{} {}  ", st.short(), n));
        }
        if !s.is_empty() {
            segment(
                buf,
                &mut x,
                y,
                limit,
                s.trim_end(),
                Style::default()
                    .fg(theme::DEBUFF)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}
