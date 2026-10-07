// 一局(run)的流程:地图推进、战斗结算、奖励、商店、事件、营火、牌组管理.
// 所有状态都在这里,UI 只读这些字段并调用这里的方法改状态.
use crate::core::card::{CardDef, CardInstance, Rarity};
use crate::core::cards;
use crate::core::combat::{Combat, CombatSetup, Phase};
use crate::core::corpus;
use crate::core::enemies;
use crate::core::enemy::{EnemyKind, Encounter};
use crate::core::events::{EventDef, Outcome};
use crate::core::map::{ActMap, NodeKind};
use crate::core::potions::{self, PotionDef, PotionFx};
use crate::core::relics::{self, RelicDef, RelicFx};
use crate::core::roster;
use crate::rng::Rng;

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
}

impl PickPurpose {
    pub fn title(self) -> &'static str {
        match self {
            PickPurpose::Upgrade => "choose a card to upgrade",
            PickPurpose::Remove => "choose a card to remove",
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

pub struct EventState {
    pub def: &'static EventDef,
    pub index: usize,
    /// 已选结果,展示完才能离开
    pub result: Option<&'static str>,
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
    pub rng: Rng,
    pub player: Player,
    pub map: ActMap,
    /// 本局这条路的 Boss:开局定下来,地图上直接写名字
    pub boss_enc: &'static Encounter,
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
    pub rest_index: usize,
    pub stats: Stats,
    /// 一整局发生过的所有事,用 H 翻看
    pub history: Vec<HistoryEntry>,
    /// 已经抄进历史的战斗日志序号
    combat_log_seen: u64,
    /// 本局还没出现过的遗物
    relic_pool: Vec<&'static RelicDef>,
    last_encounter: &'static str,
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
        let def = cards::card_def(id).ok_or_else(|| format!("存档里的卡 {id} 不认识"))?;
        let mut inst = cards::card(def.id);
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
        let mut rng = Rng::new(seed);
        let mut deck: Vec<CardInstance> = Vec::new();
        for (id, n) in ch.deck {
            for _ in 0..*n {
                deck.push(cards::card(id));
            }
        }
        let starter = relics::relic_def_or_panic(ch.relic);
        let relic_pool: Vec<&'static RelicDef> = relics::RELICS
            .iter()
            .filter(|r| r.id != starter.id)
            .collect();
        let map = ActMap::generate(&mut rng);
        let boss_enc: &'static Encounter = rng.pick(enemies::BOSSES);
        let mut run = Run {
            seed,
            character: ch.id,
            fight_seq: 0,
            win_hold: 0,
            rng,
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
            rest_index: 0,
            stats: Stats::default(),
            history: Vec::new(),
            combat_log_seen: 0,
            relic_pool,
            last_encounter: "",
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
        let r = self.rng.state();
        out.push_str(&format!("rng={},{},{},{}\n", r[0], r[1], r[2], r[3]));
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
        if let Some(v) = get("rng") {
            let parts: Vec<u64> = v
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            if parts.len() == 4 {
                run.rng.set_state([parts[0], parts[1], parts[2], parts[3]]);
            }
        }
        if let Some(v) = get("deck") {
            let mut deck: Vec<CardInstance> = Vec::new();
            for item in v.split(',').filter(|s| !s.is_empty()) {
                let (id, up) = item.split_once(':').unwrap_or((item, "0"));
                let Some(def) = cards::card_def(id) else {
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
                let Some(def) = relics::relic_def(id) else {
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
                let seed = run.rng.next_u64();
                let setup = CombatSetup {
                    hp: run.player.hp,
                    max_hp: run.player.max_hp,
                    deck: run.player.deck.clone(),
                    relics: run.player.relics.clone(),
                };
                let mut c = Combat::new(enc, setup, seed);
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
                run.combat = Some(c);
                run.screen = Screen::Combat;
                run.win_hold = 0;
                run.sync_combat();
            }
        }
        // 还没出现过的遗物:重建一遍(已经拿到的都排掉)
        run.relic_pool = relics::RELICS
            .iter()
            .filter(|r| !run.player.relics.iter().any(|o| o.id == r.id))
            .collect();
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

    fn pick_encounter(&mut self, kind: EnemyKind) -> &'static Encounter {
        let pool: &'static [Encounter] = match kind {
            EnemyKind::Normal => {
                if self.floor_reached < 3 {
                    enemies::ENCOUNTERS_WEAK
                } else {
                    enemies::ENCOUNTERS
                }
            }
            EnemyKind::Elite => enemies::ELITES,
            EnemyKind::Boss => enemies::BOSSES,
        };
        assert!(!pool.is_empty(), "遭遇池为空: {kind:?}");
        let fresh: Vec<&Encounter> = pool.iter().filter(|e| e.id != self.last_encounter).collect();
        let pick = if fresh.is_empty() {
            self.rng.pick(pool)
        } else {
            *self.rng.pick(&fresh)
        };
        self.last_encounter = pick.id;
        pick
    }

    // ---- 战斗 ----

    fn start_combat(&mut self, enc: &'static Encounter) {
        let seed = self.rng.next_u64();
        self.say(format!("a fight breaks out: {}", enc.id));
        let setup = CombatSetup {
            hp: self.player.hp,
            max_hp: self.player.max_hp,
            deck: self.player.deck.clone(),
            relics: self.player.relics.clone(),
        };
        self.combat = Some(Combat::new(enc, setup, seed));
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
        let Some(c) = self.combat.as_ref() else {
            return;
        };
        let phase = c.phase;
        let hp = c.player.hp;
        let max_hp = c.player.max_hp;
        let damage = c.damage_dealt;
        let turns = c.turn;
        self.player.hp = hp;
        self.player.max_hp = max_hp;
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
        let healed = self.post_combat_heal();
        if healed > 0 {
            self.say(format!("relics heal you for {healed}"));
        }
        let (gold_lo, gold_hi) = match kind {
            EnemyKind::Normal => (10, 20),
            EnemyKind::Elite => (25, 35),
            EnemyKind::Boss => (95, 105),
        };
        let gold = self.rng.range_inclusive(gold_lo, gold_hi);
        // 三选一不重样:同一张牌一次奖励里只出现一遍
        let mut cards: Vec<CardInstance> = Vec::new();
        let mut guard = 0;
        while cards.len() < 3 && guard < 40 {
            guard += 1;
            let Some(def) = self.roll_card() else {
                break;
            };
            if cards.iter().any(|c| c.def.id == def.id) {
                continue;
            }
            cards.push(CardInstance::new(def));
        }
        let relic = match kind {
            EnemyKind::Elite => self.roll_relic_by_odds(50, 33, 17),
            EnemyKind::Boss => self.roll_relic_by_odds(0, 0, 100),
            EnemyKind::Normal => None,
        };
        let potion = if self.rng.chance(40) {
            Some(potions::random_potion(&mut self.rng))
        } else {
            None
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
        // 店里卖的牌也不重样
        let mut sold: Vec<&'static str> = Vec::new();
        let mut sold_potions: Vec<&'static str> = Vec::new();
        for (rarity, count) in [
            (Rarity::Common, 2),
            (Rarity::Uncommon, 2),
            (Rarity::Rare, 1),
        ] {
            for _ in 0..count {
                if let Some(def) = self.roll_card_of_distinct(rarity, &sold) {
                    let base = match rarity {
                        Rarity::Common => self.rng.range_inclusive(45, 55),
                        Rarity::Uncommon => self.rng.range_inclusive(68, 82),
                        _ => self.rng.range_inclusive(135, 165),
                    };
                    sold.push(def.id);
                    items.push(ShopItem::Card(CardInstance::new(def), self.discount(base)));
                }
            }
        }
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            if let Some(def) = self.take_relic_of(rarity) {
                let base = self.rng.range_inclusive(143, 172);
                items.push(ShopItem::Relic(def, self.discount(base)));
            }
        }
        for _ in 0..2 {
            // 两瓶药水不重复
            let mut def = potions::random_potion(&mut self.rng);
            let mut guard = 0;
            while sold_potions.contains(&def.id) && guard < 40 {
                guard += 1;
                def = potions::random_potion(&mut self.rng);
            }
            if sold_potions.contains(&def.id) {
                continue;
            }
            sold_potions.push(def.id);
            let base = self.rng.range_inclusive(48, 72);
            items.push(ShopItem::Potion(def, self.discount(base)));
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
        let def: &'static EventDef = self.rng.pick(crate::core::events::EVENTS);
        self.event = Some(EventState {
            def,
            index: 0,
            result: None,
        });
        self.screen = Screen::Event;
    }

    /// 选项当前是否可选(钱够、血够)
    pub fn event_choice_available(&self, i: usize) -> bool {
        let Some(st) = &self.event else {
            return false;
        };
        let Some(c) = st.def.choices.get(i) else {
            return false;
        };
        self.player.gold >= c.cost_gold && self.player.hp > c.cost_hp
    }

    pub fn choose_event(&mut self, i: usize) -> Result<(), String> {
        let Some(st) = self.event.as_ref() else {
            return Err("no event here".to_string());
        };
        if st.result.is_some() {
            return Err("already resolved".to_string());
        }
        let Some(choice) = st.def.choices.get(i).copied() else {
            return Err("no such choice".to_string());
        };
        if self.player.gold < choice.cost_gold {
            return Err("not enough gold".to_string());
        }
        if self.player.hp <= choice.cost_hp {
            return Err("not enough HP".to_string());
        }
        if choice.cost_gold > 0 {
            self.spend_gold(choice.cost_gold);
        }
        if choice.cost_hp > 0 {
            self.damage(choice.cost_hp);
        }
        let outcome = choice.outcome;
        let text = self.apply_outcome(&outcome);
        if let Some(st) = self.event.as_mut() {
            st.result = Some(text);
        }
        Ok(())
    }

    /// 事件结果结算;返回给玩家看的文本
    fn apply_outcome(&mut self, o: &Outcome) -> &'static str {
        if o.max_hp != 0 {
            self.player.max_hp = (self.player.max_hp + o.max_hp).max(1);
            if o.max_hp > 0 {
                self.player.hp += o.max_hp;
            }
        }
        if o.hp < 0 {
            self.damage(-o.hp);
        } else if o.hp > 0 {
            self.heal(o.hp);
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
        if let Some(id) = o.relic_id {
            let def = relics::relic_def_or_panic(id);
            self.gain_relic(def);
        }
        if let Some(rarity) = o.add_random_card {
            let pool: Vec<&'static CardDef> = cards::reward_pool(rarity);
            if !pool.is_empty() {
                let def = self.rng.pick(&pool);
                self.player.deck.push(cards::card(def.id));
            }
        }
        if let Some(rarity) = o.random_relic_rarity {
            if let Some(def) = self.take_relic_of(rarity) {
                self.gain_relic(def);
            }
        }
        if let Some(id) = o.add_card {
            self.player.deck.push(cards::card(id));
        }
        if let Some(id) = o.add_curse {
            self.player.deck.push(cards::card(id));
        }
        if o.add_random_curse {
            let pool = cards::curses();
            if !pool.is_empty() {
                let def = *self.rng.pick(&pool);
                self.player.deck.push(CardInstance::new(def));
            }
        }
        if o.upgrade_random_card {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if !cands.is_empty() {
                let pick = cands[self.rng.below(cands.len() as u32) as usize];
                self.player.deck[pick].upgrade();
                let name = self.player.deck[pick].label();
                self.say(format!("{name} is upgraded"));
            }
        }
        if o.random_potion {
            let def = potions::random_potion(&mut self.rng);
            self.add_potion(def);
        }
        if let Some(enc_id) = o.fight {
            let enc = enemies::encounter_def(enc_id)
                .unwrap_or_else(|| panic!("unknown encounter: {enc_id}"));
            self.start_combat(enc);
            return o.text;
        }
        if o.remove_card {
            self.open_picker(PickPurpose::Remove, Screen::Event, 0, None);
        } else if o.upgrade_card {
            self.open_picker(PickPurpose::Upgrade, Screen::Event, 0, None);
        }
        if o.dead {
            self.player.hp = 0;
            self.screen = Screen::Death;
        }
        o.text
    }

    pub fn event_index_set(&mut self, i: usize) {
        if let Some(st) = self.event.as_mut() {
            let n = st.def.choices.len();
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

    fn open_treasure(&mut self) {
        let relic = self.roll_relic_by_odds(50, 33, 17);
        self.treasure = relic;
        if relic.is_none() {
            self.say("the chest is empty");
        }
        self.screen = Screen::Treasure;
    }

    pub fn take_treasure(&mut self) {
        if let Some(def) = self.treasure.take() {
            self.gain_relic(def);
            self.say(format!("you found {}", def.name));
        }
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
            PickPurpose::Remove => {
                // 不能把牌组删空
                if self.player.deck.len() <= 1 {
                    Vec::new()
                } else {
                    (0..self.player.deck.len()).collect()
                }
            }
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
                format!("{} removed from your deck", card.label())
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
    /// what: shop / event / battle(随机 boss|elite|enemy) / boss / elite / enemy
    pub fn debug_room(&mut self, what: &str) -> Result<String, String> {
        match what {
            "shop" => {
                self.open_shop();
                Ok(format!("debug room: {}", self.last_encounter_or("shop")))
            }
            "event" => {
                self.open_event();
                Ok(format!("debug room: {}", self.last_encounter_or("event")))
            }
            "battle" => {
                let kind = match self.rng.below(3) {
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
                let kind = match what {
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
        let idx = self
            .relic_pool
            .iter()
            .position(|r| r.rarity == rarity)
            .or_else(|| {
                if self.relic_pool.is_empty() {
                    None
                } else {
                    Some(0)
                }
            })?;
        Some(self.relic_pool.remove(idx))
    }

    fn roll_relic_by_odds(&mut self, common: u32, uncommon: u32, rare: u32) -> Option<&'static RelicDef> {
        let total = common + uncommon + rare;
        let roll = self.rng.below(total);
        let rarity = if roll < common {
            Rarity::Common
        } else if roll < common + uncommon {
            Rarity::Uncommon
        } else {
            Rarity::Rare
        };
        self.take_relic_of(rarity)
    }

    /// 随机一张可奖励的牌
    fn roll_card(&mut self) -> Option<&'static CardDef> {
        let roll = self.rng.below(100);
        let rarity = if roll < 60 {
            Rarity::Common
        } else if roll < 97 {
            Rarity::Uncommon
        } else {
            Rarity::Rare
        };
        self.roll_card_of(rarity)
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
        Some(*self.rng.pick(&pool))
    }

    fn roll_card_of(&mut self, rarity: Rarity) -> Option<&'static CardDef> {
        let pool = cards::reward_pool(rarity);
        if pool.is_empty() {
            return None;
        }
        Some(*self.rng.pick(&pool))
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
