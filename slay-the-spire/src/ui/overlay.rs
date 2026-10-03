// 叠加层:牌组、地图、遗物、药水、帮助.
// 任何界面都能开;再按一次同一个键(或 esc)关掉.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::app::App;
use crate::core::card::CardInstance;
use crate::core::run::HistoryKind;
use crate::ui::theme;
use crate::ui::{draw_box, put, put_padded, truncate};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    /// 整条路的样子,只读
    Map,
    /// 战斗里连抽牌堆/手牌/弃牌堆/消耗堆一起看
    Deck,
    Relics,
    Potions,
    /// 一整局发生过的事
    History,
    Help,
}

impl Overlay {
    pub fn title(self) -> &'static str {
        match self {
            Overlay::Map => "map",
            Overlay::Deck => "cards",
            Overlay::Relics => "relics",
            Overlay::Potions => "potions",
            Overlay::History => "history",
            Overlay::Help => "help",
        }
    }

    /// 关掉它要按的键,提示用
    pub fn close_key(self) -> &'static str {
        match self {
            Overlay::Map => "m",
            Overlay::Deck => "d",
            Overlay::Relics => "r",
            Overlay::Potions => "p",
            Overlay::History => "H",
            Overlay::Help => "?",
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
        Overlay::Map => {}
        Overlay::Deck => match run.combat() {
            Some(c) => {
                // 战斗中:把这场战斗里的每一堆都摊开
                let mut section = |name: &str, pile: &[CardInstance], style: Style| {
                    out.push((format!("{name} ({})", pile.len()), theme::fg(theme::INFO)));
                    if pile.is_empty() {
                        out.push(("    (empty)".to_string(), theme::dim()));
                    }
                    for (i, card) in pile.iter().enumerate() {
                        out.push((
                            format!("{:>3}. {}", i + 1, card_line(card)),
                            style,
                        ));
                    }
                };
                section("hand", &c.hand, theme::fg(theme::SEL_FG));
                section("draw pile", &c.draw, theme::dim());
                section("discard pile", &c.discard, theme::dim());
                section("exhausted", &c.exhaust, theme::dim());
            }
            None => {
                let deck = &run.player.deck;
                out.push((format!("{} cards in deck", deck.len()), theme::fg(theme::INFO)));
                for (i, c) in deck.iter().enumerate() {
                    out.push((
                        format!("{:>3}. {}", i + 1, card_line(c)),
                        theme::fg(theme::card_color(c.kind(), c.rarity())),
                    ));
                }
            }
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
                "1-3 drink that potion, t then 1-3 toss it".to_string(),
                theme::fg(theme::INFO),
            ));
        }
        Overlay::History => {
            out.push((
                format!("{} entries in this run", run.history.len()),
                theme::fg(theme::INFO),
            ));
            for e in run.history.iter() {
                let style = match e.kind {
                    HistoryKind::System => theme::fg(theme::FG),
                    HistoryKind::Player => theme::fg(theme::GOOD),
                    HistoryKind::Enemy => theme::fg(theme::BAD),
                    HistoryKind::Info => theme::dim(),
                };
                out.push((e.text.clone(), style));
            }
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
    if ov == Overlay::Map {
        render_map(buf, area, app);
        return;
    }
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
    let title = format!(
        "{}  (j/k scroll, {} or esc close)",
        ov.title(),
        ov.close_key()
    );
    draw_box(buf, rect, &title, theme::fg(theme::SEL_FG), theme::fg(theme::INFO));
    let rows = lines(app, ov);
    let inner_h = rect.height.saturating_sub(2) as usize;
    // 夹在"最多能滚到最后一屏"的位置:历史记录打开时就是直接看最新几条
    let scroll = (app.overlay_scroll as usize).min(rows.len().saturating_sub(inner_h));
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

/// 地图叠加层:整屏铺开,只读,自带 h/l 看路
fn render_map(buf: &mut Buffer, area: Rect, app: &App) {
    if area.height < 6 {
        return;
    }
    // 先把整屏刷成底色,不然底下的战斗界面会从地图缝隙里透出来
    for y in area.y..area.y + area.height {
        put_padded(
            buf,
            area.x,
            y,
            "",
            area.width as usize,
            Style::default().bg(theme::BG),
        );
    }
    let title = "map  (h/l look along the road, m or esc close)";
    put(buf, area.x + 2, area.y, title, theme::fg(theme::INFO));
    let inner = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    crate::ui::mapview::render(buf, inner, app);
}
