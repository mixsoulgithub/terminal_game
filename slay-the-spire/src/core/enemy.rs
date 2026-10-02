// 敌人的静态定义:招式表、出招 AI、意图.
// 具体敌人与遭遇的数据在 enemies.rs.
use crate::core::status::Status;

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

/// 敌人招式里的一段效果
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EnemyFx {
    /// 攻击玩家 amount 伤害 times 次
    Attack { amount: i32, times: u8 },
    Block { amount: i32 },
    /// 自身获得状态
    GainStatus { status: Status, n: i32 },
    /// 给玩家上状态
    PlayerStatus { status: Status, n: i32 },
}

#[derive(Clone, Copy, Debug)]
pub struct MoveDef {
    pub name: &'static str,
    pub effects: &'static [EnemyFx],
}

/// 出招规则
#[derive(Clone, Copy, Debug)]
pub enum Ai {
    /// 按招式表顺序循环
    Cycle,
    /// 按权重随机,no_repeat 时避免连续两回合同一招
    Random { weights: &'static [u32], no_repeat: bool },
    /// 前 turns 回合固定用 moves[0](睡眠),之后从 moves[wake] 开始循环
    Sleep { turns: u8, wake: usize },
}

#[derive(Debug)]
pub struct EnemyDef {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: EnemyKind,
    /// 生命区间,闭区间随机
    pub hp: (i32, i32),
    pub moves: &'static [MoveDef],
    pub ai: Ai,
    /// 开局自带的状态
    pub innate: &'static [(Status, i32)],
    /// 死亡时对玩家触发(孢子云等)
    pub on_death: &'static [EnemyFx],
}

/// 意图:给玩家看的预告,伤害是未计入增减益的原始值
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Intent {
    Attack { damage: i32, times: u8 },
    AttackDefend { damage: i32, times: u8, block: i32 },
    Defend,
    Buff,
    Debuff,
    AttackDebuff { damage: i32, times: u8 },
    Sleep,
    Unknown,
}

impl MoveDef {
    /// 把招式效果归纳成一个意图
    pub fn intent(&self) -> Intent {
        let mut damage = 0;
        let mut times = 0u8;
        let mut block = 0;
        let mut buff = false;
        let mut debuff = false;
        for fx in self.effects {
            match fx {
                EnemyFx::Attack { amount, times: t } => {
                    damage += amount;
                    times = times.saturating_add(*t);
                }
                EnemyFx::Block { amount } => block += amount,
                EnemyFx::GainStatus { .. } => buff = true,
                EnemyFx::PlayerStatus { .. } => debuff = true,
            }
        }
        if damage == 0 && block == 0 && buff && !debuff {
            return Intent::Buff;
        }
        if damage == 0 && block == 0 && debuff {
            return Intent::Debuff;
        }
        if damage == 0 && block > 0 {
            return Intent::Defend;
        }
        if damage > 0 && block > 0 {
            return Intent::AttackDefend {
                damage,
                times,
                block,
            };
        }
        if damage > 0 && debuff {
            return Intent::AttackDebuff { damage, times };
        }
        if damage > 0 {
            return Intent::Attack { damage, times };
        }
        Intent::Unknown
    }

}

/// 一场遭遇战:同一组的敌人 id
#[derive(Debug)]
pub struct Encounter {
    pub id: &'static str,
    pub kind: EnemyKind,
    pub enemies: &'static [&'static str],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_recognizes_pure_attack() {
        let m = MoveDef {
            name: "Bite",
            effects: &[EnemyFx::Attack {
                amount: 6,
                times: 1,
            }],
        };
        assert_eq!(m.intent(), Intent::Attack { damage: 6, times: 1 });
    }

    #[test]
    fn intent_recognizes_mixed_attack() {
        let m = MoveDef {
            name: "Spit",
            effects: &[
                EnemyFx::Attack {
                    amount: 7,
                    times: 1,
                },
                EnemyFx::PlayerStatus {
                    status: Status::Weak,
                    n: 1,
                },
            ],
        };
        assert_eq!(m.intent(), Intent::AttackDebuff { damage: 7, times: 1 });
    }

    #[test]
    fn intent_recognizes_block_and_buff() {
        let block = MoveDef {
            name: "Curl",
            effects: &[EnemyFx::Block { amount: 5 }],
        };
        assert_eq!(block.intent(), Intent::Defend);
        let buff = MoveDef {
            name: "Incantation",
            effects: &[EnemyFx::GainStatus {
                status: Status::Ritual,
                n: 3,
            }],
        };
        assert_eq!(buff.intent(), Intent::Buff);
        let mixed = MoveDef {
            name: "Guard",
            effects: &[
                EnemyFx::Attack {
                    amount: 5,
                    times: 1,
                },
                EnemyFx::Block { amount: 5 },
            ],
        };
        assert_eq!(
            mixed.intent(),
            Intent::AttackDefend {
                damage: 5,
                times: 1,
                block: 5
            }
        );
    }
}
