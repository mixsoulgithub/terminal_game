// 第一章的敌人与遭遇数据.
// 每个敌人一张招式表,AI 决定怎么从招式表里挑.
use crate::core::enemy::{Ai, Encounter, EnemyDef, EnemyFx, EnemyKind, MoveDef};
use crate::core::status::Status;

pub static ENEMIES: &[EnemyDef] = &[
    EnemyDef {
        id: "jaw_worm",
        name: "Jaw Worm",
        kind: EnemyKind::Normal,
        hp: (40, 44),
        moves: &[
            MoveDef {
                name: "Chomp",
                effects: &[EnemyFx::Attack {
                    amount: 11,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Thrash",
                effects: &[
                    EnemyFx::Attack {
                        amount: 7,
                        times: 1,
                    },
                    EnemyFx::Block { amount: 5 },
                ],
            },
            MoveDef {
                name: "Bellow",
                effects: &[
                    EnemyFx::GainStatus {
                        status: Status::Strength,
                        n: 3,
                    },
                    EnemyFx::Block { amount: 6 },
                ],
            },
        ],
        ai: Ai::Random {
            weights: &[25, 30, 45],
            no_repeat: true,
        },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "cultist",
        name: "Cultist",
        kind: EnemyKind::Normal,
        hp: (48, 54),
        moves: &[
            MoveDef {
                name: "Incantation",
                effects: &[EnemyFx::GainStatus {
                    status: Status::Ritual,
                    n: 3,
                }],
            },
            MoveDef {
                name: "Dark Strike",
                effects: &[EnemyFx::Attack {
                    amount: 6,
                    times: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "red_louse",
        name: "Red Louse",
        kind: EnemyKind::Normal,
        hp: (10, 15),
        moves: &[
            MoveDef {
                name: "Bite",
                effects: &[EnemyFx::Attack {
                    amount: 6,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Grow",
                effects: &[EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                }],
            },
        ],
        ai: Ai::Random {
            weights: &[75, 25],
            no_repeat: true,
        },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "green_louse",
        name: "Green Louse",
        kind: EnemyKind::Normal,
        hp: (11, 16),
        moves: &[
            MoveDef {
                name: "Bite",
                effects: &[EnemyFx::Attack {
                    amount: 6,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Spit",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 2,
                }],
            },
        ],
        ai: Ai::Random {
            weights: &[75, 25],
            no_repeat: true,
        },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "acid_slime_small",
        name: "Acid Slime (S)",
        kind: EnemyKind::Normal,
        hp: (8, 12),
        moves: &[
            MoveDef {
                name: "Tackle",
                effects: &[EnemyFx::Attack {
                    amount: 3,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Lick",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "spike_slime_small",
        name: "Spike Slime (S)",
        kind: EnemyKind::Normal,
        hp: (10, 14),
        moves: &[MoveDef {
            name: "Tackle",
            effects: &[EnemyFx::Attack {
                amount: 5,
                times: 1,
            }],
        }],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "acid_slime_medium",
        name: "Acid Slime (M)",
        kind: EnemyKind::Normal,
        hp: (28, 32),
        moves: &[
            MoveDef {
                name: "Corrosive Spit",
                effects: &[EnemyFx::Attack {
                    amount: 7,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Tackle",
                effects: &[EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Lick",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "spike_slime_medium",
        name: "Spike Slime (M)",
        kind: EnemyKind::Normal,
        hp: (28, 32),
        moves: &[
            MoveDef {
                name: "Flame Tackle",
                effects: &[EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Tackle",
                effects: &[EnemyFx::Attack {
                    amount: 8,
                    times: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "fungi_beast",
        name: "Fungi Beast",
        kind: EnemyKind::Normal,
        hp: (22, 28),
        moves: &[MoveDef {
            name: "Bite",
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 1,
            }],
        }],
        ai: Ai::Cycle,
        innate: &[],
        // 孢子云:死亡时让玩家中毒
        on_death: &[EnemyFx::PlayerStatus {
            status: Status::Vulnerable,
            n: 2,
        }],
    },
    EnemyDef {
        id: "looter",
        name: "Looter",
        kind: EnemyKind::Normal,
        hp: (44, 48),
        moves: &[
            MoveDef {
                name: "Mug",
                effects: &[EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Lunge",
                effects: &[
                    EnemyFx::Attack {
                        amount: 12,
                        times: 1,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Weak,
                        n: 2,
                    },
                ],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "blue_slaver",
        name: "Blue Slaver",
        kind: EnemyKind::Normal,
        hp: (46, 50),
        moves: &[
            MoveDef {
                name: "Stab",
                effects: &[EnemyFx::Attack {
                    amount: 12,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Rake",
                effects: &[
                    EnemyFx::Attack {
                        amount: 7,
                        times: 1,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Weak,
                        n: 1,
                    },
                ],
            },
            MoveDef {
                name: "Entangle",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Entangled,
                    n: 1,
                }],
            },
        ],
        ai: Ai::Random {
            weights: &[40, 30, 30],
            no_repeat: true,
        },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "red_slaver",
        name: "Red Slaver",
        kind: EnemyKind::Normal,
        hp: (46, 50),
        moves: &[
            MoveDef {
                name: "Stab",
                effects: &[EnemyFx::Attack {
                    amount: 13,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Scrape",
                effects: &[
                    EnemyFx::Attack {
                        amount: 8,
                        times: 1,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Frail,
                        n: 1,
                    },
                ],
            },
            MoveDef {
                name: "Entangle",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Entangled,
                    n: 1,
                }],
            },
        ],
        ai: Ai::Random {
            weights: &[40, 30, 30],
            no_repeat: true,
        },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "gremlin_nob",
        name: "Gremlin Nob",
        kind: EnemyKind::Elite,
        hp: (82, 86),
        moves: &[
            MoveDef {
                name: "Bellow",
                effects: &[EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                }],
            },
            MoveDef {
                name: "Rush",
                effects: &[EnemyFx::Attack {
                    amount: 14,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Skull Bash",
                effects: &[
                    EnemyFx::Attack {
                        amount: 6,
                        times: 1,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Vulnerable,
                        n: 2,
                    },
                ],
            },
        ],
        ai: Ai::Random {
            weights: &[25, 45, 30],
            no_repeat: true,
        },
        // 狂怒:玩家每打出一张技能牌就加力量
        innate: &[(Status::Enrage, 2)],
        on_death: &[],
    },
    EnemyDef {
        id: "lagavulin",
        name: "Lagavulin",
        kind: EnemyKind::Elite,
        hp: (109, 111),
        moves: &[
            // 睡眠:效果被忽略,只用于占位
            MoveDef {
                name: "Sleep",
                effects: &[EnemyFx::Block { amount: 8 }],
            },
            MoveDef {
                name: "Attack",
                effects: &[EnemyFx::Attack {
                    amount: 18,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Siphon Soul",
                effects: &[
                    EnemyFx::PlayerStatus {
                        status: Status::Strength,
                        n: -1,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Dexterity,
                        n: -1,
                    },
                ],
            },
        ],
        ai: Ai::Sleep { turns: 3, wake: 1 },
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "sentry",
        name: "Sentry",
        kind: EnemyKind::Elite,
        hp: (38, 42),
        moves: &[
            MoveDef {
                name: "Beam",
                effects: &[EnemyFx::Attack {
                    amount: 9,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Bolt",
                effects: &[EnemyFx::Attack {
                    amount: 9,
                    times: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "the_guardian",
        name: "The Guardian",
        kind: EnemyKind::Boss,
        hp: (240, 240),
        moves: &[
            MoveDef {
                name: "Whirlwind",
                effects: &[EnemyFx::Attack {
                    amount: 5,
                    times: 3,
                }],
            },
            MoveDef {
                name: "Fierce Bash",
                effects: &[EnemyFx::Attack {
                    amount: 32,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Vent Steam",
                effects: &[
                    EnemyFx::PlayerStatus {
                        status: Status::Weak,
                        n: 2,
                    },
                    EnemyFx::PlayerStatus {
                        status: Status::Vulnerable,
                        n: 2,
                    },
                ],
            },
            MoveDef {
                name: "Defensive Mode",
                effects: &[
                    EnemyFx::Block { amount: 20 },
                    EnemyFx::GainStatus {
                        status: Status::Thorns,
                        n: 3,
                    },
                ],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "hexaghost",
        name: "Hexaghost",
        kind: EnemyKind::Boss,
        hp: (250, 250),
        moves: &[
            MoveDef {
                name: "Activate",
                effects: &[EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 2,
                }],
            },
            MoveDef {
                name: "Divider",
                effects: &[EnemyFx::Attack {
                    amount: 6,
                    times: 6,
                }],
            },
            MoveDef {
                name: "Sear",
                effects: &[EnemyFx::Attack {
                    amount: 6,
                    times: 1,
                }],
            },
            MoveDef {
                name: "Tackle",
                effects: &[EnemyFx::Attack {
                    amount: 5,
                    times: 2,
                }],
            },
            MoveDef {
                name: "Inferno",
                effects: &[EnemyFx::Attack {
                    amount: 2,
                    times: 6,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
    EnemyDef {
        id: "slime_boss",
        name: "Slime Boss",
        kind: EnemyKind::Boss,
        hp: (140, 140),
        moves: &[
            MoveDef {
                name: "Goop Spray",
                effects: &[EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 2,
                }],
            },
            MoveDef {
                name: "Preparing",
                effects: &[EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                }],
            },
            MoveDef {
                name: "Slam",
                effects: &[EnemyFx::Attack {
                    amount: 35,
                    times: 1,
                }],
            },
        ],
        ai: Ai::Cycle,
        innate: &[],
        on_death: &[],
    },
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
        enemies: &["acid_slime_small", "spike_slime_small"],
    },
];

pub static ENCOUNTERS: &[Encounter] = &[
    Encounter {
        id: "three_louses",
        kind: EnemyKind::Normal,
        enemies: &["red_louse", "green_louse", "red_louse"],
    },
    Encounter {
        id: "medium_slimes",
        kind: EnemyKind::Normal,
        enemies: &["acid_slime_medium", "spike_slime_medium"],
    },
    Encounter {
        id: "blue_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["blue_slaver"],
    },
    Encounter {
        id: "red_slaver_solo",
        kind: EnemyKind::Normal,
        enemies: &["red_slaver"],
    },
    Encounter {
        id: "looter_solo",
        kind: EnemyKind::Normal,
        enemies: &["looter"],
    },
    Encounter {
        id: "fungi_beast_solo",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast"],
    },
    Encounter {
        id: "looters",
        kind: EnemyKind::Normal,
        enemies: &["looter", "looter"],
    },
    Encounter {
        id: "slaver_and_slime",
        kind: EnemyKind::Normal,
        enemies: &["blue_slaver", "acid_slime_small"],
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
    ENCOUNTERS
        .iter()
        .chain(ELITES.iter())
        .chain(BOSSES.iter())
        .chain(ENCOUNTERS_WEAK.iter())
        .find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::enemy::Intent;

    /// 所有遭遇表,附带该表应有的敌人类别
    fn tables() -> [(&'static [Encounter], EnemyKind); 4] {
        [
            (ENCOUNTERS_WEAK, EnemyKind::Normal),
            (ENCOUNTERS, EnemyKind::Normal),
            (ELITES, EnemyKind::Elite),
            (BOSSES, EnemyKind::Boss),
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
        let mut ids: Vec<&str> = tables()
            .iter()
            .flat_map(|(g, _)| g.iter().map(|e| e.id))
            .collect();
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
            if let Ai::Random { weights, .. } = e.ai {
                assert_eq!(
                    weights.len(),
                    e.moves.len(),
                    "{} weight count does not match moves",
                    e.id
                );
            }
            if let Ai::Sleep { wake, .. } = e.ai {
                assert!(
                    wake < e.moves.len(),
                    "{} wake move index out of range",
                    e.id
                );
            }
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
    fn every_move_yields_a_concrete_intent() {
        for e in ENEMIES {
            for m in e.moves {
                assert_ne!(
                    m.intent(),
                    Intent::Unknown,
                    "{} move {} has no concrete intent",
                    e.id,
                    m.name
                );
            }
        }
    }

    #[test]
    fn some_encounter_has_multiple_enemies() {
        let multi = ENCOUNTERS
            .iter()
            .chain(ENCOUNTERS_WEAK.iter())
            .chain(ELITES.iter())
            .chain(BOSSES.iter())
            .find(|e| e.enemies.len() >= 2);
        assert!(multi.is_some(), "need at least one multi-enemy encounter");
    }

    #[test]
    fn sleeper_enemy_is_reachable_from_an_encounter() {
        let sleeper = ENEMIES
            .iter()
            .find(|e| matches!(e.ai, Ai::Sleep { .. }))
            .expect("need at least one sleeping enemy");
        assert!(
            matches!(
                sleeper.moves[0].intent(),
                Intent::Defend | Intent::Buff | Intent::Debuff
            ),
            "sleeping enemy must not open with an attack"
        );
        let reachable = ENCOUNTERS
            .iter()
            .chain(ENCOUNTERS_WEAK.iter())
            .chain(ELITES.iter())
            .chain(BOSSES.iter())
            .any(|enc| enc.enemies.contains(&sleeper.id));
        assert!(
            reachable,
            "sleeping enemy {} is not in any encounter",
            sleeper.id
        );
    }

    #[test]
    fn lookup_helpers_resolve_known_ids() {
        assert_eq!(enemy_def("jaw_worm").unwrap().name, "Jaw Worm");
        assert_eq!(enemy_def_or_panic("the_guardian").kind, EnemyKind::Boss);
        assert!(enemy_def("no_such_enemy").is_none());
        for id in [
            "cultist_solo",
            "three_louses",
            "lagavulin_solo",
            "hexaghost",
        ] {
            assert!(encounter_def(id).is_some(), "no such encounter {id}");
        }
        assert!(encounter_def("no_such_encounter").is_none());
    }
}
