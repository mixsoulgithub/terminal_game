// 药水:一次性消耗品,配方同样是数据.
// 池子顺序与语料一致(语料按 id 字母序),发放时按"共享池 + 职业池"筛出来,
// 与参考实现的 bundle 插入顺序对齐.
//
// 42 瓶效果对照反编译逐条核过(数值/目标/树皮翻倍/禁疗交互),结论与登记:
//   数值与目标:base/targeted 与语料 potions.json 全一致(42/42).神圣树皮只翻
//     `doubled()` 里列出的那几类,与语料 sacredBarkDoubles=false 的那几瓶(万灵药/
//     赌徒之酿/锻造祝福/烟雾弹/熵能酿剂/姿态类)一一对上.
//   禁疗:血药水/再生/玩具鸟都走 heal_player,花开彼岸(no_heal)把它们连同仙女的保命
//     一起归零;果汁走 increaseMaxHp(直接改血,和原版一样不过 heal,所以不被花开彼岸挡).
//   本轮修:仙女在瓶中原先是 out_of_combat=true + fx=Nothing,能在地图/战斗里被主动喝掉
//     —— 喝下去什么都不做,却会把它的死亡保护一起丢掉.反编译里它根本没有"喝"这条路径
//     (refs/sts_lightspeed/src/combat/BattleContext.cpp:2430 对 FAIRY_POTION 直接 assert;
//     refs/sts_lightspeed/src/game/GameContext.cpp:2184 与 refs/sts_lightspeed/src/sim/search/GameAction.cpp:263-270 在非战斗场景
//     只放行血药水/熵能酿剂/果汁),参考实现 potions/index.ts:316 也自注
//     "non-drinkable death-save" → 现改成 PotionFx::Passive,quaff_potion/use_potion 一律拒绝.
// 参考侧登记(本作不改,依据都是反编译里两处自相矛盾/缺口):
//   BLOOD_POTION:战斗内 refs/sts_lightspeed/src/combat/BattleContext.cpp:2268 的树皮三元写反了(hasBark ? 20 : 40),
//     战斗外 refs/sts_lightspeed/src/game/GameContext.cpp:2185 与语料 desc "Heal for [20%|40%]" 都是 20/40,
//     本作按 20/40.
//   ENTROPIC_BREW:战斗内 refs/sts_lightspeed/src/combat/BattleContext.cpp:2314 传 limited=true
//     (refs/sts_lightspeed/src/game/Game.cpp:309 returnRandomPotionOfRarity 的循环会把果汁重掷掉,永不给出),
//     战斗外 refs/sts_lightspeed/src/game/GameContext.cpp:2188 走默认 limited=false(允许果汁);本作两处统一,
//     与战斗外那条一致.原版这两处自己打架,先按现状登记,不猜.
//   COLORLESS_POTION 的牌池:反编译 CombatColorlessCardPool(refs/sts_lightspeed/include/constants/CardPools.h:189,34 张)不含
//     Bandage Up,而商店池 ColorlessRarityCardPool(refs/sts_lightspeed/include/constants/CardPools.h:133,35 张)含;本作与参考
//     实现一样,取 colorless 里 uncommon|rare 的全部.
//   ATTACK_POTION 的牌池:反编译 CombatTypeCardPool 的攻击表(refs/sts_lightspeed/include/constants/CardPools.h:150)只有 28 张
//     (漏了 FEED/REAPER),语料里红卡非基础攻击有 30 张;本作与参考实现都收全 30 张,
//     池子顺序按语料(bundle)序.
use crate::core::card::{Rarity, Target};
use crate::core::status::Status;
use crate::rng::Rng;

/// 发现类药水从哪个池子里亮出三张候选
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiscoveryPool {
    Attack,
    Power,
    Skill,
    Colorless,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PotionFx {
    /// 指定敌人:造成伤害
    Damage { amount: i32 },
    /// 全体敌人:造成伤害
    DamageAll { amount: i32 },
    /// 指定敌人:上虚弱
    Weak { n: i32 },
    /// 指定敌人:上易伤
    Vulnerable { n: i32 },
    /// 指定敌人:上中毒
    Poison { n: i32 },
    Block { amount: i32 },
    Energy { n: i32 },
    Draw { n: i32 },
    /// 抽牌后把手上卡牌的费用随机化(蛇油)
    DrawAndRandomizeCosts { n: i32 },
    /// 按最大生命的百分比回血(血药)
    HealPercent { pct: i32 },
    /// 生命上限 +n 并回复 n 点(果汁)
    MaxHp { n: i32 },
    /// 上 n 层状态
    Status { status: Status, n: i32 },
    /// 本回合上 n 层,自己回合结束时再扣掉 n 层(力量/敏捷药水)
    TempStatus { status: Status, lose: Status, n: i32 },
    /// 从三张随机牌里挑一张加进手牌,本回合 0 费
    Discovery { pool: DiscoveryPool, n: i32 },
    /// 消耗任意张手牌(万灵药)
    ExhaustHand,
    /// 弃掉任意张手牌,再抽等量张(赌徒之酿)
    DiscardHandThenDraw,
    /// 从弃牌堆拿回 n 张手牌,本回合 0 费(液态记忆)
    ReturnFromDiscard { n: i32 },
    /// 升级手中所有牌,持续到本场战斗结束(锻造祝福)
    UpgradeHand,
    /// 打出抽牌堆顶的 n 张(混沌药剂)
    PlayTopCards { n: i32 },
    /// 往手里塞 n 张牌(药水给的特殊牌;本作没有该牌时无事发生)
    AddCardToHand { id: &'static str, n: i32, upgraded: bool },
    /// 从非 Boss 战斗里脱身(烟雾弹)
    Escape,
    /// 用随机药水填满空槽(熵能酿剂)
    FillPotionSlots,
    /// 饮下时无事发生:机制在本作里不存在(别的职业专属)
    Nothing,
    /// 没有"喝"这个动作,只在被打死的那一刻自动触发(仙女在瓶中).
    /// 依据:反编译 BattleContext::drinkPotion 把 FAIRY_POTION 归进 default 分支直接
    /// assert(注释 "invalid drink potion");GameContext::drinkPotion 与
    /// isValidPotionAction(refs/sts_lightspeed/src/sim/search/GameAction.cpp:263-270) 在非战斗场景只认血药水/熵能酿剂/果汁;
    /// 参考实现 potions/index.ts:316 的 FAIRY_POTION 也自注 "non-drinkable death-save".
    Passive,
}

impl PotionFx {
    /// 神圣树皮:数值翻倍.参考实现里 sacredBarkDoubles=false 的那几瓶
    /// (万灵药/赌徒之酿/锻造祝福/烟雾弹/熵能酿剂/姿态类)不受影响.
    pub fn doubled(self) -> PotionFx {
        match self {
            PotionFx::Damage { amount } => PotionFx::Damage { amount: amount * 2 },
            PotionFx::DamageAll { amount } => PotionFx::DamageAll { amount: amount * 2 },
            PotionFx::Weak { n } => PotionFx::Weak { n: n * 2 },
            PotionFx::Vulnerable { n } => PotionFx::Vulnerable { n: n * 2 },
            PotionFx::Poison { n } => PotionFx::Poison { n: n * 2 },
            PotionFx::Block { amount } => PotionFx::Block { amount: amount * 2 },
            PotionFx::Energy { n } => PotionFx::Energy { n: n * 2 },
            PotionFx::Draw { n } => PotionFx::Draw { n: n * 2 },
            PotionFx::DrawAndRandomizeCosts { n } => {
                PotionFx::DrawAndRandomizeCosts { n: n * 2 }
            }
            PotionFx::HealPercent { pct } => PotionFx::HealPercent { pct: pct * 2 },
            PotionFx::MaxHp { n } => PotionFx::MaxHp { n: n * 2 },
            PotionFx::Status { status, n } => PotionFx::Status { status, n: n * 2 },
            PotionFx::TempStatus { status, lose, n } => PotionFx::TempStatus {
                status,
                lose,
                n: n * 2,
            },
            PotionFx::Discovery { pool, n } => PotionFx::Discovery { pool, n: n * 2 },
            PotionFx::ReturnFromDiscard { n } => PotionFx::ReturnFromDiscard { n: n * 2 },
            PotionFx::PlayTopCards { n } => PotionFx::PlayTopCards { n: n * 2 },
            PotionFx::AddCardToHand { id, n, upgraded } => PotionFx::AddCardToHand {
                id,
                n: n * 2,
                upgraded,
            },
            other => other,
        }
    }
}

#[derive(Debug)]
pub struct PotionDef {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub rarity: Rarity,
    /// shared / red / green / blue / purple:哪些职业能抽到
    pub class: &'static str,
    /// 需要指定敌人的药水(伤害/减益类)
    pub target: Target,
    /// 能否在地图界面使用
    pub out_of_combat: bool,
    pub fx: PotionFx,
}

impl PotionDef {
    /// 能不能主动喝.只有仙女在瓶中例外:它没有"喝"这个动作,防止玩家在
    /// 地图/战斗里把它白白喝掉(那样会连带丢掉它的死亡保护).
    pub fn drinkable(&self) -> bool {
        !matches!(self.fx, PotionFx::Passive)
    }
}

/// 全 42 瓶,顺序与语料一致(参考实现的 bundle 顺序)
pub static POTIONS: &[PotionDef] = &[
    PotionDef {
        id: "ambrosia",
        name: "Ambrosia",
        desc: "Enter Divinity.",
        rarity: Rarity::Rare,
        class: "purple",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Nothing,
    },
    PotionDef {
        id: "ancient_potion",
        name: "Ancient Potion",
        desc: "Gain 1 Artifact.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Artifact, n: 1 },
    },
    PotionDef {
        id: "attack_potion",
        name: "Attack Potion",
        desc: "Choose 1 of 3 random Attack cards to add to your hand. It costs 0 this turn.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Discovery { pool: DiscoveryPool::Attack, n: 1 },
    },
    PotionDef {
        id: "blessing_of_the_forge",
        name: "Blessing of the Forge",
        desc: "Upgrade all cards in your hand for the rest of combat.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::UpgradeHand,
    },
    PotionDef {
        id: "block_potion",
        name: "Block Potion",
        desc: "Gain 12 Block.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Block { amount: 12 },
    },
    PotionDef {
        id: "blood_potion",
        name: "Blood Potion",
        desc: "Heal for 20% of your Max HP.",
        rarity: Rarity::Common,
        class: "red",
        target: Target::None,
        out_of_combat: true,
        fx: PotionFx::HealPercent { pct: 20 },
    },
    PotionDef {
        id: "bottled_miracle",
        name: "Bottled Miracle",
        desc: "Add 2 Miracles to your hand.",
        rarity: Rarity::Common,
        class: "purple",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::AddCardToHand { id: "miracle", n: 2, upgraded: false },
    },
    PotionDef {
        id: "colorless_potion",
        name: "Colorless Potion",
        desc: "Choose 1 of 3 random Colorless cards to add to your hand. It costs 0 this turn.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Discovery { pool: DiscoveryPool::Colorless, n: 1 },
    },
    PotionDef {
        id: "cultist_potion",
        name: "Cultist Potion",
        desc: "Gain 1 Ritual.",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Ritual, n: 1 },
    },
    PotionDef {
        id: "cunning_potion",
        name: "Cunning Potion",
        desc: "Add 3 Shivs+ to your hand.",
        rarity: Rarity::Uncommon,
        class: "green",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::AddCardToHand { id: "shiv", n: 3, upgraded: true },
    },
    PotionDef {
        id: "dexterity_potion",
        name: "Dexterity Potion",
        desc: "Gain 2 Dexterity.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Dexterity, n: 2 },
    },
    PotionDef {
        id: "distilled_chaos",
        name: "Distilled Chaos",
        desc: "Play the top 3 cards of your draw pile.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::PlayTopCards { n: 3 },
    },
    PotionDef {
        id: "duplication_potion",
        name: "Duplication Potion",
        desc: "This turn, your next card is played twice.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Duplication, n: 1 },
    },
    PotionDef {
        id: "elixir_potion",
        name: "Elixir",
        desc: "Exhaust any number of cards in your hand.",
        rarity: Rarity::Uncommon,
        class: "red",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::ExhaustHand,
    },
    PotionDef {
        id: "energy_potion",
        name: "Energy Potion",
        desc: "Gain 2 Energy.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Energy { n: 2 },
    },
    PotionDef {
        id: "entropic_brew",
        name: "Entropic Brew",
        desc: "Fill all your empty potion slots with random potions.",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: true,
        fx: PotionFx::FillPotionSlots,
    },
    PotionDef {
        id: "essence_of_darkness",
        name: "Essence of Darkness",
        desc: "Channel 1 Dark for each orb slot.",
        rarity: Rarity::Rare,
        class: "blue",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Nothing,
    },
    PotionDef {
        id: "essence_of_steel",
        name: "Essence of Steel",
        desc: "Gain 4 Plated Armor.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::PlatedArmor, n: 4 },
    },
    PotionDef {
        id: "explosive_potion",
        name: "Explosive Potion",
        desc: "Deal 10 damage to all enemies.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::DamageAll { amount: 10 },
    },
    PotionDef {
        id: "fairy_potion",
        name: "Fairy in a Bottle",
        desc: "When you would die, heal to 30% of your Max HP instead and discard this potion.",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Passive,
    },
    PotionDef {
        id: "fear_potion",
        name: "Fear Potion",
        desc: "Apply 3 Vulnerable to target enemy.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::Vulnerable { n: 3 },
    },
    PotionDef {
        id: "fire_potion",
        name: "Fire Potion",
        desc: "Deal 20 damage to target enemy.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::Damage { amount: 20 },
    },
    PotionDef {
        id: "flex_potion",
        name: "Flex Potion",
        desc: "Gain 5 Strength. At the end of your turn, lose 5 Strength.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::TempStatus {
            status: Status::Strength,
            lose: Status::LoseStrength,
            n: 5,
        },
    },
    PotionDef {
        id: "focus_potion",
        name: "Focus Potion",
        desc: "Gain 2 Focus.",
        rarity: Rarity::Common,
        class: "blue",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Focus, n: 2 },
    },
    PotionDef {
        id: "fruit_juice",
        name: "Fruit Juice",
        desc: "Gain 5 Max HP.",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: true,
        fx: PotionFx::MaxHp { n: 5 },
    },
    PotionDef {
        id: "gamblers_brew",
        name: "Gambler's Brew",
        desc: "Discard any number of cards, then draw that many.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::DiscardHandThenDraw,
    },
    PotionDef {
        id: "ghost_in_a_jar",
        name: "Ghost in a Jar",
        desc: "Gain 1 Intangible.",
        rarity: Rarity::Rare,
        class: "green",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Intangible, n: 1 },
    },
    PotionDef {
        id: "heart_of_iron",
        name: "Heart of Iron",
        desc: "Gain 6 Metallicize.",
        rarity: Rarity::Rare,
        class: "red",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Metallicize, n: 6 },
    },
    PotionDef {
        id: "liquid_bronze",
        name: "Liquid Bronze",
        desc: "Gain 3 Thorns.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Thorns, n: 3 },
    },
    PotionDef {
        id: "liquid_memories",
        name: "Liquid Memories",
        desc: "Choose a card in your discard pile and return it to your hand. It costs 0 this turn.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::ReturnFromDiscard { n: 1 },
    },
    PotionDef {
        id: "poison_potion",
        name: "Poison Potion",
        desc: "Apply 6 Poison to target enemy.",
        rarity: Rarity::Common,
        class: "green",
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::Poison { n: 6 },
    },
    PotionDef {
        id: "potion_of_capacity",
        name: "Potion of Capacity",
        desc: "Gain 2 Orb slots.",
        rarity: Rarity::Uncommon,
        class: "blue",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Nothing,
    },
    PotionDef {
        id: "power_potion",
        name: "Power Potion",
        desc: "Choose 1 of 3 random Power cards to add to your hand. It costs 0 this turn.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Discovery { pool: DiscoveryPool::Power, n: 1 },
    },
    PotionDef {
        id: "regen_potion",
        name: "Regen Potion",
        desc: "Gain 5 Regeneration.",
        rarity: Rarity::Uncommon,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Regenerate, n: 5 },
    },
    PotionDef {
        id: "skill_potion",
        name: "Skill Potion",
        desc: "Choose 1 of 3 random Skill cards to add to your hand. It costs 0 this turn.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Discovery { pool: DiscoveryPool::Skill, n: 1 },
    },
    PotionDef {
        id: "smoke_bomb",
        name: "Smoke Bomb",
        desc: "Escape from a non-boss combat. Receive no rewards.",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Escape,
    },
    PotionDef {
        id: "snecko_oil",
        name: "Snecko Oil",
        desc: "Draw 5 Cards. Randomize the cost of cards in your hand",
        rarity: Rarity::Rare,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::DrawAndRandomizeCosts { n: 5 },
    },
    PotionDef {
        id: "speed_potion",
        name: "Speed Potion",
        desc: "Gain 5 Dexterity. At the end of your turn, lose 5 Dexterity.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::TempStatus {
            status: Status::Dexterity,
            lose: Status::LoseDexterity,
            n: 5,
        },
    },
    PotionDef {
        id: "stance_potion",
        name: "Stance Potion",
        desc: "Enter Calm or Wrath.",
        rarity: Rarity::Uncommon,
        class: "purple",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Nothing,
    },
    PotionDef {
        id: "strength_potion",
        name: "Strength Potion",
        desc: "Gain 2 Strength.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Status { status: Status::Strength, n: 2 },
    },
    PotionDef {
        id: "swift_potion",
        name: "Swift Potion",
        desc: "Draw 3 cards.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::None,
        out_of_combat: false,
        fx: PotionFx::Draw { n: 3 },
    },
    PotionDef {
        id: "weak_potion",
        name: "Weak Potion",
        desc: "Apply 3 Weak to target enemy.",
        rarity: Rarity::Common,
        class: "shared",
        target: Target::Enemy,
        out_of_combat: false,
        fx: PotionFx::Weak { n: 3 },
    },
];

pub fn by_id(id: &str) -> Option<&'static PotionDef> {
    POTIONS.iter().find(|p| p.id == id)
}

/// 职业代号换成药水池的颜色(参考实现 classColor)
pub fn class_color(character: &str) -> &'static str {
    match character {
        "silent" => "green",
        "defect" => "blue",
        "watcher" => "purple",
        _ => "red",
    }
}

/// 某个职业能抽到的药水(共享池 + 职业池),顺序就是语料顺序(参考实现的 bundle 顺序)
pub fn pool(color: &str) -> Vec<&'static PotionDef> {
    POTIONS
        .iter()
        .filter(|p| p.class == "shared" || p.class == color)
        .collect()
}

/// 药水稀有度的两档分界(参考实现 POTION_DROP.commonBelow / uncommonBelow)
const COMMON_BELOW: i32 = 65;
const UNCOMMON_BELOW: i32 = 90;

/// 随机一瓶药水(参考实现的 returnRandomPotion):先掷稀有度,
/// 再从池子里逐瓶抽到稀有度对上为止
pub fn random_potion(rng: &mut Rng, color: &str) -> Option<&'static PotionDef> {
    let pool = pool(color);
    assert!(!pool.is_empty(), "potion pool is empty");
    let roll = rng.random_range(0, 99);
    let rarity = if roll < COMMON_BELOW {
        Rarity::Common
    } else if roll < UNCOMMON_BELOW {
        Rarity::Uncommon
    } else {
        Rarity::Rare
    };
    if !pool.iter().any(|p| p.rarity == rarity) {
        return None;
    }
    loop {
        let i = rng.random(pool.len() as u32 - 1) as usize;
        if pool[i].rarity == rarity {
            return Some(pool[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = POTIONS.iter().map(|p| p.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate potion id");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = POTIONS.iter().map(|p| p.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate potion name");
    }

    #[test]
    fn random_potion_stays_in_pool() {
        let mut rng = Rng::new(4);
        for _ in 0..50 {
            let p = random_potion(&mut rng, "red").expect("池子里三档稀有度都有");
            assert!(p.class == "shared" || p.class == "red");
            assert!(POTIONS.iter().any(|q| q.id == p.id));
        }
    }

    #[test]
    fn all_42_potions_are_present() {
        assert_eq!(POTIONS.len(), 42, "语料里一共 42 瓶药水");
    }

    /// 职业代号 -> 药水池颜色(参考实现 rewards.ts classColor);
    /// 药水池按 class 过滤,这里错一位就会把别职业的药水塞进池子
    #[test]
    fn class_color_maps_every_character() {
        assert_eq!(class_color("ironclad"), "red");
        assert_eq!(class_color("silent"), "green");
        assert_eq!(class_color("defect"), "blue");
        assert_eq!(class_color("watcher"), "purple");
    }

    #[test]
    fn table_order_matches_the_corpus() {
        // 抽签按下标走,池子顺序一旦和语料(参考的 bundle 顺序)不同,抽出来的身份就不对
        let corpus: Vec<&str> = crate::core::corpus::POTIONS.iter().map(|p| p.id).collect();
        let ours: Vec<&str> = POTIONS.iter().map(|p| p.id).collect();
        assert_eq!(ours, corpus, "药水表顺序要和语料一致");
    }

    #[test]
    fn rarity_and_class_match_the_corpus() {
        for p in POTIONS {
            let c = crate::core::corpus::POTIONS
                .iter()
                .find(|c| c.id == p.id)
                .unwrap_or_else(|| panic!("{} 在语料里没有", p.id));
            assert_eq!(p.name, c.name, "{} 的名字对不上", p.id);
            assert_eq!(p.class, c.color, "{} 的职业池对不上", p.id);
            assert_eq!(p.rarity.name().to_lowercase(), c.rarity, "{} 的稀有度对不上", p.id);
            assert_eq!(
                p.target.needs_enemy(),
                c.targeted,
                "{} 的目标需求对不上",
                p.id
            );
        }
    }

    #[test]
    fn red_pool_order_matches_reference_bundle() {
        // 参考实现:遍历 bundle(语料顺序),留下 shared 与职业色的
        let want = [
            "ancient_potion",
            "attack_potion",
            "blessing_of_the_forge",
            "block_potion",
            "blood_potion",
            "colorless_potion",
            "cultist_potion",
            "dexterity_potion",
            "distilled_chaos",
            "duplication_potion",
            "elixir_potion",
            "energy_potion",
            "entropic_brew",
            "essence_of_steel",
            "explosive_potion",
            "fairy_potion",
            "fear_potion",
            "fire_potion",
            "flex_potion",
            "fruit_juice",
            "gamblers_brew",
            "heart_of_iron",
            "liquid_bronze",
            "liquid_memories",
            "power_potion",
            "regen_potion",
            "skill_potion",
            "smoke_bomb",
            "snecko_oil",
            "speed_potion",
            "strength_potion",
            "swift_potion",
            "weak_potion",
        ];
        let ours: Vec<&str> = pool("red").iter().map(|p| p.id).collect();
        assert_eq!(ours, want, "红色职业的药水池顺序不对");
    }

    #[test]
    fn rarity_coverage() {
        for rarity in [Rarity::Common, Rarity::Uncommon, Rarity::Rare] {
            assert!(
                POTIONS.iter().any(|p| p.rarity == rarity),
                "no potion of rarity {}",
                rarity.name()
            );
        }
    }

    #[test]
    fn has_enemy_target_potion() {
        assert!(POTIONS.iter().any(|p| p.target.needs_enemy()));
    }

    #[test]
    fn has_out_of_combat_heal_potion() {
        assert!(POTIONS
            .iter()
            .any(|p| p.out_of_combat && matches!(p.fx, PotionFx::HealPercent { .. })));
    }

    /// 地图上能主动喝的只有血药水/熵能酿剂/果汁:反编译 GameContext::drinkPotion 只处理
    /// 这三瓶,isValidPotionAction(refs/sts_lightspeed/src/sim/search/GameAction.cpp:263-270) 也只放行这三瓶,其余只能丢.
    #[test]
    fn only_three_potions_are_usable_on_the_map() {
        let mut ids: Vec<&str> = POTIONS
            .iter()
            .filter(|p| p.out_of_combat)
            .map(|p| p.id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["blood_potion", "entropic_brew", "fruit_juice"]);
    }

    /// 仙女在瓶中是被动药水:没有"喝"这个动作(反编译里 BattleContext::drinkPotion 对它
    /// 直接 assert;参考实现也自注 non-drinkable death-save).
    #[test]
    fn fairy_in_a_bottle_is_not_drinkable() {
        assert!(!by_id("fairy_potion").unwrap().drinkable(), "仙女在瓶中不该能主动喝");
        assert_eq!(
            POTIONS.iter().filter(|p| !p.drinkable()).count(),
            1,
            "只有仙女在瓶中是被动药水"
        );
    }

    #[test]
    fn strings_are_ascii_only() {
        for p in POTIONS {
            assert!(p.id.is_ascii(), "non-ascii id: {}", p.id);
            assert!(p.name.is_ascii(), "non-ascii name: {}", p.name);
            assert!(p.desc.is_ascii(), "non-ascii desc: {}", p.desc);
        }
    }
}

/// 药水效果本身在战斗里怎么落地:一次把每种机制各打一遍
#[cfg(test)]
mod effect_tests {
    use super::*;
    use crate::core::combat::{ChoiceAction, ChoiceSource, Combat, CombatSetup, RunRelicCounters};
    use crate::core::enemy::Intent;
    use crate::rng::RngRegistry;

    fn enc(id: &'static str) -> &'static crate::core::enemy::Encounter {
        crate::core::enemies::encounter_def(id).unwrap_or_else(|| panic!("no encounter {id}"))
    }

    fn combat_in(encounter: &'static str, deck: &[&str]) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: deck.iter().map(|id| crate::core::cards::card(id)).collect(),
            relics: Vec::new(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
        asc: 0,
        };
        Combat::new(enc(encounter), setup, RngRegistry::new(7))
    }

    /// 15 张牌:起手抽 5 张,抽牌堆里还剩 10 张,够药水折腾
    const DECK: &[&str] = &[
        "strike", "strike", "strike", "strike", "strike", "strike", "strike", "strike", "defend",
        "defend", "defend", "defend", "defend", "bash", "bash",
    ];

    fn combat() -> Combat {
        combat_in("cultist_solo", DECK)
    }

    fn def(id: &str) -> &'static PotionDef {
        by_id(id).unwrap_or_else(|| panic!("no potion {id}"))
    }

    /// 每只怪都换成"第一招是攻击"的那一招,好让反伤/降伤在敌人回合里真的发生
    fn force_attack(c: &mut Combat) {
        for e in c.enemies.iter_mut() {
            if let Some(i) = e
                .def
                .moves
                .iter()
                .position(|m| matches!(m.intent, Intent::Attack { .. }))
            {
                e.next_move = i;
            }
        }
    }

    #[test]
    fn fire_potion_deals_20_to_the_chosen_enemy() {
        let mut c = combat();
        let before = c.enemies[0].hp;
        c.use_potion(def("fire_potion"), Some(0));
        assert_eq!(c.enemies[0].hp, before - 20);
    }

    #[test]
    fn explosive_potion_hits_every_enemy() {
        let mut c = combat_in("three_cultists", &["strike"]);
        let before: Vec<i32> = c.enemies.iter().map(|e| e.hp).collect();
        c.use_potion(def("explosive_potion"), None);
        for (i, e) in c.enemies.iter().enumerate() {
            assert_eq!(e.hp, before[i] - 10, "第 {i} 只没吃到伤害");
        }
    }

    #[test]
    fn fear_potion_only_marks_the_target() {
        let mut c = combat_in("three_cultists", &["strike"]);
        c.use_potion(def("fear_potion"), Some(1));
        assert_eq!(c.enemies[1].statuses.get(Status::Vulnerable), 3);
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 0);
        assert_eq!(c.enemies[2].statuses.get(Status::Vulnerable), 0);
    }

    #[test]
    fn weak_potion_only_marks_the_target() {
        let mut c = combat_in("three_cultists", &["strike"]);
        c.use_potion(def("weak_potion"), Some(2));
        assert_eq!(c.enemies[2].statuses.get(Status::Weak), 3);
        assert_eq!(c.enemies[0].statuses.get(Status::Weak), 0);
    }

    #[test]
    fn block_and_energy_potions() {
        let mut c = combat();
        let energy = c.energy;
        c.use_potion(def("block_potion"), None);
        c.use_potion(def("energy_potion"), None);
        assert_eq!(c.player.block, 12);
        assert_eq!(c.energy, energy + 2);
    }

    #[test]
    fn swift_potion_draws_three() {
        let mut c = combat();
        let before = c.hand.len();
        c.use_potion(def("swift_potion"), None);
        assert_eq!(c.hand.len(), before + 3);
    }

    #[test]
    fn strength_and_dexterity_potions_stick() {
        let mut c = combat();
        c.use_potion(def("strength_potion"), None);
        c.use_potion(def("dexterity_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Strength), 2);
        assert_eq!(c.player.statuses.get(Status::Dexterity), 2);
    }

    #[test]
    fn flex_potion_gives_strength_back_at_end_of_turn() {
        let mut c = combat();
        c.use_potion(def("flex_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Strength), 5);
        assert_eq!(c.player.statuses.get(Status::LoseStrength), 5);
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Strength), 0);
        assert_eq!(c.player.statuses.get(Status::LoseStrength), 0);
    }

    #[test]
    fn speed_potion_gives_dexterity_back_at_end_of_turn() {
        let mut c = combat();
        c.use_potion(def("speed_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Dexterity), 5);
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Dexterity), 0);
        assert_eq!(c.player.statuses.get(Status::LoseDexterity), 0);
    }

    #[test]
    fn duplication_potion_plays_the_next_card_twice() {
        let mut c = combat();
        c.hand = vec![crate::core::cards::card("strike")];
        let before = c.enemies[0].hp;
        c.use_potion(def("duplication_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Duplication), 1);
        c.play_card(0, Some(0)).expect("strike 打得出去");
        assert_eq!(c.enemies[0].hp, before - 12, "打击应该结算两次");
        assert_eq!(c.player.statuses.get(Status::Duplication), 0);
    }

    #[test]
    fn liquid_bronze_makes_attacks_bounce_back() {
        let mut c = combat_in("jaw_worm_solo", &["defend"]);
        force_attack(&mut c);
        let before = c.enemies[0].hp;
        c.use_potion(def("liquid_bronze"), None);
        assert_eq!(c.player.statuses.get(Status::Thorns), 3);
        c.end_turn();
        assert_eq!(c.enemies[0].hp, before - 3, "挨打要给攻击者 3 点");
        assert!(c.player.hp < 80, "这一下玩家也要挨到");
    }

    #[test]
    fn essence_of_steel_blocks_at_end_of_turn() {
        let mut c = combat_in("jaw_worm_solo", DECK);
        force_attack(&mut c);
        c.use_potion(def("essence_of_steel"), None);
        assert_eq!(c.player.statuses.get(Status::PlatedArmor), 4);
        c.end_turn();
        // 11 点攻击被回合末的 4 点格挡吃掉 4,剩下 7 点真伤
        assert_eq!(c.player.hp, 73, "回合末该按镀甲给 4 点格挡");
        assert_eq!(
            c.player.statuses.get(Status::PlatedArmor),
            3,
            "挨到没挡住的伤害就掉一层"
        );
    }

    #[test]
    fn cultist_potion_ritual_gives_strength_at_end_of_turn() {
        let mut c = combat();
        c.use_potion(def("cultist_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Ritual), 1);
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Strength), 1);
    }

    #[test]
    fn heart_of_iron_blocks_at_end_of_turn() {
        let mut c = combat_in("jaw_worm_solo", DECK);
        force_attack(&mut c);
        c.use_potion(def("heart_of_iron"), None);
        assert_eq!(c.player.statuses.get(Status::Metallicize), 6);
        c.end_turn();
        // 11 点攻击被回合末的 6 点格挡吃掉 6
        assert_eq!(c.player.hp, 75, "回合末该按金属化给 6 点格挡");
    }

    #[test]
    fn regen_potion_heals_at_end_of_turn() {
        let mut c = combat();
        c.player.hp = 40;
        c.use_potion(def("regen_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Regenerate), 5);
        c.end_turn();
        assert_eq!(c.player.hp, 45);
    }

    #[test]
    fn poison_potion_ticks_on_the_enemy() {
        let mut c = combat_in("jaw_worm_solo", &["defend"]);
        force_attack(&mut c);
        let before = c.enemies[0].hp;
        c.use_potion(def("poison_potion"), Some(0));
        assert_eq!(c.enemies[0].statuses.get(Status::Poison), 6);
        c.end_turn();
        assert_eq!(c.enemies[0].hp, before - 6, "中毒在敌人回合结束结算");
        assert_eq!(c.enemies[0].statuses.get(Status::Poison), 5);
    }

    #[test]
    fn ghost_in_a_jar_softens_the_hit_to_one() {
        let mut c = combat_in("jaw_worm_solo", &["defend"]);
        force_attack(&mut c);
        c.use_potion(def("ghost_in_a_jar"), None);
        assert_eq!(c.player.statuses.get(Status::Intangible), 1);
        c.end_turn();
        assert_eq!(c.player.hp, 79, "无形的回合里只掉 1 点");
    }

    #[test]
    fn blessing_of_the_forge_upgrades_the_hand() {
        let mut c = combat();
        c.use_potion(def("blessing_of_the_forge"), None);
        assert!(c.hand.iter().all(|card| card.upgraded), "手里的牌该全部升级");
    }

    #[test]
    fn distilled_chaos_plays_the_top_card() {
        let mut c = combat();
        c.draw = vec![crate::core::cards::card("strike")];
        let before = c.enemies[0].hp;
        let energy = c.energy;
        c.use_potion(def("distilled_chaos"), None);
        // 混沌药剂打 3 张:抽牌堆只有 1 张时,打掉的牌会被洗回抽牌堆接着打
        // (参考实现的 PlayTopCardAction 在抽牌堆空时会洗牌),所以这张打击打了 3 次
        assert_eq!(c.enemies[0].hp, before - 6 * 3);
        assert_eq!(c.energy, energy, "替打出来的牌不花能量");
        assert!(c.draw.is_empty(), "那张牌应该已经打出去了");
    }

    #[test]
    fn snecko_oil_draws_and_randomizes_costs() {
        let mut c = combat();
        c.use_potion(def("snecko_oil"), None);
        assert_eq!(c.hand.len(), 10, "抽满手牌");
        for card in c.hand.iter() {
            if let Some(cost) = card.fixed_cost() {
                assert!((0..=3).contains(&cost), "费用应该被重掷到 0~3: {cost}");
            }
        }
    }

    #[test]
    fn attack_potion_offers_three_attack_cards() {
        let mut c = combat();
        c.use_potion(def("attack_potion"), None);
        let ch = c.choice.as_ref().expect("发现类药水要开一次选择");
        assert_eq!(ch.source, ChoiceSource::Offered);
        assert_eq!(ch.action, ChoiceAction::ToHand);
        assert_eq!(ch.offered.len(), 3);
        assert!(ch
            .offered
            .iter()
            .all(|card| card.kind() == crate::core::card::CardType::Attack));
        // 挑一张进手里,本回合 0 费
        let name = ch.offered[0].def.id;
        c.choose(0).expect("候选牌选得中");
        assert!(c.hand.iter().any(|card| card.def.id == name && card.free_this_turn));
    }

    #[test]
    fn elixir_exhausts_any_number_of_cards() {
        let mut c = combat();
        c.use_potion(def("elixir_potion"), None);
        let ch = c.choice.as_ref().expect("万灵药要开一次选择");
        assert_eq!(ch.source, ChoiceSource::Hand);
        assert_eq!(ch.action, ChoiceAction::Exhaust);
        let hand = c.hand.len();
        c.choose(0).expect("第一张消耗得掉");
        assert_eq!(c.exhaust.len(), 1);
        assert_eq!(c.hand.len(), hand - 1);
        c.finish_choice();
        assert!(c.choice.is_none());
    }

    #[test]
    fn liquid_memories_returns_a_card_for_free() {
        let mut c = combat();
        // 弃牌堆两张,不是"只剩一张自动结算"那条路
        c.discard = vec![
            crate::core::cards::card("bash"),
            crate::core::cards::card("defend"),
        ];
        c.use_potion(def("liquid_memories"), None);
        let ch = c.choice.as_ref().expect("液态记忆要开一次选择");
        assert_eq!(ch.source, ChoiceSource::Discard);
        assert_eq!(ch.action, ChoiceAction::ToHand);
        c.choose(0).expect("弃牌堆里那张拿得回来");
        assert_eq!(c.discard.len(), 1, "只拿走挑中的那张");
        assert!(c
            .hand
            .iter()
            .any(|card| card.def.id == "bash" && card.free_this_turn));
    }

    #[test]
    fn gamblers_brew_discards_then_draws_the_same_count() {
        let mut c = combat();
        c.use_potion(def("gamblers_brew"), None);
        let ch = c.choice.as_ref().expect("赌徒之酿要开一次选择");
        assert_eq!(ch.action, ChoiceAction::Discard);
        c.choose(0).expect("弃得掉第一张");
        c.choose(0).expect("弃得掉第二张");
        let discarded = c.discard.len();
        assert_eq!(discarded, 2);
        c.finish_choice();
        assert!(c.choice.is_none());
        // 弃两张补抽两张:手牌张数回到弃之前
        assert_eq!(c.hand.len(), 5);
    }

    // ---- 与"升级 / 费用 / 目标"的交互(对照反编译逐条核) ----

    /// 测试用:带遗物开一场(combat_in 只给空遗物栏)
    fn combat_relics(
        encounter: &'static str,
        deck: &[&str],
        relics: &[&'static crate::core::relics::RelicDef],
    ) -> Combat {
        let setup = CombatSetup {
            rested: false,
            hp: 80,
            max_hp: 80,
            deck: deck.iter().map(|id| crate::core::cards::card(id)).collect(),
            relics: relics.to_vec(),
            gold: 0,
            lift_strength: 0,
            relic_counters: RunRelicCounters::default(),
            curse_negate: 0,
            asc: 0,
        };
        Combat::new(enc(encounter), setup, RngRegistry::new(7))
    }

    fn relic(id: &str) -> &'static crate::core::relics::RelicDef {
        crate::core::relics::relic_def_or_panic(id)
    }

    /// 发现类药水(攻/技/能/无色):亮 3 张互不相同的对应类型牌,候选与选中的牌
    /// **都不升级**,选中的那张本回合 0 费.
    /// 依据:药水 → DiscoveryAction(refs/sts_lightspeed/src/combat/BattleContext.cpp:2255-2256/2278-2279/2385-2395),
    ///      generateDiscoveryCards(Actions.cpp:564-568 → Game.cpp:228-260),
    ///      chooseDiscoveryCard 造的是 `CardInstance c(id)`(不带升级,只 setCostForTurn(0),BattleContext.cpp:2995-3008).
    #[test]
    fn discovery_potions_pick_the_right_type_unupgraded_and_free() {
        use crate::core::card::CardType;
        for (potion, want) in [
            ("attack_potion", CardType::Attack),
            ("skill_potion", CardType::Skill),
            ("power_potion", CardType::Power),
        ] {
            let mut c = combat();
            c.use_potion(def(potion), None);
            let ch = c.choice.as_ref().unwrap_or_else(|| panic!("{potion} 要开屏"));
            assert_eq!(ch.offered.len(), 3, "{potion} 亮 3 张");
            for card in &ch.offered {
                assert_eq!(card.kind(), want, "{potion} 候选类型不对: {}", card.def.id);
                assert!(!card.upgraded, "{potion} 候选不该升级");
            }
            let pick = ch.offered[0].def.id;
            c.choose(0).expect("选得中");
            let got = c.hand.iter().find(|x| x.def.id == pick).expect("进手牌");
            assert!(!got.upgraded, "{potion} 拿到的牌不该升级");
            assert!(got.free_this_turn, "{potion} 拿到的牌本回合 0 费");
        }
        // 无色:池子取无色牌的非普通档
        let mut c = combat();
        c.use_potion(def("colorless_potion"), None);
        let ch = c.choice.as_ref().expect("无色药剂要开屏");
        assert_eq!(ch.offered.len(), 3);
        for card in &ch.offered {
            assert_eq!(
                crate::core::cards::pool_of(card.def),
                "colorless",
                "无色药剂只能亮无色牌"
            );
            assert_ne!(card.rarity(), Rarity::Common, "无色池不含普通档");
        }
    }

    /// 蛋类遗物(molten/toxic/frozen egg)只在"卡进牌组"的时点生效
    /// (refs/sts_lightspeed/src/game/Deck.cpp:128-157、GameContext.cpp:1560-1576),
    /// 战斗中药水新造的发现牌不吃;候选依旧不升级.
    #[test]
    fn discovery_potions_ignore_egg_relics() {
        let mut c = combat_relics(
            "cultist_solo",
            DECK,
            &[relic("molten_egg"), relic("toxic_egg"), relic("frozen_egg")],
        );
        c.use_potion(def("attack_potion"), None);
        let ch = c.choice.as_ref().expect("攻击药剂要开屏");
        assert!(
            ch.offered.iter().all(|card| !card.upgraded),
            "蛋不影响战斗中新造的牌"
        );
        let pick = ch.offered[0].def.id;
        c.choose(0).unwrap();
        assert!(!c.hand.iter().find(|x| x.def.id == pick).unwrap().upgraded);
    }

    /// 神圣树皮把发现类的"份数"翻倍:语料 ATTACK_POTION text "add[|two copies of it]",
    /// DiscoveryAction 的 amount = hasBark ? 2 : 1(BattleContext.cpp:2255-2256),
    /// chooseDiscoveryCard 按份数把手牌复制成多份.
    #[test]
    fn sacred_bark_adds_two_copies_of_the_discovered_card() {
        let mut c = combat_relics("cultist_solo", DECK, &[relic("sacred_bark")]);
        c.use_potion(def("attack_potion"), None);
        assert_eq!(c.choice.as_ref().unwrap().copies, 2, "树皮翻成 2 份");
        let pick = c.choice.as_ref().unwrap().offered[0].def.id;
        c.choose(0).unwrap();
        assert_eq!(
            c.hand.iter().filter(|x| x.def.id == pick).count(),
            2,
            "挑中的那张给两份"
        );
    }

    /// 塞牌类药水:反编译 BOTTLED_MIRACLE → MakeTempCardInHand(MIRACLE,false,2)、
    /// CUNNING_POTION → MakeTempCardInHand(SHIV,true,3)(BattleContext.cpp:2274-2275/2286-2287).
    /// 本作没有 Miracle/Shiv 这两张 special token(cards.rs 的牌池范围只到红职+无色+诅咒状态),
    /// 于是 AddCardToHand 找不到定义就无事发生 —— 这是范围限制,不是这次要改的交互.
    #[test]
    fn add_card_to_hand_potions_are_inert_without_the_token_cards() {
        assert!(crate::core::cards::card_def("miracle").is_none());
        assert!(crate::core::cards::card_def("shiv").is_none());
        let mut c = combat();
        let before = c.hand.len();
        c.use_potion(def("bottled_miracle"), None);
        c.use_potion(def("cunning_potion"), None);
        assert_eq!(c.hand.len(), before, "没有对应的 token 牌就不该有手牌变化");
    }

    /// 灵药(ExhaustMany):可选,能消耗任意张(含诅咒),也可以一张都不消耗.
    /// 依据 Actions.cpp:973-978(无条件进 CARD_SELECT)、BattleContext.cpp:3067-3080(chooseExhaustCards 走 triggerAndMoveToExhaustPile).
    #[test]
    fn elixir_can_exhaust_curses_and_can_take_none() {
        let mut c = combat_in("cultist_solo", &["strike", "regret", "defend", "defend", "defend"]);
        // 可选:立刻收工,一张不消耗
        c.use_potion(def("elixir_potion"), None);
        assert!(c.choice.is_some(), "可选也要开屏");
        c.finish_choice();
        assert!(c.exhaust.is_empty(), "可以选择一张都不消耗");
        // 消耗一张诅咒
        c.use_potion(def("elixir_potion"), None);
        let idx = c
            .choice_candidates()
            .into_iter()
            .find(|(_, card)| card.def.id == "regret")
            .map(|(i, _)| i)
            .expect("悔恨在手上");
        c.choose(idx).unwrap();
        c.finish_choice();
        assert!(c.exhaust.iter().any(|x| x.def.id == "regret"), "诅咒也能耗掉");
    }

    /// 锻造祝福(UpgradeAllCardsInHand):只升"可升级"的牌;诅咒/状态/已升级的牌跳过;
    /// 灼热攻击这类多段升级一次只升一级.
    /// 依据 Actions.cpp:901-905 与 CardInstance::upgrade(refs/sts_lightspeed/src/combat/CardInstance.cpp:135-172).
    #[test]
    fn blessing_of_the_forge_upgrades_only_upgradable_cards_once() {
        let mut c = combat_in("cultist_solo", &["strike"; 5]);
        let mut bash_plus = crate::core::cards::card("bash");
        bash_plus.upgrade();
        c.hand = vec![
            crate::core::cards::card("strike"),
            crate::core::cards::card("wound"),
            crate::core::cards::card("regret"),
            bash_plus,
            crate::core::cards::card("searing_blow"),
        ];
        c.use_potion(def("blessing_of_the_forge"), None);
        let has = |id: &str| c.hand.iter().any(|x| x.def.id == id && x.upgraded);
        assert!(has("strike"), "普通的可升级牌要升");
        assert!(!has("wound"), "状态牌不可升级");
        assert!(!has("regret"), "诅咒不可升级");
        assert_eq!(
            c.hand.iter().filter(|x| x.def.id == "bash" && x.upgraded).count(),
            1,
            "已升级的不重复升"
        );
        assert_eq!(
            c.hand.iter().find(|x| x.def.id == "searing_blow").unwrap().plus,
            1,
            "多段升级一次只升一级"
        );
    }

    /// 液态记忆:从弃牌堆拿回的牌保持原样(升级状态不丢),本回合 0 费;弃牌堆空则不开窗口.
    /// 依据 BattleContext.cpp:2985-2991(setCostForTurn(0) 再 moveToHand)与 Actions.cpp:664-670(空堆直接 return).
    #[test]
    fn liquid_memories_keeps_the_card_intact_and_is_free_this_turn() {
        // 牌组里不放 bash,免得手牌里本来就有一张没升级的 bash 干扰断言
        let mut c = combat_in("cultist_solo", &["strike"; 5]);
        let mut bash_plus = crate::core::cards::card("bash");
        bash_plus.upgrade();
        c.discard = vec![
            bash_plus,
            crate::core::cards::card("defend"),
            crate::core::cards::card("strike"),
        ];
        c.use_potion(def("liquid_memories"), None);
        c.choose(0).expect("拿得回来");
        let got = c.hand.iter().find(|x| x.def.id == "bash").expect("bash 回来了");
        assert!(got.upgraded, "升级状态保持");
        assert!(got.free_this_turn, "本回合 0 费");
        // 空弃牌堆不开窗口
        let mut c = combat();
        c.discard.clear();
        c.use_potion(def("liquid_memories"), None);
        assert!(c.choice.is_none(), "弃牌堆空不开窗口");
    }

    /// 蛇油(RandomizeHandCost):只重掷"能算出费用"的牌;X 费与不可打出的牌跳过.
    /// 依据 Actions.cpp:423-433 的 `if (c.cost >= 0)`.
    #[test]
    fn snecko_oil_leaves_x_cost_and_unplayable_cards_alone() {
        let mut c = combat();
        c.hand = vec![
            crate::core::cards::card("whirlwind"),
            crate::core::cards::card("regret"),
            crate::core::cards::card("strike"),
        ];
        c.use_potion(def("snecko_oil"), None);
        let ww = c.hand.iter().find(|x| x.def.id == "whirlwind").unwrap();
        assert!(
            matches!(ww.cost(), crate::core::card::Cost::X),
            "X 费牌不重掷"
        );
        assert_eq!(ww.fixed_cost(), None);
        let rg = c.hand.iter().find(|x| x.def.id == "regret").unwrap();
        assert!(
            matches!(rg.cost(), crate::core::card::Cost::Unplayable),
            "不可打出的牌不重掷"
        );
    }

    /// 能量药水:0 能量照加;能量没有上限(可以超过 max_energy);冰激凌把没用完的能量留到下一回合;
    /// 化学 X 让 X 费牌的 X 额外 +2.
    /// 依据 BattleContext.cpp:2310-2312(GainEnergy)、CardManager 回合开始重置能量并保留冰激凌的余额、
    /// 化学 X 的 x_cost_bonus(见 combat.rs play_card 的 is_x 段).
    #[test]
    fn energy_potion_has_no_cap_and_stacks_with_ice_cream_and_chemical_x() {
        let mut c = combat();
        c.energy = 0;
        c.use_potion(def("energy_potion"), None);
        assert_eq!(c.energy, 2, "0 能量时 +2");
        c.energy = c.max_energy;
        c.use_potion(def("energy_potion"), None);
        assert_eq!(c.energy, c.max_energy + 2, "能量可以超过上限");

        // 冰激凌:没用完的能量留到下一回合
        let mut c = combat_relics("cultist_solo", DECK, &[relic("ice_cream")]);
        c.energy = 5;
        c.end_turn();
        assert_eq!(c.energy, c.max_energy + 5, "冰激凌留住 5 点");

        // 化学 X:能量药水打底,旋风的 X = 能量 + 2
        let mut c = combat_relics("jaw_worm_solo", &["whirlwind"; 5], &[relic("chemical_x")]);
        c.enemies[0].hp = 999;
        c.energy = 0;
        c.use_potion(def("energy_potion"), None);
        let before = c.enemies[0].hp;
        c.play_card(0, None).expect("旋风打得出去");
        assert_eq!(before - c.enemies[0].hp, 5 * (2 + 2), "X = 2 能量 + 2 化学X");
        assert_eq!(c.energy, 0, "X 费把能量全花掉");
    }

    /// 神器交互:神器药水给玩家 1 层(树皮 2 层);怪物身上的神器会顶掉恐惧/虚弱/中毒这类减益药水.
    /// 依据 BattleContext.cpp:2251-2252 / DebuffEnemy 的 ApplyPower 路线.
    #[test]
    fn artifact_potion_and_enemy_artifact_interaction() {
        let mut c = combat();
        c.use_potion(def("ancient_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Artifact), 1);
        let mut c = combat_relics("cultist_solo", DECK, &[relic("sacred_bark")]);
        c.use_potion(def("ancient_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Artifact), 2, "树皮翻倍");

        let mut c = combat_in("three_cultists", &["strike"]);
        c.enemies[1].statuses.add(Status::Artifact, 1);
        c.use_potion(def("fear_potion"), Some(1));
        assert_eq!(c.enemies[1].statuses.get(Status::Vulnerable), 0, "神器顶掉易伤");
        assert_eq!(c.enemies[1].statuses.get(Status::Artifact), 0, "顶掉一层");
        assert_eq!(c.enemies[0].statuses.get(Status::Vulnerable), 0, "只打指定的那只");
    }

    /// 百分比回血与加最大生命:血药水按最大生命 20%(树皮 40%,本作按战斗外那条);
    /// 花开彼岸把血药水的回血归零;果汁直接加最大生命、不走 heal,所以不被花开彼岸挡.
    /// 依据 heal_player 的 no_heal 早退(refs/sts_lightspeed/src/combat/Player.cpp 的 Player::heal)
    /// 与 BattleContext.cpp:2353-2355(increaseMaxHp).
    #[test]
    fn blood_and_fruit_potions_under_no_heal_and_bark() {
        let mut c = combat();
        c.player.hp = 40;
        c.use_potion(def("blood_potion"), None);
        assert_eq!(c.player.hp, 40 + 80 * 20 / 100);
        let mut c = combat_relics("cultist_solo", DECK, &[relic("sacred_bark")]);
        c.player.hp = 40;
        c.use_potion(def("blood_potion"), None);
        assert_eq!(c.player.hp, 40 + 80 * 40 / 100, "树皮 40%");

        let mut c = combat_relics("cultist_solo", DECK, &[relic("mark_of_the_bloom")]);
        c.player.hp = 40;
        c.use_potion(def("blood_potion"), None);
        assert_eq!(c.player.hp, 40, "花开彼岸挡掉回血");
        c.use_potion(def("fruit_juice"), None);
        assert_eq!(c.player.max_hp, 85, "果汁直接加最大生命");
        assert_eq!(c.player.hp, 45, "果汁不加血而是直接改当前血");
    }

    /// 液态记忆对 X 费牌(cost=-1)的"本回合0费"是空操作:反编译 setCostForTurn 只在
    /// costForTurn>=0 时生效,X 卡初值 -1(CardInstance.cpp:125-131);参考实现同样对
    /// cost<0 放行(interpreter.ts:423-425).所以拿回的旋风照常按当前能量结算,不免费.
    #[test]
    fn liquid_memories_does_not_free_an_x_cost_card() {
        let mut c = combat_in("jaw_worm_solo", &["strike"; 5]);
        c.enemies[0].hp = 999;
        c.discard = vec![
            crate::core::cards::card("whirlwind"),
            crate::core::cards::card("defend"),
        ];
        c.use_potion(def("liquid_memories"), None);
        c.choose(0).expect("拿得回来");
        let ww = c.hand.iter().find(|x| x.def.id == "whirlwind").expect("旋风回来了");
        assert!(ww.free_this_turn, "标记还在,但对 X 费不生效");
        assert_eq!(ww.fixed_cost(), None, "X 费牌不吃'本回合0费'");
        c.energy = 3;
        let before = c.enemies[0].hp;
        let idx = c.hand.iter().position(|x| x.def.id == "whirlwind").unwrap();
        c.play_card(idx, None).expect("旋风打得出去");
        assert_eq!(before - c.enemies[0].hp, 15, "X = 3 能量 -> 5x3");
        assert_eq!(c.energy, 0, "照常花光能量");
    }

    /// 神器顶掉"回合末收回"的 LoseX:反编译 flex/speed 先 BuffPlayer 再 DebuffPlayer,
    /// 后者是减益,有神器就被顶掉(BattleContext.cpp:2344-2347 / Player.h:362-376),
    /// 于是这 5 点力量永久留下.参考实现 LOSE_STRENGTH 也是 kind="debuff".
    #[test]
    fn flex_potion_strength_survives_when_artifact_blocks_the_loss() {
        let mut c = combat();
        c.use_potion(def("flex_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Strength), 5);
        assert_eq!(c.player.statuses.get(Status::LoseStrength), 5);

        // 先喝神器药水拿到 1 层神器,再喝屈伸:收回被顶掉,力量留下
        let mut c = combat();
        c.use_potion(def("ancient_potion"), None);
        c.use_potion(def("flex_potion"), None);
        assert_eq!(c.player.statuses.get(Status::Artifact), 0, "神器顶掉 Lose Strength");
        assert_eq!(c.player.statuses.get(Status::LoseStrength), 0);
        c.end_turn();
        assert_eq!(c.player.statuses.get(Status::Strength), 5, "这 5 点力量不再收回");
    }

    /// 神化(Apotheosis)只升现有四个牌堆,不升"之后新造出来的牌";新造牌升级是
    /// Master Reality(本作未实现).所以神化后喝发现类药水,候选与拿到的牌都不升级.
    /// 依据:反编译 ApotheosisAction(Actions.cpp:1005-1032 只遍历四个牌堆);
    /// 参考实现 colorless/effects.ts:214-225 apotheosisUpgradeAll 同口径,
    /// 新造牌升级另挂在 mastersReality 的 modifyCreatedCardUpgrades(watcher.ts:185-196).
    #[test]
    fn discovery_potion_after_apotheosis_is_not_upgraded() {
        let mut c = combat();
        c.hand = vec![
            crate::core::cards::card("apotheosis"),
            crate::core::cards::card("strike"),
            crate::core::cards::card("defend"),
        ];
        c.energy = 9;
        c.play_card(0, None).expect("神化打得出去");
        c.use_potion(def("attack_potion"), None);
        let ch = c.choice.as_ref().expect("攻击药剂要开屏");
        assert!(
            ch.offered.iter().all(|card| !card.upgraded),
            "神化不该升级新造的候选牌"
        );
        let pick = ch.offered[0].def.id;
        c.choose(0).unwrap();
        assert!(
            !c.hand.iter().find(|x| x.def.id == pick).unwrap().upgraded,
            "神化之后拿到的牌不该升级"
        );
    }
}
