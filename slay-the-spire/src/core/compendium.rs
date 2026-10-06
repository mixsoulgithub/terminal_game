// 图鉴:把语料里的全量卡牌/遗物/药水整理成"分组标题 + 条目"的列表.
// 只做展示;能不能真的打出来/用出去看 cards.rs relics.rs potions.rs.
use crate::core::cards;
use crate::core::corpus;
use crate::core::potions;
use crate::core::relics;
use crate::core::roster;

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

/// 图鉴里的一行:分组标题或一个条目
pub struct Item {
    /// true = 分组标题行,不可选中
    pub header: bool,
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
    /// 语料里的原始字段,对账用
    pub origin: String,
}

fn header(name: &'static str) -> Item {
    Item {
        header: true,
        name,
        sub: String::new(),
        tag: String::new(),
        text: String::new(),
        text_up: String::new(),
        done: false,
        rarity_key: "",
        origin: String::new(),
    }
}

/// 卡牌分组.和参考实现 src/content/cards/ 的结构一致:
/// 状态牌、诅咒牌各自成组,剩下的按颜色分;
/// 语料里 pool=special 的 token 牌(Shiv/Apparition 这类)和 colorless 一起算无色.
fn card_group(c: &corpus::CardInfo) -> &'static str {
    match c.kind {
        "status" => "Status",
        "curse" => "Curse",
        _ => match c.color {
            "colorless" => "Colorless",
            other => roster::color_group(other),
        },
    }
}

/// 分组先后:四个角色、无色、状态、诅咒
fn card_group_rank(g: &str) -> u8 {
    match g {
        "Ironclad" => 0,
        "Silent" => 1,
        "Defect" => 2,
        "Watcher" => 3,
        "Colorless" => 4,
        "Status" => 5,
        "Curse" => 6,
        _ => 7,
    }
}

/// 已实现的卡牌:目前只有铁甲战士(red)和通用的无色/诅咒牌能打
fn card_implemented(c: &corpus::CardInfo) -> bool {
    let known_color = matches!(c.color, "red" | "colorless" | "curse");
    known_color && cards::card_def(c.id).is_some()
}

pub fn items(lib: Library) -> Vec<Item> {
    match lib {
        Library::Cards => card_items(),
        Library::Relics => relic_items(),
        Library::Potions => potion_items(),
    }
}

fn card_items() -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    // 按 颜色 -> 稀有度 -> 名字 排,分组标题插在组前
    let mut src: Vec<&corpus::CardInfo> = corpus::CARDS.iter().collect();
    src.sort_by_key(|c| {
        (
            card_group_rank(card_group(c)),
            rarity_rank(c.rarity),
            c.name,
        )
    });
    let mut cur = "";
    for c in src {
        let g = card_group(c);
        if g != cur {
            out.push(header(g));
            cur = g;
        }
        let kind = match c.kind {
            "attack" => "Attack",
            "skill" => "Skill",
            "power" => "Power",
            "status" => "Status",
            "curse" => "Curse",
            other => other,
        };
        out.push(Item {
            header: false,
            name: c.name,
            sub: format!("{} / {} / {}", kind, title_case(c.rarity), c.target),
            tag: if c.cost == "-" {
                "-".to_string()
            } else {
                c.cost.to_string()
            },
            text: c.text.to_string(),
            text_up: c.text_up.to_string(),
            done: card_implemented(c),
            rarity_key: c.rarity,
            origin: format!("corpus: color {} / type {} / pool {}", c.color, c.kind, c.pool),
        });
    }
    out
}

fn relic_items() -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut src: Vec<&corpus::RelicInfo> = corpus::RELICS.iter().collect();
    src.sort_by_key(|r| (tier_rank(r.tier), r.name));
    let mut cur = "";
    for r in src {
        if r.tier != cur {
            out.push(header(tier_title(r.tier)));
            cur = r.tier;
        }
        out.push(Item {
            header: false,
            name: r.name,
            sub: format!("{} / {} pool", tier_title(r.tier), r.pool),
            tag: tier_tag(r.tier).to_string(),
            text: r.text.to_string(),
            text_up: String::new(),
            done: relics::relic_def(r.id).is_some(),
            rarity_key: r.tier,
            origin: format!("corpus: tier {} / pool {}", r.tier, r.pool),
        });
    }
    out
}

fn potion_items() -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut src: Vec<&corpus::PotionInfo> = corpus::POTIONS.iter().collect();
    src.sort_by_key(|p| (rarity_rank(p.rarity), p.name));
    let mut cur = "";
    for p in src {
        if p.rarity != cur {
            out.push(header(rarity_title(p.rarity)));
            cur = p.rarity;
        }
        out.push(Item {
            header: false,
            name: p.name,
            sub: format!(
                "{} / {}{}",
                title_case(p.rarity),
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
            origin: format!("corpus: class {} / rarity {}", p.color, p.rarity),
        });
    }
    out
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

fn tier_rank(t: &str) -> u8 {
    match t {
        "starter" => 0,
        "common" => 1,
        "uncommon" => 2,
        "rare" => 3,
        "boss" => 4,
        "shop" => 5,
        "event" => 6,
        "special" => 7,
        _ => 8,
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

/// 图鉴里已实现/未实现的条数,状态栏显示用
pub fn progress(lib: Library) -> (usize, usize) {
    let it = items(lib);
    let total = it.iter().filter(|i| !i.header).count();
    let done = it.iter().filter(|i| !i.header && i.done).count();
    (done, total)
}
