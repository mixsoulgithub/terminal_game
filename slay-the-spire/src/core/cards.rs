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
        text: "Deal 5 damage. If the enemy is Vulnerable, gain 1 Energy and draw 1 card.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        effects: &[Effect::DamageIfVulnerable {
            amount: 5,
            energy: 1,
            draw: 1,
        }],
        upgrade: up!(
            None,
            "Deal 8 damage. If the enemy is Vulnerable, gain 1 Energy and draw 1 card.",
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
        text: "Lose 3 HP. Gain 2 Energy.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        effects: &[Effect::LoseHp { amount: 3 }, Effect::GainEnergy { n: 2 }],
        upgrade: up!(
            None,
            "Lose 3 HP. Gain 3 Energy.",
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
        text: "Exhaust. Gain 2 Energy.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        effects: &[Effect::GainEnergy { n: 2 }],
        upgrade: up!(
            Some(Cost::Fixed(0)),
            "Exhaust. Gain 2 Energy. Costs 0.",
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
        text: "Exhaust. Lose 6 HP. Gain 2 Energy. Draw 3 cards.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        effects: &[
            Effect::LoseHp { amount: 6 },
            Effect::GainEnergy { n: 2 },
            Effect::Draw { n: 3 },
        ],
        upgrade: up!(
            None,
            "Exhaust. Lose 5 HP. Gain 2 Energy. Draw 3 cards.",
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
];

/// 按 id 找卡牌定义
/// 卡牌属于哪个颜色(费用括号上色用):查语料,红绿蓝紫白各按自己,其它灰
pub fn color_key(def: &CardDef) -> &'static str {
    let hit = crate::core::corpus::CARDS
        .iter()
        .find(|c| c.id == def.id && c.color == "red")
        .or_else(|| crate::core::corpus::CARDS.iter().find(|c| c.id == def.id));
    hit
        .map(|c| match c.color {
            "red" => "red",
            "green" => "green",
            "blue" => "blue",
            "purple" => "purple",
            "colorless" => "white",
            _ => "gray",
        })
        .unwrap_or("gray")
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
pub fn reward_pool(rarity: Rarity) -> Vec<&'static CardDef> {
    CARDS
        .iter()
        .filter(|c| c.rarity == rarity && !matches!(c.kind, CardType::Status | CardType::Curse))
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
            assert_ne!(u.text, c.text, "{} 升级后描述没变", c.id);
            let up_effects = u.effects.unwrap_or(c.effects);
            let cost_changed = u.cost.map_or(false, |x| x != c.cost);
            // 数值型升级改效果,费用型升级(如 Body Slam)改费用.
            assert!(
                up_effects != c.effects || cost_changed,
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
