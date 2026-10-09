// 由 tools/gen_golden.ts 从参考实现(refs/slay-the-cli)跑出来,不要手改.
// 生成命令:bun tools/gen_golden.ts 12345 > tools/golden/seed12345.json

/// 数字种子
pub const SEED: u64 = 12345;
/// 同一颗种子在参考实现里的 base-35 写法
pub const SEED_STRING: &str = "A2Q";
/// 第一章地图布局(行间用 \n,怪物格是 M)
pub const MAP: &str = "                     M3     M5\n                     M2,4          M4,5,6\n              M3            ?3,5   M5     $6\n                     M2,4          ?4,5   M5\n              ?1            M5     ?5,6\n       R0                          E4,5,6 ?6\n?0,1                        ?4     M5     R5\nE1     ?2                   E3     M4,5,6\n       T1     T2     T4     T4     T4     T5\n       M2     ?2            M3,4   M4\n              R1,3   ?3     M3,4,5\n       M2            E2,3   R5     M5\n              R2,3   M3,4          ?6\n              M2     $4     M4            M5,6\n              R3            R3     R3     R3";
/// 第一章的燃烧精英:(列, 行, 增益编号)
pub const BURNING_ELITE: (i32, i32, i32) = (5, 5, 2);
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
    Case { seed: 3, burning: (1, 10, 0), monsters: &[("ACID_SLIME_S", 12), ("SPIKE_SLIME_M", 31)], gold: 11, potion: None, cards: &[("HEAVY_BLADE", false), ("TWIN_STRIKE", false), ("SHRUG_IT_OFF", false)] },
    Case { seed: 5, burning: (5, 5, 3), monsters: &[("JAW_WORM", 42)], gold: 17, potion: None, cards: &[("HAVOC", false), ("PERFECTED_STRIKE", false), ("WILD_STRIKE", false)] },
    Case { seed: 55, burning: (5, 5, 2), monsters: &[("JAW_WORM", 44)], gold: 15, potion: None, cards: &[("TRUE_GRIT", false), ("IRON_WAVE", false), ("TWIN_STRIKE", false)] },
    Case { seed: 89, burning: (3, 9, 0), monsters: &[("CULTIST", 48)], gold: 15, potion: Some("EXPLOSIVE_POTION"), cards: &[("BURNING_PACT", false), ("CLOTHESLINE", false), ("PUMMEL", false)] },
];

/// Neow 掷出来的四个选项(祝福, 代价)
pub const NEOW_OPTIONS: &[(&str, &str)] = &[("THREE_CARDS", "NONE"), ("TEN_PERCENT_HP_BONUS", "NONE"), ("TWENTY_PERCENT_HP_BONUS", "CURSE"), ("BOSS_RELIC", "LOSE_STARTER_RELIC")];
/// Neow 掷完选项后 neowRng 的计数器
pub const NEOW_RNG_COUNTER: u32 = 5;
/// Neow 领第 2 项之后的局面
pub const NEOW_PICK_BONUS: &str = "TEN_PERCENT_HP_BONUS";
pub const NEOW_PICK_DRAWBACK: &str = "NONE";
pub const NEOW_PICK_HP: i32 = 80;
pub const NEOW_PICK_MAX_HP: i32 = 88;
pub const NEOW_PICK_GOLD: i32 = 99;
pub const NEOW_PICK_DECK: usize = 10;
/// 未知房判定:连续 12 次的结果
pub const UNKNOWN_ROOMS: &[&str] = &["monster", "event", "event", "event", "shop", "monster", "treasure", "event", "monster", "event", "monster", "shop"];
/// 事件抽取:连续 12 次抽到的事件 id
pub const EVENT_PICKS: &[Option<&str>] = &[Some("WE_MEET_AGAIN"), Some("TRANSMORGRIFIER"), Some("FACE_TRADER"), Some("PURIFIER"), Some("THE_WOMAN_IN_BLUE"), Some("LAB"), Some("WHEEL_OF_CHANGE"), Some("UPGRADE_SHRINE"), Some("OMINOUS_FORGE"), Some("BONFIRE_SPIRITS"), Some("NOTE_FOR_YOURSELF"), Some("MATCH_AND_KEEP")];
/// 判定与抽取交替 8 轮:判定结果,判成事件时带上抽到的 id
pub const EVENT_ROLLS: &[&str] = &["monster", "event:GOLDEN_IDOL", "event:WING_STATUE", "event:SHINING_LIGHT", "shop", "monster", "treasure", "event:THE_CLERIC"];
/// 商店货架的牌(稀有度, 价格, 是否无色)
pub const SHOP_CARDS: &[(&str, &str, i32, bool)] = &[("SWORD_BOOMERANG", "common", 45, false), ("DROPKICK", "uncommon", 74, false), ("WARCRY", "common", 26, false), ("BLOODLETTING", "uncommon", 76, false), ("FEEL_NO_PAIN", "uncommon", 74, false), ("FLASH_OF_STEEL", "uncommon", 89, true), ("HAND_OF_GREED", "rare", 168, true)];
/// 商店货架的遗物(档次, 价格)
pub const SHOP_RELICS: &[(&str, &str, i32)] = &[("DISCERNING_MONOCLE", "uncommon", 242), ("TUNGSTEN_ROD", "rare", 293), ("PRISMATIC_SHARD", "shop", 153)];
/// 商店货架的药水(价格)
pub const SHOP_POTIONS: &[(&str, i32)] = &[("DISTILLED_CHAOS", 74), ("ELIXIR_POTION", 74), ("EXPLOSIVE_POTION", 48)];
/// 商店的删牌服务价格
pub const SHOP_REMOVAL: i32 = 75;
/// 商店生成后的三个流计数器(card, merchant, potion)
pub const SHOP_STREAM_COUNTERS: (u32, u32, u32) = (12, 16, 12);
/// 奖励路径连续 12 次的药水身份(没有掉落就是 None)
pub const POTION_REWARD_SEQ: &[Option<&str>] = &[None, Some("EXPLOSIVE_POTION"), None, Some("EXPLOSIVE_POTION"), None, Some("STRENGTH_POTION"), None, Some("WEAK_POTION"), None, None, None, None];
/// 掷完这一批之后 potionRng 的位置与药水保底值
pub const POTION_REWARD_COUNTER: u32 = 24;
pub const POTION_REWARD_PITY: i32 = 40;
/// 商店路径连续 12 次(四家店 × 三瓶)抽到的药水身份
pub const POTION_SHOP_SEQ: &[&str] = &["DISTILLED_CHAOS", "ELIXIR_POTION", "EXPLOSIVE_POTION", "CULTIST_POTION", "STRENGTH_POTION", "FAIRY_POTION", "SWIFT_POTION", "ESSENCE_OF_STEEL", "STRENGTH_POTION", "SKILL_POTION", "DUPLICATION_POTION", "POWER_POTION"];
pub const POTION_SHOP_COUNTER: u32 = 35;
/// 战斗/事件档的遗物掉落(档次, 身份):连掷 12 次
pub const RELIC_COMBAT_SEQ: &[(&str, &str)] = &[("common", "ORICHALCUM"), ("common", "AKABEKO"), ("uncommon", "DISCERNING_MONOCLE"), ("uncommon", "MEAT_ON_THE_BONE"), ("rare", "TUNGSTEN_ROD"), ("common", "LANTERN"), ("common", "BLOOD_VIAL"), ("rare", "GAMBLING_CHIP"), ("common", "BRONZE_SCALES"), ("uncommon", "INK_BOTTLE"), ("uncommon", "QUESTION_CARD"), ("rare", "BIRD_FACED_URN")];
/// 精英档的遗物掉落(档次, 身份):连掷 12 次
pub const RELIC_ELITE_SEQ: &[(&str, &str)] = &[("common", "ORICHALCUM"), ("common", "AKABEKO"), ("uncommon", "DISCERNING_MONOCLE"), ("uncommon", "MEAT_ON_THE_BONE"), ("rare", "TUNGSTEN_ROD"), ("common", "LANTERN"), ("common", "BLOOD_VIAL"), ("rare", "GAMBLING_CHIP"), ("common", "BRONZE_SCALES"), ("uncommon", "INK_BOTTLE"), ("uncommon", "QUESTION_CARD"), ("rare", "BIRD_FACED_URN")];
/// Boss 遗物三选一:四组,每组三件
pub const RELIC_BOSS_CHOICES: &[[&str; 3]] = &[["BUSTED_CROWN", "EMPTY_CAGE", "PHILOSOPHERS_STONE"], ["FUSION_HAMMER", "VELVET_CHOKER", "SACRED_BARK"], ["RUNIC_DOME", "ECTOPLASM", "MARK_OF_PAIN"], ["CALLING_BELL", "COFFEE_DRIPPER", "CURSED_KEY"]];
/// 宝箱(尺寸, 有没有金币, 档次, 身份):连开 6 个
pub const RELIC_CHESTS: &[(&str, bool, &str, &str)] = &[("large", true, "uncommon", "DISCERNING_MONOCLE"), ("medium", false, "uncommon", "MEAT_ON_THE_BONE"), ("small", true, "common", "ORICHALCUM"), ("medium", false, "rare", "TUNGSTEN_ROD"), ("small", true, "common", "AKABEKO"), ("small", false, "common", "LANTERN")];
