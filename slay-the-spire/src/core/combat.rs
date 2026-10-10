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

/// 选牌的强制程度:决定"候选只剩 1 张"时开不开屏(见 begin_choice 的自动结算).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChoiceMode {
    /// 强制:必须选够 need 张.原版这类选牌候选只剩 1 张时直接替玩家结算、不开屏.
    /// 依据 refs/sts_lightspeed/src/combat/Actions.cpp:HeadbuttAction:811(自动分支 :816)、
    /// ChooseExhaustOne:824(:830,坚毅/燃烧契约)、DrawToHandAction:839(:860,秘技/秘密武器)、
    /// WarcryAction:872(:878)、ExhumeAction:755(:772)、ArmamentsAction:678(:687)、
    /// DualWieldAction:701(:725)、BetterDiscardPileToHandAction:664(:669,液体记忆)、
    /// ForethoughtAction:784(:803,基础版预谋);
    /// 对拍基准 slay-the-cli/cards/*/effects.ts 的 chooseOne 对 0/1 候选同样 auto-resolve.
    Mandatory,
    /// 可选:可以少选甚至一张不选就收工.原版这类选牌**永远开屏**,哪怕只剩 1 张候选
    /// (净化/预谋+/赌徒筹码/灵药/赌徒之酿).
    /// 依据 Actions.cpp:ExhaustMany:973(净化,无条件进 CARD_SELECT)、
    /// GambleAction:981(赌徒筹码)、ToolboxAction:988、DiscoveryAction:564;
    /// 对拍基准里 min=0 的请求(purity/elixir/gamblersBrew/gamblingChip)也都不 auto.
    Optional,
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
    /// 强制还是可选.强制单选在候选只剩 1 张时由 begin_choice 直接结算、不开屏
    pub mode: ChoiceMode,
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
    /// 这张牌是被"打抽牌堆顶"(浩劫/混沌药剂)放出来的:收尾时无条件消耗
    /// (power 退场),取消也不回手牌 —— 它不是从手牌里出去的
    pub exhaust_after: bool,
}

/// 开一次选牌窗的参数.默认是"强制单选一张"(最常见的形态);带选牌的牌在
/// begin_choice 之前把份数/免费/亮牌这些附加项挂上,自动结算时才不会丢掉它们.
struct ChoiceSpec {
    source: ChoiceSource,
    action: ChoiceAction,
    filter: ChoiceFilter,
    /// 最多选几张;0 表示不限张数
    need: usize,
    mode: ChoiceMode,
    label: String,
    /// 选中的那份给几张(双持升级版/神圣树皮)
    copies: usize,
    /// 选中的牌本回合 0 费(发现类)
    free: bool,
    /// 选完之后抽等量张(赌徒筹码/赌徒之酿)
    draw_after: bool,
    /// ChoiceSource::Offered 时亮出来的候选(发现/工具箱/抄本)
    offered: Vec<CardInstance>,
}

impl ChoiceSpec {
    /// 最常见的构造:强制单选一张
    fn mandatory(
        source: ChoiceSource,
        action: ChoiceAction,
        filter: ChoiceFilter,
        label: &str,
    ) -> ChoiceSpec {
        ChoiceSpec::new(source, action, filter, 1, ChoiceMode::Mandatory, label)
    }

    fn new(
        source: ChoiceSource,
        action: ChoiceAction,
        filter: ChoiceFilter,
        need: usize,
        mode: ChoiceMode,
        label: &str,
    ) -> ChoiceSpec {
        ChoiceSpec {
            source,
            action,
            filter,
            need,
            mode,
            label: label.to_string(),
            copies: 1,
            free: false,
            draw_after: false,
            offered: Vec::new(),
        }
    }

    fn copies(mut self, n: usize) -> ChoiceSpec {
        self.copies = n.max(1);
        self
    }

    fn free(mut self, yes: bool) -> ChoiceSpec {
        self.free = yes;
        self
    }

    fn draw_after(mut self) -> ChoiceSpec {
        self.draw_after = true;
        self
    }

    fn offered(mut self, cards: Vec<CardInstance>) -> ChoiceSpec {
        self.offered = cards;
        self
    }
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
    /// 延迟结算的"打出抽牌堆顶"(浩劫顶牌).原版打出浩劫时:先把顶牌从抽牌堆摘下来
    /// 交给卡牌队列(抽牌堆空了就在这一步先重洗弃牌堆,此时浩劫还没进弃牌堆),而浩劫
    /// 自己进弃牌堆排在动作队列里,先于卡牌队列清空 —— 所以顶牌真正结算时浩劫已经在
    /// 弃牌堆(顶牌自己的"抽 1"再触发重洗时就会把浩劫一起洗进去).见反编译
    /// BattleContext.cpp 的 useCard / onAfterUseCard 与主循环(actionQueue 先于 cardQueue).
    /// 这里照同一顺序:打牌时先摘顶牌存进这条队列,等本张牌收尾之后再结算.
    /// 每项是(浩劫链层级, 顶牌, 打完是否消耗, 摘牌时掷好的随机目标).
    /// 目标必须在这里掷(与参考实现的 PlayTopCardAction 同一时刻):原版/参考都是把
    /// PlayTopCard 动作排在"打出的那张牌"之后、"诅咒之眼(Hex)塞眩晕"之前执行,
    /// 掷点次序是"先顶牌目标、后 Hex 位置" —— 拖到真正结算顶牌时才掷,两次
    /// cardRandomRng 的次序就反了(见 tools/e2e_diff.ts a20a2 seed 3 的鸟+被选中者战).
    pending_top_plays: Vec<(u8, CardInstance, bool, Option<usize>)>,
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
    /// headless 对拍:参考实现没实现尼尔瑞的抄本
    /// (refs/slay-the-cli/src/content/relics/event.ts:146-153 标了 ENGINE-GAP:
    /// "回合中段请求选牌会把排在其后的动作(交给回合)整段丢掉",hooks:{}),
    /// 连"亮三张"那几次 cardRandomRng 都不掷.反编译其实有完整实现
    /// (refs/sts_lightspeed/src/combat/BattleContext.cpp:2046-2047 挂 CodexAction、
    /// Actions.cpp:964-970 开 CODEX 选牌、BattleContext.cpp:2929-2932 处理选择),
    /// 是参考实现这侧缺一块.原版这件遗物是可选(shuffle 或不 shuffle),
    /// 驱动侧统一按"跳过"处理:回合结束不亮牌、不掷点,两边才对得上.
    pub suppress_codex: bool,
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
                    let hp = crate::core::ascension::roll_hp(
                        streams.floor(FloorStream::MonsterHpRng),
                        def,
                        asc,
                    );
                    Spawned { id, hp, rolled: None }
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
                // 飞升换档的预置值整组覆盖基础值(颚虫三连的力量/格挡;
                // 反编译 MonsterGroup.cpp:278-279,见 ascension::PRESET_ASC)
                let (preset_statuses, preset_block) =
                    match crate::core::ascension::preset_asc(enc.id, asc) {
                        Some((st, blk)) => (st, blk),
                        None => (p.statuses, p.block),
                    };
                for (s, n) in preset_statuses {
                    statuses.add(*s, *n);
                }
                block += preset_block;
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
            pending_top_plays: Vec::new(),
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
            suppress_codex: false,
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
            // 勇气投石索只在精英战给力量,首领战不给(反编译 BattleContext.cpp:340
            // `if (room == Room::ELITE) p.buff<PS::STRENGTH>(2)`);此前误与奴隶主颈圈
            // 一样按精英/首领都算,而沙盒只有精英与普通场景,漏了首领这一半.
            if fx.combat_start_strength_elite != 0 && enc.kind == EnemyKind::Elite {
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
        // 扭曲的钳子:反编译 Actions::UpgradeRandomCardAction
        // (refs/sts_lightspeed/src/combat/Actions.cpp:940-962),在**抽牌后**的
        // applyStartOfTurnPostDrawRelics(Player.cpp:669-671)里:
        //   ①先在手里挑出"还能升级"的牌(canUpgrade,941-947);
        //   ②一张都没有就整段跳过、**不掷点**(949-951);
        //   ③否则从 shuffleRng 取一个 long 做种,喂 java.util.Random 洗这份手牌下标表
        //     (953-957),升级洗后的第一张。
        // 参考实现把 WARPED_TONGS 挂在 atStartOfTurn(抽牌前,event.ts:237-249),那一刻
        // 手里还没有牌、挑不出候选,于是它永远不升级(见 sandbox 的 warped_tongs/play)。
        // 本作照反编译:动 shuffleRng、不动 miscRng;候选只含能升级的牌。
        if self.relic_any(|fx| fx.upgrade_random_hand_at_turn_start) {
            let cands: Vec<usize> = self
                .hand
                .iter()
                .enumerate()
                .filter(|(_, k)| k.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if !cands.is_empty() {
                let mut cands = cands;
                let seed = self.streams.floor(FloorStream::ShuffleRng).random_long();
                java_shuffle(&mut cands, &mut JavaRandom::new(seed));
                let idx = cands[0];
                if self.hand[idx].upgrade() {
                    let label = self.hand[idx].label();
                    self.push_log(LogKind::Player, format!("Warped Tongs upgrades {label}"));
                }
            }
        }
        // 魔法书:战斗开始时往手里塞一张随机能力牌,本回合 0 费(只在开局,不是每回合)
        if self.turn == 1 && self.relic_any(|fx| fx.add_random_power_card) {
            self.add_random_power_to_hand();
        }
        // 赌徒筹码:开局弃任意张再抽等量张(不限张数,永远开屏)
        if self.turn == 1 && self.relic_any(|fx| fx.gambling_chip) && !self.hand.is_empty() {
            self.begin_choice(
                ChoiceSpec::new(
                    ChoiceSource::Hand,
                    ChoiceAction::Discard,
                    ChoiceFilter::Any,
                    0,
                    ChoiceMode::Optional,
                    "Gambling Chip",
                )
                .draw_after(),
            );
        }
        let brutal = self.player.statuses.get(Status::Brutality);
        if brutal > 0 {
            // 掉血来自能力而不是卡牌,所以不触发渴望
            self.lose_hp_player(1, false);
            self.draw_cards(brutal as usize);
        }
        // 混乱:回合开始打出抽牌堆顶那张(打完按它自己的规矩去弃牌堆/消耗堆)
        if self.player.statuses.has(Status::Mayhem) {
            self.play_top_of_draw_now(false, "Mayhem");
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
            self.offer_pick(
                pool,
                toolbox as usize,
                ChoiceAction::ToHand,
                false,
                1,
                "Toolbox: choose 1",
            );
        }
    }

    /// 随机无色牌进手牌;free 表示本回合 0 费,upgraded 表示直接给升级版
    fn add_random_colorless_to_hand(&mut self, free: bool, upgraded: bool) {
        let mut pool = cards::colorless_pool();
        if pool.is_empty() {
            return;
        }
        // 战斗内随机抽牌这一路要和参考实现同口径:池子按 id 排序
        // (slay-the-cli colorless/effects.ts:41-56 的 colorlessPool 明确 sort by id 作
        // ENGINE-NOTE;原版真正的牌库顺序在反编译里是打散的 Java HashMap 序
        // ——CardPools.h:189-196 的 CombatColorlessCardPool,34 张、还漏了 BANDAGE_UP,
        // 无从复现,详见 tools/sandbox_diff.ts 顶部随机池注释).
        // 炼狱之刃(combat.rs AddRandomAttackToHand)与药水以外的这几处此前漏了排序.
        pool.sort_by_key(|c| c.id);
        // 掷点一定要先做:原版是 getTrulyRandomColorlessCardInCombat 先把牌抽出来,
        // 再走 MakeTempCardInHand;手牌满只是"塞不进手",不省这一掷.
        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
        let mut inst = CardInstance::new(def);
        if upgraded {
            inst.upgrade_forced();
        }
        self.fix_new_card(&mut inst);
        inst.free_this_turn = free;
        let label = inst.label();
        // 手牌到上限就进弃牌堆(参考实现 makeTempCard:hand overflow goes to discard),
        // 与本作其它"战斗中新造的牌"同一口径,不是把这张牌直接丢掉.
        self.add_created_card_to_hand(inst);
        self.push_log(LogKind::Player, format!("{label} appears"));
    }

    /// 原版 PlayTopCard 动作的前半:抽牌堆空了先把弃牌堆洗回来(此时本张牌还没进
    /// 弃牌堆),再摘走顶牌.返回摘下的那张(没得打返回 None).
    fn take_top_card_for_play(&mut self, via: &str) -> Option<CardInstance> {
        if self.draw.is_empty() && !self.reshuffle_discard_into_draw() {
            return None;
        }
        let card = self.draw.remove(0);
        self.push_log(LogKind::Player, format!("{via} plays {}", card.label()));
        Some(card)
    }

    /// 原版 PlayTopCard 动作的后半:结算摘下来的那张牌(exhaust_after 为真时打完
    /// 直接消耗,浩劫就是),depth 是浩劫链层级,target 是摘牌那一刻掷好的随机目标.
    fn play_top_card(
        &mut self,
        mut card: CardInstance,
        exhaust_after: bool,
        depth: u8,
        target: Option<usize>,
    ) {
        let kind = card.kind();
        // 记进浩劫链(层级 +1 表示嵌了一层),表现层据此叠播报
        let saved_depth = self.havoc_depth;
        self.havoc_depth = depth;
        self.havoc_chain.push((depth, card.label()));
        self.snapshot_sharp_hide();
        let mut top_ctx = PlayCtx::default();
        self.resolve(&mut card, target, &mut top_ctx);
        self.havoc_depth = saved_depth;
        card.free_this_turn = false;
        // 这张顶牌挂着选牌(浩劫放出来的燃烧契约/头槌之类):牌的去处与"打出过"
        // 记账都等选完再收尾,走与 play_card 同一条路 —— 否则选牌之后那一截效果
        // (燃烧契约的"抽 2")会因为 close_choice 找不到 played 而整段丢掉.
        if self.choice.is_some() {
            if let Some(ch) = self.choice.as_mut() {
                ch.played = Some((card, 0));
                ch.exhaust_after = exhaust_after;
            }
            self.check_win();
            return;
        }
        if kind == crate::core::card::CardType::Power {
            // 能力牌一样是打完就退场(浩劫放出来的也不例外)
            self.vanish_card(card);
        } else if exhaust_after || card.is_exhaust() {
            self.exhaust_card(card);
        } else {
            self.discard.push(card);
        }
        self.note_card_played(kind);
        self.check_win();
        // 这张顶牌自己也可能又压了一次"打顶牌"(浩劫打浩劫),接着跑
        self.drain_pending_top_plays();
    }

    /// 立刻打抽牌堆顶:没有"本张牌"要收尾的场合(混乱/混沌药剂)走这条.
    fn play_top_of_draw_now(&mut self, exhaust_after: bool, via: &str) {
        if let Some(card) = self.take_top_card_for_play(via) {
            let target = self.pick_random_alive();
            self.play_top_card(card, exhaust_after, 1, target);
        }
    }

    /// 把延迟的"打出抽牌堆顶"跑掉.调用点都在"当前打出的那张牌已经收尾"之后 ——
    /// 顺序与原版一致(浩劫先进弃牌堆,再结算摘下来的顶牌).
    fn drain_pending_top_plays(&mut self) {
        while !self.pending_top_plays.is_empty() {
            let (depth, card, exhaust, target) = self.pending_top_plays.remove(0);
            self.play_top_card(card, exhaust, depth, target);
        }
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
            // 腐化之下技能本回合已是 0 费,不能参选(原版腐化会把技能的
            // costForTurn 也改成 0,候选要求 costForTurn>0)
            let corruption = self.player.statuses.has(Status::Corruption);
            let candidates: Vec<usize> = (0..self.hand.len())
                .filter(|&i| {
                    let c = &self.hand[i];
                    let already_free =
                        corruption && c.kind() == crate::core::card::CardType::Skill;
                    !already_free && c.fixed_cost().unwrap_or(0) > 0
                })
                .collect();
            if !candidates.is_empty() {
                let idx = candidates
                    [self.streams.floor(FloorStream::CardRandomRng).random(candidates.len() as u32 - 1) as usize];
                self.hand[idx].free_this_turn = true;
                let label = self.hand[idx].label();
                self.push_log(LogKind::Player, format!("Mummified Hand: {label} costs 0"));
            }
        }
        // 橙皮:三种类型都打出过就清掉自己的减益(清的范围见 remove_player_debuffs:
        // 还要把 Flex/敏捷药水残留的 LoseStrength/LoseDexterity、紧急按钮的 NoBlock
        // 一起清掉,负力量/敏捷归零)
        if self.relic_any(|fx| fx.clear_debuffs_on_all_types) && self.rs.types_played == 7 {
            self.player.statuses.remove_player_debuffs();
            self.push_log(LogKind::Player, "Orange Pellets clears your debuffs".to_string());
            self.rs.types_played = 0;
        }
        // 不休陀螺:手里空了就补一张.这一回合已经被时间扭曲掐掉时(force_end_turn)
        // 不补:反编译的主循环里 Unceasing Top 的检查排在"这一回合已经排队结束"
        // 分支之后(refs/sts_lightspeed/src/combat/BattleContext.cpp:802-815 的
        // endTurnQueued 分支先 continue,assert(!endTurnQueued) 就守着这一步),
        // 回合既然结束就不再补牌.
        if self.hand.is_empty() && !self.force_end_turn && self.relic_any(|fx| fx.draw_on_empty_hand) {
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
        // 诅咒之眼:打非攻击牌就往抽牌堆塞眩晕.这一条挂在玩家身上(怪物的 HEX 招
        // 用 PlayerStatus 挂上来),所以只看玩家那一份,一张牌只触发一次.
        let hex = self.player.statuses.get(Status::Hex);
        if hex > 0 && kind != CardType::Attack {
            for _ in 0..hex {
                let mut inst = CardInstance::new(cards::card_def_or_panic("dazed"));
                self.fix_new_card(&mut inst);
                let pos = self.streams.floor(FloorStream::CardRandomRng).below(self.draw.len() as u32 + 1) as usize;
                self.draw.insert(pos, inst);
            }
            self.push_log(
                LogKind::Enemy,
                format!("Hex shuffles {hex} Dazed into your draw pile"),
            );
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
                // 基线要取这张牌"当前"的费用(升级后的),不是牌面基础费用:
                // 反编译 CardManager::draw 是把掷出来的值直接写进 cost/costForTurn,
                // 升级降费的牌(havoc+ 这种基础 1 升级后 0)也得是掷出的那个值,
                // 拿基础费用当基线会凭空少 1.
                let base = match self.hand[i].cost() {
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
        use crate::core::card::CardType;
        // 吐火:抽到状态牌或诅咒牌都打全体(反编译 CardManager::draw 的两个分支各自
        // 挂了一次 DamageAllEnemy).只认状态牌会漏掉诅咒 —— 原版诅咒一样触发.
        let fires_on = matches!(card.kind(), CardType::Status | CardType::Curse);
        let fire = self.player.statuses.get(Status::FireBreathing);
        if fire > 0 && fires_on {
            for i in self.alive_enemies() {
                self.damage_enemy_plain(i, fire);
            }
            self.push_log(
                LogKind::Player,
                format!("Fire Breathing deals {fire} to all enemies"),
            );
            self.settle_deaths();
        }
        // 进化:抽到状态牌再抽 N 张(反编译里这次抽牌排在吐火伤害之后结算)
        if card.kind() == CardType::Status {
            let evolve = self.player.statuses.get(Status::Evolve);
            if evolve > 0 {
                // 递归深度受手牌上限约束
                self.draw_cards(evolve as usize);
            }
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
        // 枯枝:消耗时往手里塞一张随机牌(不是本场战斗里的牌;职业池任意稀有度).
        // 位置必须排在黑暗拥抱的抽牌**之前**:反编译把 MakeTempCardInHand 先入队、
        // DrawCards 后入队(refs/sts_lightspeed/src/combat/BattleContext.cpp:2822-2830),
        // 参考实现里枯枝是当场造牌、黑暗拥抱的抽牌才进队列
        // (refs/slay-the-cli/src/content/relics/rare.ts:96-106 + powers/ironclad.ts:57-60).
        // 手牌快满时先后决定了落点:先造的这一张进手牌,后来的抽牌才因为满手抽不动.
        let branch = self.relic_sum(|fx| fx.card_on_exhaust);
        if branch > 0 {
            self.add_random_class_card_to_hand();
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
    /// (枯枝).手牌满了也照样掷点、照样造牌,只是落点改成弃牌堆 —— 反编译的
    /// moveToHandHelper 就是这个规则(refs/sts_lightspeed/src/combat/BattleContext.cpp:2531-2540),
    /// 参考实现的 makeTempCard 同样是"hand overflow goes to discard"
    /// (refs/slay-the-cli/src/engine/combat/interpreter.ts:399-402).此前满手时在掷点前
    /// 就 return,既吞掉一次 cardRandomRng 掷点,又把那张牌整张丢了.
    fn add_random_class_card_to_hand(&mut self) {
        let pool = crate::core::cards::class_card_pool();
        if pool.is_empty() {
            return;
        }
        let def = *self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
        let mut inst = CardInstance::new(def);
        self.fix_new_card(&mut inst);
        let label = inst.label();
        self.add_created_card_to_hand(inst);
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
        // 尼尔瑞的抄本:回合结束亮出几张随机牌,挑一张洗进抽牌堆(可以跳过)。
        // headless 对拍时整件当不存在(见 Combat::suppress_codex 的注释).
        let codex = self.relic_max(|fx| fx.end_turn_shuffle_pick);
        if codex > 0 && !self.rs.nilrys_used && !self.suppress_codex {
            self.rs.nilrys_used = true;
            let pool = cards::class_card_pool();
            self.offer_pick(
                pool,
                codex as usize,
                ChoiceAction::ToDrawShuffled,
                false,
                1,
                "Nilry's Codex: choose 1",
            );
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
                        inst.upgrade_forced();
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
                // 参考实现:把玩家各堆里的灼伤就地升级(已经升过的不动).
                // 走 upgrade_forced:诅咒/状态牌被 can_upgrade 挡着,只有炼狱这条专路能升灼伤.
                for pile in [
                    &mut self.hand,
                    &mut self.draw,
                    &mut self.discard,
                    &mut self.exhaust,
                    &mut self.deck_cards,
                ] {
                    for c in pile.iter_mut() {
                        if c.def.id == "burn" {
                            c.upgrade_forced();
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
            EnemyFx::Summon { ids, slots, hp_burn } => {
                let me = self.enemies[idx].slot;
                for (slot, id) in self.open_slots(me, slots, ids.len()).into_iter().zip(ids) {
                    // 原版这两处召唤会多掷一次血量:火炬头是 construct 之后又 initHp
                    // (第二次获胜),铜球的 construct 自带一次废掷.掷点值不用,但少掷
                    // 一次会让 monsterHpRng 错位,后面每只怪的血量都跟着变.
                    for _ in 0..hp_burn {
                        let def = crate::core::enemies::enemy_def_or_panic(id);
                        let (lo, hi) = crate::core::ascension::hp_range(def, self.asc);
                        self.streams.floor(FloorStream::MonsterHpRng).random_range(lo, hi);
                    }
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
                // 拉格瓦林的自然醒(回合数到点):原版两个醒法(挨够伤害 / 满 3 回合)都会
                // 掉光 Metallicize;wiki 与语料 monsters-act1.json conflicts:883-886 都这么记
                // ("lightspeed simplification; wiki: removed both wake paths").
                // 反编译只在挨打/掉血那条路 decrement(Monster.cpp:388-391 / 448-451),
                // SLEEP@turn==2 的自然醒(MonsterSpecific.cpp:888-895)只 setMove(ATTACK)、
                // 不清 Metallicize —— 是反编译省了一块,故这里按 wiki/corpus.
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
            EnemyFx::RearmModeShift { first } => {
                // 反编译 refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:1344-1351(双拳合击)先把 miscInfo 抬 10,
                // 再以它重装 MODE_SHIFT;miscInfo 开局是 Special::ModeShift 的 d(随飞升
                // 变 30/35/40),所以第 n 次装回去的额度是 d + 10n.
                let def = self.enemies[idx].def;
                let d = match def.special {
                    Special::ModeShift { d, .. } => d,
                    _ => 30,
                };
                let base = crate::core::ascension::innate_amount(
                    def.id,
                    Status::ModeShift,
                    d,
                    self.enemies[idx].asc,
                );
                let armed = if first > 0 { first } else { base + 10 };
                let last = self.enemies[idx].state.mode_shift_base;
                let next = if last > 0 { last + 10 } else { armed };
                self.enemies[idx].state.mode_shift_base = next;
                self.enemies[idx].statuses.set(Status::ModeShift, next);
                self.push_log(
                    LogKind::Enemy,
                    format!("{name} re-arms mode shift at {next} ({mname})"),
                );
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
        // 玩家的荆棘反伤(遗物 + 液态青铜给的荆棘 + 火焰屏障):多段攻击是多次
        // attacked,每段各反一次(反编译 Player::attacked 每次调用都反,且在掉血之前)
        let thorns = self.relic_thorns
            + self.player.statuses.get(Status::Thorns)
            + self.player.statuses.get(Status::FlameBarrier);
        let mut blocked_total = 0;
        let mut hit_total = 0;
        for _ in 0..times.max(1) {
            let (taken, blocked) = self.hit_player_attack(per);
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
            if thorns > 0 && self.enemies[idx].alive() {
                self.damage_enemy_plain(idx, thorns);
                self.push_log(
                    LogKind::Player,
                    format!("Thorns deal {thorns} to {name}"),
                );
                self.settle_deaths();
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
            Scope::Leader => (0..self.enemies.len())
                .filter(|i| {
                    *i != idx
                        && self.enemies[*i].alive()
                        && (self.enemies[*i].statuses.holds(Status::MinionLeader)
                            || matches!(self.enemies[*i].def.special, Special::Leader))
                })
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

    /// 圆球者的停滞:从抽牌堆(空了改从弃牌堆)偷一张牌,它死了再还回来.
    /// 选牌不是均匀随机:反编译 MonsterSpecific.cpp:3425-3459 的 stasisHelper 先按稀有度
    /// RARE > UNCOMMON > COMMON 挑出"当前堆里最高那一档"的牌,组内按固定卡序
    /// (cardSortedIdx,参考实现按卡名)稳定排序后随机取一张;堆里只剩基础/特殊这些
    /// 没档次的牌时,才在整堆里均匀随机.两处都只掷一次 cardRandomRng.
    fn enemy_steal_card(&mut self, idx: usize, name: &str) {
        let from_draw = !self.draw.is_empty();
        let pile: &Vec<CardInstance> = if from_draw { &self.draw } else { &self.discard };
        if pile.is_empty() {
            self.push_log(
                LogKind::Enemy,
                format!("{name} finds nothing to steal"),
            );
            return;
        }
        // 档位:数字越小越优先,没有档次的牌(Basic/Special/诅咒)给 None
        let rank = |c: &CardInstance| match c.def.rarity {
            Rarity::Rare => Some(0),
            Rarity::Uncommon => Some(1),
            Rarity::Common => Some(2),
            _ => None,
        };
        let pick = match pile.iter().filter_map(rank).min() {
            Some(best) => {
                let mut group: Vec<usize> = pile
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| rank(c) == Some(best))
                    .map(|(i, _)| i)
                    .collect();
                group.sort_by(|&a, &b| pile[a].def.name.cmp(pile[b].def.name));
                let k = self.streams.floor(FloorStream::CardRandomRng).below(group.len() as u32);
                group[k as usize]
            }
            None => self.streams.floor(FloorStream::CardRandomRng).below(pile.len() as u32) as usize,
        };
        let pile = if from_draw { &mut self.draw } else { &mut self.discard };
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
            crate::core::ascension::roll_hp(self.streams.floor(FloorStream::MonsterHpRng), def, asc)
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
                // 哲学家的石头:REINCARNATE 里额外 +1 力量
                // (反编译 MonsterSpecific.cpp:1472-1474;半死时力量已被
                // resetAllStatusEffects 清零,所以这里加完就是它复活后的力量)
                let stone = self.relic_sum(|fx| fx.enemy_revive_strength);
                if stone != 0 {
                    self.enemies[idx].statuses.add(Status::Strength, stone);
                    self.push_log(
                        LogKind::Enemy,
                        format!("{name} regrows with {stone} Strength"),
                    );
                }
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
        // 花开彼岸:原版 Player::heal 的第一句就是这里 return,战斗内的一切治疗
        // (血瓶/鸟面坛/玩具鸟/再生/血药水)都归零
        if self.relic_any(|fx| fx.no_heal) {
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
    }

    /// 调试用:开一个"从手牌里删牌"的选择(调试键要能看到窗口,按可选处理)
    pub fn debug_begin_hand_remove(&mut self) {
        self.begin_choice(ChoiceSpec::new(
            ChoiceSource::Hand,
            ChoiceAction::Remove,
            ChoiceFilter::Any,
            1,
            ChoiceMode::Optional,
            "remove a card from your hand",
        ));
    }

    /// 开一次选牌:记下来,等界面那边选完再 choose().
    ///
    /// 原版口径(见 ChoiceMode 与 spec 的 mode):
    /// - 候选 0 张:不开窗口.参考实现里这些 action 在堆空(或手里没有合规牌)时都是
    ///   直接 return,例如 Headbutt(弃牌堆空)、Forethought(手牌空)、
    ///   Dual Wield(手里没有攻击/能力牌)、Armaments(没有可升级牌).
    /// - 强制单选且候选 1 张:当场替玩家结算这一张、不开窗口,后续效果照常接着跑.
    ///   出处见 ChoiceMode::Mandatory 列的那些 *Action(都在
    ///   refs/sts_lightspeed/src/combat/Actions.cpp);对拍基准 slay-the-cli 的
    ///   chooseOne 对 0/1 候选也 auto.
    /// - 可选(可少选/可不选):哪怕只剩 1 张候选也开窗口(ExhaustMany、GambleAction,
    ///   以及对拍基准里 min=0 的请求).
    fn begin_choice(&mut self, spec: ChoiceSpec) {
        let ch = Choice {
            source: spec.source,
            action: spec.action,
            filter: spec.filter,
            need: spec.need,
            mode: spec.mode,
            taken: 0,
            label: spec.label,
            played: None,
            offered: spec.offered,
            draw_after: spec.draw_after,
            copies: spec.copies,
            free: spec.free,
            exhaust_after: false,
        };
        let (n, first) = {
            let cands = self.candidates_of(&ch);
            (cands.len(), cands.first().map(|(i, _)| *i))
        };
        if n == 0 {
            return;
        }
        if ch.mode == ChoiceMode::Mandatory && n == 1 {
            // 替玩家把这一张选掉:choose 走到收尾(close_choice)后 choice 仍为 None,
            // resolve_effects 便不会把后面的效果截成尾巴,浩劫顶牌那种历史上的丢尾
            // 在这个形态下根本不会出现.
            let idx = first.expect("候选 1 张必有下标");
            self.choice = Some(ch);
            let _ = self.choose(idx);
            return;
        }
        self.choice = Some(ch);
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
                // 尼尔瑞的抄本:挑中的那张"洗进"抽牌堆.
                // 反编译 refs/sts_lightspeed/src/combat/CardManager.cpp:215-221 的
                // shuffleIntoDrawPile 只掷一次 cardRandomRng 取一个插入位,把牌插进去
                // (不是整堆重洗);抽牌堆为空就直接放顶.本作原先掷的是 shuffleRng 再
                // java_shuffle 整堆,多掷一整条流、顺序也全变 —— 与反编译不符,改成一致.
                let card = ch.offered.remove(idx);
                let label = card.label();
                if self.draw.is_empty() {
                    // 空堆:原版 moveToDrawPileTop(插到堆顶);本作 draw[0] 是堆顶.
                    self.draw.insert(0, card);
                } else {
                    let n = self.draw.len();
                    // 原版下标从堆底数(drawPile.insert(begin()+idx)),本作下标从堆顶数,
                    // 且堆顶是 draw[0];换过来 idx=0(原版堆底)对应本作 index=n.
                    let insert = {
                        let idx = self
                            .streams
                            .floor(FloorStream::CardRandomRng)
                            .random(n as u32 - 1) as usize;
                        n - idx
                    };
                    self.draw.insert(insert, card);
                }
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
        // 带选牌的牌到这里才算"牌的效果跑完":记一次"打出过这张牌"
        let played_kind = ch.played.as_ref().map(|(c, _)| c.kind());
        if ch.exhaust_after {
            // 被"打抽牌堆顶"放出来的牌打完无条件消耗(能力牌退场),与浩劫的"Exhaust it"一致
            if let Some((card, _)) = ch.played.take() {
                if card.kind() == crate::core::card::CardType::Power {
                    self.vanish_card(card);
                } else {
                    self.exhaust_card(card);
                }
            }
        } else {
            self.finish_played(ch.played.take());
        }
        if let Some(kind) = played_kind {
            self.note_card_played(kind);
        }
        // 选牌那一截尾巴里如果压了"打顶牌"(理论上没有,留着兜底),同样等收尾后再打
        self.drain_pending_top_plays();
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
            if ch.exhaust_after {
                // 打抽牌堆顶放出来的牌本来就不是手牌里出去的,取消也不能塞回手里
                if card.kind() == crate::core::card::CardType::Power {
                    self.vanish_card(card);
                } else {
                    self.exhaust_card(card);
                }
            } else {
                self.hand.push(card);
            }
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
        // 反编译的判定就是一次 cardRandomRng.randomBoolean()
        // (refs/sts_lightspeed/src/combat/BattleContext.cpp:1985-1991:
        //  `if (item.exhaustOnUse && hasRelic<STRANGE_SPOON>()) spoonProc = cardRandomRng.randomBoolean();`),
        // 取的是 nextLong 的最低位.掷点口径必须一致:用 random(99) 掷出的值虽然也是 50%,
        // 但和参考的掷点对不上,同一个种子下这张牌的去处就会两边不同.
        let coin = self.streams.floor(FloorStream::CardRandomRng).random_boolean();
        // pct 仍是 0..100 的"改成弃牌"的概率:100 一定改,50 看这一掷,低于 50 反过来.
        let to_discard = if pct >= 100 {
            true
        } else if pct >= 50 {
            coin
        } else {
            !coin
        };
        !to_discard
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
        // 化石螺壳(Buffer)只拦"伤害",不拦卡牌/能力的直接掉血:
        // 反编译里 Buffer 判在 Player::damage / Player::attacked,而 loseHp() 这条
        // 直接掉血的路径(放血、献祭、蓝蜡烛、燃烧契约、缠绕、灼烧?)没有它.
        // 无形则相反:Player::loseHp 第一句就是 INTANGIBLE 把 amount 压到 1
        // (refs/sts_lightspeed/src/combat/Player.cpp:261-275),原版无形的能力
        // 文本也写明"受到的伤害与生命流失都降为 1".顺序与反编译一致:先无形,后钨钢棒.
        let amount = if amount > 1 && self.player.statuses.has(Status::Intangible) {
            1
        } else {
            amount
        };
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
        // 花开彼岸(不能回血)把两种保命符一起挡掉:反编译 Player::wouldDie 里
        // 仙女与蜥蜴尾巴整段都包在 `if (!hasRelic<MARK_OF_THE_BLOOM>())` 里
        if !self.relic_any(|fx| fx.no_heal) {
            // 仙女在瓶中先判(反编译 wouldDie 先扫药水栏,再查蜥蜴尾巴).
            // 两条保命符都走 Player::heal(Player.cpp:156-170):
            // 先把 curHp 归零(wouldDie 第一句),再 heal —— 于是**魔法花会把回血量
            // 再乘 3/2**(仙女 30%->45%、树皮 60%->90%、蜥蜴尾 50%->75%),
            // 并顺带触发红骷髅的"回到半血以上"记账.此前直接 self.player.hp = back
            // 漏了魔法花这一档.
            if self.fairy_save {
                self.fairy_save = false;
                self.fairy_used = true;
                // 神圣树皮让保命符的数值也翻倍:30% -> 60%
                let pct = if self.relic_sum(|fx| fx.potion_potency_pct) > 0 {
                    60
                } else {
                    30
                };
                let amount = (self.player.max_hp * pct / 100).max(1);
                self.player.hp = 0;
                self.heal_player(amount);
                let back = self.player.hp;
                self.push_log(
                    LogKind::Info,
                    format!("Fairy in a Bottle heals you to {back} HP"),
                );
                return;
            }
            // 蜥蜴尾巴:每场一次,致命伤改为按最大生命的百分比回血
            let pct = self.relic_max(|fx| fx.death_save_pct);
            if pct > 0 && !self.rs.lizard_used {
                self.rs.lizard_used = true;
                let amount = (self.player.max_hp * pct / 100).max(1);
                self.player.hp = 0;
                self.heal_player(amount);
                let back = self.player.hp;
                self.push_log(
                    LogKind::Info,
                    format!("Lizard Tail heals you to {back} HP"),
                );
                return;
            }
        }
        self.player.hp = 0;
        self.phase = Phase::Lost;
        self.push_log(LogKind::Info, "you have been defeated".to_string());
    }

    // ===================== 受击链 / 格挡链 顺序表 =====================
    //
    // 行号均指 refs/sts_lightspeed 内的相对路径.
    //
    // [玩家受击链 A] 敌人"攻击"打过来(AttackPlayer -> Player::attacked, Player.cpp:210-259):
    //   0. 无形:攻击者侧已压到 1(Monster::calculateDamageToPlayer, Monster.cpp:561-599)
    //   1. 格挡吸收 block(min(block, damage))
    //   2. Buffer(化石螺壳):damage>0 时减一层并把这一击清零(在鸟居/钨钢棒之前)
    //   3. 玩家荆棘 THORNS -> DamageEnemy(enemyIdx, thorns)   [不看是否被格挡, 排队到顶]
    //   4. 火焰屏障 FLAME_BARRIER -> DamageEnemy                [同上, 与 3 同拍]
    //   5. 鸟居 TORII:扣完格挡后剩 1..5 的攻击伤害降为 1
    //   6. 钨钢棒 TUNGSTEN_ROD:每次掉血再 -1
    //   7. damage>0 才做的三件:镀甲 PLATED_ARMOR 减一层 -> 目标怪带痛苦刺击时塞伤口
    //      -> hpWasLost(见链 C).damage==0 时 lastAttackUnblockedDamage 归零
    //   本作:第 1/2 步在 hit_player_kind(helix), 第 3/4 步在 enemy_attack(荆棘+火焰
    //   屏障合成 thorns, 每段各反一次), 5/6 在 hit_player_kind, 7 在 hit_player_kind.
    //
    // [玩家受击链 B] 非攻击"伤害"(死亡律动/缠绕/灼伤这类 DamagePlayer -> Player::damage,
    //                Player.cpp:174-208):
    //   无形 -> 格挡吸收 -> Buffer -> 钨钢棒 -> hpWasLost.没有鸟居/镀甲/荆棘.
    //
    // [玩家掉血链 B'] 直接掉血(Player::loseHp, Player.cpp:261-274;卡牌/能力自伤):
    //   无形(-1 到 1) -> 钨钢棒(-1) -> hpWasLost.不吃格挡, 也不吃 Buffer.
    //
    // [链 C] hpWasLost(Player.cpp:276-321), 上面两条最终都汇到这里:
    //   扣血 -> Rupture(仅 selfDamage) -> 百年拼图(移除遗物并抽 3)
    //        -> 情绪芯片(反编译为 todo;蓝职专属充能球遗物,超出本作范围,
    //           见 relics.rs 的 GATED_RELICS) -> 自成型黏土(下回合格挡 +3)
    //        -> 符文方块(抽 1) -> 红骷髅(首次跌破半血补 3 力)
    //        -> cards.onTookDamage(血债降费) -> timesDamagedThisCombat++ -> wouldDie
    //   本作:on_hp_lost 做拼图/方块/黏土/红骷髅, 血债在 note_hp_loss, wouldDie 在
    //   resolve_player_death.
    //
    // [怪物受击链 A] 玩家"攻击"打过去(Monster::attacked, Monster.cpp:407-441):
    //   0. 目标无形:damage>0 时压到 1
    //   1. ANGRY(狂怒):在格挡之前就涨力量(onAttacked, 挡不挡都算)
    //   2. 格挡吸收;这一击把格挡打碎(且玩家有手钻)时补 2 易伤
    //   3. damage>0 -> attackedUnblockedHelper(Monster.cpp:339-405):
    //        a. 靴子 THE_BOOT:未被格挡的攻击伤害 1..4 提到 5
    //        b. 淬毒 Envenom:玩家有就给怪上毒
    //        c. else-if 独占链(只走第一条命中的):
    //           无敌(扣额度) / 镀甲(减一层, 甲碎的壳裂怪换成眩晕招)
    //           / 卷曲(一次性给格挡) / 飞行(减一层, 减到 0 落地眩晕)
    //           / 延展|反应(延展先给格挡再 +1;反应重掷意图) / 荆棘(反打玩家)
    //           / 睡眠(醒来并把金属化清 0) / 移形换影(等量扣力量, 回合末回补)
    //        d. 扣血;hp<=0 走 die, 否则 onHpLost(阈值:分裂/形态切换)
    //   本作:hit_enemy_final 做 0/2/3a/无敌/扣血;on_enemy_hp_lost 做睡眠/卷曲/延展/
    //   镀甲/飞行/移形换影/阈值;on_enemy_attacked 做 1(狂怒)与荆棘、反应.
    //   (淬毒 Envenom 是绿职(Silent)专属卡,超出本作范围:corpus 里 color=green、
    //    不在 CARDS/牌池里,所以玩家永远拿不到它.毒机制本身本作有,是卡不在范围.)
    //
    // [怪物受击链 B] 非攻击"伤害"(毒/燃烧/荆棘这类 Monster::damage, Monster.cpp:466-497):
    //   无形 -> 格挡吸收 -> damageUnblockedHelper(Monster.cpp:442-464):
    //   无敌 / 睡眠 / 移形换影(顺序同上) -> 扣血 -> onHpLost.
    //   与链 A 的差别:没有靴子/淬毒/镀甲/卷曲/飞行/延展/反应/荆棘/ANGRY.
    //   本作:on_enemy_hp_lost 里"睡眠/移形换影"在 is_attack 之外(两条链都走),
    //   其余在 is_attack 之内(只有链 A 走).
    //
    // [玩家格挡获得链] 卡牌格挡 = calculateCardBlock(BattleContext.cpp:2744-2758):
    //   有 NoBlock(紧急按钮)直接 0 -> 加敏捷 -> 虚弱 x3/4(向下取整);
    //   遗物/能力给的格挡不过这一层(直接 Actions::GainBlock(amount)).
    //   然后 Player::gainBlock(Player.cpp:68-80):block += amount;Juggernaut 触发
    //   (随机一个活怪吃伤害).
    //   本作:gain_block(amount, doubled, from_card) 里 from_card 才走敏捷/虚弱/NoBlock,
    //   Juggernaut 对所有来源的格挡都触发.
    // [回合开始的格挡清理] BattleContext.cpp:2181-2188:Barricade(全留) > Blur(递减)
    //   > Calipers(-15) > 清空.本作在 start_turn,顺序一致
    //   (Blur 是绿职(Silent)专属卡,超出本作范围:corpus 里 color=green、不在 CARDS 里,
    //    所以不会有牌挂上"下回合格挡不消失",这条链只剩 Barricade/Calipers 两支.)

    /// 敌人打玩家一次(非攻击伤害:死亡律动、荆棘、缠绕、灼伤这些一并走这里);
    /// 返回(实际掉血, 被格挡量).会掉镀甲的只有真正的攻击,见 hit_player_attack
    fn hit_player(&mut self, damage: i32) -> (i32, i32) {
        self.hit_player_kind(damage, false)
    }

    /// 敌人"攻击"打玩家一次(原版的 AttackPlayer:掉血才掉一层镀甲)
    fn hit_player_attack(&mut self, damage: i32) -> (i32, i32) {
        self.hit_player_kind(damage, true)
    }

    fn hit_player_kind(&mut self, damage: i32, attack: bool) -> (i32, i32) {
        let mut dmg = damage.max(0);
        // 无形:把这一击压到 1.攻击伤害在进这里之前,enemy_attack_damage 已经折过
        // 一遍(那里等于反编译的 Monster::calculateDamageToPlayer),这里再折是幂等的;
        // 非攻击伤害(荆棘/燃烧/死亡律动)与直接掉血没有那条前置链路,就在这一步折.
        if dmg > 1 && self.player.statuses.has(Status::Intangible) {
            dmg = 1;
        }
        let blocked = self.player.block.min(dmg);
        self.player.block -= blocked;
        let mut taken = dmg - blocked;
        if taken > 0 {
            // 化石螺壳:本场第一次掉血直接免掉(原版是 Buffer,在鸟居/钨钢棒之前)
            if self.rs.helix > 0 {
                self.rs.helix -= 1;
                self.push_log(LogKind::Info, "Fossilized Helix prevents the damage".to_string());
                return (0, blocked);
            }
            // 鸟居:扣掉格挡后还剩 1..5 点的"攻击"伤害降到 1(反编译 Player::attacked:
            // 格挡 -> 鸟居 -> 钨钢棒).非攻击伤害不走这里.
            if attack {
                let torii = self.relic_max(|fx| fx.small_attack_reduce_to);
                if torii > 0 && taken > 1 && taken <= 5 {
                    taken = torii;
                }
            }
            // 钨钢棒:每次掉血少掉 1
            let rod = self.relic_sum(|fx| fx.hp_loss_reduction);
            taken = (taken - rod).max(0);
            // 减到 0 就不算掉血(镀甲不掉层、嗜血不降费;反编译那句 if (damage > 0))
            if taken > 0 {
                self.player.hp -= taken;
                self.shake(ShakeWho::Hero, -1, ShakeKind::Hurt, taken);
                self.note_hp_loss();
                self.on_hp_lost(taken);
                // 镀甲:只有没被格挡住的"攻击"伤害才掉一层(原版 Player::attacked;
                // 死亡律动/荆棘/灼伤这些非攻击伤害走 Player::damage,不掉)
                if attack && self.player.statuses.get(Status::PlatedArmor) > 0 {
                    self.player.statuses.add(Status::PlatedArmor, -1);
                }
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
    ///
    /// 玩家 -> 敌人的攻击伤害折叠全序,照反编译 BattleContext::calculateCardDamage
    /// (refs/sts_lightspeed/src/combat/BattleContext.cpp:2671-2755).每行 =
    /// 来源(钩子) | give/receive | 加/乘 | 本作实现位置:
    ///   1  base(牌面/效果值)                       base        resolve_effects
    ///   2  Strike Dummy 打击假人 +3(遗物 atDamageModify)| give  | 加 | resolve_effects 的 relic_add
    ///   3  Wrist Blade 腕刃 +4(遗物 atDamageModify,本回合费用 0)| give | 加 | resolve_effects 的 relic_add
    ///   4  +Strength 力量(power AtDamageGive)        | give    | 加 | 本函数(按 powers 挂载顺序折 statuses)
    ///   5  +Vigor 活力,赤牛(power AtDamageGive)      | give    | 加 | 本函数(rs.vigor)
    ///   6  DoubleDamage 幻影 x2(power AtDamageGive)  | give    | 乘 | 未实现(青职 Silent,超出本作范围)
    ///   7  PenNib 笔尖 x2(power AtDamageGive,第 10 张攻击)| give | 乘 | 本函数(rs.pen_nib == 9)
    ///   8  Weak 自身虚弱 x0.75(power AtDamageGive)   | give    | 乘 | 本函数(statuses 折叠)
    ///   9  Wrath x2 / Divinity x3(stance AtDamageGive)| give   | 乘 | 未实现(观者 Watcher,超出范围)
    ///  10  Slow 慢速 x(1+0.1n)(enemy AtDamageReceive) | receive | 乘 | reduce_incoming
    ///  11  Vulnerable 易伤 x1.5 / 纸蛙 Paper Phrog x1.75(enemy AtDamageReceive)| receive | 乘 | 本函数
    ///  12  Flight 飞行 x0.5(enemy AtDamageReceiveFinal)| receive | 乘 | reduce_incoming
    ///  13  Intangible 无形 -> 1(enemy AtDamageReceiveFinal)| receive | 取值 | reduce_incoming
    ///      floor 一次                    |         |    | reduce_incoming
    ///      clamp >= 0                    |         |    | reduce_incoming
    /// 之后才是应用侧(反编译 Monster::attacked / attackedUnblockedHelper,
    /// Monster.cpp:339-440):扣格挡 -> The Boot 靴子(未格挡 1..4 抬到 5)->
    /// Hand Drill 手钻破格挡上易伤 -> 无敌上限 -> 掉血/挨打钩子,见 hit_enemy_final.
    /// 注意:力量/活力的"加法"必须全部排在笔尖/虚弱这类"乘法"之前
    /// (反编译里 4/5 在 7/8 之前),否则 (base+力量)x2 会被算成 basex2+力量.
    /// 两处口径说明:力量/虚弱按 powers 挂载顺序折(不写死"先力量后虚弱",见
    /// status.rs 的 entries 注释);慢速在 reduce_incoming、易伤在本函数,实际乘序是
    /// 易伤 -> 慢速(与反编译的慢速 -> 易伤相反),两者都是乘,整数结果不变.
    fn player_attack_damage(&self, raw: i32, target: usize, is_attack: bool) -> f32 {
        // 活力(Akabeko 的 8 点):只加在攻击牌的伤害上,和原版的 atDamageGive 一致
        let vigor = if is_attack { self.rs.vigor } else { 0 };
        // 原版把加伤与乘伤一起按 float 连乘,末尾只向下取整一次,所以中间不能各自 floor.
        // 力量/虚弱都是 powers 的 atDamageGive,按 powers 挂载顺序依次折叠
        // (原版就是按 powers List 的顺序走;先虚弱后力量会比反过来少 1 点).
        // 玩家自己的虚弱固定 -25%(原版 calculateCardDamage 写死 .75);纸鹤只作用于
        // 怪物侧的虚弱,见 enemy_attack_damage.
        let mut d = (raw + vigor) as f32;
        for (s, n) in self.player.statuses.entries() {
            match s {
                Status::Strength => d += *n as f32,
                Status::Weak => d *= 0.75,
                _ => {}
            }
        }
        // 笔尖:每第 10 张攻击牌的伤害翻倍.反编译把它当 powers 的 AtDamageGive,
        // 排在力量/活力**之后**(BattleContext.cpp:2698-2700,在 2689 的 +STRENGTH 与
        // 2691 的 +VIGOR 之后),所以这里乘的是加完力量/活力以后的值,不能再拿基数翻倍.
        // (笔尖与虚弱同为乘法,谁先谁后不影响结果.)
        // 集中在这一处,所有走本函数的攻击伤害效果(含狂暴 DamageWithBonus、回旋镖
        // DamageRandom、重击 DamageEqualBlock、完美打击 DamagePerStrike 等)才会一致.
        if is_attack && self.rs.pen_nib == 9 && self.relic_any(|fx| fx.double_damage_per_10_attacks) {
            d *= 2.0;
        }
        if self.enemies[target].statuses.has(Status::Vulnerable) {
            // 纸蛙:易伤多受 75% 伤害(默认 50%)
            let pct = self.relic_max(|fx| fx.vulnerable_damage_pct);
            d *= if pct > 0 { pct as f32 / 100.0 } else { 1.5 };
        }
        d.max(0.0)
    }

    /// 敌人攻击一次的计算.
    ///
    /// 敌人 -> 玩家的攻击伤害折叠全序,照反编译 Monster::calculateDamageToPlayer
    /// (refs/sts_lightspeed/src/combat/Monster.cpp:561-597):
    ///   1  base + 怪物自身 STRENGTH(加法,最前)         | give    | 加 | 本函数(statuses)
    ///   2  Surrounded 被夹击(背对)x1.5                 | give    | 乘 | 本函数
    ///   3  怪物自身 WEAK x0.75 / 纸鹤 Paper Krane x0.6   | give    | 乘 | 本函数(weak_pct)
    ///   4  玩家 VULNERABLE x1.5 / 奇异蘑菇 Odd Mushroom x1.25 | receive | 乘 | 本函数
    ///   5  玩家 Wrath x2(stance receive)                | receive | 乘 | 未实现(观者,超范围)
    ///   6  玩家 INTANGIBLE -> min(d,1)                   | receive | 取值 | 本函数(末尾,floor 之前)
    ///      floor 一次 + clamp >= 0                       |         |    | 本函数
    /// 之后的应用侧(反编译 Player::attacked 210-257 / Player::damage 174-208):
    ///   扣格挡 -> Buffer(化石螺壳)免掉 -> 鸟居 Torii(1..5 -> 1)->
    ///   钨钢棒 Tungsten Rod(-1)-> 镀甲/掉血,见 hit_player_kind.
    /// 力量是唯一的加法,排在乘法之前;本作按 powers 挂载顺序折(status.rs 的 entries
    /// 注释即此意,等同原版 powers List 的顺序),反编译这里写死"力量先、虚弱后",两种口径
    /// 只在"虚弱先挂、力量后挂"时差 1 点.另:Weak 与 Surrounded 的乘序在本作对调了
    /// (Weak 在 statuses 折叠里、Surrounded 在后),两者都是乘,整数结果不变.
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
        // 无形:受到的所有伤害压到 1.反编译的 Monster::calculateDamageToPlayer 末尾就是
        // `if (p.hasStatus<PS::INTANGIBLE>()) damage = std::min(damage, 1.0f);`
        // (refs/sts_lightspeed/src/combat/Monster.cpp:590-592),在 floor 之前.
        // 这一步必须落在"来袭估算"里:重放策略(replay.rs 的 incoming)与 UI 的意图预览
        // 都走本函数,而真实结算 hit_player_kind 也折无形 —— 只在结算侧折、估算侧漏折,
        // 会让两者对不上.act2 seed 92 就是这么分家的:手里有幽影时估算按 10 点威胁选牌,
        // 实际只掉 1 点,参考侧(previewIncoming 读的是已折无形的排队伤害)算出 2,选牌不同.
        // 挂上之后,predicted_damage 与 hit_player_kind 的口径完全一致(见测试
        // predicted_damage_matches_actual_hp_loss_with_intangible).
        if self.player.statuses.has(Status::Intangible) {
            d = d.min(1.0);
        }
        // 鸟居不在这一步做:它作用在"扣掉格挡之后"剩下的伤害上,见 hit_player_kind
        // (反编译 Player::attacked 的顺序是 格挡 -> 鸟居 -> 钨钢棒)
        (d.floor().max(0.0) as i32).max(0)
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

    /// 玩家掉了血:嗜血的费用跟着降(只数手牌/抽牌堆/弃牌堆这三堆)
    fn note_hp_loss(&mut self) {
        self.hp_losses += 1;
        // 消耗堆不算:反编译的 CardManager::onTookDamage
        // (refs/sts_lightspeed/src/combat/CardManager.cpp:448-490)只遍历
        // hand/drawPile/discardPile 三堆,没有 exhaustPile.被消耗掉的嗜血
        // 就算之后被"挖掘"回手牌,也还是当初那张牌面上的价.
        for card in self
            .hand
            .iter_mut()
            .chain(self.draw.iter_mut())
            .chain(self.discard.iter_mut())
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

    /// 打敌人(整数入口).牌面/效果伤害一律走 float 的 damage_enemy_f32(免得提前
    /// floor),重击改走 player_attack_damage 之后这里就没有生产调用者了,只剩测试用.
    #[cfg(test)]
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
        }
        // 移形换影:掉多少血就等量少力量,自己回合结束再补回来
        // (和黑暗镣铐走同一条路:当下真扣,回合末按 temp_strength 回补).
        // 反编译把它同时挂在 Monster::attackedUnblockedHelper 与
        // damageUnblockedHelper(refs/sts_lightspeed/src/combat/Monster.cpp:339-500)
        // 两条路上,所以"非攻击伤害"(燃烧/荆棘/废液这类 damage 路径)照样触发;
        // 参考实现的 SHIFTING 也挂在 wasHPLost 上,不限攻击.
        if self.enemies[idx].statuses.holds(Status::Shifting) {
            self.enemies[idx].statuses.add(Status::Strength, -taken);
            self.enemies[idx].temp_strength += taken;
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
        // 逃跑的(首领倒下后散场的随从)不算击杀,不能触发"击杀类"遗物
        let done_before = self
            .enemies
            .iter()
            .filter(|e| e.death_done && !e.escaped)
            .count();
        for i in 0..self.enemies.len() {
            if self.enemies[i].dead() && !self.enemies[i].death_done {
                self.handle_death(i);
            }
        }
        // 小鬼号角:这一轮真死了几只就给能量并抽牌
        let died = self
            .enemies
            .iter()
            .filter(|e| e.death_done && !e.escaped)
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
            // clear_debuffs 已按反编译 Monster::removeDebuffs 把负力量归零
            self.enemies[i].statuses.clear_debuffs();
            self.enemies[i].statuses.add(Status::Curiosity, -999);
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
                // 复活倒计时统一从 2 起数,由 enemy_end_of_turn 每过一个自己回合扣一格.
                // 死在玩家回合:这一轮的怪兽阶段会照常执行 REGROW(空过)并掷出
                // REINCARNATE,下一轮 REINCARNATE 半血站起来;
                // 死在自己回合(荆棘/火焰屏障反伤):反编译把反伤 addToTop 排在同一回合
                // 末尾 addToBot 的 RollMove 之前(Player.cpp:227-232),所以死亡先落地、
                // 那一次 rollMove 直接看到 halfDead -> REINCARNATE(MonsterSpecific.cpp:
                // 3014-3020),于是它下一轮就复活,不会多浪费一轮;死亡回合末这一格
                // 已经在它自己的 enemy_end_of_turn 里扣掉,正好对齐.
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
        // 死灵之书:本回合第一张"实际费用 >= 2"的攻击再打一次.
        // 反编译 BattleContext.cpp:1691-1694 判的是 costForTurn(X 费另看 energyOnUse >= 2),
        // 不是印刷费用 —— 混乱(蛇眼/蛇油)把狂暴掷成 2 费时,这一刀同样要翻倍.
        let necro = self.relic_any(|fx| fx.double_first_big_attack)
            && !self.rs.necro_used
            && card.kind() == crate::core::card::CardType::Attack
            && cost >= 2;
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
        // 有选牌待定:牌和花的能量先存着,等选完(choose)或取消(cancel)再收尾
        if self.choice.is_some() {
            if let Some(ch) = self.choice.as_mut() {
                ch.played = Some((card, cost));
            }
            // 这一刀如果已经砍死最后一只,后面的选牌就不该再给出去(见 check_win)
            self.check_win();
            return Ok(());
        }
        // 浮夸按"本回合打出的牌数"结算(被人替打出来的牌也算).
        // 带选牌的牌(战争怒吼/坚毅/双持之类)效果还没跑完:原版的 onUseCard 与
        // onAfterCardPlayed 都是排在牌的效果之后才结算的动作,所以这里的"打出一张牌"
        // 要等选牌收完再记(见 close_choice),否则诅咒之眼塞眩晕的位置会错.
        // 活力(Akabeko):下一张攻击牌打出后立刻用掉(参考实现挂在 VIGOR 的
        // onAfterCardPlayed 上;复读的那几下也算在里面,所以放在这里清)
        if card.kind() == crate::core::card::CardType::Attack {
            self.rs.vigor = 0;
        }
        self.note_card_played(card.kind());
        // 蓝蜡烛:打出诅咒要掉血(掉死了这张牌也照样落地)
        if relic_play && card.kind() == crate::core::card::CardType::Curse {
            let hp = self.relic_sum(|fx| fx.playable_curses_hp);
            if hp > 0 {
                self.push_log(LogKind::Player, format!("Blue Candle costs {hp} HP"));
                // 自伤:反编译是 PlayerLoseHp(1, true),所以要触发破裂
                self.lose_hp_player(hp, true);
            }
        }
        // 结算完后决定去处(与 finish_played 走同一条路:能力牌退场、该消耗的消耗)
        self.finish_played(Some((card, cost)));
        // 浩劫那张牌已经进了弃牌堆,现在才打抽牌堆顶(与原版动作队列/卡牌队列的先后一致)
        self.drain_pending_top_plays();
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
            // 腕刃看的是"本回合实际费用"(反编译 costForTurn == 0),不是印刷费用:
            // 被疯狂/化茧/木乃伊之手降到 0 费的攻击牌也算
            if card.fixed_cost() == Some(0) {
                relic_add += self.relic_sum(|fx| fx.zero_cost_attack_bonus);
            }
        }
        // 笔尖的翻倍已收进 player_attack_damage(所有攻击伤害效果共用)
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
                        let raw = amount + relic_add;
                        ctx.unblocked += self.damage_enemy_times(t, raw, is_attack, times.max(1) as i32);
                    }
                }
                Effect::DamageAll { amount, times } => {
                    for _ in 0..times.max(1) {
                        for t in self.alive_enemies() {
                            let raw = amount + relic_add;
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
                        // 易伤看的是"出牌这一刻"(反编译 refs/sts_lightspeed/src/combat/Actions.cpp:1036-1045 的
                        // DropkickAction:先按当前状态决定要不要给能量/抽牌,再把这一击
                        // 压进动作队列),所以这一刀把敌人打死也照样回能量、抽牌.
                        let vuln = self.enemies[t].statuses.has(Status::Vulnerable);
                        let d = self.player_attack_damage(amount, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
                        if vuln {
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
                        // 力量影响这张牌 mult 次:把 (mult-1) 次的力量折进基数,剩下 1 次
                        // 由 player_attack_damage 的 powers 折叠补上,再走同一套笔尖/假人/
                        // 易伤/弱体链.反编译 BattleContext.cpp:1054-1058 的 HEAVY_BLADE 就是
                        // `dmg1 = 14 + (升?4:2)*STRENGTH` 之后交给 calculateCardDamage;
                        // 参考实现(ironclad/common.ts 的 HEAVY_BLADE)同口径.
                        // 此前这里自算 0.75/1.5 并各自 floor,还漏掉笔尖与腕刃,与全序不符.
                        let extra = self.player.statuses.get(Status::Strength) * (mult - 1);
                        let d = self.player_attack_damage(amount + extra + relic_add, t, is_attack);
                        ctx.unblocked += self.damage_enemy_f32(t, d);
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
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Hand,
                        ChoiceAction::Exhaust,
                        ChoiceFilter::Any,
                        "exhaust a card",
                    ));
                }
                Effect::TopFromHand => {
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Hand,
                        ChoiceAction::ToDrawTop,
                        ChoiceFilter::Any,
                        "put a card on top of the draw pile",
                    ));
                }
                Effect::CopyFromHand { copies } => {
                    // 一次选择、复制 copies 份(升级版两份);份数要在自动结算前挂上
                    self.begin_choice(
                        ChoiceSpec::mandatory(
                            ChoiceSource::Hand,
                            ChoiceAction::Copy,
                            ChoiceFilter::AttackOrPower,
                            "copy an Attack or Power card",
                        )
                        .copies(copies as usize),
                    );
                }
                Effect::FromExhaustToHand => {
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Exhaust,
                        ChoiceAction::ToHand,
                        ChoiceFilter::Any,
                        "take a card from the exhaust pile",
                    ));
                }
                Effect::FromDiscardToDrawTop => {
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Discard,
                        ChoiceAction::ToDrawTop,
                        ChoiceFilter::Any,
                        "take a card from the discard pile",
                    ));
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
                    // 原版:浩劫先把顶牌摘进卡牌队列(这一步抽牌堆空了会先重洗弃牌堆,
                    // 浩劫还没进去),浩劫自己进弃牌堆的动作在动作队列里先跑,顶牌最后才
                    // 结算(见反编译 BattleContext.cpp).这里照同一顺序:现在只摘牌,
                    // 等本张牌收尾之后再结算(见 drain_pending_top_plays).
                    if let Some(card) = self.take_top_card_for_play("Havoc") {
                        // 目标在这里掷:与参考实现的 PlayTopCardAction 同一时刻(排在 Hex 塞眩晕之前)
                        let target = self.pick_random_alive();
                        self.pending_top_plays.push((self.havoc_depth + 1, card, true, target));
                    }
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
                    // 没有可升级的牌时 begin_choice 自己会闸掉(不开屏)
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Hand,
                        ChoiceAction::Upgrade,
                        ChoiceFilter::Upgradeable,
                        "Armaments: upgrade a card",
                    ));
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
                    // 化茧 / 变形.反编译 Actions::PutRandomCardsInDrawPile
                    // (refs/sts_lightspeed/src/combat/Actions.cpp:546-561)分两段:
                    // 先把 n 张一次性抽好(每张各掷一次 cardRandomRng),再逐张落到抽牌堆的
                    // 随机位置 —— 落位走 CardManager::shuffleIntoDrawPile
                    // (refs/sts_lightspeed/src/combat/CardManager.cpp:215-223):抽牌堆为空就放
                    // 唯一那格、不掷;否则掷 cardRandomRng.random(size-1),再
                    // insertToDrawPile 插到 `begin()+idx`(CardManager.cpp:188-199).
                    // 反编译的 drawPile 队尾是堆顶(popFromDrawPile 取 back,CardManager.cpp:141),
                    // 本作 draw[0] 才是堆顶,所以位次要镜像:反编译的插入位次 i(从堆底数)
                    // 对应本作的 span-i(span = 插入前的张数;i 取满 [0, span-1] 时
                    // 本作落点铺满 [1, span]) —— 也就是说"洗进去"的牌永远压不到当前堆顶那张,
                    // 这一点在下面测试里直接卡住.
                    // 原先写法是"塞到堆尾再整体洗一遍":多耗一个 shuffleRng 的 long,落点分布
                    // 也不同,cardRandomRng/shuffleRng 两条流从此整体错位.
                    let mut pool = cards::class_pool_of_kind(kind);
                    if pool.is_empty() {
                        continue;
                    }
                    // 与参考实现同口径:战斗内随机抽牌这条路的池子按 id 排序
                    // (slay-the-cli colorless/effects.ts:97-110 的 classPool 明确 sort by id).
                    // 化茧/变形在此之前漏了排序,抽出来的牌因此与参考对不上.
                    pool.sort_by_key(|c| c.id);
                    let mut picks: Vec<CardInstance> = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        let def = self.streams.floor(FloorStream::CardRandomRng).pick(&pool);
                        let mut inst = CardInstance::new(def);
                        self.fix_new_card(&mut inst);
                        inst.free_combat = true;
                        picks.push(inst);
                    }
                    for inst in picks {
                        let label = inst.label();
                        let idx = if self.draw.is_empty() {
                            0
                        } else {
                            let span = self.draw.len();
                            let roll = self
                                .streams
                                .floor(FloorStream::CardRandomRng)
                                .random(span as u32 - 1) as usize;
                            span - roll
                        };
                        self.draw.insert(idx, inst);
                        self.push_log(
                            LogKind::Info,
                            format!("{label} is shuffled in (costs 0 this combat)"),
                        );
                    }
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
                    // 洗牌遗物的钩子:反编译的 DEEP_BREATH 分支在洗之前先调 onShuffle()
                    // (refs/sts_lightspeed/src/combat/BattleContext.cpp:1272-1276);
                    // 参考实现把这次洗牌整个走 reshuffleDiscardIntoDraw,里面也发 onShuffle
                    // (refs/slay-the-cli/src/engine/combat/piles.ts:63-69,由 interpreter.ts:98-100 调用).
                    // 算盘/日晷都要认这一下,漏掉就少 6 格挡、少一次日晷计数.
                    self.on_shuffle();
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
                    // 反编译 ApotheosisAction 只升现有四个牌堆(Actions.cpp:1005-1032);
                    // "新造出来的牌也升级"是 Master Reality 的能力
                    // (refs/slay-the-cli/src/content/powers/watcher.ts:185-196 的
                    // modifyCreatedCardUpgrades),本作没有该能力,所以不留全局标记.
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
                    // 发现:原版 generateDiscoveryCards(refs/sts_lightspeed/src/game/Game.cpp:228-260)
                    // 反复掷点直到凑够 n 张**互不相同**的本职业牌(掷到重的就重掷,可能多掷几次),
                    // 牌池是本职业全部非基础牌(getTrulyRandomCardInCombat 的 CombatCardPool).
                    // 参考实现那条路(randomCardDefs,slay-the-cli relics/lib.ts:108-117)是
                    // "抽一张就把它从池子里拿走"、恰好掷 n 次,与反编译的流错位;
                    // offer_pick 已改成重掷口径(见其文档注释).
                    // 原先直接 pick n 次、不剔重,会亮出重复候选 —— 与两边都不符.
                    let mut pool = cards::class_card_pool();
                    // 池子顺序按 id 排(见上面随机池那一段的说明).
                    pool.sort_by_key(|c| c.id);
                    self.offer_pick(
                        pool,
                        n as usize,
                        ChoiceAction::ToHand,
                        true,
                        1,
                        "choose 1 of 3 random cards",
                    );
                }
                Effect::ExhaustUpTo { n } => {
                    // "最多消耗 n 张":可以少选甚至不选,按可选处理(永远开屏)
                    self.begin_choice(ChoiceSpec::new(
                        ChoiceSource::Hand,
                        ChoiceAction::Exhaust,
                        ChoiceFilter::Any,
                        n as usize,
                        ChoiceMode::Optional,
                        &format!("exhaust up to {n} cards"),
                    ));
                }
                Effect::ToDrawBottomFromHand { n } => {
                    // 预谋(n=1 基础版)是强制单选;升级版 n=0"任意张"按可选(永远开屏)
                    let label = if n == 0 {
                        "put any number of cards on the bottom of the draw pile"
                    } else {
                        "put a card on the bottom of the draw pile"
                    };
                    let mode = if n == 0 {
                        ChoiceMode::Optional
                    } else {
                        ChoiceMode::Mandatory
                    };
                    self.begin_choice(ChoiceSpec::new(
                        ChoiceSource::Hand,
                        ChoiceAction::ToDrawBottom,
                        ChoiceFilter::Any,
                        n as usize,
                        mode,
                        label,
                    ));
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
                    self.begin_choice(ChoiceSpec::mandatory(
                        ChoiceSource::Draw,
                        ChoiceAction::ToHand,
                        filter,
                        label,
                    ));
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
        // LoseStrength/LoseDexterity 在原版是 DEBUFF 类型,神器一样顶掉
        // (refs/sts_lightspeed/include/combat/Player.h:362-376 的 debuff<> 首查 ARTIFACT;
        // 参考实现 refs/slay-the-cli/src/content/powers/ironclad.ts:280-282 LOSE_STRENGTH
        // 也是 kind="debuff").本作 is_debuff() 没列它们,这里显式补上.
        let debuff_application =
            status.is_debuff() || matches!(status, Status::LoseStrength | Status::LoseDexterity);
        if n > 0 && debuff_application && self.player.statuses.has(Status::Artifact) {
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
        // 仙女在瓶中这类被动药水没有"喝"这个动作(反编译 BattleContext::drinkPotion 对它 assert)
        if !def.drinkable() {
            return;
        }
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
                // 反编译:先 BuffPlayer<STRENGTH/DEXTERITY> 再 DebuffPlayer<LOSE_*>
                // (BattleContext.cpp:2344-2347 / 2407-2410),后者是减益,有神器就被顶掉
                // (Player.h:362-376).所以 lose 要走 add_self_status 而不是直接挂.
                self.player.statuses.add(status, n);
                self.add_self_status(lose, n);
            }
            PotionFx::Discovery { pool, n } => {
                let cards = self.discovery_pool(pool);
                // 神圣树皮把份数翻倍:挑中的那张按 n 份进手
                self.offer_pick(cards, 3, ChoiceAction::ToHand, true, n.max(1) as usize, def.name);
            }
            PotionFx::ExhaustHand => {
                // 灵药:消耗任意张(可选,永远开屏)
                self.begin_choice(ChoiceSpec::new(
                    ChoiceSource::Hand,
                    ChoiceAction::Exhaust,
                    ChoiceFilter::Any,
                    0,
                    ChoiceMode::Optional,
                    &format!("{}: exhaust any number", def.name),
                ));
            }
            PotionFx::DiscardHandThenDraw => {
                // 赌徒之酿:弃任意张再抽等量张(可选,永远开屏)
                self.begin_choice(
                    ChoiceSpec::new(
                        ChoiceSource::Hand,
                        ChoiceAction::Discard,
                        ChoiceFilter::Any,
                        0,
                        ChoiceMode::Optional,
                        &format!("{}: discard any number", def.name),
                    )
                    .draw_after(),
                );
            }
            PotionFx::ReturnFromDiscard { n } => {
                // 液态记忆:从弃牌堆拿回 n 张(强制;只剩 1 张时自动结算)
                let need = (n as usize).min(self.discard.len());
                self.begin_choice(ChoiceSpec::new(
                    ChoiceSource::Discard,
                    ChoiceAction::ToHand,
                    ChoiceFilter::Any,
                    need,
                    ChoiceMode::Mandatory,
                    &format!("{}: take a card back", def.name),
                ));
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
                    self.play_top_of_draw_now(false, "Distilled Chaos");
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
                            inst.upgrade_forced();
                        }
                        self.fix_new_card(&mut inst);
                        self.hand.push(inst);
                    }
                }
            }
            // 这两瓶由 run 层结算:脱战要换界面,填药水要动跑图状态;
            // Passive(仙女在瓶中)在函数开头就返回了,这里只是把 match 补全
            PotionFx::Escape | PotionFx::FillPotionSlots | PotionFx::Nothing | PotionFx::Passive => {
            }
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
            .filter(|i| matches!(self.hand[*i].cost(), Cost::Fixed(_)))
            .collect();
        for i in idxs {
            // 同抽牌时的混乱:基线取当前费用(升级后的),掷出来的值就是这一张的费用
            let base = match self.hand[i].cost() {
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
    /// copies 是选中的那份给几张(神圣树皮);action 是选完干什么(抄本要洗进抽牌堆).
    ///
    /// **掷点口径照反编译**:generateDiscoveryCards
    /// (refs/sts_lightspeed/src/game/Game.cpp:228-260)每次从整个池子里随机取一张,
    /// 掷到重的**重掷**(可能多掷几次),直到凑够 n 张互不相同 —— 不是"抽一张就从池里拿走".
    /// 参考实现那条路(randomCardDefs,refs/slay-the-cli/src/content/relics/lib.ts:108-117)
    /// 会 splice 掉已抽的、恰好掷 n 次,与反编译的流错位;本轮改成重掷口径.
    fn offer_pick(
        &mut self,
        pool: Vec<&'static crate::core::card::CardDef>,
        n: usize,
        action: ChoiceAction,
        free: bool,
        copies: usize,
        label: &str,
    ) {
        let mut offered: Vec<CardInstance> = Vec::new();
        while offered.len() < n && offered.len() < pool.len() {
            let i = self
                .streams
                .floor(FloorStream::CardRandomRng)
                .random(pool.len() as u32 - 1) as usize;
            let def = pool[i];
            if offered.iter().any(|c| c.def.id == def.id) {
                continue; // 掷到重样的:重掷(反编译 while 循环不推进 cardCount)
            }
            let mut inst = CardInstance::new(def);
            self.fix_new_card(&mut inst);
            offered.push(inst);
        }
        if offered.is_empty() {
            return;
        }
        // 只亮出 1 张(牌池太小)时按强制单选自动结算(参考实现 chooseOne 同样 auto)
        self.begin_choice(
            ChoiceSpec::mandatory(ChoiceSource::Offered, action, ChoiceFilter::Any, label)
                .offered(offered)
                .free(free)
                .copies(copies),
        );
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

        // 二重身:复制一张手牌(手里摆两张可复制的牌,窗口才开得出来)
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![
            crate::core::cards::card("dual_wield"),
            crate::core::cards::card("strike"),
            crate::core::cards::card("strike"),
        ];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        let n = c.hand.len();
        c.choose(0).unwrap();
        assert_eq!(c.hand.len(), n + 1, "复制出一张");
        assert_eq!(c.hand.last().unwrap().def.id, "strike");

        // 掘出:从消耗堆拿回手牌(消耗堆留两张,不是强制单选那条自动结算的路)
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.exhaust.push(crate::core::cards::card("bash"));
        c.exhaust.push(crate::core::cards::card("bash"));
        c.hand = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Exhaust);
        c.choose(0).unwrap();
        assert!(c.hand.iter().any(|x| x.def.id == "bash"), "掘出的牌回到手牌");

        // 头槌:打伤害 + 从弃牌堆拿一张到抽牌堆顶(弃牌堆两张才开屏)
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.discard.push(crate::core::cards::card("defend"));
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
        // 手里两张可复制的牌,窗口才开得出来
        c.hand = vec![
            dw,
            crate::core::cards::card("strike"),
            crate::core::cards::card("strike"),
        ];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Hand);
        assert_eq!(c.choice.as_ref().unwrap().copies, 2, "升级版一次复制两份");
        // 打出二重身后手牌只剩那两张 strike,下标 0 是其中一张
        c.choose(0).unwrap();
        assert!(c.choice.is_none(), "一次选择就做完,不该再挂第二次选择");
        let strikes = c.hand.iter().filter(|x| x.def.id == "strike").count();
        assert_eq!(strikes, 4, "原来的 2 张 + 复制的 2 张");
    }

    /// 掘出:候选池里不能包含被消耗掉的掘出自己(wiki Update History + 参考实现)
    #[test]
    fn exhume_cannot_recover_itself() {
        // 消耗堆里放一张之前耗掉的掘出自己 + 两张别的牌(候选要够 2 张才开屏)
        let mut c = combat_with("jaw_worm_solo", &["exhume"]);
        c.exhaust.push(crate::core::cards::card("exhume"));
        c.exhaust.push(crate::core::cards::card("bash"));
        c.exhaust.push(crate::core::cards::card("bash"));
        c.hand = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Exhaust);
        let ids: Vec<&str> = c.choice_candidates().iter().map(|(_, x)| x.def.id).collect();
        assert!(!ids.contains(&"exhume"), "掘出不能拿回自己:{ids:?}");
        assert!(ids.contains(&"bash"), "别的消耗牌还能拿:{ids:?}");
    }

    /// 没有候选可选的选牌动作不开窗口:原版这些 action 在堆空时直接 return,
    /// 开了空窗口会让界面卡在"nothing to pick"上(smoke seed 101 就是这么卡的).
    #[test]
    fn empty_pile_choices_do_not_open() {
        // 头槌:弃牌堆空 -> 只打伤害,不开选牌窗口
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![crate::core::cards::card("headbutt")];
        c.energy = 3;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 9, "伤害照常结算");
        assert!(c.choice.is_none(), "弃牌堆空不该开选牌窗口");

        // 头槌:弃牌堆有两张 -> 窗口照开(别把正常路径也闸掉)
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.discard.push(crate::core::cards::card("defend"));
        c.discard.push(crate::core::cards::card("defend"));
        c.hand = vec![crate::core::cards::card("headbutt")];
        c.energy = 3;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.choice.as_ref().unwrap().source, ChoiceSource::Discard);

        // 掘出:消耗堆里只有刚打出的掘出自己 -> 没得拿,不开窗口
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![crate::core::cards::card("exhume")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "消耗堆只剩自己不该开选牌窗口");

        // 二重身:手里只剩防御(没有攻击/能力牌)-> 没得复制,不开窗口
        let mut c = combat_with("jaw_worm_solo", &["strike"; 4]);
        c.hand = vec![
            crate::core::cards::card("dual_wield"),
            crate::core::cards::card("defend"),
        ];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "没有攻击/能力牌不该开选牌窗口");
    }


    /// 浩劫打出"会抽牌"的牌时的先后:浩劫自己先进弃牌堆,顶牌最后才结算 ——
    /// 顶牌那个"抽 1"触发重洗时要把刚打出的浩劫一起洗进去.
    /// 依据:反编译 BattleContext.cpp 打出浩劫时把 PlayTopCard 压进动作队列、把
    /// OnAfterCardUsed(浩劫进弃牌堆)排在它后面,而顶牌进的是卡牌队列 —— 主循环
    /// actionQueue 先于 cardQueue 清空,所以浩劫先落地、顶牌后结算.
    #[test]
    fn havoc_enters_the_discard_pile_before_the_autoplayed_card_resolves() {
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 4]);
        c.hand = vec![card("havoc")];
        c.draw = vec![card("pommel_strike")];
        c.discard = vec![card("strike")];
        c.energy = 3;
        let e_hp = c.enemies[0].hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.enemies[0].hp, e_hp - 9, "抽牌堆顶的重拳被打了出来");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "pommel_strike"),
            "浩劫放出来的顶牌打完进消耗堆"
        );
        // 重拳的"抽 1"把弃牌堆洗回来:这时浩劫已经在弃牌堆里,所以手牌 + 抽牌堆
        // 该凑出浩劫与打击两张;按旧的顺序(浩劫等顶牌打完才落地)只会剩一张.
        let mut ids: Vec<&str> = c.hand.iter().chain(c.draw.iter()).map(|x| x.def.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["havoc", "strike"], "重洗要把刚打出的浩劫一起算进去");
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
        // 留两张手牌,战吼的放顶窗口(候选 2 张)才开得出来
        c.hand = vec![
            crate::core::cards::card("havoc"),
            crate::core::cards::card("strike"),
            crate::core::cards::card("defend"),
        ];
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

        // 灼热攻击:一直能升,伤害按 n*(n+7)/2+12 涨(反编译 BattleContext.cpp:1136)
        let mut card = crate::core::cards::card("searing_blow");
        let base = card.bonus_damage();
        assert_eq!(base, 12, "灼热攻击基础 12");
        card.upgrade();
        assert_eq!(card.bonus_damage(), 16, "第一次升级 16");
        card.upgrade();
        assert_eq!(card.bonus_damage(), 21, "第二次升级 21");
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
    fn dropkick_pays_out_even_when_it_kills_the_vulnerable_enemy() {
        // 易伤是"出牌这一刻"看的:反编译 refs/sts_lightspeed/src/combat/Actions.cpp:1036-1045 的 DropkickAction 先按当前
        // 状态决定要不要回能量/抽牌(addToTop),再把这 5 点伤害压进队列 —— 先判后打,
        // 所以这一刀把敌人打死也照样回 1 能量、抽 1 张.
        let mut c = combat_with(
            "jaw_worm_solo",
            &[
                "dropkick", "dropkick", "dropkick", "dropkick", "dropkick", "strike", "strike",
            ],
        );
        c.enemies[0].statuses.add(Status::Vulnerable, 2);
        c.enemies[0].hp = 1;
        let energy = c.energy;
        let hand = c.hand.len();
        let draw = c.draw.len();
        let i = c.hand.iter().position(|x| x.def.id == "dropkick").unwrap();
        c.play_card(i, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 0, "这一刀打死它");
        assert_eq!(c.energy, energy, "花的 1 费被易伤那条补回来");
        assert_eq!(c.draw.len(), draw - 1, "照样抽 1 张");
        assert_eq!(c.hand.len(), hand, "打出一张、抽回一张");
    }

    #[test]
    fn dropkick_on_a_healthy_enemy_gives_nothing_extra() {
        let mut c = combat_with(
            "jaw_worm_solo",
            &[
                "dropkick", "dropkick", "dropkick", "dropkick", "dropkick", "strike", "strike",
            ],
        );
        c.enemies[0].hp = 44;
        let energy = c.energy;
        let draw = c.draw.len();
        let i = c.hand.iter().position(|x| x.def.id == "dropkick").unwrap();
        c.play_card(i, Some(0)).unwrap();
        assert_eq!(c.energy, energy - 1, "没易伤就只花 1 费");
        assert_eq!(c.draw.len(), draw, "没易伤就不抽牌");
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

    /// 笔尖的第 10 张攻击翻倍要覆盖**所有**攻击伤害效果,不只普通打击.
    /// 反编译把它当 powers 的 AtDamageGive(BattleContext.cpp:2698-2700),排在
    /// 力量/活力之后,任何攻击牌的伤害都吃得到;本作曾只在 Effect::Damage / DamageAll
    /// 里翻倍,狂暴(Effect::DamageWithBonus)这类就漏了 —— seed16 里浩劫打出抽牌堆顶
    /// 那张狂暴,伤害只有一半(8 vs 16).
    /// 断言里两种效果各打一张第 10 次攻击:打击(Damage)与狂暴(DamageWithBonus).
    #[test]
    fn pen_nib_doubles_every_attack_damage_effect() {
        let pen = relic_def_or_panic("pen_nib");
        for (card_id, base) in [("strike", 6), ("rampage", 8)] {
            let mut c = Combat::new(
                enc("jaw_worm_solo"),
                setup(80, &[card_id; 5], &[pen]),
                RngRegistry::new(1),
            );
            c.enemies[0].block = 0;
            c.energy = 9;
            c.rs.pen_nib = 9; // 这一张就是第 10 张攻击
            let before = c.enemies[0].hp;
            c.play_card(0, Some(0)).unwrap();
            assert_eq!(
                c.enemies[0].hp,
                before - base * 2,
                "{card_id} 的第 10 张攻击应翻倍(笔尖是 powers AtDamageGive)"
            );
        }
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

    /// 吐火对诅咒牌一样触发(反编译 CardManager::draw 的 CURSE 分支;此前只认状态牌,
    /// 抽到"腐朽"这类诅咒时白丢一次全体伤害).
    #[test]
    fn fire_breathing_punishes_drawing_curses() {
        let mut c = combat_with("jaw_worm_solo", &["decay"; 5]);
        c.player.statuses.add(Status::FireBreathing, 6);
        c.hand.clear();
        c.draw.clear();
        c.discard = vec![card("decay"), card("decay")];
        let before = c.enemies[0].hp;
        c.draw_cards(1);
        assert_eq!(c.enemies[0].hp, before - 6, "抽到诅咒要打 6 点全体");
        // 抽普通牌不该触发:牌堆里只留一张技能牌,再抽一张血量不动
        c.draw.clear();
        c.discard = vec![card("defend")];
        let mid = c.enemies[0].hp;
        c.draw_cards(1);
        assert_eq!(c.enemies[0].hp, mid, "非状态/诅咒不触发吐火");
    }

    /// 混乱把抽到的牌改成掷出来的那个费用.升级降费的牌(havoc+ 基础 1、升级后 0)
    /// 也必须正好是掷出的值:此前拿牌面基础费用当基线算 delta,这类牌会凭空少 1,
    /// 于是"3 费打 0 费牌"这种费用账在对拍里会走到不同的分支(seed 16 的蛇怪战).
    #[test]
    fn confused_sets_the_rolled_cost_even_when_the_upgrade_discounts_it() {
        let havoc_up = || {
            let mut inst = cards::card("havoc");
            inst.upgrade();
            inst
        };
        let mut c = combat_with("jaw_worm_solo", &["havoc"; 5]);
        c.player.statuses.add(Status::Confused, 1);
        c.hand.clear();
        c.draw = vec![havoc_up()];
        assert_eq!(havoc_up().fixed_cost(), Some(0), "havoc+ 基础费用是 0");
        let before = c.streams.floor(FloorStream::CardRandomRng).state();
        let expected = crate::rng::Rng::from_state(before).random(3) as i32;
        c.draw_cards(1);
        assert_eq!(c.hand.len(), 1);
        assert_eq!(
            c.hand[0].fixed_cost(),
            Some(expected),
            "混乱后的费用就是掷出来的值(0..3),不该再被升级降费减掉一档"
        );
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
    fn apotheosis_upgrades_every_pile_but_not_later_cards() {
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
        // 神化只覆盖"打出的那一刻"的四个牌堆(反编译 ApotheosisAction,Actions.cpp:1005-1032);
        // "之后新造出来的牌也升级"是 Master Reality 的能力(watcher.ts:185-196),本作没实现,
        // 所以神化之后新造出来的牌不自动升级.
        let mut fresh = cards::card("strike");
        c.fix_new_card(&mut fresh);
        assert!(!fresh.upgraded, "神化之后新造的牌不该自动升级(那是 Master Reality)");
    }

    /// 哨卫+ 被消耗时回 3 能量(基础 2);反编译 CardInstance.cpp:203 triggerOnExhaust
    #[test]
    fn sentinel_upgrade_grants_three_energy_on_exhaust() {
        // 重整旗鼓会把手里所有非攻击牌消耗掉,正好触发哨卫的消耗效果
        let mut c = staged(&["second_wind", "sentinel"], &["second_wind", "sentinel"]);
        let s = hand_idx(&c, "sentinel");
        c.hand[s].upgrade();
        let sw = hand_idx(&c, "second_wind");
        let e0 = c.energy;
        c.play_card(sw, None).unwrap();
        // second_wind 1 费 + 哨卫+ 回 3 能
        assert_eq!(c.energy, e0 - 1 + 3, "Sentinel+ 消耗时应回 3 能量");
        assert!(c.exhaust.iter().any(|x| x.def.id == "sentinel"));
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
            &["secret_technique", "strike", "defend", "defend", "bash"],
            &["secret_technique"],
        );
        let idx = hand_idx(&c, "secret_technique");
        c.play_card(idx, None).unwrap();
        let ch = c.choice.as_ref().expect("秘技要开选择");
        assert_eq!(ch.source, ChoiceSource::Draw);
        let cands = c.choice_candidates();
        assert_eq!(cands.len(), 2, "抽牌堆里有两张技能");
        assert!(cands.iter().all(|(_, card)| card.def.id == "defend"));
        let (idx, card) = cands[0];
        assert_eq!(card.def.id, "defend");
        c.choose(idx).unwrap();
        assert!(c.hand.iter().any(|x| x.def.id == "defend"), "技能进了手牌");
        assert_eq!(
            c.draw.iter().filter(|x| x.def.id == "defend").count(),
            1,
            "只拿走了挑中的那张"
        );
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

    // ==========================================================================
    // 沙盒里那批 "(b) 参考缺口(随机池)" 卡片的不依赖参考实现的自证.
    //
    // 分三层:
    //  1) 牌面规则(张数/费用/消耗/时点/剔重)一律对反编译 refs/sts_lightspeed/,
    //     下面的测试就是这些断言的固化(每条都写了出处行号);
    //  2) "抽到哪张"这一层:反编译里战斗内随机无色牌走 CombatColorlessCardPool,
    //     是一张写死的 34 项数组(CardPools.h:189-196,Java HashMap 打散序),比
    //     ColorlessRarityCardPool 的 35 张(CardPools.h:133-138)少一张 BANDAGE_UP;
    //     CombatTypeCardPool(CardPools.h:150-156)同样缺 FEED / REAPER。按原序复刻时缺牌
    //     会让池成员与真实游戏对不上、分布也歪(已试过),而"缺的几张插在哪"反编译里没有
    //     (它是运行期从卡牌库拼的),**故无法复现**;本作与参考实现同口径 —— 战斗内随机
    //     一律把池子按 id 排序(slay-the-cli ironclad/uncommon.ts:300-306 与
    //     colorless/effects.ts:34-56 的 ENGINE-NOTE)。断言见
    //     colorless_random_pool_is_the_decompiled_35(池成员=ColorlessRarityCardPool 的 35 张);
    //  3) 化茧/变形多一层"先抽后落位"的时点,与参考实现的"逐个交替"不同,见
    //     chrysalis_and_metamorphosis_pick_then_place_like_the_decompile。
    // ==========================================================================

    /// 手牌满时"战斗中新造的无色牌"进弃牌堆、不是凭空消失:参考实现 makeTempCard 的
    /// hand overflow goes to discard(slay-the-cli interpreter.ts:407-408),本作原先直接
    /// return,把这张牌丢了还省掉一次掷点。嬗变 X=3、手牌先摆满 10 张时,第 2/3 张要落到弃牌堆。
    #[test]
    fn colorless_gift_overflows_to_discard_when_hand_is_full() {
        let mut c = staged(&["transmutation"], &["transmutation"]);
        for _ in 0..9 {
            c.hand.push(card("defend"));
        }
        assert_eq!(c.hand.len(), 10, "手牌先摆满");
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 10, "塞不进手的那些不留手上");
        let overflow = c
            .discard
            .iter()
            .filter(|x| cards::pool_of(x.def) == "colorless")
            .count();
        assert_eq!(overflow, 2, "多出来的 2 张进弃牌堆");
        assert!(c.exhaust.iter().any(|x| x.def.id == "transmutation"));
    }

    /// 摆一副牌,但用指定 seed(用于"换种子看不变式"的检查)
    fn staged_seed(seed: u64, deck: &[&str], hand: &[&str]) -> Combat {
        let mut c = Combat::new(enc("jaw_worm_solo"), from_ids(80, deck), RngRegistry::new(seed));
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

    /// 无色随机池的成员与顺序 = 反编译 ColorlessRarityCardPool::colorlessCardBlob
    /// (refs/sts_lightspeed/include/constants/CardPools.h:133-138):20 张 uncommon 打头,
    /// 15 张 rare 收尾,每段按 id 排。这是本作 colorless_pool() 的池子
    /// (src/core/cards.rs:3556-3562),万事通/磁力/嬗变都从它抽。
    #[test]
    fn colorless_random_pool_is_the_decompiled_35() {
        let want = [
            "bandage_up",
            "blind",
            "dark_shackles",
            "deep_breath",
            "discovery",
            "dramatic_entrance",
            "enlightenment",
            "finesse",
            "flash_of_steel",
            "forethought",
            "good_instincts",
            "impatience",
            "jack_of_all_trades",
            "madness",
            "mind_blast",
            "panacea",
            "panic_button",
            "purity",
            "swift_strike",
            "trip",
            "apotheosis",
            "chrysalis",
            "hand_of_greed",
            "magnetism",
            "master_of_strategy",
            "mayhem",
            "metamorphosis",
            "panache",
            "sadistic_nature",
            "secret_technique",
            "secret_weapon",
            "the_bomb",
            "thinking_ahead",
            "transmutation",
            "violence",
        ];
        let pool = cards::colorless_pool();
        let got: Vec<&str> = pool.iter().map(|c| c.id).collect();
        assert_eq!(got, want, "无色随机池成员/顺序");
        assert!(
            pool.iter().all(|c| matches!(c.rarity, Rarity::Uncommon | Rarity::Rare)),
            "无色牌只有 uncommon/rare 两档"
        );
    }

    /// 万事通:反编译 BattleContext.cpp:1367-1369 落到 Actions::JackOfAllTradesAction
    /// (Actions.cpp:580-589) —— 基础版抽 1 张、升级版抽 2 张,每张各掷一次
    /// getTrulyRandomColorlessCardInCombat;进手牌时**不动费用**(不是 0 费);
    /// 自己消耗(Cards.h:590-637 的 doesCardExhaust,基础/升级都 true)。
    #[test]
    fn jack_of_all_trades_rules_match_the_decompile() {
        // 基础版:1 张,照牌面收费
        let mut c = staged(
            &["jack_of_all_trades", "strike", "strike", "strike", "strike"],
            &["jack_of_all_trades"],
        );
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 1, "基础版给 1 张");
        let gift = &c.hand[0];
        assert_eq!(cards::pool_of(gift.def), "colorless");
        assert!(matches!(gift.rarity(), Rarity::Uncommon | Rarity::Rare));
        assert!(!gift.free_this_turn, "万事通给的牌照常收费,不是 0 费");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "jack_of_all_trades"),
            "自己消耗"
        );

        // 升级版:2 张,自己仍然消耗
        let mut c = staged(
            &["jack_of_all_trades", "strike", "strike", "strike", "strike"],
            &["jack_of_all_trades"],
        );
        c.hand[0].upgrade();
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 2, "升级版给 2 张");
        assert!(c.hand.iter().all(|x| cards::pool_of(x.def) == "colorless"));
        assert!(c.hand.iter().all(|x| !x.free_this_turn));
        assert!(c.exhaust.iter().any(|x| x.def.id == "jack_of_all_trades"));
    }

    /// 磁力:反编译里这条状态是**空实现** —— Player::applyStartOfTurnPowers 的
    /// `case PS::MAGNETISM:` 只剩一行注释(refs/sts_lightspeed/src/combat/Player.cpp:620-622),
    /// 所以"哪一刻给、给几张"退到牌面文本("At the start of each turn, add a random
    /// Colorless card to your hand.")与参考实现的同名能力。本作按"回合开始、每层各发一张"
    /// (combat.rs start_turn 里的 `for _ in 0..mag`),下面是能自证的部分:
    /// 叠两层 = 每回合两张、连续两回合都发、给的牌照常收费。
    #[test]
    fn magnetism_gifts_one_colorless_per_stack_per_turn() {
        // 牌堆开大一点,两回合内不会重洗,免得上回合收到的牌被抽回来混淆计数
        let mut deck = vec!["magnetism", "magnetism"];
        deck.extend(std::iter::repeat("defend").take(20));
        let mut c = staged(&deck, &["magnetism", "magnetism"]);
        c.play_card(0, None).unwrap();
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::Magnetism), 2, "叠两层");
        for turn in 1..=2 {
            c.end_turn();
            let gifted: Vec<&CardInstance> = c
                .hand
                .iter()
                .filter(|x| cards::pool_of(x.def) == "colorless")
                .collect();
            assert_eq!(gifted.len(), 2, "第 {turn} 回合该给两张无色牌");
            assert!(
                gifted.iter().all(|x| !x.free_this_turn),
                "磁力给的牌照常收费"
            );
        }
    }

    /// 嬗变:反编译 Actions::TransmutationAction(Actions.cpp:591-607) ——
    /// effectAmount = 花的 X + (化学 X ? 2 : 0);每张都是 getTrulyRandomColorlessCardInCombat,
    /// 升级版给的是升级牌、costForTurn 置 0;X=0 且没化学 X 时直接返回(一张都不给)、
    /// 能量照常在打牌时全花掉。基础形态见既有测试 transmutation_pours_x_colorless_cards_into_hand。
    #[test]
    fn transmutation_counts_chemical_x_and_upgrades_like_the_decompile() {
        // 化学 X:X=1,给 1+2=3 张
        let mut c = staged(&["transmutation"], &["transmutation"]);
        c.relics.push(relic_def_or_panic("chemical_x"));
        c.energy = 1;
        c.play_card(0, None).unwrap();
        assert_eq!(c.energy, 0, "X 花光能量");
        assert_eq!(c.hand.len(), 3, "X=1 + 化学 X 的 2 = 3 张");
        assert!(c.hand.iter().all(|x| x.free_this_turn));

        // X=0 且没有化学 X:一张不给
        let mut c = staged(&["transmutation"], &["transmutation"]);
        c.energy = 0;
        c.play_card(0, None).unwrap();
        assert!(c.hand.is_empty(), "X=0 什么都不给");

        // 升级版:给的牌是升级过的
        let mut c = staged(&["transmutation"], &["transmutation"]);
        c.hand[0].upgrade();
        c.energy = 2;
        c.play_card(0, None).unwrap();
        assert_eq!(c.hand.len(), 2);
        assert!(c.hand.iter().all(|x| x.upgraded), "升级版给升级牌");
        assert!(c.hand.iter().all(|x| x.free_this_turn));
    }

    /// 化茧/变形的"抽/落位"时点:反编译 Actions::PutRandomCardsInDrawPile
    /// (Actions.cpp:546-561)分两段 —— 先把 n 张一次抽完(每张一次 cardRandomRng),
    /// 再逐张插进抽牌堆的随机位置(落位同样用 cardRandomRng,抽牌堆为空时用 0、不掷;
    /// 见 CardManager.cpp:87-101 的 drawPile.insert(begin()+idx),idx ∈ [0, size-1])。
    /// 三件事一起卡:①抽牌堆里原有的牌相对顺序不动(旧写法是"塞尾再整体洗",会把老牌洗乱);
    /// ②掷点账目 = n 抽 + n 落位,且完全不碰 shuffleRng;③化茧只洗技能、变形只洗攻击,
    /// 都是本场 0 费、出自本职业池;升级版分别洗 3/5 张(BattleContext.cpp:1261-1262、1388-1389)。
    #[test]
    fn chrysalis_and_metamorphosis_pick_then_place_like_the_decompile() {
        let cases = [
            ("chrysalis", 3u32, crate::core::card::CardType::Skill),
            ("metamorphosis", 3, crate::core::card::CardType::Attack),
        ];
        for (stem, count, kind) in cases {
            // 抽牌堆里摆无色牌:既不会被化茧/变形看中(它们只洗本职业红卡),
            // 也就能干净地验证"原有牌顺序不动"
            let mut c = staged(
                &[stem, "bandage_up", "flash_of_steel", "blind", "forethought"],
                &[stem],
            );
            let before: Vec<&str> = c.draw.iter().map(|x| x.def.id).collect();
            assert_eq!(before.len(), 4, "抽牌堆摆 4 张");
            let cr0 = c.streams.floor(FloorStream::CardRandomRng).counter();
            let sh0 = c.streams.floor(FloorStream::ShuffleRng).counter();
            c.play_card(0, None).unwrap();
            let cr1 = c.streams.floor(FloorStream::CardRandomRng).counter();
            let sh1 = c.streams.floor(FloorStream::ShuffleRng).counter();
            assert_eq!(cr1 - cr0, 2 * count, "{stem}: n 次抽牌 + n 次落位");
            assert_eq!(sh1 - sh0, 0, "{stem}: 不该动 shuffleRng");
            assert_eq!(
                c.draw.len(),
                before.len() + count as usize,
                "{stem}: 洗进去 {count} 张"
            );
            let kept: Vec<&str> = c
                .draw
                .iter()
                .map(|x| x.def.id)
                .filter(|id| before.contains(id))
                .collect();
            assert_eq!(kept, before, "{stem}: 抽牌堆原有的牌顺序不能变");
            assert_eq!(
                c.draw[0].def.id,
                before[0],
                "{stem}: 落点铺满 [1, span],洗进去的牌压不到原堆顶"
            );
            let added: Vec<&CardInstance> = c
                .draw
                .iter()
                .filter(|x| !before.contains(&x.def.id))
                .collect();
            assert_eq!(added.len(), count as usize);
            for x in added {
                assert_eq!(x.kind(), kind, "{stem}: 洗进来的牌类型");
                assert_eq!(cards::pool_of(x.def), "class", "{stem}: 只洗本职业的牌");
                assert!(x.free_combat, "{stem}: 本场 0 费");
                assert_eq!(x.fixed_cost(), Some(0));
            }
        }

        // 升级版(up ? 5 : 3)
        let mut c = staged(&["metamorphosis", "strike", "defend"], &["metamorphosis"]);
        c.hand[0].upgrade();
        let cr0 = c.streams.floor(FloorStream::CardRandomRng).counter();
        c.play_card(0, None).unwrap();
        let cr1 = c.streams.floor(FloorStream::CardRandomRng).counter();
        assert_eq!(c.draw.len(), 2 + 5, "升级版洗 5 张");
        assert_eq!(cr1 - cr0, 10, "5 抽 + 5 落位");
    }

    /// 发现:反编译 generateDiscoveryCards(Game.cpp:228-260)反复掷点,凑够 3 张
    /// **互不相同**的本职业牌 —— 掷到重样的**重掷**(所以 cardRandomRng 可能掷 >3 次),
    /// 不是"抽一张就从池里拿走"的恰好 3 次;牌池是本职业全部非基础牌
    /// (getTrulyRandomCardInCombat(cc) 的 CombatCardPool);挑中的那张本回合 0 费;
    /// 基础版消耗、升级版不消耗(Cards.h:590-637 的 doesCardExhaust(DISCOVERY) = !upgraded)。
    /// 旧写法是从同一个池子连抽三次、允许重复,这条按"换 120 个种子都不许出现重复"卡住它;
    /// 另一条("至少有一次掷点 >3")卡住"抽一张就从池里 remove"的 3 次口径 ——
    /// 参考实现(randomCardDefs,lib.ts:108-117)正是那样,与反编译的流错位。
    #[test]
    fn discovery_offers_three_distinct_class_cards() {
        let mut max_rolls = 0u32;
        for seed in 1u64..=120 {
            let mut c = staged_seed(
                seed,
                &["discovery", "strike", "strike", "strike", "strike", "strike"],
                &["discovery"],
            );
            let cr0 = c.streams.floor(FloorStream::CardRandomRng).counter();
            c.play_card(0, None).unwrap();
            let rolls = c.streams.floor(FloorStream::CardRandomRng).counter() - cr0;
            assert!(rolls >= 3, "seed {seed}: 至少掷 3 次,实际 {rolls}");
            max_rolls = max_rolls.max(rolls);
            let ids: Vec<&str> = {
                let ch = c.choice.as_ref().expect("发现要开选择");
                assert_eq!(ch.offered.len(), 3, "seed {seed}: 亮三张");
                for x in &ch.offered {
                    assert_eq!(cards::pool_of(x.def), "class", "seed {seed}: 本职业池");
                    assert_ne!(x.rarity(), Rarity::Basic, "seed {seed}: 不含基础牌");
                }
                ch.offered.iter().map(|x| x.def.id).collect()
            };
            let mut uniq = ids.clone();
            uniq.sort_unstable();
            uniq.dedup();
            assert_eq!(uniq.len(), 3, "seed {seed}: 三张候选不能重复,实际 {ids:?}");
        }
        assert!(
            max_rolls > 3,
            "120 个种子里必然有掷到重样而重掷的(>3 次),否则说明退回成了 remove-from-pool 的 3 次口径"
        );

        // 挑中的那张:进手牌、本回合 0 费;基础版自己消耗
        let mut c = staged(
            &["discovery", "strike", "strike", "strike", "strike", "strike"],
            &["discovery"],
        );
        c.play_card(0, None).unwrap();
        let idx = c.choice_candidates()[0].0;
        c.choose(idx).unwrap();
        assert_eq!(c.hand.len(), 1, "挑中的进手牌");
        assert!(c.hand[0].free_this_turn, "本回合 0 费");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "discovery"),
            "基础版消耗"
        );

        // 升级版:不消耗
        let mut c = staged(
            &["discovery", "strike", "strike", "strike", "strike", "strike"],
            &["discovery"],
        );
        c.hand[0].upgrade();
        c.play_card(0, None).unwrap();
        let idx = c.choice_candidates()[0].0;
        c.choose(idx).unwrap();
        assert!(
            !c.exhaust.iter().any(|x| x.def.id == "discovery"),
            "升级版不消耗"
        );
    }

    /// 反常/痛苦/嗜血:反编译把前两者记成**手牌计数**(CardManager.cpp:280-310 的
    /// handNormalityCount / handPainCount),闸门分别在 BattleContext.cpp:714(本回合打出
    /// ≥3 张就禁)与 BattleContext.cpp:2656-2661(每打出一张"别的"牌掉 1 血,打出的那张
    /// 痛苦自己不算);嗜血走 CardInstance::tookDamage(CardInstance.cpp:175-181)每掉一次血
    /// 费用 -1。既有测试只覆盖了一层的情形,这里卡边界:两层痛苦 = 每次 2 血、两张反常也
    /// 还是 3 张上限、嗜血在三个牌堆里一起降、战斗中新拿到的继承已降的价。
    #[test]
    fn curse_hand_counters_match_the_decompile() {
        // 两层痛苦:打出一张别的牌掉 2 血
        let mut c = guarded(&["strike"]);
        c.hand = vec![card("pain"), card("pain"), card("strike")];
        c.energy = 3;
        c.play_card(2, Some(0)).unwrap();
        assert_eq!(c.player.hp, 78, "两层痛苦 = 每次 2 血");
        c.hand = vec![card("strike")];
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.player.hp, 78, "痛苦离开手牌就不再掉血");

        // 两张反常:上限仍是 3(计数只判"有没有",不是张数)
        let mut c = guarded(&["defend"]);
        c.hand = vec![
            card("normality"),
            card("normality"),
            card("defend"),
            card("defend"),
            card("defend"),
            card("defend"),
        ];
        c.energy = 10;
        for _ in 0..3 {
            c.play_card(2, None).unwrap();
        }
        assert_eq!(c.cards_played, 3);
        assert!(c.play_card(2, None).is_err(), "第 4 张仍被拦下");

        // 嗜血:掉一次血,手牌/抽牌堆/弃牌堆里的都降 1
        let mut c = staged(&["blood_for_blood"], &["blood_for_blood"]);
        c.draw = vec![card("blood_for_blood")];
        c.discard = vec![card("blood_for_blood")];
        for pile in [&c.hand, &c.draw, &c.discard] {
            assert_eq!(pile[0].cost_value(9), 4, "嗜血基础 4 费");
        }
        c.hit_player(1);
        for pile in [&c.hand, &c.draw, &c.discard] {
            assert_eq!(pile[0].cost_value(9), 3, "掉一次血每个堆里的嗜血都降 1");
        }
        // 战斗中后来拿到的嗜血继承已降的价,不能重置回 4
        let mut fresh = cards::card("blood_for_blood");
        c.fix_new_card(&mut fresh);
        assert_eq!(fresh.cost_value(9), 3, "新拿到的嗜血继承已降的价");
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
        let mut c = staged(
            &["forethought", "strike", "defend"],
            &["forethought", "strike", "defend"],
        );
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

    /// 浩劫放出的带选牌顶牌(燃烧契约):选完牌之后那一截效果(抽 2)不能丢
    #[test]
    fn havoc_plays_the_whole_choice_card() {
        let mut c = guarded(&[]);
        // 两张手牌,燃烧契约的消耗窗口(候选 2 张)才开得出来
        c.hand = vec![card("havoc"), card("strike"), card("strike")];
        c.draw = vec![card("burning_pact"), card("strike"), card("strike")];
        c.energy = 3;
        c.play_card(0, Some(0)).unwrap();
        assert!(c.choice.is_some(), "燃烧契约要挂起选牌");
        c.choose(0).unwrap();
        assert_eq!(c.hand.len(), 3, "选完牌之后要真的抽 2 张");
        assert!(c.draw.is_empty(), "顶牌自己 + 抽走的 2 张都没了");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "burning_pact"),
            "浩劫放出的顶牌打完要消耗"
        );
    }

    /// 死灵之书看的是本回合实际费用(混乱掷出来的 2 费也算),不是印刷费用
    #[test]
    fn necronomicon_counts_the_cost_rolled_by_confusion() {
        let mut c = Combat::new(
            enc("jaw_worm_solo"),
            setup(80, &["strike"], &[relic_def_or_panic("necronomicon")]),
            RngRegistry::new(1),
        );
        c.hand = vec![card("strike")];
        c.draw.clear();
        c.energy = 3;
        c.hand[0].cost_delta = 1; // 混乱把 1 费的打击掷成 2 费
        let hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp - 12, "2 费的打击要被再打一次");
    }

    /// 铜球的停滞偷的是"当前堆里稀有度最高"的那一档,不是均匀随机
    #[test]
    fn stasis_steals_the_highest_rarity_card() {
        let mut c = guarded(&[]);
        c.draw = vec![
            card("strike"),        // Basic
            card("rampage"),       // Common
            card("spot_weakness"), // Uncommon
            card("pummel"),        // Uncommon
            card("bludgeon"),      // Rare
        ];
        c.enemy_steal_card(0, "Bronze Orb");
        assert_eq!(c.stasis.len(), 1);
        assert_eq!(c.stasis[0].1.def.id, "bludgeon", "唯一的稀有牌必被偷");
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
        lock_seed_asc(id, seed, 0)
    }

    /// 同上,但指定飞升等级(开局血量档与遭遇预置的换档都看它)
    fn lock_seed_asc(id: &'static str, seed: u64, asc: u32) -> Combat {
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
        asc,
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

    /// 颚虫三连的开局力量/格挡按飞升换档:A0 3/5、A2 起 4/6、A17 起 5/9
    /// (反编译 MonsterGroup.cpp:278-279 的 `strBuff`/`blockBuff` 三目链);血量则
    /// 跟着 A7 的档走(MonsterSpecific.cpp:34 的 setRandomHp(hpRng, ascension >= 7))
    #[test]
    fn jaw_worm_horde_preset_scales_with_ascension() {
        for (asc, strength, block) in [
            (0, 3, 5),
            (1, 3, 5),
            (2, 4, 6),
            (16, 4, 6),
            (17, 5, 9),
            (20, 5, 9),
        ] {
            let c = lock_seed_asc("jaw_worm_horde", 7, asc);
            for e in &c.enemies {
                assert_eq!(e.statuses.get(Status::Strength), strength, "A{asc} 的开局力量");
                assert_eq!(e.block, block, "A{asc} 的开局格挡");
            }
        }
        let hps = |asc: u32| -> Vec<i32> {
            lock_seed_asc("jaw_worm_horde", 7, asc)
                .enemies
                .iter()
                .map(|e| e.max_hp)
                .collect()
        };
        let a0 = hps(0);
        let a7 = hps(7);
        assert!(a0.iter().all(|h| (40..=44).contains(h)), "A0 血量: {a0:?}");
        assert!(a7.iter().all(|h| (42..=46).contains(h)), "A7 血量: {a7:?}");
    }

    /// 颚虫三连开局那条"招式历史"预置,以及它为什么让首招不再锁死 Chomp.
    ///
    /// 分支清单(全部落在"开怪"这一刻,与飞升无关):
    ///   1. 反编译 MonsterGroup.cpp:274-290 的 JAW_WORM_HORDE:建三只 JAW_WORM,
    ///      然后给每一只写一次 `moveHistory[0] = MMID::DARKLING_REGROW`
    ///      (MonsterGroup.cpp:284-288,一个合法但颚虫永远掷不到的招,注释自述"是什么都行,
    ///      只要不是 INVALID").`moveHistory[1]` 不动,还是 INVALID.
    ///   2. `Monster::firstTurn()`(Monster.cpp:609-611)判的是
    ///      `moveHistory[0] == MMID::INVALID`,所以被预置过的三只 firstTurn 为假;
    ///      单只颚虫 moveHistory 空,firstTurn 为真 → 首招走
    ///      MonsterSpecific.cpp:2451-2453 的"必定咬一口".
    ///   3. 于是三连里每只的首招从第一回合起就走常规分布
    ///      (MonsterSpecific.cpp:2456-2489):roll<25 时看 lastMove(CHOMP) —— 哨兵匹配不上,
    ///      所以直接是 CHOMP;roll<55 时 lastTwoMoves(THRASH) 也不成立,给 THRASH;
    ///      其余走 lastMove(BELLOW) 不成立的 else,给 BELLOW.三个分支都可能.
    ///   4. 本作把这条预置记成 `acted_turns: 1, last_move: None`(enemies.rs 的
    ///      JAW_WORM_HORDE_PRESETS):state.last/prev 都是 None,哨兵匹配不上任何一招,
    ///      与参考侧等价;预置挂在**遭遇**上(MonsterGroup.cpp:274-290 是开怪按遭遇写的),
    ///      不是 innate,所以单只颚虫身上没有.
    ///   5. 与 PREBATTLE 钩子无关:颚虫在 `preBattleAction` 里没有分支
    ///      (MonsterSpecific.cpp:131-300 那串 switch),它在 MonsterSpecific.cpp:52 出现
    ///      是 initHp 的共用分支(掷血量);三连的力量/格挡/历史都在开怪循环里写,
    ///      早于/独立于 PREBATTLE 那批状态(黑暗、觉醒者之类).
    #[test]
    fn jaw_worm_horde_move_history_preset_skips_the_locked_chomp() {
        // 单只颚虫:没有预置,首招锁死咬一口,开局也没有力量/格挡
        let solo_openers: std::collections::HashSet<&str> = (0..128u64)
            .map(|s| move_name(&lock_seed("jaw_worm_solo", s), 0))
            .collect();
        assert_eq!(
            solo_openers,
            ["Chomp"].into_iter().collect(),
            "单只颚虫的首招只有咬一口"
        );
        let solo = lock("jaw_worm_solo");
        assert_eq!(solo.enemies[0].statuses.get(Status::Strength), 0, "预置不是 innate 的");
        assert_eq!(solo.enemies[0].block, 0);
        assert_eq!(solo.enemies[0].state.turns, 0, "没预置就不算行动过");
        assert!(solo.enemies[0].state.last.is_none(), "没有招式历史");

        // 三连:三只都记着"已经行动过一回合 + 一个匹配不到的上一招"
        // (move_rolled 两边掷完首招都是 true,区别在预置先把 firstTurn 压成假,
        //  于是 roll_first_move 里走的是常规分布那一支)
        let horde = lock("jaw_worm_horde");
        for (i, e) in horde.enemies.iter().enumerate() {
            assert_eq!(e.state.turns, 1, "第 {i} 只算已经行动过一回合");
            assert!(e.state.last.is_none(), "第 {i} 只的上一招是匹配不到的哨兵");
            assert!(e.state.prev.is_none());
        }

        // 后果:三只的第一回合不再是清一色 Chomp,常规分布的三个分支都能掷出来
        let openers: std::collections::HashSet<&str> = (0..128u64)
            .map(|s| move_name(&lock_seed("jaw_worm_horde", s), 0))
            .collect();
        for m in ["Chomp", "Thrash", "Bellow"] {
            assert!(openers.contains(m), "三连首招掷不出 {m}:{openers:?}");
        }

        // 那 3 点开局力量要真的进到伤害里(反编译 MonsterSpecific.cpp:850-852 的 11 点
        // 咬一口 + STRENGTH 3;参考实现没有这条预置,首招咬一口是裸 11)
        let mut checked = false;
        for s in 0..128u64 {
            let c = lock_seed("jaw_worm_horde", s);
            if move_name(&c, 0) == "Chomp" {
                assert_eq!(c.enemy_attack_damage(0, 11), 14, "咬一口 11 + 3 力量");
                checked = true;
            }
        }
        assert!(checked, "128 个种子里没掷出咬一口");
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

    /// 飞升 18+ 的 Gremlin Nob 换成固定节奏:头槌只在"最近两招里没有头槌"时出,
    /// 于是 Bellow 之后是 头槌、冲锋、冲锋 循环,与掷点无关(原版/wiki;反编译那段
    /// asc18 分支写成恒为冲锋是转写错误,见语料 monsters-act1.json 的 conflicts).
    #[test]
    fn gremlin_nob_a18_locks_the_skull_bash_rush_rush_pattern() {
        use crate::core::cards::card;
        let seq = |asc: u32, seed: u64| -> Vec<&'static str> {
            let enc = crate::core::enemies::encounter_def("gremlin_nob_solo").unwrap();
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
            let mut c = Combat::new(enc, setup, RngRegistry::new(seed));
            let mut v = Vec::new();
            for _ in 0..7 {
                v.push(move_name(&c, 0));
                c.end_turn();
            }
            v
        };
        let want = vec![
            "Bellow",
            "Skull Bash",
            "Rush",
            "Rush",
            "Skull Bash",
            "Rush",
            "Rush",
        ];
        // 两个不同种子跑出同一条节奏 → 这个分支不掷点
        assert_eq!(seq(18, 7), want, "A18 的节奏变了");
        assert_eq!(seq(20, 12345), want, "A20 的节奏变了");
    }

    /// 镀甲(线轴遗物给的):只有没被格挡住的"攻击"伤害才掉一层,死亡律动这类
    /// 非攻击伤害不掉.依据反编译:掉层的句子在 Player::attacked 里,而死亡律动走
    /// Actions::DamagePlayer → Player::damage(),那条路上没有掉层.
    #[test]
    fn plated_armor_only_shreds_on_unblocked_attack_damage() {
        let mut c = lock("cultist_solo");
        c.player.statuses.set(Status::PlatedArmor, 4);
        c.hit_player(3);
        assert_eq!(
            c.player.statuses.get(Status::PlatedArmor),
            4,
            "非攻击伤害不掉层"
        );
        c.hit_player_attack(3);
        assert_eq!(
            c.player.statuses.get(Status::PlatedArmor),
            3,
            "攻击伤害掉一层"
        );
        c.gain_block(50, false, false);
        c.hit_player_attack(5);
        assert_eq!(
            c.player.statuses.get(Status::PlatedArmor),
            3,
            "被格挡的攻击不掉层"
        );
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
    fn guardian_twin_slam_arms_the_next_shift_ten_higher() {
        // 反编译 refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:1344-1351(双拳合击):miscInfo += 10 之后才以它重装
        // MODE_SHIFT,而 miscInfo 开局就是 30,所以额度按 40/50/60 一路长.此前本作把它钉死
        // 成 40:守护者第二次以后都提前切换,act1 的守护者战(acts seed 22882/26951)整条
        // hp 轨迹跟着偏.
        let twin = |c: &Combat| {
            c.enemies[0]
                .def
                .moves
                .iter()
                .position(|m| m.name == "Twin Slam")
                .expect("守护者要有双拳合击")
        };
        let mut c = lock("the_guardian");
        c.damage_enemy(0, 30); // 第一次切换
        assert!(!c.enemies[0].statuses.has(Status::ModeShift));
        let twin = twin(&c);
        c.enemies[0].next_move = twin;
        c.end_turn(); // 第一记双拳合击:装回 30 + 10
        assert_eq!(c.enemies[0].statuses.get(Status::ModeShift), 40);
        // 掉 39 点还差一点才切,再掉 1 点才切 —— 说明阈值确实是 40 而不是更小
        c.damage_enemy(0, 39);
        assert_eq!(c.enemies[0].statuses.get(Status::ModeShift), 1);
        c.damage_enemy(0, 1);
        assert!(!c.enemies[0].statuses.has(Status::ModeShift), "掉够 40 才切");
        // 第二记双拳合击:50
        c.enemies[0].next_move = twin;
        c.end_turn();
        assert_eq!(c.enemies[0].statuses.get(Status::ModeShift), 50);
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
    use crate::core::relics::relic_def_or_panic;

    /// 打一场指定遭遇:80 血,牌组给几张打击/防御
    fn lock(id: &'static str) -> Combat {
        lock_asc(id, 0)
    }

    /// 同上,但指定飞升等级(开局血量档、预置换档、招式换档都看它)
    fn lock_asc(id: &'static str, asc: u32) -> Combat {
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
            asc,
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

    /// 招式分派的 `_ =>` 兜底(act2 三处):脚本里"最后一招/指定招之后"接哪一招.
    /// 圆球者 Activate→Attack Debuff→Slam→Harden,之后 Slam/Harden 交替
    /// (act2.rs 的 pick_spheric_guardian `_ => SLAM`;参考 sphericGuardian.ts:56-68);
    /// 熊 Bear Hug→Lunge→Maul,之后 Lunge/Maul 交替(act2.rs 的 pick_bear `_ => LUNGE`);
    /// 罗密欧 Mock→Agonizing→Cross,之后 Agonizing/Cross 交替(pick_romeo `_ => AGONIZING_SLASH`)
    #[test]
    fn act2_enemy_script_fallbacks_choose_the_next_move() {
        // (遭遇, 敌人 id, 上一招下标, 期望的下一招下标)
        let cases = [
            ("spheric_guardian_solo", "spheric_guardian", 3, 2), // Harden -> Slam
            ("event_bandits", "bear", 2, 1),                     // Maul -> Lunge
            ("event_bandits", "romeo", 2, 1),                    // Cross Slash -> Agonizing Slash
        ];
        for (enc, enemy, last, want) in cases {
            let mut c = lock(enc);
            let i = idx_of(&c, enemy);
            c.enemies[i].state.last = Some(last);
            c.pick_next_move(i);
            assert_eq!(
                c.enemies[i].next_move, want,
                "{enemy} 上一招 {last} 之后该接 {want}"
            );
        }
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

    /// 靴子 The Boot 表驱动:未被格挡的攻击伤害只剩 1..4 点时提到 5.
    /// 出处(不依赖参考实现,直接对反编译):
    ///   - refs/sts_lightspeed/src/combat/Monster.cpp:339-341 的 `attackedUnblockedHelper`
    ///     是全工程唯一一处 Boot:`damage > 0 && damage < 5 -> damage = 5`;
    ///   - 它只在 Monster.cpp:437-438 被 `Monster::attacked` 调用,而 attacked 先按无形压 1
    ///     (Monster.cpp:418-421)、再做 `damage -= block`(Monster.cpp:430-431);
    ///     所以 Boot 判的是"扣完格挡、过完目标侧减免之后"的剩余值,完全被格挡时因为
    ///     进不到 helper(Monster.cpp:437 的 `if (damage > 0)`)而不抬;
    ///   - 飞行减半在玩家侧伤害计算里(BattleContext.cpp:2733-2735 的 `FLIGHT -> *0.5`),
    ///     排在 Monster::attacked 之前,所以 4 点打飞行怪 = 5 而不是 2.
    #[test]
    fn boot_boosts_unblocked_damage_after_reductions() {
        let boot = crate::core::relics::relic_def_or_panic("the_boot");
        let booted = |id: &'static str| {
            let mut c = lock(id);
            c.relics.push(boot);
            c
        };

        // 表:(描述, 招式伤害, 目标已有格挡, 无形, 飞行, 期望实际掉血)
        let cases: &[(&str, i32, i32, bool, bool, i32)] = &[
            ("1 点 -> 5", 1, 0, false, false, 5),
            ("2 点 -> 5", 2, 0, false, false, 5),
            ("3 点 -> 5", 3, 0, false, false, 5),
            ("4 点 -> 5", 4, 0, false, false, 5),
            ("5 点已经是 5,不抬", 5, 0, false, false, 5),
            ("无形:4 先压成 1,再抬到 5(不是 1)", 4, 0, true, false, 5),
            ("飞行:4 先减半成 2,再抬到 5(不是 2)", 4, 0, false, true, 5),
            ("6 点打 1 格挡,剩 5 不抬", 6, 1, false, false, 5),
            ("4 点打 3 格挡只剩 1,抬到 5", 4, 3, false, false, 5),
            ("完全被格挡:4 点打 4 格挡,不抬", 4, 4, false, false, 0),
        ];
        for (what, dmg, block, intangible, flight, expect) in cases.iter().copied() {
            let mut c = booted("jaw_worm_solo");
            c.enemies[0].block = block;
            if intangible {
                c.add_enemy_status(0, Status::Intangible, 2);
            }
            if flight {
                c.add_enemy_status(0, Status::Flight, 3);
            }
            let hp = c.enemies[0].hp;
            assert_eq!(c.damage_enemy(0, dmg), expect, "{what}:实际掉血");
            assert_eq!(c.enemies[0].hp, hp - expect, "{what}:血量");
            assert_eq!(c.enemies[0].block, (block - dmg).max(0), "{what}:格挡照扣");
        }

        // 真实飞行怪(拜德)也对得上:4 点先被飞行减半成 2
        let mut c = booted("three_byrds");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "拜德的飞行把 4 减半成 2 再抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);

        // 没有靴子时 4 点还是 4
        let mut c = lock("jaw_worm_solo");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 4);
        assert_eq!(c.enemies[0].hp, hp - 4);
    }

    /// Boot 对多段攻击逐段各判一次:反编译里每一段攻击都各走一遍
    /// Monster::attacked -> attackedUnblockedHelper(Monster.cpp:407-438),
    /// 所以 4x3 的每一段都被抬到 5,合计 15;单段 4 也是 5.
    #[test]
    fn boot_applies_to_each_hit_segment() {
        let boot = crate::core::relics::relic_def_or_panic("the_boot");
        let mut c = lock("jaw_worm_solo");
        c.relics.push(boot);
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy_times(0, 4, true, 3), 15, "3 段各 4 -> 各 5");
        assert_eq!(c.enemies[0].hp, hp - 15);

        let mut c = lock("jaw_worm_solo");
        c.relics.push(boot);
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy_times(0, 4, true, 1), 5, "单段 4 -> 5");
        assert_eq!(c.enemies[0].hp, hp - 5);
    }

    /// 指定血量/遗物开一场(开局回血这类"进战斗瞬间"的行为要在这里看)
    fn with_relics(
        id: &'static str,
        hp: i32,
        max_hp: i32,
        relics: Vec<&'static crate::core::relics::RelicDef>,
    ) -> Combat {
        let enc = crate::core::enemies::encounter_def(id)
            .unwrap_or_else(|| panic!("no such encounter {id}"));
        let setup = CombatSetup {
            rested: false,
            hp,
            max_hp,
            deck: vec![card("strike"); 5],
            relics,
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        Combat::new(enc, setup, RngRegistry::new(11))
    }

    /// 血瓶(Blood Vial):每次战斗开始回 2 血.依据 relics.rs 的 blood_vial
    /// (combat_start_heal: 2)与 combat.rs 战斗初始化的 start_heal.参考实现同样在
    /// 开战时回血,差别只在导出器把这次回血算进 move 行还是随后的战斗行
    /// (act2.script seed 13 的第 33 步),不是规则差异.
    #[test]
    fn blood_vial_heals_two_at_the_start_of_every_combat() {
        let vial = relic_def_or_panic("blood_vial");
        let c = with_relics("jaw_worm_solo", 50, 80, vec![vial]);
        assert_eq!(c.player.hp, 52, "开战回 2");
    }

    /// 制图仪(Pantograph):只在 Boss 房开战时回 25,普通怪房间不回.依据 relics.rs 的
    /// pantograph(boss_combat_heal: 25)与 combat.rs 里 `enc.kind == Boss` 的判定
    /// (原版的回血遗物按房间类型决定是否触发).a20a4 seed 33 的差异只是导出器把这次
    /// 回血算进 move 行还是战斗行,终局一致.
    #[test]
    fn pantograph_heals_twenty_five_only_in_boss_combats() {
        let panto = relic_def_or_panic("pantograph");
        let boss = with_relics("the_champ", 50, 80, vec![panto]);
        assert_eq!(boss.player.hp, 75, "Boss 房开战回 25");
        let normal = with_relics("jaw_worm_solo", 50, 80, vec![panto]);
        assert_eq!(normal.player.hp, 50, "普通房不回");
    }

    /// 史莱姆每个怪物回合都掷一次 aiRng.原版 MonsterGroup::doMonsterTurn 里每只怪都
    /// rollMove(掷点可能不用);参考实现自述史莱姆首回合之后不再掷(ENGINE-GAP rng
    /// parity,见其 content/monsters/act1/slimes.ts:71-74),a20a3 seed 510 的 mindbloom
    /// 幻影史莱姆首领战因此整段错位.这里钉住本作"每回合照掷一次".
    #[test]
    fn slimes_roll_one_ai_rng_per_turn() {
        let mut c = lock("lots_of_slimes");
        let i = c
            .enemies
            .iter()
            .position(|e| e.def.id == "acid_slime_small")
            .expect("一堆史莱姆里必有一只小酸液");
        let only = c.enemies.remove(i);
        c.enemies = vec![only];
        let ai = |c: &mut Combat| c.streams.floor(FloorStream::AiRng).counter();
        for turn in 1..=3 {
            let before = ai(&mut c);
            c.end_turn();
            assert_eq!(ai(&mut c) - before, 1, "第 {turn} 回合也要掷一次");
        }
    }

    /// Boot 只抬"玩家打出去的攻击伤害".反编译里攻击走 Monster::attacked ->
    /// attackedUnblockedHelper(Monster.cpp:407-438);非攻击伤害走 Monster::damage ->
    /// damageUnblockedHelper(Monster.cpp:442-448 / 466-493),那条路径里根本没有 Boot.
    /// 玩家自己吃到的伤害(灼烧/荆棘/死亡律动)在 Player 侧结算,也不经过 Boot.
    #[test]
    fn boot_does_not_boost_non_attack_damage() {
        let boot = crate::core::relics::relic_def_or_panic("the_boot");
        // 敌方吃到的非攻击伤害(Monster::damage 那条):4 点不抬
        let mut c = lock("jaw_worm_solo");
        c.relics.push(boot);
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy_plain(0, 4), 4, "非攻击伤害不吃 Boot");
        assert_eq!(c.enemies[0].hp, hp - 4);

        // 真实的非攻击来源:火焰屏障的荆棘反伤打敌人,也不抬
        let mut c = lock("jaw_worm_solo");
        c.relics.push(boot);
        c.player.statuses.add(Status::FlameBarrier, 4);
        let hp = c.enemies[0].hp;
        c.enemy_attack(0, 10, 1, "Jaw Worm", "Chomp");
        assert_eq!(c.enemies[0].hp, hp - 4, "火焰屏障反伤 4 点不被 Boot 抬");

        // 玩家自己挨的伤害:4 点照收(Boot 是加伤遗物,不加自己受的伤)
        let mut c = lock("jaw_worm_solo");
        c.relics.push(boot);
        let hp = c.player.hp;
        let (taken, _) = c.hit_player(4);
        assert_eq!(taken, 4, "玩家吃到的伤害不被 Boot 抬");
        assert_eq!(c.player.hp, hp - 4);
        let hp = c.player.hp;
        c.lose_hp_player(3, true);
        assert_eq!(c.player.hp, hp - 3, "直接掉血(灼烧/死亡律动)不被 Boot 抬");
    }

    /// 靴子的 1..4 -> 5 在"真实怪 + 它自带的减伤/挨打机制"同时挂上时照样成立.
    /// 反编译的落点是 Monster::attackedUnblockedHelper(Monster.cpp:339-342)的**最开头**,
    /// 它排在无敌/镀甲/卷曲/飞行/延展/荆棘/睡眠/移形换影那串 else-if(Monster.cpp:348-396)
    /// 之前 —— 那套机制一个都不改这一击的数值,只各自做副作用.本作把这步放在
    /// hit_enemy_final 里(扣完格挡、过完飞行/慢速/无形之后,见 combat.rs 的
    /// small_attack_boost_to),顺序与参考一致.
    ///
    /// 用真实怪把每种机制各枚举一遍(飞行/甲壳/卷曲在第一章、第二章的怪身上,
    /// 无形是第三章精英复仇女神的),最后再把四种叠在一只第三章的怪身上看数值.
    #[test]
    fn boot_floor_holds_on_real_monsters_with_their_own_mechanics() {
        let boot = relic_def_or_panic("the_boot");
        let booted = |id: &'static str| {
            let mut c = lock(id);
            c.relics.push(boot);
            c
        };

        // 无形(复仇女神,NEMESIS 的 special = Intangible):行动完自带两层;
        // 4 点先被压成 1,再抬到 5
        let mut c = booted("nemesis_solo");
        c.player.hp = 999;
        assert!(!c.enemies[0].statuses.has(Status::Intangible), "开场还不无形");
        c.end_turn();
        assert!(c.enemies[0].statuses.has(Status::Intangible), "行动完变无形");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "无形 + 靴子:压到 1 也得抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);

        // 飞行(拜德,innate Flight 3):4 点先减半成 2,再抬到 5;飞行掉一层
        let mut c = booted("three_byrds");
        assert_eq!(c.enemies[0].statuses.get(Status::Flight), 3);
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "飞行 + 靴子:减半成 2 也得抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);
        assert_eq!(c.enemies[0].statuses.get(Status::Flight), 2, "飞行掉一层");

        // 甲壳(甲壳寄生虫,innate PlatedArmor 14,开局自带 14 格挡):
        // 18 点先被格挡吃掉 14,剩 4 点照样抬到 5;掉血就掉一层甲
        let mut c = booted("shelled_parasite_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::PlatedArmor), 14);
        assert_eq!(c.enemies[0].block, 14, "开局自带 14 格挡");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 18), 5, "甲壳 + 靴子:格挡后剩 4 抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);
        assert_eq!(c.enemies[0].block, 0, "14 点格挡照扣");
        assert_eq!(c.enemies[0].statuses.get(Status::PlatedArmor), 13, "掉血就掉一层甲");

        // 卷曲(虱子,spawn 时掷一个 Curl Up):这一击抬到 5,卷曲当场合上换格挡
        let mut c = booted("two_louses");
        let curl = c.enemies[0].statuses.get(Status::CurlUp);
        assert!(curl > 0, "虱子开局带卷曲");
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "卷曲 + 靴子");
        assert_eq!(c.enemies[0].hp, hp - 5);
        assert_eq!(c.enemies[0].block, curl, "卷曲的格挡在挨打之后才补上");
        assert_eq!(c.enemies[0].statuses.get(Status::CurlUp), 0, "一次性用完");

        // 真实分叉那场:第三章的扭曲团块(Malleable 4 / Reactive 1,延展越打格挡越多).
        // 它靠格挡把每一刀磨到 1..4,正是靴子"挡后剩 1..4 抬到 5"露头的场合
        // (act3 seed 69/284 那两条差异就出在这只怪身上)
        let mut c = booted("writhing_mass_solo");
        assert_eq!(c.enemies[0].statuses.get(Status::Malleable), 4, "开局延展 4");
        assert_eq!(c.damage_enemy(0, 10), 10, "第一刀:没格挡");
        assert_eq!(c.enemies[0].block, 4, "延展补 4 格挡");
        assert_eq!(c.damage_enemy(0, 10), 6, "第二刀:4 挡掉,剩 6 不抬");
        assert_eq!(c.enemies[0].block, 5, "延展长到 5");
        assert_eq!(c.damage_enemy(0, 10), 5, "第三刀:5 挡掉,正好剩 5 不抬");
        assert_eq!(c.enemies[0].block, 6);
        assert_eq!(c.damage_enemy(0, 10), 5, "第四刀:6 挡掉剩 4,抬到 5");
        assert_eq!(c.enemies[0].block, 7);

        // 四种机制同时挂在一只第三章的怪身上:这一击还是 5 点
        // (靴子的抬升排在它们之后,不被它们摊薄)
        let mut c = booted("nemesis_solo");
        for (s, n) in [
            (Status::Intangible, 2),
            (Status::Flight, 3),
            (Status::PlatedArmor, 9),
            (Status::CurlUp, 8),
        ] {
            c.add_enemy_status(0, s, n);
        }
        let hp = c.enemies[0].hp;
        assert_eq!(c.damage_enemy(0, 4), 5, "四种机制叠一起也抬到 5");
        assert_eq!(c.enemies[0].hp, hp - 5);
    }

    /// Battle Trance 的"本回合不能再抽牌":反编译把闸门放在**唯一**的抽牌入口上
    /// —— refs/sts_lightspeed/src/combat/BattleContext.cpp:2439-2444 的
    /// `BattleContext::drawCards` 开头就是
    /// `count <= 0 || player.hasStatus<PS::NO_DRAW>() || ... -> return`,
    /// 全工程的抽牌都从它进(Actions::DrawCards -> drawCards),所以"所有入口"其实就是
    /// 这一个闸门.本作同构:闸门落在 combat.rs:1485-1493 的 `Combat::draw_cards` 开头.
    /// 这条证据不依赖参考实现:列的是反编译里的入口清单,逐个触发本作的同名入口.
    #[test]
    fn no_draw_vetoes_every_draw_entry_point() {
        use crate::core::card::CardType;
        let no_draw = |c: &mut Combat| c.player.statuses.add(Status::NoDraw, 1);
        let vetoed = |c: &Combat| c.log.iter().any(|l| l.text.contains("No Draw"));

        // 1) 出牌抽:拨击(Pommel Strike)打 9 抽 1
        let mut c = lock("jaw_worm_solo");
        c.hand = vec![card("pommel_strike")];
        c.energy = 3;
        no_draw(&mut c);
        c.play_card(0, Some(0)).unwrap();
        assert!(c.hand.is_empty(), "出牌抽被 NoDraw 卡住");
        assert!(vetoed(&c), "出牌抽走到了 draw_cards 的闸门");

        // 2) 遗物抽:掉血抽(第一次掉血同时触发百年拼图 3 张 + 符文方块 1 张)
        let mut c = lock("jaw_worm_solo");
        c.relics.push(relic_def_or_panic("centennial_puzzle"));
        c.relics.push(relic_def_or_panic("runic_cube"));
        let hand = c.hand.len();
        no_draw(&mut c);
        c.lose_hp_player(1, false);
        assert_eq!(c.hand.len(), hand, "掉血触发的遗物抽被卡住");
        assert!(vetoed(&c));

        // 3) 遗物抽:墨水瓶(打满 10 张抽 1)
        let mut c = lock("jaw_worm_solo");
        c.relics.push(relic_def_or_panic("ink_bottle"));
        let hand = c.hand.len();
        no_draw(&mut c);
        c.rs.cards_total = 9;
        c.note_card_played(CardType::Attack);
        assert_eq!(c.hand.len(), hand, "墨水瓶抽被卡住");

        // 4) 遗物抽:不休陀螺(手里空了补 1)
        let mut c = lock("jaw_worm_solo");
        c.relics.push(relic_def_or_panic("unceasing_top"));
        c.hand = vec![card("defend")];
        no_draw(&mut c);
        c.play_card(0, None).unwrap();
        assert!(c.hand.is_empty(), "不休陀螺补牌被卡住");

        // 5) 能力抽:残暴(自己回合开始抽,掉血那一下不受 NoDraw 影响)
        let mut c = lock("jaw_worm_solo");
        let hand = c.hand.len();
        let hp = c.player.hp;
        no_draw(&mut c);
        c.player.statuses.add(Status::Brutality, 1);
        c.start_turn(0);
        assert_eq!(c.hand.len(), hand, "残暴的抽牌被卡住");
        assert_eq!(c.player.hp, hp - 1, "残暴的掉血照旧");

        // 6) 能力抽:死亡拥抱(消耗一张就抽 1)
        let mut c = lock("jaw_worm_solo");
        c.hand = vec![card("defend")];
        let hand = c.hand.len();
        no_draw(&mut c);
        c.player.statuses.add(Status::DarkEmbrace, 1);
        c.exhaust_card(card("strike"));
        assert_eq!(c.hand.len(), hand, "死亡拥抱的抽牌被卡住");

        // 7) 药水抽:迅捷药水抽 3
        let mut c = lock("jaw_worm_solo");
        let hand = c.hand.len();
        no_draw(&mut c);
        let potion = crate::core::potions::by_id("swift_potion").unwrap();
        c.use_potion(potion, None);
        assert_eq!(c.hand.len(), hand, "迅捷药水抽被卡住");

        // 8) 回合开始的基础抽牌:同一个闸门
        let mut c = lock("jaw_worm_solo");
        let hand = c.hand.len();
        no_draw(&mut c);
        c.start_turn(0);
        assert_eq!(c.hand.len(), hand, "回合开始的抽牌也过 draw_cards");
    }

    /// NoDraw 只否决"抽牌",不否决"把牌加到手牌 / 放到抽牌堆顶"这类操作
    /// (反编译的 NO_DRAW 只判在 drawCards 上,BattleContext.cpp:2439-2444).
    #[test]
    fn no_draw_does_not_veto_adding_cards_to_hand() {
        // 磁力:回合开始随机塞一张无色牌进手牌(是加牌,不是抽牌)
        let mut c = lock("jaw_worm_solo");
        c.player.statuses.add(Status::NoDraw, 1);
        c.player.statuses.add(Status::Magnetism, 1);
        let hand = c.hand.len();
        c.start_turn(0);
        assert_eq!(c.hand.len(), hand + 1, "加牌不受 NoDraw 限制");

        // 枯枝:消耗时把一张随机牌塞进手牌(不是抽牌)
        let mut c = lock("jaw_worm_solo");
        c.relics.push(relic_def_or_panic("dead_branch"));
        c.hand = vec![card("defend")];
        c.player.statuses.add(Status::NoDraw, 1);
        let hand = c.hand.len();
        c.exhaust_card(card("strike"));
        assert_eq!(c.hand.len(), hand + 1, "枯枝塞牌不受 NoDraw 限制");

        // Battle Trance 自己:先抽 3 再挂 NoDraw(顺序不能反,否则自己都抽不到)
        let mut c = lock("jaw_worm_solo");
        c.hand = vec![card("battle_trance")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert!(c.player.statuses.has(Status::NoDraw), "打完挂上 NoDraw");
        assert_eq!(c.hand.len(), 3, "先抽的那 3 张照抽");
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

    /// 瞬变体的移形换影(Shifting)是**临时**力量,以及它到底在哪一刻补回来.
    ///
    /// 分支清单:
    ///   1. 开局 `buff<SHIFTING>()` + `buff<FADING>(asc17 ? 6 : 5)`
    ///      (MonsterSpecific.cpp:172-174 的 preBattleAction).
    ///   2. 挨打(攻击 `Monster::attacked` 与非攻击 `Monster::damage` 两条路都算)
    ///      当下 `addDebuff<STRENGTH>(-damage)`,力量可以掉成负数;
    ///      同时 `buff<SHACKLED>(damage)` 把这份损失记下来
    ///      (Monster.cpp:393-395 与 453-455).
    ///   3. 回补点在 `Monster::applyEndOfTurnTriggers`:`buff<STRENGTH>(SHACKLED)`
    ///      + 清掉 SHACKLED(Monster.cpp:63-66).这个函数由
    ///      `BattleContext::applyEndOfRoundPowers`(BattleContext.cpp:2132-2150)调用,
    ///      后者的调用点在 `afterMonsterTurns`(BattleContext.cpp:2152-2156),
    ///      也就是**怪物都行动完之后** —— 所以它这一轮的来袭仍然带着被削掉的力量,
    ///      下一轮才恢复满力.
    ///   4. 本作同构:当下真扣、损失记在 `Enemy::temp_strength`,在 `start_turn`
    ///      (combat.rs,一层 end_turn 把敌人阶段走完之后)整块补回.
    ///      这就是 seed 197 分叉那个"临时力量回补时点":本作按反编译把它放在怪物行动之后.
    #[test]
    fn transient_shifting_strength_is_temporary_and_returns_after_its_action() {
        let mut c = lock("transient_solo");
        c.player.hp = 999;
        // 第 1 回合:力量 0,来袭 30
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 30);
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 0, "还没挨打");
        // 玩家回合打它 40 点:力量当场 -40,另记 40 待回补
        assert_eq!(c.damage_enemy(0, 40), 40);
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), -40, "当下真扣,允许为负");
        assert_eq!(c.enemies[0].temp_strength, 40, "损失记着回补");
        // 估算口径(UI/策略读的预估来袭)与结算同一条路:也带上被削掉的力量
        assert_eq!(c.predicted_damage(0), (0, 1), "估算:基础 40 - 40 = 0");
        // 它这一轮的攻击(基础 40)因此被削到 0:回补排在它行动之后
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 0, "同一轮来袭仍用被削掉的力量");
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 0, "行动完就补回来");
        assert_eq!(c.enemies[0].temp_strength, 0);
        assert_eq!(c.predicted_damage(0), (50, 1), "回补后预估跟着回到 50");
        // 下一轮恢复满力:基础 50
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 50, "下一轮不再打折");
    }

    /// 瞬变体的倒计时与每回合加伤按飞升换档:
    ///   Fading 层数 = asc >= 17 ? 6 : 5(MonsterSpecific.cpp:172-175)
    ///   首击 = asc >= 2 ? 40 : 30,每回合 +10
    ///   (MonsterSpecific.cpp:1500-1502 的 `(asc2 ? 40 : 30) + 10*(turnNumber-1)`)
    /// 倒计时递减也在攻击里:打完这一击若 Fading == 1 就退场(MonsterSpecific.cpp:1502-1506),
    /// 所以 5 层打 5 击、6 层打 6 击.
    #[test]
    fn transient_fading_and_damage_scale_with_ascension() {
        for (asc, first, second, fading) in [(0u32, 30, 40, 5), (2, 40, 50, 5), (17, 40, 50, 6)] {
            let mut c = lock_asc("transient_solo", asc);
            c.player.hp = 999;
            assert_eq!(c.enemies[0].statuses.get(Status::Fading), fading, "A{asc} 的倒计时");
            let hp = c.player.hp;
            c.end_turn();
            assert_eq!(hp - c.player.hp, first, "A{asc} 首击");
            let hp = c.player.hp;
            c.end_turn();
            assert_eq!(hp - c.player.hp, second, "A{asc} 第二击");
        }
        // A17 的 6 层多撑一击:前 5 击之后还在场,第 6 击(90)打完才消失
        let mut c = lock_asc("transient_solo", 17);
        c.player.hp = 999;
        let mut taken = 0;
        for _ in 0..5 {
            let hp = c.player.hp;
            c.end_turn();
            taken += hp - c.player.hp;
        }
        assert_eq!(taken, 40 + 50 + 60 + 70 + 80);
        assert!(c.enemies[0].up(), "A17 的倒计时还没走完");
        let hp = c.player.hp;
        c.end_turn();
        assert_eq!(hp - c.player.hp, 90, "第 6 击");
        assert!(c.enemies[0].escaped, "第 6 击打完才消失");
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

    /// 不休陀螺不补"被时间扭曲掐掉的那一回合":第 12 张牌打空了手牌,也不补牌.
    /// 依据反编译 BattleContext.cpp:802-815 的主循环 —— endTurnQueued 的分支先
    /// continue,底下那段"手里空了补一张"根本走不到(那里还压着 assert(!endTurnQueued)).
    #[test]
    fn unceasing_top_does_not_refill_after_time_warp() {
        let mut c = lock("time_eater");
        c.relics
            .push(crate::core::relics::relic_def_or_panic("unceasing_top"));
        c.hand.clear();
        for _ in 0..12 {
            let mut inst = cards::card("strike");
            inst.cost_delta = -1;
            c.hand.push(inst);
        }
        c.energy = 12;
        for _ in 0..12 {
            if c.force_end_turn || c.phase != Phase::PlayerTurn {
                break;
            }
            let _ = c.play_card(0, Some(0));
        }
        assert!(c.force_end_turn, "第 12 张牌打完就该结束回合");
        assert!(!c.draw.is_empty(), "抽牌堆还有牌,补不补才看得出差别");
        assert!(c.hand.is_empty(), "回合已被掐掉,不休陀螺不该补牌");

        // 反例对照:不是时间扭曲掐掉的那一张(第 11 张)打空手牌照常补
        let mut c = lock("time_eater");
        c.relics
            .push(crate::core::relics::relic_def_or_panic("unceasing_top"));
        c.hand.clear();
        for _ in 0..11 {
            let mut inst = cards::card("strike");
            inst.cost_delta = -1;
            c.hand.push(inst);
        }
        c.energy = 11;
        for _ in 0..11 {
            if c.force_end_turn || c.phase != Phase::PlayerTurn {
                break;
            }
            let _ = c.play_card(0, Some(0));
        }
        assert!(!c.force_end_turn, "第 11 张还不结束回合");
        assert_eq!(c.hand.len(), 1, "手里空了不休陀螺补一张");
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

    /// 心脏的 Buff 招式挂着 EnemyFx::Escalate(act34.rs:1588),每用一次 stage 递增,
    /// 第 5 档起落到 heart_escalate 的 `_ =>`(combat.rs:2580):反编译
    /// MonsterSpecific.cpp:1816-1835 的 default 分支给 50 Strength.前几档一并钉住.
    #[test]
    fn heart_escalation_grants_the_decompiled_stage_values() {
        // Buff 招式确实挂着 Escalate,否则下面的档数永远不会递增
        let heart = crate::core::enemies::enemy_def("corrupt_heart").unwrap();
        let buff = heart.moves.iter().find(|m| m.name == "Buff").unwrap();
        assert!(
            buff.effects.iter().any(|fx| matches!(fx, EnemyFx::Escalate)),
            "Buff 招式没有 Escalate"
        );
        let mut c = lock("the_heart");
        let base = c.enemies[0].statuses.get(Status::Strength);
        c.heart_escalate(0, 1, "Corrupt Heart");
        assert_eq!(c.enemies[0].statuses.get(Status::Artifact), 2);
        c.heart_escalate(0, 2, "Corrupt Heart");
        assert_eq!(c.enemies[0].statuses.get(Status::BeatOfDeath), 2);
        c.heart_escalate(0, 3, "Corrupt Heart");
        assert_eq!(c.enemies[0].statuses.get(Status::PainfulStabs), 1);
        c.heart_escalate(0, 4, "Corrupt Heart");
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), base + 10);
        c.heart_escalate(0, 5, "Corrupt Heart");
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), base + 60);
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

    /// 冠军的怒吼先把自己的负力量归零,再加 6 点:原版 Monster::removeDebuffs
    /// (反编译 refs/sts_lightspeed/src/combat/Monster.cpp:538-543)在清减益前把负力量抬回 0,所以被缴械
    /// 削到 -3 的冠军怒吼之后是 6 点力量而不是 3 点.本作原先的 clear_debuffs 只 retain
    /// 非减益状态(力量算增益,负力量也跟着留下),于是比原版少一截力量.
    #[test]
    fn champ_anger_floors_negative_strength_before_buffing() {
        let mut c = lock("the_champ");
        let champ = idx_of(&c, "the_champ");
        c.enemies[champ].statuses.add(Status::Strength, -3);
        let anger = c.enemies[champ]
            .def
            .moves
            .iter()
            .position(|m| m.name == "Anger")
            .unwrap();
        c.enemies[champ].next_move = anger;
        c.end_turn();
        assert_eq!(
            c.enemies[champ].statuses.get(Status::Strength),
            6,
            "缴械后的 -3 要先归零,再加怒吼的 6 点"
        );
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

    /// 半死暗灵照常在自己槽位行动,而且自己那一个回合恰好消耗一次 aiRng(rollMove).
    /// 出处(反编译,不依赖参考实现):
    ///  - refs/sts_lightspeed/src/combat/MonsterGroup.cpp:573-580 的 doMonsterTurn 守卫是
    ///    `(!m.isDeadOrEscaped() || m.isHalfDead())`,半死那只照样 takeTurn;
    ///  - MonsterSpecific.cpp:3014-3020 的 getMove:`if (halfDead) return DARKLING_REINCARNATE;`
    ///    —— 半死时选招不走掷点分档;
    ///  - MonsterSpecific.cpp:1459-1478:DARKLING_REGROW 与 DARKLING_REINCARNATE 收尾各自
    ///    `rollMove(bc)`(rollMove 必消耗一次 aiRng.random(99),Monster.cpp:629-635),
    ///    REINCARNATE 半血复活.
    /// 本作把"回合末那一次 rollMove"落成 enemy_end_of_turn 里的 pick_next_move.下面把
    /// 另外两只的后继写死(forced)让它们各只消耗一次,好把多出来的那次归给半死的那只.
    #[test]
    fn half_dead_darkling_acts_in_slot_and_burns_one_roll_per_turn() {
        let mut c = lock("three_darklings");
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        let ai = |c: &mut Combat| c.streams.floor(FloorStream::AiRng).counter();
        let move0 = |c: &Combat| c.enemies[0].def.moves[c.enemies[0].next_move].name;
        let pos = |c: &Combat, needle: &str| c.log.iter().position(|l| l.text.contains(needle));

        // 开局三只各 roll_first_move 一次
        assert_eq!(ai(&mut c), 3);
        // 玩家回合把 0 号打到 0(还有同伴在)-> 半死,摆 Regrow,没掷点
        c.enemies[0].hp = 1;
        c.damage_enemy(0, 20);
        c.settle_deaths();
        assert!(c.enemies[0].state.half_dead);
        assert_eq!(c.enemies[0].hp, 0);
        assert_eq!(move0(&c), "Regrow");
        assert_eq!(ai(&mut c), 3, "半死不掷点(反编译里 die() 是直接 setMove)");

        // 1、2 号后继写死成 HARDEN:forced 分支固定只消耗一次 aiRng.random(99)
        let force = |c: &mut Combat| {
            c.enemies[1].state.forced = Some(2);
            c.enemies[2].state.forced = Some(2);
        };
        force(&mut c);
        c.log.clear();
        let before = ai(&mut c);
        c.end_turn();
        assert_eq!(ai(&mut c) - before, 3, "forced 两只各 1 次 + 半死那只 1 次");
        assert_eq!(move0(&c), "Reincarnate", "Regrow 回合末换成 Reincarnate");
        assert!(c.enemies[0].state.half_dead, "这一轮还没站起来");
        assert!(pos(&c, "Darkling #1 regrows").is_none(), "Revive 还没发生");

        // 下一轮:Reincarnate,自己回合末以半血复活,且槽 0 先动
        force(&mut c);
        c.log.clear();
        let hp0 = c.enemies[0].hp;
        let before = ai(&mut c);
        c.end_turn();
        assert!(
            ai(&mut c) - before >= 3,
            "forced 两只各 1 次,复活那只照常 rollMove 至少 1 次"
        );
        assert!(!c.enemies[0].state.half_dead, "第二轮结束站起来");
        assert!(c.enemies[0].hp > hp0, "以半血复活");
        assert_ne!(move0(&c), "Regrow");
        assert_ne!(move0(&c), "Reincarnate");
        let regrow_at = pos(&c, "Darkling #1 regrows").expect("复活有日志");
        let two_at = pos(&c, "Darkling #2 gains").expect("2 号有行动日志");
        assert!(regrow_at < two_at, "槽 0 先动,之后才轮到 1/2 号");
    }

    /// 三只暗灵同一瞬全倒下:没有同伴可依,多出来的那两只半死尸体直接变真死,战斗结束,
    /// 不会集体复活.出处:反编译 Monster::die 一进门 `--monstersAlive`,到 0 就判
    /// PLAYER_VICTORY 并 return(Monster.cpp:284-295),胜负看 monstersAlive<=0
    /// (MonsterGroup.cpp:14/37);本作对应 handle_death 里"同族都倒下了,半死的那些
    /// 也一起彻底死掉"(combat.rs:3997-4000 附近).
    #[test]
    fn darklings_dying_together_do_not_revive() {
        let mut c = lock("three_darklings");
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        for i in 0..c.enemies.len() {
            c.enemies[i].hp = 1;
        }
        // 顺劈:一次打全体 8
        c.hand = vec![card("cleave")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(c.phase, Phase::Won, "全倒 -> 直接胜利");
        for e in c.enemies.iter() {
            assert!(!e.state.half_dead, "没有半死复活");
            assert!(e.death_done);
        }
    }

    /// 留一只活着时,先倒下的两只半死等复活;打死最后一只才结束,半死的两只一起作废.
    #[test]
    fn last_living_darkling_death_discards_the_corpses() {
        let mut c = lock("three_darklings");
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        for i in 0..2 {
            c.enemies[i].hp = 1;
            c.damage_enemy(i, 20);
            c.settle_deaths();
            assert!(c.enemies[i].state.half_dead, "还有同伴时先半死");
        }
        c.enemies[2].hp = 1;
        c.damage_enemy(2, 20);
        c.check_win();
        assert_eq!(c.phase, Phase::Won);
        for e in c.enemies.iter() {
            assert!(!e.state.half_dead, "最后一只倒下,半死的也真死");
        }
    }

    /// 半死暗灵在自己回合被荆棘反杀时,复活倒计时不少算一轮.
    /// 依据(反编译,不依赖参考):
    ///  - Player::attacked 把荆棘反伤 addToTop(Player.cpp:227-232),排在 takeTurn 末尾
    ///    addToBot(Actions::RollMove) 之前 -> 死亡先落地,那一次 rollMove 才轮得到;
    ///  - 因此那一次选招已经看到 isHalfDead,getMove 直接返回 REINCARNATE
    ///    (MonsterSpecific.cpp:3014-3020),意图当场就是 Reincarnate;
    ///  - 下一轮它执行 REINCARNATE,以 maxHp/2 站起来(MonsterSpecific.cpp:1464-1469),
    ///    不会再多空过一轮 REGROW.
    /// 有同伴活着时死亡只是半死(Monster.cpp:296-306),骰子照常每回合烧一次.
    #[test]
    fn darkling_killed_by_thorns_revives_on_its_next_turn() {
        let mut c = lock("three_darklings");
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        c.player.statuses.add(Status::Thorns, 30);
        let ai = |c: &mut Combat| c.streams.floor(FloorStream::AiRng).counter();
        let move0 = |c: &Combat| c.enemies[0].def.moves[c.enemies[0].next_move].name;
        let max0 = c.enemies[0].max_hp;
        // 0 号首招定为 Nip(会攻击),残血保证被一次反伤打死;同伴后继写死少掺和
        c.enemies[0].next_move = 0;
        c.enemies[0].hp = 3;
        let force = |c: &mut Combat| {
            c.enemies[1].state.forced = Some(2);
            c.enemies[2].state.forced = Some(2);
        };
        force(&mut c);
        let before = ai(&mut c);
        c.end_turn();
        assert!(c.enemies[0].state.half_dead, "被反杀只是半死");
        assert_eq!(c.enemies[0].hp, 0);
        assert_eq!(move0(&c), "Reincarnate", "死在掷点之前,意图当场就是 Reincarnate");
        assert_eq!(ai(&mut c) - before, 3, "两只 forced 各 1 次 + 半死那只 1 次");
        // 下一轮就是 REINCARNATE:半血复活,不多摆一轮 REGROW
        force(&mut c);
        c.end_turn();
        assert!(!c.enemies[0].state.half_dead, "下一轮就复活");
        assert_eq!(c.enemies[0].hp, max0 / 2, "半血复活");
        assert_ne!(move0(&c), "Regrow", "不再有 REGROW 意图");
        assert_ne!(move0(&c), "Reincarnate", "复活后掷的是普通招");
    }

    /// 暗灵复活:原地复活(同槽位/同出生号,不是新怪)、半血、REGROW 重新挂上能再半死一次;
    /// 哲学家的石头让 REINCARNATE 额外 +1 力量(MonsterSpecific.cpp:1472-1474).
    #[test]
    fn darkling_reincarnates_in_place_with_optional_stone_strength() {
        // 不带石头:复活后力量 0
        let mut plain = lock("three_darklings");
        plain.player.hp = 999;
        plain.enemies[0].hp = 10;
        plain.damage_enemy_plain(0, 20);
        plain.settle_deaths();
        assert!(plain.enemies[0].state.half_dead);
        let uid0 = plain.enemies[0].uid;
        let slot0 = plain.enemies[0].slot;
        let max0 = plain.enemies[0].max_hp;
        for _ in 0..3 {
            if plain.enemies[0].state.half_dead && plain.phase == Phase::PlayerTurn {
                plain.end_turn();
            }
        }
        assert!(!plain.enemies[0].state.half_dead, "两轮后复活");
        assert_eq!(plain.enemies[0].hp, max0 / 2, "半血复活");
        assert_eq!(plain.enemies[0].uid, uid0, "还是原来那只怪(出生号不变)");
        assert_eq!(plain.enemies[0].slot, slot0, "槽位不变");
        assert_eq!(
            plain.enemies[0].statuses.get(Status::Strength),
            0,
            "没石头就没有额外力量"
        );
        assert_eq!(
            plain.enemies[0].statuses.get(Status::Regrow),
            1,
            "复活后 REGROW 重新挂上"
        );
        // 第二次倒下还能半死(REGROW 被 REINCARNATE 重新武装)
        plain.enemies[0].hp = 5;
        plain.damage_enemy_plain(0, 20);
        plain.settle_deaths();
        assert!(plain.enemies[0].state.half_dead, "第二次倒下照样半死");

        // 带石头:开战敌力 +1,半死清零,复活只看 REINCARNATE 的 +1
        let stone = relic_def_or_panic("philosophers_stone");
        let mut c = with_relics("three_darklings", 80, 80, vec![stone]);
        c.player.hp = 999;
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            1,
            "开战敌力 +1"
        );
        c.enemies[0].hp = 10;
        c.damage_enemy_plain(0, 20);
        c.settle_deaths();
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            0,
            "半死清掉力量(含开战那 1 点)"
        );
        for _ in 0..3 {
            if c.enemies[0].state.half_dead && c.phase == Phase::PlayerTurn {
                c.end_turn();
            }
        }
        assert!(!c.enemies[0].state.half_dead);
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            1,
            "REINCARNATE 带石头 +1 力量"
        );
    }

    /// 半死瞬间:清空全部状态与力量(反编译 resetAllStatusEffects,Monster.cpp:554-558),
    /// 变成不可选中(isTargetable = !isDeadOrEscaped,Monster.cpp:237-255),
    /// 群体攻击(顺劈)因此跳过它,不会再被"打死一次".
    #[test]
    fn half_dead_darkling_is_stripped_and_untargetable() {
        let mut c = lock("three_darklings");
        c.player.hp = 9999;
        c.player.max_hp = 9999;
        c.enemies[0].statuses.add(Status::Strength, 5);
        c.enemies[0].statuses.add(Status::Vulnerable, 4);
        c.enemies[0].hp = 10;
        c.damage_enemy_plain(0, 20);
        c.settle_deaths();
        assert!(c.enemies[0].state.half_dead);
        assert_eq!(c.enemies[0].statuses.get(Status::Strength), 0, "半死清掉力量");
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 0, "半死清掉减益");
        assert_eq!(c.enemies[0].statuses.get(Status::Regrow), 1, "只留 REGROW");
        assert!(!c.enemies[0].alive(), "半死不可选中");
        assert!(c.enemies[0].up(), "但仍在战斗里,还要回合");
        // 顺劈打全体:半死那只不受影响,也不会被"再打死一次"
        let hp1 = c.enemies[1].hp;
        c.hand = vec![card("cleave")];
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert!(
            c.enemies[0].state.half_dead && c.enemies[0].hp == 0,
            "群体攻击跳过半死尸体"
        );
        assert!(c.enemies[1].hp < hp1, "活着的同伴照常吃顺劈");
        assert_eq!(c.phase, Phase::PlayerTurn, "还有两只活着,没赢");
    }

    /// 开局:每只暗灵各掷一次 aiRng.random(99) 定首招,roll<50 摆 HARDEN 否则 NIP
    /// (MonsterSpecific.cpp:3005-3010).镜像一条同种子的 aiRng 流核对阈值两侧.
    #[test]
    fn darkling_first_move_uses_the_half_threshold() {
        let mut c = lock("three_darklings");
        // 镜像流:同种子重放,取出引擎开局替三只怪各掷的那一次
        let mut mirror = RngRegistry::new(11);
        let rolls: Vec<i32> = (0..3)
            .map(|_| mirror.floor(FloorStream::AiRng).random(99) as i32)
            .collect();
        for (i, r) in rolls.iter().enumerate() {
            let want = if *r < 50 { "Harden" } else { "Nip" };
            assert_eq!(
                c.enemies[i].def.moves[c.enemies[i].next_move].name, want,
                "第 {i} 只首招:roll {r} -> {want}"
            );
        }
        assert_eq!(
            c.streams.floor(FloorStream::AiRng).counter(),
            3,
            "开局只掷三次首招"
        );
    }

    /// 常规选招级联的"逐掷点可执行规格":镜像一条同种子的 aiRng 流,按反编译的判定树
    /// (MonsterSpecific.cpp:3012-3044)从同一段掷点推出期望招,与引擎每回合每只怪
    /// 选出来的招逐条比对.镜像只要与引擎多掷或少掷一次就整段错位,所以这条同时钉住了
    /// "每次 rollMove 必掷一次 aiRng.random(99),落进 40-99 补掷分支或整棵递归时额外掷".
    #[test]
    fn darkling_cascade_replays_the_decompiled_decision_tree() {
        const NIP: usize = 0;
        const CHOMP: usize = 1;
        const HARDEN: usize = 2;
        // 反编译 getMove 的判定树,掷点从镜像流取.counts 记两条"额外掷点"分支
        // (40-99 补掷 / 整树递归)各被走到多少次,末尾用来确认这条规格不是空跑.
        let expect = |rng: &mut RngRegistry,
                      idx: usize,
                      last: Option<usize>,
                      prev: Option<usize>,
                      counts: &mut [u32; 2]|
         -> usize {
            let mut r = rng.floor(FloorStream::AiRng).random(99) as i32;
            if last.is_none() {
                // 开局那一掷:roll<50 -> HARDEN,否则 NIP
                return if r < 50 { HARDEN } else { NIP };
            }
            loop {
                if r < 40 {
                    if last != Some(CHOMP) && idx != 1 {
                        return CHOMP;
                    }
                    // 不能用 Chomp 时补掷 40-99,继续往下走
                    counts[0] += 1;
                    r = rng.floor(FloorStream::AiRng).random_range(40, 99);
                }
                if r < 70 {
                    return if last != Some(HARDEN) { HARDEN } else { NIP };
                }
                if !(last == Some(NIP) && prev == Some(NIP)) {
                    return NIP;
                }
                // 两次 Nip 之后的兜底:整棵判定树拿新掷点重跑
                counts[1] += 1;
                r = rng.floor(FloorStream::AiRng).random(99) as i32;
            }
        };
        let mut counts = [0u32; 2];

        for seed in 0..24u64 {
            let enc = crate::core::enemies::encounter_def("three_darklings").expect("三只暗灵");
            let setup = CombatSetup {
                rested: false,
                hp: 9999,
                max_hp: 9999,
                deck: vec![card("defend"); 10],
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
                asc: 0,
            };
            let mut c = Combat::new(enc, setup, RngRegistry::new(seed));
            let mut mirror = RngRegistry::new(seed);
            for i in 0..3 {
                let want = expect(&mut mirror, i, None, None, &mut counts);
                assert_eq!(c.enemies[i].next_move, want, "seed {seed}:第 {i} 只首招");
            }
            for round in 1..=6 {
                c.end_turn();
                for i in 0..3 {
                    // 每只怪的选招发生在它自己行动之后,历史就是它当前的 last/prev
                    let want = expect(
                        &mut mirror,
                        i,
                        c.enemies[i].state.last,
                        c.enemies[i].state.prev,
                        &mut counts,
                    );
                    assert_eq!(
                        c.enemies[i].next_move, want,
                        "seed {seed} 第 {round} 轮:第 {i} 只"
                    );
                }
            }
            // 整场跑下来镜像与引擎掷点流应当正好同步
            assert_eq!(
                c.streams.floor(FloorStream::AiRng).counter(),
                mirror.floor(FloorStream::AiRng).counter(),
                "seed {seed}:掷点次数"
            );
        }
        assert!(counts[0] > 20, "40-99 补掷分支没被走到:{}", counts[0]);
        assert!(counts[1] > 0, "整树递归兜底没被走到:{}", counts[1]);
    }

    /// 常规选招的四条硬约束(MonsterSpecific.cpp:3022-3044;语料 historyRules):
    ///  - CHOMP 不连续两次;
    ///  - CHOMP 永不出现在中间那只(出生下标 1);
    ///  - HARDEN 不连续两次;
    ///  - NIP 不连续三次.
    /// 多颗种子连打若干回合逐回合校验;顺带确认三只活怪每回合各至少掷一次(rollMove).
    #[test]
    fn darkling_move_history_rules_hold_over_many_turns() {
        const NIP: usize = 0;
        const CHOMP: usize = 1;
        const HARDEN: usize = 2;
        for seed in 0..40u64 {
            let enc = crate::core::enemies::encounter_def("three_darklings").expect("三只暗灵");
            let setup = CombatSetup {
                rested: false,
                hp: 9999,
                max_hp: 9999,
                deck: vec![card("defend"); 10],
                relics: Vec::new(),
                gold: 0,
                lift_strength: 0,
                relic_counters: RunRelicCounters::default(),
                curse_negate: 0,
                asc: 0,
            };
            let mut c = Combat::new(enc, setup, RngRegistry::new(seed));
            let n = c.enemies.len();
            let mut hist: Vec<Vec<usize>> = vec![Vec::new(); n];
            for turn in 1..=6 {
                let before = c.streams.floor(FloorStream::AiRng).counter();
                c.end_turn();
                assert!(
                    c.streams.floor(FloorStream::AiRng).counter() - before >= n as u32,
                    "seed {seed} 第 {turn} 回合:三只活怪每只至少掷一次"
                );
                for (i, h) in hist.iter_mut().enumerate() {
                    let last = c.enemies[i].state.last.expect("acted");
                    if i == 1 {
                        assert_ne!(last, CHOMP, "seed {seed}:中间那只从不用 Chomp");
                    }
                    if let Some(prev) = h.last() {
                        assert!(
                            !(*prev == CHOMP && last == CHOMP),
                            "seed {seed} 第 {turn} 回合:Chomp 连着两次"
                        );
                        assert!(
                            !(*prev == HARDEN && last == HARDEN),
                            "seed {seed} 第 {turn} 回合:Harden 连着两次"
                        );
                    }
                    if h.len() >= 2
                        && last == NIP
                        && h[h.len() - 1] == NIP
                        && h[h.len() - 2] == NIP
                    {
                        panic!("seed {seed} 第 {turn} 回合:Nip 连着三次");
                    }
                    h.push(last);
                }
            }
        }
    }

    /// "三只全半死 -> 全复活"在原版里不可达:die() 一进门 --monstersAlive,
    /// 到 0 直接判 PLAYER_VICTORY 并 return(Monster.cpp:284-295),不会走半死那支.
    /// 三种击杀顺序枚举:前两只半死之后第三只一死就结束,半死尸体一起作废.
    #[test]
    fn darklings_never_reach_three_half_dead_in_any_kill_order() {
        for (first, second, last) in [(0usize, 1usize, 2usize), (0, 2, 1), (1, 2, 0)] {
            let mut c = lock("three_darklings");
            c.player.hp = 9999;
            c.player.max_hp = 9999;
            for i in [first, second] {
                c.enemies[i].hp = 1;
                c.damage_enemy_plain(i, 20);
                c.settle_deaths();
                assert!(c.enemies[i].state.half_dead, "还有同伴在就先半死");
                assert_eq!(c.phase, Phase::PlayerTurn, "两只半死还不结束");
            }
            c.enemies[last].hp = 1;
            c.damage_enemy_plain(last, 20);
            c.settle_deaths();
            c.check_win();
            assert_eq!(c.phase, Phase::Won, "最后一只真死 -> 立刻胜利");
            assert!(!c.enemies[last].state.half_dead, "最后一只不留半死");
            for i in [first, second] {
                assert!(!c.enemies[i].state.half_dead, "半死的两只一起作废");
                assert!(c.enemies[i].dead());
            }
        }
    }

    /// 瞬变体每回合攻击 +10(反编译 MonsterSpecific.cpp:1498-1502:
    /// damage = (asc2 ? 40 : 30) + 10 * (getMonsterTurnNumber() - 1)).
    #[test]
    fn transient_attack_grows_ten_per_turn() {
        let mut c = lock("transient_solo");
        c.player.hp = 999;
        for (turn, want) in [(1, 30), (2, 40), (3, 50)] {
            let hp = c.player.hp;
            c.end_turn();
            assert_eq!(hp - c.player.hp, want, "第 {turn} 回合");
        }
    }
}


/// 召唤物的站位:参考实现里每只怪占一个固定槽位,召唤物进的是指定的空槽,
/// 不是队尾;行动顺序也就按槽位从左到右排
#[cfg(test)]
mod summon_tests {
    use super::*;
    use crate::core::cards::card;
    use crate::core::relics::relic_def_or_panic;

    /// 手牌里某张牌的下标(测试用)
    fn hand_idx(c: &Combat, id: &str) -> usize {
        c.hand
            .iter()
            .position(|x| x.def.id == id)
            .unwrap_or_else(|| panic!("手里没有 {id}"))
    }

    /// 一场可摆布的战斗:手牌/抽牌堆按参数摆好,遗物挂上,能量 9
    fn staged(deck: &[&str], hand: &[&str], relics: &[&'static RelicDef]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: deck.iter().map(|id| card(id)).collect(),
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
            c.hand.push(card(id));
        }
        for id in deck {
            c.draw.push(card(id));
        }
        c.energy = 9;
        c
    }

    /// 指定遭遇的战斗:牌组 10 张打击、遗物挂上、能量 9
    fn combat_of(encounter: &'static str, relics: &[&'static RelicDef]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: vec![card("strike"); 10],
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        let enc = crate::core::enemies::encounter_def(encounter).expect("遭遇应存在");
        Combat::new(enc, setup, RngRegistry::new(21))
    }

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

    /// 铜制自动机的固定脚本在飞升 19+ 下"光束后接增幅":反编译 BRONZE_AUTOMATON_HYPER_BEAM
    /// 分支是 `if (asc19) setMove(BOOST); else setMove(STUNNED)`
    /// (refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:492-499).本作原先一律回 Stunned,
    /// 飞升 20 下光束后少一次增幅(格挡+力量),a20a2 seed 3 整场差 10 hp.
    #[test]
    fn automaton_hyper_beam_chains_into_boost_at_a19() {
        fn auto_at(asc: u32) -> Combat {
            let enc =
                crate::core::enemies::encounter_def("bronze_automaton").expect("automaton 遭遇");
            Combat::new(
                enc,
                CombatSetup {
                    rested: false,
                    hp: 300,
                    max_hp: 300,
                    deck: vec![card("strike"); 10],
                    relics: Vec::new(),
                    gold: 0,
                    lift_strength: 0,
                    relic_counters: RunRelicCounters::default(),
                    curse_negate: 0,
                    asc,
                },
                RngRegistry::new(7),
            )
        }
        // 脚本:放球 → 连枷 → 增幅 → 连枷 → 增幅 → 光束,再看下一招
        let chain = ["Spawn Orbs", "Flail", "Boost", "Flail", "Boost", "Hyper Beam"];
        for (asc, after) in [(0u32, "Stunned"), (20u32, "Boost")] {
            let mut c = auto_at(asc);
            for want in chain {
                let i = idx_of(&c, "bronze_automaton");
                let next_move = c.enemies[i].next_move;
                assert_eq!(
                    c.enemies[i].def.moves[next_move].name, want,
                    "飞升 {asc}: 脚本该走到 {want}"
                );
                use_move(&mut c, "bronze_automaton", next_move);
            }
            let i = idx_of(&c, "bronze_automaton");
            assert_eq!(
                c.enemies[i].def.moves[c.enemies[i].next_move].name, after,
                "飞升 {asc}: 光束之后应是 {after}"
            );
        }
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

    /// 鸟居:只把"扣掉格挡之后"剩下的 1..5 点攻击伤害降到 1
    /// (反编译 Player::attacked 的顺序:格挡 -> 鸟居 -> 钨钢棒)
    #[test]
    fn torii_reduces_unblocked_attack_damage_only() {
        let relics = vec![relic_def_or_panic("torii")];
        let mut c = staged(&["strike"; 5], &[], &relics);
        // 5 点攻击 3 点格挡 -> 剩 2 点,鸟居降到 1
        c.player.block = 3;
        c.player.hp = 50;
        let (taken, blocked) = c.hit_player_attack(5);
        assert_eq!(blocked, 3);
        assert_eq!(taken, 1, "格挡后剩 2 点,鸟居降到 1");
        assert_eq!(c.player.hp, 49);
        // 完全被格挡时鸟居无从参与
        c.player.block = 5;
        c.player.hp = 50;
        let (taken, _) = c.hit_player_attack(5);
        assert_eq!(taken, 0);
        assert_eq!(c.player.hp, 50);
        // 6 点(超过 5)不降
        c.player.hp = 50;
        let (taken, _) = c.hit_player_attack(6);
        assert_eq!(taken, 6);
    }

    /// 钨钢棒排在鸟居之后:被减到 0 就不算掉血(镀甲不掉层、嗜血不降费)
    #[test]
    fn tungsten_rod_applies_after_torii() {
        let relics = vec![
            relic_def_or_panic("torii"),
            relic_def_or_panic("tungsten_rod"),
        ];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.player.block = 2;
        c.player.hp = 50;
        c.player.statuses.add(Status::PlatedArmor, 3);
        let (taken, _) = c.hit_player_attack(5);
        assert_eq!(taken, 0, "5-2=3 -> 鸟居 1 -> 钨钢棒 0");
        assert_eq!(c.player.hp, 50);
        assert_eq!(
            c.player.statuses.get(Status::PlatedArmor),
            3,
            "掉血被减到 0 就不掉镀甲"
        );
    }

    /// 荆棘:多段攻击每段各反一次(反编译里每次 attacked 都反)
    #[test]
    fn thorns_reflect_each_hit_of_a_multi_attack() {
        let relics = vec![relic_def_or_panic("bronze_scales")];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.enemies[0].hp = 999;
        c.enemies[0].max_hp = 999;
        c.player.hp = 80;
        c.enemy_attack(0, 1, 3, "test", "triple");
        assert_eq!(c.enemies[0].hp, 990, "3 段各反 3 点 = 9(不是整段只反一次)");
    }

    /// 小鬼号角:首领倒下后"逃跑"的随从不算击杀
    #[test]
    fn gremlin_horn_does_not_count_escaped_minions() {
        let relics = vec![relic_def_or_panic("gremlin_horn")];
        let mut c = combat_of("gremlin_gang", &relics);
        // 一只逃跑(首领倒下带走的随从就是这个状态),另一只真死
        c.enemies[1].escaped = true;
        c.enemies[1].death_done = true;
        c.enemies[0].hp = 0;
        c.energy = 3;
        let hand = c.hand.len();
        c.settle_deaths();
        assert_eq!(c.energy, 4, "只有真死的那一只给能量");
        assert_eq!(c.hand.len(), hand + 1, "并抽 1 张");
    }

    /// 蓝蜡烛的掉血算自伤:会触发破裂(反编译 PlayerLoseHp(1, true))
    #[test]
    fn blue_candle_hp_loss_triggers_rupture() {
        let relics = vec![relic_def_or_panic("blue_candle")];
        let mut c = staged(&["injury"; 6], &["injury"], &relics);
        c.player.statuses.add(Status::Rupture, 2);
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::Strength), 2, "破裂加 2 力量");
    }

    /// 仙女在瓶中的保命回血吃神圣树皮(30% -> 60%),且排在蜥蜴尾巴之前
    #[test]
    fn fairy_potion_is_doubled_by_sacred_bark_and_wins_over_lizard_tail() {
        let relics = vec![
            relic_def_or_panic("sacred_bark"),
            relic_def_or_panic("lizard_tail"),
        ];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.fairy_save = true;
        c.player.hp = 3;
        c.hit_player(999);
        assert_eq!(c.player.hp, 48, "最大生命 80 的 60%");
        assert!(c.fairy_used, "先用药水");
        assert!(!c.rs.lizard_used, "反过来蜥蜴尾巴留着");
    }

    /// 魔法花把保命符的回血量也放大 3/2:反编译 wouldDie 走 Player::heal,
    /// heal 里 MAGIC_FLOWER 把 amount 乘 3/2(Player.cpp:156-170 / 320-345)。
    /// 仙女 30% -> 45%(80 上限给 36),蜥蜴尾 50% -> 75%(给 60)。
    #[test]
    fn magic_flower_boosts_death_save_heals() {
        // 仙女在瓶中:80 的 30% = 24,魔法花 → 36
        let relics = vec![relic_def_or_panic("magic_flower")];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.fairy_save = true;
        c.player.hp = 3;
        c.hit_player(999);
        assert_eq!(c.player.hp, 36, "仙女 30% 被魔法花放大到 45%");
        assert_ne!(c.phase, Phase::Lost);

        // 蜥蜴尾巴:80 的 50% = 40,魔法花 → 60
        let relics = vec![
            relic_def_or_panic("magic_flower"),
            relic_def_or_panic("lizard_tail"),
        ];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.player.hp = 3;
        c.hit_player(999);
        assert_eq!(c.player.hp, 60, "蜥蜴尾 50% 被魔法花放大到 75%");
        assert_ne!(c.phase, Phase::Lost);
    }

    /// 花开彼岸:两种保命符都不生效(反编译 wouldDie 整段被跳过)
    #[test]
    fn mark_of_the_bloom_blocks_fairy_and_lizard_tail() {
        let relics = vec![relic_def_or_panic("mark_of_the_bloom")];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.fairy_save = true;
        c.player.hp = 3;
        c.hit_player(999);
        assert_eq!(c.phase, Phase::Lost, "仙女救不了");
    }

    /// 花开彼岸:战斗内的一切治疗归零(反编译 Player::heal 第一句就 return)
    #[test]
    fn mark_of_the_bloom_blocks_in_combat_healing() {
        let relics = vec![relic_def_or_panic("mark_of_the_bloom")];
        let mut c = staged(&["strike"; 5], &[], &relics);
        c.player.hp = 50;
        c.heal_player(10);
        assert_eq!(c.player.hp, 50, "鸟面坛/再生/血药水这些都回不了血");
    }

    /// 橙皮:连打三类型清减益时,连 Flex 挂的 "Lose Strength" 也要清掉
    #[test]
    fn orange_pellets_clears_lose_strength_and_negative_stats() {
        let relics = vec![relic_def_or_panic("orange_pellets")];
        let mut c = staged(&["inflame"; 5], &["inflame"], &relics);
        c.player.statuses.add(Status::LoseStrength, 2);
        c.player.statuses.add(Status::Vulnerable, 3);
        c.player.statuses.add(Status::Strength, 2);
        c.rs.types_played = 7;
        c.play_card(0, None).unwrap();
        assert!(!c.player.statuses.holds(Status::LoseStrength), "清掉 Lose Strength");
        assert!(!c.player.statuses.holds(Status::Vulnerable), "清掉易伤");
        assert_eq!(c.player.statuses.get(Status::Strength), 4, "正面力量留着(2+炽热攻击 2)");
    }

    /// 木乃伊之手:腐化之下技能本回合已是 0 费,不参选(原版候选要求 costForTurn>0)
    #[test]
    fn mummified_hand_skips_corrupted_skills() {
        let relics = vec![relic_def_or_panic("mummified_hand")];
        let mut c = staged(
            &["inflame", "defend", "defend", "defend"],
            &["inflame", "defend"],
            &relics,
        );
        c.player.statuses.add(Status::Corruption, 1);
        c.play_card(0, None).unwrap();
        let defend = hand_idx(&c, "defend");
        assert!(
            !c.hand[defend].free_this_turn,
            "腐化下技能本来就 0 费,不该被选成木乃伊之手的随机目标"
        );
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

    /// 尼尔瑞的抄本:回合结束亮三张,挑中的"洗进"抽牌堆(只掷一次 cardRandomRng
    /// 取插入位,不动 shuffleRng、不整堆重洗 —— 反编译 sts_lightspeed
    /// src/combat/CardManager.cpp:215-221 的 shuffleIntoDrawPile),选完才轮到对面
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
        // 亮三张用 cardRandomRng(3 次);插入位再掷 1 次 cardRandomRng,
        // shuffleRng 一步都不许动(原先整堆 java_shuffle 会多掷一条流)
        let card_before = c.streams.floor(FloorStream::CardRandomRng).counter();
        let shuffle_before = c.streams.floor(FloorStream::ShuffleRng).counter();
        c.choose(0).unwrap();
        assert!(c.choice.is_none());
        assert!(c.pending_end_turn == false, "选完接着走回合尾巴");
        assert_eq!(
            c.streams.floor(FloorStream::CardRandomRng).counter(),
            card_before + 1,
            "只掷一次插入位"
        );
        assert_eq!(
            c.streams.floor(FloorStream::ShuffleRng).counter(),
            shuffle_before,
            "抄本不整堆重洗,不动 shuffleRng"
        );
        // 插进抽牌堆之后回合尾巴接着抽 5 张,插进去的那张可能已经被抽上手
        assert!(
            c.draw.iter().chain(c.hand.iter()).any(|k| k.def.id == picked),
            "挑中的进抽牌堆(抽到手上也算)"
        );
        assert!(
            !c.discard.iter().any(|k| k.def.id == picked),
            "挑中的不该落到弃牌堆"
        );
        assert_eq!(c.turn, 2, "回合已经交给对面并回到自己");
    }

    /// 抄本在 headless 对拍里被折平(参考实现没实现这件遗物):
    /// 回合结束既不亮牌也不掷 cardRandomRng,与"跳过"一致
    #[test]
    fn nilrys_codex_is_suppressed_in_headless_replay() {
        let relics = vec![relic_def_or_panic("nilrys_codex")];
        let mut c = staged(&["strike"; 10], &["defend"], &relics);
        c.suppress_codex = true;
        let card_before = c.streams.floor(FloorStream::CardRandomRng).counter();
        let draw_before = c.draw.len();
        c.end_turn();
        assert!(c.choice.is_none(), "折平后不亮牌");
        assert_eq!(
            c.streams.floor(FloorStream::CardRandomRng).counter(),
            card_before,
            "折平后连掷点都没有"
        );
        assert_eq!(
            c.draw.len(),
            draw_before - DRAW_PER_TURN,
            "抽牌堆只少开局抽的五张,没有多的牌被塞进来"
        );
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

    /// 扭曲的钳子:照反编译在**抽牌后**升级一张手牌,且掷点账 = 动一次 shuffleRng、
    /// 不动 miscRng;候选只含"还能升级"的牌(一张都没有就整段不掷点)。
    /// 反编译 Actions.cpp:940-962(筛 canUpgrade → 空则 return → 否则 shuffleRng.randomLong()
    /// 喂 JavaRandom 洗下标表取第一张);参考实现把钩子挂在抽牌前的 atStartOfTurn(event.ts:237),
    /// 那一刻手里没牌,永远不升级,所以这条不能靠参考对拍,只能自证。
    #[test]
    fn warped_tongs_upgrades_after_draw_off_shuffle_rng() {
        let relics = vec![relic_def_or_panic("warped_tongs")];
        // 抽牌堆全是未升级的打击;手里空.回合开始先抽 5 张,再触发升级(=抽牌后)
        let mut c = staged(&["strike"; 12], &[], &relics);
        c.hand.clear();
        c.draw = (0..12).map(|_| cards::card("strike")).collect();
        c.discard.clear();
        c.exhaust.clear();
        let misc0 = c.streams.floor(FloorStream::MiscRng).counter();
        let sh0 = c.streams.floor(FloorStream::ShuffleRng).counter();
        c.end_turn();
        assert_eq!(
            c.streams.floor(FloorStream::MiscRng).counter(),
            misc0,
            "钳子不动 miscRng"
        );
        assert_eq!(
            c.streams.floor(FloorStream::ShuffleRng).counter(),
            sh0 + 1,
            "钳子正好从 shuffleRng 取一个 long"
        );
        assert_eq!(
            c.hand.iter().filter(|k| k.upgraded).count(),
            1,
            "抽牌后升级了恰好一张"
        );

        // 手里一张能升的都没有 → 整段跳过,两条流都不动(反编译 949-951 的 return)
        let mut c = staged(&[], &[], &relics);
        c.hand.clear();
        c.draw = (0..10)
            .map(|_| {
                let mut k = cards::card("strike");
                k.upgrade();
                k
            })
            .collect();
        c.discard.clear();
        c.exhaust.clear();
        let misc0 = c.streams.floor(FloorStream::MiscRng).counter();
        let sh0 = c.streams.floor(FloorStream::ShuffleRng).counter();
        c.end_turn();
        assert_eq!(
            c.streams.floor(FloorStream::MiscRng).counter(),
            misc0,
            "没候选时 miscRng 不动"
        );
        assert_eq!(
            c.streams.floor(FloorStream::ShuffleRng).counter(),
            sh0,
            "没候选时不掷 shuffleRng(反编译先判空再掷)"
        );

        // 候选要剔掉不可升级的:手里 5 张已升级、抽牌堆 1 张未升级,
        // 升级必须落在那张未升级的上(旧写法对整只手均匀抽会白抽一次、什么都不升)
        let mut c = staged(&["strike"], &[], &relics);
        c.hand = (0..5)
            .map(|_| {
                let mut k = cards::card("strike");
                k.upgrade();
                k
            })
            .collect();
        c.draw = vec![cards::card("strike")];
        c.discard.clear();
        c.exhaust.clear();
        c.end_turn();
        // 回合末弃 5 张、抽 5 张(先从唯一的抽牌堆牌=未升级那张抽起),钳子升级未升级的那张,
        // 于是这一手 5 张全是升级态(旧写法对整只手均匀抽,抽到已升级的牌就白抽、留下未升级的)
        let up = c.hand.iter().filter(|k| k.upgraded).count();
        assert_eq!(up, c.hand.len(), "候选剔重:未升级那张被升了,整手都是升级态");
        assert_eq!(
            c.hand
                .iter()
                .filter(|k| k.def.id == "strike" && k.upgraded)
                .count(),
            c.hand.len(),
            "全是升级打击(候选剔重生效)"
        );
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

    // ---- 消耗钩子:蓝蜡烛/医疗包打出的牌算不算"消耗",决定后面一整套钩子 ----

    /// 医疗包把状态牌打出去:这张牌也算"被消耗了一张",无痛感/黑暗拥抱/卡戎之灰/枯枝
    /// 四条一起触发.反编译 onUseStatusOrCurseCard 给状态牌置 exhaustOnUse
    /// (refs/sts_lightspeed/src/combat/BattleContext.cpp:1920-1923),收尾走
    /// triggerAndMoveToExhaustPile(:2812-2841).
    #[test]
    fn medical_kit_status_play_fires_every_exhaust_hook() {
        let relics = vec![
            relic_def_or_panic("medical_kit"),
            relic_def_or_panic("charons_ashes"),
            relic_def_or_panic("dead_branch"),
        ];
        let mut c = staged(&["wound"; 8], &["wound"], &relics);
        c.player.statuses.add(Status::FeelNoPain, 3);
        c.player.statuses.add(Status::DarkEmbrace, 1);
        c.enemies[0].hp = 999;
        let e_hp = c.enemies[0].hp;
        let hand_before = c.hand.len();
        c.play_card(0, None).unwrap();
        assert_eq!(c.exhaust.len(), 1, "状态牌进消耗堆");
        assert_eq!(e_hp - c.enemies[0].hp, 3, "卡戎之灰:对全体 3 点");
        assert_eq!(c.player.block, 3, "无痛感:3 格挡");
        // 手牌 = 打掉 1 张 - 黑暗拥抱抽 1 + 枯枝加 1
        assert_eq!(c.hand.len(), hand_before - 1 + 1 + 1, "黑暗拥抱抽 1、枯枝加 1");
    }

    /// 蓝蜡烛把诅咒打出去:一样算消耗,四条钩子全触发,并且掉 1 血
    #[test]
    fn blue_candle_curse_play_fires_every_exhaust_hook() {
        let relics = vec![
            relic_def_or_panic("blue_candle"),
            relic_def_or_panic("charons_ashes"),
            relic_def_or_panic("dead_branch"),
        ];
        let mut c = staged(&["injury"; 8], &["injury"], &relics);
        c.player.statuses.add(Status::FeelNoPain, 2);
        c.player.statuses.add(Status::DarkEmbrace, 1);
        c.enemies[0].hp = 999;
        let e_hp = c.enemies[0].hp;
        let hp = c.player.hp;
        let hand_before = c.hand.len();
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.hp, hp - 1, "蓝蜡烛:打出诅咒掉 1 血");
        assert_eq!(c.exhaust.len(), 1, "诅咒进消耗堆");
        assert_eq!(e_hp - c.enemies[0].hp, 3, "卡戎之灰");
        assert_eq!(c.player.block, 2, "无痛感");
        assert_eq!(c.hand.len(), hand_before - 1 + 1 + 1);
    }

    /// 两条钩子各自只认自己那一类:没医疗包时状态牌打不出去,没蓝蜡烛时诅咒打不出去
    /// (反编译 CardInstance.cpp:320-331 的 canUse 分支)
    #[test]
    fn the_two_playability_relics_do_not_cover_each_other() {
        let kit = staged(&["injury", "wound"], &["injury"], &[relic_def_or_panic("medical_kit")]);
        assert!(kit.playable(0).is_err(), "医疗包不管诅咒");
        let candle =
            staged(&["injury", "wound"], &["wound"], &[relic_def_or_panic("blue_candle")]);
        assert!(candle.playable(0).is_err(), "蓝蜡烛不管状态牌");
        // 黏液(唯一本来就能打出的状态牌)没有医疗包也能打,打完照旧消耗
        let mut plain = staged(&["slimed"; 4], &["slimed"], &[]);
        assert!(plain.playable(0).is_ok(), "黏液本来就能打");
        plain.play_card(0, None).unwrap();
        assert_eq!(plain.exhaust.len(), 1, "黏液自带消耗");
    }

    /// 死灵诅咒:被消耗掉也躲不开,补一张新的回手牌;带蓝蜡烛时打一次掉 1 血、补一张
    /// (反编译 BattleContext.cpp:2836-2839 的 `c.getId() == NECRONOMICURSE` 分支)
    #[test]
    fn necronomicurse_escapes_the_exhaust_pile() {
        let relics = vec![relic_def_or_panic("blue_candle")];
        let mut c = staged(&["necronomicurse"; 4], &["necronomicurse"], &relics);
        let hp = c.player.hp;
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.hp, hp - 1, "蓝蜡烛掉 1 血");
        assert_eq!(c.exhaust.len(), 1, "原张还是进消耗堆");
        assert_eq!(c.hand.len(), 1, "补一张新的回手牌");
        assert_eq!(c.hand[0].def.id, "necronomicurse");
        // 补回来的那张不是"又消耗了一次",不该再触发消耗钩子
        let relics = vec![relic_def_or_panic("charons_ashes")];
        let mut c = staged(&["necronomicurse"; 4], &["necronomicurse"], &relics);
        c.enemies[0].hp = 999;
        // 没蓝蜡烛打不出去,直接走消耗
        c.exhaust_card(cards::card("necronomicurse"));
        assert_eq!(999 - c.enemies[0].hp, 3, "消耗这张只算一次");
        assert_eq!(c.hand.len(), 2, "手里那张还在,补回来一张");
        assert_eq!(c.hand[1].def.id, "necronomicurse", "新补的那张");
    }

    /// 一张牌消耗一次就只触发一次钩子(卡戎之灰 3 点,不是 6 点)
    #[test]
    fn exhaust_hooks_fire_once_per_exhausted_card() {
        let relics = vec![relic_def_or_panic("charons_ashes")];
        let mut c = staged(&["slimed"; 4], &["slimed", "slimed"], &relics);
        c.enemies[0].hp = 999;
        c.play_card(0, None).unwrap();
        assert_eq!(999 - c.enemies[0].hp, 3, "只掉 3 点");
        c.play_card(0, None).unwrap();
        assert_eq!(999 - c.enemies[0].hp, 6, "第二张再 3 点");
    }

    /// 枯枝排在黑暗拥抱前面:手牌 9 张时枯枝那张先占住第 10 格,黑暗拥抱抽不动
    /// (反编译先入队 MakeTempCardInHand 再入队 DrawCards,BattleContext.cpp:2822-2830)
    #[test]
    fn dead_branch_resolves_before_dark_embrace() {
        let relics = vec![relic_def_or_panic("dead_branch")];
        let mut c = staged(&["wound"; 30], &["wound"; 9], &relics);
        c.player.statuses.add(Status::DarkEmbrace, 1);
        c.exhaust_card(cards::card("wound"));
        assert_eq!(c.hand.len(), 10, "9 + 枯枝那张");
        assert_eq!(c.draw.len(), 30, "顺序反了的话这里会少一张(黑暗拥抱先抽走)");
        assert!(c.discard.is_empty(), "顺序反了的话枯枝那张会被挤进弃牌堆");
    }

    /// 枯枝在手牌已满时:照样掷一次 cardRandomRng、照样造牌,牌落到弃牌堆,
    /// 不是整张消失(反编译 moveToHandHelper 的手牌溢出规则,
    /// refs/sts_lightspeed/src/combat/BattleContext.cpp:2531-2540)
    #[test]
    fn dead_branch_overflows_to_the_discard_pile() {
        let relics = vec![relic_def_or_panic("dead_branch")];
        let mut c = staged(&["wound"; 12], &["wound"; 10], &relics);
        assert_eq!(c.hand.len(), 10);
        assert!(c.discard.is_empty());
        // 掷点照旧消耗:拿同一条流上的下一次取牌当期望
        let mut probe = c.streams.clone();
        let expected = probe
            .floor(FloorStream::CardRandomRng)
            .pick(&crate::core::cards::class_card_pool())
            .id;
        c.exhaust_card(cards::card("wound"));
        assert_eq!(c.hand.len(), 10, "满手还是满手");
        assert_eq!(c.discard.len(), 1, "枯枝那张进弃牌堆");
        assert_eq!(
            c.discard[0].def.id, expected,
            "掷点没被吞掉,牌就是掷到的那张"
        );
    }

    // ---- 洗牌钩子:算盘 +6 格挡、日晷每 3 次洗牌 +2 能量 ----

    /// 三种洗牌都要认:抽牌把抽牌堆抽空时的洗、深呼吸、打抽牌堆顶时的洗.
    /// 反编译的 onShuffle 调用点是 drawCards(BattleContext.cpp:2452)、
    /// DEEP_BREATH(:1274)、PlayTopCard 的 EmptyDeckShuffle(:2518-2520);
    /// 参考实现的洗牌动作统一走 reshuffleDiscardIntoDraw
    /// (refs/slay-the-cli/src/engine/combat/piles.ts:63-69).
    #[test]
    fn the_abacus_and_sundial_fire_on_every_shuffle_trigger() {
        let relics = vec![
            relic_def_or_panic("the_abacus"),
            relic_def_or_panic("sundial"),
        ];

        // 1) 抽牌抽空抽牌堆
        let mut c = staged(&["strike"], &[], &relics);
        c.draw.clear();
        c.discard.push(cards::card("defend"));
        c.draw_cards(1);
        assert_eq!(c.player.block, 6, "算盘:每次洗牌 +6 格挡");
        assert_eq!(c.rs.sundial, 1, "日晷:计数 1(还没到 3)");

        // 2) 深呼吸(会把弃牌堆洗回抽牌堆)
        let mut c = staged(&["deep_breath"], &["deep_breath"], &relics);
        c.discard.push(cards::card("defend"));
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 6, "深呼吸这次洗牌也要给格挡");
        assert_eq!(c.rs.sundial, 1);

        // 3) 浩劫打抽牌堆顶:抽牌堆空了先洗回来(顶上放一张不打格挡的牌,
        //    免得它自己的效果混进格挡数)
        let mut c = staged(&["havoc"], &["havoc"], &relics);
        c.discard.push(cards::card("strike"));
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 6, "打抽牌堆顶的洗也要给格挡");
        assert_eq!(c.rs.sundial, 1);
    }

    /// 日晷正好在第 3 次洗牌给 2 点能量(反编译 BattleContext.cpp:2804-2810 的
    /// `sundialCounter == 2` 判定:计数 0/1 累加,第 3 次归零并给能量)
    #[test]
    fn sundial_gives_two_energy_on_the_third_shuffle() {
        let relics = vec![relic_def_or_panic("sundial")];
        let mut c = staged(&["defend"], &[], &relics);
        c.energy = 0;
        for i in 1..=3 {
            c.hand.clear();
            c.draw.clear();
            c.discard.push(cards::card("defend"));
            c.draw_cards(1);
            assert_eq!(c.rs.sundial, i % 3, "第 {i} 次洗牌后的计数");
        }
        assert_eq!(c.energy, 2, "第 3 次洗牌 +2 能量");
        // 继续洗到第 6 次再来一份
        for _ in 1..=3 {
            c.hand.clear();
            c.draw.clear();
            c.discard.push(cards::card("defend"));
            c.draw_cards(1);
        }
        assert_eq!(c.energy, 4, "每 3 次洗牌 +2");
    }

    /// 没有牌可洗的时候不洗也不该触发钩子(反编译 drawCards 开头的
    /// `drawPile.size() + discardPile.size() == 0` 直接返回,BattleContext.cpp:2439-2444);
    /// 回合末把手牌弃掉也不算洗牌(onShuffle 的调用点只有 drawCards/DEEP_BREATH/
    /// PlayTopCard 三处,弃牌那条路一个都没有)
    #[test]
    fn an_empty_shuffle_fires_nothing() {
        let relics = vec![
            relic_def_or_panic("the_abacus"),
            relic_def_or_panic("sundial"),
        ];
        let mut c = staged(&[], &[], &relics);
        assert!(c.draw.is_empty() && c.discard.is_empty());
        c.draw_cards(1);
        assert_eq!(c.player.block, 0, "没洗牌就没有格挡");
        assert_eq!(c.rs.sundial, 0, "日晷计数不动");

        // 回合末弃牌:抽牌堆里还有牌,不该有任何洗牌
        let mut c = staged(&["defend"; 8], &["defend", "defend"], &relics);
        c.end_turn();
        assert_eq!(c.rs.sundial, 0, "回合末弃牌不算洗牌");
    }

    // ---- 能量/费用闸门 ----

    /// 维可夹克:一回合最多 6 张,第 7 张打不出去,换回合重新计数
    /// (反编译 isCardPlayAllowed 的 `cardsPlayedThisTurn >= 6`,BattleContext.cpp:708-716)
    #[test]
    fn velvet_choker_stops_the_seventh_card() {
        let relics = vec![relic_def_or_panic("velvet_choker")];
        let mut c = staged(&["strike"; 12], &["strike"; 8], &relics);
        c.enemies[0].hp = 999;
        for i in 1..=6 {
            assert!(c.playable(0).is_ok(), "第 {i} 张还能打");
            c.play_card(0, Some(0)).unwrap();
        }
        assert_eq!(c.cards_played, 6);
        assert!(c.playable(0).is_err(), "第 7 张被拦下");
        assert!(c.play_card(0, Some(0)).is_err());
        // 打到 6 张也不影响别的:技能照样打不出去,但能量还在
        assert_eq!(c.energy, 3, "6 张 0 费打击没花能量");
        // 下一回合重新计数
        c.hand.push(cards::card("strike"));
        c.end_turn();
        assert!(
            c.playable(0).is_ok(),
            "新回合的计数归零,又能打了(回合={})",
            c.turn
        );
    }

    /// 化学 X:X 费牌的效果多算 2 点,能量照常花光;0 能量也能打出 X=2 的效果
    /// (反编译 Actions.cpp:591-594 / :1253-1256 的 `energy + (hasRelic<CHEMICAL_X>() ? 2 : 0)`)
    #[test]
    fn chemical_x_counts_two_more_with_and_without_energy() {
        let relics = vec![relic_def_or_panic("chemical_x")];
        // 3 点能量 -> X = 3 + 2 = 5 次
        let mut c = staged(&["whirlwind"; 5], &["whirlwind"], &relics);
        c.enemies[0].hp = 999;
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(999 - c.enemies[0].hp, 25);
        assert_eq!(c.energy, 0, "能量按 X 全花掉");

        // 0 点能量 -> X = 0 + 2 = 2 次(不是打不出去)
        let mut c = staged(&["whirlwind"; 5], &["whirlwind"], &relics);
        c.enemies[0].hp = 999;
        c.energy = 0;
        c.play_card(0, None).unwrap();
        assert_eq!(999 - c.enemies[0].hp, 10, "0 能量也吃化学 X 的 +2");

        // 不带化学 X 时 X 就是花掉的能量
        let mut c = staged(&["whirlwind"; 5], &["whirlwind"], &[]);
        c.enemies[0].hp = 999;
        c.energy = 3;
        c.play_card(0, None).unwrap();
        assert_eq!(999 - c.enemies[0].hp, 15);
    }

    /// 冰激凌:没用完的能量留到下一回合(反编译 Player::rechargeEnergy 的
    /// `gainEnergy(energyPerTurn)`,Player.cpp:714-719)
    #[test]
    fn ice_cream_carries_unspent_energy() {
        let relics = vec![relic_def_or_panic("ice_cream")];
        let mut c = staged(&["strike"; 6], &["strike"], &relics);
        c.max_energy = 3;
        c.energy = 3;
        c.play_card(0, Some(0)).unwrap(); // 花 1 点
        assert_eq!(c.energy, 2);
        c.end_turn();
        // 新回合 = 每回合 3 点 + 上回合剩下的 2 点
        assert_eq!(c.energy, 5, "3 + 存下来的 2");
        // 没用冰激凌的话归零重来
        let mut c = staged(&["strike"; 6], &["strike"], &[]);
        c.max_energy = 3;
        c.energy = 3;
        c.play_card(0, Some(0)).unwrap();
        c.end_turn();
        assert_eq!(c.energy, 3, "没有冰激凌就是每回合固定 3 点");
    }

    /// 战争艺术:上一回合没打攻击才补 1 点,且第一回合不补(反编译
    /// Player::applyStartOfTurnRelics 的 `attacksPlayedThisTurn == 0`,Player.cpp:491-495;
    /// 参考实现另外要求 turn > 1,refs/slay-the-cli/src/content/relics/common.ts:30-36)
    #[test]
    fn art_of_war_needs_an_attack_free_previous_turn() {
        // 第一回合:回合开始的时刻 turn == 1,走的是"第一回合"那条分支,战争艺术不补
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: vec![cards::card("defend"); 10],
            relics: vec![relic_def_or_panic("art_of_war")],
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").unwrap();
        let mut c = Combat::new(enc, setup, RngRegistry::new(3));
        assert_eq!(c.turn, 1);
        assert_eq!(c.energy, c.max_energy, "第一回合没有额外能量");
        assert_eq!(c.max_energy, 3);

        // 第一回合不打攻击 -> 第二回合补 1 点
        c.energy = 3;
        c.end_turn();
        assert_eq!(c.turn, 2);
        assert_eq!(c.energy, 4, "上一回合没打攻击,+1");

        // 第二回合打了攻击 -> 第三回合不补
        c.hand.push(cards::card("strike"));
        let strike_idx = c.hand.len() - 1;
        c.energy = 3;
        c.play_card(strike_idx, Some(0)).unwrap();
        c.end_turn();
        assert_eq!(c.turn, 3);
        assert_eq!(c.energy, 3, "上一回合打了攻击,不补");

        // 第三回合又没打 -> 第四回合再补
        c.end_turn();
        assert_eq!(c.turn, 4);
        assert_eq!(c.energy, 4, "上一回合没攻击,+1");
    }

    /// 嗜血:每掉一次血降 1 费,最低钳到 0;而且要恰好钳在 0(反编译
    /// CardInstance::updateCost 的 `std::max(0, cost + amount)`,CardInstance.cpp:108-117)
    #[test]
    fn blood_for_blood_cost_clamps_at_zero() {
        let mut c = staged(&["blood_for_blood"; 2], &["blood_for_blood"], &[]);
        assert_eq!(c.hand[0].fixed_cost(), Some(4), "基础 4 费");
        for _ in 0..4 {
            c.hit_player(1);
        }
        assert_eq!(c.hand[0].fixed_cost(), Some(0), "掉 4 次血到 0 费");
        for _ in 0..3 {
            c.hit_player(1);
        }
        assert_eq!(c.hand[0].fixed_cost(), Some(0), "再掉也不会变成负费");
    }

    /// 消耗堆里的嗜血不再跟着掉血降价;被挖掘回手牌后还是当初那张牌的价
    /// (反编译 CardManager::onTookDamage 只遍历 hand/drawPile/discardPile,
    /// refs/sts_lightspeed/src/combat/CardManager.cpp:448-490)
    #[test]
    fn blood_for_blood_in_the_exhaust_pile_keeps_its_cost() {
        let mut c = staged(&["blood_for_blood"; 2], &[], &[]);
        c.draw = vec![cards::card("blood_for_blood")];
        let buried = c.draw.pop().unwrap();
        c.exhaust.push(buried);
        c.hit_player(1);
        c.hit_player(1);
        assert_eq!(c.exhaust[0].fixed_cost(), Some(4), "消耗堆里的保持 4 费");
        // 手牌/弃牌堆里的照降
        c.hand.push(cards::card("blood_for_blood"));
        c.hit_player(1);
        assert_eq!(c.hand[0].fixed_cost(), Some(3), "手牌里的降到 3 费");
        assert_eq!(c.exhaust[0].fixed_cost(), Some(4), "消耗堆里的还是 4 费");
    }

    /// 奇异勺的掷点口径:反编译只掷一次 cardRandomRng.randomBoolean()
    /// (refs/sts_lightspeed/src/combat/BattleContext.cpp:1985-1991),不是 random(99).
    /// 这里用同一条流上"下一次 randomBoolean"的取值来钉住口径.
    #[test]
    fn strange_spoon_rolls_one_random_boolean() {
        let spoon = relic_def_or_panic("strange_spoon");
        let mut c = staged(&["slimed"; 2], &["slimed"], &[spoon]);
        let mut probe = c.streams.clone();
        let coin = probe.floor(FloorStream::CardRandomRng).random_boolean();
        c.play_card(0, None).unwrap();
        if coin {
            assert_eq!(c.discard.len(), 1, "掷到 true -> 改弃牌堆");
            assert!(c.exhaust.is_empty());
        } else {
            assert_eq!(c.exhaust.len(), 1, "掷到 false -> 照常消耗");
            assert!(c.discard.is_empty());
        }
    }
}


/// 覆盖率补丁:一批"代码在、但没有任何断言约束"的分支.每条都注明反编译出处;
/// 其中若发现与出处不符的,在对应行注明.这里的断言只钉"真实分支",不写空泛判断.
#[cfg(test)]
mod branch_assertions {
    use super::*;
    use crate::core::cards::card;
    use crate::core::relics::relic_def_or_panic;

    fn enc(id: &'static str) -> &'static crate::core::enemy::Encounter {
        crate::core::enemies::encounter_def(id).unwrap_or_else(|| panic!("no encounter {id}"))
    }

    /// 一只怪(颚虫)、指定牌组与遗物,清空四个牌堆、能量给 9
    fn board(ids: &[&'static str], relics: &[&'static RelicDef], asc: u32) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: ids.iter().map(|id| card(id)).collect(),
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc,
        };
        let mut c = Combat::new(enc("jaw_worm_solo"), setup, RngRegistry::new(1));
        c.hand.clear();
        c.draw.clear();
        c.discard.clear();
        c.exhaust.clear();
        c.energy = 9;
        c
    }

    /// 红骷髅:血量从半血以上掉到半血以下补 3 力量,再治疗回到半血以上收回那 3 点.
    /// 反编译 Player::heal(`wasBloodied && curHp > maxHp/2` 才 debuff<STRENGTH>(3),
    /// refs/sts_lightspeed/src/combat/Player.cpp:169-171);"掉血补力量"那一半由
    /// tools/sandbox_relics.ts 的 red_skull 行覆盖,"治回来收回"这一半此前没有任何断言.
    #[test]
    fn red_skull_takes_back_the_strength_when_healing_above_half() {
        let relics = [relic_def_or_panic("red_skull")];
        let mut c = board(&["defend"], &relics, 0);
        assert_eq!(c.player.statuses.get(Status::Strength), 0, "满血时不加");
        c.lose_hp_player(50, false); // 80 -> 30,跨过半血
        assert_eq!(c.player.hp, 30);
        assert_eq!(c.player.statuses.get(Status::Strength), 3, "掉到半血以下补 3");
        c.heal_player(30); // 30 -> 60
        assert_eq!(c.player.hp, 60);
        assert_eq!(c.player.statuses.get(Status::Strength), 0, "回到半血以上收回 3");
    }

    /// 圆球哨卫的停滞:持有者被打死时把偷走的牌还给玩家;手牌没满进手牌,满了进弃牌堆.
    /// 反编译 Monster::died 的 returnStasisCard(refs/sts_lightspeed/src/combat/Monster.cpp:308-310)
    /// 与 moveToHandHelper(refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:3502-3513);
    /// 此前只有"偷"有断言(stasis_steals_the_highest_rarity_card),"还"整条路径没有.
    #[test]
    fn killing_a_stasis_holder_returns_the_card() {
        let mut c = board(&["strike"; 5], &[], 0);
        c.draw = vec![card("bludgeon"), card("strike")];
        c.enemy_steal_card(0, "Bronze Orb");
        assert_eq!(c.stasis.len(), 1, "先偷走一张");
        c.enemies[0].hp = 0;
        c.settle_deaths();
        assert!(c.stasis.is_empty(), "持有者死了要还回来");
        assert!(c.hand.iter().any(|x| x.def.id == "bludgeon"), "手牌没满就进手牌");
        assert!(!c.discard.iter().any(|x| x.def.id == "bludgeon"));

        // 手牌满 10 张:只能进弃牌堆
        let mut c = board(&["strike"; 5], &[], 0);
        c.hand = (0..HAND_LIMIT).map(|_| card("defend")).collect();
        c.draw = vec![card("bludgeon")];
        c.enemy_steal_card(0, "Bronze Orb");
        c.enemies[0].hp = 0;
        c.settle_deaths();
        assert!(
            c.discard.iter().any(|x| x.def.id == "bludgeon"),
            "手牌满时进弃牌堆"
        );
    }

    /// 浩劫从抽牌堆顶打出的若是一张能力牌:能力牌一样"打完就退场",不进任何牌堆.
    /// 反编译 PlayTopCard 把牌打出去(refs/sts_lightspeed/src/combat/Actions.cpp:213-217),
    /// 能力牌打出后进 powers(等价本作的 vanish);此前浩劫的测试顶牌全是攻击/技能牌.
    #[test]
    fn havoc_plays_a_power_off_the_top_and_it_leaves_play() {
        let mut c = board(&["havoc", "demon_form", "strike", "strike"], &[], 0);
        c.hand = vec![card("havoc")];
        c.draw = vec![card("demon_form"), card("strike")];
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.statuses.get(Status::DemonForm), 2, "能力牌结算(恶魔形态每回合 +2 力量)");
        assert!(
            !c.discard.iter().any(|x| x.def.id == "demon_form"),
            "能力牌不进弃牌堆"
        );
        assert!(
            !c.exhaust.iter().any(|x| x.def.id == "demon_form"),
            "能力牌不进消耗堆"
        );
        assert!(
            c.havoc_chain.iter().any(|(_, l)| l.contains("Demon Form")),
            "记进了浩劫链"
        );
    }

    /// 炼狱之焰在手牌只剩自己时不打任何一段(段数 = 手牌数 - 自己).
    /// 反编译 Actions::FiendFireAction 按 cardsInHand 段数结算
    /// (refs/sts_lightspeed/src/combat/Actions.cpp:1092-1100);空手那条 false 分支此前没有断言.
    #[test]
    fn fiend_fire_with_only_itself_in_hand_hits_nothing() {
        let mut c = board(&["fiend_fire"], &[], 0);
        c.hand = vec![card("fiend_fire")];
        c.enemies[0].hp = 999;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 999, "没有别的牌可消耗,一段也不打");
    }

    /// 御守也能顶掉战斗中怪物塞进牌组的诅咒(反编译 Deck::obtain 的 CURSE 分支,
    /// refs/sts_lightspeed/src/game/Deck.cpp:157-166).战斗内这条路此前没有任何断言
    /// (所有测试 setup 的 curse_negate 都是 0).
    #[test]
    fn omamori_negates_a_curse_pushed_into_the_deck_in_combat() {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: vec![card("strike"); 5],
            relics: Vec::new(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 2,
            asc: 0,
        };
        let mut c = Combat::new(enc("jaw_worm_solo"), setup, RngRegistry::new(1));
        let implant = EnemyFx::PlayerCard {
            card: "parasite",
            spot: CardSpot::Deck,
            n: 1,
        };
        c.apply_enemy_fx(0, implant, 1, "Writhing Mass", "Implant");
        assert_eq!(c.curse_negate, 1, "顶掉一层");
        assert!(
            !c.deck_cards.iter().any(|x| x.def.id == "parasite"),
            "顶掉的诅咒不进牌组"
        );
        c.apply_enemy_fx(0, implant, 1, "Writhing Mass", "Implant");
        c.apply_enemy_fx(0, implant, 1, "Writhing Mass", "Implant");
        assert_eq!(c.curse_negate, 0, "两次用光");
        assert_eq!(
            c.deck_cards
                .iter()
                .filter(|x| x.def.id == "parasite")
                .count(),
            1,
            "用光之后那次进牌组"
        );
    }

    /// 死灵之书每回合只翻倍第一张 >=2 费攻击,回合结束重置.
    /// 反编译 BattleContext.cpp:1691-1694 的 haveUsedNecronomiconThisTurn;
    /// "同回合第二张不再翻倍"与"下回合重置"两个分支此前都没有断言.
    #[test]
    fn necronomicon_doubles_only_the_first_big_attack_each_turn() {
        let relics = [relic_def_or_panic("necronomicon")];
        let mut c = board(&["bludgeon"; 4], &relics, 0);
        c.hand = vec![card("bludgeon"), card("bludgeon")];
        c.enemies[0].hp = 999;
        let hp0 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(hp0 - c.enemies[0].hp, 64, "第一张 3 费攻击翻倍(32x2)");
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(hp0 - c.enemies[0].hp, 96, "同回合第二张不再翻倍");
        c.end_turn();
        assert!(!c.rs.necro_used, "回合结束重置,下回合还能再翻一次");
    }

    /// 腕刃看"本回合实际费用为 0":被降费到 0 的攻击也算(反编译判 costForTurn == 0,
    /// 参考实现 damageCalc 的 wristBlade 判据);此前只测过印刷 0 费的迅捷打击.
    #[test]
    fn wrist_blade_boosts_an_attack_whose_cost_was_reduced_to_zero() {
        let relics = [relic_def_or_panic("wrist_blade")];
        let mut c = board(&["bludgeon"], &relics, 0);
        let mut b = card("bludgeon");
        b.cost_delta = -3; // 本回合实际费用被降到 0(疯狂/腐化那一类)
        c.hand = vec![b];
        c.enemies[0].hp = 999;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 999 - 36, "0 费攻击 32+4");

        // 对照:没有腕刃,同样降费也只有 32
        let mut c2 = board(&["bludgeon"], &[], 0);
        let mut b2 = card("bludgeon");
        b2.cost_delta = -3;
        c2.hand = vec![b2];
        c2.enemies[0].hp = 999;
        c2.play_card(0, Some(0)).unwrap();
        assert_eq!(c2.enemies[0].hp, 999 - 32, "没有腕刃就是原伤害");
    }

    /// 化石螺壳:本场第一次受到的攻击伤害被直接免掉(Buffer).
    /// 反编译 BattleContext.cpp:271-273 给 BUFFER(1),Buffer 在 Player::damage 的
    /// 格挡之后、钨钢棒之前消耗(refs/sts_lightspeed/src/combat/Player.cpp:193-196);
    /// 此前 sandbox_relics 只测了反向(放血的自伤不该被免),正面这一半没有任何断言.
    #[test]
    fn fossilized_helix_absorbs_the_first_hit() {
        let relics = [relic_def_or_panic("fossilized_helix")];
        let mut c = board(&["defend"], &relics, 0);
        let hp = c.player.hp;
        c.phase = Phase::EnemyTurn;
        c.enemies[0].next_move = 0; // 颚虫的 Chomp
        c.enemy_act(0);
        assert_eq!(c.player.hp, hp, "第一次攻击伤害被免掉");
        c.enemies[0].next_move = 0;
        c.enemy_act(0);
        assert!(c.player.hp < hp, "第二次照常挨打");
    }

    /// 军备(Armaments):手里一张能升级的牌都没有时不挂选牌窗口(效果里那条 false 分支)
    /// (refs/sts_lightspeed 的 Armaments 升级选牌与 cards.rs:820 的 UpgradeChosenInHand;
    /// 此前 sandbox_diff 的 armaments 场景手牌里总有可升级牌,只走 true 分支).
    #[test]
    fn armaments_with_no_upgradable_card_opens_no_choice() {
        let mut c = board(&["armaments", "strike"], &[], 0);
        let mut upgraded = card("strike");
        upgraded.upgraded = true;
        c.hand = vec![card("armaments"), upgraded];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "没有能升级的牌就不挂选牌窗口");
        assert_eq!(c.player.block, 5, "格挡照给");
    }

    /// 军备+(Armaments+):升级手里所有"能升级的牌",不开选牌窗口
    /// (refs/sts_lightspeed/src/combat/Actions.cpp:901 UpgradeAllCardsInHand;
    /// cards.rs 的 UpgradeAllInHand.此前只测过基础版选牌/0 候选两条路).
    #[test]
    fn armaments_plus_upgrades_the_whole_hand() {
        let mut c = board(&["armaments", "strike", "defend", "wound"], &[], 0);
        let mut up = card("armaments");
        up.upgraded = true;
        let mut already = card("defend");
        already.upgraded = true;
        c.hand = vec![up, card("strike"), already, card("wound")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "升级版不开选牌窗口");
        assert_eq!(c.player.block, 5, "格挡仍是 5(升级不改格挡)");
        let strike = c.hand.iter().find(|x| x.def.id == "strike").unwrap();
        assert!(strike.upgraded, "打击被升级");
        assert_eq!(strike.effects(), &[Effect::Damage { amount: 9, times: 1 }]);
        assert!(
            c.hand.iter().any(|x| x.def.id == "wound" && !x.upgraded),
            "伤口不可升级,保持原样"
        );
    }

    /// 军备(以及一切"选一张升级"的屏)都不该把诅咒/状态牌列进候选:反编译
    /// CardInstance::canUpgrade()(refs/sts_lightspeed/src/combat/CardInstance.cpp:55-59)
    /// 与 Card::canUpgrade()(同仓 src/game/Card.cpp:67-80)对 CURSE/STATUS 直接 false.
    /// 灼伤虽然在语料里带一条升级数据(magic 2->4),但只有六火幽魂的炼狱走
    /// UpgradePlayerBurns 那条专路能升它.原先 can_upgrade 只认"有没有升级数据",
    /// 于是军备会把灼伤升成 4 点,act3 seed 197 那一场由此整段错开.
    #[test]
    fn armaments_never_offers_a_curse_or_status_to_upgrade() {
        // 灼伤带升级数据,但类型是状态 -> 不进候选;炼狱的专路照样能升它
        let mut burn = card("burn");
        assert!(burn.def.upgradable(), "灼伤确实带一条升级数据");
        assert!(!burn.can_upgrade(), "状态牌不列进升级候选");
        assert!(burn.upgrade_forced(), "炼狱那条专路照样能升它");
        assert!(burn.upgraded);

        // 手里只剩灼伤"能升"时,军备不开选牌窗口(等同没有可升级的牌)
        let mut c = board(&["armaments", "strike"], &[], 0);
        let mut up_strike = card("strike");
        up_strike.upgraded = true;
        c.hand = vec![card("armaments"), card("burn"), up_strike];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "只剩灼伤可升时不开屏");
        assert!(
            !c.hand.iter().find(|x| x.def.id == "burn").unwrap().upgraded,
            "灼伤没被升"
        );

        // 有真能升级的牌时,候选里只该有它(单候选时本作会直接结算,两条路都认)
        let mut c = board(&["armaments", "strike"], &[], 0);
        c.hand = vec![card("armaments"), card("burn"), card("strike")];
        c.play_card(0, None).unwrap();
        if let Some(ch) = c.choice.as_ref() {
            let cands = c.candidates_of(ch);
            assert_eq!(cands.len(), 1, "灼伤不算候选");
            assert_eq!(cands[0].1.def.id, "strike");
        } else {
            assert!(
                c.hand.iter().find(|x| x.def.id == "strike").unwrap().upgraded,
                "单候选直接升打击"
            );
        }
        assert!(
            !c.hand.iter().find(|x| x.def.id == "burn").unwrap().upgraded,
            "灼伤始终没被升"
        );
    }

    /// 坚毅:手里没牌可随机消耗时,格挡照给、消耗堆不动
    /// (refs/sts_lightspeed 的 True Grit 语义;此前只测过手里有牌的那条路).
    #[test]
    fn true_grit_with_an_empty_hand_blocks_and_exhausts_nothing() {
        let mut c = board(&["true_grit"], &[], 0);
        c.hand = vec![card("true_grit")];
        c.play_card(0, None).unwrap();
        assert_eq!(c.player.block, 7, "格挡照给");
        assert!(c.exhaust.is_empty(), "手里没牌可消耗");
    }

    /// 巨首的"时候到了"在 A18 提前一回合.反编译没有这条分支
    /// (refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:3157-3179 恒 turnNumber >= 4);
    /// 本作按 wiki 与参考实现头注 "CONFLICT HONORED (asc18)"(giantHead.ts)采信
    /// "A18 起只数 3 回合",这条此前没有任何断言.
    #[test]
    fn giant_head_uses_it_is_time_one_turn_earlier_at_a18() {
        let seq = |asc: u32| -> Vec<&'static str> {
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
            let mut c = Combat::new(enc("giant_head_solo"), setup, RngRegistry::new(3));
            let mut v = Vec::new();
            for _ in 0..6 {
                v.push(c.enemies[0].def.moves[c.enemies[0].next_move].name);
                c.end_turn();
            }
            v
        };
        let first = |v: &[&'static str]| {
            v.iter()
                .position(|m| *m == "It Is Time")
                .expect("六回合内一定会打出 It Is Time")
        };
        let a0 = seq(0);
        let a18 = seq(18);
        assert_eq!(first(&a0), 4, "A0 已经行动 4 回合才摆 It Is Time");
        assert_eq!(first(&a18), 3, "A18 提前一回合");
    }

    /// 指定遭遇、指定遗物的一场满状态战斗(牌组给 5 张打击,牌堆照抽)
    fn combat_vs(encounter: &'static str, relics: &[&'static RelicDef]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: (0..5).map(|_| card("strike")).collect(),
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        Combat::new(enc(encounter), setup, RngRegistry::new(7))
    }

    /// 勇气投石索:只在精英战给 2 力量,首领战不给.
    /// 反编译 refs/sts_lightspeed/src/combat/BattleContext.cpp:340-343
    /// `case R::SLING_OF_COURAGE: if (room == Room::ELITE) p.buff<PS::STRENGTH>(2);`
    /// 此前它与奴隶主颈圈共用 `elite_or_boss` 判据,sandbox_relics 的 sling 场景只有
    /// 精英与普通两种,首领这一半从未被检验 —— 首领战多给 2 力量的 bug.
    #[test]
    fn sling_of_courage_gives_strength_only_in_elite_combats() {
        let relics = [relic_def_or_panic("sling_of_courage")];
        let elite = combat_vs("gremlin_nob_solo", &relics);
        assert_eq!(elite.player.statuses.get(Status::Strength), 2, "精英战 +2");
        let boss = combat_vs("the_guardian", &relics);
        assert_eq!(boss.player.statuses.get(Status::Strength), 0, "首领战不给力量");
        let normal = combat_vs("cultist_solo", &relics);
        assert_eq!(normal.player.statuses.get(Status::Strength), 0, "普通战不给");
    }

    /// 奴隶主颈圈:精英与首领战斗都每回合 +1 能量,普通战不加.
    /// 反编译 BattleContext.cpp:334-338 `if (room == Room::ELITE || room == Room::BOSS)`.
    /// 沙盒只测了精英与普通,首领这一半没断言.
    #[test]
    fn slavers_collar_gives_energy_in_elite_and_boss_combats() {
        let relics = [relic_def_or_panic("slavers_collar")];
        assert_eq!(combat_vs("gremlin_nob_solo", &relics).max_energy, 4, "精英战 +1");
        assert_eq!(combat_vs("the_guardian", &relics).max_energy, 4, "首领战 +1");
        assert_eq!(combat_vs("cultist_solo", &relics).max_energy, 3, "普通战不加");
    }

    /// 风筝:同一回合只有第一次主动弃牌给 1 能量,回合开始才复位.
    /// 反编译 Hovering Kite 的 `firstDiscardThisTurn`;沙盒 scene 只弃了 1 张,
    /// "同回合第二次弃牌不再给"这一半此前没有断言.
    #[test]
    fn hovering_kite_grants_energy_only_on_the_first_discard_each_turn() {
        let relics = [relic_def_or_panic("hovering_kite")];
        let mut c = combat_vs("cultist_solo", &relics);
        let e = c.energy;
        c.on_manual_discard();
        assert_eq!(c.energy, e + 1, "本回合第一次弃牌 +1");
        c.on_manual_discard();
        assert_eq!(c.energy, e + 1, "本回合第二次弃牌不再给");
        c.start_turn(0);
        let e2 = c.energy;
        c.on_manual_discard();
        assert_eq!(c.energy, e2 + 1, "下回合第一次弃牌又能给");
    }

    /// 振奋(Akabeko)给的活力只加到本场第一张攻击上,砍完立刻用掉:
    /// 参考实现把 VIGOR 挂在 onAfterCardPlayed(refs/sts_lightspeed 的 VIGOR 结算),
    /// 第二张攻击不再吃那 8 点.沙盒 akabeko 场景只打一张打击,清空那一半此前没有断言.
    #[test]
    fn akabeko_vigor_applies_only_to_the_first_attack() {
        let relics = [relic_def_or_panic("akabeko")];
        let mut c = board(&["strike", "strike"], &relics, 0);
        c.hand = vec![card("strike"), card("strike")];
        c.enemies[0].hp = 500;
        let hp0 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        let first = hp0 - c.enemies[0].hp;
        c.energy = 9;
        let hp1 = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        let second = hp1 - c.enemies[0].hp;
        assert_eq!(first, 6 + 8, "第一张打击吃到活力 8");
        assert_eq!(second, 6, "第二张打击不再吃活力");
    }
    // ---- 选牌的强制/可选维度(ChoiceMode) ----
    // 原版口径见 ChoiceMode 的注释与反编译 Actions.cpp 各自的动作定义:
    // 强制单选候选 1 张 -> 自动结算不开屏;可选(可少选/不选)-> 1 张也开屏.

    /// 候选 0 张:强制/可选的选牌动作都不开窗口(参考实现的 action 在堆空时直接 return)
    #[test]
    fn zero_candidates_open_no_window_either_mode() {
        // 强制:头槌弃牌堆空 -> 只打伤害
        let mut c = board(&["headbutt"], &[], 0);
        c.hand = vec![card("headbutt")];
        let hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, hp - 9);
        assert!(c.choice.is_none(), "头槌弃牌堆空不开屏");

        // 可选:净化手牌空 -> 消耗堆不动、不开屏
        let mut c = board(&["purity"], &[], 0);
        c.hand = vec![card("purity")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "净化手牌空不开屏");
        assert!(c.exhaust.iter().all(|x| x.def.id == "purity"), "没有别的牌被耗");

        // 可选:预谋升级版(任意张)手牌空 -> 不开屏
        let mut c = board(&["forethought"], &[], 0);
        let mut ft = card("forethought");
        ft.upgraded = true;
        c.hand = vec![ft];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "预谋+ 手牌空不开屏");
    }

    /// 1 张候选且强制:当场自动结算,不开窗口
    #[test]
    fn mandatory_single_candidate_auto_resolves() {
        // 头槌:弃牌堆只剩 1 张 -> 自动放到抽牌堆顶
        let mut c = board(&["headbutt"], &[], 0);
        c.hand = vec![card("headbutt")];
        c.discard = vec![card("defend")];
        let hp = c.enemies[0].hp;
        c.play_card(0, Some(0)).unwrap();
        assert!(c.choice.is_none(), "强制单选 1 张:自动结算");
        assert_eq!(c.enemies[0].hp, hp - 9, "伤害照打");
        assert_eq!(c.draw.first().unwrap().def.id, "defend", "那张自动上堆顶");

        // 掘出:消耗堆只有 1 张别的牌 -> 自动回手(掘出自己不算候选)
        let mut c = board(&["exhume"], &[], 0);
        c.hand = vec![card("exhume")];
        c.exhaust = vec![card("bash")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "掘出 1 张候选:自动结算");
        assert!(c.hand.iter().any(|x| x.def.id == "bash"), "那张直接回手");

        // 战吼:打完自己后手里只剩 1 张 -> 自动放顶
        let mut c = board(&["warcry", "strike"], &[], 0);
        c.hand = vec![card("warcry"), card("strike")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "战吼 1 张候选:自动结算");
        assert_eq!(c.draw.first().unwrap().def.id, "strike", "自动放顶");

        // 军备:手里只有 1 张可升级 -> 自动升级
        let mut c = board(&["armaments", "strike"], &[], 0);
        c.hand = vec![card("armaments"), card("strike")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "军备 1 张可升级:自动结算");
        assert!(c.hand.iter().all(|x| x.upgraded), "那张自动升级");

        // 坚毅+:手里只有 1 张 -> 自动消耗(格挡照给)
        let mut c = board(&["true_grit", "strike"], &[], 0);
        let mut tg = card("true_grit");
        tg.upgraded = true;
        c.hand = vec![tg, card("strike")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "坚毅+ 1 张候选:自动结算");
        assert_eq!(c.player.block, 9, "格挡照给");
        assert!(c.exhaust.iter().any(|x| x.def.id == "strike"), "那张被消耗");
    }

    /// 多张候选:照常开屏,一张都还没选
    #[test]
    fn multiple_candidates_still_open_the_window() {
        let mut c = board(&["headbutt"], &[], 0);
        c.hand = vec![card("headbutt")];
        c.discard = vec![card("defend"), card("strike"), card("bash")];
        c.play_card(0, Some(0)).unwrap();
        let ch = c.choice.as_ref().expect("多候选照常开屏");
        assert_eq!(ch.mode, ChoiceMode::Mandatory);
        assert_eq!(c.choice_candidates().len(), 3);
        assert_eq!(ch.taken, 0, "开屏时一张都还没选");
    }

    /// 1 张候选但可选:仍然开窗口(玩家能少选/不选)
    #[test]
    fn optional_single_candidate_still_opens() {
        // 净化:手里只剩 1 张也开屏
        let mut c = board(&["purity", "strike"], &[], 0);
        c.hand = vec![card("purity"), card("strike")];
        c.play_card(0, None).unwrap();
        let ch = c.choice.as_ref().expect("净化 1 张候选也要开屏");
        assert_eq!(ch.mode, ChoiceMode::Optional);
        assert_eq!(c.choice_candidates().len(), 1);

        // 预谋升级版(任意张):手里 1 张也开屏
        let mut c = board(&["forethought", "strike"], &[], 0);
        let mut ft = card("forethought");
        ft.upgraded = true;
        c.hand = vec![ft, card("strike")];
        c.play_card(0, None).unwrap();
        let ch = c.choice.as_ref().expect("预谋+ 1 张候选也要开屏");
        assert_eq!(ch.mode, ChoiceMode::Optional);

        // 灵药:手里 1 张也开屏(消耗任意张)
        let mut c = board(&["strike"], &[], 0);
        c.hand = vec![card("strike")];
        c.use_potion(crate::core::potions::by_id("elixir_potion").unwrap(), None);
        let ch = c.choice.as_ref().expect("灵药 1 张候选也要开屏");
        assert_eq!(ch.mode, ChoiceMode::Optional);
        assert_eq!(ch.need, 0);

        // 赌徒筹码:开局手里只有 1 张也开屏(弃任意张)
        let chip = [relic_def_or_panic("gambling_chip")];
        let c = board(&["strike"], &chip, 0);
        let ch = c.choice.as_ref().expect("赌徒筹码 1 张候选也要开屏");
        assert_eq!(ch.mode, ChoiceMode::Optional);
    }

    /// 自动结算不能吞掉后面的效果(历史上的丢尾 bug 就在这里)
    #[test]
    fn auto_resolve_keeps_the_remaining_effects() {
        // 燃烧契约(消耗一张 + 抽 2):手里只剩 1 张 -> 自动消耗后照样抽 2
        let mut c = board(&["burning_pact", "strike"], &[], 0);
        c.hand = vec![card("burning_pact"), card("strike")];
        c.draw = vec![card("strike"), card("strike")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "只剩一张:自动消耗,不开屏");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "strike"),
            "那张被消耗"
        );
        assert_eq!(c.hand.len(), 2, "后续的抽 2 照跑");
        assert!(c.draw.is_empty(), "抽牌堆那 2 张被抽走");

        // 浩劫放出的带选牌顶牌(燃烧契约):顶牌的选牌自动结算,后续的抽 2 也不能丢
        let mut c = board(&[], &[], 0);
        c.hand = vec![card("havoc"), card("strike")];
        c.draw = vec![card("burning_pact"), card("strike"), card("strike")];
        c.play_card(0, Some(0)).unwrap();
        assert!(c.choice.is_none(), "顶牌的强制单选也只有 1 张:自动结算");
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "burning_pact"),
            "顶牌打完要消耗"
        );
        assert!(
            c.exhaust.iter().any(|x| x.def.id == "strike"),
            "被自动选中的那张也消耗了"
        );
        assert_eq!(c.hand.len(), 2, "顶牌的抽 2 照跑");
        assert!(c.draw.is_empty(), "抽牌堆那 2 张被抽走");
    }

    /// 自动结算也要带上附加项:二重身升级版只剩 1 张攻击/能力牌时,一次复制两份
    #[test]
    fn auto_resolve_keeps_the_extra_copies() {
        let mut c = board(&["strike"], &[], 0);
        let mut dw = card("dual_wield");
        dw.upgraded = true;
        c.hand = vec![dw, card("strike")];
        c.play_card(0, None).unwrap();
        assert!(c.choice.is_none(), "只剩一张可复制:自动结算");
        let strikes = c.hand.iter().filter(|x| x.def.id == "strike").count();
        assert_eq!(strikes, 3, "原来的 1 张 + 自动复制的 2 张");
    }

    /// 笔尖翻倍排在力量/活力**之后**:反编译把 PenNib 当 powers 的 AtDamageGive
    /// (BattleContext.cpp:2689 +STRENGTH -> 2691 +VIGOR -> 2698 PEN_NIB x2 ->
    /// 2702 WEAK x0.75),所以第 10 张攻击是 (base+力量+活力)x2,而不是 basex2+力量.
    /// 本作此前把翻倍放在加力量/活力之前(照参考实现的 relic-hook 口径),差一截.
    #[test]
    fn pen_nib_doubles_after_strength_and_vigor() {
        let pen = relic_def_or_panic("pen_nib");
        let mut c = board(&["strike"], &[pen], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("strike")];
        c.player.statuses.add(Status::Strength, 2);
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - (6 + 2) * 2, "力量先加再翻倍:(6+2)x2=16");

        // 活力(赤牛 8 点)也是加法,同样排在翻倍之前:(6+8)x2=28
        let mut c = board(&["strike"], &[pen, relic_def_or_panic("akabeko")], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("strike")];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(
            c.enemies[0].hp,
            500 - (6 + 8) * 2,
            "活力先加再翻倍:(6+8)x2=28"
        );

        // 多段攻击:一张牌只折一次这份值,每段共用(反编译 attackPlayerHelper 先算
        // 一次 damage 再逐段 AttackPlayer).连击(5x2)吃笔尖是 (5x2) 每段 10,合计 20,
        // 不是每段各自翻倍 20+20.
        let mut c = board(&["twin_strike"], &[pen], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("twin_strike")];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 20, "连击每段 5 先翻倍成 10,两段共 20");
    }

    /// 遗物 atDamageModify(打击假人 +3 / 腕刃 +4)排在 powers 与笔尖之前,所以会被
    /// 笔尖一起翻倍:反编译 BattleContext.cpp:2677-2683 的 STRIKE_DUMMY/WRIST_BLADE 在
    /// 2689 的 +STRENGTH 之前,笔尖在 2698.本作由 resolve_effects 先把 relic_add 并进
    /// base(player_attack_damage 的入参),再走 powers/笔尖/易伤.
    #[test]
    fn strike_dummy_and_wrist_blade_add_before_the_powers() {
        // 打击假人 +3 与力量 2 同为加法:(6+3+2)=11
        let mut c = board(&["strike"], &[relic_def_or_panic("strike_dummy")], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("strike")];
        c.player.statuses.add(Status::Strength, 2);
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 11, "假人 +3 与力量同为加法:(6+3+2)");

        // 打击假人 +3 在笔尖之前:(6+3)x2=18
        let mut c = board(
            &["strike"],
            &[relic_def_or_panic("strike_dummy"), relic_def_or_panic("pen_nib")],
            0,
        );
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("strike")];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 18, "假人的 +3 也吃笔尖翻倍:(6+3)x2");

        // 腕刃 +4(本回合费用降到 0)也在笔尖之前:(6+4)x2=20
        let mut c = board(
            &["strike"],
            &[relic_def_or_panic("wrist_blade"), relic_def_or_panic("pen_nib")],
            0,
        );
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        let mut s = card("strike");
        s.cost_delta = -1; // 本回合实际费用压到 0
        c.hand = vec![s];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(
            c.enemies[0].hp,
            500 - (6 + 4) * 2,
            "腕刃 +4 也吃笔尖翻倍:(6+4)x2"
        );
    }

    /// 纸蛙把"敌人易伤"的乘数从 1.5 提到 1.75:反编译 calculateCardDamage 里
    /// PAPER_PHROG -> x1.75(BattleContext.cpp:2721-2727).
    #[test]
    fn paper_phrog_raises_the_vulnerable_multiplier() {
        let mut c = board(&["strike"], &[relic_def_or_panic("paper_phrog")], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("strike")];
        c.add_enemy_status(0, Status::Vulnerable, 2);
        c.play_card(0, Some(0)).unwrap();
        // 6 * 1.75 = 10.5 -> floor 10
        assert_eq!(c.enemies[0].hp, 500 - 10, "纸蛙:6*1.75=10.5 -> 10");

        // 对照:没有纸蛙是 1.5 -> 9
        let mut c2 = board(&["strike"], &[], 0);
        c2.enemies[0].hp = 500;
        c2.enemies[0].block = 0;
        c2.hand = vec![card("strike")];
        c2.add_enemy_status(0, Status::Vulnerable, 2);
        c2.play_card(0, Some(0)).unwrap();
        assert_eq!(c2.enemies[0].hp, 500 - 9, "默认易伤:6*1.5=9");
    }

    /// 奇异蘑菇把"玩家自己易伤"的乘数从 1.5 降到 1.25:反编译
    /// Monster::calculateDamageToPlayer 里 ODD_MUSHROOM -> x1.25(Monster.cpp:578-584).
    #[test]
    fn odd_mushroom_lowers_the_vulnerable_multiplier_on_the_player() {
        let relics = [relic_def_or_panic("odd_mushroom")];
        let mut c = board(&["defend"], &relics, 0);
        c.player.statuses.add(Status::Vulnerable, 2);
        assert_eq!(c.enemy_attack_damage(0, 12), 15, "奇异蘑菇:12*1.25=15");

        let mut c2 = board(&["defend"], &[], 0);
        c2.player.statuses.add(Status::Vulnerable, 2);
        assert_eq!(c2.enemy_attack_damage(0, 12), 18, "默认:12*1.5=18");
    }

    /// 纸鹤把"怪物自身虚弱"的乘数从 0.75 降到 0.6:反编译
    /// Monster::calculateDamageToPlayer 里 PAPER_KRANE -> x0.6(Monster.cpp:570-576).
    #[test]
    fn paper_krane_weakens_the_monsters_attack_more() {
        let relics = [relic_def_or_panic("paper_krane")];
        let mut c = board(&["defend"], &relics, 0);
        c.enemies[0].statuses.add(Status::Weak, 2);
        assert_eq!(c.enemy_attack_damage(0, 12), 7, "纸鹤:12*0.6=7.2 -> 7");

        let mut c2 = board(&["defend"], &[], 0);
        c2.enemies[0].statuses.add(Status::Weak, 2);
        assert_eq!(c2.enemy_attack_damage(0, 12), 9, "默认:12*0.75=9");
    }

    /// 怪物侧加伤链:base + 怪物力量(加法,最前)-> 被夹击 x1.5 -> 虚弱 x0.75/0.6 ->
    /// 玩家易伤 x1.5/1.25 -> 无形 -> floor.反编译 Monster::calculateDamageToPlayer
    /// (Monster.cpp:561-597);力量之外全是乘法,所以力量必须最先加进去.
    #[test]
    fn monster_strength_is_added_before_the_multipliers() {
        let mut c = board(&["defend"], &[], 0);
        c.enemies[0].statuses.add(Status::Strength, 3);
        assert_eq!(c.enemy_attack_damage(0, 12), 15, "力量 +3:12+3=15");

        // 力量先加、再吃玩家易伤:(12+3)*1.5=22.5 -> 22
        c.player.statuses.add(Status::Vulnerable, 2);
        assert_eq!(
            c.enemy_attack_damage(0, 12),
            22,
            "力量先加,再乘易伤:(12+3)*1.5 -> 22"
        );
    }

    /// 重击(Heavy Blade)把力量按 3 倍(升级 5 倍)计入,但必须走和别的攻击一样的全序:
    /// 笔尖翻倍、遗物加伤、易伤/弱体只取整一次.反编译 BattleContext.cpp:1054-1058 先算
    /// `14 + (升?4:2)*STRENGTH` 再交 calculateCardDamage;参考实现 ironclad/common.ts 同口径.
    /// 此前这里自算 0.75/1.5 并各自 floor,还漏掉笔尖与腕刃.
    #[test]
    fn heavy_blade_runs_the_standard_damage_pipeline() {
        // 力量 5:14 + 5*3 = 29
        let mut c = board(&["heavy_blade"], &[], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("heavy_blade")];
        c.player.statuses.add(Status::Strength, 5);
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 29, "力量 5:14+5*3=29");

        // 力量 5 + 自身弱体 + 敌人易伤:只取整一次 (14+15)*0.75*1.5=32.625 -> 32
        let mut c = board(&["heavy_blade"], &[], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("heavy_blade")];
        c.player.statuses.add(Status::Strength, 5);
        c.player.statuses.add(Status::Weak, 2);
        c.add_enemy_status(0, Status::Vulnerable, 2);
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 32, "(14+15)*0.75*1.5=32.625 -> 32");

        // 笔尖把重击也翻倍:14*2=28
        let mut c = board(&["heavy_blade"], &[relic_def_or_panic("pen_nib")], 0);
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        c.hand = vec![card("heavy_blade")];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 28, "笔尖把重击也翻倍:14*2=28");

        // 腕刃(+4,本回合费用压到 0)再被笔尖翻倍:(14+4)*2=36
        let mut c = board(
            &["heavy_blade"],
            &[relic_def_or_panic("wrist_blade"), relic_def_or_panic("pen_nib")],
            0,
        );
        c.enemies[0].hp = 500;
        c.enemies[0].block = 0;
        let mut hb = card("heavy_blade");
        hb.cost_delta = -2; // 本回合压到 0 费
        c.hand = vec![hb];
        c.rs.pen_nib = 9;
        c.play_card(0, Some(0)).unwrap();
        assert_eq!(c.enemies[0].hp, 500 - 36, "腕刃 +4 也翻倍:(14+4)*2=36");
    }
}

/// 怪物选招的飞升档位分支(A17 / A18 / A19)逐条断言,外加一张全怪物选招表.
///
/// 做法:直接驱动 `EnemyDef.pick` —— 造一个只含这只怪(以及可选配角)的最小战场,
/// 把引擎每回合必掷的那次 `aiRng.random(99)` 当 `first_roll` 传进去.这样能把
/// "掷点落在哪一档"钉死,精确核对边界(`asc >= 18` 与 `asc > 18` 一字之差都会翻车),
/// 比跑真实战斗观察出招可靠得多.
///
/// 全怪物飞升档位选招表(怪物 -> 档位 -> 分支 -> 断言名).依据是
/// refs/sts_lightspeed/src/combat/MonsterSpecific.cpp 的 `getMoveForRoll`(该函数从
/// 1882 行起;下面括号里是分支所在行):
///
/// | 怪物                | 档位 | 分支(与 A0 的差别)                                    | 断言 |
/// |---------------------|------|-------------------------------------------------------|------|
/// | 酸液史莱姆(小)      | 17   | 首招固定舔一口(不再 50/50)                            | acid_slime_small_a17_first_move_is_always_lick |
/// | 酸液史莱姆(中/大)   | 17   | 档位 30/70 -> 中 40/80、大 40/70;"舔"只看前一招        | acid_slime_thresholds_move_at_17 |
/// | 尖刺史莱姆(中/大)   | 17   | "舔"只需不看前一招即可再出                            | spike_slime_lick_gate_moves_at_17 |
/// | 蓝奴隶主            | 17   | 耙只看前一招(不再看最近两招)                          | blue_slaver_rake_gate_moves_at_17 |
/// | 红奴隶主            | 17   | 耙只看前一招                                          | red_slaver_scrape_gate_moves_at_17 |
/// | 红/绿虱子           | 17   | 特殊招只看前一招                                      | louse_special_gate_moves_at_17 |
/// | 被选中者            | 17   | 首招直接上咒;第二回合不再强制上咒                     | chosen_a17_opener_and_second_turn |
/// | 甲壳寄生体          | 17   | 首招固定重击(不再 50/50)                              | shelled_parasite_a17_first_move_is_fell |
/// | 蛇草                | 17   | 孢子只看前一招                                        | snake_plant_spores_gate_moves_at_17 |
/// | 神秘客              | 17   | 治疗门槛 16->21;削弱打击只看前一招                    | mystic_a17_heal_threshold_and_debuff_gate |
/// | 小鬼头目            | 18   | 固定节奏:最近两招无头槌就头槌,否则冲锋                | gremlin_nob_a18_locks_the_skull_bash_rush_rush_pattern |
/// | 刺击之书            | 18   | 单刺也自增刺击数                                      | book_of_stabbing_a18_single_stab_also_grows_the_count |
/// | 铜制机械人          | 19   | 光束之后接增幅(不再眩晕)                              | bronze_automaton_a19_hyper_beam_then_boost |
/// | 勇士                | 19   | 防守姿态掷点门槛 15->30                               | the_champ_a19_stance_threshold |
/// | 小鬼巫师            | 17   | 首次大招之后每回合都放大招(不再重新充能)              | gremlin_wizard_a17_blasts_every_turn |
/// | 尖塔增生            | 17   | 缠绕不再看掷点门槛                                    | spire_growth_a17_constricts_without_roll_gate |
/// | 巨大头颅            | 18   | "时候到了"提前一回合(行动 3 回合即定)                 | giant_head_uses_it_is_time_one_turn_earlier_at_a18 |
///
/// 另外几条 A17/A18/A19 分支落在"招式执行"(takeTurn)而不是选招上, 由
/// core::ascension 的数据层覆盖并已有 e2e:任务达人鞭打(A18 加 1 力 + 3 伤口)、
/// 尖塔盾猛击(A18 挡 99)、尖塔枪燃击(A18 燃烧入抽牌堆)、收集者/时间吞噬者/
/// 德卡(19 的各段数值)、雷普托曼瑟(A18 召唤 2 只匕首).这些的边界由
/// ascension.rs 的 every_move_tier_takes_effect_exactly_at_its_level 逐个钉住.
#[cfg(test)]
mod ascension_move_branches {
    use super::*;

    fn mk(def: &'static EnemyDef, hp: i32, max_hp: i32, slot: usize) -> Enemy {
        Enemy {
            def,
            name: def.name.to_string(),
            hp,
            max_hp,
            block: 0,
            statuses: Statuses::default(),
            next_move: 0,
            death_done: false,
            temp_strength: 0,
            fresh_powers: Vec::new(),
            escaped: false,
            slot,
            uid: slot as u64 + 1,
            asc: 0,
            state: EnemyState::default(),
        }
    }

    /// 开局那一掷(首招还没掷)
    fn opening() -> EnemyState {
        EnemyState::default()
    }

    /// 已经掷过首招, 招式历史 = (last, prev);prev 给 None 表示只行动过一回合
    fn hist(last: Option<usize>, prev: Option<usize>) -> EnemyState {
        EnemyState {
            move_rolled: true,
            last,
            prev,
            ..Default::default()
        }
    }

    /// 跑一次选招,返回 (选中的招下标, 选招函数记完账的状态).
    /// board 是整条槽位表 (def, hp, max_hp);me_idx 是被测怪所在槽位.
    fn pick_in(
        board: &[(&'static EnemyDef, i32, i32)],
        me_idx: usize,
        asc: u32,
        first_roll: i32,
        state: EnemyState,
        seed: u64,
    ) -> (usize, EnemyState) {
        let def = board[me_idx].0;
        let mut enemies: Vec<Enemy> = board
            .iter()
            .enumerate()
            .map(|(i, (d, hp, mx))| mk(d, *hp, *mx, i))
            .collect();
        enemies[me_idx].state = state.clone();
        let player = PlayerBattle {
            hp: 80,
            max_hp: 80,
            block: 0,
            statuses: Statuses::default(),
            fresh_debuffs: Vec::new(),
        };
        let mut rng = RngRegistry::new(seed);
        let mut live = state;
        let mut ctx = crate::core::enemy::PickCtx {
            rng: &mut rng,
            idx: me_idx,
            all: &enemies,
            player: &player,
            state: &mut live,
            asc,
            first_roll,
            roll_consumed: false,
        };
        let ret = (def.pick)(&mut ctx);
        (ret, live)
    }

    /// 单怪表格:一条 = 说明 + 敌人 id + 招式历史 + 掷点 + 两档 asc 与期望招名
    struct Case {
        what: &'static str,
        id: &'static str,
        state: fn(&'static EnemyDef) -> EnemyState,
        first_roll: i32,
        lo: (u32, &'static str),
        hi: (u32, &'static str),
    }

    fn name_of(def: &'static EnemyDef, idx: usize) -> &'static str {
        def.moves[idx].name
    }

    #[test]
    fn ascension_move_selection_boundaries() {
        use crate::core::enemies::enemy_def;
        let cases: &[Case] = &[
            // 酸液中史:A0 掷点 30/70,A17 改成 40/80(中).roll=35 在 A16 是撞、A17 变吐口水
            Case {
                what: "酸液中史 档位 30->40",
                id: "acid_slime_medium",
                state: |_| hist(None, None),
                first_roll: 35,
                lo: (16, "Tackle"),
                hi: (17, "Corrosive Spit"),
            },
            // 酸液大史:A0 30/70,A17 40/70
            Case {
                what: "酸液大史 档位 30->40",
                id: "acid_slime_large",
                state: |_| hist(None, None),
                first_roll: 35,
                lo: (16, "Tackle"),
                hi: (17, "Corrosive Spit"),
            },
            // 尖刺史莱姆(大):上招是舔,A0 再舔,A17 出冲撞
            Case {
                what: "尖刺大史 舔只看前一招",
                id: "spike_slime_large",
                state: |d| hist(Some(d.move_index("Lick").unwrap()), None),
                first_roll: 50,
                lo: (16, "Lick"),
                hi: (17, "Flame Tackle"),
            },
            // 蓝奴隶主:上招耙一次,A0 允许再耙(最近两招才挡),A17 直接改捅
            Case {
                what: "蓝奴隶主 耙只看前一招",
                id: "blue_slaver",
                state: |d| hist(Some(d.move_index("Rake").unwrap()), None),
                first_roll: 10,
                lo: (16, "Rake"),
                hi: (17, "Stab"),
            },
            // 红奴隶主:同理,上招耙,A17 直接改捅
            Case {
                what: "红奴隶主 耙只看前一招",
                id: "red_slaver",
                state: |d| hist(Some(d.move_index("Scrape").unwrap()), None),
                first_roll: 10,
                lo: (16, "Scrape"),
                hi: (17, "Stab"),
            },
            // 红虱子:上招特殊,A0 允许再特殊,A17 改咬
            Case {
                what: "红虱子 特殊只看前一招",
                id: "red_louse",
                state: |d| hist(Some(d.move_index("Grow").unwrap()), None),
                first_roll: 10,
                lo: (16, "Grow"),
                hi: (17, "Bite"),
            },
            // 被选中者:只行动过一回合时,A0 必上咒,A17 直接掷(掷到削弱打击)
            Case {
                what: "被选中者 第二回合不再强制上咒",
                id: "chosen",
                state: |d| hist(Some(d.move_index("Poke").unwrap()), None),
                first_roll: 10,
                lo: (16, "Hex"),
                hi: (17, "Debilitate"),
            },
            // 蛇草:上招孢子,A0 转啃咬,A17 允许连续孢子
            Case {
                what: "蛇草 孢子只看前一招",
                id: "snake_plant",
                state: |d| hist(Some(d.move_index("Enfeebling Spores").unwrap()), None),
                first_roll: 70,
                lo: (16, "Chomp"),
                hi: (17, "Enfeebling Spores"),
            },
            // 小鬼头目:只吼过一嗓子,A0 掷到大点出冲锋,A18 走固定节奏出头槌
            Case {
                what: "小鬼头目 A18 固定节奏",
                id: "gremlin_nob",
                state: |d| hist(Some(d.move_index("Bellow").unwrap()), None),
                first_roll: 50,
                lo: (17, "Rush"),
                hi: (18, "Skull Bash"),
            },
            // 铜制机械人:光束之后,A18 眩晕,A19 改增幅
            Case {
                what: "铜制机械人 A19 光束接增幅",
                id: "bronze_automaton",
                state: |d| hist(Some(d.move_index("Hyper Beam").unwrap()), None),
                first_roll: 0,
                lo: (18, "Stunned"),
                hi: (19, "Boost"),
            },
            // 勇士:防守姿态门槛 15->30,roll=20 在 A18 是嘲弄、A19 变防守姿态
            Case {
                what: "勇士 A19 防守姿态门槛",
                id: "the_champ",
                state: |d| {
                    let mut s = hist(Some(d.move_index("Heavy Slash").unwrap()), None);
                    s.turns = 1; // (turns+1) % 4 != 0,避开嘲讽回合
                    s
                },
                first_roll: 20,
                lo: (18, "Gloat"),
                hi: (19, "Defensive Stance"),
            },
            // 小鬼巫师:首爆之后,A16 重新充能,A17 每回合放大招
            Case {
                what: "小鬼巫师 A17 连续大招",
                id: "gremlin_wizard",
                state: |d| hist(Some(d.move_index("Ultimate Blast").unwrap()), None),
                first_roll: 0,
                lo: (16, "Charging"),
                hi: (17, "Ultimate Blast"),
            },
            // 尖塔增生:掷点小,A16 走快速撞击,A17 去掉门槛直接缠绕
            Case {
                what: "尖塔增生 A17 去掉缠绕门槛",
                id: "spire_growth",
                state: |_| hist(None, None),
                first_roll: 10,
                lo: (16, "Quick Tackle"),
                hi: (17, "Constrict"),
            },
            // 巨大头颅:只行动过 3 回合,A17 还在数数,A18 已经"时候到了"
            Case {
                what: "巨大头颅 A18 提前一回合",
                id: "giant_head",
                state: |_| {
                    let mut s = hist(Some(0), Some(0));
                    s.turns = 3;
                    s
                },
                first_roll: 0,
                lo: (17, "Glare"),
                hi: (18, "It Is Time"),
            },
        ];

        for c in cases {
            let def = enemy_def(c.id).unwrap_or_else(|| panic!("no enemy {}", c.id));
            let board = [(def, 80, 80)];
            for (asc, want) in [c.lo, c.hi] {
                let st = (c.state)(def);
                let (idx, _) = pick_in(&board, 0, asc, c.first_roll, st, 7);
                assert_eq!(
                    name_of(def, idx),
                    want,
                    "{} @A{} (first_roll={})",
                    c.what,
                    asc,
                    c.first_roll
                );
            }
        }
    }

    /// 神秘客:治疗门槛 16->21;削弱打击只看前一招.
    /// 依据 MonsterSpecific.cpp 的 MYSTIC 分支(healNeedAmt = asc17 ? 21 : 16).
    #[test]
    fn mystic_a17_heal_threshold_and_debuff_gate() {
        let def = crate::core::enemies::enemy_def("mystic").unwrap();
        let knight = crate::core::enemies::enemy_def("centurion").unwrap();
        let board = [(knight, 80, 80), (def, 40, 58)];
        let debuff = def.move_index("Attack Debuff").unwrap();

        // 缺血 18:16 档就治,A17 的 21 档还不治(掷点 0 -> 增益)
        let st = hist(Some(0), Some(0));
        let (lo, _) = pick_in(&board, 1, 16, 0, st.clone(), 7);
        assert_eq!(name_of(def, lo), "Heal");
        let (hi, _) = pick_in(&board, 1, 17, 0, st, 7);
        assert_eq!(name_of(def, hi), "Buff");

        // 上招削弱打击:掷点 50,A16 允许再来一发(只看最近两招),A17 转增益.
        // 这一组满血(自己与骑士都不缺血),排除治疗的干扰
        let full = [(knight, 80, 80), (def, 58, 58)];
        let st = hist(Some(debuff), None);
        let (lo, _) = pick_in(&full, 1, 16, 50, st.clone(), 7);
        assert_eq!(name_of(def, lo), "Attack Debuff");
        let (hi, _) = pick_in(&full, 1, 17, 50, st, 7);
        assert_eq!(name_of(def, hi), "Buff");
    }

    /// 刺击之书:A18 起"单刺"也要给刺击数 +1(反编译把两处 `if (asc18) ++stabCount`
    /// 写在 return 之后成了死代码,这里按它的意图钉住;出处 MonsterSpecific.cpp
    /// 的 BOOK_OF_STABBING 分支).
    #[test]
    fn book_of_stabbing_a18_single_stab_grows_the_count() {
        let def = crate::core::enemies::enemy_def("book_of_stabbing").unwrap();
        let multi = def.move_index("Multi Stab").unwrap();
        let board = [(def, 160, 160)];
        let st = hist(Some(multi), None);

        let (lo, lo_state) = pick_in(&board, 0, 16, 0, st.clone(), 7);
        assert_eq!(name_of(def, lo), "Single Stab");
        assert_eq!(lo_state.stab, 0, "A16 单刺不自增");
        let (hi, hi_state) = pick_in(&board, 0, 18, 0, st, 7);
        assert_eq!(name_of(def, hi), "Single Stab");
        assert_eq!(hi_state.stab, 1, "A18 单刺自增一层");
    }

    /// 酸液中史:上招是舔时,A0 允许再舔(它只看最近两招),A17 改看前一招 ——
    /// roll>=80 那一支必转出攻击.出处 MonsterSpecific.cpp 的中史莱姆分支
    /// (A17 段用 `lastMove(LICK)`,A0 段用 `lastTwoMoves(LICK)`,两者回退概率不同).
    #[test]
    fn acid_slime_medium_lick_gate_moves_at_17() {
        let def = crate::core::enemies::enemy_def("acid_slime_medium").unwrap();
        let lick = def.move_index("Lick").unwrap();
        let board = [(def, 30, 30)];
        let st = hist(Some(lick), None);
        let (lo, _) = pick_in(&board, 0, 16, 90, st.clone(), 7);
        assert_eq!(name_of(def, lo), "Lick", "A16 上招是舔还会再舔");
        let (hi, _) = pick_in(&board, 0, 17, 90, st, 7);
        assert!(
            matches!(name_of(def, hi), "Corrosive Spit" | "Tackle"),
            "A17 上招是舔就必转攻击,实得 {}",
            name_of(def, hi)
        );
    }

    /// 酸液史莱姆(小):首招 A17 固定舔一口,不再 50/50(反编译 ACID_SLIME_S 的
    /// `if (asc17) return LICK`).
    #[test]
    fn acid_slime_small_a17_first_move_is_always_lick() {
        let def = crate::core::enemies::enemy_def("acid_slime_small").unwrap();
        let board = [(def, 10, 10)];
        let mut a16 = std::collections::BTreeSet::new();
        let mut a17 = std::collections::BTreeSet::new();
        for seed in 0..64u64 {
            let (lo, _) = pick_in(&board, 0, 16, 0, opening(), seed);
            a16.insert(name_of(def, lo));
            let (hi, _) = pick_in(&board, 0, 17, 0, opening(), seed);
            a17.insert(name_of(def, hi));
        }
        assert_eq!(a17.into_iter().collect::<Vec<_>>(), vec!["Lick"], "A17 必舔");
        assert!(a16.contains("Lick"), "A16 掷得出舔: {a16:?}");
        assert!(a16.contains("Tackle"), "A16 掷得出撞: {a16:?}");
    }

    /// 甲壳寄生体:首招 A17 固定重击,不再 50/50(反编译 SHELLED_PARASITE 的
    /// `if (asc17) return FELL`).
    #[test]
    fn shelled_parasite_a17_first_move_is_fell() {
        let def = crate::core::enemies::enemy_def("shelled_parasite").unwrap();
        let board = [(def, 70, 70)];
        let mut a16 = std::collections::BTreeSet::new();
        let mut a17 = std::collections::BTreeSet::new();
        for seed in 0..64u64 {
            let (lo, _) = pick_in(&board, 0, 16, 0, opening(), seed);
            a16.insert(name_of(def, lo));
            let (hi, _) = pick_in(&board, 0, 17, 0, opening(), seed);
            a17.insert(name_of(def, hi));
        }
        assert_eq!(a17.into_iter().collect::<Vec<_>>(), vec!["Fell"], "A17 必重击");
        assert!(!a16.contains("Fell"), "A16 首招不该是重击: {a16:?}");
        assert_eq!(
            a16.into_iter().collect::<Vec<_>>(),
            vec!["Double Strike", "Suck"],
            "A16 首招是双击/吸血 50/50"
        );
    }

    /// 造一场最简单的战斗(牌组几张防御,不动手,只看机制)
    fn combat(enc: &'static str, asc: u32) -> Combat {
        combat_r(enc, asc, &[])
    }

    fn combat_r(enc: &'static str, asc: u32, relics: &[&'static RelicDef]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: vec![crate::core::cards::card("defend"); 3],
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc,
        };
        Combat::new(
            crate::core::enemies::encounter_def(enc).unwrap(),
            setup,
            RngRegistry::new(1),
        )
    }

    /// 无形把"直接掉血"(卡牌/能力自伤,走 Player::loseHp)也压到 1.
    /// 依据:反编译 Player::loseHp 第一句就是 INTANGIBLE(Player.cpp:261-275);
    /// 原版无形的能力文本也写明"受到的伤害与生命流失都降为 1".
    /// 之前这条路径只做钨钢棒,漏了无形.
    #[test]
    fn intangible_clamps_direct_hp_loss() {
        let mut c = combat("cultist_solo", 0);
        let hp0 = c.player.hp;
        c.lose_hp_player(6, true);
        assert_eq!(hp0 - c.player.hp, 6, "没有无形时照掉 6");

        let mut c = combat("cultist_solo", 0);
        c.player.statuses.add(Status::Intangible, 1);
        let hp0 = c.player.hp;
        c.lose_hp_player(6, true);
        assert_eq!(hp0 - c.player.hp, 1, "无形把直接掉血压到 1");

        // 无形 + 钨钢棒:先无形压到 1,再钨钢棒 -1 -> 完全不掉
        // (反编译 Player::loseHp 的顺序就是 无形 -> 钨钢棒)
        let rod = [crate::core::relics::relic_def_or_panic("tungsten_rod")];
        let mut c = combat_r("cultist_solo", 0, &rod);
        c.player.statuses.add(Status::Intangible, 1);
        let hp0 = c.player.hp;
        c.lose_hp_player(6, true);
        assert_eq!(hp0, c.player.hp, "无形 1 点再被钨钢棒减到 0,不掉血");

        // 只有钨钢棒(没无形):每次掉血少 1
        let mut c = combat_r("cultist_solo", 0, &rod);
        let hp0 = c.player.hp;
        c.lose_hp_player(6, true);
        assert_eq!(hp0 - c.player.hp, 5, "钨钢棒对直接掉血也有效");
    }

    /// 来袭估算与真实结算必须同一口径:玩家有无形时,敌人攻击的 predicted_damage
    /// 要等于真打这一下掉的血.
    /// 依据:反编译 Monster::calculateDamageToPlayer 末尾就折无形
    /// (refs/sts_lightspeed/src/combat/Monster.cpp:590-592),Player::attacked 反而不折
    /// ("assume intangible is already handled",Player.cpp:211-213).本作真实结算走
    /// hit_player_kind、估算走 enemy_attack_damage,两边都要折.act2 seed 92 的重放策略
    /// 分家就是估算侧漏折无形(估 10、真掉 1)造成的.
    #[test]
    fn predicted_damage_matches_actual_hp_loss_with_intangible() {
        // 邪教徒的 Dark Strike 是 6 点攻击,玩家 0 格挡
        let mut c = combat("cultist_solo", 0);
        c.enemies[0].next_move = 1;
        assert_eq!(c.predicted_damage(0), (6, 1), "没有无形时估 6");
        let hp0 = c.player.hp;
        c.enemy_attack(0, 6, 1, "Cultist", "Dark Strike");
        assert_eq!(hp0 - c.player.hp, 6, "没有无形时真掉 6");

        // 有无形:估算压到 1,真打也掉 1
        let mut c = combat("cultist_solo", 0);
        c.enemies[0].next_move = 1;
        c.player.statuses.add(Status::Intangible, 1);
        assert_eq!(c.predicted_damage(0), (1, 1), "无形把这一击估到 1");
        let (per, _times) = c.predicted_damage(0);
        let hp0 = c.player.hp;
        c.enemy_attack(0, 6, 1, "Cultist", "Dark Strike");
        assert_eq!(hp0 - c.player.hp, per, "单段:估算 == 实伤");

        // 多段:每段都折到 1,总实伤 = 段数,与策略算的 per * times 一致
        let mut c = combat("cultist_solo", 0);
        c.enemies[0].next_move = 1;
        c.player.statuses.add(Status::Intangible, 1);
        let (per, _) = c.predicted_damage(0);
        let hp0 = c.player.hp;
        c.enemy_attack(0, 6, 4, "Cultist", "Dark Strike");
        assert_eq!(hp0 - c.player.hp, per * 4, "四段:估算总和 == 实伤");
    }

    /// 移形换影在"非攻击伤害"上也触发.依据:反编译把它同时挂在
    /// Monster::attackedUnblockedHelper 与 damageUnblockedHelper(Monster.cpp:339-500)
    /// 两条路上,参考实现的 SHIFTING 也挂在 wasHPLost 上,不限攻击.
    /// 之前只在 is_attack 分支里扣力,非攻击伤害(燃烧/荆棘反伤之类)漏了.
    #[test]
    fn shifting_also_triggers_on_non_attack_damage() {
        let mut c = combat("transient_solo", 0);
        assert!(c.enemies[0].statuses.holds(Status::Shifting), "瞬变体自带移形换影");
        let str0 = c.enemies[0].statuses.get(Status::Strength);

        c.damage_enemy_plain(0, 7); // 非攻击伤害
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            str0 - 7,
            "非攻击伤害也等量扣力"
        );
        assert_eq!(c.enemies[0].temp_strength, 7, "扣掉的力量记着回合末回补");
    }

    /// 卡钳:回合开始时只掉 15 点格挡,不清空(反编译 BattleContext.cpp:2181-2188 的
    /// Barricade > Blur > Calipers > 清空 那条链).Blur 是绿职专属卡、超出本作范围,
    /// 这条链只剩 Barricade 与 Calipers 两支,所以这里只钉卡钳.
    #[test]
    fn calipers_keeps_all_but_fifteen_block() {
        let cal = [crate::core::relics::relic_def_or_panic("calipers")];
        let mut c = combat_r("cultist_solo", 0, &cal);
        c.player.block = 40;
        c.start_turn(0);
        assert_eq!(c.player.block, 25, "40 - 15 = 25,不是清空");

        // 没有卡钳就整块清掉
        let mut c = combat("cultist_solo", 0);
        c.player.block = 40;
        c.start_turn(0);
        assert_eq!(c.player.block, 0, "没有卡钳照常清空");
    }

    /// 主宰:获得格挡就让一个随机活怪吃伤害;"非卡牌"来源的格挡也触发
    /// (反编译 Juggernaut 判在 Player::gainBlock 里,遗物/能力给的格挡同样走 gainBlock).
    #[test]
    fn juggernaut_triggers_on_every_block_gain() {
        let mut c = combat("cultist_solo", 0);
        c.player.statuses.add(Status::Juggernaut, 5);

        let hp0 = c.enemies[0].hp;
        c.gain_block(3, false, true); // 卡牌格挡
        assert_eq!(hp0 - c.enemies[0].hp, 5, "卡牌格挡触发主宰");

        let hp1 = c.enemies[0].hp;
        c.gain_block(3, false, false); // 遗物/能力格挡
        assert_eq!(hp1 - c.enemies[0].hp, 5, "非卡牌格挡也触发主宰");
    }

}
