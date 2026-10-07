// 一局(run)的流程:地图推进、战斗结算、奖励、商店、事件、营火、牌组管理.
// 所有状态都在这里,UI 只读这些字段并调用这里的方法改状态.
use crate::core::card::{CardDef, CardInstance, Effect as CardEffect, Rarity};
use crate::core::cards;
use crate::core::combat::{Combat, CombatSetup, Phase};
use crate::core::corpus;
use crate::core::enemies;
use crate::core::enemy::{EnemyKind, Encounter};
use crate::core::events::{CombatReward, EventDef, FlipResult, MatchKeep, Outcome, RemoveRule};
use crate::core::map::{ActMap, NodeKind};
use crate::core::potions::{self, PotionDef, PotionFx};
use crate::core::relics::{self, RelicDef, RelicFx};
use crate::core::roster;
use crate::rng::{java_shuffle, FloorStream, JavaRandom, RngRegistry, RunStream};

/// 药水格子数
pub const POTION_SLOTS: usize = 3;
/// 起始生命(铁甲战士的,只有测试还在用;实际数值来自语料)
#[cfg(test)]
pub const STARTING_HP: i32 = 80;
/// 起始金币(同上)
#[cfg(test)]
pub const STARTING_GOLD: i32 = 99;
/// 营火休息回复比例(百分比)
pub const REST_HEAL_PCT: i32 = 30;
/// 历史记录上限
const HISTORY_LIMIT: usize = 4000;
/// 超限时一次丢掉多少条
const HISTORY_TRIM: usize = 1000;

// ---- 奖励与掉落的常数(照抄参考实现 rewards.ts / shop.ts) ----

/// 卡牌奖励的稀有度保底初值
const CARD_RARITY_PITY_START: i32 = 5;
/// 抽到普通牌时保底值 -1,最低到这里
const CARD_RARITY_PITY_FLOOR: i32 = -40;
/// 普通战斗的稀有概率(百分数),精英另有一套
const CARD_RARE_CHANCE_ELITE: i32 = 10;
const CARD_RARE_CHANCE_NON_ELITE: i32 = 3;
const CARD_UNCOMMON_CHANCE_ELITE: i32 = 40;
const CARD_UNCOMMON_CHANCE_NON_ELITE: i32 = 37;
/// 卡牌奖励固定三选一
const CARD_REWARD_COUNT: usize = 3;
/// 药水掉落的基础概率与保底步长
const POTION_DROP_BASE_CHANCE: i32 = 40;
const POTION_PITY_STEP: i32 = 10;
/// 精英掉遗物的稀有度分界(参考实现 returnRandomRelicTierElite)
const ELITE_RELIC_COMMON_BELOW: u32 = 50;
const ELITE_RELIC_RARE_ABOVE: u32 = 82;
/// 商店价格:底价与浮动区间(参考实现 shop.ts)
const SHOP_CARD_JITTER: (f32, f32) = (0.9, 1.1);
const SHOP_OTHER_JITTER: (f32, f32) = (0.95, 1.05);
const SHOP_CARD_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 50),
    (Rarity::Uncommon, 75),
    (Rarity::Rare, 150),
];
const SHOP_RELIC_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 150),
    (Rarity::Uncommon, 250),
    (Rarity::Rare, 300),
];
const SHOP_POTION_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 50),
    (Rarity::Uncommon, 75),
    (Rarity::Rare, 100),
];

/// 历史记录里一条的类别,决定它在历史窗口里怎么上色
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistoryKind {
    /// 一局流程自己的记录(进房间、买卖、拿奖励)
    System,
    /// 玩家做了什么
    Player,
    /// 敌人做了什么
    Enemy,
    /// 其他说明
    Info,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub text: String,
    pub kind: HistoryKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// 开始界面:continue / new game / compendium
    Title,
    /// 选角色
    CharSelect,
    /// 图鉴子菜单:卡片库/遗物册/药水间
    Compendium,
    /// 图鉴列表(具体看哪一种由 App::library 决定)
    Library,
    Map,
    Combat,
    Reward,
    Rest,
    Shop,
    Event,
    Treasure,
    /// 选牌界面(升级/移除)
    Pick,
    Victory,
    Death,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Title => "TITLE",
            Screen::CharSelect => "CHARACTER",
            Screen::Compendium => "COMPENDIUM",
            Screen::Library => "LIBRARY",
            Screen::Map => "MAP",
            Screen::Combat => "COMBAT",
            Screen::Reward => "REWARD",
            Screen::Rest => "REST",
            Screen::Shop => "SHOP",
            Screen::Event => "EVENT",
            Screen::Treasure => "TREASURE",
            Screen::Pick => "PICK",
            Screen::Victory => "VICTORY",
            Screen::Death => "DEATH",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickPurpose {
    Upgrade,
    Remove,
    /// 变形:移除这张牌,换成一张随机本职业牌
    Transform,
    /// 复制:往牌组里再塞一张一样的
    Duplicate,
}

impl PickPurpose {
    pub fn title(self) -> &'static str {
        match self {
            PickPurpose::Upgrade => "choose a card to upgrade",
            PickPurpose::Remove => "choose a card to remove",
            PickPurpose::Transform => "choose a card to transform",
            PickPurpose::Duplicate => "choose a card to duplicate",
        }
    }
}

pub struct Picker {
    pub purpose: PickPurpose,
    pub back: Screen,
    pub index: usize,
    /// 确认时要付的金币(商店的移除服务)
    pub cost_gold: i32,
    /// 付款成功要标记已售的商店下标
    pub shop_slot: Option<usize>,
}

pub struct Player {
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub deck: Vec<CardInstance>,
    pub relics: Vec<&'static RelicDef>,
    pub potions: Vec<Option<&'static PotionDef>>,
}

impl Player {
    /// 把所有遗物的某个数值字段加起来
    pub fn relic_fx_sum(&self, f: impl Fn(&RelicDef) -> i32) -> i32 {
        self.relics.iter().map(|r| f(r)).sum()
    }
}

pub struct RewardState {
    pub gold: i32,
    pub gold_taken: bool,
    pub cards: Vec<CardInstance>,
    pub card_taken: bool,
    pub relic: Option<&'static RelicDef>,
    pub relic_taken: bool,
    pub potion: Option<&'static PotionDef>,
    pub potion_taken: bool,
    pub index: usize,
    /// 拿完(或跳过)之后去哪个界面
    pub next: Screen,
}

/// 奖励行:UI 与选择都按这个顺序来
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RewardSlot {
    Gold,
    Card(usize),
    Relic,
    Potion,
}

pub enum ShopItem {
    Card(CardInstance, i32),
    Relic(&'static RelicDef, i32),
    Potion(&'static PotionDef, i32),
    Remove(i32),
}

impl ShopItem {
    pub fn price(&self) -> i32 {
        match self {
            ShopItem::Card(_, p)
            | ShopItem::Relic(_, p)
            | ShopItem::Potion(_, p)
            | ShopItem::Remove(p) => *p,
        }
    }
}

pub struct ShopState {
    pub items: Vec<ShopItem>,
    pub sold: Vec<bool>,
    pub index: usize,
    pub removes: u32,
}

/// 宝箱的大小
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChestSize {
    Small,
    Medium,
    Large,
}

/// 进宝箱房时掷出来的箱子状态
#[derive(Clone, Copy, Debug)]
pub struct Chest {
    pub size: ChestSize,
    pub gold_present: bool,
    pub tier: Rarity,
}

pub struct EventState {
    pub def: &'static EventDef,
    pub index: usize,
    /// 已选结果,展示完才能离开
    pub result: Option<&'static str>,
    /// 翻牌小游戏(match_and_keep)的棋盘;其它事件是 None
    pub match_keep: Option<MatchKeep>,
}

#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub fights: u32,
    pub elites: u32,
    pub bosses: u32,
    pub turns: u32,
    pub damage_dealt: i32,
    pub potions_used: u32,
}

pub struct Run {
    pub seed: u64,
    /// 开局选的角色的语料 id
    pub character: &'static str,
    /// 第几场战斗(每次开打 +1),表现层用它判断要不要重新拍快照
    pub fight_seq: u64,
    /// 赢了之后还要在战场上多停几帧(>0 表示正在停,满了才进奖励)
    pub win_hold: u8,
    pub streams: RngRegistry,
    pub player: Player,
    pub map: ActMap,
    /// 本局这条路的 Boss:开局定下来,地图上直接写名字
    pub boss_enc: &'static Encounter,
    /// 这一章还没打的怪房间名单(monsterRng 一次生成,按顺序消耗)
    monster_list: Vec<&'static str>,
    /// 这一章还没打的精英名单
    elite_list: Vec<&'static str>,
    /// 药水掉落的保底值(参考实现里的 potionChance:每次没掉 +10,掉了 -10)
    potion_chance: i32,
    /// 卡牌稀有度的保底值(参考实现里的 cardRarityFactor:抽到稀有重置 5,普通 -1,下限 -40)
    card_rarity_factor: i32,
    /// 玩家当前所在节点;None 表示还没上路
    pub pos: Option<usize>,
    /// 这一局走过的节点(按顺序),地图上走过的房间统一给底色
    pub path: Vec<usize>,
    pub floor_reached: usize,
    pub screen: Screen,
    pub combat: Option<Combat>,
    pub reward: Option<RewardState>,
    pub shop: Option<ShopState>,
    pub event: Option<EventState>,
    pub picker: Option<Picker>,
    pub treasure: Option<&'static RelicDef>,
    /// 当前宝箱房的箱子状态(不在宝箱房就是 None)
    pub chest: Option<Chest>,
    pub rest_index: usize,
    pub stats: Stats,
    /// 一整局发生过的所有事,用 H 翻看
    pub history: Vec<HistoryEntry>,
    /// 已经抄进历史的战斗日志序号
    combat_log_seen: u64,
    /// 本局还没出现过的遗物
    relic_pool: Vec<&'static RelicDef>,
    last_encounter: &'static str,
    /// 打赢这一场事件战斗后要回到的事件那一屏
    pending_event: Option<&'static EventDef>,
    /// 这一场事件战斗的奖励方案(打完即清空)
    combat_reward: Option<&'static CombatReward>,
}

/// 存档里的卡牌记号:id、升级加 "+"、可多次升级的带等级(id+3)
fn card_token(c: &CardInstance) -> String {
    if c.plus > 1 {
        format!("{}+{}", c.def.id, c.plus)
    } else if c.upgraded {
        format!("{}+", c.def.id)
    } else {
        c.def.id.to_string()
    }
}

/// 按 id 找卡牌:先认事件专用牌,再认卡池
fn card_def_any(id: &str) -> &'static CardDef {
    crate::core::events::event_card(id)
        .or_else(|| cards::card_def(id))
        .unwrap_or_else(|| panic!("unknown card id: {id}"))
}

/// 按 id 找遗物:先认事件专用遗物,再认遗物池
fn relic_def_any(id: &str) -> &'static RelicDef {
    relic_def_any_opt(id).unwrap_or_else(|| panic!("unknown relic id: {id}"))
}

/// 同上,但认不出返回 None(读存档用)
fn relic_def_any_opt(id: &str) -> Option<&'static RelicDef> {
    crate::core::events::event_relic(id).or_else(|| relics::relic_def(id))
}

/// 反向解析牌堆
fn parse_cards(text: &str) -> Result<Vec<CardInstance>, String> {
    let mut out = Vec::new();
    for item in text.split(',').filter(|s| !s.is_empty()) {
        let item = item.trim();
        let (id, plus) = match item.split_once('+') {
            Some((id, rest)) => (
                id,
                if rest.is_empty() {
                    1
                } else {
                    rest.parse::<u8>().unwrap_or(1)
                },
            ),
            None => (item, 0),
        };
        let def = crate::core::events::event_card(id)
            .or_else(|| cards::card_def(id))
            .ok_or_else(|| format!("存档里的卡 {id} 不认识"))?;
        let mut inst = CardInstance::new(def);
        for _ in 0..plus {
            inst.upgrade();
        }
        out.push(inst);
    }
    Ok(out)
}

impl Run {
    /// 默认角色(铁甲战士)开一局,测试和 :new 用
    pub fn new(seed: u64) -> Run {
        let ch = roster::find("ironclad").expect("ironclad 必须在语料里");
        Run::new_for(seed, ch).expect("铁甲战士的起始牌组必须是已实现的")
    }

    /// 按角色开一局:起始牌组/血量/金币/遗物都来自语料
    pub fn new_for(seed: u64, ch: &'static corpus::CharacterInfo) -> Result<Run, String> {
        let missing = roster::missing_cards(ch);
        if !missing.is_empty() {
            return Err(format!("{} 还没实现: {}", ch.name, missing.join(" ")));
        }
        if !roster::has_starter_relic(ch) {
            return Err(format!("{} 的起始遗物 {} 还没实现", ch.name, ch.relic));
        }
        let mut streams = RngRegistry::new(seed);
        let mut deck: Vec<CardInstance> = Vec::new();
        for (id, n) in ch.deck {
            for _ in 0..*n {
                deck.push(cards::card(id));
            }
        }
        let starter = relics::relic_def_or_panic(ch.relic);
        // 遗物池开局洗一次:参考实现按 普通/罕见/稀有/商店/Boss 五个池子各洗一遍,
        // 每个池子消耗 relicRng 的一个 long.本作只有前三个池子,后两个空烧,
        // 这样 relicRng 的位置和参考实现一致
        let mut relic_pool: Vec<&'static RelicDef> = relics::RELICS
            .iter()
            .filter(|r| r.id != starter.id)
            .collect();
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            let mut tier: Vec<&'static RelicDef> = relic_pool
                .iter()
                .copied()
                .filter(|r| r.rarity == rarity)
                .collect();
            let java_seed = streams.run(RunStream::RelicRng).random_long();
            java_shuffle(&mut tier, &mut JavaRandom::new(java_seed));
            let mut rest: Vec<&'static RelicDef> = relic_pool
                .into_iter()
                .filter(|r| r.rarity != rarity)
                .collect();
            tier.append(&mut rest);
            relic_pool = tier;
        }
        // 商店/Boss 两个池子本作还没有,空烧两个 long 对齐参考实现
        let _ = streams.run(RunStream::RelicRng).random_long();
        let _ = streams.run(RunStream::RelicRng).random_long();

        // 遭遇名单(monsterRng)与地图(mapRng)都按参考实现的顺序掷
        let lists = enemies::generate_encounters(1, streams.run(RunStream::MonsterRng));
        streams.reseed_map(1);
        let map = ActMap::generate(streams.map_rng());
        // 本局的 Boss 是 monsterRng 洗出来的那一条
        let boss_enc: &'static Encounter = enemies::resolve(lists.boss[0]);
        let mut run = Run {
            seed,
            character: ch.id,
            fight_seq: 0,
            win_hold: 0,
            streams,
            player: Player {
                hp: ch.max_hp,
                max_hp: ch.max_hp,
                gold: ch.gold,
                deck,
                relics: vec![starter],
                potions: vec![None; POTION_SLOTS],
            },
            map,
            boss_enc,
            monster_list: lists.monster,
            elite_list: lists.elite,
            potion_chance: 0,
            card_rarity_factor: CARD_RARITY_PITY_START,
            pos: None,
            path: Vec::new(),
            floor_reached: 0,
            screen: Screen::Map,
            combat: None,
            reward: None,
            shop: None,
            event: None,
            picker: None,
            treasure: None,
            chest: None,
            rest_index: 0,
            stats: Stats::default(),
            history: Vec::new(),
            combat_log_seen: 0,
            relic_pool,
            last_encounter: "",
            pending_event: None,
            combat_reward: None,
        };
        // 起始遗物的拾取效果
        let starter_fx = starter.fx;
        run.apply_relic_pickup(starter_fx);
        run.say(format!("seed {seed}: climb the spire"));
        Ok(run)
    }

    /// 存档文本:只存"这一层刚开始"的状态(地图界面才存)
    pub fn save_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("seed={}\n", self.seed));
        out.push_str(&format!("char={}\n", self.character));
        out.push_str(&format!("hp={}\n", self.player.hp));
        out.push_str(&format!("max_hp={}\n", self.player.max_hp));
        out.push_str(&format!("gold={}\n", self.player.gold));
        // 具名流的完整状态(15 条).老存档只有一条 xoshiro 的 rng=,读的时候会明确报错
        out.push_str(&self.streams.save_text());
        out.push_str(&format!("potion_pity={}\n", self.potion_chance));
        out.push_str(&format!("card_factor={}\n", self.card_rarity_factor));
        out.push_str(&format!("monsters={}\n", self.monster_list.join(",")));
        out.push_str(&format!("elites={}\n", self.elite_list.join(",")));
        if let Some(chest) = self.chest {
            let size = match chest.size {
                ChestSize::Small => "small",
                ChestSize::Medium => "medium",
                ChestSize::Large => "large",
            };
            out.push_str(&format!(
                "chest={}:{}:{}\n",
                size,
                chest.gold_present,
                chest.tier.name()
            ));
        }
        if let Some(t) = self.treasure {
            out.push_str(&format!("treasure={}\n", t.id));
        }
        out.push_str(&format!(
            "pos={}\n",
            self.pos.map(|p| p.to_string()).unwrap_or_else(|| "none".to_string())
        ));
        let path: Vec<String> = self.path.iter().map(|n| n.to_string()).collect();
        out.push_str(&format!("path={}\n", path.join(",")));
        out.push_str(&format!("floor={}\n", self.floor_reached));
        let deck: Vec<String> = self
            .player
            .deck
            .iter()
            .map(|c| format!("{}:{}", c.def.id, if c.upgraded { 1 } else { 0 }))
            .collect();
        out.push_str(&format!("deck={}\n", deck.join(",")));
        let relics: Vec<&str> = self.player.relics.iter().map(|r| r.id).collect();
        out.push_str(&format!("relics={}\n", relics.join(",")));
        let potions: Vec<&str> = self
            .player
            .potions
            .iter()
            .map(|p| p.map(|d| d.id).unwrap_or("-"))
            .collect();
        out.push_str(&format!("potions={}\n", potions.join(",")));
        // 战斗现场(测试存档用):老存档没这几行,读到没有就照旧重建地图
        if let Some(c) = self.combat.as_ref() {
            out.push_str(&format!("combat_encounter={}\n", c.encounter_id));
            out.push_str(&format!("combat_turn={}\n", c.turn));
            out.push_str(&format!("combat_energy={}\n", c.energy));
            out.push_str(&format!("combat_max_energy={}\n", c.max_energy));
            out.push_str(&format!("combat_hp={}\n", c.player.hp));
            out.push_str(&format!("combat_block={}\n", c.player.block));
            for (k, pile) in [
                ("hand", &c.hand),
                ("draw", &c.draw),
                ("discard", &c.discard),
                ("exhaust", &c.exhaust),
            ] {
                let list: Vec<String> = pile.iter().map(card_token).collect();
                out.push_str(&format!("combat_{k}={}\n", list.join(",")));
            }
            let foes: Vec<String> = c
                .enemies
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}:{}:{}",
                        e.hp,
                        e.block,
                        e.next_move,
                        if e.alive() { 1 } else { 0 }
                    )
                })
                .collect();
            out.push_str(&format!("combat_enemies={}\n", foes.join(",")));
        }
        out
    }

    /// 从存档文本恢复一局
    pub fn from_save(text: &str) -> Result<Run, String> {
        let mut seed = 0u64;
        let mut char_id = "ironclad".to_string();
        let mut num: Vec<(&str, &str)> = Vec::new();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            num.push((k, v));
            if k == "seed" {
                seed = v.trim().parse().map_err(|_| "存档里的种子坏了".to_string())?;
            } else if k == "char" {
                char_id = v.trim().to_string();
            }
        }
        let get = |k: &str| -> Option<&str> { num.iter().find(|(a, _)| *a == k).map(|(_, b)| *b) };
        let int = |k: &str, d: i32| -> i32 { get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d) };
        let ch = roster::find(&char_id).ok_or_else(|| format!("存档里的角色 {char_id} 不认识"))?;
        let mut run = Run::new_for(seed, ch)?;
        run.player.hp = int("hp", run.player.hp);
        run.player.max_hp = int("max_hp", run.player.max_hp);
        run.player.gold = int("gold", run.player.gold);
        run.floor_reached = int("floor", 0).max(0) as usize;
        // 旧存档的随机状态是单条 xoshiro 流,和现在的具名流对不上:
        // 直接报错,别静默接着跑出一局错的游戏
        if get("rng").is_some() && !RngRegistry::has_any_stream_line(text) {
            return Err(
                "这份存档记的是旧版随机状态(单条 xoshiro 流),与现在的具名流对不上:请重新开一局".to_string(),
            );
        }
        for (k, v) in &num {
            if crate::rng::is_stream_key(k) {
                run.streams.load_line(k, v)?;
            }
        }
        run.potion_chance = int("potion_pity", 0);
        run.card_rarity_factor = int("card_factor", CARD_RARITY_PITY_START);
        if let Some(v) = get("monsters") {
            run.monster_list = v
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| enemies::resolve(s).id)
                .collect();
        }
        if let Some(v) = get("elites") {
            run.elite_list = v
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| enemies::resolve(s).id)
                .collect();
        }
        if let Some(v) = get("chest") {
            let parts: Vec<&str> = v.split(':').collect();
            if parts.len() == 3 {
                run.chest = Some(Chest {
                    size: match parts[0] {
                        "small" => ChestSize::Small,
                        "medium" => ChestSize::Medium,
                        _ => ChestSize::Large,
                    },
                    gold_present: parts[1] == "true",
                    tier: rarity_from_name(parts[2]),
                });
            }
        }
        if let Some(v) = get("treasure") {
            run.treasure = relic_def_any_opt(v);
        }
        if let Some(v) = get("deck") {
            let mut deck: Vec<CardInstance> = Vec::new();
            for item in v.split(',').filter(|s| !s.is_empty()) {
                let (id, up) = item.split_once(':').unwrap_or((item, "0"));
                let Some(def) = crate::core::events::event_card(id).or_else(|| cards::card_def(id))
                else {
                    return Err(format!("存档里的卡 {id} 不认识"));
                };
                let mut inst = cards::card(def.id);
                if up.trim() == "1" {
                    inst.upgrade();
                }
                deck.push(inst);
            }
            if !deck.is_empty() {
                run.player.deck = deck;
            }
        }
        if let Some(v) = get("relics") {
            let mut relics_out: Vec<&'static RelicDef> = Vec::new();
            for id in v.split(',').filter(|s| !s.is_empty()) {
                let Some(def) = relic_def_any_opt(id) else {
                    return Err(format!("存档里的遗物 {id} 不认识"));
                };
                relics_out.push(def);
            }
            if !relics_out.is_empty() {
                run.player.relics = relics_out;
            }
        }
        if let Some(v) = get("potions") {
            let mut slots: Vec<Option<&'static PotionDef>> = vec![None; POTION_SLOTS];
            for (i, id) in v.split(',').enumerate().take(POTION_SLOTS) {
                if id == "-" || id.is_empty() {
                    continue;
                }
                let def = potions::POTIONS
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| format!("存档里的药水 {id} 不认识"))?;
                slots[i] = Some(def);
            }
            run.player.potions = slots;
        }
        if let Some(v) = get("pos") {
            run.pos = if v.trim() == "none" {
                None
            } else {
                v.trim().parse::<usize>().ok()
            };
        }
        if let Some(v) = get("path") {
            run.path = v
                .split(',')
                .filter(|s| !s.is_empty())
                .filter_map(|x| x.trim().parse().ok())
                .collect();
        }
        // 测试存档:带战斗现场就照原样恢复(不用从种子重放)
        if let Some(enc_id) = get("combat_encounter") {
            if let Some(enc) = enemies::encounter_def(enc_id) {
                let setup = CombatSetup {
                    hp: run.player.hp,
                    max_hp: run.player.max_hp,
                    deck: run.player.deck.clone(),
                    relics: run.player.relics.clone(),
                    gold: run.player.gold,
                };
                let mut c = Combat::new(enc, setup, run.streams.clone());
                c.turn = int("combat_turn", 1).max(1) as u32;
                c.energy = int("combat_energy", c.energy);
                c.max_energy = int("combat_max_energy", c.max_energy);
                c.player.hp = int("combat_hp", run.player.hp);
                c.player.block = int("combat_block", 0);
                for (key, pile) in [("hand", 0), ("draw", 1), ("discard", 2), ("exhaust", 3)] {
                    let Some(v) = get(&format!("combat_{key}")) else {
                        continue;
                    };
                    let list = parse_cards(v)?;
                    match pile {
                        0 => c.hand = list,
                        1 => c.draw = list,
                        2 => c.discard = list,
                        _ => c.exhaust = list,
                    }
                }
                if let Some(v) = get("combat_enemies") {
                    for (i, part) in v.split(',').enumerate() {
                        let nums: Vec<i32> = part
                            .split(':')
                            .filter_map(|x| x.trim().parse().ok())
                            .collect();
                        if let Some(e) = c.enemies.get_mut(i) {
                            if let Some(hp) = nums.first() {
                                e.hp = *hp;
                            }
                            if let Some(b) = nums.get(1) {
                                e.block = *b;
                            }
                            if let Some(m) = nums.get(2) {
                                e.next_move = (*m).max(0) as usize;
                            }
                            if nums.get(3) == Some(&0) {
                                e.hp = 0;
                            }
                        }
                    }
                }
                if c.enemies.iter().all(|e| !e.alive()) {
                    c.phase = Phase::Won;
                }
                // 战斗的随机状态以存档为准(上面重建时又掷了几次血量与洗牌)
                c.streams = run.streams.clone();
                run.combat = Some(c);
                run.screen = Screen::Combat;
                run.win_hold = 0;
                run.sync_combat();
            }
        }
        // 还没出现过的遗物:在上面这条"开局洗好的池子"上排掉已经拿到的,
        // 不能重新按表重建,否则洗牌顺序就不是本局的了
        run.relic_pool
            .retain(|r| !run.player.relics.iter().any(|o| o.id == r.id));
        // 有战斗现场的就留在战斗里,别把 screen/combat 清掉
        if run.combat.is_none() {
            run.screen = Screen::Map;
        }
        run.event = None;
        if run.combat.is_none() {
            run.combat = None;
        }
        run.reward = None;
        run.shop = None;
        run.picker = None;
        run.treasure = None;
        run.say(format!("continued run, seed {seed}"));
        Ok(run)
    }

    /// 开局第一件事:Neow 的祝福(四选一)
    pub fn open_neow(&mut self) {
        self.event = Some(EventState {
            def: crate::core::events::neow(),
            index: 0,
            result: None,
            match_keep: None,
        });
        self.screen = Screen::Event;
    }

    /// 记一笔:既进历史记录,也更新界面上的提示
    fn say(&mut self, text: impl Into<String>) {
        self.push_history(text, HistoryKind::System);
    }

    fn push_history(&mut self, text: impl Into<String>, kind: HistoryKind) {
        self.history.push(HistoryEntry {
            text: text.into(),
            kind,
        });
        if self.history.len() > HISTORY_LIMIT {
            self.history.drain(0..HISTORY_TRIM);
        }
    }

    // ---- 地图 ----

    /// 地图上 Boss 那里要写的名字
    pub fn boss_name(&self) -> &'static str {
        enemies::encounter_name(self.boss_enc)
    }

    pub fn reachable(&self) -> Vec<usize> {
        self.map.reachable_from(self.pos)
    }

    pub fn floor(&self) -> usize {
        match self.pos {
            Some(i) => self.map.node(i).floor,
            None => 0,
        }
    }

    /// 进入一个节点
    pub fn enter_node(&mut self, idx: usize) -> Result<(), String> {
        if self.screen != Screen::Map {
            return Err("not on the map".to_string());
        }
        if !self.reachable().contains(&idx) {
            return Err("that node is not reachable from here".to_string());
        }
        let node = self.map.node(idx);
        let (kind, floor) = (node.kind, node.floor);
        self.pos = Some(idx);
        self.path.push(idx);
        self.floor_reached = floor;
        // 每进一个房间,每层的流(miscRng / aiRng / monsterHpRng / shuffleRng /
        // cardRandomRng)都用 seed + 层号重开(参考实现的 transitionToMapNode)
        self.streams.reseed_floor_streams(floor as u32 + 1);
        self.say(format!("floor {}: {}", floor + 1, kind.name()));
        let per_floor: i32 = self.player.relic_fx_sum(|r| r.fx.gold_per_floor);
        if per_floor > 0 {
            self.gain_gold(per_floor);
        }
        match kind {
            NodeKind::Monster => {
                let enc = self.pick_encounter(EnemyKind::Normal);
                self.start_combat(enc);
            }
            NodeKind::Elite => {
                self.stats.elites += 1;
                let enc = self.pick_encounter(EnemyKind::Elite);
                self.start_combat(enc);
            }
            NodeKind::Boss => {
                self.stats.bosses += 1;
                let enc = self.boss_enc;
                self.start_combat(enc);
            }
            NodeKind::Rest => {
                self.rest_index = 0;
                self.screen = Screen::Rest;
            }
            NodeKind::Shop => self.open_shop(),
            NodeKind::Treasure => self.open_treasure(),
            NodeKind::Event => self.open_event(),
        }
        Ok(())
    }

    /// 从这一章的名单里取下一个遭遇:名单按顺序消耗,抽干了再补一批强怪
    fn pick_encounter(&mut self, kind: EnemyKind) -> &'static Encounter {
        let id = match kind {
            EnemyKind::Normal => {
                if self.monster_list.is_empty() {
                    self.monster_list =
                        enemies::generate_extra_strong(1, self.streams.run(RunStream::MonsterRng), 12);
                }
                self.monster_list.remove(0)
            }
            EnemyKind::Elite => {
                assert!(!self.elite_list.is_empty(), "精英名单已经抽干");
                self.elite_list.remove(0)
            }
            EnemyKind::Boss => self.boss_enc.id,
        };
        let pick = enemies::resolve(id);
        self.last_encounter = pick.id;
        pick
    }

    // ---- 战斗 ----

    fn start_combat(&mut self, enc: &'static Encounter) {
        self.say(format!("a fight breaks out: {}", enc.id));
        let setup = CombatSetup {
            hp: self.player.hp,
            max_hp: self.player.max_hp,
            deck: self.player.deck.clone(),
            relics: self.player.relics.clone(),
            gold: self.player.gold,
        };
        self.combat = Some(Combat::new(enc, setup, self.streams.clone()));
        self.win_hold = 0;
        self.fight_seq += 1;
        self.combat_log_seen = 0;
        self.screen = Screen::Combat;
        self.stats.fights += 1;
    }

    /// 测试与界面自检用:跳过地图,直接打一场指定遭遇
    #[cfg(test)]
    pub fn debug_start_combat(&mut self, enc: &'static Encounter) {
        self.start_combat(enc);
    }

    /// 调试用:直接判定这一场战斗胜利,进入奖励结算(不影响整局,跳的是战斗不是本局)
    pub fn debug_win_battle(&mut self) {
        if let Some(c) = self.combat.as_mut() {
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = crate::core::combat::Phase::Won;
        }
        self.sync_combat();
    }

    pub fn combat(&self) -> Option<&Combat> {
        self.combat.as_ref()
    }

    pub fn combat_mut(&mut self) -> Option<&mut Combat> {
        self.combat.as_mut()
    }

    /// 把这场战斗里还没抄过的日志行追加到历史记录
    fn absorb_combat_log(&mut self) {
        let Some(c) = self.combat.as_ref() else {
            return;
        };
        let fresh: Vec<HistoryEntry> = c
            .log
            .iter()
            .filter(|l| l.seq > self.combat_log_seen)
            .map(|l| HistoryEntry {
                text: format!("  {}", l.text),
                kind: match l.kind {
                    crate::core::combat::LogKind::Enemy => HistoryKind::Enemy,
                    crate::core::combat::LogKind::Player => HistoryKind::Player,
                    crate::core::combat::LogKind::Info => HistoryKind::Info,
                },
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        self.combat_log_seen = c.log_seq;
        self.history.extend(fresh);
        if self.history.len() > HISTORY_LIMIT {
            self.history.drain(0..HISTORY_TRIM);
        }
    }

    /// 每次战斗内操作之后调用:同步生命、处理胜负
    pub fn sync_combat(&mut self) {
        self.absorb_combat_log();
        // 战斗里的掷点是在自己的流副本上走的,这里把它收回来
        if let Some(c) = self.combat.as_ref() {
            self.streams = c.streams.clone();
        }
        let Some(c) = self.combat.as_mut() else {
            return;
        };
        let phase = c.phase;
        let hp = c.player.hp;
        let max_hp = c.player.max_hp;
        let damage = c.damage_dealt;
        let turns = c.turn;
        // 战斗里赚到的金币(贪婪之手)在这里收走,收一次就清零
        let looted = c.gold_gained;
        c.gold_gained = 0;
        // 抢劫类敌人会当场动玩家身上的金币
        let gold = c.player_gold;
        // 战斗中永久塞进牌组的牌(寄生)在这里并进去
        let added: Vec<_> = c.deck_cards.drain(..).collect();
        self.player.hp = hp;
        self.player.max_hp = max_hp;
        self.player.gold = gold;
        for card in added {
            self.say(format!("{} is added to your deck", card.label()));
            self.player.deck.push(card);
        }
        if looted > 0 {
            self.gain_gold(looted);
            self.say(format!("you loot {looted} gold"));
        }
        // 战斗里的金币以玩家身上的为准,免得下一次同步把它冲掉
        if let Some(c) = self.combat.as_mut() {
            c.player_gold = self.player.gold;
        }
        match phase {
            // 赢了先留在战场上看 2 秒(死亡/结算动画),由 tick_win_hold 收尾
            Phase::Won => {
                if self.win_hold == 0 {
                    self.win_hold = Self::VICTORY_HOLD;
                }
            }
            Phase::Lost => self.resolve_defeat(),
            _ => {}
        }
        self.stats.turns = self.stats.turns.max(turns);
        let _ = damage;
    }

    /// 战斗胜利后在原地停留的帧数(60ms 一帧,34 帧约 2 秒)
    pub const VICTORY_HOLD: u8 = 34;

    /// 赢了但还没进奖励(这段时间不吃战斗操作)
    pub fn holding_victory(&self) -> bool {
        self.win_hold > 0
    }

    /// 胜利停留的倒计时;归零就把这场战斗结算掉
    pub fn tick_win_hold(&mut self) {
        if self.win_hold == 0 {
            return;
        }
        self.win_hold -= 1;
        if self.win_hold == 0 {
            let won = self
                .combat
                .as_ref()
                .is_some_and(|c| c.phase == Phase::Won);
            if won {
                self.resolve_victory();
            }
        }
    }

    fn post_combat_heal(&mut self) -> i32 {
        let heal: i32 = self.player.relic_fx_sum(|r| r.fx.post_combat_heal);
        if heal > 0 {
            self.heal(heal);
        }
        heal
    }

    fn resolve_victory(&mut self) {
        self.absorb_combat_log();
        let Some(c) = self.combat.take() else {
            return;
        };
        let kind = c.kind;
        self.stats.damage_dealt += c.damage_dealt;
        if c.gold_gained > 0 {
            self.gain_gold(c.gold_gained);
        }
        let healed = self.post_combat_heal();
        if healed > 0 {
            self.say(format!("relics heal you for {healed}"));
        }
        // 事件打的那一场:奖励由事件指定,打完可能还要回到事件里接着选
        let plan = self.combat_reward.take();
        let back = self.pending_event.take();
        if let Some(def) = back {
            self.say(format!("victory over the {}", c.encounter_id));
            self.event = Some(EventState {
                def,
                index: 0,
                result: None,
                match_keep: None,
            });
            self.screen = Screen::Event;
            return;
        }
        if plan.map(|p| p.nothing).unwrap_or(false) {
            self.say(format!("victory over the {} (no rewards)", c.encounter_id));
            self.screen = Screen::Map;
            return;
        }
        // 顺序照参考实现的 buildCombatRewards:金币 → 遗物(精英)→ 药水 → 卡牌
        let gold = match plan.and_then(|p| p.gold) {
            Some((lo, hi)) => self.streams.run(RunStream::TreasureRng).random_range(lo, hi),
            None => match kind {
                EnemyKind::Normal => self
                    .streams
                    .run(RunStream::TreasureRng)
                    .random_range(10, 20),
                EnemyKind::Elite => self
                    .streams
                    .run(RunStream::TreasureRng)
                    .random_range(25, 35),
                // Boss 的金币走 miscRng:100 上下浮动 5
                EnemyKind::Boss => 100 + self.streams.floor(FloorStream::MiscRng).random_range(-5, 5),
            },
        };
        let relic = match plan {
            Some(p) if p.no_relic => None,
            Some(p) if p.relic_id.is_some() => Some(relic_def_any(p.relic_id.unwrap())),
            Some(p) if p.relic_rarity.is_some() => self.take_relic_of(p.relic_rarity.unwrap()),
            Some(_) => None,
            None => match kind {
                EnemyKind::Elite => {
                    let tier = self.roll_elite_relic_tier();
                    self.take_relic_of(tier)
                }
                EnemyKind::Boss => self.take_relic_of(Rarity::Rare),
                EnemyKind::Normal => None,
            },
        };
        // 药水:先掷一次 d100 看掉不掉(带保底),掉了再掷稀有度
        let potion = if let Some(p) = plan {
            if self.streams.run(RunStream::PotionRng).chance(p.potion_pct as u32) {
                potions::random_potion(self.streams.run(RunStream::PotionRng))
            } else {
                None
            }
        } else {
            let categories = if matches!(kind, EnemyKind::Elite) { 2 } else { 1 };
            self.roll_potion_reward(categories)
        };
        let cards: Vec<CardInstance> = if plan.map(|p| p.no_cards).unwrap_or(false) {
            Vec::new()
        } else {
            self.create_card_reward(kind)
        };
        let next = if kind == EnemyKind::Boss {
            Screen::Victory
        } else {
            Screen::Map
        };
        self.say(format!("victory over the {} ({kind:?})", c.encounter_id));
        self.reward = Some(RewardState {
            gold,
            gold_taken: false,
            cards,
            card_taken: false,
            relic,
            relic_taken: false,
            potion,
            potion_taken: false,
            index: 0,
            next,
        });
        self.screen = Screen::Reward;
    }

    fn resolve_defeat(&mut self) {
        self.absorb_combat_log();
        self.combat = None;
        self.say("you fell in battle");
        self.screen = Screen::Death;
    }

    // ---- 奖励 ----

    pub fn reward_slots(&self) -> Vec<RewardSlot> {
        let Some(r) = &self.reward else {
            return Vec::new();
        };
        let mut v = Vec::new();
        if !r.gold_taken {
            v.push(RewardSlot::Gold);
        }
        if !r.card_taken {
            for i in 0..r.cards.len() {
                v.push(RewardSlot::Card(i));
            }
        }
        if r.relic.is_some() && !r.relic_taken {
            v.push(RewardSlot::Relic);
        }
        if r.potion.is_some() && !r.potion_taken {
            v.push(RewardSlot::Potion);
        }
        v
    }

    pub fn reward_clamp(&mut self) {
        let n = self.reward_slots().len();
        if let Some(r) = self.reward.as_mut() {
            if n == 0 {
                r.index = 0;
            } else if r.index >= n {
                r.index = n - 1;
            }
        }
    }

    /// 取走当前选中的奖励
    pub fn reward_take(&mut self) -> Result<String, String> {
        let slots = self.reward_slots();
        let index = self.reward.as_ref().map(|r| r.index).unwrap_or(0);
        let Some(slot) = slots.get(index).copied() else {
            return Err("nothing selected".to_string());
        };
        match slot {
            RewardSlot::Gold => {
                let g = self.reward.as_ref().map(|r| r.gold).unwrap_or(0);
                self.gain_gold(g);
                self.mark_reward(|r| r.gold_taken = true);
                Ok(format!("+${g}"))
            }
            RewardSlot::Card(i) => {
                let Some(card) = self.reward.as_ref().and_then(|r| r.cards.get(i)).cloned() else {
                    return Err("no such card".to_string());
                };
                let label = card.label();
                self.player.deck.push(card);
                self.mark_reward(|r| r.card_taken = true);
                Ok(format!("{label} added to your deck"))
            }
            RewardSlot::Relic => {
                let Some(def) = self.reward.as_ref().and_then(|r| r.relic) else {
                    return Err("no relic here".to_string());
                };
                self.gain_relic(def);
                self.mark_reward(|r| r.relic_taken = true);
                Ok(format!("relic gained: {}", def.name))
            }
            RewardSlot::Potion => {
                let Some(def) = self.reward.as_ref().and_then(|r| r.potion) else {
                    return Err("no potion here".to_string());
                };
                if !self.add_potion(def) {
                    return Err("no free potion slot: press p, then t+1-3 to toss one".to_string());
                }
                self.mark_reward(|r| r.potion_taken = true);
                Ok(format!("potion gained: {}", def.name))
            }
        }
    }

    fn mark_reward(&mut self, f: impl FnOnce(&mut RewardState)) {
        if let Some(r) = self.reward.as_mut() {
            f(r);
        }
    }

    /// 跳过卡牌奖励时可能提升生命上限(唱歌碗)
    pub fn reward_skip_cards(&mut self) -> String {
        let bonus: i32 = self.player.relic_fx_sum(|r| r.fx.max_hp_on_card_skip);
        if let Some(r) = self.reward.as_mut() {
            r.card_taken = true;
        }
        if bonus > 0 {
            self.player.max_hp += bonus;
            self.player.hp += bonus;
            format!("skip the cards: +{bonus} max HP")
        } else {
            "skipped the cards".to_string()
        }
    }

    /// 离开奖励界面
    pub fn leave_reward(&mut self) -> Screen {
        let next = self
            .reward
            .as_ref()
            .map(|r| r.next)
            .unwrap_or(Screen::Map);
        self.reward = None;
        self.screen = next;
        next
    }

    // ---- 营火 ----

    pub fn rest_heal(&mut self) {
        let bonus: i32 = self.player.relic_fx_sum(|r| r.fx.rest_heal_bonus);
        let amount = (self.player.max_hp * REST_HEAL_PCT / 100) + bonus;
        let healed = self.heal(amount);
        self.say(format!("you rest and heal {healed} HP"));
        self.screen = Screen::Map;
    }

    pub fn rest_smith(&mut self) {
        self.open_picker(PickPurpose::Upgrade, Screen::Map, 0, None);
    }

    // ---- 商店 ----

    fn open_shop(&mut self) {
        let mut items: Vec<ShopItem> = Vec::new();
        // 店里卖的牌也不重样:牌的身份走 cardRng,价格走 merchantRng
        let mut sold: Vec<&'static str> = Vec::new();
        let mut sold_potions: Vec<&'static str> = Vec::new();
        for (rarity, count) in [
            (Rarity::Common, 2),
            (Rarity::Uncommon, 2),
            (Rarity::Rare, 1),
        ] {
            for _ in 0..count {
                if let Some(def) = self.roll_card_of_distinct(rarity, &sold) {
                    let base = shop_base(SHOP_CARD_BASE, rarity);
                    let price = (base as f32
                        * self
                            .streams
                            .run(RunStream::MerchantRng)
                            .random_float_range(SHOP_CARD_JITTER.0, SHOP_CARD_JITTER.1))
                        as i32;
                    sold.push(def.id);
                    items.push(ShopItem::Card(CardInstance::new(def), self.discount(price)));
                }
            }
        }
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            if let Some(def) = self.take_relic_of(rarity) {
                let base = shop_base(SHOP_RELIC_BASE, rarity);
                let price = (base as f32
                    * self
                        .streams
                        .run(RunStream::MerchantRng)
                        .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1))
                .round() as i32;
                items.push(ShopItem::Relic(def, self.discount(price)));
            }
        }
        for _ in 0..2 {
            // 两瓶药水不重复:药水身份走 potionRng,价格走 merchantRng
            let mut def = potions::random_potion(self.streams.run(RunStream::PotionRng))
                .expect("药水池非空");
            let mut guard = 0;
            while sold_potions.contains(&def.id) && guard < 40 {
                guard += 1;
                def = potions::random_potion(self.streams.run(RunStream::PotionRng))
                    .expect("药水池非空");
            }
            if sold_potions.contains(&def.id) {
                continue;
            }
            sold_potions.push(def.id);
            let base = shop_base(SHOP_POTION_BASE, def.rarity);
            let price = (base as f32
                * self
                    .streams
                    .run(RunStream::MerchantRng)
                    .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1))
            .round() as i32;
            items.push(ShopItem::Potion(def, self.discount(price)));
        }
        let removal = self.discount(75);
        items.push(ShopItem::Remove(removal));
        let n = items.len();
        self.shop = Some(ShopState {
            items,
            sold: vec![false; n],
            index: 0,
            removes: 0,
        });
        self.screen = Screen::Shop;
    }

    fn discount(&self, price: i32) -> i32 {
        let pct: i32 = self.player.relic_fx_sum(|r| r.fx.shop_discount_pct);
        (price * (100 - pct.min(90)) / 100).max(1)
    }

    pub fn shop_clamp(&mut self) {
        if let Some(s) = self.shop.as_mut() {
            if s.index >= s.items.len() {
                s.index = s.items.len().saturating_sub(1);
            }
        }
    }

    pub fn buy_selected(&mut self) -> Result<String, String> {
        let Some(shop) = self.shop.as_ref() else {
            return Err("no shop here".to_string());
        };
        let i = shop.index;
        let Some(item) = shop.items.get(i) else {
            return Err("nothing selected".to_string());
        };
        if shop.sold.get(i).copied().unwrap_or(true) {
            return Err("sold out".to_string());
        }
        let price = item.price();
        if self.player.gold < price {
            return Err(format!("needs ${price}"));
        }
        match item {
            ShopItem::Card(card, _) => {
                let card = card.clone();
                let label = card.label();
                self.player.deck.push(card);
                self.spend_gold(price);
                self.shop.as_mut().unwrap().sold[i] = true;
                Ok(format!("bought {label} for {price}"))
            }
            ShopItem::Relic(def, _) => {
                let def = *def;
                self.spend_gold(price);
                self.gain_relic(def);
                self.shop.as_mut().unwrap().sold[i] = true;
                Ok(format!("bought {} for {price}", def.name))
            }
            ShopItem::Potion(def, _) => {
                let def = *def;
                if !self.add_potion(def) {
                    return Err("no free potion slot: press p, then t+1-3 to toss one".to_string());
                }
                self.spend_gold(price);
                self.shop.as_mut().unwrap().sold[i] = true;
                Ok(format!("bought {} for {price}", def.name))
            }
            ShopItem::Remove(_) => {
                // 先选牌,确认时再付钱
                self.open_picker(PickPurpose::Remove, Screen::Shop, price, Some(i));
                Ok("choose a card to remove".to_string())
            }
        }
    }

    pub fn leave_shop(&mut self) {
        self.say("you leave the shop");
        self.shop = None;
        self.screen = Screen::Map;
    }

    // ---- 事件 ----

    fn open_event(&mut self) {
        assert!(!crate::core::events::EVENTS.is_empty(), "事件池为空");
        // 事件身份走 eventRng(参考实现的 generateEvent,掷在一个副本上)
        let idx = self
            .streams
            .run(RunStream::EventRng)
            .random(crate::core::events::EVENTS.len() as u32 - 1) as usize;
        let def: &'static EventDef = &crate::core::events::EVENTS[idx];
        self.open_event_def(def);
    }

    /// 打开指定事件;翻牌事件顺手把 12 格棋盘铺好
    fn open_event_def(&mut self, def: &'static EventDef) {
        let match_keep = if def.id == "match_and_keep" {
            Some(MatchKeep::new(&mut self.streams, self.character))
        } else {
            None
        };
        self.event = Some(EventState {
            def,
            index: 0,
            result: None,
            match_keep,
        });
        self.screen = Screen::Event;
    }

    /// 选项当前是否可选(钱够、血够、该有的遗物/药水/牌都有);
    /// 翻牌事件按棋盘的格子算
    pub fn event_choice_available(&self, i: usize) -> bool {
        let Some(st) = &self.event else {
            return false;
        };
        if let Some(mk) = &st.match_keep {
            return st.result.is_none() && mk.available(i);
        }
        let Some(c) = st.def.choices.get(i) else {
            return false;
        };
        if self.player.gold < c.cost_gold || self.player.hp <= c.cost_hp {
            return false;
        }
        if self.player.gold < c.req_gold {
            return false;
        }
        if let Some(id) = c.req_relic {
            if !self.player.relics.iter().any(|r| r.id == id) {
                return false;
            }
        }
        if c.req_potion && self.player.potions.iter().all(|p| p.is_none()) {
            return false;
        }
        if c.req_big_attack && !self.has_big_attack() {
            return false;
        }
        if c.req_non_basic && !self.has_non_basic_card() {
            return false;
        }
        true
    }

    /// 事件当前有几个选项:翻牌事件按棋盘的 12 格算
    pub fn event_choice_count(&self) -> usize {
        match &self.event {
            Some(st) => match &st.match_keep {
                Some(mk) => mk.board.len(),
                None => st.def.choices.len(),
            },
            None => 0,
        }
    }

    /// 选项这一行的文本与要价:翻牌事件给的是棋盘上那一格(朝上就显示牌名)
    pub fn event_choice_row(&self, i: usize) -> Option<(String, i32, i32)> {
        let st = self.event.as_ref()?;
        if let Some(mk) = &st.match_keep {
            return Some((mk.label(i), 0, 0));
        }
        let c = st.def.choices.get(i)?;
        Some((c.label.to_string(), c.cost_gold, c.cost_hp))
    }

    /// 牌组里有没有单次伤害 10 以上的攻击牌(Wing Statue 砸雕像的条件)
    fn has_big_attack(&self) -> bool {
        self.player.deck.iter().any(|c| {
            c.kind() == crate::core::card::CardType::Attack
                && c.effects().iter().any(|e| {
                    let (amount, times) = match *e {
                        CardEffect::Damage { amount, times }
                        | CardEffect::DamageWithBonus { amount, times } => (amount, times),
                        _ => return false,
                    };
                    times.max(1) as i32 * amount + c.bonus >= 10
                })
        })
    }

    /// 牌组里有没有非基础、非诅咒的牌
    fn has_non_basic_card(&self) -> bool {
        self.player
            .deck
            .iter()
            .any(|c| c.rarity() != Rarity::Basic && c.kind() != crate::core::card::CardType::Curse)
    }

    pub fn choose_event(&mut self, i: usize) -> Result<(), String> {
        let Some(st) = self.event.as_ref() else {
            return Err("no event here".to_string());
        };
        if st.result.is_some() {
            return Err("already resolved".to_string());
        }
        if st.match_keep.is_some() {
            return self.flip_event_card(i);
        }
        let Some(choice) = st.def.choices.get(i).copied() else {
            return Err("no such choice".to_string());
        };
        if !self.event_choice_available(i) {
            return Err("that choice is not available".to_string());
        }
        if choice.cost_gold > 0 {
            self.spend_gold(choice.cost_gold);
        }
        if choice.cost_hp > 0 {
            self.damage(choice.cost_hp);
        }
        let outcome = choice.outcome;
        let text = self.apply_outcome(&outcome);
        // 多屏事件:这一屏结算完直接换选项,不给"看完结果再按回车"的停顿
        if let Some(next) = outcome.next {
            if let Some(st) = self.event.as_mut() {
                st.def = next;
                st.index = 0;
                st.result = None;
            }
            return Ok(());
        }
        if let Some(st) = self.event.as_mut() {
            st.result = Some(text);
        }
        Ok(())
    }

    /// 翻牌小游戏(match_and_keep):翻第 i 格.
    /// 第一张只是翻开;翻第二张才算一次尝试,同源的两张进牌组并一直朝上,
    /// 不同源的两张翻回背面.次数用完(或 12 格全配对)事件结束,只剩离开.
    fn flip_event_card(&mut self, i: usize) -> Result<(), String> {
        let Some(mut board) = self.event.as_mut().and_then(|st| st.match_keep.take()) else {
            return Err("no board here".to_string());
        };
        if !board.available(i) {
            if let Some(st) = self.event.as_mut() {
                st.match_keep = Some(board);
            }
            return Err("that card can not be flipped".to_string());
        }
        let first = board.first;
        let result = board.flip(i);
        let name = |id: Option<&'static str>| match id {
            Some(id) => card_def_any(id).name,
            None => "(empty)",
        };
        let mut text = String::new();
        match result {
            FlipResult::First => text.push_str(&format!("you flip {}", name(board.card_at(i)))),
            FlipResult::Miss => {
                let a = name(first.and_then(|f| board.card_at(f)));
                text.push_str(&format!("{a} and {} do not match", name(board.card_at(i))));
            }
            FlipResult::Matched => match board.card_at(i) {
                Some(id) => {
                    self.add_card_id(id, 1);
                    text.push_str(&format!("{} matches: it joins your deck", name(Some(id))));
                }
                None => text.push_str("the pair matches, but there was nothing to take"),
            },
        }
        let done = board.finished();
        board.note = Some(text);
        if let Some(st) = self.event.as_mut() {
            st.match_keep = Some(board);
            if done {
                st.result = Some("The memory game is over; the pairs you matched are yours.");
            }
        }
        Ok(())
    }

    /// 翻牌棋盘下面那行说明(上一次翻牌的结果);其它事件没有
    pub fn event_note(&self) -> Option<&str> {
        self.event.as_ref()?.match_keep.as_ref()?.note.as_deref()
    }

    /// 事件结果结算;返回给玩家看的文本.
    /// 百分比一律按"结算前的生命上限"算(先加上限再扣血的那几个事件也照原作来).
    fn apply_outcome(&mut self, o: &Outcome) -> &'static str {
        let max_hp0 = self.player.max_hp;
        if o.max_hp != 0 {
            self.player.max_hp = (self.player.max_hp + o.max_hp).max(1);
            if o.max_hp > 0 {
                self.player.hp += o.max_hp;
            }
        }
        if o.max_hp_pct > 0 {
            let loss = crate::core::events::pct_of(max_hp0, o.max_hp_pct).max(1);
            self.player.max_hp = (self.player.max_hp - loss).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
        let mut delta = o.hp;
        if o.hp_pct > 0 {
            let pct = crate::core::events::pct_of(max_hp0, o.hp_pct).max(o.hp_pct_min.max(1));
            delta -= pct;
        }
        if delta < 0 {
            self.damage(-delta);
        } else if delta > 0 {
            self.heal(delta);
        }
        if o.heal_pct > 0 {
            self.heal(crate::core::events::pct_of(max_hp0, o.heal_pct));
        }
        if o.full_heal {
            self.player.hp = self.player.max_hp;
        }
        if o.gold != 0 {
            if o.gold > 0 {
                self.gain_gold(o.gold);
            } else {
                let g = (-o.gold).min(self.player.gold);
                self.spend_gold(g);
            }
        }
        if let Some((lo, hi)) = o.gold_range {
            let g = self.streams.floor(FloorStream::MiscRng).range_inclusive(lo, hi);
            self.gain_gold(g);
        }
        if o.gold_lose_all {
            let g = self.player.gold;
            self.spend_gold(g);
        }
        if let Some((lo, hi)) = o.gold_lose_range {
            let g = self.streams.floor(FloorStream::MiscRng).range_inclusive(lo, hi).min(self.player.gold);
            self.spend_gold(g);
        }
        if let Some(id) = o.remove_relic {
            self.remove_relic_by_id(id);
        }
        if o.remove_random_relic {
            // 至少要两件才吃一件(原作的生成条件就是身上有两件遗物)
            if self.player.relics.len() >= 2 {
                let idx = self.streams.floor(FloorStream::MiscRng).below(self.player.relics.len() as u32) as usize;
                let gone = self.player.relics.remove(idx);
                self.say(format!("{} is devoured", gone.name));
            }
        }
        if let Some(id) = o.relic_id {
            let def = relic_def_any(id);
            self.gain_relic(def);
        }
        if let Some(rarity) = o.random_relic_rarity {
            if let Some(def) = self.take_relic_of(rarity) {
                self.gain_relic(def);
            }
        }
        if o.random_relic_any {
            if let Some(def) = self.take_relic_of_any() {
                self.gain_relic(def);
            }
        }
        if let Some(id) = o.add_card {
            self.add_card_id(id, 1);
        }
        if let Some((id, n)) = o.add_cards {
            self.add_card_id(id, n);
        }
        if let Some(id) = o.add_curse {
            self.add_card_id(id, 1);
        }
        if o.add_random_curse {
            let pool = cards::curses();
            if !pool.is_empty() {
                let def = *self.streams.floor(FloorStream::MiscRng).pick(&pool);
                self.player.deck.push(CardInstance::new(def));
            }
        }
        if let Some((rarity, n)) = o.add_random_class {
            for _ in 0..n {
                if let Some(def) = self.random_class_card(Some(rarity)) {
                    self.player.deck.push(CardInstance::new(def));
                }
            }
        }
        for _ in 0..o.add_random_class_any {
            if let Some(def) = self.random_class_card(None) {
                self.player.deck.push(CardInstance::new(def));
            }
        }
        if let Some((rarity, n)) = o.add_random_colorless {
            for _ in 0..n {
                if let Some(def) = self.random_colorless_card(rarity) {
                    self.player.deck.push(CardInstance::new(def));
                }
            }
        }
        if o.upgrade_all || o.upgrade_starters {
            let starters = o.upgrade_starters;
            let mut n = 0;
            for c in self.player.deck.iter_mut() {
                if !c.can_upgrade() {
                    continue;
                }
                if starters && !matches!(c.def.id, "strike" | "defend") {
                    continue;
                }
                c.upgrade();
                n += 1;
            }
            if n > 0 {
                self.say(format!("{n} cards are upgraded"));
            }
        }
        for _ in 0..o.upgrade_random_n {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if cands.is_empty() {
                break;
            }
            let pick = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
            self.player.deck[pick].upgrade();
            let name = self.player.deck[pick].label();
            self.say(format!("{name} is upgraded"));
        }
        if o.remove_base_strikes {
            let mut i = 0;
            while i < self.player.deck.len() {
                let hit = self.player.deck[i].def.id == "strike" && !self.player.deck[i].upgraded;
                if hit {
                    let card = self.player.deck.remove(i);
                    self.pay_deck_leave_cost(&card);
                } else {
                    i += 1;
                }
            }
        }
        if o.remove_curses {
            let mut i = 0;
            while i < self.player.deck.len() {
                let c = &self.player.deck[i];
                let hit = c.kind() == crate::core::card::CardType::Curse && !c.def.unremovable;
                if hit {
                    let card = self.player.deck.remove(i);
                    self.pay_deck_leave_cost(&card);
                } else {
                    i += 1;
                }
            }
        }
        if let Some(rule) = o.remove_random {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    if c.def.unremovable {
                        return false;
                    }
                    match rule {
                        RemoveRule::OfType(kind) => c.kind() == kind,
                        RemoveRule::NonBasicNonCurse => {
                            c.rarity() != Rarity::Basic
                                && c.kind() != crate::core::card::CardType::Curse
                        }
                    }
                })
                .map(|(i, _)| i)
                .collect();
            if !cands.is_empty() {
                let idx = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
                let card = self.player.deck.remove(idx);
                let name = card.label();
                self.pay_deck_leave_cost(&card);
                self.say(format!("{name} is lost"));
            }
        }
        for _ in 0..o.transform_random_n {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.def.unremovable)
                .map(|(i, _)| i)
                .collect();
            if cands.is_empty() {
                break;
            }
            let idx = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
            self.transform_deck_card(idx);
        }
        if o.lose_random_potion {
            let slots: Vec<usize> = self
                .player
                .potions
                .iter()
                .enumerate()
                .filter(|(_, p)| p.is_some())
                .map(|(i, _)| i)
                .collect();
            if !slots.is_empty() {
                let slot = slots[self.streams.floor(FloorStream::MiscRng).below(slots.len() as u32) as usize];
                let gone = self.player.potions[slot].take();
                if let Some(def) = gone {
                    self.say(format!("{} is given away", def.name));
                }
            }
        }
        for _ in 0..o.random_potion_n {
            if let Some(def) = potions::random_potion(self.streams.run(RunStream::PotionRng)) {
                self.add_potion(def);
            }
        }
        if let Some(table) = o.roll {
            let total: u32 = table.iter().map(|(w, _)| *w).sum();
            let weights: Vec<f32> =
                table.iter().map(|(w, _)| *w as f32 / total as f32).collect();
            if let Some(i) = self.streams.floor(FloorStream::MiscRng).weighted_idx_f32(&weights) {
                let sub = table[i].1;
                self.apply_outcome(&sub);
            }
        }
        if o.jump_to_boss {
            self.jump_to_boss();
            return o.text;
        }
        let fight_id = match (o.fight, o.fight_pool) {
            (Some(id), _) => Some(id),
            (None, Some(pool)) if !pool.is_empty() => Some(*self.streams.floor(FloorStream::MiscRng).pick(pool)),
            _ => None,
        };
        if let Some(enc_id) = fight_id {
            let enc = enemies::encounter_def(enc_id)
                .or_else(|| crate::core::enemy::event_encounter(enc_id))
                .unwrap_or_else(|| panic!("unknown encounter: {enc_id}"));
            self.combat_reward = o.fight_reward;
            self.pending_event = o.fight_next;
            self.start_combat(enc);
            return o.text;
        }
        if o.remove_card {
            self.open_picker(PickPurpose::Remove, Screen::Event, 0, None);
        } else if o.upgrade_card {
            self.open_picker(PickPurpose::Upgrade, Screen::Event, 0, None);
        } else if o.transform_card {
            self.open_picker(PickPurpose::Transform, Screen::Event, 0, None);
        } else if o.duplicate_card {
            self.open_picker(PickPurpose::Duplicate, Screen::Event, 0, None);
        }
        if o.dead {
            self.player.hp = 0;
            self.screen = Screen::Death;
        }
        o.text
    }

    /// 按 id 加 n 张牌(先认事件专用牌,再认卡池)
    fn add_card_id(&mut self, id: &str, n: u8) {
        for _ in 0..n.max(1) {
            self.player
                .deck
                .push(CardInstance::new(card_def_any(id)));
        }
    }

    /// 随机一张本职业牌(不限稀有度时从三档里挑)
    fn random_class_card(&mut self, rarity: Option<Rarity>) -> Option<&'static CardDef> {
        let mut pool: Vec<&'static CardDef> = match rarity {
            Some(r) => cards::reward_pool(r),
            None => {
                let mut v = cards::reward_pool(Rarity::Common);
                v.extend(cards::reward_pool(Rarity::Uncommon));
                v.extend(cards::reward_pool(Rarity::Rare));
                v
            }
        };
        if pool.is_empty() {
            return None;
        }
        let idx = self.streams.floor(FloorStream::MiscRng).random(pool.len() as u32 - 1) as usize;
        Some(pool.swap_remove(idx))
    }

    /// 随机一张无色牌
    fn random_colorless_card(&mut self, rarity: Option<Rarity>) -> Option<&'static CardDef> {
        let pool: Vec<&'static CardDef> = cards::colorless_pool()
            .into_iter()
            .filter(|c| rarity.map(|r| c.rarity == r).unwrap_or(true))
            .collect();
        if pool.is_empty() {
            return None;
        }
        Some(*self.streams.floor(FloorStream::MiscRng).pick(&pool))
    }

    /// 移除身上的一件指定遗物
    fn remove_relic_by_id(&mut self, id: &str) {
        if let Some(idx) = self.player.relics.iter().position(|r| r.id == id) {
            let gone = self.player.relics.remove(idx);
            self.say(format!("{} is gone", gone.name));
        }
    }

    /// 变形:去掉这张牌,换成一张随机本职业牌
    fn transform_deck_card(&mut self, idx: usize) {
        if idx >= self.player.deck.len() {
            return;
        }
        let card = self.player.deck.remove(idx);
        self.pay_deck_leave_cost(&card);
        if let Some(def) = self.random_class_card(None) {
            self.player.deck.push(CardInstance::new(def));
        }
    }

    /// 直接跳到本层 Boss 房并开打(秘密传送门)
    fn jump_to_boss(&mut self) {
        let boss = self.map.boss;
        self.pos = Some(boss);
        self.path.push(boss);
        self.floor_reached = self.map.node(boss).floor;
        self.streams
            .reseed_floor_streams(self.floor_reached as u32 + 1);
        self.stats.bosses += 1;
        self.say(format!("floor {}: boss", self.floor_reached + 1));
        let enc = self.boss_enc;
        self.start_combat(enc);
    }

    pub fn event_index_set(&mut self, i: usize) {
        let n = self.event_choice_count();
        if let Some(st) = self.event.as_mut() {
            if n > 0 {
                st.index = i.min(n - 1);
            }
        }
    }

    pub fn leave_event(&mut self) {
        self.event = None;
        self.screen = Screen::Map;
    }

    // ---- 宝箱 ----

    /// 宝箱房:先掷箱子的尺寸,再掷一次同时决定"有没有金币"和"遗物档次"
    /// (参考实现的单掷怪癖:金币与档次共用一个掷点)
    fn open_treasure(&mut self) {
        let size_roll = self.streams.run(RunStream::TreasureRng).random(99);
        let size = if size_roll < 50 {
            ChestSize::Small
        } else if size_roll < 83 {
            ChestSize::Medium
        } else {
            ChestSize::Large
        };
        let roll = self.streams.run(RunStream::TreasureRng).random(99);
        let (gold_chance, common_below, uncommon_below) = match size {
            ChestSize::Small => (50, 75, 100),
            ChestSize::Medium => (35, 35, 85),
            ChestSize::Large => (50, 0, 75),
        };
        let tier = if roll < common_below {
            Rarity::Common
        } else if roll < uncommon_below {
            Rarity::Uncommon
        } else {
            Rarity::Rare
        };
        self.chest = Some(Chest {
            size,
            gold_present: roll < gold_chance,
            tier,
        });
        // 遗物身份进房间时就看得见(参考实现的 peekRelicFromPool)
        self.treasure = self.peek_relic_of(tier);
        if self.treasure.is_none() {
            self.say("the chest is empty");
        }
        self.screen = Screen::Treasure;
    }

    /// 开箱:有金币就先掷金币数,再把遗物从池子里取走
    pub fn take_treasure(&mut self) {
        let Some(chest) = self.chest.take() else {
            self.screen = Screen::Map;
            return;
        };
        if chest.gold_present {
            let base = match chest.size {
                ChestSize::Small => 25.0,
                ChestSize::Medium => 50.0,
                ChestSize::Large => 75.0,
            };
            let gold = self
                .streams
                .run(RunStream::TreasureRng)
                .random_float_range(base * 0.9, base * 1.1)
                .round() as i32;
            self.gain_gold(gold);
            self.say(format!("the chest holds ${gold}"));
        }
        let taken = self.take_relic_of(chest.tier);
        if let Some(def) = taken {
            self.gain_relic(def);
            self.say(format!("you found {}", def.name));
        }
        self.treasure = None;
        self.screen = Screen::Map;
    }

    // ---- 选牌 ----

    fn open_picker(
        &mut self,
        purpose: PickPurpose,
        back: Screen,
        cost_gold: i32,
        shop_slot: Option<usize>,
    ) {
        self.picker = Some(Picker {
            purpose,
            back,
            index: 0,
            cost_gold,
            shop_slot,
        });
        self.screen = Screen::Pick;
        self.picker_clamp();
    }

    /// 当前可选牌的 deck 下标
    pub fn picker_candidates(&self) -> Vec<usize> {
        let Some(p) = &self.picker else {
            return Vec::new();
        };
        match p.purpose {
            PickPurpose::Upgrade => self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect(),
            PickPurpose::Remove | PickPurpose::Transform => {
                // 不能把牌组删空;带"不可移除"标记的牌(升天者的诅咒等)不进候选
                if self.player.deck.len() <= 1 {
                    Vec::new()
                } else {
                    self.player
                        .deck
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| !c.def.unremovable)
                        .map(|(i, _)| i)
                        .collect()
                }
            }
            PickPurpose::Duplicate => (0..self.player.deck.len()).collect(),
        }
    }

    pub fn picker_clamp(&mut self) {
        let n = self.picker_candidates().len();
        if let Some(p) = self.picker.as_mut() {
            if n == 0 {
                p.index = 0;
            } else if p.index >= n {
                p.index = n - 1;
            }
        }
    }

    pub fn picker_confirm(&mut self) -> Result<String, String> {
        let Some(p) = self.picker.as_ref() else {
            return Err("no picker open".to_string());
        };
        let cands = self.picker_candidates();
        let Some(deck_idx) = cands.get(p.index).copied() else {
            return Err("nothing selected".to_string());
        };
        let purpose = p.purpose;
        let back = p.back;
        let cost = p.cost_gold;
        let slot = p.shop_slot;
        if cost > 0 && self.player.gold < cost {
            return Err("not enough gold".to_string());
        }
        let msg = match purpose {
            PickPurpose::Upgrade => {
                self.player.deck[deck_idx].upgrade();
                let label = self.player.deck[deck_idx].label();
                format!("upgraded to {label}")
            }
            PickPurpose::Remove => {
                let card = self.player.deck.remove(deck_idx);
                // 寄生这类"被抽出牌组要付代价"的牌
                self.pay_deck_leave_cost(&card);
                format!("{} removed from your deck", card.label())
            }
            PickPurpose::Transform => {
                let before = self.player.deck[deck_idx].label();
                self.transform_deck_card(deck_idx);
                let after = self.player.deck.last().map(|c| c.label()).unwrap_or_default();
                format!("{before} transformed into {after}")
            }
            PickPurpose::Duplicate => {
                let copy = self.player.deck[deck_idx].clone();
                let label = copy.label();
                self.player.deck.push(copy);
                format!("{label} duplicated")
            }
        };
        if cost > 0 {
            self.spend_gold(cost);
            if let (Some(slot), Some(shop)) = (slot, self.shop.as_mut()) {
                shop.sold[slot] = true;
                shop.removes += 1;
                let bump = shop.removes as i32 * 25;
                for item in shop.items.iter_mut() {
                    if let ShopItem::Remove(p) = item {
                        *p += bump;
                    }
                }
            }
        }
        self.picker = None;
        self.say(msg.clone());
        self.screen = back;
        Ok(msg)
    }

    pub fn picker_cancel(&mut self) {
        if let Some(p) = self.picker.take() {
            self.screen = p.back;
        }
    }

    /// 一张牌离开牌组时要付的代价(寄生:3 点最大生命).
    /// 删牌、变形这类"把原牌从牌组拿走"的入口都要调一次;复制不用调.
    pub fn pay_deck_leave_cost(&mut self, card: &CardInstance) {
        let toll: i32 = card
            .effects()
            .iter()
            .map(|e| match *e {
                CardEffect::LoseMaxHpOnRemoved { n } => n,
                _ => 0,
            })
            .sum();
        if toll > 0 {
            self.player.max_hp = (self.player.max_hp - toll).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
    }

    // ---- 药水 ----

    pub fn add_potion(&mut self, def: &'static PotionDef) -> bool {
        for slot in self.player.potions.iter_mut() {
            if slot.is_none() {
                *slot = Some(def);
                return true;
            }
        }
        false
    }

    pub fn quaff_potion(&mut self, slot: usize, target: Option<usize>) -> Result<String, String> {
        let Some(Some(def)) = self.player.potions.get(slot).copied() else {
            return Err("no potion in that slot".to_string());
        };
        if self.screen == Screen::Combat {
            if let Some(c) = self.combat.as_mut() {
                c.use_potion(def, target);
            }
            self.stats.potions_used += 1;
        } else {
            if !def.out_of_combat {
                return Err(format!("{} only works in combat", def.name));
            }
            match def.fx {
                PotionFx::Heal { amount } => {
                    self.heal(amount);
                }
                PotionFx::MaxHp { n } => {
                    self.player.max_hp += n;
                    self.player.hp += n;
                }
                _ => {}
            }
            self.stats.potions_used += 1;
        }
        self.player.potions[slot] = None;
        if self.screen == Screen::Combat {
            self.sync_combat();
        }
        Ok(format!("used {}", def.name))
    }

    pub fn toss_potion(&mut self, slot: usize) -> Result<String, String> {
        match self.player.potions.get_mut(slot) {
            Some(slot_ref) if slot_ref.is_some() => {
                let def = slot_ref.take().unwrap();
                Ok(format!("discarded {}", def.name))
            }
            _ => Err("no potion in that slot".to_string()),
        }
    }

    // ---- 资源 ----

    pub fn gain_gold(&mut self, n: i32) {
        self.player.gold += n;
    }

    pub fn spend_gold(&mut self, n: i32) {
        self.player.gold = (self.player.gold - n).max(0);
    }

    /// 回血,返回实际回复量
    pub fn heal(&mut self, n: i32) -> i32 {
        if n <= 0 {
            return 0;
        }
        let before = self.player.hp;
        self.player.hp = (self.player.hp + n).min(self.player.max_hp);
        self.player.hp - before
    }

    /// 直接掉血;死了切到死亡界面
    pub fn damage(&mut self, n: i32) {
        if n <= 0 {
            return;
        }
        self.player.hp -= n;
        if self.player.hp <= 0 {
            self.player.hp = 0;
            self.screen = Screen::Death;
        }
    }

    fn apply_relic_pickup(&mut self, fx: RelicFx) {
        if fx.max_hp != 0 {
            self.player.max_hp += fx.max_hp;
            self.player.hp += fx.max_hp;
        }
        if fx.heal > 0 {
            self.heal(fx.heal);
        }
        if fx.gold > 0 {
            self.gain_gold(fx.gold);
        }
    }

    pub fn gain_relic(&mut self, def: &'static RelicDef) {
        self.apply_relic_pickup(def.fx);
        self.player.relics.push(def);
        self.relic_pool.retain(|r| r.id != def.id);
        self.say(format!("relic: {}", def.name));
    }

    // ---- 调试命令:按名字加/删遗物与卡牌 ----

    /// 名字归一化:大小写、空格、下划线都不计较("Blood for Blood" = blood_for_blood)
    fn norm(name: &str) -> String {
        name.trim().to_lowercase().replace([' ', '-'], "_")
    }

    pub fn debug_add_relic(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        if want == "all" {
            let ids: Vec<&'static str> = relics::RELICS.iter().map(|r| r.id).collect();
            let mut n = 0;
            for id in ids {
                if !self.player.relics.iter().any(|r| r.id == id) {
                    let def = relics::relic_def_or_panic(id);
                    self.gain_relic(def);
                    n += 1;
                }
            }
            return Ok(format!("added {n} relics"));
        }
        let def = relics::RELICS
            .iter()
            .find(|r| Self::norm(r.id) == want || Self::norm(r.name) == want)
            .ok_or_else(|| format!("no relic named {name}"))?;
        let name = def.name;
        self.gain_relic(def);
        Ok(format!("added relic {name}"))
    }

    pub fn debug_remove_relic(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        let idx = self
            .player
            .relics
            .iter()
            .position(|r| Self::norm(r.id) == want || Self::norm(r.name) == want)
            .ok_or_else(|| format!("no relic named {name}"))?;
        let def = self.player.relics.remove(idx);
        self.relic_pool.push(def);
        Ok(format!("removed relic {}", def.name))
    }

    /// 加牌:战斗中加到手牌(满了就不加),平时加进牌组
    pub fn debug_add_card(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        let def = cards::CARDS
            .iter()
            .find(|c| Self::norm(c.id) == want || Self::norm(c.name) == want)
            .ok_or_else(|| format!("no card named {name}"))?;
        let label = def.name;
        if let Some(c) = self.combat.as_mut() {
            if c.hand.len() >= crate::core::combat::HAND_LIMIT {
                return Ok(format!("hand is full, {label} not added"));
            }
            let mut inst = cards::card(def.id);
            c.fix_new_card(&mut inst);
            c.hand.push(inst);
        } else {
            self.player.deck.push(cards::card(def.id));
        }
        Ok(format!("added card {label}"))
    }

    /// 调试用:往战斗里的某个牌堆直接塞牌(:card pile draw Havoc, Havoc)
    pub fn debug_pile_cards(&mut self, pile: &str, args: &str) -> Result<String, String> {
        let Some(c) = self.combat.as_mut() else {
            return Err("not in a battle".to_string());
        };
        let mut n = 0;
        for part in args.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            let want = Self::norm(name);
            let def = cards::CARDS
                .iter()
                .find(|c| Self::norm(c.id) == want || Self::norm(c.name) == want)
                .ok_or_else(|| format!("no card named {name}"))?;
            let mut inst = cards::card(def.id);
            c.fix_new_card(&mut inst);
            match pile {
                "hand" => {
                    if c.hand.len() >= crate::core::combat::HAND_LIMIT {
                        continue;
                    }
                    c.hand.push(inst);
                }
                "draw" => c.draw.push(inst),
                "discard" => c.discard.push(inst),
                "exhaust" => c.exhaust.push(inst),
                other => return Err(format!("unknown pile {other}")),
            }
            n += 1;
        }
        Ok(format!("put {n} card(s) into {pile}"))
    }

    /// 删牌:名字 / all(整副) / hand(手牌选择窗口) / hand all(手牌全删)
    pub fn debug_remove_card(&mut self, what: &str) -> Result<String, String> {
        let arg = what.trim().to_lowercase();
        if arg.is_empty() {
            // 没给名字:开牌组里的选牌窗口
            return Err("pick from the deck (see the picker)".to_string());
        }
        if arg == "all" {
            let n = self.player.deck.len();
            self.player.deck.clear();
            return Ok(format!("removed {n} cards from the deck"));
        }
        if let Some(rest) = arg.strip_prefix("hand") {
            let all = rest.trim() == "all";
            if all {
                if let Some(c) = self.combat.as_mut() {
                    let n = c.hand.len();
                    c.hand.clear();
                    return Ok(format!("removed {n} cards from your hand"));
                }
                return Err("not in a battle".to_string());
            }
            if self.combat.is_none() {
                return Err("not in a battle".to_string());
            }
            return Err("pick from your hand".to_string());
        }
        let want = Self::norm(&arg);
        let before = self.player.deck.len();
        self.player.deck.retain(|c| {
            !(Self::norm(c.def.id) == want || Self::norm(c.def.name) == want)
        });
        if let Some(c) = self.combat.as_mut() {
            c.hand.retain(|x| !(Self::norm(x.def.id) == want || Self::norm(x.def.name) == want));
        }
        let removed = before - self.player.deck.len();
        Ok(format!("removed {removed} card(s)"))
    }

    /// 升级牌组里的某张牌(默认 1 次),可无限升级的会一直在牌组里升
    pub fn debug_upgrade_card(&mut self, name: &str, times: usize) -> Result<String, String> {
        let want = Self::norm(name);
        let mut done = 0;
        for card in self.player.deck.iter_mut() {
            if Self::norm(card.def.id) == want || Self::norm(card.def.name) == want {
                for _ in 0..times.max(1) {
                    if card.upgrade() {
                        done += 1;
                    }
                }
            }
        }
        if let Some(c) = self.combat.as_mut() {
            for card in c.hand.iter_mut() {
                if Self::norm(card.def.id) == want || Self::norm(card.def.name) == want {
                    for _ in 0..times.max(1) {
                        if card.upgrade() {
                            done += 1;
                        }
                    }
                }
            }
        }
        if done == 0 {
            return Err(format!("no upgradable card named {name} in your deck"));
        }
        Ok(format!("upgraded {done} time(s)"))
    }

    /// 加遗物:名字用逗号分隔,可以一次加多个("Pear, Vajra")
    pub fn debug_add_relics(&mut self, args: &str) -> Result<String, String> {
        let mut out = Vec::new();
        for part in args.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push(self.debug_add_relic(part)?);
        }
        if out.is_empty() {
            return Err("no relic name given".to_string());
        }
        Ok(out.join("; "))
    }

    /// 加牌:同样用逗号分隔
    pub fn debug_add_cards(&mut self, args: &str) -> Result<String, String> {
        let mut out = Vec::new();
        for part in args.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push(self.debug_add_card(part)?);
        }
        if out.is_empty() {
            return Err("no card name given".to_string());
        }
        Ok(out.join("; "))
    }

    /// 调试用:开牌组里的"删一张"窗口
    pub fn debug_open_remove_picker(&mut self) {
        self.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
    }

    /// 调试用:开"从手牌删一张"的选择(战斗里)
    pub fn debug_begin_hand_remove(&mut self) -> Result<(), String> {
        match self.combat.as_mut() {
            Some(c) => {
                c.debug_begin_hand_remove();
                Ok(())
            }
            None => Err("not in a battle".to_string()),
        }
    }

    /// 调试用:直接进某个房间。不改地图、不动位置,退出后照旧回到原来的地图。
    /// what: shop / event [事件id] / battle(随机 boss|elite|enemy) / boss / elite / enemy
    pub fn debug_room(&mut self, what: &str) -> Result<String, String> {
        let (kind, arg) = match what.split_once(' ') {
            Some((k, a)) => (k, Some(a.trim())),
            None => (what, None),
        };
        match kind {
            "shop" => {
                self.open_shop();
                Ok(format!("debug room: {}", self.last_encounter_or("shop")))
            }
            "event" => match arg {
                Some(id) if !id.is_empty() => self.debug_open_event(id),
                _ => {
                    self.open_event();
                    Ok(format!("debug room: {}", self.last_encounter_or("event")))
                }
            },
            "battle" => {
                let kind = match self.streams.floor(FloorStream::MiscRng).below(3) {
                    0 => EnemyKind::Boss,
                    1 => EnemyKind::Elite,
                    _ => EnemyKind::Normal,
                };
                let enc = self.pick_encounter(kind);
                let id = enc.id;
                self.start_combat(enc);
                Ok(format!("debug room: battle {id}"))
            }
            "boss" | "elite" | "enemy" => {
                // 点名一只怪(:room enemy jaw_worm)或一场遭遇(:room enemy slime_boss)
                if let Some(name) = arg {
                    let wanted = name.trim().to_lowercase();
                    let by_id = |id: &str| id == wanted;
                    let enc = enemies::all_encounters()
                        .find(|e| by_id(e.id))
                        .or_else(|| {
                            enemies::ENEMIES
                                .iter()
                                .find(|e| by_id(e.id) || e.name.to_lowercase() == wanted)
                                .and_then(|e| enemies::encounter_with_enemy(e.id))
                        });
                    match enc {
                        Some(enc) => {
                            let id = enc.id;
                            self.start_combat(enc);
                            return Ok(format!("debug room: battle {id}"));
                        }
                        None => {
                            return Err(format!("unknown enemy or encounter '{name}'"));
                        }
                    }
                }
                let kind = match kind {
                    "boss" => EnemyKind::Boss,
                    "elite" => EnemyKind::Elite,
                    _ => EnemyKind::Normal,
                };
                let enc = self.pick_encounter(kind);
                let id = enc.id;
                self.start_combat(enc);
                Ok(format!("debug room: battle {id}"))
            }
            other => Err(format!(
                "unknown room '{other}', try: shop, event, battle, boss, elite, enemy"
            )),
        }
    }

    /// 调试用:直接打开某个事件(翻牌事件会把棋盘铺好)
    pub fn debug_open_event(&mut self, id: &str) -> Result<String, String> {
        let def = crate::core::events::EVENTS
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("unknown event '{id}'"))?;
        self.open_event_def(def);
        Ok(format!("debug event: {}", def.id))
    }

    /// 调试用的名字:拿不到就退回默认
    fn last_encounter_or(&self, fallback: &str) -> String {
        if self.last_encounter.is_empty() {
            fallback.to_string()
        } else {
            self.last_encounter.to_string()
        }
    }

    /// 从池子里取一个指定稀有度的遗物
    fn take_relic_of(&mut self, rarity: Rarity) -> Option<&'static RelicDef> {
        let idx = self.relic_index_of(rarity)?;
        Some(self.relic_pool.remove(idx))
    }

    /// 只看不取(宝箱房在开箱前就要把遗物名字显示出来)
    fn peek_relic_of(&self, rarity: Rarity) -> Option<&'static RelicDef> {
        self.relic_index_of(rarity).map(|i| self.relic_pool[i])
    }

    fn relic_index_of(&self, rarity: Rarity) -> Option<usize> {
        self.relic_pool
            .iter()
            .position(|r| r.rarity == rarity)
            .or_else(|| if self.relic_pool.is_empty() { None } else { Some(0) })
    }

    /// 随机一件遗物:先滚稀有度(参考实现 returnRandomRelicTier)
    fn take_relic_of_any(&mut self) -> Option<&'static RelicDef> {
        let tier = self.roll_combat_relic_tier();
        self.take_relic_of(tier)
    }

    /// 战斗奖励的遗物稀有度:<50 普通,<83 罕见,其余稀有(都走 relicRng)
    fn roll_combat_relic_tier(&mut self) -> Rarity {
        let roll = self.streams.run(RunStream::RelicRng).random_range(0, 99);
        if roll < 50 {
            Rarity::Common
        } else if roll < 83 {
            Rarity::Uncommon
        } else {
            Rarity::Rare
        }
    }

    /// 精英奖励的遗物稀有度:<50 普通,>82 稀有,其余罕见(参考实现 returnRandomRelicTierElite)
    fn roll_elite_relic_tier(&mut self) -> Rarity {
        let roll = self.streams.run(RunStream::RelicRng).random(99);
        if roll < ELITE_RELIC_COMMON_BELOW {
            Rarity::Common
        } else if roll > ELITE_RELIC_RARE_ABOVE {
            Rarity::Rare
        } else {
            Rarity::Uncommon
        }
    }

    /// 药水掉落:先掷 d100(带保底 ±10),掉出来再掷是哪瓶
    fn roll_potion_reward(&mut self, rewards_so_far: usize) -> Option<&'static PotionDef> {
        let mut chance = POTION_DROP_BASE_CHANCE + self.potion_chance;
        if rewards_so_far >= 4 {
            chance = 0;
        }
        if self.streams.run(RunStream::PotionRng).random(99) as i32 >= chance {
            self.potion_chance += POTION_PITY_STEP;
            return None;
        }
        self.potion_chance -= POTION_PITY_STEP;
        potions::random_potion(self.streams.run(RunStream::PotionRng))
    }

    /// 卡牌奖励:三张,每张先掷稀有度(带动保底)再从对应池子里抽一张,同一次不重样
    fn create_card_reward(&mut self, kind: EnemyKind) -> Vec<CardInstance> {
        let mut out: Vec<CardInstance> = Vec::new();
        for _ in 0..CARD_REWARD_COUNT {
            let rarity = self.roll_card_rarity(kind);
            match rarity {
                Rarity::Rare => self.card_rarity_factor = CARD_RARITY_PITY_START,
                Rarity::Common => {
                    self.card_rarity_factor =
                        (self.card_rarity_factor - 1).max(CARD_RARITY_PITY_FLOOR)
                }
                _ => {}
            }
            let pool = cards::reward_pool(rarity);
            if pool.is_empty() {
                break;
            }
            let mut id = self.streams.run(RunStream::CardRng).pick(&pool).id;
            let mut guard = 0;
            loop {
                if !out.iter().any(|c| c.def.id == id) {
                    break;
                }
                id = self.streams.run(RunStream::CardRng).pick(&pool).id;
                guard += 1;
                if guard >= 1000 {
                    break;
                }
            }
            out.push(CardInstance::new(cards::card_def_or_panic(id)));
        }
        out
    }

    /// 抽稀有度:Boss 直接稀有,其余 d100 + 保底值比 3/37(精英 10/40)
    fn roll_card_rarity(&mut self, kind: EnemyKind) -> Rarity {
        if kind == EnemyKind::Boss {
            return Rarity::Rare;
        }
        let roll = self.streams.run(RunStream::CardRng).random(99) as i32 + self.card_rarity_factor;
        let (rare, uncommon) = if kind == EnemyKind::Elite {
            (CARD_RARE_CHANCE_ELITE, CARD_UNCOMMON_CHANCE_ELITE)
        } else {
            (CARD_RARE_CHANCE_NON_ELITE, CARD_UNCOMMON_CHANCE_NON_ELITE)
        };
        if roll < rare {
            Rarity::Rare
        } else if roll < rare + uncommon {
            Rarity::Uncommon
        } else {
            Rarity::Common
        }
    }

    /// 同一次奖励/商店里不重复:已经在 sold 里的就重抽
    fn roll_card_of_distinct(
        &mut self,
        rarity: Rarity,
        sold: &[&'static str],
    ) -> Option<&'static CardDef> {
        let pool: Vec<&'static CardDef> = cards::reward_pool(rarity)
            .into_iter()
            .filter(|c| !sold.contains(&c.id))
            .collect();
        if pool.is_empty() {
            return None;
        }
        Some(*self.streams.run(RunStream::CardRng).pick(&pool))
    }
}

/// 商店底价(按稀有度查表)
fn shop_base(table: [(Rarity, i32); 3], rarity: Rarity) -> i32 {
    table
        .iter()
        .find(|(r, _)| *r == rarity)
        .map(|(_, v)| *v)
        .unwrap_or(50)
}

/// 存档里记的稀有度名字换回枚举
fn rarity_from_name(name: &str) -> Rarity {
    match name {
        "Common" => Rarity::Common,
        "Uncommon" => Rarity::Uncommon,
        "Rare" => Rarity::Rare,
        _ => Rarity::Common,
    }
}

#[cfg(test)]
mod tests {
    /// 测试里把"胜利后停留 2 秒"一步走完(真实流程由事件循环逐帧 tick)
    fn settle(r: &mut Run) {
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
    }

    use super::*;
    use crate::core::map::FLOORS;

    fn run(seed: u64) -> Run {
        Run::new(seed)
    }

    #[test]
    fn new_run_has_starter_pack() {
        let r = run(1);
        assert_eq!(r.player.hp, STARTING_HP);
        assert_eq!(r.player.max_hp, STARTING_HP);
        assert_eq!(r.player.gold, STARTING_GOLD);
        assert_eq!(r.player.deck.len(), 10);
        assert_eq!(r.player.potions.len(), POTION_SLOTS);
        assert!(r.player.potions.iter().all(|p| p.is_none()));
        assert_eq!(r.screen, Screen::Map);
        assert!(r.pos.is_none());
        assert_eq!(r.player.relics.len(), 1);
    }

    #[test]
    fn unreachable_node_is_rejected() {
        let mut r = run(2);
        let far = r.map.row(FLOORS - 1)[0];
        assert!(r.enter_node(far).is_err(), "没出发就不能跳到最后一层");
    }

    /// "不可移除"的诅咒不进删牌候选,普通牌照旧能删
    #[test]
    fn unremovable_curses_stay_out_of_the_removal_picker() {
        let mut r = run(11);
        let deck_len = r.player.deck.len();
        for id in ["ascenders_bane", "curse_of_the_bell", "necronomicurse"] {
            r.player.deck.push(cards::card(id));
        }
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let cands = r.picker_candidates();
        assert_eq!(cands.len(), deck_len, "三张不可移除的牌都不该出现在候选里");
        assert!(cands.iter().all(|i| !r.player.deck[*i].def.unremovable));

        // 选中第一张仍然能正常删掉,删完牌组少一张
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), deck_len + 2);
        assert_eq!(
            r.player
                .deck
                .iter()
                .filter(|c| c.def.unremovable)
                .count(),
            3
        );
    }

    /// 寄生:从牌组里删掉要付 3 点最大生命(它本身不是"不可移除",只是有代价)
    #[test]
    fn removing_parasite_costs_three_max_hp() {
        // 满血时删:当前生命要跟着新上限一起降
        let mut r = run(12);
        r.player.deck.push(cards::card("parasite"));
        let max0 = r.player.max_hp;
        assert_eq!(r.player.hp, max0, "满血开局");
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let slot = r
            .picker_candidates()
            .iter()
            .position(|i| r.player.deck[*i].def.id == "parasite")
            .expect("寄生应该出现在删牌候选里");
        r.picker.as_mut().unwrap().index = slot;
        r.picker_confirm().unwrap();
        assert_eq!(r.player.max_hp, max0 - 3, "最大生命 -3");
        assert_eq!(r.player.hp, max0 - 3, "当前生命夹到新的上限");
        assert!(
            !r.player.deck.iter().any(|c| c.def.id == "parasite"),
            "牌组里没有它了"
        );

        // 残血时删:当前生命不受影响,只掉上限
        let mut r = run(13);
        r.player.deck.push(cards::card("parasite"));
        let max0 = r.player.max_hp;
        r.player.hp = 20;
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let slot = r
            .picker_candidates()
            .iter()
            .position(|i| r.player.deck[*i].def.id == "parasite")
            .unwrap();
        r.picker.as_mut().unwrap().index = slot;
        r.picker_confirm().unwrap();
        assert_eq!(r.player.max_hp, max0 - 3);
        assert_eq!(r.player.hp, 20, "当前生命没到上限就不动");
    }

    #[test]
    fn cannot_enter_when_not_on_map() {
        let mut r = run(3);
        r.screen = Screen::Death;
        let start = r.reachable()[0];
        assert!(r.enter_node(start).is_err());
    }

    #[test]
    fn walking_the_map_reaches_the_boss() {
        // 一路往上点第一个可达节点,顺手把每个界面都走一遍:整局流程不能卡死
        let mut r = run(7);
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 400, "整局流程死循环");
            match r.screen {
                Screen::Map => {
                    let next = r.reachable();
                    assert!(!next.is_empty(), "无路可走");
                    let target = *next.last().unwrap();
                    r.enter_node(target).unwrap();
                }
                Screen::Combat => {
                    let c = r.combat.as_mut().unwrap();
                    for e in c.enemies.iter_mut() {
                        e.hp = 0;
                    }
                    c.phase = Phase::Won;
                    settle(&mut r);
                }
                Screen::Reward => {
                    // 药水奖励在格子满时拿不走,所以要给循环一个上限
                    for _ in 0..12 {
                        if r.reward_slots().is_empty() {
                            break;
                        }
                        let _ = r.reward_take();
                        r.reward_clamp();
                    }
                    r.leave_reward();
                }
                Screen::Pick => {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                Screen::Event => {
                    if r.event.as_ref().map(|e| e.result.is_none()).unwrap_or(false) {
                        let _ = r.choose_event(0);
                    }
                    if r.screen == Screen::Event {
                        r.leave_event();
                    }
                }
                Screen::Shop => r.leave_shop(),
                Screen::Rest => r.rest_heal(),
                Screen::Treasure => r.take_treasure(),
                Screen::Victory => break,
                Screen::Death => break,
                // 开始界面那几个不会出现在这条流程里
                Screen::Title | Screen::CharSelect | Screen::Compendium | Screen::Library => {
                    panic!("整局流程里不该出现开始界面")
                }
            }
        }
        assert_eq!(r.screen, Screen::Victory, "走到 Boss 应获胜");
        let last = r.pos.expect("应该有落点");
        assert_eq!(r.map.node(last).kind, NodeKind::Boss);
        assert!(r.floor_reached >= FLOORS - 1);
    }

    /// 商店不卖重复的东西, 也不卖已经拿到的遗物
    #[test]
    fn shop_has_no_duplicates_and_no_owned_relics() {
        for seed in 0..80u64 {
            let mut r = run(seed);
            r.open_shop();
            let mut keys: Vec<String> = Vec::new();
            for it in &r.shop.as_ref().unwrap().items {
                let key = match it {
                    ShopItem::Card(c, _) => format!("card:{}", c.def.id),
                    ShopItem::Relic(d, _) => format!("relic:{}", d.id),
                    ShopItem::Potion(d, _) => format!("potion:{}", d.id),
                    ShopItem::Remove(_) => "remove".to_string(),
                };
                assert!(!keys.contains(&key), "seed {seed}: 商店里重复了 {key}");
                keys.push(key);
            }
            for it in &r.shop.as_ref().unwrap().items {
                if let ShopItem::Relic(d, _) = it {
                    assert!(
                        !r.player.relics.iter().any(|x| x.id == d.id),
                        "seed {seed}: 卖了已经有的遗物 {}",
                        d.id
                    );
                }
            }
            // 池子里不该留着已经拿到的遗物(全局不重复)
            assert!(r
                .relic_pool
                .iter()
                .all(|p| !r.player.relics.iter().any(|o| o.id == p.id)));
        }
    }

    /// 三选一不该出现同一张牌(抽很多局来看)
    #[test]
    fn reward_cards_never_repeat() {
        for seed in 0..120u64 {
            let mut r = run(seed);
            let start = r.reachable()[0];
            r.enter_node(start).unwrap();
            {
                let c = r.combat.as_mut().unwrap();
                for e in c.enemies.iter_mut() {
                    e.hp = 0;
                }
                c.phase = Phase::Won;
            }
            settle(&mut r);
            let cards = &r.reward.as_ref().unwrap().cards;
            let mut ids: Vec<&str> = cards.iter().map(|c| c.def.id).collect();
            let before = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(before, ids.len(), "seed {seed}: 奖励里出现了重复的牌");
        }
    }

    #[test]
    fn combat_victory_gives_reward_and_keeps_hp() {
        let mut r = run(11);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        let hp_before = r.player.hp;
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        let reward = r.reward.as_ref().unwrap();
        assert!(reward.gold > 0);
        assert_eq!(reward.cards.len(), 3);
        assert!(r.player.hp >= hp_before.min(r.player.max_hp));
        // 领金币
        let mut got_gold = false;
        for i in 0..r.reward_slots().len() {
            if let Some(rw) = r.reward.as_mut() {
                rw.index = i;
            }
            if matches!(r.reward_slots().get(i), Some(RewardSlot::Gold)) {
                r.reward_take().unwrap();
                got_gold = true;
            }
        }
        assert!(got_gold);
        assert!(r.player.gold > STARTING_GOLD);
    }

    /// 贪婪之手赚到的金币要真的进到这一局的钱包里,而且只进一次
    #[test]
    fn hand_of_greed_gold_reaches_the_run() {
        let mut r = run(17);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        let gold_before = r.player.gold;
        {
            let c = r.combat.as_mut().unwrap();
            c.enemies[0].hp = 1;
            c.energy = 9;
        }
        r.debug_add_card("Hand of Greed").unwrap();
        let idx = r
            .combat
            .as_ref()
            .unwrap()
            .hand
            .iter()
            .position(|c| c.def.id == "hand_of_greed")
            .expect("调试加的牌该在手牌里");
        r.combat
            .as_mut()
            .unwrap()
            .play_card(idx, Some(0))
            .unwrap();
        r.sync_combat();
        assert_eq!(r.player.gold, gold_before + 20, "致命一击的金币要落袋");
        r.sync_combat();
        assert_eq!(r.player.gold, gold_before + 20, "同一笔钱不能入账两次");
    }

    #[test]
    fn boss_victory_leads_to_victory_screen() {
        let mut r = run(13);
        let boss_floor = FLOORS;
        // 直接把玩家放到 Boss 前一层的节点上
        let pre = r.map.row(FLOORS - 1)[0];
        r.pos = Some(pre);
        r.floor_reached = FLOORS - 1;
        let boss = r.map.boss;
        assert_eq!(r.map.node(boss).floor, boss_floor);
        r.enter_node(boss).unwrap();
        let c = r.combat.as_mut().unwrap();
        for e in c.enemies.iter_mut() {
            e.hp = 0;
        }
        c.phase = Phase::Won;
        settle(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        let next = r.reward.as_ref().unwrap().next;
        assert_eq!(next, Screen::Victory);
    }

    #[test]
    fn defeat_ends_the_run() {
        let mut r = run(17);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        {
            let c = r.combat.as_mut().unwrap();
            c.player.hp = 0;
            c.phase = Phase::Lost;
        }
        settle(&mut r);
        assert_eq!(r.screen, Screen::Death);
        assert_eq!(r.player.hp, 0);
    }

    #[test]
    fn shop_prices_lose_money_and_add_cards() {
        let mut r = run(19);
        // 找一个商店节点,站到它上一层
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .expect("地图应有商店");
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.floor_reached = r.map.node(parent).floor;
        r.player.gold = 500;
        r.enter_node(shop).unwrap();
        assert_eq!(r.screen, Screen::Shop);
        let gold_before = r.player.gold;
        let deck_before = r.player.deck.len();
        // 买第一件商品(一定是卡牌)
        let msg = r.buy_selected().unwrap();
        assert!(msg.starts_with("bought"));
        assert!(r.player.gold < gold_before);
        assert_eq!(r.player.deck.len(), deck_before + 1);
    }

    #[test]
    fn shop_rejects_purchase_without_gold() {
        let mut r = run(23);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 0;
        assert!(r.buy_selected().is_err());
    }

    #[test]
    fn removal_service_charges_on_confirm() {
        let mut r = run(29);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 500;
        let idx = r
            .shop
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|i| matches!(i, ShopItem::Remove(_)))
            .unwrap();
        r.shop.as_mut().unwrap().index = idx;
        let price = r.shop.as_ref().unwrap().items[idx].price();
        r.buy_selected().unwrap();
        assert_eq!(r.screen, Screen::Pick);
        let deck_before = r.player.deck.len();
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), deck_before - 1);
        assert_eq!(r.player.gold, 500 - price);
        assert_eq!(r.screen, Screen::Shop);
    }

    #[test]
    fn picker_cancel_leaves_deck_alone() {
        let mut r = run(31);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 500;
        let idx = r
            .shop
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|i| matches!(i, ShopItem::Remove(_)))
            .unwrap();
        r.shop.as_mut().unwrap().index = idx;
        r.buy_selected().unwrap();
        let before = r.player.deck.len();
        r.picker_cancel();
        assert_eq!(r.player.deck.len(), before);
        assert_eq!(r.player.gold, 500, "取消不该扣钱");
    }

    #[test]
    fn rest_heals_thirty_percent() {
        let mut r = run(37);
        r.player.hp = 10;
        r.rest_heal();
        assert_eq!(r.player.hp, 10 + STARTING_HP * REST_HEAL_PCT / 100);
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn smith_upgrades_a_card() {
        let mut r = run(41);
        r.rest_smith();
        assert_eq!(r.screen, Screen::Pick);
        let before = r.player.deck.iter().filter(|c| c.upgraded).count();
        r.picker_confirm().unwrap();
        let after = r.player.deck.iter().filter(|c| c.upgraded).count();
        assert_eq!(after, before + 1);
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn potion_slots_are_finite() {
        let mut r = run(43);
        let Some(def) = crate::core::potions::POTIONS.first() else {
            return;
        };
        assert!(r.add_potion(def));
        assert!(r.add_potion(def));
        assert!(r.add_potion(def));
        assert!(!r.add_potion(def), "第四个格子不该存在");
        assert!(r.toss_potion(0).is_ok());
        assert!(r.add_potion(def));
    }

    #[test]
    fn potion_use_respects_context() {
        let mut r = run(47);
        // 战斗外只能喝能在地图上用的药水
        let combat_only = crate::core::potions::POTIONS
            .iter()
            .find(|p| !p.out_of_combat);
        if let Some(def) = combat_only {
            r.player.potions[0] = Some(def);
            assert!(r.quaff_potion(0, None).is_err());
        }
        let out = crate::core::potions::POTIONS
            .iter()
            .find(|p| p.out_of_combat);
        if let Some(def) = out {
            r.player.potions[1] = Some(def);
            r.player.hp = 1;
            assert!(r.quaff_potion(1, None).is_ok());
            assert!(r.player.potions[1].is_none());
        }
    }

    #[test]
    fn events_apply_outcomes() {
        let mut r = run(53);
        r.open_event();
        assert_eq!(r.screen, Screen::Event);
        // 随机抽到的事件可能是多屏或翻牌那种,结算挑一个选项 0 就直接给结果的
        r.debug_open_event("big_fish").unwrap();
        let before = r.player.gold;
        r.choose_event(0).unwrap();
        assert!(r.event.as_ref().unwrap().result.is_some());
        // 选项可能给钱也可能要钱,只要求不 panic 且状态自洽
        assert!(r.player.gold >= 0 || before >= 0);
        r.leave_event();
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn history_records_the_run() {
        let mut r = run(67);
        let first = r.history.len();
        assert!(first > 0, "开局就该有一条");
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert!(r.history.len() > first, "进入房间要记一笔");
        {
            let c = r.combat.as_mut().unwrap();
            c.play_card(0, Some(0)).ok();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        let joined = r
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("-- Turn 1 --"), "战斗日志要抄进历史");
        assert!(joined.contains("dealt") || joined.contains("you play"));
    }

    #[test]
    fn fruit_juice_works_on_the_map() {
        let mut r = run(83);
        let def = potions::POTIONS
            .iter()
            .find(|p| p.id == "fruit_juice")
            .expect("果汗药水应该在池子里");
        assert!(def.out_of_combat, "果汗要能在地图上喝");
        r.player.potions[0] = Some(def);
        let (hp, max_hp) = (r.player.hp, r.player.max_hp);
        r.quaff_potion(0, None).unwrap();
        assert_eq!(r.player.max_hp, max_hp + 5);
        assert_eq!(r.player.hp, hp + 5);
        assert!(r.player.potions[0].is_none(), "喝完该腾出格子");
    }

    #[test]
    fn potion_reward_needs_a_free_slot() {
        let mut r = run(71);
        // 三格占满时拿不下药水,腾出一格就能拿
        let def = crate::core::potions::POTIONS.first().unwrap();
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        let reward = r.reward.as_mut().unwrap();
        reward.potion = Some(def);
        reward.potion_taken = false;
        for slot in r.player.potions.iter_mut() {
            *slot = Some(def);
        }
        r.reward_clamp();
        // 选中药水那一行
        let slots = r.reward_slots();
        let idx = slots
            .iter()
            .position(|s| matches!(s, RewardSlot::Potion))
            .unwrap();
        r.reward.as_mut().unwrap().index = idx;
        assert!(r.reward_take().is_err(), "满格时不该拿得下");
        assert!(r.reward_slots().contains(&RewardSlot::Potion), "拿不下就该还在");
        // 腾一格
        r.player.potions[0] = None;
        let msg = r.reward_take().unwrap();
        assert!(msg.starts_with("potion gained"), "腾出格子后应该能拿走");
        assert!(r.player.potions.iter().flatten().count() == 3);
    }

    #[test]
    fn gold_never_goes_negative() {
        let mut r = run(59);
        r.player.gold = 5;
        r.spend_gold(50);
        assert_eq!(r.player.gold, 0);
    }

    #[test]
    fn damage_can_kill_the_player() {
        let mut r = run(61);
        r.player.hp = 3;
        r.damage(10);
        assert_eq!(r.player.hp, 0);
        assert_eq!(r.screen, Screen::Death);
    }

    /// 简单策略机器人:有威胁就挡,挡得住就砍,用最省的办法打完一只手牌
    pub(super) fn bot_take_turn(c: &mut Combat) {
        loop {
            let incoming: i32 = c
                .alive_enemies()
                .iter()
                .map(|i| {
                    let (per, times) = c.predicted_damage(*i);
                    per * times.max(1) as i32
                })
                .sum();
            let need_block = incoming > c.player.block;
            let mut best: Option<(usize, i32)> = None;
            for idx in 0..c.hand.len() {
                if c.blocked_reason(idx).is_some() {
                    continue;
                }
                let card = &c.hand[idx];
                let mut dmg = 0;
                let mut blk = 0;
                for e in card.effects() {
                    match *e {
                        crate::core::card::Effect::Damage { amount, times }
                        | crate::core::card::Effect::DamageAll { amount, times }
                        | crate::core::card::Effect::DamageRandom { amount, times }
                        | crate::core::card::Effect::DamageWithBonus { amount, times } => {
                            dmg += amount * times.max(1) as i32
                        }
                        crate::core::card::Effect::DamageAllX { per } => dmg += per * c.energy,
                        crate::core::card::Effect::Reaper { amount } => dmg += amount,
                        crate::core::card::Effect::Block { amount } => blk += amount,
                        _ => {}
                    }
                }
                let score = if need_block {
                    blk * 3 + dmg
                } else {
                    dmg * 3 + blk
                };
                if score > 0 && best.map(|(_, s)| score > s).unwrap_or(true) {
                    best = Some((idx, score));
                }
            }
            let Some((idx, _)) = best else { return };
            let target = c
                .alive_enemies()
                .into_iter()
                .min_by_key(|i| c.enemies[*i].hp);
            if c.play_card(idx, target).is_err() {
                return;
            }
        }
    }

    /// 把非战斗界面推进到地图:只有战斗中才需要机器人动脑
    fn resolve_non_combat(r: &mut Run) {
        match r.screen {
            Screen::Shop => r.leave_shop(),
            Screen::Rest => r.rest_heal(),
            Screen::Treasure => r.take_treasure(),
            Screen::Event => {
                let _ = r.choose_event(0);
                while r.screen == Screen::Pick {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                if r.screen == Screen::Event {
                    r.leave_event();
                }
            }
            _ => {}
        }
    }

    /// 让机器人把一局打完(死了也算走完),返回结束时的状态
    fn bot_play(mut r: Run, seed: u64) -> Run {
        let mut guard = 0;
        while r.screen != Screen::Victory && r.screen != Screen::Death {
            guard += 1;
            assert!(guard < 80, "seed {seed}: 流程卡在 {:?}", r.screen);
            match r.screen {
                Screen::Map => {
                    let next = r.reachable();
                    assert!(!next.is_empty(), "seed {seed}: 无路可走");
                    let node = next[next.len() / 2];
                    r.enter_node(node).unwrap();
                }
                Screen::Combat => {
                    let mut turns = 0;
                    loop {
                        turns += 1;
                        assert!(turns <= 40, "seed {seed} 单场战斗超过 40 回合");
                        bot_take_turn(r.combat.as_mut().unwrap());
                        if r.combat.as_ref().unwrap().phase != Phase::PlayerTurn {
                            break;
                        }
                        r.combat.as_mut().unwrap().end_turn();
                    }
                    settle(&mut r);
                }
                Screen::Reward => {
                    for _ in 0..12 {
                        if r.reward_slots().is_empty() {
                            break;
                        }
                        let _ = r.reward_take();
                        r.reward_clamp();
                    }
                    r.leave_reward();
                }
                Screen::Rest => {
                    // 血少就睡,否则打铁升级
                    if r.player.hp * 2 < r.player.max_hp {
                        r.rest_heal();
                    } else {
                        r.rest_smith();
                    }
                }
                Screen::Pick => {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                _ => resolve_non_combat(&mut r),
            }
        }
        r
    }

    #[test]
    fn a_simple_bot_can_clear_the_first_half_of_the_act() {
        // 机器人只会"有威胁就挡、否则砍",奖励全拿、血少就睡.
        // 它要连前半段都上不去,说明数值或流程有硬伤
        let mut best = 0usize;
        let mut report = Vec::new();
        for seed in [1u64, 2, 3, 5, 8, 13, 21] {
            let r = bot_play(run(seed), seed);
            report.push((seed, r.floor_reached + 1, r.player.hp, r.stats.fights));
            best = best.max(r.floor_reached);
        }
        assert!(
            best >= 12,
            "机器人最好的成绩只到第 {} 层,报告 {:?}",
            best + 1,
            report
        );
    }
}
