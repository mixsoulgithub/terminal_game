// 战斗引擎:回合流转、抽牌弃牌、卡牌结算、敌人行动.
// 纯逻辑,不碰终端;一局流程(run.rs)只负责在一次战斗前后同步生命与遗物结算.
use crate::core::card::{CardInstance, Cost, Effect, Target};
use crate::core::cards;
use crate::core::enemy::{Ai, EnemyDef, EnemyFx, EnemyKind, Encounter, Intent};
use crate::core::potions::{PotionDef, PotionFx};
use crate::core::relics::RelicDef;
use crate::core::status::{Status, Statuses};
use crate::rng::Rng;

/// 手牌上限
pub const HAND_LIMIT: usize = 10;
/// 每回合默认抽牌数
pub const DRAW_PER_TURN: usize = 5;
/// 每回合默认能量
pub const BASE_ENERGY: i32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    PlayerTurn,
    /// 敌人正在行动,期间不接受玩家操作
    EnemyTurn,
    Won,
    Lost,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    Info,
    Player,
    Enemy,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
    /// 递增序号:一局的"历史记录"靠它只抄一次日志
    pub seq: u64,
}

pub struct Enemy {
    pub def: &'static EnemyDef,
    /// 同名敌人会带 #1/#2 后缀
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub statuses: Statuses,
    /// 下一招在 def.moves 里的下标
    pub next_move: usize,
    /// 睡眠 AI 剩余回合数
    pub sleep_left: u8,
    pub awake: bool,
    /// 死亡触发是否已结算,避免重复触发
    pub death_done: bool,
}

impl Enemy {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    pub fn dead(&self) -> bool {
        self.hp <= 0
    }

    pub fn intent(&self) -> Intent {
        if self.sleep_left > 0 && !self.awake {
            return Intent::Sleep;
        }
        self.def.moves[self.next_move].intent()
    }
}

/// 玩家在战斗中的镜像:只带战斗需要的数据
#[derive(Clone, Debug)]
pub struct PlayerBattle {
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub statuses: Statuses,
}

/// 构造一场战斗需要的输入.拥有所有权,避免借用纠缠.
pub struct CombatSetup {
    pub hp: i32,
    pub max_hp: i32,
    pub deck: Vec<CardInstance>,
    pub relics: Vec<&'static RelicDef>,
}

/// 抖动:谁在抖、往哪边(负 = 左,正 = 右)。表现层取走后自己清空。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShakeWho {
    Hero,
    Enemy(usize),
}

/// 抖动的由来:出手(朝对面冲)还是挨打(往反方向退)。表现层据此决定先后。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShakeKind {
    Attack,
    Hurt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shake {
    pub who: ShakeWho,
    pub dir: i32,
    pub kind: ShakeKind,
    /// 挨打时的伤害量(出手事件是 0),表现层按它决定抖多大
    pub amount: i32,
}

/// 选择卡牌的来源
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceSource {
    Hand,
    Discard,
    Exhaust,
}

/// 选完之后干什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceAction {
    /// 消耗掉
    Exhaust,
    /// 复制一份
    Copy,
    /// 拿回手牌
    ToHand,
    /// 放到抽牌堆顶
    ToDrawTop,
    /// 直接移出这局(调试用)
    Remove,
}

/// 一次待选择:比如"从手牌选一张消耗"
pub struct Choice {
    pub source: ChoiceSource,
    pub action: ChoiceAction,
    /// 只能选攻击/能力牌(二重身)
    pub attack_or_power_only: bool,
    /// 一共要选几张(目前这些牌都只要选 1 张)
    pub need: usize,
    /// 是哪张牌引起的,信息栏提示用
    pub label: String,
    /// 还没收尾的那张牌 + 它花的能量:取消时原样退回
    pub played: Option<(CardInstance, i32)>,
}

pub struct Combat {
    /// 待选择(选牌窗口/手牌选择模式)
    pub choice: Option<Choice>,
    /// 这一帧攒下来的抖动事件,表现层消费
    pub shakes: Vec<Shake>,
    pub enemies: Vec<Enemy>,
    pub player: PlayerBattle,
    pub hand: Vec<CardInstance>,
    pub draw: Vec<CardInstance>,
    pub discard: Vec<CardInstance>,
    pub exhaust: Vec<CardInstance>,
    pub energy: i32,
    pub max_energy: i32,
    pub turn: u32,
    pub phase: Phase,
    pub log: Vec<LogLine>,
    pub rng: Rng,
    pub encounter_id: &'static str,
    pub kind: EnemyKind,
    /// 本场对敌人造成的总伤害,结算界面用
    pub damage_dealt: i32,
    /// 已经产出的日志条数(日志会截断,所以用序号而不是长度)
    pub log_seq: u64,
    relic_thorns: i32,
}

/// 单次打牌过程中的临时统计
#[derive(Default)]
struct PlayCtx {
    x: i32,
    exhausted: i32,
    unblocked: i32,
}

impl Combat {
    pub fn new(enc: &'static Encounter, setup: CombatSetup, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut enemies = Vec::new();
        for (i, id) in enc.enemies.iter().enumerate() {
            let def = crate::core::enemies::enemy_def_or_panic(id);
            // 同名敌人加编号,保证日志与选中项能对上
            let dup = enc.enemies.iter().filter(|e| *e == id).count() > 1;
            let name = if dup {
                format!("{} #{}", def.name, i + 1)
            } else {
                def.name.to_string()
            };
            let hp = rng.range_inclusive(def.hp.0, def.hp.1);
            let mut statuses = Statuses::new();
            for (s, n) in def.innate {
                statuses.add(*s, *n);
            }
            let sleep_left = match def.ai {
                Ai::Sleep { turns, .. } => turns,
                _ => 0,
            };
            enemies.push(Enemy {
                def,
                name,
                hp,
                max_hp: hp,
                block: 0,
                statuses,
                next_move: 0,
                sleep_left,
                awake: sleep_left == 0,
                death_done: false,
            });
        }

        // 战斗开始时洗牌,天生牌直接入手
        let mut deck = setup.deck;
        rng.shuffle(&mut deck);
        let (innate, rest): (Vec<CardInstance>, Vec<CardInstance>) =
            deck.into_iter().partition(|c| c.is_innate());

        let mut c = Combat {
            choice: None,
            shakes: Vec::new(),
            enemies,
            player: PlayerBattle {
                hp: setup.hp,
                max_hp: setup.max_hp,
                block: 0,
                statuses: Statuses::new(),
            },
            hand: innate,
            draw: rest,
            discard: Vec::new(),
            exhaust: Vec::new(),
            energy: BASE_ENERGY,
            max_energy: BASE_ENERGY,
            turn: 0,
            phase: Phase::PlayerTurn,
            log: Vec::new(),
            rng,
            encounter_id: enc.id,
            kind: enc.kind,
            damage_dealt: 0,
            log_seq: 0,
            relic_thorns: 0,
        };

        // 遗物:战斗开始结算
        let mut extra_energy = 0;
        let mut extra_draw = 0usize;
        let mut start_heal = 0;
        for r in &setup.relics {
            let fx = r.fx;
            extra_energy += fx.combat_start_energy;
            extra_draw += fx.combat_start_draw.max(0) as usize;
            c.relic_thorns += fx.thorns;
            start_heal += fx.combat_start_heal;
            if fx.combat_start_strength != 0 {
                c.player
                    .statuses
                    .add(Status::Strength, fx.combat_start_strength);
            }
            if fx.combat_start_dexterity != 0 {
                c.player
                    .statuses
                    .add(Status::Dexterity, fx.combat_start_dexterity);
            }
            if fx.combat_start_block != 0 {
                c.player.block += fx.combat_start_block;
            }
        }
        c.max_energy = BASE_ENERGY + extra_energy;
        if start_heal > 0 {
            c.heal_player(start_heal);
        }
        let summary = c.enemy_summary();
        c.push_log(LogKind::Info, format!("Combat begins: {summary}"));
        if extra_energy > 0 {
            c.push_log(
                LogKind::Info,
                format!("relics grant +{extra_energy} energy this combat"),
            );
        }
        c.start_turn(extra_draw);
        c
    }

    fn enemy_summary(&self) -> String {
        self.enemies
            .iter()
            .map(|e| format!("{} ({}/{})", e.name, e.hp, e.max_hp))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn alive_enemies(&self) -> Vec<usize> {
        self.enemies
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive())
            .map(|(i, _)| i)
            .collect()
    }

    pub fn first_alive(&self) -> Option<usize> {
        self.enemies.iter().position(|e| e.alive())
    }

    fn push_log(&mut self, kind: LogKind, text: String) {
        self.log_seq += 1;
        self.log.push(LogLine {
            kind,
            text,
            seq: self.log_seq,
        });
        if self.log.len() > 200 {
            self.log.drain(0..100);
        }
    }

    // ---- 回合流转 ----

    fn start_turn(&mut self, extra_draw: usize) {
        self.turn += 1;
        // 上回合"本回合 0 费"的牌恢复原价
        for card in self.hand.iter_mut() {
            card.free_this_turn = false;
        }
        self.energy = self.max_energy;
        // 格挡在回合开始清空,除非有壁垒
        if !self.player.statuses.has(Status::Barricade) {
            self.player.block = 0;
        }
        let berserk = self.player.statuses.get(Status::Berserk);
        if berserk > 0 {
            self.energy += berserk;
        }
        let demon = self.player.statuses.get(Status::DemonForm);
        if demon > 0 {
            self.player.statuses.add(Status::Strength, demon);
            self.push_log(
                LogKind::Player,
                format!("Demon Form: +{demon} Strength"),
            );
        }
        self.push_log(LogKind::Info, format!("-- Turn {} --", self.turn));
        self.draw_cards(DRAW_PER_TURN + extra_draw);
        let brutal = self.player.statuses.get(Status::Brutality);
        if brutal > 0 {
            // 掉血来自能力而不是卡牌,所以不触发渴望
            self.lose_hp_player(1, false);
            self.draw_cards(brutal as usize);
        }
    }

    /// 抽牌;抽牌堆空了就把弃牌堆洗回来
    pub fn draw_cards(&mut self, n: usize) {
        for _ in 0..n {
            if self.hand.len() >= HAND_LIMIT {
                self.push_log(LogKind::Info, format!("hand is full ({HAND_LIMIT})"));
                return;
            }
            if self.draw.is_empty() {
                if self.discard.is_empty() {
                    return;
                }
                self.draw = std::mem::take(&mut self.discard);
                let count = self.draw.len();
                self.rng.shuffle(&mut self.draw);
                self.push_log(
                    LogKind::Info,
                    format!("shuffled {count} cards into the draw pile"),
                );
            }
            let card = self.draw.pop().unwrap();
            self.hand.push(card);
            self.on_status_drawn();
        }
    }

    /// 抽到状态牌时触发的能力(进化、吐火)
    fn on_status_drawn(&mut self) {
        let is_status = self
            .hand
            .last()
            .map(|c| c.kind() == crate::core::card::CardType::Status)
            .unwrap_or(false);
        if !is_status {
            return;
        }
        let fire = self.player.statuses.get(Status::FireBreathing);
        if fire > 0 {
            for i in self.alive_enemies() {
                self.damage_enemy(i, fire);
            }
            self.push_log(
                LogKind::Player,
                format!("Fire Breathing deals {fire} to all enemies"),
            );
            self.settle_deaths();
        }
        let evolve = self.player.statuses.get(Status::Evolve);
        if evolve > 0 {
            // 递归深度受手牌上限约束
            self.draw_cards(evolve as usize);
        }
    }

    /// 消耗一张牌,并结算"消耗时"的能力
    fn exhaust_card(&mut self, card: CardInstance) {
        let back: i32 = card
            .effects()
            .iter()
            .map(|e| match *e {
                Effect::EnergyOnExhaust { n } => n,
                _ => 0,
            })
            .sum();
        if back > 0 {
            self.energy += back;
            self.push_log(LogKind::Player, format!("exhausted: +{back} energy"));
        }
        self.exhaust.push(card);
        let fnp = self.player.statuses.get(Status::FeelNoPain);
        if fnp > 0 {
            self.gain_block(fnp, false);
        }
        let dark = self.player.statuses.get(Status::DarkEmbrace);
        if dark > 0 {
            self.draw_cards(dark as usize);
        }
    }

    pub fn end_turn(&mut self) {
        if self.phase != Phase::PlayerTurn {
            return;
        }
        let metal = self.player.statuses.get(Status::Metallicize);
        if metal > 0 {
            self.gain_block(metal, false);
        }
        let combust = self.player.statuses.get(Status::Combust);
        if combust > 0 {
            self.lose_hp_player(1, false);
            let n = self.enemies.len();
            for i in 0..n {
                if self.damage_enemy(i, combust) > 0 {
                    self.push_log(LogKind::Player, format!("Combust: {combust} to all enemies"));
                }
            }
        }
        let regen = self.player.statuses.get(Status::Regenerate);
        if regen > 0 {
            self.heal_player(regen);
        }
        // 手牌:保留牌留下,虚灵牌消耗,其余进弃牌堆
        let hand = std::mem::take(&mut self.hand);
        for card in hand {
            if card.is_retain() {
                self.hand.push(card);
            } else if card.is_ethereal() {
                self.exhaust_card(card);
            } else {
                self.discard.push(card);
            }
        }
        // 玩家的减益在自己回合结束时递减
        self.player.statuses.decay_debuffs();
        self.phase = Phase::EnemyTurn;
        self.enemy_turn();
    }

    fn enemy_turn(&mut self) {
        for idx in self.alive_enemies() {
            if self.phase != Phase::EnemyTurn {
                return;
            }
            self.enemy_act(idx);
        }
        if self.phase != Phase::EnemyTurn {
            return;
        }
        self.phase = Phase::PlayerTurn;
        self.start_turn(0);
    }

    fn enemy_act(&mut self, idx: usize) {
        let move_idx = self.enemies[idx].next_move;
        let def = self.enemies[idx].def;
        let effects = def.moves[move_idx].effects;
        let mname = def.moves[move_idx].name;
        let name = self.enemies[idx].name.clone();
        if self.enemies[idx].sleep_left > 0 && !self.enemies[idx].awake {
            self.push_log(LogKind::Enemy, format!("{name} is asleep ({mname})"));
        } else {
            for fx in effects {
                match *fx {
                    EnemyFx::Attack { amount, times } => {
                        self.shake(ShakeWho::Enemy(idx), -1, ShakeKind::Attack, 0);
                        let per = self.enemy_attack_damage(idx, amount);
                        let mut blocked_total = 0;
                        for _ in 0..times.max(1) {
                            let (taken, blocked) = self.hit_player(per);
                            blocked_total += blocked;
                            if taken > 0 {
                                self.push_log(
                                    LogKind::Enemy,
                                    format!("{name} uses {mname}: {taken} damage"),
                                );
                            }
                            if self.phase == Phase::Lost {
                                return;
                            }
                        }
                        if blocked_total > 0 {
                            self.push_log(
                                LogKind::Info,
                                format!("{name} hit into {blocked_total} block"),
                            );
                        }
                        // 荆棘反伤
                        if self.relic_thorns > 0 {
                            let thorns = self.relic_thorns;
                            self.damage_enemy(idx, thorns);
                            self.push_log(
                                LogKind::Player,
                                format!("Thorns deal {thorns} to {name}"),
                            );
                            self.settle_deaths();
                        }
                    }
                    EnemyFx::Block { amount } => {
                        self.enemies[idx].block += amount;
                        self.push_log(
                            LogKind::Enemy,
                            format!("{name} gains {amount} Block ({mname})"),
                        );
                    }
                    EnemyFx::GainStatus { status, n } => {
                        self.enemies[idx].statuses.add(status, n);
                        self.push_log(
                            LogKind::Enemy,
                            format!("{name} gains {n} {}", status.name()),
                        );
                    }
                    EnemyFx::PlayerStatus { status, n } => {
                        self.player.statuses.add(status, n);
                        self.push_log(
                            LogKind::Enemy,
                            format!("{name} applies {n} {} to you", status.name()),
                        );
                    }
                }
            }
        }
        // 回合结束的敌人能力
        let ritual = self.enemies[idx].statuses.get(Status::Ritual);
        if ritual > 0 {
            self.enemies[idx].statuses.add(Status::Strength, ritual);
            self.push_log(
                LogKind::Enemy,
                format!("{name} channels Ritual: +{ritual} Strength"),
            );
        }
        self.enemies[idx].statuses.decay_debuffs();
        self.pick_next_move(idx);
    }

    /// 选出下一招
    fn pick_next_move(&mut self, idx: usize) {
        let def = self.enemies[idx].def;
        let len = def.moves.len();
        let cur = self.enemies[idx].next_move;
        match def.ai {
            Ai::Cycle => {
                self.enemies[idx].next_move = (cur + 1) % len;
            }
            Ai::Random {
                weights,
                no_repeat,
            } => {
                let pick = if no_repeat && len > 1 {
                    let mut w = weights.to_vec();
                    if cur < w.len() {
                        w[cur] = 0;
                    }
                    if w.iter().all(|x| *x == 0) {
                        self.rng.below(len as u32) as usize
                    } else {
                        self.rng.weighted_idx(&w).unwrap_or(cur)
                    }
                } else {
                    self.rng.weighted_idx(weights).unwrap_or(0)
                };
                self.enemies[idx].next_move = pick.min(len - 1);
            }
            Ai::Sleep { wake, .. } => {
                let left = self.enemies[idx].sleep_left;
                if left > 1 {
                    self.enemies[idx].sleep_left = left - 1;
                    self.enemies[idx].next_move = 0;
                } else {
                    self.enemies[idx].sleep_left = 0;
                    self.enemies[idx].awake = true;
                    self.enemies[idx].next_move = wake.min(len - 1);
                }
            }
        }
    }

    // ---- 数值结算 ----

    fn gain_block(&mut self, amount: i32, doubled: bool) {
        let mut n = amount.max(0);
        if !doubled {
            n += self.player.statuses.get(Status::Dexterity);
            if self.player.statuses.has(Status::Frail) {
                n = (n as f32 * 0.75).floor() as i32;
            }
        }
        if n <= 0 {
            return;
        }
        self.player.block += n;
        let jug = self.player.statuses.get(Status::Juggernaut);
        if jug > 0 {
            if let Some(t) = self.pick_random_alive() {
                self.damage_enemy(t, jug);
                self.push_log(LogKind::Player, format!("Juggernaut: {jug} damage"));
            }
        }
    }

    fn heal_player(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        let before = self.player.hp;
        self.player.hp = (self.player.hp + amount).min(self.player.max_hp);
        let healed = self.player.hp - before;
        if healed > 0 {
            self.push_log(LogKind::Player, format!("you heal {healed} HP"));
        }
    }

    /// 调试用:开一个"从手牌里删牌"的选择
    pub fn debug_begin_hand_remove(&mut self) {
        self.begin_choice(
            ChoiceSource::Hand,
            ChoiceAction::Remove,
            false,
            "remove a card from your hand",
        );
    }

    /// 开一次选牌:记下来,等界面那边选完再 choose()
    fn begin_choice(
        &mut self,
        source: ChoiceSource,
        action: ChoiceAction,
        attack_or_power_only: bool,
        label: &str,
    ) {
        self.choice = Some(Choice {
            source,
            action,
            attack_or_power_only,
            need: 1,
            label: label.to_string(),
            played: None,
        });
    }

    /// 这一堆里现在能选的卡(索引 + 卡)
    pub fn choice_candidates(&self) -> Vec<(usize, &CardInstance)> {
        let Some(ch) = self.choice.as_ref() else {
            return Vec::new();
        };
        let pile: &[CardInstance] = match ch.source {
            ChoiceSource::Hand => &self.hand,
            ChoiceSource::Discard => &self.discard,
            ChoiceSource::Exhaust => &self.exhaust,
        };
        pile.iter()
            .enumerate()
            .filter(|(_, c)| {
                !ch.attack_or_power_only
                    || matches!(
                        c.kind(),
                        crate::core::card::CardType::Attack | crate::core::card::CardType::Power
                    )
            })
            .collect()
    }

    /// 选完了:执行动作,然后把刚才那张牌按原样收尾(消耗/弃掉)
    pub fn choose(&mut self, idx: usize) -> Result<(), String> {
        if !self.choice_candidates().iter().any(|(i, _)| *i == idx) {
            return Err("that card cannot be chosen".to_string());
        }
        let Some(mut ch) = self.choice.take() else {
            return Err("nothing to choose".to_string());
        };
        match (ch.source, ch.action) {
            (ChoiceSource::Hand, ChoiceAction::Exhaust) => {
                let card = self.hand.remove(idx);
                self.exhaust_card(card);
            }
            (ChoiceSource::Hand, ChoiceAction::Copy) => {
                let card = self.hand[idx].clone();
                self.hand.push(card);
            }
            (ChoiceSource::Hand, ChoiceAction::ToDrawTop) => {
                let card = self.hand.remove(idx);
                self.draw.insert(0, card);
            }
            (ChoiceSource::Exhaust, ChoiceAction::ToHand) => {
                let card = self.exhaust.remove(idx);
                self.hand.push(card);
            }
            (ChoiceSource::Discard, ChoiceAction::ToDrawTop) => {
                let card = self.discard.remove(idx);
                self.draw.insert(0, card);
            }
            (ChoiceSource::Hand, ChoiceAction::Remove) => {
                self.hand.remove(idx);
            }
            _ => {}
        }
        self.finish_played(ch.played.take());
        Ok(())
    }

    /// 取消这次出牌:能量退回、牌回手牌
    pub fn cancel_choice(&mut self) {
        let Some(mut ch) = self.choice.take() else {
            return;
        };
        if let Some((card, cost)) = ch.played.take() {
            self.energy += cost;
            self.hand.push(card);
        }
    }

    /// 出牌收尾:该消耗的消耗,该弃的弃
    fn finish_played(&mut self, played: Option<(CardInstance, i32)>) {
        let Some((card, _)) = played else {
            return;
        };
        let corrupted_skill = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        let exhaust_self = card.is_exhaust()
            || card.effects().contains(&Effect::ExhaustSelf)
            || corrupted_skill;
        if exhaust_self {
            self.exhaust_card(card);
        } else {
            self.discard.push(card);
        }
    }

    /// 记一次抖动:谁、往哪边(负左正右)、出手还是挨打
    fn shake(&mut self, who: ShakeWho, dir: i32, kind: ShakeKind, amount: i32) {
        self.shakes.push(Shake {
            who,
            dir,
            kind,
            amount,
        });
    }

    /// 玩家直接掉血(不吃格挡);from_card 用于渴望的触发判断
    fn lose_hp_player(&mut self, amount: i32, from_card: bool) {
        if amount <= 0 {
            return;
        }
        self.player.hp -= amount;
        self.shake(ShakeWho::Hero, -1, ShakeKind::Hurt, amount);
        self.note_hp_loss();
        let rupt = self.player.statuses.get(Status::Rupture);
        if from_card && rupt > 0 {
            self.player.statuses.add(Status::Strength, rupt);
        }
        if self.player.hp <= 0 {
            self.player.hp = 0;
            self.phase = Phase::Lost;
            self.push_log(LogKind::Info, "you have been defeated".to_string());
        }
    }

    /// 敌人打玩家一次;返回(实际掉血, 被格挡量)
    fn hit_player(&mut self, damage: i32) -> (i32, i32) {
        let dmg = damage.max(0);
        let blocked = self.player.block.min(dmg);
        self.player.block -= blocked;
        let taken = dmg - blocked;
        if taken > 0 {
            self.player.hp -= taken;
            self.shake(ShakeWho::Hero, -1, ShakeKind::Hurt, taken);
            self.note_hp_loss();
        }
        if self.player.hp <= 0 {
            self.player.hp = 0;
            self.phase = Phase::Lost;
            self.push_log(LogKind::Info, "you have been defeated".to_string());
        }
        (taken, blocked)
    }

    /// 玩家攻击一次的计算:力量、虚弱、目标易伤
    fn player_attack_damage(&self, raw: i32, target: usize) -> i32 {
        let mut d = raw + self.player.statuses.get(Status::Strength);
        if self.player.statuses.has(Status::Weak) {
            d = (d as f32 * 0.75).floor() as i32;
        }
        if self.enemies[target].statuses.has(Status::Vulnerable) {
            d = (d as f32 * 1.5).floor() as i32;
        }
        d.max(0)
    }

    /// 敌人攻击一次的计算
    fn enemy_attack_damage(&self, idx: usize, raw: i32) -> i32 {
        let mut d = raw + self.enemies[idx].statuses.get(Status::Strength);
        if self.enemies[idx].statuses.has(Status::Weak) {
            d = (d as f32 * 0.75).floor() as i32;
        }
        if self.player.statuses.has(Status::Vulnerable) {
            d = (d as f32 * 1.5).floor() as i32;
        }
        d.max(0)
    }

    /// UI 用:敌人下一招的显示数值(伤害已计入增减益)
    pub fn predicted_damage(&self, idx: usize) -> (i32, u8) {
        match self.enemies[idx].intent() {
            Intent::Attack { damage, times }
            | Intent::AttackDebuff { damage, times }
            | Intent::AttackDefend { damage, times, .. } => {
                (self.enemy_attack_damage(idx, damage), times)
            }
            _ => (0, 0),
        }
    }

    /// UI 用:敌人下一招的格挡量
    pub fn intent_block(&self, idx: usize) -> i32 {
        match self.enemies[idx].intent() {
            Intent::AttackDefend { block, .. } => block,
            Intent::Defend => {
                let mut total = 0;
                for fx in self.enemies[idx].def.moves[self.enemies[idx].next_move].effects {
                    if let EnemyFx::Block { amount } = fx {
                        total += *amount;
                    }
                }
                total
            }
            _ => 0,
        }
    }

    /// 玩家掉了血:嗜血的费用跟着降(手牌/抽牌堆/弃牌堆/消耗堆里那些)
    fn note_hp_loss(&mut self) {
        for card in self
            .hand
            .iter_mut()
            .chain(self.draw.iter_mut())
            .chain(self.discard.iter_mut())
            .chain(self.exhaust.iter_mut())
        {
            if card.def.id == "blood_for_blood" {
                card.cost_delta -= 1;
            }
        }
    }

    /// 这个敌人这回合是不是要攻击(观察弱点用)
    fn enemy_intends_attack(&self, idx: usize) -> bool {
        use crate::core::enemy::Intent;
        let Some(e) = self.enemies.get(idx) else {
            return false;
        };
        let Some(m) = e.def.moves.get(e.next_move) else {
            return false;
        };
        matches!(
            m.intent(),
            Intent::Attack { .. } | Intent::AttackDefend { .. } | Intent::AttackDebuff { .. }
        )
    }

    /// 打敌人:damage 是算好的最终值;返回扣格挡后实际造成的伤害
    fn damage_enemy(&mut self, idx: usize, damage: i32) -> i32 {
        if idx >= self.enemies.len() || self.enemies[idx].dead() {
            return 0;
        }
        let dmg = damage.max(0);
        let blocked = self.enemies[idx].block.min(dmg);
        self.enemies[idx].block -= blocked;
        let taken = dmg - blocked;
        if taken > 0 {
            self.enemies[idx].hp -= taken;
            self.damage_dealt += taken;
            self.shake(ShakeWho::Enemy(idx), 1, ShakeKind::Hurt, taken);
        }
        // 睡眠中的敌人被打醒
        if taken > 0 && !self.enemies[idx].awake && self.enemies[idx].sleep_left > 0 {
            if let Ai::Sleep { wake, .. } = self.enemies[idx].def.ai {
                let e = &mut self.enemies[idx];
                e.sleep_left = 0;
                e.awake = true;
                e.next_move = wake.min(e.def.moves.len() - 1);
                let name = e.name.clone();
                self.push_log(LogKind::Info, format!("{name} wakes up!"));
            }
        }
        taken
    }

    /// 结算本回合新死的敌人(死亡触发只在第一次结算)
    fn settle_deaths(&mut self) {
        for i in 0..self.enemies.len() {
            if self.enemies[i].dead() && !self.enemies[i].death_done {
                self.enemies[i].death_done = true;
                let on_death = self.enemies[i].def.on_death;
                if on_death.is_empty() {
                    continue;
                }
                let name = self.enemies[i].name.clone();
                for fx in on_death {
                    match *fx {
                        EnemyFx::PlayerStatus { status, n } => {
                            self.player.statuses.add(status, n);
                            self.push_log(
                                LogKind::Enemy,
                                format!("{name} bursts: you gain {n} {}", status.name()),
                            );
                        }
                        EnemyFx::Attack { amount, .. } => {
                            let (taken, _) = self.hit_player(amount);
                            self.push_log(
                                LogKind::Enemy,
                                format!("{name} bursts for {taken} damage"),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn check_win(&mut self) {
        if self.phase == Phase::Lost {
            return;
        }
        self.settle_deaths();
        if self.alive_enemies().is_empty() {
            self.phase = Phase::Won;
            self.push_log(LogKind::Info, "victory".to_string());
        }
    }

    // ---- 打牌 ----

    /// 这张牌能不能打;不能则给出原因
    pub fn playable(&self, hand_idx: usize) -> Result<(), &'static str> {
        if self.phase != Phase::PlayerTurn {
            return Err("not your turn");
        }
        let Some(card) = self.hand.get(hand_idx) else {
            return Err("no such card");
        };
        if !card.playable() {
            return Err("unplayable");
        }
        // 冲撞:手里只要有一张不是攻击牌就打不出去
        if card.def.id == "clash"
            && self
                .hand
                .iter()
                .any(|c| c.kind() != crate::core::card::CardType::Attack)
        {
            return Err("clash needs a hand of only attacks");
        }
        // 腐化:技能都是 0 费,所以这里不能按原价拦
        let corrupted = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        if !corrupted && card.cost_value(self.energy) > self.energy {
            return Err("not enough energy");
        }
        if card.kind() == crate::core::card::CardType::Attack
            && self.player.statuses.has(Status::Entangled)
        {
            return Err("entangled: no attacks");
        }
        if card.needs_target() && self.first_alive().is_none() {
            return Err("no target");
        }
        Ok(())
    }

    /// UI 用:返回不可打出的原因,可打出时返回 None
    pub fn blocked_reason(&self, hand_idx: usize) -> Option<&'static str> {
        self.playable(hand_idx).err()
    }

    /// 打出第 hand_idx 张牌,target 是敌人下标(需要目标时)
    pub fn play_card(&mut self, hand_idx: usize, target: Option<usize>) -> Result<(), &'static str> {
        self.playable(hand_idx)?;
        let mut card = self.hand.remove(hand_idx);
        let corrupted_skill = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        let cost = if corrupted_skill { 0 } else { card.cost_value(self.energy) };
        self.energy -= cost.min(self.energy);
        let is_x = card.cost() == Cost::X;
        let mut ctx = PlayCtx {
            x: if is_x { cost } else { 0 },
            ..Default::default()
        };
        let chosen = match card.target() {
            Target::Enemy => Some(
                target
                    .filter(|i| self.enemies.get(*i).map(|e| e.alive()).unwrap_or(false))
                    .or_else(|| self.first_alive())
                    .unwrap_or(0),
            ),
            Target::Random => self.pick_random_alive(),
            _ => None,
        };
        let label = card.label();
        let is_skill = card.kind() == crate::core::card::CardType::Skill;
        if card.kind() == crate::core::card::CardType::Attack {
            self.shake(ShakeWho::Hero, 1, ShakeKind::Attack, 0);
        }
        self.push_log(LogKind::Player, format!("you play {label}"));
        let blocked_before = self.player.block;
        self.resolve(&mut card, chosen, &mut ctx);
        // 双发:这一击再打一次
        if card.kind() == crate::core::card::CardType::Attack {
            let dt = self.player.statuses.get(Status::DoubleTap);
            if dt > 0 {
                self.player.statuses.add(Status::DoubleTap, -1);
                self.resolve(&mut card, chosen, &mut ctx);
            }
            let rage = self.player.statuses.get(Status::Rage);
            if rage > 0 {
                self.gain_block(rage, false);
            }
        }
        if self.player.block > blocked_before {
            let gained = self.player.block - blocked_before;
            self.push_log(LogKind::Player, format!("you gain {gained} Block"));
        }
        if ctx.unblocked > 0 {
            self.push_log(LogKind::Info, format!("dealt {} damage", ctx.unblocked));
        }
        // 有选牌待定:牌和花的能量先存着,等选完(choose)或取消(cancel)再收尾
        if let Some(ch) = self.choice.as_mut() {
            ch.played = Some((card, cost));
            return Ok(());
        }
        // 结算完后决定去处
        let exhaust_self = card.is_exhaust()
            || card.effects().contains(&Effect::ExhaustSelf)
            || corrupted_skill;
        if exhaust_self {
            self.exhaust_card(card);
        } else {
            self.discard.push(card);
        }
        // 怒意:玩家打出技能时敌人获得力量
        if is_skill {
            for i in 0..self.enemies.len() {
                if self.enemies[i].alive() {
                    let enrage = self.enemies[i].statuses.get(Status::Enrage);
                    if enrage > 0 {
                        self.enemies[i].statuses.add(Status::Strength, enrage);
                        let name = self.enemies[i].name.clone();
                        self.push_log(
                            LogKind::Enemy,
                            format!("{name}'s Enrage: +{enrage} Strength"),
                        );
                    }
                }
            }
        }
        self.check_win();
        Ok(())
    }

    fn pick_random_alive(&mut self) -> Option<usize> {
        let alive = self.alive_enemies();
        if alive.is_empty() {
            return None;
        }
        Some(alive[self.rng.below(alive.len() as u32) as usize])
    }

    fn resolve(&mut self, card: &mut CardInstance, target: Option<usize>, ctx: &mut PlayCtx) {
        let effects = card.effects();
        let is_strike = card.is_strike();
        let card_bonus = card.bonus;
        for e in effects {
            match *e {
                Effect::Damage { amount, times } => {
                    if let Some(t) = target {
                        for _ in 0..times.max(1) {
                            if self.enemies[t].dead() {
                                break;
                            }
                            let d = self.player_attack_damage(amount, t);
                            ctx.unblocked += self.damage_enemy(t, d);
                        }
                    }
                }
                Effect::DamageAll { amount, times } => {
                    for _ in 0..times.max(1) {
                        for t in self.alive_enemies() {
                            let d = self.player_attack_damage(amount, t);
                            ctx.unblocked += self.damage_enemy(t, d);
                        }
                    }
                }
                Effect::DamageRandom { amount, times } => {
                    for _ in 0..times.max(1) {
                        let Some(t) = self.pick_random_alive() else {
                            break;
                        };
                        let d = self.player_attack_damage(amount, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }
                Effect::DamageEqualBlock => {
                    if let Some(t) = target {
                        let raw = self.player.block;
                        let d = self.player_attack_damage(raw, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }
                Effect::DamageWithBonus { amount, times } => {
                    if let Some(t) = target {
                        let raw = amount + card_bonus;
                        for _ in 0..times.max(1) {
                            if self.enemies[t].dead() {
                                break;
                            }
                            let d = self.player_attack_damage(raw, t);
                            ctx.unblocked += self.damage_enemy(t, d);
                        }
                    }
                }
                Effect::DamagePerStrike { base, per } => {
                    if let Some(t) = target {
                        let mut n = 0;
                        for pile in [&self.hand, &self.draw, &self.discard, &self.exhaust] {
                            n += pile.iter().filter(|c| c.is_strike()).count() as i32;
                        }
                        if is_strike {
                            n += 1;
                        }
                        let d = self.player_attack_damage(base + per * n, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }                Effect::DamagePerExhausted { per } => {
                    if let Some(t) = target {
                        let d = self.player_attack_damage(per * ctx.exhausted, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }
                Effect::DamageAllX { per } => {
                    let raw = per * ctx.x;
                    if raw > 0 {
                        for t in self.alive_enemies() {
                            let d = self.player_attack_damage(raw, t);
                            ctx.unblocked += self.damage_enemy(t, d);
                        }
                    }
                }
                Effect::DamageIfVulnerable {
                    amount,
                    energy,
                    draw,
                } => {
                    if let Some(t) = target {
                        let vuln = self.enemies[t].statuses.has(Status::Vulnerable);
                        let d = self.player_attack_damage(amount, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                        if vuln && self.enemies[t].alive() {
                            self.energy += energy;
                            self.draw_cards(draw as usize);
                        }
                    }
                }
                Effect::DamageAndKillMaxHp {
                    amount,
                    times,
                    max_hp,
                } => {
                    if let Some(t) = target {
                        let before = self.enemies[t].hp;
                        for _ in 0..times.max(1) {
                            if self.enemies[t].dead() {
                                break;
                            }
                            let d = self.player_attack_damage(amount, t);
                            ctx.unblocked += self.damage_enemy(t, d);
                        }
                        if before > 0 && self.enemies[t].dead() {
                            self.player.max_hp += max_hp;
                            self.player.hp += max_hp;
                            self.push_log(
                                LogKind::Player,
                                format!("you devour it: +{max_hp} max HP"),
                            );
                        }
                    }
                }
                Effect::DamageStrengthMult { amount, mult } => {
                    if let Some(t) = target {
                        let raw = amount + self.player.statuses.get(Status::Strength) * mult;
                        let mut d = raw;
                        if self.player.statuses.has(Status::Weak) {
                            d = (d as f32 * 0.75).floor() as i32;
                        }
                        if self.enemies[t].statuses.has(Status::Vulnerable) {
                            d = (d as f32 * 1.5).floor() as i32;
                        }
                        ctx.unblocked += self.damage_enemy(t, d.max(0));
                    }
                }
                Effect::Reaper { amount } => {
                    let mut total = 0;
                    for t in self.alive_enemies() {
                        let d = self.player_attack_damage(amount, t);
                        total += self.damage_enemy(t, d);
                    }
                    ctx.unblocked += total;
                    if total > 0 {
                        self.heal_player(total);
                    }
                }
                Effect::BonusSelf { n } => {
                    card.bonus += n;
                }
                Effect::Block { amount } => {
                    self.gain_block(amount, false);
                }
                Effect::BlockPerExhausted { per } => {
                    self.gain_block(per * ctx.exhausted, false);
                }
                Effect::DoubleBlock => {
                    let b = self.player.block;
                    self.gain_block(b, true);
                }                Effect::LoseHp { amount } => {
                    self.lose_hp_player(amount, true);
                }
                Effect::GainEnergy { n } => {
                    self.energy = (self.energy + n).max(0);
                }
                Effect::Draw { n } => {
                    self.draw_cards(n as usize);
                }
                Effect::AddSelfStatus { status, n } => {
                    self.player.statuses.add(status, n);
                }
                Effect::AddTargetStatus { status, n } => {
                    if let Some(t) = target {
                        if self.enemies[t].alive() {
                            self.enemies[t].statuses.add(status, n);
                        }
                    }
                }
                Effect::AddAllEnemiesStatus { status, n } => {
                    for i in self.alive_enemies() {
                        self.enemies[i].statuses.add(status, n);
                    }
                }
                Effect::DoubleSelfStatus(status) => {
                    self.player.statuses.double(status);
                }
                Effect::ExhaustHand => {
                    let hand = std::mem::take(&mut self.hand);
                    let mut n = 0;
                    for c in hand {
                        n += 1;
                        self.exhaust_card(c);
                    }
                    ctx.exhausted += n;
                    self.push_log(LogKind::Info, format!("{n} cards are exhausted"));
                }
                Effect::ExhaustRandomInHand { n } => {
                    for _ in 0..n {
                        if self.hand.is_empty() {
                            break;
                        }
                        let i = self.rng.below(self.hand.len() as u32) as usize;
                        let c = self.hand.remove(i);
                        ctx.exhausted += 1;
                        self.exhaust_card(c);
                    }
                }
                Effect::ExhaustNonAttacks { damage } => {
                    let hand = std::mem::take(&mut self.hand);
                    let mut kept = Vec::new();
                    let mut n = 0;
                    for c in hand {
                        if c.kind() == crate::core::card::CardType::Attack {
                            kept.push(c);
                        } else {
                            n += 1;
                            self.exhaust_card(c);
                        }
                    }
                    self.hand = kept;
                    ctx.exhausted += n;
                    if let Some(t) = target {
                        let d = self.player_attack_damage(damage, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }
                Effect::ExhaustSelf => {
                    // 去处由 play_card 统一处理
                }                Effect::AddCardToDraw { id, n } => {
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        self.draw.push(CardInstance::new(def));
                    }
                }
                Effect::ExhaustFromHand => {
                    self.begin_choice(ChoiceSource::Hand, ChoiceAction::Exhaust, false, "exhaust a card");
                }
                Effect::TopFromHand => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::ToDrawTop,
                        false,
                        "put a card on top of the draw pile",
                    );
                }
                Effect::CopyFromHand => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Copy,
                        true,
                        "copy an Attack or Power card",
                    );
                }
                Effect::FromExhaustToHand => {
                    self.begin_choice(
                        ChoiceSource::Exhaust,
                        ChoiceAction::ToHand,
                        false,
                        "take a card from the exhaust pile",
                    );
                }
                Effect::FromDiscardToDrawTop => {
                    self.begin_choice(
                        ChoiceSource::Discard,
                        ChoiceAction::ToDrawTop,
                        false,
                        "take a card from the discard pile",
                    );
                }
                Effect::AddRandomAttackToHand => {
                    let pool: Vec<&'static crate::core::card::CardDef> = cards::CARDS
                        .iter()
                        .filter(|c| c.kind == crate::core::card::CardType::Attack)
                        .collect();
                    if !pool.is_empty() && self.hand.len() < HAND_LIMIT {
                        let def = self.rng.pick(&pool);
                        let mut inst = CardInstance::new(def);
                        inst.free_this_turn = true;
                        let label = inst.label();
                        self.hand.push(inst);
                        self.push_log(LogKind::Player, format!("{label} appears (costs 0)"));
                    }
                }
                Effect::PlayTopOfDraw => {
                    if let Some(mut card) = self.draw.pop() {
                        let label = card.label();
                        self.push_log(LogKind::Player, format!("Havoc plays {label}"));
                        let target = self.pick_random_alive();
                        let mut top_ctx = PlayCtx::default();
                        self.resolve(&mut card, target, &mut top_ctx);
                        card.free_this_turn = false;
                        self.exhaust_card(card);
                    }
                }
                Effect::AddCardToHand { id, n } => {
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        if self.hand.len() >= HAND_LIMIT {
                            break;
                        }
                        self.hand.push(CardInstance::new(def));
                    }
                }
                Effect::EnergyOnExhaust { .. } => {
                    // 在 exhaust_card 里结算
                }
                Effect::StrengthIfTargetAttacks { n } => {
                    // 目标这回合打算攻击才给力量
                    if let Some(t) = target {
                        if self.enemy_intends_attack(t) {
                            self.player.statuses.add(Status::Strength, n);
                        }
                    }
                }
                Effect::AddCardToDiscard { id, n } => {
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        self.discard.push(CardInstance::new(def));
                    }
                }
                Effect::UpgradeRandomInHand { n } => {
                    for _ in 0..n {
                        let cands: Vec<usize> = self
                            .hand
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.can_upgrade())
                            .map(|(i, _)| i)
                            .collect();
                        if cands.is_empty() {
                            break;
                        }
                        let pick = cands[self.rng.below(cands.len() as u32) as usize];
                        self.hand[pick].upgrade();
                        let name = self.hand[pick].label();
                        self.push_log(LogKind::Info, format!("{name} is upgraded for this combat"));
                    }
                }
            }
        }
    }

    /// 战斗中用药水
    pub fn use_potion(&mut self, def: &'static PotionDef, target: Option<usize>) {
        self.push_log(LogKind::Player, format!("you drink {}", def.name));
        match def.fx {
            PotionFx::Damage { amount } => {
                let t = target
                    .filter(|i| self.enemies.get(*i).map(|e| e.alive()).unwrap_or(false))
                    .or_else(|| self.first_alive());
                if let Some(t) = t {
                    let taken = self.damage_enemy(t, amount);
                    self.push_log(
                        LogKind::Player,
                        format!("{} deals {taken} damage", def.name),
                    );
                }
            }
            PotionFx::DamageAll { amount } => {
                for t in self.alive_enemies() {
                    self.damage_enemy(t, amount);
                }
            }
            PotionFx::Block { amount } => {
                self.gain_block(amount, false);
            }
            PotionFx::Energy { n } => {
                self.energy += n;
            }
            PotionFx::Draw { n } => {
                self.draw_cards(n as usize);
            }
            PotionFx::Strength { n } => {
                self.player.statuses.add(Status::Strength, n);
            }
            PotionFx::Dexterity { n } => {
                self.player.statuses.add(Status::Dexterity, n);
            }
            PotionFx::WeakAll { n } => {
                for i in self.alive_enemies() {
                    self.enemies[i].statuses.add(Status::Weak, n);
                }
            }
            PotionFx::VulnerableAll { n } => {
                for i in self.alive_enemies() {
                    self.enemies[i].statuses.add(Status::Vulnerable, n);
                }
            }
            PotionFx::Heal { amount } => {
                self.heal_player(amount);
            }
            PotionFx::MaxHp { n } => {
                self.player.max_hp += n;
                self.player.hp += n;
            }
            PotionFx::ClearDebuffs => {
                self.player.statuses.clear_debuffs();
            }
        }
        self.check_win();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cards::card;
    use crate::core::relics::relic_def_or_panic;

    /// 按 id 列表造一副牌
    fn from_ids(hp: i32, ids: &[&str]) -> CombatSetup {
        CombatSetup {
            hp,
            max_hp: hp,
            deck: ids.iter().map(|id| cards::card(id)).collect(),
            relics: Vec::new(),
        }
    }

    fn setup(hp: i32, ids: &[&str], relics: &[&'static RelicDef]) -> CombatSetup {
        let mut s = from_ids(hp, ids);
        s.relics = relics.to_vec();
        s
    }

    fn enc(id: &'static str) -> &'static Encounter {
        crate::core::enemies::encounter_def(id).unwrap_or_else(|| panic!("no encounter {id}"))
    }

    fn combat_with(encounter: &'static str, ids: &[&str]) -> Combat {
        Combat::new(enc(encounter), setup(80, ids, &[]), 1)
    }

    /// 新补的红卡:几个关键钩子各验一条
    #[test]
    fn new_ironclad_cards_hook_into_the_engine() {
        // 放血:掉 2 血 + 15 伤
        let mut c = combat_with("jaw_worm_solo", &["hemokinesis"; 5]);
        c.energy = 3;
        let hp_before = c.player.hp;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.player.hp, hp_before - 2, "先掉 2 血");
        assert_eq!(c.enemies[0].hp, e_hp - 15, "再打 15");

        // 硬撑:手里多两张伤口,还给了格挡
        let mut c = combat_with("jaw_worm_solo", &["power_through"; 5]);
        c.energy = 3;
        let hand0 = c.hand.len();
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), hand0 - 1 + 2, "打出去一张,再进两张伤口");
        assert!(c.player.block >= 15, "还应该给格挡");

        // 燃烧:回合结束掉 1 血并对所有敌人造成 5
        let mut c = combat_with("three_sentries", &["combust"; 5]);
        c.energy = 3;
        c.play_card(0, None).unwrap();
        let hp_before = c.player.hp;
        let e_hp: Vec<i32> = c.enemies.iter().map(|e| e.hp).collect();
        c.end_turn();
        // end_turn 之后敌人也会出手,所以玩家血量只能断言"至少掉了自伤这 1 点"
        assert!(c.player.hp <= hp_before - 1, "结束回合自伤 1 点");
        for (i, hp) in e_hp.iter().enumerate() {
            assert_eq!(c.enemies[i].hp, hp - 5, "所有敌人吃 5");
        }

        // 主宰:拿到格挡就对随机敌人来一下
        let mut c = combat_with("jaw_worm_solo", &["defend"; 4]);
        c.energy = 5;
        c.hand = vec![
            crate::core::cards::card("juggernaut"),
            crate::core::cards::card("defend"),
        ];
        c.play_card(0, None).unwrap();
        let e_hp = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 5, "主宰补了 5 点");

        // 腐化:技能 0 费,而且打出就被消耗
        let mut c = combat_with("jaw_worm_solo", &["defend"; 4]);
        c.energy = 3;
        c.hand = vec![
            crate::core::cards::card("corruption"),
            crate::core::cards::card("defend"),
        ];
        c.play_card(0, None).unwrap();
        let energy_before = c.energy;
        c.play_card(0, None).unwrap();
        assert_eq!(c.energy, energy_before, "技能不再花能量");
        assert_eq!(c.exhaust.len(), 1, "打出的技能被消耗");

        // 双发:这一击打两次
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.energy = 5;
        c.hand = vec![
            crate::core::cards::card("double_tap"),
            crate::core::cards::card("strike"),
        ];
        c.play_card(0, None).unwrap();
        let e_hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 12, "双发打两次 6 点");

        // 暴怒:这回合每次攻击都给格挡
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.energy = 5;
        c.hand = vec![
            crate::core::cards::card("rage"),
            crate::core::cards::card("strike"),
        ];
        c.play_card(0, None).unwrap();
        c.play_card(0, Some(0)).unwrap();
        assert!(c.player.block >= 3, "暴怒给 3 点格挡");
    }

    /// 需要选牌的五张:开了选择、选完动作对、esc 能原样退回
    #[test]
    fn choice_cards_open_a_choice_and_apply_it() {
        // 燃烧契约:先抽 2,再选一张手牌消耗
        let mut c = combat_with("jaw_worm_solo", &["burning_pact"; 5]);
        c.energy = 3;
        let exhausted_before = c.exhaust.len();
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_some(), "应该等着选牌");
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Hand);
        let target = c.hand[0].def.id;
        c.choose(0).unwrap();
        assert!(c.choice.is_none());
        assert_eq!(c.exhaust.len(), exhausted_before + 1, "选中的牌被消耗");
        assert_eq!(c.exhaust.last().unwrap().def.id, target);

        // 战吼:选一张手牌放回抽牌堆顶(它自己会消耗掉)
        let mut c = combat_with("jaw_worm_solo", &["warcry"; 5]);
        c.play_card(0, None).unwrap();
        let picked = c.hand[0].def.id;
        c.choose(0).unwrap();
        assert_eq!(c.draw[0].def.id, picked, "放到抽牌堆顶");
        assert_eq!(c.exhaust.len(), 1, "战吼自己被消耗");

        // 二重身:复制一张手牌
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![
            crate::core::cards::card("dual_wield"),
            crate::core::cards::card("strike"),
        ];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        let n = c.hand.len();
        c.choose(0).unwrap();
        assert_eq!(c.hand.len(), n + 1, "复制出一张");
        assert_eq!(c.hand.last().unwrap().def.id, "strike");

        // 掘出:从消耗堆拿回手牌
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.exhaust.push(crate::core::cards::card("bash"));
        c.hand = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Exhaust);
        c.choose(0).unwrap();
        assert!(c.hand.iter().any(|x| x.def.id == "bash"), "掘出的牌回到手牌");

        // 头槌:打伤害 + 从弃牌堆拿一张到抽牌堆顶
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.discard.push(crate::core::cards::card("defend"));
        c.hand = vec![crate::core::cards::card("headbutt")];
        c.energy = 3;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 9, "先打 9");
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Discard);
        c.choose(0).unwrap();
        assert_eq!(c.draw[0].def.id, "defend");

        // esc 取消:能量退回、牌回手牌
        let mut c = combat_with("jaw_worm_solo", &["burning_pact"; 5]);
        let hand_before = c.hand.len();
        let energy_before = c.energy;
        c.play_card(0, None).unwrap();
        c.cancel_choice();
        assert!(c.choice.is_none());
        assert_eq!(c.energy, energy_before, "取消要把能量退回来");
        assert_eq!(c.hand.len(), hand_before, "取消要把牌放回手牌");
    }

    /// 最后四张红卡:嗜血降费、炼狱之刃给 0 费攻击、浩劫打出顶上那张、灼热可反复升
    #[test]
    fn last_four_ironclad_cards_work() {
        // 嗜血:每掉一次血便宜 1
        let mut c = combat_with("jaw_worm_solo", &["strike"; 5]);
        c.hand = vec![crate::core::cards::card("blood_for_blood")];
        let before = c.hand[0].cost_value(c.energy);
        c.hit_player(5);
        assert_eq!(c.hand[0].cost_value(c.energy), before - 1, "掉血后便宜 1");

        // 炼狱之刃:手里多一张本回合 0 费的攻击牌
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![crate::core::cards::card("infernal_blade")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        let added = c
            .hand
            .iter()
            .find(|x| x.free_this_turn)
            .expect("应该多一张 0 费牌");
        assert_eq!(added.kind(), crate::core::card::CardType::Attack);
        assert_eq!(added.cost_value(c.energy), 0, "本回合 0 费");

        // 浩劫:把抽牌堆顶那张打出来并消耗
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![crate::core::cards::card("havoc")];
        c.draw = vec![crate::core::cards::card("strike")];
        c.energy = 3;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 6, "顶上那张 Strike 被打出来了");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "strike"),
            "打出的牌应该被消耗"
        );

        // 灼热攻击:一直能升,伤害跟着涨
        let mut card = crate::core::cards::card("searing_blow");
        let base = card.bonus_damage();
        card.upgrade();
        card.upgrade();
        assert_eq!(card.plus, 2, "升了两次");
        assert!(card.bonus_damage() > base, "越升越痛");
        assert!(card.can_upgrade(), "还能继续升");
        assert_eq!(card.label(), "Searing Blow+2");
    }

    #[test]
    fn first_turn_draws_five_and_gives_energy() {
        let c = combat_with("jaw_worm_solo", &["strike"; 10]);
        assert_eq!(c.turn, 1);
        assert_eq!(c.energy, BASE_ENERGY);
        assert_eq!(c.hand.len(), 5);
        assert_eq!(c.draw.len(), 5);
        assert_eq!(c.phase, Phase::PlayerTurn);
    }

    #[test]
    fn playing_strike_spends_energy_and_damages() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 10]);
        let before = c.enemies[0].hp;
        assert!(c.play_card(0, Some(0)).is_ok());
        assert_eq!(c.energy, 2);
        assert_eq!(c.enemies[0].hp, before - 6);
        assert_eq!(c.hand.len(), 4);
        assert_eq!(c.discard.len(), 1);
    }

    #[test]
    fn block_absorbs_enemy_attack() {
        let mut c = combat_with("jaw_worm_solo", &["defend"; 10]);
        assert!(c.play_card(0, None).is_ok());
        assert_eq!(c.player.block, 5);
        c.enemies[0].next_move = 0;
        let mv = &c.enemies[0].def.moves[0];
        assert!(
            matches!(mv.effects[0], EnemyFx::Attack { .. }),
            "测试假设第 0 招是攻击"
        );
        c.end_turn();
        // 11 点攻击里 5 点被格挡
        assert_eq!(c.player.hp, 80 - (11 - 5));
    }

    #[test]
    fn energy_limit_blocks_play() {
        let mut c = combat_with("jaw_worm_solo", &["bash"; 10]);
        assert!(c.play_card(0, Some(0)).is_ok());
        assert_eq!(c.energy, 1);
        assert_eq!(c.play_card(0, Some(0)), Err("not enough energy"));
    }

    #[test]
    fn unplayable_card_is_rejected() {
        let mut c = combat_with("jaw_worm_solo", &["wound"; 10]);
        assert_eq!(c.play_card(0, None), Err("unplayable"));
    }

    #[test]
    fn vulnerable_increases_damage_and_decays() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 10]);
        c.enemies[0].statuses.add(Status::Vulnerable, 2);
        let before = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        // 6 * 1.5 = 9
        assert_eq!(c.enemies[0].hp, before - 9);
        c.player.hp = 999;
        c.enemies[0].hp = 999;
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 1);
    }

    #[test]
    fn strength_applies_per_hit() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 10]);
        c.player.statuses.add(Status::Strength, 2);
        let before = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, before - 8);
    }

    #[test]
    fn weak_reduces_damage_rounded_down() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 10]);
        c.player.statuses.add(Status::Weak, 2);
        let before = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        // 6 * 0.75 = 4.5 -> 4
        assert_eq!(c.enemies[0].hp, before - 4);
    }

    #[test]
    fn exhausted_cards_go_to_exhaust_pile() {
        let mut c = combat_with("jaw_worm_solo", &["slimed"; 10]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.exhaust.len(), 1);
        assert!(c.discard.is_empty());
    }

    #[test]
    fn kill_all_enemies_wins() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 10]);
        c.enemies[0].hp = 5;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.phase, Phase::Won);
        assert!(c.enemies[0].dead());
    }

    #[test]
    fn player_death_sets_lost() {
        let mut c = combat_with("jaw_worm_solo", &["wound"; 10]);
        c.player.hp = 1;
        c.enemies[0].next_move = 0;
        c.phase = Phase::EnemyTurn;
        c.enemy_act(0);
        assert_eq!(c.phase, Phase::Lost);
        assert_eq!(c.player.hp, 0);
    }

    #[test]
    fn draw_reshuffles_discard_when_empty() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 6]);
        assert_eq!(c.hand.len(), 5);
        let card = c.hand.pop().unwrap();
        c.discard.push(card);
        c.draw_cards(2);
        assert_eq!(c.hand.len(), 6);
    }

    #[test]
    fn hand_limit_is_ten() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 30]);
        c.draw_cards(30);
        assert_eq!(c.hand.len(), HAND_LIMIT);
    }

    #[test]
    fn whirlwind_scales_with_energy() {
        let mut c = combat_with("jaw_worm_solo", &["whirlwind"; 5]);
        let before = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        // 3 点能量 * 5 = 15
        assert_eq!(before - c.enemies[0].hp, 15);
        assert_eq!(c.energy, 0);
    }

    #[test]
    fn body_slam_uses_block() {
        let mut c = combat_with("jaw_worm_solo", &["defend"; 10]);
        c.play_card(0, None).unwrap();
        c.hand.clear();
        c.hand.push(card("body_slam"));
        let before = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(before - c.enemies[0].hp, 5);
    }

    #[test]
    fn rampage_keeps_growing_within_combat() {
        let mut c = combat_with("jaw_worm_solo", &["rampage"; 6]);
        c.energy = 99;
        let before = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        let i = c
            .discard
            .iter()
            .position(|x| x.def.id == "rampage")
            .unwrap();
        let back = c.discard.remove(i);
        c.hand.push(back);
        c.play_card(c.hand.len() - 1, Some(0)).unwrap();
        // 8 + 13 = 21
        assert_eq!(before - c.enemies[0].hp, 21);
    }

    #[test]
    fn barricade_keeps_block_between_turns() {
        let mut c = combat_with("jaw_worm_solo", &["defend"; 10]);
        // 先垫一大坨格挡,免得被敌人一击打光(打的不是"有没有保留"这件事)
        c.player.block = 50;
        c.player.statuses.add(Status::Barricade, 1);
        c.player.hp = 999;
        c.enemies[0].hp = 999;
        c.enemies[0].next_move = 0;
        let mv = &c.enemies[0].def.moves[0];
        assert!(
            matches!(mv.effects[0], EnemyFx::Attack { .. }),
            "测试假设第 0 招是攻击"
        );
        c.end_turn();
        assert!(c.player.block > 0, "壁垒应保留格挡,实际 {}", c.player.block);
        assert_eq!(c.player.hp, 999, "格挡够厚就不该掉血");
    }

    #[test]
    fn thorns_relic_hits_attacker() {
        let relics = [relic_def_or_panic("bronze_scales")];
        let mut c = Combat::new(enc("jaw_worm_solo"), setup(80, &["wound"; 5], &relics), 7);
        c.enemies[0].hp = 50;
        c.enemies[0].block = 0;
        c.enemies[0].next_move = 0;
        assert!(matches!(c.enemies[0].def.moves[0].effects[0], EnemyFx::Attack { .. }));
        c.phase = Phase::EnemyTurn;
        let before = c.enemies[0].hp;
        c.enemy_act(0);
        assert!(c.enemies[0].hp < before, "荆棘应反伤");
        assert_eq!(before - c.enemies[0].hp, 3);
    }

    #[test]
    fn sleeping_enemy_wakes_when_hit() {
        let sleeper = crate::core::enemies::ENEMIES
            .iter()
            .find(|e| matches!(e.ai, Ai::Sleep { .. }));
        let Some(sleeper) = sleeper else {
            return;
        };
        let enc_id = crate::core::enemies::ENCOUNTERS
            .iter()
            .chain(crate::core::enemies::ELITES.iter())
            .chain(crate::core::enemies::BOSSES.iter())
            .find(|en| en.enemies.contains(&sleeper.id))
            .map(|en| en.id);
        let Some(enc_id) = enc_id else { return };
        let mut c = combat_with(enc_id, &["strike"; 10]);
        let i = c
            .enemies
            .iter()
            .position(|e| e.def.id == sleeper.id)
            .unwrap();
        assert!(!c.enemies[i].awake);
        c.play_card(0, Some(i)).unwrap();
        assert!(c.enemies[i].awake, "睡眠中的敌人被打后应醒来");
    }

    #[test]
    fn full_turn_cycle_returns_to_player() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 12]);
        c.player.hp = 999;
        let turn = c.turn;
        c.end_turn();
        assert_eq!(c.phase, Phase::PlayerTurn);
        assert_eq!(c.turn, turn + 1);
        assert_eq!(c.energy, BASE_ENERGY);
    }

    #[test]
    fn relic_lantern_grants_energy_on_combat_start() {
        let relics = [relic_def_or_panic("lantern")];
        let c = Combat::new(enc("jaw_worm_solo"), setup(80, &["strike"; 5], &relics), 3);
        assert_eq!(c.energy, BASE_ENERGY + 1);
    }

    #[test]
    fn fire_breathing_punishes_drawing_status() {
        let mut c = combat_with("jaw_worm_solo", &["wound"; 5]);
        c.player.statuses.add(Status::FireBreathing, 6);
        c.hand.clear();
        c.draw.clear();
        c.discard = vec![card("wound"), card("wound")];
        let before = c.enemies[0].hp;
        c.draw_cards(1);
        assert!(c.enemies[0].hp < before);
    }

    #[test]
    fn evoke_feel_no_pain_on_exhaust() {
        let mut c = combat_with("jaw_worm_solo", &["slimed"; 10]);
        c.player.statuses.add(Status::FeelNoPain, 4);
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 4);
    }

    #[test]
    fn multi_enemy_encounter_targets_individually() {
        let multi = crate::core::enemies::ENCOUNTERS
            .iter()
            .chain(crate::core::enemies::ENCOUNTERS_WEAK.iter())
            .chain(crate::core::enemies::ELITES.iter())
            .chain(crate::core::enemies::BOSSES.iter())
            .find(|e| e.enemies.len() > 1)
            .map(|e| e.id);
        let Some(multi) = multi else { return };
        let mut c = combat_with(multi, &["strike"; 10]);
        let hp0 = c.enemies[0].hp;
        let hp1 = c.enemies[1].hp;
        c.play_card(0, Some(1)).unwrap();
        assert_eq!(c.enemies[0].hp, hp0);
        assert_eq!(c.enemies[1].hp, hp1 - 6);
    }
}
