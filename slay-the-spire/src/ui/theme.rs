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
pub const RELIC: Color = Color::Rgb(198, 160, 255);
pub const ATTACK: Color = Color::Rgb(255, 122, 122);
pub const BUFF: Color = Color::Rgb(206, 150, 255);
pub const DEBUFF: Color = Color::Rgb(140, 220, 170);
pub const SLEEP: Color = Color::Rgb(140, 150, 200);
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
        NodeKind::Monster => Style::default().fg(BLOOD),
        NodeKind::Elite => Style::default()
            .fg(SEL_FG)
            .bg(Color::Rgb(150, 30, 42))
            .add_modifier(Modifier::BOLD),
        NodeKind::Event => Style::default().fg(YELLOW),
        NodeKind::Rest => Style::default().fg(BG).bg(BLOOD),
        NodeKind::Shop => Style::default().fg(GOLD),
        NodeKind::Treasure => Style::default().fg(Color::Rgb(246, 226, 120)),
        NodeKind::Boss => Style::default()
            .fg(Color::Rgb(255, 86, 104))
            .add_modifier(Modifier::BOLD),
    }
}

/// 敌人意图颜色
pub fn intent_color(intent: &crate::core::enemy::Intent) -> Color {
    use crate::core::enemy::Intent::*;
    match intent {
        Attack { .. } | AttackDefend { .. } | AttackDebuff { .. } => ATTACK,
        Defend => BLOCK,
        Buff => BUFF,
        Debuff => DEBUFF,
        Sleep => SLEEP,
        Unknown => DIM,
    }
}

/// 卡牌类型颜色;稀有牌再提亮一档
pub fn card_color(kind: crate::core::card::CardType, rarity: crate::core::card::Rarity) -> Color {
    use crate::core::card::{CardType, Rarity};
    let base = match kind {
        CardType::Attack => ATTACK,
        CardType::Skill => Color::Rgb(140, 190, 255),
        CardType::Power => BUFF,
        CardType::Curse => Color::Rgb(200, 90, 160),
        CardType::Status => DIM,
    };
    let bump = match rarity {
        Rarity::Uncommon => 15,
        Rarity::Rare => 35,
        _ => 0,
    };
    if let Color::Rgb(r, g, b) = base {
        Color::Rgb(
            r.saturating_add(bump),
            g.saturating_add(bump * 2 / 3),
            b.saturating_add(bump),
        )
    } else {
        base
    }
}
