// 游戏核心:纯逻辑,不依赖 ratatui,可以脱离终端单测.
pub mod card;
pub mod cards;
pub mod combat;
pub mod enemy;
pub mod enemies;
pub mod events;
pub mod map;
pub mod potions;
pub mod relics;
pub mod run;
pub mod status;

#[cfg(test)]
mod content_tests {
    //! 跨文件的内容契约:单个数据文件自己测不出"引用的 id 是否存在",
    //! 这里把各文件之间的引用关系钉死.
    use crate::core::{cards, enemies, events, potions, relics};
    use crate::core::card::Rarity;

    #[test]
    fn event_outcomes_reference_existing_ids() {
        for e in events::EVENTS {
            for choice in e.choices {
                let o = &choice.outcome;
                if let Some(id) = o.add_card {
                    assert!(cards::card_def(id).is_some(), "事件 {} 引用了未知卡牌 {id}", e.id);
                }
                if let Some(id) = o.add_curse {
                    assert!(cards::card_def(id).is_some(), "事件 {} 引用了未知诅咒 {id}", e.id);
                }
                if let Some(id) = o.relic_id {
                    assert!(
                        relics::relic_def(id).is_some(),
                        "事件 {} 引用了未知遗物 {id}",
                        e.id
                    );
                }
                if let Some(id) = o.fight {
                    assert!(
                        enemies::encounter_def(id).is_some(),
                        "事件 {} 引用了未知遭遇 {id}",
                        e.id
                    );
                }
            }
        }
    }

    #[test]
    fn starting_deck_cards_exist() {
        for id in ["strike", "defend", "bash", "wound", "slimed", "dazed", "burn"] {
            assert!(cards::card_def(id).is_some(), "缺少卡牌 {id}");
        }
    }

    #[test]
    fn every_reward_rarity_can_fill_a_pick() {
        // 奖励三选一与商店都要按稀有度抽牌,池子太小会直接抽空
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            let n = cards::reward_pool(rarity).len();
            assert!(n >= 3, "{rarity:?} 奖励池只有 {n} 张");
        }
    }

    #[test]
    fn every_relic_rarity_can_drop() {
        // 宝箱按稀有度随机,某个稀有度为空时宝箱会静默变成空箱子
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            assert!(
                !relics::relics_of(rarity).is_empty(),
                "{rarity:?} 没有遗物,宝箱/商店会抽不到"
            );
        }
    }

    #[test]
    fn encounter_pools_are_usable() {
        for (name, pool) in [
            ("weak", enemies::ENCOUNTERS_WEAK),
            ("normal", enemies::ENCOUNTERS),
            ("elite", enemies::ELITES),
            ("boss", enemies::BOSSES),
        ] {
            assert!(!pool.is_empty(), "{name} 遭遇池为空");
            for enc in pool {
                for id in enc.enemies {
                    assert!(
                        enemies::enemy_def(id).is_some(),
                        "遭遇 {} 引用了未知敌人 {id}",
                        enc.id
                    );
                }
            }
        }
    }

    #[test]
    fn potions_have_a_heal_for_the_map_and_damage_for_combat() {
        assert!(
            potions::POTIONS.iter().any(|p| p.target.needs_enemy()),
            "没有需要敌人目标的药水"
        );
        assert!(
            potions::POTIONS.iter().any(|p| p.out_of_combat),
            "没有能在地图上使用的药水"
        );
    }

    #[test]
    fn player_effects_can_be_dealt_by_something() {
        // 玩家能力(能力牌)必须真的有人给:否则战斗里的钩子永远不会触发
        use crate::core::card::Effect;
        use crate::core::status::Status;
        let gives = |s: Status| -> bool {
            cards::CARDS.iter().any(|c| {
                let mut lists: Vec<&[Effect]> = vec![c.effects];
                if let Some(u) = c.upgrade {
                    lists.push(u.effects.unwrap_or(c.effects));
                }
                lists.iter().any(|list| {
                    list.iter().any(|e| match *e {
                        Effect::AddSelfStatus { status, .. } => status == s,
                        _ => false,
                    })
                })
            })
        };
        for s in [
            Status::DemonForm,
            Status::Metallicize,
            Status::FeelNoPain,
            Status::DarkEmbrace,
            Status::Evolve,
            Status::FireBreathing,
            Status::Barricade,
            Status::Brutality,
            Status::Rupture,
        ] {
            assert!(gives(s), "没有任何卡牌能给 {s:?},这套机制是死代码");
        }
    }
}

