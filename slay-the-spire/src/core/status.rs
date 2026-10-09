// 状态(增益/减益/能力).玩家与敌人共用同一套结构,右侧数值表示层数或强度.
// 减益(is_debuff)在持有者回合结束时减一;能力(is_power)永久保留.
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Status {
    // 通用增减益
    Strength,
    Dexterity,
    Vulnerable,
    Weak,
    Frail,
    Entangled,
    // 玩家能力
    DemonForm,
    Metallicize,
    FeelNoPain,
    DarkEmbrace,
    Evolve,
    FireBreathing,
    Barricade,
    Brutality,
    Rupture,
    Regenerate,
    Thorns,
    Berserk,
    Combust,
    Corruption,
    DoubleTap,
    Juggernaut,
    Rage,
    FlameBarrier,
    Artifact,
    /// 本回合起一段时间内不能从卡牌获得格挡(紧急按钮)
    NoBlock,
    /// 本回合不能再抽牌(战斗恍惚)
    NoDraw,
    Mayhem,
    Magnetism,
    Panache,
    SadisticNature,
    // 玩家与敌人共用、来自敌人的状态
    /// 回合结束获得等量格挡,受到未被格挡的伤害时减一
    PlatedArmor,
    /// 玩家回合结束时受到等量伤害(缠绕)
    Constricted,
    /// 抽到的牌费用随机化(蛇眼)
    Confused,
    /// 每打出一张非攻击牌就往抽牌堆洗入等量眩晕(被选中者)
    Hex,
    /// 下回合少抽等量张牌
    DrawReduction,
    /// 被两面夹击:从背后受到攻击时多吃 50% 伤害
    Surrounded,
    /// 受到的所有伤害降为 1(复仇女神的循环)
    Intangible,
    // 敌人能力
    Ritual,
    Enrage,
    /// 受到攻击伤害时获得等量力量(狂怒小鬼)
    Anger,
    /// 玩家每打出一张攻击牌就受到等量伤害(守护者的防御姿态)
    SharpHide,
    /// 玩家每打出一张牌就受到等量伤害(心脏)
    BeatOfDeath,
    /// 玩家每打出一张能力牌就获得等量力量(觉醒者)
    Curiosity,
    /// 第一次受到攻击伤害时获得等量格挡,一次性(虱子)
    CurlUp,
    /// 倒计时,回合数到就自爆
    Explosive,
    /// 倒计时,回合数到就消失(瞬变体)
    Fading,
    /// 受到的攻击伤害减半,每次被命中掉一层,掉光则落地眩晕(鸟)
    Flight,
    /// 本回合最多再掉等量生命(心脏)
    Invincible,
    /// 受到攻击伤害时获得等量格挡,每命中一次层数加一,自己回合结束重置(扭动巨物/蛇草)
    Malleable,
    /// 首领死亡时一起退场(召唤物)
    Minion,
    /// 死亡时带走所有召唤物
    MinionLeader,
    /// 掉够等量生命就换防御姿态(守护者)
    ModeShift,
    /// 造成未被格挡的攻击伤害时往玩家弃牌堆塞等量伤口(痛苦刺击)
    PainfulStabs,
    /// 受到攻击伤害就重新选招(扭动巨物)
    Reactive,
    /// 死亡后若还有同伴就半血复活一次(暗灵)
    Regrow,
    /// 掉血时临时扣除等量力量,自己回合结束回补(瞬变体)
    Shifting,
    /// 玩家每打出一张牌就多吃 10% 攻击伤害,自己回合结束重置(巨大头颅)
    Slow,
    /// 死亡时给玩家上等量易伤(孢子云)
    SporeCloud,
    /// 血量掉到一半就分裂成两只小史莱姆
    Split,
    /// 身上压着一张被偷走的牌,死亡时归还
    Stasis,
    /// 每回合结束时获得等量力量(圆球步行者)
    StrengthUp,
    /// 玩家每打出第 12 张牌就结束其回合并获得等量力量(时间吞噬者)
    TimeWarp,
    /// 睡着(拉格文):不行动,挨打会醒
    Asleep,
    // 药水需要的状态
    /// 接下来的 n 张牌打两次(复制药水)
    Duplication,
    /// 自己回合结束时扣掉等量力量(力量药水)
    LoseStrength,
    /// 自己回合结束时扣掉等量敏捷(敏捷药水)
    LoseDexterity,
    /// 集中:本作没有充能球,挂上也不影响任何计算(集中药水)
    Focus,
    /// 自己回合开始时掉等量生命再减一层(毒药水)
    Poison,
}

impl Status {
    pub fn name(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "Strength",
            Dexterity => "Dexterity",
            Vulnerable => "Vulnerable",
            Weak => "Weak",
            Frail => "Frail",
            Entangled => "Entangled",
            DemonForm => "Demon Form",
            Metallicize => "Metallicize",
            FeelNoPain => "Feel No Pain",
            DarkEmbrace => "Dark Embrace",
            Evolve => "Evolve",
            FireBreathing => "Fire Breathing",
            Barricade => "Barricade",
            Brutality => "Brutality",
            Rupture => "Rupture",
            Regenerate => "Regenerate",
            Thorns => "Thorns",
            Berserk => "Berserk",
            Combust => "Combust",
            Corruption => "Corruption",
            DoubleTap => "Double Tap",
            Juggernaut => "Juggernaut",
            Rage => "Rage",
            FlameBarrier => "Flame Barrier",
            Artifact => "Artifact",
            NoBlock => "No Block",
            NoDraw => "No Draw",
            Mayhem => "Mayhem",
            Magnetism => "Magnetism",
            Panache => "Panache",
            SadisticNature => "Sadistic Nature",
            PlatedArmor => "Plated Armor",
            Constricted => "Constricted",
            Confused => "Confused",
            Hex => "Hex",
            DrawReduction => "Draw Reduction",
            Surrounded => "Surrounded",
            Intangible => "Intangible",
            Ritual => "Ritual",
            Enrage => "Enrage",
            Anger => "Anger",
            SharpHide => "Sharp Hide",
            BeatOfDeath => "Beat of Death",
            Curiosity => "Curiosity",
            CurlUp => "Curl Up",
            Explosive => "Explosive",
            Fading => "Fading",
            Flight => "Flight",
            Invincible => "Invincible",
            Malleable => "Malleable",
            Minion => "Minion",
            MinionLeader => "Minion Leader",
            ModeShift => "Mode Shift",
            PainfulStabs => "Painful Stabs",
            Reactive => "Reactive",
            Regrow => "Regrow",
            Shifting => "Shifting",
            Slow => "Slow",
            SporeCloud => "Spore Cloud",
            Split => "Split",
            Stasis => "Stasis",
            StrengthUp => "Strength Up",
            TimeWarp => "Time Warp",
            Asleep => "Asleep",
            Duplication => "Duplication",
            LoseStrength => "Lose Strength",
            LoseDexterity => "Lose Dexterity",
            Focus => "Focus",
            Poison => "Poison",
        }
    }

    /// 状态行里用的短标签,控制在 4 个字符内
    pub fn short(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "STR",
            Dexterity => "DEX",
            Vulnerable => "VULN",
            Weak => "WEAK",
            Frail => "FRAIL",
            Entangled => "ENTG",
            DemonForm => "DEMON",
            Metallicize => "METAL",
            FeelNoPain => "FNP",
            DarkEmbrace => "DARK",
            Evolve => "EVOLV",
            FireBreathing => "FIRE",
            Barricade => "BARR",
            Brutality => "BRUT",
            Rupture => "RUPT",
            Regenerate => "REGEN",
            Thorns => "THORN",
            Berserk => "BSRK",
            Combust => "CMBS",
            Corruption => "CORR",
            DoubleTap => "DBLT",
            Juggernaut => "JUGG",
            Rage => "RAGE",
            FlameBarrier => "FLMB",
            Artifact => "ARTF",
            NoBlock => "NBLK",
            NoDraw => "NDRW",
            Mayhem => "MAYH",
            Magnetism => "MAGN",
            Panache => "PAN",
            SadisticNature => "SAD",
            PlatedArmor => "PLATE",
            Constricted => "CSTR",
            Confused => "CNFS",
            Hex => "HEX",
            DrawReduction => "DRED",
            Surrounded => "SURR",
            Intangible => "INTG",
            Ritual => "RITUAL",
            Enrage => "ENRAGE",
            Anger => "ANGR",
            SharpHide => "HIDE",
            BeatOfDeath => "BEAT",
            Curiosity => "CURI",
            CurlUp => "CURL",
            Explosive => "XPLS",
            Fading => "FADE",
            Flight => "FLGT",
            Invincible => "INVC",
            Malleable => "MALL",
            Minion => "MINN",
            MinionLeader => "LEAD",
            ModeShift => "MODE",
            PainfulStabs => "PSTB",
            Reactive => "REAC",
            Regrow => "RGRW",
            Shifting => "SHFT",
            Slow => "SLOW",
            SporeCloud => "SPOR",
            Split => "SPLT",
            Stasis => "STAS",
            StrengthUp => "STR+",
            TimeWarp => "TIME",
            Asleep => "SLP",
            Duplication => "DUPL",
            LoseStrength => "LSTR",
            LoseDexterity => "LDEX",
            Focus => "FOC",
            Poison => "PSN",
        }
    }

    /// 负面状态(渲染成红色、会被"清除减益"清掉)
    pub fn is_debuff(self) -> bool {
        use Status::*;
        matches!(
            self,
            Vulnerable
                | Weak
                | Frail
                | Entangled
                | Confused
                | Hex
                | DrawReduction
                | Surrounded
                | Constricted
                | Slow
                | Poison
                | NoDraw
        )
    }

    /// 回合结束时层数减一(持续整场战斗的减益不算)
    pub fn decays(self) -> bool {
        use Status::*;
        if matches!(self, Confused | Hex | Surrounded | Constricted | Slow | Poison) {
            return false;
        }
        self.is_debuff() || matches!(self, DoubleTap | NoBlock | Duplication)
    }

    /// 层数可以降到 0 以下(参考实现里 canGoNegative 的那几条):
    /// 力量/敏捷/集中的减益是把强度减成负数,而不是"减到 0 就消失"
    /// (拉格文的汲魂在 0 力量时会把你压成 -1 力量).
    pub fn can_go_negative(self) -> bool {
        use Status::*;
        matches!(self, Strength | Dexterity | Focus)
    }

}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 一组状态.用 Vec 保存,保证渲染顺序稳定(便于 diff 与测试)
#[derive(Clone, Default, Debug)]
pub struct Statuses {
    list: Vec<(Status, i32)>,
}

impl Statuses {
    pub fn new() -> Self {
        Statuses { list: Vec::new() }
    }

    pub fn get(&self, s: Status) -> i32 {
        self.list
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    /// 按挂载顺序遍历(原版把加成当 atDamageGive 钩子,按 powers 挂的顺序折叠,
    /// 所以同一条状态先挂后挂会影响结果的第 1 点小数)
    pub fn entries(&self) -> impl Iterator<Item = &(Status, i32)> {
        self.list.iter()
    }

    pub fn has(&self, s: Status) -> bool {
        self.get(s) != 0
    }

    /// 状态栏里挂没挂这一条(层数 0 也算挂着,慢速开局显示的就是 Slow 0)
    pub fn holds(&self, s: Status) -> bool {
        self.list.iter().any(|(k, _)| *k == s)
    }

    /// 挂上一条层数为 0 的状态(用于开局就显示的倒计时/慢速)
    pub fn mark(&mut self, s: Status) {
        if !self.holds(s) {
            self.list.push((s, 0));
        }
    }

    /// 直接把层数设成 n(每回合重置延展/慢速/飞行用);
    /// 0 且不会自然递减的状态留一条空壳,界面上仍然显示(Slow 0)
    pub fn set(&mut self, s: Status, n: i32) {
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            slot.1 = n;
        } else if n > 0 || !s.decays() {
            self.list.push((s, n));
        }
        self.list.retain(|(k, n)| *k != s || *n != 0 || !k.decays());
    }

    /// 叠加 n,n 可以为负;多数状态层数降到 0 就移除,
    /// 力量/敏捷/集中这类"强度"状态允许压到负数(见 can_go_negative).
    /// 它们回到 0 也留着那一条:原版的 ApplyPowerAction 只在 !canGoNegative
    /// 时把 0 层的强度摘掉,留着才能保住它在 powers 列表里的位置(伤害折叠
    /// 按挂载顺序走,重挂会让顺序变、同一场伤害算出来差 1)
    pub fn add(&mut self, s: Status, n: i32) {
        if n == 0 {
            return;
        }
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            slot.1 += n;
            if slot.1 <= 0 && !s.can_go_negative() {
                self.list.retain(|(k, _)| *k != s);
            }
            return;
        }
        if n > 0 || s.can_go_negative() {
            self.list.push((s, n));
        }
    }

    /// 层数翻倍(强化卡与部分药水用),负数不翻
    pub fn double(&mut self, s: Status) {
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            if slot.1 > 0 {
                slot.1 *= 2;
            }
        }
    }

    /// 每个到期的状态层数减一
    pub fn decay_debuffs(&mut self) {
        self.decay_debuffs_except(&[]);
    }

    /// 同 decay_debuffs,但 skip 里的状态这一次不减.
    /// 敌人刚给玩家挂上的持续状态(参考实现里的 justApplied)要跳过第一次递减,
    /// 否则持续时间会短一整回合
    pub fn decay_debuffs_except(&mut self, skip: &[Status]) {
        for slot in self.list.iter_mut() {
            if slot.0.decays() && !skip.contains(&slot.0) {
                slot.1 -= 1;
            }
        }
        // 不递减的状态留着(包括层数为 0 的标记,比如 Slow 0)
        self.list.retain(|(k, n)| *n > 0 || !k.decays());
    }

    pub fn clear_debuffs(&mut self) {
        self.list.retain(|(k, _)| !k.is_debuff());
    }

    pub fn iter(&self) -> impl Iterator<Item = (Status, i32)> + '_ {
        self.list.iter().copied()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_get() {
        let mut s = Statuses::new();
        assert_eq!(s.get(Status::Strength), 0);
        s.add(Status::Strength, 2);
        s.add(Status::Strength, 1);
        assert_eq!(s.get(Status::Strength), 3);
        s.add(Status::Strength, -3);
        assert!(!s.has(Status::Strength));
        // 强度类回到 0 仍留着那一条(保住它在 powers 列表里的位置)
        assert_eq!(s.iter().count(), 1);
    }

    #[test]
    fn negative_add_on_missing_is_noop() {
        let mut s = Statuses::new();
        s.add(Status::Weak, -4);
        assert_eq!(s.iter().count(), 0);
    }

    /// 力量/敏捷可以压到 0 以下(拉格文的汲魂在 0 力量时把你压成 -1);
    /// 回到 0 也留着那一条(原版 canGoNegative 的强度不在 0 层被摘掉,
    /// 位置保住后伤害折叠顺序才对)
    #[test]
    fn strength_can_go_negative() {
        let mut s = Statuses::new();
        s.add(Status::Strength, -1);
        assert_eq!(s.get(Status::Strength), -1);
        s.add(Status::Dexterity, -2);
        assert_eq!(s.get(Status::Dexterity), -2);
        // 负的加上正的回到 0:层数是 0,但条目还在
        s.add(Status::Strength, 1);
        assert!(!s.has(Status::Strength));
        assert!(s.holds(Status::Strength));
        // 减益类的状态仍旧不允许为负
        s.add(Status::Vulnerable, 2);
        s.add(Status::Vulnerable, -5);
        assert!(!s.holds(Status::Vulnerable));
    }

    #[test]
    fn double_only_positive() {
        let mut s = Statuses::new();
        s.add(Status::Strength, 3);
        s.double(Status::Strength);
        assert_eq!(s.get(Status::Strength), 6);
        s.double(Status::DemonForm);
        assert_eq!(s.get(Status::DemonForm), 0);
    }

    #[test]
    fn decay_only_hits_debuffs() {
        let mut s = Statuses::new();
        s.add(Status::Vulnerable, 2);
        s.add(Status::Weak, 1);
        s.add(Status::Strength, 4);
        s.add(Status::DemonForm, 1);
        s.decay_debuffs();
        assert_eq!(s.get(Status::Vulnerable), 1);
        assert!(!s.has(Status::Weak));
        assert_eq!(s.get(Status::Strength), 4);
        assert_eq!(s.get(Status::DemonForm), 1);
    }

    #[test]
    fn clear_debuffs_keeps_powers() {
        let mut s = Statuses::new();
        s.add(Status::Vulnerable, 3);
        s.add(Status::Frail, 2);
        s.add(Status::Metallicize, 3);
        s.clear_debuffs();
        assert_eq!(s.get(Status::Metallicize), 3);
        assert_eq!(s.get(Status::Vulnerable), 0);
    }

    #[test]
    fn iter_order_is_insertion_order() {
        let mut s = Statuses::new();
        s.add(Status::Weak, 1);
        s.add(Status::Strength, 2);
        let v: Vec<_> = s.iter().collect();
        assert_eq!(v, vec![(Status::Weak, 1), (Status::Strength, 2)]);
    }
}
