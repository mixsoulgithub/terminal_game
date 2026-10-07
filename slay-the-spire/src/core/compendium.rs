// 图鉴:把语料里的全量卡牌/遗物/药水按"标签页"分组,每组一个条目列表.
// 只做展示;能不能真的打出来/用出去看 cards.rs relics.rs potions.rs.
use crate::core::cards;
use crate::core::corpus;
use crate::core::events;
use crate::core::potions;
use crate::core::relics;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Library {
    Cards,
    Relics,
    Potions,
    Events,
}

impl Library {
    pub const ALL: [Library; 4] =
        [Library::Cards, Library::Relics, Library::Potions, Library::Events];

    pub fn title(self) -> &'static str {
        match self {
            Library::Cards => "card library",
            Library::Relics => "relic collection",
            Library::Potions => "potion lab",
            Library::Events => "event ledger",
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
        // 事件按语料里的 pool 分页;名字/颜色见 EVENT_POOLS
        Library::Events => EVENT_POOLS
            .iter()
            .map(|&(_, title, color)| group(title, color))
            .collect(),
    }
}

/// 事件册的标签页:语料 pool 值 -> (页名, 底色)
const EVENT_POOLS: [(&str, &str, &str); 5] = [
    ("act1", "Act 1", "red"),
    ("act2", "Act 2", "green"),
    ("act3", "Act 3", "blue"),
    ("shrine", "Shrines", "yellow"),
    ("oneTime", "One Time", "purple"),
];

/// 语料 pool 值 -> 页名
fn event_pool_title(pool: &str) -> &'static str {
    EVENT_POOLS
        .iter()
        .find(|(p, _, _)| *p == pool)
        .map(|(_, t, _)| *t)
        .unwrap_or("Other")
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
        Library::Events => event_items(g.name),
    }
}

/// 事件册列表上方那一行统计;别的册没有
pub fn header_note(lib: Library) -> Option<String> {
    match lib {
        Library::Events => {
            let (done, total) = progress(Library::Events);
            let there = corpus::EVENTS
                .iter()
                .filter(|e| slate_cli_event_implemented(e.id))
                .count();
            Some(format!(
                "implemented here {done}/{total}   slay-the-cli {there}/{}",
                corpus::EVENTS.len()
            ))
        }
        _ => None,
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
    known_color && (cards::card_def(c.id).is_some() || events::event_card(c.id).is_some())
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
            done: relics::relic_def(r.id).is_some() || events::event_relic(r.id).is_some(),
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

/// 事件册的一条:名字、acts/pool、语料选项,详情里再附两行对齐信息
fn event_items(tab: &str) -> Vec<Item> {
    let mut src: Vec<&corpus::EventInfo> = corpus::EVENTS.iter().collect();
    src.sort_by_key(|e| e.name);
    src.iter()
        .filter(|e| event_pool_title(e.pool) == tab)
        .map(|e| {
            let here = event_here_implemented(e.id);
            let there = slate_cli_event_implemented(e.id);
            let mut text = String::from("options:");
            for o in e.options {
                text.push_str("\n- ");
                text.push_str(o);
            }
            text.push_str("\n\nhere: ");
            text.push_str(if here { "implemented" } else { "not implemented" });
            text.push_str("\nslay-the-cli: ");
            text.push_str(if there { "implemented" } else { "not implemented" });
            Item {
                name: e.name,
                sub: format!("acts {} / {} pool", e.acts, e.pool),
                tag: String::new(),
                text,
                text_up: String::new(),
                done: here,
                rarity_key: "event",
                tag_up: String::new(),
                kind: "",
                target_tag: String::new(),
                color_key: "gray",
                show_cost: false,
            }
        })
        .collect()
}

/// 本作是否实现了这个事件(只看 events.rs 里的事件表,neow 不在语料里)
fn event_here_implemented(id: &str) -> bool {
    events::EVENTS.iter().any(|e| e.id == id)
}

/// slay-the-cli 参考实现的事件 id(小写,已排序),来自
/// refs/slay-the-cli/src/content/events/{act1,act2,act3,shrines,oneTime}.ts 的 `id: "..."`,
/// 共 51 条,与语料一一对应.
pub const SLATE_CLI_EVENTS: &[&str] = &[
    "ancient_writing", "augmenter", "big_fish", "bonfire_spirits", "colosseum", "cursed_tome",
    "dead_adventurer", "designer_in_spire", "duplicator", "face_trader", "falling",
    "forgotten_altar", "ghosts", "golden_idol", "golden_shrine",
    "hypnotizing_colored_mushrooms", "knowing_skull", "lab", "living_wall", "masked_bandits",
    "match_and_keep", "mindbloom", "mysterious_sphere", "nloth", "note_for_yourself",
    "old_beggar", "ominous_forge", "pleading_vagrant", "purifier", "scrap_ooze",
    "secret_portal", "sensory_stone", "shining_light", "the_cleric", "the_divine_fountain",
    "the_joust", "the_library", "the_mausoleum", "the_moai_head", "the_nest",
    "the_ssssserpent", "the_woman_in_blue", "tomb_of_lord_red_mask", "transmorgrifier",
    "upgrade_shrine", "vampires", "we_meet_again", "wheel_of_change", "winding_halls",
    "wing_statue", "world_of_goop",
];

pub fn slate_cli_event_implemented(id: &str) -> bool {
    SLATE_CLI_EVENTS.binary_search(&id).is_ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_items_match_corpus() {
        let total: usize = groups(Library::Events)
            .iter()
            .enumerate()
            .map(|(i, _)| items(Library::Events, i).len())
            .sum();
        assert_eq!(total, corpus::EVENTS.len());
        assert_eq!(total, 51);
    }

    #[test]
    fn every_event_pool_has_a_tab() {
        for (pool, title, _color) in EVENT_POOLS {
            let want = corpus::EVENTS.iter().filter(|e| e.pool == pool).count();
            let got = groups(Library::Events)
                .iter()
                .position(|g| g.name == title)
                .map(|i| items(Library::Events, i).len());
            assert_eq!(got, Some(want), "pool {pool} 的标签页对不上");
        }
        // 5 个 pool 之外没有别的事件
        let covered: usize = EVENT_POOLS
            .iter()
            .map(|(p, _, _)| corpus::EVENTS.iter().filter(|e| e.pool == *p).count())
            .sum();
        assert_eq!(covered, corpus::EVENTS.len());
    }

    #[test]
    fn event_done_flag_matches_events_rs() {
        let (done, total) = progress(Library::Events);
        assert_eq!(total, corpus::EVENTS.len());
        let want = corpus::EVENTS
            .iter()
            .filter(|e| event_here_implemented(e.id))
            .count();
        assert_eq!(done, want, "已实现计数与 events.rs 对不上");
        // 本作实现的事件都在语料 51 条里(neow 不在语料)
        for e in events::EVENTS {
            if e.id == "neow" {
                continue;
            }
            assert!(
                corpus::EVENTS.iter().any(|c| c.id == e.id),
                "实现的事件 {} 不在语料事件册里",
                e.id
            );
        }
        for (i, _) in groups(Library::Events).iter().enumerate() {
            for it in items(Library::Events, i) {
                let c = corpus::EVENTS
                    .iter()
                    .find(|c| c.name == it.name)
                    .expect("事件名不在语料里");
                assert_eq!(it.done, event_here_implemented(c.id), "{}", c.id);
            }
        }
    }

    #[test]
    fn slate_cli_event_table_matches_corpus() {
        assert_eq!(SLATE_CLI_EVENTS.len(), 51);
        assert!(
            SLATE_CLI_EVENTS.windows(2).all(|w| w[0] < w[1]),
            "常量表要排序好给 binary_search 用"
        );
        for e in corpus::EVENTS {
            assert!(slate_cli_event_implemented(e.id), "参考实现少了 {}", e.id);
        }
        for id in SLATE_CLI_EVENTS {
            assert!(
                corpus::EVENTS.iter().any(|e| e.id == *id),
                "参考实现多了 {id}"
            );
        }
    }

    #[test]
    fn event_header_note_counts() {
        let (done, total) = progress(Library::Events);
        let n = header_note(Library::Events).expect("事件册要有统计行");
        assert!(n.contains(&format!("implemented here {done}/{total}")), "{n}");
        assert!(n.contains("slay-the-cli 51/51"), "{n}");
        assert!(header_note(Library::Cards).is_none());
        assert!(header_note(Library::Relics).is_none());
        assert!(header_note(Library::Potions).is_none());
    }
}
