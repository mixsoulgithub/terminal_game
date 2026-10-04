// 配色:延续 neon 的 24 位真彩风格,但游戏里不做流动动画,颜色只用来区分信息.
use ratatui::style::{Color, Modifier, Style};

use crate::core::map::NodeKind;

pub const BG: Color = Color::Rgb(9, 10, 17);
pub const FG: Color = Color::Rgb(212, 218, 232);
pub const DIM: Color = Color::Rgb(112, 120, 146);
pub const BORDER: Color = Color::Rgb(62, 74, 110);
pub const SEL_BG: Color = Color::Rgb(46, 56, 96);
pub const SEL_FG: Color = Color::Rgb(242, 246, 255);
pub const GOOD: Color = Color::Rgb(120, 222, 140);
pub const BAD: Color = Color::Rgb(236, 96, 108);
/// 血的颜色(顶栏和地图上的怪都用它)
pub const BLOOD: Color = Color::Rgb(236, 96, 108);
pub const YELLOW: Color = Color::Rgb(238, 226, 104);
pub const BLOCK: Color = Color::Rgb(122, 176, 240);
pub const ENERGY: Color = Color::Rgb(238, 226, 104);
pub const GOLD: Color = Color::Rgb(250, 186, 66);
pub const BUFF: Color = Color::Rgb(206, 150, 255);
pub const DEBUFF: Color = Color::Rgb(140, 220, 170);
pub const INFO: Color = Color::Rgb(150, 190, 230);
pub const WARN: Color = Color::Rgb(240, 170, 90);

pub fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}

pub fn selected() -> Style {
    Style::default().fg(SEL_FG).bg(SEL_BG).add_modifier(Modifier::BOLD)
}

/// 地图上各类房间的颜色与底色:
/// E 红字 = 普通怪,E 红底 = 精英,R 红底(更亮)= 休息,? 黄 = 未知
pub fn kind_style(kind: NodeKind) -> Style {
    match kind {
        NodeKind::Monster => Style::default().fg(FG),
        NodeKind::Elite => Style::default()
            .fg(SEL_FG)
            .bg(Color::Rgb(150, 30, 42))
            .add_modifier(Modifier::BOLD),
        NodeKind::Event => Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        NodeKind::Rest => Style::default().fg(BG).bg(BLOOD).add_modifier(Modifier::BOLD),
        NodeKind::Shop => Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        NodeKind::Treasure => Style::default().fg(Color::Rgb(246, 226, 120)).add_modifier(Modifier::BOLD),
        NodeKind::Boss => Style::default()
            .fg(Color::Rgb(255, 86, 104))
            .add_modifier(Modifier::BOLD),
    }
}

/// 遗物名字的颜色:按稀有度——starter(基本)/common 默认白,uncommon 蓝,
/// rare 橙黄;Special 暂按金币色处理(数据里目前只有前四种)
pub fn relic_color(rarity: crate::core::card::Rarity) -> Color {
    use crate::core::card::Rarity;
    match rarity {
        Rarity::Uncommon => Color::Rgb(120, 170, 240),
        Rarity::Rare => Color::Rgb(250, 186, 66),
        Rarity::Special => GOLD,
        _ => FG,
    }
}

/// 卡牌名字的颜色:只看稀有度——Basic/Common 白,Uncommon 蓝,Rare 橙黄,Special 也当白
pub fn card_color(rarity: crate::core::card::Rarity) -> Color {
    use crate::core::card::Rarity;
    match rarity {
        Rarity::Uncommon => Color::Rgb(120, 170, 240),
        Rarity::Rare => Color::Rgb(250, 186, 66),
        _ => FG,
    }
}
