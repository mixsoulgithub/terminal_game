// 药水:一次性消耗品,配方同样是数据.
use crate::core::card::{Rarity, Target};
use crate::rng::Rng;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PotionFx {
    Damage { amount: i32 },
    DamageAll { amount: i32 },
    Block { amount: i32 },
    Energy { n: i32 },
    Draw { n: u8 },
    Strength { n: i32 },
    Dexterity { n: i32 },
    WeakAll { n: i32 },
    VulnerableAll { n: i32 },
    Heal { amount: i32 },
    MaxHp { n: i32 },
    /// 清除自身所有减益
    ClearDebuffs,
}

#[derive(Debug)]
pub struct PotionDef {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub rarity: Rarity,
    /// 需要指定敌人的药水(伤害类)
    pub target: Target,
    /// 能否在地图界面使用
    pub out_of_combat: bool,
    pub fx: PotionFx,
}

pub static POTIONS: &[PotionDef] = &[
    PotionDef {
        id: "fire_potion",
        name: "Fire Potion",
        desc: "Deal 20 damage to one enemy.",
        rarity: Rarity::Common,
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::Damage { amount: 20 },
    },
    PotionDef {
        id: "block_potion",
        name: "Block Potion",
        desc: "Gain 12 Block.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Block { amount: 12 },
    },
    PotionDef {
        id: "energy_potion",
        name: "Energy Potion",
        desc: "Gain (2).",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Energy { n: 2 },
    },
    PotionDef {
        id: "strength_potion",
        name: "Strength Potion",
        desc: "Gain 2 Strength.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Strength { n: 2 },
    },
    PotionDef {
        id: "dexterity_potion",
        name: "Dexterity Potion",
        desc: "Gain 2 Dexterity.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Dexterity { n: 2 },
    },
    PotionDef {
        id: "swift_potion",
        name: "Swift Potion",
        desc: "Draw 3 cards.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Draw { n: 3 },
    },
    PotionDef {
        id: "weak_potion",
        name: "Weak Potion",
        desc: "Apply 3 Weak to all enemies.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::WeakAll { n: 3 },
    },
    PotionDef {
        id: "fear_potion",
        name: "Fear Potion",
        desc: "Apply 3 Vulnerable to all enemies.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::VulnerableAll { n: 3 },
    },
    PotionDef {
        id: "blood_potion",
        name: "Blood Potion",
        desc: "Heal 16 HP.",
        rarity: Rarity::Common,
        target: Target::None,
        out_of_combat: true,
        fx: PotionFx::Heal { amount: 16 },
    },
    PotionDef {
        id: "fruit_juice",
        name: "Fruit Juice",
        desc: "Raise your max HP by 5.",
        rarity: Rarity::Rare,
        target: Target::None,
        out_of_combat: true,
        fx: PotionFx::MaxHp { n: 5 },
    },
    PotionDef {
        id: "explosive_potion",
        name: "Explosive Potion",
        desc: "Deal 10 damage to all enemies.",
        rarity: Rarity::Uncommon,
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::DamageAll { amount: 10 },
    },
    PotionDef {
        id: "ancient_potion",
        name: "Ancient Potion",
        desc: "Remove all debuffs from yourself.",
        rarity: Rarity::Rare,
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::ClearDebuffs,
    },
];

/// 药水稀有度的两档分界(参考实现 POTION_DROP.commonBelow / uncommonBelow)
const COMMON_BELOW: i32 = 65;
const UNCOMMON_BELOW: i32 = 90;

/// 随机一瓶药水(参考实现的 returnRandomPotion):先掷稀有度,
/// 再从池子里逐瓶抽到稀有度对上为止
pub fn random_potion(rng: &mut Rng) -> Option<&'static PotionDef> {
    assert!(!POTIONS.is_empty(), "potion pool is empty");
    let roll = rng.random_range(0, 99);
    let rarity = if roll < COMMON_BELOW {
        Rarity::Common
    } else if roll < UNCOMMON_BELOW {
        Rarity::Uncommon
    } else {
        Rarity::Rare
    };
    if !POTIONS.iter().any(|p| p.rarity == rarity) {
        return None;
    }
    loop {
        let i = rng.random(POTIONS.len() as u32 - 1) as usize;
        if POTIONS[i].rarity == rarity {
            return Some(&POTIONS[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = POTIONS.iter().map(|p| p.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate potion id");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = POTIONS.iter().map(|p| p.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate potion name");
    }

    #[test]
    fn random_potion_stays_in_pool() {
        let mut rng = Rng::new(4);
        for _ in 0..50 {
            let p = random_potion(&mut rng).expect("池子里三档稀有度都有");
            assert!(POTIONS.iter().any(|q| q.id == p.id));
        }
    }

    #[test]
    fn pool_is_large_enough() {
        assert!(POTIONS.len() >= 10, "need at least 10 potions");
    }

    #[test]
    fn rarity_coverage() {
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            assert!(
                POTIONS.iter().any(|p| p.rarity == rarity),
                "no potion of rarity {}",
                rarity.name()
            );
        }
    }

    #[test]
    fn has_enemy_target_potion() {
        assert!(POTIONS.iter().any(|p| p.target.needs_enemy()));
    }

    #[test]
    fn has_out_of_combat_heal_potion() {
        assert!(POTIONS
            .iter()
            .any(|p| p.out_of_combat && matches!(p.fx, PotionFx::Heal { .. })));
    }

    #[test]
    fn strings_are_ascii_only() {
        for p in POTIONS {
            assert!(p.id.is_ascii(), "non-ascii id: {}", p.id);
            assert!(p.name.is_ascii(), "non-ascii name: {}", p.name);
            assert!(p.desc.is_ascii(), "non-ascii desc: {}", p.desc);
        }
    }
}
