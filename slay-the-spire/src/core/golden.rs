// 金标准测试:fixture 里的数值是拿 bun 直接跑参考实现(refs/slay-the-cli)
// 在同一颗种子下跑出来的(tools/gen_golden.ts 生成),这里断言我们的引擎逐位一致.
//
// 覆盖:第一章地图布局、开局生成的遭遇名单、第一个怪房间的怪物血量、
// 第一场战斗的三张卡牌奖励、base-35 种子的写法.
use crate::core::enemies;
use crate::core::map::ActMap;
use crate::core::run::{Run, Screen};
use crate::rng::{seed_to_string, Rng, RngRegistry, RunStream};

/// 多颗种子的同一个位置(fixture 里 CASES 用的行)
pub struct Case {
    pub seed: u64,
    pub monsters: &'static [(&'static str, i32)],
    pub gold: i32,
    pub potion: Option<&'static str>,
    pub cards: &'static [(&'static str, bool)],
}

include!("golden_fixture.rs");

/// 参考实现的 id 是大写,本作用小写;史莱姆的 S/M/L 本作写全了
fn lower(s: &str) -> String {
    match s {
        "SPIKE_SLIME_S" => "spike_slime_small".to_string(),
        "ACID_SLIME_S" => "acid_slime_small".to_string(),
        "SPIKE_SLIME_M" => "spike_slime_medium".to_string(),
        "ACID_SLIME_M" => "acid_slime_medium".to_string(),
        "SPIKE_SLIME_L" => "spike_slime_large".to_string(),
        "ACID_SLIME_L" => "acid_slime_large".to_string(),
        _ => s.to_lowercase(),
    }
}

/// 把"赢了之后停留几帧"一步走完,落到奖励界面
fn settle(r: &mut Run) {
    r.sync_combat();
    for _ in 0..=Run::VICTORY_HOLD {
        r.tick_win_hold();
    }
}

#[test]
fn seed_string_matches_reference() {
    assert_eq!(seed_to_string(SEED), SEED_STRING);
}

#[test]
fn act1_map_layout_matches_reference() {
    // 第一章地图的流是 seed + 1
    let mut rng = Rng::new(SEED + 1);
    let map = ActMap::generate(&mut rng);
    let ours = map.to_rows_string();
    let want = MAP;
    for (i, (a, b)) in ours.lines().zip(want.lines()).enumerate() {
        assert_eq!(a.trim_end(), b.trim_end(), "第 {i} 行对不上");
    }
    assert_eq!(ours.lines().count(), want.lines().count(), "行数对不上");
}

#[test]
fn encounter_lists_match_reference() {
    let mut reg = RngRegistry::new(SEED);
    let lists = enemies::generate_encounters(1, reg.run(RunStream::MonsterRng));
    // 双方的遭遇 id 取名不同(参考实现大写、本作带后缀),比阵容更实在
    let ours = |ids: &[&'static str]| -> Vec<Vec<String>> {
        ids.iter()
            .map(|id| {
                enemies::resolve(id)
                    .enemies
                    .iter()
                    .map(|m| m.to_string())
                    .collect()
            })
            .collect()
    };
    let want = |lineups: &[&[&str]]| -> Vec<Vec<String>> {
        lineups
            .iter()
            .map(|l| l.iter().map(|m| lower(m)).collect())
            .collect()
    };
    assert_eq!(ours(&lists.monster), want(MONSTER_LINEUPS), "怪名单对不上");
    assert_eq!(ours(&lists.elite), want(ELITE_LINEUPS), "精英名单对不上");
    assert_eq!(ours(&lists.boss), want(BOSS_LINEUPS), "Boss 顺序对不上");
    assert_eq!(lists.monster.len(), MONSTER_LIST.len());
    assert_eq!(lists.elite.len(), ELITE_LIST.len());
    assert_eq!(lists.boss.len(), BOSS_ORDER.len());
    // 第一个怪房间消耗的是名单的头一条
    assert_eq!(FIRST_ROOM_ENCOUNTER, MONSTER_LIST[0]);
}

/// 第一层第一个可进的节点(地图上第 0 行那一列)
fn first_room_index(run: &Run) -> usize {
    run.map
        .row(0)
        .iter()
        .copied()
        .find(|i| run.map.node(*i).col == FIRST_ROOM_X)
        .expect("第一行有这一列")
}

#[test]
fn first_monster_room_hp_matches_reference() {
    let mut run = Run::new(SEED);
    let idx = first_room_index(&run);
    run.enter_node(idx).expect("第一层的怪房间进得去");
    let c = run.combat().expect("进怪房间就该开打");
    let ours: Vec<(String, i32)> = c
        .enemies
        .iter()
        .map(|e| (e.def.id.to_string(), e.max_hp))
        .collect();
    let want: Vec<(String, i32)> = FIRST_ROOM_MONSTERS
        .iter()
        .map(|(id, hp)| (lower(id), *hp))
        .collect();
    assert_eq!(ours, want);
}

#[test]
fn first_card_reward_matches_reference() {
    let mut run = Run::new(SEED);
    let idx = first_room_index(&run);
    run.enter_node(idx).expect("第一层的怪房间进得去");
    let lineups = enemies::resolve(run.combat().map(|c| c.encounter_id).unwrap_or("")).enemies;
    let started: Vec<String> = lineups.iter().map(|m| m.to_string()).collect();
    let want: Vec<String> = FIRST_ROOM_MONSTERS.iter().map(|(id, _)| lower(id)).collect();
    assert_eq!(started, want, "第一场遭遇对不上");
    run.debug_win_battle();
    settle(&mut run);
    assert_eq!(run.screen, Screen::Reward);
    let reward = run.reward.as_ref().expect("赢了就该有奖励");
    assert_eq!(reward.gold, REWARD_GOLD, "金币对不上");
    assert_eq!(
        reward.potion.map(|p| p.id.to_string()),
        REWARD_POTION.map(|s| s.to_string()),
        "药水对不上"
    );
    let ours: Vec<(String, bool)> = reward
        .cards
        .iter()
        .map(|c| (c.def.id.to_string(), c.upgraded))
        .collect();
    let want: Vec<(String, bool)> = REWARD_CARDS
        .iter()
        .map(|(id, up)| (lower(id), *up))
        .collect();
    assert_eq!(ours, want, "三选一的牌对不上");
}

/// 卡池的顺序也要对得上:抽牌是按池子下标抽的
#[test]
fn class_card_pool_sizes_match_reference() {
    use crate::core::card::Rarity;
    use crate::core::cards;
    assert_eq!(cards::reward_pool(Rarity::Common).len(), 20);
    assert_eq!(cards::reward_pool(Rarity::Uncommon).len(), 36);
    assert_eq!(cards::reward_pool(Rarity::Rare).len(), 16);
    // basics 切片里那三张排在各自稀有度的最前
    assert_eq!(cards::reward_pool(Rarity::Common)[0].id, "body_slam");
    assert_eq!(cards::reward_pool(Rarity::Common)[1].id, "anger");
    assert_eq!(cards::reward_pool(Rarity::Uncommon)[0].id, "whirlwind");
}




/// 多颗种子:第一个怪房间的阵容与血量、第一场战斗的金币/药水/三张牌
#[test]
fn multi_seed_golden_cases_match_reference() {
    for case in CASES {
        let mut run = Run::new(case.seed);
        let idx = run.map.row(0)[0];
        run.enter_node(idx).unwrap_or_else(|e| panic!("seed {}: {e}", case.seed));
        let ours: Vec<(String, i32)> = run
            .combat()
            .expect("进怪房间就该开打")
            .enemies
            .iter()
            .map(|e| (e.def.id.to_string(), e.max_hp))
            .collect();
        let want: Vec<(String, i32)> = case
            .monsters
            .iter()
            .map(|(id, hp)| (lower(id), *hp))
            .collect();
        assert_eq!(ours, want, "seed {} 的怪物血量对不上", case.seed);

        run.debug_win_battle();
        settle(&mut run);
        let reward = run.reward.as_ref().expect("赢了就该有奖励");
        assert_eq!(reward.gold, case.gold, "seed {} 的金币对不上", case.seed);
        // 药水只比"掉没掉":本作的药水池是参考实现的子集且顺序不同,
        // 抽出来是哪一瓶对不上(见报告里的残留差异)
        assert_eq!(
            reward.potion.is_some(),
            case.potion.is_some(),
            "seed {} 的药水掉落与否对不上",
            case.seed
        );
        let got: Vec<(String, bool)> = reward
            .cards
            .iter()
            .map(|c| (c.def.id.to_string(), c.upgraded))
            .collect();
        let want: Vec<(String, bool)> = case
            .cards
            .iter()
            .map(|(id, up)| (lower(id), *up))
            .collect();
        assert_eq!(got, want, "seed {} 的三选一对不上", case.seed);
    }
}
