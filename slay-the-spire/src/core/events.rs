// 事件房(问号房)的数据.
// 一个事件 = 一段文本 + 若干选项;选项的结算用 Outcome 描述,结算由 run.rs 执行.
use crate::core::card::Rarity;

/// 选项结算结果.用 outcome! 宏构造,未写到的字段取 Outcome::NONE 的值.
#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub text: &'static str,
    /// 生命变化(可负)
    pub hp: i32,
    /// 生命上限变化(可负)
    pub max_hp: i32,
    /// 金币变化(可负)
    pub gold: i32,
    /// 回满血
    pub full_heal: bool,
    /// 指定遗物
    pub relic_id: Option<&'static str>,
    /// 随机一个该稀有度的遗物
    pub random_relic_rarity: Option<Rarity>,
    /// 加入牌组的具体卡牌
    pub add_card: Option<&'static str>,
    /// 加入牌组的诅咒牌
    pub add_curse: Option<&'static str>,
    /// 随机加一张该稀有度的卡(Neow 的祝福)
    pub add_random_card: Option<Rarity>,
    /// 随机一张诅咒
    pub add_random_curse: bool,
    /// 随机升级牌组里的一张牌
    pub upgrade_random_card: bool,
    /// 打开选牌界面升级一张牌
    pub upgrade_card: bool,
    /// 打开选牌界面移除一张牌
    pub remove_card: bool,
    /// 随机一瓶药水
    pub random_potion: bool,
    /// 进入战斗(填遭遇 id)
    pub fight: Option<&'static str>,
    /// 直接死亡
    pub dead: bool,
}

impl Outcome {
    pub const NONE: Outcome = Outcome {
        text: "",
        hp: 0,
        max_hp: 0,
        gold: 0,
        full_heal: false,
        relic_id: None,
        random_relic_rarity: None,
        add_card: None,
        add_curse: None,
        add_random_card: None,
        add_random_curse: false,
        upgrade_random_card: false,
        upgrade_card: false,
        remove_card: false,
        random_potion: false,
        fight: None,
        dead: false,
    };
}

/// 只写关心的字段,其余用 NONE 补齐
#[macro_export]
macro_rules! outcome {
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::core::events::Outcome {
            $($field: $value,)*
            ..$crate::core::events::Outcome::NONE
        }
    };
}

#[derive(Clone, Copy, Debug)]
pub struct EventChoice {
    pub label: &'static str,
    /// 需要支付的金币,不够则选项不可选
    pub cost_gold: i32,
    /// 需要支付的直接生命(无视格挡)
    pub cost_hp: i32,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub struct EventDef {
    pub id: &'static str,
    pub name: &'static str,
    /// 事件正文,按行拆开渲染
    pub body: &'static [&'static str],
    pub choices: &'static [EventChoice],
}

pub static EVENTS: &[EventDef] = &[
    EventDef {
        id: "big_fish",
        name: "Big Fish",
        body: &[
            "You spot a huge fish stranded in the shallows.",
            "It looks like it could feed you for days.",
        ],
        choices: &[
            EventChoice {
                label: "Banana: heal 26 HP",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(hp: 26, text: "You eat well and feel restored."),
            },
            EventChoice {
                label: "Donut: raise max HP by 5",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(max_hp: 5, text: "Sturdy and sweet."),
            },
            EventChoice {
                label: "Box: take an uncommon relic",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Uncommon),
                    add_random_curse: true,
                    text: "A fine relic lies within, but it feels wrong."
                ),
            },
        ],
    },
    EventDef {
        id: "the_cleric",
        name: "The Cleric",
        body: &[
            "A strange cleric offers you a blessing.",
            "Her hands glow with pale light.",
        ],
        choices: &[
            EventChoice {
                label: "Heal: pay $75",
                cost_gold: 75,
                cost_hp: 0,
                outcome: outcome!(hp: 25, text: "Your wounds close under her touch."),
            },
            EventChoice {
                label: "Purify: pay $50, remove a card",
                cost_gold: 50,
                cost_hp: 0,
                outcome: outcome!(remove_card: true, text: "She burns one card from your deck."),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You walk away."),
            },
        ],
    },
    EventDef {
        id: "dead_adventurer",
        name: "Dead Adventurer",
        body: &[
            "A dead adventurer lies slumped against a tree.",
            "His pack might still hold something useful.",
        ],
        choices: &[
            EventChoice {
                label: "Search: lose 12 HP",
                cost_gold: 0,
                cost_hp: 12,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Common),
                    text: "You pry a relic from his cold hands."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You leave him to rest."),
            },
        ],
    },
    EventDef {
        id: "golden_idol",
        name: "Golden Idol",
        body: &[
            "A golden idol glints on a stone altar.",
            "The air around it feels heavy and wrong.",
        ],
        choices: &[
            EventChoice {
                label: "Take the idol",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Rare),
                    add_random_curse: true,
                    text: "The idol's curse settles over you."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You step back and leave the idol alone."),
            },
        ],
    },
    EventDef {
        id: "scrap_ooze",
        name: "Scrap Ooze",
        body: &[
            "A moving pile of scrap and ooze blocks the path.",
            "Something shines beneath the surface.",
        ],
        choices: &[
            EventChoice {
                label: "Reach in: pay $30 and lose 6 HP",
                cost_gold: 30,
                cost_hp: 6,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Common),
                    text: "You fish a relic out of the muck."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You step around it and keep moving."),
            },
        ],
    },
    EventDef {
        id: "living_wall",
        name: "Living Wall",
        body: &[
            "A vast living wall breathes in the dark.",
            "It offers to change you for a price.",
        ],
        choices: &[
            EventChoice {
                label: "Forget: remove a card",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(remove_card: true, text: "A memory fades and a card is gone."),
            },
            EventChoice {
                label: "Change: upgrade a card",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(upgrade_card: true, text: "The wall reshapes one of your cards."),
            },
            EventChoice {
                label: "Grow: raise max HP by 4",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(max_hp: 4, text: "You feel sturdier."),
            },
        ],
    },
    EventDef {
        id: "the_ssssserpent",
        name: "The Sssserpent",
        body: &[
            "A giant serpent coils across the road.",
            "You know I can make you rich, it hisses.",
        ],
        choices: &[
            EventChoice {
                label: "Agree: gain $120",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(
                    gold: 120,
                    add_random_curse: true,
                    text: "Coins pour from its mouth, cold to the touch."
                ),
            },
            EventChoice {
                label: "Refuse",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You refused the serpent's gift."),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You slip past it without a word."),
            },
        ],
    },
    EventDef {
        id: "bonfire_spirits",
        name: "Bonfire Spirits",
        body: &[
            "Small spirits dance in a dying bonfire.",
            "They ask for a gift in exchange.",
        ],
        choices: &[
            EventChoice {
                label: "Sacrifice a card: lose 12 HP",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(
                    remove_card: true,
                    hp: 12,
                    text: "You feed a card to the flames and lose blood."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                outcome: outcome!(text: "You back away from the fire."),
            },
        ],
    },
];


/// 开局祝福(Neow)。不进 EVENTS,只在开局时单独打开。
pub fn neow() -> &'static EventDef {
    &NEOW
}

pub static NEOW: EventDef = EventDef {
    id: "neow",
    name: "Neow's Blessing",
    body: &[
        "You wake at the foot of the spire.",
        "A whale-shaped thing looms over you and offers a blessing.",
    ],
    choices: &[
        EventChoice {
            label: "Gain 100 gold",
            cost_gold: 0,
            cost_hp: 0,
            outcome: outcome!(gold: 100, text: "Coins rain down on you."),
        },
        EventChoice {
            label: "Max HP +8",
            cost_gold: 0,
            cost_hp: 0,
            outcome: outcome!(max_hp: 8, text: "You feel tougher than before."),
        },
        EventChoice {
            label: "Remove a card from your deck",
            cost_gold: 0,
            cost_hp: 0,
            outcome: outcome!(remove_card: true, text: "One card is unmade."),
        },
        EventChoice {
            label: "Take 10 damage: gain a random rare relic",
            cost_gold: 0,
            cost_hp: 10,
            outcome: outcome!(
                random_relic_rarity: Some(Rarity::Rare),
                text: "Pain for power: a rare relic is yours."
            ),
        },
    ],
};

/// 按 id 找事件(自检用)
#[cfg(test)]
pub fn event_def(id: &str) -> Option<&'static EventDef> {
    EVENTS.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = EVENTS.iter().map(|e| e.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "事件 id 有重复");
    }

    #[test]
    fn at_least_seven_events() {
        assert!(EVENTS.len() >= 7, "事件数量不足 7");
    }

    #[test]
    fn events_have_choices() {
        for e in EVENTS {
            assert!(!e.choices.is_empty(), "事件 {} 没有选项", e.id);
            assert!(!e.body.is_empty(), "事件 {} 没有正文", e.id);
            assert!(e.choices.len() >= 2, "事件 {} 选项少于 2 个", e.id);
        }
    }

    #[test]
    fn every_event_has_unconditional_choice() {
        for e in EVENTS {
            let ok = e
                .choices
                .iter()
                .any(|c| c.cost_gold == 0 && c.cost_hp == 0);
            assert!(ok, "事件 {} 没有无条件选项", e.id);
        }
    }

    #[test]
    fn texts_are_ascii() {
        fn check(s: &str, what: &str) {
            assert!(s.is_ascii(), "{} 含非 ASCII: {}", what, s);
        }
        for e in EVENTS {
            check(e.id, "event id");
            check(e.name, "event name");
            for line in e.body {
                check(line, "body");
                assert!(line.chars().count() <= 70, "正文行超过 70 字符: {}", line);
            }
            for c in e.choices {
                check(c.label, "label");
                check(c.outcome.text, "outcome text");
            }
        }
    }

    #[test]
    fn outcome_macro_fills_defaults() {
        let o = outcome!(hp: -3, gold: 10);
        assert_eq!(o.hp, -3);
        assert_eq!(o.gold, 10);
        assert_eq!(o.max_hp, 0);
        assert!(o.relic_id.is_none());
        assert!(!o.full_heal);
        assert!(o.fight.is_none());
    }

    #[test]
    fn referenced_card_ids_are_whitelisted() {
        const CARDS: [&str; 3] = ["strike", "defend", "bash"];
        const CURSES: [&str; 3] = ["injury", "clumsy", "parasite"];
        for e in EVENTS {
            for c in e.choices {
                if let Some(id) = c.outcome.add_card {
                    assert!(CARDS.contains(&id), "事件 {} 引用了白名单外的卡 {}", e.id, id);
                }
                if let Some(id) = c.outcome.add_curse {
                    assert!(CURSES.contains(&id), "事件 {} 引用了白名单外的诅咒 {}", e.id, id);
                }
            }
        }
    }

    #[test]
    fn lookup_works() {
        let e = event_def("big_fish").expect("big_fish 应存在");
        assert_eq!(e.id, "big_fish");
        assert!(event_def("no_such_event").is_none());
    }
}
