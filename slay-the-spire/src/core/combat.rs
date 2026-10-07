// 战斗引擎:回合流转、抽牌弃牌、卡牌结算、敌人行动.
// 纯逻辑,不碰终端;一局流程(run.rs)只负责在一次战斗前后同步生命与遗物结算.
use crate::core::card::{CardInstance, Cost, Effect, Target};
use crate::core::cards;
use crate::core::enemy::{
    CardSpot, Encounter, EnemyDef, EnemyFx, EnemyKind, EnemyState, Intent, PickCtx, Scope, Special,
};
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
    /// 死亡触发是否已结算,避免重复触发
    pub death_done: bool,
    /// 本回合被扣掉的力量(黑暗镣铐),回合结束回补
    pub temp_strength: i32,
    /// 已经脱离战斗(逃跑 / 被首领带走),不会再行动也不再算敌人
    pub escaped: bool,
    /// 参考实现里的槽位:站位与召唤都按它排,下标会变它不会
    pub slot: usize,
    /// 出生序号.召唤会把队里其它怪的下标顶走,回合内靠它认住"正在行动的那只"
    pub uid: u64,
    /// 跨回合的怪物状态(回合数、招式历史、各种计数)
    pub state: EnemyState,
}

impl Enemy {
    /// 还站着、能被选中:逃跑的和半死的觉醒者都不算
    pub fn alive(&self) -> bool {
        self.hp > 0 && !self.escaped && !self.state.half_dead
    }

    /// 还在战斗里(半死的觉醒者仍然要回合)
    pub fn up(&self) -> bool {
        (self.hp > 0 || self.state.half_dead) && !self.escaped
    }

    pub fn dead(&self) -> bool {
        !self.up()
    }

    /// 这只怪是不是召唤物
    pub fn is_minion(&self) -> bool {
        self.statuses.holds(Status::Minion)
    }

    pub fn intent(&self) -> Intent {
        if self.state.half_dead {
            return Intent::Unknown;
        }
        self.def.moves[self.next_move].intent
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
    /// 玩家身上的金币(抢劫类敌人要用)
    pub gold: i32,
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
    /// 玩家身上的金币(抢劫类敌人要用,由一局流程填)
    pub player_gold: i32,
    /// 被圆球哨卫偷走的牌:(持有者下标, 牌),持有者死了就还回来
    pub stasis: Vec<(usize, CardInstance)>,
    /// 上一招实际打掉的血(吸血与"格挡等于伤害"要看它)
    last_hit: i32,
    /// 战斗中永久塞进牌组的牌(寄生),一局流程在战斗结束后收走
    pub deck_cards: Vec<CardInstance>,
    /// 时间吞噬者的时间扭曲:这一张牌打完就要结束回合
    pub force_end_turn: bool,
    /// 玩家最近一次指向的敌人(被夹击时判断从哪边挨打)
    pub facing: usize,
    /// 下一只怪的出生序号
    next_uid: u64,
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
                if *n == 0 {
                    statuses.mark(*s);
                } else {
                    statuses.add(*s, *n);
                }
            }
            let mut state = EnemyState::default();
            let block = def.start_block;
            let mut spawn = crate::core::enemy::SpawnCtx {
                rng: &mut rng,
                statuses: &mut statuses,
                state: &mut state,
            };
            (def.spawn)(&mut spawn);
            enemies.push(Enemy {
                def,
                name,
                hp,
                max_hp: hp,
                block,
                statuses,
                next_move: 0,
                death_done: false,
                temp_strength: 0,
                escaped: false,
                // 站位按遭遇表给的槽位,不一定是 0,1,2...(自动机的铜球要排在它前面)
                slot: crate::core::enemies::initial_slot(enc, i),
                uid: i as u64,
                state,
            });
        }

        let enemies_len = enemies.len();
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
            player_gold: setup.gold,
            stasis: Vec::new(),
            last_hit: 0,
            deck_cards: Vec::new(),
            force_end_turn: false,
            facing: 1,
            next_uid: enemies_len as u64,
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
        // 开局的第一次掷招(参考实现里这一掷看到的是 turn == 0)
        for i in 0..c.enemies.len() {
            c.roll_first_move(i);
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
        // 时间吞噬者的头疼:下回合少抽几张
        let less = self.player.statuses.get(Status::DrawReduction).max(0) as usize;
        if less > 0 {
            self.push_log(
                LogKind::Player,
                format!("Draw Reduction: you draw {less} fewer cards"),
            );
        }
        self.draw_cards((DRAW_PER_TURN + extra_draw).saturating_sub(less));
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
        let kind = card.kind();
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
        self.note_card_played(kind);
        self.check_win();
    }

    /// 记一张打出的牌;浮夸每打满 5 张就对所有敌人来一下;痛苦手里有就掉血
    fn note_card_played(&mut self, kind: crate::core::card::CardType) {
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
        self.on_enemy_card_hooks(kind);
        let pan = self.player.statuses.get(Status::Panache);
        if pan <= 0 || self.cards_played % 5 != 0 {
            return;
        }
        for i in self.alive_enemies() {
            self.damage_enemy_plain(i, pan);
        }
        self.push_log(
            LogKind::Player,
            format!("Panache deals {pan} to all enemies"),
        );
        self.settle_deaths();
    }

    /// 敌人身上"玩家每打出一张牌"就触发的机制
    fn on_enemy_card_hooks(&mut self, kind: crate::core::card::CardType) {
        use crate::core::card::CardType;
        for i in 0..self.enemies.len() {
            if !self.enemies[i].alive() {
                continue;
            }
            let name = self.enemies[i].name.clone();
            // 慢速:每打一张牌就让巨大头颅多挨一成
            if self.enemies[i].statuses.holds(Status::Slow) {
                let slow = self.enemies[i].statuses.get(Status::Slow);
                self.enemies[i].statuses.set(Status::Slow, slow + 1);
            }
            // 死亡律动:打一张掉一次血
            let beat = self.enemies[i].statuses.get(Status::BeatOfDeath);
            if beat > 0 {
                let (taken, _) = self.hit_player(beat);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Beat of Death deals {taken}"),
                );
                if self.phase == Phase::Lost {
                    return;
                }
            }
            // 尖刺外壳:打攻击牌就挨刺
            let hide = self.enemies[i].statuses.get(Status::SharpHide);
            if hide > 0 && kind == CardType::Attack {
                let (taken, _) = self.hit_player(hide);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Sharp Hide deals {taken}"),
                );
                if self.phase == Phase::Lost {
                    return;
                }
            }
            // 好奇:打能力牌就给觉醒者涨力量
            let cur = self.enemies[i].statuses.get(Status::Curiosity);
            if cur > 0 && kind == CardType::Power {
                self.enemies[i].statuses.add(Status::Strength, cur);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Curiosity: +{cur} Strength"),
                );
            }
            // 狂怒:打技能牌就给小鬼头目涨力量
            let enrage = self.enemies[i].statuses.get(Status::Enrage);
            if enrage > 0 && kind == CardType::Skill {
                self.enemies[i].statuses.add(Status::Strength, enrage);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Enrage: +{enrage} Strength"),
                );
            }
            // 诅咒之眼:打非攻击牌就往抽牌堆塞眩晕
            let hex = self.enemies[i].statuses.get(Status::Hex);
            if hex > 0 && kind != CardType::Attack {
                for _ in 0..hex {
                    let mut inst = CardInstance::new(cards::card_def_or_panic("dazed"));
                    self.fix_new_card(&mut inst);
                    let pos = self.rng.below(self.draw.len() as u32 + 1) as usize;
                    self.draw.insert(pos, inst);
                }
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Hex shuffles {hex} Dazed into your draw pile"),
                );
            }
            // 时间扭曲:打满 12 张就结束这一回合
            if self.enemies[i].statuses.holds(Status::TimeWarp) {
                let n = self.enemies[i].statuses.get(Status::TimeWarp) + 1;
                if n >= 12 {
                    self.enemies[i].statuses.set(Status::TimeWarp, 0);
                    self.enemies[i].statuses.add(Status::Strength, 2);
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name}'s Time Warp stops time (turn over)"),
                    );
                    self.force_end_turn = true;
                } else {
                    self.enemies[i].statuses.set(Status::TimeWarp, n);
                }
            }
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
            // 混乱:抽到的牌费用随机化
            if self.player.statuses.has(Status::Confused) {
                let i = self.hand.len() - 1;
                let base = match self.hand[i].def.cost {
                    Cost::Fixed(n) => n as i32,
                    _ => -1,
                };
                if base >= 0 {
                    let target = self.rng.below(4) as i32;
                    self.hand[i].cost_delta = target - base;
                }
            }
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
                self.damage_enemy_plain(i, fire);
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
        self.force_end_turn = false;
        // 缠绕:回合结束先挨一下(非攻击伤害)
        let constricted = self.player.statuses.get(Status::Constricted);
        if constricted > 0 {
            let (taken, _) = self.hit_player(constricted);
            self.push_log(
                LogKind::Enemy,
                format!("Constricted tightens: {taken} damage"),
            );
            if self.phase != Phase::PlayerTurn {
                return;
            }
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
                self.damage_enemy_plain(i, d);
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
                if self.damage_enemy_plain(i, combust) > 0 {
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
        // 这一回合轮到谁,开局就定死(用出生序号认怪).召唤会把别人顶走,
        // 所以不能存下标;新召唤出来的也不在这一轮里,下一轮才动
        let actors: Vec<u64> = self.enemies.iter().filter(|e| e.up()).map(|e| e.uid).collect();
        for uid in actors {
            if self.phase != Phase::EnemyTurn {
                return;
            }
            let Some(idx) = self.enemies.iter().position(|e| e.uid == uid) else {
                continue;
            };
            if !self.enemies[idx].up() {
                continue;
            }
            self.enemy_act(idx);
        }
        if self.phase != Phase::EnemyTurn {
            return;
        }
        self.check_win();
        if self.phase != Phase::EnemyTurn {
            return;
        }
        self.phase = Phase::PlayerTurn;
        self.start_turn(0);
    }

    fn enemy_act(&mut self, idx: usize) {
        let def = self.enemies[idx].def;
        let uid = self.enemies[idx].uid;
        let move_idx = self.enemies[idx].next_move;
        let mname = def.moves[move_idx].name;
        let name = self.enemies[idx].name.clone();
        // 本回合开始的伤害统计(心脏的无敌按回合算)
        let turn = {
            let st = &mut self.enemies[idx].state;
            st.turns += 1;
            st.taken_this_turn = 0;
            st.turns
        };
        if self.enemies[idx].statuses.holds(Status::Asleep) {
            self.push_log(LogKind::Enemy, format!("{name} is asleep"));
        }
        for fx in def.moves[move_idx].effects {
            // 招式里可能召唤/分裂,会把队里的下标挪走,所以每次按出生序号重新认
            let Some(cur) = self.enemies.iter().position(|e| e.uid == uid) else {
                return;
            };
            self.apply_enemy_fx(cur, *fx, turn, &name, mname);
            if self.phase == Phase::Lost {
                return;
            }
        }
        let Some(idx) = self.enemies.iter().position(|e| e.uid == uid) else {
            return;
        };
        // 觉醒者复活:半死那一回合就是来补血的,血补上就进二阶段
        if def.special == Special::Rebirth
            && self.enemies[idx].state.half_dead
            && self.enemies[idx].hp > 0
        {
            self.enemies[idx].state.half_dead = false;
            self.enemies[idx].state.phase2 = true;
            self.push_log(LogKind::Enemy, format!("{name} rises again!"));
        }
        // 自己已经离场(分裂/自爆)就不用收尾了
        if !self.enemies[idx].up() {
            return;
        }
        self.enemy_end_of_turn(idx, &name);
        if self.phase == Phase::Lost {
            return;
        }
        // 无形循环:复仇女神每次行动完都会补上两层
        if def.special == Special::Intangible
            && !self.enemies[idx].statuses.has(Status::Intangible)
        {
            self.enemies[idx].statuses.add(Status::Intangible, 2);
            self.push_log(LogKind::Enemy, format!("{name} becomes intangible"));
        }
        // 记进出招历史,再定下一招
        let e = &mut self.enemies[idx];
        e.state.prev = e.state.last;
        e.state.last = Some(move_idx);
        self.pick_next_move(idx);
    }

    /// 一段敌人招式的效果
    fn apply_enemy_fx(&mut self, idx: usize, fx: EnemyFx, turn: u32, name: &str, mname: &str) {
        match fx {
            EnemyFx::Attack { amount, times } => {
                self.enemy_attack(idx, amount, times, name, mname);
            }
            EnemyFx::AttackScaling {
                amount,
                per_turn,
                cap,
                times,
            } => {
                let extra = per_turn * turn.saturating_sub(1).min(cap) as i32;
                self.enemy_attack(idx, amount + extra, times, name, mname);
            }
            EnemyFx::AttackGrowing { amount } => {
                let hits = ((turn + 1) / 2).max(1) as u8;
                self.enemy_attack(idx, amount, hits, name, mname);
            }
            EnemyFx::AttackStabCount { amount } => {
                let hits = self.enemies[idx].state.stab.max(1) as u8;
                self.enemy_attack(idx, amount, hits, name, mname);
            }
            EnemyFx::AttackRolled { times } => {
                let d = self.enemies[idx].state.rolled;
                self.enemy_attack(idx, d, times, name, mname);
            }
            EnemyFx::PlainDamage { amount } => {
                let (taken, _) = self.hit_player(amount);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} blasts you for {taken} damage"),
                );
            }
            EnemyFx::Block { amount, scope } => {
                let targets = self.scope_targets(idx, scope);
                for t in targets {
                    self.enemies[t].block += amount;
                    let who = self.enemies[t].name.clone();
                    self.push_log(
                        LogKind::Enemy,
                        format!("{who} gains {amount} Block ({mname})"),
                    );
                }
            }
            EnemyFx::BlockFromDamage => {
                let n = self.last_hit;
                self.enemies[idx].block += n;
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} gains {n} Block ({mname})"),
                );
            }
            EnemyFx::GainStatus { status, n, scope } => {
                for t in self.scope_targets(idx, scope) {
                    self.enemies[t].statuses.add(status, n);
                }
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} gains {n} {} ({mname})", status.name()),
                );
            }
            EnemyFx::PlayerStatus { status, n } => {
                self.add_player_status_from_enemy(status, n);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} applies {n} {} to you", status.name()),
                );
            }
            EnemyFx::Heal { n, scope } => {
                for t in self.scope_targets(idx, scope) {
                    let healed = self.enemies[t].max_hp.min(self.enemies[t].hp + n) - self.enemies[t].hp;
                    self.enemies[t].hp += healed;
                    if healed > 0 {
                        let who = self.enemies[t].name.clone();
                        self.push_log(LogKind::Enemy, format!("{who} heals {healed} HP"));
                    }
                }
            }
            EnemyFx::HealFromDamage => {
                let n = self.last_hit;
                if n > 0 {
                    let healed = self.enemies[idx].max_hp.min(self.enemies[idx].hp + n) - self.enemies[idx].hp;
                    self.enemies[idx].hp += healed;
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} drains {healed} HP ({mname})"),
                    );
                }
            }
            EnemyFx::HealToHalf => {
                let half = self.enemies[idx].max_hp / 2;
                if self.enemies[idx].hp < half {
                    self.enemies[idx].hp = half;
                    self.push_log(LogKind::Enemy, format!("{name} heals to {half} HP"));
                }
            }
            EnemyFx::ClearDebuffs => {
                self.enemies[idx].statuses.clear_debuffs();
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} shakes off its debuffs"),
                );
            }
            EnemyFx::ResetStrength { n } => {
                let s = self.enemies[idx].statuses.get(Status::Strength);
                if s < 0 {
                    self.enemies[idx].statuses.add(Status::Strength, -s);
                }
                self.enemies[idx].statuses.add(Status::Strength, n);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} gains {n} Strength ({mname})"),
                );
            }
            EnemyFx::Escalate => {
                let stage = {
                    let st = &mut self.enemies[idx].state;
                    st.stage += 1;
                    st.stage
                };
                self.heart_escalate(idx, stage, name);
            }
            EnemyFx::PlayerCard { card, spot, n } => {
                for _ in 0..n {
                    let mut inst = CardInstance::new(cards::card_def_or_panic(card));
                    self.fix_new_card(&mut inst);
                    let label = inst.label();
                    match spot {
                        CardSpot::Discard => self.discard.push(inst),
                        CardSpot::DrawShuffle => {
                            let pos = self.rng.below(self.draw.len() as u32 + 1) as usize;
                            self.draw.insert(pos, inst);
                        }
                        CardSpot::Deck => self.deck_cards.push(inst),
                    }
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} puts a {label} in your deck"),
                    );
                }
            }
            EnemyFx::StealGold { n } => {
                let got = n.min(self.player_gold.max(0));
                self.player_gold -= got;
                self.enemies[idx].state.stolen += got;
                if got > 0 {
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} steals {got} gold from you"),
                    );
                }
            }
            EnemyFx::StealCard => {
                self.enemy_steal_card(idx, name);
            }
            EnemyFx::Summon { ids, slots } => {
                let me = self.enemies[idx].slot;
                for (slot, id) in self.open_slots(me, slots, ids.len()).into_iter().zip(ids) {
                    self.summon_one(id, slot, name);
                }
            }
            EnemyFx::SummonRandom {
                pool,
                count,
                slots,
            } => {
                let me = self.enemies[idx].slot;
                for slot in self.open_slots(me, slots, count as usize) {
                    // 每只单独掷点挑,允许抽到同一只(参考实现就是各抽各的)
                    let i = self.rng.range_inclusive(0, pool.len() as i32 - 1) as usize;
                    let id = pool[i];
                    self.summon_one(id, slot, name);
                }
            }
            EnemyFx::DrawReduction { n } => {
                self.player.statuses.add(Status::DrawReduction, n);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} clouds your next draw"),
                );
            }
            EnemyFx::Charge => {
                self.enemies[idx].state.charge += 1;
            }
            EnemyFx::WakeUp { at_turn } => {
                if turn >= at_turn && self.enemies[idx].statuses.holds(Status::Asleep) {
                    self.enemies[idx].statuses.add(Status::Asleep, -1);
                    let met = self.enemies[idx].statuses.get(Status::Metallicize);
                    if met > 0 {
                        self.enemies[idx].statuses.add(Status::Metallicize, -met);
                    }
                    self.push_log(LogKind::Info, format!("{name} wakes up!"));
                }
            }
            EnemyFx::RollDamage { div, add } => {
                self.enemies[idx].state.rolled = self.player.hp / div.max(1) + add;
            }
            EnemyFx::LoseStatus { status, scope } => {
                for t in self.scope_targets(idx, scope) {
                    let n = self.enemies[t].statuses.get(status);
                    if n > 0 {
                        self.enemies[t].statuses.add(status, -n);
                    } else {
                        self.enemies[t].statuses.add(status, -1);
                    }
                }
            }
            EnemyFx::ForceNext { idx: next } => {
                self.enemies[idx].state.forced = Some(next);
            }
            EnemyFx::Split => {
                self.enemy_split(idx, name);
            }
            EnemyFx::Suicide => {
                self.enemies[idx].hp = 0;
                self.push_log(LogKind::Enemy, format!("{name} is destroyed"));
                self.settle_deaths();
            }
            EnemyFx::Escape => {
                self.enemies[idx].escaped = true;
                self.enemies[idx].death_done = true;
                self.push_log(LogKind::Enemy, format!("{name} escapes"));
                self.check_win();
            }
        }
    }

    /// 一次敌人攻击:算一次伤害,然后打 times 下
    fn enemy_attack(&mut self, idx: usize, amount: i32, times: u8, name: &str, mname: &str) {
        if !self.enemies[idx].alive() {
            return;
        }
        self.shake(ShakeWho::Enemy(idx), -1, ShakeKind::Attack, 0);
        let per = self.enemy_attack_damage(idx, amount);
        let mut blocked_total = 0;
        let mut hit_total = 0;
        for _ in 0..times.max(1) {
            let (taken, blocked) = self.hit_player(per);
            blocked_total += blocked;
            hit_total += taken;
            if taken > 0 {
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} uses {mname}: {taken} damage"),
                );
            }
            // 痛苦刺击:命中一次塞一张伤口
            let stabs = self.enemies[idx].statuses.get(Status::PainfulStabs);
            if stabs > 0 && taken > 0 {
                for _ in 0..stabs {
                    let mut inst = CardInstance::new(cards::card_def_or_panic("wound"));
                    self.fix_new_card(&mut inst);
                    self.discard.push(inst);
                }
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s stabs leave {stabs} Wound in your discard pile"),
                );
            }
            if self.phase == Phase::Lost {
                return;
            }
        }
        self.last_hit = hit_total;
        if blocked_total > 0 {
            self.push_log(
                LogKind::Info,
                format!("{name} hit into {blocked_total} block"),
            );
        }
        // 玩家的荆棘反伤
        if self.relic_thorns > 0 {
            let thorns = self.relic_thorns;
            self.damage_enemy_plain(idx, thorns);
            self.push_log(
                LogKind::Player,
                format!("Thorns deal {thorns} to {name}"),
            );
            self.settle_deaths();
        }
    }

    /// 效果的作用范围
    fn scope_targets(&mut self, idx: usize, scope: Scope) -> Vec<usize> {
        match scope {
            Scope::SelfOnly => vec![idx],
            Scope::Team => (0..self.enemies.len())
                .filter(|i| self.enemies[*i].alive())
                .collect(),
            Scope::Allies => (0..self.enemies.len())
                .filter(|i| *i != idx && self.enemies[*i].alive())
                .collect(),
            Scope::RandomOne => {
                let alive: Vec<usize> =
                    (0..self.enemies.len()).filter(|i| self.enemies[*i].alive()).collect();
                if alive.is_empty() {
                    Vec::new()
                } else {
                    vec![alive[self.rng.below(alive.len() as u32) as usize]]
                }
            }
        }
    }

    /// 心脏的递增增益:每用一次涨一档
    fn heart_escalate(&mut self, idx: usize, stage: i32, name: &str) {
        match stage {
            1 => {
                self.enemies[idx].statuses.add(Status::Artifact, 2);
                self.push_log(LogKind::Enemy, format!("{name} gains 2 Artifact"));
            }
            2 => {
                self.enemies[idx].statuses.add(Status::BeatOfDeath, 1);
                self.push_log(LogKind::Enemy, format!("{name}'s Beat of Death quickens"));
            }
            3 => {
                self.enemies[idx].statuses.add(Status::PainfulStabs, 1);
                self.push_log(LogKind::Enemy, format!("{name} sharpens its stabs"));
            }
            4 => {
                self.enemies[idx].statuses.add(Status::Strength, 10);
                self.push_log(LogKind::Enemy, format!("{name} gains 10 Strength"));
            }
            _ => {
                self.enemies[idx].statuses.add(Status::Strength, 50);
                self.push_log(LogKind::Enemy, format!("{name} gains 50 Strength"));
            }
        }
    }

    /// 圆球哨卫的停滞:从抽牌堆偷一张牌,它死了再还回来
    fn enemy_steal_card(&mut self, idx: usize, name: &str) {
        let pile = if !self.draw.is_empty() {
            &mut self.draw
        } else if !self.discard.is_empty() {
            &mut self.discard
        } else {
            self.push_log(
                LogKind::Enemy,
                format!("{name} finds nothing to steal"),
            );
            return;
        };
        let pick = self.rng.below(pile.len() as u32) as usize;
        let card = pile.remove(pick);
        let label = card.label();
        self.stasis.push((idx, card));
        self.enemies[idx].statuses.add(Status::Stasis, 1);
        self.push_log(
            LogKind::Enemy,
            format!("{name} takes {label} and holds it in stasis"),
        );
    }

    /// 被偷的牌回到玩家手里(哨卫死了就吐出来)
    fn return_stolen_card(&mut self, idx: usize) {
        let Some(pos) = self.stasis.iter().position(|(i, _)| *i == idx) else {
            return;
        };
        let (_, card) = self.stasis.remove(pos);
        let label = card.label();
        if self.hand.len() < HAND_LIMIT {
            self.hand.push(card);
            self.push_log(LogKind::Info, format!("{label} returns to your hand"));
        } else {
            self.discard.push(card);
            self.push_log(LogKind::Info, format!("{label} returns to your discard pile"));
        }
    }

    /// 大史莱姆分裂:自己离场,原地补上两只小史莱姆(生命等于分裂时的血量).
    /// 两只子体占自己那一格和下一格,和参考实现一样顶掉原来的位置
    fn enemy_split(&mut self, idx: usize, name: &str) {
        let Special::Split { a, b } = self.enemies[idx].def.special else {
            return;
        };
        let hp = self.enemies[idx].hp;
        let slot = self.enemies[idx].slot;
        self.enemies[idx].escaped = true;
        self.enemies[idx].death_done = true;
        self.push_log(
            LogKind::Enemy,
            format!("{name} splits into two slimes ({hp} HP each)"),
        );
        self.spawn_enemy_at(a, Some(hp), slot);
        self.spawn_enemy_at(b, Some(hp), slot + 1);
    }

    /// 造一只新敌人.代价是掷生命、掷开场(可能记状态),不进队里
    fn make_enemy(&mut self, id: &str, hp: Option<i32>, slot: usize) -> Enemy {
        let def = crate::core::enemies::enemy_def_or_panic(id);
        let same = self.enemies.iter().filter(|e| e.def.id == def.id).count();
        let name = if same > 0 {
            format!("{} #{}", def.name, same + 1)
        } else {
            def.name.to_string()
        };
        let hp = hp.unwrap_or_else(|| self.rng.range_inclusive(def.hp.0, def.hp.1));
        let mut statuses = Statuses::new();
        for (s, n) in def.innate {
            if *n == 0 {
                statuses.mark(*s);
            } else {
                statuses.add(*s, *n);
            }
        }
        let mut state = EnemyState::default();
        let block = def.start_block;
        {
            let mut ctx = crate::core::enemy::SpawnCtx {
                rng: &mut self.rng,
                statuses: &mut statuses,
                state: &mut state,
            };
            (def.spawn)(&mut ctx);
        }
        self.next_uid += 1;
        Enemy {
            def,
            name,
            hp,
            max_hp: hp,
            block,
            statuses,
            next_move: 0,
            death_done: false,
            temp_strength: 0,
            escaped: false,
            slot,
            uid: self.next_uid,
            state,
        }
    }

    /// 生成一只新敌人并放进指定的槽位(召唤/分裂).
    /// 参考实现是往槽位数组里直接赋值:那一格还躺着尸体(或已离场的那只)就顶掉它,
    /// 否则按槽位顺序插进队里.这样队里一直按槽位从小到大排,
    /// 行动顺序也就和参考一致,召唤物不会挤到队尾
    fn spawn_enemy_at(&mut self, id: &str, hp: Option<i32>, slot: usize) -> usize {
        let e = self.make_enemy(id, hp, slot);
        let at = match self.enemies.iter().position(|x| x.slot == slot && !x.up()) {
            Some(i) => {
                self.enemies[i] = e;
                i
            }
            None => {
                let at = self
                    .enemies
                    .iter()
                    .position(|x| x.slot > slot)
                    .unwrap_or(self.enemies.len());
                self.enemies.insert(at, e);
                at
            }
        };
        // 同名的按队伍顺序重排编号:新来的可能插在原来那只前面(收集者的火炬头)
        let id = self.enemies[at].def.id;
        if self.enemies.iter().filter(|e| e.def.id == id).count() > 1 {
            let mut k = 0;
            for e in self.enemies.iter_mut() {
                if e.def.id == id {
                    k += 1;
                    e.name = format!("{} #{}", e.def.name, k);
                }
            }
        }
        self.roll_first_move(at);
        at
    }

    /// 槽位空着吗(没人,或者只躺着尸体 / 已离场的那只)
    fn slot_open(&self, slot: usize) -> bool {
        match self.enemies.iter().find(|e| e.slot == slot) {
            Some(e) => !e.up(),
            None => true,
        }
    }

    /// 召唤一只到指定槽位,顺便记上召唤物标记
    fn summon_one(&mut self, id: &str, slot: usize, caller: &str) -> usize {
        let at = self.spawn_enemy_at(id, None, slot);
        if !self.enemies[at].statuses.holds(Status::Minion) {
            self.enemies[at].statuses.add(Status::Minion, 1);
        }
        let who = self.enemies[at].name.clone();
        self.push_log(LogKind::Enemy, format!("{caller} calls {who} for help"));
        at
    }

    /// 按参考实现的槽位顺序挑空槽:从 slots 里取前 n 个没被占的(自己那一格不算)
    fn open_slots(&self, me_slot: usize, slots: &[u8], n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for &slot in slots {
            if out.len() >= n {
                break;
            }
            let slot = slot as usize;
            if slot != me_slot && self.slot_open(slot) {
                out.push(slot);
            }
        }
        out
    }

    /// 敌人自己的回合结束:回合末生效的能力与倒计时
    fn enemy_end_of_turn(&mut self, idx: usize, name: &str) {
        let def = self.enemies[idx].def;
        let metallicize = self.enemies[idx].statuses.get(Status::Metallicize);
        let plated = self.enemies[idx].statuses.get(Status::PlatedArmor);
        if metallicize + plated > 0 {
            self.enemies[idx].block += metallicize + plated;
            self.push_log(
                LogKind::Enemy,
                format!("{name} gains {} Block", metallicize + plated),
            );
        }
        let up = self.enemies[idx].statuses.get(Status::StrengthUp);
        if up > 0 {
            self.enemies[idx].statuses.add(Status::Strength, up);
            self.push_log(
                LogKind::Enemy,
                format!("{name} gains {up} Strength"),
            );
        }
        let regen = self.enemies[idx].statuses.get(Status::Regenerate);
        if regen > 0 {
            let healed = self.enemies[idx].max_hp.min(self.enemies[idx].hp + regen)
                - self.enemies[idx].hp;
            self.enemies[idx].hp += healed;
            if healed > 0 {
                self.push_log(LogKind::Enemy, format!("{name} regenerates {healed} HP"));
            }
        }
        let ritual = self.enemies[idx].statuses.get(Status::Ritual);
        if ritual > 0 {
            self.enemies[idx].statuses.add(Status::Strength, ritual);
            self.push_log(
                LogKind::Enemy,
                format!("{name} channels Ritual: +{ritual} Strength"),
            );
        }
        // 每回合回满的能力:延展、慢速、飞行
        for s in [Status::Malleable, Status::Slow, Status::Flight] {
            if !self.enemies[idx].statuses.holds(s) {
                continue;
            }
            if s == Status::Flight && self.enemies[idx].statuses.get(s) == 0 {
                continue;
            }
            let base = Self::innate_amount_of(def, s);
            self.enemies[idx].statuses.set(s, base);
        }
        // 暗灵的复活倒计时:半死的那一只熬到头就半血站起来
        if self.enemies[idx].state.half_dead
            && self.enemies[idx].def.special == Special::Regrow
            && self.enemies[idx].state.regrow_ticks <= 1
        {
            let half = self.enemies[idx].max_hp / 2;
            self.enemies[idx].hp = half;
            self.enemies[idx].state.half_dead = false;
            self.push_log(LogKind::Enemy, format!("{name} regrows ({half} HP)"));
        }
        // 倒计时:爆裂与消逝
        let explosive = self.enemies[idx].statuses.get(Status::Explosive);
        if explosive > 0 {
            self.enemies[idx].statuses.set(Status::Explosive, explosive - 1);
        }
        let fading = self.enemies[idx].statuses.get(Status::Fading);
        if fading > 0 {
            if fading == 1 {
                self.enemies[idx].hp = 0;
                self.enemies[idx].escaped = true;
                self.enemies[idx].death_done = true;
                self.push_log(LogKind::Enemy, format!("{name} fades away"));
                self.check_win();
                return;
            }
            self.enemies[idx].statuses.set(Status::Fading, fading - 1);
        }
        self.enemies[idx].statuses.decay_debuffs();
    }

    /// 选出下一招
    fn pick_next_move(&mut self, idx: usize) {
        if idx >= self.enemies.len() {
            return;
        }
        let def = self.enemies[idx].def;
        let len = def.moves.len();
        // 被上一招指定的后继
        if let Some(f) = self.enemies[idx].state.forced.take() {
            self.enemies[idx].next_move = f.min(len - 1);
            return;
        }
        let pick = self.run_script(idx, def.pick);
        self.enemies[idx].next_move = pick.min(len - 1);
    }

    /// 开局的那一次掷招(这时候回合数还是 0,参考实现里 firstTurn 为真)
    fn roll_first_move(&mut self, idx: usize) {
        let def = self.enemies[idx].def;
        let len = def.moves.len();
        let m = self.run_script(idx, def.pick);
        self.enemies[idx].next_move = m.min(len - 1);
    }

    /// 跑一遍某只怪的选招函数.状态是副本,跑完写回(选招里可以记账)
    fn run_script(&mut self, idx: usize, f: crate::core::enemy::PickFn) -> usize {
        let mut state = self.enemies[idx].state.clone();
        let pick = {
            let Combat {
                enemies,
                player,
                rng,
                ..
            } = self;
            let mut ctx = PickCtx {
                rng,
                idx,
                all: enemies,
                player,
                state: &mut state,
            };
            f(&mut ctx)
        };
        self.enemies[idx].state = state;
        pick
    }

    /// 重新掷一次当前意图(扭动巨物的反应)
    fn reroll_intent(&mut self, idx: usize) {
        let cur = self.enemies[idx].next_move;
        let saved = (self.enemies[idx].state.last, self.enemies[idx].state.prev);
        self.enemies[idx].state.last = Some(cur);
        self.enemies[idx].state.prev = saved.0;
        self.pick_next_move(idx);
        let now = self.enemies[idx].next_move;
        self.enemies[idx].state.last = saved.0;
        self.enemies[idx].state.prev = saved.1;
        if now != cur {
            let name = self.enemies[idx].name.clone();
            let m = self.enemies[idx].def.moves[now].name;
            self.push_log(
                LogKind::Enemy,
                format!("{name} shifts its stance: {m}"),
            );
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
                self.damage_enemy_plain(t, jug);
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
        // 被夹击:从背后打过来的多吃一半
        if self.player.statuses.has(Status::Surrounded) && idx != self.facing {
            d = (d as f32 * 1.5).floor() as i32;
        }
        if self.enemies[idx].statuses.has(Status::Weak) {
            d = (d as f32 * 0.75).floor() as i32;
        }
        if self.player.statuses.has(Status::Vulnerable) {
            d = (d as f32 * 1.5).floor() as i32;
        }
        d.max(0)
    }

    /// 开局写死的层数(每回合重置的延展/慢速/飞行要看它)
    fn innate_amount_of(def: &'static EnemyDef, s: Status) -> i32 {
        def.innate
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    /// 下一招的基础伤害与命中次数(还没算力量/虚弱/易伤),按当前回合数动态算
    pub fn intent_raw_damage(&self, idx: usize) -> (i32, u8) {
        let e = &self.enemies[idx];
        let mv = &e.def.moves[e.next_move];
        let turn = e.state.turns + 1;
        let mut damage = 0;
        let mut times = 0u8;
        for fx in mv.effects {
            let (d, t) = match *fx {
                EnemyFx::Attack { amount, times } => (amount, times),
                EnemyFx::AttackScaling {
                    amount,
                    per_turn,
                    cap,
                    times,
                } => (amount + per_turn * turn.saturating_sub(1).min(cap) as i32, times),
                EnemyFx::AttackGrowing { amount } => (amount, ((turn + 1) / 2).max(1) as u8),
                EnemyFx::AttackStabCount { amount } => {
                    (amount, e.state.stab.max(1) as u8)
                }
                EnemyFx::AttackRolled { times } => (e.state.rolled, times),
                _ => continue,
            };
            damage += d;
            times = times.saturating_add(t);
        }
        (damage, times)
    }

    /// UI 用:敌人下一招的显示数值(伤害已计入增减益)
    pub fn predicted_damage(&self, idx: usize) -> (i32, u8) {
        if !self.enemies[idx].intent().attacks() {
            return (0, 0);
        }
        let (damage, times) = self.intent_raw_damage(idx);
        (self.enemy_attack_damage(idx, damage), times)
    }

    /// UI 用:敌人下一招的格挡量
    pub fn intent_block(&self, idx: usize) -> i32 {
        let e = &self.enemies[idx];
        let mut total = 0;
        for fx in e.def.moves[e.next_move].effects {
            if let EnemyFx::Block { amount, .. } = fx {
                total += *amount;
            }
        }
        total
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
        let Some(e) = self.enemies.get(idx) else {
            return false;
        };
        let Some(m) = e.def.moves.get(e.next_move) else {
            return false;
        };
        m.intent.attacks()
    }

    /// 打敌人:damage 是算好的最终值;返回扣格挡后实际造成的伤害.
    /// 卡牌打出来的是"攻击伤害",会走飞行/慢速/无形/无敌这一整套.
    fn damage_enemy(&mut self, idx: usize, damage: i32) -> i32 {
        self.hit_enemy(idx, damage, true)
    }

    /// 非攻击伤害(中毒、燃烧、荆棘之类):不吃飞行/慢速这些减免
    fn damage_enemy_plain(&mut self, idx: usize, damage: i32) -> i32 {
        self.hit_enemy(idx, damage, false)
    }

    fn hit_enemy(&mut self, idx: usize, damage: i32, is_attack: bool) -> i32 {
        if idx >= self.enemies.len() || !self.enemies[idx].alive() {
            return 0;
        }
        let mut dmg = damage.max(0);
        if is_attack {
            // 飞行:受到的攻击伤害减半
            if self.enemies[idx].statuses.has(Status::Flight) {
                dmg = (dmg as f32 * 0.5).floor() as i32;
            }
            // 慢速:这回合每打出一张牌就多吃 10%
            let slow = self.enemies[idx].statuses.get(Status::Slow);
            if slow > 0 {
                dmg = (dmg as f32 * (1.0 + 0.1 * slow as f32)).floor() as i32;
            }
        }
        // 无形:什么伤害都降到 1
        if self.enemies[idx].statuses.has(Status::Intangible) && dmg > 1 {
            dmg = 1;
        }
        let blocked = self.enemies[idx].block.min(dmg);
        self.enemies[idx].block -= blocked;
        let mut taken = dmg - blocked;
        // 无敌:一回合之内最多再掉这么多
        let inv = self.enemies[idx].statuses.get(Status::Invincible);
        if inv > 0 {
            let left = (inv - self.enemies[idx].state.taken_this_turn).max(0);
            taken = taken.min(left);
        }
        if taken > 0 {
            self.enemies[idx].hp -= taken;
            self.enemies[idx].state.taken_this_turn += taken;
            self.damage_dealt += taken;
            self.shake(ShakeWho::Enemy(idx), 1, ShakeKind::Hurt, taken);
            self.on_enemy_hp_lost(idx, taken, is_attack);
        }
        if is_attack {
            self.on_enemy_attacked(idx, taken);
        }
        taken
    }

    /// 敌人真的掉血了(阈值触发都挂在这儿)
    fn on_enemy_hp_lost(&mut self, idx: usize, taken: i32, is_attack: bool) {
        // 睡着的怪被打醒
        if self.enemies[idx].statuses.holds(Status::Asleep) {
            self.enemies[idx].statuses.add(Status::Asleep, -1);
            let met = self.enemies[idx].statuses.get(Status::Metallicize);
            if met > 0 {
                self.enemies[idx].statuses.add(Status::Metallicize, -met);
            }
            let name = self.enemies[idx].name.clone();
            self.push_log(LogKind::Info, format!("{name} wakes up!"));
        }
        if is_attack {
            // 卷曲:第一次被攻击就获得格挡,一次性
            let curl = self.enemies[idx].statuses.get(Status::CurlUp);
            if curl > 0 {
                self.enemies[idx].block += curl;
                self.enemies[idx].statuses.add(Status::CurlUp, -curl);
                let name = self.enemies[idx].name.clone();
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} curls up: {curl} Block"),
                );
            }
            // 延展:先按当前层数拿格挡,层数再涨
            if self.enemies[idx].statuses.holds(Status::Malleable) {
                let mal = self.enemies[idx].statuses.get(Status::Malleable);
                self.enemies[idx].block += mal;
                self.enemies[idx].statuses.set(Status::Malleable, mal + 1);
            }
            // 甲壳:每掉一次血掉一层
            let plated = self.enemies[idx].statuses.get(Status::PlatedArmor);
            if plated > 0 {
                self.enemies[idx].statuses.add(Status::PlatedArmor, -1);
                if plated == 1 && self.stun_move(idx).is_some() {
                    let stun = self.stun_move(idx).unwrap();
                    self.enemies[idx].next_move = stun;
                    let name = self.enemies[idx].name.clone();
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name}'s shell cracks: it is stunned"),
                    );
                }
            }
            // 飞行:每次被攻击削一层,掉光就落地
            let flight = self.enemies[idx].statuses.get(Status::Flight);
            if flight > 0 {
                if flight == 1 {
                    self.enemies[idx].statuses.add(Status::Flight, -1);
                    if let Some(stun) = self.stun_move(idx) {
                        self.enemies[idx].next_move = stun;
                    }
                    let name = self.enemies[idx].name.clone();
                    self.push_log(LogKind::Info, format!("{name} is knocked to the ground"));
                } else {
                    self.enemies[idx].statuses.add(Status::Flight, -1);
                }
            }
            // 移形换影:掉多少血就临时少多少力量
            if self.enemies[idx].statuses.holds(Status::Shifting) {
                self.enemies[idx].temp_strength -= taken;
            }
        }
        self.check_hp_thresholds(idx, taken);
    }

    /// 命中就触发、掉不掉血都算的(荆棘、狂怒)
    fn on_enemy_attacked(&mut self, idx: usize, _taken: i32) {
        let angry = self.enemies[idx].statuses.get(Status::Anger);
        if angry > 0 {
            self.enemies[idx].statuses.add(Status::Strength, angry);
            let name = self.enemies[idx].name.clone();
            self.push_log(
                LogKind::Enemy,
                format!("{name}'s Anger: +{angry} Strength"),
            );
        }
        let thorns = self.enemies[idx].statuses.get(Status::Thorns);
        if thorns > 0 {
            let name = self.enemies[idx].name.clone();
            let (hurt, _) = self.hit_player(thorns);
            self.push_log(
                LogKind::Enemy,
                format!("{name}'s Thorns deal {hurt} to you"),
            );
        }
        // 扭动巨物:挨打就换招
        if self.enemies[idx].def.special == Special::Reactive && self.phase == Phase::PlayerTurn {
            self.reroll_intent(idx);
        }
    }

    /// 招式表里有没有"眩晕"那一招
    fn stun_move(&self, idx: usize) -> Option<usize> {
        let def = self.enemies[idx].def;
        def.moves
            .iter()
            .position(|m| m.intent == Intent::Stun)
    }

    /// 血量阈值:分裂、形态切换
    fn check_hp_thresholds(&mut self, idx: usize, taken: i32) {
        if idx >= self.enemies.len() || self.enemies[idx].hp <= 0 {
            return;
        }
        let def = self.enemies[idx].def;
        // 史莱姆:掉到一半就把意图换成"分裂"那一招,自己回合执行
        if matches!(def.special, Special::Split { .. }) {
            if self.enemies[idx].hp <= self.enemies[idx].max_hp / 2 {
                if let Some(split) = def
                    .moves
                    .iter()
                    .position(|m| m.effects.contains(&EnemyFx::Split))
                {
                    if self.enemies[idx].next_move != split {
                        self.enemies[idx].next_move = split;
                        let name = self.enemies[idx].name.clone();
                        self.push_log(LogKind::Enemy, format!("{name} is about to split"));
                    }
                }
            }
        }
        // 守护者:形态切换的额度掉光就换防御姿态
        if let Special::ModeShift { guard, .. } = def.special {
            let left = self.enemies[idx].statuses.get(Status::ModeShift) - taken;
            if self.enemies[idx].statuses.holds(Status::ModeShift) && left <= 0 {
                self.enemies[idx].statuses.add(Status::ModeShift, -999);
                self.enemies[idx].block += 20;
                self.enemies[idx].next_move = guard;
                let name = self.enemies[idx].name.clone();
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} shifts into defensive mode"),
                );
            } else if left > 0 {
                self.enemies[idx].statuses.set(Status::ModeShift, left);
            }
        }
    }

    /// 结算本回合新死的敌人(死亡触发只在第一次结算)
    fn settle_deaths(&mut self) {
        for i in 0..self.enemies.len() {
            if self.enemies[i].dead() && !self.enemies[i].death_done {
                self.handle_death(i);
            }
        }
    }

    /// 一只怪死了:先看它的独有机制,再走通用的死亡触发
    fn handle_death(&mut self, i: usize) {
        let def = self.enemies[i].def;
        // 觉醒者:第一阶段"死"掉只是半死,躺着等复活
        if def.special == Special::Rebirth && !self.enemies[i].state.phase2 {
            self.enemies[i].state.half_dead = true;
            self.enemies[i].statuses.clear_debuffs();
            self.enemies[i].statuses.add(Status::Curiosity, -999);
            let strength = self.enemies[i].statuses.get(Status::Strength);
            if strength < 0 {
                self.enemies[i].statuses.add(Status::Strength, -strength);
            }
            // 当前意图立刻换成复活那一招
            if let Some(rebirth) = def.moves.iter().position(|m| m.name == "Rebirth") {
                self.enemies[i].next_move = rebirth;
            }
            let name = self.enemies[i].name.clone();
            self.push_log(
                LogKind::Enemy,
                format!("{name}'s body crumbles... it is not done yet"),
            );
            return;
        }
        // 暗灵:只要有别的暗灵还活着,就先半死等着复活
        if def.special == Special::Regrow && !self.enemies[i].state.regrow_used {
            let id = def.id;
            let kin = self
                .enemies
                .iter()
                .enumerate()
                .any(|(j, e)| j != i && e.def.id == id && e.hp > 0 && !e.escaped && !e.state.half_dead);
            if kin {
                let e = &mut self.enemies[i];
                e.state.half_dead = true;
                e.state.regrow_used = true;
                e.state.regrow_ticks = 2;
                e.statuses = Statuses::new();
                e.statuses.add(Status::Regrow, 1);
                e.block = 0;
                e.death_done = true;
                if let Some(regrow) = def.moves.iter().position(|m| m.name == "Regrow") {
                    e.next_move = regrow;
                }
                let name = e.name.clone();
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} clings to life and will regrow"),
                );
                return;
            }
            // 同族都倒下了,半死的那些也一起彻底死掉
            for j in 0..self.enemies.len() {
                if self.enemies[j].def.id == id {
                    self.enemies[j].state.half_dead = false;
                }
            }
        }
        self.enemies[i].death_done = true;
        self.return_stolen_card(i);
        // 抢钱的被打死了,赃款吐出来
        let stolen = self.enemies[i].state.stolen;
        if stolen > 0 {
            self.enemies[i].state.stolen = 0;
            self.player_gold += stolen;
            let name = self.enemies[i].name.clone();
            self.push_log(
                LogKind::Player,
                format!("{name} drops the {stolen} gold it stole"),
            );
        }
        // 首领死了,召唤物一起退场(不算它们死亡)
        if def.special == Special::Leader {
            for j in 0..self.enemies.len() {
                if self.enemies[j].up() && self.enemies[j].is_minion() {
                    self.enemies[j].escaped = true;
                    self.enemies[j].death_done = true;
                    let name = self.enemies[j].name.clone();
                    self.push_log(LogKind::Info, format!("{name} flees"));
                }
            }
        }
        let on_death = def.on_death;
        if on_death.is_empty() {
            return;
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
                EnemyFx::PlainDamage { amount } => {
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

    fn check_win(&mut self) {
        if self.phase == Phase::Lost {
            return;
        }
        self.settle_deaths();
        // 召唤物不算数:首领倒了它们就散
        if self.enemies.iter().any(|e| e.up() && !e.is_minion()) {
            return;
        }
        for i in 0..self.enemies.len() {
            if self.enemies[i].up() && self.enemies[i].is_minion() {
                self.enemies[i].escaped = true;
                self.enemies[i].death_done = true;
            }
        }
        self.phase = Phase::Won;
        self.push_log(LogKind::Info, "victory".to_string());
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
        // 被夹击时,玩家指哪边就朝哪边
        if card.target() == Target::Enemy {
            if let Some(t) = chosen {
                self.facing = t;
            }
        }
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
        self.note_card_played(card.kind());
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
        // 神器:先拿一层顶掉这次减益
        if n > 0 && status.is_debuff() && self.enemies[idx].statuses.has(Status::Artifact) {
            self.enemies[idx].statuses.add(Status::Artifact, -1);
            let name = self.enemies[idx].name.clone();
            self.push_log(
                LogKind::Enemy,
                format!("{name}'s Artifact blocks {}", status.name()),
            );
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
        self.damage_enemy_plain(idx, sad);
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
                    let taken = self.damage_enemy_plain(t, amount);
                    self.push_log(
                        LogKind::Player,
                        format!("{} deals {taken} damage", def.name),
                    );
                }
            }
            PotionFx::DamageAll { amount } => {
                for t in self.alive_enemies() {
                    self.damage_enemy_plain(t, amount);
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
            gold: 0,
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
            .find(|e| e.innate.iter().any(|(s, _)| *s == Status::Asleep))
            .expect("总得有一只睡着的怪");
        let enc = crate::core::enemies::encounter_with_enemy(sleeper.id)
            .expect("睡着的怪要能打得到");
        let mut c = combat_with(enc.id, &["strike"; 10]);
        let i = c
            .enemies
            .iter()
            .position(|e| e.def.id == sleeper.id)
            .unwrap();
        assert!(c.enemies[i].statuses.has(Status::Asleep));
        // 开局自带的格挡要先打掉,不然这一下不算掉血
        c.enemies[i].block = 0;
        c.play_card(0, Some(i)).unwrap();
        assert!(
            !c.enemies[i].statuses.has(Status::Asleep),
            "睡眠中的敌人被打后应醒来"
        );
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

#[cfg(test)]
mod monster_tests {
    use super::*;
    use crate::core::cards::card;

    /// 打一场指定遭遇:80 血,牌组给几张打击/防御够用就行
    fn lock(id: &'static str) -> Combat {
        let enc = crate::core::enemies::encounter_def(id)
            .unwrap_or_else(|| panic!("no such encounter {id}"));
        let deck = vec![
            card("strike"),
            card("strike"),
            card("strike"),
            card("defend"),
            card("defend"),
            card("strike"),
            card("strike"),
            card("defend"),
            card("strike"),
            card("strike"),
        ];
        let setup = CombatSetup {
            hp: 80,
            max_hp: 80,
            deck,
            relics: Vec::new(),
            gold: 0,
        };
        Combat::new(enc, setup, 7)
    }

    /// 敌人这一招的意图(不看睡眠/半死这些覆盖)
    fn intent(c: &Combat, i: usize) -> Intent {
        c.enemies[i].def.moves[c.enemies[i].next_move].intent
    }

    #[test]
    fn cultist_charges_once_then_strikes() {
        let mut c = lock("cultist_solo");
        assert_eq!(intent(&c, 0), Intent::Buff, "开场先充能");
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Ritual), 3);
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 3, "仪式当回合结算成力量");
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 6, times: 1 });
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(c.player.hp, hp - 9, "黑暗打击 6 + 3 力量");
    }

    #[test]
    fn jaw_worm_opens_with_chomp() {
        let c = lock("jaw_worm_solo");
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 11, times: 1 });
    }

    #[test]
    fn louse_rolls_one_bite_damage_and_curls_up_once() {
        let mut c = lock("two_louses");
        let bite = c.enemies[0].state.rolled;
        assert!((5..=7).contains(&bite), "咬的伤害开局掷在 5..7,实得 {bite}");
        assert!(c.enemies[0].statuses.has(Status::CurlUp), "虱子开局带卷曲");
        let curl = c.enemies[0].statuses.get(Status::CurlUp);
        assert!((3..=7).contains(&curl));
        // 挨第一下:拿到等量格挡,卷曲消耗掉
        c.damage_enemy(0, 1);
        assert_eq!(c.enemies[0].block, curl);
        assert!(!c.enemies[0].statuses.has(Status::CurlUp), "卷曲只吃一次");
        // 挨第二下不再给格挡
        let block = c.enemies[0].block;
        c.damage_enemy(0, 1);
        assert_eq!(c.enemies[0].block, block - 1);
    }

    #[test]
    fn gremlin_wizard_charges_twice_then_blasts() {
        let mut c = lock("gremlin_gang_alt");
        let w = c
            .enemies
            .iter()
            .position(|e| e.def.id == "gremlin_wizard")
            .expect("阵容里要有小鬼巫师");
        assert_eq!(intent(&c, w), Intent::Unknown, "第一回合充能");
        c.end_turn();
        assert_eq!(intent(&c, w), Intent::Unknown, "第二回合还在充能");
        c.end_turn();
        assert_eq!(intent(&c, w), Intent::Attack { damage: 25, times: 1 });
    }

    #[test]
    fn sentry_alternates_and_artifact_blocks_a_debuff() {
        let mut c = lock("three_sentries");
        assert_eq!(c.enemies[0].statuses.get(Status::Artifact), 1);
        assert_eq!(intent(&c, 0), Intent::Debuff, "偶数位先放螺栓");
        assert_eq!(intent(&c, 1), Intent::Attack { damage: 9, times: 1 }, "奇数位先射线");
        // 神器挡掉一次减益
        c.add_enemy_status(0, Status::Weak, 2);
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 0, "被神器顶掉");
        assert_eq!(c.enemies[0].statuses.get(Status::Artifact), 0);
        c.add_enemy_status(0, Status::Weak, 2);
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 2);
        // 交替
        let before = c.enemies[0].next_move;
        c.end_turn();
        assert_ne!(c.enemies[0].next_move, before);
    }

    #[test]
    fn lagavulin_sleeps_then_attacks_and_wakes_on_damage() {
        let mut c = lock("lagavulin_solo");
        assert!(c.enemies[0].statuses.has(Status::Asleep));
        assert_eq!(c.enemies[0].block, 8, "开局自带 8 格挡");
        assert_eq!(intent(&c, 0), Intent::Sleep);
        c.end_turn();
        c.end_turn();
        assert_eq!(intent(&c, 0), Intent::Sleep, "前三回合都在睡");
        c.end_turn();
        assert!(!c.enemies[0].statuses.has(Status::Asleep), "睡满三回合自己醒");
        assert_eq!(c.enemies[0].statuses.get(Status::Metallicize), 0, "醒来丢掉金属化");
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 18, times: 1 });

        // 挨打会提前醒:先破掉它开局的 8 点格挡
        let mut c = lock("lagavulin_solo");
        c.damage_enemy(0, 8);
        assert!(c.enemies[0].statuses.has(Status::Asleep), "全挡住的时候不醒");
        c.damage_enemy(0, 5);
        assert!(!c.enemies[0].statuses.has(Status::Asleep));
        assert_eq!(c.enemies[0].statuses.get(Status::Metallicize), 0, "醒来丢掉金属化");
    }

    #[test]
    fn gremlin_nob_bellow_then_enrage_punishes_skills() {
        let mut c = lock("gremlin_nob_solo");
        assert_eq!(intent(&c, 0), Intent::Buff);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Enrage), 2);
        let strength = c.enemies[0].statuses.get(Status::Strength);
        // 玩家打一张技能牌 → 狂怒 +2 力量
        c.energy = 3;
        let defend = c.hand.iter().position(|x| x.def.id == "defend").unwrap();
        c.play_card(defend, None).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), strength + 2);
    }

    #[test]
    fn slime_boss_goops_then_splits_at_half_health() {
        let mut c = lock("slime_boss");
        assert_eq!(intent(&c, 0), Intent::StrongDebuff, "开场喷粘液");
        c.end_turn();
        assert_eq!(c.discard.iter().filter(|x| x.def.id == "slimed").count(), 3);
        // 掉到一半血:意图立刻换成分裂
        c.enemies[0].hp = 70;
        c.damage_enemy(0, 1);
        let split = c.enemies[0].next_move;
        assert_eq!(
            c.enemies[0].def.moves[split].name, "Split",
            "半血必须改成分裂"
        );
        // 轮到它时分成两只大史莱姆,生命等于分裂时的血量
        let hp = c.enemies[0].hp;
        c.end_turn();
        assert!(c.enemies.iter().any(|e| e.def.id == "spike_slime_large"));
        assert!(c.enemies.iter().any(|e| e.def.id == "acid_slime_large"));
        for e in c.enemies.iter().filter(|e| e.def.id != "slime_boss") {
            assert_eq!(e.hp, hp, "小史莱姆继承分裂时的血量");
        }
    }

    #[test]
    fn large_slime_splits_into_two_mediums() {
        let mut c = lock("large_slime");
        assert_eq!(c.enemies[0].def.id, "acid_slime_large");
        c.enemies[0].hp = 30;
        c.damage_enemy(0, 1);
        c.end_turn();
        let mediums = c
            .enemies
            .iter()
            .filter(|e| e.def.id == "acid_slime_medium")
            .count();
        assert_eq!(mediums, 2, "大史莱姆分裂成两只中史莱姆");
    }

    #[test]
    fn guardian_shifts_to_defensive_mode_after_enough_damage() {
        let mut c = lock("the_guardian");
        assert_eq!(c.enemies[0].statuses.get(Status::ModeShift), 30);
        assert_eq!(intent(&c, 0), Intent::Defend, "开场蓄力");
        c.damage_enemy(0, 20);
        assert_eq!(c.enemies[0].statuses.get(Status::ModeShift), 10);
        assert_eq!(intent(&c, 0), Intent::Defend, "还没掉够,不改意图");
        c.damage_enemy(0, 10);
        assert!(!c.enemies[0].statuses.has(Status::ModeShift));
        assert_eq!(c.enemies[0].block, 20, "切换时白拿 20 格挡");
        assert_eq!(c.enemies[0].def.moves[c.enemies[0].next_move].name, "Defensive Mode");
    }

    #[test]
    fn guardian_defensive_mode_has_sharp_hide_and_twin_slam_removes_it() {
        let mut c = lock("the_guardian");
        c.damage_enemy(0, 30);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::SharpHide), 3);
        // 玩家打攻击牌 → 挨尖刺
        c.energy = 3;
        let hp = c.player.hp;
        let strike = c.hand.iter().position(|x| x.def.id == "strike").unwrap();
        c.play_card(strike, Some(0)).unwrap();
        assert!(c.player.hp < hp, "尖刺外壳反伤");
        // 双拳合击会把尖刺收起来
        let twin = c.enemies[0]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Twin Slam")
            .expect("守护者要有双拳合击");
        c.enemies[0].next_move = twin;
        c.end_turn();
        assert!(!c.enemies[0].statuses.has(Status::SharpHide));
    }

    #[test]
    fn hexaghost_divider_scales_with_player_health() {
        let mut c = lock("hexaghost");
        c.player.hp = 72;
        assert_eq!(intent(&c, 0), Intent::Unknown, "开场点火");
        c.end_turn();
        assert_eq!(c.enemies[0].state.rolled, 7, "72/12 + 1");
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 0, times: 6 });
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(c.player.hp, hp - 7 * 6, "分裂打 6 下,每下 7");
    }

    #[test]
    fn thief_steals_gold_and_flees() {
        let mut enc = lock("looter_solo");
        enc.player_gold = 100;
        let mut c = enc;
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 10, times: 1 });
        c.end_turn();
        assert_eq!(c.player_gold, 85, "抢走 15");
        assert_eq!(c.enemies[0].state.stolen, 15);
        // 抢完两回合就霰雾弹跑路
        for _ in 0..6 {
            if c.enemies[0].escaped {
                break;
            }
            c.end_turn();
        }
        assert!(c.enemies[0].escaped, "最后一定会逃");
        assert_eq!(c.phase, Phase::Won, "只剩它一只,逃跑就算赢");
    }

    #[test]
    fn killing_a_thief_gives_the_gold_back() {
        let mut c = lock("looter_solo");
        c.player_gold = 100;
        c.end_turn();
        assert_eq!(c.player_gold, 85);
        c.damage_enemy(0, 999);
        c.settle_deaths();
        assert_eq!(c.player_gold, 100, "打死就把赃款吐出来");
    }

    #[test]
    fn fungi_beast_bursts_on_death() {
        let mut c = lock("two_fungi_beasts");
        assert_eq!(c.enemies[0].statuses.get(Status::SporeCloud), 2);
        c.damage_enemy(0, 999);
        c.settle_deaths();
        assert_eq!(c.player.statuses.get(Status::Vulnerable), 2);
    }

    #[test]
    fn maw_nom_hits_grow_and_force_drool() {
        let mut c = lock("the_maw_solo");
        let nom = c.enemies[0]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Nom")
            .expect("巨口要有咬这一招");
        let drool = c.enemies[0]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Drool")
            .expect("巨口要有流口水");
        // 直接把它推到 NOM:第 2 回合咬一下
        c.enemies[0].next_move = nom;
        c.enemies[0].state.turns = 1;
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 5, "第 2 回合咬一下");
        assert_eq!(c.enemies[0].next_move, drool, "咬完必定接流口水");
    }

    #[test]
    fn acid_slime_small_alternates_lick_and_tackle() {
        let mut c = lock("lots_of_slimes");
        // 只留中间那只小酸液,免得被别的走位干扰
        c.enemies.drain(0..3);
        assert_eq!(c.enemies[0].def.id, "acid_slime_small");
        let first = c.enemies[0].next_move;
        c.end_turn();
        assert_ne!(c.enemies[0].next_move, first, "两个动作严格交替");
        c.end_turn();
        assert_eq!(c.enemies[0].next_move, first);
    }

    #[test]
    fn enemy_intent_shows_damage_after_modifiers() {
        let mut c = lock("jaw_worm_solo");
        // 敌人身上有力量,意图显示的伤害要跟着涨
        c.enemies[0].statuses.add(Status::Strength, 3);
        c.enemies[0].next_move = 0;
        assert_eq!(c.predicted_damage(0), (14, 1));
        // 玩家易伤 → 再多一半
        c.player.statuses.add(Status::Vulnerable, 2);
        assert_eq!(c.predicted_damage(0), (21, 1));
    }
}

#[cfg(test)]
mod power_tests {
    use super::*;
    use crate::core::cards::card;

    /// 打一场指定遭遇:80 血,牌组给几张打击/防御
    fn lock(id: &'static str) -> Combat {
        let enc = crate::core::enemies::encounter_def(id)
            .unwrap_or_else(|| panic!("no such encounter {id}"));
        let deck = vec![
            card("strike"),
            card("strike"),
            card("defend"),
            card("strike"),
            card("defend"),
            card("strike"),
            card("strike"),
            card("defend"),
            card("strike"),
            card("strike"),
        ];
        let setup = CombatSetup {
            hp: 80,
            max_hp: 80,
            deck,
            relics: Vec::new(),
            gold: 0,
        };
        Combat::new(enc, setup, 11)
    }

    fn idx_of(c: &Combat, id: &str) -> usize {
        c.enemies
            .iter()
            .position(|e| e.def.id == id)
            .unwrap_or_else(|| panic!("no {id} in this fight"))
    }

    #[test]
    fn spheric_guardian_keeps_block_and_blocks_debuffs() {
        let mut c = lock("spheric_guardian_solo");
        assert_eq!(c.enemies[0].block, 40);
        assert_eq!(c.enemies[0].statuses.get(Status::Artifact), 3);
        // 壁垒:回合开始不清格挡
        c.end_turn();
        assert!(c.enemies[0].block >= 40, "壁垒让它一直攒着格挡");
        // 神器一层一层顶
        for left in (0..3).rev() {
            c.add_enemy_status(0, Status::Weak, 5);
            assert_eq!(c.enemies[0].statuses.get(Status::Artifact), left);
        }
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 0, "三次都被顶掉");
        c.add_enemy_status(0, Status::Weak, 5);
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 5, "神器用完就挡不住了");
    }

    #[test]
    fn nemesis_intangible_caps_damage_and_comes_back() {
        let mut c = lock("nemesis_solo");
        let hp = c.enemies[0].hp;
        // 开场还没有无形:这一下打满
        c.damage_enemy(0, 30);
        assert_eq!(c.enemies[0].hp, hp - 30);
        // 它行动一次就会补上无形
        c.end_turn();
        assert!(c.enemies[0].statuses.has(Status::Intangible));
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 30);
        assert_eq!(c.enemies[0].hp, hp - 1, "无形把伤害压到 1");
    }

    #[test]
    fn byrd_flight_halves_damage_and_grounds_after_three_hits() {
        let mut c = lock("three_byrds");
        assert_eq!(c.enemies[0].statuses.get(Status::Flight), 3);
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 11);
        assert_eq!(c.enemies[0].hp, hp - 5, "飞行让 11 变成 5");
        c.damage_enemy(0, 11);
        assert_eq!(c.enemies[0].statuses.get(Status::Flight), 1);
        c.damage_enemy(0, 11);
        assert!(!c.enemies[0].statuses.has(Status::Flight), "第三次命中就落地");
        // 落地之后被打不再减半
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 11);
        assert_eq!(c.enemies[0].hp, hp - 11);
    }

    #[test]
    fn snake_plant_malleable_grows_then_resets() {
        let mut c = lock("snake_plant_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::Malleable), 3);
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 10);
        assert_eq!(c.enemies[0].hp, hp - 10, "这一下还没格挡可用");
        assert_eq!(c.enemies[0].block, 3, "挨完打才长出 3 点格挡");
        assert_eq!(c.enemies[0].statuses.get(Status::Malleable), 4);
        // 下一次挨打就先吃这 3 点格挡
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 10);
        assert_eq!(c.enemies[0].hp, hp - 7);
        // 自己回合结束重置回 3
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Malleable), 3);
    }

    #[test]
    fn shelled_parasite_plated_armor_and_stun() {
        let mut c = lock("shelled_parasite_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::PlatedArmor), 14);
        assert_eq!(c.enemies[0].block, 14);
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 20);
        assert_eq!(c.enemies[0].statuses.get(Status::PlatedArmor), 13);
        assert_eq!(c.enemies[0].hp, hp - 6);
        // 打光甲壳那一下会把它打懵
        c.enemies[0].statuses.set(Status::PlatedArmor, 1);
        c.damage_enemy(0, 4);
        assert!(!c.enemies[0].statuses.has(Status::PlatedArmor));
        assert_eq!(
            c.enemies[0].intent(),
            Intent::Stun,
            "破甲的那一下把它打成眩晕"
        );
    }

    #[test]
    fn spiker_thorns_hurt_the_player() {
        let mut c = lock("three_shapes");
        let spiker = idx_of(&c, "spiker");
        c.enemies.retain(|e| e.def.id == "spiker");
        let _ = spiker;
        assert_eq!(c.enemies[0].statuses.get(Status::Thorns), 3);
        let hp = c.player.hp;
        c.damage_enemy(0, 5);
        assert_eq!(c.player.hp, hp - 3, "荆棘反伤 3");
        // 非攻击伤害不吃荆棘
        let hp = c.player.hp;
        c.damage_enemy_plain(0, 5);
        assert_eq!(c.player.hp, hp);
    }

    #[test]
    fn book_of_stabbing_hits_grow_and_leave_wounds() {
        let mut c = lock("book_of_stabbing_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::PainfulStabs), 1);
        let multi = c.enemies[0]
            .def
            .moves
            .iter()
            .position(|m| m.effects.iter().any(|fx| matches!(fx, EnemyFx::AttackStabCount { .. })))
            .expect("刺击之书要多段刺击");
        c.enemies[0].next_move = multi;
        c.enemies[0].state.stab = 1;
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 6, "开场段数是 1,先刺一下");
        assert!(c.discard.iter().any(|x| x.def.id == "wound"), "命中塞伤口");
        // 之后每次多刺一下
        let mut last = 0;
        for _ in 0..3 {
            if c.phase != Phase::PlayerTurn {
                break;
            }
            let before = c.player.hp;
            let m = c.enemies[0].next_move;
            if m != multi {
                // 这回合不是多段刺击,跳过
                c.end_turn();
                continue;
            }
            let stab = c.enemies[0].state.stab;
            c.end_turn();
            let dealt = before - c.player.hp;
            assert_eq!(dealt, 6 * stab as i32, "每段 6 点,共 {stab} 段");
            assert!(stab >= last, "段数只会涨");
            last = stab;
        }
        assert!(last >= 2, "刺击段数确实在积累");
    }

    #[test]
    fn orb_walker_gains_strength_every_turn() {
        let mut c = lock("orb_walker_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::StrengthUp), 3);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 3);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 6);
    }

    #[test]
    fn giant_head_slow_scales_damage_and_resets() {
        let mut c = lock("giant_head_solo");
        assert!(c.enemies[0].statuses.holds(Status::Slow));
        assert_eq!(c.enemies[0].statuses.get(Status::Slow), 0, "开场是 Slow 0");
        c.energy = 3;
        let strike = c.hand.iter().position(|x| x.def.id == "strike").unwrap();
        c.play_card(strike, Some(0)).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Slow), 1, "每张牌加一层");
        // 慢速放大的是"之后"的攻击伤害
        let hp = c.enemies[0].hp;
        c.damage_enemy(0, 10);
        assert_eq!(c.enemies[0].hp, hp - 11, "10 * 1.1");
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Slow), 0, "自己回合结束重置");
    }

    #[test]
    fn transient_fades_after_its_countdown() {
        let mut c = lock("transient_solo");
        c.player.hp = 999;
        assert_eq!(c.enemies[0].hp, 999);
        assert_eq!(c.enemies[0].statuses.get(Status::Fading), 5);
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 30, "第一回合打 30");
        assert_eq!(c.enemies[0].statuses.get(Status::Fading), 4);
        // 掉血会让它掉等量力量(移形换影)
        c.damage_enemy(0, 40);
        assert!(c.enemies[0].temp_strength < 0, "挨打就掉力量");
        for _ in 0..4 {
            if c.phase != Phase::PlayerTurn {
                break;
            }
            c.end_turn();
        }
        assert!(c.enemies[0].escaped, "倒计时走完就自己消失");
        assert_eq!(c.phase, Phase::Won);
    }

    #[test]
    fn exploder_slams_twice_then_blows_up() {
        let mut c = lock("three_shapes");
        let i = idx_of(&c, "exploder");
        c.enemies.retain(|e| e.def.id == "exploder");
        let i = i.min(0);
        let _ = i;
        assert_eq!(c.enemies[0].hp, 30);
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 9);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Explosive), 1);
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 30, "自爆打 30 点非攻击伤害");
        assert!(c.enemies[0].dead(), "自爆之后自己也死了");
        c.check_win();
        assert_eq!(c.phase, Phase::Won);
    }

    #[test]
    fn darkling_regrows_once_while_kin_lives() {
        let mut c = lock("three_darklings");
        c.player.hp = 999;
        assert_eq!(c.enemies[0].statuses.get(Status::Regrow), 1);
        c.enemies[0].hp = 10;
        c.damage_enemy(0, 20);
        c.settle_deaths();
        assert!(c.enemies[0].state.half_dead, "有同伴在就先半死");
        assert!(c.enemies[0].hp <= 0);
        // 熬过复活倒计时之后以半血站起来
        for _ in 0..4 {
            if !c.enemies[0].state.half_dead || c.phase != Phase::PlayerTurn {
                break;
            }
            c.end_turn();
        }
        assert!(!c.enemies[0].state.half_dead, "倒计时结束就复活");
        assert!(c.enemies[0].hp > 0);
    }

    #[test]
    fn mad_gremlin_gains_strength_when_hit() {
        let mut c = lock("gremlin_gang_alt");
        let i = idx_of(&c, "mad_gremlin");
        assert_eq!(c.enemies[i].statuses.get(Status::Anger), 1);
        // 全挡住也算
        c.enemies[i].block = 99;
        c.damage_enemy(i, 5);
        assert_eq!(c.enemies[i].statuses.get(Status::Strength), 1);
    }

    #[test]
    fn champion_enters_phase_two_below_half_health() {
        let mut c = lock("the_champ");
        assert_eq!(c.enemies[0].hp, 420);
        let first = c.enemies[0].def.moves[c.enemies[0].next_move].name;
        assert!(
            ["Defensive Stance", "Gloat", "Face Slap", "Heavy Slash"].contains(&first),
            "开场只能用共用表里的招,实得 {first}"
        );
        // 打到一半以下,下一次掷招就会进入二阶段(那一掷是暴怒)
        c.enemies[0].statuses.add(Status::Vulnerable, 3);
        c.damage_enemy(0, 215);
        c.end_turn();
        assert_eq!(
            c.enemies[0].def.moves[c.enemies[0].next_move].name,
            "Anger",
            "掉到一半以下的那一掷必是暴怒"
        );
        c.end_turn();
        assert!(c.enemies[0].statuses.get(Status::Strength) >= 6, "暴怒给 6 力量");
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 0, "先清掉自己的减益");
    }

    #[test]
    fn time_eater_stops_time_after_twelve_cards() {
        let mut c = lock("time_eater");
        assert!(c.enemies[0].statuses.holds(Status::TimeWarp));
        // 手里塞一堆 0 费牌,连打 12 张
        c.hand.clear();
        for _ in 0..12 {
            let mut inst = cards::card("strike");
            inst.cost_delta = -1;
            c.hand.push(inst);
        }
        c.energy = 12;
        for i in 0..12 {
            if c.force_end_turn || c.phase != Phase::PlayerTurn {
                break;
            }
            let _ = c.play_card(0, Some(0));
            let _ = i;
        }
        assert!(c.force_end_turn, "第 12 张牌打完就该结束回合");
        assert_eq!(c.enemies[0].statuses.get(Status::TimeWarp), 0, "计数清零");
        assert!(c.enemies[0].statuses.get(Status::Strength) >= 2, "时间扭曲还给力量");
    }

    #[test]
    fn corrupt_heart_beat_of_death_and_invincible() {
        let mut c = lock("the_heart");
        assert_eq!(c.enemies[0].statuses.get(Status::BeatOfDeath), 1);
        assert_eq!(c.enemies[0].statuses.get(Status::Invincible), 300);
        c.energy = 3;
        let strike = c.hand.iter().position(|x| x.def.id == "strike").unwrap();
        let hp = c.player.hp;
        c.play_card(strike, Some(0)).unwrap();
        assert_eq!(c.player.hp, hp - 1, "每打一张牌挨一下死亡律动");
        // 无敌:一回合最多掉 300(刚才那张打击已经用掉 6 点额度)
        let spent = c.enemies[0].state.taken_this_turn;
        let ehp = c.enemies[0].hp;
        c.damage_enemy(0, 400);
        assert_eq!(c.enemies[0].hp, ehp - (300 - spent));
        assert_eq!(c.enemies[0].state.taken_this_turn, 300);
        let ehp = c.enemies[0].hp;
        c.damage_enemy(0, 400);
        assert_eq!(c.enemies[0].hp, ehp, "这一回合已经不能再掉血了");
        // 下一回合额度重新回满
        c.end_turn();
        let ehp = c.enemies[0].hp;
        c.damage_enemy(0, 50);
        assert_eq!(c.enemies[0].hp, ehp - 50);
    }

    #[test]
    fn awakened_one_revives_into_phase_two() {
        let mut c = lock("awakened_one");
        let i = idx_of(&c, "awakened_one");
        assert_eq!(c.enemies[i].statuses.get(Status::Curiosity), 1);
        assert_eq!(c.enemies[i].statuses.get(Status::Regenerate), 10);
        c.damage_enemy(i, 400);
        c.settle_deaths();
        assert!(c.enemies[i].state.half_dead, "一阶段被打死只是半死");
        assert_eq!(c.phase, Phase::PlayerTurn, "还没赢");
        // 轮到它时会复活
        c.end_turn();
        assert!(!c.enemies[i].state.half_dead);
        assert!(c.enemies[i].state.phase2);
        assert_eq!(c.enemies[i].hp, c.enemies[i].max_hp, "复活回满血");
    }

    #[test]
    fn leader_death_takes_its_minions_with_it() {
        let mut c = lock("bronze_automaton");
        c.end_turn();
        let orbs = c
            .enemies
            .iter()
            .filter(|e| e.def.id == "bronze_orb")
            .count();
        assert_eq!(orbs, 2, "铜制机械人开场召两个铜球");
        assert!(c.enemies.iter().any(|e| e.is_minion()));
        // 铜球排在它前面,所以按 id 找首领,不能直接打 0 号
        let boss = c
            .enemies
            .iter()
            .position(|e| e.def.id == "bronze_automaton")
            .unwrap();
        c.damage_enemy(boss, 999);
        c.check_win();
        assert!(c.enemies.iter().all(|e| !e.is_minion() || !e.up()));
        assert_eq!(c.phase, Phase::Won, "首领倒下,召唤物一起退场");
    }

    #[test]
    fn collector_spawns_torch_heads_and_mega_debuffs_on_turn_four() {
        let mut c = lock("the_collector");
        c.end_turn();
        assert_eq!(
            c.enemies
                .iter()
                .filter(|e| e.def.id == "torch_head")
                .count(),
            2,
            "开场召两只火炬头"
        );
        // 打到第 4 回合那一次掷招必定是超大减益(她在火炬头后面,按 id 找)
        c.end_turn();
        c.end_turn();
        let her = c
            .enemies
            .iter()
            .position(|e| e.def.id == "the_collector")
            .unwrap();
        assert_eq!(c.enemies[her].def.moves[c.enemies[her].next_move].name, "Mega Debuff");
    }
}


/// 召唤物的站位:参考实现里每只怪占一个固定槽位,召唤物进的是指定的空槽,
/// 不是队尾;行动顺序也就按槽位从左到右排
#[cfg(test)]
mod summon_tests {
    use super::*;
    use crate::core::cards::card;

    /// 召集用的 8 只小鬼池(和 act2 里那张表一致)
    const POOL: &[&str] = &[
        "mad_gremlin",
        "mad_gremlin",
        "sneaky_gremlin",
        "sneaky_gremlin",
        "fat_gremlin",
        "fat_gremlin",
        "shield_gremlin",
        "gremlin_wizard",
    ];

    fn fight(id: &'static str, seed: u64) -> Combat {
        let enc = crate::core::enemies::encounter_def(id)
            .unwrap_or_else(|| panic!("no encounter {id}"));
        let deck: Vec<CardInstance> = [
            "strike", "strike", "strike", "defend", "defend", "strike", "strike", "defend",
            "strike", "strike",
        ]
        .iter()
        .map(|c| card(c))
        .collect();
        Combat::new(
            enc,
            CombatSetup {
                hp: 80,
                max_hp: 80,
                deck,
                relics: Vec::new(),
                gold: 0,
            },
            seed,
        )
    }

    fn slots(c: &Combat) -> Vec<usize> {
        c.enemies.iter().map(|e| e.slot).collect()
    }

    fn ids(c: &Combat) -> Vec<&str> {
        c.enemies.iter().map(|e| e.def.id).collect()
    }

    fn idx_of(c: &Combat, id: &str) -> usize {
        c.enemies
            .iter()
            .position(|e| e.def.id == id)
            .unwrap_or_else(|| panic!("no {id} on the field"))
    }

    /// 一段日志里"谁先动的":按提到的敌人名字(取最长匹配的名字)去重保序
    fn acted(c: &Combat, from: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in &c.log[from..] {
            let mut hit: Option<&Enemy> = None;
            for e in &c.enemies {
                if line.text.starts_with(&e.name)
                    && hit.map_or(true, |h| e.name.len() > h.name.len())
                {
                    hit = Some(e);
                }
            }
            if let Some(e) = hit {
                if !out.contains(&e.name) {
                    out.push(e.name.clone());
                }
            }
        }
        out
    }

    /// 首领的某一招(按下标),直接让它出手一次
    fn use_move(c: &mut Combat, id: &str, move_idx: usize) {
        let i = idx_of(c, id);
        c.enemies[i].next_move = move_idx;
        c.enemy_act(i);
    }

    #[test]
    fn automaton_orbs_take_slots_0_and_2_around_it() {
        let mut c = fight("bronze_automaton", 7);
        assert_eq!(slots(&c), vec![1], "自动机站在槽 1");
        c.end_turn();
        assert_eq!(ids(&c), vec!["bronze_orb", "bronze_automaton", "bronze_orb"]);
        assert_eq!(slots(&c), vec![0, 1, 2], "两颗铜球占 0 和 2");
        assert!(
            idx_of(&c, "bronze_orb") < idx_of(&c, "bronze_automaton"),
            "槽 0 的铜球排在首领前面"
        );
        // 召集的那一轮里只有首领动过,铜球下一轮才动
        assert_eq!(acted(&c, 0), vec!["Bronze Automaton"], "当回合铜球不动");
        // 下一轮按槽位从左到右:槽 0 铜球 → 首领 → 槽 2 铜球
        let from = c.log.len();
        c.end_turn();
        assert_eq!(
            acted(&c, from),
            vec!["Bronze Orb #1", "Bronze Automaton", "Bronze Orb #2"],
            "行动顺序按槽位排"
        );
    }

    #[test]
    fn collector_torch_heads_stand_in_front_of_her() {
        let mut c = fight("the_collector", 7);
        assert_eq!(slots(&c), vec![2], "收集者在槽 2");
        c.end_turn();
        assert_eq!(ids(&c), vec!["torch_head", "torch_head", "the_collector"]);
        assert_eq!(slots(&c), vec![0, 1, 2], "火炬头占 0 和 1");
        assert_eq!(c.enemies[0].name, "Torch Head #1", "队里的编号按位置排");
        assert!(
            idx_of(&c, "torch_head") < idx_of(&c, "the_collector"),
            "火炬头都排在她前面"
        );
        // 同一轮里火炬头先出手(只有一只活着的火炬头在她前面也一样)
        let from = c.log.len();
        c.end_turn();
        let order = acted(&c, from);
        let head = order.iter().position(|n| n.starts_with("Torch Head")).unwrap();
        let her = order.iter().position(|n| n == "The Collector").unwrap();
        assert!(head < her, "火炬头先于收集者出手:{order:?}");
    }

    #[test]
    fn collector_spawn_fills_only_the_open_slots() {
        let mut c = fight("the_collector", 7);
        c.end_turn();
        // 打死槽 0 的火炬头,再让她召一次:空的只有槽 0,所以只补一只
        c.enemies[0].hp = 0;
        use_move(&mut c, "the_collector", 0);
        assert_eq!(slots(&c), vec![0, 1, 2]);
        let heads = c.enemies.iter().filter(|e| e.def.id == "torch_head").count();
        assert_eq!(heads, 2, "补到两只就停(场上一共 3 只)");
        assert!(c.enemies[0].up(), "槽 0 重新有人");
    }

    #[test]
    fn gremlin_leader_rally_fills_the_minion_slots_from_the_pool() {
        let mut c = fight("gremlin_leader_gang", 7);
        assert_eq!(slots(&c), vec![1, 2, 3], "小鬼在 1、2,首领在 3");
        // 干掉两只小鬼,空出 1、2 两格
        for e in c.enemies.iter_mut() {
            if e.def.id != "gremlin_leader" {
                e.hp = 0;
            }
        }
        c.check_win();
        let from = c.log.len();
        use_move(&mut c, "gremlin_leader", 0); // Rally
        assert_eq!(slots(&c), vec![1, 2, 3], "顶掉空槽,不是排到队尾");
        let summoned: Vec<&Enemy> = c
            .enemies
            .iter()
            .filter(|e| e.def.id != "gremlin_leader")
            .collect();
        assert_eq!(summoned.len(), 2, "一次召集两只");
        for g in &summoned {
            assert!(POOL.contains(&g.def.id), "{} 不在 8 只小鬼池里", g.def.id);
            assert!(g.is_minion(), "{} 应当是召唤物", g.def.id);
            assert!(g.hp > 0, "{} 是活的", g.def.id);
        }
        let called = c.log[from..]
            .iter()
            .filter(|l| l.text.contains("calls"))
            .count();
        assert_eq!(called, 2, "两条召唤日志");
    }

    #[test]
    fn rally_into_a_corpse_slot_keeps_the_team_in_slot_order() {
        let mut c = fight("gremlin_leader_gang", 7);
        // 槽 1 的小鬼先死、槽 2 的还活着:召集补的是槽 1 和槽 0.
        // 尸体那一格被顶掉,新来的按槽位排进队里
        let mad = idx_of(&c, "mad_gremlin");
        c.enemies[mad].hp = 0;
        use_move(&mut c, "gremlin_leader", 0);
        assert_eq!(slots(&c), vec![0, 1, 2, 3], "补完后按槽位排");
        assert_eq!(c.enemies[0].slot, 0, "槽 0 的新鬼排在队首");
        assert_eq!(c.enemies[3].def.id, "gremlin_leader");
    }

    #[test]
    fn gremlin_leader_rally_stops_when_three_minions_are_alive() {
        let mut c = fight("gremlin_leader_gang", 7);
        use_move(&mut c, "gremlin_leader", 0); // 补满槽 0
        assert_eq!(slots(&c), vec![0, 1, 2, 3], "槽 0 最后补上");
        let names: Vec<String> = c.enemies.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names.len(), 4);
        // 三只小鬼都在,再召集一次什么也不会有
        use_move(&mut c, "gremlin_leader", 0);
        assert_eq!(slots(&c), vec![0, 1, 2, 3], "满了就不再召");
        let after: Vec<String> = c.enemies.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, after, "阵容没变");
    }

    /// 一次召集抽到的两只(按抽取顺序)
    fn rally_pair(seed: u64) -> (String, String) {
        let mut c = fight("gremlin_leader_gang", seed);
        for e in c.enemies.iter_mut() {
            if e.def.id != "gremlin_leader" {
                e.hp = 0;
            }
        }
        c.check_win();
        use_move(&mut c, "gremlin_leader", 0);
        let mut got: Vec<(usize, String)> = c
            .enemies
            .iter()
            .filter(|e| e.def.id != "gremlin_leader")
            .map(|e| (e.slot, e.def.id.to_string()))
            .collect();
        got.sort();
        (got[0].1.clone(), got[1].1.clone())
    }

    #[test]
    fn rally_draws_each_gremlin_on_its_own_and_repeats_with_the_same_seed() {
        assert_eq!(rally_pair(7), rally_pair(7), "同 seed 抽出同一对");
        let mut pairs = std::collections::BTreeSet::new();
        for seed in 0..24 {
            pairs.insert(rally_pair(seed));
        }
        // 参考实现是两只各掷各的,所以抽到一样的两只也正常
        assert!(
            pairs.iter().any(|(a, b)| a == b),
            "应当能抽到两只一样的:{pairs:?}"
        );
        assert!(
            pairs.iter().any(|(a, b)| a != b),
            "也应当抽到两只不一样的:{pairs:?}"
        );
        for (a, b) in &pairs {
            assert!(POOL.contains(&a.as_str()) && POOL.contains(&b.as_str()), "{a}/{b} 不在池里");
        }
    }

    #[test]
    fn reptomancer_daggers_fill_the_reference_slots() {
        let mut c = fight("reptomancer_solo", 7);
        assert_eq!(slots(&c), vec![1, 2, 4], "小刀在 1 和 4,爬行者在 2");
        // 搜索顺序 4、1、3、0:4 和 1 都占着,第一把进 3
        use_move(&mut c, "reptomancer", 0);
        assert_eq!(slots(&c), vec![1, 2, 3, 4], "第一把补进槽 3");
        use_move(&mut c, "reptomancer", 0);
        assert_eq!(slots(&c), vec![0, 1, 2, 3, 4], "最后一把进槽 0");
        assert_eq!(
            c.enemies.iter().filter(|e| e.def.id == "dagger").count(),
            4,
            "四把小刀"
        );
        assert_eq!(c.enemies[0].name, "Dagger #1", "编号按队里位置排");
        assert_eq!(c.enemies[4].name, "Dagger #4");
    }

    #[test]
    fn split_slimes_take_the_parent_slot_and_the_next_one() {
        let mut c = fight("large_slime", 7);
        assert_eq!(slots(&c), vec![0]);
        c.enemies[0].hp = 30;
        c.damage_enemy(0, 1);
        c.end_turn();
        assert_eq!(ids(&c), vec!["acid_slime_medium", "acid_slime_medium"]);
        assert_eq!(slots(&c), vec![0, 1], "两只子体占原来那一格和下一格");
    }
}
