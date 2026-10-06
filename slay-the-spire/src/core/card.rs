// 卡牌的静态定义(CardDef)、卡牌实例(CardInstance)与卡牌效果(Effect).
// 卡池数据在 cards.rs;这里只放类型与实例上的解析逻辑.
use crate::core::status::Status;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardType {
    Attack,
    Skill,
    Power,
    Status,
    Curse,
}

impl CardType {
    pub fn name(self) -> &'static str {
        match self {
            CardType::Attack => "Attack",
            CardType::Skill => "Skill",
            CardType::Power => "Power",
            CardType::Status => "Status",
            CardType::Curse => "Curse",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Rarity {
    Basic,
    Common,
    Uncommon,
    Rare,
    Special,
}

impl Rarity {
    pub fn name(self) -> &'static str {
        match self {
            Rarity::Basic => "Basic",
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Special => "Special",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cost {
    Fixed(u8),
    /// 消耗当前全部能量,效果按消耗量结算
    X,
    /// 不能主动打出
    Unplayable,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    None,
    Enemy,
    All,
    Random,
}

impl Target {
    /// 需要玩家先选一个敌人才能打出
    pub fn needs_enemy(self) -> bool {
        matches!(self, Target::Enemy)
    }
}

/// 卡牌效果.战斗引擎按顺序逐条结算,所以顺序有意义
/// (例如先 ExhaustHand 再 BlockPerExhausted 才算得对).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Effect {
    /// 对目标敌人造成 amount 伤害 times 次,每次单独结算力量与易伤
    Damage { amount: i32, times: u8 },
    /// 对所有敌人
    DamageAll { amount: i32, times: u8 },
    /// 对随机敌人
    DamageRandom { amount: i32, times: u8 },
    /// 伤害等于自身当前格挡
    DamageEqualBlock,
    /// 额外加上卡牌实例的 bonus(打出后累积)
    DamageWithBonus { amount: i32, times: u8 },
    /// 基础伤害 + 每张名字含 Strike 的牌 per 点
    DamagePerStrike { base: i32, per: i32 },
    /// 伤害等于本场已消耗牌数 * per
    DamagePerExhausted { per: i32 },
    /// 消耗全部能量,每点能量对所有敌人造成 per 伤害
    DamageAllX { per: i32 },
    /// 造成伤害;若目标处于易伤则回复能量并抽牌
    DamageIfVulnerable { amount: i32, energy: i32, draw: u8 },
    /// 造成伤害;若因此击杀则最大生命 +max_hp
    DamageAndKillMaxHp { amount: i32, times: u8, max_hp: i32 },
    /// 伤害 = amount + 力量 * mult(重刃类)
    DamageStrengthMult { amount: i32, mult: i32 },
    /// 对所有敌人造成 amount 伤害,并按实际造成的伤害回血
    Reaper { amount: i32 },
    /// 本卡在本场战斗内的 bonus += n(打出后)
    BonusSelf { n: i32 },
    Block { amount: i32 },
    /// 每消耗一张牌获得 per 格挡(配合 ExhaustHand)
    BlockPerExhausted { per: i32 },
    /// 当前格挡翻倍
    DoubleBlock,
    /// 直接掉血,不吃格挡也不吃力量
    LoseHp { amount: i32 },
    GainEnergy { n: i32 },
    Draw { n: u8 },
    AddSelfStatus { status: Status, n: i32 },
    AddTargetStatus { status: Status, n: i32 },
    AddAllEnemiesStatus { status: Status, n: i32 },
    /// 自身某状态层数翻倍
    DoubleSelfStatus(Status),
    /// 消耗整手牌(不含本卡自身,本卡按 exhaust 字段处理)
    ExhaustHand,
    ExhaustRandomInHand { n: u8 },
    /// 造成伤害并消耗手牌里所有非攻击牌
    ExhaustNonAttacks { damage: i32 },
    ExhaustSelf,
    /// 往抽牌堆塞一张牌(状态牌常用)
    AddCardToDraw { id: &'static str, n: u8 },
    /// 往手牌塞一张牌(如伤口)
    AddCardToHand { id: &'static str, n: u8 },
    /// 往弃牌堆塞一张牌(愤怒)
    AddCardToDiscard { id: &'static str, n: u8 },
    /// 随机往手牌加一张攻击牌,本回合 0 费(炼狱之刃)
    AddRandomAttackToHand,
    /// 打出抽牌堆顶那张并消耗(浩劫)
    PlayTopOfDraw,
    /// 从手牌选一张消耗(燃烧契约)
    ExhaustFromHand,
    /// 从手牌选一张放回抽牌堆顶(战吼)
    TopFromHand,
    /// 从手牌选一张攻击/能力牌,复制一份(二重身)
    CopyFromHand,
    /// 从消耗堆选一张回手牌(掘出)
    FromExhaustToHand,
    /// 从弃牌堆选一张放到抽牌堆顶(头槌)
    FromDiscardToDrawTop,
    /// 这张牌被消耗时获得能量(哨卫)
    EnergyOnExhaust { n: i32 },
    /// 目标这回合打算攻击的话,给自己加力量(观察弱点)
    StrengthIfTargetAttacks { n: i32 },
    UpgradeRandomInHand { n: u8 },
}

/// 升级后的覆盖项:None 表示沿用基础值
#[derive(Clone, Copy, Debug)]
pub struct CardUpgrade {
    pub cost: Option<Cost>,
    pub text: &'static str,
    pub effects: Option<&'static [Effect]>,
    pub exhaust: Option<bool>,
    pub retain: Option<bool>,
    pub ethereal: Option<bool>,
    pub innate: Option<bool>,
}

#[derive(Debug)]
pub struct CardDef {
    pub id: &'static str,
    pub name: &'static str,
    pub cost: Cost,
    pub kind: CardType,
    pub rarity: Rarity,
    pub target: Target,
    /// 基础描述,{d} 占位符会替换成计入 bonus 后的伤害
    pub text: &'static str,
    pub exhaust: bool,
    pub ethereal: bool,
    pub innate: bool,
    pub retain: bool,
    /// 可以无限升级(灼热攻击)
    pub multi_upgrade: bool,
    pub effects: &'static [Effect],
    pub upgrade: Option<CardUpgrade>,
}

impl CardDef {
    /// 不可升级的牌(状态/诅咒/特殊)
    pub fn upgradable(&self) -> bool {
        self.upgrade.is_some()
    }
}

/// 卡牌实例:定义 + 是否已升级 + 本场战斗内累积的加成
#[derive(Clone, Debug)]
pub struct CardInstance {
    pub def: &'static CardDef,
    pub upgraded: bool,
    pub bonus: i32,
    /// 费用增减(负=更便宜),嗜血这类按失血次数往下减
    pub cost_delta: i32,
    /// 本回合费用为 0(炼狱之刃给的牌)
    pub free_this_turn: bool,
    /// 升级次数:可以多次升级的牌(灼热攻击)才 >1
    pub plus: u8,
}

impl CardInstance {
    pub fn new(def: &'static CardDef) -> Self {
        CardInstance {
            def,
            upgraded: false,
            bonus: 0,
            cost_delta: 0,
            free_this_turn: false,
            plus: 0,
        }
    }

    /// 展示用名字,升级后带加号
    pub fn label(&self) -> String {
        if self.plus > 1 {
            format!("{}+{}", self.def.name, self.plus)
        } else if self.upgraded {
            format!("{}+", self.def.name)
        } else {
            self.def.name.to_string()
        }
    }

    pub fn kind(&self) -> CardType {
        self.def.kind
    }

    pub fn rarity(&self) -> Rarity {
        self.def.rarity
    }

    pub fn target(&self) -> Target {
        self.def.target
    }

    /// 是否属于"打击"系列(Perfected Strike 用)
    pub fn is_strike(&self) -> bool {
        self.def.name.contains("Strike")
    }

    pub fn cost(&self) -> Cost {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.cost.unwrap_or(self.def.cost),
            _ => self.def.cost,
        }
    }

    /// 能量不足时能否打出;X 费用始终可打
    pub fn cost_value(&self, energy: i32) -> i32 {
        if self.free_this_turn {
            return 0;
        }
        match self.cost() {
            Cost::Fixed(n) => (n as i32 + self.cost_delta).max(0),
            Cost::X => energy.max(0),
            Cost::Unplayable => i32::MAX,
        }
    }

    pub fn playable(&self) -> bool {
        !matches!(self.cost(), Cost::Unplayable)
    }

    pub fn effects(&self) -> &'static [Effect] {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.effects.unwrap_or(self.def.effects),
            _ => self.def.effects,
        }
    }

    pub fn text(&self) -> &'static str {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.text,
            _ => self.def.text,
        }
    }

    pub fn is_exhaust(&self) -> bool {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.exhaust.unwrap_or(self.def.exhaust),
            _ => self.def.exhaust,
        }
    }

    pub fn is_ethereal(&self) -> bool {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.ethereal.unwrap_or(self.def.ethereal),
            _ => self.def.ethereal,
        }
    }

    pub fn is_innate(&self) -> bool {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.innate.unwrap_or(self.def.innate),
            _ => self.def.innate,
        }
    }

    pub fn is_retain(&self) -> bool {
        match (&self.def.upgrade, self.upgraded) {
            (Some(up), true) => up.retain.unwrap_or(self.def.retain),
            _ => self.def.retain,
        }
    }

    /// 能无限升级的牌(灼热攻击)
    pub fn multi_upgrade(&self) -> bool {
        self.def.multi_upgrade
    }

    pub fn can_upgrade(&self) -> bool {
        if self.multi_upgrade() {
            return true;
        }
        !self.upgraded && self.def.upgradable()
    }

    /// 成功升级返回 true
    pub fn upgrade(&mut self) -> bool {
        if !self.can_upgrade() {
            return false;
        }
        self.upgraded = true;
        if self.multi_upgrade() {
            // 每升一级 += 当前等级 + 3(1 级 +4、2 级 +5……),和原作一致
            self.plus += 1;
            self.bonus += self.plus as i32 + 3;
        }
        true
    }

    /// 生效的伤害值(bonus 计入)用于展示
    pub fn bonus_damage(&self) -> i32 {
        let mut total = 0;
        for e in self.effects() {
            match e {
                Effect::DamageWithBonus { amount, .. } => total += amount + self.bonus,
                _ => {}
            }
        }
        total
    }

    /// 把 {d} 替换成本场实际伤害
    pub fn display_text(&self) -> String {
        let d = self.bonus_damage();
        if self.text().contains("{d}") {
            self.text().replace("{d}", &d.to_string())
        } else {
            self.text().to_string()
        }
    }

    /// 供 UI 判断这张牌是否只能打向敌人
    pub fn needs_target(&self) -> bool {
        self.target().needs_enemy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cards::card_def;

    #[test]
    fn upgrade_switches_cost_and_effects() {
        let def = card_def("strike").unwrap();
        let mut c = CardInstance::new(def);
        assert_eq!(c.cost(), Cost::Fixed(1));
        assert_eq!(c.effects(), &[Effect::Damage { amount: 6, times: 1 }]);
        assert!(c.upgrade());
        assert!(!c.upgrade(), "不能升级两次");
        assert_eq!(c.effects(), &[Effect::Damage { amount: 9, times: 1 }]);
        assert_eq!(c.label(), "Strike+");
    }

    #[test]
    fn unplayable_cards_stay_unplayable() {
        let def = card_def("wound").unwrap();
        let c = CardInstance::new(def);
        assert!(!c.playable());
        assert!(!c.can_upgrade());
        assert_eq!(c.cost_value(3), i32::MAX);
    }

    #[test]
    fn x_cost_uses_current_energy() {
        let def = card_def("whirlwind").unwrap();
        let c = CardInstance::new(def);
        assert_eq!(c.cost(), Cost::X);
        assert_eq!(c.cost_value(2), 2);
        assert_eq!(c.cost_value(-1), 0);
    }

    #[test]
    fn bonus_only_shows_up_in_scaling_cards() {
        let def = card_def("rampage").unwrap();
        let mut c = CardInstance::new(def);
        assert_eq!(c.bonus_damage(), 8);
        c.bonus += 5;
        assert_eq!(c.bonus_damage(), 13);
        assert!(c.display_text().contains("13 damage"));
        let strike = CardInstance::new(card_def("strike").unwrap());
        assert_eq!(strike.bonus_damage(), 0);
    }

    #[test]
    fn strike_detection_covers_perfected_strike() {
        let def = card_def("perfected_strike").unwrap();
        let c = CardInstance::new(def);
        assert!(c.is_strike());
        assert!(CardInstance::new(card_def("strike").unwrap()).is_strike());
        assert!(!CardInstance::new(card_def("defend").unwrap()).is_strike());
    }
}
