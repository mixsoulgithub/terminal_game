// 飞升(ascension)难度 1-20 的数值层.
//
// 数据来自 refs/slay-the-cli/data/corpus/monsters-*.json(由 sts_lightspeed 反编译
// 整理),对照 https://slaythespire.wiki.gg/wiki/Ascension 的等级表.这里只放"数值"层:
// 怪物血量、招式数值、开局状态数值;运行层规则(精英频率/开局受伤/诅咒/药水槽/
// 奖励升级率/Boss 金币/商店价格/切幕回血/双 Boss)分别落在 map.rs 与 run.rs 里.
//
// asc == 0 时所有函数都返回传入的基础值,保证 A0 与之前逐字节一致.
use std::borrow::Cow;

use crate::core::enemy::{EnemyDef, EnemyFx, Intent, Scope};
use crate::core::status::Status;

/// 最高飞升等级
pub const MAX: u32 = 20;

/// 把外部输入(CLI / 存档)夹到 0..=20
pub fn clamp(v: i64) -> u32 {
    v.clamp(0, MAX as i64) as u32
}

/// 等级说明(UI 用),下标就是等级
pub const LABELS: [&str; 21] = [
    "The standard climb",
    "Elites spawn more often",
    "Normal enemies deal more damage",
    "Elites deal more damage",
    "Bosses deal more damage",
    "Heal less after boss fights",
    "Start each run damaged",
    "Normal enemies have more HP",
    "Elites have more HP",
    "Bosses have more HP",
    "Start with Ascender's Bane",
    "Start with one less potion slot",
    "Upgraded cards appear less often",
    "Bosses drop less gold",
    "Lower max HP",
    "Unfavorable event odds",
    "Shop prices are higher",
    "Normal enemies have deadlier moves",
    "Elites have deadlier moves",
    "Bosses have deadlier moves",
    "Face two bosses at the end of Act 3",
];

pub fn label(asc: u32) -> &'static str {
    LABELS[asc.min(MAX) as usize]
}

/// 招式飞升覆盖里的一段状态数值(照语料的 asc 字段原样搬)
#[derive(Clone, Copy, Debug)]
pub struct FxAsc {
    pub power: &'static str,
    pub amount: i32,
    pub target: &'static str,
}

/// 一招在某个飞升阈值上的覆盖.字段按 ascTier 语义逐字段累加:
/// 后一档只改写到的字段,没写到的沿用更早档(或基础值).
#[derive(Clone, Copy, Debug)]
pub struct MoveAsc {
    pub level: u32,
    pub damage: Option<i32>,
    pub hits: Option<u8>,
    pub block: Option<i32>,
    /// Some 表示这一档整体重写招式效果列表(空列表也是重写)
    pub fx: Option<&'static [FxAsc]>,
}

// ---- 生成的数据表(见文件头来源) ----
/// 血量飞升档:(敌人 id, 换档等级, 飞升血量区间)
#[rustfmt::skip]
static HP_ASC: &[(&str, u32, (i32, i32))] = &[
    ("cultist", 7, (50, 56)),
    ("jaw_worm", 7, (42, 46)),
    ("red_louse", 7, (11, 16)),
    ("green_louse", 7, (12, 18)),
    ("acid_slime_small", 7, (9, 13)),
    ("acid_slime_medium", 7, (29, 34)),
    ("acid_slime_large", 7, (68, 72)),
    ("spike_slime_small", 7, (11, 15)),
    ("spike_slime_medium", 7, (29, 34)),
    ("spike_slime_large", 7, (67, 73)),
    ("mad_gremlin", 7, (21, 25)),
    ("sneaky_gremlin", 7, (11, 15)),
    ("fat_gremlin", 7, (14, 18)),
    ("shield_gremlin", 7, (13, 17)),
    ("gremlin_wizard", 7, (22, 26)),
    ("looter", 7, (46, 50)),
    ("fungi_beast", 7, (24, 28)),
    ("blue_slaver", 7, (48, 52)),
    ("red_slaver", 7, (48, 52)),
    ("gremlin_nob", 8, (85, 90)),
    ("lagavulin", 8, (112, 115)),
    ("sentry", 8, (39, 45)),
    ("slime_boss", 9, (150, 150)),
    ("the_guardian", 9, (250, 250)),
    ("hexaghost", 9, (264, 264)),
    ("chosen", 7, (98, 103)),
    ("shelled_parasite", 7, (70, 75)),
    ("byrd", 7, (26, 33)),
    ("mugger", 7, (50, 54)),
    ("centurion", 7, (78, 83)),
    ("mystic", 7, (50, 58)),
    ("snake_plant", 7, (78, 82)),
    ("snecko", 7, (120, 125)),
    ("book_of_stabbing", 8, (168, 172)),
    ("gremlin_leader", 8, (145, 155)),
    ("taskmaster", 8, (57, 64)),
    ("bronze_automaton", 9, (320, 320)),
    ("bronze_orb", 9, (54, 60)),
    ("the_collector", 9, (300, 300)),
    ("torch_head", 9, (40, 45)),
    ("the_champ", 9, (440, 440)),
    ("bear", 7, (40, 44)),
    ("romeo", 7, (37, 41)),
    ("pointy", 7, (34, 34)),
    ("darkling", 7, (50, 59)),
    ("orb_walker", 7, (92, 102)),
    ("spiker", 7, (44, 60)),
    ("repulsor", 7, (31, 38)),
    ("exploder", 7, (30, 35)),
    ("spire_growth", 7, (190, 190)),
    ("writhing_mass", 7, (175, 175)),
    ("giant_head", 8, (520, 520)),
    ("nemesis", 8, (200, 200)),
    ("reptomancer", 8, (190, 200)),
    // 觉醒者的飞升档是"开局掷一次"的区间:飞升 9+ 是 300..320(照语料,掷点
    // 要消耗一次 monsterHpRng);一阶段重伤复活时 REBIRTH 再把上限盖成定值 320.
    ("awakened_one", 9, (300, 320)),
    ("time_eater", 9, (480, 480)),
    ("donu", 9, (265, 265)),
    ("deca", 9, (265, 265)),
    ("spire_shield", 8, (125, 125)),
    ("spire_spear", 8, (180, 180)),
    ("corrupt_heart", 9, (800, 800)),
];

/// 招式飞升覆盖:(敌人 id, 归一化招式名, 分档覆盖)
#[rustfmt::skip]
static MOVE_ASC: &[(&str, &[(&str, &[MoveAsc])])] = &[
    ("cultist", &[
        ("incantation", &[
            MoveAsc { level: 2, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "RITUAL", amount: 4, target: "self" }]) },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "RITUAL", amount: 5, target: "self" }]) },
        ]),
    ]),
    ("jaw_worm", &[
        ("chomp", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
        ("bellow", &[
            MoveAsc { level: 2, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }]) },
            MoveAsc { level: 17, damage: None, hits: None, block: Some(9), fx: Some(&[FxAsc { power: "STRENGTH", amount: 5, target: "self" }]) },
        ]),
    ]),
    ("red_louse", &[
        ("grow", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }]) },
        ]),
    ]),
    ("acid_slime_small", &[
        ("tackle", &[
            MoveAsc { level: 2, damage: Some(4), hits: None, block: None, fx: None },
        ]),
    ]),
    ("acid_slime_medium", &[
        ("corrosivespit", &[
            MoveAsc { level: 2, damage: Some(8), hits: None, block: None, fx: None },
        ]),
        ("tackle", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
    ]),
    ("acid_slime_large", &[
        ("corrosivespit", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
        ("tackle", &[
            MoveAsc { level: 2, damage: Some(18), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spike_slime_small", &[
        ("tackle", &[
            MoveAsc { level: 2, damage: Some(6), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spike_slime_medium", &[
        ("flametackle", &[
            MoveAsc { level: 2, damage: Some(10), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spike_slime_large", &[
        ("flametackle", &[
            MoveAsc { level: 2, damage: Some(18), hits: None, block: None, fx: None },
        ]),
        ("lick", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "FRAIL", amount: 3, target: "player" }]) },
        ]),
    ]),
    ("mad_gremlin", &[
        ("scratch", &[
            MoveAsc { level: 2, damage: Some(5), hits: None, block: None, fx: None },
        ]),
    ]),
    ("sneaky_gremlin", &[
        ("puncture", &[
            MoveAsc { level: 2, damage: Some(10), hits: None, block: None, fx: None },
        ]),
    ]),
    ("fat_gremlin", &[
        ("smash", &[
            MoveAsc { level: 2, damage: Some(5), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 1, target: "player" }, FxAsc { power: "FRAIL", amount: 1, target: "player" }]) },
        ]),
    ]),
    ("shield_gremlin", &[
        ("protect", &[
            MoveAsc { level: 7, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "BLOCK", amount: 8, target: "allies" }]) },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "BLOCK", amount: 11, target: "allies" }]) },
        ]),
        ("shieldbash", &[
            MoveAsc { level: 2, damage: Some(8), hits: None, block: None, fx: None },
        ]),
    ]),
    ("gremlin_wizard", &[
        ("ultimateblast", &[
            MoveAsc { level: 2, damage: Some(30), hits: None, block: None, fx: None },
        ]),
    ]),
    ("looter", &[
        ("mug", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STEAL_GOLD", amount: 20, target: "player" }]) },
        ]),
        ("lunge", &[
            MoveAsc { level: 2, damage: Some(14), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STEAL_GOLD", amount: 20, target: "player" }]) },
        ]),
    ]),
    ("fungi_beast", &[
        ("grow", &[
            MoveAsc { level: 2, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }]) },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 5, target: "self" }]) },
        ]),
    ]),
    ("blue_slaver", &[
        ("stab", &[
            MoveAsc { level: 2, damage: Some(13), hits: None, block: None, fx: None },
        ]),
        ("rake", &[
            MoveAsc { level: 2, damage: Some(8), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 2, target: "player" }]) },
        ]),
    ]),
    ("red_slaver", &[
        ("stab", &[
            MoveAsc { level: 2, damage: Some(14), hits: None, block: None, fx: None },
        ]),
        ("scrape", &[
            MoveAsc { level: 2, damage: Some(9), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "VULNERABLE", amount: 2, target: "player" }]) },
        ]),
    ]),
    ("gremlin_nob", &[
        ("bellow", &[
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "ENRAGE", amount: 3, target: "self" }]) },
        ]),
        ("rush", &[
            MoveAsc { level: 3, damage: Some(16), hits: None, block: None, fx: None },
        ]),
        ("skullbash", &[
            MoveAsc { level: 3, damage: Some(8), hits: None, block: None, fx: None },
        ]),
    ]),
    ("lagavulin", &[
        ("attack", &[
            MoveAsc { level: 3, damage: Some(20), hits: None, block: None, fx: None },
        ]),
        ("siphonsoul", &[
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "DEXTERITY", amount: -2, target: "player" }, FxAsc { power: "STRENGTH", amount: -2, target: "player" }]) },
        ]),
    ]),
    ("sentry", &[
        ("bolt", &[
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "CARD:DAZED", amount: 3, target: "player" }]) },
        ]),
        ("beam", &[
            MoveAsc { level: 3, damage: Some(10), hits: None, block: None, fx: None },
        ]),
    ]),
    ("slime_boss", &[
        ("goopspray", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "CARD:SLIMED", amount: 5, target: "player" }]) },
        ]),
        ("slam", &[
            MoveAsc { level: 4, damage: Some(38), hits: None, block: None, fx: None },
        ]),
    ]),
    ("the_guardian", &[
        ("fiercebash", &[
            MoveAsc { level: 4, damage: Some(36), hits: None, block: None, fx: None },
        ]),
        ("defensivemode", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "SHARP_HIDE", amount: 4, target: "self" }]) },
        ]),
        ("rollattack", &[
            MoveAsc { level: 4, damage: Some(10), hits: None, block: None, fx: None },
        ]),
        // 双连击打完后重新装上形态切换额度:额度 = 本档基础 + 10(30/35/40 各 +10)
        ("twinslam", &[
            MoveAsc { level: 9, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "MODE_SHIFT", amount: 45, target: "self" }]) },
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "MODE_SHIFT", amount: 50, target: "self" }]) },
        ]),
    ]),
    ("hexaghost", &[
        ("sear", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "CARD:BURN", amount: 2, target: "player" }]) },
        ]),
        ("tackle", &[
            MoveAsc { level: 4, damage: Some(6), hits: None, block: None, fx: None },
        ]),
        ("inflame", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 3, target: "self" }]) },
        ]),
        ("inferno", &[
            MoveAsc { level: 4, damage: Some(3), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spheric_guardian", &[
        ("activate", &[
            MoveAsc { level: 17, damage: None, hits: None, block: Some(35), fx: None },
        ]),
        ("attackdebuff", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
        ]),
        ("slam", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
        ]),
        ("harden", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
        ]),
    ]),
    ("chosen", &[
        ("poke", &[
            MoveAsc { level: 2, damage: Some(6), hits: None, block: None, fx: None },
        ]),
        ("zap", &[
            MoveAsc { level: 2, damage: Some(21), hits: None, block: None, fx: None },
        ]),
        ("debilitate", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
    ]),
    ("shelled_parasite", &[
        ("fell", &[
            MoveAsc { level: 2, damage: Some(21), hits: None, block: None, fx: None },
        ]),
        ("doublestrike", &[
            MoveAsc { level: 2, damage: Some(7), hits: None, block: None, fx: None },
        ]),
        ("suck", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
    ]),
    ("byrd", &[
        ("peck", &[
            MoveAsc { level: 2, damage: None, hits: Some(6), block: None, fx: None },
        ]),
        ("swoop", &[
            MoveAsc { level: 2, damage: Some(14), hits: None, block: None, fx: None },
        ]),
        ("fly", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "FLIGHT", amount: 4, target: "self" }]) },
        ]),
    ]),
    ("mugger", &[
        ("mug", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STEAL_GOLD", amount: 20, target: "player" }]) },
        ]),
        ("lunge", &[
            MoveAsc { level: 2, damage: Some(18), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STEAL_GOLD", amount: 20, target: "player" }]) },
        ]),
        ("smokebomb", &[
            MoveAsc { level: 17, damage: None, hits: None, block: Some(17), fx: None },
        ]),
    ]),
    ("centurion", &[
        ("slash", &[
            MoveAsc { level: 2, damage: Some(14), hits: None, block: None, fx: None },
        ]),
        ("fury", &[
            MoveAsc { level: 2, damage: Some(7), hits: None, block: None, fx: None },
        ]),
        ("defend", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "BLOCK", amount: 20, target: "allies" }]) },
        ]),
    ]),
    ("mystic", &[
        ("attackdebuff", &[
            MoveAsc { level: 2, damage: Some(9), hits: None, block: None, fx: None },
        ]),
        ("heal", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "HEAL", amount: 20, target: "self" }, FxAsc { power: "HEAL", amount: 20, target: "allies" }]) },
        ]),
        ("buff", &[
            MoveAsc { level: 2, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 3, target: "self" }, FxAsc { power: "STRENGTH", amount: 3, target: "allies" }]) },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }, FxAsc { power: "STRENGTH", amount: 4, target: "allies" }]) },
        ]),
    ]),
    ("snake_plant", &[
        ("chomp", &[
            MoveAsc { level: 2, damage: Some(8), hits: None, block: None, fx: None },
        ]),
    ]),
    ("snecko", &[
        ("bite", &[
            MoveAsc { level: 2, damage: Some(18), hits: None, block: None, fx: None },
        ]),
        ("tailwhip", &[
            MoveAsc { level: 2, damage: Some(10), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "VULNERABLE", amount: 2, target: "player" }, FxAsc { power: "WEAK", amount: 2, target: "player" }]) },
        ]),
    ]),
    ("book_of_stabbing", &[
        ("multistab", &[
            MoveAsc { level: 3, damage: Some(7), hits: None, block: None, fx: None },
        ]),
        ("singlestab", &[
            MoveAsc { level: 3, damage: Some(24), hits: None, block: None, fx: None },
        ]),
    ]),
    ("gremlin_leader", &[
        ("encourage", &[
            MoveAsc { level: 3, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }, FxAsc { power: "STRENGTH", amount: 4, target: "allies" }, FxAsc { power: "BLOCK", amount: 6, target: "allies" }]) },
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 5, target: "self" }, FxAsc { power: "STRENGTH", amount: 5, target: "allies" }, FxAsc { power: "BLOCK", amount: 10, target: "allies" }]) },
        ]),
    ]),
    ("taskmaster", &[
        ("scouringwhip", &[
            MoveAsc { level: 3, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "ADD_WOUND_TO_DISCARD", amount: 2, target: "player" }]) },
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "ADD_WOUND_TO_DISCARD", amount: 3, target: "player" }, FxAsc { power: "STRENGTH", amount: 1, target: "self" }]) },
        ]),
    ]),
    ("bronze_automaton", &[
        ("flail", &[
            MoveAsc { level: 4, damage: Some(8), hits: None, block: None, fx: None },
        ]),
        ("boost", &[
            MoveAsc { level: 4, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }]) },
            MoveAsc { level: 9, damage: None, hits: None, block: Some(12), fx: None },
        ]),
        ("hyperbeam", &[
            MoveAsc { level: 4, damage: Some(50), hits: None, block: None, fx: None },
        ]),
    ]),
    ("the_collector", &[
        ("fireball", &[
            MoveAsc { level: 4, damage: Some(21), hits: None, block: None, fx: None },
        ]),
        ("buff", &[
            MoveAsc { level: 4, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }, FxAsc { power: "STRENGTH", amount: 4, target: "allies" }]) },
            MoveAsc { level: 9, damage: None, hits: None, block: Some(18), fx: None },
            MoveAsc { level: 19, damage: None, hits: None, block: Some(23), fx: Some(&[FxAsc { power: "STRENGTH", amount: 5, target: "self" }, FxAsc { power: "STRENGTH", amount: 5, target: "allies" }]) },
        ]),
        ("megadebuff", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 5, target: "player" }, FxAsc { power: "VULNERABLE", amount: 5, target: "player" }, FxAsc { power: "FRAIL", amount: 5, target: "player" }]) },
        ]),
    ]),
    ("the_champ", &[
        ("heavyslash", &[
            MoveAsc { level: 4, damage: Some(18), hits: None, block: None, fx: None },
        ]),
        ("faceslap", &[
            MoveAsc { level: 4, damage: Some(14), hits: None, block: None, fx: None },
        ]),
        ("defensivestance", &[
            MoveAsc { level: 9, damage: None, hits: None, block: Some(18), fx: Some(&[FxAsc { power: "METALLICIZE", amount: 6, target: "self" }]) },
            MoveAsc { level: 19, damage: None, hits: None, block: Some(20), fx: Some(&[FxAsc { power: "METALLICIZE", amount: 7, target: "self" }]) },
        ]),
        ("gloat", &[
            MoveAsc { level: 4, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 3, target: "self" }]) },
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 4, target: "self" }]) },
        ]),
        ("anger", &[
            MoveAsc { level: 4, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "REMOVE_DEBUFFS", amount: 0, target: "self" }, FxAsc { power: "STRENGTH", amount: 9, target: "self" }]) },
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "REMOVE_DEBUFFS", amount: 0, target: "self" }, FxAsc { power: "STRENGTH", amount: 12, target: "self" }]) },
        ]),
    ]),
    ("bear", &[
        ("bearhug", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "DEXTERITY", amount: -4, target: "player" }]) },
        ]),
        ("lunge", &[
            MoveAsc { level: 2, damage: Some(10), hits: None, block: None, fx: None },
        ]),
        ("maul", &[
            MoveAsc { level: 2, damage: Some(20), hits: None, block: None, fx: None },
        ]),
    ]),
    ("romeo", &[
        ("agonizingslash", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 3, target: "player" }]) },
        ]),
        ("crossslash", &[
            MoveAsc { level: 2, damage: Some(17), hits: None, block: None, fx: None },
        ]),
    ]),
    ("pointy", &[
        ("attack", &[
            MoveAsc { level: 2, damage: Some(6), hits: None, block: None, fx: None },
        ]),
    ]),
    ("darkling", &[
        ("chomp", &[
            MoveAsc { level: 2, damage: Some(9), hits: None, block: None, fx: None },
        ]),
        ("harden", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 2, target: "self" }]) },
        ]),
    ]),
    ("orb_walker", &[
        ("laser", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
        ]),
        ("claw", &[
            MoveAsc { level: 2, damage: Some(16), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spiker", &[
        ("cut", &[
            MoveAsc { level: 2, damage: Some(9), hits: None, block: None, fx: None },
        ]),
    ]),
    ("repulsor", &[
        ("bash", &[
            MoveAsc { level: 2, damage: Some(13), hits: None, block: None, fx: None },
        ]),
    ]),
    ("exploder", &[
        ("slam", &[
            MoveAsc { level: 2, damage: Some(11), hits: None, block: None, fx: None },
        ]),
    ]),
    ("transient", &[
        ("attack", &[
            MoveAsc { level: 2, damage: Some(40), hits: None, block: None, fx: None },
        ]),
    ]),
    ("the_maw", &[
        ("roar", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 5, target: "player" }, FxAsc { power: "FRAIL", amount: 5, target: "player" }]) },
        ]),
        ("drool", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "STRENGTH", amount: 5, target: "self" }]) },
        ]),
        ("slam", &[
            MoveAsc { level: 2, damage: Some(30), hits: None, block: None, fx: None },
        ]),
    ]),
    ("spire_growth", &[
        ("quicktackle", &[
            MoveAsc { level: 2, damage: Some(18), hits: None, block: None, fx: None },
        ]),
        ("smash", &[
            MoveAsc { level: 2, damage: Some(25), hits: None, block: None, fx: None },
        ]),
        ("constrict", &[
            MoveAsc { level: 17, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "CONSTRICTED", amount: 12, target: "player" }]) },
        ]),
    ]),
    ("writhing_mass", &[
        ("strongstrike", &[
            MoveAsc { level: 2, damage: Some(38), hits: None, block: None, fx: None },
        ]),
        ("multistrike", &[
            MoveAsc { level: 2, damage: Some(9), hits: None, block: None, fx: None },
        ]),
        ("flail", &[
            MoveAsc { level: 2, damage: Some(16), hits: None, block: Some(18), fx: None },
        ]),
        ("wither", &[
            MoveAsc { level: 2, damage: Some(12), hits: None, block: None, fx: None },
        ]),
    ]),
    ("giant_head", &[
        ("itistime", &[
            MoveAsc { level: 3, damage: Some(40), hits: None, block: None, fx: None },
        ]),
    ]),
    ("nemesis", &[
        ("triattack", &[
            MoveAsc { level: 3, damage: Some(7), hits: None, block: None, fx: None },
        ]),
        ("triburn", &[
            MoveAsc { level: 3, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "AddCard:BURN:discard", amount: 5, target: "player" }]) },
        ]),
    ]),
    ("reptomancer", &[
        ("summon", &[
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "SummonDaggers", amount: 2, target: "allies" }]) },
        ]),
        ("snakestrike", &[
            MoveAsc { level: 3, damage: Some(16), hits: None, block: None, fx: None },
        ]),
        ("bigbite", &[
            MoveAsc { level: 3, damage: Some(34), hits: None, block: None, fx: None },
        ]),
    ]),
    ("time_eater", &[
        ("reverberate", &[
            MoveAsc { level: 4, damage: Some(8), hits: None, block: None, fx: None },
        ]),
        ("headslam", &[
            MoveAsc { level: 4, damage: Some(32), hits: None, block: None, fx: None },
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "DRAW_REDUCTION", amount: 1, target: "player" }, FxAsc { power: "AddCard:SLIMED:discard", amount: 2, target: "player" }]) },
        ]),
        ("ripple", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "WEAK", amount: 1, target: "player" }, FxAsc { power: "VULNERABLE", amount: 1, target: "player" }, FxAsc { power: "FRAIL", amount: 1, target: "player" }]) },
        ]),
        ("haste", &[
            MoveAsc { level: 19, damage: None, hits: None, block: Some(32), fx: None },
        ]),
    ]),
    ("donu", &[
        ("beam", &[
            MoveAsc { level: 4, damage: Some(12), hits: None, block: None, fx: None },
        ]),
    ]),
    ("deca", &[
        ("beam", &[
            MoveAsc { level: 4, damage: Some(12), hits: None, block: None, fx: None },
        ]),
        ("squareofprotection", &[
            MoveAsc { level: 19, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "Block", amount: 16, target: "team" }, FxAsc { power: "PLATED_ARMOR", amount: 3, target: "team" }]) },
        ]),
    ]),
    ("spire_shield", &[
        ("bash", &[
            MoveAsc { level: 3, damage: Some(14), hits: None, block: None, fx: None },
        ]),
        ("smash", &[
            MoveAsc { level: 3, damage: Some(38), hits: None, block: None, fx: None },
            MoveAsc { level: 18, damage: None, hits: None, block: Some(99), fx: Some(&[]) },
        ]),
    ]),
    ("spire_spear", &[
        ("burnstrike", &[
            MoveAsc { level: 3, damage: Some(6), hits: None, block: None, fx: None },
            MoveAsc { level: 18, damage: None, hits: None, block: None, fx: Some(&[FxAsc { power: "AddCard:BURN:drawTop", amount: 2, target: "player" }]) },
        ]),
        ("skewer", &[
            MoveAsc { level: 3, damage: None, hits: Some(4), block: None, fx: None },
        ]),
    ]),
    ("corrupt_heart", &[
        ("bloodshots", &[
            MoveAsc { level: 4, damage: None, hits: Some(15), block: None, fx: None },
        ]),
        ("echo", &[
            MoveAsc { level: 4, damage: Some(45), hits: None, block: None, fx: None },
        ]),
    ]),
];


/// 合并后的招式覆盖
#[derive(Default)]
struct Merged {
    damage: Option<i32>,
    hits: Option<u8>,
    block: Option<i32>,
    fx: Option<&'static [FxAsc]>,
}

impl Merged {
    fn is_empty(&self) -> bool {
        self.damage.is_none() && self.hits.is_none() && self.block.is_none() && self.fx.is_none()
    }
}

/// 招式名归一化:小写去除非字母数字(生成数据里也是这么归一化的)
fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// 取 (敌人 id, 归一化招式名) 在 asc 下生效的合并覆盖
fn merged(id: &str, move_name: &str, asc: u32) -> Option<Merged> {
    let (_, table) = MOVE_ASC.iter().find(|(eid, _)| *eid == id)?;
    let key = norm(move_name);
    let (_, tiers) = table.iter().find(|(k, _)| *k == key)?;
    let mut m = Merged::default();
    let mut any = false;
    for t in *tiers {
        if asc < t.level {
            continue;
        }
        any = true;
        if t.damage.is_some() {
            m.damage = t.damage;
        }
        if t.hits.is_some() {
            m.hits = t.hits;
        }
        if t.block.is_some() {
            m.block = t.block;
        }
        if t.fx.is_some() {
            m.fx = t.fx;
        }
    }
    if any {
        Some(m)
    } else {
        None
    }
}

// ---- 血量 ----

/// 敌人血量区间:达到换档等级就用飞升血量,否则基础血量
pub fn hp_range(def: &EnemyDef, asc: u32) -> (i32, i32) {
    if asc == 0 {
        return def.hp;
    }
    for (id, lvl, rng) in HP_ASC {
        if *id == def.id && asc >= *lvl {
            return *rng;
        }
    }
    def.hp
}

/// 原版里"血量固定、连一次掷点都不消耗"的怪(反编译 MonsterSpecific.cpp 的
/// initHp 直接 curHp = maxHp = 固定值):球形守卫 / 巨口 / 闪现者
pub fn hp_fixed_no_roll(def_id: &str) -> bool {
    matches!(def_id, "spheric_guardian" | "the_maw" | "transient")
}

/// 掷一次怪物血量.上面那三只不掷;其余的即使区间塌成一点也要掷一次 ——
/// 原版的 Random.random(min, max) 一律消耗一次掷点(Monster::setRandomHp)
pub fn roll_hp(rng: &mut crate::rng::Rng, def: &crate::core::enemy::EnemyDef, asc: u32) -> i32 {
    let (lo, hi) = hp_range(def, asc);
    if hp_fixed_no_roll(def.id) {
        return lo;
    }
    rng.range_inclusive(lo, hi)
}

/// 觉醒者二阶段血量:飞升 9+ 是 320,否则 300(参考实现 REBIRTH)
pub fn awakened_phase2_hp(asc: u32) -> i32 {
    if asc >= 9 {
        320
    } else {
        300
    }
}

/// 虱子的卷曲层数区间(飞升 7 / 17 换档)
pub fn curl_up_range(asc: u32) -> (i32, i32) {
    if asc >= 17 {
        (9, 12)
    } else if asc >= 7 {
        (4, 8)
    } else {
        (3, 7)
    }
}

/// 虱子的咬伤区间(飞升 2+ 换档)
pub fn louse_bite_range(asc: u32) -> (i32, i32) {
    if asc >= 2 {
        (6, 8)
    } else {
        (5, 7)
    }
}

/// 暗灵的撕咬:飞升 2+ 掷 9..13 再额外 +2(参考实现 darkling.ts)
pub fn darkling_nip_roll(asc: u32) -> (i32, i32, i32) {
    if asc >= 2 {
        (9, 13, 2)
    } else {
        (7, 11, 0)
    }
}

// ---- 开局状态 ----

/// 开局自带状态的飞升数值(按敌人 id + 状态查)
pub fn innate_amount(id: &str, status: Status, base: i32, asc: u32) -> i32 {
    if asc == 0 {
        return base;
    }
    use Status::*;
    match (id, status) {
        ("mad_gremlin", Anger) if asc >= 17 => 2,
        ("the_guardian", ModeShift) => {
            if asc >= 19 {
                40
            } else if asc >= 9 {
                35
            } else {
                base
            }
        }
        ("byrd", Flight) if asc >= 17 => 4,
        ("orb_walker", StrengthUp) if asc >= 17 => 5,
        ("spiker", Thorns) => {
            if asc >= 17 {
                7
            } else if asc >= 2 {
                4
            } else {
                base
            }
        }
        ("transient", Fading) if asc >= 17 => 6,
        ("awakened_one", Regenerate) if asc >= 19 => 15,
        ("awakened_one", Curiosity) if asc >= 19 => 2,
        ("donu" | "deca", Artifact) if asc >= 19 => 3,
        ("spire_shield" | "spire_spear", Artifact) if asc >= 18 => 2,
        ("corrupt_heart", BeatOfDeath) if asc >= 19 => 2,
        ("corrupt_heart", Invincible) if asc >= 19 => 200,
        _ => base,
    }
}

/// 基础列表之外、飞升才额外给的开局状态
pub fn bonus_innate(id: &str, asc: u32) -> &'static [(Status, i32)] {
    if id == "awakened_one" && asc >= 4 {
        &[(Status::Strength, 2)]
    } else {
        &[]
    }
}

// ---- 意图 ----

/// 招式的飞升意图(把 damage/hits/block 覆盖到基础意图上)
pub fn intent(id: &str, move_name: &str, base: Intent, asc: u32) -> Intent {
    if asc == 0 {
        return base;
    }
    let Some(m) = merged(id, move_name, asc) else {
        return base;
    };
    match base {
        Intent::Attack { damage, times } => Intent::Attack {
            damage: m.damage.unwrap_or(damage),
            times: m.hits.unwrap_or(times),
        },
        Intent::AttackDefend {
            damage,
            times,
            block,
        } => Intent::AttackDefend {
            damage: m.damage.unwrap_or(damage),
            times: m.hits.unwrap_or(times),
            block: m.block.unwrap_or(block),
        },
        Intent::AttackDebuff { damage, times } => Intent::AttackDebuff {
            damage: m.damage.unwrap_or(damage),
            times: m.hits.unwrap_or(times),
        },
        Intent::AttackBuff { damage, times } => Intent::AttackBuff {
            damage: m.damage.unwrap_or(damage),
            times: m.hits.unwrap_or(times),
        },
        Intent::DefendBuff { block } => Intent::DefendBuff {
            block: m.block.unwrap_or(block),
        },
        Intent::DefendDebuff { block } => Intent::DefendDebuff {
            block: m.block.unwrap_or(block),
        },
        other => other,
    }
}

// ---- 效果 ----

/// 效果的类型标签:飞升覆盖按 (kind, detail) 匹配基础招式里对应的那一段
fn fx_key(fx: &EnemyFx) -> (&'static str, String) {
    match fx {
        EnemyFx::GainStatus { status, .. } => ("status", status_key(*status).to_string()),
        EnemyFx::PlayerStatus { status, .. } => ("status", status_key(*status).to_string()),
        EnemyFx::Block { .. } => ("block", String::new()),
        EnemyFx::BlockFromDamage => ("blockfromdmg", String::new()),
        EnemyFx::StealGold { .. } => ("steal", String::new()),
        EnemyFx::PlayerCard { card, .. } => ("card", card.to_ascii_uppercase()),
        EnemyFx::Heal { .. } => ("heal", String::new()),
        EnemyFx::DrawReduction { .. } => ("draw", String::new()),
        EnemyFx::ClearDebuffs => ("clear", String::new()),
        EnemyFx::Summon { .. } => ("summon", String::new()),
        EnemyFx::HealFromDamage => ("healfromdmg", String::new()),
        _ => ("other", String::new()),
    }
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Strength => "STRENGTH",
        Status::Dexterity => "DEXTERITY",
        Status::Weak => "WEAK",
        Status::Frail => "FRAIL",
        Status::Vulnerable => "VULNERABLE",
        Status::Constricted => "CONSTRICTED",
        Status::Ritual => "RITUAL",
        Status::Enrage => "ENRAGE",
        Status::Metallicize => "METALLICIZE",
        Status::Flight => "FLIGHT",
        Status::SharpHide => "SHARP_HIDE",
        Status::PlatedArmor => "PLATED_ARMOR",
        Status::Anger => "ANGER",
        Status::Thorns => "THORNS",
        Status::Artifact => "ARTIFACT",
        Status::ModeShift => "MODE_SHIFT",
        _ => "",
    }
}

/// 语料里的 power 名 → 效果标签
fn asc_key(f: &FxAsc) -> (&'static str, String) {
    let p = f.power.to_ascii_uppercase();
    match p.as_str() {
        "BLOCK" => ("block", String::new()),
        "BLOCKFROMDAMAGEDEALT" => ("blockfromdmg", String::new()),
        "STEAL_GOLD" => ("steal", String::new()),
        "HEAL" => ("heal", String::new()),
        "DRAW_REDUCTION" => ("draw", String::new()),
        "REMOVE_DEBUFFS" => ("clear", String::new()),
        "SUMMONDAGGERS" => ("summon", String::new()),
        "HEAL_FOR_UNBLOCKED_DAMAGE" => ("healfromdmg", String::new()),
        "STRENGTH" | "DEXTERITY" | "WEAK" | "FRAIL" | "VULNERABLE" | "CONSTRICTED" | "RITUAL"
        | "ENRAGE" | "METALLICIZE" | "FLIGHT" | "SHARP_HIDE" | "PLATED_ARMOR" | "ANGER"
        | "THORNS" | "ARTIFACT" | "MODE_SHIFT" => ("status", p),
        // 塞牌:统一归到 card,按卡名匹配
        _ if p.starts_with("CARD:") => ("card", p["CARD:".len()..].to_string()),
        _ if p.starts_with("ADDCARD:") => {
            let rest = &p["ADDCARD:".len()..];
            let card = rest.split(':').next().unwrap_or("").to_string();
            ("card", card)
        }
        "ADD_WOUND_TO_DISCARD" => ("card", "WOUND".to_string()),
        _ => ("other", p),
    }
}

/// 覆盖某段基础效果里的数值
fn set_amount(fx: &mut EnemyFx, n: i32) {
    match fx {
        EnemyFx::GainStatus { n: v, .. }
        | EnemyFx::PlayerStatus { n: v, .. }
        | EnemyFx::StealGold { n: v }
        | EnemyFx::Heal { n: v, .. }
        | EnemyFx::DrawReduction { n: v }
        | EnemyFx::PlayerCard { n: v, .. } => *v = n,
        EnemyFx::Block { amount, .. } => *amount = n,
        _ => {}
    }
}

/// 覆盖里新增的一段效果(基础招式里原本没有的)
fn new_fx(f: &FxAsc) -> EnemyFx {
    let scope = match f.target {
        "allies" => Scope::Allies,
        "team" => Scope::Team,
        _ => Scope::SelfOnly,
    };
    let p = f.power.to_ascii_uppercase();
    if let Some(card) = card_of_power(&p) {
        let spot = if p.contains("DRAWTOP") {
            crate::core::enemy::CardSpot::DrawTop
        } else {
            crate::core::enemy::CardSpot::Discard
        };
        return EnemyFx::PlayerCard {
            card,
            spot,
            n: f.amount,
        };
    }
    match p.as_str() {
        "STRENGTH" => EnemyFx::GainStatus {
            status: Status::Strength,
            n: f.amount,
            scope,
        },
        "PLATED_ARMOR" => EnemyFx::GainStatus {
            status: Status::PlatedArmor,
            n: f.amount,
            scope,
        },
        "WEAK" => EnemyFx::PlayerStatus {
            status: Status::Weak,
            n: f.amount,
        },
        "FRAIL" => EnemyFx::PlayerStatus {
            status: Status::Frail,
            n: f.amount,
        },
        "VULNERABLE" => EnemyFx::PlayerStatus {
            status: Status::Vulnerable,
            n: f.amount,
        },
        _ => EnemyFx::Block {
            amount: f.amount,
            scope,
        },
    }
}

/// 语料 power 名里带的卡牌 → 本作的卡 id
fn card_of_power(p: &str) -> Option<&'static str> {
    let name = if let Some(rest) = p.strip_prefix("ADDCARD:") {
        rest.split(':').next().unwrap_or("")
    } else if let Some(rest) = p.strip_prefix("CARD:") {
        rest
    } else if p == "ADD_WOUND_TO_DISCARD" {
        "WOUND"
    } else {
        return None;
    };
    match name {
        "BURN" => Some("burn"),
        "DAZED" => Some("dazed"),
        "SLIMED" => Some("slimed"),
        "WOUND" => Some("wound"),
        _ => None,
    }
}

/// 少数需要整体替换效果的招式(参考实现里是重写 effects 列表)
fn hard_replace(id: &str, move_name: &str, asc: u32) -> Option<&'static [EnemyFx]> {
    // 巨大头颅的"时候到了":基础 30(A3 起 40) + 每回合 +5、上限 +30;
    // A18 起首用回合从第 5 提前到第 4(A20 起就对得上参考实现).
    // AttackScaling 的算式是 amount + per_turn * min(turns-1, cap),
    // 所以把"首用回合"折进 amount、cap 取 首用-1 + 6.
    if id == "giant_head" && norm(move_name) == "itistime" {
        if asc >= 18 {
            return Some(&[EnemyFx::AttackScaling {
                amount: 25,
                per_turn: 5,
                cap: 9,
                times: 1,
            }]);
        }
        if asc >= 3 {
            return Some(&[EnemyFx::AttackScaling {
                amount: 20,
                per_turn: 5,
                cap: 10,
                times: 1,
            }]);
        }
        return None;
    }
    if asc < 18 {
        return None;
    }
    match (id, norm(move_name).as_str()) {
        // 高塔盾手:重砸不再按伤害给格挡,固定给 99(伤害此时已是 38)
        ("spire_shield", "smash") => Some(&[
            EnemyFx::Attack {
                amount: 38,
                times: 1,
            },
            EnemyFx::Block {
                amount: 99,
                scope: Scope::SelfOnly,
            },
        ]),
        // 高塔矛手:灼伤从弃牌堆改成抽牌堆顶(伤害此时已是 6x2)
        ("spire_spear", "burnstrike") => Some(&[
            EnemyFx::Attack {
                amount: 6,
                times: 2,
            },
            EnemyFx::PlayerCard {
                card: "burn",
                spot: crate::core::enemy::CardSpot::DrawTop,
                n: 2,
            },
        ]),
        // 蛇形祭司:召唤两只匕首(参考实现按 [4,1,3,0] 的顺序逐个填空槽,
        // 所以候选槽要留全,只取前两个空位)
        ("reptomancer", "summon") => Some(&[EnemyFx::Summon {
            ids: &["dagger", "dagger"],
            slots: &[4, 1, 3, 0],
            hp_burn: 0,
        }]),
        _ => None,
    }
}

/// 招式的飞升效果列表(asc==0 或没有覆盖时借用基础切片)
pub fn effects(
    id: &str,
    move_name: &str,
    base: &'static [EnemyFx],
    asc: u32,
) -> Cow<'static, [EnemyFx]> {
    if asc == 0 {
        return Cow::Borrowed(base);
    }
    if let Some(rep) = hard_replace(id, move_name, asc) {
        return Cow::Borrowed(rep);
    }
    let Some(m) = merged(id, move_name, asc) else {
        return Cow::Borrowed(base);
    };
    if m.is_empty() {
        return Cow::Borrowed(base);
    }
    let mut v = base.to_vec();
    if let Some(fxs) = m.fx {
        // 同一种效果可能在列表里出现多次(神秘客的治疗/鼓舞对自方与友军各一段),
        // 所以每条覆盖要认领一段还没被认领过的同类效果
        let mut used = vec![false; v.len()];
        for f in fxs {
            let (kind, detail) = asc_key(f);
            let slot = v.iter().enumerate().position(|(i, x)| {
                if used[i] {
                    return false;
                }
                let (k, d) = fx_key(x);
                k == kind && (detail.is_empty() || d == detail)
            });
            match slot {
                Some(i) => {
                    set_amount(&mut v[i], f.amount);
                    used[i] = true;
                }
                None => {
                    v.push(new_fx(f));
                    used.push(true);
                }
            }
        }
    }
    // 攻击数值:改写所有攻击段(含 AttackScaling 之类的基础值)
    if m.damage.is_some() || m.hits.is_some() {
        for x in v.iter_mut() {
            match x {
                EnemyFx::Attack { amount, times }
                | EnemyFx::AttackScaling { amount, times, .. } => {
                    if let Some(d) = m.damage {
                        *amount = d;
                    }
                    if let Some(h) = m.hits {
                        *times = h;
                    }
                }
                EnemyFx::AttackGrowing { amount } | EnemyFx::AttackStabCount { amount } => {
                    if let Some(d) = m.damage {
                        *amount = d;
                    }
                }
                _ => {}
            }
        }
    }
    // 格挡:改写现有格挡段,没有就补一段
    if let Some(b) = m.block {
        let mut found = false;
        for x in v.iter_mut() {
            if let EnemyFx::Block { amount, .. } = x {
                *amount = b;
                found = true;
            }
        }
        if !found {
            v.push(EnemyFx::Block {
                amount: b,
                scope: Scope::SelfOnly,
            });
        }
    }
    Cow::Owned(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::enemies;

    /// 生成数据里的每个敌人 id 与招式名都要能在 ENEMIES 表里对上,
    /// 否则飞升数据会静默失效
    #[test]
    fn every_asc_entry_resolves_to_a_real_enemy_and_move() {
        for (id, _, _) in HP_ASC {
            assert!(enemies::ENEMIES.iter().any(|e| e.id == *id), "血量表里的 {id} 不存在");
        }
        for (id, moves) in MOVE_ASC {
            let def = enemies::ENEMIES
                .iter()
                .find(|e| e.id == *id)
                .unwrap_or_else(|| panic!("招式表里的 {id} 不存在"));
            for (key, _) in *moves {
                assert!(
                    def.moves.iter().any(|m| norm(m.name) == *key),
                    "{id} 没有招 {key}"
                );
            }
        }
    }

    /// 开局的飞升数值必须真的比基础大(语料里就是这样)
    #[test]
    fn asc_hp_is_never_lower_than_base() {
        for def in enemies::ENEMIES {
            let (blo, bhi) = def.hp;
            let (alo, ahi) = hp_range(def, 20);
            assert!(alo >= blo && ahi >= bhi, "{} 的飞升血量反而更低", def.id);
        }
    }

    /// 逐条断言几个等级表上的关键数值
    #[test]
    fn spot_check_ascension_numbers() {
        // A2 普通怪加伤害
        let jaw = enemies::enemy_def("jaw_worm").unwrap();
        let chomp = jaw.moves.iter().find(|m| m.name == "Chomp").unwrap();
        assert_eq!(intent("jaw_worm", "Chomp", chomp.intent, 1), chomp.intent);
        assert_eq!(
            intent("jaw_worm", "Chomp", chomp.intent, 2),
            Intent::Attack {
                damage: 12,
                times: 1
            }
        );
        // A7 血量换档
        assert_eq!(hp_range(jaw, 6), (40, 44));
        assert_eq!(hp_range(jaw, 7), (42, 46));
        // A9 Boss 血量
        let guardian = enemies::enemy_def("the_guardian").unwrap();
        assert_eq!(hp_range(guardian, 8), (240, 240));
        assert_eq!(hp_range(guardian, 9), (250, 250));
        // A19 Boss 招式
        let heart = enemies::enemy_def("corrupt_heart").unwrap();
        let echo = heart.moves.iter().find(|m| m.name == "Echo").unwrap();
        assert_eq!(
            intent("corrupt_heart", "Echo", echo.intent, 4),
            Intent::Attack {
                damage: 45,
                times: 1
            }
        );
        // A17 精英招式:哨卫的 Bolt 改成塞 3 张眩晕
        let sentry = enemies::enemy_def("sentry").unwrap();
        let bolt = sentry.moves.iter().find(|m| m.name == "Bolt").unwrap();
        let fx = effects("sentry", "Bolt", bolt.effects, 18);
        assert!(fx
            .iter()
            .any(|f| matches!(f, EnemyFx::PlayerCard { card: "dazed", n: 3, .. })));
        // 开局状态:A9 守护者的换姿态阈值
        assert_eq!(innate_amount("the_guardian", Status::ModeShift, 30, 8), 30);
        assert_eq!(innate_amount("the_guardian", Status::ModeShift, 30, 9), 35);
        assert_eq!(innate_amount("the_guardian", Status::ModeShift, 30, 19), 40);
        // A19 心脏的无敌上限
        assert_eq!(innate_amount("corrupt_heart", Status::Invincible, 300, 18), 300);
        assert_eq!(innate_amount("corrupt_heart", Status::Invincible, 300, 19), 200);
        // 硬替换:高塔盾手 A18 固定 99 格挡
        let shield = enemies::enemy_def("spire_shield").unwrap();
        let smash = shield.moves.iter().find(|m| m.name == "Smash").unwrap();
        let fx = effects("spire_shield", "Smash", smash.effects, 18);
        assert!(fx
            .iter()
            .any(|f| matches!(f, EnemyFx::Block { amount: 99, .. })));
        assert!(!fx.iter().any(|f| matches!(f, EnemyFx::BlockFromDamage)));
    }

    /// 覆盖里的每个 power 名都要能认出来(否则会落到 Block 兜底上)
    #[test]
    fn every_power_name_is_recognized() {
        for (_, moves) in MOVE_ASC {
            for (_, tiers) in *moves {
                for t in *tiers {
                    if let Some(fxs) = t.fx {
                        for f in fxs {
                            assert_ne!(
                                asc_key(f).0,
                                "other",
                                "没认出来的 power: {}",
                                f.power
                            );
                        }
                    }
                }
            }
        }
    }

    /// 每一条覆盖在 A20 都要真的改出点东西(防止名字对不上导致静默失效)
    #[test]
    fn every_override_changes_something_at_20() {
        for (id, moves) in MOVE_ASC {
            let def = enemies::ENEMIES.iter().find(|e| e.id == *id).unwrap();
            for (key, _) in *moves {
                let m = def.moves.iter().find(|m| norm(m.name) == *key).unwrap();
                let ni = intent(id, m.name, m.intent, 20);
                let ne = effects(id, m.name, m.effects, 20);
                assert!(
                    ni != m.intent || ne.as_ref() != m.effects,
                    "{id}/{key} 在 A20 没有任何变化"
                );
            }
        }
    }

    /// 同一种效果出现多次时,多条覆盖要各认领一段(神秘客的治疗/鼓舞)
    #[test]
    fn duplicate_effects_are_all_patched() {
        // 神秘客鼓舞:自方与友军的力量都从 2 提到 4
        let fx = effects("mystic", "Buff", &[], 17);
        let strengths: Vec<i32> = fx
            .iter()
            .filter_map(|f| match f {
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n,
                    ..
                } => Some(*n),
                _ => None,
            })
            .collect();
        assert!(strengths.iter().all(|n| *n == 4), "两段力量都要到 4: {strengths:?}");
        assert_eq!(strengths.len(), 2);
    }

    /// A0 完全走基础值(飞升不介入)
    #[test]
    fn ascension_zero_is_identity() {
        for def in enemies::ENEMIES {
            assert_eq!(hp_range(def, 0), def.hp);
            for m in def.moves {
                assert_eq!(intent(def.id, m.name, m.intent, 0), m.intent);
                assert!(matches!(
                    effects(def.id, m.name, m.effects, 0),
                    Cow::Borrowed(_)
                ));
            }
        }
    }
}
