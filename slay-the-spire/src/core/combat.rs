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

#[derive(Clone)]
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
    /// 本回合被扣掉的力量(黑暗镣铐),回合结束回补
    pub temp_strength: i32,
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
    /// 抽牌堆(秘技/秘密武器)
    Draw,
    /// 亮出来的几张候选(发现)
    Offered,
}

/// 哪些牌可以被选中
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceFilter {
    Any,
    AttackOrPower,
    AttackOnly,
    SkillOnly,
}

impl ChoiceFilter {
    pub fn allows(self, card: &CardInstance) -> bool {
        use crate::core::card::CardType;
        match self {
            ChoiceFilter::Any => true,
            ChoiceFilter::AttackOrPower => {
                matches!(card.kind(), CardType::Attack | CardType::Power)
            }
            ChoiceFilter::AttackOnly => card.kind() == CardType::Attack,
            ChoiceFilter::SkillOnly => card.kind() == CardType::Skill,
        }
    }
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
    /// 放到抽牌堆底
    ToDrawBottom,
    /// 直接移出这局(调试用)
    Remove,
}

/// 一次待选择:比如"从手牌选一张消耗"
#[derive(Clone)]
pub struct Choice {
    pub source: ChoiceSource,
    pub action: ChoiceAction,
    /// 能选中哪些类型
    pub filter: ChoiceFilter,
    /// 最多选几张;0 表示不限张数(选到玩家主动结束为止)
    pub need: usize,
    /// 已经选了几张
    pub taken: usize,
    /// 是哪张牌引起的,信息栏提示用
    pub label: String,
    /// 还没收尾的那张牌 + 它花的能量:取消时原样退回
    pub played: Option<(CardInstance, i32)>,
    /// ChoiceSource::Offered 时亮出来的候选牌
    pub offered: Vec<CardInstance>,
}

#[derive(Clone)]
pub struct Combat {
    /// 本场玩家掉血的次数(嗜血按这个降费)
    pub hp_losses: i32,
    /// 浩劫链:依次被打出的牌(层级, 牌名),表现层拿去做"链式播报"
    pub havoc_chain: Vec<(u8, String)>,
    havoc_depth: u8,
    /// 放到抽牌堆顶的序号,每次放都 +1
    top_seq: u32,
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
    /// 本场战斗通过卡牌赚到的金币,由一局流程收走(贪婪之手)
    pub gold_gained: i32,
    /// 本场战斗的牌都算已升级(神化);之后新加进来的牌也直接升级
    all_upgraded: bool,
    /// 本回合已经打出几张牌(浮夸每 5 张结算一次)
    cards_played: i32,
    /// 定时炸弹:(剩余回合, 伤害)
    bombs: Vec<(u8, i32)>,
    relic_thorns: i32,
}

/// 单次打牌过程中的临时统计
#[derive(Default)]
struct PlayCtx {
    x: i32,
    exhausted: i32,
    unblocked: i32,
    /// 结算时的手牌张数快照(悔恨按手牌数掉血,回合结束时手牌已经清完,所以要提前记)
    hand_size: i32,
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
                temp_strength: 0,
            });
        }

        // 战斗开始时洗牌,天生牌直接入手
        let mut deck = setup.deck;
        rng.shuffle(&mut deck);
        let (innate, rest): (Vec<CardInstance>, Vec<CardInstance>) =
            deck.into_iter().partition(|c| c.is_innate());

        let mut c = Combat {
            hp_losses: 0,
            havoc_chain: Vec::new(),
            havoc_depth: 0,
            top_seq: 0,
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
            gold_gained: 0,
            all_upgraded: false,
            cards_played: 0,
            bombs: Vec::new(),
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
        // 上回合"本回合 0 费 / 本回合降到 1 费"的效果恢复原样;
        // 四个牌堆都要清:牌可能已经不在手里(弃掉/洗完/被消耗).
        // 本场战斗的 0 费与费用上限(cost_cap_combat)不受影响.
        for pile in [
            &mut self.hand,
            &mut self.draw,
            &mut self.discard,
            &mut self.exhaust,
        ] {
            for card in pile.iter_mut() {
                card.free_this_turn = false;
                card.cost_cap_this_turn = 0;
            }
        }
        // 上回合被黑暗镣铐扣掉的力量到期回补
        for e in self.enemies.iter_mut() {
            if e.temp_strength != 0 {
                e.statuses.add(Status::Strength, e.temp_strength);
                e.temp_strength = 0;
            }
        }
        self.cards_played = 0;
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
        // 混乱:回合开始打出抽牌堆顶那张(打完按它自己的规矩去弃牌堆/消耗堆)
        if self.player.statuses.has(Status::Mayhem) {
            self.play_top_of_draw(false);
        }
        // 磁力:回合开始随机给一张无色牌
        let mag = self.player.statuses.get(Status::Magnetism);
        for _ in 0..mag {
            self.add_random_colorless_to_hand(false, false);
        }
    }

    /// 随机无色牌进手牌;free 表示本回合 0 费,upgraded 表示直接给升级版
    fn add_random_colorless_to_hand(&mut self, free: bool, upgraded: bool) {
        if self.hand.len() >= HAND_LIMIT {
            return;
        }
        let pool = cards::colorless_pool();
        if pool.is_empty() {
            return;
        }
        let def = self.rng.pick(&pool);
        let mut inst = CardInstance::new(def);
        if upgraded {
            inst.upgrade();
        }
        self.fix_new_card(&mut inst);
        inst.free_this_turn = free;
        let label = inst.label();
        self.hand.push(inst);
        self.push_log(LogKind::Player, format!("{label} appears"));
    }

    /// 打出抽牌堆顶那张;exhaust_after 为真时打完直接消耗(浩劫)
    fn play_top_of_draw(&mut self, exhaust_after: bool) {
        let Some(mut card) = self.draw.pop() else {
            return;
        };
        let label = card.label();
        self.push_log(
            LogKind::Player,
            format!("{} plays {label}", if exhaust_after { "Havoc" } else { "Mayhem" }),
        );
        // 记进浩劫链(层级 +1 表示嵌了一层),表现层据此叠播报
        self.havoc_depth = self.havoc_depth.saturating_add(1);
        self.havoc_chain.push((self.havoc_depth, label));
        let target = self.pick_random_alive();
        let mut top_ctx = PlayCtx::default();
        self.resolve(&mut card, target, &mut top_ctx);
        self.havoc_depth = self.havoc_depth.saturating_sub(1);
        card.free_this_turn = false;
        if exhaust_after || card.is_exhaust() {
            self.exhaust_card(card);
        } else {
            self.discard.push(card);
        }
        self.note_card_played();
        self.check_win();
    }

    /// 记一张打出的牌;浮夸每打满 5 张就对所有敌人来一下;痛苦手里有就掉血
    fn note_card_played(&mut self) {
        self.cards_played += 1;
        // 痛苦:在自己手里时,别人被打出就掉 1 血(本张牌已经从手牌/抽牌堆拿走)
        let pain: i32 = self
            .hand
            .iter()
            .flat_map(|c| c.in_hand())
            .map(|e| match *e {
                Effect::LoseHpOnOtherCardPlayed { amount } => amount,
                _ => 0,
            })
            .sum();
        if pain > 0 {
            self.push_log(LogKind::Player, format!("Pain: lose {pain} HP"));
            self.lose_hp_player(pain, true);
        }
        let pan = self.player.statuses.get(Status::Panache);
        if pan <= 0 || self.cards_played % 5 != 0 {
            return;
        }
        for i in self.alive_enemies() {
            self.damage_enemy(i, pan);
        }
        self.push_log(
            LogKind::Player,
            format!("Panache deals {pan} to all enemies"),
        );
        self.settle_deaths();
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
            self.on_card_drawn();
        }
    }

    /// 抽到一张牌时触发:牌自己的"抽到时"效果(虚空掉能量),状态牌再触发能力(进化、吐火)
    fn on_card_drawn(&mut self) {
        let Some(mut card) = self.hand.last().cloned() else {
            return;
        };
        if !card.on_draw().is_empty() {
            let label = card.label();
            let effects = card.on_draw();
            let mut ctx = PlayCtx::default();
            self.resolve_effects(&mut card, effects, None, &mut ctx);
            self.push_log(LogKind::Player, format!("{label} triggers when drawn"));
        }
        if card.kind() != crate::core::card::CardType::Status {
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
        // 死灵诅咒:消耗也逃不掉,补一张新的回手牌(手牌满就退到弃牌堆)
        let escapes = card
            .effects()
            .iter()
            .any(|e| matches!(*e, Effect::SelfToHandOnExhaust));
        let def = card.def;
        self.exhaust.push(card);
        if escapes {
            let mut copy = CardInstance::new(def);
            self.fix_new_card(&mut copy);
            let label = copy.label();
            if self.hand.len() < HAND_LIMIT {
                self.hand.push(copy);
                self.push_log(LogKind::Player, format!("{label} escapes to your hand"));
            } else {
                self.discard.push(copy);
                self.push_log(
                    LogKind::Player,
                    format!("{label} escapes to your discard pile (hand is full)"),
                );
            }
        }
        let fnp = self.player.statuses.get(Status::FeelNoPain);
        if fnp > 0 {
            self.gain_block(fnp, false, false);
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
        // 定时炸弹:回合数减一,归零就炸
        let mut boom: Vec<i32> = Vec::new();
        for b in self.bombs.iter_mut() {
            b.0 = b.0.saturating_sub(1);
            if b.0 == 0 {
                boom.push(b.1);
            }
        }
        self.bombs.retain(|b| b.0 > 0);
        for dmg in boom {
            for i in self.alive_enemies() {
                let d = self.player_attack_damage(dmg, i);
                self.damage_enemy(i, d);
            }
            self.push_log(LogKind::Player, format!("The Bomb explodes for {dmg}"));
            self.settle_deaths();
        }
        self.check_win();
        if self.phase != Phase::PlayerTurn {
            return;
        }
        let metal = self.player.statuses.get(Status::Metallicize);
        if metal > 0 {
            self.gain_block(metal, false, false);
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
        // 手牌里"回合结束触发"的诅咒先按现状记下来:手牌张数以这一刻为准(悔恨),
        // 效果留到减益递减之后再结算,刚拿到的 Weak/Frail 才不会被当场扣掉一层
        let eot: Vec<CardInstance> = self
            .hand
            .iter()
            .filter(|c| !c.on_end_turn().is_empty())
            .cloned()
            .collect();
        let hand_size = self.hand.len() as i32;
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
        for mut card in eot {
            let label = card.label();
            let effects = card.on_end_turn();
            let mut ctx = PlayCtx {
                hand_size,
                ..Default::default()
            };
            self.resolve_effects(&mut card, effects, None, &mut ctx);
            self.push_log(LogKind::Player, format!("{label} triggers at end of turn"));
        }
        // 悔恨/腐烂可能把玩家打死,这时不能再把回合交给敌人
        if self.phase != Phase::PlayerTurn {
            return;
        }
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
                        self.add_player_status_from_enemy(status, n);
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

    /// from_card 为真表示这次格挡来自卡牌,会受"紧急按钮"的限制
    fn gain_block(&mut self, amount: i32, doubled: bool, from_card: bool) {
        if from_card && self.player.statuses.has(Status::NoBlock) {
            self.push_log(
                LogKind::Info,
                "No Block: cannot gain Block from cards".to_string(),
            );
            return;
        }
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

    /// 战斗中后来拿到的牌:把本场已有的降费补给嗜血;神化之后一律直接升级
    pub fn fix_new_card(&self, card: &mut CardInstance) {
        if card.def.id == "blood_for_blood" {
            let base = match card.def.cost {
                crate::core::card::Cost::Fixed(n) => n as i32,
                _ => 0,
            };
            card.cost_delta = -(self.hp_losses.min(base));
        }
        if self.all_upgraded {
            card.upgrade();
        }
    }

    /// 调试用:开一个"从手牌里删牌"的选择
    pub fn debug_begin_hand_remove(&mut self) {
        self.begin_choice(
            ChoiceSource::Hand,
            ChoiceAction::Remove,
            ChoiceFilter::Any,
            1,
            "remove a card from your hand",
        );
    }

    /// 开一次选牌:记下来,等界面那边选完再 choose()
    /// need 是最多选几张,0 表示不限张数
    fn begin_choice(
        &mut self,
        source: ChoiceSource,
        action: ChoiceAction,
        filter: ChoiceFilter,
        need: usize,
        label: &str,
    ) {
        self.choice = Some(Choice {
            source,
            action,
            filter,
            need,
            taken: 0,
            label: label.to_string(),
            played: None,
            offered: Vec::new(),
        });
    }

    /// 这次选择里还能选的卡(索引 + 卡);发现模式下列的是亮出来的那几张
    fn candidates_of<'a>(&'a self, ch: &'a Choice) -> Vec<(usize, &'a CardInstance)> {
        let pile: &[CardInstance] = match ch.source {
            ChoiceSource::Hand => &self.hand,
            ChoiceSource::Discard => &self.discard,
            ChoiceSource::Exhaust => &self.exhaust,
            ChoiceSource::Draw => &self.draw,
            ChoiceSource::Offered => &ch.offered,
        };
        pile.iter()
            .enumerate()
            .filter(|(_, c)| ch.filter.allows(c))
            .collect()
    }

    /// 这一堆里现在能选的卡(索引 + 卡)
    pub fn choice_candidates(&self) -> Vec<(usize, &CardInstance)> {
        match self.choice.as_ref() {
            Some(ch) => self.candidates_of(ch),
            None => Vec::new(),
        }
    }

    /// 选完了:执行动作;还要继续选(多选)时选择窗口留着
    pub fn choose(&mut self, idx: usize) -> Result<(), String> {
        let Some(mut ch) = self.choice.take() else {
            return Err("nothing to choose".to_string());
        };
        let ok = self.candidates_of(&ch).iter().any(|(i, _)| *i == idx);
        if !ok {
            self.choice = Some(ch);
            return Err("that card cannot be chosen".to_string());
        }
        match (ch.source, ch.action) {
            (ChoiceSource::Hand, ChoiceAction::Exhaust) => {
                let card = self.hand.remove(idx);
                self.exhaust_card(card);
            }
            (ChoiceSource::Hand, ChoiceAction::Copy) => {
                let card = self.hand[idx].clone();
                if self.hand.len() < HAND_LIMIT {
                    self.hand.push(card);
                }
            }
            (ChoiceSource::Hand, ChoiceAction::ToDrawTop) => {
                // 抽牌堆的"顶"是 Vec 末尾(draw_cards 从末尾 pop),所以 push 才是放顶上
                let mut card = self.hand.remove(idx);
                self.top_seq += 1;
                card.topped = self.top_seq;
                self.draw.push(card);
            }
            (ChoiceSource::Hand, ChoiceAction::ToDrawBottom) => {
                // "底"就是抽牌堆的开头,没被放到顶上的牌都从末尾抽
                let mut card = self.hand.remove(idx);
                // 预谋:放到堆底之后一直 0 费,直到被打出(打出时才清掉)
                card.free_combat = true;
                self.draw.insert(0, card);
            }
            (ChoiceSource::Exhaust, ChoiceAction::ToHand) => {
                if self.hand.len() < HAND_LIMIT {
                    let card = self.exhaust.remove(idx);
                    self.hand.push(card);
                }
            }
            (ChoiceSource::Draw, ChoiceAction::ToHand) => {
                if self.hand.len() < HAND_LIMIT {
                    let card = self.draw.remove(idx);
                    let label = card.label();
                    self.hand.push(card);
                    self.push_log(LogKind::Player, format!("{label} rises to your hand"));
                }
            }
            (ChoiceSource::Offered, ChoiceAction::ToHand) => {
                let mut card = ch.offered.remove(idx);
                // 发现:挑中的那张本回合 0 费,剩下的几张就此消失
                card.free_this_turn = true;
                let label = card.label();
                if self.hand.len() < HAND_LIMIT {
                    self.hand.push(card);
                }
                self.push_log(LogKind::Player, format!("{label} is added to your hand"));
            }
            (ChoiceSource::Discard, ChoiceAction::ToDrawTop) => {
                let mut card = self.discard.remove(idx);
                self.top_seq += 1;
                card.topped = self.top_seq;
                self.draw.push(card);
            }
            (ChoiceSource::Hand, ChoiceAction::Remove) => {
                self.hand.remove(idx);
            }
            _ => {}
        }
        ch.taken += 1;
        let full = ch.need != 0 && ch.taken >= ch.need;
        let empty = self.candidates_of(&ch).is_empty();
        if full || empty {
            self.finish_played(ch.played.take());
        } else {
            self.choice = Some(ch);
        }
        Ok(())
    }

    /// 多选模式下玩家主动收工(比如"最多消耗 3 张",只消耗 1 张就结束)
    pub fn finish_choice(&mut self) {
        if let Some(mut ch) = self.choice.take() {
            self.finish_played(ch.played.take());
        }
    }

    /// 取消这次出牌:能量退回、牌回手牌;已经选出结果的几次收不回来
    pub fn cancel_choice(&mut self) {
        let Some(mut ch) = self.choice.take() else {
            return;
        };
        if ch.taken > 0 {
            // 前面几次已经生效了,这时 esc 只能当"选完了"
            self.finish_played(ch.played.take());
            return;
        }
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
        self.hp_losses += 1;
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
                            self.add_player_status_from_enemy(status, n);
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
        // 反常:只要它在手里,本回合最多打出 3 张
        for c in self.hand.iter() {
            for e in c.in_hand() {
                if let Effect::PlayLimitWhileInHand { max } = *e {
                    if self.cards_played >= max as i32 {
                        return Err("normality: too many cards played this turn");
                    }
                }
            }
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
        // 0 费只对这一回合的那一次打出有效,出手后立刻失效
        card.free_this_turn = false;
        let corrupted_skill = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        let cost = if corrupted_skill { 0 } else { card.cost_value(self.energy) };
        self.energy -= cost.min(self.energy);
        let is_x = card.cost() == Cost::X;
        let mut ctx = PlayCtx {
            x: if is_x { cost } else { 0 },
            hand_size: self.hand.len() as i32,
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
                self.gain_block(rage, false, false);
            }
        }
        if self.player.block > blocked_before {
            let gained = self.player.block - blocked_before;
            self.push_log(LogKind::Player, format!("you gain {gained} Block"));
        }
        if ctx.unblocked > 0 {
            self.push_log(LogKind::Info, format!("dealt {} damage", ctx.unblocked));
        }
        // 浮夸按"本回合打出的牌数"结算(被人替打出来的牌也算)
        self.note_card_played();
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
        self.resolve_effects(card, effects, target, ctx);
    }

    /// 逐条结算一份效果列表:打出时传 effects(),抽到/回合结束时传 on_draw / on_end_turn
    fn resolve_effects(
        &mut self,
        card: &mut CardInstance,
        effects: &'static [Effect],
        target: Option<usize>,
        ctx: &mut PlayCtx,
    ) {
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
                    self.gain_block(amount, false, true);
                }
                Effect::BlockPerExhausted { per } => {
                    self.gain_block(per * ctx.exhausted, false, true);
                }
                Effect::DoubleBlock => {
                    let b = self.player.block;
                    self.gain_block(b, true, true);
                }                Effect::LoseHp { amount } => {
                    self.lose_hp_player(amount, true);
                }
                Effect::LoseHpPerHandCard => {
                    let n = ctx.hand_size.max(0);
                    self.lose_hp_player(n, true);
                }
                Effect::CopySelfToDrawTop => {
                    // 抽牌堆的"顶"是 Vec 末尾(draw_cards 从末尾 pop)
                    let mut copy = CardInstance::new(card.def);
                    copy.upgraded = card.upgraded;
                    copy.plus = card.plus;
                    self.top_seq += 1;
                    copy.topped = self.top_seq;
                    let label = copy.label();
                    self.draw.push(copy);
                    self.push_log(
                        LogKind::Player,
                        format!("a copy of {label} goes on top of your draw pile"),
                    );
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
                        self.add_enemy_status(t, status, n);
                    }
                }
                Effect::AddAllEnemiesStatus { status, n } => {
                    for i in self.alive_enemies() {
                        self.add_enemy_status(i, status, n);
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
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        self.draw.push(inst);
                    }
                }
                Effect::ExhaustFromHand => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Exhaust,
                        ChoiceFilter::Any,
                        1,
                        "exhaust a card",
                    );
                }
                Effect::TopFromHand => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::ToDrawTop,
                        ChoiceFilter::Any,
                        1,
                        "put a card on top of the draw pile",
                    );
                }
                Effect::CopyFromHand => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Copy,
                        ChoiceFilter::AttackOrPower,
                        1,
                        "copy an Attack or Power card",
                    );
                }
                Effect::FromExhaustToHand => {
                    self.begin_choice(
                        ChoiceSource::Exhaust,
                        ChoiceAction::ToHand,
                        ChoiceFilter::Any,
                        1,
                        "take a card from the exhaust pile",
                    );
                }
                Effect::FromDiscardToDrawTop => {
                    self.begin_choice(
                        ChoiceSource::Discard,
                        ChoiceAction::ToDrawTop,
                        ChoiceFilter::Any,
                        1,
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
                        self.fix_new_card(&mut inst);
                        inst.free_this_turn = true;
                        let label = inst.label();
                        self.hand.push(inst);
                        self.push_log(LogKind::Player, format!("{label} appears (costs 0)"));
                    }
                }
                Effect::PlayTopOfDraw => {
                    self.play_top_of_draw(true);
                }
                Effect::AddCardToHand { id, n } => {
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        if self.hand.len() >= HAND_LIMIT {
                            break;
                        }
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        self.hand.push(inst);
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
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        self.discard.push(inst);
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
                Effect::Heal { amount } => {
                    self.heal_player(amount);
                }
                Effect::DamagePerDrawPile { per } => {
                    if let Some(t) = target {
                        let raw = per * self.draw.len() as i32;
                        let d = self.player_attack_damage(raw, t);
                        ctx.unblocked += self.damage_enemy(t, d);
                    }
                }
                Effect::DamageAndGoldOnKill {
                    amount,
                    times,
                    gold,
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
                            self.gold_gained += gold;
                            self.push_log(
                                LogKind::Player,
                                format!("you loot {gold} gold"),
                            );
                        }
                    }
                }
                Effect::DrawIfNoAttacks { n } => {
                    let has_attack = self
                        .hand
                        .iter()
                        .any(|c| c.kind() == crate::core::card::CardType::Attack);
                    if !has_attack {
                        self.draw_cards(n as usize);
                    }
                }
                Effect::AddRandomColorlessToHand { n, free, upgraded } => {
                    for _ in 0..n {
                        self.add_random_colorless_to_hand(free, upgraded);
                    }
                }
                Effect::AddRandomColorlessXToHand { upgraded } => {
                    for _ in 0..ctx.x {
                        self.add_random_colorless_to_hand(true, upgraded);
                    }
                }
                Effect::AddRandomToDrawFree { kind, n } => {
                    let pool = cards::class_pool_of_kind(kind);
                    if pool.is_empty() {
                        continue;
                    }
                    for _ in 0..n {
                        let def = self.rng.pick(&pool);
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        inst.free_combat = true;
                        let label = inst.label();
                        self.draw.push(inst);
                        self.push_log(
                            LogKind::Info,
                            format!("{label} is shuffled in (costs 0 this combat)"),
                        );
                    }
                    self.rng.shuffle(&mut self.draw);
                }
                Effect::ShuffleDiscardIntoDraw => {
                    if !self.discard.is_empty() {
                        let n = self.discard.len();
                        self.draw.append(&mut self.discard);
                        self.rng.shuffle(&mut self.draw);
                        self.push_log(
                            LogKind::Info,
                            format!("shuffled {n} cards into the draw pile"),
                        );
                    }
                }
                Effect::FreeRandomInHand => {
                    if !self.hand.is_empty() {
                        let pick = self.rng.below(self.hand.len() as u32) as usize;
                        self.hand[pick].free_combat = true;
                        let name = self.hand[pick].label();
                        self.push_log(
                            LogKind::Info,
                            format!("{name} costs 0 for the rest of combat"),
                        );
                    }
                }
                Effect::CapHandCost { cap, combat } => {
                    for c in self.hand.iter_mut() {
                        if let Some(base) = c.fixed_cost() {
                            if base > cap as i32 {
                                if combat {
                                    c.cost_cap_combat = cap;
                                } else {
                                    c.cost_cap_this_turn = cap;
                                }
                            }
                        }
                    }
                }
                Effect::UpgradeAllForCombat => {
                    self.all_upgraded = true;
                    for pile in [
                        &mut self.hand,
                        &mut self.draw,
                        &mut self.discard,
                        &mut self.exhaust,
                    ] {
                        for c in pile.iter_mut() {
                            c.upgrade();
                        }
                    }
                    self.push_log(
                        LogKind::Player,
                        "all your cards are upgraded for this combat".to_string(),
                    );
                }
                Effect::TargetLoseStrengthThisTurn { n } => {
                    if let Some(t) = target {
                        if self.enemies[t].alive() {
                            // 只能扣掉它当前真有的力量,回合结束按扣掉的量补回来
                            let cur = self.enemies[t].statuses.get(Status::Strength);
                            let loss = n.min(cur.max(0));
                            if loss > 0 {
                                self.enemies[t].statuses.add(Status::Strength, -loss);
                                self.enemies[t].temp_strength += loss;
                                self.push_log(
                                    LogKind::Player,
                                    format!(
                                        "{} loses {loss} Strength this turn",
                                        self.enemies[t].name
                                    ),
                                );
                            }
                        }
                    }
                }
                Effect::OfferRandomCardsFromClass { n } => {
                    let pool = cards::class_card_pool();
                    if pool.is_empty() {
                        continue;
                    }
                    let mut offered: Vec<CardInstance> = Vec::new();
                    for _ in 0..n {
                        let def = self.rng.pick(&pool);
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        offered.push(inst);
                    }
                    self.begin_choice(
                        ChoiceSource::Offered,
                        ChoiceAction::ToHand,
                        ChoiceFilter::Any,
                        1,
                        "choose 1 of 3 random cards",
                    );
                    if let Some(ch) = self.choice.as_mut() {
                        ch.offered = offered;
                    }
                }
                Effect::ExhaustUpTo { n } => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Exhaust,
                        ChoiceFilter::Any,
                        n as usize,
                        &format!("exhaust up to {n} cards"),
                    );
                }
                Effect::ToDrawBottomFromHand { n } => {
                    let label = if n == 0 {
                        "put any number of cards on the bottom of the draw pile"
                    } else {
                        "put a card on the bottom of the draw pile"
                    };
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::ToDrawBottom,
                        ChoiceFilter::Any,
                        n as usize,
                        label,
                    );
                }
                Effect::TakeFromDrawToHand { kind } => {
                    let hit = self.draw.iter().any(|c| c.kind() == kind);
                    if !hit {
                        self.push_log(
                            LogKind::Info,
                            format!("no {} left in the draw pile", kind.name().to_lowercase()),
                        );
                        continue;
                    }
                    let (filter, label) = match kind {
                        crate::core::card::CardType::Attack => {
                            (ChoiceFilter::AttackOnly, "put an Attack from your draw pile into your hand")
                        }
                        _ => (ChoiceFilter::SkillOnly, "put a Skill from your draw pile into your hand"),
                    };
                    self.begin_choice(
                        ChoiceSource::Draw,
                        ChoiceAction::ToHand,
                        filter,
                        1,
                        label,
                    );
                }
                Effect::RandomFromDrawToHand { kind, n } => {
                    for _ in 0..n {
                        if self.hand.len() >= HAND_LIMIT {
                            break;
                        }
                        let cands: Vec<usize> = self
                            .draw
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.kind() == kind)
                            .map(|(i, _)| i)
                            .collect();
                        if cands.is_empty() {
                            break;
                        }
                        let pick = cands[self.rng.below(cands.len() as u32) as usize];
                        let card = self.draw.remove(pick);
                        let label = card.label();
                        self.hand.push(card);
                        self.push_log(LogKind::Player, format!("{label} rises to your hand"));
                    }
                }
                Effect::Bomb { turns, damage } => {
                    self.bombs.push((turns, damage));
                    self.push_log(
                        LogKind::Player,
                        format!("a bomb is set: {damage} damage in {turns} turns"),
                    );
                }
                // 这三类不由"打出"触发,各有自己的钩子:
                // 消耗时(exhaust_card)/ 别人被打出时(note_card_played)/ 可打性检查(playable)
                Effect::SelfToHandOnExhaust
                | Effect::LoseHpOnOtherCardPlayed { .. }
                | Effect::PlayLimitWhileInHand { .. } => {}
                // 抽出牌组时由 run 层的删牌流程结算(picker_confirm)
                Effect::LoseMaxHpOnRemoved { .. } => {}
            }
        }
    }

    /// 给敌人上状态;玩家造成的减益会触发残虐天性
    fn add_enemy_status(&mut self, idx: usize, status: Status, n: i32) {
        if idx >= self.enemies.len() || self.enemies[idx].dead() {
            return;
        }
        self.enemies[idx].statuses.add(status, n);
        if n <= 0 || !status.is_debuff() {
            return;
        }
        let sad = self.player.statuses.get(Status::SadisticNature);
        if sad <= 0 {
            return;
        }
        self.damage_enemy(idx, sad);
        self.push_log(
            LogKind::Player,
            format!("Sadistic Nature deals {sad} damage"),
        );
        self.settle_deaths();
    }

    /// 敌人给玩家上状态;有神器就先拿一层顶掉这次减益
    fn add_player_status_from_enemy(&mut self, status: Status, n: i32) {
        if n > 0 && status.is_debuff() && self.player.statuses.has(Status::Artifact) {
            self.player.statuses.add(Status::Artifact, -1);
            self.push_log(
                LogKind::Player,
                format!("Artifact blocks {}", status.name()),
            );
            return;
        }
        self.player.statuses.add(status, n);
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
                self.gain_block(amount, false, false);
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
                    self.add_enemy_status(i, Status::Weak, n);
                }
            }
            PotionFx::VulnerableAll { n } => {
                for i in self.alive_enemies() {
                    self.add_enemy_status(i, Status::Vulnerable, n);
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
        let before = c.draw.len();
        c.choose(0).unwrap();
        assert_eq!(c.draw.last().unwrap().def.id, picked, "放到抽牌堆顶");
        assert_eq!(c.draw.len(), before + 1);
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
        // 抽牌堆的顶是末尾,而且下一次抽牌就要抽到它
        assert_eq!(c.draw.last().unwrap().def.id, "defend", "应该放在抽牌堆顶");
        let hand_before = c.hand.len();
        c.draw_cards(1);
        assert_eq!(
            c.hand.len(),
            hand_before + 1,
            "下一次抽牌应该抽到它(手牌 +1)"
        );
        assert_eq!(c.hand.last().unwrap().def.id, "defend");

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

    /// 浩劫连锁:浩劫打浩劫再打出一张普通牌,链上每张都记下来
    #[test]
    fn havoc_chain_records_every_card_it_plays() {
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 4]);
        c.hand = vec![crate::core::cards::card("havoc")];
        // 抽牌堆的顶是末尾,所以这样排:先被抽到的是最后一个 havoc
        c.draw = vec![
            crate::core::cards::card("strike"),
            crate::core::cards::card("havoc"),
            crate::core::cards::card("havoc"),
        ];
        c.energy = 3;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        let chain: Vec<&str> = c.havoc_chain.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(chain, vec!["Havoc", "Havoc", "Strike"], "三层链都要记下来");
        let depth: Vec<u8> = c.havoc_chain.iter().map(|(d, _)| *d).collect();
        assert_eq!(depth, vec![1, 2, 3], "层级逐层加深");
        assert_eq!(c.enemies[0].hp, e_hp - 6, "最里面那张 Strike 真的打出来了");
        assert_eq!(c.exhaust.len(), 3, "三张都进了消耗堆");
        assert_eq!(c.havoc_depth, 0, "链走完之后层级归零");
        assert!(c.hand.is_empty(), "手里那张 havoc 也消耗掉了");
    }

    /// 浩劫打出"需要选牌"的牌(战吼/掘出):选择照样挂出来,选完正常结算
    #[test]
    fn havoc_playing_a_choice_card_still_asks() {
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 4]);
        c.hand = vec![crate::core::cards::card("havoc")];
        c.draw = vec![crate::core::cards::card("warcry")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(
            c.havoc_chain.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
            vec!["Warcry"],
            "播报里要能看到浩劫打出了战吼"
        );
        let ch = c.choice.as_ref().expect("战吼的选择应该挂出来");
        assert_eq!(ch.source, ChoiceSource::Hand);
        // 选一张手牌放回抽牌堆顶
        if !c.hand.is_empty() {
            let picked = c.hand[0].def.id;
            c.choose(0).unwrap();
            assert!(c.choice.is_none(), "选完就结束");
            assert_eq!(c.draw.last().unwrap().def.id, picked, "放到抽牌堆顶");
        }

        // 掘出(从消耗堆拿)也一样:选择来自消耗堆
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 4]);
        c.exhaust.push(crate::core::cards::card("bash"));
        c.hand = vec![crate::core::cards::card("havoc")];
        c.draw = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        let ch = c.choice.as_ref().expect("掘出的选择应该挂出来");
        assert_eq!(ch.source, ChoiceSource::Exhaust);
        c.choose(0).unwrap();
        assert!(c.hand.iter().any(|x| x.def.id == "bash"), "掘出的牌回手牌");
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

        // 下一回合恢复原价:牌这时候多半已经进了弃牌堆,那边也得清
        c.end_turn();
        for pile in [&c.hand, &c.draw, &c.discard, &c.exhaust] {
            assert!(
                pile.iter().all(|x| !x.free_this_turn),
                "本回合 0 费不该留到下一回合"
            );
        }
        // 恢复的是"本来的费用",不是固定值:同一张牌下回合该回到它的原价
        let plain = crate::core::cards::card("strike");
        assert_eq!(plain.cost_value(3), 1, "普通牌照旧");
        assert_eq!(crate::core::cards::card("clash").cost_value(3), 0, "本来就 0 费的照旧");

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

    // ---- 无色牌 ----

    /// 手牌里某张牌的下标
    fn hand_idx(c: &Combat, id: &str) -> usize {
        c.hand
            .iter()
            .position(|x| x.def.id == id)
            .unwrap_or_else(|| panic!("{id} 不在手牌里"))
    }

    /// 摆一副牌:hand 里的进手牌,其余按 deck 的顺序进抽牌堆
    fn staged(deck: &[&str], hand: &[&str]) -> Combat {
        let mut c = combat_with("jaw_worm_solo", deck);
        c.hand.clear();
        c.draw.clear();
        c.discard.clear();
        c.exhaust.clear();
        for id in deck {
            let inst = cards::card(id);
            if hand.contains(id) && c.hand.len() < HAND_LIMIT {
                c.hand.push(inst);
            } else {
                c.draw.push(inst);
            }
        }
        c.energy = 9;
        c
    }

    #[test]
    fn apotheosis_upgrades_every_pile_and_later_cards() {
        let mut c = staged(&["apotheosis", "strike", "defend", "bash"], &["apotheosis"]);
        // 弃牌堆里也放一张,四个牌堆都要覆盖到(正在打出的这张自己不算)
        c.discard.push(cards::card("defend"));
        let idx = hand_idx(&c, "apotheosis");
        c.play_card(idx, None).unwrap();
        assert!(c.draw.iter().all(|x| x.upgraded), "抽牌堆没升级");
        assert!(c.discard.iter().all(|x| x.upgraded), "弃牌堆没升级");
        let defend = c.discard.iter().find(|x| x.def.id == "defend").unwrap();
        assert_eq!(defend.effects(), &[Effect::Block { amount: 8 }]);
        let strike = c.draw.iter().find(|x| x.def.id == "strike").unwrap();
        assert_eq!(
            strike.effects(),
            &[Effect::Damage {
                amount: 9,
                times: 1
            }],
            "升级后的打击是 9 伤"
        );
        // 之后新拿到的牌也直接是升级版
        let mut fresh = cards::card("strike");
        c.fix_new_card(&mut fresh);
        assert!(fresh.upgraded, "神化之后新拿到的牌应直接升级");
    }

    #[test]
    fn madness_zeroes_a_card_for_the_whole_combat() {
        let mut c = staged(&["madness", "bludgeon"], &["madness", "bludgeon"]);
        let idx = hand_idx(&c, "madness");
        c.play_card(idx, None).unwrap();
        let b = c.hand.iter().find(|x| x.def.id == "bludgeon").unwrap();
        assert_eq!(b.fixed_cost(), Some(0), "疯狂把手里那张降到 0");
        // 过了回合依然是 0 费(本场战斗)
        c.end_turn();
        let b = [&c.hand, &c.draw, &c.discard, &c.exhaust]
            .into_iter()
            .flatten()
            .find(|x| x.def.id == "bludgeon")
            .unwrap();
        assert_eq!(b.fixed_cost(), Some(0), "本场战斗的 0 费不该被回合开始清掉");
    }

    #[test]
    fn enlightenment_caps_hand_cost() {
        // 基础版:只到回合结束
        let mut c = staged(
            &["enlightenment", "bludgeon", "bludgeon"],
            &["enlightenment", "bludgeon", "bludgeon"],
        );
        let idx = hand_idx(&c, "enlightenment");
        c.play_card(idx, None).unwrap();
        assert!(c.hand.iter().all(|x| x.fixed_cost() == Some(1)), "都降到 1 费");
        c.end_turn();
        let b = [&c.hand, &c.draw, &c.discard, &c.exhaust]
            .into_iter()
            .flatten()
            .find(|x| x.def.id == "bludgeon")
            .unwrap();
        assert_eq!(b.fixed_cost(), Some(3), "只降本回合,回合结束恢复原价");

        // 升级版:整场战斗都是 1 费
        let mut c = staged(&["enlightenment", "bludgeon"], &["enlightenment", "bludgeon"]);
        let idx = hand_idx(&c, "enlightenment");
        c.hand[idx].upgrade();
        c.play_card(idx, None).unwrap();
        c.end_turn();
        let b = [&c.hand, &c.draw, &c.discard, &c.exhaust]
            .into_iter()
            .flatten()
            .find(|x| x.def.id == "bludgeon")
            .unwrap();
        assert_eq!(b.fixed_cost(), Some(1), "升级版整场战斗都是 1 费");
    }

    #[test]
    fn hand_of_greed_pays_only_on_a_kill() {
        let mut c = combat_with("jaw_worm_solo", &["hand_of_greed"; 5]);
        c.enemies[0].hp = 5;
        let idx = hand_idx(&c, "hand_of_greed");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "20 伤打死 5 血");
        assert_eq!(c.gold_gained, 20, "致命一击才给金币");

        let mut c = combat_with("jaw_worm_solo", &["hand_of_greed"; 5]);
        c.enemies[0].hp = 200;
        c.enemies[0].max_hp = 200;
        let idx = hand_idx(&c, "hand_of_greed");
        c.play_card(idx, Some(0)).unwrap();
        assert_eq!(c.gold_gained, 0, "打不死就没金币");
        assert_eq!(c.enemies[0].hp, 180);
    }

    #[test]
    fn panache_pays_out_on_every_fifth_card() {
        let deck = ["panache", "defend", "defend", "defend", "defend", "defend"];
        let mut c = staged(&deck, &deck);
        let hp0 = c.enemies[0].hp;
        let idx = hand_idx(&c, "panache");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.enemies[0].hp, hp0, "浮夸自己不打伤害");
        for _ in 0..4 {
            let i = hand_idx(&c, "defend");
            c.play_card(i, None).unwrap();
        }
        // 浮夸 + 4 张防御 = 本回合第 5 张牌,触发 10 点全体伤害
        assert_eq!(c.enemies[0].hp, hp0 - 10);
        // 第 10 张牌才会再触发一次,这里只有 5 张
        assert_eq!(c.enemies[0].hp, hp0 - 10);
    }

    #[test]
    fn sadistic_nature_punishes_debuffs_on_enemies() {
        let mut c = staged(&["sadistic_nature", "blind"], &["sadistic_nature", "blind"]);
        let idx = hand_idx(&c, "sadistic_nature");
        c.play_card(idx, None).unwrap();
        let hp0 = c.enemies[0].hp;
        let idx = hand_idx(&c, "blind");
        c.play_card(idx, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp0 - 5, "上减益就挨 5 点");
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 2, "减益照样生效");
    }

    #[test]
    fn panacea_artifact_refuses_one_debuff() {
        let mut c = staged(&["panacea"], &["panacea"]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::Artifact), 1);
        assert!(c.exhaust.iter().any(|x| x.def.id == "panacea"), "用完就耗掉");
        // 敌人下一次给减益时被顶掉
        c.add_player_status_from_enemy(Status::Frail, 2);
        assert!(!c.player.statuses.has(Status::Frail));
        assert_eq!(c.player.statuses.get(Status::Artifact), 0, "神器用掉一层");
        // 没有神器了就正常生效
        c.add_player_status_from_enemy(Status::Frail, 2);
        assert_eq!(c.player.statuses.get(Status::Frail), 2);
    }

    #[test]
    fn panic_button_stops_card_block_for_two_turns() {
        let deck = [
            "panic_button",
            "good_instincts",
            "good_instincts",
            "good_instincts",
            "good_instincts",
        ];
        let mut c = staged(&deck, &deck);
        let idx = hand_idx(&c, "panic_button");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.player.block, 30);
        assert_eq!(c.player.statuses.get(Status::NoBlock), 2);
        let idx = hand_idx(&c, "good_instincts");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.player.block, 30, "两回合内卡牌给不了格挡");

        // 第一个回合结束:NoBlock 2 -> 1
        c.end_turn();
        c.player.block = 0;
        let idx = hand_idx(&c, "good_instincts");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.player.block, 0, "第二回合还是拿不到");

        // 第二个回合结束:NoBlock 归零
        c.end_turn();
        c.player.block = 0;
        let idx = hand_idx(&c, "good_instincts");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.player.block, 6, "两回合过后恢复正常");
    }

    #[test]
    fn dark_shackles_strength_comes_back_next_turn() {
        let mut c = staged(&["dark_shackles", "strike"], &["dark_shackles", "strike"]);
        c.enemies[0].statuses.add(Status::Strength, 5);
        let idx = hand_idx(&c, "dark_shackles");
        c.play_card(idx, Some(0)).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 0, "这回合力量被扣光");
        // 它这一击因此从 16 掉到 11
        c.end_turn();
        assert_eq!(c.player.hp, 80 - 11, "力量被扣掉后攻击也变弱了");
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 5, "回合结束后补回来");
    }

    #[test]
    fn the_bomb_explodes_after_three_turns() {
        let mut c = staged(&["the_bomb", "defend"], &["the_bomb", "defend"]);
        c.enemies[0].hp = 200;
        c.enemies[0].max_hp = 200;
        let idx = hand_idx(&c, "the_bomb");
        c.play_card(idx, None).unwrap();
        c.end_turn();
        c.end_turn();
        // 第三个回合结束时爆炸;先把敌人攒的格挡抹掉,免得吃掉爆炸伤害
        c.enemies[0].block = 0;
        let before = c.damage_dealt;
        c.end_turn();
        assert_eq!(c.damage_dealt - before, 40, "三回合后炸 40 点");
    }

    #[test]
    fn discovery_offers_three_class_cards_and_gives_one() {
        let mut c = staged(&["discovery"], &["discovery"]);
        c.play_card(0, None).unwrap();
        let ch = c.choice.as_ref().expect("发现要开选择");
        assert_eq!(ch.source, ChoiceSource::Offered);
        let cands = c.choice_candidates();
        assert_eq!(cands.len(), 3, "亮三张");
        for (_, card) in &cands {
            assert_eq!(cards::pool_of(card.def), "class", "发现只给本职业牌");
        }
        let picked = cands[0].1.def.id;
        c.choose(0).unwrap();
        assert!(c.choice.is_none(), "选完就收工");
        assert_eq!(c.hand.len(), 1);
        assert_eq!(c.hand[0].def.id, picked);
        assert!(c.hand[0].free_this_turn, "挑中的这张本回合 0 费");
        // 没挑中的两张直接消失,不会被塞进别的牌堆
        let total = c.hand.len() + c.draw.len() + c.discard.len() + c.exhaust.len();
        assert_eq!(total, 2, "只剩手里的那张和消耗堆里的发现");
    }

    #[test]
    fn secret_technique_pulls_only_skills_from_the_draw_pile() {
        let mut c = staged(
            &["secret_technique", "strike", "defend", "bash"],
            &["secret_technique"],
        );
        let idx = hand_idx(&c, "secret_technique");
        c.play_card(idx, None).unwrap();
        let ch = c.choice.as_ref().expect("秘技要开选择");
        assert_eq!(ch.source, ChoiceSource::Draw);
        let cands = c.choice_candidates();
        assert_eq!(cands.len(), 1, "抽牌堆里只有一张技能");
        let (idx, card) = cands[0];
        assert_eq!(card.def.id, "defend");
        c.choose(idx).unwrap();
        assert!(c.hand.iter().any(|x| x.def.id == "defend"), "技能进了手牌");
        assert!(!c.draw.iter().any(|x| x.def.id == "defend"), "从抽牌堆里拿走了");
        assert!(c.draw.iter().any(|x| x.def.id == "strike"), "攻击牌不动");
    }

    #[test]
    fn transmutation_pours_x_colorless_cards_into_hand() {
        let mut c = staged(&["transmutation"], &["transmutation"]);
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.energy, 0, "X 费吃掉全部能量");
        assert_eq!(c.hand.len(), 3, "X=3 给三张");
        for card in &c.hand {
            assert_eq!(cards::pool_of(card.def), "colorless");
            assert!(card.free_this_turn, "嬗变给的牌本回合 0 费");
            assert_eq!(card.fixed_cost(), Some(0));
        }
    }

    #[test]
    fn mind_blast_is_innate_and_hits_for_the_draw_pile() {
        let deck = ["mind_blast", "defend", "defend", "defend", "defend", "defend"];
        let mut c = combat_with("jaw_worm_solo", &deck);
        assert!(
            c.hand.iter().any(|x| x.def.id == "mind_blast"),
            "天生牌开局就在手上"
        );
        c.hand.clear();
        c.draw.clear();
        c.hand.push(cards::card("mind_blast"));
        for _ in 0..5 {
            c.draw.push(cards::card("defend"));
        }
        c.energy = 9;
        let hp0 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp0 - 5, "伤害等于抽牌堆张数");
    }

    #[test]
    fn forethought_parking_a_card_keeps_it_free() {
        let mut c = staged(&["forethought", "strike"], &["forethought", "strike"]);
        let idx = hand_idx(&c, "forethought");
        c.play_card(idx, None).unwrap();
        let ch = c.choice.as_ref().expect("预谋要开选择");
        assert_eq!(ch.source, ChoiceSource::Hand);
        assert_eq!(ch.need, 1);
        let idx = c.choice_candidates()[0].0;
        assert_eq!(c.hand[idx].def.id, "strike");
        c.choose(idx).unwrap();
        assert!(c.choice.is_none());
        let card = c.draw.first().expect("放到抽牌堆底");
        assert_eq!(card.def.id, "strike");
        assert_eq!(card.fixed_cost(), Some(0), "直到打出前都是 0 费");
        // 过了一个回合依然 0 费
        c.end_turn();
        let card = [&c.hand, &c.draw, &c.discard, &c.exhaust]
            .into_iter()
            .flatten()
            .find(|x| x.def.id == "strike")
            .unwrap();
        assert_eq!(card.fixed_cost(), Some(0));
    }

    #[test]
    fn purity_exhausts_a_chosen_subset() {
        let mut c = staged(
            &["purity", "strike", "defend", "bash"],
            &["purity", "strike", "defend", "bash"],
        );
        let idx = hand_idx(&c, "purity");
        c.play_card(idx, None).unwrap();
        let ch = c.choice.as_ref().expect("净化要开选择");
        assert_eq!(ch.need, 3, "最多三张");
        // 只挑两张:打击与重击
        let picks: Vec<usize> = c
            .hand
            .iter()
            .enumerate()
            .filter(|(_, x)| matches!(x.def.id, "strike" | "bash"))
            .map(|(i, _)| i)
            .collect();
        for idx in picks.into_iter().rev() {
            c.choose(idx).unwrap();
        }
        assert!(c.choice.is_some(), "没选满还要等玩家收工");
        c.finish_choice();
        assert!(c.choice.is_none());
        assert_eq!(c.hand.len(), 1);
        assert_eq!(c.hand[0].def.id, "defend");
        assert_eq!(c.exhaust.len(), 3, "被耗掉的两张 + 净化自己");
    }

    #[test]
    fn chrysalis_shuffles_free_class_skills_into_the_draw_pile() {
        let mut c = staged(&["chrysalis"], &["chrysalis"]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.draw.len(), 3, "洗三张进去");
        for card in &c.draw {
            assert_eq!(card.kind(), crate::core::card::CardType::Skill);
            assert_eq!(cards::pool_of(card.def), "class", "只洗本职业的牌");
            assert!(card.free_combat);
        }
        // 跨回合依然是 0 费
        c.end_turn();
        assert_eq!(c.hand.len(), 3, "下回合抽到手上");
        assert!(c.hand.iter().all(|x| x.fixed_cost() == Some(0)));
    }

    #[test]
    fn magnetism_and_mayhem_fire_at_the_start_of_the_turn() {
        // 磁力:回合开始白给一张无色牌
        let mut c = staged(&["magnetism", "strike"], &["magnetism"]);
        let idx = hand_idx(&c, "magnetism");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::Magnetism), 1);
        // 磁力自己也是无色牌,先把它从弃牌堆拿走,免得混进计数
        c.discard.clear();
        c.end_turn();
        let gifted = c
            .hand
            .iter()
            .filter(|x| cards::pool_of(x.def) == "colorless")
            .count();
        assert_eq!(gifted, 1, "回合开始给一张无色牌");

        // 混乱:回合开始替我们打出手牌堆顶那张
        let deck = [
            "mayhem", "defend", "defend", "defend", "defend", "defend", "defend", "defend",
        ];
        let mut c = staged(&deck, &["mayhem"]);
        let idx = hand_idx(&c, "mayhem");
        c.play_card(idx, None).unwrap();
        c.end_turn();
        assert_eq!(c.player.block, 5, "打出的那张防御给了 5 格挡");
    }

    #[test]
    fn impatience_and_violence_scan_hand_and_draw_pile() {
        // 手里没有攻击牌就抽两张
        let mut c = staged(&["impatience", "defend"], &["impatience", "defend"]);
        for _ in 0..4 {
            c.draw.push(cards::card("defend"));
        }
        let idx = hand_idx(&c, "impatience");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.hand.len(), 3, "剩一张防御,再抽 2 张");

        // 手里有攻击牌就不抽
        let mut c = staged(&["impatience", "strike"], &["impatience", "strike"]);
        for _ in 0..4 {
            c.draw.push(cards::card("defend"));
        }
        let idx = hand_idx(&c, "impatience");
        c.play_card(idx, None).unwrap();
        assert_eq!(c.hand.len(), 1, "有攻击牌不抽");

        // 暴动:抽牌堆里随机三张攻击进手
        let mut c = staged(&["violence"], &["violence"]);
        for _ in 0..2 {
            c.draw.push(cards::card("defend"));
        }
        for _ in 0..4 {
            c.draw.push(cards::card("strike"));
        }
        c.play_card(0, None).unwrap();
        let drawn = c.hand.iter().filter(|x| x.def.id == "strike").count();
        assert_eq!(drawn, 3, "三张攻击进手");
        assert_eq!(c.draw.len(), 3, "抽牌堆只剩两张防御和一张打击");
    }

    #[test]
    fn the_small_colorless_tricks_do_what_they_say() {
        // 包扎:回血
        let mut c = staged(&["bandage_up"], &["bandage_up"]);
        c.player.hp = 50;
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.hp, 54);
        assert!(c.exhaust.iter().any(|x| x.def.id == "bandage_up"));

        // 深呼吸:弃牌堆洗回抽牌堆,再抽一张
        let mut c = staged(&["deep_breath", "strike"], &["deep_breath"]);
        c.discard.push(cards::card("defend"));
        c.play_card(0, None).unwrap();
        assert_eq!(c.discard.len(), 1, "洗完之后弃牌堆只剩深呼吸自己");
        assert_eq!(c.discard[0].def.id, "deep_breath");
        assert_eq!(c.hand.len(), 1, "顺手抽一张");
        assert!(matches!(c.hand[0].def.id, "strike" | "defend"));

        // 疾行:2 格挡 + 抽 1
        let mut c = staged(&["finesse", "defend"], &["finesse"]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 2);
        assert_eq!(c.hand.len(), 1, "顺带抽一张");

        // 好直觉:6 格挡
        let mut c = staged(&["good_instincts"], &["good_instincts"]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 6);

        // 快斩:7 伤
        let mut c = staged(&["swift_strike"], &["swift_strike"]);
        let hp0 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp0 - 7);

        // 闪钢:3 伤 + 抽 1
        let mut c = staged(&["flash_of_steel", "defend"], &["flash_of_steel"]);
        let hp0 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp0 - 3);
        assert_eq!(c.hand.len(), 1, "打完抽一张");

        // 致盲 / 绊倒:上减益
        let mut c = staged(&["blind"], &["blind"]);
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 2);

        let mut c = staged(&["trip"], &["trip"]);
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 2);

        // 大师策略:抽三张后自己消耗
        let mut c = staged(&["master_of_strategy"], &["master_of_strategy"]);
        for _ in 0..4 {
            c.draw.push(cards::card("defend"));
        }
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 3);
        assert!(c.exhaust.iter().any(|x| x.def.id == "master_of_strategy"));

        // 临时想一下:抽二之后放一张回堆顶
        let mut c = staged(&["thinking_ahead", "strike"], &["thinking_ahead"]);
        c.draw.push(cards::card("defend"));
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 2, "先抽两张");
        let idx = c.choice_candidates()[0].0;
        c.choose(idx).unwrap();
        assert_eq!(c.draw.len(), 1);
        assert!(c.draw[0].topped > 0, "被放到抽牌堆顶");
        assert!(c.exhaust.iter().any(|x| x.def.id == "thinking_ahead"), "用完消耗");

        // 万事通:随机一张无色牌进手
        let mut c = staged(&["jack_of_all_trades"], &["jack_of_all_trades"]);
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 1);
        assert_eq!(cards::pool_of(c.hand[0].def), "colorless");
    }

    /// 垫满格挡的场景:敌人打不动玩家,测的只是回合结束/打牌触发的那些效果
    fn guarded(deck: &[&str]) -> Combat {
        let mut c = combat_with("jaw_worm_solo", deck);
        c.player.hp = 80;
        c.player.block = 999;
        c.hand.clear();
        c.draw.clear();
        c
    }

    /// 腐烂:回合结束时掉 2 血(直接掉血,不吃格挡)
    #[test]
    fn decay_deals_two_damage_at_end_of_turn() {
        let mut c = guarded(&["decay"]);
        c.hand = vec![card("decay"), card("strike")];
        c.end_turn();
        assert_eq!(c.player.hp, 78, "回合结束掉 2 血");
    }

    /// 怀疑:回合结束时拿 1 层 Weak,而且不会被当场递减掉
    #[test]
    fn doubt_gives_weak_at_end_of_turn() {
        let mut c = guarded(&["doubt"]);
        c.hand = vec![card("doubt")];
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Weak), 1, "回合结束拿 1 层 Weak");
    }

    /// 羞耻:回合结束时拿 1 层 Frail,Frail 参与格挡结算
    #[test]
    fn shame_gives_frail_and_frail_cuts_block() {
        let mut c = guarded(&["shame"]);
        c.hand = vec![card("shame")];
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Frail), 1);
        // 新回合:一张 Defend 只给 floor(5 * 0.75) = 3 格挡
        c.player.block = 0;
        c.hand = vec![card("defend")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 3, "Frail 让格挡打七五折");
    }

    /// 悔恨:回合结束时按手牌张数掉血
    #[test]
    fn regret_loses_hp_equal_to_hand_size() {
        let mut c = guarded(&["regret"]);
        c.hand = vec![card("regret"), card("strike"), card("strike"), card("defend")];
        c.end_turn();
        assert_eq!(c.player.hp, 76, "手里 4 张就掉 4 血");
    }

    /// 虚空:抽到时掉 1 点能量,能量见底也不为负
    #[test]
    fn void_costs_energy_when_drawn() {
        let mut c = guarded(&["strike"]);
        c.draw = vec![card("strike"), card("void")];
        c.energy = 3;
        c.draw_cards(2);
        assert!(c.hand.iter().any(|x| x.def.id == "void"), "虚空进手牌");
        assert_eq!(c.energy, 2, "抽到虚空掉 1 能量");

        c.energy = 0;
        c.draw = vec![card("void")];
        c.draw_cards(1);
        assert_eq!(c.energy, 0, "能量不会变成负数");
    }

    /// 痛苦:它在手里时,打出别的牌就掉 1 血
    #[test]
    fn pain_costs_hp_when_another_card_is_played() {
        let mut c = guarded(&["strike"]);
        c.hand = vec![card("pain"), card("strike"), card("strike")];
        c.energy = 3;
        assert_eq!(c.play_card(0, None), Err("unplayable"), "痛苦自己打不出去");
        c.play_card(1, Some(0)).unwrap();
        assert_eq!(c.player.hp, 79, "打出别的牌掉 1 血");

        // 痛苦离开手牌之后就不再掉血
        c.hand = vec![card("strike")];
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.player.hp, 79);
    }

    /// 反常:它在手里时,本回合最多打出 3 张牌
    #[test]
    fn normality_caps_plays_at_three() {
        // 先看不带反常时的对照:第 4 张照样能打
        let mut c = guarded(&["defend"]);
        c.hand = vec![card("defend"); 5];
        c.energy = 10;
        for _ in 0..4 {
            c.play_card(0, None).unwrap();
        }
        assert_eq!(c.cards_played, 4);

        // 手里有反常:第 4 张被拦下
        let mut c = guarded(&["defend"]);
        c.hand = vec![
            card("normality"),
            card("defend"),
            card("defend"),
            card("defend"),
            card("defend"),
        ];
        c.energy = 10;
        for _ in 0..3 {
            c.play_card(1, None).unwrap();
        }
        assert_eq!(c.cards_played, 3);
        assert_eq!(
            c.play_card(1, None),
            Err("normality: too many cards played this turn")
        );
    }

    /// 傲慢:回合结束时在抽牌堆顶留一张副本
    #[test]
    fn pride_copies_itself_onto_the_draw_pile() {
        let mut c = guarded(&["pride"]);
        c.hand = vec![card("pride")];
        // 抽牌堆塞够 6 张,新回合不会洗到弃牌堆里的原牌
        c.draw = vec![card("strike"); 6];
        c.end_turn();
        assert_eq!(c.hand[0].def.id, "pride", "副本躺在抽牌堆顶,新回合第一张抽到");
        assert!(c.discard.iter().any(|x| x.def.id == "pride"), "原牌进弃牌堆");
        let prides = c
            .hand
            .iter()
            .chain(c.draw.iter())
            .chain(c.discard.iter())
            .chain(c.exhaust.iter())
            .filter(|x| x.def.id == "pride")
            .count();
        assert_eq!(prides, 2, "原牌 + 一张副本");
    }

    /// 纠缠:Innate,开局就在手里
    #[test]
    fn writhe_is_innate_and_starts_in_hand() {
        let c = combat_with(
            "jaw_worm_solo",
            &["strike", "strike", "strike", "strike", "strike", "writhe"],
        );
        assert!(c.hand.iter().any(|x| x.def.id == "writhe"), "开局在手");
        assert_eq!(c.hand.len(), 6, "5 张起手 + 1 张 Innate");
        assert!(!c.draw.iter().any(|x| x.def.id == "writhe"));
    }

    /// 死灵诅咒:被消耗也逃不掉,补一张新的回手牌
    #[test]
    fn necronomicurse_escapes_the_exhaust_pile() {
        let mut c = guarded(&["strike"]);
        c.hand = vec![card("strike")];
        c.exhaust_card(card("necronomicurse"));
        assert!(c.exhaust.iter().any(|x| x.def.id == "necronomicurse"));
        assert!(
            c.hand.iter().any(|x| x.def.id == "necronomicurse"),
            "消耗之后手里又回来一张"
        );
    }
}
