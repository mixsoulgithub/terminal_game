// 战斗引擎:回合流转、抽牌弃牌、卡牌结算、敌人行动.
// 纯逻辑,不碰终端;一局流程(run.rs)只负责在一次战斗前后同步生命与遗物结算.
use crate::core::card::{CardInstance, Cost, Effect, Rarity, Target};
use crate::core::cards;
use crate::core::enemy::{
    CardSpot, Encounter, EnemyDef, EnemyFx, EnemyKind, EnemyState, Intent, PickCtx, Scope,
    Spawned, Special,
};
use crate::core::potions::{DiscoveryPool, PotionDef, PotionFx};
use crate::core::relics::{RelicDef, RelicFx};
use crate::core::status::{Status, Statuses};
use crate::rng::{java_shuffle, FloorStream, JavaRandom, RngRegistry};

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
    /// 本回合行动中刚挂上、当回合回合末不触发的能力(邪教徒的 Ritual).
    /// 参考实现里挂能力是排队动作,排在它自己的回合末钩子之后,所以晚一回合生效.
    pub fresh_powers: Vec<Status>,
    /// 已经脱离战斗(逃跑 / 被首领带走),不会再行动也不再算敌人
    pub escaped: bool,
    /// 参考实现里的槽位:站位与召唤都按它排,下标会变它不会
    pub slot: usize,
    /// 出生序号.召唤会把队里其它怪的下标顶走,回合内靠它认住"正在行动的那只"
    pub uid: u64,
    /// 这一局的飞升等级(0 = 关),意图与招式数值按它换档
    pub asc: u32,
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
        let m = &self.def.moves[self.next_move];
        crate::core::ascension::intent(self.def.id, m.name, m.intent, self.asc)
    }
}

/// 玩家在战斗中的镜像:只带战斗需要的数据
#[derive(Clone, Debug)]
pub struct PlayerBattle {
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub statuses: Statuses,
    /// 敌人这回合刚挂上、第一次回合末递减要跳过的持续状态(参考实现的 justApplied)
    pub fresh_debuffs: Vec<Status>,
}

/// 构造一场战斗需要的输入.拥有所有权,避免借用纠缠.
pub struct CombatSetup {
    pub hp: i32,
    pub max_hp: i32,
    pub deck: Vec<CardInstance>,
    pub relics: Vec<&'static RelicDef>,
    /// 玩家身上的金币(抢劫类敌人要用)
    pub gold: i32,
    /// 开打前是否刚在营火休息过(古代茶具要看这个)
    pub rested: bool,
    /// 营火举铁攒下的力量(吉利亚),开局直接上身
    pub lift_strength: i32,
    /// 整局持续的遗物计数器(笔尖/快乐花/薰香/日晷/双节棍/墨水瓶)
    pub relic_counters: RunRelicCounters,
    /// 御守剩的挡诅咒次数(战斗里"塞进牌组"的寄生也要被它挡掉)
    pub curse_negate: i32,
    /// 这一局的飞升等级(0 = 关):怪物血量/招式/开局状态按它换档
    pub asc: u32,
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
    /// 只有还能升级的牌(武装)
    Upgradeable,
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
            ChoiceFilter::Upgradeable => card.can_upgrade(),
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
    /// 弃掉(进弃牌堆)
    Discard,
    /// 直接移出这局(调试用)
    Remove,
    /// 洗进抽牌堆(尼尔瑞的抄本)
    ToDrawShuffled,
    /// 升级这张牌(武装)
    Upgrade,
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
    /// 选完之后抽等量张(赌徒之酿)
    pub draw_after: bool,
    /// 选中的那份给几张(默认 1;神圣树皮把发现类药水的份数翻倍)
    pub copies: usize,
    /// 选中的牌本回合 0 费(发现类药水;工具箱与抄本不免费)
    pub free: bool,
}

/// 整局持续的遗物计数器:参考实现里这些数挂在 Run 的遗物实例上(relic counter),
/// 跨战斗累加并随存档走.进战斗时由一局流程注入,战斗中随时由 sync_combat 收回.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunRelicCounters {
    /// 笔尖 0..9:计数到 9 时打出的攻击翻倍,第 10 张打完归零
    pub pen_nib: i32,
    /// 快乐花 0..2:每 3 回合给 1 点能量
    pub happy_flower: i32,
    /// 薰香 0..5:每 6 回合给 1 层无形
    pub incense: i32,
    /// 日晷 0..2:每 3 次洗牌给 2 点能量
    pub sundial: i32,
    /// 双节棍累计打出的攻击数:每 10 张给 1 点能量
    pub attacks_total: i32,
    /// 墨水瓶累计打出的牌数:每 10 张抽 1 张
    pub cards_total: i32,
}

/// 战斗中遗物需要的计时器与一次性标记(与参考实现的 relicCounter 对应).
/// 其中"跨战斗累计"的那几个(笔尖/快乐花/薰香/日晷/双节棍/墨水瓶)在进出战斗时
/// 与 Run::relic_counters 同步;其余都是本场战斗内的状态,每场重建.
#[derive(Clone, Debug, Default)]
pub struct RelicState {
    /// 本回合打出的攻击/技能数(苦无/手里剑/折扇/拆信刀)
    pub attacks_this_turn: i32,
    pub skills_this_turn: i32,
    /// 整局累计打出的攻击数(双节棍;跨战斗,随 Run 走)
    pub attacks_total: i32,
    /// 整局累计打出的牌数(墨水瓶;跨战斗,随 Run 走)
    pub cards_total: i32,
    /// 笔尖 0..9:数到 9 时下一张攻击翻倍(跨战斗,随 Run 走)
    pub pen_nib: i32,
    /// 快乐花 0..2(跨战斗,随 Run 走)
    pub happy_flower: i32,
    /// 薰香 0..5(跨战斗,随 Run 走)
    pub incense: i32,
    /// 日晷 0..2(跨战斗,随 Run 走)
    pub sundial: i32,
    /// 上一回合打出的攻击数(战争艺术)与牌数(怀表)
    pub attacks_last_turn: i32,
    pub cards_last_turn: i32,
    /// 本回合打出过的类型(橙皮,位掩码 1 攻击 2 技能 4 能力)
    pub types_played: i32,
    /// 本场掉过血(百年拼图)
    pub hp_lost: bool,
    /// 下回合补上的格挡(自成型黏土)
    pub banked_block: i32,
    /// 下回合多抽几张(怀表)
    pub next_turn_draw: i32,
    /// 振奋剩余层数(赤牛:第一次攻击加伤)
    pub vigor: i32,
    /// 只在第一回合有的力量(诱变力量),第 1 回合结束收回
    pub strength_turn1: i32,
    /// 化石螺壳还能挡几次掉血
    pub helix: i32,
    /// 本回合已经因为弃牌回过能(风筝)
    pub kite_used: bool,
    /// 本回合已经触发过死灵之书
    pub necro_used: bool,
    /// 红骷髅是否生效中
    pub bloodied: bool,
    /// 这一场已经靠蜥蜴尾巴保过一次命
    pub lizard_used: bool,
    /// 这个回合结束已经问过尼尔瑞的抄本
    pub nilrys_used: bool,
}

impl RelicState {
    /// 取出跨战斗累计的那部分计数(交给一局流程保存/写存档)
    pub fn run_counters(&self) -> RunRelicCounters {
        RunRelicCounters {
            pen_nib: self.pen_nib,
            happy_flower: self.happy_flower,
            incense: self.incense,
            sundial: self.sundial,
            attacks_total: self.attacks_total,
            cards_total: self.cards_total,
        }
    }

    /// 注入跨战斗累计的那部分计数(战斗开始时;本场内的状态不受影响)
    pub fn set_run_counters(&mut self, c: RunRelicCounters) {
        self.pen_nib = c.pen_nib;
        self.happy_flower = c.happy_flower;
        self.incense = c.incense;
        self.sundial = c.sundial;
        self.attacks_total = c.attacks_total;
        self.cards_total = c.cards_total;
    }
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
    pub streams: RngRegistry,
    pub encounter_id: &'static str,
    pub kind: EnemyKind,
    /// 这一局的飞升等级(0 = 关)
    pub asc: u32,
    /// 这一场是不是"燃烧精英"的战斗(打通给绿钥匙)
    pub burning: bool,
    /// 本场对敌人造成的总伤害,结算界面用
    pub damage_dealt: i32,
    /// 已经产出的日志条数(日志会截断,所以用序号而不是长度)
    pub log_seq: u64,
    /// 本场战斗通过卡牌赚到的金币,由一局流程收走(贪婪之手)
    pub gold_gained: i32,
    /// 战斗内永久成长的牌(血祭匕首):(牌组原件下标, 增长量),由一局流程写回
    pub deck_growth: Vec<(usize, i32)>,
    /// 本场战斗带的遗物(效果在对应时机读)
    pub relics: Vec<&'static RelicDef>,
    /// 遗物的计时与一次性标记
    pub rs: RelicState,
    /// 冰激凌:上一回合没用完的能量
    pub carry_energy: i32,
    /// 开打前休息过(古代茶具)
    pub rested: bool,
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
    /// 御守剩的挡诅咒次数(战斗里塞进牌组的诅咒也走它),一局流程在同步时收走
    pub curse_negate: i32,
    /// 时间吞噬者的时间扭曲:这一张牌打完就要结束回合
    pub force_end_turn: bool,
    /// 玩家最近一次指向的敌人(被夹击时判断从哪边挨打)
    pub facing: usize,
    /// 下一只怪的出生序号
    next_uid: u64,
    /// 仙女在瓶中:身上有这瓶药时打开,致命伤改为回血
    pub fairy_save: bool,
    /// 这一场已经靠仙女药水保过一次命(run 层据此消耗那一格)
    pub fairy_used: bool,
    /// 回合结束正等一个选牌(尼尔瑞的抄本):选完才把回合交给对面
    pub pending_end_turn: bool,
    /// 选牌之后还没跑的效果:出牌时挂起的收尾,选完再按序补跑.
    /// 参考实现把动作队列的尾巴快照进 resumeArgs.__tail,选完 replayTail 放回去;
    /// 这里同理 —— 否则"选牌之后的效果"会抢在选牌前面跑,顺序与掷点位置都错
    choice_tail: Vec<Effect>,
    /// 那一截效果瞄准的敌人
    choice_tail_target: Option<usize>,
    /// 那一截效果接着用的结算统计(已打出的伤害/消耗数)
    choice_tail_ctx: PlayCtx,
    /// 打这张牌之前,身上有尖刺外壳的敌人快照:(敌人下标, 层数).
    /// 原版把这道反伤排在这张牌的效果之后结算,而战斗胜利清空动作队列时
    /// 只清 clearOnCombatVictory=true 的那些,反伤不在其中 —— 所以这一击
    /// 把守护者打死,反伤照样结算.出牌前先快照,结算后再看现在是否还活着就错了.
    sharp_hide: Vec<(usize, i32)>,
}

/// 单次打牌过程中的临时统计
#[derive(Default, Clone, Copy)]
struct PlayCtx {
    x: i32,
    exhausted: i32,
    unblocked: i32,
    /// 结算时的手牌张数快照(悔恨按手牌数掉血,回合结束时手牌已经清完,所以要提前记)
    hand_size: i32,
}

impl Combat {
    pub fn new(enc: &'static Encounter, setup: CombatSetup, mut streams: RngRegistry) -> Self {
        // 阵容:带抽签规则的遭遇(原版开战才定阵容)按原版规则抽,其余用固定名单.
        // 抽签函数连候选的血都掷在里面(原版构造怪物组时就是这样烧 monsterHpRng 的);
        // 固定名单的血在这里按槽位顺序补掷,顺序与抽签函数内的掷法一致.
        let asc = setup.asc;
        let lineup: Vec<Spawned> = match enc.lineup {
            Some(roll) => roll(&mut streams, asc),
            None => enc
                .enemies
                .iter()
                .map(|&id| {
                    let def = crate::core::enemies::enemy_def_or_panic(id);
                    let (lo, hi) = crate::core::ascension::hp_range(def, asc);
                    Spawned {
                        id,
                        hp: streams.floor(FloorStream::MonsterHpRng).range_inclusive(lo, hi),
                        rolled: None,
                    }
                })
                .collect(),
        };
        let mut enemies = Vec::new();
        for (i, sp) in lineup.iter().enumerate() {
            let id = sp.id;
            let def = crate::core::enemies::enemy_def_or_panic(id);
            // 同名敌人加编号,保证日志与选中项能对上
            let dup = lineup.iter().filter(|e| e.id == id).count() > 1;
            let name = if dup {
                format!("{} #{}", def.name, i + 1)
            } else {
                def.name.to_string()
            };
            // 血量由抽签函数(或上面的固定名单补掷)给定
            let hp = sp.hp;
            let mut statuses = Statuses::new();
            for (s, n) in def.innate {
                let n = crate::core::ascension::innate_amount(def.id, *s, *n, asc);
                if n == 0 {
                    statuses.mark(*s);
                } else {
                    statuses.add(*s, n);
                }
            }
            for (s, n) in crate::core::ascension::bonus_innate(def.id, asc) {
                statuses.add(*s, *n);
            }
            let mut state = EnemyState::default();
            // 抽签时就把咬伤掷好的怪:记下来,spawn 钩子不再重掷
            if let Some(r) = sp.rolled {
                state.rolled = r;
                state.rolled_preset = true;
            }
            let mut block = def.start_block;
            // 遭遇级预置状态:开局的状态/格挡,以及"已经行动过"的招式历史
            for p in enc.presets.iter().filter(|p| p.slots.contains(&i)) {
                for (s, n) in p.statuses {
                    statuses.add(*s, *n);
                }
                block += p.block;
                state.turns = p.acted_turns;
                // 预置了"已经行动过"的招式历史:首招已经掷过,不能再走开局分支
                // (参考实现里这些怪的 moveHistory 非空,firstTurn 为假)
                if p.acted_turns > 0 || p.last_move.is_some() {
                    state.move_rolled = true;
                }
                if let Some(name) = p.last_move {
                    state.last = Some(def.move_index(name).unwrap_or_else(|| {
                        panic!("enemy {} has no move named {name}", def.id)
                    }));
                }
            }
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
                fresh_powers: Vec::new(),
                escaped: false,
                // 站位按遭遇表给的槽位,不一定是 0,1,2...(自动机的铜球要排在它前面)
                slot: crate::core::enemies::initial_slot(enc, i),
                uid: i as u64,
                asc,
                state,
            });
        }

        let enemies_len = enemies.len();
        // 战斗开始时洗牌:参考实现是 shuffleRng 掷一个 long 给 java.Random 定种,
        // 再用 Collections.shuffle 洗牌堆
        let mut deck = setup.deck;
        // 记下每张牌在牌组原件里的下标:战斗内永久成长(血祭匕首)要写回去.
        // setup.deck 是 run 牌组的按序克隆,所以这里的位置就是原件下标.
        for (i, c) in deck.iter_mut().enumerate() {
            c.master_idx = Some(i);
        }
        // 恶咒人偶:每张诅咒开局给力量,洗牌前先数一下
        let deck_curses = deck
            .iter()
            .filter(|card| card.kind() == crate::core::card::CardType::Curse)
            .count() as i32;
        java_shuffle(
            &mut deck,
            &mut JavaRandom::new(streams.floor(FloorStream::ShuffleRng).random_long()),
        );
        // 抽牌堆的顶牌是下标 0(draw_cards 从头取).Innate 牌在洗牌后挪到堆顶,
        // 开局的 DRAW_PER_TURN 张照样从头抽,自然先把它们抓进手;
        // 多张 Innate 之间保持洗出来的先后(参考实现也是稳定地挪到最前).
        let (innate, rest): (Vec<CardInstance>, Vec<CardInstance>) =
            deck.into_iter().partition(|c| c.is_innate());
        let deck: Vec<CardInstance> = innate.into_iter().chain(rest).collect();

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
                fresh_debuffs: Vec::new(),
            },
            hand: Vec::new(),
            draw: deck,
            discard: Vec::new(),
            exhaust: Vec::new(),
            energy: BASE_ENERGY,
            max_energy: BASE_ENERGY,
            turn: 0,
            phase: Phase::PlayerTurn,
            log: Vec::new(),
            streams,
            encounter_id: enc.id,
            kind: enc.kind,
            asc,
            burning: false,
            damage_dealt: 0,
            log_seq: 0,
            gold_gained: 0,
            deck_growth: Vec::new(),
            relics: setup.relics.clone(),
            rs: RelicState::default(),
            carry_energy: 0,
            rested: setup.rested,
            all_upgraded: false,
            cards_played: 0,
            bombs: Vec::new(),
            relic_thorns: 0,
            player_gold: setup.gold,
            stasis: Vec::new(),
            last_hit: 0,
            deck_cards: Vec::new(),
            curse_negate: setup.curse_negate,
            force_end_turn: false,
            facing: 1,
            next_uid: enemies_len as u64,
            fairy_save: false,
            fairy_used: false,
            pending_end_turn: false,
            choice_tail: Vec::new(),
            choice_tail_target: None,
            choice_tail_ctx: PlayCtx::default(),
            sharp_hide: Vec::new(),
        };
        // 跨战斗的遗物计数器由一局流程注入(参考实现里这些数挂在 Run 的遗物上,
        // 开局第一回合就会 +1,所以必须在 start_turn 之前放进去)
        c.rs.set_run_counters(setup.relic_counters);

        // 开局的 spawn 掷点(参考实现里这一批排在洗牌之后、掷首招之前).
        // 用的还是 monsterHpRng:卷曲层数、开局定死的咬伤都在这一掷
        {
            let Combat {
                streams, enemies, ..
            } = &mut c;
            for e in enemies.iter_mut() {
                let mut spawn = crate::core::enemy::SpawnCtx {
                    rng: streams.floor(FloorStream::MonsterHpRng),
                    statuses: &mut e.statuses,
                    state: &mut e.state,
                    asc,
                };
                (e.def.spawn)(&mut spawn);
            }
        }

        // 遭遇级预置:开战就给玩家的状态(第四幕精英的被包围)
        for (s, n) in enc.player_statuses {
            c.player.statuses.add(*s, *n);
        }
        // 遗物:战斗开始结算(参考实现的 atBattleStart / atBattleStartPreDraw)
        let mut extra_energy = 0;
        let mut extra_draw = 0usize;
        let mut start_heal = 0;
        let elite_or_boss = matches!(enc.kind, EnemyKind::Elite | EnemyKind::Boss);
        for r in &setup.relics {
            let fx = r.fx;
            extra_energy += fx.combat_start_energy_per_turn;
            if elite_or_boss {
                extra_energy += fx.combat_start_energy_elite_only;
            }
            extra_draw += fx.combat_start_draw.max(0) as usize;
            c.relic_thorns += fx.thorns;
            start_heal += fx.combat_start_heal;
            c.rs.vigor += fx.combat_start_vigor;
            c.rs.helix += fx.combat_start_buffer;
            if fx.combat_start_strength != 0 {
                c.player
                    .statuses
                    .add(Status::Strength, fx.combat_start_strength);
            }
            if fx.combat_start_strength_turn1 != 0 {
                c.player
                    .statuses
                    .add(Status::Strength, fx.combat_start_strength_turn1);
                c.rs.strength_turn1 += fx.combat_start_strength_turn1;
            }
            if fx.combat_start_dexterity != 0 {
                c.player
                    .statuses
                    .add(Status::Dexterity, fx.combat_start_dexterity);
            }
            if fx.combat_start_artifact != 0 {
                c.player
                    .statuses
                    .add(Status::Artifact, fx.combat_start_artifact);
            }
            if fx.combat_start_plated_armor != 0 {
                c.player
                    .statuses
                    .add(Status::PlatedArmor, fx.combat_start_plated_armor);
            }
            if fx.combat_start_confused {
                c.player.statuses.add(Status::Confused, 1);
            }
            if fx.combat_start_self_weak != 0 {
                c.player.statuses.add(Status::Weak, fx.combat_start_self_weak);
            }
            if fx.combat_start_strength_elite != 0 && elite_or_boss {
                c.player
                    .statuses
                    .add(Status::Strength, fx.combat_start_strength_elite);
            }
            if fx.combat_start_strength_per_curse != 0 {
                let curses = deck_curses;
                if curses > 0 {
                    c.player
                        .statuses
                        .add(Status::Strength, fx.combat_start_strength_per_curse * curses);
                }
            }
            if fx.combat_start_enemy_strength != 0 {
                for e in c.enemies.iter_mut() {
                    e.statuses.add(Status::Strength, fx.combat_start_enemy_strength);
                }
            }
            if fx.combat_start_enemy_vulnerable != 0 {
                // 走"上减益"那条路:原版的开战减益也是 ApplyPowerAction,神器照样顶掉
                for i in 0..c.enemies.len() {
                    c.add_enemy_status(i, Status::Vulnerable, fx.combat_start_enemy_vulnerable);
                }
            }
            if fx.combat_start_enemy_weak != 0 {
                for i in 0..c.enemies.len() {
                    c.add_enemy_status(i, Status::Weak, fx.combat_start_enemy_weak);
                }
            }
            if fx.elite_hp_reduction_pct > 0 && enc.kind == EnemyKind::Elite {
                let pct = fx.elite_hp_reduction_pct;
                for e in c.enemies.iter_mut() {
                    // 反编译(sts_lightspeed BattleContext.cpp):`m.curHp = (int)(m.maxHp * .75)`,
                    // 即把当前血量截断到上限的 75%,上限不动(开战满血,所以用 hp 与 max_hp 等价).
                    // 只降当前血量,不做 max_hp - floor(max_hp*25/100) 那种算法(83 -> 62 而非 63).
                    e.hp = (e.hp * (100 - pct) / 100).max(1);
                }
            }
            if fx.boss_combat_heal > 0 && enc.kind == EnemyKind::Boss {
                start_heal += fx.boss_combat_heal;
            }
            if fx.combat_start_wounds > 0 {
                for _ in 0..fx.combat_start_wounds {
                    c.draw.push(crate::core::cards::card("wound"));
                }
                let seed = c
                    .streams
                    .floor(FloorStream::ShuffleRng)
                    .random_long();
                java_shuffle(&mut c.draw, &mut JavaRandom::new(seed));
            }
        }
        // 瓶装遗物:被封装的牌开局就压在抽牌堆顶(下标 0 一侧),
        // 开局那一抽直接进手.多张封装的牌照参考实现:堆里越深的越靠上.
        let idxs: Vec<usize> = c
            .draw
            .iter()
            .enumerate()
            .filter(|(_, x)| x.bottled)
            .map(|(i, _)| i)
            .collect();
        let mut bottled: Vec<CardInstance> = idxs
            .into_iter()
            .rev()
            .map(|i| c.draw.remove(i))
            .collect();
        bottled.reverse();
        for card in bottled {
            c.draw.insert(0, card);
        }
        // 吉利亚:营火举铁的层数在开局上身(参考实现 atBattleStart)
        if setup.lift_strength > 0 {
            c.player.statuses.add(Status::Strength, setup.lift_strength);
            c.push_log(
                LogKind::Info,
                format!("Girya grants {} Strength", setup.lift_strength),
            );
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
        // 红骷髅:开战就在半血以下的话,原版 atBattleStart 当场补上力量
        // (不进这一步的话要等玩家先掉一次血才补,第一回合就少 3 点力量)
        c.refresh_bloodied();
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

    /// 所有遗物某个数值字段的和
    fn relic_sum(&self, f: impl Fn(&RelicFx) -> i32) -> i32 {
        self.relics.iter().map(|r| f(&r.fx)).sum()
    }

    /// 所有遗物某个数值字段的最大值(同一数值的多件遗物取大,不叠加)
    fn relic_max(&self, f: impl Fn(&RelicFx) -> i32) -> i32 {
        self.relics.iter().map(|r| f(&r.fx)).max().unwrap_or(0)
    }

    /// 有没有任意一件遗物满足这个开关
    fn relic_any(&self, f: impl Fn(&RelicFx) -> bool) -> bool {
        self.relics.iter().any(|r| f(&r.fx))
    }

    /// 红骷髅:血量掉到一半以下就补力量,回到一半以上就收回
    fn refresh_bloodied(&mut self) {
        let want = self.relic_sum(|fx| fx.strength_when_bloodied);
        if want <= 0 {
            return;
        }
        let below = self.player.hp * 2 <= self.player.max_hp;
        if below == self.rs.bloodied {
            return;
        }
        self.rs.bloodied = below;
        let delta = if below { want } else { -want };
        self.player.statuses.add(Status::Strength, delta);
        self.push_log(
            LogKind::Info,
            format!("Red Skull: {} Strength", if below { "gains" } else { "loses" }),
        );
    }

    /// 掉血后要看的遗物(百年拼图 / 符文方块 / 自成型黏土 / 红骷髅)
    fn on_hp_lost(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        if !self.rs.hp_lost {
            self.rs.hp_lost = true;
            let n = self.relic_sum(|fx| fx.draw_on_first_hp_loss);
            if n > 0 {
                self.push_log(LogKind::Player, format!("Centennial Puzzle: draw {n}"));
                self.draw_cards(n as usize);
            }
        }
        let cube = self.relic_sum(|fx| fx.draw_on_hp_loss);
        if cube > 0 {
            self.draw_cards(cube as usize);
        }
        let clay = self.relic_sum(|fx| fx.block_next_turn_on_hp_loss);
        if clay > 0 {
            self.rs.banked_block += clay;
        }
        self.refresh_bloodied();
    }

    fn start_turn(&mut self, extra_draw: usize) {
        self.turn += 1;
        // 本回合的计时器归零
        self.rs.attacks_this_turn = 0;
        self.rs.skills_this_turn = 0;
        self.rs.types_played = 0;
        self.rs.kite_used = false;
        self.rs.necro_used = false;
        self.rs.nilrys_used = false;
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
        // 冰激凌:上一回合没用完的能量留到这一回合
        if self.relic_any(|fx| fx.conserve_energy) {
            self.energy += self.carry_energy;
            self.carry_energy = 0;
        }
        // 格挡在回合开始清空,除非有壁垒;卡钳只掉固定的一截
        if !self.player.statuses.has(Status::Barricade) {
            let cap = self.relic_sum(|fx| fx.block_loss_cap);
            if cap > 0 {
                self.player.block = (self.player.block - cap).max(0);
            } else {
                self.player.block = 0;
            }
        }
        // 锚之类的"开局格挡"要在回合开始的清空之后再补,否则第 1 回合就被清掉了
        if self.turn == 1 {
            let b = self.relic_sum(|fx| fx.combat_start_block);
            if b > 0 {
                self.player.block += b;
            }
        }
        // 自成型黏土:上一个回合攒下的格挡
        if self.rs.banked_block > 0 {
            let b = self.rs.banked_block;
            self.rs.banked_block = 0;
            self.gain_block(b, false, false);
            self.push_log(LogKind::Player, format!("Self-Forming Clay: {b} Block"));
        }
        // 快乐花:每 3 个回合给 1 点能量
        let flower = self.relic_sum(|fx| fx.energy_every_3_turns);
        if flower > 0 {
            self.rs.happy_flower += 1;
            if self.rs.happy_flower >= 3 {
                self.rs.happy_flower = 0;
                self.energy += flower;
            }
        }
        // 薰香:每 6 个回合给 1 层无形
        let incense = self.relic_sum(|fx| fx.intangible_every_6_turns);
        if incense > 0 {
            self.rs.incense += 1;
            if self.rs.incense >= 6 {
                self.rs.incense = 0;
                self.player.statuses.add(Status::Intangible, incense);
                self.push_log(LogKind::Player, "Incense Burner: Intangible".to_string());
            }
        }
        if self.turn == 1 {
            // 灯笼之类的"第一回合"能量
            self.energy += self.relic_sum(|fx| fx.combat_start_energy);
            if self.rested {
                self.energy += self.relic_sum(|fx| fx.energy_turn1_if_rested);
            }
        } else if self.rs.attacks_last_turn == 0 {
            // 战争艺术:上一回合没打攻击就给能量
            self.energy += self.relic_sum(|fx| fx.energy_if_no_attack_last_turn);
        }
        // 怀表:上一回合打出的牌少就多抽.第 1 回合没有"上一回合",
        // cards_last_turn 的初值 0 不能当成"这回合一张没出"(参考实现挂在回合末)
        if self.turn > 1 && self.rs.cards_last_turn <= 3 {
            self.rs.next_turn_draw += self.relic_sum(|fx| fx.draw_next_turn_if_low_play);
        }
        // 硫磺:每回合自己 +2 力量,敌人 +1
        let brim = self.relic_max(|fx| fx.brimstone_self);
        if brim > 0 {
            self.player.statuses.add(Status::Strength, brim);
            let enemy = self.relic_max(|fx| fx.brimstone_enemy);
            for i in 0..self.enemies.len() {
                if self.enemies[i].alive() {
                    self.enemies[i].statuses.add(Status::Strength, enemy);
                }
            }
            self.push_log(LogKind::Info, format!("Brimstone: +{brim} Strength"));
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
        let relic_draw = self.relic_sum(|fx| fx.draw_per_turn).max(0) as usize;
        let bonus_draw = extra_draw + relic_draw + self.rs.next_turn_draw.max(0) as usize;
        self.rs.next_turn_draw = 0;
        self.draw_cards((DRAW_PER_TURN + bonus_draw).saturating_sub(less));
        // 水银沙漏:回合开始对全体敌人造成伤害
        let hourglass = self.relic_sum(|fx| fx.damage_all_turn_start);
        if hourglass > 0 {
            for i in self.alive_enemies() {
                self.damage_enemy_plain(i, hourglass);
            }
            self.push_log(
                LogKind::Player,
                format!("Mercury Hourglass deals {hourglass} to all enemies"),
            );
            self.settle_deaths();
        }
        // 角盔/船长的轮子:第 2/第 3 回合给格挡
        if self.turn == 2 {
            let b = self.relic_sum(|fx| fx.block_turn2);
            if b > 0 {
                self.gain_block(b, false, false);
            }
        }
        if self.turn == 3 {
            let b = self.relic_sum(|fx| fx.block_turn3);
            if b > 0 {
                self.gain_block(b, false, false);
            }
        }
        // 扭曲的钳子:回合开始随机升级手里一张牌
        if self.relic_any(|fx| fx.upgrade_random_hand_at_turn_start) && !self.hand.is_empty() {
            let idx = self
                .streams
                .floor(FloorStream::MiscRng)
                .random(self.hand.len() as u32 - 1) as usize;
            if self.hand[idx].upgrade() {
                let label = self.hand[idx].label();
                self.push_log(LogKind::Player, format!("Warped Tongs upgrades {label}"));
            }
        }
        // 魔法书:战斗开始时往手里塞一张随机能力牌,本回合 0 费(只在开局,不是每回合)
        if self.turn == 1 && self.relic_any(|fx| fx.add_random_power_card) {
            self.add_random_power_to_hand();
        }
        // 赌徒筹码:开局弃任意张再抽等量张
        if self.turn == 1 && self.relic_any(|fx| fx.gambling_chip) && !self.hand.is_empty() {
            self.begin_choice(
                ChoiceSource::Hand,
                ChoiceAction::Discard,
                ChoiceFilter::Any,
                0,
                "Gambling Chip",
            );
            if let Some(ch) = self.choice.as_mut() {
                ch.draw_after = true;
            }
        }
        let brutal = self.player.statuses.get(Status::Brutality);
        if brutal > 0 {
            // 掉血来自能力而不是卡牌,所以不触发渴望
            self.lose_hp_player(1, false);
            self.draw_cards(brutal as usize);
        }
        // 混乱:回合开始打出抽牌堆顶那张(打完按它自己的规矩去弃牌堆/消耗堆)
        if self.player.statuses.has(Status::Mayhem) {
            self.play_top_of_draw(false, "Mayhem");
        }
        // 磁力:回合开始随机给一张无色牌
        let mag = self.player.statuses.get(Status::Magnetism);
        for _ in 0..mag {
            self.add_random_colorless_to_hand(false, false);
        }
        // 工具箱:第一回合抽完牌亮出几张无色牌,挑一张进手(参考实现 atStartOfTurnPostDraw)
        let toolbox = self.relic_max(|fx| fx.combat_start_colorless_pick);
        if self.turn == 1 && toolbox > 0 {
            let pool = self.discovery_pool(DiscoveryPool::Colorless);
            self.offer_pick(pool, toolbox as usize, false, "Toolbox: choose 1");
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
        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
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

    /// 打出抽牌堆顶那张;exhaust_after 为真时打完直接消耗(浩劫),
    /// via 是播报里"谁打出了它"(浩劫/万物皆动/混沌药剂)
    fn play_top_of_draw(&mut self, exhaust_after: bool, via: &str) {
        // 抽牌堆空了先把弃牌堆洗回来(参考实现的 PlayTopCardAction 会洗),
        // 没得洗才什么都不做
        if self.draw.is_empty() && !self.reshuffle_discard_into_draw() {
            return;
        }
        let mut card = self.draw.remove(0);
        let label = card.label();
        self.push_log(LogKind::Player, format!("{via} plays {label}"));
        let kind = card.kind();
        // 记进浩劫链(层级 +1 表示嵌了一层),表现层据此叠播报
        self.havoc_depth = self.havoc_depth.saturating_add(1);
        self.havoc_chain.push((self.havoc_depth, label));
        let target = self.pick_random_alive();
        self.snapshot_sharp_hide();
        let mut top_ctx = PlayCtx::default();
        self.resolve(&mut card, target, &mut top_ctx);
        self.havoc_depth = self.havoc_depth.saturating_sub(1);
        card.free_this_turn = false;
        if card.kind() == crate::core::card::CardType::Power {
            // 能力牌一样是打完就退场(浩劫放出来的也不例外)
            self.vanish_card(card);
        } else if exhaust_after || card.is_exhaust() {
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
        self.on_relic_card_played(kind);
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

    /// 遗物里"每打出一张牌"就触发的部分
    fn on_relic_card_played(&mut self, kind: crate::core::card::CardType) {
        use crate::core::card::CardType;
        // 墨水瓶:每 10 张牌抽 1 张(没有这件遗物时不数,拿到手时从 0 起)
        let ink = self.relic_sum(|fx| fx.draw_per_10_cards);
        if ink > 0 {
            self.rs.cards_total += 1;
        }
        match kind {
            CardType::Attack => {
                self.rs.attacks_this_turn += 1;
                self.rs.types_played |= 1;
                // 双节棍:数满 10 张攻击回 1 点能量(没有这件遗物时不数)
                if self.relic_sum(|fx| fx.energy_per_10_attacks) > 0 {
                    self.rs.attacks_total += 1;
                }
                // 笔尖:数满 10 归零,第 10 张的攻击在结算时(pen_nib == 9)翻倍.
                // 没有这件遗物时不数,拿到手时自然从 0 起(参考实现挂在遗物上)
                if self.relic_any(|fx| fx.double_damage_per_10_attacks) {
                    self.rs.pen_nib += 1;
                    if self.rs.pen_nib >= 10 {
                        self.rs.pen_nib = 0;
                    }
                }
            }
            CardType::Skill => {
                self.rs.skills_this_turn += 1;
                self.rs.types_played |= 2;
            }
            CardType::Power => {
                self.rs.types_played |= 4;
                // 鸟面坛:每打出一张能力牌回血
                let urn = self.relic_sum(|fx| fx.heal_on_power_card);
                if urn > 0 {
                    self.heal_player(urn);
                }
            }
            _ => {}
        }
        // 双节棍:每 10 张攻击回 1 点能量(数满 10 归零,与参考实现一致)
        let nunchaku = self.relic_sum(|fx| fx.energy_per_10_attacks);
        if nunchaku > 0 && kind == CardType::Attack && self.rs.attacks_total >= 10 {
            self.rs.attacks_total = 0;
            self.energy += nunchaku;
            self.push_log(LogKind::Player, format!("Nunchaku: +{nunchaku} energy"));
        }
        // 墨水瓶:每 10 张牌抽 1(数满 10 归零)
        if ink > 0 && self.rs.cards_total >= 10 {
            self.rs.cards_total = 0;
            self.draw_cards(ink as usize);
        }
        // 苦无/手里剑/折扇:每 3 张攻击
        if kind == CardType::Attack && self.rs.attacks_this_turn % 3 == 0 {
            let dex = self.relic_sum(|fx| fx.dexterity_per_3_attacks);
            if dex > 0 {
                self.player.statuses.add(Status::Dexterity, dex);
            }
            let str = self.relic_sum(|fx| fx.strength_per_3_attacks);
            if str > 0 {
                self.player.statuses.add(Status::Strength, str);
            }
            let block = self.relic_sum(|fx| fx.block_per_3_attacks);
            if block > 0 {
                self.gain_block(block, false, false);
            }
        }
        // 拆信刀:每 3 张技能对全体敌人造成伤害
        if kind == CardType::Skill && self.rs.skills_this_turn % 3 == 0 {
            let dmg = self.relic_sum(|fx| fx.damage_all_per_3_skills);
            if dmg > 0 {
                for i in self.alive_enemies() {
                    self.damage_enemy_plain(i, dmg);
                }
                self.push_log(
                    LogKind::Player,
                    format!("Letter Opener deals {dmg} to all enemies"),
                );
                self.settle_deaths();
            }
        }
        // 木乃伊之手:打出能力牌就让手里一张随机牌本回合 0 费.
        // 候选只有"当前还真的要花费用"的牌(原版:cost>0 且本回合费用>0 且不是免费打出),
        // 已经 0 费/本回合已免费的牌不参选 —— 参选集合不同,掷点结果与后面整条链都会偏.
        if kind == CardType::Power && self.relic_any(|fx| fx.zero_hand_card_on_power) {
            let candidates: Vec<usize> = (0..self.hand.len())
                .filter(|&i| self.hand[i].fixed_cost().unwrap_or(0) > 0)
                .collect();
            if !candidates.is_empty() {
                let idx = candidates
                    [self.streams.floor(FloorStream::CardRandomRng).random(candidates.len() as u32 - 1) as usize];
                self.hand[idx].free_this_turn = true;
                let label = self.hand[idx].label();
                self.push_log(LogKind::Player, format!("Mummified Hand: {label} costs 0"));
            }
        }
        // 橙皮:三种类型都打出过就清掉自己的减益
        if self.relic_any(|fx| fx.clear_debuffs_on_all_types) && self.rs.types_played == 7 {
            self.player.statuses.clear_debuffs();
            self.push_log(LogKind::Player, "Orange Pellets clears your debuffs".to_string());
            self.rs.types_played = 0;
        }
        // 不休陀螺:手里空了就补一张
        if self.hand.is_empty() && self.relic_any(|fx| fx.draw_on_empty_hand) {
            self.draw_cards(1);
        }
    }

    /// 出牌前快照:此刻站着的、身上有尖刺外壳的敌人(层数).结算时用这份快照,
    /// 这样"被这张牌打死的守护者"也会照常反伤(见 sharp_hide 字段的注释).
    fn snapshot_sharp_hide(&mut self) {
        self.sharp_hide = self
            .enemies
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive())
            .map(|(i, e)| (i, e.statuses.get(Status::SharpHide)))
            .filter(|(_, n)| *n > 0)
            .collect();
    }

    /// 敌人身上"玩家每打出一张牌"就触发的机制
    fn on_enemy_card_hooks(&mut self, kind: crate::core::card::CardType) {
        use crate::core::card::CardType;
        // 尖刺外壳:打攻击牌就挨刺.伤害排在牌的效果之后,但不看目标此刻是否还活着 ——
        // 原版这道反伤不落在"胜利时要清掉的动作"里(clearOnCombatVictory=false),
        // 所以把守护者打死的这一击照吃.层数取出牌前的快照(见 sharp_hide 字段).
        if kind == CardType::Attack {
            let hides: Vec<(usize, i32)> = std::mem::take(&mut self.sharp_hide);
            for (i, hide) in hides {
                if hide <= 0 {
                    continue;
                }
                let name = self
                    .enemies
                    .get(i)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                let (taken, _) = self.hit_player(hide);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name}'s Sharp Hide deals {taken}"),
                );
                if self.phase == Phase::Lost {
                    return;
                }
            }
        }
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
                    let pos = self.streams.floor(FloorStream::CardRandomRng).below(self.draw.len() as u32 + 1) as usize;
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

    /// 弃牌堆洗回抽牌堆(洗牌掷点与 onShuffle 都在里面).没牌可洗返回 false
    fn reshuffle_discard_into_draw(&mut self) -> bool {
        if self.discard.is_empty() {
            return false;
        }
        self.draw = std::mem::take(&mut self.discard);
        let count = self.draw.len();
        java_shuffle(&mut self.draw, &mut JavaRandom::new(self.streams.floor(FloorStream::ShuffleRng).random_long()));
        self.push_log(
            LogKind::Info,
            format!("shuffled {count} cards into the draw pile"),
        );
        self.on_shuffle();
        true
    }

    /// 抽牌;抽牌堆空了就把弃牌堆洗回来.顶牌在下标 0,从头取.
    /// 挂 No Draw 时本回合不能再抽(连洗牌掷点也不消耗).
    pub fn draw_cards(&mut self, n: usize) {
        if self.player.statuses.has(Status::NoDraw) {
            self.push_log(
                LogKind::Info,
                "No Draw: cannot draw any more cards this turn".to_string(),
            );
            return;
        }
        for _ in 0..n {
            if self.hand.len() >= HAND_LIMIT {
                self.push_log(LogKind::Info, format!("hand is full ({HAND_LIMIT})"));
                return;
            }
            if self.draw.is_empty() && !self.reshuffle_discard_into_draw() {
                return;
            }
            let card = self.draw.remove(0);
            self.hand.push(card);
            // 混乱:抽到的牌费用随机化
            if self.player.statuses.has(Status::Confused) {
                let i = self.hand.len() - 1;
                let base = match self.hand[i].def.cost {
                    Cost::Fixed(n) => n as i32,
                    _ => -1,
                };
                if base >= 0 {
                    let target = self.streams.floor(FloorStream::CardRandomRng).random(3) as i32;
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
        // 卡戎之灰:消耗就给全体敌人来一下
        let ashes = self.relic_sum(|fx| fx.damage_all_on_exhaust);
        if ashes > 0 {
            for i in self.alive_enemies() {
                self.damage_enemy_plain(i, ashes);
            }
            self.push_log(
                LogKind::Player,
                format!("Charon's Ashes deals {ashes} to all enemies"),
            );
            self.settle_deaths();
        }
        // 枯枝:消耗时往手里塞一张随机牌(不是本场战斗里的牌;职业池任意稀有度)
        let branch = self.relic_sum(|fx| fx.card_on_exhaust);
        if branch > 0 {
            self.add_random_class_card_to_hand();
        }
    }

    /// 能力牌打完就离开这一场战斗:既不进弃牌堆也不进消耗堆,所以
    /// 不会触发任何"消耗时"的能力与遗物(参考实现里 powers vanish).
    fn vanish_card(&mut self, card: CardInstance) {
        let label = card.label();
        self.push_log(
            LogKind::Player,
            format!("{label} is gone for the rest of the combat"),
        );
    }

    /// 玩家主动弃牌时触发的遗物(荆棘之环/结实绷带/风筝)
    fn on_manual_discard(&mut self) {
        let tingsha = self.relic_sum(|fx| fx.damage_random_on_discard);
        if tingsha > 0 {
            if let Some(t) = self.pick_random_alive() {
                self.damage_enemy_plain(t, tingsha);
                self.push_log(
                    LogKind::Player,
                    format!("Tingsha deals {tingsha} to a random enemy"),
                );
                self.settle_deaths();
            }
        }
        let bandages = self.relic_sum(|fx| fx.block_on_discard);
        if bandages > 0 {
            self.gain_block(bandages, false, false);
        }
        let kite = self.relic_sum(|fx| fx.gain_energy_first_discard_per_turn);
        if kite > 0 && !self.rs.kite_used {
            self.rs.kite_used = true;
            self.energy += kite;
            self.push_log(LogKind::Player, format!("Hovering Kite: +{kite} energy"));
        }
        // 不休陀螺:弃到手里没牌也补一张
        if self.hand.is_empty() && self.relic_any(|fx| fx.draw_on_empty_hand) {
            self.draw_cards(1);
        }
    }

    /// 洗牌堆时触发(算盘给格挡,日晷每 3 次给能量)
    fn on_shuffle(&mut self) {
        let block = self.relic_sum(|fx| fx.block_on_shuffle);
        if block > 0 {
            self.gain_block(block, false, false);
        }
        let sundial = self.relic_sum(|fx| fx.energy_per_3_shuffles);
        if sundial > 0 {
            self.rs.sundial += 1;
            if self.rs.sundial >= 3 {
                self.rs.sundial = 0;
                self.energy += sundial;
                self.push_log(LogKind::Player, format!("Sundial: +{sundial} energy"));
            }
        }
    }

    /// 从本职业牌池里随机拿一张牌(任意稀有度)进手牌
    fn add_random_class_card_to_hand(&mut self) {
        if self.hand.len() >= HAND_LIMIT {
            return;
        }
        let pool = crate::core::cards::class_card_pool();
        if pool.is_empty() {
            return;
        }
        let def = *self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
        let mut inst = CardInstance::new(def);
        self.fix_new_card(&mut inst);
        let label = inst.label();
        self.hand.push(inst);
        self.push_log(LogKind::Player, format!("{label} appears"));
    }

    /// 魔法书:随机一张本职业能力牌进手牌,本回合 0 费
    fn add_random_power_to_hand(&mut self) {
        if self.hand.len() >= HAND_LIMIT {
            return;
        }
        let pool: Vec<&'static crate::core::card::CardDef> =
            crate::core::cards::class_card_pool()
                .into_iter()
                .filter(|c| c.kind == crate::core::card::CardType::Power)
                .collect();
        if pool.is_empty() {
            return;
        }
        let def = *self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
        let mut inst = CardInstance::new(def);
        inst.free_this_turn = true;
        self.fix_new_card(&mut inst);
        let label = inst.label();
        self.hand.push(inst);
        self.push_log(LogKind::Player, format!("{label} appears (free)"));
    }

    pub fn end_turn(&mut self) {
        if self.phase != Phase::PlayerTurn {
            return;
        }
        self.force_end_turn = false;
        // 下一回合要看的计数
        self.rs.cards_last_turn = self.cards_played;
        self.rs.attacks_last_turn = self.rs.attacks_this_turn;
        // 冰激凌:把没用完的能量存起来
        if self.relic_any(|fx| fx.conserve_energy) {
            self.carry_energy = self.energy.max(0);
        }
        // 诱变力量:只在第一回合有的力量,回合结束收回
        if self.turn == 1 && self.rs.strength_turn1 != 0 {
            let n = self.rs.strength_turn1;
            self.rs.strength_turn1 = 0;
            self.player.statuses.add(Status::Strength, -n);
            self.push_log(LogKind::Info, format!("Mutagenic Strength fades (-{n})"));
        }
        // 石历:第 7 回合结束来一下
        if self.turn == 7 {
            let dmg = self.relic_sum(|fx| fx.damage_all_turn7);
            if dmg > 0 {
                for i in self.alive_enemies() {
                    self.damage_enemy_plain(i, dmg);
                }
                self.push_log(
                    LogKind::Player,
                    format!("Stone Calendar deals {dmg} to all enemies"),
                );
                self.settle_deaths();
            }
        }
        // 山铜:回合结束时没有格挡就补一点(排在金属化之前)
        if self.player.block == 0 {
            let b = self.relic_sum(|fx| fx.block_if_no_block_at_end);
            if b > 0 {
                self.gain_block(b, false, false);
            }
        }
        // 斗篷扣:手牌每张牌给 1 格挡
        let clasp = self.relic_sum(|fx| fx.block_per_card_in_hand_at_end);
        if clasp > 0 && !self.hand.is_empty() {
            let n = self.hand.len() as i32 * clasp;
            self.gain_block(n, false, false);
            self.push_log(LogKind::Player, format!("Cloak Clasp: {n} Block"));
        }
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
                let d = self.player_attack_damage(dmg, i, false);
                self.damage_enemy_plain(i, d.floor() as i32);
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
        // 镀甲:回合结束获得等量格挡
        let plated = self.player.statuses.get(Status::PlatedArmor);
        if plated > 0 {
            self.gain_block(plated, false, false);
        }
        // 仪式:回合结束获得等量力量
        let ritual = self.player.statuses.get(Status::Ritual);
        if ritual > 0 {
            self.player.statuses.add(Status::Strength, ritual);
            self.push_log(
                LogKind::Info,
                format!("Ritual grants {ritual} Strength"),
            );
        }
        // 力量/敏捷药水:这一回合的力量/敏捷在回合结束时收回
        let lose_str = self.player.statuses.get(Status::LoseStrength);
        if lose_str > 0 {
            self.player.statuses.add(Status::Strength, -lose_str);
            self.player.statuses.add(Status::LoseStrength, -lose_str);
        }
        let lose_dex = self.player.statuses.get(Status::LoseDexterity);
        if lose_dex > 0 {
            self.player.statuses.add(Status::Dexterity, -lose_dex);
            self.player.statuses.add(Status::LoseDexterity, -lose_dex);
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
        let keep_hand = self.relic_any(|fx| fx.retain_hand);
        for card in hand {
            if card.is_retain() || keep_hand {
                self.hand.push(card);
            } else if card.is_ethereal() {
                self.exhaust_card(card);
            } else {
                self.discard.push(card);
            }
        }
        for mut card in eot {
            let label = card.label();
            let effects = card.on_end_turn();
            let mut ctx = PlayCtx {
                hand_size,
                ..Default::default()
            };
            let had: Vec<Status> = self
                .player
                .statuses
                .iter()
                .filter(|(s, _)| s.decays())
                .map(|(s, _)| s)
                .collect();
            self.resolve_effects(&mut card, effects, None, &mut ctx);
            // 回合末诅咒(怀疑/羞耻)挂上的减益:参考实现把它当成"怪物挂的",
            // 跳过本轮结束时的第一次递减,否则它当场就掉光、根本管不到下个回合
            for (s, _) in self.player.statuses.iter() {
                if s.decays() && !had.contains(&s) && !self.player.fresh_debuffs.contains(&s) {
                    self.player.fresh_debuffs.push(s);
                }
            }
            self.push_log(LogKind::Player, format!("{label} triggers at end of turn"));
        }
        // 悔恨/腐烂可能把玩家打死,这时不能再把回合交给敌人
        if self.phase != Phase::PlayerTurn {
            return;
        }
        // 尼尔瑞的抄本:回合结束亮出几张随机牌,挑一张洗进抽牌堆(可以跳过)
        let codex = self.relic_max(|fx| fx.end_turn_shuffle_pick);
        if codex > 0 && !self.rs.nilrys_used {
            self.rs.nilrys_used = true;
            let pool = cards::class_card_pool();
            self.offer_pick(pool, codex as usize, false, "Nilry's Codex: choose 1");
            if let Some(ch) = self.choice.as_mut() {
                ch.action = ChoiceAction::ToDrawShuffled;
            }
            if self.choice.is_some() {
                // 选完(或跳过)之后由 resume_after_choice 把回合交出去
                self.pending_end_turn = true;
                return;
            }
        }
        self.finish_end_turn();
    }

    fn enemy_turn(&mut self) {
        // 参考实现(MonsterGroup::applyPreTurnLogic)在一轮怪物行动开始前,
        // 先把每只怪的格挡统一清空(有壁垒的除外).盾卫在同一轮里发出的格挡
        // 因此能留到玩家下一个回合,而不是轮到它自己时被掀掉.
        for e in self.enemies.iter_mut() {
            if e.up() && !e.statuses.has(Status::Barricade) {
                e.block = 0;
            }
        }
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
        // 无形只护这一回合:怪物都动完就减一层(参考实现里 turnBased 的能力
        // 在回合末 tick,玩家与怪物都算).薰香的"每 6 回合 1 层"因此只管当回合
        self.player.statuses.add(Status::Intangible, -1);
        for e in self.enemies.iter_mut() {
            e.statuses.add(Status::Intangible, -1);
        }
        self.check_win();
        if self.phase != Phase::EnemyTurn {
            return;
        }
        // 一轮结束:玩家的持续减益在这儿递减(参考实现的 endRound 先 tick 玩家再 tick 怪物).
        // 怪物这一轮刚挂上的第一次跳过(参考的 justApplied),否则易伤/虚弱会少管一个回合.
        self.decay_player_debuffs_round_end();
        self.phase = Phase::PlayerTurn;
        self.start_turn(0);
    }

    /// 一轮结束时的玩家减益递减:本轮刚挂上的(怪物来源)跳过第一次
    fn decay_player_debuffs_round_end(&mut self) {
        let fresh = std::mem::take(&mut self.player.fresh_debuffs);
        self.player.statuses.decay_debuffs_except(&fresh);
        // 再生药水的"每回合回复 X 点、X 每回合减 1":参考实现把玩家身上的这条
        // 标成 turnBased,一轮结束时掉一层.怪物身上的再生(觉醒者)是常驻的,
        // 不在递减之列,所以这里只动玩家的.
        let regen = self.player.statuses.get(Status::Regenerate);
        if regen > 0 {
            self.player.statuses.add(Status::Regenerate, -1);
        }
        // 双发/复制:只在"打这张牌的那一回合"有效,回合末整条移除
        // (参考实现的 atEndOfTurn 直接 removePower,不按层数递减)
        for s in [Status::DoubleTap, Status::Duplication] {
            let n = self.player.statuses.get(s);
            if n != 0 {
                self.player.statuses.add(s, -n);
            }
        }
        // 暴怒/火焰屏障的层数是"效果数值"而不是持续回合数:
        // 参考实现里暴怒在回合末整条移除、火焰屏障在下回合开始时整条移除.
        // 这里统一在"一轮结束"(怪物已经行动完、下个玩家回合之前)清掉,
        // 火焰屏障的荆棘因此仍然覆盖整个怪物回合.
        for s in [Status::Rage, Status::FlameBarrier] {
            let n = self.player.statuses.get(s);
            if n != 0 {
                self.player.statuses.add(s, -n);
            }
        }
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
        let fx_list =
            crate::core::ascension::effects(def.id, mname, def.moves[move_idx].effects, self.asc);
        for fx in fx_list.iter() {
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
        // 自己已经离场(分裂视作逃逸、逃跑)就不用收尾了:参考实现里这类怪
        // isEscaped,rollMove 也会跳过.但"死在自己回合里"的怪(爆炸者自爆、
        // 撞荆棘撞死)不一样——参考实现里这些死亡是排队生效的,rollMove 在它
        // 生效前已经跑过,所以这里也要照常记历史、掷下一招;少掷一次会让它之后
        // 所有怪的 aiRng 掷点整体错位.
        if self.enemies[idx].up() {
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
        } else if self.enemies[idx].escaped {
            return;
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
                    // 原版只有 RitualPower 带 skipFirst(= 刚挂上时第一次回合末触发不生效):
                    // 祭礼是唯一"当回合挂上、当回合不结算"的回合末能力.
                    // Metallicize / Plated Armor / Regenerate / StrengthUp 都是每个自己回合
                    // 末按当前层数无条件结算(灯怪 Defensive Stance 当回合就给 5 格挡).
                    if t == idx && status == Status::Ritual {
                        self.enemies[t].fresh_powers.push(status);
                    }
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
                            let pos = self.streams.floor(FloorStream::CardRandomRng).below(self.draw.len() as u32 + 1) as usize;
                            self.draw.insert(pos, inst);
                        }
                        // 抽牌堆顶:不掷点,直接插到最前面
                        CardSpot::DrawTop => self.draw.insert(0, inst),
                        // 塞进牌组:御守还能挡掉这一张诅咒(原版 Implant 就是这么判的)
                        CardSpot::Deck => {
                            if inst.kind() == crate::core::card::CardType::Curse
                                && self.curse_negate > 0
                            {
                                self.curse_negate -= 1;
                                self.push_log(
                                    LogKind::Enemy,
                                    format!("Omamori negates {label}"),
                                );
                                continue;
                            }
                            self.deck_cards.push(inst)
                        }
                    }
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} puts a {label} in your deck"),
                    );
                }
            }
            EnemyFx::PlayerCardUpgraded {
                card,
                spot,
                n,
                from_turn,
            } => {
                let upgrade = match from_turn {
                    None => true,
                    Some(t) => turn >= t,
                };
                for _ in 0..n {
                    let mut inst = CardInstance::new(cards::card_def_or_panic(card));
                    self.fix_new_card(&mut inst);
                    if upgrade {
                        inst.upgrade();
                    }
                    let label = inst.label();
                    match spot {
                        CardSpot::Discard => self.discard.push(inst),
                        CardSpot::DrawShuffle => {
                            let pos = self.streams.floor(FloorStream::CardRandomRng).below(self.draw.len() as u32 + 1) as usize;
                            self.draw.insert(pos, inst);
                        }
                        // 抽牌堆顶:不掷点,直接插到最前面
                        CardSpot::DrawTop => self.draw.insert(0, inst),
                        // 塞进牌组:御守还能挡掉这一张诅咒(原版 Implant 就是这么判的)
                        CardSpot::Deck => {
                            if inst.kind() == crate::core::card::CardType::Curse
                                && self.curse_negate > 0
                            {
                                self.curse_negate -= 1;
                                self.push_log(
                                    LogKind::Enemy,
                                    format!("Omamori negates {label}"),
                                );
                                continue;
                            }
                            self.deck_cards.push(inst)
                        }
                    }
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} puts a {label} in your deck"),
                    );
                }
            }
            EnemyFx::UpgradePlayerBurns => {
                // 参考实现:把玩家各堆里的灼伤就地升级(已经升过的不动)
                for pile in [
                    &mut self.hand,
                    &mut self.draw,
                    &mut self.discard,
                    &mut self.exhaust,
                    &mut self.deck_cards,
                ] {
                    for c in pile.iter_mut() {
                        if c.def.id == "burn" {
                            c.upgrade();
                        }
                    }
                }
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} upgrades every Burn ({mname})"),
                );
            }
            EnemyFx::ParityCoin { num, den, turn: at } => {
                if turn == at {
                    let _ = self
                        .streams
                        .floor(FloorStream::AiRng)
                        .random_bool_chance(num as f32 / den.max(1) as f32);
                }
            }
            EnemyFx::ParityRand { n } => {
                let _ = self.streams.floor(FloorStream::AiRng).random(n);
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
                    let i = self.streams.floor(FloorStream::AiRng).range_inclusive(0, pool.len() as i32 - 1) as usize;
                    let id = pool[i];
                    self.summon_one(id, slot, name);
                }
            }
            EnemyFx::DrawReduction { n } => {
                // 走"怪物给玩家挂减益"那条路:神器照样顶掉,且本轮结束不递减
                // (参考实现 applyPower 的 justApplied),否则下回合根本少抽不到牌.
                self.add_player_status_from_enemy(Status::DrawReduction, n);
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
            EnemyFx::MarkImplantUsed => {
                self.enemies[idx].state.implant_used = true;
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
        // 玩家的荆棘反伤(遗物 + 液态青铜给的荆棘 + 火焰屏障)
        let thorns = self.relic_thorns
            + self.player.statuses.get(Status::Thorns)
            + self.player.statuses.get(Status::FlameBarrier);
        if thorns > 0 {
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
                // 参考实现/原版(盾卫 Protect):从"除自己外的存活同伴"里随机挑一个,
                // 只有单挑时才落到自己头上;掷点走 aiRng,不是 cardRandomRng.
                let others: Vec<usize> = (0..self.enemies.len())
                    .filter(|i| *i != idx && self.enemies[*i].alive())
                    .collect();
                if others.is_empty() {
                    vec![idx]
                } else {
                    let chosen =
                        others[self.streams.floor(FloorStream::AiRng).below(others.len() as u32) as usize];
                    vec![chosen]
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
        let pick = self.streams.floor(FloorStream::CardRandomRng).below(pile.len() as u32) as usize;
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
    /// a 占自己那一格,b 放在 a 之后第 b_offset 格(大史莱姆紧挨着;史莱姆首领空一格)
    fn enemy_split(&mut self, idx: usize, name: &str) {
        let Special::Split { a, b, b_offset } = self.enemies[idx].def.special else {
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
        self.spawn_enemy_at(b, Some(hp), slot + b_offset as usize);
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
        let asc = self.asc;
        let hp = hp.unwrap_or_else(|| {
            let (lo, hi) = crate::core::ascension::hp_range(def, asc);
            self.streams.floor(FloorStream::MonsterHpRng).range_inclusive(lo, hi)
        });
        let mut statuses = Statuses::new();
        for (s, n) in def.innate {
            let n = crate::core::ascension::innate_amount(def.id, *s, *n, asc);
            if n == 0 {
                statuses.mark(*s);
            } else {
                statuses.add(*s, n);
            }
        }
        for (s, n) in crate::core::ascension::bonus_innate(def.id, asc) {
            statuses.add(*s, *n);
        }
        let mut state = EnemyState::default();
        let block = def.start_block;
        {
            let mut ctx = crate::core::enemy::SpawnCtx {
                rng: self.streams.floor(FloorStream::MonsterHpRng),
                statuses: &mut statuses,
                state: &mut state,
                asc,
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
            fresh_powers: Vec::new(),
            escaped: false,
            slot,
            uid: self.next_uid,
            asc,
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
        // 只有祭礼带"刚挂上当回合不结算"(原版 RitualPower 的 skipFirst);其余回合末
        // 能力(金属化/板甲/再生/力量渐增)都在自己回合末按当前层数无条件结算.
        let fresh = std::mem::take(&mut self.enemies[idx].fresh_powers);
        // 中毒:每回合掉等量生命(不吃格挡),再减一层
        let poison = self.enemies[idx].statuses.get(Status::Poison);
        if poison > 0 {
            self.enemies[idx].hp -= poison;
            self.enemies[idx].statuses.add(Status::Poison, -1);
            self.push_log(
                LogKind::Enemy,
                format!("{name} suffers {poison} poison"),
            );
            if self.enemies[idx].hp <= 0 {
                self.enemies[idx].hp = 0;
            }
            self.settle_deaths();
        }
        let plate_block = self.enemies[idx].statuses.get(Status::Metallicize)
            + self.enemies[idx].statuses.get(Status::PlatedArmor);
        if plate_block > 0 {
            self.enemies[idx].block += plate_block;
            self.push_log(
                LogKind::Enemy,
                format!("{name} gains {plate_block} Block"),
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
        if ritual > 0 && !fresh.contains(&Status::Ritual) {
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
            let base = Self::innate_amount_of(def, s, self.enemies[idx].asc);
            self.enemies[idx].statuses.set(s, base);
        }
        // 暗灵的复活倒计时:半死的那一只每个自己回合扣一格,扣到 0 就半血站起来.
        // 扣到 1 的那次由选招函数把意图换成 REINCARNATE,玩家能看到"下一回合复活".
        if self.enemies[idx].state.half_dead && self.enemies[idx].def.special == Special::Regrow {
            let left = (self.enemies[idx].state.regrow_ticks - 1).max(0);
            self.enemies[idx].state.regrow_ticks = left;
            if left == 0 {
                let half = self.enemies[idx].max_hp / 2;
                self.enemies[idx].hp = half;
                self.enemies[idx].state.half_dead = false;
                // 原版 REINCARNATE 会重新挂上 REGROW:复活后还能再半死一次,
                // 死亡触发也要重新武装,否则第二次倒下就再也起不来了.
                self.enemies[idx].state.regrow_used = false;
                self.enemies[idx].death_done = false;
                self.push_log(LogKind::Enemy, format!("{name} regrows ({half} HP)"));
            }
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
            // 参考实现的 rollMove 无论如何都会先掷一次 aiRng.random(99)(值可能不用),
            // 写死的后继(巨口的 NOM→DROOL)也一样,不掷就会让后面所有掷点错位
            self.streams.floor(FloorStream::AiRng).random(99);
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
        // 首招一旦掷出,参考实现的 moveHistory 就非空了:之后的重掷(Reactive)
        // 与常规选招都走级联,不再走"开局三选一"分支.
        self.enemies[idx].state.move_rolled = true;
    }

    /// 跑一遍某只怪的选招函数.状态是副本,跑完写回(选招里可以记账)
    fn run_script(&mut self, idx: usize, f: crate::core::enemy::PickFn) -> usize {
        let mut state = self.enemies[idx].state.clone();
        // 参考实现的 rollMove 每次选招都先消耗一次 aiRng.random(99)(哪怕这一招用不到),
        // 所以这里先掷出来交给选招函数,保证掷点流与参考逐步对齐
        let first_roll = self.streams.floor(FloorStream::AiRng).random(99) as i32;
        let asc = self.enemies[idx].asc;
        let pick = {
            let Combat {
                enemies,
                player,
                streams,
                ..
            } = self;
            let mut ctx = PickCtx {
                rng: streams,
                idx,
                all: enemies,
                player,
                state: &mut state,
                asc,
                first_roll,
                roll_consumed: false,
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
        // 敏捷与虚弱只作用在"来自卡牌"的格挡上(参考实现 calcBlock 里 fromCard 才走 modifyBlock);
        // 遗物/能力给的格挡不吃这两项
        if from_card && !doubled {
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
        // 魔法花:战斗中的治疗多 50%
        let pct = self.relic_max(|fx| fx.combat_heal_pct);
        let amount = if pct > 100 { amount * pct / 100 } else { amount };
        let before = self.player.hp;
        self.player.hp = (self.player.hp + amount).min(self.player.max_hp);
        let healed = self.player.hp - before;
        if healed > 0 {
            self.push_log(LogKind::Player, format!("you heal {healed} HP"));
            self.refresh_bloodied();
        }
    }

    /// 遗物带来的荆棘层数(与卡牌给的 Thorns 状态分开记)
    pub fn relic_thorns(&self) -> i32 {
        self.relic_thorns
    }

    /// 场上定时炸弹的剩余回合数(没有炸弹就是 0)
    pub fn bomb_turns(&self) -> i32 {
        self.bombs.iter().map(|b| b.0 as i32).max().unwrap_or(0)
    }

    /// 新造一张牌放进手牌:手牌到上限时装不下,进弃牌堆
    /// (参考实现 makeTempCard 的规则:hand overflow goes to discard)
    fn add_created_card_to_hand(&mut self, card: CardInstance) {
        if self.hand.len() >= HAND_LIMIT {
            self.discard.push(card);
        } else {
            self.hand.push(card);
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
            draw_after: false,
            copies: 1,
            free: false,
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
            // 掘出不能把自己(刚被消耗掉的那张)拿回来(wiki Update History + 参考实现)
            .filter(|(_, c)| {
                !(ch.source == ChoiceSource::Exhaust
                    && ch.action == ChoiceAction::ToHand
                    && c.def.id == "exhume")
            })
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
                // 二重身升级版一次选择、复制两份(参考实现同样是一次选择加多份)
                for _ in 0..ch.copies.max(1) {
                    self.add_created_card_to_hand(card.clone());
                }
            }
            (ChoiceSource::Hand, ChoiceAction::ToDrawTop) => {
                // 抽牌堆的顶是下标 0(从头取,插入也要插到最前)
                let mut card = self.hand.remove(idx);
                self.top_seq += 1;
                card.topped = self.top_seq;
                self.draw.insert(0, card);
            }
            (ChoiceSource::Hand, ChoiceAction::ToDrawBottom) => {
                // 抽牌堆的底是 Vec 末尾(顶在下标 0),放到末尾等轮到它才抽得到
                let mut card = self.hand.remove(idx);
                // 预谋:放到堆底之后一直 0 费,直到被打出(打出时才清掉)
                card.free_combat = true;
                self.draw.push(card);
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
                card.free_this_turn = ch.free;
                let label = card.label();
                for _ in 0..ch.copies.max(1) {
                    self.add_created_card_to_hand(card.clone());
                }
                self.push_log(LogKind::Player, format!("{label} is added to your hand"));
            }
            (ChoiceSource::Offered, ChoiceAction::ToDrawShuffled) => {
                // 尼尔瑞的抄本:挑中的那张洗进抽牌堆
                let card = ch.offered.remove(idx);
                let label = card.label();
                self.draw.push(card);
                let seed = self.streams.floor(FloorStream::ShuffleRng).random_long();
                java_shuffle(&mut self.draw, &mut JavaRandom::new(seed));
                self.on_shuffle();
                self.push_log(
                    LogKind::Player,
                    format!("{label} is shuffled into your draw pile"),
                );
            }
            (ChoiceSource::Discard, ChoiceAction::ToDrawTop) => {
                let mut card = self.discard.remove(idx);
                self.top_seq += 1;
                card.topped = self.top_seq;
                self.draw.insert(0, card);
            }
            (ChoiceSource::Discard, ChoiceAction::ToHand) => {
                if self.hand.len() < HAND_LIMIT {
                    let mut card = self.discard.remove(idx);
                    // 液态记忆:拿回来的这张本回合 0 费
                    card.free_this_turn = true;
                    let label = card.label();
                    self.hand.push(card);
                    self.push_log(LogKind::Player, format!("{label} returns to your hand"));
                }
            }
            (ChoiceSource::Hand, ChoiceAction::Discard) => {
                let card = self.hand.remove(idx);
                self.discard.push(card);
                self.on_manual_discard();
            }
            (ChoiceSource::Hand, ChoiceAction::Remove) => {
                self.hand.remove(idx);
            }
            (ChoiceSource::Hand, ChoiceAction::Upgrade) => {
                // 武装:升级手牌里选中的那张(升级完仍留在手上)
                self.hand[idx].upgrade();
            }
            _ => {}
        }
        ch.taken += 1;
        let full = ch.need != 0 && ch.taken >= ch.need;
        let empty = self.candidates_of(&ch).is_empty();
        if full || empty {
            let ch = self.choice.take().unwrap_or(ch);
            self.close_choice(ch);
        } else {
            self.choice = Some(ch);
        }
        Ok(())
    }

    /// 收尾一次选择:该弃的弃、该消耗的消耗,赌徒之酿再补抽等量张
    fn close_choice(&mut self, mut ch: Choice) {
        // 出牌时挂起的那一截效果现在接着跑(参考实现 replayTail)
        if !self.choice_tail.is_empty() {
            if let Some((card, _)) = ch.played.as_mut() {
                let tail = std::mem::take(&mut self.choice_tail);
                let mut ctx = std::mem::take(&mut self.choice_tail_ctx);
                let target = self.choice_tail_target;
                self.resolve_effects(card, &tail, target, &mut ctx);
            }
        }
        self.finish_played(ch.played.take());
        if ch.draw_after && ch.taken > 0 {
            self.draw_cards(ch.taken);
        }
        self.resume_after_choice();
    }

    /// 回合结束的尾巴:等着的选牌收完之后才把回合交给对面
    fn resume_after_choice(&mut self) {
        // 选牌本身也可能打死最后一只(比如"选一张消耗掉"触发的后续伤害)
        self.check_win();
        if self.pending_end_turn {
            self.pending_end_turn = false;
            self.finish_end_turn();
        }
    }

    /// 把回合交给对面(所有收尾都走这里,免得漏掉一条路径)
    fn finish_end_turn(&mut self) {
        if self.phase != Phase::PlayerTurn {
            return;
        }
        self.phase = Phase::EnemyTurn;
        self.enemy_turn();
    }

    /// 多选模式下玩家主动收工(比如"最多消耗 3 张",只消耗 1 张就结束)
    pub fn finish_choice(&mut self) {
        if let Some(ch) = self.choice.take() {
            self.close_choice(ch);
        }
    }

    /// 取消这次出牌:能量退回、牌回手牌;已经选出结果的几次收不回来
    pub fn cancel_choice(&mut self) {
        let Some(mut ch) = self.choice.take() else {
            return;
        };
        if ch.taken > 0 {
            // 前面几次已经生效了,这时 esc 只能当"选完了"
            self.close_choice(ch);
            return;
        }
        if let Some((card, cost)) = ch.played.take() {
            self.energy += cost;
            self.hand.push(card);
        }
        // 牌都没打出去,挂起的那一截效果也一并作废
        self.choice_tail.clear();
        self.choice_tail_target = None;
        self.resume_after_choice();
    }

    /// 出牌收尾:该消耗的消耗,该弃的弃
    fn finish_played(&mut self, played: Option<(CardInstance, i32)>) {
        let Some((card, _)) = played else {
            return;
        };
        // 能力牌落地就退场,这一条比"消耗"更彻底:弃牌堆和消耗堆都不进
        if card.kind() == crate::core::card::CardType::Power {
            self.vanish_card(card);
            return;
        }
        let relic_play = self.relic_playable(&card);
        if self.played_card_exhausts(&card, relic_play) {
            self.exhaust_card(card);
        } else {
            self.discard.push(card);
        }
    }

    /// 这张"不可打出"的牌能不能靠遗物打出去:蓝蜡烛放诅咒,医疗包放状态牌
    fn relic_playable(&self, card: &CardInstance) -> bool {
        if card.playable() {
            return false;
        }
        match card.kind() {
            crate::core::card::CardType::Curse => {
                self.relic_sum(|fx| fx.playable_curses_hp) > 0
            }
            crate::core::card::CardType::Status => self.relic_any(|fx| fx.playable_statuses),
            _ => false,
        }
    }

    /// 打完的这张牌要不要进消耗堆:奇异勺按概率把它改成进弃牌堆
    fn played_card_exhausts(&mut self, card: &CardInstance, relic_play: bool) -> bool {
        let corrupted_skill = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        let forced = card.is_exhaust()
            || card.effects().contains(&Effect::ExhaustSelf)
            || corrupted_skill
            || relic_play;
        if !forced {
            return false;
        }
        // 奇异勺:按概率把"该消耗的"改成进弃牌堆(能力牌压根不走这条路)
        let pct = self.relic_sum(|fx| fx.exhaust_to_discard_pct);
        if pct <= 0 {
            return true;
        }
        let roll = self.streams.floor(FloorStream::CardRandomRng).random(99) as i32;
        roll >= pct
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
        if self.rs.helix > 0 {
            self.rs.helix -= 1;
            self.push_log(LogKind::Info, "Fossilized Helix prevents the damage".to_string());
            return;
        }
        let amount = (amount - self.relic_sum(|fx| fx.hp_loss_reduction)).max(0);
        if amount <= 0 {
            return;
        }
        self.player.hp -= amount;
        self.shake(ShakeWho::Hero, -1, ShakeKind::Hurt, amount);
        self.note_hp_loss();
        self.on_hp_lost(amount);
        let rupt = self.player.statuses.get(Status::Rupture);
        if from_card && rupt > 0 {
            self.player.statuses.add(Status::Strength, rupt);
        }
        if self.player.hp <= 0 {
            self.resolve_player_death();
        }
    }

    /// 掉到 0 血时的收尾:仙女在瓶中 / 蜥蜴尾巴先保命,否则判负
    fn resolve_player_death(&mut self) {
        // 蜥蜴尾巴:每场一次,致命伤改为按最大生命的百分比回血
        let pct = self.relic_max(|fx| fx.death_save_pct);
        if pct > 0 && !self.rs.lizard_used {
            self.rs.lizard_used = true;
            let back = (self.player.max_hp * pct / 100).max(1);
            self.player.hp = back;
            self.push_log(
                LogKind::Info,
                format!("Lizard Tail heals you to {back} HP"),
            );
            return;
        }
        if self.fairy_save {
            self.fairy_save = false;
            self.fairy_used = true;
            let back = (self.player.max_hp * 30 / 100).max(1);
            self.player.hp = back;
            self.push_log(
                LogKind::Info,
                format!("Fairy in a Bottle heals you to {back} HP"),
            );
            return;
        }
        self.player.hp = 0;
        self.phase = Phase::Lost;
        self.push_log(LogKind::Info, "you have been defeated".to_string());
    }

    /// 敌人打玩家一次;返回(实际掉血, 被格挡量)
    fn hit_player(&mut self, damage: i32) -> (i32, i32) {
        let mut dmg = damage.max(0);
        // 无形:受到的所有伤害降为 1(幽灵在瓶中)
        if dmg > 1 && self.player.statuses.has(Status::Intangible) {
            dmg = 1;
        }
        let blocked = self.player.block.min(dmg);
        self.player.block -= blocked;
        let taken = dmg - blocked;
        if taken > 0 {
            // 化石螺壳:本场第一次掉血直接免掉
            if self.rs.helix > 0 {
                self.rs.helix -= 1;
                self.push_log(LogKind::Info, "Fossilized Helix prevents the damage".to_string());
                return (0, blocked);
            }
            // 钨钢棒:每次掉血少掉 1
            let rod = self.relic_sum(|fx| fx.hp_loss_reduction);
            let taken = (taken - rod).max(0);
            self.player.hp -= taken;
            self.shake(ShakeWho::Hero, -1, ShakeKind::Hurt, taken);
            self.note_hp_loss();
            self.on_hp_lost(taken);
            // 镀甲:挨到没格挡住的伤害就掉一层
            if self.player.statuses.get(Status::PlatedArmor) > 0 {
                self.player.statuses.add(Status::PlatedArmor, -1);
            }
        }
        if self.player.hp <= 0 {
            self.resolve_player_death();
        }
        (taken, blocked)
    }

    /// 状态牌(灼伤)在回合结束时对自己造成的可格挡伤害:
    /// 走 hit_player 的格挡/无形/减伤链,并像掉血一样触发破裂
    fn damage_self_blockable(&mut self, amount: i32) {
        let (taken, _) = self.hit_player(amount);
        if taken > 0 {
            let rupt = self.player.statuses.get(Status::Rupture);
            if rupt > 0 {
                self.player.statuses.add(Status::Strength, rupt);
            }
        }
    }

    /// 玩家攻击一次的计算:力量、虚弱、目标易伤.原版把加伤与乘伤放在同一条
    /// float 链上,末尾(连同目标侧的飞行/慢速)只向下取整一次,所以这里不取整,
    /// 把 float 交给 damage_enemy_f32(卡牌路径).
    fn player_attack_damage(&self, raw: i32, target: usize, is_attack: bool) -> f32 {
        // 活力(Akabeko 的 8 点):只加在攻击牌的伤害上,和原版的 atDamageGive 一致
        let vigor = if is_attack { self.rs.vigor } else { 0 };
        // 原版把加伤与乘伤一起按 float 连乘,末尾只向下取整一次,所以中间不能各自 floor
        let mut d = (raw + vigor) as f32;
        // 身上这些加成在原版都是 atDamageGive,按挂载顺序依次折叠:
        // 力量加一次、虚弱乘一次,谁先挂谁先算(先虚弱后力量会比反过来少 1 点).
        // 玩家自己的虚弱固定 -25%(原版 calculateCardDamage 写死 .75);纸鹤只作用于
        // 怪物侧的虚弱,见 enemy_attack_damage.
        for (s, n) in self.player.statuses.entries() {
            match s {
                Status::Strength => d += *n as f32,
                Status::Weak => d *= 0.75,
                _ => {}
            }
        }
        if self.enemies[target].statuses.has(Status::Vulnerable) {
            // 纸蛙:易伤多受 75% 伤害(默认 50%)
            let pct = self.relic_max(|fx| fx.vulnerable_damage_pct);
            d *= if pct > 0 { pct as f32 / 100.0 } else { 1.5 };
        }
        d.max(0.0)
    }

    /// 敌人攻击一次的计算
    fn enemy_attack_damage(&self, idx: usize, raw: i32) -> i32 {
        // 同样按 float 连乘,末尾只 floor 一次
        let mut d = raw as f32;
        // 纸鹤:虚弱的怪物只打出 60% 伤害(默认 75%).依据反编译 calculateDamageToPlayer
        let weak_pct = {
            let pct = self.relic_max(|fx| fx.weak_damage_pct);
            if pct > 0 {
                pct as f32 / 100.0
            } else {
                0.75
            }
        };
        // 怪物自己身上的力量/虚弱照挂载顺序折叠
        for (s, n) in self.enemies[idx].statuses.entries() {
            match s {
                Status::Strength => d += *n as f32,
                Status::Weak => d *= weak_pct,
                _ => {}
            }
        }
        // 被夹击:从背后打过来的多吃一半
        if self.player.statuses.has(Status::Surrounded) && idx != self.facing {
            d *= 1.5;
        }
        if self.player.statuses.has(Status::Vulnerable) {
            // 奇异蘑菇:自己身上的易伤只多受 25% 伤害(默认 50%)
            let pct = self.relic_max(|fx| fx.vulnerable_taken_pct);
            d *= if pct > 0 { pct as f32 / 100.0 } else { 1.5 };
        }
        let mut d = d.floor().max(0.0) as i32;
        // 鸟居:5 点以下(含)的未被格挡攻击伤害降为 1
        let torii = self.relic_max(|fx| fx.small_attack_reduce_to);
        if torii > 0 && d > 1 && d <= 5 {
            d = torii;
        }
        d.max(0)
    }

    /// 开局写死的层数(每回合重置的延展/慢速/飞行要看它).飞升会换档(如鸟的飞行)
    fn innate_amount_of(def: &'static EnemyDef, s: Status, asc: u32) -> i32 {
        let base = def
            .innate
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        crate::core::ascension::innate_amount(def.id, s, base, asc)
    }

    /// 下一招的基础伤害与命中次数(还没算力量/虚弱/易伤),按当前回合数动态算
    pub fn intent_raw_damage(&self, idx: usize) -> (i32, u8) {
        let e = &self.enemies[idx];
        let mv = &e.def.moves[e.next_move];
        let turn = e.state.turns + 1;
        let mut damage = 0;
        let mut times = 0u8;
        let fx_list = crate::core::ascension::effects(e.def.id, mv.name, mv.effects, e.asc);
        for fx in fx_list.iter() {
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
        let mv = &e.def.moves[e.next_move];
        let mut total = 0;
        for fx in crate::core::ascension::effects(e.def.id, mv.name, mv.effects, e.asc).iter() {
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

    /// 打敌人(整数入口:测试与少数按整数算好的地方).转发到 float 版.
    fn damage_enemy(&mut self, idx: usize, damage: i32) -> i32 {
        self.damage_enemy_f32(idx, damage as f32)
    }

    /// 打敌人:damage 是玩家侧算完的 float(还没过目标侧的飞行/慢速),末尾只取整一次.
    /// 卡牌打出来的是"攻击伤害",会走飞行/慢速/无形/无敌这一整套.
    fn damage_enemy_f32(&mut self, idx: usize, damage: f32) -> i32 {
        self.hit_enemy(idx, damage, true)
    }

    /// 非攻击伤害(中毒、燃烧、荆棘之类):不吃飞行/慢速这些减免
    fn damage_enemy_plain(&mut self, idx: usize, damage: i32) -> i32 {
        self.hit_enemy(idx, damage as f32, false)
    }

    /// 多段攻击:原版一张牌只算一次伤害(算完飞行/慢速/无形/靴子),之后每一段
    /// 都拿这份值去打,逐段扣格挡、逐段触发挨打钩子(飞行层数就是这么掉的).
    /// 只有随机选目标的多段(回旋镖)才逐段重算,因为它每一段可能打到不同的怪.
    fn damage_enemy_times(&mut self, idx: usize, damage: i32, is_attack: bool, times: i32) -> i32 {
        if idx >= self.enemies.len() || !self.enemies[idx].alive() {
            return 0;
        }
        let raw = self.player_attack_damage(damage, idx, is_attack);
        let dmg = self.reduce_incoming(idx, raw, is_attack);
        let mut total = 0;
        for _ in 0..times.max(1) {
            total += self.hit_enemy_final(idx, dmg, is_attack);
        }
        total
    }

    /// 受伤侧的减免:飞行减半、慢速加伤、无形压到 1.原版把飞行与慢速放在同一条
    /// float 链上(玩家侧的加/乘伤也在里面),末尾只向下取整一次,所以这里收 float.
    /// 一张牌的多段伤害共用同一份算好的值(见 damage_enemy_times).
    fn reduce_incoming(&self, idx: usize, damage: f32, is_attack: bool) -> i32 {
        let mut dmg = damage.max(0.0);
        if is_attack {
            // 飞行:受到的攻击伤害减半
            if self.enemies[idx].statuses.has(Status::Flight) {
                dmg *= 0.5;
            }
            // 慢速:这回合每打出一张牌就多吃 10%
            let slow = self.enemies[idx].statuses.get(Status::Slow);
            if slow > 0 {
                dmg *= 1.0 + 0.1 * slow as f32;
            }
        }
        let mut out = dmg.floor().max(0.0) as i32;
        // 无形:什么伤害都降到 1
        if self.enemies[idx].statuses.has(Status::Intangible) && out > 1 {
            out = 1;
        }
        out
    }

    fn hit_enemy(&mut self, idx: usize, damage: f32, is_attack: bool) -> i32 {
        let dmg = self.reduce_incoming(idx, damage, is_attack);
        self.hit_enemy_final(idx, dmg, is_attack)
    }

    /// 已经算过减免的一击:只扣格挡、掉血、走挨打触发的钩子
    fn hit_enemy_final(&mut self, idx: usize, dmg: i32, is_attack: bool) -> i32 {
        if idx >= self.enemies.len() || !self.enemies[idx].alive() {
            return 0;
        }
        let had_block = self.enemies[idx].block > 0;
        let blocked = self.enemies[idx].block.min(dmg);
        self.enemies[idx].block -= blocked;
        // 手钻:这一击把格挡打碎(打到 0)时给易伤
        let broke_block = had_block && blocked > 0 && self.enemies[idx].block == 0;
        let mut taken = dmg - blocked;
        // 靴子 The Boot:未被格挡的攻击伤害只剩 1..4 点时提到 5.依据反编译
        // (sts_lightspeed Monster::attackedUnblockedHelper),这一步排在格挡与目标侧的
        // 飞行/慢速/无形之后、掉血之前,所以 4 点打在无形怪身上也被抬到 5.
        if is_attack && taken > 0 {
            let boost = self.relic_max(|fx| fx.small_attack_boost_to);
            if taken < boost {
                taken = boost;
            }
        }
        // 无敌:一回合之内最多再掉这么多
        let inv = self.enemies[idx].statuses.get(Status::Invincible);
        if inv > 0 {
            let left = (inv - self.enemies[idx].state.taken_this_turn).max(0);
            taken = taken.min(left);
        }
        if taken > 0 {
            // 过量伤害不把血量压到 0 以下(参考实现结算时夹在 0)
            self.enemies[idx].hp = (self.enemies[idx].hp - taken).max(0);
            self.enemies[idx].state.taken_this_turn += taken;
            self.damage_dealt += taken;
            self.shake(ShakeWho::Enemy(idx), 1, ShakeKind::Hurt, taken);
            self.on_enemy_hp_lost(idx, taken, is_attack);
        }
        if is_attack {
            self.on_enemy_attacked(idx, taken);
        }
        if is_attack && broke_block {
            let n = self.relic_sum(|fx| fx.vulnerable_on_block_break);
            if n > 0 {
                self.push_log(LogKind::Player, format!("Hand Drill applies {n} Vulnerable"));
                self.add_enemy_status(idx, Status::Vulnerable, n);
            }
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
            // 移形换影:掉多少血就等量少力量,自己回合结束再补回来
            // (和黑暗镣铐走同一条路:当下真扣,回合末按 temp_strength 回补)
            if self.enemies[idx].statuses.holds(Status::Shifting) {
                self.enemies[idx].statuses.add(Status::Strength, -taken);
                self.enemies[idx].temp_strength += taken;
            }
        }
        self.check_hp_thresholds(idx, taken);
    }

    /// 命中就触发、掉不掉血都算的(荆棘、狂怒)
    fn on_enemy_attacked(&mut self, idx: usize, taken: i32) {
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
        // 扭动巨物:挨到实打实的伤害才换招(全挡下来的不换)
        if taken > 0
            && self.enemies[idx].def.special == Special::Reactive
            && self.phase == Phase::PlayerTurn
        {
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
        let done_before = self.enemies.iter().filter(|e| e.death_done).count();
        for i in 0..self.enemies.len() {
            if self.enemies[i].dead() && !self.enemies[i].death_done {
                self.handle_death(i);
            }
        }
        // 小鬼号角:这一轮真死了几只就给能量并抽牌
        let died = self
            .enemies
            .iter()
            .filter(|e| e.death_done)
            .count()
            .saturating_sub(done_before) as i32;
        if died > 0 {
            let horn = self.relic_sum(|fx| fx.energy_on_kill);
            if horn > 0 {
                let gain = horn * died;
                self.energy += gain;
                self.push_log(LogKind::Player, format!("Gremlin Horn: +{gain} energy"));
            }
            let draw = self.relic_sum(|fx| fx.draw_on_kill);
            if draw > 0 {
                self.draw_cards((draw * died) as usize);
            }
        }
    }

    /// 一只怪死了:先看它的独有机制,再走通用的死亡触发
    fn handle_death(&mut self, i: usize) {
        let def = self.enemies[i].def;
        // 觉醒者:第一阶段"死"掉只是半死,躺着等复活
        if def.special == Special::Rebirth && !self.enemies[i].state.phase2 {
            self.enemies[i].state.half_dead = true;
            // 飞升 9+ 二阶段血量 320(参考实现 REBIRTH 把 maxHp 设成 320/300)
            self.enemies[i].max_hp = crate::core::ascension::awakened_phase2_hp(self.asc);
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
                // 死在玩家回合:它这一轮还没行动,从下一轮开始数两轮复活;
                // 死在自己回合(荆棘之类):这一轮已经算过,要多等一轮
                e.state.regrow_ticks = if self.phase == Phase::EnemyTurn { 3 } else { 2 };
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
        // 参考实现:战斗一旦分出胜负,动作队列当场清空,挂起的选牌不再交付
        // (decided fights leave no pending picks),所以这里也把待定的选牌丢掉.
        self.choice = None;
        self.pending_end_turn = false;
        self.push_log(LogKind::Info, "victory".to_string());
    }

    // ---- 打牌 ----

    /// 这张牌能不能打;不能则给出原因
    pub fn playable(&self, hand_idx: usize) -> Result<(), &'static str> {
        if self.phase != Phase::PlayerTurn {
            return Err("not your turn");
        }
        // 时间扭曲:第 12 张牌已经把这一回合掐掉了,之后不能再出牌
        // (参考实现的 queueEndTurn 立刻排入 endPlayerTurn)
        if self.force_end_turn {
            return Err("your turn has already ended");
        }
        let Some(card) = self.hand.get(hand_idx) else {
            return Err("no such card");
        };
        // 蓝蜡烛/医疗包让不可打出的诅咒与状态牌可以打出(费用按 0 算)
        let relic_play = self.relic_playable(card);
        if !card.playable() && !relic_play {
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
        // 维可夹克:一回合最多打 6 张
        let cap = self.relic_max(|fx| fx.card_play_cap);
        if cap > 0 && self.cards_played >= cap {
            return Err("Velvet Choker: no more cards this turn");
        }
        // 腐化:技能都是 0 费,所以这里不能按原价拦
        let corrupted = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        if !corrupted && !relic_play && card.cost_value(self.energy) > self.energy {
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
        let corrupted_skill = card.kind() == crate::core::card::CardType::Skill
            && self.player.statuses.has(Status::Corruption);
        let relic_play = self.relic_playable(&card);
        let cost = if corrupted_skill || relic_play {
            0
        } else {
            card.cost_value(self.energy)
        };
        // 0 费只对这一回合的那一次打出有效,出手后立刻失效.这一步必须排在"算完费用"之后 ——
        // 费用是按 free_this_turn 归零的(见 fixed_cost),先清掉就等于这项优惠从来没生效过.
        card.free_this_turn = false;
        self.energy -= cost.min(self.energy);
        let is_x = card.cost() == Cost::X;
        // 化学 X:X 费牌的 X 额外加 2(参考实现:效果按 X+2 结算,能量照常全花)
        let x_bonus = self.relic_sum(|fx| fx.x_cost_bonus);
        let mut ctx = PlayCtx {
            x: if is_x { cost + x_bonus } else { 0 },
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
            // 随机目标牌(回旋镖)不在这里掷点:原版这张牌的牌面目标是"所有敌人",
            // 每一次命中才各自掷一只怪(见 Effect::DamageRandom),提前掷一次会把
            // cardRandomRng 的计数器顶偏,后面所有随机都跟着错位
            Target::Random => None,
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
        self.snapshot_sharp_hide();
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
        // 复制:这一张再打一次(任意类型)
        let dup = self.player.statuses.get(Status::Duplication);
        if dup > 0 {
            self.player.statuses.add(Status::Duplication, -1);
            self.resolve(&mut card, chosen, &mut ctx);
        }
        // 死灵之书:本回合第一张 2 费以上的攻击再打一次
        let necro = self.relic_any(|fx| fx.double_first_big_attack)
            && !self.rs.necro_used
            && card.kind() == crate::core::card::CardType::Attack
            && matches!(card.cost(), Cost::Fixed(c) if c >= 2);
        if necro {
            self.rs.necro_used = true;
            self.push_log(LogKind::Player, "Necronomicon: the attack plays twice".to_string());
            self.resolve(&mut card, chosen, &mut ctx);
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
        // 活力(Akabeko):下一张攻击牌打出后立刻用掉(参考实现挂在 VIGOR 的
        // onAfterCardPlayed 上;复读的那几下也算在里面,所以放在这里清)
        if card.kind() == crate::core::card::CardType::Attack {
            self.rs.vigor = 0;
        }
        // 有选牌待定:牌和花的能量先存着,等选完(choose)或取消(cancel)再收尾
        if self.choice.is_some() {
            if let Some(ch) = self.choice.as_mut() {
                ch.played = Some((card, cost));
            }
            // 这一刀如果已经砍死最后一只,后面的选牌就不该再给出去(见 check_win)
            self.check_win();
            return Ok(());
        }
        // 蓝蜡烛:打出诅咒要掉血(掉死了这张牌也照样落地)
        if relic_play && card.kind() == crate::core::card::CardType::Curse {
            let hp = self.relic_sum(|fx| fx.playable_curses_hp);
            if hp > 0 {
                self.push_log(LogKind::Player, format!("Blue Candle costs {hp} HP"));
                self.lose_hp_player(hp, false);
            }
        }
        // 结算完后决定去处(与 finish_played 走同一条路:能力牌退场、该消耗的消耗)
        self.finish_played(Some((card, cost)));
        self.check_win();
        Ok(())
    }

    fn pick_random_alive(&mut self) -> Option<usize> {
        let alive = self.alive_enemies();
        if alive.is_empty() {
            return None;
        }
        Some(alive[self.streams.floor(FloorStream::CardRandomRng).below(alive.len() as u32) as usize])
    }

    fn resolve(&mut self, card: &mut CardInstance, target: Option<usize>, ctx: &mut PlayCtx) {
        let effects = card.effects();
        self.resolve_effects(card, effects, target, ctx);
    }

    /// 逐条结算一份效果列表:打出时传 effects(),抽到/回合结束时传 on_draw / on_end_turn
    fn resolve_effects(
        &mut self,
        card: &mut CardInstance,
        effects: &[Effect],
        target: Option<usize>,
        ctx: &mut PlayCtx,
    ) {
        let is_strike = card.is_strike();
        let card_bonus = card.bonus;
        // 遗物给这张牌的加伤:打击木偶(名字带 Strike)+ 腕刃(0 费攻击)
        let is_attack = card.kind() == crate::core::card::CardType::Attack;
        let mut relic_add = 0;
        if is_attack {
            if is_strike {
                relic_add += self.relic_sum(|fx| fx.strike_damage_bonus);
            }
            if matches!(card.cost(), crate::core::card::Cost::Fixed(0)) {
                relic_add += self.relic_sum(|fx| fx.zero_cost_attack_bonus);
            }
        }
        // 笔尖:第 10 张攻击翻倍
        let pen_nib_double = is_attack && self.rs.pen_nib == 9 && self.relic_any(|fx| fx.double_damage_per_10_attacks);
        let _ = pen_nib_double;
        for (i, e) in effects.iter().enumerate() {
            // 上一条效果挂起了选牌(消耗/放顶那类):后面的效果先原样存起来,等选完
            // 由 close_choice 接着跑.参考实现把动作队列的尾巴快照进 resumeArgs.__tail
            // 再 replayTail,顺序与掷点位置才对得上
            if self.choice.is_some() {
                self.choice_tail = effects[i..].to_vec();
                self.choice_tail_target = target;
                self.choice_tail_ctx = PlayCtx {
                    x: ctx.x,
                    exhausted: ctx.exhausted,
                    unblocked: ctx.unblocked,
                    hand_size: ctx.hand_size,
                };
                break;
            }
            match *e {
                Effect::Damage { amount, times } => {
                    if let Some(t) = target {
                        let mut raw = amount + relic_add;
                        if pen_nib_double {
                            raw *= 2;
                        }
                        ctx.unblocked += self.damage_enemy_times(t, raw, is_attack, times.max(1) as i32);
                    }
                }
                Effect::DamageAll { amount, times } => {
                    for _ in 0..times.max(1) {
                        for t in self.alive_enemies() {
                            let mut raw = amount + relic_add;
                            if pen_nib_double {
                                raw *= 2;
                            }
                            let d = self.player_attack_damage(raw, t, is_attack);
                            ctx.unblocked += self.damage_enemy_f32(t, d);
                        }
                    }
                }
                Effect::DamageRandom { amount, times } => {
                    for _ in 0..times.max(1) {
                        let Some(t) = self.pick_random_alive() else {
                            break;
                        };
                        let d = self.player_attack_damage(amount, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                    }
                }
                Effect::DamageEqualBlock => {
                    if let Some(t) = target {
                        let raw = self.player.block;
                        let d = self.player_attack_damage(raw, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                    }
                }
                Effect::DamageWithBonus { amount, times } => {
                    if let Some(t) = target {
                        let raw = amount + card_bonus;
                        for _ in 0..times.max(1) {
                            if self.enemies[t].dead() {
                                break;
                            }
                            let d = self.player_attack_damage(raw, t, is_attack);
                            ctx.unblocked += self.damage_enemy_f32(t, d);
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
                        let d = self.player_attack_damage(base + per * n, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                    }
                }                Effect::DamagePerExhausted { per } => {
                    if let Some(t) = target {
                        // 原版恶魔之焰是"每消耗一张牌打一段 7 点",段数 = 手牌数 - 自己;
                        // 每段用同一份算好的伤害(飞行/无形按段结算层数,但减伤只算一次)
                        if ctx.exhausted > 0 {
                            ctx.unblocked +=
                                self.damage_enemy_times(t, per, is_attack, ctx.exhausted);
                        }
                    }
                }
                Effect::DamageAllX { per } => {
                    let raw = per * ctx.x;
                    if raw > 0 {
                        for t in self.alive_enemies() {
                            let d = self.player_attack_damage(raw, t, is_attack);
                            ctx.unblocked += self.damage_enemy_f32(t, d);
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
                        let d = self.player_attack_damage(amount, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
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
                            let d = self.player_attack_damage(amount, t, is_attack);
                            ctx.unblocked += self.damage_enemy_f32(t, d);
                        }
                        // 击杀随从不算数:只有非随从(含精英与首领)才给最大生命
                        if before > 0 && self.enemies[t].dead() && !self.enemies[t].is_minion() {
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
                        let vigor = if is_attack { self.rs.vigor } else { 0 };
                        let raw = amount + self.player.statuses.get(Status::Strength) * mult + vigor;
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
                        let d = self.player_attack_damage(amount, t, is_attack);
                        total += self.damage_enemy_f32(t, d);
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
                    // 原版是一张牌一发 GainBlockAction:每发各过一次敏捷/虚弱(取整按张算,
                    // 不是先乘张数再取整一次),Juggernaut 这类"获得格挡"的钩子也每发一次
                    for _ in 0..ctx.exhausted.max(0) {
                        self.gain_block(per, false, true);
                    }
                }
                Effect::DoubleBlock => {
                    let b = self.player.block;
                    self.gain_block(b, true, true);
                }                Effect::LoseHp { amount } => {
                    self.lose_hp_player(amount, true);
                }
                Effect::DamageSelf { amount } => {
                    self.damage_self_blockable(amount);
                }
                Effect::LoseHpPerHandCard => {
                    let n = ctx.hand_size.max(0);
                    self.lose_hp_player(n, true);
                }
                Effect::CopySelfToDrawTop => {
                    // 抽牌堆的顶是下标 0
                    let mut copy = CardInstance::new(card.def);
                    copy.upgraded = card.upgraded;
                    copy.plus = card.plus;
                    self.top_seq += 1;
                    copy.topped = self.top_seq;
                    let label = copy.label();
                    self.draw.insert(0, copy);
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
                    self.add_self_status(status, n);
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
                        let i = self.streams.floor(FloorStream::CardRandomRng).below(self.hand.len() as u32)
                            as usize;
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
                        let d = self.player_attack_damage(damage, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                    }
                }
                Effect::ExhaustSelf => {
                    // 去处由 play_card 统一处理
                }                Effect::AddCardToDraw { id, n } => {
                    // 卡面是"洗进抽牌堆":位置用 cardRandomRng 掷(参考实现 moveCard random)
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        let pos = self.streams.floor(FloorStream::CardRandomRng).below(self.draw.len() as u32 + 1) as usize;
                        self.draw.insert(pos, inst);
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
                Effect::CopyFromHand { copies } => {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Copy,
                        ChoiceFilter::AttackOrPower,
                        1,
                        "copy an Attack or Power card",
                    );
                    // 一次选择、复制 copies 份(升级版两份)
                    if let Some(ch) = self.choice.as_mut() {
                        ch.copies = copies as usize;
                    }
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
                    // 本职业攻击牌池(不含基础/特殊/无色):原版的
                    // returnTrulyRandomCardInCombat(ATTACK) 就是从本职业牌池里抽.
                    let mut pool = cards::class_pool_of_kind(crate::core::card::CardType::Attack);
                    pool.sort_by_key(|c| c.id);
                    if !pool.is_empty() {
                        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        inst.free_this_turn = true;
                        let label = inst.label();
                        self.add_created_card_to_hand(inst);
                        self.push_log(LogKind::Player, format!("{label} appears (costs 0)"));
                    }
                }
                Effect::PlayTopOfDraw => {
                    self.play_top_of_draw(true, "Havoc");
                }
                Effect::AddCardToHand { id, n } => {
                    let def = cards::card_def_or_panic(id);
                    for _ in 0..n {
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        self.add_created_card_to_hand(inst);
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
                Effect::AddSelfToDiscard { n } => {
                    // 副本照抄这张牌自己的升级数与战斗内加成(愤怒+ 塞的是 愤怒+)
                    for _ in 0..n {
                        let mut copy = CardInstance::new(card.def);
                        copy.upgraded = card.upgraded;
                        copy.plus = card.plus;
                        copy.bonus = card.bonus;
                        self.fix_new_card(&mut copy);
                        self.discard.push(copy);
                    }
                }
                Effect::UpgradeChosenInHand => {
                    if self.hand.iter().any(|c| c.can_upgrade()) {
                        self.begin_choice(
                            ChoiceSource::Hand,
                            ChoiceAction::Upgrade,
                            ChoiceFilter::Upgradeable,
                            1,
                            "Armaments: upgrade a card",
                        );
                    }
                }
                Effect::UpgradeAllInHand => {
                    let mut n = 0;
                    for c in self.hand.iter_mut() {
                        if c.can_upgrade() {
                            c.upgrade();
                            n += 1;
                        }
                    }
                    if n > 0 {
                        self.push_log(
                            LogKind::Info,
                            format!("{n} cards in your hand are upgraded"),
                        );
                    }
                }
                Effect::Heal { amount } => {
                    self.heal_player(amount);
                }
                Effect::DamagePerDrawPile { per } => {
                    if let Some(t) = target {
                        let raw = per * self.draw.len() as i32;
                        let d = self.player_attack_damage(raw, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
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
                            let d = self.player_attack_damage(amount, t, is_attack);
                            ctx.unblocked += self.damage_enemy_f32(t, d);
                        }
                        // 击杀随从不算数:只有非随从才掉金币
                        if before > 0 && self.enemies[t].dead() && !self.enemies[t].is_minion() {
                            self.gold_gained += gold;
                            self.push_log(
                                LogKind::Player,
                                format!("you loot {gold} gold"),
                            );
                        }
                    }
                }
                Effect::DamageAndKillBonusSelf { amount, bonus } => {
                    if let Some(t) = target {
                        let before = self.enemies[t].hp;
                        let d = self.player_attack_damage(amount + card_bonus, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                        // 击杀随从不算数:只有非随从(含精英与首领)才让这张牌成长
                        if before > 0 && self.enemies[t].dead() && !self.enemies[t].is_minion() {
                            card.bonus += bonus;
                            // 写回牌组原件,让成长在战斗之间保留
                            if let Some(mi) = card.master_idx {
                                self.deck_growth.push((mi, bonus));
                            }
                            self.push_log(
                                LogKind::Player,
                                format!("the dagger grows: +{bonus} damage"),
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
                        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
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
                    java_shuffle(&mut self.draw, &mut JavaRandom::new(self.streams.floor(FloorStream::ShuffleRng).random_long()));
                }
                Effect::ShuffleDiscardIntoDraw => {
                    // 参考实现的 reshuffleDiscardIntoDraw 无条件洗一次:哪怕弃牌堆是空的,
                    // 也照样从 shuffleRng 取一个 long 并洗整个抽牌堆.这里的掷点消耗
                    // 必须一致,不然之后的随机(洗牌、随机目标)会整体错位.
                    let n = self.discard.len();
                    self.draw.append(&mut self.discard);
                    java_shuffle(&mut self.draw, &mut JavaRandom::new(self.streams.floor(FloorStream::ShuffleRng).random_long()));
                    self.push_log(
                        LogKind::Info,
                        format!("shuffled {n} cards into the draw pile"),
                    );
                }
                Effect::FreeRandomInHand => {
                    // 疯狂:把手牌里随机一张"仍要花费用"的牌降到 0.
                    // 原版 MadnessAction 会反复掷点直到抽到 eligible 的牌
                    // (0 费与 X 费都重掷),整手牌都没有能降的就一次也不掷.
                    let eligible = |c: &CardInstance| matches!(c.fixed_cost(), Some(n) if n > 0);
                    if self.hand.iter().any(&eligible) {
                        let mut guard = 0;
                        loop {
                            guard += 1;
                            let pick = self.streams.floor(FloorStream::CardRandomRng).below(self.hand.len() as u32) as usize;
                            if eligible(&self.hand[pick]) || guard >= 1000 {
                                self.hand[pick].free_combat = true;
                                let name = self.hand[pick].label();
                                self.push_log(
                                    LogKind::Info,
                                    format!("{name} costs 0 for the rest of combat"),
                                );
                                break;
                            }
                        }
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
                            // 原版直接把力量压成负数(力量允许低于 0),
                            // 回合结束再按扣掉的量补回来.
                            let loss = n;
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
                Effect::OfferRandomCardsFromClass { n } => {
                    let pool = cards::class_card_pool();
                    if pool.is_empty() {
                        continue;
                    }
                    let mut offered: Vec<CardInstance> = Vec::new();
                    for _ in 0..n {
                        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
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
                        // 发现:挑中的那张本回合 0 费
                        ch.free = true;
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
                        let pick = cands[self.streams.floor(FloorStream::CardRandomRng).below(cands.len() as u32) as usize];
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
        // 神器:减益,以及"可以压到负数的增益"的负数应用(Disarm 的 -力量按减益算),
        // 都先拿一层顶掉这次施加(参考实现 applyPower 的同一条规则).
        let debuff_application =
            (status.is_debuff() && n > 0) || (status.can_go_negative() && n < 0);
        if debuff_application && self.enemies[idx].statuses.has(Status::Artifact) {
            self.enemies[idx].statuses.add(Status::Artifact, -1);
            let name = self.enemies[idx].name.clone();
            self.push_log(
                LogKind::Enemy,
                format!("{name}'s Artifact blocks {}", status.name()),
            );
            return;
        }
        self.enemies[idx].statuses.add(status, n);
        // 冠军腰带:玩家给敌人上易伤时附带虚弱
        if status == Status::Vulnerable && n > 0 {
            let weak = self.relic_sum(|fx| fx.weak_on_vulnerable);
            if weak > 0 {
                self.add_enemy_status(idx, Status::Weak, weak);
            }
        }
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
        // 姜:免疫虚弱;萝卜:免疫脆弱
        if n > 0 && status == Status::Weak && self.relic_any(|fx| fx.immune_weak) {
            return;
        }
        if n > 0 && status == Status::Frail && self.relic_any(|fx| fx.immune_frail) {
            return;
        }
        if n > 0 && status.is_debuff() && self.player.statuses.has(Status::Artifact) {
            self.player.statuses.add(Status::Artifact, -1);
            self.push_log(
                LogKind::Player,
                format!("Artifact blocks {}", status.name()),
            );
            return;
        }
        // 同上:压到负数的力量/敏捷也算减益,玩家身上的神器一样顶掉
        if n < 0 && status.can_go_negative() && self.player.statuses.has(Status::Artifact) {
            self.player.statuses.add(Status::Artifact, -1);
            self.push_log(
                LogKind::Player,
                format!("Artifact blocks {}", status.name()),
            );
            return;
        }
        // 怪物给玩家挂上的持续状态:本轮结束时不递减(参考实现 applyPower 的 justApplied:
        // 只要来源是怪物、目标玩家、且是持续型就打标,不管是出手还是亡语).
        // 下一次递减才算第一次,否则易伤/虚弱会少管一个回合.
        if n > 0 && status.decays() && !self.player.statuses.holds(status) {
            self.player.fresh_debuffs.push(status);
        }
        self.player.statuses.add(status, n);
    }

    /// 牌/药水给自己挂状态.减益一样先被神器顶掉(原版所有 ApplyPower 都走这条路),
    /// 但不算"怪物来源",本轮结束照常递减(悔恨/羞耻挂的虚弱不会多管一回合).
    fn add_self_status(&mut self, status: Status, n: i32) {
        if n > 0 && status == Status::Weak && self.relic_any(|fx| fx.immune_weak) {
            return;
        }
        if n > 0 && status == Status::Frail && self.relic_any(|fx| fx.immune_frail) {
            return;
        }
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
        // 神圣树皮:药水数值翻倍
        let fx = if self.relic_sum(|f| f.potion_potency_pct) > 0 {
            def.fx.doubled()
        } else {
            def.fx
        };
        // 原版 Surrounded(wiki:"Use targeting cards or potions to change your
        // orientation"):指向敌人的药水命中的那只就是新朝向,夹击的 1.5 倍随之换边
        // (判定见 enemy_attack_damage).不指向敌人的药水不动朝向.
        if def.target.needs_enemy() {
            if let Some(t) = self.potion_target(target) {
                self.facing = t;
            }
        }
        match fx {
            PotionFx::Damage { amount } => {
                if let Some(t) = self.potion_target(target) {
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
            PotionFx::Weak { n } => {
                if let Some(t) = self.potion_target(target) {
                    self.add_enemy_status(t, Status::Weak, n);
                }
            }
            PotionFx::Vulnerable { n } => {
                if let Some(t) = self.potion_target(target) {
                    self.add_enemy_status(t, Status::Vulnerable, n);
                }
            }
            PotionFx::Poison { n } => {
                if let Some(t) = self.potion_target(target) {
                    self.add_enemy_status(t, Status::Poison, n);
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
            PotionFx::DrawAndRandomizeCosts { n } => {
                self.draw_cards(n as usize);
                self.randomize_hand_costs();
            }
            PotionFx::HealPercent { pct } => {
                let amount = self.player.max_hp * pct / 100;
                self.heal_player(amount);
            }
            PotionFx::MaxHp { n } => {
                self.player.max_hp += n;
                self.player.hp += n;
            }
            PotionFx::Status { status, n } => {
                self.player.statuses.add(status, n);
            }
            PotionFx::TempStatus { status, lose, n } => {
                self.player.statuses.add(status, n);
                self.player.statuses.add(lose, n);
            }
            PotionFx::Discovery { pool, n } => {
                let cards = self.discovery_pool(pool);
                self.offer_pick(cards, 3, true, def.name);
                // 神圣树皮把份数翻倍:挑中的那张按 n 份进手
                if let Some(ch) = self.choice.as_mut() {
                    ch.copies = n.max(1) as usize;
                }
            }
            PotionFx::ExhaustHand => {
                if !self.hand.is_empty() {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Exhaust,
                        ChoiceFilter::Any,
                        0,
                        &format!("{}: exhaust any number", def.name),
                    );
                }
            }
            PotionFx::DiscardHandThenDraw => {
                if !self.hand.is_empty() {
                    self.begin_choice(
                        ChoiceSource::Hand,
                        ChoiceAction::Discard,
                        ChoiceFilter::Any,
                        0,
                        &format!("{}: discard any number", def.name),
                    );
                    if let Some(ch) = self.choice.as_mut() {
                        ch.draw_after = true;
                    }
                }
            }
            PotionFx::ReturnFromDiscard { n } => {
                if !self.discard.is_empty() {
                    let need = (n as usize).min(self.discard.len());
                    self.begin_choice(
                        ChoiceSource::Discard,
                        ChoiceAction::ToHand,
                        ChoiceFilter::Any,
                        need,
                        &format!("{}: take a card back", def.name),
                    );
                }
            }
            PotionFx::UpgradeHand => {
                let mut n = 0;
                for c in self.hand.iter_mut() {
                    if c.can_upgrade() {
                        c.upgrade();
                        n += 1;
                    }
                }
                if n > 0 {
                    self.push_log(
                        LogKind::Info,
                        format!("{n} cards are upgraded for this combat"),
                    );
                }
            }
            PotionFx::PlayTopCards { n } => {
                for _ in 0..n {
                    self.play_top_of_draw(false, "Distilled Chaos");
                }
            }
            PotionFx::AddCardToHand { id, n, upgraded } => {
                if let Some(card_def) = cards::card_def(id) {
                    for _ in 0..n {
                        if self.hand.len() >= HAND_LIMIT {
                            break;
                        }
                        let mut inst = CardInstance::new(card_def);
                        if upgraded {
                            inst.upgrade();
                        }
                        self.fix_new_card(&mut inst);
                        self.hand.push(inst);
                    }
                }
            }
            // 这两瓶由 run 层结算:脱战要换界面,填药水要动跑图状态
            PotionFx::Escape | PotionFx::FillPotionSlots | PotionFx::Nothing => {}
        }
        // 玩具鸟:每次喝药回血
        let bird = self.relic_sum(|fx| fx.heal_on_potion_use);
        if bird > 0 {
            self.heal_player(bird);
        }
        self.check_win();
    }

    /// 指定敌人的药水挑目标:玩家点的那个还活着就用它,否则打倒下的第一个
    fn potion_target(&self, target: Option<usize>) -> Option<usize> {
        target
            .filter(|i| self.enemies.get(*i).map(|e| e.alive()).unwrap_or(false))
            .or_else(|| self.first_alive())
    }

    /// 蛇油:手上能算出费用的牌重掷成 0~3 费(整场战斗有效,和蛇眼的"混乱"一致)
    fn randomize_hand_costs(&mut self) {
        let idxs: Vec<usize> = (0..self.hand.len())
            .filter(|i| matches!(self.hand[*i].def.cost, Cost::Fixed(_)))
            .collect();
        for i in idxs {
            let base = match self.hand[i].def.cost {
                Cost::Fixed(n) => n as i32,
                _ => continue,
            };
            let new = self.streams.floor(FloorStream::CardRandomRng).random(3) as i32;
            self.hand[i].cost_delta = new - base;
        }
    }

    /// 发现类药水的池子:本职业的某一类型(不含基础牌),或无色牌里的非普通档
    fn discovery_pool(&self, kind: DiscoveryPool) -> Vec<&'static crate::core::card::CardDef> {
        match kind {
            DiscoveryPool::Colorless => cards::colorless_pool()
                .into_iter()
                .filter(|c| matches!(c.rarity, Rarity::Uncommon | Rarity::Rare))
                .collect(),
            _ => {
                let want = match kind {
                    DiscoveryPool::Attack => crate::core::card::CardType::Attack,
                    DiscoveryPool::Power => crate::core::card::CardType::Power,
                    _ => crate::core::card::CardType::Skill,
                };
                cards::class_card_pool()
                    .into_iter()
                    .filter(|c| c.kind == want)
                    .collect()
            }
        }
    }

    /// 亮出 n 张不重样的候选,开一次"挑一张"的选择;牌池抽干了就少亮几张.
    /// free 表示选中的那张本回合 0 费(发现类药水);工具箱与抄本不免费.
    fn offer_pick(
        &mut self,
        mut pool: Vec<&'static crate::core::card::CardDef>,
        n: usize,
        free: bool,
        label: &str,
    ) {
        let mut offered: Vec<CardInstance> = Vec::new();
        while offered.len() < n && !pool.is_empty() {
            let i = self
                .streams
                .floor(FloorStream::CardRandomRng)
                .random(pool.len() as u32 - 1) as usize;
            let def = pool.remove(i);
            let mut inst = CardInstance::new(def);
            self.fix_new_card(&mut inst);
            offered.push(inst);
        }
        if offered.is_empty() {
            return;
        }
        self.begin_choice(
            ChoiceSource::Offered,
            ChoiceAction::ToHand,
            ChoiceFilter::Any,
            1,
            label,
        );
        if let Some(ch) = self.choice.as_mut() {
            ch.offered = offered;
            ch.free = free;
        }
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
            rested: false,
            hp,
            max_hp: hp,
            deck: ids.iter().map(|id| cards::card(id)).collect(),
            relics: Vec::new(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
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
        Combat::new(enc(encounter), setup(80, ids, &[]), RngRegistry::new(1))
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
        assert_eq!(c.draw.first().unwrap().def.id, picked, "放到抽牌堆顶");
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
        // 抽牌堆的顶是下标 0,而且下一次抽牌就要抽到它
        assert_eq!(c.draw.first().unwrap().def.id, "defend", "应该放在抽牌堆顶");
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

    /// 二重身升级版:一次选择就把选中的牌复制两份(参考实现同样一次选择加两份)
    #[test]
    fn dual_wield_plus_copies_twice_in_one_choice() {
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        let mut dw = crate::core::cards::card("dual_wield");
        dw.upgraded = true;
        c.hand = vec![dw, crate::core::cards::card("strike")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Hand);
        assert_eq!(c.choice.as_ref().unwrap().copies, 2, "升级版一次复制两份");
        // 打出二重身后手牌只剩那张 strike,下标 0 就是它
        c.choose(0).unwrap();
        assert!(c.choice.is_none(), "一次选择就做完,不该再挂第二次选择");
        let strikes = c.hand.iter().filter(|x| x.def.id == "strike").count();
        assert_eq!(strikes, 3, "原来的 1 张 + 复制的 2 张");
    }

    /// 掘出:候选池里不能包含被消耗掉的掘出自己(wiki Update History + 参考实现)
    #[test]
    fn exhume_cannot_recover_itself() {
        let mut c = combat_with("jaw_worm_solo", &["exhume"]);
        c.exhaust.push(crate::core::cards::card("bash"));
        c.hand = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Exhaust);
        let ids: Vec<&str> = c.choice_candidates().iter().map(|(_, x)| x.def.id).collect();
        assert!(!ids.contains(&"exhume"), "掘出不能拿回自己:{ids:?}");
        assert!(ids.contains(&"bash"), "别的消耗牌还能拿:{ids:?}");
    }


    /// 浩劫连锁:浩劫打浩劫再打出一张普通牌,链上每张都记下来
    #[test]
    fn havoc_chain_records_every_card_it_plays() {
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 4]);
        c.hand = vec![crate::core::cards::card("havoc")];
        // 抽牌堆的顶是下标 0,所以这样排:先被抽到的是最前面那个 havoc
        c.draw = vec![
            crate::core::cards::card("havoc"),
            crate::core::cards::card("havoc"),
            crate::core::cards::card("strike"),
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
            assert_eq!(c.draw.first().unwrap().def.id, picked, "放到抽牌堆顶");
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
        let mut c = Combat::new(
            enc("jaw_worm_solo"),
            setup(80, &["wound"; 5], &relics),
            RngRegistry::new(7),
        );
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
        let c = Combat::new(
            enc("jaw_worm_solo"),
            setup(80, &["strike"; 5], &relics),
            RngRegistry::new(3),
        );
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

    /// 疯狂只挑"还花费用"的牌:掷到 0 费/不可打出的会重掷
    #[test]
    fn madness_skips_zero_and_unplayable_cards() {
        let mut c = staged(&["madness", "wound", "bludgeon"], &["madness", "wound", "bludgeon"]);
        let idx = hand_idx(&c, "madness");
        c.play_card(idx, None).unwrap();
        let w = c.hand.iter().find(|x| x.def.id == "wound").unwrap();
        assert_eq!(w.fixed_cost(), None, "伤口不可打出,不该被选为目标");
        let b = c.hand.iter().find(|x| x.def.id == "bludgeon").unwrap();
        assert_eq!(b.fixed_cost(), Some(0), "重掷到要花费用的打击并降成 0");
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
    fn feed_only_grows_max_hp_on_non_minion_kills() {
        // 普通怪:击杀给 3 最大生命,同时回 3 血
        let mut c = combat_with("jaw_worm_solo", &["feed"; 5]);
        c.enemies[0].hp = 5;
        let (max0, hp0) = (c.player.max_hp, c.player.hp);
        let idx = hand_idx(&c, "feed");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "10 伤打死 5 血的虫子");
        assert_eq!(c.player.max_hp, max0 + 3, "普通怪给上限");
        assert_eq!(c.player.hp, hp0 + 3, "回等量血");

        // 首领(不是随从):照给
        let mut c = combat_with("bronze_automaton", &["feed"; 5]);
        let boss = c.enemies.iter().position(|e| !e.is_minion()).unwrap();
        c.enemies[boss].hp = 5;
        let max0 = c.player.max_hp;
        let idx = hand_idx(&c, "feed");
        c.play_card(idx, Some(boss)).unwrap();
        assert!(c.enemies[boss].dead(), "10 伤打死 5 血的首领");
        assert_eq!(c.player.max_hp, max0 + 3, "首领不算随从,照给上限");

        // 随从:打死也不给
        let mut c = combat_with("daggers", &["feed"; 5]);
        assert!(c.enemies[0].is_minion(), "小刀是随从");
        c.enemies[0].hp = 5;
        let (max0, hp0) = (c.player.max_hp, c.player.hp);
        let idx = hand_idx(&c, "feed");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "10 伤打死 5 血的小刀");
        assert_eq!(c.player.max_hp, max0, "随从不算数");
        assert_eq!(c.player.hp, hp0, "也不回血");
    }

    #[test]
    fn hand_of_greed_does_not_pay_for_minion_kills() {
        let mut c = combat_with("daggers", &["hand_of_greed"; 5]);
        assert!(c.enemies[0].is_minion(), "小刀是随从");
        c.enemies[0].hp = 5;
        let idx = hand_idx(&c, "hand_of_greed");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "20 伤打死 5 血的小刀");
        assert_eq!(c.gold_gained, 0, "击杀随从不给金币");

        let mut c = combat_with("jaw_worm_solo", &["hand_of_greed"; 5]);
        c.enemies[0].hp = 5;
        let idx = hand_idx(&c, "hand_of_greed");
        c.play_card(idx, Some(0)).unwrap();
        assert_eq!(c.gold_gained, 20, "普通怪照给");
    }

    /// 血祭匕首:只有"击杀非随从"才让这张牌永久 +bonus;随从与没打死都不长
    #[test]
    fn ritual_dagger_grows_only_on_a_non_minion_kill() {
        let dagger = crate::core::events::event_card("ritual_dagger").expect("血祭匕首在事件牌里");
        // 造一场手上全是匕首的战斗(匕首是事件牌,不在常规卡池里)
        let with_dagger = |encounter: &'static str| -> Combat {
            let deck: Vec<CardInstance> = (0..5).map(|_| CardInstance::new(dagger)).collect();
            let setup = CombatSetup {
                rested: false,
                hp: 80,
                max_hp: 80,
                deck,
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
            asc: 0,
            };
            let mut c = Combat::new(enc(encounter), setup, RngRegistry::new(9));
            let rest: Vec<CardInstance> = c.draw.drain(..).collect();
            c.hand.extend(rest);
            c.energy = 9;
            c
        };
        let dagger_bonus = |c: &Combat| -> i32 {
            [&c.hand, &c.draw, &c.discard, &c.exhaust]
                .into_iter()
                .flatten()
                .filter(|x| x.def.id == "ritual_dagger")
                .map(|x| x.bonus)
                .max()
                .unwrap_or(0)
        };

        // 击杀普通怪:这张牌 +3
        let mut c = with_dagger("jaw_worm_solo");
        c.enemies[0].hp = 5;
        let idx = hand_idx(&c, "ritual_dagger");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "15 伤打死 5 血的虫子");
        assert_eq!(dagger_bonus(&c), 3, "击杀非随从才成长");

        // 击杀随从:不成长
        let mut c = with_dagger("daggers");
        assert!(c.enemies[0].is_minion(), "小刀是随从");
        c.enemies[0].hp = 5;
        let idx = hand_idx(&c, "ritual_dagger");
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "15 伤打死 5 血的小刀");
        assert_eq!(dagger_bonus(&c), 0, "随从不算数");

        // 没打死:不成长
        let mut c = with_dagger("jaw_worm_solo");
        c.enemies[0].hp = 200;
        c.enemies[0].max_hp = 200;
        let idx = hand_idx(&c, "ritual_dagger");
        c.play_card(idx, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 185, "只打掉 15 血");
        assert_eq!(dagger_bonus(&c), 0, "没打死不长");
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
        // 原版直接扣 9 点力量,力量允许被压成负数(参考实现也是 -9)
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), -4, "这回合力量被扣成 -4");
        // 它这一击因此从 16 掉到 7
        c.end_turn();
        assert_eq!(c.player.hp, 80 - 7, "力量被扣掉后攻击也变弱了");
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

    /// 纠缠:Innate,挪到抽牌堆顶,开局的五张里就有它
    #[test]
    fn writhe_is_innate_and_starts_in_hand() {
        let c = combat_with(
            "jaw_worm_solo",
            &["strike", "strike", "strike", "strike", "strike", "writhe"],
        );
        assert!(c.hand.iter().any(|x| x.def.id == "writhe"), "开局在手");
        assert_eq!(c.hand.len(), DRAW_PER_TURN, "起手就是五张");
        assert!(!c.draw.iter().any(|x| x.def.id == "writhe"));
        assert_eq!(c.draw.len(), 1, "六张牌里抓到五张,还剩一张 strike");
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

    /// 战斗恍惚:先抽 3 张,再挂 No Draw,本回合之后连卡牌效果的抽牌也挡住
    #[test]
    fn battle_trance_gives_no_draw_for_the_rest_of_the_turn() {
        let deck = [
            "battle_trance",
            "pommel_strike",
            "strike",
            "strike",
            "strike",
            "strike",
            "strike",
        ];
        let mut c = staged(&deck, &["battle_trance", "pommel_strike"]);
        let trance = hand_idx(&c, "battle_trance");
        c.play_card(trance, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::NoDraw), 1, "挂上一层");
        assert_eq!(c.hand.len(), 4, "本体先抽 3 张(手牌剩 pommel + 抽到的 3)");
        assert_eq!(c.draw.len(), 2, "抽牌堆被拿掉 3 张");
        // 打磨打击还要抽 1 张,被 No Draw 挡下
        let pommel = hand_idx(&c, "pommel_strike");
        c.play_card(pommel, Some(0)).unwrap();
        assert_eq!(c.hand.len(), 3, "卡牌效果的抽牌也被挡住");
        assert_eq!(c.draw.len(), 2, "抽牌堆没动");
        // 回合末整条消失,下回合正常抽 5 张
        c.end_turn();
        assert!(!c.player.statuses.has(Status::NoDraw), "回合末消失");
        assert_eq!(c.hand.len(), 5, "下回合起手 5 张");
    }

    /// No Draw 算减益,神器顶掉它(Battle Trance 自己照样抽 3 张)
    #[test]
    fn artifact_blocks_battle_trance_no_draw() {
        let deck = ["battle_trance", "strike", "defend", "defend", "defend", "defend"];
        let mut c = staged(&deck, &["battle_trance", "strike"]);
        c.player.statuses.add(Status::Artifact, 1);
        let trance = hand_idx(&c, "battle_trance");
        c.play_card(trance, None).unwrap();
        assert!(!c.player.statuses.has(Status::NoDraw), "被神器挡下");
        assert_eq!(c.player.statuses.get(Status::Artifact), 0, "用掉一层神器");
        assert_eq!(c.hand.len(), 4, "抽 3 张照常");
    }
}

#[cfg(test)]
mod monster_tests {
    use super::*;
    use crate::core::cards::card;

    /// 打一场指定遭遇:80 血,牌组给几张打击/防御够用就行
    fn lock(id: &'static str) -> Combat {
        lock_seed(id, 7)
    }

    /// 同上,但种子自己定(开局的阵容抽签要看它)
    fn lock_seed(id: &'static str, seed: u64) -> Combat {
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
            rested: false,
            hp: 80,
            max_hp: 80,
            deck,
            relics: Vec::new(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
        };
        Combat::new(enc, setup, RngRegistry::new(seed))
    }

    /// 敌人这一招的意图(不看睡眠/半死这些覆盖)
    fn intent(c: &Combat, i: usize) -> Intent {
        c.enemies[i].def.moves[c.enemies[i].next_move].intent
    }

    /// 阵容里的敌人 id(按槽位序)
    fn lineup_ids(c: &Combat) -> Vec<&'static str> {
        c.enemies.iter().map(|e| e.def.id).collect()
    }

    /// 敌人这一招的名字
    fn move_name(c: &Combat, i: usize) -> &'static str {
        c.enemies[i].def.moves[c.enemies[i].next_move].name
    }

    /// 扭动巨物:开局那一次掷招走 33/33/33(多段/连枷/枯萎,按语料 ai.firstTurn
    /// 记的 lightspeed 招式 1/2/3);
    /// 挨打后的 Reactive 重掷走的是级联(参考实现 firstTurn 看 moveHistory 空不空,
    /// 首招掷出后它就不空了),所以重掷能掷出开局分支掷不到的寄生.
    #[test]
    fn writhing_mass_rerolls_into_cascade_moves_on_turn_one() {
        let mut saw_flail = false;
        let mut saw_implant = false;
        for seed in 0..400u64 {
            let mut c = lock_seed("writhing_mass_solo", seed);
            let opener = move_name(&c, 0);
            assert!(
                matches!(opener, "Multi Strike" | "Flail" | "Wither"),
                "开场应是 多段/连枷/枯萎(语料+反编译),实得 {opener}(seed {seed})"
            );
            c.damage_enemy(0, 3);
            let now = move_name(&c, 0);
            saw_flail |= now == "Flail";
            saw_implant |= now == "Implant";
        }
        assert!(saw_flail, "挨打重掷应该掷得到连枷");
        assert!(saw_implant, "挨打重掷应该掷得到寄生");
    }

    /// 寄生只算"真的出手用过"那一次:选中又被重掷掉的不算
    /// (参考实现把 usedImplant 记在 Implant 的 execute 里,不是选招时)
    #[test]
    fn writhing_mass_implant_counts_only_when_it_is_used() {
        let mut found = None;
        for seed in 0..400u64 {
            let mut c = lock_seed("writhing_mass_solo", seed);
            c.damage_enemy(0, 3);
            if move_name(&c, 0) == "Implant" && c.enemies[0].hp > 0 {
                found = Some(c);
                break;
            }
        }
        let mut c = found.expect("应该能重掷到寄生");
        assert!(!c.enemies[0].state.implant_used, "还没出手,不算用过");
        // 让它把这招真的打出来
        c.end_turn();
        assert!(c.enemies[0].state.implant_used, "出手过就该记成用过");
    }

    #[test]
    fn cultist_charges_once_then_strikes() {
        let mut c = lock("cultist_solo");
        assert_eq!(intent(&c, 0), Intent::Buff, "开场先充能");
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::Ritual), 3);
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            0,
            "刚挂上的仪式当回合不结算,下一回合起才涨力量"
        );
        assert_eq!(intent(&c, 0), Intent::Attack { damage: 6, times: 1 });
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(c.player.hp, hp - 6, "黑暗打击 6,这时力量还没涨");
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            3,
            "这一回合末才涨力量"
        );
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
    fn three_sentries_start_as_if_they_already_acted() {
        let c = lock("three_sentries");
        for e in &c.enemies {
            assert_eq!(e.state.turns, 1, "三哨兵开局就算已经行动过一回合");
        }
        // 外两只先放螺栓、中间那只先射线(与参考实现一致)
        assert_eq!(
            (0..3).map(|i| move_name(&c, i)).collect::<Vec<_>>(),
            vec!["Bolt", "Beam", "Bolt"]
        );
        // 别的遭遇里的哨兵没这段历史,首招仍按站位定
        let pair = lock("sentry_and_sphere");
        assert_eq!(pair.enemies[0].state.turns, 0, "单只哨兵不该被预置历史");
        assert_eq!(move_name(&pair, 0), "Bolt");
    }

    #[test]
    fn jaw_worm_horde_starts_buffed_and_rolls_its_first_move() {
        let horde = lock("jaw_worm_horde");
        assert_eq!(horde.enemies.len(), 3);
        for e in &horde.enemies {
            assert_eq!(e.statuses.get(Status::Strength), 3, "每只开局力量 3");
            assert_eq!(e.block, 5, "每只开局格挡 5");
            assert_eq!(e.state.turns, 1, "每只开局就算已经行动过一回合");
        }
        // 常规颚虫第一回合必定咬一口,三连里的颚虫被预置了历史,第一回合就按分布掷
        assert_eq!(move_name(&lock("jaw_worm_solo"), 0), "Chomp");
        let openers: std::collections::HashSet<&str> =
            (0..64).map(|s| move_name(&lock_seed("jaw_worm_horde", s), 0)).collect();
        assert!(
            openers.iter().any(|m| *m != "Chomp"),
            "三连的颚虫第一回合不该被锁死成咬一口"
        );
    }

    #[test]
    fn shape_encounters_roll_the_lineup_per_seed() {
        // 同一种子两次进入,阵容要一致
        assert_eq!(
            lineup_ids(&lock_seed("three_shapes", 99)),
            lineup_ids(&lock_seed("three_shapes", 99))
        );
        // 不同种子会抽出不同阵容
        let seen: std::collections::HashSet<Vec<&str>> =
            (0..64).map(|s| lineup_ids(&lock_seed("three_shapes", s))).collect();
        assert!(seen.len() >= 3, "三只形状只抽出 {} 种阵容", seen.len());
        assert!(seen.iter().all(|l| l.len() == 3), "三只形状就该抽三只");
        // 四只的那场数量是 4;球体守卫固定在场
        assert_eq!(lineup_ids(&lock_seed("four_shapes", 99)).len(), 4);
        for seed in 0..16u64 {
            let ids = lineup_ids(&lock_seed("sphere_and_two_shapes", seed));
            assert_eq!(ids.len(), 3);
            assert_eq!(ids[2], "spheric_guardian", "球体守卫恒定存在");
        }
    }

    #[test]
    fn gremlin_leader_escorts_are_minions() {
        let mut c = lock("gremlin_leader_gang");
        assert!(
            c.enemies[0].is_minion() && c.enemies[1].is_minion(),
            "头目开局带的两只小鬼是随从"
        );
        assert!(!c.enemies[2].is_minion(), "头目自己不是随从");
        // 头目一倒,两只随从小鬼就退场,这一场立刻算赢
        let leader = c
            .enemies
            .iter()
            .position(|e| e.def.id == "gremlin_leader")
            .unwrap();
        c.damage_enemy(leader, 999);
        c.check_win();
        assert_eq!(c.phase, Phase::Won, "头目倒下就算赢");
        assert!(
            c.enemies.iter().all(|e| !e.is_minion() || !e.up()),
            "随从跟着退场"
        );
    }

    #[test]
    fn shield_and_spear_start_with_the_player_surrounded() {
        let c = lock("shield_and_spear");
        assert!(c.player.statuses.has(Status::Surrounded), "开局就被包围");
        // 初始朝向是右边的长矛(slot 1):盾从背后打,吃 1.5 倍
        assert_eq!(c.enemy_attack_damage(0, 12), 18);
        assert_eq!(c.enemy_attack_damage(1, 12), 12);
    }

    #[test]
    fn back_attack_follows_the_faced_enemy_not_the_last_card() {
        // 原版语义(wiki Surrounded:"Receive 50% more damage if attacked from
        // behind. Use targeting cards or potions to change your orientation."):
        // 谁吃 1.5 只由"玩家现在朝着哪只"决定,而朝向只被指向敌人的牌改写;
        // 不指向敌人的牌(能力/技能)不改朝向,背后的那只从头到尾照旧吃满.
        let mut c = lock("shield_and_spear");
        assert_eq!(c.facing, 1, "开局朝向右边的长矛(slot 1)");
        // 一张牌都不打:背后的盾吃 1.5,正面的矛原样
        assert_eq!(c.enemy_attack_damage(0, 12), 18, "背后的盾 12 * 1.5");
        assert_eq!(c.enemy_attack_damage(1, 12), 12, "正面的矛不打折也不加成");
        // 打一张不指向敌人的能力牌:朝向不动,谁吃 1.5 也不动
        c.energy = 3;
        c.hand.push(card("inflame"));
        let inflame = c.hand.iter().position(|x| x.def.id == "inflame").unwrap();
        c.play_card(inflame, None).unwrap();
        assert_eq!(c.facing, 1, "不指向敌人的牌不改朝向");
        assert_eq!(c.enemy_attack_damage(0, 12), 18, "盾还在背后");
        assert_eq!(c.enemy_attack_damage(1, 12), 12, "矛还在正面");
        // 打一张指向盾的牌:朝向翻到盾,改由矛吃 1.5
        c.hand.push(card("strike"));
        let strike = c.hand.iter().position(|x| x.def.id == "strike").unwrap();
        c.play_card(strike, Some(0)).unwrap();
        assert_eq!(c.facing, 0, "指向谁就朝向谁");
        assert_eq!(c.enemy_attack_damage(0, 12), 12, "正面的盾不再加成");
        assert_eq!(c.enemy_attack_damage(1, 12), 18, "背后的矛改成 1.5");
        // 朝着的那只倒下也算(语料:朝向保持"最后指向的那只",不因它倒下而换边)
        c.damage_enemy(0, 9999);
        assert_eq!(c.enemy_attack_damage(1, 12), 18, "盾倒下后矛依旧算背后");
    }

    #[test]
    fn targeting_potions_flip_the_facing_like_targeting_cards() {
        // 原版 Surrounded(wiki:"Use targeting cards or potions to change your
        // orientation"):指向敌人的药水命中的那只就是新朝向,夹击的 1.5 倍随之换边.
        let mut c = lock("shield_and_spear");
        assert_eq!(c.facing, 1, "开局朝向右边的长矛(slot 1)");
        assert_eq!(c.enemy_attack_damage(0, 12), 18, "背后的盾 12 * 1.5");
        assert_eq!(c.enemy_attack_damage(1, 12), 12, "正面的矛不打折也不加成");
        // 恐惧药水指向盾(0):朝向翻到盾,改由矛吃 1.5
        let fear = crate::core::potions::by_id("fear_potion").expect("恐惧药剂");
        c.use_potion(fear, Some(0));
        assert_eq!(c.facing, 0, "药水指向谁就朝向谁");
        assert_eq!(c.enemy_attack_damage(0, 12), 12, "正面的盾不再加成");
        assert_eq!(c.enemy_attack_damage(1, 12), 18, "背后的矛改成 1.5");
        // 火焰药水指回矛(1):朝向再翻回去
        let fire = crate::core::potions::by_id("fire_potion").expect("火焰药剂");
        c.use_potion(fire, Some(1));
        assert_eq!(c.facing, 1, "再指回矛");
        assert_eq!(c.enemy_attack_damage(0, 12), 18, "盾又回到背后");
        assert_eq!(c.enemy_attack_damage(1, 12), 12, "矛又回到正面");
        // 不指向敌人的药水(格挡)不动朝向
        let block = crate::core::potions::by_id("block_potion").expect("格挡药剂");
        c.use_potion(block, None);
        assert_eq!(c.facing, 1, "自身药水不改朝向");
        assert_eq!(c.enemy_attack_damage(0, 12), 18, "盾照旧在背后");
        assert_eq!(c.enemy_attack_damage(1, 12), 12, "矛照旧在正面");
    }

    #[test]
    fn spire_shield_rolls_its_opener_and_smashes_on_turns_three_six_nine() {
        // 语料 SPIRE_SHIELD.ai:开局 aiRng.randomBoolean() 五五开定先撞还是先固守;
        // 之后每三回合一块,块的头两回合是撞与固守各一次(先后五五开),第 3/6/9… 回合重砸.
        let mut saw_bash = false;
        let mut saw_fortify = false;
        for seed in 1..=32u64 {
            let mut c = lock_seed("shield_and_spear", seed);
            c.player.hp = 9999;
            c.player.max_hp = 9999;
            let mut seq = Vec::new();
            for _ in 0..9 {
                seq.push(move_name(&c, 0).to_string());
                c.end_turn();
            }
            assert!(
                seq[0] == "Bash" || seq[0] == "Fortify",
                "首招只能是撞或固守(五五开): {seq:?}"
            );
            saw_bash |= seq[0] == "Bash";
            saw_fortify |= seq[0] == "Fortify";
            for block in 0..3 {
                let a = seq[block * 3].as_str();
                let b = seq[block * 3 + 1].as_str();
                assert!(
                    matches!((a, b), ("Bash", "Fortify") | ("Fortify", "Bash")),
                    "第 {}、{} 回合是撞与固守各一次: {seq:?}",
                    block * 3 + 1,
                    block * 3 + 2
                );
                assert_eq!(
                    seq[block * 3 + 2],
                    "Smash",
                    "第 {} 回合重砸: {seq:?}",
                    block * 3 + 3
                );
            }
        }
        assert!(saw_bash && saw_fortify, "开局五五开:两种首招都要出现过");
    }

    #[test]
    fn spire_shield_cadence_survives_a_pre_seeded_opener() {
        // 沙盒/回放给敌人预置首招时(replay.rs 的 build_enemies 把 next_move 与
        // state.last 一起指到首招),块的头一回合不能被读成"块内第二回合":
        // 重砸仍要落在第 3/6/9 回合.
        let mut c = lock_seed("shield_and_spear", 3);
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        c.enemies[0].next_move = 0;
        c.enemies[0].state.last = Some(0);
        c.enemies[0].state.move_rolled = true;
        let mut seq = Vec::new();
        for _ in 0..9 {
            seq.push(move_name(&c, 0).to_string());
            c.end_turn();
        }
        assert_eq!(seq[0], "Bash", "预置的首招照用: {seq:?}");
        assert_eq!(seq[1], "Fortify", "撞之后是本块的另一招: {seq:?}");
        assert_eq!(seq[2], "Smash", "第 3 回合重砸: {seq:?}");
        assert_eq!(seq[5], "Smash", "第 6 回合重砸: {seq:?}");
        assert_eq!(seq[8], "Smash", "第 9 回合重砸: {seq:?}");
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
        // 玩家打一张技能牌 → 狂怒 +2 力量(手牌是洗出来的,这里补一张技能牌,不挑种子)
        c.energy = 3;
        c.hand.push(crate::core::cards::card("defend"));
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
        // 大史莱姆是酸/尖刺五五开抽的,分裂产物要跟着抽到的那只走
        let id = c.enemies[0].def.id;
        let medium = match id {
            "acid_slime_large" => "acid_slime_medium",
            "spike_slime_large" => "spike_slime_medium",
            other => panic!("大史莱姆不该是 {other}"),
        };
        c.enemies[0].hp = 30;
        c.damage_enemy(0, 1);
        c.end_turn();
        let mediums = c.enemies.iter().filter(|e| e.def.id == medium).count();
        assert_eq!(mediums, 2, "{id} 分裂成两只 {medium}");
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
    fn sharp_hide_still_bites_when_the_attack_kills_the_guardian() {
        // 原版把尖刺的伤害排在这张牌之后结算,而战斗胜利清空动作队列时只清
        // clearOnCombatVictory=true 的动作,所以击杀的那一击照样挨刺.
        let mut c = lock("the_guardian");
        c.damage_enemy(0, 30);
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::SharpHide), 3);
        c.enemies[0].hp = 1;
        c.energy = 3;
        let hp = c.player.hp;
        let strike = c.hand.iter().position(|x| x.def.id == "strike").unwrap();
        c.play_card(strike, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 0, "这一击把它打死");
        assert!(
            c.player.hp < hp,
            "击杀的那一击也要吃尖刺外壳(反伤排在牌之后,不因目标已死而取消)"
        );
    }

    #[test]
    fn a_card_that_costs_zero_this_turn_is_actually_free() {
        // free_this_turn 要在"算费用"时还生效(木乃伊之手/发现类药水/液态记忆都靠它).
        let mut c = lock("looter_solo");
        c.hand[0] = card("bash"); // 2 费
        c.hand[0].free_this_turn = true;
        c.energy = 1;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.energy, 1, "本回合 0 费的牌不该花能量");
    }

    #[test]
    fn mummified_hand_skips_cards_that_already_cost_nothing() {
        // 候选只认"当前还要花费用"的牌;手里只剩 0 费牌时不该乱点一张.
        let mut c = lock("looter_solo");
        c.relics
            .push(crate::core::relics::relic_def_or_panic("mummified_hand"));
        c.hand = vec![card("flex"), card("inflame")]; // flex 0 费,inflame 是能力牌
        c.energy = 3;
        c.play_card(1, None).unwrap();
        assert_eq!(c.hand.len(), 1);
        assert!(
            !c.hand[0].free_this_turn,
            "候选为空时不该把已有的 0 费牌再标一次"
        );
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

    /// 书呆子(Book of Stabbing)A18:单刺也会让刺击数自增,所以多段刺击来得更快.
    /// 依据:反编译 MonsterSpecific.cpp 的两句 `if (asc18) ++stabCount` 写在 return 之后
    /// (死代码),但它明确表达了 A18 的意图;参考实现按 wiki/原版把它算进去.
    #[test]
    fn book_of_stabbing_a18_single_stab_also_grows_the_count() {
        use crate::core::cards::card;
        let seq = |asc: u32| -> Vec<u32> {
            let enc = crate::core::enemies::encounter_def("book_of_stabbing_solo").unwrap();
            let setup = CombatSetup {
                rested: false,
                hp: 999,
                max_hp: 999,
                deck: vec![card("defend"); 10],
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
                asc,
            };
            let mut c = Combat::new(enc, setup, RngRegistry::new(11));
            let mut v = Vec::new();
            for _ in 0..6 {
                c.end_turn();
                v.push(c.enemies[0].state.stab);
            }
            v
        };
        let a0 = seq(0);
        let a18 = seq(18);
        assert_eq!(a0, vec![3, 3, 4, 5, 5, 6], "A0 的段数序列变了: {a0:?}");
        assert_eq!(a18, vec![3, 4, 5, 6, 7, 8], "A18 单刺也自增: {a18:?}");
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
        assert!(
            c.log.iter().any(|l| l.text.contains("steals 15 gold")),
            "偷钱要在战斗日志里看得见"
        );
        // 抢完两回合就霰雾弹跑路
        for _ in 0..6 {
            if c.enemies[0].escaped {
                break;
            }
            c.end_turn();
        }
        assert!(c.enemies[0].escaped, "最后一定会逃");
        assert_eq!(c.phase, Phase::Won, "只剩它一只,逃跑就算赢");
        // 逃跑就不退赃款:金币停在扣掉赃款的数上
        let stolen = c.enemies[0].state.stolen;
        assert!(stolen > 0, "逃跑前确实抢到过钱");
        assert_eq!(c.player_gold, 100 - stolen, "逃跑不退赃款");
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
        assert!(
            c.log.iter().any(|l| l.text.contains("drops the 15 gold")),
            "退赃要在战斗日志里看得见"
        );
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
        // 只留一只小酸液(一堆史莱姆的阵容是随机顺序,先按 id 找出来)
        let i = c
            .enemies
            .iter()
            .position(|e| e.def.id == "acid_slime_small")
            .expect("一堆史莱姆里必有一只小酸液");
        let only = c.enemies.remove(i);
        c.enemies = vec![only];
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
            rested: false,
            hp: 80,
            max_hp: 80,
            deck,
            relics: Vec::new(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
        };
        Combat::new(enc, setup, RngRegistry::new(11))
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
        // 落地之后被打不再减半(给足血量,免得被"过量伤害夹在 0"盖住)
        c.enemies[0].hp = 50;
        c.enemies[0].max_hp = 50;
        c.damage_enemy(0, 11);
        assert_eq!(c.enemies[0].hp, 50 - 11);
    }

    /// 靴子 The Boot:未被格挡的攻击伤害只剩 1..4 点时提到 5.依据反编译
    /// (sts_lightspeed Monster::attackedUnblockedHelper)这一步排在格挡与目标侧的
    /// 飞行/慢速/无形之后,所以 4 点打在无形怪身上也是 5,不是 1.
    #[test]
    fn boot_boosts_unblocked_damage_after_reductions() {
        let boot = crate::core::relics::relic_def_or_panic("the_boot");
        let booted = |id: &'static str| {
            let mut c = lock(id);
            c.relics.push(boot);
            c
        };

        // 无形:4 点先被压到 1,靴子再抬到 5
        let mut c = booted("jaw_worm_solo");
        c.add_enemy_status(0, Status::Intangible, 2);
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "无形之下的 4 点被靴子抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);

        // 飞行:4 点先减半到 2,靴子再抬到 5
        let mut c = booted("three_byrds");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "飞行之下 4 点减半到 2 也被靴子抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);

        // 无减伤无格挡:4 -> 5
        let mut c = booted("jaw_worm_solo");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5);
        assert_eq!(c.enemies[0].hp, hp - 5);

        // 被格挡的部分不算:6 点打在 5 格挡上,剩 1 也抬到 5
        let mut c = booted("jaw_worm_solo");
        c.enemies[0].block = 5;
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 6), 5, "6 打 5 格挡,剩 1 被靴子抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);
        assert_eq!(c.enemies[0].block, 0, "5 点格挡照扣");

        // 全挡住就不抬:4 点打在 4 格挡上
        let mut c = booted("jaw_worm_solo");
        c.enemies[0].block = 4;
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 0, "全挡住不掉血也不抬");
        assert_eq!(c.enemies[0].hp, hp);

        // 没有靴子时 4 点还是 4
        let mut c = lock("jaw_worm_solo");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 4);
        assert_eq!(c.enemies[0].hp, hp - 4);
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
        // 掉血会让它当下少等量力量(移形换影),自己的回合结束再补回来
        c.damage_enemy(0, 40);
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), -40, "挨打就掉力量");
        assert_eq!(c.enemies[0].temp_strength, 40, "掉的力量记着回合末回补");
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
        // 只留一只自爆怪:阵容是抽签来的,可能抽到两只
        c.enemies.retain(|e| e.def.id == "exploder");
        c.enemies.truncate(1);
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

    /// 暗灵半死:第一个自己回合摆 Regrow,最后一个自己回合换成 Reincarnate
    /// (玩家能预读到"下一回合复活"),复活后就位
    #[test]
    fn darkling_intent_turns_to_reincarnate_before_reviving() {
        let mut c = lock("three_darklings");
        c.player.hp = 999;
        c.enemies[0].hp = 10;
        c.damage_enemy(0, 20);
        c.settle_deaths();
        assert!(c.enemies[0].state.half_dead);
        let intent_name = |c: &Combat| c.enemies[0].def.moves[c.enemies[0].next_move].name;
        assert_eq!(intent_name(&c), "Regrow", "刚倒下先摆 Regrow");
        c.end_turn();
        assert_eq!(intent_name(&c), "Reincarnate", "最后一轮换成 Reincarnate");
        assert!(c.enemies[0].state.half_dead, "这轮还没站起来");
        c.end_turn();
        assert!(!c.enemies[0].state.half_dead, "下一轮真正复活");
        assert!(c.enemies[0].hp > 0);
    }

    /// 扭动巨物开场三选一:多段 / 连枷(格挡攻击) / 枯萎.
    /// 语料 ai.firstTurn 记的是 lightspeed 的 33/66 分档落在招式 1,2,3,
    /// wiki 那句"重击"是笔误(冲突已登记);开场也绝不会是重击/寄生/连枷之外的怪招.
    #[test]
    fn writhing_mass_opens_with_multi_big_hit_or_debuff() {
        let mut seen_flail = false;
        let mut seen_multi = false;
        let mut seen_wither = false;
        for seed in 0..200u64 {
            let enc = crate::core::enemies::encounter_def("writhing_mass_solo").unwrap();
            let setup = CombatSetup {
                rested: false,
                hp: 80,
                max_hp: 80,
                deck: vec![card("strike")],
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
            asc: 0,
            };
            let c = Combat::new(enc, setup, RngRegistry::new(seed));
            let name = c.enemies[0].def.moves[c.enemies[0].next_move].name;
            assert!(
                matches!(name, "Multi Strike" | "Flail" | "Wither"),
                "开场应是 多段/连枷/枯萎(语料+反编译),实得 {name}(seed {seed})"
            );
            seen_flail |= name == "Flail";
            seen_multi |= name == "Multi Strike";
            seen_wither |= name == "Wither";
        }
        assert!(seen_flail, "开场该掷得到连枷");
        assert!(seen_multi, "开场该掷得到多段");
        assert!(seen_wither, "开场该掷得到枯萎");
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
        // 时间扭曲已经把这一回合掐掉了:再出牌要被拦下(参考实现 queueEndTurn)
        c.hand.push(cards::card("strike"));
        assert!(c.play_card(c.hand.len() - 1, Some(0)).is_err(), "第 12 张之后再出牌无效");
    }

    /// 时间吞噬者的重击:少抽一次要撑到下一个玩家回合,用掉之后才消失
    #[test]
    fn head_slam_draw_reduction_costs_a_card_next_turn() {
        let mut c = lock("time_eater");
        c.player.hp = 999;
        let hs = c.enemies[0].def.move_index("Head Slam").unwrap();
        c.enemies[0].next_move = hs;
        c.end_turn();
        assert_eq!(
            c.player.statuses.get(Status::DrawReduction),
            1,
            "怪物回合挂上的少抽一次本轮结束不该递减"
        );
        assert_eq!(c.hand.len(), 4, "这一回合抽 4 张(5 减 1)");
        // 用掉之后再过一个回合就该消失
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::DrawReduction), 0, "只影响一个回合");
        assert_eq!(c.hand.len(), 5, "下一回合恢复正常抽牌");
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

    /// 多努&迪卡:多努开场给全队加力量,之后与光束交替;迪卡开场光束(塞 Dazed),
    /// 之后与团队护盾交替.两只都带两层神器.
    #[test]
    fn donu_and_deca_alternate_buff_block_and_beam() {
        let mut c = lock("donu_and_deca");
        let donu = idx_of(&c, "donu");
        let deca = idx_of(&c, "deca");
        let move_of = |c: &Combat, i: usize| c.enemies[i].def.moves[c.enemies[i].next_move].name;
        assert_eq!(c.enemies[donu].statuses.get(Status::Artifact), 2);
        assert_eq!(c.enemies[deca].statuses.get(Status::Artifact), 2);
        // 开场:多努加力量,迪卡光束(顺带塞 Dazed)
        assert_eq!(move_of(&c, donu), "Circle of Power");
        assert_eq!(move_of(&c, deca), "Beam");
        c.end_turn();
        assert!(c.enemies[donu].statuses.get(Status::Strength) >= 3, "多努给全队加力量");
        assert!(c.enemies[deca].statuses.get(Status::Strength) >= 3, "力量是全队的");
        assert!(
            c.discard.iter().chain(c.hand.iter()).any(|x| x.def.id == "dazed"),
            "迪卡的光束塞 Dazed"
        );
        // 之后严格交替
        assert_eq!(move_of(&c, donu), "Beam");
        assert_eq!(move_of(&c, deca), "Square of Protection");
        c.end_turn();
        assert_eq!(move_of(&c, donu), "Circle of Power");
        assert_eq!(move_of(&c, deca), "Beam");
        assert!(c.enemies[deca].block > 0, "迪卡的团队护盾给自己(和全队)格挡");
    }

    /// 灯怪的"防御姿态"当回合就给 5 点金属化格挡:原版 MetallicizePower 的
    /// atEndOfTurn 没有 skipFirst,怪物回合末按当前层数无条件结算(参考实现的
    /// 测试也这么要求:stance block + end-of-turn metallicize).
    #[test]
    fn champ_defensive_stance_metallicize_pays_out_the_same_turn() {
        let mut c = lock("the_champ");
        let champ = idx_of(&c, "the_champ");
        let stance = c.enemies[champ]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Defensive Stance")
            .unwrap();
        c.enemies[champ].next_move = stance;
        c.end_turn();
        assert_eq!(c.enemies[champ].statuses.get(Status::Metallicize), 5);
        assert_eq!(c.enemies[champ].block, 20, "15 格挡 + 当回合挂上的 5 点金属化");
    }

    /// 迪卡 A19 的团队护盾:除了 16 格挡还挂 3 点板甲,当回合就按板甲结算;
    /// 第二次用时板甲涨到 6,当回合照样按 6 结算(触发器读的是当前层数).
    #[test]
    fn deca_square_of_protection_plated_armor_pays_out_the_same_turn() {
        let mut c = lock("donu_and_deca");
        c.asc = 19;
        let deca = idx_of(&c, "deca");
        let square = c.enemies[deca]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Square of Protection")
            .unwrap();
        c.enemies[deca].next_move = square;
        c.end_turn();
        assert_eq!(c.enemies[deca].statuses.get(Status::PlatedArmor), 3);
        assert_eq!(c.enemies[deca].block, 19, "16 团队格挡 + 当回合挂上的 3 点板甲");
        c.enemies[deca].next_move = square;
        c.end_turn();
        assert_eq!(c.enemies[deca].statuses.get(Status::PlatedArmor), 6);
        assert_eq!(c.enemies[deca].block, 22, "板甲是 6 层,当回合按 6 结算");
    }

    /// 祭礼是唯一的例外:原版 RitualPower 带 skipFirst,刚挂上的那一次回合末不结算.
    #[test]
    fn cultist_ritual_skips_its_first_end_of_turn() {
        let mut c = lock("cultist_solo");
        let cultist = idx_of(&c, "cultist");
        c.end_turn();
        assert_eq!(
            c.enemies[cultist].statuses.get(Status::Ritual),
            3,
            "开场咒语挂上 3 层祭礼"
        );
        assert_eq!(
            c.enemies[cultist].statuses.get(Status::Strength),
            0,
            "挂上祭礼的那个自己回合末不结算"
        );
        c.end_turn();
        assert_eq!(
            c.enemies[cultist].statuses.get(Status::Strength),
            3,
            "下一次自己回合末才给力量"
        );
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
            rested: false,
                hp: 80,
                max_hp: 80,
                deck,
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
            asc: 0,
            },
            RngRegistry::new(seed),
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
        // 尸体那一格被顶掉,新来的按槽位排进队里(开局的小鬼是哪几只随种子变)
        let slot1 = c
            .enemies
            .iter()
            .position(|e| e.slot == 1)
            .expect("槽 1 上有一只开战小鬼");
        c.enemies[slot1].hp = 0;
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

    #[test]
    fn gremlin_leader_encourage_consumes_the_quote_roll() {
        // 原版 GREMLIN_LEADER_ENCOURAGE 出手时先掷一次 aiRng.random(0, 2) 挑台词
        // (= 反编译 MonsterSpecific.cpp 里的 `bc.aiRng.random(0, 2); // for in game quote`),
        // 再上增益/格挡. 这一掷不补,后面所有 aiRng 掷点都会错位(A20 第二幕头目小鬼
        // 种子 13/4 就是这样分叉的).
        let mut c = fight("gremlin_leader_gang", 7);
        assert_eq!(c.enemies.len(), 3, "两只开战小鬼 + 首领");
        let before = c.streams.floor(FloorStream::AiRng).counter();
        use_move(&mut c, "gremlin_leader", 1); // Encourage
        let used = c.streams.floor(FloorStream::AiRng).counter() - before;
        assert_eq!(used, 2, "鼓励应掷 2 次 aiRng(台词 + 回合末选招)");
        // 顺带确认增益/格挡照常落到自己和小鬼身上
        let leader = idx_of(&c, "gremlin_leader");
        assert_eq!(c.enemies[leader].statuses.get(Status::Strength), 3, "首领 +3 力量");
        for e in c.enemies.iter().filter(|e| e.def.id != "gremlin_leader") {
            assert_eq!(e.statuses.get(Status::Strength), 3, "{} +3 力量", e.def.id);
            assert_eq!(e.block, 6, "{} +6 格挡", e.def.id);
        }
        // 对照:捅刺只掷回合末选招那一次
        let mut c2 = fight("gremlin_leader_gang", 7);
        let before2 = c2.streams.floor(FloorStream::AiRng).counter();
        use_move(&mut c2, "gremlin_leader", 2); // Stab
        let used2 = c2.streams.floor(FloorStream::AiRng).counter() - before2;
        assert_eq!(used2, 1, "捅刺只掷选招那一次");
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
        // 参考实现的数组就是 [匕首, 爬行者, 匕首](0/1/2),到召唤时才补空槽
        assert_eq!(slots(&c), vec![0, 1, 2], "匕首在 0 和 2,爬行者在 1");
        // 搜索顺序 4、1、3、0:1 是爬行者,第一把补进 4
        use_move(&mut c, "reptomancer", 0);
        assert_eq!(slots(&c), vec![0, 1, 2, 4], "第一把补进槽 4");
        use_move(&mut c, "reptomancer", 0);
        assert_eq!(slots(&c), vec![0, 1, 2, 3, 4], "下一把补进槽 3");
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
        let parent = c.enemies[0].def.id;
        let medium = match parent {
            "acid_slime_large" => "acid_slime_medium",
            "spike_slime_large" => "spike_slime_medium",
            other => panic!("大史莱姆不该是 {other}"),
        };
        c.enemies[0].hp = 30;
        c.damage_enemy(0, 1);
        c.end_turn();
        assert_eq!(ids(&c), vec![medium, medium]);
        assert_eq!(slots(&c), vec![0, 1], "两只子体占原来那一格和下一格");
    }
}


/// 本批新实现的遗物:战斗侧的钩子各来一条真实断言
#[cfg(test)]
mod relic_hook_tests {
    use super::*;
    use crate::core::cards;
    use crate::core::relics::{relic_def_or_panic, RelicDef, RelicFx, RelicTier};

    /// 测试用遗物:把"打出即消耗"100% 改成进弃牌堆的奇异勺
    static SPOON_ALL: RelicDef = RelicDef {
        id: "test_spoon_all",
        name: "Test Spoon",
        desc: "test only",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            exhaust_to_discard_pct: 100,
            ..RelicFx::ZERO
        },
        note: "",
    };

    /// 一场可摆布的战斗:手牌/抽牌堆按参数摆好,遗物挂上,能量 9
    fn staged(deck: &[&str], hand: &[&str], relics: &[&'static RelicDef]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: deck.iter().map(|id| cards::card(id)).collect(),
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
        };
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").expect("jaw worm");
        let mut c = Combat::new(enc, setup, RngRegistry::new(21));
        c.hand.clear();
        c.draw.clear();
        c.discard.clear();
        c.exhaust.clear();
        for id in hand {
            c.hand.push(cards::card(id));
        }
        for id in deck {
            c.draw.push(cards::card(id));
        }
        c.energy = 9;
        c
    }

    /// 瓶装火焰:被封装的牌开局就在手里
    #[test]
    fn bottled_card_is_in_the_opening_hand() {
        let relics = vec![relic_def_or_panic("bottled_flame")];
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: {
                let mut deck: Vec<CardInstance> = (0..10).map(|_| cards::card("strike")).collect();
                let mut bottled = cards::card("bash");
                bottled.bottled = true;
                deck.push(bottled);
                deck
            },
            relics,
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
        };
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").expect("jaw worm");
        let c = Combat::new(enc, setup, RngRegistry::new(5));
        assert_eq!(c.hand.len(), DRAW_PER_TURN, "开局抽五张");
        assert!(
            c.hand.iter().any(|x| x.def.id == "bash"),
            "封装的牌开局就在手里"
        );
        assert_eq!(c.hand[0].def.id, "bash", "压在抽牌堆顶,第一个抽到");
    }

    /// 工具箱:第一回合亮出三张无色牌,挑中的进手
    #[test]
    fn toolbox_offers_three_colorless_cards() {
        let relics = vec![relic_def_or_panic("toolbox")];
        let mut c = staged(&["strike"; 10], &[], &relics);
        let ch = c.choice.as_ref().expect("工具箱要亮牌");
        assert_eq!(ch.source, ChoiceSource::Offered);
        let cands = c.choice_candidates();
        assert_eq!(cands.len(), 3, "亮三张");
        assert!(
            cands.iter().all(|(_, k)| cards::pool_of(k.def) == "colorless"),
            "只给无色牌"
        );
        let picked = cands[0].1.def.id;
        c.choose(0).unwrap();
        assert!(c.choice.is_none());
        assert!(c.hand.iter().any(|k| k.def.id == picked), "挑中的进手");
    }

    /// 赌徒筹码:开局亮一个"弃任意张再抽等量张"的选择,弃一张补一张
    #[test]
    fn gambling_chip_discards_then_draws() {
        let relics = vec![relic_def_or_panic("gambling_chip")];
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: (0..10).map(|_| cards::card("strike")).collect(),
            relics,
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").expect("jaw worm");
        let mut c = Combat::new(enc, setup, RngRegistry::new(21));
        let ch = c.choice.as_ref().expect("赌徒筹码开局亮牌");
        assert_eq!(ch.source, ChoiceSource::Hand);
        assert_eq!(ch.action, ChoiceAction::Discard);
        assert!(ch.draw_after, "弃完要补抽");
        let hand_before = c.hand.len();
        let idx = c.choice_candidates()[0].0;
        c.choose(idx).unwrap();
        c.finish_choice();
        assert_eq!(c.hand.len(), hand_before, "弃一张补一张");
    }

    /// 尼尔瑞的抄本:回合结束亮三张,挑中的洗进抽牌堆,选完才轮到对面
    #[test]
    fn nilrys_codex_shuffles_a_chosen_card_into_the_draw_pile() {
        let relics = vec![relic_def_or_panic("nilrys_codex")];
        let mut c = staged(&["strike"; 10], &["defend"], &relics);
        c.end_turn();
        let ch = c.choice.as_ref().expect("回合结束要亮牌");
        assert_eq!(ch.source, ChoiceSource::Offered);
        assert_eq!(ch.action, ChoiceAction::ToDrawShuffled);
        assert_eq!(c.choice_candidates().len(), 3, "亮三张");
        let picked = c.choice_candidates()[0].1.def.id;
        c.choose(0).unwrap();
        assert!(c.choice.is_none());
        assert!(c.pending_end_turn == false, "选完接着走回合尾巴");
        assert!(
            c.draw.iter().chain(c.hand.iter()).any(|k| k.def.id == picked),
            "挑中的洗进抽牌堆(洗完后被抽到手上也算)"
        );
        assert!(
            !c.discard.iter().any(|k| k.def.id == picked),
            "挑中的不该落到弃牌堆"
        );
        assert_eq!(c.turn, 2, "回合已经交给对面并回到自己");
    }

    /// 抄本可以跳过:不选就不会往抽牌堆塞牌
    #[test]
    fn nilrys_codex_can_be_skipped() {
        let relics = vec![relic_def_or_panic("nilrys_codex")];
        let mut c = staged(&["strike"; 10], &["defend"], &relics);
        c.end_turn();
        assert!(c.choice.is_some());
        let before = c.draw.len();
        c.cancel_choice();
        assert!(c.choice.is_none());
        // 对面行动后自己抽了开局五张,抽牌堆只少这五张(没有多的牌被塞进来)
        assert_eq!(c.draw.len(), before - DRAW_PER_TURN, "跳过就不塞牌");
        assert_eq!(c.turn, 2, "跳过也要把回合交出去");
    }

    /// 冠军腰带:给敌人上易伤时附带 1 层虚弱
    #[test]
    fn champion_belt_adds_weak_when_vulnerable_lands() {
        let relics = vec![relic_def_or_panic("champion_belt")];
        let mut c = staged(&["bash"; 5], &["bash"], &relics);
        c.enemies[0].hp = 999;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 2);
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 1, "附带虚弱");
    }

    /// 蜥蜴尾巴:每场一次,致命伤改成回一半最大生命
    #[test]
    fn lizard_tail_saves_you_once_per_combat() {
        let relics = vec![relic_def_or_panic("lizard_tail")];
        let mut c = staged(&["strike"; 10], &[], &relics);
        c.player.hp = 4;
        c.hit_player(999);
        assert_eq!(c.player.hp, 40, "回到最大生命的一半");
        assert_ne!(c.phase, Phase::Lost, "没死");
        c.player.hp = 4;
        c.hit_player(999);
        assert_eq!(c.phase, Phase::Lost, "同一场只有一次");
    }

    /// 蓝蜡烛:不可打出的诅咒可以打出,打出掉 1 血并消耗
    #[test]
    fn blue_candle_makes_curses_playable_at_a_price() {
        let relics = vec![relic_def_or_panic("blue_candle")];
        let mut c = staged(&["injury"; 5], &["injury"], &relics);
        assert!(c.playable(0).is_ok(), "有蓝蜡烛就能打诅咒");
        let hp = c.player.hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.hp, hp - 1, "打出诅咒掉 1 血");
        assert_eq!(c.exhaust.len(), 1, "打完消耗");
        assert!(c.discard.is_empty());
    }

    /// 医疗包:不可打出的状态牌可以打出,打出后消耗且不掉血
    #[test]
    fn medical_kit_makes_status_cards_playable_and_exhausts_them() {
        let relics = vec![relic_def_or_panic("medical_kit")];
        let mut c = staged(&["wound"; 5], &["wound"], &relics);
        assert!(c.playable(0).is_ok(), "有医疗包就能打状态牌");
        let hp = c.player.hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.hp, hp, "状态牌不掉血");
        assert_eq!(c.exhaust.len(), 1, "打完消耗");
    }

    /// 没有遗物时,诅咒与状态牌还是打不出去
    #[test]
    fn curses_and_statuses_stay_unplayable_without_the_relics() {
        for id in ["injury", "wound"] {
            let c = staged(&[id; 5], &[id], &[]);
            assert!(c.playable(0).is_err(), "{id} 不该能打出去");
        }
    }

    /// 奇异勺:打出即消耗的牌改为进弃牌堆(每张各掷一次)
    #[test]
    fn strange_spoon_redirects_exhausting_cards() {
        // 100% 版本:全都进弃牌堆
        let mut c = staged(&["slimed"; 6], &["slimed"; 6], &[&SPOON_ALL]);
        for _ in 0..6 {
            c.play_card(0, None).unwrap();
        }
        assert_eq!(c.discard.len(), 6, "全都改进弃牌堆");
        assert!(c.exhaust.is_empty());

        // 真实奇异勺是 50%:六张里两边都该有
        let spoon = relic_def_or_panic("strange_spoon");
        let mut c = staged(&["slimed"; 6], &["slimed"; 6], &[spoon]);
        for _ in 0..6 {
            c.play_card(0, None).unwrap();
        }
        assert_eq!(c.discard.len() + c.exhaust.len(), 6, "六张都有去处");
        assert!(!c.discard.is_empty() && !c.exhaust.is_empty(), "两边都该有");
    }

    /// 化学 X:X 费牌的 X 加 2(能量照常全花)
    #[test]
    fn chemical_x_adds_two_to_x_cost_cards() {
        let relics = vec![relic_def_or_panic("chemical_x")];
        let mut c = staged(&["whirlwind"; 5], &["whirlwind"], &relics);
        c.enemies[0].hp = 999;
        c.energy = 3;
        let before = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        // 旋风每次 5 点,X = 3 + 2 = 5 次
        assert_eq!(before - c.enemies[0].hp, 25);
        assert_eq!(c.energy, 0, "能量还是按 X 全花掉");
    }

    /// 手钻:这一击打碎格挡时上 2 层易伤
    #[test]
    fn hand_drill_applies_vulnerable_when_block_breaks() {
        let relics = vec![relic_def_or_panic("hand_drill")];
        let mut c = staged(&["strike"; 5], &["strike"], &relics);
        c.enemies[0].hp = 999;
        // 打击 6 点,格挡正好 6:打碎但没掉血
        c.enemies[0].block = 6;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].block, 0);
        assert_eq!(c.enemies[0].hp, 999, "全被格挡吃掉");
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 2);

        // 没打碎就不上易伤(场上已有易伤,这一击 6 -> 9)
        c.enemies[0].block = 20;
        c.hand.push(cards::card("strike"));
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].block, 11);
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 2, "不再加");
    }

    /// 神圣树皮:药水数值翻倍(火焰药剂 20 -> 40)
    #[test]
    fn sacred_bark_doubles_potion_effects() {
        let potion = crate::core::potions::by_id("fire_potion").expect("火焰药剂");
        let plain = staged(&["strike"; 10], &[], &[]);
        let mut plain = plain;
        plain.enemies[0].hp = 999;
        plain.use_potion(potion, Some(0));
        assert_eq!(999 - plain.enemies[0].hp, 20, "本来 20 点");

        let mut c = staged(&["strike"; 10], &[], &[relic_def_or_panic("sacred_bark")]);
        c.enemies[0].hp = 999;
        c.use_potion(potion, Some(0));
        assert_eq!(999 - c.enemies[0].hp, 40, "神圣树皮翻倍");
    }
}
