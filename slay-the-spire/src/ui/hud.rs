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
    // 右上角固定放"第几层/共几层",其它内容都别压过来
    let floor = if run.pos.is_some() { run.floor() + 1 } else { 0 };
    let floor_text = format!("Floor {}/{}", floor, run.map.total_floors());
    let floor_w = display_width(&floor_text) as u16;
    let right_x = (area.x + area.width).saturating_sub(floor_w + 2);
    let limit = right_x.saturating_sub(1).max(area.x + 2);
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
    let mut rest = format!("DECK {}  RELIC {}", p.deck.len(), p.relics.len());
    if let Some(c) = run.combat() {
        rest.push_str(&format!(
            "  draw {} disc {} exh {}",
            c.draw.len(),
            c.discard.len(),
            c.exhaust.len()
        ));
    }
    x = seg(buf, x, y, limit, &rest, dim);

    // 自身增减益
    if let Some(c) = run.combat() {
        let mut s = String::new();
        for (st, n) in c.player.statuses.iter() {
            s.push_str(&format!("{} {}  ", st.short(), n));
        }
        if !s.trim().is_empty() {
            x = seg(
                buf,
                x + 2,
                y,
                limit,
                s.trim_end(),
                Style::default().fg(theme::DEBUFF).add_modifier(Modifier::BOLD),
            );
        }
    }

    // 药水区:写全名,用 " - " 连起来;放不下才把 "Potion" 缩成 "~"
    let names: Vec<&str> = p.potions.iter().flatten().map(|q| q.name).collect();
    let prefix = "Potion ";
    let avail = (limit.saturating_sub(x + 2) as usize).saturating_sub(prefix.len());
    let body = crate::ui::potion_names(&names, avail);
    let potions = if body.is_empty() {
        format!("{prefix}-")
    } else {
        format!("{prefix}{body}")
    };
    let _ = seg(buf, x + 2, y, limit, &potions, theme::fg(theme::GOLD));

    // 最右:Floor
    put(buf, right_x, y, &floor_text, Style::default().fg(theme::INFO));
}
