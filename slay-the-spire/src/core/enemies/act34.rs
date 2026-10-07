// 第三、四章的怪:招式、数值与选招规则.
// 数值取参考实现的"无飞升"基础值,选招规则按 getMove 的判定逐条照搬.
use crate::core::enemy::{
    spawn_default, CardSpot, EnemyDef, EnemyFx, EnemyKind, Intent, MoveDef, PickCtx, Scope,
    Special, SpawnCtx,
};
use crate::core::status::Status;

/// 只有一招的怪:永远用第一招
fn pick_only_zero(_ctx: &mut PickCtx) -> usize {
    0
}

// ============================ 普通怪 ============================

// ---- 暗灵:三只一组;死了只要还有同伴就先半死,两回合后回满一半血 ----

pub const DARKLING: EnemyDef = EnemyDef {
    id: "darkling",
    name: "Darkling",
    kind: EnemyKind::Normal,
    hp: (48, 56),
    moves: &[
        MoveDef {
            // 显示值取区间下限,实际伤害是开局掷出来的那一个
            name: "Nip",
            intent: Intent::Attack {
                damage: 7,
                times: 1,
            },
            effects: &[EnemyFx::AttackRolled { times: 1 }],
        },
        MoveDef {
            name: "Chomp",
            intent: Intent::Attack {
                damage: 8,
                times: 2,
            },
            effects: &[EnemyFx::Attack {
                amount: 8,
                times: 2,
            }],
        },
        MoveDef {
            name: "Harden",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 12,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Regrow",
            intent: Intent::Unknown,
            effects: &[],
        },
        MoveDef {
            name: "Reincarnate",
            intent: Intent::Buff,
            effects: &[],
        },
    ],
    pick: pick_darkling,
    innate: &[(Status::Regrow, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Regrow,
    spawn: darkling_spawn,
};

/// 开局掷一次撕咬伤害(三个暗灵各掷一次)
fn darkling_spawn(ctx: &mut SpawnCtx) {
    ctx.state.rolled = ctx.rng.range_inclusive(7, 11);
}

/// 半死时先摆 REGROW 再摆 REINCARNATE;活着时撕咬/硬化都有限制
fn pick_darkling(ctx: &mut PickCtx) -> usize {
    const NIP: usize = 0;
    const CHOMP: usize = 1;
    const HARDEN: usize = 2;
    const REGROW: usize = 3;
    const REINCARNATE: usize = 4;
    let mut r = ctx.roll();
    if ctx.state.half_dead {
        // 半死期间不出招:还剩一个以上复活回合就继续摆 REGROW,最后一回合换 REINCARNATE
        let ticks = ctx.state.regrow_ticks;
        if ticks > 1 {
            ctx.state.regrow_ticks = ticks - 1;
            return REGROW;
        }
        return REINCARNATE;
    }
    if ctx.first_turn() {
        return if r < 50 { HARDEN } else { NIP };
    }
    loop {
        if r < 40 {
            // 队里第二只(下标 1)从不用 CHOMP
            if !ctx.last_is(CHOMP) && ctx.idx != 1 {
                return CHOMP;
            }
            r = ctx.range(40, 99);
        }
        if r < 70 {
            return if !ctx.last_is(HARDEN) { HARDEN } else { NIP };
        }
        if !ctx.last_two_is(NIP) {
            return NIP;
        }
        r = ctx.roll();
    }
}

// ---- 圆球步行者:回合末力量一直在涨,激光还会塞燃烧 ----

pub const ORB_WALKER: EnemyDef = EnemyDef {
    id: "orb_walker",
    name: "Orb Walker",
    kind: EnemyKind::Normal,
    hp: (90, 96),
    moves: &[
        MoveDef {
            name: "Laser",
            intent: Intent::AttackDebuff {
                damage: 10,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "burn",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
                EnemyFx::PlayerCard {
                    card: "burn",
                    spot: CardSpot::Discard,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Claw",
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
    pick: pick_orb_walker,
    innate: &[(Status::StrengthUp, 3)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 4 成爪击、6 成激光,两种都不许连三
fn pick_orb_walker(ctx: &mut PickCtx) -> usize {
    const LASER: usize = 0;
    const CLAW: usize = 1;
    let roll = ctx.roll();
    if roll < 40 {
        if ctx.last_two_is(CLAW) {
            LASER
        } else {
            CLAW
        }
    } else if ctx.last_two_is(LASER) {
        CLAW
    } else {
        LASER
    }
}

// ---- 尖刺:给自己叠荆棘,最多叠六次,之后一直割 ----

pub const SPIKER: EnemyDef = EnemyDef {
    id: "spiker",
    name: "Spiker",
    kind: EnemyKind::Normal,
    hp: (42, 56),
    moves: &[
        MoveDef {
            name: "Cut",
            intent: Intent::Attack {
                damage: 7,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 7,
                times: 1,
            }],
        },
        MoveDef {
            name: "Spike",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Thorns,
                n: 2,
                scope: Scope::SelfOnly,
            }],
        },
    ],
    pick: pick_spiker,
    innate: &[(Status::Thorns, 3)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 一半概率割一刀(不许连二);叠满六次尖刺之后永远割
fn pick_spiker(ctx: &mut PickCtx) -> usize {
    const CUT: usize = 0;
    const SPIKE: usize = 1;
    if ctx.state.spikes > 5 {
        return CUT;
    }
    let roll = ctx.roll();
    if roll < 50 && !ctx.last_is(CUT) {
        return CUT;
    }
    // 选到尖刺就记账(参考实现在 SPIKE 执行时加一,这里选到即执行)
    ctx.state.spikes += 1;
    SPIKE
}

// ---- 排斥者:八成塞眩晕,两成撞人 ----

pub const REPULSOR: EnemyDef = EnemyDef {
    id: "repulsor",
    name: "Repulsor",
    kind: EnemyKind::Normal,
    hp: (29, 35),
    moves: &[
        MoveDef {
            name: "Bash",
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
            name: "Repulse",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerCard {
                card: "dazed",
                spot: CardSpot::DrawShuffle,
                n: 2,
            }],
        },
    ],
    pick: pick_repulsor,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 两成撞人(不许连二),其余塞眩晕
fn pick_repulsor(ctx: &mut PickCtx) -> usize {
    const BASH: usize = 0;
    const REPULSE: usize = 1;
    if ctx.roll() < 20 && !ctx.last_is(BASH) {
        BASH
    } else {
        REPULSE
    }
}

// ---- 自爆怪:撞两下,第三回合自爆(非攻击伤害 + 真死) ----

pub const EXPLODER: EnemyDef = EnemyDef {
    id: "exploder",
    name: "Exploder",
    kind: EnemyKind::Normal,
    hp: (30, 30),
    moves: &[
        MoveDef {
            name: "Slam",
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
            name: "Explode",
            intent: Intent::Unknown,
            effects: &[EnemyFx::PlainDamage { amount: 30 }, EnemyFx::Suicide],
        },
    ],
    pick: pick_exploder,
    innate: &[(Status::Explosive, 3)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 撞、撞、爆,永远这个顺序
fn pick_exploder(ctx: &mut PickCtx) -> usize {
    const SLAM: usize = 0;
    const EXPLODE: usize = 1;
    if ctx.last_two_is(SLAM) {
        EXPLODE
    } else {
        SLAM
    }
}

// ---- 瞬变体:999 血,每回合攻击越来越重,第五回合自己消失 ----

pub const TRANSIENT: EnemyDef = EnemyDef {
    id: "transient",
    name: "Transient",
    kind: EnemyKind::Normal,
    hp: (999, 999),
    moves: &[MoveDef {
        name: "Attack",
        intent: Intent::Attack {
            damage: 30,
            times: 1,
        },
        effects: &[EnemyFx::AttackScaling {
            amount: 30,
            per_turn: 10,
            cap: 999,
            times: 1,
        }],
    }],
    pick: pick_only_zero,
    innate: &[(Status::Shifting, 1), (Status::Fading, 5)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

// ---- 巨口:开场咆哮,之后猛击/啃咬/流涎 ----

pub const THE_MAW: EnemyDef = EnemyDef {
    id: "the_maw",
    name: "The Maw",
    kind: EnemyKind::Normal,
    hp: (300, 300),
    moves: &[
        MoveDef {
            name: "Roar",
            intent: Intent::StrongDebuff,
            effects: &[
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 3,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 3,
                },
            ],
        },
        MoveDef {
            name: "Drool",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 3,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Slam",
            intent: Intent::Attack {
                damage: 25,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 25,
                times: 1,
            }],
        },
        MoveDef {
            // 命中次数随回合数涨(ceil(回合数 / 2));啃完必接流涎
            name: "Nom",
            intent: Intent::Attack {
                damage: 5,
                times: 1,
            },
            effects: &[
                EnemyFx::AttackGrowing { amount: 5 },
                EnemyFx::ForceNext { idx: 1 },
            ],
        },
    ],
    pick: pick_the_maw,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开场咆哮;之后一半概率啃咬,猛击不许连二
fn pick_the_maw(ctx: &mut PickCtx) -> usize {
    const ROAR: usize = 0;
    const SLAM: usize = 2;
    const NOM: usize = 3;
    const DROOL: usize = 1;
    if ctx.first_turn() {
        return ROAR;
    }
    // 啃咬之后必接流涎,由 ForceNext 直接指定,这里不会再被掷到
    if ctx.roll() < 50 && !ctx.last_is(NOM) {
        return NOM;
    }
    if !ctx.last_is(SLAM) {
        return SLAM;
    }
    DROOL
}

// ---- 高塔增生:给玩家永久缠绕,自己只会快撞和重击 ----

pub const SPIRE_GROWTH: EnemyDef = EnemyDef {
    id: "spire_growth",
    name: "Spire Growth",
    kind: EnemyKind::Normal,
    hp: (170, 170),
    moves: &[
        MoveDef {
            name: "Quick Tackle",
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
            name: "Smash",
            intent: Intent::Attack {
                damage: 22,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 22,
                times: 1,
            }],
        },
        MoveDef {
            name: "Constrict",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Constricted,
                n: 10,
            }],
        },
    ],
    pick: pick_spire_growth,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 玩家没被缠绕、上招不是缠绕、且掷点过半时缠绕;两种攻击都不许连三
fn pick_spire_growth(ctx: &mut PickCtx) -> usize {
    const QUICK_TACKLE: usize = 0;
    const SMASH: usize = 1;
    const CONSTRICT: usize = 2;
    let roll = ctx.roll();
    let use_constrict =
        !ctx.player_has(Status::Constricted) && !ctx.last_is(CONSTRICT) && roll >= 50;
    if use_constrict {
        return CONSTRICT;
    }
    if roll < 50 && !ctx.last_two_is(QUICK_TACKLE) {
        return QUICK_TACKLE;
    }
    if !ctx.last_two_is(SMASH) {
        return SMASH;
    }
    QUICK_TACKLE
}

// ---- 扭动巨物:挨打就重掷意图,招式的分段掷点有一串回退 ----

pub const WRITHING_MASS: EnemyDef = EnemyDef {
    id: "writhing_mass",
    name: "Writhing Mass",
    kind: EnemyKind::Normal,
    hp: (160, 160),
    moves: &[
        MoveDef {
            name: "Strong Strike",
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
            name: "Multi Strike",
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
            name: "Flail",
            intent: Intent::AttackDefend {
                damage: 15,
                times: 1,
                block: 16,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 15,
                    times: 1,
                },
                EnemyFx::Block {
                    amount: 16,
                    scope: Scope::SelfOnly,
                },
            ],
        },
        MoveDef {
            name: "Wither",
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
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 2,
                },
            ],
        },
        MoveDef {
            // 寄生:往牌组里永久塞一张寄生诅咒,一场只用一次
            name: "Implant",
            intent: Intent::StrongDebuff,
            effects: &[EnemyFx::PlayerCard {
                card: "parasite",
                spot: CardSpot::Deck,
                n: 1,
            }],
        },
    ],
    pick: pick_writhing_mass,
    innate: &[(Status::Reactive, 1), (Status::Malleable, 4)],
    start_block: 0,
    on_death: &[],
    special: Special::Reactive,
    spawn: spawn_default,
};

/// 开场 33/33/33 掷多段/连枷/枯萎;之后按 spec 的五段级联,段内还会重掷
fn pick_writhing_mass(ctx: &mut PickCtx) -> usize {
    const STRONG_STRIKE: usize = 0;
    const MULTI_STRIKE: usize = 1;
    const FLAIL: usize = 2;
    const WITHER: usize = 3;
    const IMPLANT: usize = 4;
    let mut r = ctx.roll();
    if ctx.first_turn() {
        if r < 33 {
            return MULTI_STRIKE;
        }
        if r < 66 {
            return FLAIL;
        }
        return WITHER;
    }
    let used_implant = ctx.state.implant_used;
    loop {
        // 第一段(roll < 10):重击不许连二
        if r < 10 {
            if !ctx.last_is(STRONG_STRIKE) {
                return STRONG_STRIKE;
            }
            r = ctx.range(10, 99);
        }
        // 第二段(roll < 20):寄生一场一次,不许连二
        if r < 20 {
            if !used_implant && !ctx.last_is(IMPLANT) {
                ctx.state.implant_used = true;
                return IMPLANT;
            }
            if ctx.flip(1, 10) {
                return STRONG_STRIKE;
            }
            r = ctx.range(20, 99);
        }
        // 第三段(roll < 40):枯萎不许连二,否则四成再掷一次小点
        if r < 40 {
            if !ctx.last_is(WITHER) {
                return WITHER;
            }
            if ctx.flip(4, 10) {
                let r2 = ctx.range(0, 19);
                if r2 < 10 {
                    return STRONG_STRIKE;
                }
                if !used_implant {
                    ctx.state.implant_used = true;
                    return IMPLANT;
                }
                if ctx.flip(1, 10) {
                    return STRONG_STRIKE;
                }
                r = ctx.range(20, 99);
                continue;
            }
            r = ctx.range(40, 99);
        }
        // 第四段(roll < 70):多段不许连二,否则三成连枷,否则回头再掷
        if r < 70 {
            if !ctx.last_is(MULTI_STRIKE) {
                return MULTI_STRIKE;
            }
            if ctx.flip(3, 10) {
                return FLAIL;
            }
            r = ctx.range(0, 39);
            continue;
        }
        // 第五段(roll >= 70):连枷不许连二,否则枯萎
        if !ctx.last_is(FLAIL) {
            return FLAIL;
        }
        return WITHER;
    }
}

// ---- 巨大头颅:开场挂 0 层慢速显示,第五回合起只剩"时候到了" ----

pub const GIANT_HEAD: EnemyDef = EnemyDef {
    id: "giant_head",
    name: "Giant Head",
    kind: EnemyKind::Elite,
    hp: (500, 500),
    moves: &[
        MoveDef {
            name: "Count",
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
            name: "Glare",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerStatus {
                status: Status::Weak,
                n: 1,
            }],
        },
        MoveDef {
            // 第 5 回合首次使用,每回合 +5,最多 +30
            // (账本记的是 amount + per_turn * min(回合数-1, cap),
            //  所以这里把起点折进 amount、cap 取 10 才等于 30 + 5*min(回合数-5, 6))
            name: "It Is Time",
            intent: Intent::Attack {
                damage: 30,
                times: 1,
            },
            effects: &[EnemyFx::AttackScaling {
                amount: 10,
                per_turn: 5,
                cap: 10,
                times: 1,
            }],
        },
    ],
    pick: pick_giant_head,
    innate: &[(Status::Slow, 0)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 掷招时已经行动过 4 回合(即第 5 回合要用)就"时候到了",之前数数/瞪视
fn pick_giant_head(ctx: &mut PickCtx) -> usize {
    const COUNT: usize = 0;
    const GLARE: usize = 1;
    const IT_IS_TIME: usize = 2;
    if ctx.turn() >= 4 {
        return IT_IS_TIME;
    }
    let roll = ctx.roll();
    if roll < 50 {
        if ctx.last_two_is(GLARE) {
            COUNT
        } else {
            GLARE
        }
    } else if ctx.last_two_is(COUNT) {
        GLARE
    } else {
        COUNT
    }
}

// ---- 复仇女神:隔一回合补一次无形,长柄镰刀与三连击都有历史限制 ----

pub const NEMESIS: EnemyDef = EnemyDef {
    id: "nemesis",
    name: "Nemesis",
    kind: EnemyKind::Elite,
    hp: (185, 185),
    moves: &[
        MoveDef {
            name: "Tri Attack",
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
            name: "Scythe",
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
            name: "Tri Burn",
            intent: Intent::Debuff,
            effects: &[EnemyFx::PlayerCard {
                card: "burn",
                spot: CardSpot::Discard,
                n: 3,
            }],
        },
    ],
    pick: pick_nemesis,
    innate: &[],
    start_block: 0,
    on_death: &[],
    special: Special::Intangible,
    spawn: spawn_default,
};

/// 30% 镰刀(最近两招里没有才行)/ 35% 三连击 / 其余三张燃烧,互相都有限制
fn pick_nemesis(ctx: &mut PickCtx) -> usize {
    const ATTACK: usize = 0;
    const SCYTHE: usize = 1;
    const DEBUFF: usize = 2;
    let roll = ctx.roll();
    if ctx.first_turn() {
        return if roll < 50 { ATTACK } else { DEBUFF };
    }
    if roll < 30 {
        if !ctx.last_two_has(SCYTHE) {
            return SCYTHE;
        }
        if ctx.flip(1, 2) {
            if ctx.last_two_is(ATTACK) {
                DEBUFF
            } else {
                ATTACK
            }
        } else if ctx.last_is(DEBUFF) {
            ATTACK
        } else {
            DEBUFF
        }
    } else if roll < 65 {
        if !ctx.last_two_is(ATTACK) {
            return ATTACK;
        }
        let coin = ctx.flip(1, 2);
        if !coin || ctx.last_two_has(SCYTHE) {
            DEBUFF
        } else {
            SCYTHE
        }
    } else {
        if !ctx.last_is(DEBUFF) {
            return DEBUFF;
        }
        if ctx.flip(1, 2) && !ctx.last_two_has(SCYTHE) {
            SCYTHE
        } else {
            ATTACK
        }
    }
}

// ---- 蛇术士:召唤小刀,刀满四把就改成咬 ----

pub const REPTOMANCER: EnemyDef = EnemyDef {
    id: "reptomancer",
    name: "Reptomancer",
    kind: EnemyKind::Elite,
    hp: (180, 190),
    moves: &[
        MoveDef {
            name: "Summon",
            intent: Intent::Unknown,
            // 自己的槽位是 2;小刀按 4、1、3、0 的顺序填空槽(参考实现)
            effects: &[EnemyFx::Summon {
                ids: &["dagger"],
                slots: &[4, 1, 3, 0],
            }],
        },
        MoveDef {
            name: "Snake Strike",
            intent: Intent::AttackDebuff {
                damage: 13,
                times: 2,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 13,
                    times: 2,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Big Bite",
            intent: Intent::Attack {
                damage: 30,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 30,
                times: 1,
            }],
        },
    ],
    pick: pick_reptomancer,
    innate: &[(Status::MinionLeader, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Leader,
    spawn: spawn_default,
};

/// 首招必召唤;之后按 spec 的回退循环掷,场上小刀满四把就不再召唤
fn pick_reptomancer(ctx: &mut PickCtx) -> usize {
    const SUMMON: usize = 0;
    const SNAKE_STRIKE: usize = 1;
    const BIG_BITE: usize = 2;
    if ctx.first_turn() {
        return SUMMON;
    }
    let can_spawn = ctx
        .all
        .iter()
        .filter(|e| e.def.id == "dagger" && e.alive())
        .count()
        < 4;
    let mut r = ctx.roll();
    loop {
        if r < 33 {
            if !ctx.last_is(SNAKE_STRIKE) {
                return SNAKE_STRIKE;
            }
            r = ctx.range(33, 99);
        }
        if r < 66 {
            if !ctx.last_two_is(SUMMON) && can_spawn {
                return SUMMON;
            }
            return SNAKE_STRIKE;
        }
        if !ctx.last_is(BIG_BITE) {
            return BIG_BITE;
        }
        r = ctx.range(0, 65);
    }
}

// ---- 小刀:固定刺一刀再自爆,主怪死了就跟着退场 ----

pub const DAGGER: EnemyDef = EnemyDef {
    id: "dagger",
    name: "Dagger",
    kind: EnemyKind::Normal,
    hp: (20, 25),
    moves: &[
        MoveDef {
            name: "Stab",
            intent: Intent::AttackDebuff {
                damage: 9,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 9,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "wound",
                    spot: CardSpot::Discard,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Explode",
            intent: Intent::Attack {
                damage: 25,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 25,
                    times: 1,
                },
                EnemyFx::Suicide,
            ],
        },
    ],
    pick: pick_dagger,
    innate: &[(Status::Minion, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 刺一刀之后必自爆
fn pick_dagger(ctx: &mut PickCtx) -> usize {
    const STAB: usize = 0;
    const EXPLODE: usize = 1;
    if ctx.last_is(STAB) {
        EXPLODE
    } else {
        STAB
    }
}

// ============================ Boss ============================

// ---- 觉醒者:一阶段打死只是半死,复活后血满、力量保留、带小怪领袖 ----

pub const AWAKENED_ONE: EnemyDef = EnemyDef {
    id: "awakened_one",
    name: "Awakened One",
    kind: EnemyKind::Boss,
    hp: (300, 300),
    moves: &[
        MoveDef {
            name: "Slash",
            intent: Intent::Attack {
                damage: 20,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 20,
                times: 1,
            }],
        },
        MoveDef {
            name: "Soul Strike",
            intent: Intent::Attack {
                damage: 6,
                times: 4,
            },
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 4,
            }],
        },
        MoveDef {
            // 复活招式:血补满(半死标记与二阶段由框架接手)
            name: "Rebirth",
            intent: Intent::Unknown,
            effects: &[EnemyFx::Heal {
                n: 9999,
                scope: Scope::SelfOnly,
            }],
        },
        MoveDef {
            name: "Dark Echo",
            intent: Intent::Attack {
                damage: 40,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 40,
                times: 1,
            }],
        },
        MoveDef {
            name: "Sludge",
            intent: Intent::AttackDebuff {
                damage: 18,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 18,
                    times: 1,
                },
                EnemyFx::PlayerCard {
                    card: "void",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Tackle",
            intent: Intent::Attack {
                damage: 10,
                times: 3,
            },
            effects: &[EnemyFx::Attack {
                amount: 10,
                times: 3,
            }],
        },
    ],
    pick: pick_awakened_one,
    innate: &[(Status::Regenerate, 10), (Status::Curiosity, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::Rebirth,
    spawn: spawn_default,
};

/// 半死必复活;一阶段开场斩击、魂击不许连二;二阶段泥浆/乱打交替
fn pick_awakened_one(ctx: &mut PickCtx) -> usize {
    const SLASH: usize = 0;
    const SOUL_STRIKE: usize = 1;
    const REBIRTH: usize = 2;
    const DARK_ECHO: usize = 3;
    const SLUDGE: usize = 4;
    const TACKLE: usize = 5;
    if ctx.state.half_dead {
        return REBIRTH;
    }
    // 复活之后的下一招写死是黑暗回声
    if ctx.last_is(REBIRTH) {
        return DARK_ECHO;
    }
    let roll = ctx.roll();
    if !ctx.state.phase2 {
        if ctx.first_turn() {
            return SLASH;
        }
        if roll < 25 {
            return if ctx.last_is(SOUL_STRIKE) {
                SLASH
            } else {
                SOUL_STRIKE
            };
        }
        return if ctx.last_two_is(SLASH) {
            SOUL_STRIKE
        } else {
            SLASH
        };
    }
    if roll < 50 {
        if ctx.last_two_is(SLUDGE) {
            TACKLE
        } else {
            SLUDGE
        }
    } else if ctx.last_two_is(TACKLE) {
        SLUDGE
    } else {
        TACKLE
    }
}

// ---- 时间吞噬者:玩家打出第 12 张牌就断其回合;半血后急速一次 ----

pub const TIME_EATER: EnemyDef = EnemyDef {
    id: "time_eater",
    name: "Time Eater",
    kind: EnemyKind::Boss,
    hp: (456, 456),
    moves: &[
        MoveDef {
            name: "Reverberate",
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
            name: "Head Slam",
            intent: Intent::AttackDebuff {
                damage: 26,
                times: 1,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 26,
                    times: 1,
                },
                EnemyFx::DrawReduction { n: 1 },
            ],
        },
        MoveDef {
            name: "Ripple",
            intent: Intent::DefendDebuff { block: 20 },
            effects: &[
                EnemyFx::Block {
                    amount: 20,
                    scope: Scope::SelfOnly,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Vulnerable,
                    n: 1,
                },
            ],
        },
        MoveDef {
            // 半血时补到一半血并清掉自己的减益,一场一次
            name: "Haste",
            intent: Intent::Buff,
            effects: &[EnemyFx::HealToHalf, EnemyFx::ClearDebuffs],
        },
    ],
    pick: pick_time_eater,
    innate: &[(Status::TimeWarp, 0)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 半血以下第一次掷招用急速;之后 45/35/20 三段,段内还要重掷
fn pick_time_eater(ctx: &mut PickCtx) -> usize {
    const REVERBERATE: usize = 0;
    const HEAD_SLAM: usize = 1;
    const RIPPLE: usize = 2;
    const HASTE: usize = 3;
    if !ctx.state.haste_used && ctx.hp() < ctx.max_hp() / 2 {
        ctx.state.haste_used = true;
        return HASTE;
    }
    let mut r = ctx.roll();
    if r < 45 {
        if !ctx.last_two_is(REVERBERATE) {
            return REVERBERATE;
        }
        r = ctx.range(50, 99);
    }
    if r < 80 {
        if !ctx.last_is(HEAD_SLAM) {
            return HEAD_SLAM;
        }
        return if ctx.flip(66, 100) {
            REVERBERATE
        } else {
            RIPPLE
        };
    }
    if ctx.last_is(RIPPLE) {
        return if ctx.range(0, 74) < 45 {
            REVERBERATE
        } else {
            HEAD_SLAM
        };
    }
    RIPPLE
}

// ---- 多努:开场给全队加力量,之后与光束严格交替 ----

pub const DONU: EnemyDef = EnemyDef {
    id: "donu",
    name: "Donu",
    kind: EnemyKind::Boss,
    hp: (250, 250),
    moves: &[
        MoveDef {
            name: "Circle of Power",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 3,
                scope: Scope::Team,
            }],
        },
        MoveDef {
            name: "Beam",
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
    pick: pick_donu,
    innate: &[(Status::Artifact, 2)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 首招加力量,之后加力量 / 光束交替
fn pick_donu(ctx: &mut PickCtx) -> usize {
    const CIRCLE_OF_POWER: usize = 0;
    const BEAM: usize = 1;
    if ctx.first_turn() {
        return CIRCLE_OF_POWER;
    }
    if ctx.last_is(CIRCLE_OF_POWER) {
        BEAM
    } else {
        CIRCLE_OF_POWER
    }
}

// ---- 迪卡:开场光束,之后与团队护盾交替 ----

pub const DECA: EnemyDef = EnemyDef {
    id: "deca",
    name: "Deca",
    kind: EnemyKind::Boss,
    hp: (250, 250),
    moves: &[
        MoveDef {
            name: "Beam",
            intent: Intent::AttackDebuff {
                damage: 10,
                times: 2,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 10,
                    times: 2,
                },
                EnemyFx::PlayerCard {
                    card: "dazed",
                    spot: CardSpot::Discard,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Square of Protection",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 16,
                scope: Scope::Team,
            }],
        },
    ],
    pick: pick_deca,
    innate: &[(Status::Artifact, 2)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 首招光束,之后光束 / 团队护盾交替
fn pick_deca(ctx: &mut PickCtx) -> usize {
    const BEAM: usize = 0;
    const SQUARE: usize = 1;
    if ctx.first_turn() {
        return BEAM;
    }
    if ctx.last_is(BEAM) {
        SQUARE
    } else {
        BEAM
    }
}

// ---- 高塔盾卫:每三回合重砸一次,砸之前先撞/固守各一次(先后各半) ----

pub const SPIRE_SHIELD: EnemyDef = EnemyDef {
    id: "spire_shield",
    name: "Spire Shield",
    kind: EnemyKind::Elite,
    hp: (110, 110),
    moves: &[
        MoveDef {
            name: "Bash",
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
                    status: Status::Strength,
                    n: -1,
                },
            ],
        },
        MoveDef {
            name: "Fortify",
            intent: Intent::Defend,
            effects: &[EnemyFx::Block {
                amount: 30,
                scope: Scope::Team,
            }],
        },
        MoveDef {
            // 拿到的格挡等于这一下打出的伤害
            name: "Smash",
            intent: Intent::AttackDefend {
                damage: 34,
                times: 1,
                block: 0,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 34,
                    times: 1,
                },
                EnemyFx::BlockFromDamage,
            ],
        },
    ],
    pick: pick_spire_shield,
    innate: &[(Status::Artifact, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 开局与重砸之后按 50/50 定先撞还是先固守;两块之间的下一招是重砸
fn pick_spire_shield(ctx: &mut PickCtx) -> usize {
    const BASH: usize = 0;
    const FORTIFY: usize = 1;
    const SMASH: usize = 2;
    // 开局和上一招是重砸:掷一次五五开
    if ctx.first_turn() || ctx.last_is(SMASH) {
        return if ctx.flip(1, 2) { FORTIFY } else { BASH };
    }
    // 撞/固守之后:前一步是重砸(或刚开始)就走"三回合一块"的另一招,否则接重砸
    let started_block = ctx.prev_is(SMASH) || ctx.prev().is_none();
    if ctx.last_is(BASH) {
        if started_block {
            FORTIFY
        } else {
            SMASH
        }
    } else if started_block {
        BASH
    } else {
        SMASH
    }
}

// ---- 高塔矛手:第二回合起每三回合穿心一次,间插燃击/刺穿 ----

pub const SPIRE_SPEAR: EnemyDef = EnemyDef {
    id: "spire_spear",
    name: "Spire Spear",
    kind: EnemyKind::Elite,
    hp: (160, 160),
    moves: &[
        MoveDef {
            name: "Burn Strike",
            intent: Intent::AttackDebuff {
                damage: 5,
                times: 2,
            },
            effects: &[
                EnemyFx::Attack {
                    amount: 5,
                    times: 2,
                },
                EnemyFx::PlayerCard {
                    card: "burn",
                    spot: CardSpot::Discard,
                    n: 2,
                },
            ],
        },
        MoveDef {
            name: "Piercer",
            intent: Intent::Buff,
            effects: &[EnemyFx::GainStatus {
                status: Status::Strength,
                n: 2,
                scope: Scope::Team,
            }],
        },
        MoveDef {
            name: "Skewer",
            intent: Intent::Attack {
                damage: 10,
                times: 3,
            },
            effects: &[EnemyFx::Attack {
                amount: 10,
                times: 3,
            }],
        },
    ],
    pick: pick_spire_spear,
    innate: &[(Status::Artifact, 1)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 首招燃击;穿心之后 50/50 定先燃击还是先刺穿;两块之间下一步是穿心
fn pick_spire_spear(ctx: &mut PickCtx) -> usize {
    const BURN_STRIKE: usize = 0;
    const PIERCER: usize = 1;
    const SKEWER: usize = 2;
    if ctx.first_turn() {
        return BURN_STRIKE;
    }
    if ctx.last_is(SKEWER) {
        return if ctx.flip(1, 2) { PIERCER } else { BURN_STRIKE };
    }
    if ctx.last_is(BURN_STRIKE) {
        if ctx.prev_is(SKEWER) {
            PIERCER
        } else {
            SKEWER
        }
    } else if ctx.prev_is(SKEWER) {
        BURN_STRIKE
    } else {
        SKEWER
    }
}

// ---- 腐化之心:开场三减益加五种负面牌,每三回合递增一次增益 ----

pub const CORRUPT_HEART: EnemyDef = EnemyDef {
    id: "corrupt_heart",
    name: "Corrupt Heart",
    kind: EnemyKind::Boss,
    hp: (750, 750),
    moves: &[
        MoveDef {
            name: "Debilitate",
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
                EnemyFx::PlayerStatus {
                    status: Status::Frail,
                    n: 2,
                },
                EnemyFx::PlayerCard {
                    card: "dazed",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
                EnemyFx::PlayerCard {
                    card: "slimed",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
                EnemyFx::PlayerCard {
                    card: "wound",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
                EnemyFx::PlayerCard {
                    card: "burn",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
                EnemyFx::PlayerCard {
                    card: "void",
                    spot: CardSpot::DrawShuffle,
                    n: 1,
                },
            ],
        },
        MoveDef {
            name: "Blood Shots",
            intent: Intent::Attack {
                damage: 2,
                times: 12,
            },
            effects: &[EnemyFx::Attack {
                amount: 2,
                times: 12,
            }],
        },
        MoveDef {
            name: "Echo",
            intent: Intent::Attack {
                damage: 40,
                times: 1,
            },
            effects: &[EnemyFx::Attack {
                amount: 40,
                times: 1,
            }],
        },
        MoveDef {
            // 先清掉负力量再加 2,随后按档数递增增益
            name: "Buff",
            intent: Intent::Buff,
            effects: &[EnemyFx::ResetStrength { n: 2 }, EnemyFx::Escalate],
        },
    ],
    pick: pick_corrupt_heart,
    innate: &[(Status::BeatOfDeath, 1), (Status::Invincible, 300)],
    start_block: 0,
    on_death: &[],
    special: Special::None,
    spawn: spawn_default,
};

/// 首招三减益;两击之间各半,已行动回合数是 3 的倍数就接递增增益
fn pick_corrupt_heart(ctx: &mut PickCtx) -> usize {
    const DEBILITATE: usize = 0;
    const BLOOD_SHOTS: usize = 1;
    const ECHO: usize = 2;
    const BUFF: usize = 3;
    if ctx.first_turn() {
        return DEBILITATE;
    }
    if ctx.last_is(DEBILITATE) || ctx.last_is(BUFF) {
        return if ctx.flip(1, 2) { BLOOD_SHOTS } else { ECHO };
    }
    if ctx.turn() % 3 == 0 {
        return BUFF;
    }
    if ctx.last_is(BLOOD_SHOTS) {
        ECHO
    } else {
        BLOOD_SHOTS
    }
}

