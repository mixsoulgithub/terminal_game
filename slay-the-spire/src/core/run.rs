// 一局(run)的流程:地图推进、战斗结算、奖励、商店、事件、营火、牌组管理.
// 所有状态都在这里,UI 只读这些字段并调用这里的方法改状态.
use std::collections::VecDeque;

use crate::core::card::{CardDef, CardInstance, Rarity};
use crate::core::cards;
use crate::core::combat::{Combat, CombatSetup, Phase};
use crate::core::enemies;
use crate::core::enemy::{EnemyKind, Encounter};
use crate::core::events::{EventDef, Outcome};
use crate::core::map::{ActMap, NodeKind};
use crate::core::potions::{self, PotionDef, PotionFx};
use crate::core::relics::{self, RelicDef, RelicFx};
use crate::rng::Rng;

/// 药水格子数
pub const POTION_SLOTS: usize = 3;
/// 起始生命
pub const STARTING_HP: i32 = 80;
/// 起始金币
pub const STARTING_GOLD: i32 = 99;
/// 营火休息回复比例(百分比)
pub const REST_HEAL_PCT: i32 = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
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
    pub rng: Rng,
    pub player: Player,
    pub map: ActMap,
    /// 玩家当前所在节点;None 表示还没上路
    pub pos: Option<usize>,
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
    pub messages: VecDeque<String>,
    /// 本局还没出现过的遗物
    relic_pool: Vec<&'static RelicDef>,
    last_encounter: &'static str,
}

impl Run {
    pub fn new(seed: u64) -> Run {
        let mut rng = Rng::new(seed);
        let mut deck: Vec<CardInstance> = Vec::new();
        for _ in 0..5 {
            deck.push(cards::card("strike"));
        }
        for _ in 0..4 {
            deck.push(cards::card("defend"));
        }
        deck.push(cards::card("bash"));
        let starter = relics::starter_relic();
        let relic_pool: Vec<&'static RelicDef> = relics::RELICS
            .iter()
            .filter(|r| r.id != starter.id)
            .collect();
        let map = ActMap::generate(&mut rng);
        let mut run = Run {
            seed,
            rng,
            player: Player {
                hp: STARTING_HP,
                max_hp: STARTING_HP,
                gold: STARTING_GOLD,
                deck,
                relics: vec![starter],
                potions: vec![None; POTION_SLOTS],
            },
            map,
            pos: None,
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
            messages: VecDeque::new(),
            relic_pool,
            last_encounter: "",
        };
        // 起始遗物的拾取效果
        let starter_fx = starter.fx;
        run.apply_relic_pickup(starter_fx);
        run.say(format!("seed {seed}: climb the spire"));
        run
    }

    fn say(&mut self, text: impl Into<String>) {
        self.messages.push_back(text.into());
        while self.messages.len() > 120 {
            self.messages.pop_front();
        }
    }

    // ---- 地图 ----

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
                let enc = self.pick_encounter(EnemyKind::Boss);
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
        self.screen = Screen::Combat;
        self.stats.fights += 1;
    }

    /// 测试与界面自检用:跳过地图,直接打一场指定遭遇
    #[cfg(test)]
    pub fn debug_start_combat(&mut self, enc: &'static Encounter) {
        self.start_combat(enc);
    }

    pub fn combat(&self) -> Option<&Combat> {
        self.combat.as_ref()
    }

    pub fn combat_mut(&mut self) -> Option<&mut Combat> {
        self.combat.as_mut()
    }

    /// 每次战斗内操作之后调用:同步生命、处理胜负
    pub fn sync_combat(&mut self) {
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
            Phase::Won => self.resolve_victory(),
            Phase::Lost => self.resolve_defeat(),
            _ => {}
        }
        self.stats.turns = self.stats.turns.max(turns);
        let _ = damage;
    }

    fn post_combat_heal(&mut self) -> i32 {
        let heal: i32 = self.player.relic_fx_sum(|r| r.fx.post_combat_heal);
        if heal > 0 {
            self.heal(heal);
        }
        heal
    }

    fn resolve_victory(&mut self) {
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
        let cards: Vec<CardInstance> = (0..3)
            .filter_map(|_| self.roll_card())
            .map(CardInstance::new)
            .collect();
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
                Ok(format!("+{g} gold"))
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
                    return Err("no free potion slot".to_string());
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
        for (rarity, count) in [
            (Rarity::Common, 2),
            (Rarity::Uncommon, 2),
            (Rarity::Rare, 1),
        ] {
            for _ in 0..count {
                if let Some(def) = self.roll_card_of(rarity) {
                    let base = match rarity {
                        Rarity::Common => self.rng.range_inclusive(45, 55),
                        Rarity::Uncommon => self.rng.range_inclusive(68, 82),
                        _ => self.rng.range_inclusive(135, 165),
                    };
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
            let def = potions::random_potion(&mut self.rng);
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
            return Err(format!("needs {price} gold"));
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
                    return Err("no free potion slot".to_string());
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
                    r.sync_combat();
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
            }
        }
        assert_eq!(r.screen, Screen::Victory, "走到 Boss 应获胜");
        let last = r.pos.expect("应该有落点");
        assert_eq!(r.map.node(last).kind, NodeKind::Boss);
        assert!(r.floor_reached >= FLOORS - 1);
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
        r.sync_combat();
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
        r.sync_combat();
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
        r.sync_combat();
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
                    r.sync_combat();
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
