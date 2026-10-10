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
pub const GOLD: Color = Color::Rgb(250, 186, 66);
pub const BUFF: Color = Color::Rgb(206, 150, 255);
pub const DEBUFF: Color = Color::Rgb(140, 220, 170);
pub const INFO: Color = Color::Rgb(150, 190, 230);
pub const WARN: Color = Color::Rgb(240, 170, 90);
/// 选中色块用到的几个底色
/// 精英与 Boss 的字色(比休息那只血红更亮一点)
pub const ELITE_FG: Color = Color::Rgb(255, 86, 104);

pub fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}

pub fn selected() -> Style {
    Style::default().fg(SEL_FG).bg(SEL_BG).add_modifier(Modifier::BOLD)
}

/// 地图上各类房间的前景色:没选中的房间只有字色,没有底色
pub fn kind_style(kind: NodeKind) -> Style {
    match kind {
        NodeKind::Monster => Style::default().fg(FG),
        NodeKind::Elite => Style::default().fg(ELITE_FG).add_modifier(Modifier::BOLD),
        NodeKind::Event => Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        NodeKind::Rest => Style::default().fg(BLOOD).add_modifier(Modifier::BOLD),
        NodeKind::Shop => Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        NodeKind::Treasure => Style::default()
            .fg(Color::Rgb(246, 226, 120))
            .add_modifier(Modifier::BOLD),
        NodeKind::Boss => Style::default().fg(ELITE_FG).add_modifier(Modifier::BOLD),
    }
}

/// 色块(在路线上/选中/已到达的房间才有底色):
/// 到达=绿,怪物=白,精英与燃烧精英=现在的精英红,休息=现在的休息血红,
/// 商店=金,未知=灰,宝箱=紫;一律黑字
pub const CHIP_ARRIVED: Color = Color::Rgb(46, 140, 74);
pub const CHIP_ENEMY: Color = Color::Rgb(245, 246, 250);
pub const CHIP_ELITE: Color = Color::Rgb(150, 30, 42);
pub const CHIP_REST: Color = Color::Rgb(236, 96, 108);
pub const CHIP_MERCHANT: Color = Color::Rgb(250, 186, 66);
pub const CHIP_EVENT: Color = Color::Rgb(128, 134, 150);
pub const CHIP_TREASURE: Color = Color::Rgb(146, 92, 214);
/// 色块上的字:一律黑字(和终端底色同色)+ 加粗
pub const CHIP_FG: Color = BG;

/// 房间色块(选中/在路线上共用;调用方自己再叠闪烁之类的修饰)
pub fn kind_chip(kind: NodeKind) -> Style {
    let bg = match kind {
        NodeKind::Monster => CHIP_ENEMY,
        NodeKind::Elite => CHIP_ELITE,
        // Boss 和精英同一块底色
        NodeKind::Boss => CHIP_ELITE,
        NodeKind::Event => CHIP_EVENT,
        NodeKind::Rest => CHIP_REST,
        NodeKind::Shop => CHIP_MERCHANT,
        NodeKind::Treasure => CHIP_TREASURE,
    };
    let fg = CHIP_FG;
    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
}

/// 已经走过(含当前站的那间)的房间:绿底白字
pub fn arrived_chip() -> Style {
    Style::default()
        .fg(CHIP_FG)
        .bg(CHIP_ARRIVED)
        .add_modifier(Modifier::BOLD)
}


/// 遗物名字的颜色:按档次——起始/普通默认白,罕见蓝,稀有多橙黄,
/// Boss 红,商店绿,事件紫,兜底(Special)用金币色
pub fn relic_tier_color(tier: crate::core::relics::RelicTier) -> Color {
    use crate::core::relics::RelicTier;
    match tier {
        RelicTier::Uncommon => Color::Rgb(120, 170, 240),
        RelicTier::Rare => Color::Rgb(250, 186, 66),
        RelicTier::Boss => Color::Rgb(236, 96, 108),
        RelicTier::Shop => Color::Rgb(120, 222, 140),
        RelicTier::Event => Color::Rgb(206, 150, 255),
        RelicTier::Special => GOLD,
        RelicTier::Starter | RelicTier::Common => FG,
    }
}

/// 费用/能量里括号的颜色:红绿蓝紫白各按自己,其它一律灰
pub fn energy_color(key: &str) -> Color {
    match key {
        "red" => Color::Rgb(236, 96, 108),
        "green" => Color::Rgb(120, 222, 140),
        "blue" => Color::Rgb(122, 176, 240),
        "purple" => Color::Rgb(206, 150, 255),
        "white" => Color::Rgb(232, 238, 250),
        _ => Color::Rgb(150, 156, 170),
    }
}

/// 标签页选中时的字色:底色亮的页(白/灰)用深黄,不然黄字看不清
pub fn tab_fg(key: &str) -> Color {
    match key {
        "white" | "gray" => Color::Rgb(150, 110, 0),
        _ => YELLOW,
    }
}

/// 标签页的实色
pub fn tab_color(key: &str) -> Color {
    match key {
        "red" => Color::Rgb(214, 82, 96),
        "green" => Color::Rgb(108, 205, 130),
        "blue" => Color::Rgb(108, 162, 232),
        "purple" => Color::Rgb(186, 132, 246),
        "white" => Color::Rgb(216, 222, 236),
        "gray" => Color::Rgb(146, 152, 168),
        "darkgray" => Color::Rgb(88, 94, 110),
        "yellow" => Color::Rgb(232, 202, 92),
        _ => FG,
    }
}

/// 未选中的标签底色:自己的颜色掺灰再压暗
pub fn tab_bg(key: &str) -> Color {
    let Color::Rgb(r, g, b) = tab_color(key) else {
        return Color::Rgb(40, 44, 56);
    };
    let gray = 110.0f32;
    let t = 0.55f32;
    let mix = |v: u8| (v as f32) * (1.0 - t) + gray * t;
    let dim = |v: f32| (v * 0.55).clamp(0.0, 255.0) as u8;
    Color::Rgb(dim(mix(r)), dim(mix(g)), dim(mix(b)))
}

/// 语料里的稀有度/阶字符串 -> 颜色(卡牌、遗物、药水共用)
pub fn corpus_color(rarity: &str) -> Color {
    match rarity {
        "uncommon" => Color::Rgb(120, 170, 240),
        "rare" | "boss" => Color::Rgb(250, 186, 66),
        "special" | "event" | "shop" => BUFF,
        "curse" => DEBUFF,
        _ => FG,
    }
}

/// 角色主色(语料 color 字段)
pub fn role_color(color: &str) -> Color {
    match color {
        "red" => Color::Rgb(236, 96, 108),
        "green" => Color::Rgb(120, 222, 140),
        "blue" => Color::Rgb(122, 176, 240),
        "purple" => Color::Rgb(206, 150, 255),
        "curse" => DEBUFF,
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
