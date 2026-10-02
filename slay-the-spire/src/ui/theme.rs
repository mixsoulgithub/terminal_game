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

/// 地图节点颜色
pub fn kind_color(kind: NodeKind) -> Color {
    match kind {
        NodeKind::Monster => Color::Rgb(210, 130, 130),
        NodeKind::Elite => Color::Rgb(255, 140, 90),
        NodeKind::Event => Color::Rgb(120, 210, 220),
        NodeKind::Rest => Color::Rgb(140, 220, 160),
        NodeKind::Shop => GOLD,
        NodeKind::Treasure => Color::Rgb(240, 220, 120),
        NodeKind::Boss => Color::Rgb(255, 90, 110),
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
