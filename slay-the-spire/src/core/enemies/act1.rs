// 第一章的怪:招式、数值与选招规则.
// 数值取参考实现的"无飞升"基础值,选招规则按 getMove 的判定逐条照搬.
use crate::core::enemy::{
    spawn_default, EnemyDef, EnemyFx, EnemyKind, Intent, MoveDef, PickCtx, Scope, Special,
    SpawnCtx,
};
use crate::core::status::Status;

// ---- 共用的选招工具:招式表下标 ----
// 每只怪的招式下标都写死成常量,选招函数读起来才像参考实现里的名字.

/// 虱子/巨口之类"开局掷一次固定伤害"的怪在构造时掷
fn louse_spawn(ctx: &mut SpawnCtx) {
    ctx.state.rolled = ctx.rng.range_inclusive(5, 7);
    let curl = ctx.rng.range_inclusive(3, 7);
    ctx.statuses.add(Status::CurlUp, curl);
}

// ============================ 普通怪 ============================

pub const CULTIST: EnemyDef = EnemyDef {
    id: "cultist",
    name: "Cultist",
    kind: EnemyKind::Normal,
    hp: (48, 54),
    moves: &[
        MoveDef {
            name: "Incantation",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Ritual,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Dark Strike",
            intent: Intent::Attack {
                damage: 6,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 1,
            }],
        },
    ],
    pick: pick_cultist,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 第一回合必充能,之后一直黑暗打击
fn pick_cultist(ctx: &mut PickCtx) -> usize {
    if ctx.first_turn() {
        0
    } else {
        1
    }
}

pub const JAW_WORM: EnemyDef = EnemyDef {
    id: "jaw_worm",
    name: "Jaw Worm",
    kind: EnemyKind::Normal,
    hp: (40, 44),
    moves: &[
        MoveDef {
            name: "Chomp",
            intent: Intent::Attack {
                damage: 11,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 11,
                times: 1,
            }],
        },
        MoveDef {
            name: "Thrash",
            intent: Intent::AttackDefend {
                damage: 7,
                times: 1,
                block: 5,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 7,
                    times: 1,
                },
                EnemyFx::Block {
                    amount: 5,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Bellow",
            intent: Intent::DefendBuff { block: 6 },
            effects: &[
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::Block {
                    amount: 6,
                    scope: Scope::SelfOnly,
                },
            ],
        },
    ],
    pick: pick_jaw_worm,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 第一回合咬一口;之后按 25/30/45 的分布掷,并且不许连续重复
fn pick_jaw_worm(ctx: &mut PickCtx) -> usize {
    const CHOMP: usize = 0;
    const THRASH: usize = 1;
    const BELLOW: usize = 2;
    if ctx.first_turn() {
        return CHOMP;
    }
    let roll = ctx.roll();
    if roll < 25 {
        if ctx.last_is(CHOMP) {
            // 9/16
            if ctx.flip(9, 16) {
                BELLOW
            } else {
                THRASH
            }
        } else {
            CHOMP
        }
    } else if roll < 55 {
        if ctx.last_two_is(THRASH) {
            if ctx.flip(357, 1000) {
                CHOMP
            } else {
                BELLOW
            }
        } else {
            THRASH
        }
    } else if ctx.last_is(BELLOW) {
        if ctx.flip(416, 1000) {
            CHOMP
        } else {
            THRASH
        }
    } else {
        BELLOW
    }
}

pub const RED_LOUSE: EnemyDef = EnemyDef {
    id: "red_louse",
    name: "Red Louse",
    kind: EnemyKind::Normal,
    hp: (10, 15),
    moves: &[
        MoveDef {
            // 显示值取区间下限,实际伤害是开局掷出来的那一个
            name: "Bite",
            intent: Intent::Attack {
                damage: 5,
                times: 1,
            },
            effects: &[EnemyFx::AttackRolled { times: 1 }],
        },
        MoveDef {
            name: "Grow",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
    ],
    pick: pick_louse,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: louse_spawn,
};

pub const GREEN_LOUSE: EnemyDef = EnemyDef {
    id: "green_louse",
    name: "Green Louse",
    kind: EnemyKind::Normal,
    hp: (11, 17),
    moves: &[
        MoveDef {
            name: "Bite",
            intent: Intent::Attack {
                damage: 5,
                times: 1,
            },
            effects: &[EnemyFx::AttackRolled { times: 1 }],
        },
        MoveDef {
            name: "Spit Web",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 2,
            }],
        },
    ],
    pick: pick_louse,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: louse_spawn,
};

/// 咬/缠蛛丝各 25%;不许连着三下咬,也不许连着三下特殊动作
fn pick_louse(ctx: &mut PickCtx) -> usize {
    const BITE: usize = 0;
    const SPECIAL: usize = 1;
    let roll = ctx.roll();
    if roll < 25 {
        if ctx.last_two_is(SPECIAL) {
            BITE
        } else {
            SPECIAL
        }
    } else if ctx.last_two_is(BITE) {
        SPECIAL
    } else {
        BITE
    }
}

pub const ACID_SLIME_SMALL: EnemyDef = EnemyDef {
    id: "acid_slime_small",
    name: "Acid Slime (S)",
    kind: EnemyKind::Normal,
    hp: (8, 12),
    moves: &[
        MoveDef {
            name: "Lick",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 1,
            }],
        },
        MoveDef {
            name: "Tackle",
            intent: Intent::Attack {
                damage: 3,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 3,
                times: 1,
            }],
        },
    ],
    pick: pick_acid_slime_small,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 第一招 50/50,之后严格交替
fn pick_acid_slime_small(ctx: &mut PickCtx) -> usize {
    const LICK: usize = 0;
    const TACKLE: usize = 1;
    if ctx.first_turn() {
        return if ctx.flip(1, 2) { TACKLE } else { LICK };
    }
    if ctx.last_is(LICK) {
        TACKLE
    } else {
        LICK
    }
}

pub const ACID_SLIME_MEDIUM: EnemyDef = EnemyDef {
    id: "acid_slime_medium",
    name: "Acid Slime (M)",
    kind: EnemyKind::Normal,
    hp: (28, 32),
    moves: &[
        MoveDef {
            name: "Corrosive Spit",
            intent: Intent::AttackDebuff {
                damage: 7,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 7,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "slimed",
                    spot: crate::core::enemy::CardSpot::Discard,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Tackle",
            intent: Intent::Attack {
                damage: 10,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 10,
                times: 1,
            }],
        },
        MoveDef {
            name: "Lick",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 1,
            }],
        },
    ],
    pick: pick_acid_slime,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 中/大酸液史莱姆共用:30/40/30,吐口水不许连三,撞不许连二,舔不许连三
fn pick_acid_slime(ctx: &mut PickCtx) -> usize {
    const SPIT: usize = 0;
    const TACKLE: usize = 1;
    const LICK: usize = 2;
    let roll = ctx.roll();
    if roll < 30 {
        if ctx.last_two_is(SPIT) {
            if ctx.flip(1, 2) {
                TACKLE
            } else {
                LICK
            }
        } else {
            SPIT
        }
    } else if roll < 70 {
        if ctx.last_is(TACKLE) {
            if ctx.flip(2, 5) {
                SPIT
            } else {
                LICK
            }
        } else {
            TACKLE
        }
    } else if ctx.last_two_is(LICK) {
        if ctx.flip(2, 5) {
            SPIT
        } else {
            TACKLE
        }
    } else {
        LICK
    }
}

pub const ACID_SLIME_LARGE: EnemyDef = EnemyDef {
    id: "acid_slime_large",
    name: "Acid Slime (L)",
    kind: EnemyKind::Normal,
    hp: (65, 69),
    moves: &[
        MoveDef {
            name: "Corrosive Spit",
            intent: Intent::AttackDebuff {
                damage: 11,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 11,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "slimed",
                    spot: crate::core::enemy::CardSpot::Discard,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Tackle",
            intent: Intent::Attack {
                damage: 16,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 16,
                times: 1,
            }],
        },
        MoveDef {
            name: "Lick",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 2,
            }],
        },
        MoveDef {
            name: "Split",
            intent: Intent::Unknown,
            effects: &[EnemyFx::Split],
        },
    ],
    pick: pick_acid_slime,
    innate: &[(Status::Split, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Split {
        a: "acid_slime_medium",
        b: "acid_slime_medium",
    },
    spawn: spawn_default,
};

pub const SPIKE_SLIME_SMALL: EnemyDef = EnemyDef {
    id: "spike_slime_small",
    name: "Spike Slime (S)",
    kind: EnemyKind::Normal,
    hp: (10, 14),
    moves: &[MoveDef {
        name: "Tackle",
        intent: Intent::Attack {
            damage: 5,
            times: 1,
        },
        effects: &[EnemyFx::Attack {
            amount: 5,
            times: 1,
        }],
    }],
    pick: pick_spike_slime_small,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

fn pick_spike_slime_small(_ctx: &mut PickCtx) -> usize {
    0
}

pub const SPIKE_SLIME_MEDIUM: EnemyDef = EnemyDef {
    id: "spike_slime_medium",
    name: "Spike Slime (M)",
    kind: EnemyKind::Normal,
    hp: (28, 32),
    moves: &[
        MoveDef {
            name: "Flame Tackle",
            intent: Intent::AttackDebuff {
                damage: 8,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 8,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "slimed",
                    spot: crate::core::enemy::CardSpot::Discard,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Lick",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Frail,
                n: 1,
            }],
        },
    ],
    pick: pick_spike_slime,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 中/大尖刺史莱姆共用:30% 冲撞,其余舔;两种都不许连三
fn pick_spike_slime(ctx: &mut PickCtx) -> usize {
    const FLAME: usize = 0;
    const LICK: usize = 1;
    let roll = ctx.roll();
    if roll < 30 {
        if ctx.last_two_is(FLAME) {
            LICK
        } else {
            FLAME
        }
    } else if ctx.last_two_is(LICK) {
        FLAME
    } else {
        LICK
    }
}

pub const SPIKE_SLIME_LARGE: EnemyDef = EnemyDef {
    id: "spike_slime_large",
    name: "Spike Slime (L)",
    kind: EnemyKind::Normal,
    hp: (64, 70),
    moves: &[
        MoveDef {
            name: "Flame Tackle",
            intent: Intent::AttackDebuff {
                damage: 16,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 16,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "slimed",
                    spot: crate::core::enemy::CardSpot::Discard,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Lick",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Frail,
                n: 2,
            }],
        },
        MoveDef {
            name: "Split",
            intent: Intent::Unknown,
            effects: &[EnemyFx::Split],
        },
    ],
    pick: pick_spike_slime,
    innate: &[(Status::Split, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Split {
        a: "spike_slime_medium",
        b: "spike_slime_medium",
    },
    spawn: spawn_default,
};

pub const MAD_GREMLIN: EnemyDef = EnemyDef {
    id: "mad_gremlin",
    name: "Mad Gremlin",
    kind: EnemyKind::Normal,
    hp: (20, 24),
    moves: &[MoveDef {
        name: "Scratch",
        intent: Intent::Attack {
            damage: 4,
            times: 1,
        },
        effects: &[EnemyFx::Attack {
            amount: 4,
            times: 1,
        }],
    }],
    pick: pick_only_zero,
    innate: &[(Status::Anger, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

pub const SNEAKY_GREMLIN: EnemyDef = EnemyDef {
    id: "sneaky_gremlin",
    name: "Sneaky Gremlin",
    kind: EnemyKind::Normal,
    hp: (10, 14),
    moves: &[MoveDef {
        name: "Puncture",
        intent: Intent::Attack {
            damage: 9,
            times: 1,
        },
        effects: &[EnemyFx::Attack {
            amount: 9,
            times: 1,
        }],
    }],
    pick: pick_only_zero,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

pub const FAT_GREMLIN: EnemyDef = EnemyDef {
    id: "fat_gremlin",
    name: "Fat Gremlin",
    kind: EnemyKind::Normal,
    hp: (13, 17),
    moves: &[MoveDef {
        name: "Smash",
        intent: Intent::AttackDebuff {
            damage: 4,
            times: 1,
        },
        effects: &[
            EnemyFx::Attack {
                amount: 4,
                times: 1,
            },
            EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 1,
            },
        ],
    }],
    pick: pick_only_zero,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 只有一招的怪:永远用第一招
fn pick_only_zero(_ctx: &mut PickCtx) -> usize {
    0
}

pub const SHIELD_GREMLIN: EnemyDef = EnemyDef {
    id: "shield_gremlin",
    name: "Shield Gremlin",
    kind: EnemyKind::Normal,
    hp: (12, 15),
    moves: &[
        MoveDef {
            name: "Protect",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 7,
                scope: Scope::RandomOne,
            }],
        },
        MoveDef {
            name: "Shield Bash",
            intent: Intent::Attack {
                damage: 6,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 1,
            }],
        },
    ],
    pick: pick_shield_gremlin,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 有人就给别人套盾;只剩自己就一直盾击
fn pick_shield_gremlin(ctx: &mut PickCtx) -> usize {
    const PROTECT: usize = 0;
    const BASH: usize = 1;
    if ctx.first_turn() {
        return PROTECT;
    }
    if ctx.last_is(BASH) {
        return BASH;
    }
    if ctx.alive() <= 1 {
        BASH
    } else {
        PROTECT
    }
}

pub const GREMLIN_WIZARD: EnemyDef = EnemyDef {
    id: "gremlin_wizard",
    name: "Gremlin Wizard",
    kind: EnemyKind::Normal,
    hp: (21, 25),
    moves: &[
        MoveDef {
            name: "Charging",
            intent: Intent::Unknown,
            effects: &[EnemyFx::Charge],
        },
        MoveDef {
            name: "Ultimate Blast",
            intent: Intent::Attack {
                damage: 25,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 25,
                times: 1,
            }],
        },
    ],
    pick: pick_gremlin_wizard,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 充能两次之后放大招,循环往复
fn pick_gremlin_wizard(ctx: &mut PickCtx) -> usize {
    const CHARGING: usize = 0;
    const BLAST: usize = 1;
    if ctx.first_turn() {
        ctx.state.charge = 1;
        return CHARGING;
    }
    if ctx.last_is(BLAST) {
        ctx.state.charge = 0;
        return CHARGING;
    }
    if ctx.state.charge >= 3 {
        BLAST
    } else {
        CHARGING
    }
}

pub const LOOTER: EnemyDef = EnemyDef {
    id: "looter",
    name: "Looter",
    kind: EnemyKind::Normal,
    hp: (44, 48),
    moves: &[
        MoveDef {
            name: "Mug",
            intent: Intent::Attack {
                damage: 10,
                times: 1,
            },
            effects: &[
                EnemyFx::StealGold { n: 15 },
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
            ],
        },
        MoveDef {
            name: "Lunge",
            intent: Intent::Attack {
                damage: 12,
                times: 1,
            },
            effects: &[
                EnemyFx::StealGold { n: 15 },
                EnemyFx::Attack {
                    amount: 12,
                    times: 1,
                },
            ],
        },
        MoveDef {
            name: "Smoke Bomb",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 6,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Escape",
            intent: Intent::Escape,
            effects: &[EnemyFx::Escape],
        },
    ],
    pick: pick_thief,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 抢两回合,然后霰雾弹跑路
fn pick_thief(ctx: &mut PickCtx) -> usize {
    const MUG: usize = 0;
    const LUNGE: usize = 1;
    const SMOKE: usize = 2;
    const ESCAPE: usize = 3;
    if ctx.first_turn() {
        return MUG;
    }
    if ctx.last_is(MUG) {
        if ctx.turn() == 1 {
            return MUG;
        }
        return if ctx.flip(1, 2) { SMOKE } else { LUNGE };
    }
    if ctx.last_is(LUNGE) {
        return SMOKE;
    }
    if ctx.last_is(SMOKE) {
        return ESCAPE;
    }
    MUG
}

pub const FUNGI_BEAST: EnemyDef = EnemyDef {
    id: "fungi_beast",
    name: "Fungi Beast",
    kind: EnemyKind::Normal,
    hp: (22, 28),
    moves: &[
        MoveDef {
            name: "Bite",
            intent: Intent::Attack {
                damage: 6,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 1,
            }],
        },
        MoveDef {
            name: "Grow",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
    ],
    pick: pick_fungi_beast,
    innate: &[(Status::SporeCloud, 2)],
    start_block: 0,
    // 孢子云:死亡时让玩家中毒
    on_death: &[EnemyFx::PlayerStatus {
        status: Status::Vulnerable,
        n: 2,
    }],
    special: Special::None,
    spawn: spawn_default,
};

/// 60% 咬,40% 生长;咬不许连三,生长不许连二
fn pick_fungi_beast(ctx: &mut PickCtx) -> usize {
    const BITE: usize = 0;
    const GROW: usize = 1;
    let roll = ctx.roll();
    if roll < 60 {
        if ctx.last_two_is(BITE) {
            GROW
        } else {
            BITE
        }
    } else if ctx.last_is(GROW) {
        BITE
    } else {
        GROW
    }
}

pub const BLUE_SLAVER: EnemyDef = EnemyDef {
    id: "blue_slaver",
    name: "Blue Slaver",
    kind: EnemyKind::Normal,
    hp: (46, 50),
    moves: &[
        MoveDef {
            name: "Stab",
            intent: Intent::Attack {
                damage: 12,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 12,
                times: 1,
            }],
        },
        MoveDef {
            name: "Rake",
            intent: Intent::AttackDebuff {
                damage: 7,
                times: 1,
            },
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
    ],
    pick: pick_blue_slaver,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 60% 捅一刀,40% 耙;两种都不许连三
fn pick_blue_slaver(ctx: &mut PickCtx) -> usize {
    const STAB: usize = 0;
    const RAKE: usize = 1;
    let roll = ctx.roll();
    if roll >= 40 && !ctx.last_two_is(STAB) {
        STAB
    } else if !ctx.last_two_is(RAKE) {
        RAKE
    } else {
        STAB
    }
}

pub const RED_SLAVER: EnemyDef = EnemyDef {
    id: "red_slaver",
    name: "Red Slaver",
    kind: EnemyKind::Normal,
    hp: (46, 50),
    moves: &[
        MoveDef {
            name: "Stab",
            intent: Intent::Attack {
                damage: 13,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 13,
                times: 1,
            }],
        },
        MoveDef {
            name: "Scrape",
            intent: Intent::AttackDebuff {
                damage: 8,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 8,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Entangle",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Entangled,
                n: 1,
            }],
        },
    ],
    pick: pick_red_slaver,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开场捅一刀;缠斗每场只用一次,其余时候 [耙, 耙, 捅] 循环
fn pick_red_slaver(ctx: &mut PickCtx) -> usize {
    const STAB: usize = 0;
    const SCRAPE: usize = 1;
    const ENTANGLE: usize = 2;
    if ctx.first_turn() {
        return STAB;
    }
    let roll = ctx.roll();
    if roll >= 75 && !ctx.state.entangle_used {
        ctx.state.entangle_used = true;
        return ENTANGLE;
    }
    if roll >= 50 && ctx.state.entangle_used && !ctx.last_two_is(STAB) {
        return STAB;
    }
    if !ctx.last_two_is(SCRAPE) {
        SCRAPE
    } else {
        STAB
    }
}

// ============================ 精英 ============================

pub const GREMLIN_NOB: EnemyDef = EnemyDef {
    id: "gremlin_nob",
    name: "Gremlin Nob",
    kind: EnemyKind::Elite,
    hp: (82, 86),
    moves: &[
        MoveDef {
            name: "Bellow",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Enrage,
                n: 2,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Rush",
            intent: Intent::Attack {
                damage: 14,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 14,
                times: 1,
            }],
        },
        MoveDef {
            name: "Skull Bash",
            intent: Intent::AttackDebuff {
                damage: 6,
                times: 1,
            },
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
    pick: pick_gremlin_nob,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开场吼一嗓子(拿到狂怒),之后 1/3 头槌、2/3 冲锋,冲锋不许连三
fn pick_gremlin_nob(ctx: &mut PickCtx) -> usize {
    const BELLOW: usize = 0;
    const RUSH: usize = 1;
    const SKULL: usize = 2;
    if ctx.first_turn() {
        return BELLOW;
    }
    let roll = ctx.roll();
    if roll < 33 || ctx.last_two_is(RUSH) {
        SKULL
    } else {
        RUSH
    }
}

pub const LAGAVULIN: EnemyDef = EnemyDef {
    id: "lagavulin",
    name: "Lagavulin",
    kind: EnemyKind::Elite,
    hp: (109, 111),
    moves: &[
        MoveDef {
            name: "Sleep",
            intent: Intent::Sleep,
            effects: &[EnemyFx::WakeUp { at_turn: 3 }],
        },
        MoveDef {
            name: "Attack",
            intent: Intent::Attack {
                damage: 18,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 18,
                times: 1,
            }],
        },
        MoveDef {
            name: "Siphon Soul",
            intent: Intent::StrongDebuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Dexterity,
                    n: -1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Strength,
                    n: -1,
                },
            ],
        },
    ],
    pick: pick_lagavulin,
    // 睡着的拉格文有 8 点金属化与 8 点格挡;挨打会醒,睡满三回合也会醒
    innate: &[(Status::Asleep, 1), (Status::Metallicize, 8)],
    start_block: 8,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 睡着就继续睡;醒了之后 [攻击, 攻击, 汲魂] 循环
fn pick_lagavulin(ctx: &mut PickCtx) -> usize {
    const SLEEP: usize = 0;
    const ATTACK: usize = 1;
    const SIPHON: usize = 2;
    if ctx.first_turn() {
        return if ctx.has_status(Status::Asleep) {
            SLEEP
        } else {
            SIPHON
        };
    }
    if ctx.last_is(SLEEP) {
        return if ctx.has_status(Status::Asleep) {
            SLEEP
        } else {
            ATTACK
        };
    }
    if ctx.last_is(ATTACK) {
        return if ctx.last_two_is(ATTACK) { SIPHON } else { ATTACK };
    }
    ATTACK
}

pub const SENTRY: EnemyDef = EnemyDef {
    id: "sentry",
    name: "Sentry",
    kind: EnemyKind::Elite,
    hp: (38, 42),
    moves: &[
        MoveDef {
            name: "Bolt",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerCard {
                card: "dazed",
                spot: crate::core::enemy::CardSpot::DrawShuffle,
                n: 2,
            }],
        },
        MoveDef {
            name: "Beam",
            intent: Intent::Attack {
                damage: 9,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 9,
                times: 1,
            }],
        },
    ],
    pick: pick_sentry,
    innate: &[(Status::Artifact, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开场按站位决定先放螺栓还是射线,之后严格交替
fn pick_sentry(ctx: &mut PickCtx) -> usize {
    const BOLT: usize = 0;
    const BEAM: usize = 1;
    if ctx.first_turn() {
        return if ctx.idx % 2 == 0 { BOLT } else { BEAM };
    }
    if ctx.last_is(BOLT) {
        BEAM
    } else {
        BOLT
    }
}

// ============================ Boss ============================

pub const SLIME_BOSS: EnemyDef = EnemyDef {
    id: "slime_boss",
    name: "Slime Boss",
    kind: EnemyKind::Boss,
    hp: (140, 140),
    moves: &[
        MoveDef {
            name: "Goop Spray",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerCard {
                card: "slimed",
                spot: crate::core::enemy::CardSpot::Discard,
                n: 3,
            }],
        },
        MoveDef {
            name: "Preparing",
            intent: Intent::Unknown,
            effects: &[],
        },
        MoveDef {
            name: "Slam",
            intent: Intent::Attack {
                damage: 35,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 35,
                times: 1,
            }],
        },
        MoveDef {
            name: "Split",
            intent: Intent::Unknown,
            effects: &[EnemyFx::Split],
        },
    ],
    pick: pick_slime_boss,
    innate: &[(Status::Split, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Split {
        a: "spike_slime_large",
        b: "acid_slime_large",
    },
    spawn: spawn_default,
};

/// 开场喷粘液,之后 [喷, 蓄力, 猛击] 死循环
fn pick_slime_boss(ctx: &mut PickCtx) -> usize {
    const GOOP: usize = 0;
    const PREPARING: usize = 1;
    const SLAM: usize = 2;
    if ctx.first_turn() {
        return GOOP;
    }
    if ctx.last_is(GOOP) {
        return PREPARING;
    }
    if ctx.last_is(PREPARING) {
        return SLAM;
    }
    GOOP
}

pub const THE_GUARDIAN: EnemyDef = EnemyDef {
    id: "the_guardian",
    name: "The Guardian",
    kind: EnemyKind::Boss,
    hp: (240, 240),
    moves: &[
        MoveDef {
            name: "Charging Up",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 9,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Fierce Bash",
            intent: Intent::Attack {
                damage: 32,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 32,
                times: 1,
            }],
        },
        MoveDef {
            name: "Vent Steam",
            intent: Intent::StrongDebuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 2,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Whirlwind",
            intent: Intent::Attack {
                damage: 5,
                times: 4,
            },
            effects: &[EnemyFx::Attack {
                amount: 5,
                times: 4,
            }],
        },
        MoveDef {
            name: "Defensive Mode",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::SharpHide,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Roll Attack",
            intent: Intent::Attack {
                damage: 9,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 9,
                times: 1,
            }],
        },
        MoveDef {
            name: "Twin Slam",
            intent: Intent::AttackBuff {
                damage: 8,
                times: 2,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 8,
                    times: 2,
                },
                // 打完之后收起尖刺,并把形态切换的额度重新装上(比原来多 10)
                EnemyFx::LoseStatus {
                    status: Status::SharpHide,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::ModeShift,
                    n: 40,
                    scope: Scope::SelfOnly,
                },
            ],
        },
    ],
    pick: pick_the_guardian,
    innate: &[(Status::ModeShift, 30)],
    start_block: 0,
    on_death: &[],
    special: Special::ModeShift { d: 30, guard: 4 },
    spawn: spawn_default,
};

/// 攻守两套固定循环;掉够血量会被 check_hp_thresholds 打断成防御姿态
fn pick_the_guardian(ctx: &mut PickCtx) -> usize {
    const CHARGING: usize = 0;
    const FIERCE: usize = 1;
    const VENT: usize = 2;
    const WHIRLWIND: usize = 3;
    const GUARD: usize = 4;
    const ROLL: usize = 5;
    const TWIN: usize = 6;
    match ctx.last() {
        None => CHARGING,
        Some(CHARGING) => FIERCE,
        Some(FIERCE) => VENT,
        Some(VENT) => WHIRLWIND,
        Some(WHIRLWIND) => CHARGING,
        Some(GUARD) => ROLL,
        Some(ROLL) => TWIN,
        Some(TWIN) => GUARD,
        _ => CHARGING,
    }
}

pub const HEXAGHOST: EnemyDef = EnemyDef {
    id: "hexaghost",
    name: "Hexaghost",
    kind: EnemyKind::Boss,
    hp: (250, 250),
    moves: &[
        MoveDef {
            name: "Activate",
            intent: Intent::Unknown,
            // 分裂伤害 = 玩家当时生命的 1/12 + 1,记下来给下一招用
            effects: &[EnemyFx::RollDamage { div: 12, add: 1 }],
        },
        MoveDef {
            name: "Divider",
            intent: Intent::Attack {
                damage: 0,
                times: 6,
            },
            effects: &[EnemyFx::AttackRolled { times: 6 }],
        },
        MoveDef {
            name: "Sear",
            intent: Intent::AttackDebuff {
                damage: 6,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 6,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "burn",
                    spot: crate::core::enemy::CardSpot::Discard,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Tackle",
            intent: Intent::Attack {
                damage: 5,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 5,
                times: 2,
            }],
        },
        MoveDef {
            name: "Inflame",
            intent: Intent::DefendBuff { block: 12 },
            effects: &[
                EnemyFx::Block {
                    amount: 12,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 2,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Inferno",
            intent: Intent::AttackDebuff {
                damage: 2,
                times: 6,
            },
            effects: &[EnemyFx::Attack {
                amount: 2,
                times: 6,
            }],
        },
    ],
    pick: pick_hexaghost,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开场点火 → 第二回合分裂 → 之后走固定的七招循环
fn pick_hexaghost(ctx: &mut PickCtx) -> usize {
    const ACTIVATE: usize = 0;
    const DIVIDER: usize = 1;
    const SEAR: usize = 2;
    const TACKLE: usize = 3;
    const INFLAME: usize = 4;
    const INFERNO: usize = 5;
    const LOOP: [usize; 7] = [SEAR, TACKLE, SEAR, INFLAME, TACKLE, SEAR, INFERNO];
    if ctx.first_turn() {
        return ACTIVATE;
    }
    if ctx.turn() == 1 {
        return DIVIDER;
    }
    LOOP[((ctx.turn() - 2) % 7) as usize]
}
