// 卡池数据.每张牌一个 CardDef;升级信息放在 CardUpgrade 里.
// 数值按"接近原作但不追求逐字复刻"处理,单位以能自洽为准.
use crate::core::card::{CardDef, CardType, CardUpgrade, Cost, Effect, Rarity, Target};
use crate::core::status::Status;

/// 升级覆盖项.只覆盖 cost / text / effects;exhaust、ethereal 等标记沿用基础值.
macro_rules! up {
    ($cost:expr, $text:expr, [$($e:expr),* $(,)?]) => {
        Some(CardUpgrade {
            cost: $cost,
            text: $text,
            effects: Some(&[$($e),*]),
            exhaust: None,
            retain: None,
            ethereal: None,
            innate: None,
        })
    };
}

/// 升级后不再 Exhaust 的写法(秘密技巧/秘密武器/先想后做).
macro_rules! up_no_exhaust {
    ($cost:expr, $text:expr, [$($e:expr),* $(,)?]) => {
        Some(CardUpgrade {
            cost: $cost,
            text: $text,
            effects: Some(&[$($e),*]),
            exhaust: Some(false),
            retain: None,
            ethereal: None,
            innate: None,
        })
    };
}

pub static CARDS: &[CardDef] = &[
    // ---- 基础牌 ----
    CardDef {
        id: "strike",
        name: "Strike",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Basic,
        target: Target::Enemy,
        text: "Deal 6 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 6, times: 1 }],
        upgrade: up!(None, "Deal 9 damage.", [Effect::Damage { amount: 9, times: 1 }]),
    },
    CardDef {
        id: "defend",
        name: "Defend",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Basic,
        target: Target::None,
        text: "Gain 5 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 5 }],
        upgrade: up!(None, "Gain 8 Block.", [Effect::Block { amount: 8 }]),
    },
    CardDef {
        id: "bash",
        name: "Bash",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Basic,
        target: Target::Enemy,
        text: "Deal 8 damage. Apply 2 Vulnerable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 8, times: 1 },
            Effect::AddTargetStatus { status: Status::Vulnerable, n: 2 },
        ],
        upgrade: up!(
            None,
            "Deal 10 damage. Apply 3 Vulnerable.",
            [
                Effect::Damage { amount: 10, times: 1 },
                Effect::AddTargetStatus { status: Status::Vulnerable, n: 3 }
            ]
        ),
    },
    // ---- 状态牌 ----
    CardDef {
        id: "wound",
        name: "Wound",
        cost: Cost::Unplayable,
        kind: CardType::Status,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    CardDef {
        id: "slimed",
        name: "Slimed",
        cost: Cost::Fixed(1),
        kind: CardType::Status,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    CardDef {
        id: "dazed",
        name: "Dazed",
        cost: Cost::Unplayable,
        kind: CardType::Status,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable. Ethereal.",
        exhaust: false,
        ethereal: true,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    CardDef {
        id: "burn",
        name: "Burn",
        cost: Cost::Unplayable,
        kind: CardType::Status,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    // ---- 诅咒牌 ----
    CardDef {
        id: "injury",
        name: "Injury",
        cost: Cost::Unplayable,
        kind: CardType::Curse,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    CardDef {
        id: "clumsy",
        name: "Clumsy",
        cost: Cost::Unplayable,
        kind: CardType::Curse,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    CardDef {
        id: "parasite",
        name: "Parasite",
        cost: Cost::Unplayable,
        kind: CardType::Curse,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Unplayable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[],
        upgrade: None,
    },
    // ---- 普通攻击 ----
    CardDef {
        id: "anger",
        name: "Anger",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 6 damage. Add a copy of this card into your discard pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 6, times: 1 },
            Effect::AddCardToDiscard { id: "anger", n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 8 damage. Add a copy of this card into your discard pile.",
            [
                Effect::Damage { amount: 8, times: 1 },
                Effect::AddCardToDiscard { id: "anger", n: 1 }
            ]
        ),
    },
    CardDef {
        id: "body_slam",
        name: "Body Slam",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal damage equal to your Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageEqualBlock],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Deal damage equal to your Block. Costs 0.",
            [Effect::DamageEqualBlock]
        ),
    },
    CardDef {
        id: "cleave",
        name: "Cleave",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::All,
        text: "Deal 8 damage to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageAll { amount: 8, times: 1 }],
        upgrade: up!(
            None,
            "Deal 11 damage to ALL enemies.",
            [Effect::DamageAll { amount: 11, times: 1 }]
        ),
    },
    CardDef {
        id: "clothesline",
        name: "Clothesline",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 12 damage. Apply 2 Weak.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 12, times: 1 },
            Effect::AddTargetStatus { status: Status::Weak, n: 2 },
        ],
        upgrade: up!(
            None,
            "Deal 14 damage. Apply 3 Weak.",
            [
                Effect::Damage { amount: 14, times: 1 },
                Effect::AddTargetStatus { status: Status::Weak, n: 3 }
            ]
        ),
    },
    CardDef {
        id: "iron_wave",
        name: "Iron Wave",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 5 damage. Gain 5 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 5, times: 1 },
            Effect::Block { amount: 5 },
        ],
        upgrade: up!(
            None,
            "Deal 7 damage. Gain 7 Block.",
            [
                Effect::Damage { amount: 7, times: 1 },
                Effect::Block { amount: 7 }
            ]
        ),
    },
    CardDef {
        id: "pommel_strike",
        name: "Pommel Strike",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 9 damage. Draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 9, times: 1 },
            Effect::Draw { n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 10 damage. Draw 2 cards.",
            [
                Effect::Damage { amount: 10, times: 1 },
                Effect::Draw { n: 2 }
            ]
        ),
    },
    CardDef {
        id: "sword_boomerang",
        name: "Sword Boomerang",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Random,
        text: "Deal 3 damage to a random enemy 3 times.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageRandom { amount: 3, times: 3 }],
        upgrade: up!(
            None,
            "Deal 3 damage to a random enemy 4 times.",
            [Effect::DamageRandom { amount: 3, times: 4 }]
        ),
    },
    CardDef {
        id: "thunderclap",
        name: "Thunderclap",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::All,
        text: "Deal 4 damage and apply 1 Vulnerable to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::DamageAll { amount: 4, times: 1 },
            Effect::AddAllEnemiesStatus { status: Status::Vulnerable, n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 7 damage and apply 1 Vulnerable to ALL enemies.",
            [
                Effect::DamageAll { amount: 7, times: 1 },
                Effect::AddAllEnemiesStatus { status: Status::Vulnerable, n: 1 }
            ]
        ),
    },
    CardDef {
        id: "twin_strike",
        name: "Twin Strike",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 5 damage twice.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 5, times: 2 }],
        upgrade: up!(
            None,
            "Deal 7 damage twice.",
            [Effect::Damage { amount: 7, times: 2 }]
        ),
    },
    CardDef {
        id: "wild_strike",
        name: "Wild Strike",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 12 damage. Add a Wound into your draw pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 12, times: 1 },
            Effect::AddCardToDraw { id: "wound", n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 17 damage. Add a Wound into your draw pile.",
            [
                Effect::Damage { amount: 17, times: 1 },
                Effect::AddCardToDraw { id: "wound", n: 1 }
            ]
        ),
    },
    // ---- 普通技能 ----
    CardDef {
        id: "armaments",
        name: "Armaments",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Gain 5 Block. Upgrade a random card in your hand.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Block { amount: 5 },
            Effect::UpgradeRandomInHand { n: 1 },
        ],
        upgrade: up!(
            None,
            "Gain 5 Block. Upgrade 2 random cards in your hand.",
            [
                Effect::Block { amount: 5 },
                Effect::UpgradeRandomInHand { n: 2 }
            ]
        ),
    },
    CardDef {
        id: "flex",
        name: "Flex",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Gain 2 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus { status: Status::Strength, n: 2 }],
        upgrade: up!(
            None,
            "Gain 4 Strength.",
            [Effect::AddSelfStatus { status: Status::Strength, n: 4 }]
        ),
    },
    CardDef {
        id: "shrug_it_off",
        name: "Shrug It Off",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Gain 8 Block. Draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 8 }, Effect::Draw { n: 1 }],
        upgrade: up!(
            None,
            "Gain 11 Block. Draw 1 card.",
            [Effect::Block { amount: 11 }, Effect::Draw { n: 1 }]
        ),
    },
    CardDef {
        id: "true_grit",
        name: "True Grit",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Gain 7 Block. Exhaust a random card in your hand.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Block { amount: 7 },
            Effect::ExhaustRandomInHand { n: 1 },
        ],
        upgrade: up!(
            None,
            "Gain 9 Block. Exhaust a random card in your hand.",
            [
                Effect::Block { amount: 9 },
                Effect::ExhaustRandomInHand { n: 1 }
            ]
        ),
    },
    CardDef {
        id: "battle_trance",
        name: "Battle Trance",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Draw 3 cards.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Draw { n: 3 }],
        upgrade: up!(None, "Draw 4 cards.", [Effect::Draw { n: 4 }]),
    },
    // ---- 罕见攻击 ----
    CardDef {
        id: "carnage",
        name: "Carnage",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Ethereal. Deal 20 damage.",
        exhaust: false,
        ethereal: true,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 20, times: 1 }],
        upgrade: up!(
            None,
            "Ethereal. Deal 28 damage.",
            [Effect::Damage { amount: 28, times: 1 }]
        ),
    },
    CardDef {
        id: "dropkick",
        name: "Dropkick",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 5 damage. If the enemy is Vulnerable, gain (1) and draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageIfVulnerable {
            amount: 5,
            energy: 1,
            draw: 1,
        }],
        upgrade: up!(
            None,
            "Deal 8 damage. If the enemy is Vulnerable, gain (1) and draw 1 card.",
            [Effect::DamageIfVulnerable {
                amount: 8,
                energy: 1,
                draw: 1
            }]
        ),
    },
    CardDef {
        id: "heavy_blade",
        name: "Heavy Blade",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 14 damage. Strength affects this card 3 times.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageStrengthMult {
            amount: 14,
            mult: 3,
        }],
        upgrade: up!(
            None,
            "Deal 18 damage. Strength affects this card 3 times.",
            [Effect::DamageStrengthMult {
                amount: 18,
                mult: 3
            }]
        ),
    },
    CardDef {
        id: "perfected_strike",
        name: "Perfected Strike",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 6 damage plus 2 damage for each Strike in your deck.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamagePerStrike { base: 6, per: 2 }],
        upgrade: up!(
            None,
            "Deal 6 damage plus 3 damage for each Strike in your deck.",
            [Effect::DamagePerStrike { base: 6, per: 3 }]
        ),
    },
    CardDef {
        id: "rampage",
        name: "Rampage",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal {d} damage. Increase this card's damage by 5 this combat.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::DamageWithBonus {
                amount: 8,
                times: 1,
            },
            Effect::BonusSelf { n: 5 },
        ],
        upgrade: up!(
            None,
            "Deal {d} damage. Increase this card's damage by 8 this combat.",
            [
                Effect::DamageWithBonus {
                    amount: 8,
                    times: 1
                },
                Effect::BonusSelf { n: 8 }
            ]
        ),
    },
    CardDef {
        id: "uppercut",
        name: "Uppercut",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 13 damage. Apply 1 Weak and 1 Vulnerable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage { amount: 13, times: 1 },
            Effect::AddTargetStatus { status: Status::Weak, n: 1 },
            Effect::AddTargetStatus { status: Status::Vulnerable, n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 13 damage. Apply 2 Weak and 2 Vulnerable.",
            [
                Effect::Damage { amount: 13, times: 1 },
                Effect::AddTargetStatus { status: Status::Weak, n: 2 },
                Effect::AddTargetStatus { status: Status::Vulnerable, n: 2 }
            ]
        ),
    },
    CardDef {
        id: "sever_soul",
        name: "Sever Soul",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 16 damage. Exhaust all non-Attack cards in your hand.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::ExhaustNonAttacks { damage: 16 }],
        upgrade: up!(
            None,
            "Deal 22 damage. Exhaust all non-Attack cards in your hand.",
            [Effect::ExhaustNonAttacks { damage: 22 }]
        ),
    },
    // ---- 罕见技能 ----
    CardDef {
        id: "bloodletting",
        name: "Bloodletting",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Lose 3 HP. Gain (2).",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::LoseHp { amount: 3 }, Effect::GainEnergy { n: 2 }],
        upgrade: up!(
            None,
            "Lose 3 HP. Gain (3).",
            [Effect::LoseHp { amount: 3 }, Effect::GainEnergy { n: 3 }]
        ),
    },
    CardDef {
        id: "entrench",
        name: "Entrench",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Double your Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DoubleBlock],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "Double your Block. Costs 1.",
            [Effect::DoubleBlock]
        ),
    },
    CardDef {
        id: "second_wind",
        name: "Second Wind",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Exhaust all non-Attack cards in your hand. Gain 5 Block for each card Exhausted.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::ExhaustNonAttacks { damage: 0 },
            Effect::BlockPerExhausted { per: 5 },
        ],
        upgrade: up!(
            None,
            "Exhaust all non-Attack cards in your hand. Gain 7 Block for each card Exhausted.",
            [
                Effect::ExhaustNonAttacks { damage: 0 },
                Effect::BlockPerExhausted { per: 7 }
            ]
        ),
    },
    CardDef {
        id: "seeing_red",
        name: "Seeing Red",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Exhaust. Gain (2).",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::GainEnergy { n: 2 }],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Exhaust. Gain (2). Costs 0.",
            [Effect::GainEnergy { n: 2 }]
        ),
    },
    CardDef {
        id: "shockwave",
        name: "Shockwave",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::All,
        text: "Exhaust. Apply 3 Weak and 3 Vulnerable to ALL enemies.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::AddAllEnemiesStatus { status: Status::Weak, n: 3 },
            Effect::AddAllEnemiesStatus {
                status: Status::Vulnerable,
                n: 3,
            },
        ],
        upgrade: up!(
            None,
            "Exhaust. Apply 5 Weak and 5 Vulnerable to ALL enemies.",
            [
                Effect::AddAllEnemiesStatus { status: Status::Weak, n: 5 },
                Effect::AddAllEnemiesStatus {
                    status: Status::Vulnerable,
                    n: 5
                }
            ]
        ),
    },
    CardDef {
        id: "disarm",
        name: "Disarm",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Exhaust. The enemy loses 2 Strength.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddTargetStatus {
            status: Status::Strength,
            n: -2,
        }],
        upgrade: up!(
            None,
            "Exhaust. The enemy loses 3 Strength.",
            [Effect::AddTargetStatus {
                status: Status::Strength,
                n: -3
            }]
        ),
    },
    CardDef {
        id: "intimidate",
        name: "Intimidate",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::All,
        text: "Exhaust. Apply 1 Weak to ALL enemies.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddAllEnemiesStatus { status: Status::Weak, n: 1 }],
        upgrade: up!(
            None,
            "Exhaust. Apply 2 Weak to ALL enemies.",
            [Effect::AddAllEnemiesStatus { status: Status::Weak, n: 2 }]
        ),
    },
    // ---- 罕见能力 ----
    CardDef {
        id: "dark_embrace",
        name: "Dark Embrace",
        cost: Cost::Fixed(2),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever a card is Exhausted, draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::DarkEmbrace,
            n: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "Whenever a card is Exhausted, draw 1 card. Costs 1.",
            [Effect::AddSelfStatus {
                status: Status::DarkEmbrace,
                n: 1
            }]
        ),
    },
    CardDef {
        id: "evolve",
        name: "Evolve",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever you draw a Status card, draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Evolve,
            n: 1,
        }],
        upgrade: up!(
            None,
            "Whenever you draw a Status card, draw 2 cards.",
            [Effect::AddSelfStatus {
                status: Status::Evolve,
                n: 2
            }]
        ),
    },
    CardDef {
        id: "feel_no_pain",
        name: "Feel No Pain",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever a card is Exhausted, gain 3 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::FeelNoPain,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Whenever a card is Exhausted, gain 4 Block.",
            [Effect::AddSelfStatus {
                status: Status::FeelNoPain,
                n: 4
            }]
        ),
    },
    CardDef {
        id: "fire_breathing",
        name: "Fire Breathing",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever you draw a Status card, deal 6 damage to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::FireBreathing,
            n: 6,
        }],
        upgrade: up!(
            None,
            "Whenever you draw a Status card, deal 10 damage to ALL enemies.",
            [Effect::AddSelfStatus {
                status: Status::FireBreathing,
                n: 10
            }]
        ),
    },
    CardDef {
        id: "inflame",
        name: "Inflame",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 3 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Strength,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Gain 4 Strength.",
            [Effect::AddSelfStatus {
                status: Status::Strength,
                n: 4
            }]
        ),
    },
    CardDef {
        id: "metallicize",
        name: "Metallicize",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "At the end of your turn, gain 3 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Metallicize,
            n: 3,
        }],
        upgrade: up!(
            None,
            "At the end of your turn, gain 4 Block.",
            [Effect::AddSelfStatus {
                status: Status::Metallicize,
                n: 4
            }]
        ),
    },
    CardDef {
        id: "rupture",
        name: "Rupture",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever you lose HP from a card, gain 1 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Rupture,
            n: 1,
        }],
        upgrade: up!(
            None,
            "Whenever you lose HP from a card, gain 2 Strength.",
            [Effect::AddSelfStatus {
                status: Status::Rupture,
                n: 2
            }]
        ),
    },
    // ---- 稀有牌 ----
    CardDef {
        id: "bludgeon",
        name: "Bludgeon",
        cost: Cost::Fixed(3),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::Enemy,
        text: "Deal 32 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 32, times: 1 }],
        upgrade: up!(
            None,
            "Deal 42 damage.",
            [Effect::Damage { amount: 42, times: 1 }]
        ),
    },
    CardDef {
        id: "fiend_fire",
        name: "Fiend Fire",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::Enemy,
        text: "Exhaust. Exhaust your hand. Deal 7 damage for each card Exhausted.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::ExhaustHand,
            Effect::DamagePerExhausted { per: 7 },
        ],
        upgrade: up!(
            None,
            "Exhaust. Exhaust your hand. Deal 10 damage for each card Exhausted.",
            [
                Effect::ExhaustHand,
                Effect::DamagePerExhausted { per: 10 }
            ]
        ),
    },
    CardDef {
        id: "feed",
        name: "Feed",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::Enemy,
        text: "Exhaust. Deal 10 damage. If this kills a non-Minion enemy, gain 3 Max HP.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageAndKillMaxHp {
            amount: 10,
            times: 1,
            max_hp: 3,
        }],
        upgrade: up!(
            None,
            "Exhaust. Deal 12 damage. If this kills a non-Minion enemy, gain 4 Max HP.",
            [Effect::DamageAndKillMaxHp {
                amount: 12,
                times: 1,
                max_hp: 4
            }]
        ),
    },
    CardDef {
        id: "reaper",
        name: "Reaper",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::All,
        text: "Exhaust. Deal 4 damage to ALL enemies. Heal HP equal to unblocked damage dealt.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Reaper { amount: 4 }],
        upgrade: up!(
            None,
            "Exhaust. Deal 5 damage to ALL enemies. Heal HP equal to unblocked damage dealt.",
            [Effect::Reaper { amount: 5 }]
        ),
    },
    CardDef {
        id: "whirlwind",
        name: "Whirlwind",
        cost: Cost::X,
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::All,
        text: "Deal 5 damage to ALL enemies X times.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageAllX { per: 5 }],
        upgrade: up!(
            None,
            "Deal 8 damage to ALL enemies X times.",
            [Effect::DamageAllX { per: 8 }]
        ),
    },
    CardDef {
        id: "demon_form",
        name: "Demon Form",
        cost: Cost::Fixed(3),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "At the start of your turn, gain 2 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::DemonForm,
            n: 2,
        }],
        upgrade: up!(
            None,
            "At the start of your turn, gain 3 Strength.",
            [Effect::AddSelfStatus {
                status: Status::DemonForm,
                n: 3
            }]
        ),
    },
    CardDef {
        id: "barricade",
        name: "Barricade",
        cost: Cost::Fixed(3),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Block is not removed at the start of your turn.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Barricade,
            n: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(2)),
            "Block is not removed at the start of your turn. Costs 2.",
            [Effect::AddSelfStatus {
                status: Status::Barricade,
                n: 1
            }]
        ),
    },
    CardDef {
        id: "limit_break",
        name: "Limit Break",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Exhaust. Double your Strength.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DoubleSelfStatus(Status::Strength)],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Exhaust. Double your Strength. Costs 0.",
            [Effect::DoubleSelfStatus(Status::Strength)]
        ),
    },
    CardDef {
        id: "offering",
        name: "Offering",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Exhaust. Lose 6 HP. Gain (2). Draw 3 cards.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::LoseHp { amount: 6 },
            Effect::GainEnergy { n: 2 },
            Effect::Draw { n: 3 },
        ],
        upgrade: up!(
            None,
            "Exhaust. Lose 5 HP. Gain (2). Draw 3 cards.",
            [
                Effect::LoseHp { amount: 5 },
                Effect::GainEnergy { n: 2 },
                Effect::Draw { n: 3 }
            ]
        ),
    },
    CardDef {
        id: "impervious",
        name: "Impervious",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Exhaust. Gain 30 Block.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 30 }],
        upgrade: up!(
            None,
            "Exhaust. Gain 40 Block.",
            [Effect::Block { amount: 40 }]
        ),
    },
    CardDef {
        id: "brutality",
        name: "Brutality",
        cost: Cost::Fixed(0),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "At the start of your turn, lose 1 HP and draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Brutality,
            n: 1,
        }],
        upgrade: up!(
            None,
            "At the start of your turn, lose 1 HP and draw 2 cards.",
            [Effect::AddSelfStatus {
                status: Status::Brutality,
                n: 2
            }]
        ),
    },
    // ---- 铁甲战士:补全语料里剩下的 ----
    CardDef {
        id: "ghostly_armor",
        name: "Ghostly Armor",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Ethereal. Gain 10 Block.",
        exhaust: false,
        ethereal: true,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 10 }],
        upgrade: up!(None, "Ethereal. Gain 13 Block.", [Effect::Block { amount: 13 }]),
    },
    CardDef {
        id: "hemokinesis",
        name: "Hemokinesis",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Lose 2 HP. Deal 15 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::LoseHp { amount: 2 },
            Effect::Damage {
                amount: 15,
                times: 1,
            },
        ],
        upgrade: up!(
            None,
            "Lose 2 HP. Deal 20 damage.",
            [
                Effect::LoseHp { amount: 2 },
                Effect::Damage {
                    amount: 20,
                    times: 1
                }
            ]
        ),
    },
    CardDef {
        id: "immolate",
        name: "Immolate",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::All,
        text: "Deal 21 damage to ALL enemies. Add a Burn to your discard pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::DamageAll {
                amount: 21,
                times: 1,
            },
            Effect::AddCardToDiscard { id: "burn", n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 28 damage to ALL enemies. Add a Burn to your discard pile.",
            [
                Effect::DamageAll {
                    amount: 28,
                    times: 1
                },
                Effect::AddCardToDiscard { id: "burn", n: 1 }
            ]
        ),
    },
    CardDef {
        id: "pummel",
        name: "Pummel",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 2 damage 4 times. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage {
            amount: 2,
            times: 4,
        }],
        upgrade: up!(
            None,
            "Deal 2 damage 5 times. Exhaust.",
            [Effect::Damage {
                amount: 2,
                times: 5
            }]
        ),
    },
    CardDef {
        id: "reckless_charge",
        name: "Reckless Charge",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 7 damage. Shuffle a Dazed into your draw pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage {
                amount: 7,
                times: 1,
            },
            Effect::AddCardToDraw { id: "dazed", n: 1 },
        ],
        upgrade: up!(
            None,
            "Deal 10 damage. Shuffle a Dazed into your draw pile.",
            [
                Effect::Damage {
                    amount: 10,
                    times: 1
                },
                Effect::AddCardToDraw { id: "dazed", n: 1 }
            ]
        ),
    },
    CardDef {
        id: "power_through",
        name: "Power Through",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Add 2 Wounds to your hand. Gain 15 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::AddCardToHand { id: "wound", n: 2 },
            Effect::Block { amount: 15 },
        ],
        upgrade: up!(
            None,
            "Add 2 Wounds to your hand. Gain 20 Block.",
            [
                Effect::AddCardToHand { id: "wound", n: 2 },
                Effect::Block { amount: 20 }
            ]
        ),
    },
    CardDef {
        id: "flame_barrier",
        name: "Flame Barrier",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Block { amount: 12 },
            Effect::AddSelfStatus {
                status: Status::FlameBarrier,
                n: 4,
            },
        ],
        upgrade: up!(
            None,
            "Gain 16 Block. Whenever you are attacked this turn, deal 6 damage back.",
            [
                Effect::Block { amount: 16 },
                Effect::AddSelfStatus {
                    status: Status::FlameBarrier,
                    n: 6
                }
            ]
        ),
    },
    CardDef {
        id: "berserk",
        name: "Berserk",
        cost: Cost::Fixed(0),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Gain 2 Vulnerable. At the start of your turn, gain (1).",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::AddSelfStatus {
                status: Status::Vulnerable,
                n: 2,
            },
            Effect::AddSelfStatus {
                status: Status::Berserk,
                n: 1,
            },
        ],
        upgrade: up!(
            None,
            "Gain 1 Vulnerable. At the start of your turn, gain (1).",
            [
                Effect::AddSelfStatus {
                    status: Status::Vulnerable,
                    n: 1
                },
                Effect::AddSelfStatus {
                    status: Status::Berserk,
                    n: 1
                }
            ]
        ),
    },
    CardDef {
        id: "combust",
        name: "Combust",
        cost: Cost::Fixed(1),
        kind: CardType::Power,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "At the end of your turn, lose 1 HP and deal 5 damage to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Combust,
            n: 5,
        }],
        upgrade: up!(
            None,
            "At the end of your turn, lose 1 HP and deal 7 damage to ALL enemies.",
            [Effect::AddSelfStatus {
                status: Status::Combust,
                n: 7
            }]
        ),
    },
    CardDef {
        id: "corruption",
        name: "Corruption",
        cost: Cost::Fixed(3),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Skills cost 0. Whenever you play a Skill, Exhaust it.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Corruption,
            n: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(2)),
            "Skills cost 0. Whenever you play a Skill, Exhaust it. Costs 2.",
            [Effect::AddSelfStatus {
                status: Status::Corruption,
                n: 1
            }]
        ),
    },
    CardDef {
        id: "double_tap",
        name: "Double Tap",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "This turn, your next Attack is played twice.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::DoubleTap,
            n: 1,
        }],
        upgrade: up!(
            None,
            "This turn, your next 2 Attacks are played twice.",
            [Effect::AddSelfStatus {
                status: Status::DoubleTap,
                n: 2
            }]
        ),
    },
    CardDef {
        id: "juggernaut",
        name: "Juggernaut",
        cost: Cost::Fixed(2),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Whenever you gain Block, deal 5 damage to a random enemy.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Juggernaut,
            n: 5,
        }],
        upgrade: up!(
            None,
            "Whenever you gain Block, deal 7 damage to a random enemy.",
            [Effect::AddSelfStatus {
                status: Status::Juggernaut,
                n: 7
            }]
        ),
    },
    CardDef {
        id: "rage",
        name: "Rage",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Whenever you play an Attack this turn, gain 3 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Rage,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Whenever you play an Attack this turn, gain 5 Block.",
            [Effect::AddSelfStatus {
                status: Status::Rage,
                n: 5
            }]
        ),
    },
    CardDef {
        id: "spot_weakness",
        name: "Spot Weakness",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "If the enemy intends to attack, gain 3 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::StrengthIfTargetAttacks { n: 3 }],
        upgrade: up!(None, "If the enemy intends to attack, gain 4 Strength.", [Effect::StrengthIfTargetAttacks { n: 4 }]),
    },
    CardDef {
        id: "sentinel",
        name: "Sentinel",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 5 Block. If this card is Exhausted, gain (2).",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Block { amount: 5 },
            Effect::EnergyOnExhaust { n: 2 },
        ],
        upgrade: up!(
            None,
            "Gain 8 Block. If this card is Exhausted, gain (2).",
            [
                Effect::Block { amount: 8 },
                Effect::EnergyOnExhaust { n: 2 }
            ]
        ),
    },
    // ---- 铁甲战士:需要选牌的那几张 ----
    CardDef {
        id: "burning_pact",
        name: "Burning Pact",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Exhaust a card. Draw 2 cards.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Draw { n: 2 }, Effect::ExhaustFromHand],
        upgrade: up!(None, "Exhaust a card. Draw 3 cards.", [Effect::Draw { n: 3 }, Effect::ExhaustFromHand]),
    },
    CardDef {
        id: "warcry",
        name: "Warcry",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Draw 1 card. Put a card from your hand onto the top of your draw pile. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Draw { n: 1 }, Effect::TopFromHand],
        upgrade: up!(
            None,
            "Draw 2 cards. Put a card from your hand onto the top of your draw pile. Exhaust.",
            [Effect::Draw { n: 2 }, Effect::TopFromHand]
        ),
    },
    CardDef {
        id: "dual_wield",
        name: "Dual Wield",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Copy an Attack or Power card in your hand.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::CopyFromHand],
        upgrade: up!(None, "Copy an Attack or Power card in your hand twice.", [Effect::CopyFromHand, Effect::CopyFromHand]),
    },
    CardDef {
        id: "exhume",
        name: "Exhume",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Put a card from your exhaust pile into your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::FromExhaustToHand],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Put a card from your exhaust pile into your hand. Exhaust. Costs 0.",
            [Effect::FromExhaustToHand]
        ),
    },
    CardDef {
        id: "headbutt",
        name: "Headbutt",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Deal 9 damage. Put a card from your discard pile on top of your draw pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Damage {
                amount: 9,
                times: 1,
            },
            Effect::FromDiscardToDrawTop,
        ],
        upgrade: up!(
            None,
            "Deal 12 damage. Put a card from your discard pile on top of your draw pile.",
            [
                Effect::Damage {
                    amount: 12,
                    times: 1
                },
                Effect::FromDiscardToDrawTop
            ]
        ),
    },
    CardDef {
        id: "clash",
        name: "Clash",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Common,
        target: Target::Enemy,
        text: "Can only be played if every card in your hand is an Attack. Deal 14 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage {
            amount: 14,
            times: 1,
        }],
        upgrade: up!(
            None,
            "Can only be played if every card in your hand is an Attack. Deal 18 damage.",
            [Effect::Damage {
                amount: 18,
                times: 1
            }]
        ),
    },
    CardDef {
        id: "blood_for_blood",
        name: "Blood for Blood",
        cost: Cost::Fixed(4),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Costs (1) less for each time you lose HP this combat. Deal 18 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage {
            amount: 18,
            times: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(3)),
            "Costs (1) less for each time you lose HP this combat. Deal 22 damage. Costs 3.",
            [Effect::Damage {
                amount: 22,
                times: 1
            }]
        ),
    },
    CardDef {
        id: "havoc",
        name: "Havoc",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Common,
        target: Target::None,
        text: "Play the top card of your draw pile and Exhaust it.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::PlayTopOfDraw],
        upgrade: up!(Some(Cost::Fixed(0)), "Play the top card of your draw pile and Exhaust it. Costs 0.", [Effect::PlayTopOfDraw]),
    },
    CardDef {
        id: "infernal_blade",
        name: "Infernal Blade",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Add a random Attack into your hand. It costs 0 this turn. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddRandomAttackToHand],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Add a random Attack into your hand. It costs 0 this turn. Exhaust. Costs 0.",
            [Effect::AddRandomAttackToHand]
        ),
    },
    CardDef {
        id: "searing_blow",
        name: "Searing Blow",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal {d} damage. Can be upgraded any number of times.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: true,
        effects: &[Effect::DamageWithBonus {
            amount: 12,
            times: 1,
        }],
        upgrade: up!(
            None,
            "Deal {d} damage. Can be upgraded any number of times.",
            [Effect::DamageWithBonus {
                amount: 12,
                times: 1
            }]
        ),
    },
    // ---- 无色牌:普通 ----
    CardDef {
        id: "bandage_up",
        name: "Bandage Up",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Heal 4 HP. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Heal { amount: 4 }],
        upgrade: up!(None, "Heal 6 HP. Exhaust.", [Effect::Heal { amount: 6 }]),
    },
    CardDef {
        id: "blind",
        name: "Blind",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Apply 2 Weak.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddTargetStatus {
            status: Status::Weak,
            n: 2,
        }],
        upgrade: up!(
            None,
            "Apply 2 Weak to ALL enemies.",
            [Effect::AddAllEnemiesStatus {
                status: Status::Weak,
                n: 2
            }]
        ),
    },
    CardDef {
        id: "dark_shackles",
        name: "Dark Shackles",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Enemy loses 9 Strength this turn. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::TargetLoseStrengthThisTurn { n: 9 }],
        upgrade: up!(
            None,
            "Enemy loses 15 Strength this turn. Exhaust.",
            [Effect::TargetLoseStrengthThisTurn { n: 15 }]
        ),
    },
    CardDef {
        id: "deep_breath",
        name: "Deep Breath",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Shuffle your discard pile into your draw pile. Draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::ShuffleDiscardIntoDraw, Effect::Draw { n: 1 }],
        upgrade: up!(
            None,
            "Shuffle your discard pile into your draw pile. Draw 2 cards.",
            [Effect::ShuffleDiscardIntoDraw, Effect::Draw { n: 2 }]
        ),
    },
    CardDef {
        id: "discovery",
        name: "Discovery",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Choose 1 of 3 random cards to add into your hand. It costs 0 this turn. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::OfferRandomCardsFromClass { n: 3 }],
        upgrade: None,
    },
    CardDef {
        id: "dramatic_entrance",
        name: "Dramatic Entrance",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::All,
        text: "Innate. Deal 8 damage to ALL enemies. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: true,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageAll {
            amount: 8,
            times: 1,
        }],
        upgrade: up!(
            None,
            "Innate. Deal 12 damage to ALL enemies. Exhaust.",
            [Effect::DamageAll {
                amount: 12,
                times: 1
            }]
        ),
    },
    CardDef {
        id: "enlightenment",
        name: "Enlightenment",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Reduce the cost of all cards in your hand to 1 this turn.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::CapHandCost {
            cap: 1,
            combat: false,
        }],
        upgrade: up!(
            None,
            "Reduce the cost of all cards in your hand to 1 this combat.",
            [Effect::CapHandCost {
                cap: 1,
                combat: true
            }]
        ),
    },
    CardDef {
        id: "finesse",
        name: "Finesse",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 2 Block. Draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 2 }, Effect::Draw { n: 1 }],
        upgrade: up!(
            None,
            "Gain 4 Block. Draw 1 card.",
            [Effect::Block { amount: 4 }, Effect::Draw { n: 1 }]
        ),
    },
    CardDef {
        id: "flash_of_steel",
        name: "Flash of Steel",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 3 damage. Draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 3, times: 1 }, Effect::Draw { n: 1 }],
        upgrade: up!(
            None,
            "Deal 6 damage. Draw 1 card.",
            [Effect::Damage { amount: 6, times: 1 }, Effect::Draw { n: 1 }]
        ),
    },
    CardDef {
        id: "forethought",
        name: "Forethought",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Put a card from your hand to the bottom of your draw pile. It costs 0 until played.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::ToDrawBottomFromHand { n: 1 }],
        upgrade: up!(
            None,
            "Put any number of cards from your hand to the bottom of your draw pile. They cost 0 until played.",
            [Effect::ToDrawBottomFromHand { n: 0 }]
        ),
    },
    CardDef {
        id: "good_instincts",
        name: "Good Instincts",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 6 Block.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Block { amount: 6 }],
        upgrade: up!(None, "Gain 9 Block.", [Effect::Block { amount: 9 }]),
    },
    CardDef {
        id: "impatience",
        name: "Impatience",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "If you have no Attacks in your hand, draw 2 cards.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DrawIfNoAttacks { n: 2 }],
        upgrade: up!(
            None,
            "If you have no Attacks in your hand, draw 3 cards.",
            [Effect::DrawIfNoAttacks { n: 3 }]
        ),
    },
    CardDef {
        id: "jack_of_all_trades",
        name: "Jack of All Trades",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Add 1 random Colorless card into your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddRandomColorlessToHand {
            n: 1,
            free: false,
            upgraded: false,
        }],
        upgrade: up!(
            None,
            "Add 2 random Colorless cards into your hand. Exhaust.",
            [Effect::AddRandomColorlessToHand {
                n: 2,
                free: false,
                upgraded: false
            }]
        ),
    },
    CardDef {
        id: "madness",
        name: "Madness",
        cost: Cost::Fixed(1),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Reduce the cost of a random card in your hand to 0 this combat. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::FreeRandomInHand],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Reduce the cost of a random card in your hand to 0 this combat. Exhaust. Costs 0.",
            [Effect::FreeRandomInHand]
        ),
    },
    CardDef {
        id: "mind_blast",
        name: "Mind Blast",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Innate. Deal damage equal to the number of cards in your draw pile.",
        exhaust: false,
        ethereal: false,
        innate: true,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamagePerDrawPile { per: 1 }],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "Innate. Deal damage equal to the number of cards in your draw pile. Costs 1.",
            [Effect::DamagePerDrawPile { per: 1 }]
        ),
    },
    CardDef {
        id: "panacea",
        name: "Panacea",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 1 Artifact. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Artifact,
            n: 1,
        }],
        upgrade: up!(
            None,
            "Gain 2 Artifact. Exhaust.",
            [Effect::AddSelfStatus {
                status: Status::Artifact,
                n: 2
            }]
        ),
    },
    CardDef {
        id: "panic_button",
        name: "Panic Button",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Gain 30 Block. You cannot gain Block from cards for 2 turns. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[
            Effect::Block { amount: 30 },
            Effect::AddSelfStatus {
                status: Status::NoBlock,
                n: 2,
            },
        ],
        upgrade: up!(
            None,
            "Gain 40 Block. You cannot gain Block from cards for 2 turns. Exhaust.",
            [
                Effect::Block { amount: 40 },
                Effect::AddSelfStatus {
                    status: Status::NoBlock,
                    n: 2
                }
            ]
        ),
    },
    CardDef {
        id: "purity",
        name: "Purity",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::None,
        text: "Exhaust up to 3 cards in your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::ExhaustUpTo { n: 3 }],
        upgrade: up!(
            None,
            "Exhaust up to 5 cards in your hand. Exhaust.",
            [Effect::ExhaustUpTo { n: 5 }]
        ),
    },
    CardDef {
        id: "swift_strike",
        name: "Swift Strike",
        cost: Cost::Fixed(0),
        kind: CardType::Attack,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Deal 7 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Damage { amount: 7, times: 1 }],
        upgrade: up!(None, "Deal 10 damage.", [Effect::Damage { amount: 10, times: 1 }]),
    },
    CardDef {
        id: "trip",
        name: "Trip",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        text: "Apply 2 Vulnerable.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddTargetStatus {
            status: Status::Vulnerable,
            n: 2,
        }],
        upgrade: up!(
            None,
            "Apply 2 Vulnerable to ALL enemies.",
            [Effect::AddAllEnemiesStatus {
                status: Status::Vulnerable,
                n: 2
            }]
        ),
    },
    // ---- 无色牌:稀有 ----
    CardDef {
        id: "apotheosis",
        name: "Apotheosis",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Upgrade ALL your cards for the rest of combat. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::UpgradeAllForCombat],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "Upgrade ALL your cards for the rest of combat. Exhaust. Costs 1.",
            [Effect::UpgradeAllForCombat]
        ),
    },
    CardDef {
        id: "chrysalis",
        name: "Chrysalis",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Shuffle 3 random Skills into your draw pile. They cost 0 this combat. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddRandomToDrawFree {
            kind: CardType::Skill,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Shuffle 5 random Skills into your draw pile. They cost 0 this combat. Exhaust.",
            [Effect::AddRandomToDrawFree {
                kind: CardType::Skill,
                n: 5
            }]
        ),
    },
    CardDef {
        id: "hand_of_greed",
        name: "Hand of Greed",
        cost: Cost::Fixed(2),
        kind: CardType::Attack,
        rarity: Rarity::Rare,
        target: Target::Enemy,
        text: "Deal 20 damage. If Fatal, gain 20 Gold.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::DamageAndGoldOnKill {
            amount: 20,
            times: 1,
            gold: 20,
        }],
        upgrade: up!(
            None,
            "Deal 25 damage. If Fatal, gain 25 Gold.",
            [Effect::DamageAndGoldOnKill {
                amount: 25,
                times: 1,
                gold: 25
            }]
        ),
    },
    CardDef {
        id: "magnetism",
        name: "Magnetism",
        cost: Cost::Fixed(2),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "At the start of your turn, add a random Colorless card into your hand.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Magnetism,
            n: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "At the start of your turn, add a random Colorless card into your hand. Costs 1.",
            [Effect::AddSelfStatus {
                status: Status::Magnetism,
                n: 1
            }]
        ),
    },
    CardDef {
        id: "master_of_strategy",
        name: "Master of Strategy",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Draw 3 cards. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Draw { n: 3 }],
        upgrade: up!(None, "Draw 4 cards. Exhaust.", [Effect::Draw { n: 4 }]),
    },
    CardDef {
        id: "mayhem",
        name: "Mayhem",
        cost: Cost::Fixed(2),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "At the start of your turn, play the top card of your draw pile.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Mayhem,
            n: 1,
        }],
        upgrade: up!(
            Some(Cost::Fixed(1)),
            "At the start of your turn, play the top card of your draw pile. Costs 1.",
            [Effect::AddSelfStatus {
                status: Status::Mayhem,
                n: 1
            }]
        ),
    },
    CardDef {
        id: "metamorphosis",
        name: "Metamorphosis",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Shuffle 3 random Attacks into your draw pile. They cost 0 this combat. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddRandomToDrawFree {
            kind: CardType::Attack,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Shuffle 5 random Attacks into your draw pile. They cost 0 this combat. Exhaust.",
            [Effect::AddRandomToDrawFree {
                kind: CardType::Attack,
                n: 5
            }]
        ),
    },
    CardDef {
        id: "panache",
        name: "Panache",
        cost: Cost::Fixed(0),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Every time you play 5 cards in a single turn, deal 10 damage to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::Panache,
            n: 10,
        }],
        upgrade: up!(
            None,
            "Every time you play 5 cards in a single turn, deal 14 damage to ALL enemies.",
            [Effect::AddSelfStatus {
                status: Status::Panache,
                n: 14
            }]
        ),
    },
    CardDef {
        id: "sadistic_nature",
        name: "Sadistic Nature",
        cost: Cost::Fixed(0),
        kind: CardType::Power,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Whenever you apply a debuff to an enemy, they take 5 damage.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddSelfStatus {
            status: Status::SadisticNature,
            n: 5,
        }],
        upgrade: up!(
            None,
            "Whenever you apply a debuff to an enemy, they take 7 damage.",
            [Effect::AddSelfStatus {
                status: Status::SadisticNature,
                n: 7
            }]
        ),
    },
    CardDef {
        id: "secret_technique",
        name: "Secret Technique",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Put a Skill from your draw pile into your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::TakeFromDrawToHand {
            kind: CardType::Skill,
        }],
        upgrade: up_no_exhaust!(
            None,
            "Put a Skill from your draw pile into your hand.",
            [Effect::TakeFromDrawToHand {
                kind: CardType::Skill
            }]
        ),
    },
    CardDef {
        id: "secret_weapon",
        name: "Secret Weapon",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Put an Attack from your draw pile into your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::TakeFromDrawToHand {
            kind: CardType::Attack,
        }],
        upgrade: up_no_exhaust!(
            None,
            "Put an Attack from your draw pile into your hand.",
            [Effect::TakeFromDrawToHand {
                kind: CardType::Attack
            }]
        ),
    },
    CardDef {
        id: "the_bomb",
        name: "The Bomb",
        cost: Cost::Fixed(2),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "At the end of 3 turns, deal 40 damage to ALL enemies.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Bomb {
            turns: 3,
            damage: 40,
        }],
        upgrade: up!(
            None,
            "At the end of 3 turns, deal 50 damage to ALL enemies.",
            [Effect::Bomb {
                turns: 3,
                damage: 50
            }]
        ),
    },
    CardDef {
        id: "thinking_ahead",
        name: "Thinking Ahead",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Draw 2 cards. Put a card from your hand on top of your draw pile. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::Draw { n: 2 }, Effect::TopFromHand],
        upgrade: up_no_exhaust!(
            None,
            "Draw 2 cards. Put a card from your hand on top of your draw pile.",
            [Effect::Draw { n: 2 }, Effect::TopFromHand]
        ),
    },
    CardDef {
        id: "transmutation",
        name: "Transmutation",
        cost: Cost::X,
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Add X random Colorless cards into your hand. They cost 0 this turn. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::AddRandomColorlessXToHand { upgraded: false }],
        upgrade: up!(
            None,
            "Add X random Upgraded Colorless cards into your hand. They cost 0 this turn. Exhaust.",
            [Effect::AddRandomColorlessXToHand { upgraded: true }]
        ),
    },
    CardDef {
        id: "violence",
        name: "Violence",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Rare,
        target: Target::None,
        text: "Put 3 random Attacks from your draw pile into your hand. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        effects: &[Effect::RandomFromDrawToHand {
            kind: CardType::Attack,
            n: 3,
        }],
        upgrade: up!(
            None,
            "Put 4 random Attacks from your draw pile into your hand. Exhaust.",
            [Effect::RandomFromDrawToHand {
                kind: CardType::Attack,
                n: 4
            }]
        ),
    },
];

/// 语料里这条卡牌的记录(展示数据以它为准)
fn corpus_entry(def: &CardDef) -> Option<&'static crate::core::corpus::CardInfo> {
    let all = crate::core::corpus::CARDS;
    all.iter()
        .find(|c| c.id == def.id && c.color == "red")
        .or_else(|| all.iter().find(|c| c.id == def.id))
}

/// 卡牌属于哪个颜色(费用括号上色用):查语料,红绿蓝紫白各按自己,其它灰
pub fn color_key(def: &CardDef) -> &'static str {
    match corpus_entry(def).map(|c| c.color) {
        Some("red") => "red",
        Some("green") => "green",
        Some("blue") => "blue",
        Some("purple") => "purple",
        Some("colorless") => "white",
        _ => "gray",
    }
}

/// 语料里的卡池归属:class / colorless / special / curse ...
pub fn pool_of(def: &CardDef) -> &'static str {
    corpus_entry(def).map(|c| c.pool).unwrap_or("")
}

/// 无色牌池(不含各职业衍生出来的 special token)
pub fn colorless_pool() -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| pool_of(c) == "colorless")
        .collect()
}

/// 本职业里某一类型的牌(化茧/变形洗进抽牌堆的那些)
pub fn class_pool_of_kind(kind: CardType) -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| c.kind == kind && pool_of(c) == "class")
        .collect()
}

/// 发现用的本职业牌池:攻击/技能/能力,不含基础牌
pub fn class_card_pool() -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| {
            pool_of(c) == "class"
                && c.rarity != Rarity::Basic
                && matches!(c.kind, CardType::Attack | CardType::Skill | CardType::Power)
        })
        .collect()
}

pub fn card_def(id: &str) -> Option<&'static CardDef> {
    CARDS.iter().find(|c| c.id == id)
}

/// 按 id 找定义,找不到直接 panic:卡池数据是编译期常量,id 写错必须当场暴露
pub fn card_def_or_panic(id: &str) -> &'static CardDef {
    card_def(id).unwrap_or_else(|| panic!("unknown card id: {id}"))
}

/// 生成一张实例(未升级)
pub fn card(id: &str) -> crate::core::card::CardInstance {
    crate::core::card::CardInstance::new(card_def_or_panic(id))
}

/// 按稀有度列出可奖励的卡(排除基础牌与状态牌)
/// 无色牌不进普通奖励池:它们在原作里另有来路(商店/棱彩/事件)
pub fn reward_pool(rarity: Rarity) -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| {
            c.rarity == rarity
                && !matches!(c.kind, CardType::Status | CardType::Curse)
                && pool_of(c) == "class"
        })
        .collect()
}

/// 诅咒牌(事件与遗物会把它们塞进牌组)
pub fn curses() -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| c.kind == CardType::Curse)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无色牌池(语料里 pool=colorless)的每一张都要实现,一张都不留
    #[test]
    fn every_colorless_pool_card_is_implemented() {
        let mut missing: Vec<&str> = crate::core::corpus::CARDS
            .iter()
            .filter(|c| c.color == "colorless" && c.pool == "colorless")
            .filter(|c| card_def(c.id).is_none())
            .map(|c| c.id)
            .collect();
        missing.sort();
        assert!(missing.is_empty(), "无色牌还没实现: {missing:?}");
        assert_eq!(colorless_pool().len(), 35, "无色牌池有 35 张");
        // special token(飞刀/重击这种)不算无色牌池
        assert!(!colorless_pool().iter().any(|c| c.id == "shiv"));
    }

    /// 随机无色牌的池子必须真的抽得到牌
    #[test]
    fn colorless_pool_is_usable_for_random_effects() {
        assert!(colorless_pool().len() >= 30);
        assert!(class_pool_of_kind(CardType::Skill).len() >= 5, "本职业技能太少");
        assert!(class_pool_of_kind(CardType::Attack).len() >= 5, "本职业攻击太少");
        assert!(!class_card_pool().is_empty(), "发现没有牌可亮");
        for c in class_card_pool() {
            assert!(c.rarity != Rarity::Basic, "{} 是基础牌", c.id);
        }
    }

    /// 无色牌的升级沿用语料里的差别
    #[test]
    fn colorless_upgrades_match_the_corpus() {
        // 神化升级后只要 1 费
        let mut apo = card("apotheosis");
        assert_eq!(apo.cost(), Cost::Fixed(2));
        apo.upgrade();
        assert_eq!(apo.cost(), Cost::Fixed(1));

        // 秘密技巧升级后不再消耗
        let mut st = card("secret_technique");
        assert!(st.is_exhaust());
        st.upgrade();
        assert!(!st.is_exhaust());
        let mut sw = card("secret_weapon");
        sw.upgrade();
        assert!(!sw.is_exhaust());

        // 嬗变升级后给的是升级版无色牌
        let mut tr = card("transmutation");
        assert!(tr
            .effects()
            .contains(&Effect::AddRandomColorlessXToHand { upgraded: false }));
        tr.upgrade();
        assert!(tr
            .effects()
            .contains(&Effect::AddRandomColorlessXToHand { upgraded: true }));

        // 心灵冲击是天生牌,升级后只要 1 费
        let mut mb = card("mind_blast");
        assert!(mb.is_innate());
        assert_eq!(mb.cost(), Cost::Fixed(2));
        mb.upgrade();
        assert_eq!(mb.cost(), Cost::Fixed(1));

        // 发现没有升级版(语料里升级前后一模一样)
        assert!(!card("discovery").can_upgrade());
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = CARDS.iter().map(|c| c.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "卡牌 id 有重复");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = CARDS.iter().map(|c| c.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "卡牌名字有重复");
    }

    #[test]
    fn every_card_is_ascii_only() {
        for c in CARDS {
            assert!(c.id.is_ascii(), "{} id 非 ASCII", c.id);
            assert!(c.name.is_ascii(), "{} 名字非 ASCII", c.name);
            assert!(c.text.is_ascii(), "{} 描述非 ASCII", c.name);
            if let Some(u) = &c.upgrade {
                assert!(u.text.is_ascii(), "{} 升级描述非 ASCII", c.name);
            }
        }
    }

    #[test]
    fn every_reward_rarity_has_enough_cards() {
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            let n = reward_pool(rarity).len();
            assert!(n >= 8, "{:?} 可奖励牌只有 {n} 张", rarity);
        }
    }

    #[test]
    fn status_and_curse_stay_out_of_reward_pools() {
        assert!(reward_pool(Rarity::Special).is_empty());
        for c in CARDS.iter().filter(|c| c.rarity == Rarity::Special) {
            assert!(
                matches!(c.kind, CardType::Status | CardType::Curse),
                "{} 是 Special 但不是状态/诅咒",
                c.id
            );
        }
    }

    #[test]
    fn curses_are_exactly_the_three_expected() {
        let mut ids: Vec<&str> = curses().iter().map(|c| c.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["clumsy", "injury", "parasite"]);
        for c in curses() {
            assert_eq!(c.cost, Cost::Unplayable, "{} 诅咒必须不可打出", c.id);
            assert!(!c.upgradable(), "{} 诅咒不可升级", c.id);
        }
    }

    #[test]
    fn referenced_card_ids_exist() {
        for c in CARDS {
            for e in card_effects(c) {
                if let Effect::AddCardToDraw { id, .. }
                | Effect::AddCardToDiscard { id, .. } = e
                {
                    assert!(
                        card_def(id).is_some(),
                        "{} 引用了不存在的卡 id: {id}",
                        c.id
                    );
                }
            }
        }
    }

    #[test]
    fn basics_and_upgrades_are_consistent() {
        assert!(card_def("strike").is_some());
        assert!(card_def("defend").is_some());
        assert!(card_def("bash").is_some());
        for c in CARDS {
            if c.rarity == Rarity::Basic {
                assert!(c.upgradable(), "{} 基础牌应可升级", c.id);
            }
            if matches!(c.kind, CardType::Status | CardType::Curse) {
                assert!(!c.upgradable(), "{} 状态/诅咒不可升级", c.id);
            }
        }
    }

    #[test]
    fn upgrades_change_the_card() {
        for c in CARDS {
            let Some(u) = &c.upgrade else { continue };
            // 可无限升级的牌(灼热攻击)伤害写在 {d} 占位符里,文本与效果都不变,
            // 涨的是实例上的 bonus,所以这条规矩对它不适用
            if c.multi_upgrade {
                continue;
            }
            assert_ne!(u.text, c.text, "{} 升级后描述没变", c.id);
            let up_effects = u.effects.unwrap_or(c.effects);
            let cost_changed = u.cost.map_or(false, |x| x != c.cost);
            // 数值型升级改效果,费用型升级(如 Body Slam)改费用,
            // 只有 Exhaust 变化的(秘密技巧/秘密武器/先想后做)也算真变化
            let exhaust_changed = u.exhaust.map_or(false, |e| e != c.exhaust);
            assert!(
                up_effects != c.effects || cost_changed || exhaust_changed,
                "{} 的升级没有任何变化",
                c.id
            );
        }
    }

    #[test]
    fn targets_match_effects() {
        for c in CARDS {
            let has_single = card_effects(c).iter().any(|e| {
                match e {
                    Effect::Damage { .. }
                    | Effect::DamageEqualBlock
                    | Effect::DamageWithBonus { .. }
                    | Effect::DamagePerStrike { .. }
                    | Effect::DamageStrengthMult { .. }
                    | Effect::DamageIfVulnerable { .. }
                    | Effect::DamageAndKillMaxHp { .. } => true,
                    // 伤害为 0 的 ExhaustNonAttacks 只是在消耗非攻击牌(重整旗鼓),不需要目标
                    Effect::ExhaustNonAttacks { damage } => *damage != 0,
                    _ => false,
                }
            });
            let has_all = card_effects(c).iter().any(|e| {
                matches!(
                    e,
                    Effect::DamageAll { .. } | Effect::DamageAllX { .. } | Effect::Reaper { .. }
                )
            });
            let has_random = card_effects(c)
                .iter()
                .any(|e| matches!(e, Effect::DamageRandom { .. }));
            if has_single {
                assert_eq!(c.target, Target::Enemy, "{} 单体伤害 target 不对", c.id);
            }
            if has_all {
                assert_eq!(c.target, Target::All, "{} 群体伤害 target 不对", c.id);
            }
            if has_random {
                assert_eq!(c.target, Target::Random, "{} 随机伤害 target 不对", c.id);
            }
        }
    }

    #[test]
    fn pool_has_exhaust_ethereal_and_self_copy() {
        let exhaust = CARDS.iter().filter(|c| c.exhaust).count();
        assert!(exhaust >= 3, "只有 {exhaust} 张 exhaust 牌");
        let ethereal = CARDS.iter().filter(|c| c.ethereal).count();
        assert!(ethereal >= 2, "只有 {ethereal} 张 ethereal 牌");
        let adders = CARDS
            .iter()
            .filter(|c| {
                card_effects(c).iter().any(|e| {
                    matches!(
                        e,
                        Effect::AddCardToDraw { .. }
                            | Effect::AddCardToDiscard { .. }
                    )
                })
            })
            .count();
        assert!(adders >= 2, "只有 {adders} 张牌会往牌堆塞牌");
        // Anger 的复制目标必须是它自己.
        let anger = card_def("anger").unwrap();
        assert!(anger.effects.contains(&Effect::AddCardToDiscard {
            id: "anger",
            n: 1
        }));
    }

    /// 一张牌基础 + 升级用到的所有效果
    fn card_effects(c: &CardDef) -> Vec<Effect> {
        let mut v: Vec<Effect> = c.effects.to_vec();
        if let Some(u) = &c.upgrade {
            v.extend(u.effects.unwrap_or(c.effects));
        }
        v
    }
}
