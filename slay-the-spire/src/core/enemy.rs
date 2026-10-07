// 敌人的静态定义:招式表、出招 AI、意图.
// 具体敌人与遭遇的数据在 enemies.rs(按 act 分成几个子模块).
use crate::core::combat::{Enemy, PlayerBattle};
use crate::core::status::{Status, Statuses};
use crate::rng::{FloorStream, Rng, RngRegistry};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnemyKind {
    Normal,
    Elite,
    Boss,
}

impl EnemyKind {
    pub fn name(self) -> &'static str {
        match self {
            EnemyKind::Normal => "Normal",
            EnemyKind::Elite => "Elite",
            EnemyKind::Boss => "Boss",
        }
    }
}

/// 招式作用的范围
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// 只有自己
    SelfOnly,
    /// 场上所有活着的敌人(含自己)
    Team,
    /// 除了自己以外的同伴(没有就落空)
    Allies,
    /// 随机一个活着的同伴(包括自己)
    RandomOne,
}

/// 牌被塞进玩家的哪个牌堆
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardSpot {
    Discard,
    /// 洗进抽牌堆
    DrawShuffle,
    /// 永久塞进牌组(战斗结束后还在)
    Deck,
}

/// 敌人招式里的一段效果
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EnemyFx {
    /// 攻击玩家 amount 伤害 times 次
    Attack { amount: i32, times: u8 },
    /// 伤害随自己行动过的回合数增长:amount + per_turn * min(turns-1, cap)
    AttackScaling {
        amount: i32,
        per_turn: i32,
        cap: u32,
        times: u8,
    },
    /// 命中次数随回合数增长:ceil(turns / 2) 次
    AttackGrowing { amount: i32 },
    /// 命中次数由"连续刺击数"决定(刺击之书)
    AttackStabCount { amount: i32 },
    /// 伤害用构造时掷出的固定值(虱子的咬、暗灵的撕咬)
    AttackRolled { times: u8 },
    /// 非攻击伤害:不吃力量/虚弱/易伤,但会被格挡
    PlainDamage { amount: i32 },
    /// 自己/同伴获得格挡
    Block { amount: i32, scope: Scope },
    /// 获得与自己这招打出的伤害等量的格挡
    BlockFromDamage,
    /// 获得状态
    GainStatus { status: Status, n: i32, scope: Scope },
    /// 给玩家上状态
    PlayerStatus { status: Status, n: i32 },
    /// 回血
    Heal { n: i32, scope: Scope },
    /// 回复自己这招造成的伤害
    HealFromDamage,
    /// 把自己的生命补到上限的一半(时间吞噬者)
    HealToHalf,
    /// 清除自己的减益
    ClearDebuffs,
    /// 把负的力量清零之后再加力量(心脏)
    ResetStrength { n: i32 },
    /// 心脏的递增增益:按阶段获得神器/死亡律动/痛苦刺击/力量
    Escalate,
    /// 往玩家牌堆塞牌
    PlayerCard {
        card: &'static str,
        spot: CardSpot,
        n: i32,
    },
    /// 偷玩家金币
    StealGold { n: i32 },
    /// 偷玩家一张牌(圆球哨卫的停滞)
    StealCard,
    /// 召唤固定的几只.参考实现里每只怪占一个固定槽位,召唤物进的是指定的空槽
    /// (不是队尾),所以 `ids` 与 `slots` 一一对应:第 i 只放进 slots[i].
    Summon {
        ids: &'static [&'static str],
        slots: &'static [u8],
    },
    /// 从池子里随机抽 count 只召唤(小鬼头目的召集):
    /// 每只单独掷点挑一个,允许抽到同一只;同样按 slots 的顺序填空槽.
    SummonRandom {
        pool: &'static [&'static str],
        count: u8,
        slots: &'static [u8],
    },
    /// 下回合少抽牌
    DrawReduction { n: i32 },
    /// 小鬼巫师充能:计数加一,本身没有别的效果
    Charge,
    /// 第 at_turn 次行动时醒来(拉格文):移除睡眠并扣掉金属化
    WakeUp { at_turn: u32 },
    /// 把 state.rolled 设成"玩家当前生命 / div + add"(六火幽魂的分裂伤害)
    RollDamage { div: i32, add: i32 },
    /// 去掉某个状态(守护者的双拳合击会打散尖刺外壳)
    LoseStatus { status: Status, scope: Scope },
    /// 指定下一招(巨口的 NOM 之后必接 DROOL 之类)
    ForceNext { idx: usize },
    /// 大史莱姆分裂:自己被两只小史莱姆替换(种类见 EnemyDef::special)
    Split,
    /// 自己立刻死亡(自爆/献祭)
    Suicide,
    /// 从战斗中逃离(保留已偷的金币)
    Escape,
}

/// 一个招式:名字、意图、效果
#[derive(Clone, Copy, Debug)]
pub struct MoveDef {
    pub name: &'static str,
    pub intent: Intent,
    pub effects: &'static [EnemyFx],
}

/// 选招函数:读战场 + 拿随机数(状态是副本,可以就地记账),返回下一招的下标
pub type PickFn = fn(&mut PickCtx) -> usize;

/// 跨回合的怪物状态.参考实现里挂在 Monster::miscInfo 上的那些计数都在这儿.
#[derive(Clone, Default, Debug)]
pub struct EnemyState {
    /// 已经行动过的回合数(参考实现里的 monsterTurnNumber)
    pub turns: u32,
    /// 上一招 / 上上招在招式表里的下标
    pub last: Option<usize>,
    pub prev: Option<usize>,
    /// 构造时掷出的固定伤害(虱子的咬、暗灵的撕咬)
    pub rolled: i32,
    /// 刺击之书:多段刺击当前是几段
    pub stab: u32,
    /// 小鬼巫师:充能计数
    pub charge: i32,
    /// 勇士:防守姿态用过几次
    pub guard_uses: u32,
    /// 二阶段(勇士 / 觉醒者)
    pub phase2: bool,
    /// 觉醒者:一阶段被打死、还没复活
    pub half_dead: bool,
    /// 圆球哨卫:停滞用过没有
    pub stasis_used: bool,
    /// 红奴隶主:缠绕用过没有
    pub entangle_used: bool,
    /// 扭动巨物:寄生用过没有
    pub implant_used: bool,
    /// 时间吞噬者:急速用过没有
    pub haste_used: bool,
    /// 心脏:递增增益到第几档
    pub stage: i32,
    /// 尖刺:已经用过几次尖刺
    pub spikes: u32,
    /// 暗灵:已经复活过一次
    pub regrow_used: bool,
    /// 暗灵:还有几回合复活
    pub regrow_ticks: i32,
    /// 本回合已经受到过的伤害(心脏的无敌)
    pub taken_this_turn: i32,
    /// 这只怪从玩家身上抢走的金币(被击杀时会还回来)
    pub stolen: i32,
    /// 被指定的下一招(某些招的后继是写死的)
    pub forced: Option<usize>,
}

/// 每只怪独有的机制.参考实现里写在 takeTurn / onHpLost 里的特殊分支.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    None,
    /// 掉到半血就分裂成两只(史莱姆).a 插在自己原来的位置,b 插在它后面
    Split {
        a: &'static str,
        b: &'static str,
    },
    /// 掉够 d 点生命就换防御姿态(守护者),guard 是防御姿态那一招的下标
    ModeShift {
        d: i32,
        guard: usize,
    },
    /// 一阶段被打死之后半血复活(觉醒者)
    Rebirth,
    /// 死后若还有同伴就半血复活一次(暗灵)
    Regrow,
    /// 每次行动完若没有无形就获得无形 2(复仇女神)
    Intangible,
    /// 受到攻击伤害就重掷下一招(扭动巨物)
    Reactive,
    /// 首领死亡时带走所有召唤物(铜制机械人 / 收集者 / 爬行者 / 小鬼头目)
    Leader,
}

/// 构造一只敌人时的掷点:卷曲层数、开局就定下来的固定伤害之类
pub struct SpawnCtx<'a> {
    pub rng: &'a mut Rng,
    pub statuses: &'a mut Statuses,
    pub state: &'a mut EnemyState,
}

pub type SpawnHook = fn(&mut SpawnCtx);

/// 大多数敌人开局不需要额外掷点
pub fn spawn_default(_: &mut SpawnCtx) {}

#[derive(Debug)]
pub struct EnemyDef {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: EnemyKind,
    /// 生命区间,闭区间随机
    pub hp: (i32, i32),
    pub moves: &'static [MoveDef],
    /// 选招规则
    pub pick: PickFn,
    /// 开局自带的状态
    pub innate: &'static [(Status, i32)],
    /// 开局自带的格挡
    pub start_block: i32,
    /// 死亡时对玩家触发(孢子云等)
    pub on_death: &'static [EnemyFx],
    /// 独有机制
    pub special: Special,
    /// 开局的额外掷点
    pub spawn: SpawnHook,
}

impl EnemyDef {
    /// 招式名对应的下标(遭遇里的预置状态按名字点招)
    pub fn move_index(&self, name: &str) -> Option<usize> {
        self.moves.iter().position(|m| m.name == name)
    }
}

/// 意图:给玩家看的预告,伤害是未计入增减益的原始值
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Intent {
    Attack { damage: i32, times: u8 },
    AttackDefend { damage: i32, times: u8, block: i32 },
    AttackDebuff { damage: i32, times: u8 },
    AttackBuff { damage: i32, times: u8 },
    Defend,
    DefendBuff { block: i32 },
    DefendDebuff { block: i32 },
    Buff,
    Debuff,
    StrongDebuff,
    Stun,
    Escape,
    Sleep,
    Unknown,
}

impl Intent {
    /// 这招是不是打人的
    pub fn attacks(self) -> bool {
        matches!(
            self,
            Intent::Attack { .. }
                | Intent::AttackDefend { .. }
                | Intent::AttackDebuff { .. }
                | Intent::AttackBuff { .. }
        )
    }
}

/// 选招时需要看到的战场.
/// `state` 是这只怪状态的副本,选招函数可以就地记账(参考实现里在 getMove 里改 miscInfo 的那些),
/// 跑完由战斗引擎写回.
pub struct PickCtx<'a> {
    pub rng: &'a mut RngRegistry,
    pub idx: usize,
    pub all: &'a [Enemy],
    pub player: &'a PlayerBattle,
    pub state: &'a mut EnemyState,
}

impl PickCtx<'_> {
    pub fn me(&self) -> &Enemy {
        &self.all[self.idx]
    }

    /// 参考实现里的 aiRng.random(99)
    pub fn roll(&mut self) -> i32 {
        self.rng.floor(FloorStream::AiRng).random(99) as i32
    }

    /// 参考实现里的 aiRng.randomRange(lo, hi)
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        self.rng.floor(FloorStream::AiRng).random_range(lo, hi)
    }

    /// 参考实现里的 aiRng.randomBoolean(p):一次 nextFloat 对概率,和原版一一对应
    pub fn flip(&mut self, num: u32, den: u32) -> bool {
        self.rng
            .floor(FloorStream::AiRng)
            .random_bool_chance(num as f32 / den.max(1) as f32)
    }

    /// 这是开局第一掷(还没行动过)
    pub fn first_turn(&self) -> bool {
        self.state.turns == 0
    }

    /// 参考实现里的 getMonsterTurnNumber():选这一招时已经行动过几回合
    pub fn turn(&self) -> u32 {
        self.state.turns
    }

    pub fn last(&self) -> Option<usize> {
        self.state.last
    }

    pub fn prev(&self) -> Option<usize> {
        self.state.prev
    }

    /// 上一招是不是 m
    pub fn last_is(&self, m: usize) -> bool {
        self.state.last == Some(m)
    }

    /// 再上一招是不是 m
    pub fn prev_is(&self, m: usize) -> bool {
        self.state.prev == Some(m)
    }

    /// 最近两招是不是都是 m(lastTwoMovesWere)
    pub fn last_two_is(&self, m: usize) -> bool {
        self.state.last == Some(m) && self.state.prev == Some(m)
    }

    /// 最近两招里有没有 m(lastTwoContain)
    pub fn last_two_has(&self, m: usize) -> bool {
        self.state.last == Some(m) || self.state.prev == Some(m)
    }


    pub fn hp(&self) -> i32 {
        self.me().hp
    }

    pub fn max_hp(&self) -> i32 {
        self.me().max_hp
    }


    pub fn alive(&self) -> usize {
        self.all.iter().filter(|e| e.alive()).count()
    }

    pub fn alive_allies(&self) -> usize {
        self.all
            .iter()
            .enumerate()
            .filter(|(i, e)| *i != self.idx && e.alive())
            .count()
    }

    /// 某个槽位上的敌人(可能是尸体)
    pub fn slot(&self, i: usize) -> Option<&Enemy> {
        self.all.get(i)
    }


    pub fn has_status(&self, s: Status) -> bool {
        self.me().statuses.has(s)
    }

    pub fn player_has(&self, s: Status) -> bool {
        self.player.statuses.has(s)
    }


}

/// 遭遇开局挂在若干只怪身上的预置状态.参考实现里在怪物构造完之后按槽位写死的那点东西.
#[derive(Clone, Copy, Debug)]
pub struct EnemyPreset {
    /// 目标在遭遇名单里的下标(一条预置可以一次点好几个槽位)
    pub slots: &'static [usize],
    /// 开局加的状态(力量、随从标记之类)
    pub statuses: &'static [(Status, i32)],
    /// 开局的格挡(叠在它自己的 start_block 之上)
    pub block: i32,
    /// 开局算"已经行动过"几回合.大于 0 时 firstTurn 为假,招式历史上的约束照它生效
    pub acted_turns: u32,
    /// 开局预置的"上一招"招式名;None 表示预置一个匹配不到任何招式的哨兵值
    pub last_move: Option<&'static str>,
}

impl EnemyPreset {
    /// 不加状态/格挡,只把这些槽位摆成"已经行动过"
    pub const fn acted(
        slots: &'static [usize],
        turns: u32,
        last_move: Option<&'static str>,
    ) -> Self {
        Self {
            slots,
            statuses: &[],
            block: 0,
            acted_turns: turns,
            last_move,
        }
    }

    /// 开局只给这些槽位挂状态
    pub const fn buffed(slots: &'static [usize], statuses: &'static [(Status, i32)]) -> Self {
        Self {
            slots,
            statuses,
            block: 0,
            acted_turns: 0,
            last_move: None,
        }
    }
}

/// 遭遇开局按参考规则重抽阵容:返回按槽位排好的敌人 id
pub type LineupFn = fn(&mut Rng) -> Vec<&'static str>;

/// 一场遭遇战:同一组的敌人 id,加上开局的阵容抽签与预置状态
#[derive(Debug)]
pub struct Encounter {
    pub id: &'static str,
    pub kind: EnemyKind,
    /// 固定阵容.有 lineup 时它只当代表阵容(图鉴/查找用),真正开局按抽签来
    pub enemies: &'static [&'static str],
    /// 开局重新抽阵容(三种"形状"遭遇).None 时直接用 enemies
    pub lineup: Option<LineupFn>,
    /// 开局按槽位施加的预置状态
    pub presets: &'static [EnemyPreset],
    /// 开局给玩家的状态(第四幕精英的被包围)
    pub player_statuses: &'static [(Status, i32)],
    /// 进随机遭遇池的权重,取自参考实现 acts.ts 的 strongEncounters.
    /// 0 表示只作为固定阵容存在,不参与抽取.
    pub weight: u32,
}

impl Encounter {
    /// 固定阵容、开局没有任何预置的遭遇基线
    pub const PLAIN: Encounter = Encounter {
        id: "",
        kind: EnemyKind::Normal,
        enemies: &[],
        lineup: None,
        presets: &[],
        player_statuses: &[],
        weight: 1,
    };
}

/// 事件直接开战用的遭遇.不进地图的遭遇池,等级按普通算(奖励由事件自己给).
/// 面具土匪用 Pointy/Romeo/Bear 三名,神秘球体用两只圆球步行者.
pub static EVENT_ENCOUNTERS: &[Encounter] = &[
    Encounter {
        id: "event_three_fungi",
        kind: EnemyKind::Normal,
        enemies: &["fungi_beast", "fungi_beast", "fungi_beast"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_colosseum_slavers",
        kind: EnemyKind::Normal,
        enemies: &["blue_slaver", "taskmaster", "red_slaver"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_colosseum_nobs",
        kind: EnemyKind::Normal,
        enemies: &["taskmaster", "gremlin_nob"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_bandits",
        kind: EnemyKind::Normal,
        enemies: &["pointy", "romeo", "bear"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_two_orbs",
        kind: EnemyKind::Normal,
        enemies: &["orb_walker", "orb_walker"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_phantom_guardian",
        kind: EnemyKind::Normal,
        enemies: &["the_guardian"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_phantom_hexaghost",
        kind: EnemyKind::Normal,
        enemies: &["hexaghost"],
        ..Encounter::PLAIN
    },
    Encounter {
        id: "event_phantom_slime_boss",
        kind: EnemyKind::Normal,
        enemies: &["slime_boss"],
        ..Encounter::PLAIN
    },
];

/// 按 id 找事件遭遇
pub fn event_encounter(id: &str) -> Option<&'static Encounter> {
    EVENT_ENCOUNTERS.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_encounters_use_real_enemies() {
        for enc in EVENT_ENCOUNTERS {
            assert!(!enc.enemies.is_empty(), "{} 没有敌人", enc.id);
            for id in enc.enemies {
                assert!(
                    crate::core::enemies::enemy_def(id).is_some(),
                    "{} 引用了不存在的敌人 {}",
                    enc.id,
                    id
                );
            }
        }
        assert!(event_encounter("event_bandits").is_some());
        assert!(event_encounter("no_such_encounter").is_none());
    }

    #[test]
    fn intent_categories_match_their_damage() {
        // 意图与效果要自洽:标成攻击的必须有伤害,有伤害的必须标成攻击
        for def in crate::core::enemies::ENEMIES {
            for m in def.moves {
                let hits_back = m.effects.iter().any(|fx| {
                    matches!(
                        fx,
                        EnemyFx::Attack { .. }
                            | EnemyFx::AttackScaling { .. }
                            | EnemyFx::AttackGrowing { .. }
                            | EnemyFx::AttackStabCount { .. }
                            | EnemyFx::AttackRolled { .. }
                    )
                });
                assert_eq!(
                    m.intent.attacks(),
                    hits_back,
                    "{} 的招式 {} 意图与效果对不上",
                    def.id,
                    m.name
                );
            }
        }
    }

    #[test]
    fn intent_categories_know_whether_they_hit() {
        assert!(Intent::Attack { damage: 6, times: 1 }.attacks());
        assert!(Intent::AttackDebuff { damage: 7, times: 1 }.attacks());
        assert!(!Intent::Defend.attacks());
        assert!(!Intent::Buff.attacks());
    }
}
