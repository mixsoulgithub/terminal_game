// 顶栏:一行纯数字,靠颜色区分含义——血红、格挡蓝、能量黄、金币金黄.
// 能量不在这里(它挪到手牌旁边),层数/牌数这类次要信息压在后面,用暗色.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::ui::theme;
use crate::ui::{display_width, put, truncate};

/// 写一段文本,返回推进后的 x
fn seg(buf: &mut Buffer, x: u16, y: u16, limit: u16, text: &str, style: Style) -> u16 {
    if x >= limit {
        return x;
    }
    let t = truncate(text, (limit - x) as usize);
    put(buf, x, y, &t, style);
    x + display_width(&t) as u16
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App) {
    let run = &app.run;
    let p = &run.player;
    let y = area.y;
    let limit = area.x + area.width;
    // 左上空出两格:有些终端在左上角会吃掉第一个格子
    let mut x = area.x + 2;
    let dim = theme::dim();

    // 血量 / 上限 / 格挡
    x = seg(buf, x, y, limit, &p.hp.to_string(), theme::fg(theme::BAD));
    x = seg(buf, x, y, limit, "/", theme::dim());
    x = seg(buf, x, y, limit, &p.max_hp.to_string(), theme::fg(theme::BAD));
    x = seg(buf, x, y, limit, "/", theme::dim());
    let block = run.combat().map(|c| c.player.block).unwrap_or(0);
    x = seg(buf, x, y, limit, &block.to_string(), theme::fg(theme::BLOCK));
    x += 3;

    // 金币
    x = seg(buf, x, y, limit, &format!("${}", p.gold), theme::fg(theme::GOLD));
    x += 3;

    // 次要信息
    let floor = if run.pos.is_some() { run.floor() + 1 } else { 0 };
    let potions = p.potions.iter().flatten().count();
    let mut rest = format!(
        "F{floor}/{}  DECK {}  RELIC {}  POT {potions}",
        run.map.total_floors(),
        p.deck.len(),
        p.relics.len()
    );
    if let Some(c) = run.combat() {
        rest.push_str(&format!(
            "  draw {} disc {} exh {}",
            c.draw.len(),
            c.discard.len(),
            c.exhaust.len()
        ));
    }
    x = seg(buf, x, y, limit, &rest, dim);

    // 自身增减益:短名 + 层数
    if let Some(c) = run.combat() {
        let mut s = String::new();
        for (st, n) in c.player.statuses.iter() {
            s.push_str(&format!("{} {}  ", st.short(), n));
        }
        if !s.is_empty() {
            let _ = x;
            let _ = seg(
                buf,
                x,
                y,
                limit,
                s.trim_end(),
                Style::default().fg(theme::DEBUFF).add_modifier(Modifier::BOLD),
            );
        }
    }
}
