// 敌人与遭遇的数据:按 act 分成几个子模块,这里汇总成一张总表.
// 每个敌人一张招式表,AI 决定怎么从招式表里挑(选招规则写在各自的 act 模块里).
pub mod act1;
pub mod act2;
pub mod act34;

use crate::core::enemy::{Encounter, EnemyDef, EnemyKind};

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
    },
    Encounter {
        id: "jaw_worm_solo",
        kind: EnemyKind::Normal,
        enemies: &["jaw_worm"],
    },
    Encounter {
        id: "two_louses",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "green_louse"],
    },
    Encounter {
        id: "small_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_small", "acid_slime_medium"],
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
    },
    Encounter {
        id: "red_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["red_slaver"],
    },
    Encounter {
        id: "exordium_thugs",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "blue_slaver"],
    },
    Encounter {
        id: "exordium_wildlife",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast", "jaw_worm"],
    },
    Encounter {
        id: "blue_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["blue_slaver"],
    },
    Encounter {
        id: "looter_solo",
        kind: EnemyKind::Normal,
        enemies: &["looter"],
    },
    Encounter {
        id: "large_slime",
        kind: EnemyKind::Normal,
        enemies: &["acid_slime_large"],
    },
    Encounter {
        id: "three_louses",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "green_louse", "red_louse"],
    },
    Encounter {
        id: "two_fungi_beasts",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast", "fungi_beast"],
    },
];

pub static ELITES: &[Encounter] = &[
    Encounter {
        id: "gremlin_nob_solo",
        kind: EnemyKind::Elite,
        enemies: &["gremlin_nob"],
    },
    Encounter {
        id: "lagavulin_solo",
        kind: EnemyKind::Elite,
        enemies: &["lagavulin"],
    },
    Encounter {
        id: "three_sentries",
        kind: EnemyKind::Elite,
        enemies: &["sentry", "sentry", "sentry"],
    },
];

pub static BOSSES: &[Encounter] = &[
    Encounter {
        id: "the_guardian",
        kind: EnemyKind::Boss,
        enemies: &["the_guardian"],
    },
    Encounter {
        id: "hexaghost",
        kind: EnemyKind::Boss,
        enemies: &["hexaghost"],
    },
    Encounter {
        id: "slime_boss",
        kind: EnemyKind::Boss,
        enemies: &["slime_boss"],
    },
];

/// 第二章的弱怪池
pub static ACT2_WEAK: &[Encounter] = &[
    Encounter {
        id: "spheric_guardian_solo",
        kind: EnemyKind::Normal,
        enemies: &["spheric_guardian"],
    },
    Encounter {
        id: "chosen_solo",
        kind: EnemyKind::Normal,
        enemies: &["chosen"],
    },
    Encounter {
        id: "shelled_parasite_solo",
        kind: EnemyKind::Normal,
        enemies: &["shelled_parasite"],
    },
    Encounter {
        id: "three_byrds",
        kind: EnemyKind::Normal,
        enemies: &["byrd", "byrd", "byrd"],
    },
    Encounter {
        id: "two_thieves",
        kind: EnemyKind::Normal,
        enemies: &["looter", "mugger"],
    },
];

/// 第二章的普通遭遇
pub static ACT2: &[Encounter] = &[
    Encounter {
        id: "chosen_and_byrds",
        kind: EnemyKind::Normal,
        enemies: &["byrd", "chosen"],
    },
    Encounter {
        id: "sentry_and_sphere",
        kind: EnemyKind::Normal,
        enemies: &["sentry", "spheric_guardian"],
    },
    Encounter {
        id: "cultist_and_chosen",
        kind: EnemyKind::Normal,
        enemies: &["cultist", "chosen"],
    },
    Encounter {
        id: "three_cultists",
        kind: EnemyKind::Normal,
        enemies: &["cultist", "cultist", "cultist"],
    },
    Encounter {
        id: "shelled_parasite_and_fungi",
        kind: EnemyKind::Normal,
        enemies: &["shelled_parasite", "fungi_beast"],
    },
    Encounter {
        id: "snecko_solo",
        kind: EnemyKind::Normal,
        enemies: &["snecko"],
    },
    Encounter {
        id: "snake_plant_solo",
        kind: EnemyKind::Normal,
        enemies: &["snake_plant"],
    },
    Encounter {
        id: "centurion_and_healer",
        kind: EnemyKind::Normal,
        enemies: &["centurion", "mystic"],
    },
];

pub static ACT2_ELITES: &[Encounter] = &[
    Encounter {
        id: "gremlin_leader_gang",
        kind: EnemyKind::Elite,
        enemies: &["mad_gremlin", "sneaky_gremlin", "gremlin_leader"],
    },
    Encounter {
        id: "slavers",
        kind: EnemyKind::Elite,
        enemies: &["blue_slaver", "taskmaster", "red_slaver"],
    },
    Encounter {
        id: "book_of_stabbing_solo",
        kind: EnemyKind::Elite,
        enemies: &["book_of_stabbing"],
    },
];

pub static ACT2_BOSSES: &[Encounter] = &[
    Encounter {
        id: "bronze_automaton",
        kind: EnemyKind::Boss,
        enemies: &["bronze_automaton"],
    },
    Encounter {
        id: "the_collector",
        kind: EnemyKind::Boss,
        enemies: &["the_collector"],
    },
    Encounter {
        id: "the_champ",
        kind: EnemyKind::Boss,
        enemies: &["the_champ"],
    },
];

/// 第三章的弱怪池。三只"形状"的阵容在参考实现里是随机抽的,
/// 这里固定成一手能打出来的组合。
pub static ACT3_WEAK: &[Encounter] = &[
    Encounter {
        id: "three_darklings_weak",
        kind: EnemyKind::Normal,
        enemies: &["darkling", "darkling", "darkling"],
    },
    Encounter {
        id: "orb_walker_solo",
        kind: EnemyKind::Normal,
        enemies: &["orb_walker"],
    },
    Encounter {
        id: "three_shapes",
        kind: EnemyKind::Normal,
        enemies: &["spiker", "repulsor", "exploder"],
    },
];

pub static ACT3: &[Encounter] = &[
    Encounter {
        id: "spire_growth_solo",
        kind: EnemyKind::Normal,
        enemies: &["spire_growth"],
    },
    Encounter {
        id: "transient_solo",
        kind: EnemyKind::Normal,
        enemies: &["transient"],
    },
    Encounter {
        id: "four_shapes",
        kind: EnemyKind::Normal,
        enemies: &["repulsor", "exploder", "spiker", "repulsor"],
    },
    Encounter {
        id: "the_maw_solo",
        kind: EnemyKind::Normal,
        enemies: &["the_maw"],
    },
    Encounter {
        id: "sphere_and_two_shapes",
        kind: EnemyKind::Normal,
        enemies: &["spiker", "repulsor", "spheric_guardian"],
    },
    Encounter {
        id: "jaw_worm_horde",
        kind: EnemyKind::Normal,
        enemies: &["jaw_worm", "jaw_worm", "jaw_worm"],
    },
    Encounter {
        id: "three_darklings",
        kind: EnemyKind::Normal,
        enemies: &["darkling", "darkling", "darkling"],
    },
    Encounter {
        id: "writhing_mass_solo",
        kind: EnemyKind::Normal,
        enemies: &["writhing_mass"],
    },
];

pub static ACT3_ELITES: &[Encounter] = &[
    Encounter {
        id: "giant_head_solo",
        kind: EnemyKind::Elite,
        enemies: &["giant_head"],
    },
    Encounter {
        id: "nemesis_solo",
        kind: EnemyKind::Elite,
        enemies: &["nemesis"],
    },
    Encounter {
        id: "reptomancer_solo",
        kind: EnemyKind::Elite,
        enemies: &["dagger", "reptomancer", "dagger"],
    },
];

pub static ACT3_BOSSES: &[Encounter] = &[
    Encounter {
        id: "awakened_one",
        kind: EnemyKind::Boss,
        enemies: &["cultist", "cultist", "awakened_one"],
    },
    Encounter {
        id: "time_eater",
        kind: EnemyKind::Boss,
        enemies: &["time_eater"],
    },
    Encounter {
        id: "donu_and_deca",
        kind: EnemyKind::Boss,
        enemies: &["deca", "donu"],
    },
];

/// 第四章:一对精英和心脏
pub static ACT4_ELITES: &[Encounter] = &[Encounter {
    id: "shield_and_spear",
    kind: EnemyKind::Elite,
    enemies: &["spire_shield", "spire_spear"],
}];

pub static ACT4_BOSSES: &[Encounter] = &[Encounter {
    id: "the_heart",
    kind: EnemyKind::Boss,
    enemies: &["corrupt_heart"],
}];

/// 只会从分裂里出来的怪(大史莱姆裂开时生成),单列一张表便于直接打到
pub static SPLIT_ONLY: &[Encounter] = &[
    Encounter {
        id: "medium_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_medium", "acid_slime_medium"],
    },
    Encounter {
        id: "boss_split_slimes",
        kind: EnemyKind::Normal,
        enemies: &["spike_slime_large", "acid_slime_large"],
    },
];

/// 只会被召唤出来的小怪:单列一张表,调试可以直接打,不进地图池子
pub static MINIONS: &[Encounter] = &[
    Encounter {
        id: "bronze_orbs",
        kind: EnemyKind::Normal,
        enemies: &["bronze_orb", "bronze_orb"],
    },
    Encounter {
        id: "torch_heads",
        kind: EnemyKind::Normal,
        enemies: &["torch_head", "torch_head"],
    },
    Encounter {
        id: "daggers",
        kind: EnemyKind::Normal,
        enemies: &["dagger", "dagger", "dagger"],
    },
];

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
