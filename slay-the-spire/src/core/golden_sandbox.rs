//! 沙盒差分尺(tools/sandbox_diff.ts + src/core/replay.rs 的 sandbox 模块)扫出来的
//! (a) 类差异固化成的回归测试。
//!
//! 每一条对应用户可感知的一处不一致:参考实现(以及原版)是那样,本作曾经是另一样。
//! 场景都是"手牌/抽牌堆摆死 + 一只怪"的最小局面,不依赖随机。

use crate::core::card::{CardInstance};
use crate::core::combat::{Combat, CombatSetup, RunRelicCounters};
use crate::core::status::Status;
use crate::core::{cards, enemies, potions};
use crate::rng::{FloorStream, RngRegistry};

/// 一场可摆布的战斗:手牌与抽牌堆按参数摆好,能量 9,一只颚虫
fn staged(deck: &[&str], hand: &[&str]) -> Combat {
    let setup = CombatSetup {
        rested: false,
        hp: 80,
        max_hp: 80,
        deck: deck.iter().map(|id| cards::card(id)).collect(),
        relics: Vec::new(),
        gold: 0,
        lift_strength: 0,
        relic_counters: RunRelicCounters::default(),
        curse_negate: 0,
    asc: 0,
    };
    let enc = enemies::encounter_def("jaw_worm_solo").expect("颚虫遭遇要在表里");
    let mut c = Combat::new(enc, setup, RngRegistry::new(21));
    c.hand.clear();
    c.draw.clear();
    c.discard.clear();
    c.exhaust.clear();
    for id in hand {
        c.hand.push(cards::card(id));
    }
    for id in deck {
        c.draw.push(cards::card(id));
    }
    c.energy = 9;
    c.enemies[0].hp = 200;
    c.enemies[0].max_hp = 200;
    c
}

fn hand_idx(c: &Combat, id: &str) -> usize {
    c.hand
        .iter()
        .position(|x| x.def.id == id)
        .unwrap_or_else(|| panic!("手里没有 {id}"))
}

/// 牌堆记号:id / id+ / id+N
fn tokens(pile: &[CardInstance]) -> Vec<String> {
    pile.iter()
        .map(|c| {
            if c.plus > 1 {
                format!("{}+{}", c.def.id, c.plus)
            } else if c.upgraded {
                format!("{}+", c.def.id)
            } else {
                c.def.id.to_string()
            }
        })
        .collect()
}

fn upgraded(c: &mut Combat, id: &str) {
    let i = hand_idx(c, id);
    assert!(c.hand[i].upgrade(), "{id} 应该能升级");
}

/// 愤怒:塞进弃牌堆的副本要照抄升级数(原版 Anger+ 塞 Anger+)
#[test]
fn anger_copy_keeps_the_upgrade() {
    let mut c = staged(&["anger"], &["anger"]);
    upgraded(&mut c, "anger");
    c.play_card(0, Some(0)).unwrap();
    let copies = tokens(&c.discard);
    assert_eq!(copies.iter().filter(|t| t.starts_with("anger")).count(), 2);
    assert!(
        copies.iter().all(|t| t == "anger+"),
        "打出去的与复制回来的都该是 anger+,实际 {copies:?}"
    );
}

/// 重刃:升级是"力量多吃两次"(14 伤害 ×5),不是把基础伤害抬到 18
#[test]
fn heavy_blade_upgrade_takes_strength_five_times() {
    let mut c = staged(&["heavy_blade"], &["heavy_blade"]);
    upgraded(&mut c, "heavy_blade");
    c.player.statuses.add(Status::Strength, 3);
    c.play_card(0, Some(0)).unwrap();
    // 14 + 3×5 = 29
    assert_eq!(c.enemies[0].hp, 200 - 29);
}

/// 献祭:升级后是"抽 5 张",掉血仍是 6 点
#[test]
fn offering_upgrade_draws_five_and_loses_six() {
    let mut c = staged(
        &["strike", "strike", "strike", "strike", "strike"],
        &["offering"],
    );
    upgraded(&mut c, "offering");
    c.play_card(0, None).unwrap();
    assert_eq!(c.player.hp, 80 - 6);
    assert_eq!(c.hand.len(), 5, "应该抽满 5 张");
    assert!(c.draw.is_empty());
}

/// 突破极限:升级不再消耗、费用仍是 1
#[test]
fn limit_break_upgrade_keeps_cost_and_stops_exhausting() {
    let mut c = staged(&["limit_break"], &["limit_break"]);
    upgraded(&mut c, "limit_break");
    c.player.statuses.add(Status::Strength, 4);
    c.play_card(0, None).unwrap();
    assert_eq!(c.player.statuses.get(Status::Strength), 8, "力量翻倍");
    assert_eq!(c.energy, 9 - 1, "升级版费用仍是 1");
    assert!(c.exhaust.is_empty(), "升级后不再消耗");
    assert_eq!(tokens(&c.discard), vec!["limit_break+".to_string()]);
}

/// 暴戾:升级只加 Innate,回合开始掉 1 血抽 1 张(不是抽 2)
#[test]
fn brutality_upgrade_is_innate_and_still_draws_one() {
    let strikes = ["strike"; 10];
    let mut c = staged(&strikes, &["brutality"]);
    upgraded(&mut c, "brutality");
    assert!(c.hand[0].is_innate(), "暴戾+ 该是 Innate");
    c.play_card(0, None).unwrap();
    assert_eq!(
        c.player.statuses.get(Status::Brutality),
        1,
        "暴戾+ 层数仍是 1(升级只加 Innate)"
    );
    // 挡满这一轮,免得混进敌人的伤害
    c.player.block = 999;
    let hp_before = c.player.hp;
    c.end_turn();
    assert_eq!(c.player.hp, hp_before - 1, "回合开始掉 1 血(不是 2)");
    assert_eq!(c.hand.len(), 5 + 1, "正常抽 5 张,暴戾再补 1 张");
}

/// 双发只在打出的那一回合有效:升级给 2 层也一样,回合末整条消失
#[test]
fn double_tap_expires_at_end_of_turn() {
    let mut c = staged(&["defend"], &["double_tap"]);
    upgraded(&mut c, "double_tap");
    c.play_card(0, None).unwrap();
    assert_eq!(c.player.statuses.get(Status::DoubleTap), 2);
    c.end_turn();
    assert_eq!(c.player.statuses.get(Status::DoubleTap), 0, "回合末该整条移除");
}

/// 复制药水同样是"这回合",回合末整条消失
#[test]
fn duplication_expires_at_end_of_turn() {
    let mut c = staged(&["defend"], &["defend"]);
    let def = potions::POTIONS
        .iter()
        .find(|p| p.id == "duplication_potion")
        .expect("复制药水");
    c.use_potion(def, None);
    assert!(c.player.statuses.get(Status::Duplication) > 0);
    c.end_turn();
    assert_eq!(c.player.statuses.get(Status::Duplication), 0, "回合末该整条移除");
}

/// 满手时新造的牌进弃牌堆,而不是凭空消失(力劈华山塞的两张伤口)
#[test]
fn created_cards_overflow_into_the_discard_pile() {
    let mut c = staged(&[], &[]);
    c.hand.push(cards::card("power_through"));
    for _ in 0..9 {
        c.hand.push(cards::card("strike"));
    }
    assert_eq!(c.hand.len(), 10);
    c.play_card(0, None).unwrap();
    assert_eq!(c.hand.len(), 10, "打掉 1 张 + 塞回 1 张伤口,手牌仍是 10");
    let disc = tokens(&c.discard);
    assert_eq!(disc.iter().filter(|t| *t == "wound").count(), 1);
    assert_eq!(disc.iter().filter(|t| *t == "power_through").count(), 1);
    assert_eq!(c.player.block, 15);
}

/// 过量伤害把敌人血量夹在 0,不会留下负数
#[test]
fn overkill_damage_leaves_the_enemy_at_zero() {
    let mut c = staged(&["strike"], &["strike"]);
    c.enemies[0].hp = 4;
    c.enemies[0].max_hp = 4;
    c.play_card(0, Some(0)).unwrap();
    assert_eq!(c.enemies[0].hp, 0);
}

/// 黑暗镣铐:力量可以压成负数(原版就是 -9),回合末原样补回来
#[test]
fn dark_shackles_pushes_strength_negative() {
    let mut c = staged(&["dark_shackles"], &["dark_shackles"]);
    c.play_card(0, Some(0)).unwrap();
    assert_eq!(c.enemies[0].statuses.get(Status::Strength), -9);
    c.end_turn();
    assert_eq!(c.enemies[0].statuses.get(Status::Strength), 0, "回合末补回来");
}

/// 深呼吸:哪怕弃牌堆是空的,也要消耗一次洗牌掷点(否则之后所有随机错位)
#[test]
fn deep_breath_burns_a_shuffle_roll_even_with_an_empty_discard() {
    let mut c = staged(&[], &["deep_breath"]);
    let mut before = c.streams.clone();
    c.play_card(0, None).unwrap();
    assert!(c.discard.is_empty() || c.discard.len() == 1, "弃牌堆里只有打出的那张");
    let untouched = before.floor(FloorStream::ShuffleRng).random_long();
    let actual = c.streams.floor(FloorStream::ShuffleRng).random_long();
    assert_ne!(actual, untouched, "空弃牌堆也必须消耗一次洗牌掷点");
}

/// 浩劫:抽牌堆空了先把弃牌堆洗回来再打(参考实现会洗)
#[test]
fn havoc_reshuffles_an_empty_draw_pile() {
    let mut c = staged(&[], &["havoc"]);
    c.discard.push(cards::card("strike"));
    c.play_card(0, None).unwrap();
    assert_eq!(c.enemies[0].hp, 200 - 6, "洗回来的那张打击该被打出去");
    assert_eq!(c.discard.len(), 1, "打出去的浩劫进弃牌堆");
}

/// 沙盒工具本身:一份 scenario 能跑出逐步状态(与参考侧同 schema)
#[test]
fn sandbox_scenario_runs_and_reports_state() {
    let scenario = r#"{
        "player": {"hp": 80, "max_hp": 80, "energy": 9, "max_energy": 9},
        "hand": ["strike", "defend"],
        "draw": ["strike"],
        "enemies": [{"id": "cultist", "hp": 50, "max_hp": 50, "move": "Incantation"}],
        "actions": [{"op": "play", "hand": 0, "target": 0}]
    }"#;
    let text = super::replay::sandbox::run(12345, scenario).expect("scenario 该跑得动");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "init + 一步");
    assert!(lines[0].contains(r#""step":0,"op":"init""#));
    assert!(lines[1].contains(r#""op":"play""#));
    assert!(lines[1].contains(r#""hp":44"#), "50 - 6 伤害");
    assert!(lines[1].contains(r#""energy":8"#));
    assert!(lines[1].contains(r#""move":"incantation""#));
}
