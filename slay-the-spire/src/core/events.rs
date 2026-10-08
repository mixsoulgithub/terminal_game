// 事件房(问号房)的数据.
// 一个事件 = 一段文本 + 若干选项;选项的结算用 Outcome 描述,结算由 run.rs 执行.
// 多屏事件(斗兽场、诅咒之书、会说话的骷髅)用 Outcome::next 换到下一屏的 EventDef,
// 打完一场架再回到事件用 Outcome::fight_next:这些"下一屏"的 EventDef 也放在本文件末尾,
// id 沿用父事件的 id,不进 EVENTS,所以不影响图鉴与事件池.
use crate::core::card::{CardDef, CardType, CardUpgrade, Cost, Effect, Rarity, Target};
use crate::core::cards;
use crate::core::relics::{self, RelicDef};
use crate::core::status::Status;
use crate::rng::{java_shuffle, FloorStream, JavaRandom, Rng, RngRegistry, RunStream};

/// 按生命上限的千分比取整(四舍五入):125 表示 12.5%.
pub fn pct_of(max_hp: i32, per_mille: i32) -> i32 {
    if per_mille <= 0 {
        return 0;
    }
    (max_hp * per_mille + 500) / 1000
}

/// 随机移除牌组里的牌时的筛选规则
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RemoveRule {
    /// 随机移除一张该类型的牌(坠落)
    OfType(CardType),
    /// 随机移除一张非基础、非诅咒的牌(再会)
    NonBasicNonCurse,
}

/// 事件战斗胜利后的奖励方案:覆盖 run.rs 平时按遭遇等级发的奖励
#[derive(Clone, Copy, Debug)]
pub struct CombatReward {
    /// 打完什么都不给,也不进奖励界面(斗兽场第一场)
    pub nothing: bool,
    /// 覆盖金币区间;None 用战斗本身的区间
    pub gold: Option<(i32, i32)>,
    /// 固定给的遗物
    pub relic_id: Option<&'static str>,
    /// 随机遗物的稀有度
    pub relic_rarity: Option<Rarity>,
    /// 不给遗物
    pub no_relic: bool,
    /// 不给卡牌奖励
    pub no_cards: bool,
    /// 掉药水的百分比
    pub potion_pct: i32,
}

/// 选项结算结果.用 outcome! 宏构造,未写到的字段取 Outcome::NONE 的值.
#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub text: &'static str,
    /// 生命变化(可负)
    pub hp: i32,
    /// 按生命上限千分比扣血,和 hp 叠加
    pub hp_pct: i32,
    /// hp_pct 算出来的扣血下限(0 表示不设下限)
    pub hp_pct_min: i32,
    /// 按生命上限千分比回血
    pub heal_pct: i32,
    /// 生命上限变化(可负)
    pub max_hp: i32,
    /// 按生命上限千分比永久降低上限(至少留 1 点)
    pub max_hp_pct: i32,
    /// 金币变化(可负)
    pub gold: i32,
    /// 随机给的金币区间
    pub gold_range: Option<(i32, i32)>,
    /// 随机扣的金币区间(扣不到就扣到 0)
    pub gold_lose_range: Option<(i32, i32)>,
    /// 金币全丢
    pub gold_lose_all: bool,
    /// 回满血
    pub full_heal: bool,
    /// 指定遗物
    pub relic_id: Option<&'static str>,
    /// 随机一个该稀有度的遗物
    pub random_relic_rarity: Option<Rarity>,
    /// 按 50/33/17 滚稀有度再随机一个遗物
    pub random_relic_any: bool,
    /// 移除指定遗物
    pub remove_relic: Option<&'static str>,
    /// 随机移除一件身上的遗物
    pub remove_random_relic: bool,
    /// 加入牌组的具体卡牌
    pub add_card: Option<&'static str>,
    /// 加入 n 张指定卡牌
    pub add_cards: Option<(&'static str, u8)>,
    /// 加入牌组的诅咒牌
    pub add_curse: Option<&'static str>,
    /// 随机一张诅咒
    pub add_random_curse: bool,
    /// 随机加 n 张该稀有度的本职业牌
    pub add_random_class: Option<(Rarity, u8)>,
    /// 随机加 n 张本职业牌(不限稀有度)
    pub add_random_class_any: u8,
    /// 随机加 n 张无色牌(None 稀有度表示不限)
    pub add_random_colorless: Option<(Option<Rarity>, u8)>,
    /// 打开选牌界面升级一张牌
    pub upgrade_card: bool,
    /// 随机升级 n 张牌
    pub upgrade_random_n: u8,
    /// 升级全部可升级的牌
    pub upgrade_all: bool,
    /// 升级起始的打击与防御
    pub upgrade_starters: bool,
    /// 打开选牌界面移除一张牌
    pub remove_card: bool,
    /// 移除全部起始打击
    pub remove_base_strikes: bool,
    /// 移除全部可移除的诅咒
    pub remove_curses: bool,
    /// 按规则随机移除一张牌
    pub remove_random: Option<RemoveRule>,
    /// 打开选牌界面:变形一张牌(移除后换成随机本职业牌)
    pub transform_card: bool,
    /// 随机变形 n 张牌
    pub transform_random_n: u8,
    /// 打开选牌界面:复制一张牌
    pub duplicate_card: bool,
    /// 随机丢掉一瓶药水
    pub lose_random_potion: bool,
    /// 随机给 n 瓶药水(药水栏满就丢了)
    pub random_potion_n: u8,
    /// 进入战斗(固定遭遇 id)
    pub fight: Option<&'static str>,
    /// 从这几个遭遇里随机挑一个开战
    pub fight_pool: Option<&'static [&'static str]>,
    /// 这一场战斗的奖励方案
    pub fight_reward: Option<&'static CombatReward>,
    /// 打赢这场战斗后回到事件的这一屏
    pub fight_next: Option<&'static EventDef>,
    /// 结算后再按权重随机结算一个结果(权重, 结果)
    pub roll: Option<&'static [(u32, Outcome)]>,
    /// 结算后换到下一屏(选项换成它)
    pub next: Option<&'static EventDef>,
    /// 直接跳到本层 Boss 房开打
    pub jump_to_boss: bool,
    /// 直接死亡
    pub dead: bool,
}

impl Outcome {
    pub const NONE: Outcome = Outcome {
        text: "",
        hp: 0,
        hp_pct: 0,
        hp_pct_min: 0,
        heal_pct: 0,
        max_hp: 0,
        max_hp_pct: 0,
        gold: 0,
        gold_range: None,
        gold_lose_range: None,
        gold_lose_all: false,
        full_heal: false,
        relic_id: None,
        random_relic_rarity: None,
        random_relic_any: false,
        remove_relic: None,
        remove_random_relic: false,
        add_card: None,
        add_cards: None,
        add_curse: None,
        add_random_curse: false,
        add_random_class: None,
        add_random_class_any: 0,
        add_random_colorless: None,
        upgrade_card: false,
        upgrade_random_n: 0,
        upgrade_all: false,
        upgrade_starters: false,
        remove_card: false,
        remove_base_strikes: false,
        remove_curses: false,
        remove_random: None,
        transform_card: false,
        transform_random_n: 0,
        duplicate_card: false,
        lose_random_potion: false,
        random_potion_n: 0,
        fight: None,
        fight_pool: None,
        fight_reward: None,
        fight_next: None,
        roll: None,
        next: None,
        jump_to_boss: false,
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
    /// 至少要有这么多金币才可选(金币不从结算里预扣,由 outcome 扣)
    pub req_gold: i32,
    /// 需要拥有这件遗物
    pub req_relic: Option<&'static str>,
    /// 需要身上至少有一瓶药水
    pub req_potion: bool,
    /// 需要牌组里有单次伤害 10 以上的攻击牌
    pub req_big_attack: bool,
    /// 需要牌组里有非基础、非诅咒的牌
    pub req_non_basic: bool,
    pub outcome: Outcome,
}

impl EventChoice {
    pub const NONE: EventChoice = EventChoice {
        label: "",
        cost_gold: 0,
        cost_hp: 0,
        req_gold: 0,
        req_relic: None,
        req_potion: false,
        req_big_attack: false,
        req_non_basic: false,
        outcome: Outcome::NONE,
    };
}

/// 只写关心的字段,其余用 NONE 补齐
#[macro_export]
macro_rules! choice {
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::core::events::EventChoice {
            $($field: $value,)*
            ..$crate::core::events::EventChoice::NONE
        }
    };
}

#[derive(Debug)]
pub struct EventDef {
    pub id: &'static str,
    pub name: &'static str,
    /// 事件正文,按行拆开渲染
    pub body: &'static [&'static str],
    pub choices: &'static [EventChoice],
}

// ---- 翻牌小游戏(match_and_keep)的棋盘 ----

/// 一共能试几次;次数用完事件就结束(参考实现 MATCH_AND_KEEP).
pub const MATCH_KEEP_ATTEMPTS: u8 = 5;

/// 各角色的起始牌(参考实现的 STARTER_BY_CLASS)
const STARTER_BY_CLASS: [(&str, &str); 4] = [
    ("ironclad", "bash"),
    ("silent", "neutralize"),
    ("defect", "zap"),
    ("watcher", "eruption"),
];

/// 翻两张之后这一步的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipResult {
    /// 只翻开了第一张,还没结算
    First,
    /// 两张同一个牌位:配对成功,两张都常驻朝上
    Matched,
    /// 两张不同源:两张翻回背面
    Miss,
}

/// 12 格翻牌棋盘:6 个牌位各两张,同一个牌位的两张就是同一张牌.
/// 进入事件时铺一次,之后每次尝试翻两张,配对的牌进牌组.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchKeep {
    /// 6 个牌位各是什么牌;取不到牌的位置是 None
    pub slots: [Option<&'static str>; 6],
    /// 12 个格子各铺的是哪个牌位
    pub board: [u8; 12],
    /// 12 个格子是否已配对(配对的一直朝上)
    pub matched: [bool; 12],
    /// 本次尝试先翻开的格子
    pub first: Option<usize>,
    /// 已经用掉的尝试次数(翻第二张才算一次)
    pub attempts: u8,
    /// 上一次翻牌的说明,界面上显示在棋盘下面(不翻就一直留着)
    pub note: Option<String>,
}

/// 从池子里随机取一张的 id;池子空就取不到
fn pick_id(rng: &mut Rng, pool: Vec<&'static CardDef>) -> Option<&'static str> {
    if pool.is_empty() {
        return None;
    }
    Some(rng.pick(&pool).id)
}

/// 无色牌那一格:整池 java 洗一遍(种子来自 shuffleRng),取第一张要的稀有度
fn colorless_via_shuffle(streams: &mut RngRegistry, rarity: Rarity) -> Option<&'static str> {
    let mut pool = cards::colorless_pool();
    if pool.is_empty() {
        return None;
    }
    java_shuffle(
        &mut pool,
        &mut JavaRandom::new(streams.floor(FloorStream::ShuffleRng).random_long()),
    );
    pool.into_iter().find(|c| c.rarity == rarity).map(|c| c.id)
}

/// 卡名:认不出就退回 id
fn card_name_of(id: &'static str) -> &'static str {
    cards::card_def(id).map(|c| c.name).unwrap_or(id)
}

impl MatchKeep {
    /// 铺棋盘:稀有/非普通/普通本职业牌、非普通无色牌、随机诅咒、本职业起始牌,
    /// 再用 [0..5] 各两张洗一遍,按参考实现的 (i%3)*4 + (i%4) 铺进 12 格.
    /// 三张本职业牌与诅咒走 cardRng,无色牌走 shuffleRng,棋盘洗牌走 miscRng.
    pub fn new(streams: &mut RngRegistry, character: &str) -> MatchKeep {
        let mut slots: [Option<&'static str>; 6] = [None; 6];
        slots[0] = pick_id(
            streams.run(RunStream::CardRng),
            cards::reward_pool(Rarity::Rare),
        );
        slots[1] = pick_id(
            streams.run(RunStream::CardRng),
            cards::reward_pool(Rarity::Uncommon),
        );
        slots[2] = pick_id(
            streams.run(RunStream::CardRng),
            cards::reward_pool(Rarity::Common),
        );
        slots[3] = colorless_via_shuffle(streams, Rarity::Uncommon);
        slots[4] = pick_id(
            streams.run(RunStream::CardRng),
            cards::curses()
                .into_iter()
                .filter(|c| cards::pool_of(c) == "curse")
                .collect(),
        );
        slots[5] = STARTER_BY_CLASS
            .iter()
            .find(|(id, _)| *id == character)
            .map(|(_, card)| *card);
        let mut idxs: Vec<usize> = (0..12).map(|i| i % 6).collect();
        java_shuffle(
            &mut idxs,
            &mut JavaRandom::new(streams.floor(FloorStream::MiscRng).random_long()),
        );
        let mut board = [0u8; 12];
        for (i, slot) in idxs.iter().enumerate() {
            board[(i % 3) * 4 + (i % 4)] = *slot as u8;
        }
        MatchKeep {
            slots,
            board,
            matched: [false; 12],
            first: None,
            attempts: 0,
            note: None,
        }
    }

    /// 这一格是哪个牌位的牌
    pub fn card_at(&self, slot: usize) -> Option<&'static str> {
        let pair = *self.board.get(slot)? as usize;
        self.slots.get(pair).copied().flatten()
    }

    /// 这一格现在朝上吗(已配对或本次刚翻开)
    pub fn revealed(&self, slot: usize) -> bool {
        self.matched.get(slot).copied().unwrap_or(false) || self.first == Some(slot)
    }

    /// 这一格显示什么:朝上的显示牌名,其余显示牌背
    pub fn label(&self, slot: usize) -> String {
        if !self.revealed(slot) {
            return format!("Flip card {}", slot + 1);
        }
        match self.card_at(slot) {
            Some(id) => format!("Card {}: {}", slot + 1, card_name_of(id)),
            None => format!("Card {}: (empty)", slot + 1),
        }
    }

    /// 这一格能不能翻:没配对、不是本次刚翻开的、次数还没用完
    pub fn available(&self, slot: usize) -> bool {
        slot < self.board.len()
            && !self.matched[slot]
            && self.first != Some(slot)
            && self.attempts < MATCH_KEEP_ATTEMPTS
    }

    /// 翻一格:第一张只是翻开;第二张结算一次尝试,同源则两张都配对
    pub fn flip(&mut self, slot: usize) -> FlipResult {
        match self.first.take() {
            None => {
                self.first = Some(slot);
                FlipResult::First
            }
            Some(first) => {
                self.attempts += 1;
                if self.board[first] == self.board[slot] {
                    self.matched[first] = true;
                    self.matched[slot] = true;
                    FlipResult::Matched
                } else {
                    FlipResult::Miss
                }
            }
        }
    }

    /// 没得翻了:次数用完或 12 格全配对
    pub fn finished(&self) -> bool {
        self.attempts >= MATCH_KEEP_ATTEMPTS || self.matched.iter().all(|m| *m)
    }
}

// ---- 事件战斗的奖励方案 ----

/// 打完只进奖励界面,不额外给东西(斗兽场第一场)
static REWARD_NOTHING: CombatReward = CombatReward {
    nothing: true,
    gold: None,
    relic_id: None,
    relic_rarity: None,
    no_relic: true,
    no_cards: true,
    potion_pct: 0,
};

/// 蘑菇:20-30 金币 + 怪蘑菇
static REWARD_MUSHROOMS: CombatReward = CombatReward {
    nothing: false,
    gold: Some((20, 30)),
    relic_id: Some("odd_mushroom"),
    relic_rarity: None,
    no_relic: false,
    no_cards: false,
    potion_pct: 40,
};

/// 土匪:25-35 金币 + 红面具
static REWARD_BANDITS: CombatReward = CombatReward {
    nothing: false,
    gold: Some((25, 35)),
    relic_id: Some("red_mask"),
    relic_rarity: None,
    no_relic: false,
    no_cards: false,
    potion_pct: 40,
};

/// 斗兽场第二场:100 金币 + 一个稀有遗物
static REWARD_COLOSSEUM: CombatReward = CombatReward {
    nothing: false,
    gold: Some((100, 100)),
    relic_id: None,
    relic_rarity: Some(Rarity::Rare),
    no_relic: false,
    no_cards: false,
    potion_pct: 40,
};

/// 神秘球体:45-55 金币 + 一个稀有遗物
static REWARD_SPHERE: CombatReward = CombatReward {
    nothing: false,
    gold: Some((45, 55)),
    relic_id: None,
    relic_rarity: Some(Rarity::Rare),
    no_relic: false,
    no_cards: false,
    potion_pct: 40,
};

/// 心花怒放的"我是战争":50 金币 + 一个稀有遗物
static REWARD_PHANTOM: CombatReward = CombatReward {
    nothing: false,
    gold: Some((50, 50)),
    relic_id: None,
    relic_rarity: Some(Rarity::Rare),
    no_relic: false,
    no_cards: false,
    potion_pct: 40,
};

/// 变化之轮:六个结果等概率
static WHEEL: [(u32, Outcome); 6] = [
    (1, outcome!(gold: 100, text: "The wheel stops on a pile of gold.")),
    (1, outcome!(random_relic_any: true, text: "The wheel grants a relic.")),
    (1, outcome!(full_heal: true, text: "The wheel pours warm light over you.")),
    (1, outcome!(add_curse: Some("decay"), text: "The wheel leaves a curse in your deck.")),
    (1, outcome!(remove_card: true, text: "The wheel takes one card away.")),
    (1, outcome!(hp_pct: 100, hp_pct_min: 1, text: "The wheel snaps back and hurts you.")),
];

/// 陵墓:50% 再送一个纠葛诅咒
static MAUSOLEUM_CURSE: [(u32, Outcome); 2] = [
    (1, outcome!(add_curse: Some("writhe"), text: "Writhe crawls out of the coffin too.")),
    (1, outcome!(text: "The coffin holds nothing else.")),
];

/// 骑士对决赛:押凶手,七成赢
static JOUST_MURDERER: [(u32, Outcome); 2] = [
    (70, outcome!(gold: 100, text: "The murderer wins: you collect 100 gold.")),
    (30, outcome!(text: "The knight wins: your wager is gone.")),
];

/// 押骑士,三成赢
static JOUST_OWNER: [(u32, Outcome); 2] = [
    (30, outcome!(gold: 250, text: "The knight wins: you collect 250 gold.")),
    (70, outcome!(text: "The murderer wins: your wager is gone.")),
];

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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(hp: 26, text: "You eat well and feel restored."),
            },
            EventChoice {
                label: "Donut: raise max HP by 5",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(max_hp: 5, text: "Sturdy and sweet."),
            },
            EventChoice {
                label: "Box: take an uncommon relic",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(hp: 25, text: "Your wounds close under her touch."),
            },
            EventChoice {
                label: "Purify: pay $50, remove a card",
                cost_gold: 50,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(remove_card: true, text: "She burns one card from your deck."),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Common),
                    text: "You pry a relic from his cold hands."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(
                    random_relic_rarity: Some(Rarity::Common),
                    text: "You fish a relic out of the muck."
                ),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(remove_card: true, text: "A memory fades and a card is gone."),
            },
            EventChoice {
                label: "Change: upgrade a card",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(upgrade_card: true, text: "The wall reshapes one of your cards."),
            },
            EventChoice {
                label: "Grow: raise max HP by 4",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(text: "You refused the serpent's gift."),
            },
            EventChoice {
                label: "Leave",
                cost_gold: 0,
                cost_hp: 0,
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
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
                req_gold: 0,
                req_relic: None,
                req_potion: false,
                req_big_attack: false,
                req_non_basic: false,
                outcome: outcome!(text: "You back away from the fire."),
            },
        ],
    },
    // ---- 第一幕 ----
    EventDef {
        id: "wing_statue",
        name: "Wing Statue",
        body: &[
            "A winged statue trades pain for a card removal, or can be smashed for",
            "gold.",
        ],
        choices: &[
            choice!(
                label: "Pray: take 7 damage, remove a card",
                outcome: outcome!(hp: -7, remove_card: true, text: "The statue takes its fee in blood.")
            ),
            choice!(
                label: "Destroy: gain 50-80 gold (needs an attack dealing 10+ damage in one hit)",
                req_big_attack: true,
                outcome: outcome!(
                    gold_range: Some((50, 80)),
                    text: "You shatter the statue and pocket the gold inside."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the statue be.")),
        ],
    },
    EventDef {
        id: "world_of_goop",
        name: "World of Goop",
        body: &[
            "Gold has spilled into burning slime; wade in for it or abandon some of",
            "your own gold.",
        ],
        choices: &[
            choice!(
                label: "Gather gold: take 11 damage, gain 75 gold",
                outcome: outcome!(hp: -11, gold: 75, text: "You wade through the slime and come back richer.")
            ),
            choice!(
                label: "Leave it: lose 20-50 gold (capped at current gold)",
                outcome: outcome!(
                    gold_lose_range: Some((20, 50)),
                    text: "Some of your gold is lost to the slime."
                )
            ),
        ],
    },
    EventDef {
        id: "hypnotizing_colored_mushrooms",
        name: "Hypnotizing Colored Mushrooms",
        body: &[
            "A mushroom-filled corridor compels you to fight the fungus or eat it.",
        ],
        choices: &[
            choice!(
                label: "Stomp: fight 3 Fungi Beasts; victory adds Odd Mushroom and 20-30 gold to the rewards",
                outcome: outcome!(
                    fight: Some("event_three_fungi"),
                    fight_reward: Some(&REWARD_MUSHROOMS),
                    text: "The fungus rears up and lunges."
                )
            ),
            choice!(
                label: "Eat: heal 25% of max HP, obtain the Parasite curse",
                outcome: outcome!(
                    heal_pct: 250,
                    add_curse: Some("parasite"),
                    text: "The mushrooms fill you with warmth and something that squirms."
                )
            ),
        ],
    },
    EventDef {
        id: "shining_light",
        name: "Shining Light",
        body: &[
            "A glowing light upgrades random cards in exchange for a chunk of HP.",
        ],
        choices: &[
            choice!(
                label: "Enter: take damage equal to 20% of max HP; upgrade 2 random upgradeable cards (1 if only one exists)",
                outcome: outcome!(
                    hp_pct: 200,
                    hp_pct_min: 1,
                    upgrade_random_n: 2,
                    text: "The light burns through you and reshapes two of your cards."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You step away from the light.")),
        ],
    },
    // ---- 第二幕 ----
    EventDef {
        id: "pleading_vagrant",
        name: "Pleading Vagrant",
        body: &[
            "A vagrant offers a relic for coin; it can also simply be robbed at the",
            "cost of a curse.",
        ],
        choices: &[
            choice!(
                label: "Offer gold: pay 85 gold, obtain a random relic",
                cost_gold: 85,
                outcome: outcome!(random_relic_any: true, text: "The vagrant hands over a relic.")
            ),
            choice!(
                label: "Rob: obtain a random relic and the Shame curse",
                outcome: outcome!(
                    random_relic_any: true,
                    add_curse: Some("shame"),
                    text: "You take the relic by force and the shame sticks."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the vagrant alone.")),
        ],
    },
    EventDef {
        id: "ancient_writing",
        name: "Ancient Writing",
        body: &[
            "Glowing glyphs grant either a card removal or upgrades to every",
            "starter Strike and Defend.",
        ],
        choices: &[
            choice!(
                label: "Elegance: remove a card",
                outcome: outcome!(remove_card: true, text: "The glyphs erase one card from your deck.")
            ),
            choice!(
                label: "Simplicity: upgrade all starter Strikes and Defends",
                outcome: outcome!(
                    upgrade_starters: true,
                    text: "Every Strike and Defend sharpens."
                )
            ),
        ],
    },
    EventDef {
        id: "old_beggar",
        name: "Old Beggar",
        body: &[
            "A beggar removes a card from your deck in exchange for alms.",
        ],
        choices: &[
            choice!(
                label: "Offer gold: pay 75 gold, remove a card",
                cost_gold: 75,
                outcome: outcome!(remove_card: true, text: "The beggar burns a card for his alms.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You keep your coin.")),
        ],
    },
    EventDef {
        id: "colosseum",
        name: "The Colosseum",
        body: &[
            "You are thrown into an arena: a forced fight, then a choice to flee or",
            "face a harder bout for big rewards.",
        ],
        choices: &[
            choice!(
                label: "Fight (forced): battle Blue Slaver + Red Slaver with no combat rewards",
                outcome: outcome!(
                    fight: Some("event_colosseum_slavers"),
                    fight_reward: Some(&REWARD_NOTHING),
                    fight_next: Some(&COLOSSEUM_AFTER),
                    text: "The gates slam shut behind you."
                )
            ),
        ],
    },
    EventDef {
        id: "cursed_tome",
        name: "Cursed Tome",
        body: &[
            "A sinister book charges escalating HP per page and finally offers one",
            "of three book relics.",
        ],
        choices: &[
            choice!(
                label: "Read: begin reading (no cost); or Leave immediately for no effect",
                outcome: outcome!(next: Some(&TOME_PAGE_1), text: "")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the book closed.")),
        ],
    },
    EventDef {
        id: "augmenter",
        name: "Augmenter",
        body: &[
            "A back-alley scientist offers a mutagen card, a double transform, or",
            "an unstable strength relic.",
        ],
        choices: &[
            choice!(
                label: "Test J.A.X.: obtain the J.A.X. card",
                outcome: outcome!(add_card: Some("jax"), text: "A syringe of raw strength is yours.")
            ),
            choice!(
                label: "Become test subject: transform 2 cards",
                outcome: outcome!(
                    transform_random_n: 2,
                    text: "Two cards twist into something else."
                )
            ),
            choice!(
                label: "Ingest mutagens: obtain Mutagenic Strength relic",
                outcome: outcome!(relic_id: Some("mutagenic_strength"), text: "Your blood runs hot.")
            ),
        ],
    },
    EventDef {
        id: "forgotten_altar",
        name: "Forgotten Altar",
        body: &[
            "An altar demands sacrifice: trade the Golden Idol, bleed for max HP,",
            "or desecrate it and be cursed.",
        ],
        choices: &[
            choice!(
                label: "Offer Golden Idol: replace Golden Idol with Bloody Idol",
                req_relic: Some("golden_idol"),
                outcome: outcome!(
                    remove_relic: Some("golden_idol"),
                    relic_id: Some("bloody_idol"),
                    text: "The idol drinks the offering and changes shape."
                )
            ),
            choice!(
                label: "Sacrifice: gain 5 max HP, lose HP equal to 25% of max HP",
                outcome: outcome!(
                    max_hp: 5,
                    hp_pct: 250,
                    hp_pct_min: 1,
                    text: "The altar takes blood and gives back a sturdier body."
                )
            ),
            choice!(
                label: "Desecrate: obtain the Decay curse",
                outcome: outcome!(
                    add_curse: Some("decay"),
                    text: "You spit on the altar; something rots in your deck."
                )
            ),
        ],
    },
    EventDef {
        id: "ghosts",
        name: "Council of Ghosts",
        body: &[
            "Spectral figures offer Apparition cards in exchange for half your max",
            "HP.",
        ],
        choices: &[
            choice!(
                label: "Accept: lose 50% of max HP permanently (capped at max HP - 1), obtain 5 Apparition cards",
                outcome: outcome!(
                    max_hp_pct: 500,
                    add_cards: Some(("ghostly_armor", 5)),
                    text: "Your body thins; five ghostly guards join your deck."
                )
            ),
            choice!(label: "Refuse: no effect", outcome: outcome!(text: "You refuse the bargain.")),
        ],
    },
    EventDef {
        id: "masked_bandits",
        name: "Masked Bandits",
        body: &[
            "Bandits demand every coin you carry; refusing starts a fight for their",
            "leader's mask.",
        ],
        choices: &[
            choice!(
                label: "Pay: lose ALL gold",
                outcome: outcome!(gold_lose_all: true, text: "You hand over every coin you have.")
            ),
            choice!(
                label: "Fight: battle the bandits; victory yields Red Mask, 25-35 gold, and a card reward",
                outcome: outcome!(
                    fight: Some("event_bandits"),
                    fight_reward: Some(&REWARD_BANDITS),
                    text: "You draw your weapon instead of your purse."
                )
            ),
        ],
    },
    EventDef {
        id: "the_nest",
        name: "The Nest",
        body: &[
            "Infiltrating a cult offers either a quick gold grab or a bloody",
            "initiation for a ritual blade.",
        ],
        choices: &[
            choice!(
                label: "Smash and grab: gain 99 gold",
                outcome: outcome!(gold: 99, text: "You grab the cult's coin and run.")
            ),
            choice!(
                label: "Stay in line: take 6 damage, obtain the Ritual Dagger card",
                outcome: outcome!(
                    hp: -6,
                    add_card: Some("ritual_dagger"),
                    text: "The initiation cuts deep and leaves you a blade."
                )
            ),
        ],
    },
    EventDef {
        id: "the_library",
        name: "The Library",
        body: &[
            "An abandoned library lets you study one of twenty cards or nap for a",
            "heal.",
        ],
        choices: &[
            choice!(
                label: "Read: choose 1 of 20 distinct class cards to obtain",
                outcome: outcome!(
                    add_random_class_any: 1,
                    text: "You study one of the volumes and take its lesson with you."
                )
            ),
            choice!(
                label: "Sleep: heal 33% of max HP",
                outcome: outcome!(heal_pct: 330, text: "You nap between the shelves and wake refreshed.")
            ),
        ],
    },
    EventDef {
        id: "the_mausoleum",
        name: "The Mausoleum",
        body: &[
            "A leaking sarcophagus holds a relic; opening it risks a curse.",
        ],
        choices: &[
            choice!(
                label: "Open coffin: obtain a random relic; 50% chance to also obtain the Writhe curse",
                outcome: outcome!(
                    random_relic_any: true,
                    roll: Some(&MAUSOLEUM_CURSE),
                    text: "You pry the lid open."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the coffin shut.")),
        ],
    },
    EventDef {
        id: "vampires",
        name: "Vampires(?)",
        body: &[
            "A blood cult converts your starter Strikes into Bites for a price paid",
            "in max HP or a Blood Vial.",
        ],
        choices: &[
            choice!(
                label: "Offer Blood Vial: lose the Blood Vial relic; remove all starter Strikes; obtain 5 Bite cards",
                req_relic: Some("blood_vial"),
                outcome: outcome!(
                    remove_relic: Some("blood_vial"),
                    remove_base_strikes: true,
                    add_cards: Some(("bite", 5)),
                    text: "The vial is drained and your Strikes become Bites."
                )
            ),
            choice!(
                label: "Accept: lose 30% of max HP (capped at max HP - 1); remove all starter Strikes; obtain 5 Bite cards",
                outcome: outcome!(
                    max_hp_pct: 300,
                    remove_base_strikes: true,
                    add_cards: Some(("bite", 5)),
                    text: "The cult drinks deep and hands you five Bites."
                )
            ),
            choice!(label: "Refuse: no effect", outcome: outcome!(text: "You walk away from the cult.")),
        ],
    },
    // ---- 第三幕 ----
    EventDef {
        id: "falling",
        name: "Falling",
        body: &[
            "Mid-fall you must jettison one card: a preselected random skill,",
            "power, or attack.",
        ],
        choices: &[
            choice!(
                label: "Land: lose the shown skill card",
                outcome: outcome!(
                    remove_random: Some(RemoveRule::OfType(CardType::Skill)),
                    text: "A skill slips out of your hands."
                )
            ),
            choice!(
                label: "Channel: lose the shown power card",
                outcome: outcome!(
                    remove_random: Some(RemoveRule::OfType(CardType::Power)),
                    text: "A power slips out of your hands."
                )
            ),
            choice!(
                label: "Strike: lose the shown attack card",
                outcome: outcome!(
                    remove_random: Some(RemoveRule::OfType(CardType::Attack)),
                    text: "An attack slips out of your hands."
                )
            ),
            choice!(
                label: "Land on your head (only if no option is available): no card lost",
                outcome: outcome!(text: "You land head first and keep every card.")
            ),
        ],
    },
    EventDef {
        id: "mindbloom",
        name: "Mindbloom",
        body: &[
            "Your thoughts become real: fight a phantom Act 1 boss, upgrade",
            "everything, or take gold/health with a curse.",
        ],
        choices: &[
            choice!(
                label: "I am War: fight a random Act 1 boss; victory yields a rare relic, 50 gold, potions, and a card reward",
                outcome: outcome!(
                    fight_pool: Some(&[
                        "event_phantom_guardian",
                        "event_phantom_hexaghost",
                        "event_phantom_slime_boss",
                    ]),
                    fight_reward: Some(&REWARD_PHANTOM),
                    text: "A phantom of a boss you have not met takes shape."
                )
            ),
            choice!(
                label: "I am Awake: upgrade every upgradeable card; obtain Mark of the Bloom (can no longer heal)",
                outcome: outcome!(
                    upgrade_all: true,
                    relic_id: Some("mark_of_the_bloom"),
                    text: "Every card sharpens, and healing leaves you forever."
                )
            ),
            choice!(
                label: "I am Rich: gain 999 gold, obtain 2 Normality curses (floors 40 and below)",
                outcome: outcome!(
                    gold: 999,
                    add_cards: Some(("normality", 2)),
                    text: "Gold rains down and two rules clamp onto your hands."
                )
            ),
            choice!(
                label: "I am Healthy: heal to full, obtain the Doubt curse (floors 41+, replaces I am Rich)",
                outcome: outcome!(
                    full_heal: true,
                    add_curse: Some("doubt"),
                    text: "You are whole again, but doubt creeps in."
                )
            ),
        ],
    },
    EventDef {
        id: "the_moai_head",
        name: "The Moai Head",
        body: &[
            "A stone head swallows the wounded whole, or swallows a Golden Idol for",
            "a fortune.",
        ],
        choices: &[
            choice!(
                label: "Jump inside: lose 12.5% of max HP permanently, then heal to full",
                outcome: outcome!(
                    max_hp_pct: 125,
                    full_heal: true,
                    text: "The head swallows you and spits you out whole but smaller."
                )
            ),
            choice!(
                label: "Offer Golden Idol: lose the Golden Idol relic, gain 333 gold",
                req_relic: Some("golden_idol"),
                outcome: outcome!(
                    remove_relic: Some("golden_idol"),
                    gold: 333,
                    text: "The idol is swallowed and a fortune spills out."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You walk past the stone face.")),
        ],
    },
    EventDef {
        id: "mysterious_sphere",
        name: "Mysterious Sphere",
        body: &[
            "A guarded bone sphere hides a rare relic; cracking it wakes its",
            "sentries.",
        ],
        choices: &[
            choice!(
                label: "Open sphere: fight 2 Orb Walkers; victory yields a rare relic, 45-55 gold, potions, and a card reward",
                outcome: outcome!(
                    fight: Some("event_two_orbs"),
                    fight_reward: Some(&REWARD_SPHERE),
                    text: "The sphere cracks and its sentries wake."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the sphere where it lies.")),
        ],
    },
    EventDef {
        id: "sensory_stone",
        name: "Sensory Stone",
        body: &[
            "A memory-stone dispenses colorless card rewards; deeper recall costs",
            "HP.",
        ],
        choices: &[
            choice!(
                label: "Recall 1: receive 1 colorless card reward",
                outcome: outcome!(
                    add_random_colorless: Some((None, 1)),
                    text: "A colorless memory takes shape in your deck."
                )
            ),
            choice!(
                label: "Recall 2: lose 5 HP, receive 2 colorless card rewards",
                outcome: outcome!(
                    hp: -5,
                    add_random_colorless: Some((None, 2)),
                    text: "Two colorless memories take shape, and your head throbs."
                )
            ),
            choice!(
                label: "Recall 3: lose 10 HP, receive 3 colorless card rewards",
                outcome: outcome!(
                    hp: -10,
                    add_random_colorless: Some((None, 3)),
                    text: "Three colorless memories take shape and your nose bleeds."
                )
            ),
        ],
    },
    EventDef {
        id: "tomb_of_lord_red_mask",
        name: "Tomb of Lord Red Mask",
        body: &[
            "A tomb pays tribute to mask-wearers and sells its mask for everything",
            "you own.",
        ],
        choices: &[
            choice!(
                label: "Don the Red Mask: gain 222 gold (requires the Red Mask relic)",
                req_relic: Some("red_mask"),
                outcome: outcome!(gold: 222, text: "The tomb pays tribute to the mask you wear.")
            ),
            choice!(
                label: "Offer gold: lose ALL gold, obtain the Red Mask relic",
                outcome: outcome!(
                    gold_lose_all: true,
                    relic_id: Some("red_mask"),
                    text: "You trade every coin for the mask."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the tomb sealed.")),
        ],
    },
    EventDef {
        id: "winding_halls",
        name: "Winding Halls",
        body: &[
            "Lost in shifting halls, you choose between madness, a cursed rest, or",
            "backtracking at a max HP cost.",
        ],
        choices: &[
            choice!(
                label: "Embrace madness: lose 12.5% of max HP, obtain 2 Madness cards",
                outcome: outcome!(
                    hp_pct: 125,
                    hp_pct_min: 1,
                    add_cards: Some(("madness", 2)),
                    text: "Your thoughts crack and two Madness cards slip in."
                )
            ),
            choice!(
                label: "Press on: heal 25% of max HP, obtain the Writhe curse",
                outcome: outcome!(
                    heal_pct: 250,
                    add_curse: Some("writhe"),
                    text: "You press on, healed but marked."
                )
            ),
            choice!(
                label: "Retrace your steps: lose 5% of max HP permanently",
                outcome: outcome!(
                    max_hp_pct: 50,
                    text: "You walk back the way you came, a little smaller."
                )
            ),
        ],
    },
    // ---- 神龛 ----
    EventDef {
        id: "match_and_keep",
        name: "Match and Keep",
        body: &[
            "A 12-card memory game: five flip attempts, matched pairs join your",
            "deck (curses included).",
        ],
        // 12 格棋盘由 Run 在进入事件时铺好,选项也由棋盘给;
        // 这里留一条兜底选项,免得事件表里出现没有选项的事件.
        choices: &[choice!(
            label: "Leave: no effect",
            outcome: outcome!(text: "You leave the cards face down.")
        )],
    },
    EventDef {
        id: "golden_shrine",
        name: "Golden Shrine",
        body: &[
            "A gilded shrine gives modest gold when honored or a fortune plus a",
            "curse when defiled.",
        ],
        choices: &[
            choice!(
                label: "Pray: gain 100 gold",
                outcome: outcome!(gold: 100, text: "The shrine rewards your respect.")
            ),
            choice!(
                label: "Desecrate: gain 275 gold, obtain the Regret curse",
                outcome: outcome!(
                    gold: 275,
                    add_curse: Some("regret"),
                    text: "You take the shrine's gold and its regret."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the shrine untouched.")),
        ],
    },
    EventDef {
        id: "transmorgrifier",
        name: "Transmogrifier",
        body: &["A shrine that transforms one card."],
        choices: &[
            choice!(
                label: "Pray: transform a card",
                outcome: outcome!(transform_card: true, text: "The shrine reshapes one card.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the shrine alone.")),
        ],
    },
    EventDef {
        id: "purifier",
        name: "Purifier",
        body: &["A shrine that removes one card."],
        choices: &[
            choice!(
                label: "Pray: remove a card",
                outcome: outcome!(remove_card: true, text: "The shrine burns one card away.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the shrine alone.")),
        ],
    },
    EventDef {
        id: "upgrade_shrine",
        name: "Upgrade Shrine",
        body: &["A shrine that upgrades one card."],
        choices: &[
            choice!(
                label: "Pray: upgrade a card",
                outcome: outcome!(upgrade_card: true, text: "The shrine sharpens one card.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the shrine alone.")),
        ],
    },
    EventDef {
        id: "wheel_of_change",
        name: "Wheel of Change",
        body: &[
            "A gremlin forces one spin of a six-outcome prize wheel; results range",
            "from riches to injury.",
        ],
        choices: &[
            choice!(
                label: "Spin (forced): uniform roll over gold / relic / full heal / Decay curse / card removal / HP loss",
                outcome: outcome!(roll: Some(&WHEEL), text: "The gremlin spins the wheel.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You walk away from the wheel.")),
        ],
    },
    // ---- 一次性事件 ----
    EventDef {
        id: "ominous_forge",
        name: "Ominous Forge",
        body: &[
            "An abandoned forge upgrades a card, or its stash yields a relic bound",
            "to a curse.",
        ],
        choices: &[
            choice!(
                label: "Forge: upgrade a card",
                outcome: outcome!(upgrade_card: true, text: "The forge reshapes one of your cards.")
            ),
            choice!(
                label: "Rummage: obtain Warped Tongs relic and the Pain curse",
                outcome: outcome!(
                    relic_id: Some("warped_tongs"),
                    add_curse: Some("pain"),
                    text: "The tongs are yours, and so is the pain."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the forge cold.")),
        ],
    },
    EventDef {
        id: "designer_in_spire",
        name: "Designer In-Spire",
        body: &[
            "A snob designer sells deck services; each visit rolls which variant of",
            "two services is offered.",
        ],
        choices: &[
            choice!(
                label: "Adjustments: pay 40 gold; upgrade a chosen card OR upgrade 2 random cards (variant rolled at setup)",
                cost_gold: 40,
                outcome: outcome!(upgrade_card: true, text: "The designer adjusts one of your cards.")
            ),
            choice!(
                label: "Clean up: pay 60 gold; remove a chosen card OR transform 2 random cards (variant rolled at setup)",
                cost_gold: 60,
                outcome: outcome!(remove_card: true, text: "The designer cleans one card out of your deck.")
            ),
            choice!(
                label: "Full service: pay 90 gold; remove a chosen card, then upgrade a random card",
                cost_gold: 90,
                outcome: outcome!(
                    remove_card: true,
                    upgrade_random_n: 1,
                    text: "The designer trims one card and sharpens another."
                )
            ),
            choice!(
                label: "Punch: lose 3 HP",
                outcome: outcome!(hp: -3, text: "You punch the designer and walk out.")
            ),
        ],
    },
    EventDef {
        id: "duplicator",
        name: "Duplicator",
        body: &["An altar that copies one card in your deck."],
        choices: &[
            choice!(
                label: "Pray: duplicate a card (gain a copy)",
                outcome: outcome!(duplicate_card: true, text: "The altar makes a copy of one card.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the altar alone.")),
        ],
    },
    EventDef {
        id: "face_trader",
        name: "Face Trader",
        body: &[
            "A mask peddler pays gold to touch your face, or swaps it for a random",
            "face relic.",
        ],
        choices: &[
            choice!(
                label: "Touch: take damage equal to 10% of max HP (min 1), gain 75 gold",
                outcome: outcome!(
                    hp_pct: 100,
                    hp_pct_min: 1,
                    gold: 75,
                    text: "The peddler's touch stings, but the gold is real."
                )
            ),
            choice!(
                label: "Trade: obtain a random face relic you do not own",
                outcome: outcome!(
                    random_relic_any: true,
                    text: "You trade your face for a relic."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You keep your face.")),
        ],
    },
    EventDef {
        id: "the_divine_fountain",
        name: "The Divine Fountain",
        body: &["Sacred water washes away every removable curse."],
        choices: &[
            choice!(
                label: "Drink: remove all removable curses from your deck",
                outcome: outcome!(remove_curses: true, text: "The water burns every curse out of your deck.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the fountain untouched.")),
        ],
    },
    EventDef {
        id: "knowing_skull",
        name: "Knowing Skull",
        body: &[
            "A flaming skull sells gold, a colorless card, or a potion for HP; each",
            "purchase raises that item's price.",
        ],
        choices: &[
            choice!(
                label: "Riches: lose X HP, gain 90 gold (repeatable; X rises 1 per purchase of this option)",
                outcome: outcome!(
                    hp_pct: 100,
                    hp_pct_min: 6,
                    gold: 90,
                    next: Some(&SKULL_1),
                    text: ""
                )
            ),
            choice!(
                label: "Success: lose X HP, obtain a random uncommon colorless card (repeatable; X rises 1 per purchase)",
                outcome: outcome!(
                    hp_pct: 100,
                    hp_pct_min: 6,
                    add_random_colorless: Some((Some(Rarity::Uncommon), 1)),
                    next: Some(&SKULL_1),
                    text: ""
                )
            ),
            choice!(
                label: "A pick me up: lose X HP, obtain a random potion (repeatable; X rises 1 per purchase; works even with full slots, potion lost)",
                outcome: outcome!(
                    hp_pct: 100,
                    hp_pct_min: 6,
                    random_potion_n: 1,
                    next: Some(&SKULL_1),
                    text: ""
                )
            ),
            choice!(
                label: "How do I leave: lose base X HP (no increment), event ends",
                outcome: outcome!(
                    hp_pct: 100,
                    hp_pct_min: 6,
                    text: "The skull lets you go, for its price in blood."
                )
            ),
        ],
    },
    EventDef {
        id: "lab",
        name: "Lab",
        body: &["An alchemy lab hands over free potions, no choice involved."],
        choices: &[
            choice!(
                label: "Search (automatic): receive 3 random potions via the reward screen",
                outcome: outcome!(random_potion_n: 3, text: "You pocket three random potions.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You leave the lab untouched.")),
        ],
    },
    EventDef {
        id: "nloth",
        name: "N'loth",
        body: &[
            "A hungry creature eats one of two randomly chosen relics you own and",
            "leaves its gift behind.",
        ],
        choices: &[
            choice!(
                label: "Offer relic A: lose that relic, obtain N'loth's Gift",
                outcome: outcome!(
                    remove_random_relic: true,
                    relic_id: Some("nloths_gift"),
                    text: "N'loth eats a relic and leaves its gift."
                )
            ),
            choice!(
                label: "Offer relic B: lose that relic, obtain N'loth's Gift",
                outcome: outcome!(
                    remove_random_relic: true,
                    relic_id: Some("nloths_gift"),
                    text: "N'loth eats a relic and leaves its gift."
                )
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You keep your relics and back away.")),
        ],
    },
    EventDef {
        id: "note_for_yourself",
        name: "Note For Yourself",
        body: &[
            "A hidden note swaps a stored card from a past run for one of your",
            "current cards.",
        ],
        choices: &[
            choice!(
                label: "Take and give: obtain the stored card, then choose a deck card to remove (it is stored for a future run)",
                outcome: outcome!(
                    add_card: Some("iron_wave"),
                    remove_card: true,
                    text: "The note gives you Iron Wave; you leave a card behind."
                )
            ),
            choice!(label: "Ignore: no effect", outcome: outcome!(text: "You leave the note where it is.")),
        ],
    },
    EventDef {
        id: "secret_portal",
        name: "Secret Portal",
        body: &[
            "A portal offers an instant jump to the Act 3 boss, skipping every",
            "floor between.",
        ],
        choices: &[
            choice!(
                label: "Enter the portal: travel immediately to the act boss room",
                outcome: outcome!(jump_to_boss: true, text: "The portal pulls you through the floors.")
            ),
            choice!(label: "Leave: no effect", outcome: outcome!(text: "You step back from the portal.")),
        ],
    },
    EventDef {
        id: "the_joust",
        name: "The Joust",
        body: &[
            "A forced 50-gold wager on a duel between a knight and his pet's",
            "murderer.",
        ],
        choices: &[
            choice!(
                label: "Bet on the murderer: pay 50 gold; 70% chance to win 100 gold",
                outcome: outcome!(
                    gold: -50,
                    roll: Some(&JOUST_MURDERER),
                    text: "You put 50 gold on the murderer."
                )
            ),
            choice!(
                label: "Bet on the owner: pay 50 gold; 30% chance to win 250 gold",
                outcome: outcome!(
                    gold: -50,
                    roll: Some(&JOUST_OWNER),
                    text: "You put 50 gold on the knight."
                )
            ),
        ],
    },
    EventDef {
        id: "we_meet_again",
        name: "We Meet Again!",
        body: &[
            "A stranger claims friendship and trades a random relic for a potion,",
            "gold, or a card.",
        ],
        choices: &[
            choice!(
                label: "Give potion: lose a random held potion, obtain a random relic",
                req_potion: true,
                outcome: outcome!(
                    lose_random_potion: true,
                    random_relic_any: true,
                    text: "The stranger takes a potion and hands over a relic."
                )
            ),
            choice!(
                label: "Give gold: lose a random 50-150 gold amount, obtain a random relic",
                req_gold: 50,
                outcome: outcome!(
                    gold_lose_range: Some((50, 150)),
                    random_relic_any: true,
                    text: "The stranger takes a purse of gold and hands over a relic."
                )
            ),
            choice!(
                label: "Give card: lose a random non-basic, non-curse card, obtain a random relic",
                req_non_basic: true,
                outcome: outcome!(
                    remove_random: Some(RemoveRule::NonBasicNonCurse),
                    random_relic_any: true,
                    text: "The stranger takes a card and hands over a relic."
                )
            ),
            choice!(
                label: "Attack: no effect, event ends",
                outcome: outcome!(text: "You swing at the stranger and he vanishes.")
            ),
        ],
    },
    EventDef {
        id: "the_woman_in_blue",
        name: "The Woman in Blue",
        body: &[
            "A pushy shopkeeper insists you buy 1-3 random potions; refusing has a",
            "price at high ascension.",
        ],
        choices: &[
            choice!(
                label: "Buy 1 potion: pay 20 gold, receive 1 random potion",
                cost_gold: 20,
                outcome: outcome!(random_potion_n: 1, text: "You buy one potion.")
            ),
            choice!(
                label: "Buy 2 potions: pay 30 gold, receive 2 random potions",
                cost_gold: 30,
                outcome: outcome!(random_potion_n: 2, text: "You buy two potions.")
            ),
            choice!(
                label: "Buy 3 potions: pay 40 gold, receive 3 random potions",
                cost_gold: 40,
                outcome: outcome!(random_potion_n: 3, text: "You buy three potions.")
            ),
            choice!(
                label: "Leave: no effect (at A15+: lose 5% of max HP)",
                outcome: outcome!(text: "You leave without buying anything.")
            ),
        ],
    },
];

// ---- 多屏事件的后半段(不进 EVENTS,id 沿用父事件) ----

/// 魔咒之书:第一页
static TOME_PAGE_1: EventDef = EventDef {
    id: "cursed_tome",
    name: "Cursed Tome",
    body: &[
        "Page one turns by itself and bites into your hand.",
        "Keep reading, or put the book down.",
    ],
    choices: &[
        choice!(
            label: "Continue: lose 1 HP",
            outcome: outcome!(hp: -1, next: Some(&TOME_PAGE_2), text: "")
        ),
        choice!(
            label: "Stop: put the book down",
            outcome: outcome!(text: "You shut the book on page one.")
        ),
    ],
};

/// 魔咒之书:第二页
static TOME_PAGE_2: EventDef = EventDef {
    id: "cursed_tome",
    name: "Cursed Tome",
    body: &[
        "Page two drinks deeper.",
        "Keep reading, or put the book down.",
    ],
    choices: &[
        choice!(
            label: "Continue: lose 2 HP",
            outcome: outcome!(hp: -2, next: Some(&TOME_FINAL), text: "")
        ),
        choice!(
            label: "Stop: put the book down",
            outcome: outcome!(text: "You shut the book on page two.")
        ),
    ],
};

/// 魔咒之书:读完三页,书里掉出一件遗物
static TOME_FINAL: EventDef = EventDef {
    id: "cursed_tome",
    name: "Cursed Tome",
    body: &[
        "The book is read to the end and something falls out of its spine.",
    ],
    choices: &[
        choice!(
            label: "Take: lose 10 HP; obtain Necronomicon, Enchiridion, or Nilry's Codex (uniform)",
            outcome: outcome!(
                hp: -10,
                roll: Some(&TOME_RELICS),
                text: "You reach into the spine of the book."
            )
        ),
        choice!(
            label: "Stop (instead of Take): lose 3 HP, no relic",
            outcome: outcome!(hp: -3, text: "You drop the book before it takes more.")
        ),
    ],
};

/// 魔咒之书的三选一:三件书遗物等概率
static TOME_RELICS: [(u32, Outcome); 3] = [
    (1, outcome!(relic_id: Some("necronomicon"), text: "The Necronomicon is yours.")),
    (1, outcome!(relic_id: Some("enchiridion"), text: "The Enchiridion is yours.")),
    (1, outcome!(relic_id: Some("nilrys_codex"), text: "Nilry's Codex is yours.")),
];

/// 斗兽场:第一场之后
static COLOSSEUM_AFTER: EventDef = EventDef {
    id: "colosseum",
    name: "The Colosseum",
    body: &[
        "The crowd roars. The gate behind you is open, and a harder gate ahead.",
    ],
    choices: &[
        choice!(
            label: "Cowardice (after first fight): escape, event ends",
            outcome: outcome!(text: "You slip out through the open gate.")
        ),
        choice!(
            label: "Victory (after first fight): battle Taskmaster + Gremlin Nob; win 100 gold, a rare relic, an uncommon relic, and a card reward",
            outcome: outcome!(
                fight: Some("event_colosseum_nobs"),
                fight_reward: Some(&REWARD_COLOSSEUM),
                text: "You raise your weapon to the crowd."
            )
        ),
    ],
};

/// 会说话的骷髅:每买一次同一档涨价 1 点,分屏实现(第 n 屏的额外扣血写负数)
macro_rules! skull_stage {
    ($name:ident, $extra:expr, $next:expr) => {
        static $name: EventDef = EventDef {
            id: "knowing_skull",
            name: "Knowing Skull",
            body: &["The skull names its price in blood and waits."],
            choices: &[
                choice!(
                    label: "Riches: lose X HP, gain 90 gold (repeatable; X rises 1 per purchase of this option)",
                    outcome: outcome!(
                        hp_pct: 100,
                        hp_pct_min: 6,
                        hp: $extra,
                        gold: 90,
                        next: $next,
                        text: ""
                    )
                ),
                choice!(
                    label: "Success: lose X HP, obtain a random uncommon colorless card (repeatable; X rises 1 per purchase)",
                    outcome: outcome!(
                        hp_pct: 100,
                        hp_pct_min: 6,
                        hp: $extra,
                        add_random_colorless: Some((Some(Rarity::Uncommon), 1)),
                        next: $next,
                        text: ""
                    )
                ),
                choice!(
                    label: "A pick me up: lose X HP, obtain a random potion (repeatable; X rises 1 per purchase; works even with full slots, potion lost)",
                    outcome: outcome!(
                        hp_pct: 100,
                        hp_pct_min: 6,
                        hp: $extra,
                        random_potion_n: 1,
                        next: $next,
                        text: ""
                    )
                ),
                choice!(
                    label: "How do I leave: lose base X HP (no increment), event ends",
                    outcome: outcome!(
                        hp_pct: 100,
                        hp_pct_min: 6,
                        text: "The skull lets you go, for its price in blood."
                    )
                ),
            ],
        };
    };
}

skull_stage!(SKULL_1, -1, Some(&SKULL_2));
skull_stage!(SKULL_2, -2, Some(&SKULL_3));
skull_stage!(SKULL_3, -3, Some(&SKULL_3));

/// 多屏事件的后半段,自检用(不进事件池)
#[cfg(test)]
pub static STAGES: &[&EventDef] = &[
    &TOME_PAGE_1,
    &TOME_PAGE_2,
    &TOME_FINAL,
    &COLOSSEUM_AFTER,
    &SKULL_1,
    &SKULL_2,
    &SKULL_3,
];

// ---- 事件专用遗物 ----

// 这些遗物只由事件给出(事件档),定义在 relics.rs 里;这里只登记 id,
// 免得两处维护同一份数据,也保证宝箱/商店的池子不会抽到它们.
pub static EVENT_RELICS: &[&str] = &[
    "bloody_idol",
    "red_mask",
    "nloths_gift",
    "warped_tongs",
    "mutagenic_strength",
    "mark_of_the_bloom",
    "odd_mushroom",
    "necronomicon",
    "enchiridion",
    "nilrys_codex",
];

/// 按 id 找事件专用遗物(不在事件清单里的返回 None)
pub fn event_relic(id: &str) -> Option<&'static RelicDef> {
    EVENT_RELICS
        .contains(&id)
        .then(|| relics::relic_def_or_panic(id))
}

// ---- 事件专用卡牌 ----

/// 这些牌只由事件塞进牌组,不在 cards.rs 的卡池里(不参与奖励与商店)。
macro_rules! event_up {
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

pub static EVENT_CARDS: &[CardDef] = &[
    CardDef {
        id: "bite",
        name: "Bite",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Special,
        target: Target::Enemy,
        text: "Deal 7 damage. Heal 2 HP.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        unremovable: false,
        on_draw: &[],
        on_end_turn: &[],
        in_hand: &[],
        effects: &[Effect::Damage { amount: 7, times: 1 }, Effect::Heal { amount: 2 }],
        upgrade: event_up!(
            None,
            "Deal 8 damage. Heal 3 HP.",
            [
                Effect::Damage { amount: 8, times: 1 },
                Effect::Heal { amount: 3 }
            ]
        ),
    },
    CardDef {
        id: "jax",
        name: "J.A.X.",
        cost: Cost::Fixed(0),
        kind: CardType::Skill,
        rarity: Rarity::Special,
        target: Target::None,
        text: "Lose 3 HP. Gain 2 Strength.",
        exhaust: false,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        unremovable: false,
        on_draw: &[],
        on_end_turn: &[],
        in_hand: &[],
        effects: &[
            Effect::LoseHp { amount: 3 },
            Effect::AddSelfStatus {
                status: Status::Strength,
                n: 2,
            },
        ],
        upgrade: event_up!(
            None,
            "Lose 3 HP. Gain 3 Strength.",
            [
                Effect::LoseHp { amount: 3 },
                Effect::AddSelfStatus {
                    status: Status::Strength,
                    n: 3
                }
            ]
        ),
    },
    CardDef {
        id: "ritual_dagger",
        name: "Ritual Dagger",
        cost: Cost::Fixed(1),
        kind: CardType::Attack,
        rarity: Rarity::Special,
        target: Target::Enemy,
        text: "Deal 15 damage. If Fatal, permanently increase this card's damage by 3. Exhaust.",
        exhaust: true,
        ethereal: false,
        innate: false,
        retain: false,
        multi_upgrade: false,
        unremovable: false,
        on_draw: &[],
        on_end_turn: &[],
        in_hand: &[],
        effects: &[Effect::DamageAndKillBonusSelf {
            amount: 15,
            bonus: 3,
        }],
        upgrade: event_up!(
            None,
            "Deal 15 damage. If Fatal, permanently increase this card's damage by 5. Exhaust.",
            [Effect::DamageAndKillBonusSelf {
                amount: 15,
                bonus: 5
            }]
        ),
    },
];

/// 按 id 找事件专用卡牌
pub fn event_card(id: &str) -> Option<&'static CardDef> {
    EVENT_CARDS.iter().find(|c| c.id == id)
}

// ---- Neow 的祝福(参考实现 neow.ts / Neow.cpp) ----
//
// 开局用 neowRng 掷四个选项:第一个从 TABLE_0 里挑(无代价),第二个从 TABLE_1
// 里挑(无代价),第三个带一个代价(代价与祝福各掷一次;代价是 PERCENT_DAMAGE
// 时祝福从七项全表里挑),第四个固定是"换 Boss 遗物"。最后还多掷一次
// random(0)(参考实现紧跟在 Boss 遗物那项后面),这一步只为了对齐流位置。

/// Neow 掷出来的一项祝福
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeowOption {
    pub bonus: &'static str,
    pub drawback: &'static str,
}

pub const NEOW_BONUS_TABLE_0: [&str; 6] = [
    "three_cards",
    "one_random_rare_card",
    "remove_card",
    "upgrade_card",
    "transform_card",
    "random_colorless",
];

pub const NEOW_BONUS_TABLE_1: [&str; 5] = [
    "three_small_potions",
    "random_common_relic",
    "ten_percent_hp_bonus",
    "three_enemy_kill",
    "hundred_gold",
];

pub const NEOW_DRAWBACKS: [&str; 4] = ["ten_percent_hp_loss", "no_gold", "curse", "percent_damage"];

/// 带代价那一档的七个祝福(代价是 PERCENT_DAMAGE 时从这里挑)
pub const NEOW_TIER2_ALL: [&str; 7] = [
    "random_colorless_2",
    "remove_two",
    "one_rare_relic",
    "three_rare_cards",
    "two_fifty_gold",
    "transform_two_cards",
    "twenty_percent_hp_bonus",
];

/// 每个代价各有一个六项表(去掉的那项:掉上限不给 20% 上限、没钱不给 250 金币、
/// 诅咒不给移除两张)
const NEOW_BONUS_BY_HP_LOSS: [&str; 6] = [
    "random_colorless_2",
    "remove_two",
    "one_rare_relic",
    "three_rare_cards",
    "two_fifty_gold",
    "transform_two_cards",
];
const NEOW_BONUS_BY_NO_GOLD: [&str; 6] = [
    "random_colorless_2",
    "remove_two",
    "one_rare_relic",
    "three_rare_cards",
    "transform_two_cards",
    "twenty_percent_hp_bonus",
];
const NEOW_BONUS_BY_CURSE: [&str; 6] = [
    "random_colorless_2",
    "one_rare_relic",
    "three_rare_cards",
    "two_fifty_gold",
    "transform_two_cards",
    "twenty_percent_hp_bonus",
];

/// Neow 的数值表
pub const NEOW_TEN_PERCENT_HP_BONUS: f64 = 0.1;
pub const NEOW_TWENTY_PERCENT_HP_BONUS: f64 = 0.2;
pub const NEOW_HUNDRED_GOLD: i32 = 100;
pub const NEOW_TWO_FIFTY_GOLD: i32 = 250;
/// "前三场战斗敌人 1 血"(Neow's Lament 的层数)
pub const NEOW_THREE_ENEMY_KILL: u8 = 3;
/// "三瓶小药水"给几瓶
pub const NEOW_THREE_SMALL_POTIONS: u8 = 3;
/// 祝福发牌时每张牌掷出的"非普通"概率
pub const NEOW_CARD_UNCOMMON_CHANCE: f32 = 0.33;

/// 掷 Neow 的四个选项(参考实现 Neow::getOptions,流位置逐位对齐)
pub fn neow_options(rng: &mut Rng) -> Vec<NeowOption> {
    let mut out = Vec::with_capacity(4);
    out.push(NeowOption {
        bonus: NEOW_BONUS_TABLE_0[rng.random_range(0, 5) as usize],
        drawback: "none",
    });
    out.push(NeowOption {
        bonus: NEOW_BONUS_TABLE_1[rng.random_range(0, 4) as usize],
        drawback: "none",
    });
    let drawback = NEOW_DRAWBACKS[rng.random_range(0, 3) as usize];
    let bonus = match drawback {
        "percent_damage" => NEOW_TIER2_ALL[rng.random_range(0, 6) as usize],
        "ten_percent_hp_loss" => NEOW_BONUS_BY_HP_LOSS[rng.random_range(0, 5) as usize],
        "no_gold" => NEOW_BONUS_BY_NO_GOLD[rng.random_range(0, 5) as usize],
        _ => NEOW_BONUS_BY_CURSE[rng.random_range(0, 5) as usize],
    };
    out.push(NeowOption { bonus, drawback });
    out.push(NeowOption {
        bonus: "boss_relic",
        drawback: "lose_starter_relic",
    });
    // Boss 遗物那项定下来之后参考实现还多掷一次 random(0)(随机数不参与选择)
    rng.random_range(0, 0);
    out
}

/// 祝福的名字(界面上那一行)
pub fn neow_bonus_label(bonus: &str) -> &'static str {
    match bonus {
        "three_cards" => "Choose 1 of 3 class cards",
        "one_random_rare_card" => "Gain a random rare card",
        "remove_card" => "Remove a card",
        "upgrade_card" => "Upgrade a card",
        "transform_card" => "Transform a card",
        "random_colorless" => "Choose 1 of 3 colorless cards",
        "three_small_potions" => "Gain 3 potions",
        "random_common_relic" => "Gain a random common relic",
        "ten_percent_hp_bonus" => "Max HP +10%",
        "three_enemy_kill" => "Enemies in your first 3 combats have 1 HP",
        "hundred_gold" => "Gain 100 gold",
        "random_colorless_2" => "Choose 1 of 3 rare colorless cards",
        "remove_two" => "Remove 2 cards",
        "one_rare_relic" => "Gain a random rare relic",
        "three_rare_cards" => "Choose 1 of 3 rare class cards",
        "two_fifty_gold" => "Gain 250 gold",
        "transform_two_cards" => "Transform 2 cards",
        "twenty_percent_hp_bonus" => "Max HP +20%",
        "boss_relic" => "Swap your starter relic for a Boss relic",
        _ => "an unknown blessing",
    }
}

/// 代价的名字
pub fn neow_drawback_label(drawback: &str) -> &'static str {
    match drawback {
        "ten_percent_hp_loss" => "lose 10% max HP",
        "no_gold" => "lose all gold",
        "curse" => "gain a curse",
        "percent_damage" => "take 30% of current HP as damage",
        "lose_starter_relic" => "lose your starter relic",
        _ => "a drawback",
    }
}

/// 开局祝福(Neow)。不进 EVENTS,只在开局时单独打开;选项是掷出来的,见 neow_options.
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
    choices: &[],
};

/// 按 id 找事件(自检用)
#[cfg(test)]
pub fn event_def(id: &str) -> Option<&'static EventDef> {
    EVENTS.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::run::{Run, Screen};

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = EVENTS.iter().map(|e| e.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "事件 id 有重复");
    }

    #[test]
    fn every_corpus_event_is_implemented() {
        let missing: Vec<&str> = crate::core::corpus::EVENTS
            .iter()
            .filter(|c| !EVENTS.iter().any(|e| e.id == c.id))
            .map(|c| c.id)
            .collect();
        assert!(missing.is_empty(), "语料里还没实现的事件: {missing:?}");
        assert_eq!(EVENTS.len(), crate::core::corpus::EVENTS.len());
    }

    #[test]
    fn at_least_seven_events() {
        assert!(EVENTS.len() >= 7, "事件数量不足 7");
    }

    #[test]
    fn every_corpus_pool_is_covered() {
        for pool in ["act1", "act2", "act3", "shrine", "oneTime"] {
            let n = crate::core::corpus::EVENTS
                .iter()
                .filter(|c| c.pool == pool && EVENTS.iter().any(|e| e.id == c.id))
                .count();
            assert!(n > 0, "事件池 {pool} 一个都没实现");
        }
    }

    #[test]
    fn events_have_choices() {
        // 多屏事件(斗兽场那类)的选项分散在几屏里,按 id 合起来数;
        // 翻牌小游戏的 12 个选项由棋盘现铺,也一起算.
        for e in EVENTS {
            let staged: usize = STAGES
                .iter()
                .filter(|s| s.id == e.id)
                .map(|s| s.choices.len())
                .sum();
            let board = if e.id == "match_and_keep" {
                MatchKeep::new(&mut RngRegistry::new(1), "ironclad").board.len()
            } else {
                0
            };
            assert!(!e.choices.is_empty(), "事件 {} 没有选项", e.id);
            assert!(!e.body.is_empty(), "事件 {} 没有正文", e.id);
            assert!(
                e.choices.len() + staged + board >= 2,
                "事件 {} 选项少于 2 个",
                e.id
            );
            for c in e.choices {
                assert!(!c.label.is_empty(), "事件 {} 有空选项文本", e.id);
            }
        }
    }

    #[test]
    fn stages_have_choices() {
        for e in STAGES {
            assert!(!e.choices.is_empty(), "分屏 {} 没有选项", e.id);
            assert!(!e.body.is_empty(), "分屏 {} 没有正文", e.id);
            for c in e.choices {
                assert!(!c.label.is_empty(), "分屏 {} 有空选项文本", e.id);
            }
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
        for e in STAGES {
            for line in e.body {
                check(line, "stage body");
                assert!(line.chars().count() <= 70, "分屏正文行超过 70 字符: {}", line);
            }
            for c in e.choices {
                check(c.label, "stage label");
                check(c.outcome.text, "stage outcome text");
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
        let c = choice!(label: "x");
        assert_eq!(c.cost_gold, 0);
        assert!(c.req_relic.is_none());
        assert!(!c.req_potion);
    }

    #[test]
    fn referenced_card_ids_resolve() {
        // 事件塞进牌组的牌必须真的认得出来:要么在卡池里,要么是事件专用牌
        for e in EVENTS {
            for c in e.choices {
                let ids = [
                    c.outcome.add_card,
                    c.outcome.add_curse,
                    c.outcome.add_cards.map(|(id, _)| id),
                ];
                for id in ids.into_iter().flatten() {
                    assert!(
                        crate::core::cards::card_def(id).is_some() || event_card(id).is_some(),
                        "事件 {} 引用了未知卡牌 {}",
                        e.id,
                        id
                    );
                }
                if let Some(id) = c.outcome.add_curse {
                    let def = crate::core::cards::card_def(id).expect("诅咒要在卡池里");
                    assert_eq!(def.kind, CardType::Curse, "{} 不是诅咒", id);
                }
            }
        }
    }

    #[test]
    fn stage_defs_reuse_parent_ids() {
        for s in STAGES {
            assert!(
                EVENTS.iter().any(|e| e.id == s.id),
                "分屏 {} 的 id 不在事件表里",
                s.id
            );
        }
    }

    #[test]
    fn event_relics_and_cards_resolve() {
        assert_eq!(event_relic("red_mask").map(|r| r.name), Some("Red Mask"));
        assert!(event_relic("golden_idol").is_none());
        assert_eq!(event_card("bite").map(|c| c.name), Some("Bite"));
        assert!(event_card("strike").is_none());
    }

    #[test]
    fn pct_of_rounds_to_nearest() {
        assert_eq!(pct_of(80, 250), 20);
        assert_eq!(pct_of(80, 125), 10);
        assert_eq!(pct_of(80, 1000), 80);
        assert_eq!(pct_of(0, 500), 0);
    }

    // ---- 行为断言 ----

    /// 把一局直接摆到某个事件的某一屏上
    fn open(r: &mut Run, def: &'static EventDef) {
        r.event = Some(crate::core::run::EventState {
            def,
            neow_options: Vec::new(),
            index: 0,
            result: None,
            match_keep: None,
        });
        r.screen = Screen::Event;
    }

    fn apply(id: &str, seed: u64, choice: usize) -> Run {
        let mut r = Run::new(seed);
        let def = event_def(id).expect("事件应存在");
        open(&mut r, def);
        r.choose_event(choice).expect("选项应能结算");
        r
    }

    #[test]
    fn wing_statue_pray_costs_hp_and_opens_removal() {
        let mut r = Run::new(11);
        let def = event_def("wing_statue").unwrap();
        open(&mut r, def);
        let hp = r.player.hp;
        r.choose_event(0).unwrap();
        assert_eq!(r.player.hp, hp - 7, "祈祷要扣 7 点血");
        assert_eq!(r.screen, Screen::Pick, "要弹出去牌界面");
    }

    #[test]
    fn wing_statue_destroy_needs_a_big_attack() {
        let mut r = Run::new(11);
        let def = event_def("wing_statue").unwrap();
        open(&mut r, def);
        // 起始牌组只有打击(6 点),打不碎雕像
        assert!(!r.event_choice_available(1), "没有 10 点以上的攻击就不该可选");
        r.player.deck.push(crate::core::cards::card("bludgeon"));
        assert!(r.event_choice_available(1), "拿了重击之后应该可选");
        let gold = r.player.gold;
        r.choose_event(1).unwrap();
        let gained = r.player.gold - gold;
        assert!((50..=80).contains(&gained), "打碎雕像给 50-80 金币,实得 {gained}");
    }

    #[test]
    fn world_of_goop_gather_pays_hp_for_gold() {
        let r = apply("world_of_goop", 12, 0);
        assert!(r.player.hp < 80, "捞金币要掉血");
        assert!(r.player.gold > 99, "捞金币要给钱");
    }

    #[test]
    fn world_of_goop_leave_costs_gold_only() {
        let r = apply("world_of_goop", 12, 1);
        assert_eq!(r.player.hp, 80, "走开不掉血");
        let lost = 99 - r.player.gold;
        assert!((20..=50).contains(&lost), "丢 20-50 金币,实丢 {lost}");
    }

    #[test]
    fn glowing_light_upgrades_and_hurts() {
        let r = apply("shining_light", 13, 0);
        assert!(r.player.hp < 80, "进光里要掉血");
        let upgraded = r.player.deck.iter().filter(|c| c.upgraded).count();
        assert_eq!(upgraded, 2, "要升级两张牌");
    }

    #[test]
    fn pleading_vagrant_rob_takes_a_curse() {
        let r = apply("pleading_vagrant", 14, 1);
        let curses = r
            .player
            .deck
            .iter()
            .filter(|c| c.kind() == CardType::Curse)
            .count();
        assert_eq!(curses, 1, "抢劫会吃一个羞愧诅咒");
        assert_eq!(r.player.relics.len(), 2, "抢劫给一件遗物");
    }

    #[test]
    fn ancient_writing_upgrades_every_starter() {
        let r = apply("ancient_writing", 15, 1);
        for c in &r.player.deck {
            if matches!(c.def.id, "strike" | "defend") {
                assert!(c.upgraded, "{} 应该被升级", c.def.id);
            }
        }
    }

    #[test]
    fn old_beggar_charges_seventy_five_and_removes() {
        let mut r = Run::new(16);
        let def = event_def("old_beggar").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert_eq!(r.player.gold, 99 - 75, "乞丐收 75 金币");
        assert_eq!(r.screen, Screen::Pick, "要弹出去牌界面");
    }

    #[test]
    fn forgotten_altar_trades_the_idol() {
        let mut r = Run::new(17);
        r.player
            .relics
            .push(crate::core::relics::relic_def_or_panic("golden_idol"));
        let def = event_def("forgotten_altar").unwrap();
        open(&mut r, def);
        assert!(r.event_choice_available(0), "有金像就该能献祭");
        r.choose_event(0).unwrap();
        assert!(!r.player.relics.iter().any(|x| x.id == "golden_idol"));
        assert!(r.player.relics.iter().any(|x| x.id == "bloody_idol"));
    }

    #[test]
    fn forgotten_altar_sacrifice_trades_hp_for_max_hp() {
        let r = apply("forgotten_altar", 17, 1);
        assert_eq!(r.player.max_hp, 85, "献祭 +5 上限");
        assert_eq!(r.player.hp, 85 - 20, "先加 5 血再按上限交 25%(80 的 25% = 20)");
    }

    #[test]
    fn ghosts_cost_half_of_max_hp() {
        let r = apply("ghosts", 18, 0);
        assert_eq!(r.player.max_hp, 40, "接受要永久掉一半上限");
        assert_eq!(
            r.player.deck.iter().filter(|c| c.def.id == "ghostly_armor").count(),
            5
        );
    }

    #[test]
    fn masked_bandits_pay_takes_all_gold() {
        let r = apply("masked_bandits", 19, 0);
        assert_eq!(r.player.gold, 0, "土匪要拿走所有金币");
    }

    #[test]
    fn masked_bandits_fight_starts_the_bandit_battle() {
        let r = apply("masked_bandits", 19, 1);
        assert_eq!(r.screen, Screen::Combat);
        assert_eq!(r.combat().unwrap().encounter_id, "event_bandits");
    }

    #[test]
    fn nest_smash_gives_ninety_nine_gold() {
        let r = apply("the_nest", 20, 0);
        assert_eq!(r.player.gold, 99 + 99);
    }

    #[test]
    fn nest_stay_in_line_grants_the_ritual_dagger() {
        let r = apply("the_nest", 20, 1);
        assert_eq!(r.player.hp, 74, "入会要挨 6 点");
        assert!(r.player.deck.iter().any(|c| c.def.id == "ritual_dagger"));
    }

    #[test]
    fn vampires_accept_costs_max_hp_and_converts_strikes() {
        let r = apply("vampires", 21, 1);
        assert_eq!(r.player.max_hp, 56, "接受要掉 30% 上限(80 的 30% = 24)");
        assert!(
            !r.player
                .deck
                .iter()
                .any(|c| c.def.id == "strike" && !c.upgraded),
            "基础打击都被换成咬"
        );
        assert_eq!(r.player.deck.iter().filter(|c| c.def.id == "bite").count(), 5);
    }

    #[test]
    fn mausoleum_open_gives_a_relic() {
        let r = apply("the_mausoleum", 22, 0);
        assert_eq!(r.player.relics.len(), 2, "开棺必得一件遗物");
    }

    #[test]
    fn golden_shrine_desecrate_pays_and_curses() {
        let r = apply("golden_shrine", 23, 1);
        assert_eq!(r.player.gold, 99 + 275);
        assert!(r.player
            .deck
            .iter()
            .any(|c| c.def.id == "regret" && c.kind() == CardType::Curse));
    }

    #[test]
    fn purifier_pray_opens_the_removal_picker() {
        let r = apply("purifier", 24, 0);
        assert_eq!(r.screen, Screen::Pick);
        assert_eq!(
            r.picker.as_ref().map(|p| p.purpose),
            Some(crate::core::run::PickPurpose::Remove)
        );
    }

    #[test]
    fn upgrade_shrine_pray_opens_the_upgrade_picker() {
        let r = apply("upgrade_shrine", 24, 0);
        assert_eq!(r.screen, Screen::Pick);
        assert_eq!(
            r.picker.as_ref().map(|p| p.purpose),
            Some(crate::core::run::PickPurpose::Upgrade)
        );
    }

    #[test]
    fn transmorgrifier_transforms_a_card() {
        let mut r = Run::new(25);
        let def = event_def("transmorgrifier").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert_eq!(r.screen, Screen::Pick);
        assert_eq!(
            r.picker.as_ref().map(|p| p.purpose),
            Some(crate::core::run::PickPurpose::Transform)
        );
        let before = r.player.deck.len();
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), before, "变形换一张,不增减张数");
        assert_eq!(r.screen, Screen::Event);
    }

    #[test]
    fn duplicator_adds_a_copy() {
        let mut r = Run::new(25);
        let def = event_def("duplicator").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        let before = r.player.deck.len();
        let target = r.player.deck[0].def.id;
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), before + 1, "复制会多一张");
        assert_eq!(r.player.deck.last().unwrap().def.id, target);
    }

    #[test]
    fn divine_fountain_washes_curses_away() {
        let mut r = Run::new(26);
        r.player.deck.push(crate::core::cards::card("regret"));
        r.player.deck.push(crate::core::cards::card("clumsy"));
        r.player.deck.push(crate::core::cards::card("ascenders_bane"));
        let def = event_def("the_divine_fountain").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert!(!r.player.deck.iter().any(|c| c.def.id == "regret"));
        assert!(!r.player.deck.iter().any(|c| c.def.id == "clumsy"));
        assert!(
            r.player.deck.iter().any(|c| c.def.id == "ascenders_bane"),
            "不可移除的诅咒要留下"
        );
    }

    #[test]
    fn lab_hands_over_three_potions() {
        let r = apply("lab", 27, 0);
        assert_eq!(r.player.potions.iter().filter(|p| p.is_some()).count(), 3);
    }

    #[test]
    fn woman_in_blue_sells_potions_for_gold() {
        let mut r = Run::new(28);
        let def = event_def("the_woman_in_blue").unwrap();
        open(&mut r, def);
        r.choose_event(2).unwrap();
        assert_eq!(r.player.gold, 99 - 40, "三瓶收 40 金币");
        assert_eq!(r.player.potions.iter().filter(|p| p.is_some()).count(), 3);
    }

    #[test]
    fn face_trader_touch_pays_hp_for_gold() {
        let r = apply("face_trader", 29, 0);
        assert_eq!(r.player.hp, 80 - 8, "按上限的 10% 掉血");
        assert_eq!(r.player.gold, 99 + 75);
    }

    #[test]
    fn tomb_of_lord_red_mask_trades_all_gold_for_the_mask() {
        let r = apply("tomb_of_lord_red_mask", 30, 1);
        assert_eq!(r.player.gold, 0);
        assert!(r.player.relics.iter().any(|x| x.id == "red_mask"));
    }

    #[test]
    fn winding_halls_retrace_costs_max_hp() {
        let r = apply("winding_halls", 31, 2);
        assert_eq!(r.player.max_hp, 76, "往回走永久掉 5% 上限(80 的 5% = 4)");
        assert_eq!(r.player.hp, 76);
    }

    #[test]
    fn moai_head_offer_idol_pays_333() {
        let mut r = Run::new(32);
        r.player
            .relics
            .push(crate::core::relics::relic_def_or_panic("golden_idol"));
        let def = event_def("the_moai_head").unwrap();
        open(&mut r, def);
        r.choose_event(1).unwrap();
        assert!(!r.player.relics.iter().any(|x| x.id == "golden_idol"));
        assert_eq!(r.player.gold, 99 + 333);
    }

    #[test]
    fn secret_portal_drops_you_at_the_boss() {
        let r = apply("secret_portal", 33, 0);
        assert_eq!(r.screen, Screen::Combat);
        assert_eq!(r.pos, Some(r.map.boss), "人应该直接站在 Boss 房");
    }

    #[test]
    fn the_joust_takes_the_wager_and_may_pay_out() {
        let r = apply("the_joust", 34, 0);
        // 押凶手:一定先扣 50,七成再补 100
        assert!(
            r.player.gold == 99 - 50 || r.player.gold == 99 - 50 + 100,
            "侏儒赛马的赌注结算不对: {}",
            r.player.gold
        );
    }

    #[test]
    fn sensory_stone_deep_recall_costs_hp_and_gives_cards() {
        let r = apply("sensory_stone", 35, 2);
        assert_eq!(r.player.hp, 80 - 10);
        assert_eq!(r.player.deck.len(), 10 + 3, "三张无色牌进牌组");
        for c in r.player.deck.iter().rev().take(3) {
            assert_eq!(
                crate::core::corpus::CARDS
                    .iter()
                    .find(|x| x.id == c.def.id)
                    .map(|x| x.color),
                Some("colorless"),
                "{} 不是无色牌",
                c.def.id
            );
        }
    }

    #[test]
    fn knowing_skull_raises_the_price_each_purchase() {
        let mut r = Run::new(36);
        let def = event_def("knowing_skull").unwrap();
        open(&mut r, def);
        let hp0 = r.player.hp;
        r.choose_event(0).unwrap();
        let first = hp0 - r.player.hp;
        assert_eq!(r.player.gold, 99 + 90, "买一次给 90 金币");
        assert_eq!(first, 8, "上限 80 的 10% 是 8(最低 6)");
        // 涨价之后还在同一事件的下一屏
        assert_eq!(r.screen, Screen::Event);
        assert_eq!(r.event.as_ref().unwrap().def.choices.len(), 4);
        let hp1 = r.player.hp;
        r.choose_event(0).unwrap();
        assert_eq!(hp1 - r.player.hp, 9, "第二次买同一档要贵 1 点");
    }

    #[test]
    fn colosseum_first_fight_returns_to_the_event() {
        let mut r = Run::new(37);
        let def = event_def("colosseum").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        r.debug_win_battle();
        // 胜利后要停留几帧才结算,这里直接把定格走完
        let mut guard = 0;
        while r.screen == Screen::Combat && guard < 200 {
            guard += 1;
            r.tick_win_hold();
        }
        assert_eq!(r.screen, Screen::Event, "第一场打完要回到看台");
        assert_eq!(r.event.as_ref().unwrap().def.choices.len(), 2);
        let gold = r.player.gold;
        r.choose_event(1).unwrap();
        assert_eq!(r.combat().unwrap().encounter_id, "event_colosseum_nobs");
        assert_eq!(r.player.gold, gold, "第二场的奖励在打赢之后才给");
    }

    #[test]
    fn cursed_tome_pages_charge_escalating_hp() {
        let mut r = Run::new(38);
        let def = event_def("cursed_tome").unwrap();
        open(&mut r, def);
        let hp0 = r.player.hp;
        r.choose_event(0).unwrap(); // 读第一页
        assert_eq!(r.player.hp, hp0, "翻到第一页还不掉血");
        r.choose_event(0).unwrap(); // 翻第二页:1 点
        assert_eq!(r.player.hp, hp0 - 1);
        r.choose_event(0).unwrap(); // 翻到读完:2 点
        assert_eq!(r.player.hp, hp0 - 3);
        // 最后一屏:留下或拿走书里的遗物
        assert!(r.event.as_ref().unwrap().def.choices[0].label.starts_with("Take"));
        let relics = r.player.relics.len();
        r.choose_event(0).unwrap();
        assert_eq!(r.player.hp, hp0 - 13, "拿走要再掉 10 点");
        assert_eq!(r.player.relics.len(), relics + 1, "书里掉出一件遗物");
    }

    #[test]
    fn mindbloom_rich_gives_gold_and_normality() {
        let r = apply("mindbloom", 39, 2);
        assert_eq!(r.player.gold, 99 + 999);
        assert_eq!(
            r.player.deck.iter().filter(|c| c.def.id == "normality").count(),
            2
        );
    }

    #[test]
    fn mindbloom_awake_upgrades_everything() {
        let r = apply("mindbloom", 39, 1);
        assert!(
            r.player.deck.iter().all(|c| c.upgraded || !c.can_upgrade()),
            "我是清醒要把能升的都升了"
        );
        assert!(r.player.relics.iter().any(|x| x.id == "mark_of_the_bloom"));
    }

    #[test]
    fn augmenter_jax_adds_the_card() {
        let r = apply("augmenter", 40, 0);
        assert!(r.player.deck.iter().any(|c| c.def.id == "jax"));
    }

    #[test]
    fn ominous_forge_rummage_grants_tongs_and_pain() {
        let r = apply("ominous_forge", 41, 1);
        assert!(r.player.relics.iter().any(|x| x.id == "warped_tongs"));
        assert!(r.player.deck.iter().any(|c| c.def.id == "pain"));
    }

    #[test]
    fn falling_land_removes_a_skill() {
        let mut r = Run::new(42);
        r.player.deck.push(crate::core::cards::card("shrug_it_off"));
        let def = event_def("falling").unwrap();
        open(&mut r, def);
        let skills = r
            .player
            .deck
            .iter()
            .filter(|c| c.kind() == CardType::Skill)
            .count();
        r.choose_event(0).unwrap();
        assert_eq!(
            r.player.deck.iter().filter(|c| c.kind() == CardType::Skill).count(),
            skills - 1,
            "坠落会丢掉一张技能牌"
        );
    }

    #[test]
    fn we_meet_again_needs_a_potion_for_the_potion_trade() {
        let mut r = Run::new(43);
        let def = event_def("we_meet_again").unwrap();
        open(&mut r, def);
        assert!(!r.event_choice_available(0), "没药水就不能给药水");
        assert!(r.event_choice_available(1), "有 99 金币就能给钱");
        let potion = crate::core::potions::POTIONS.first().unwrap();
        r.add_potion(potion);
        assert!(r.event_choice_available(0), "有药水之后就能给");
        r.choose_event(0).unwrap();
        assert!(r.player.potions.iter().all(|p| p.is_none()), "药水被拿走");
        assert_eq!(r.player.relics.len(), 2);
    }

    #[test]
    fn nloth_takes_a_relic_and_leaves_a_gift() {
        let r = apply("nloth", 44, 0);
        assert_eq!(r.player.relics.len(), 2, "吃一件给一件");
        assert!(r.player.relics.iter().any(|x| x.id == "nloths_gift"));
    }

    #[test]
    fn wheel_of_change_settles_exactly_one_outcome() {
        // 落点随随机流变过,下面每个种子都是按现在的 miscRng 量出来的具体结果
        // (抽到"随机遗物"那一格的种子,落点是当前池子里量出来的那件)
        let relic = apply("wheel_of_change", 6, 0);
        assert!(relic.player.relics.iter().any(|r| r.id == "bottled_lightning"));
        assert_eq!(relic.player.gold, 99, "随机遗物那一格不该动金币");
        let berry = apply("wheel_of_change", 1, 0);
        assert_eq!(berry.player.max_hp, 80, "随机遗物那一格不动生命上限");
        assert!(berry.player.relics.iter().any(|r| r.id == "tiny_chest"));
        let bowl = apply("wheel_of_change", 2, 0);
        assert!(bowl.player.relics.iter().any(|r| r.id == "shuriken"));
        assert_eq!(bowl.player.gold, 99, "随机遗物那一格不该动金币");
        let heal = apply("wheel_of_change", 4, 0);
        assert_eq!((heal.player.hp, heal.player.max_hp), (80, 80));
        assert_eq!(heal.player.relics.len(), 1, "满血那一格不该给别的东西");
        let curse = apply("wheel_of_change", 12, 0);
        assert_eq!(curse.player.deck.len(), 11, "诅咒那一格多一张牌");
        assert!(curse.player.deck.iter().any(|c| c.def.id == "decay"));
        let toke = apply("wheel_of_change", 10, 0);
        assert!(toke.picker.is_some(), "移除那一格该开选牌");
        let hurt = apply("wheel_of_change", 3, 0);
        assert_eq!((hurt.player.hp, hurt.player.max_hp), (72, 80), "掉血那一格");
        assert_eq!(hurt.player.deck.len(), 10);
    }

    #[test]
    fn mushroom_fight_rewards_the_odd_mushroom() {
        let mut r = Run::new(45);
        let def = event_def("hypnotizing_colored_mushrooms").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert_eq!(r.combat().unwrap().encounter_id, "event_three_fungi");
        let gold = r.player.gold;
        r.debug_win_battle();
        let mut guard = 0;
        while r.screen == Screen::Combat && guard < 200 {
            guard += 1;
            r.tick_win_hold();
        }
        assert_eq!(r.screen, Screen::Reward);
        let reward = r.reward.as_ref().unwrap();
        assert_eq!(reward.relic.map(|d| d.id), Some("odd_mushroom"));
        assert!((20..=30).contains(&reward.gold), "金币 {}", reward.gold);
        assert_eq!(r.player.gold, gold, "金币要等玩家自己拿走");
    }

    #[test]
    fn mindbloom_war_fights_a_phantom_boss() {
        let mut r = Run::new(46);
        let def = event_def("mindbloom").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        let enc = r.combat().unwrap().encounter_id;
        assert!(
            [
                "event_phantom_guardian",
                "event_phantom_hexaghost",
                "event_phantom_slime_boss"
            ]
            .contains(&enc),
            "幻影 Boss 不对: {enc}"
        );
        r.debug_win_battle();
        let mut guard = 0;
        while r.screen == Screen::Combat && guard < 200 {
            guard += 1;
            r.tick_win_hold();
        }
        let reward = r.reward.as_ref().expect("打完要有奖励");
        assert_eq!(reward.gold, 50);
        assert_eq!(reward.relic.map(|d| d.rarity()), Some(Rarity::Rare));
    }

    /// 直接开一局翻牌小游戏
    fn match_keep(seed: u64) -> Run {
        let mut r = Run::new(seed);
        r.debug_open_event("match_and_keep").expect("事件应存在");
        r
    }

    /// 当前棋盘
    fn board_of(r: &Run) -> &MatchKeep {
        r.event
            .as_ref()
            .expect("要有事件")
            .match_keep
            .as_ref()
            .expect("要有棋盘")
    }

    /// 卡名
    fn name_of(id: &str) -> &'static str {
        crate::core::cards::card_def(id).expect("认得出来").name
    }

    /// 找同一个牌位的两格(一定能配成对)
    fn pair_slots(mk: &MatchKeep) -> (usize, usize) {
        let a = 0;
        let b = (0..12).find(|s| *s != a && mk.board[*s] == mk.board[a]).expect("每张牌都有两张");
        (a, b)
    }

    /// 找两个不同牌位的格子(一定配不成对)
    fn miss_slots(mk: &MatchKeep) -> (usize, usize) {
        let a = 0;
        let b = (0..12).find(|s| mk.board[*s] != mk.board[a]).expect("6 个牌位不止一种");
        (a, b)
    }

    #[test]
    fn match_and_keep_board_has_six_pairs() {
        let r = match_keep(47);
        let mk = board_of(&r);
        assert_eq!(mk.board.len(), 12, "棋盘 12 格");
        let mut counts = [0u8; 6];
        for slot in 0..12 {
            counts[mk.board[slot] as usize] += 1;
        }
        assert_eq!(counts, [2; 6], "6 个牌位每个各两张");
        // 6 个牌位依次是:稀有/非普通/普通本职业牌、非普通无色牌、随机诅咒、本职业起始牌
        let slot_card = |i: usize| {
            crate::core::cards::card_def(mk.slots[i].expect("牌位都要有牌"))
                .expect("牌位认得出来")
        };
        assert_eq!(slot_card(0).rarity, Rarity::Rare, "第一个牌位是稀有本职业牌");
        assert_eq!(slot_card(1).rarity, Rarity::Uncommon, "第二个是非普通本职业牌");
        assert_eq!(slot_card(2).rarity, Rarity::Common, "第三个是普通本职业牌");
        assert_eq!(
            crate::core::cards::pool_of(slot_card(3)),
            "colorless",
            "第四个是无色牌"
        );
        assert_eq!(slot_card(3).rarity, Rarity::Uncommon, "无色牌是非普通的");
        assert_eq!(slot_card(4).kind, CardType::Curse, "第五个是诅咒");
        assert_eq!(mk.slots[5], Some("bash"), "第六个是铁甲战士的起始牌");
    }

    #[test]
    fn match_and_keep_board_is_reproducible() {
        let a = match_keep(1234);
        let b = match_keep(1234);
        assert_eq!(board_of(&a), board_of(&b), "同 seed 两次进事件棋盘一致");
        let c = match_keep(4321);
        assert_ne!(
            board_of(&a).board,
            board_of(&c).board,
            "换 seed 应该铺出不一样的棋盘"
        );
    }

    #[test]
    fn match_and_keep_first_flip_only_turns_one_card() {
        let mut r = match_keep(47);
        let deck = r.player.deck.len();
        r.choose_event(0).unwrap();
        assert_eq!(board_of(&r).first, Some(0), "第一张是刚翻开的");
        assert_eq!(board_of(&r).attempts, 0, "只翻一张不算一次尝试");
        let name = board_of(&r).card_at(0).expect("第一格有牌");
        assert_eq!(
            board_of(&r).label(0),
            format!("Card 1: {}", name_of(name)),
            "翻开的格子显示牌名"
        );
        assert_eq!(board_of(&r).label(1), "Flip card 2", "没翻的格子还是牌背");
        assert_eq!(
            board_of(&r).note.as_deref(),
            Some(format!("you flip {}", name_of(name)).as_str()),
            "棋盘下面说明翻出了什么"
        );
        assert!(!r.event_choice_available(0), "刚翻开的格子不能再翻");
        assert!(r.event_choice_available(1), "别的格子还能翻");
        assert_eq!(r.player.deck.len(), deck, "只翻一张不给牌");
    }

    #[test]
    fn match_and_keep_matched_pair_joins_the_deck() {
        let mut r = match_keep(47);
        let (a, b) = pair_slots(board_of(&r));
        let card = board_of(&r).card_at(a).expect("这一格有牌");
        r.choose_event(a).unwrap();
        let deck = r.player.deck.len();
        r.choose_event(b).unwrap();
        assert!(
            board_of(&r).matched[a] && board_of(&r).matched[b],
            "同源的两张都配对"
        );
        assert_eq!(board_of(&r).attempts, 1, "翻第二张才算一次尝试");
        assert_eq!(r.player.deck.len(), deck + 1, "配对的牌进牌组");
        assert_eq!(
            r.player.deck.last().unwrap().def.id,
            card,
            "进牌组的正是那一格的牌"
        );
        assert_eq!(
            board_of(&r).label(a),
            format!("Card {}: {}", a + 1, name_of(card)),
            "配对的格子显示牌名"
        );
        assert!(!r.event_choice_available(a), "配对的格子不能再翻");
        assert_eq!(
            board_of(&r).note.as_deref(),
            Some(format!("{} matches: it joins your deck", name_of(card)).as_str()),
            "棋盘下面说明配对成功"
        );
        // 之后再翻别的格子,配对的这两张一直朝上
        let other = (0..12)
            .find(|s| *s != a && *s != b)
            .expect("棋盘上还有别的格子");
        r.choose_event(other).unwrap();
        assert!(board_of(&r).matched[a], "配对之后一直朝上");
        assert_eq!(
            board_of(&r).label(a),
            format!("Card {}: {}", a + 1, name_of(card))
        );
    }

    #[test]
    fn match_and_keep_mismatch_turns_both_back() {
        let mut r = match_keep(47);
        let (a, b) = miss_slots(board_of(&r));
        let (card_a, card_b) = (
            board_of(&r).card_at(a).expect("第一格有牌"),
            board_of(&r).card_at(b).expect("第二格有牌"),
        );
        let deck = r.player.deck.len();
        let (hp, gold) = (r.player.hp, r.player.gold);
        r.choose_event(a).unwrap();
        r.choose_event(b).unwrap();
        let mk = board_of(&r);
        assert_eq!(mk.attempts, 1, "配错也只用掉一次尝试");
        assert_eq!(mk.first, None, "结算完就没有翻开的了");
        assert_eq!(mk.label(a), format!("Flip card {}", a + 1), "配错的两张翻回背面");
        assert_eq!(mk.label(b), format!("Flip card {}", b + 1));
        assert!(r.event_choice_available(a), "配错的格子还能再翻");
        assert_eq!(
            mk.note.as_deref(),
            Some(
                format!(
                    "{} and {} do not match",
                    name_of(card_a),
                    name_of(card_b)
                )
                .as_str()
            ),
            "棋盘下面说明哪两张没配上"
        );
        assert_eq!(r.player.deck.len(), deck, "配错不给牌");
        assert_eq!((r.player.hp, r.player.gold), (hp, gold), "配错不动血和钱");
    }

    #[test]
    fn match_and_keep_ends_after_five_attempts() {
        let mut r = match_keep(47);
        let (a, b) = miss_slots(board_of(&r));
        for n in 0..5 {
            r.choose_event(a).unwrap();
            r.choose_event(b).unwrap();
            assert_eq!(board_of(&r).attempts, n + 1);
            if n < 4 {
                assert!(r.event.as_ref().unwrap().result.is_none(), "还没翻完就接着翻");
            }
        }
        assert_eq!(board_of(&r).attempts, 5, "5 次尝试用完");
        assert!(
            r.event.as_ref().unwrap().result.is_some(),
            "次数用完事件结束,只剩离开"
        );
        assert!(!r.event_choice_available(a), "结束之后不能接着翻");
        assert!(r.choose_event(a).is_err(), "结束之后不能再翻");
        r.leave_event();
        assert!(r.event.is_none(), "可以离开");
    }

    #[test]
    fn the_library_reads_or_sleeps() {
        let mut r = Run::new(48);
        let def = event_def("the_library").unwrap();
        open(&mut r, def);
        let before = r.player.deck.len();
        r.choose_event(0).unwrap();
        assert_eq!(r.player.deck.len(), before + 1, "读一本书得一张本职业牌");

        let mut r = Run::new(48);
        r.player.hp = 40;
        open(&mut r, def);
        r.choose_event(1).unwrap();
        assert_eq!(r.player.hp, 40 + 26, "睡觉按上限的 33% 回血");
    }

    #[test]
    fn mysterious_sphere_fight_pays_a_rare_relic() {
        let mut r = Run::new(49);
        let def = event_def("mysterious_sphere").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert_eq!(r.combat().unwrap().encounter_id, "event_two_orbs");
        r.debug_win_battle();
        let mut guard = 0;
        while r.screen == Screen::Combat && guard < 200 {
            guard += 1;
            r.tick_win_hold();
        }
        let reward = r.reward.as_ref().expect("开球要有奖励");
        assert!((45..=55).contains(&reward.gold), "金币 {}", reward.gold);
        assert_eq!(reward.relic.map(|d| d.rarity()), Some(Rarity::Rare));
    }

    #[test]
    fn note_for_yourself_swaps_a_card() {
        let mut r = Run::new(50);
        let def = event_def("note_for_yourself").unwrap();
        open(&mut r, def);
        r.choose_event(0).unwrap();
        assert!(r.player.deck.iter().any(|c| c.def.id == "iron_wave"));
        assert_eq!(r.screen, Screen::Pick, "拿走之后要选一张留下");
        let before = r.player.deck.len();
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), before - 1);
    }

    #[test]
    fn designer_in_spire_sells_services_for_gold() {
        let mut r = Run::new(51);
        r.player.gold = 200;
        let def = event_def("designer_in_spire").unwrap();
        open(&mut r, def);
        let upgraded0 = r.player.deck.iter().filter(|c| c.upgraded).count();
        r.choose_event(2).unwrap();
        assert_eq!(r.player.gold, 200 - 90, "全套服务收 90 金币");
        assert_eq!(
            r.player.deck.iter().filter(|c| c.upgraded).count(),
            upgraded0 + 1,
            "全套服务会随机升一张"
        );
        assert_eq!(r.screen, Screen::Pick, "然后还要选一张删掉");
        let before = r.player.deck.len();
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), before - 1);
    }

    #[test]
    fn lookup_works() {
        let e = event_def("big_fish").expect("big_fish 应存在");
        assert_eq!(e.id, "big_fish");
        assert!(event_def("no_such_event").is_none());
    }
}
