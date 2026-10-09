// 顶栏:一行纯数字,靠颜色区分含义——血红、格挡蓝、能量黄、金币金黄.
// 能量不在这里(它挪到手牌旁边),层数/牌数这类次要信息压在后面,用暗色.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

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

pub fn render(buf: &mut Buffer, area: Rect, app: &App) -> Rect {
    let run = &app.run;
    let p = &run.player;
    let y = area.y;
    // 左上角空出两格:有些终端会吃掉第一个格子
    let mut x = area.x + 2;
    let limit = area.x + area.width;
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
    x += 2;

    // 三把钥匙(绿/红/蓝):拿到的是彩字,没有的是暗色 -
    x = seg(buf, x, y, limit, "K:", theme::dim());
    let key = |owned: bool, c| {
        if owned {
            theme::fg(c)
        } else {
            theme::dim()
        }
    };
    x = seg(buf, x, y, limit, if run.keys.emerald { "E" } else { "-" }, key(run.keys.emerald, theme::GOOD));
    x = seg(buf, x, y, limit, if run.keys.ruby { "R" } else { "-" }, key(run.keys.ruby, theme::BAD));
    x = seg(buf, x, y, limit, if run.keys.sapphire { "S" } else { "-" }, key(run.keys.sapphire, theme::BLOCK));
    x += 3;

    // 最右:Act 紧挨着 Floor 再紧挨着 Deck,整组右对齐,右边留 2 格.
    // 药水区要画在它左边,所以先把这一组算出来(窄终端上三瓶长名字会压过来)
    let floor = if run.pos.is_some() { run.floor() + 1 } else { 0 };
    let floor_text = format!("Act {}  Floor {}/{}", run.act, floor, run.map.total_floors());
    let asc = if run.ascension > 0 {
        format!("A{}   ", run.ascension)
    } else {
        String::new()
    };
    let right = format!("{asc}{}   Deck {}", floor_text, p.deck.len());
    let rx = (area.x + area.width).saturating_sub(display_width(&right) as u16 + 2);

    // 药水区:每个槽位一个 (),空的也写出来;选中那个铺底色
    let px0 = x + 2;
    let avail = rx.saturating_sub(px0 + 1) as usize;
    let labels = |short: bool| -> Vec<String> {
        p.potions
            .iter()
            .map(|slot| match slot {
                Some(d) => {
                    if short {
                        format!("({})", crate::ui::potion_label(d.name))
                    } else {
                        format!("({})", d.name)
                    }
                }
                None => "()".to_string(),
            })
            .collect()
    };
    let mut texts = labels(false);
    let total: usize = texts.iter().map(|t| display_width(t)).sum();
    if total > avail {
        texts = labels(true);
    }
    let mut px = px0;
    for (i, text) in texts.iter().enumerate() {
        if px >= rx {
            break;
        }
        // 空格子也上色,不压暗;放不下就截断,别压到右边那一组
        let style = if app.potion_sel == Some(i) {
            theme::selected()
        } else {
            theme::fg(theme::GOLD)
        };
        let t = truncate(text, rx.saturating_sub(px) as usize);
        put(buf, px, y, &t, style);
        px += display_width(&t) as u16;
    }
    let potion_rect = Rect::new(px0, y, px.saturating_sub(px0), 1);

    let _ = seg(buf, rx, y, limit, &right, dim);
    potion_rect
}
