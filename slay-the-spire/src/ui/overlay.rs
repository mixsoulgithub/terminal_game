// 叠加层:牌组、弃牌堆、消耗堆、遗物、药水、帮助.
// 都是只读的滚动列表,j/k 翻,esc 或 q 关.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::app::App;
use crate::core::card::CardInstance;
use crate::ui::theme;
use crate::ui::{draw_box, put_padded, truncate};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    Deck,
    Discard,
    Exhaust,
    Relics,
    Potions,
    Help,
}

impl Overlay {
    pub fn title(self) -> &'static str {
        match self {
            Overlay::Deck => "deck",
            Overlay::Discard => "discard pile",
            Overlay::Exhaust => "exhausted",
            Overlay::Relics => "relics",
            Overlay::Potions => "potions",
            Overlay::Help => "help",
        }
    }
}

fn card_line(card: &CardInstance) -> String {
    let cost = match card.cost() {
        crate::core::card::Cost::Fixed(n) => n.to_string(),
        crate::core::card::Cost::X => "X".to_string(),
        crate::core::card::Cost::Unplayable => "-".to_string(),
    };
    format!(
        "{:<18} cost {:<2} {:<8} {}",
        card.label(),
        cost,
        card.kind().name(),
        card.display_text()
    )
}

/// 叠加层要显示的所有行(按需生成,不缓存)
pub fn lines(app: &App, ov: Overlay) -> Vec<(String, Style)> {
    let run = &app.run;
    let mut out: Vec<(String, Style)> = Vec::new();
    match ov {
        Overlay::Deck => {
            let deck = &run.player.deck;
            out.push((
                format!("{} cards in deck", deck.len()),
                theme::fg(theme::INFO),
            ));
            for (i, c) in deck.iter().enumerate() {
                out.push((
                    format!("{:>3}. {}", i + 1, card_line(c)),
                    theme::fg(theme::card_color(c.kind(), c.rarity())),
                ));
            }
        }
        Overlay::Discard => match run.combat() {
            Some(c) => {
                out.push((
                    format!("{} cards in the discard pile", c.discard.len()),
                    theme::fg(theme::INFO),
                ));
                for (i, card) in c.discard.iter().enumerate() {
                    out.push((
                        format!("{:>3}. {}", i + 1, card_line(card)),
                        theme::fg(theme::card_color(card.kind(), card.rarity())),
                    ));
                }
            }
            None => out.push(("no combat right now".to_string(), theme::dim())),
        },
        Overlay::Exhaust => match run.combat() {
            Some(c) => {
                out.push((
                    format!("{} cards exhausted this combat", c.exhaust.len()),
                    theme::fg(theme::INFO),
                ));
                for (i, card) in c.exhaust.iter().enumerate() {
                    out.push((
                        format!("{:>3}. {}", i + 1, card_line(card)),
                        theme::dim(),
                    ));
                }
            }
            None => out.push(("no combat right now".to_string(), theme::dim())),
        },
        Overlay::Relics => {
            for (i, r) in run.player.relics.iter().enumerate() {
                out.push((
                    format!("{:>3}. {:<20} [{}]", i + 1, r.name, r.rarity.name()),
                    theme::fg(theme::RELIC),
                ));
                out.push((format!("     {}", r.desc), theme::dim()));
            }
        }
        Overlay::Potions => {
            for (i, slot) in run.player.potions.iter().enumerate() {
                match slot {
                    Some(p) => {
                        out.push((
                            format!("{:>3}. {:<20} [{}]", i + 1, p.name, p.rarity.name()),
                            theme::fg(theme::BUFF),
                        ));
                        out.push((format!("     {}", p.desc), theme::dim()));
                    }
                    None => out.push((format!("{:>3}. (empty)", i + 1), theme::dim())),
                }
            }
            out.push((String::new(), theme::dim()));
            out.push((
                "in this list: 1-3 quaff the potion, t then slot to toss".to_string(),
                theme::fg(theme::INFO),
            ));
        }
        Overlay::Help => {
            for (k, v) in app.help_rows() {
                out.push((format!("  {:<16} {}", k, v), theme::fg(theme::FG)));
            }
        }
    }
    out
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App, ov: Overlay) {
    let w = area.width.saturating_sub(8).min(96).max(30);
    let h = area.height.saturating_sub(4).max(6);
    let rect = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    // 先清底,免得和后面的界面文字糊在一起
    for y in rect.y..rect.y + rect.height {
        put_padded(buf, rect.x, y, "", rect.width as usize, Style::default().bg(theme::BG));
    }
    let title = format!("{}  (j/k scroll, esc close)", ov.title());
    draw_box(buf, rect, &title, theme::fg(theme::SEL_FG), theme::fg(theme::INFO));
    let rows = lines(app, ov);
    let inner_h = rect.height.saturating_sub(2) as usize;
    let scroll = (app.overlay_scroll as usize).min(rows.len().saturating_sub(1));
    let inner_w = rect.width.saturating_sub(4) as usize;
    for i in 0..inner_h {
        let Some((text, style)) = rows.get(scroll + i) else {
            break;
        };
        put_padded(
            buf,
            rect.x + 2,
            rect.y + 1 + i as u16,
            &truncate(text, inner_w),
            inner_w,
            *style,
        );
    }
    // 底部指示还有多少行
    if rows.len() > inner_h {
        let footer = format!(" {}/{} ", (scroll + inner_h).min(rows.len()), rows.len());
        put_padded(
            buf,
            rect.x + rect.width - footer.len() as u16 - 2,
            rect.y + rect.height - 1,
            &footer,
            footer.len(),
            theme::fg(theme::WARN),
        );
    }
}
