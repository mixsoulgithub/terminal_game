// 图鉴:把语料里的全量卡牌/遗物/药水按"标签页"分组,每组一个条目列表.
// 只做展示;能不能真的打出来/用出去看 cards.rs relics.rs potions.rs.
use crate::core::cards;
use crate::core::corpus;
use crate::core::potions;
use crate::core::relics;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Library {
    Cards,
    Relics,
    Potions,
}

impl Library {
    pub const ALL: [Library; 3] = [Library::Cards, Library::Relics, Library::Potions];

    pub fn title(self) -> &'static str {
        match self {
            Library::Cards => "card library",
            Library::Relics => "relic collection",
            Library::Potions => "potion lab",
        }
    }
}

/// 标签页:名字 + 底色(UI 侧按 key 查真色)
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub name: &'static str,
    /// red / green / blue / purple / white / gray / darkgray / yellow
    pub color: &'static str,
}

const fn group(name: &'static str, color: &'static str) -> Group {
    Group { name, color }
}

/// 图鉴里的一条
pub struct Item {
    pub name: &'static str,
    /// 副标题(类型/稀有度/职业)
    pub sub: String,
    /// 左边的短标记(费/阶/瓶)
    pub tag: String,
    /// 正文
    pub text: String,
    /// 升级后的正文(卡牌才有)
    pub text_up: String,
    /// 本游戏实现了没
    pub done: bool,
    /// 上色用的稀有度/阶词
    pub rarity_key: &'static str,
    /// 升级后的费用(卡牌才有)
    pub tag_up: String,
    /// 语料里的类型(attack/skill/power/status/curse)
    pub kind: &'static str,
    /// 目标标签(卡牌才有): "[all enemy]" 这类,没有就空
    pub target_tag: String,
    /// 费用括号的颜色键
    pub color_key: &'static str,
    /// tag 是不是"费用"(卡牌是,遗物/药水不是)
    pub show_cost: bool,
}

/// 卡牌分组:和参考实现的 src/content/cards/ 一致 —— 状态牌、诅咒牌各自成组,
/// 其余按颜色分;语料里 pool=special 的 token 牌算无色。
fn card_group(c: &corpus::CardInfo) -> &'static str {
    match c.kind {
        "status" => "Status",
        "curse" => "Curses",
        _ => match c.color {
            "red" => "Red",
            "green" => "Green",
            "blue" => "Blue",
            "purple" => "Purple",
            _ => "Colorless",
        },
    }
}

pub fn groups(lib: Library) -> Vec<Group> {
    match lib {
        Library::Cards => vec![
            group("Red", "red"),
            group("Green", "green"),
            group("Blue", "blue"),
            group("Purple", "purple"),
            group("Colorless", "white"),
            group("Status", "gray"),
            group("Curses", "darkgray"),
        ],
        Library::Relics => vec![
            group("Starter", "white"),
            group("Common", "gray"),
            group("Uncommon", "blue"),
            group("Rare", "yellow"),
            group("Boss", "red"),
            group("Shop", "yellow"),
            group("Event", "purple"),
            group("Special", "darkgray"),
        ],
        Library::Potions => vec![
            group("Common", "white"),
            group("Uncommon", "blue"),
            group("Rare", "yellow"),
        ],
    }
}

/// 某一页里的条目(按稀有度、名字排)
pub fn items(lib: Library, tab: usize) -> Vec<Item> {
    let gs = groups(lib);
    let Some(g) = gs.get(tab % gs.len().max(1)) else {
        return Vec::new();
    };
    match lib {
        Library::Cards => card_items(g.name),
        Library::Relics => relic_items(g.name),
        Library::Potions => potion_items(g.name),
    }
}

/// 已实现/全部条数,标题上显示
pub fn progress(lib: Library) -> (usize, usize) {
    let gs = groups(lib);
    let mut done = 0;
    let mut total = 0;
    for (i, _) in gs.iter().enumerate() {
        for it in items(lib, i) {
            total += 1;
            if it.done {
                done += 1;
            }
        }
    }
    (done, total)
}

/// 某一页的已实现/条数
pub fn tab_progress(lib: Library, tab: usize) -> (usize, usize) {
    let it = items(lib, tab);
    let total = it.len();
    let done = it.iter().filter(|i| i.done).count();
    (done, total)
}

fn card_implemented(c: &corpus::CardInfo) -> bool {
    let known_color = matches!(c.color, "red" | "colorless" | "curse");
    known_color && cards::card_def(c.id).is_some()
}

fn card_items(tab: &str) -> Vec<Item> {
    let mut src: Vec<&corpus::CardInfo> = corpus::CARDS.iter().collect();
    src.sort_by_key(|c| (rarity_rank(c.rarity), c.name));
    src.iter()
        .filter(|c| card_group(c) == tab)
        .map(|c| {
            let kind = match c.kind {
                "attack" => "Attack",
                "skill" => "Skill",
                "power" => "Power",
                "status" => "Status",
                "curse" => "Curse",
                other => other,
            };
            Item {
                name: c.name,
                sub: format!("{} / {}", kind, title_case(c.rarity)),
                tag: c.cost.to_string(),
                tag_up: if c.cost_up.is_empty() { c.cost.to_string() } else { c.cost_up.to_string() },
                kind: c.kind,
                target_tag: target_tag(c.target).to_string(),
                show_cost: true,
                color_key: match c.color {
                    "red" => "red",
                    "green" => "green",
                    "blue" => "blue",
                    "purple" => "purple",
                    "colorless" => "white",
                    _ => "gray",
                },
                text: c.text.to_string(),
                text_up: c.text_up.to_string(),
                done: card_implemented(c),
                rarity_key: c.rarity,
            }
        })
        .collect()
}

fn relic_items(tab: &str) -> Vec<Item> {
    let mut src: Vec<&corpus::RelicInfo> = corpus::RELICS.iter().collect();
    src.sort_by_key(|r| r.name);
    src.iter()
        .filter(|r| tier_title(r.tier) == tab)
        .map(|r| Item {
            name: r.name,
            sub: format!("{} / {} pool", tier_title(r.tier), r.pool),
            tag: tier_tag(r.tier).to_string(),
            text: r.text.to_string(),
            text_up: String::new(),
            done: relics::relic_def(r.id).is_some(),
            rarity_key: r.tier,
            tag_up: String::new(),
            kind: "",
            target_tag: String::new(),
            color_key: "gray",
            show_cost: false,
        })
        .collect()
}

fn potion_items(tab: &str) -> Vec<Item> {
    let mut src: Vec<&corpus::PotionInfo> = corpus::POTIONS.iter().collect();
    src.sort_by_key(|p| p.name);
    src.iter()
        .filter(|p| rarity_title(p.rarity) == tab)
        .map(|p| Item {
            name: p.name,
            sub: format!(
                "{} / {}{}",
                rarity_title(p.rarity),
                if p.targeted { "targeted" } else { "self" },
                if p.color.is_empty() {
                    String::new()
                } else {
                    format!(" / {}", p.color)
                }
            ),
            tag: "o".to_string(),
            text: p.text.to_string(),
            text_up: String::new(),
            done: potions::POTIONS.iter().any(|d| d.id == p.id),
            rarity_key: p.rarity,
            tag_up: String::new(),
            kind: "",
            target_tag: String::new(),
            color_key: "gray",
            show_cost: false,
        })
        .collect()
}

/// 语料里的 target -> 列表右边的小标签
fn target_tag(target: &str) -> &'static str {
    match target {
        "enemy" => "[enemy]",
        "allenemy" => "[all enemy]",
        "self" => "[self]",
        "selfandenemy" => "[self & enemy]",
        "all" => "[all]",
        _ => "",
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn rarity_rank(r: &str) -> u8 {
    match r {
        "basic" => 0,
        "common" => 1,
        "uncommon" => 2,
        "rare" => 3,
        "boss" => 4,
        "special" => 5,
        "curse" => 6,
        _ => 7,
    }
}

fn rarity_title(r: &str) -> &'static str {
    match r {
        "basic" => "Basic",
        "common" => "Common",
        "uncommon" => "Uncommon",
        "rare" => "Rare",
        _ => "Other",
    }
}

fn tier_title(t: &str) -> &'static str {
    match t {
        "starter" => "Starter",
        "common" => "Common",
        "uncommon" => "Uncommon",
        "rare" => "Rare",
        "boss" => "Boss",
        "shop" => "Shop",
        "event" => "Event",
        "special" => "Special",
        _ => "Other",
    }
}

fn tier_tag(t: &str) -> &'static str {
    match t {
        "starter" => "S",
        "common" => "C",
        "uncommon" => "U",
        "rare" => "R",
        "boss" => "B",
        "shop" => "$",
        "event" => "?",
        "special" => "*",
        _ => "-",
    }
}
