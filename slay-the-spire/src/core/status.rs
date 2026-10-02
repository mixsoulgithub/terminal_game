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
    // 敌人能力
    Ritual,
    Enrage,
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
            Ritual => "Ritual",
            Enrage => "Enrage",
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
            Ritual => "RITUAL",
            Enrage => "ENRAGE",
        }
    }

    /// 回合结束时层数减一
    pub fn is_debuff(self) -> bool {
        use Status::*;
        matches!(self, Vulnerable | Weak | Frail | Entangled)
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

    pub fn has(&self, s: Status) -> bool {
        self.get(s) != 0
    }

    /// 叠加 n,n 可以为负;层数降到 0 以下就移除
    pub fn add(&mut self, s: Status, n: i32) {
        if n == 0 {
            return;
        }
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            slot.1 += n;
            if slot.1 <= 0 {
                self.list.retain(|(k, _)| *k != s);
            }
            return;
        }
        if n > 0 {
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

    /// 每个减益层数减一
    pub fn decay_debuffs(&mut self) {
        for slot in self.list.iter_mut() {
            if slot.0.is_debuff() {
                slot.1 -= 1;
            }
        }
        self.list.retain(|(_, n)| *n > 0);
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
        assert_eq!(s.iter().count(), 0);
    }

    #[test]
    fn negative_add_on_missing_is_noop() {
        let mut s = Statuses::new();
        s.add(Status::Weak, -4);
        assert_eq!(s.iter().count(), 0);
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
