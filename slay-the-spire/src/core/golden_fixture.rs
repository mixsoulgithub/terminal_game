// 由 tools/gen_golden.ts 从参考实现(refs/slay-the-cli)跑出来,不要手改.
// 生成命令:bun tools/gen_golden.ts 12345 > tools/golden/seed12345.json

/// 数字种子
pub const SEED: u64 = 12345;
/// 同一颗种子在参考实现里的 base-35 写法
pub const SEED_STRING: &str = "A2Q";
/// 第一章地图布局(行间用 \n,怪物格是 M)
pub const MAP: &str = "                     M3     M5\n                     M2,4          M4,5,6\n              M3            ?3,5   M5     $6\n                     M2,4          ?4,5   M5\n              ?1            M5     ?5,6\n       R0                          E4,5,6 ?6\n?0,1                        ?4     M5     R5\nE1     ?2                   E3     M4,5,6\n       T1     T2     T4     T4     T4     T5\n       M2     ?2            M3,4   M4\n              R1,3   ?3     M3,4,5\n       M2            E2,3   R5     M5\n              R2,3   M3,4          ?6\n              M2     $4     M4            M5,6\n              R3            R3     R3     R3";
/// 开局生成的怪名单(monsterRng)
pub const MONSTER_LIST: &[&str] = &["CULTIST", "JAW_WORM", "SMALL_SLIMES", "LOOTER", "BLUE_SLAVER", "RED_SLAVER", "THREE_LOUSE", "EXORDIUM_WILDLIFE", "LOOTER", "RED_SLAVER", "EXORDIUM_THUGS", "LARGE_SLIME", "EXORDIUM_WILDLIFE", "EXORDIUM_THUGS", "THREE_LOUSE", "EXORDIUM_WILDLIFE"];
/// 开局生成的精英名单
pub const ELITE_LIST: &[&str] = &["GREMLIN_NOB", "THREE_SENTRIES", "GREMLIN_NOB", "LAGAVULIN", "GREMLIN_NOB", "LAGAVULIN", "THREE_SENTRIES", "LAGAVULIN", "THREE_SENTRIES", "GREMLIN_NOB"];
/// 本局 Boss 顺序
pub const BOSS_ORDER: &[&str] = &["HEXAGHOST", "THE_GUARDIAN", "SLIME_BOSS"];
/// 开局生成的怪名单(monsterRng):每一场的怪物阵容
pub const MONSTER_LINEUPS: &[&[&str]] = &[&["CULTIST"], &["JAW_WORM"], &["SPIKE_SLIME_S", "ACID_SLIME_M"], &["LOOTER"], &["BLUE_SLAVER"], &["RED_SLAVER"], &["RED_LOUSE", "GREEN_LOUSE", "RED_LOUSE"], &["FUNGI_BEAST", "JAW_WORM"], &["LOOTER"], &["RED_SLAVER"], &["RED_LOUSE", "BLUE_SLAVER"], &["ACID_SLIME_L"], &["FUNGI_BEAST", "JAW_WORM"], &["RED_LOUSE", "BLUE_SLAVER"], &["RED_LOUSE", "GREEN_LOUSE", "RED_LOUSE"], &["FUNGI_BEAST", "JAW_WORM"]];
/// 开局生成的精英名单的阵容
pub const ELITE_LINEUPS: &[&[&str]] = &[&["GREMLIN_NOB"], &["SENTRY", "SENTRY", "SENTRY"], &["GREMLIN_NOB"], &["LAGAVULIN"], &["GREMLIN_NOB"], &["LAGAVULIN"], &["SENTRY", "SENTRY", "SENTRY"], &["LAGAVULIN"], &["SENTRY", "SENTRY", "SENTRY"], &["GREMLIN_NOB"]];
/// 本局 Boss 顺序的阵容
pub const BOSS_LINEUPS: &[&[&str]] = &[&["HEXAGHOST"], &["THE_GUARDIAN"], &["SLIME_BOSS"]];
/// 第一个怪房间在哪一列
pub const FIRST_ROOM_X: usize = 3;
/// 第一个怪房间的遭遇 id
pub const FIRST_ROOM_ENCOUNTER: &str = "CULTIST";
/// 第一个怪房间的怪物与掷出来的血量
pub const FIRST_ROOM_MONSTERS: &[(&str, i32)] = &[("CULTIST", 51)];
/// 第一场战斗的金币奖励
pub const REWARD_GOLD: i32 = 13;
/// 第一场战斗的药水奖励(没有就是 None)
pub const REWARD_POTION: Option<&str> = None;
/// 第一场战斗的三张卡牌(升级标记)
pub const REWARD_CARDS: &[(&str, bool)] = &[("ARMAMENTS", false), ("TWIN_STRIKE", false), ("ANGER", false)];
/// 多颗种子的第一个怪房间与第一场战斗奖励
pub const CASES: &[Case] = &[
    Case { seed: 3, monsters: &[("SPIKE_SLIME_S", 14), ("ACID_SLIME_M", 31)], gold: 11, potion: None, cards: &[("HEAVY_BLADE", false), ("TWIN_STRIKE", false), ("SHRUG_IT_OFF", false)] },
    Case { seed: 5, monsters: &[("JAW_WORM", 42)], gold: 17, potion: None, cards: &[("HAVOC", false), ("PERFECTED_STRIKE", false), ("WILD_STRIKE", false)] },
    Case { seed: 55, monsters: &[("JAW_WORM", 44)], gold: 15, potion: None, cards: &[("TRUE_GRIT", false), ("IRON_WAVE", false), ("TWIN_STRIKE", false)] },
    Case { seed: 89, monsters: &[("CULTIST", 48)], gold: 15, potion: Some("EXPLOSIVE_POTION"), cards: &[("BURNING_PACT", false), ("CLOTHESLINE", false), ("PUMMEL", false)] },
];
