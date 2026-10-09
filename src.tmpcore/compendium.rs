// 图鉴:把语料里的全量卡牌/遗物/药水/事件/怪物按"标签页"分组,每组一个条目列表.
// 只做展示;能不能真的打出来/用出去看 cards.rs relics.rs potions.rs events.rs enemies.rs.
use crate::core::cards;
use crate::core::corpus;
use crate::core::enemies;
use crate::core::events;
use crate::core::potions;
use crate::core::relics;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Library {
    Cards,
    Relics,
    Potions,
    Events,
    Enemies,
}

impl Library {
    pub const ALL: [Library; 5] = [
        Library::Cards,
        Library::Relics,
        Library::Potions,
        Library::Events,
        Library::Enemies,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Library::Cards => "card library",
            Library::Relics => "relic collection",
            Library::Potions => "potion lab",
            Library::Events => "event ledger",
            Library::Enemies => "enemy list",
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
        // 怪物按层分页,见 MONSTER_ACTS
        Library::Enemies => MONSTER_ACTS
            .iter()
            .map(|&(_, title, color)| group(title, color))
            .collect(),
    }
}

/// 怪物册的标签页:(层关键词, 页名, 底色)
const MONSTER_ACTS: [(&str, &str, &str); 3] = [
    ("1", "Act 1", "red"),
    ("2", "Act 2", "green"),
    ("34", "Act 3+4", "blue"),
];

/// 怪物在哪一页:只看它出现的最早一层(语料 acts 是升序),
/// 3 层和 4 层合成一页,这样每只怪物只出现一次
fn monster_tab(m: &corpus::MonsterInfo) -> &'static str {
    let first = m.acts.split(',').next().unwrap_or("");
    MONSTER_ACTS
        .iter()
        .find(|(act, _, _)| match *act {
            "34" => first == "3" || first == "4",
            a => first == a,
        })
        .map(|(_, title, _)| *title)
        .unwrap_or("Other")
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
        Library::Enemies => monster_items(g.name),
    }
}

/// 事件册和怪物册列表上方那一行统计;别的册没有
pub fn header_note(lib: Library) -> Option<String> {
    let (there, all) = match lib {
        Library::Events => (
            corpus::EVENTS
                .iter()
                .filter(|e| slate_cli_event_implemented(e.id))
                .count(),
            corpus::EVENTS.len(),
        ),
        Library::Enemies => (
            corpus::MONSTERS
                .iter()
                .filter(|m| slate_cli_monster_implemented(m.corpus_id))
                .count(),
            corpus::MONSTERS.len(),
        ),
        _ => return None,
    };
    let (done, total) = progress(lib);
    Some(format!(
        "implemented here {done}/{total}   slay-the-cli {there}/{all}"
    ))
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

/// 怪物册的一条:名字、类别/层数,详情里是血量和招式名,最后附两行对齐信息
fn monster_items(tab: &str) -> Vec<Item> {
    let mut src: Vec<&corpus::MonsterInfo> = corpus::MONSTERS.iter().collect();
    src.sort_by_key(|m| (category_rank(m.category), m.name));
    src.iter()
        .filter(|m| monster_tab(m) == tab)
        .map(|m| {
            let here = monster_here_implemented(m.id);
            let there = slate_cli_monster_implemented(m.corpus_id);
            let mut text = format!(
                "hp {}\nasc {}",
                hp_range(m.hp_lo, m.hp_hi),
                hp_range(m.hp_asc_lo, m.hp_asc_hi)
            );
            text.push_str("\nmoves:");
            for mv in m.moves {
                text.push_str("\n- ");
                text.push_str(&move_title(m.corpus_id, mv));
            }
            text.push_str("\n\nhere: ");
            text.push_str(if here { "implemented" } else { "not implemented" });
            text.push_str("\nslay-the-cli: ");
            text.push_str(if there { "implemented" } else { "not implemented" });
            Item {
                name: m.name,
                sub: format!("{} / acts {}", category_title(m.category), m.acts),
                tag: category_tag(m.category).to_string(),
                text,
                text_up: String::new(),
                done: here,
                rarity_key: category_color_key(m.category),
                tag_up: String::new(),
                kind: "",
                target_tag: String::new(),
                color_key: "gray",
                show_cost: false,
            }
        })
        .collect()
}

/// 血量区间:同值只写一个数
fn hp_range(lo: i32, hi: i32) -> String {
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}-{hi}")
    }
}

fn category_title(c: &str) -> &'static str {
    match c {
        "normal" => "normal",
        "elite" => "elite",
        "boss" => "boss",
        "minion" => "minion",
        "event" => "event",
        _ => "other",
    }
}

/// 排序用的类别顺序:先普通,再精英、Boss、小怪、事件
fn category_rank(c: &str) -> u8 {
    match c {
        "normal" => 0,
        "elite" => 1,
        "boss" => 2,
        "minion" => 3,
        "event" => 4,
        _ => 5,
    }
}

/// 列表左边的小标记
fn category_tag(c: &str) -> &'static str {
    match c {
        "normal" => "N",
        "elite" => "E",
        "boss" => "B",
        "minion" => "M",
        "event" => "?",
        _ => "-",
    }
}

/// 名字的颜色:借用现有的稀有度配色
fn category_color_key(c: &str) -> &'static str {
    match c {
        "elite" => "uncommon",
        "boss" => "boss",
        "minion" => "special",
        "event" => "event",
        _ => "common",
    }
}

/// 招式 id -> 展示名:裁掉怪物 id 前缀,下划线换空格,首字母大写
fn move_title(corpus_id: &str, move_id: &str) -> String {
    let prefix = format!("{corpus_id}_");
    let rest = move_id.strip_prefix(prefix.as_str()).unwrap_or(move_id);
    rest.split('_')
        .map(|w| title_case(&w.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 本作是否实现了这只怪物:它得出现在某场遭遇的敌人表里,
/// 也就是说真的能打出来(含分裂/召唤出来的和事件直接开战的),只定义不出来的不算
pub fn monster_here_implemented(game_id: &str) -> bool {
    enemies::encounter_with_enemy(game_id).is_some()
}

/// slay-the-cli 参考实现的怪物 id,来自
/// refs/slay-the-cli/src/content/monsters/{act1,act2,act34}/*.ts 里的 `MonsterDef = { id: "..." }`,
/// 共 65 条,与语料一一对应(大小写和语料一致).
pub const SLATE_CLI_MONSTERS: &[&str] = &[
    "ACID_SLIME_L", "ACID_SLIME_M", "ACID_SLIME_S", "AWAKENED_ONE", "BEAR", "BLUE_SLAVER",
    "BOOK_OF_STABBING", "BRONZE_AUTOMATON", "BRONZE_ORB", "BYRD", "CENTURION", "CHOSEN",
    "CORRUPT_HEART", "CULTIST", "DAGGER", "DARKLING", "DECA", "DONU", "EXPLODER", "FAT_GREMLIN",
    "FUNGI_BEAST", "GIANT_HEAD", "GREEN_LOUSE", "GREMLIN_LEADER", "GREMLIN_NOB",
    "GREMLIN_WIZARD", "HEXAGHOST", "JAW_WORM", "LAGAVULIN", "LOOTER", "MAD_GREMLIN", "MUGGER",
    "MYSTIC", "NEMESIS", "ORB_WALKER", "POINTY", "RED_LOUSE", "RED_SLAVER", "REPTOMANCER",
    "REPULSOR", "ROMEO", "SENTRY", "SHELLED_PARASITE", "SHIELD_GREMLIN", "SLIME_BOSS",
    "SNAKE_PLANT", "SNEAKY_GREMLIN", "SNECKO", "SPHERIC_GUARDIAN", "SPIKER", "SPIKE_SLIME_L",
    "SPIKE_SLIME_M", "SPIKE_SLIME_S", "SPIRE_GROWTH", "SPIRE_SHIELD", "SPIRE_SPEAR",
    "TASKMASTER", "THE_CHAMP", "THE_COLLECTOR", "THE_GUARDIAN", "THE_MAW", "TIME_EATER",
    "TORCH_HEAD", "TRANSIENT", "WRITHING_MASS",
];

pub fn slate_cli_monster_implemented(corpus_id: &str) -> bool {
    SLATE_CLI_MONSTERS.binary_search(&corpus_id).is_ok()
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

    /// 语料里三张怪物 json 的条数之和
    const MONSTER_JSON_ROWS: usize = 25 + 20 + 20;

    /// act1 目前能打出来的 18 只,后面的 act 会陆续加,所以只断言这批至少还在
    const ACT1_ENEMIES: [&str; 18] = [
        "cultist",
        "jaw_worm",
        "red_louse",
        "green_louse",
        "acid_slime_small",
        "spike_slime_small",
        "acid_slime_medium",
        "spike_slime_medium",
        "fungi_beast",
        "looter",
        "blue_slaver",
        "red_slaver",
        "gremlin_nob",
        "lagavulin",
        "sentry",
        "the_guardian",
        "hexaghost",
        "slime_boss",
    ];

    #[test]
    fn monster_items_match_corpus() {
        assert_eq!(corpus::MONSTERS.len(), MONSTER_JSON_ROWS);
        let total: usize = groups(Library::Enemies)
            .iter()
            .enumerate()
            .map(|(i, _)| items(Library::Enemies, i).len())
            .sum();
        assert_eq!(total, MONSTER_JSON_ROWS);
        // 名字唯一,页面之间不会互相盖掉
        let mut names: Vec<&str> = Vec::new();
        for m in corpus::MONSTERS {
            assert!(!names.contains(&m.name), "重名 {}", m.name);
            names.push(m.name);
        }
    }

    #[test]
    fn every_act_tab_holds_only_its_own_monsters() {
        let mut seen: Vec<&str> = Vec::new();
        for (act, title, _color) in MONSTER_ACTS {
            let tab = groups(Library::Enemies)
                .iter()
                .position(|g| g.name == title)
                .expect("每个 act 都要有标签页");
            let mut n = 0;
            for it in items(Library::Enemies, tab) {
                let m = corpus::MONSTERS
                    .iter()
                    .find(|m| m.name == it.name)
                    .expect("列表里的怪物不在语料里");
                let first = m.acts.split(',').next().unwrap_or("");
                let ok = if act == "34" {
                    first == "3" || first == "4"
                } else {
                    first == act
                };
                assert!(ok, "{} 不该出现在 {} 页", m.id, title);
                seen.push(m.id);
                n += 1;
            }
            assert!(n > 0, "{} 页是空的", title);
        }
        // 每只怪物只算一次,三页加起来就是全部
        assert_eq!(seen.len(), corpus::MONSTERS.len());
    }

    #[test]
    fn monster_done_flag_matches_encounters() {
        let (done, total) = progress(Library::Enemies);
        assert_eq!(total, corpus::MONSTERS.len());
        let want = corpus::MONSTERS
            .iter()
            .filter(|m| monster_here_implemented(m.id))
            .count();
        assert_eq!(done, want, "已实现计数与遭遇表对不上");
        // 判定要能真打出来:有定义,而且出现在某场遭遇里
        for m in corpus::MONSTERS
            .iter()
            .filter(|m| monster_here_implemented(m.id))
        {
            assert!(enemies::enemy_def(m.id).is_some(), "{} 没有 EnemyDef", m.id);
        }
        // 本作定义的敌人都能对上语料,不然统计会漏
        for def in enemies::ENEMIES {
            assert!(
                enemies::all_encounters().any(|e| e.enemies.contains(&def.id)),
                "{} 不在任何遭遇里",
                def.id
            );
            assert!(
                corpus::MONSTERS.iter().any(|m| m.id == def.id),
                "{} 不在语料里",
                def.id
            );
        }
        // 遇到的敌人也都要有定义
        for enc in enemies::all_encounters() {
            for id in enc.enemies {
                assert!(enemies::enemy_def(id).is_some(), "{} 没有定义", id);
            }
        }
        // 这些 act1 的老敌人必须还能打(新增的遭遇只会让 done 更大)
        for id in ACT1_ENEMIES {
            assert!(monster_here_implemented(id), "{id} 应该还能打");
            assert!(
                corpus::MONSTERS.iter().any(|m| m.id == id),
                "{id} 不在语料里"
            );
        }
        assert!(done >= ACT1_ENEMIES.len(), "done {done} 比 act1 还少");
    }

    #[test]
    fn reference_monster_table_matches_corpus() {
        assert_eq!(SLATE_CLI_MONSTERS.len(), MONSTER_JSON_ROWS);
        assert!(
            SLATE_CLI_MONSTERS.windows(2).all(|w| w[0] < w[1]),
            "常量表要排序好给 binary_search 用"
        );
        for m in corpus::MONSTERS {
            assert!(
                slate_cli_monster_implemented(m.corpus_id),
                "参考实现少了 {}",
                m.corpus_id
            );
        }
        for id in SLATE_CLI_MONSTERS {
            assert!(
                corpus::MONSTERS.iter().any(|m| m.corpus_id == *id),
                "参考实现多了 {id}"
            );
        }
    }

    #[test]
    fn monster_header_note_counts() {
        let (done, total) = progress(Library::Enemies);
        assert_eq!(total, MONSTER_JSON_ROWS);
        assert!(done >= ACT1_ENEMIES.len());
        let n = header_note(Library::Enemies).expect("怪物册要有统计行");
        assert!(n.contains(&format!("implemented here {done}/{total}")), "{n}");
        assert!(n.contains("slay-the-cli 65/65"), "{n}");
    }

    #[test]
    fn monster_detail_lists_moves() {
        let tab = groups(Library::Enemies)
            .iter()
            .position(|g| g.name == "Act 1")
            .unwrap();
        let cultist = items(Library::Enemies, tab)
            .into_iter()
            .find(|i| i.name == "Cultist")
            .expect("Cultist 在 Act 1");
        assert!(cultist.done, "Cultist 本作能打");
        assert!(cultist.sub.contains("normal") && cultist.sub.contains("acts 1,2,3"));
        assert!(cultist.text.contains("hp 48-54"), "{}", cultist.text);
        assert!(cultist.text.contains("asc 50-56"), "{}", cultist.text);
        assert!(cultist.text.contains("\n- Incantation"), "{}", cultist.text);
        assert!(cultist.text.contains("\n- Dark Strike"), "{}", cultist.text);
        // 语料里的 boss 血量是单值,只写一个数
        let boss = items(Library::Enemies, tab)
            .into_iter()
            .find(|i| i.name == "The Guardian")
            .expect("The Guardian 在 Act 1");
        assert!(boss.text.contains("hp 240\n"), "{}", boss.text);
        assert!(boss.text.contains("asc 250\n"), "{}", boss.text);
    }
}
