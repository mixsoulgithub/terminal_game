// 遗物的静态定义.遗物效果用一堆开关式字段描述,战斗/一局流程在对应时机读取.
// 数据表在文件末尾.
use crate::core::card::Rarity;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct RelicFx {
    /// 拾取时:提升生命上限
    pub max_hp: i32,
    /// 拾取时:立即回血
    pub heal: i32,
    /// 拾取时:立即给钱
    pub gold: i32,
    /// 每场战斗开始:获得格挡
    pub combat_start_block: i32,
    /// 每场战斗开始:获得能量
    pub combat_start_energy: i32,
    /// 每场战斗开始:额外抽牌
    pub combat_start_draw: i32,
    /// 每场战斗开始:回血
    pub combat_start_heal: i32,
    /// 每场战斗开始:获得力量
    pub combat_start_strength: i32,
    /// 每场战斗开始:获得敏捷
    pub combat_start_dexterity: i32,
    /// 战斗胜利后回血
    pub post_combat_heal: i32,
    /// 常驻荆棘
    pub thorns: i32,
    /// 商店折扣(百分比)
    pub shop_discount_pct: i32,
    /// 每上一层楼给的金币
    pub gold_per_floor: i32,
    /// 休息时额外回复的生命
    pub rest_heal_bonus: i32,
    /// 跳过卡牌奖励时提升的生命上限
    pub max_hp_on_card_skip: i32,
}

impl RelicFx {
    /// 静态初始化用的全零值:const 上下文里不能用 Default::default()
    pub const ZERO: RelicFx = RelicFx {
        max_hp: 0,
        heal: 0,
        gold: 0,
        combat_start_block: 0,
        combat_start_energy: 0,
        combat_start_draw: 0,
        combat_start_heal: 0,
        combat_start_strength: 0,
        combat_start_dexterity: 0,
        post_combat_heal: 0,
        thorns: 0,
        shop_discount_pct: 0,
        gold_per_floor: 0,
        rest_heal_bonus: 0,
        max_hp_on_card_skip: 0,
    };

}

#[derive(Debug)]
pub struct RelicDef {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub rarity: Rarity,
    pub fx: RelicFx,
}

pub static RELICS: &[RelicDef] = &[
    RelicDef {
        id: "burning_blood",
        name: "Burning Blood",
        desc: "At the end of combat, heal 6 HP.",
        rarity: Rarity::Basic,
        fx: RelicFx {
            post_combat_heal: 6,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "lantern",
        name: "Lantern",
        desc: "Gain 1 extra Energy at the start of each combat.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_energy: 1,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "bronze_scales",
        name: "Bronze Scales",
        desc: "Start each combat with 3 Thorns.",
        rarity: Rarity::Common,
        fx: RelicFx {
            thorns: 3,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "vajra",
        name: "Vajra",
        desc: "Start each combat with 1 Strength.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_strength: 1,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "anchor",
        name: "Anchor",
        desc: "Start each combat with 10 Block.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_block: 10,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "bag_of_preparation",
        name: "Bag of Preparation",
        desc: "At the start of each combat, draw 2 additional cards.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_draw: 2,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "blood_vial",
        name: "Blood Vial",
        desc: "At the start of each combat, heal 2 HP.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_heal: 2,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "oddly_smooth_stone",
        name: "Oddly Smooth Stone",
        desc: "Start each combat with 1 Dexterity.",
        rarity: Rarity::Common,
        fx: RelicFx {
            combat_start_dexterity: 1,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "strawberry",
        name: "Strawberry",
        desc: "On pickup, raise your max HP by 7.",
        rarity: Rarity::Common,
        fx: RelicFx {
            max_hp: 7,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "pear",
        name: "Pear",
        desc: "On pickup, raise your max HP by 10.",
        rarity: Rarity::Uncommon,
        fx: RelicFx {
            max_hp: 10,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "maw_bank",
        name: "Maw Bank",
        desc: "Gain 12 gold whenever you climb to a new floor.",
        rarity: Rarity::Uncommon,
        fx: RelicFx {
            gold_per_floor: 12,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "regal_pillow",
        name: "Regal Pillow",
        desc: "Resting at a campfire heals 15 additional HP.",
        rarity: Rarity::Uncommon,
        fx: RelicFx {
            rest_heal_bonus: 15,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "singing_bowl",
        name: "Singing Bowl",
        desc: "When you skip a card reward, raise your max HP by 2.",
        rarity: Rarity::Uncommon,
        fx: RelicFx {
            max_hp_on_card_skip: 2,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "golden_idol",
        name: "Golden Idol",
        desc: "On pickup, gain 100 gold.",
        rarity: Rarity::Uncommon,
        fx: RelicFx {
            gold: 100,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "membership_card",
        name: "Membership Card",
        desc: "Shop prices are 20 percent lower.",
        rarity: Rarity::Rare,
        fx: RelicFx {
            shop_discount_pct: 20,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "meat_on_the_bone",
        name: "Meat on the Bone",
        desc: "On pickup, heal 12 HP.",
        rarity: Rarity::Rare,
        fx: RelicFx {
            heal: 12,
            ..RelicFx::ZERO
        },
    },
    RelicDef {
        id: "brimstone",
        name: "Brimstone",
        desc: "Start each combat with 2 Strength.",
        rarity: Rarity::Rare,
        fx: RelicFx {
            combat_start_strength: 2,
            ..RelicFx::ZERO
        },
    },
];

pub fn relic_def(id: &str) -> Option<&'static RelicDef> {
    RELICS.iter().find(|r| r.id == id)
}

pub fn relic_def_or_panic(id: &str) -> &'static RelicDef {
    relic_def(id).unwrap_or_else(|| panic!("unknown relic id: {id}"))
}

/// 该稀有度的全部遗物(自检用:宝箱与商店按稀有度掉落)
#[cfg(test)]
pub fn relics_of(rarity: Rarity) -> Vec<&'static RelicDef> {
    RELICS.iter().filter(|r| r.rarity == rarity).collect()
}

/// 起始遗物(如燃烧之血)
#[cfg(test)]
pub fn starter_relic() -> &'static RelicDef {
    relic_def_or_panic("burning_blood")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = RELICS.iter().map(|r| r.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate relic id");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = RELICS.iter().map(|r| r.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate relic name");
    }

    #[test]
    fn pool_is_large_enough() {
        assert!(RELICS.len() >= 15, "need at least 15 relics");
    }

    #[test]
    fn rarity_coverage() {
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            let n = relics_of(rarity).len();
            assert!(n >= 3, "{} has only {n} relics", rarity.name());
        }
    }

    #[test]
    fn required_relics_have_expected_fields() {
        let blood = relic_def_or_panic("burning_blood");
        assert_eq!(blood.rarity, Rarity::Basic);
        assert_eq!(blood.fx.post_combat_heal, 6);
        assert_eq!(blood.desc, "At the end of combat, heal 6 HP.");

        let lantern = relic_def_or_panic("lantern");
        assert_eq!(lantern.rarity, Rarity::Common);
        assert_eq!(lantern.fx.combat_start_energy, 1);

        let scales = relic_def_or_panic("bronze_scales");
        assert_eq!(scales.rarity, Rarity::Common);
        assert_eq!(scales.fx.thorns, 3);
    }

    #[test]
    fn starter_relic_is_burning_blood() {
        assert_eq!(starter_relic().id, "burning_blood");
    }

    #[test]
    fn some_relic_has_non_zero_fx() {
        assert!(RELICS.iter().any(|r| r.fx != RelicFx::ZERO));
    }

    #[test]
    fn every_fx_field_is_used() {
        let sum = |f: fn(&RelicFx) -> i32| RELICS.iter().map(|r| f(&r.fx)).sum::<i32>();
        assert_ne!(sum(|fx| fx.max_hp), 0);
        assert_ne!(sum(|fx| fx.heal), 0);
        assert_ne!(sum(|fx| fx.gold), 0);
        assert_ne!(sum(|fx| fx.combat_start_block), 0);
        assert_ne!(sum(|fx| fx.combat_start_energy), 0);
        assert_ne!(sum(|fx| fx.combat_start_draw), 0);
        assert_ne!(sum(|fx| fx.combat_start_heal), 0);
        assert_ne!(sum(|fx| fx.combat_start_strength), 0);
        assert_ne!(sum(|fx| fx.combat_start_dexterity), 0);
        assert_ne!(sum(|fx| fx.post_combat_heal), 0);
        assert_ne!(sum(|fx| fx.thorns), 0);
        assert_ne!(sum(|fx| fx.shop_discount_pct), 0);
        assert_ne!(sum(|fx| fx.gold_per_floor), 0);
        assert_ne!(sum(|fx| fx.rest_heal_bonus), 0);
        assert_ne!(sum(|fx| fx.max_hp_on_card_skip), 0);
    }

    #[test]
    fn strings_are_ascii_only() {
        for r in RELICS {
            assert!(r.id.is_ascii(), "non-ascii id: {}", r.id);
            assert!(r.name.is_ascii(), "non-ascii name: {}", r.name);
            assert!(r.desc.is_ascii(), "non-ascii desc: {}", r.desc);
        }
    }
}
