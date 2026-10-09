// 第二章的怪:招式、数值与选招规则.
// 数值取参考实现的"无飞升"基础值,选招规则按 getMove 的判定逐条照搬.
use crate::core::enemy::{
    spawn_default, EnemyDef, EnemyFx, EnemyKind, Intent, MoveDef, PickCtx, Scope, Special,
    SpawnCtx,
};
use crate::core::status::Status;

/// 刺击之书:连续的刺击数开局是 1,每选中一次多段刺击就在选招时自增
fn book_spawn(ctx: &mut SpawnCtx) {
    ctx.state.stab = 1;
}

// ============================ 普通怪 ============================

pub const SPHERIC_GUARDIAN: EnemyDef = EnemyDef {
    id: "spheric_guardian",
    name: "Spheric Guardian",
    kind: EnemyKind::Normal,
    hp: (20, 20),
    moves: &[
        MoveDef {
            name: "Activate",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 25,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Attack Debuff",
            intent: Intent::AttackDebuff {
                damage: 10,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 5,
                },
            ],
        },
        MoveDef {
            name: "Slam",
            intent: Intent::Attack {
                damage: 10,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 10,
                times: 2,
            }],
        },
        MoveDef {
            name: "Harden",
            intent: Intent::AttackDefend {
                damage: 10,
                times: 1,
                block: 15,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::Block {
                    amount: 15,
                    scope: Scope::SelfOnly,
                },
            ],
        },
    ],
    pick: pick_spheric_guardian,
    // 神器 3 挡掉前三次减益,壁垒让开局的 40 点格挡一直留着
    innate: &[(Status::Artifact, 3), (Status::Barricade, 1)],
    start_block: 40,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 固定脚本:激活,削弱打击,之后猛击与硬化交替
fn pick_spheric_guardian(ctx: &mut PickCtx) -> usize {
    const ACTIVATE: usize = 0;
    const ATTACK_DEBUFF: usize = 1;
    const SLAM: usize = 2;
    const HARDEN: usize = 3;
    match ctx.last() {
        None => ACTIVATE,
        Some(ACTIVATE) => ATTACK_DEBUFF,
        Some(ATTACK_DEBUFF) => SLAM,
        Some(SLAM) => HARDEN,
        _ => SLAM,
    }
}

pub const CHOSEN: EnemyDef = EnemyDef {
    id: "chosen",
    name: "Chosen",
    kind: EnemyKind::Normal,
    hp: (95, 99),
    moves: &[
        MoveDef {
            name: "Poke",
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
            name: "Zap",
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
            name: "Debilitate",
            intent: Intent::AttackDebuff {
                damage: 10,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Drain",
            intent: Intent::Debuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 3,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Hex",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Hex,
                n: 1,
            }],
        },
    ],
    pick: pick_chosen,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开局戳一下,第二回合上咒;之后debuff回合与攻击回合严格交替
fn pick_chosen(ctx: &mut PickCtx) -> usize {
    const POKE: usize = 0;
    const ZAP: usize = 1;
    const DEBILITATE: usize = 2;
    const DRAIN: usize = 3;
    const HEX: usize = 4;
    if ctx.first_turn() {
        return POKE;
    }
    // 只行动过一回合(也就是这一掷定的是第 2 回合):必上咒
    if ctx.prev().is_none() {
        return HEX;
    }
    let roll = ctx.roll();
    if !ctx.last_is(DEBILITATE) && !ctx.last_is(DRAIN) {
        if roll < 50 {
            DEBILITATE
        } else {
            DRAIN
        }
    } else if roll < 40 {
        ZAP
    } else {
        POKE
    }
}

pub const SHELLED_PARASITE: EnemyDef = EnemyDef {
    id: "shelled_parasite",
    name: "Shelled Parasite",
    kind: EnemyKind::Normal,
    hp: (68, 72),
    moves: &[
        MoveDef {
            name: "Fell",
            intent: Intent::AttackDebuff {
                damage: 18,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 18,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Double Strike",
            intent: Intent::Attack {
                damage: 6,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 2,
            }],
        },
        MoveDef {
            name: "Suck",
            intent: Intent::AttackBuff {
                damage: 10,
                times: 1,
            },
            // 吸血:回复这一下打出来的伤害
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::HealFromDamage,
            ],
        },
        MoveDef {
            name: "Stunned",
            intent: Intent::Stun,
            effects: &[],
        },
    ],
    pick: pick_shelled_parasite,
    // 甲板装甲每回合末给 14 格挡,掉光的那一下会把这招换成眩晕
    innate: &[(Status::PlatedArmor, 14)],
    start_block: 14,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开局 50/50 双击或吸血;之后重击 20%、双击 40%、吸血 40%,都不许连三
fn pick_shelled_parasite(ctx: &mut PickCtx) -> usize {
    const FELL: usize = 0;
    const DOUBLE_STRIKE: usize = 1;
    const SUCK: usize = 2;
    const STUNNED: usize = 3;
    if ctx.first_turn() {
        return if ctx.coin() {
            DOUBLE_STRIKE
        } else {
            SUCK
        };
    }
    // 壳裂那一回合是眩晕,参考实现把它当重击记进出招历史,所以重击不会紧跟着再来
    let last_fell = ctx.last_is(FELL) || ctx.last_is(STUNNED);
    let roll = ctx.roll();
    let mut roll2 = 100;
    if roll < 20 {
        if !last_fell {
            return FELL;
        }
        // 重击被历史挡住时在 20..99 里重掷,重掷值 < 60 仍会落到双击那一支
        roll2 = ctx.range(20, 99);
    }
    if roll < 60 || roll2 < 60 {
        if ctx.last_two_is(DOUBLE_STRIKE) {
            SUCK
        } else {
            DOUBLE_STRIKE
        }
    } else if ctx.last_two_is(SUCK) {
        DOUBLE_STRIKE
    } else {
        SUCK
    }
}

pub const BYRD: EnemyDef = EnemyDef {
    id: "byrd",
    name: "Byrd",
    kind: EnemyKind::Normal,
    hp: (25, 31),
    moves: &[
        MoveDef {
            name: "Peck",
            intent: Intent::Attack {
                damage: 1,
                times: 5,
            },
            effects: &[EnemyFx::Attack {
                amount: 1,
                times: 5,
            }],
        },
        MoveDef {
            name: "Swoop",
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
            name: "Caw",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 1,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Stunned",
            intent: Intent::Stun,
            effects: &[],
        },
        MoveDef {
            name: "Headbutt",
            intent: Intent::Attack {
                damage: 3,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 3,
                times: 1,
            }],
        },
        MoveDef {
            name: "Fly",
            intent: Intent::Unknown,
            effects: &[EnemyFx::GainStatus {
                status: Status::Flight,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
    ],
    pick: pick_byrd,
    // 飞行:攻击伤害减半,每挨一次打掉一层,掉光落地
    innate: &[(Status::Flight, 3)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 空中按 50/30/20 掷招;落地之后是固定的 眩晕 → 头槌 → 起飞
fn pick_byrd(ctx: &mut PickCtx) -> usize {
    const PECK: usize = 0;
    const SWOOP: usize = 1;
    const CAW: usize = 2;
    const STUNNED: usize = 3;
    const HEADBUTT: usize = 4;
    const FLY: usize = 5;
    if ctx.last_is(STUNNED) {
        return HEADBUTT;
    }
    if ctx.last_is(HEADBUTT) {
        return FLY;
    }
    if ctx.first_turn() {
        // 37.5% 啼叫,其余啄击;开场不俯冲
        return if ctx.flip(3, 8) { CAW } else { PECK };
    }
    let roll = ctx.roll();
    if roll < 50 {
        if ctx.last_two_is(PECK) {
            // 40% 俯冲,否则啼叫
            if ctx.flip(2, 5) {
                SWOOP
            } else {
                CAW
            }
        } else {
            PECK
        }
    } else if roll < 70 {
        if ctx.last_is(SWOOP) {
            // 37.5% 啼叫,否则啄击
            if ctx.flip(3, 8) {
                CAW
            } else {
                PECK
            }
        } else {
            SWOOP
        }
    } else if ctx.last_is(CAW) {
        // 2/7 俯冲,否则啄击
        if ctx.flip(2857, 10000) {
            SWOOP
        } else {
            PECK
        }
    } else {
        CAW
    }
}

pub const MUGGER: EnemyDef = EnemyDef {
    id: "mugger",
    name: "Mugger",
    kind: EnemyKind::Normal,
    hp: (48, 52),
    moves: &[
        MoveDef {
            name: "Mug",
            intent: Intent::Attack {
                damage: 10,
                times: 1,
            },
            // 先抢钱再打人;台词掷点按参考实现放在招式里(第 2 回合多一次)
            effects: &[
                EnemyFx::ParityRand { n: 2 },
                EnemyFx::ParityCoin {
                    num: 3,
                    den: 5,
                    turn: 2,
                },
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
                damage: 16,
                times: 1,
            },
            effects: &[
                EnemyFx::ParityRand { n: 2 },
                EnemyFx::StealGold { n: 15 },
                EnemyFx::Attack {
                    amount: 16,
                    times: 1,
                },
            ],
        },
        MoveDef {
            name: "Smoke Bomb",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 11,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Escape",
            intent: Intent::Escape,
            effects: &[EnemyFx::Escape],
        },
    ],
    pick: pick_mugger,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 固定脚本:抢两回合,然后一半概率直接霰雾弹跑路,一半概率先猛扑
fn pick_mugger(ctx: &mut PickCtx) -> usize {
    const MUG: usize = 0;
    const LUNGE: usize = 1;
    const SMOKE_BOMB: usize = 2;
    const ESCAPE: usize = 3;
    if ctx.first_turn() {
        return MUG;
    }
    if ctx.last_is(MUG) {
        // 只行动过一回合就是第 2 回合,还是抢
        if ctx.prev().is_none() {
            return MUG;
        }
        // 第 2 回合行动完这一掷:50% 霰雾弹,否则猛扑
        return if ctx.flip(1, 2) { SMOKE_BOMB } else { LUNGE };
    }
    if ctx.last_is(LUNGE) {
        return SMOKE_BOMB;
    }
    ESCAPE
}

pub const CENTURION: EnemyDef = EnemyDef {
    id: "centurion",
    name: "Centurion",
    kind: EnemyKind::Normal,
    hp: (76, 80),
    moves: &[
        MoveDef {
            name: "Slash",
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
            name: "Fury",
            intent: Intent::Attack {
                damage: 6,
                times: 3,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 3,
            }],
        },
        MoveDef {
            name: "Defend",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 15,
                scope: Scope::Allies,
            }],
        },
    ],
    pick: pick_centurion,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 有同伴就套盾、落单就狂怒;65% 砍一刀,斩击不许连三
fn pick_centurion(ctx: &mut PickCtx) -> usize {
    const SLASH: usize = 0;
    const FURY: usize = 1;
    const DEFEND: usize = 2;
    let support = if ctx.alive() > 1 { DEFEND } else { FURY };
    let roll = ctx.roll();
    if roll >= 65 && !ctx.last_two_is(DEFEND) && !ctx.last_two_is(FURY) {
        return support;
    }
    if !ctx.last_two_is(SLASH) {
        return SLASH;
    }
    support
}

pub const MYSTIC: EnemyDef = EnemyDef {
    id: "mystic",
    name: "Mystic",
    kind: EnemyKind::Normal,
    hp: (48, 56),
    moves: &[
        MoveDef {
            name: "Attack Debuff",
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
                    status: Status::Frail,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Heal",
            intent: Intent::Buff,
            effects: &[
                EnemyFx::Heal {
                    n: 16,
                    scope: Scope::Allies,
                },
                EnemyFx::Heal {
                    n: 16,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Buff",
            intent: Intent::Buff,
            effects: &[
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 2,
                    scope: Scope::Allies,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 2,
                    scope: Scope::SelfOnly,
                },
            ],
        },
    ],
    pick: pick_mystic,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 自己或骑士缺血满 16 就先治疗;否则 60% 削弱打击,40% 增益,都不许连三
fn pick_mystic(ctx: &mut PickCtx) -> usize {
    const ATTACK_DEBUFF: usize = 0;
    const HEAL: usize = 1;
    const BUFF: usize = 2;
    const HEAL_NEED: i32 = 16;
    // 骑士站在 0 号位
    let knight_needs = ctx.idx != 0
        && ctx
            .slot(0)
            .is_some_and(|k| k.alive() && k.max_hp - k.hp >= HEAL_NEED);
    if ctx.max_hp() - ctx.hp() >= HEAL_NEED || knight_needs {
        return HEAL;
    }
    let roll = ctx.roll();
    if roll >= 40 && !ctx.last_two_is(ATTACK_DEBUFF) {
        return ATTACK_DEBUFF;
    }
    if !ctx.last_two_is(BUFF) {
        return BUFF;
    }
    ATTACK_DEBUFF
}

pub const SNAKE_PLANT: EnemyDef = EnemyDef {
    id: "snake_plant",
    name: "Snake Plant",
    kind: EnemyKind::Normal,
    hp: (75, 79),
    moves: &[
        MoveDef {
            name: "Chomp",
            intent: Intent::Attack {
                damage: 7,
                times: 3,
            },
            effects: &[EnemyFx::Attack {
                amount: 7,
                times: 3,
            }],
        },
        MoveDef {
            name: "Enfeebling Spores",
            intent: Intent::StrongDebuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 2,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 2,
                },
            ],
        },
    ],
    pick: pick_snake_plant,
    // 延展:挨打先拿格挡再涨一层,回合末重置回 3
    innate: &[(Status::Malleable, 3)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 65% 啃咬(不许连三),其余减速孢子(不许连二)
fn pick_snake_plant(ctx: &mut PickCtx) -> usize {
    const CHOMP: usize = 0;
    const SPORES: usize = 1;
    let roll = ctx.roll();
    if roll < 65 {
        if ctx.last_two_is(CHOMP) {
            SPORES
        } else {
            CHOMP
        }
    } else if ctx.last_is(SPORES) {
        CHOMP
    } else {
        SPORES
    }
}

pub const SNECKO: EnemyDef = EnemyDef {
    id: "snecko",
    name: "Snecko",
    kind: EnemyKind::Normal,
    hp: (114, 120),
    moves: &[
        MoveDef {
            name: "Perplexing Glare",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Confused,
                n: 1,
            }],
        },
        MoveDef {
            name: "Bite",
            intent: Intent::Attack {
                damage: 15,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 15,
                times: 1,
            }],
        },
        MoveDef {
            name: "Tail Whip",
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
                    n: 2,
                },
            ],
        },
    ],
    pick: pick_snecko,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开局甩一眼混乱,之后 40% 甩尾(咬不许连三),其余咬
fn pick_snecko(ctx: &mut PickCtx) -> usize {
    const GLARE: usize = 0;
    const BITE: usize = 1;
    const TAIL_WHIP: usize = 2;
    if ctx.first_turn() {
        return GLARE;
    }
    let roll = ctx.roll();
    if roll < 40 || ctx.last_two_is(BITE) {
        TAIL_WHIP
    } else {
        BITE
    }
}

// ============================ 精英 ============================

pub const BOOK_OF_STABBING: EnemyDef = EnemyDef {
    id: "book_of_stabbing",
    name: "Book of Stabbing",
    kind: EnemyKind::Elite,
    hp: (160, 164),
    moves: &[
        MoveDef {
            name: "Multi Stab",
            intent: Intent::Attack {
                damage: 6,
                times: 1,
            },
            // 命中次数由连续刺击数决定
            effects: &[EnemyFx::AttackStabCount { amount: 6 }],
        },
        MoveDef {
            name: "Single Stab",
            intent: Intent::Attack {
                damage: 21,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 21,
                times: 1,
            }],
        },
    ],
    pick: pick_book_of_stabbing,
    // 痛苦刺击:每次没挡住的一段攻击伤害都往弃牌堆塞一张伤口
    innate: &[(Status::PainfulStabs, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: book_spawn,
};

/// 15% 单刺(不许连二),其余多段刺击(不许连三),刺击数在选招时自增
fn pick_book_of_stabbing(ctx: &mut PickCtx) -> usize {
    const MULTI_STAB: usize = 0;
    const SINGLE_STAB: usize = 1;
    let roll = ctx.roll();
    if roll < 15 {
        if ctx.last_is(SINGLE_STAB) {
            ctx.state.stab += 1;
            return MULTI_STAB;
        }
        return SINGLE_STAB;
    }
    if ctx.last_two_is(MULTI_STAB) {
        return SINGLE_STAB;
    }
    ctx.state.stab += 1;
    MULTI_STAB
}

/// 召集时抽的 8 只小鬼池(参考实现里 MonsterGroup 的 getGremlin 表):
/// 疯的两只、偷偷的两只、胖的两只、盾牌一只、巫师一只.
/// 第一章的小鬼团伙与第二章头目开局的小鬼也用这张表.
pub static GREMLIN_POOL: &[&'static str] = &[
    "mad_gremlin",
    "mad_gremlin",
    "sneaky_gremlin",
    "sneaky_gremlin",
    "fat_gremlin",
    "fat_gremlin",
    "shield_gremlin",
    "gremlin_wizard",
];

pub const GREMLIN_LEADER: EnemyDef = EnemyDef {
    id: "gremlin_leader",
    name: "Gremlin Leader",
    kind: EnemyKind::Elite,
    hp: (140, 148),
    moves: &[
        MoveDef {
            name: "Rally",
            intent: Intent::Unknown,
            // 参考实现各掷各的,所以允许召出两只一样的
            effects: &[EnemyFx::SummonRandom {
                pool: GREMLIN_POOL,
                count: 2,
                slots: &[1, 2, 0],
            }],
        },
        MoveDef {
            name: "Encourage",
            intent: Intent::DefendBuff { block: 6 },
            effects: &[
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::Allies,
                },
                EnemyFx::Block {
                    amount: 6,
                    scope: Scope::Allies,
                },
            ],
        },
        MoveDef {
            name: "Stab",
            intent: Intent::Attack {
                damage: 6,
                times: 3,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 3,
            }],
        },
    ],
    pick: pick_gremlin_leader,
    innate: &[(Status::MinionLeader, 1)],
    start_block: 0,
    on_death: &[],
    // 首领死了,召唤来的小鬼一起退场
    special: Special::Leader,
    spawn: spawn_default,
};

/// 按还活着的小鬼数分三张表:0 只、1 只、2 只以上
fn pick_gremlin_leader(ctx: &mut PickCtx) -> usize {
    const RALLY: usize = 0;
    const ENCOURAGE: usize = 1;
    const STAB: usize = 2;
    let gremlins = ctx.alive_allies();
    let roll = ctx.roll();
    if gremlins == 0 {
        if roll < 75 {
            if ctx.last_is(RALLY) {
                STAB
            } else {
                RALLY
            }
        } else if ctx.last_is(STAB) {
            RALLY
        } else {
            STAB
        }
    } else if gremlins == 1 {
        if roll < 50 {
            if ctx.last_is(RALLY) {
                // 被挡住时在 50..99 里重掷:80 以下鼓励,否则捅
                if ctx.range(50, 99) < 80 {
                    ENCOURAGE
                } else {
                    STAB
                }
            } else {
                RALLY
            }
        } else if roll < 80 {
            if ctx.last_is(ENCOURAGE) {
                STAB
            } else {
                ENCOURAGE
            }
        } else if ctx.last_is(STAB) {
            // 被挡住时在 0..80 里重掷:50 以下召集,否则鼓励
            if ctx.range(0, 80) < 50 {
                RALLY
            } else {
                ENCOURAGE
            }
        } else {
            STAB
        }
    } else if roll < 66 {
        if ctx.last_is(ENCOURAGE) {
            STAB
        } else {
            ENCOURAGE
        }
    } else if ctx.last_is(STAB) {
        ENCOURAGE
    } else {
        STAB
    }
}

pub const TASKMASTER: EnemyDef = EnemyDef {
    id: "taskmaster",
    name: "Taskmaster",
    kind: EnemyKind::Elite,
    hp: (54, 60),
    moves: &[MoveDef {
        name: "Scouring Whip",
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
                card: "wound",
                spot: crate::core::enemy::CardSpot::Discard,
                n: 1,
            },
        ],
    }],
    pick: pick_scouring_whip,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 只有一招:每回合都是鞭笞
fn pick_scouring_whip(_ctx: &mut PickCtx) -> usize {
    0
}

// ============================ Boss ============================

pub const BRONZE_AUTOMATON: EnemyDef = EnemyDef {
    id: "bronze_automaton",
    name: "Bronze Automaton",
    kind: EnemyKind::Boss,
    hp: (300, 300),
    moves: &[
        MoveDef {
            name: "Spawn Orbs",
            intent: Intent::Unknown,
            // 自己在槽 1,两颗铜球占 0 和 2:一颗排在自己前面
            effects: &[EnemyFx::Summon {
                ids: &["bronze_orb", "bronze_orb"],
                slots: &[0, 2],
            }],
        },
        MoveDef {
            name: "Flail",
            intent: Intent::Attack {
                damage: 7,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 7,
                times: 2,
            }],
        },
        MoveDef {
            name: "Boost",
            intent: Intent::DefendBuff { block: 9 },
            effects: &[
                EnemyFx::Block {
                    amount: 9,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Hyper Beam",
            intent: Intent::Attack {
                damage: 45,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 45,
                times: 1,
            }],
        },
        MoveDef {
            name: "Stunned",
            intent: Intent::Stun,
            effects: &[],
        },
    ],
    pick: pick_bronze_automaton,
    innate: &[(Status::MinionLeader, 1), (Status::Artifact, 3)],
    start_block: 0,
    on_death: &[],
    // 首领死了,两只圆球一起退场
    special: Special::Leader,
    spawn: spawn_default,
};

/// 固定脚本:开局放球,之后连枷/增幅交替,增幅两次后放光束再眩晕
fn pick_bronze_automaton(ctx: &mut PickCtx) -> usize {
    const SPAWN_ORBS: usize = 0;
    const FLAIL: usize = 1;
    const BOOST: usize = 2;
    const HYPER_BEAM: usize = 3;
    const STUNNED: usize = 4;
    if ctx.first_turn() {
        return SPAWN_ORBS;
    }
    if ctx.last_is(SPAWN_ORBS) {
        return FLAIL;
    }
    if ctx.last_is(FLAIL) {
        return BOOST;
    }
    if ctx.last_is(BOOST) {
        // 上一次增幅前面是连枷,这次增幅完就放光束;否则回去连枷
        return if ctx.state.phase2 {
            ctx.state.phase2 = false;
            HYPER_BEAM
        } else {
            ctx.state.phase2 = true;
            FLAIL
        };
    }
    if ctx.last_is(HYPER_BEAM) {
        return STUNNED;
    }
    FLAIL
}

pub const BRONZE_ORB: EnemyDef = EnemyDef {
    id: "bronze_orb",
    name: "Bronze Orb",
    kind: EnemyKind::Normal,
    hp: (52, 58),
    moves: &[
        MoveDef {
            name: "Beam",
            intent: Intent::Attack {
                damage: 8,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 8,
                times: 1,
            }],
        },
        MoveDef {
            name: "Support Beam",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 12,
                scope: Scope::Allies,
            }],
        },
        MoveDef {
            name: "Stasis",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::StealCard],
        },
    ],
    pick: pick_bronze_orb,
    innate: &[(Status::Minion, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 停滞每场只用一次;用过之后 70% 激光、30% 支援,两种都不许连三
fn pick_bronze_orb(ctx: &mut PickCtx) -> usize {
    const BEAM: usize = 0;
    const SUPPORT_BEAM: usize = 1;
    const STASIS: usize = 2;
    let roll = ctx.roll();
    if !ctx.state.stasis_used && roll >= 25 {
        ctx.state.stasis_used = true;
        return STASIS;
    }
    if roll >= 70 && !ctx.last_two_is(SUPPORT_BEAM) {
        return SUPPORT_BEAM;
    }
    if !ctx.last_two_is(BEAM) {
        return BEAM;
    }
    SUPPORT_BEAM
}

pub const THE_COLLECTOR: EnemyDef = EnemyDef {
    id: "the_collector",
    name: "The Collector",
    kind: EnemyKind::Boss,
    hp: (282, 282),
    moves: &[
        MoveDef {
            name: "Spawn",
            intent: Intent::Unknown,
            // 她在槽 2,火炬头占 0 和 1:都在她前面,出手比自己早
            effects: &[EnemyFx::Summon {
                ids: &["torch_head", "torch_head"],
                slots: &[1, 0],
            }],
        },
        MoveDef {
            name: "Fireball",
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
            name: "Buff",
            intent: Intent::DefendBuff { block: 15 },
            effects: &[
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 3,
                    scope: Scope::Allies,
                },
                EnemyFx::Block {
                    amount: 15,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Mega Debuff",
            intent: Intent::StrongDebuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 3,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 3,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 3,
                },
            ],
        },
    ],
    pick: pick_the_collector,
    innate: &[(Status::MinionLeader, 1)],
    start_block: 0,
    on_death: &[],
    // 首领死了,火炬头一起退场
    special: Special::Leader,
    spawn: spawn_default,
};

/// 开局召唤,第 4 回合必放大削弱;剩下按 26/45/29 掷,召唤与增益都不许连二
fn pick_the_collector(ctx: &mut PickCtx) -> usize {
    const SPAWN: usize = 0;
    const FIREBALL: usize = 1;
    const BUFF: usize = 2;
    const MEGA_DEBUFF: usize = 3;
    if ctx.first_turn() {
        return SPAWN;
    }
    // 已经行动过三回合时这一掷定的是第 4 回合,必是大削弱
    if ctx.turn() == 3 {
        return MEGA_DEBUFF;
    }
    let can_spawn = ctx.alive() < 3 && !ctx.last_is(SPAWN);
    let roll = ctx.roll();
    if roll <= 25 && can_spawn {
        return SPAWN;
    }
    if roll <= 70 && !ctx.last_two_is(FIREBALL) {
        return FIREBALL;
    }
    if ctx.last_is(BUFF) {
        return FIREBALL;
    }
    BUFF
}

pub const TORCH_HEAD: EnemyDef = EnemyDef {
    id: "torch_head",
    name: "Torch Head",
    kind: EnemyKind::Normal,
    hp: (38, 40),
    moves: &[MoveDef {
        name: "Tackle",
        intent: Intent::Attack {
            damage: 7,
            times: 1,
        },
        effects: &[EnemyFx::Attack {
            amount: 7,
            times: 1,
        }],
    }],
    pick: pick_tackle,
    innate: &[(Status::Minion, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 只有一招:每回合都撞一下
fn pick_tackle(_ctx: &mut PickCtx) -> usize {
    0
}

pub const THE_CHAMP: EnemyDef = EnemyDef {
    id: "the_champ",
    name: "The Champ",
    kind: EnemyKind::Boss,
    hp: (420, 420),
    moves: &[
        MoveDef {
            name: "Heavy Slash",
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
            name: "Face Slap",
            intent: Intent::AttackDebuff {
                damage: 12,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 12,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 2,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Defensive Stance",
            intent: Intent::DefendBuff { block: 15 },
            effects: &[
                EnemyFx::Block {
                    amount: 15,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::GainStatus {
                    status: Status::Metallicize,
                    n: 5,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Gloat",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 2,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Taunt",
            intent: Intent::Debuff,
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
            name: "Anger",
            intent: Intent::Buff,
            effects: &[
                EnemyFx::ClearDebuffs,
                EnemyFx::GainStatus {
                    status: Status::Strength,
                    n: 6,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Execute",
            intent: Intent::Attack {
                damage: 10,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 10,
                times: 2,
            }],
        },
    ],
    pick: pick_the_champ,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 一阶段第 4、8、12…… 回合必嘲讽;半血以下进入二阶段并怒吼一次,之后每三回合处决
fn pick_the_champ(ctx: &mut PickCtx) -> usize {
    const HEAVY_SLASH: usize = 0;
    const FACE_SLAP: usize = 1;
    const STANCE: usize = 2;
    const GLOAT: usize = 3;
    const TAUNT: usize = 4;
    const ANGER: usize = 5;
    const EXECUTE: usize = 6;
    if ctx.state.phase2 {
        // 最近两招里没有处决就一直处决
        if !ctx.last_is(EXECUTE) && !ctx.prev_is(EXECUTE) {
            return EXECUTE;
        }
    } else {
        // 第一次在半血以下掷招:进二阶段,这一掷必是怒吼
        if ctx.hp() * 2 < ctx.max_hp() {
            ctx.state.phase2 = true;
            return ANGER;
        }
        // 已行动数 +1 能被 4 整除,就是第 4、8、12…… 回合
        if (ctx.turn() + 1) % 4 == 0 {
            return TAUNT;
        }
    }
    let roll = ctx.roll();
    // 防守姿态每场最多两次,且不许连二
    if roll <= 15 && !ctx.last_is(STANCE) && ctx.state.guard_uses < 2 {
        ctx.state.guard_uses += 1;
        return STANCE;
    }
    if roll <= 30 && !ctx.last_is(GLOAT) && !ctx.last_is(STANCE) {
        return GLOAT;
    }
    if roll <= 55 && !ctx.last_is(FACE_SLAP) {
        return FACE_SLAP;
    }
    if !ctx.last_is(HEAVY_SLASH) {
        return HEAVY_SLASH;
    }
    FACE_SLAP
}

// ============================ 事件怪 ============================

pub const BEAR: EnemyDef = EnemyDef {
    id: "bear",
    name: "Bear",
    kind: EnemyKind::Normal,
    hp: (38, 42),
    moves: &[
        MoveDef {
            name: "Bear Hug",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Dexterity,
                n: -2,
            }],
        },
        MoveDef {
            name: "Lunge",
            intent: Intent::AttackDefend {
                damage: 9,
                times: 1,
                block: 9,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 9,
                    times: 1,
                },
                EnemyFx::Block {
                    amount: 9,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Maul",
            intent: Intent::Attack {
                damage: 18,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 18,
                times: 1,
            }],
        },
    ],
    pick: pick_bear,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 固定脚本:先抱一下,之后猛扑与撕咬交替
fn pick_bear(ctx: &mut PickCtx) -> usize {
    const BEAR_HUG: usize = 0;
    const LUNGE: usize = 1;
    const MAUL: usize = 2;
    if ctx.first_turn() {
        return BEAR_HUG;
    }
    match ctx.last() {
        Some(BEAR_HUG) => LUNGE,
        Some(LUNGE) => MAUL,
        _ => LUNGE,
    }
}

pub const ROMEO: EnemyDef = EnemyDef {
    id: "romeo",
    name: "Romeo",
    kind: EnemyKind::Normal,
    hp: (35, 39),
    moves: &[
        MoveDef {
            name: "Mock",
            intent: Intent::Unknown,
            effects: &[],
        },
        MoveDef {
            name: "Agonizing Slash",
            intent: Intent::AttackDebuff {
                damage: 10,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Cross Slash",
            intent: Intent::Attack {
                damage: 15,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 15,
                times: 1,
            }],
        },
    ],
    pick: pick_romeo,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 固定脚本:先嘲笑一声,之后折磨斩与交叉斩交替
fn pick_romeo(ctx: &mut PickCtx) -> usize {
    const MOCK: usize = 0;
    const AGONIZING_SLASH: usize = 1;
    const CROSS_SLASH: usize = 2;
    if ctx.first_turn() {
        return MOCK;
    }
    match ctx.last() {
        Some(MOCK) => AGONIZING_SLASH,
        Some(AGONIZING_SLASH) => CROSS_SLASH,
        _ => AGONIZING_SLASH,
    }
}

pub const POINTY: EnemyDef = EnemyDef {
    id: "pointy",
    name: "Pointy",
    kind: EnemyKind::Normal,
    hp: (30, 30),
    moves: &[MoveDef {
        name: "Attack",
        intent: Intent::Attack {
            damage: 5,
            times: 2,
        },
        effects: &[EnemyFx::Attack {
            amount: 5,
            times: 2,
        }],
    }],
    pick: pick_pointy_attack,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 只有一招:每回合捅两下
fn pick_pointy_attack(_ctx: &mut PickCtx) -> usize {
    0
}
