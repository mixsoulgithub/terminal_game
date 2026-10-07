// 敌人与遭遇的数据:按 act 分成几个子模块,这里汇总成一张总表.
// 每个敌人一张招式表,AI 决定怎么从招式表里挑(选招规则写在各自的 act 模块里).
pub mod act1;
pub mod act2;
pub mod act34;

use crate::core::enemy::{Encounter, EnemyDef, EnemyKind, EnemyPreset};
use crate::core::status::Status;
use crate::rng::Rng;

/// 本作实现的全部怪物(按 act1 / act2 / act3+4 的顺序)
pub static ENEMIES: &[EnemyDef] = &[
    act1::CULTIST,
    act1::JAW_WORM,
    act1::RED_LOUSE,
    act1::GREEN_LOUSE,
    act1::ACID_SLIME_SMALL,
    act1::ACID_SLIME_MEDIUM,
    act1::ACID_SLIME_LARGE,
    act1::SPIKE_SLIME_SMALL,
    act1::SPIKE_SLIME_MEDIUM,
    act1::SPIKE_SLIME_LARGE,
    act1::MAD_GREMLIN,
    act1::SNEAKY_GREMLIN,
    act1::FAT_GREMLIN,
    act1::SHIELD_GREMLIN,
    act1::GREMLIN_WIZARD,
    act1::LOOTER,
    act1::FUNGI_BEAST,
    act1::BLUE_SLAVER,
    act1::RED_SLAVER,
    act1::GREMLIN_NOB,
    act1::LAGAVULIN,
    act1::SENTRY,
    act1::SLIME_BOSS,
    act1::THE_GUARDIAN,
    act1::HEXAGHOST,
    act2::SPHERIC_GUARDIAN,
    act2::CHOSEN,
    act2::SHELLED_PARASITE,
    act2::BYRD,
    act2::MUGGER,
    act2::CENTURION,
    act2::MYSTIC,
    act2::SNAKE_PLANT,
    act2::SNECKO,
    act2::BOOK_OF_STABBING,
    act2::GREMLIN_LEADER,
    act2::TASKMASTER,
    act2::BRONZE_AUTOMATON,
    act2::BRONZE_ORB,
    act2::THE_COLLECTOR,
    act2::TORCH_HEAD,
    act2::THE_CHAMP,
    act2::BEAR,
    act2::ROMEO,
    act2::POINTY,
    act34::DARKLING,
    act34::ORB_WALKER,
    act34::SPIKER,
    act34::REPULSOR,
    act34::EXPLODER,
    act34::TRANSIENT,
    act34::THE_MAW,
    act34::SPIRE_GROWTH,
    act34::WRITHING_MASS,
    act34::GIANT_HEAD,
    act34::NEMESIS,
    act34::REPTOMANCER,
    act34::DAGGER,
    act34::AWAKENED_ONE,
    act34::TIME_EATER,
    act34::DONU,
    act34::DECA,
    act34::SPIRE_SHIELD,
    act34::SPIRE_SPEAR,
    act34::CORRUPT_HEART,
];

/// 前三层用的弱遭遇
pub static ENCOUNTERS_WEAK: &[Encounter] = &[
    Encounter {
        id: "cultist_solo",
        kind: EnemyKind::Normal,
        enemies: &["cultist"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "jaw_worm_solo",
        kind: EnemyKind::Normal,
        enemies: &["jaw_worm"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "two_louses",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "green_louse"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "small_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_small", "acid_slime_medium"],
        ..Encounter::PLAIN
    },
];

pub static ENCOUNTERS: &[Encounter] = &[
    Encounter {
        id: "gremlin_gang",
        kind: EnemyKind::Normal,
        enemies: &[
            "mad_gremlin",
            "sneaky_gremlin",
            "fat_gremlin",
            "shield_gremlin",
        ],
        weight: 2,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "gremlin_gang_alt",
        kind: EnemyKind::Normal,
        enemies: &[
            "mad_gremlin",
            "sneaky_gremlin",
            "shield_gremlin",
            "gremlin_wizard",
        ],
        weight: 0,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "lots_of_slimes",
        kind: EnemyKind::Normal,
        enemies: &[
            "spike_slime_small",
            "spike_slime_small",
            "spike_slime_small",
            "acid_slime_small",
            "acid_slime_small",
        ],
        weight: 2,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "red_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["red_slaver"],
        weight: 2,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "exordium_thugs",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "blue_slaver"],
        weight: 3,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "exordium_wildlife",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast", "jaw_worm"],
        weight: 3,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "blue_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["blue_slaver"],
        weight: 4,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "looter_solo",
        kind: EnemyKind::Normal,
        enemies: &["looter"],
        weight: 4,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "large_slime",
        kind: EnemyKind::Normal,
        enemies: &["acid_slime_large"],
        weight: 4,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_louses",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "green_louse", "red_louse"],
        weight: 4,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "two_fungi_beasts",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast", "fungi_beast"],
        weight: 4,
        ..Encounter::PLAIN
    },
];

pub static ELITES: &[Encounter] = &[
    Encounter {
        id: "gremlin_nob_solo",
        kind: EnemyKind::Elite,
        enemies: &["gremlin_nob"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "lagavulin_solo",
        kind: EnemyKind::Elite,
        enemies: &["lagavulin"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_sentries",
        kind: EnemyKind::Elite,
        enemies: &["sentry", "sentry", "sentry"],
        presets: THREE_SENTRIES_PRESETS,
        ..Encounter::PLAIN
    },
];

/// 三哨兵遭遇里的哨兵开局就算"已经行动过一回合":
/// 参考实现给它们预置了一段招式历史,于是 firstTurn 为假,首招由严格的
/// 螺栓/射线交替决定.外两只的上一招是射线(所以先放螺栓),中间那只的上一招
/// 是螺栓(所以先射线),与按站位定的相位一致.
const THREE_SENTRIES_PRESETS: &[EnemyPreset] = &[
    // 外两侧的上一招是射线,于是首招是螺栓
    EnemyPreset::acted(&[0, 2], 1, Some("Beam")),
    // 中间那只的上一招是螺栓,于是首招是射线
    EnemyPreset::acted(&[1], 1, Some("Bolt")),
];

pub static BOSSES: &[Encounter] = &[
    Encounter {
        id: "the_guardian",
        kind: EnemyKind::Boss,
        enemies: &["the_guardian"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "hexaghost",
        kind: EnemyKind::Boss,
        enemies: &["hexaghost"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "slime_boss",
        kind: EnemyKind::Boss,
        enemies: &["slime_boss"],
        ..Encounter::PLAIN
    },
];

/// 第二章的弱怪池
pub static ACT2_WEAK: &[Encounter] = &[
    Encounter {
        id: "spheric_guardian_solo",
        kind: EnemyKind::Normal,
        enemies: &["spheric_guardian"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "chosen_solo",
        kind: EnemyKind::Normal,
        enemies: &["chosen"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "shelled_parasite_solo",
        kind: EnemyKind::Normal,
        enemies: &["shelled_parasite"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_byrds",
        kind: EnemyKind::Normal,
        enemies: &["byrd", "byrd", "byrd"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "two_thieves",
        kind: EnemyKind::Normal,
        enemies: &["looter", "mugger"],
        ..Encounter::PLAIN
    },
];

/// 第二章的普通遭遇
pub static ACT2: &[Encounter] = &[
    Encounter {
        id: "chosen_and_byrds",
        kind: EnemyKind::Normal,
        enemies: &["byrd", "chosen"],
        weight: 2,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "sentry_and_sphere",
        kind: EnemyKind::Normal,
        enemies: &["sentry", "spheric_guardian"],
        weight: 2,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "cultist_and_chosen",
        kind: EnemyKind::Normal,
        enemies: &["cultist", "chosen"],
        weight: 3,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_cultists",
        kind: EnemyKind::Normal,
        enemies: &["cultist", "cultist", "cultist"],
        weight: 3,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "shelled_parasite_and_fungi",
        kind: EnemyKind::Normal,
        enemies: &["shelled_parasite", "fungi_beast"],
        weight: 3,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "snecko_solo",
        kind: EnemyKind::Normal,
        enemies: &["snecko"],
        weight: 4,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "snake_plant_solo",
        kind: EnemyKind::Normal,
        enemies: &["snake_plant"],
        weight: 6,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "centurion_and_healer",
        kind: EnemyKind::Normal,
        enemies: &["centurion", "mystic"],
        weight: 6,
        ..Encounter::PLAIN
    },
];

/// 小鬼头目开局带的两只小鬼是它的随从(参考实现里由头目的开战动作挂上 MINION):
/// 头目一倒它们就跟着退场,场上只剩随从时这一场也算结束.
const GREMLIN_LEADER_PRESETS: &[EnemyPreset] =
    &[EnemyPreset::buffed(&[0, 1], &[(Status::Minion, 1)])];

pub static ACT2_ELITES: &[Encounter] = &[
    Encounter {
        id: "gremlin_leader_gang",
        kind: EnemyKind::Elite,
        enemies: &["mad_gremlin", "sneaky_gremlin", "gremlin_leader"],
        presets: GREMLIN_LEADER_PRESETS,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "slavers",
        kind: EnemyKind::Elite,
        enemies: &["blue_slaver", "taskmaster", "red_slaver"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "book_of_stabbing_solo",
        kind: EnemyKind::Elite,
        enemies: &["book_of_stabbing"],
        ..Encounter::PLAIN
    },
];

pub static ACT2_BOSSES: &[Encounter] = &[
    Encounter {
        id: "bronze_automaton",
        kind: EnemyKind::Boss,
        enemies: &["bronze_automaton"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "the_collector",
        kind: EnemyKind::Boss,
        enemies: &["the_collector"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "the_champ",
        kind: EnemyKind::Boss,
        enemies: &["the_champ"],
        ..Encounter::PLAIN
    },
];

/// 三种"形状"遭遇的抽签池:参考实现从这 6 个里不放回地抽(每种最多两只)
const SHAPE_POOL: [&str; 6] = [
    "repulsor",
    "repulsor",
    "exploder",
    "exploder",
    "spiker",
    "spiker",
];

/// 不放回地抽 n 只形状:每抽一只就把它从池子里删掉
/// (参考实现里就是 miscRng.random(lastIdx),lastIdx 依次是 5、4、3、2)
fn draw_shapes(rng: &mut Rng, n: usize) -> Vec<&'static str> {
    let mut pool = SHAPE_POOL.to_vec();
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let i = rng.range_inclusive(0, pool.len() as i32 - 1) as usize;
        out.push(pool.remove(i));
    }
    out
}

fn three_shapes_lineup(rng: &mut Rng) -> Vec<&'static str> {
    draw_shapes(rng, 3)
}

fn four_shapes_lineup(rng: &mut Rng) -> Vec<&'static str> {
    draw_shapes(rng, 4)
}

/// 两只形状放回地抽(池子只有三种),球体守卫固定排在最后
fn sphere_and_two_shapes_lineup(rng: &mut Rng) -> Vec<&'static str> {
    const POOL: [&str; 3] = ["spiker", "repulsor", "exploder"];
    let mut out = Vec::with_capacity(3);
    for _ in 0..2 {
        out.push(POOL[rng.range_inclusive(0, 2) as usize]);
    }
    out.push("spheric_guardian");
    out
}

/// 颚虫三连里的每只颚虫:开局带力量 3、格挡 5,并且算"已经行动过一回合".
/// 参考实现把它的招式历史预置成一个匹配不到任何招式的哨兵值,于是第一回合那种
/// "必定咬一口"的开局被跳过,从第一回合起就走 25/30/45 的常规分布.
const JAW_WORM_HORDE_PRESETS: &[EnemyPreset] = &[EnemyPreset {
    slots: &[0, 1, 2],
    statuses: &[(Status::Strength, 3)],
    block: 5,
    acted_turns: 1,
    last_move: None,
}];

/// 第三章的弱怪池。三只"形状"开局按参考规则从池子里抽(见 draw_shapes)。
pub static ACT3_WEAK: &[Encounter] = &[
    Encounter {
        id: "three_darklings_weak",
        kind: EnemyKind::Normal,
        enemies: &["darkling", "darkling", "darkling"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "orb_walker_solo",
        kind: EnemyKind::Normal,
        enemies: &["orb_walker"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_shapes",
        kind: EnemyKind::Normal,
        enemies: &["spiker", "repulsor", "exploder"],
        lineup: Some(three_shapes_lineup),
        ..Encounter::PLAIN
    },
];

pub static ACT3: &[Encounter] = &[
    Encounter {
        id: "spire_growth_solo",
        kind: EnemyKind::Normal,
        enemies: &["spire_growth"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "transient_solo",
        kind: EnemyKind::Normal,
        enemies: &["transient"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "four_shapes",
        kind: EnemyKind::Normal,
        enemies: &["repulsor", "exploder", "spiker", "repulsor"],
        lineup: Some(four_shapes_lineup),
        ..Encounter::PLAIN
    },
    Encounter {
        id: "the_maw_solo",
        kind: EnemyKind::Normal,
        enemies: &["the_maw"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "sphere_and_two_shapes",
        kind: EnemyKind::Normal,
        enemies: &["spiker", "repulsor", "spheric_guardian"],
        lineup: Some(sphere_and_two_shapes_lineup),
        ..Encounter::PLAIN
    },
    Encounter {
        id: "jaw_worm_horde",
        kind: EnemyKind::Normal,
        enemies: &["jaw_worm", "jaw_worm", "jaw_worm"],
        presets: JAW_WORM_HORDE_PRESETS,
        ..Encounter::PLAIN
    },
    Encounter {
        id: "three_darklings",
        kind: EnemyKind::Normal,
        enemies: &["darkling", "darkling", "darkling"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "writhing_mass_solo",
        kind: EnemyKind::Normal,
        enemies: &["writhing_mass"],
        ..Encounter::PLAIN
    },
];

pub static ACT3_ELITES: &[Encounter] = &[
    Encounter {
        id: "giant_head_solo",
        kind: EnemyKind::Elite,
        enemies: &["giant_head"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "nemesis_solo",
        kind: EnemyKind::Elite,
        enemies: &["nemesis"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "reptomancer_solo",
        kind: EnemyKind::Elite,
        enemies: &["dagger", "reptomancer", "dagger"],
        ..Encounter::PLAIN
    },
];

pub static ACT3_BOSSES: &[Encounter] = &[
    Encounter {
        id: "awakened_one",
        kind: EnemyKind::Boss,
        enemies: &["cultist", "cultist", "awakened_one"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "time_eater",
        kind: EnemyKind::Boss,
        enemies: &["time_eater"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "donu_and_deca",
        kind: EnemyKind::Boss,
        enemies: &["deca", "donu"],
        ..Encounter::PLAIN
    },
];

/// 第四章:一对精英和心脏
pub static ACT4_ELITES: &[Encounter] = &[Encounter {
    id: "shield_and_spear",
    kind: EnemyKind::Elite,
    enemies: &["spire_shield", "spire_spear"],
    // 开局玩家就被包围,初始朝向是右边的长矛(slot 1):盾的攻击吃 1.5 倍
    player_statuses: &[(Status::Surrounded, 1)],
    ..Encounter::PLAIN
}];

pub static ACT4_BOSSES: &[Encounter] = &[Encounter {
    id: "the_heart",
    kind: EnemyKind::Boss,
    enemies: &["corrupt_heart"],
    ..Encounter::PLAIN
}];

/// 只会从分裂里出来的怪(大史莱姆裂开时生成),单列一张表便于直接打到
pub static SPLIT_ONLY: &[Encounter] = &[
    Encounter {
        id: "medium_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_medium", "acid_slime_medium"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "boss_split_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_large", "acid_slime_large"],
        ..Encounter::PLAIN
    },
];

/// 只会被召唤出来的小怪:单列一张表,调试可以直接打,不进地图池子
pub static MINIONS: &[Encounter] = &[
    Encounter {
        id: "bronze_orbs",
        kind: EnemyKind::Normal,
        enemies: &["bronze_orb", "bronze_orb"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "torch_heads",
        kind: EnemyKind::Normal,
        enemies: &["torch_head", "torch_head"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "daggers",
        kind: EnemyKind::Normal,
        enemies: &["dagger", "dagger", "dagger"],
        ..Encounter::PLAIN
    },
];

/// 开局的槽位.参考实现里每只怪站在一个固定槽位上,有几场遭遇是刻意留空槽的:
/// 小鬼头目的首领在 3、两只小鬼在 1 和 2、槽 0 空着;自动机在 1(铜球占 0 和 2);
/// 收集者在 2(火炬头占 0 和 1);爬行者在 2(小刀占 1 和 4).
/// 每项是"该遭遇里第 i 只怪的槽位",没列到的遭遇就按 0,1,2... 密集排.
pub static ENCOUNTER_SLOTS: &[(&str, &[usize])] = &[
    ("gremlin_leader_gang", &[1, 2, 3]),
    ("bronze_automaton", &[1]),
    ("the_collector", &[2]),
    ("reptomancer_solo", &[1, 2, 4]),
];

/// 遭遇里第 i 只怪开局的槽位
pub fn initial_slot(enc: &Encounter, i: usize) -> usize {
    match ENCOUNTER_SLOTS.iter().find(|(id, _)| *id == enc.id) {
        Some((_, slots)) => slots.get(i).copied().unwrap_or(i),
        None => i,
    }
}

/// 所有遭遇表(图鉴与查找用)
pub static ENCOUNTER_TABLES: &[&[Encounter]] = &[
    ENCOUNTERS_WEAK,
    ENCOUNTERS,
    ELITES,
    BOSSES,
    ACT2_WEAK,
    ACT2,
    ACT2_ELITES,
    ACT2_BOSSES,
    ACT3_WEAK,
    ACT3,
    ACT3_ELITES,
    ACT3_BOSSES,
    ACT4_ELITES,
    ACT4_BOSSES,
    SPLIT_ONLY,
    MINIONS,
    crate::core::enemy::EVENT_ENCOUNTERS,
];

/// 全部遭遇(地图上的各层池子 + 事件直接开战的)
pub fn all_encounters() -> impl Iterator<Item = &'static Encounter> {
    ENCOUNTER_TABLES.iter().flat_map(|t| t.iter())
}

/// 遭遇的显示名:只有一只敌人的遭遇就用那只敌人的名字(地图上的 Boss 用得到)
pub fn encounter_name(enc: &Encounter) -> &'static str {
    match enc.enemies {
        [only] => enemy_def_or_panic(only).name,
        _ => enc.id,
    }
}

pub fn enemy_def(id: &str) -> Option<&'static EnemyDef> {
    ENEMIES.iter().find(|e| e.id == id)
}

pub fn enemy_def_or_panic(id: &str) -> &'static EnemyDef {
    enemy_def(id).unwrap_or_else(|| panic!("unknown enemy id: {id}"))
}

pub fn encounter_def(id: &str) -> Option<&'static Encounter> {
    all_encounters().find(|e| e.id == id)
}

// ---- 遭遇名单的生成(参考实现 engine/run/encounters.ts) ----

/// 每章弱怪名单抽几条:第一章 3,之后 2
fn weak_count(act: u32) -> usize {
    if act == 1 {
        3
    } else {
        2
    }
}

/// 强怪名单抽几条:1 条"首强" + 12 条
const STRONG_GENERATED: usize = 12;
/// 精英名单抽几条
const ELITE_GENERATED: usize = 10;
/// 重抽上限:池子太小时当场炸掉,而不是死循环
const REROLL_CAP: u32 = 10_000;

fn weak_table(act: u32) -> &'static [Encounter] {
    match act {
        1 => ENCOUNTERS_WEAK,
        2 => ACT2_WEAK,
        _ => ACT3_WEAK,
    }
}

fn strong_table(act: u32) -> &'static [Encounter] {
    match act {
        1 => ENCOUNTERS,
        2 => ACT2,
        _ => ACT3,
    }
}

fn elite_table(act: u32) -> &'static [Encounter] {
    match act {
        1 => ELITES,
        2 => ACT2_ELITES,
        _ => ACT3_ELITES,
    }
}

fn boss_table(act: u32) -> &'static [Encounter] {
    match act {
        1 => BOSSES,
        2 => ACT2_BOSSES,
        _ => ACT3_BOSSES,
    }
}

/// 一张表里进抽取池的条目:weight 为 0 的只当固定阵容存在,不参与抽取
fn pool_entries(table: &'static [Encounter]) -> Vec<&'static Encounter> {
    table.iter().filter(|e| e.weight > 0).collect()
}

/// 候选名单里不许出现名单末尾两条(参考实现的 no-repeat 规则)
fn populate_monster_list(
    list: &mut Vec<&'static str>,
    ids: &[&'static str],
    weights: &[f32],
    count: usize,
    rng: &mut Rng,
) {
    let mut guard = 0;
    let mut done = 0;
    while done < count {
        let to_add = ids[rng.weighted_idx_f32(weights).expect("权重表非空")];
        let n = list.len();
        if n > 0 && (to_add == list[n - 1] || (n > 1 && to_add == list[n - 2])) {
            guard += 1;
            assert!(guard <= REROLL_CAP, "遭遇池太小,满足不了不重样的规则");
            continue;
        }
        list.push(to_add);
        done += 1;
    }
}

/// 首强:弱怪尾巴是 small_slimes 时避开大史莱姆系,尾巴是 two_louses 时避开三只跳蚤
fn populate_first_strong_enemy(
    list: &mut Vec<&'static str>,
    ids: &[&'static str],
    weights: &[f32],
    rng: &mut Rng,
) {
    let last = *list.last().expect("弱怪名单非空");
    let mut guard = 0;
    loop {
        let to_add = ids[rng.weighted_idx_f32(weights).expect("权重表非空")];
        let slime = (to_add == "large_slime" || to_add == "lots_of_slimes") && last == "small_slimes";
        let louse = to_add == "three_louses" && last == "two_louses";
        if slime || louse {
            guard += 1;
            assert!(guard <= REROLL_CAP, "首强重抽次数超上限");
            continue;
        }
        list.push(to_add);
        return;
    }
}

/// 一章的遭遇名单:怪/精英/Boss,都按顺序消耗
pub struct EncounterLists {
    pub monster: Vec<&'static str>,
    pub elite: Vec<&'static str>,
    pub boss: Vec<&'static str>,
}

/// 名单里的 id 换成遭遇定义
pub fn resolve(id: &str) -> &'static Encounter {
    encounter_def(id).unwrap_or_else(|| panic!("名单里的遭遇 {id} 不存在"))
}

/// 生成一章的遭遇名单(全部掷点走 monsterRng)
pub fn generate_encounters(act: u32, rng: &mut Rng) -> EncounterLists {
    let mut monster: Vec<&'static str> = Vec::new();

    // 弱怪:逐只等概率
    let weak = pool_entries(weak_table(act));
    let weak_ids: Vec<&'static str> = weak.iter().map(|e| e.id).collect();
    let w = 1.0f32 / weak_ids.len() as f32;
    let weak_weights = vec![w; weak_ids.len()];
    populate_monster_list(&mut monster, &weak_ids, &weak_weights, weak_count(act), rng);

    // 强怪:权重取自参考实现(分母是权重之和)
    let strong = pool_entries(strong_table(act));
    let strong_ids: Vec<&'static str> = strong.iter().map(|e| e.id).collect();
    let total: u32 = strong.iter().map(|e| e.weight).sum();
    let strong_weights: Vec<f32> = strong
        .iter()
        .map(|e| e.weight as f32 / total as f32)
        .collect();
    populate_first_strong_enemy(&mut monster, &strong_ids, &strong_weights, rng);
    populate_monster_list(
        &mut monster,
        &strong_ids,
        &strong_weights,
        STRONG_GENERATED,
        rng,
    );

    // 精英:三选一,不许和上一条重样
    let elites = pool_entries(elite_table(act));
    let elite_ids: Vec<&'static str> = elites.iter().map(|e| e.id).collect();
    let ew = 1.0f32 / elite_ids.len() as f32;
    let elite_weights = vec![ew; elite_ids.len()];
    let mut elite: Vec<&'static str> = Vec::new();
    let mut guard = 0;
    while elite.len() < ELITE_GENERATED {
        let to_add = elite_ids[rng.weighted_idx_f32(&elite_weights).expect("权重表非空")];
        let n = elite.len();
        if n > 0 && to_add == elite[n - 1] && elite_ids.len() > 1 {
            guard += 1;
            assert!(guard <= REROLL_CAP, "精英重抽次数超上限");
            continue;
        }
        elite.push(to_add);
    }

    // Boss 顺序:一次 monsterRng.randomLong() 给 JavaRandom 定种再洗
    let bosses = boss_table(act);
    let mut idxs: Vec<usize> = (0..bosses.len()).collect();
    crate::rng::java_shuffle(&mut idxs, &mut crate::rng::JavaRandom::new(rng.random_long()));
    let boss: Vec<&'static str> = idxs.into_iter().map(|i| bosses[i].id).collect();

    EncounterLists {
        monster,
        elite,
        boss,
    }
}

/// 名单抽干了就再补一批强怪(参考实现的 generateExtraStrongEncounters)
pub fn generate_extra_strong(act: u32, rng: &mut Rng, count: usize) -> Vec<&'static str> {
    let strong = pool_entries(strong_table(act));
    let strong_ids: Vec<&'static str> = strong.iter().map(|e| e.id).collect();
    let total: u32 = strong.iter().map(|e| e.weight).sum();
    let strong_weights: Vec<f32> = strong
        .iter()
        .map(|e| e.weight as f32 / total as f32)
        .collect();
    let mut list = Vec::new();
    populate_monster_list(&mut list, &strong_ids, &strong_weights, count, rng);
    list
}

/// 按敌人 id 找一场能打的遭遇(调试入口用)
pub fn encounter_with_enemy(id: &str) -> Option<&'static Encounter> {
    all_encounters().find(|e| e.enemies.contains(&id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::enemy::{EnemyFx, Intent};

    /// 所有遭遇表,附带该表应有的敌人类别
    fn tables() -> Vec<(&'static [Encounter], EnemyKind)> {
        vec![
            (ENCOUNTERS_WEAK, EnemyKind::Normal),
            (ENCOUNTERS, EnemyKind::Normal),
            (ELITES, EnemyKind::Elite),
            (BOSSES, EnemyKind::Boss),
            (ACT2_WEAK, EnemyKind::Normal),
            (ACT2, EnemyKind::Normal),
            (ACT2_ELITES, EnemyKind::Elite),
            (ACT2_BOSSES, EnemyKind::Boss),
            (ACT3_WEAK, EnemyKind::Normal),
            (ACT3, EnemyKind::Normal),
            (ACT3_ELITES, EnemyKind::Elite),
            (ACT3_BOSSES, EnemyKind::Boss),
            (ACT4_ELITES, EnemyKind::Elite),
            (ACT4_BOSSES, EnemyKind::Boss),
        ]
    }

    #[test]
    fn enemy_ids_and_names_are_unique() {
        let mut ids: Vec<&str> = ENEMIES.iter().map(|e| e.id).collect();
        let mut names: Vec<&str> = ENEMIES.iter().map(|e| e.name).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate enemy id");
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate enemy name");
    }

    #[test]
    fn encounter_ids_are_unique() {
        let mut ids: Vec<&str> = all_encounters().map(|e| e.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate encounter id");
    }

    #[test]
    fn each_table_is_non_empty_and_kind_matches() {
        for (group, kind) in tables() {
            assert!(!group.is_empty(), "empty encounter table");
            for enc in group {
                assert_eq!(
                    enc.kind, kind,
                    "encounter {} kind mismatch with table",
                    enc.id
                );
            }
        }
        assert!(
            ENCOUNTERS_WEAK.len() >= 3,
            "need at least 3 weak encounters"
        );
        assert!(ENCOUNTERS.len() >= 6, "need at least 6 normal encounters");
        assert!(ELITES.len() >= 3, "need at least 3 elite encounters");
        assert_eq!(BOSSES.len(), 3, "act 1 has 3 bosses");
    }

    #[test]
    fn encounter_slot_table_matches_its_encounters() {
        for (id, slots) in ENCOUNTER_SLOTS {
            let enc = all_encounters()
                .find(|e| e.id == *id)
                .unwrap_or_else(|| panic!("站位表里的遭遇 {id} 不存在"));
            assert_eq!(slots.len(), enc.enemies.len(), "{id} 的槽位数对不上");
            assert!(
                slots.windows(2).all(|w| w[0] < w[1]),
                "{id} 的槽位要从小到大"
            );
        }
        // 有留空槽的遭遇:首领先走,小鬼在 1、2,槽 0 空着
        let leader = encounter_def("gremlin_leader_gang").unwrap();
        assert_eq!(
            (0..leader.enemies.len())
                .map(|i| initial_slot(leader, i))
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // 没列到的遭遇就按 0,1,2... 密集排
        let solo = encounter_def("jaw_worm_solo").unwrap();
        assert_eq!(initial_slot(solo, 0), 0);
        let three = encounter_def("three_sentries").unwrap();
        assert_eq!(
            (0..three.enemies.len())
                .map(|i| initial_slot(three, i))
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    /// 按遭遇自带的抽签规则抽一次阵容
    fn lineup(enc: &Encounter, seed: u64) -> Vec<&'static str> {
        let roll = enc.lineup.expect("这个遭遇没有抽签规则");
        roll(&mut Rng::new(seed))
    }

    fn encounter_with_id(id: &str) -> &'static Encounter {
        all_encounters()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("没有遭遇 {id}"))
    }

    #[test]
    fn encounter_presets_point_at_real_slots_and_moves() {
        for enc in all_encounters() {
            for p in enc.presets {
                assert!(!p.slots.is_empty(), "{} 有一条空槽位的预置", enc.id);
                for slot in p.slots {
                    assert!(
                        *slot < enc.enemies.len(),
                        "{} 的预置槽位 {} 越界",
                        enc.id,
                        slot
                    );
                    if let Some(name) = p.last_move {
                        let def = enemy_def_or_panic(enc.enemies[*slot]);
                        assert!(
                            def.move_index(name).is_some(),
                            "{} 的预置招式 {name} 在 {} 身上不存在",
                            enc.id,
                            def.id
                        );
                    }
                }
                for (s, n) in p.statuses {
                    assert!(*n > 0, "{} 的预置状态 {s} 层数要为正", enc.id);
                }
            }
            for (s, n) in enc.player_statuses {
                assert!(*n > 0, "{} 给玩家的预置状态 {s} 层数要为正", enc.id);
            }
        }
    }

    #[test]
    fn three_and_four_shapes_draw_without_replacement() {
        let three = encounter_with_id("three_shapes");
        let four = encounter_with_id("four_shapes");
        let kinds = ["spiker", "repulsor", "exploder"];
        let mut seen_three = std::collections::HashSet::new();
        let mut seen_four = std::collections::HashSet::new();
        for seed in 0..256u64 {
            let a = lineup(three, seed);
            assert_eq!(a.len(), 3, "三只形状就该抽三只");
            assert_eq!(a, lineup(three, seed), "同一种子要抽出一致的阵容");
            for k in kinds {
                assert!(
                    a.iter().filter(|x| **x == k).count() <= 2,
                    "池子里每种只有两个,seed {seed} 抽出了 {a:?}"
                );
            }
            seen_three.insert(a);

            let b = lineup(four, seed);
            assert_eq!(b.len(), 4, "四只形状就该抽四只");
            for k in kinds {
                assert!(
                    b.iter().filter(|x| **x == k).count() <= 2,
                    "seed {seed} 抽出了 {b:?}"
                );
            }
            seen_four.insert(b);
        }
        assert!(seen_three.len() >= 3, "三只形状只抽出一种阵容");
        assert!(seen_four.len() >= 3, "四只形状只抽出一种阵容");
    }

    #[test]
    fn sphere_and_two_shapes_keeps_the_sphere_and_redraws_the_shapes() {
        let enc = encounter_with_id("sphere_and_two_shapes");
        let mut seen = std::collections::HashSet::new();
        for seed in 0..128u64 {
            let ids = lineup(enc, seed);
            assert_eq!(ids.len(), 3);
            assert_eq!(ids[2], "spheric_guardian", "球体守卫固定排在最后");
            for id in &ids[..2] {
                assert!(
                    ["spiker", "repulsor", "exploder"].contains(id),
                    "前两只要从形状池里放回地抽,抽到了 {id}"
                );
            }
            seen.insert((ids[0], ids[1]));
        }
        assert!(seen.len() >= 3, "两只形状的抽签没变化过");
        // 放回地抽,所以允许两只同种
        assert!(
            seen.contains(&("spiker", "spiker")) || seen.contains(&("exploder", "exploder")),
            "放回地抽应该出现过两只同种"
        );
    }

    #[test]
    fn encounters_reference_known_enemies() {
        for (group, _) in tables() {
            for enc in group {
                assert!(
                    !enc.enemies.is_empty(),
                    "encounter {} has no enemies",
                    enc.id
                );
                for id in enc.enemies {
                    assert!(
                        enemy_def(id).is_some(),
                        "encounter {} references unknown enemy {id}",
                        enc.id
                    );
                }
            }
        }
    }

    #[test]
    fn every_enemy_has_moves_and_hp() {
        for e in ENEMIES {
            assert!(!e.moves.is_empty(), "{} has no moves", e.id);
            assert!(
                e.hp.0 > 0 && e.hp.1 >= e.hp.0,
                "{} has invalid hp range",
                e.id
            );
        }
    }

    #[test]
    fn every_enemy_can_be_reached_in_an_encounter() {
        for e in ENEMIES {
            assert!(
                encounter_with_enemy(e.id).is_some(),
                "{} 不在任何遭遇里,打不到",
                e.id
            );
        }
    }

    #[test]
    fn move_names_are_ascii_and_unique_within_enemy() {
        for e in ENEMIES {
            assert!(
                e.id.is_ascii() && e.name.is_ascii(),
                "{} contains non-ASCII",
                e.id
            );
            let mut names: Vec<&str> = e.moves.iter().map(|m| m.name).collect();
            let n = names.len();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), n, "{} has duplicate move name", e.id);
            for m in e.moves {
                assert!(m.name.is_ascii(), "{} move name contains non-ASCII", e.id);
            }
        }
    }

    #[test]
    fn forced_move_targets_are_in_range() {
        for e in ENEMIES {
            for m in e.moves {
                for fx in m.effects {
                    if let EnemyFx::ForceNext { idx } = fx {
                        assert!(
                            *idx < e.moves.len(),
                            "{} 的 {} 指定了越界的后继招",
                            e.id,
                            m.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sleeper_enemy_is_reachable_from_an_encounter() {
        let sleeper = ENEMIES
            .iter()
            .find(|e| e.innate.iter().any(|(s, _)| *s == crate::core::status::Status::Asleep))
            .expect("need at least one sleeping enemy");
        assert!(
            matches!(
                sleeper.moves[0].intent,
                Intent::Defend | Intent::Buff | Intent::Debuff | Intent::Sleep | Intent::Unknown
            ),
            "sleeping enemy must not open with an attack"
        );
        assert!(
            encounter_with_enemy(sleeper.id).is_some(),
            "sleeping enemy {} is not in any encounter",
            sleeper.id
        );
    }

    #[test]
    fn some_encounter_has_multiple_enemies() {
        let multi = all_encounters().find(|e| e.enemies.len() >= 2);
        assert!(multi.is_some(), "need at least one multi-enemy encounter");
    }

    #[test]
    fn every_corpus_monster_is_implemented() {
        let mut missing: Vec<&str> = Vec::new();
        for m in crate::core::corpus::MONSTERS {
            if enemy_def(m.id).is_none() {
                missing.push(m.id);
            }
        }
        assert!(missing.is_empty(), "语料里的怪还没实现:{missing:?}");
        assert_eq!(crate::core::corpus::MONSTERS.len(), 65);
        assert_eq!(ENEMIES.len(), 65, "本作定义数量要与语料一致");
    }

    #[test]
    fn every_defined_enemy_matches_a_corpus_id() {
        for e in ENEMIES {
            assert!(
                crate::core::corpus::MONSTERS.iter().any(|m| m.id == e.id),
                "{} 在语料里找不到",
                e.id
            );
        }
    }

    #[test]
    fn lookup_helpers_resolve_known_ids() {
        assert_eq!(enemy_def("jaw_worm").unwrap().name, "Jaw Worm");
        assert_eq!(enemy_def_or_panic("the_guardian").kind, EnemyKind::Boss);
        assert_eq!(enemy_def_or_panic("corrupt_heart").kind, EnemyKind::Boss);
        assert!(enemy_def("no_such_enemy").is_none());
        for id in [
            "cultist_solo",
            "three_louses",
            "lagavulin_solo",
            "hexaghost",
            "the_heart",
        ] {
            assert!(encounter_def(id).is_some(), "no such encounter {id}");
        }
        assert!(encounter_def("no_such_encounter").is_none());
    }
}
