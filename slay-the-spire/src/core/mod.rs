// 游戏核心:纯逻辑,不依赖 ratatui,可以脱离终端单测.
pub mod card;
pub mod cards;
pub mod combat;
pub mod compendium;
pub mod corpus;
pub mod enemy;
pub mod enemies;
pub mod events;
pub mod map;
pub mod potions;
pub mod relics;
pub mod roster;
pub mod run;
pub mod save;
pub mod status;

#[cfg(test)]
mod content_tests {
    //! 跨文件的内容契约:单个数据文件自己测不出"引用的 id 是否存在",
    //! 这里把各文件之间的引用关系钉死.
    use crate::core::{cards, corpus, enemies, enemy, events, potions, relics, roster};
    use crate::core::card::Rarity;

    /// 已经实现的卡/遗物/药水/事件,id 必须能在语料里找到(导入对得上)
    #[test]
    fn implemented_content_exists_in_the_corpus() {
        let card_ids: Vec<&str> = corpus::CARDS.iter().map(|c| c.id).collect();
        for c in cards::CARDS {
            assert!(
                card_ids.contains(&c.id),
                "卡牌 {} 在语料里没有对应条目",
                c.id
            );
        }
        let relic_ids: Vec<&str> = corpus::RELICS.iter().map(|r| r.id).collect();
        for r in relics::RELICS {
            assert!(relic_ids.contains(&r.id), "遗物 {} 在语料里没有对应条目", r.id);
        }
        let potion_ids: Vec<&str> = corpus::POTIONS.iter().map(|p| p.id).collect();
        for p in potions::POTIONS {
            assert!(
                potion_ids.contains(&p.id),
                "药水 {} 在语料里没有对应条目",
                p.id
            );
        }
        let event_ids: Vec<&str> = corpus::EVENTS.iter().map(|e| e.id).collect();
        for e in events::EVENTS {
            assert!(event_ids.contains(&e.id), "事件 {} 在语料里没有对应条目", e.id);
        }
        // 事件专用牌与遗物:语料里也要有对应条目
        let card_ids: Vec<&str> = corpus::CARDS.iter().map(|c| c.id).collect();
        for c in events::EVENT_CARDS {
            assert!(card_ids.contains(&c.id), "事件专用牌 {} 没有语料条目", c.id);
        }
        let relic_ids: Vec<&str> = corpus::RELICS.iter().map(|r| r.id).collect();
        for r in events::EVENT_RELICS {
            assert!(relic_ids.contains(&r.id), "事件专用遗物 {} 没有语料条目", r.id);
        }
    }

    /// 四个角色的起始牌组都要在卡池里查得到(没实现的会被 roster 挡住)
    #[test]
    fn character_starting_decks_reference_known_cards() {
        for ch in roster::all() {
            for (id, n) in ch.deck {
                assert!(*n > 0, "{} 的起始牌组里 {} 张数为 0", ch.name, id);
                assert!(
                    cards::card_def(id).is_some() || roster::missing_cards(ch).contains(id),
                    "{} 的起始牌 {} 既没实现也不是已知缺失",
                    ch.name,
                    id
                );
            }
            assert!(
                roster::playable(ch) || roster::blocked_reason(ch).is_some(),
                "{} 不可玩但没给出原因",
                ch.name
            );
        }
    }

    #[test]
    fn event_outcomes_reference_existing_ids() {
        // 事件表 + 多屏事件的后半段都要查:牌、诅咒、遗物、遭遇都得认得出来
        let defs: Vec<&'static events::EventDef> = events::EVENTS
            .iter()
            .chain(events::STAGES.iter().copied())
            .collect();
        for e in defs {
            for choice in e.choices {
                check_outcome(e.id, &choice.outcome);
            }
        }
    }

    /// 递归检查一个结算结果里引用到的 id(roll 里的结果也要查)
    fn check_outcome(event_id: &str, o: &events::Outcome) {
        for id in [
            o.add_card,
            o.add_curse,
            o.add_cards.map(|(id, _)| id),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                cards::card_def(id).is_some() || events::event_card(id).is_some(),
                "事件 {event_id} 引用了未知卡牌 {id}"
            );
        }
        if let Some(id) = o.relic_id {
            assert!(
                relics::relic_def(id).is_some() || events::event_relic(id).is_some(),
                "事件 {event_id} 引用了未知遗物 {id}"
            );
        }
        if let Some(id) = o.remove_relic {
            assert!(
                relics::relic_def(id).is_some() || events::event_relic(id).is_some(),
                "事件 {event_id} 引用了未知遗物 {id}"
            );
        }
        if let Some(id) = o.fight {
            assert!(
                enemies::encounter_def(id).is_some() || enemy::event_encounter(id).is_some(),
                "事件 {event_id} 引用了未知遭遇 {id}"
            );
        }
        if let Some(r) = o.fight_reward {
            if let Some(id) = r.relic_id {
                assert!(
                    relics::relic_def(id).is_some() || events::event_relic(id).is_some(),
                    "事件 {event_id} 的战斗奖励引用了未知遗物 {id}"
                );
            }
        }
        for id in o.fight_pool.unwrap_or(&[]) {
            assert!(
                enemies::encounter_def(id).is_some() || enemy::event_encounter(id).is_some(),
                "事件 {event_id} 引用了未知遭遇 {id}"
            );
        }
        for (_, sub) in o.roll.unwrap_or(&[]) {
            check_outcome(event_id, sub);
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

