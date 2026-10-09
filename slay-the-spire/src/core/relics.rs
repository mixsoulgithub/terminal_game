// 遗物的静态定义.遗物效果用一堆开关式字段描述,战斗/一局流程在对应时机读取.
// 数据表在文件末尾,顺序与参考实现的 bundle 插入顺序一致(起始/普通/罕见/稀有/Boss/
// 商店/事件),各档池子按 tier 筛出来再洗牌,所以顺序决定掉落身份.
use crate::core::card::Rarity;

/// 遗物档次.决定它进哪个池子,也决定"池子抽干"时的兜底链.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum RelicTier {
    Starter,
    Common,
    Uncommon,
    Rare,
    Boss,
    Shop,
    Event,
    /// 兜底遗物(Circlet)与语料里标为 special 的条目
    Special,
}

impl RelicTier {
    pub fn name(self) -> &'static str {
        match self {
            RelicTier::Starter => "Starter",
            RelicTier::Common => "Common",
            RelicTier::Uncommon => "Uncommon",
            RelicTier::Rare => "Rare",
            RelicTier::Boss => "Boss",
            RelicTier::Shop => "Shop",
            RelicTier::Event => "Event",
            RelicTier::Special => "Special",
        }
    }
}
/// 一件遗物的全部效果开关.零值表示没有任何效果(note 里写明原因).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct RelicFx {
    // ---- 拾取 ----
    pub max_hp: i32,
    pub heal: i32,
    pub gold: i32,
    pub full_heal: bool,
    pub potion_slots: i32,
    pub upgrade_random_attacks: i32,
    pub upgrade_random_skills: i32,
    pub upgrade_random_cards: i32,
    pub remove_cards: i32,
    pub transform_cards: i32,
    pub transform_strikes_defends: bool,
    pub duplicate_cards: i32,
    pub add_relics: i32,
    pub add_potions: i32,
    pub add_cards: i32,
    pub add_curse: bool,
    pub removes_starter_relic: bool,
    /// 小房子:拾取效果的掷点顺序/随机源很特别,由 Run::apply_relic_pickup 特判
    pub pickup_tiny_house: bool,
    // ---- 战斗开始 ----
    pub combat_start_block: i32,
    pub combat_start_energy: i32,
    pub combat_start_energy_per_turn: i32,
    pub combat_start_energy_elite_only: i32,
    pub combat_start_draw: i32,
    pub combat_start_heal: i32,
    pub combat_start_strength: i32,
    pub combat_start_strength_turn1: i32,
    pub combat_start_dexterity: i32,
    pub thorns: i32,
    pub combat_start_artifact: i32,
    pub combat_start_plated_armor: i32,
    pub combat_start_vigor: i32,
    pub combat_start_buffer: i32,
    pub combat_start_confused: bool,
    pub combat_start_self_weak: i32,
    pub combat_start_enemy_strength: i32,
    pub combat_start_enemy_vulnerable: i32,
    pub combat_start_enemy_weak: i32,
    pub combat_start_strength_per_curse: i32,
    pub combat_start_strength_elite: i32,
    pub combat_start_wounds: i32,
    pub elite_hp_reduction_pct: i32,
    pub boss_combat_heal: i32,
    pub add_random_power_card: bool,
    // ---- 回合开始/结束 ----
    pub draw_per_turn: i32,
    pub energy_every_3_turns: i32,
    pub energy_turn1_if_rested: i32,
    pub energy_if_no_attack_last_turn: i32,
    pub block_turn2: i32,
    pub block_turn3: i32,
    pub block_if_no_block_at_end: i32,
    pub block_per_card_in_hand_at_end: i32,
    pub damage_all_turn_start: i32,
    pub damage_all_turn7: i32,
    pub intangible_every_6_turns: i32,
    pub brimstone_self: i32,
    pub brimstone_enemy: i32,
    pub draw_next_turn_if_low_play: i32,
    pub upgrade_random_hand_at_turn_start: bool,
    pub block_loss_cap: i32,
    pub retain_hand: bool,
    pub conserve_energy: bool,
    pub gain_energy_first_discard_per_turn: i32,
    pub gambling_chip: bool,
    // ---- 打出的牌 ----
    pub heal_on_power_card: i32,
    pub energy_per_10_attacks: i32,
    pub draw_per_10_cards: i32,
    pub double_damage_per_10_attacks: bool,
    pub dexterity_per_3_attacks: i32,
    pub strength_per_3_attacks: i32,
    pub block_per_3_attacks: i32,
    pub damage_all_per_3_skills: i32,
    pub zero_hand_card_on_power: bool,
    pub card_play_cap: i32,
    pub double_first_big_attack: bool,
    pub clear_debuffs_on_all_types: bool,
    pub strike_damage_bonus: i32,
    pub zero_cost_attack_bonus: i32,
    pub small_attack_boost_to: i32,
    pub small_attack_reduce_to: i32,
    // ---- 掉血 ----
    pub draw_on_first_hp_loss: i32,
    pub draw_on_hp_loss: i32,
    pub block_next_turn_on_hp_loss: i32,
    pub hp_loss_reduction: i32,
    pub strength_when_bloodied: i32,
    pub combat_heal_pct: i32,
    // ---- 消耗/弃牌/洗牌 ----
    pub damage_all_on_exhaust: i32,
    pub card_on_exhaust: i32,
    pub damage_random_on_discard: i32,
    pub block_on_discard: i32,
    pub block_on_shuffle: i32,
    pub energy_per_3_shuffles: i32,
    pub draw_on_empty_hand: bool,
    // ---- 击杀/战后 ----
    pub energy_on_kill: i32,
    pub draw_on_kill: i32,
    pub post_combat_heal: i32,
    pub post_combat_heal_if_below_half: i32,
    pub max_hp_on_victory: i32,
    // ---- 伤害修正 ----
    pub vulnerable_damage_pct: i32,
    pub weak_damage_pct: i32,
    pub vulnerable_taken_pct: i32,
    pub immune_weak: bool,
    pub immune_frail: bool,
    // ---- 一局流程 ----
    pub gold_per_floor: i32,
    pub gold_on_card_add: i32,
    pub gold_on_unknown_room: i32,
    pub heal_on_shop_enter: i32,
    pub rest_heal_bonus: i32,
    pub rest_heal_per_5_deck: i32,
    pub heal_on_gold_gain: i32,
    pub heal_on_potion_use: i32,
    pub max_hp_on_card_skip: i32,
    pub gold_reward_pct: i32,
    pub shop_discount_pct: i32,
    pub removal_cost_fixed: i32,
    pub no_heal: bool,
    pub no_gold: bool,
    pub no_potions: bool,
    pub no_rest: bool,
    pub no_smith: bool,
    pub potions_always: bool,
    pub rare_card_chance_x3: bool,
    pub card_reward_bonus: i32,
    pub extra_elite_relic: i32,
    pub extra_chest_relic_charges: i32,
    pub curse_on_chest: bool,
    pub chest_empty_charges: i32,
    pub curse_negate: i32,
    pub max_hp_on_curse: i32,
    pub no_normal_combat_in_unknown: bool,
    pub treasure_every_4_unknown: bool,
    pub neow_lament_combats: i32,
    pub egg_attack_upgrade: bool,
    pub egg_skill_upgrade: bool,
    pub egg_power_upgrade: bool,
    // ---- 营火 / 握手 / 发现式选择 / 战斗钩子 / 地图与显示 / 药水 ----
    /// 休息后可以再挑一张牌进牌组(梦中情网)
    pub rest_card_reward: bool,
    /// 营火举铁的次数上限(吉利亚)
    pub rest_lift_max: i32,
    /// 营火可以删牌(和平烟斗)
    pub rest_toke: bool,
    /// 营火可以挖遗物(铲子)
    pub rest_dig: bool,
    /// 拾取时封装一张攻击牌,战斗开局进手(瓶装火焰)
    pub bottle_attack: bool,
    /// 同上,技能牌(瓶装闪电)
    pub bottle_skill: bool,
    /// 同上,能力牌(瓶装龙卷风)
    pub bottle_power: bool,
    /// 战斗第一回合亮出几张无色牌挑一张进手(工具箱)
    pub combat_start_colorless_pick: i32,
    /// 回合结束亮出几张随机牌,挑一张洗进抽牌堆(尼尔瑞的抄本)
    pub end_turn_shuffle_pick: i32,
    /// 拾取时连开几次卡牌三选一(浑天仪)
    pub pickup_card_picks: i32,
    /// 普通战斗多几组卡牌奖励(祈祷轮)
    pub extra_card_reward_group: i32,
    /// 卡牌奖励混入无色与其它颜色(棱彩碎片)
    pub prismatic_rewards: bool,
    /// 不可打出的诅咒可以打出,打出时掉几点血(蓝蜡烛)
    pub playable_curses_hp: i32,
    /// 不可打出的状态牌可以打出(医疗包)
    pub playable_statuses: bool,
    /// 给敌人上易伤时附带几层虚弱(冠军腰带)
    pub weak_on_vulnerable: i32,
    /// 击破敌人格挡时上几层易伤(手钻)
    pub vulnerable_on_block_break: i32,
    /// 致命伤时按最大生命的百分之几回血,每场一次(蜥蜴尾巴)
    pub death_save_pct: i32,
    /// X 费牌的 X 额外加多少(化学 X)
    pub x_cost_bonus: i32,
    /// 被打出时消耗的牌改为进弃牌堆的概率(奇异勺)
    pub exhaust_to_discard_pct: i32,
    /// 地图上无视路径的次数(羽翼靴)
    pub map_wing_charges: i32,
    /// 抽牌堆按抽取顺序显示(冰冻之眼)
    pub draw_pile_in_order: bool,
    /// 药水强度额外加百分之几(神圣树皮)
    pub potion_potency_pct: i32,
}

impl RelicFx {
    /// 静态初始化用的全零值:const 上下文里不能用 Default::default()
    pub const ZERO: RelicFx = RelicFx {
        max_hp: 0,
        heal: 0,
        gold: 0,
        full_heal: false,
        potion_slots: 0,
        upgrade_random_attacks: 0,
        upgrade_random_skills: 0,
        upgrade_random_cards: 0,
        remove_cards: 0,
        transform_cards: 0,
        transform_strikes_defends: false,
        duplicate_cards: 0,
        add_relics: 0,
        add_potions: 0,
        add_cards: 0,
        add_curse: false,
        removes_starter_relic: false,
        pickup_tiny_house: false,
        combat_start_block: 0,
        combat_start_energy: 0,
        combat_start_energy_per_turn: 0,
        combat_start_energy_elite_only: 0,
        combat_start_draw: 0,
        combat_start_heal: 0,
        combat_start_strength: 0,
        combat_start_strength_turn1: 0,
        combat_start_dexterity: 0,
        thorns: 0,
        combat_start_artifact: 0,
        combat_start_plated_armor: 0,
        combat_start_vigor: 0,
        combat_start_buffer: 0,
        combat_start_confused: false,
        combat_start_self_weak: 0,
        combat_start_enemy_strength: 0,
        combat_start_enemy_vulnerable: 0,
        combat_start_enemy_weak: 0,
        combat_start_strength_per_curse: 0,
        combat_start_strength_elite: 0,
        combat_start_wounds: 0,
        elite_hp_reduction_pct: 0,
        boss_combat_heal: 0,
        add_random_power_card: false,
        draw_per_turn: 0,
        energy_every_3_turns: 0,
        energy_turn1_if_rested: 0,
        energy_if_no_attack_last_turn: 0,
        block_turn2: 0,
        block_turn3: 0,
        block_if_no_block_at_end: 0,
        block_per_card_in_hand_at_end: 0,
        damage_all_turn_start: 0,
        damage_all_turn7: 0,
        intangible_every_6_turns: 0,
        brimstone_self: 0,
        brimstone_enemy: 0,
        draw_next_turn_if_low_play: 0,
        upgrade_random_hand_at_turn_start: false,
        block_loss_cap: 0,
        retain_hand: false,
        conserve_energy: false,
        gain_energy_first_discard_per_turn: 0,
        gambling_chip: false,
        heal_on_power_card: 0,
        energy_per_10_attacks: 0,
        draw_per_10_cards: 0,
        double_damage_per_10_attacks: false,
        dexterity_per_3_attacks: 0,
        strength_per_3_attacks: 0,
        block_per_3_attacks: 0,
        damage_all_per_3_skills: 0,
        zero_hand_card_on_power: false,
        card_play_cap: 0,
        double_first_big_attack: false,
        clear_debuffs_on_all_types: false,
        strike_damage_bonus: 0,
        zero_cost_attack_bonus: 0,
        small_attack_boost_to: 0,
        small_attack_reduce_to: 0,
        draw_on_first_hp_loss: 0,
        draw_on_hp_loss: 0,
        block_next_turn_on_hp_loss: 0,
        hp_loss_reduction: 0,
        strength_when_bloodied: 0,
        combat_heal_pct: 0,
        damage_all_on_exhaust: 0,
        card_on_exhaust: 0,
        damage_random_on_discard: 0,
        block_on_discard: 0,
        block_on_shuffle: 0,
        energy_per_3_shuffles: 0,
        draw_on_empty_hand: false,
        energy_on_kill: 0,
        draw_on_kill: 0,
        post_combat_heal: 0,
        post_combat_heal_if_below_half: 0,
        max_hp_on_victory: 0,
        vulnerable_damage_pct: 0,
        weak_damage_pct: 0,
        vulnerable_taken_pct: 0,
        immune_weak: false,
        immune_frail: false,
        gold_per_floor: 0,
        gold_on_card_add: 0,
        gold_on_unknown_room: 0,
        heal_on_shop_enter: 0,
        rest_heal_bonus: 0,
        rest_heal_per_5_deck: 0,
        heal_on_gold_gain: 0,
        heal_on_potion_use: 0,
        max_hp_on_card_skip: 0,
        gold_reward_pct: 0,
        shop_discount_pct: 0,
        removal_cost_fixed: 0,
        no_heal: false,
        no_gold: false,
        no_potions: false,
        no_rest: false,
        no_smith: false,
        potions_always: false,
        rare_card_chance_x3: false,
        card_reward_bonus: 0,
        extra_elite_relic: 0,
        extra_chest_relic_charges: 0,
        curse_on_chest: false,
        chest_empty_charges: 0,
        curse_negate: 0,
        max_hp_on_curse: 0,
        no_normal_combat_in_unknown: false,
        treasure_every_4_unknown: false,
        neow_lament_combats: 0,
        egg_attack_upgrade: false,
        egg_skill_upgrade: false,
        egg_power_upgrade: false,
        rest_card_reward: false,
        rest_lift_max: 0,
        rest_toke: false,
        rest_dig: false,
        bottle_attack: false,
        bottle_skill: false,
        bottle_power: false,
        combat_start_colorless_pick: 0,
        end_turn_shuffle_pick: 0,
        pickup_card_picks: 0,
        extra_card_reward_group: 0,
        prismatic_rewards: false,
        playable_curses_hp: 0,
        playable_statuses: false,
        weak_on_vulnerable: 0,
        vulnerable_on_block_break: 0,
        death_save_pct: 0,
        x_cost_bonus: 0,
        exhaust_to_discard_pct: 0,
        map_wing_charges: 0,
        draw_pile_in_order: false,
        potion_potency_pct: 0,
    };

    /// 这件遗物在本作里没有任何效果(fx 全零)
    #[cfg(test)]
    pub fn is_noop(&self) -> bool {
        *self == RelicFx::ZERO
    }
}

#[derive(Debug)]
pub struct RelicDef {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub tier: RelicTier,
    /// shared / red / green / blue / purple:哪些职业的池子里会出现它
    pub pool: &'static str,
    pub fx: RelicFx,
    /// fx 全零时写明原因;有 fx 的遗物这里是空串
    pub note: &'static str,
}

impl RelicDef {
    /// 展示用稀有度(颜色).Boss/商店/事件档都归到 Special
    pub fn rarity(&self) -> Rarity {
        match self.tier {
            RelicTier::Starter => Rarity::Basic,
            RelicTier::Common => Rarity::Common,
            RelicTier::Uncommon => Rarity::Uncommon,
            RelicTier::Rare => Rarity::Rare,
            RelicTier::Boss | RelicTier::Shop | RelicTier::Event | RelicTier::Special => {
                Rarity::Special
            }
        }
    }
}

/// 全 181 件,顺序 = 参考实现 bundle 的插入顺序
pub static RELICS: &[RelicDef] = &[
    RelicDef {
        id: "burning_blood",
        name: "Burning Blood",
        desc: "At the end of combat, heal 6 HP.",
        tier: RelicTier::Starter,
        pool: "red",
        fx: RelicFx {
            post_combat_heal: 6,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ring_of_the_snake",
        name: "Ring of the Snake",
        desc: "At the start of each combat, draw 2 additional cards.",
        tier: RelicTier::Starter,
        pool: "green",
        fx: RelicFx {
            combat_start_draw: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "cracked_core",
        name: "Cracked Core",
        desc: "At the start of each combat, Channel 1 Lightning.",
        tier: RelicTier::Starter,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职起始遗物)",
    },
    RelicDef {
        id: "pure_water",
        name: "Pure Water",
        desc: "At the start of each combat, add 1 Miracle into your hand.",
        tier: RelicTier::Starter,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "Miracle 不在本作牌池(紫职起始遗物)",
    },
    RelicDef {
        id: "akabeko",
        name: "Akabeko",
        desc: "Your first Attack each combat deals 8 additional damage",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_vigor: 8,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "art_of_war",
        name: "Art of War",
        desc: "If you do not play any Attacks during your turn, gain an additional Energy next turn.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            energy_if_no_attack_last_turn: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "anchor",
        name: "Anchor",
        desc: "Start each combat with 10 Block.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_block: 10,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ancient_tea_set",
        name: "Ancient Tea Set",
        desc: "Whenever you enter a Rest Site, start the next combat with 2 extra Energy.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            energy_turn1_if_rested: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bag_of_marbles",
        name: "Bag of Marbles",
        desc: "At the start of each combat, apply 1 Vulnerable to ALL enemies.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_enemy_vulnerable: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bag_of_preparation",
        name: "Bag of Preparation",
        desc: "At the start of each combat, draw 2 additional cards.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_draw: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "blood_vial",
        name: "Blood Vial",
        desc: "At the start of each combat, heal 2 HP.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_heal: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bronze_scales",
        name: "Bronze Scales",
        desc: "Start each combat with 3 Thorns.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            thorns: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "centennial_puzzle",
        name: "Centennial Puzzle",
        desc: "The first time you lose HP each combat, draw 3 cards.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            draw_on_first_hp_loss: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ceramic_fish",
        name: "Ceramic Fish",
        desc: "Whenever you add a card to your deck, gain 9 Gold.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            gold_on_card_add: 9,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "damaru",
        name: "Damaru",
        desc: "At the start of your turn, gain 1 Mantra.",
        tier: RelicTier::Common,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "Mantra 机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "data_disk",
        name: "Data Disk",
        desc: "Start each combat with 1 Focus.",
        tier: RelicTier::Common,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "集中/充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "dream_catcher",
        name: "Dream Catcher",
        desc: "Whenever you Rest, you may add a card to your deck.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            rest_card_reward: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "happy_flower",
        name: "Happy Flower",
        desc: "Every 3 turns, gain 1 Energy.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            energy_every_3_turns: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "juzu_bracelet",
        name: "Juzu Bracelet",
        desc: "Regular enemy combats are no longer encountered in ? rooms.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            no_normal_combat_in_unknown: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "lantern",
        name: "Lantern",
        desc: "Gain 1 Energy on the first turn of each combat.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "maw_bank",
        name: "Maw Bank",
        desc: "Whenever you climb a floor, gain 12 Gold. No longer works when you spend any Gold at a shop.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            gold_per_floor: 12,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "meal_ticket",
        name: "Meal Ticket",
        desc: "Whenever you enter a shop, heal 15 HP.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            heal_on_shop_enter: 15,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "nunchaku",
        name: "Nunchaku",
        desc: "Every time you play 10 Attacks, gain 1 Energy.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            energy_per_10_attacks: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "oddly_smooth_stone",
        name: "Oddly Smooth Stone",
        desc: "At the start of each combat, gain 1 Dexterity.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_dexterity: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "omamori",
        name: "Omamori",
        desc: "Negate the next 2 Curses you obtain.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            curse_negate: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "orichalcum",
        name: "Orichalcum",
        desc: "If you end your turn without Block, gain 6 Block.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            block_if_no_block_at_end: 6,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "pen_nib",
        name: "Pen Nib",
        desc: "Every 10th Attack you play deals double damage.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            double_damage_per_10_attacks: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "preserved_insect",
        name: "Preserved Insect",
        desc: "Enemies in Elite combats have 25% less HP.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            elite_hp_reduction_pct: 25,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "potion_belt",
        name: "Potion Belt",
        desc: "Upon pickup, gain 2 Potion slots.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            potion_slots: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "regal_pillow",
        name: "Regal Pillow",
        desc: "Whenever you Rest, heal an additional 15 HP.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            rest_heal_bonus: 15,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "red_skull",
        name: "Red Skull",
        desc: "While your HP is at or below 50%, you have 3 additional Strength.",
        tier: RelicTier::Common,
        pool: "red",
        fx: RelicFx {
            strength_when_bloodied: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "smiling_mask",
        name: "Smiling Mask",
        desc: "The merchant's card removal service now always costs 50 Gold.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            removal_cost_fixed: 50,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "snecko_skull",
        name: "Snecko Skull",
        desc: "Whenever you apply Poison, apply an additional 1 Poison.",
        tier: RelicTier::Common,
        pool: "green",
        fx: RelicFx::ZERO,
        note: "中毒机制本作没有(绿职专属)",
    },
    RelicDef {
        id: "strawberry",
        name: "Strawberry",
        desc: "Upon pickup, raise your Max HP by 7.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            max_hp: 7,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "the_boot",
        name: "The Boot",
        desc: "Whenever you would deal 4 or less unblocked Attack damage, increase it to 5.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            small_attack_boost_to: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "tiny_chest",
        name: "Tiny Chest",
        desc: "Every 4th ? room is a Treasure room.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            treasure_every_4_unknown: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "toy_ornithopter",
        name: "Toy Ornithopter",
        desc: "Whenever you use a potion, heal 5 HP.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            heal_on_potion_use: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "vajra",
        name: "Vajra",
        desc: "At the start of each combat, gain 1 Strength.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            combat_start_strength: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "war_paint",
        name: "War Paint",
        desc: "Upon pick up, Upgrade 2 random Skills.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            upgrade_random_skills: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "whetstone",
        name: "Whetstone",
        desc: "Upon pickup, Upgrade 2 random Attacks.",
        tier: RelicTier::Common,
        pool: "shared",
        fx: RelicFx {
            upgrade_random_attacks: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "blue_candle",
        name: "Blue Candle",
        desc: "Unplayable Curse cards can now be played. Whenever you play a Curse, lose 1 HP and Exhaust it.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            playable_curses_hp: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bottled_flame",
        name: "Bottled Flame",
        desc: "Upon pickup, choose an Attack card. At the start of each combat, this card will be in your hand.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            bottle_attack: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bottled_lightning",
        name: "Bottled Lightning",
        desc: "Upon pickup, choose a Skill card. At the start of each combat, this card will be in your hand.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            bottle_skill: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bottled_tornado",
        name: "Bottled Tornado",
        desc: "Upon pickup, choose a Power card. At the start of each combat, this card will be in your hand.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            bottle_power: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "darkstone_periapt",
        name: "Darkstone Periapt",
        desc: "Whenever you obtain a Curse, increase your Max HP by 6",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            max_hp_on_curse: 6,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "duality",
        name: "Duality",
        desc: "Whenever you play an Attack, gain 1 temporary Dexterity.",
        tier: RelicTier::Uncommon,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "架势机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "gremlin_horn",
        name: "Gremlin Horn",
        desc: "Whenever an enemy dies, gain 1 Energy and draw 1 card.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            energy_on_kill: 1,
            draw_on_kill: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "gold_plated_cables",
        name: "Gold-Plated Cables",
        desc: "Your rightmost Orb triggers its passive an additional time.",
        tier: RelicTier::Uncommon,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "horn_cleat",
        name: "Horn Cleat",
        desc: "At the start of your 2nd turn, gain 14 Block.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            block_turn2: 14,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ink_bottle",
        name: "Ink Bottle",
        desc: "Whenever you play 10 cards, draw 1 card.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            draw_per_10_cards: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "kunai",
        name: "Kunai",
        desc: "Every time you play 3 Attacks in a single turn, gain 1 Dexterity",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            dexterity_per_3_attacks: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "letter_opener",
        name: "Letter Opener",
        desc: "Every time you play 3 Skills in a single turn, deal 5 damage to ALL enemies.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            damage_all_per_3_skills: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "meat_on_the_bone",
        name: "Meat on the Bone",
        desc: "If your HP is at or below 50% at the end of combat, heal 12 HP.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            post_combat_heal_if_below_half: 12,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "mercury_hourglass",
        name: "Mercury Hourglass",
        desc: "At the start of your turn, deal 3 damage to ALL enemies.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            damage_all_turn_start: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "mummified_hand",
        name: "Mummified Hand",
        desc: "Whenever you play a Power card, a random card in your hand costs 0 that turn.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            zero_hand_card_on_power: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ninja_scroll",
        name: "Ninja Scroll",
        desc: "At the start each combat, add 3 Shivs into your hand.",
        tier: RelicTier::Uncommon,
        pool: "green",
        fx: RelicFx::ZERO,
        note: "Shiv 不在本作牌池(绿职专属)",
    },
    RelicDef {
        id: "ornamental_fan",
        name: "Ornamental Fan",
        desc: "Every time you play 3 Attacks in a single turn, gain 4 Block.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            block_per_3_attacks: 4,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "paper_krane",
        name: "Paper Krane",
        desc: "Enemies with Weak deal 40% less damage rather than 25%.",
        tier: RelicTier::Uncommon,
        pool: "green",
        fx: RelicFx {
            weak_damage_pct: 60,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "paper_phrog",
        name: "Paper Phrog",
        desc: "Enemies with Vulnerable take 75% more damage rather than 50%.",
        tier: RelicTier::Uncommon,
        pool: "red",
        fx: RelicFx {
            vulnerable_damage_pct: 175,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "pantograph",
        name: "Pantograph",
        desc: "At the start of Boss combats, heal 25 HP.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            boss_combat_heal: 25,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "self_forming_clay",
        name: "Self-Forming Clay",
        desc: "Whenever you lose HP, gain 3 Block next turn.",
        tier: RelicTier::Uncommon,
        pool: "red",
        fx: RelicFx {
            block_next_turn_on_hp_loss: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "shuriken",
        name: "Shuriken",
        desc: "Every time you play 3 Attacks in a single turn, gain 1 Strength.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            strength_per_3_attacks: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "strike_dummy",
        name: "Strike Dummy",
        desc: "Cards containing \"Strike\" deal 3 additional damage.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            strike_damage_bonus: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "sundial",
        name: "Sundial",
        desc: "Every 3 times you shuffle your draw pile, gain 2 Energy.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            energy_per_3_shuffles: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "symbiotic_virus",
        name: "Symbiotic Virus",
        desc: "At the start of each combat, Channel 1 Dark.",
        tier: RelicTier::Uncommon,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "teardrop_locket",
        name: "Teardrop Locket",
        desc: "Start each combat in Calm.",
        tier: RelicTier::Uncommon,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "架势机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "eternal_feather",
        name: "Eternal Feather",
        desc: "For every 5 cards in your deck, heal 3 HP whenever you enter a Rest Site.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            rest_heal_per_5_deck: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "frozen_egg",
        name: "Frozen Egg",
        desc: "Whenever you add a Power card to your deck, Upgrade it.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            egg_power_upgrade: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "matryoshka",
        name: "Matryoshka",
        desc: "The next 2 non-boss chests you open contain 2 Relics.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            extra_chest_relic_charges: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "molten_egg",
        name: "Molten Egg",
        desc: "Whenever you add an Attack card to your deck, Upgrade it.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            egg_attack_upgrade: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "pear",
        name: "Pear",
        desc: "Upon pickup, raise your Max HP by 10.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            max_hp: 10,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "question_card",
        name: "Question Card",
        desc: "Future card rewards have 1 additional card to choose from.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            card_reward_bonus: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "singing_bowl",
        name: "Singing Bowl",
        desc: "When adding cards to your deck, you may raise your Max HP by 2 instead.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            max_hp_on_card_skip: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "the_courier",
        name: "The Courier",
        desc: "The Merchant restocks cards, relics, and potions. All prices are reduced by 20%.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            shop_discount_pct: 20,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "toxic_egg",
        name: "Toxic Egg",
        desc: "Whenever you add a Skill card to your deck, Upgrade it.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            egg_skill_upgrade: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "white_beast_statue",
        name: "White Beast Statue",
        desc: "Potions always appear in combat rewards.",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx {
            potions_always: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "discerning_monocle",
        name: "Discerning Monocle",
        desc: "Merchant prices are reduced by 20%",
        tier: RelicTier::Uncommon,
        pool: "shared",
        fx: RelicFx::ZERO,
        note: "语料标为不可获得(unobtainable),参考实现也不给效果",
    },
    RelicDef {
        id: "bird_faced_urn",
        name: "Bird-Faced Urn",
        desc: "Whenever you play a Power card, heal 2 HP.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            heal_on_power_card: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "calipers",
        name: "Calipers",
        desc: "At the start of your turn, lose 15 Block rather than all of your Block.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            block_loss_cap: 15,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "captains_wheel",
        name: "Captain's Wheel",
        desc: "At the start of your 3rd turn, gain 18 Block.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            block_turn3: 18,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "champion_belt",
        name: "Champion Belt",
        desc: "Whenever you apply Vulnerable, also apply 1 Weak.",
        tier: RelicTier::Rare,
        pool: "red",
        fx: RelicFx {
            weak_on_vulnerable: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "charons_ashes",
        name: "Charon's Ashes",
        desc: "Whenever you Exhaust a card, deal 3 damage to ALL enemies.",
        tier: RelicTier::Rare,
        pool: "red",
        fx: RelicFx {
            damage_all_on_exhaust: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "cloak_clasp",
        name: "Cloak Clasp",
        desc: "At the end of your turn, gain 1 Block for each card in your hand.",
        tier: RelicTier::Rare,
        pool: "purple",
        fx: RelicFx {
            block_per_card_in_hand_at_end: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "dead_branch",
        name: "Dead Branch",
        desc: "Whenever you Exhaust a card, add a random card to your hand.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            card_on_exhaust: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "du_vu_doll",
        name: "Du-Vu Doll",
        desc: "For each Curse in your deck, start each combat with 1 Strength.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            combat_start_strength_per_curse: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "emotion_chip",
        name: "Emotion Chip",
        desc: "If you lost HP during the previous turn, trigger the passive ability of all Orbs at the start of your turn.",
        tier: RelicTier::Rare,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "fossilized_helix",
        name: "Fossilized Helix",
        desc: "Prevent the first time you would lose HP in combat.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            combat_start_buffer: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "gambling_chip",
        name: "Gambling Chip",
        desc: "At the start of each combat, discard any number of cards, then draw that many cards.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            gambling_chip: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ginger",
        name: "Ginger",
        desc: "You can no longer become Weakened.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            immune_weak: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "golden_eye",
        name: "Golden Eye",
        desc: "Whenever you Scry, Scry 2 additional cards.",
        tier: RelicTier::Rare,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "观星机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "ice_cream",
        name: "Ice Cream",
        desc: "Energy is now conserved between turns.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            conserve_energy: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "incense_burner",
        name: "Incense Burner",
        desc: "Every 6 turns, gain 1 Intangible.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            intangible_every_6_turns: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "lizard_tail",
        name: "Lizard Tail",
        desc: "When you would die, heal to 50% of your Max HP instead (works once).",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            death_save_pct: 50,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "magic_flower",
        name: "Magic Flower",
        desc: "Healing is 50% more effective during combat.",
        tier: RelicTier::Rare,
        pool: "red",
        fx: RelicFx {
            combat_heal_pct: 150,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "mango",
        name: "Mango",
        desc: "Upon pickup, raise your Max HP by 14.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            max_hp: 14,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "old_coin",
        name: "Old Coin",
        desc: "Upon pickup, gain 300 Gold.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            gold: 300,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "peace_pipe",
        name: "Peace Pipe",
        desc: "You can now remove cards from your deck at Rest Sites.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            rest_toke: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "pocketwatch",
        name: "Pocketwatch",
        desc: "Whenever you play 3 or less cards during your turn, draw 3 additional cards at the start of your next turn.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            draw_next_turn_if_low_play: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "prayer_wheel",
        name: "Prayer Wheel",
        desc: "Normal enemies drop an additional card reward.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            extra_card_reward_group: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "shovel",
        name: "Shovel",
        desc: "You can now Dig for relics at Rest Sites.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            rest_dig: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "stone_calendar",
        name: "Stone Calendar",
        desc: "At the end of turn 7, deal 52 damage to ALL enemies.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            damage_all_turn7: 52,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "the_specimen",
        name: "The Specimen",
        desc: "Whenever an enemy dies, transfer any Poison it has to a random enemy.",
        tier: RelicTier::Rare,
        pool: "green",
        fx: RelicFx::ZERO,
        note: "中毒机制本作没有(绿职专属)",
    },
    RelicDef {
        id: "thread_and_needle",
        name: "Thread and Needle",
        desc: "At the start of each combat, gain 4 Plated Armor.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            combat_start_plated_armor: 4,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "tingsha",
        name: "Tingsha",
        desc: "Whenever you discard a card during your turn, deal 3 damage to a random enemy.",
        tier: RelicTier::Rare,
        pool: "green",
        fx: RelicFx {
            damage_random_on_discard: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "torii",
        name: "Torii",
        desc: "Whenever you would receive 5 or less unblocked Attack damage, reduce it to 1.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            small_attack_reduce_to: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "tough_bandages",
        name: "Tough Bandages",
        desc: "Whenever you discard a card during your turn, gain 3 Block.",
        tier: RelicTier::Rare,
        pool: "green",
        fx: RelicFx {
            block_on_discard: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "tungsten_rod",
        name: "Tungsten Rod",
        desc: "Whenever you would lose HP, lose 1 less.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            hp_loss_reduction: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "turnip",
        name: "Turnip",
        desc: "You can no longer become Frail.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            immune_frail: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "unceasing_top",
        name: "Unceasing Top",
        desc: "Whenever you have no cards in hand during your turn, draw a card.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            draw_on_empty_hand: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "girya",
        name: "Girya",
        desc: "You can now gain Strength at Rest Sites (up to 3 times).",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            rest_lift_max: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "wing_boots",
        name: "Wing Boots",
        desc: "You may ignore paths when choosing the next room to travel to 3 times.",
        tier: RelicTier::Rare,
        pool: "shared",
        fx: RelicFx {
            map_wing_charges: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "black_blood",
        name: "Black Blood",
        desc: "Replaces Burning Blood. At the end of combat, heal 12 HP.",
        tier: RelicTier::Boss,
        pool: "red",
        fx: RelicFx {
            removes_starter_relic: true,
            post_combat_heal: 12,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "busted_crown",
        name: "Busted Crown",
        desc: "Gain 1 Energy at the start of your turn. Future card rewards have 2 less cards to choose from.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            card_reward_bonus: -2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "coffee_dripper",
        name: "Coffee Dripper",
        desc: "Gain 1 Energy at the start of your turn. You can no longer Rest at Rest Sites.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            no_rest: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "cursed_key",
        name: "Cursed Key",
        desc: "Gain 1 Energy at the start of your turn. Whenever you open a non-Boss chest, obtain a Curse.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            curse_on_chest: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ectoplasm",
        name: "Ectoplasm",
        desc: "Gain 1 Energy at the start of your turn. You can no longer gain Gold.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            no_gold: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "frozen_core",
        name: "Frozen Core",
        desc: "Replaces Cracked Core. If you end your turn with any empty Orb slots, Channel 1 Frost.",
        tier: RelicTier::Boss,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "fusion_hammer",
        name: "Fusion Hammer",
        desc: "Gain 1 Energy at the start of your turn. You can no longer Smith at Rest Sites.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            no_smith: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "holy_water",
        name: "Holy Water",
        desc: "Replaces Pure Water. At the start of each combat, add 3 Miracles into your hand.",
        tier: RelicTier::Boss,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "Miracle 不在本作牌池(紫职专属)",
    },
    RelicDef {
        id: "hovering_kite",
        name: "Hovering Kite",
        desc: "The first time you discard a card each turn, gain 1 Energy.",
        tier: RelicTier::Boss,
        pool: "green",
        fx: RelicFx {
            gain_energy_first_discard_per_turn: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "inserter",
        name: "Inserter",
        desc: "Every 2 turns, gain 1 Orb slot.",
        tier: RelicTier::Boss,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "mark_of_pain",
        name: "Mark of Pain",
        desc: "Gain 1 Energy at the start of your turn. At the start of combat, shuffle 2 Wounds into your draw pile.",
        tier: RelicTier::Boss,
        pool: "red",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            combat_start_wounds: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "nuclear_battery",
        name: "Nuclear Battery",
        desc: "At the start of each combat, Channel 1 Plasma.",
        tier: RelicTier::Boss,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "philosophers_stone",
        name: "Philosopher's Stone",
        desc: "Gain 1 Energy at the start of your turn. ALL enemies start combat with 1 Strength.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            combat_start_enemy_strength: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ring_of_the_serpent",
        name: "Ring of the Serpent",
        desc: "Replaces Ring of the Snake. At the start of your turn, draw 1 additional card.",
        tier: RelicTier::Boss,
        pool: "green",
        fx: RelicFx {
            draw_per_turn: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "runic_dome",
        name: "Runic Dome",
        desc: "Gain 1 Energy at the start of your turn. You can no longer see enemy intents.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "runic_cube",
        name: "Runic Cube",
        desc: "Whenever you lose HP, draw 1 card.",
        tier: RelicTier::Boss,
        pool: "red",
        fx: RelicFx {
            draw_on_hp_loss: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "runic_pyramid",
        name: "Runic Pyramid",
        desc: "At the end of your turn, you no longer discard your hand.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            retain_hand: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "sacred_bark",
        name: "Sacred Bark",
        desc: "Double the effectiveness of potions.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            potion_potency_pct: 100,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "slavers_collar",
        name: "Slaver's Collar",
        desc: "During Boss and Elite combats, gain 1 Energy at the start of your turn.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_elite_only: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "snecko_eye",
        name: "Snecko Eye",
        desc: "At the start of your turn, draw 2 additional cards. Start each combat Confused.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_confused: true,
            draw_per_turn: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "sozu",
        name: "Sozu",
        desc: "Gain 1 Energy at the start of your turn. You can no longer obtain potions.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            no_potions: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "velvet_choker",
        name: "Velvet Choker",
        desc: "Gain 1 Energy at the start of your turn. You cannot play more than 6 cards per turn.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            combat_start_energy_per_turn: 1,
            card_play_cap: 6,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "violet_lotus",
        name: "Violet Lotus",
        desc: "Whenever you exit Calm, gain an additional Energy.",
        tier: RelicTier::Boss,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "架势机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "wrist_blade",
        name: "Wrist Blade",
        desc: "Attacks that cost 0 deal 4 additional damage.",
        tier: RelicTier::Boss,
        pool: "green",
        fx: RelicFx {
            zero_cost_attack_bonus: 4,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "astrolabe",
        name: "Astrolabe",
        desc: "Upon pickup, Transform 3 cards, then Upgrade them.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            transform_cards: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "black_star",
        name: "Black Star",
        desc: "Elites now drop an additional relic when defeated.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            extra_elite_relic: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "calling_bell",
        name: "Calling Bell",
        desc: "Upon pickup, obtain a unique Curse and 3 relics.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            add_relics: 3,
            add_curse: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "empty_cage",
        name: "Empty Cage",
        desc: "Upon pickup, remove 2 cards from your deck.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            remove_cards: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "pandoras_box",
        name: "Pandora's Box",
        desc: "Upon pickup, Transform all Strike and Defend cards.",
        tier: RelicTier::Boss,
        pool: "shared",
        fx: RelicFx {
            transform_strikes_defends: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "tiny_house",
        name: "Tiny House",
        desc: "Upon pickup, obtain 1 Potion. Gain 50 Gold. Raise your Max HP by 5. Obtain 1 card. Upgrade 1 random card.",
        tier: RelicTier::Boss,
        pool: "shared",
        // 效果在 Run::apply_relic_pickup 里特判(掷点顺序与药水池走 miscRng),不开通用 fx
        fx: RelicFx {
            pickup_tiny_house: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "brimstone",
        name: "Brimstone",
        desc: "At the start of your turn, gain 2 Strength and ALL enemies gain 1 Strength.",
        tier: RelicTier::Shop,
        pool: "red",
        fx: RelicFx {
            brimstone_self: 2,
            brimstone_enemy: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "chemical_x",
        name: "Chemical X",
        desc: "The effects of your cost X cards are increased by 2.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            x_cost_bonus: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "clockwork_souvenir",
        name: "Clockwork Souvenir",
        desc: "Start each combat with 1 Artifact.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            combat_start_artifact: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "frozen_eye",
        name: "Frozen Eye",
        desc: "When viewing your Draw Pile, the cards are now shown in order.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            draw_pile_in_order: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "hand_drill",
        name: "Hand Drill",
        desc: "Whenever you break an enemy's Block, apply 2 Vulnerable.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            vulnerable_on_block_break: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "medical_kit",
        name: "Medical Kit",
        desc: "Unplayable Status cards can now be played. Whenever you play a Status card, Exhaust it.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            playable_statuses: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "membership_card",
        name: "Membership Card",
        desc: "50% discount on all products!",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            shop_discount_pct: 50,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "melange",
        name: "Melange",
        desc: "Whenever you shuffle your draw pile, Scry 3.",
        tier: RelicTier::Shop,
        pool: "purple",
        fx: RelicFx::ZERO,
        note: "观星机制本作没有(紫职专属)",
    },
    RelicDef {
        id: "orange_pellets",
        name: "Orange Pellets",
        desc: "Whenever you play a Power, Attack, and Skill in the same turn, remove all of your debuffs.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            clear_debuffs_on_all_types: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "runic_capacitor",
        name: "Runic Capacitor",
        desc: "Start each combat with 3 additional Orb slots.",
        tier: RelicTier::Shop,
        pool: "blue",
        fx: RelicFx::ZERO,
        note: "充能球机制本作没有(蓝职专属)",
    },
    RelicDef {
        id: "sling_of_courage",
        name: "Sling of Courage",
        desc: "Start each Elite combat with 2 Strength.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            combat_start_strength_elite: 2,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "strange_spoon",
        name: "Strange Spoon",
        desc: "Cards which Exhaust when played will instead discard 50% of the time.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            exhaust_to_discard_pct: 50,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "the_abacus",
        name: "The Abacus",
        desc: "Whenever you shuffle your draw pile, gain 6 Block.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            block_on_shuffle: 6,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "toolbox",
        name: "Toolbox",
        desc: "At the start of each combat, choose 1 of 3 random Colorless cards and add the chosen card into your hand.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            combat_start_colorless_pick: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "twisted_funnel",
        name: "Twisted Funnel",
        desc: "At the start of each combat, apply 4 Poison to ALL enemies.",
        tier: RelicTier::Shop,
        pool: "green",
        fx: RelicFx::ZERO,
        note: "中毒机制本作没有(绿职专属)",
    },
    RelicDef {
        id: "cauldron",
        name: "Cauldron",
        desc: "When obtained, brews 5 random potions.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            add_potions: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "dollys_mirror",
        name: "Dolly's Mirror",
        desc: "Upon pickup, obtain an additional copy of a card in your deck.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            duplicate_cards: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "lees_waffle",
        name: "Lee's Waffle",
        desc: "Upon pickup, raise your Max HP by 7 and heal all of your HP.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            max_hp: 7,
            full_heal: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "orrery",
        name: "Orrery",
        desc: "Upon pickup, choose and add 5 cards to your deck.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            pickup_card_picks: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "prismatic_shard",
        name: "Prismatic Shard",
        desc: "Combat reward screens now contain Colorless cards and cards from other colors.",
        tier: RelicTier::Shop,
        pool: "shared",
        fx: RelicFx {
            prismatic_rewards: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "bloody_idol",
        name: "Bloody Idol",
        desc: "Whenever you gain Gold, heal 5 HP.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            heal_on_gold_gain: 5,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "cultist_headpiece",
        name: "Cultist Headpiece",
        desc: "You feel more talkative.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx::ZERO,
        note: "纯风味遗物,没有游戏效果",
    },
    RelicDef {
        id: "enchiridion",
        name: "Enchiridion",
        desc: "At the start of each combat, add a random Power card into your hand. It costs 0 for that turn.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            add_random_power_card: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "face_of_cleric",
        name: "Face of Cleric",
        desc: "At the end of combat, raise your Max HP by 1.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            max_hp_on_victory: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "golden_idol",
        name: "Golden Idol",
        desc: "Enemies drop 25% more Gold.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            gold_reward_pct: 25,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "gremlin_visage",
        name: "Gremlin Visage",
        desc: "Start each combat with 1 Weak.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            combat_start_self_weak: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "mark_of_the_bloom",
        name: "Mark of the Bloom",
        desc: "You can no longer heal.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            no_heal: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "mutagenic_strength",
        name: "Mutagenic Strength",
        desc: "Start each combat with 3 Strength. At the end of your first turn, lose 3 Strength.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            combat_start_strength_turn1: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "necronomicon",
        name: "Necronomicon",
        desc: "The first Attack played each turn that costs 2 or more is played twice. Upon pickup, obtain a special Curse.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            double_first_big_attack: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "nilrys_codex",
        name: "Nilry's Codex",
        desc: "At the end of each turn, you may shuffle 1 of 3 random cards to shuffle into your draw pile.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            end_turn_shuffle_pick: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "neows_lament",
        name: "Neow's Lament",
        desc: "Enemies in your first 3 combats will have 1 HP.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            neow_lament_combats: 3,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "nloths_gift",
        name: "N'loth's Gift",
        desc: "Triples the chance of finding Rare cards from combat rewards.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            rare_card_chance_x3: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "nloths_hungry_face",
        name: "N'loth's Hungry Face",
        desc: "The next non-Boss chest you open is empty.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            chest_empty_charges: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "odd_mushroom",
        name: "Odd Mushroom",
        desc: "When Vulnerable, take 25% more attack damage rather than 50%.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            vulnerable_taken_pct: 125,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "red_mask",
        name: "Red Mask",
        desc: "At the start of each combat, apply 1 Weak to ALL enemies.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            combat_start_enemy_weak: 1,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "ssserpent_head",
        name: "Ssserpent Head",
        desc: "Whenever you enter a ? room, gain 50 Gold.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            gold_on_unknown_room: 50,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "spirit_poop",
        name: "Spirit Poop",
        desc: "It's unpleasant.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx::ZERO,
        note: "纯风味遗物,没有游戏效果",
    },
    RelicDef {
        id: "warped_tongs",
        name: "Warped Tongs",
        desc: "At the start of your turn, Upgrade a random card in your hand for the rest of combat.",
        tier: RelicTier::Event,
        pool: "shared",
        fx: RelicFx {
            upgrade_random_hand_at_turn_start: true,
            ..RelicFx::ZERO
        },
        note: "",
    },
    RelicDef {
        id: "circlet",
        name: "Circlet",
        desc: "Collect as many as you can.",
        tier: RelicTier::Special,
        pool: "shared",
        fx: RelicFx::ZERO,
        note: "纯风味遗物(池子抽干后的兜底)",
    },
    RelicDef {
        id: "red_circlet",
        name: "Red Circlet",
        desc: "You ran out of relics. Impressive!",
        tier: RelicTier::Special,
        pool: "shared",
        fx: RelicFx::ZERO,
        note: "纯风味遗物(Boss 池抽干后的兜底)",
    },
];

pub fn relic_def(id: &str) -> Option<&'static RelicDef> {
    RELICS.iter().find(|r| r.id == id)
}

pub fn relic_def_or_panic(id: &str) -> &'static RelicDef {
    relic_def(id).unwrap_or_else(|| panic!("unknown relic id: {id}"))
}

/// 某个角色(red/green/blue/purple)某个档次的池子,顺序 = RELICS 里的顺序.
/// 与参考实现的 buildRelicPool 一致:shared 加本职业专属.
pub fn pool_for(tier: RelicTier, color: &str) -> Vec<&'static RelicDef> {
    RELICS
        .iter()
        .filter(|r| r.tier == tier && (r.pool == "shared" || r.pool == color))
        .collect()
}

/// 该档次的全部遗物(不分职业;自检用)
#[cfg(test)]
pub fn relics_of(tier: RelicTier) -> Vec<&'static RelicDef> {
    RELICS.iter().filter(|r| r.tier == tier).collect()
}

/// 起始遗物(如燃烧之血)
#[cfg(test)]
pub fn starter_relic() -> &'static RelicDef {
    relic_def_or_panic("burning_blood")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 各档件数(与参考实现 bundle 完全一致)
    const TIER_COUNTS: &[(RelicTier, usize)] = &[
        (RelicTier::Starter, 4),
        (RelicTier::Common, 36),
        (RelicTier::Uncommon, 37),
        (RelicTier::Rare, 34),
        (RelicTier::Boss, 30),
        (RelicTier::Shop, 20),
        (RelicTier::Event, 18),
        (RelicTier::Special, 2),
    ];

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = RELICS.iter().map(|r| r.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate relic id");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = RELICS.iter().map(|r| r.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate relic name");
    }

    #[test]
    fn corpus_is_fully_covered() {
        assert_eq!(RELICS.len(), 181, "语料里共 181 件遗物");
        for info in crate::core::corpus::RELICS {
            assert!(relic_def(info.id).is_some(), "语料遗物 {} 没实装", info.id);
        }
    }

    #[test]
    fn tier_counts_match_reference() {
        for (tier, want) in TIER_COUNTS {
            assert_eq!(relics_of(*tier).len(), *want, "{} 档件数不对", tier.name());
        }
    }

    /// 铁甲战士能抽到的各档池子大小(shared + red)
    #[test]
    fn ironclad_pool_sizes_match_reference() {
        assert_eq!(pool_for(RelicTier::Common, "red").len(), 33);
        assert_eq!(pool_for(RelicTier::Uncommon, "red").len(), 31);
        assert_eq!(pool_for(RelicTier::Rare, "red").len(), 28);
        assert_eq!(pool_for(RelicTier::Boss, "red").len(), 22);
        assert_eq!(pool_for(RelicTier::Shop, "red").len(), 17);
        assert_eq!(pool_for(RelicTier::Event, "red").len(), 18);
    }

    /// 池子顺序 = RELICS 顺序 = 参考实现 bundle 顺序(洗牌后的身份由它决定)
    #[test]
    fn pool_order_matches_reference() {
        let c = pool_for(RelicTier::Common, "red");
        assert_eq!(c[0].id, "akabeko");
        assert_eq!(c[1].id, "art_of_war");
        assert_eq!(c[2].id, "anchor");
        let b = pool_for(RelicTier::Boss, "red");
        assert_eq!(b[0].id, "black_blood");
        assert_eq!(b[1].id, "busted_crown");
        let s = pool_for(RelicTier::Shop, "red");
        assert_eq!(s[0].id, "brimstone");
        assert_eq!(s[1].id, "chemical_x");
        // 职业专属只进本职业的池子
        assert!(pool_for(RelicTier::Common, "green").iter().any(|r| r.id == "snecko_skull"));
        assert!(!pool_for(RelicTier::Common, "red").iter().any(|r| r.id == "snecko_skull"));
    }

    /// 还没实现的遗物:其它职业专属(充能球/集中/毒/Shiv/Mantra/观星/Scry)
    /// 或纯风味、不可获得.48 件 fx 全零 = 本表 26 件 + 已实现的 22 件.
    const GATED_RELICS: &[&str] = &[
        "cracked_core",
        "pure_water",
        "damaru",
        "data_disk",
        "snecko_skull",
        "duality",
        "gold_plated_cables",
        "ninja_scroll",
        "symbiotic_virus",
        "teardrop_locket",
        "discerning_monocle",
        "emotion_chip",
        "golden_eye",
        "the_specimen",
        "frozen_core",
        "holy_water",
        "inserter",
        "nuclear_battery",
        "violet_lotus",
        "melange",
        "runic_capacitor",
        "twisted_funnel",
        "cultist_headpiece",
        "spirit_poop",
        "circlet",
        "red_circlet",
    ];

    /// fx 全零的遗物就是上面那张表:一件不多一件不少
    #[test]
    fn gated_relics_are_exactly_the_unimplemented_ones() {
        let mut zero: Vec<&str> = RELICS
            .iter()
            .filter(|r| r.fx.is_noop())
            .map(|r| r.id)
            .collect();
        zero.sort_unstable();
        let mut want: Vec<&str> = GATED_RELICS.to_vec();
        want.sort_unstable();
        assert_eq!(zero, want, "fx 全零的集合必须与 GATED_RELICS 一致");
        assert_eq!(zero.len(), 26);
        // 每一件都写明了原因
        for id in GATED_RELICS {
            assert!(
                !relic_def_or_panic(id).note.is_empty(),
                "{id} 没实现也没写原因"
            );
        }
    }

    /// 本批实现的 22 件:开关值一件一件钉住
    #[test]
    fn this_batch_of_relics_carries_the_expected_switches() {
        let fx = |id: &str| relic_def_or_panic(id).fx;
        // 营火
        assert!(fx("dream_catcher").rest_card_reward);
        assert!(fx("peace_pipe").rest_toke);
        assert!(fx("shovel").rest_dig);
        assert_eq!(fx("girya").rest_lift_max, 3);
        // 握手与发现式选择
        assert!(fx("bottled_flame").bottle_attack);
        assert!(fx("bottled_lightning").bottle_skill);
        assert!(fx("bottled_tornado").bottle_power);
        assert_eq!(fx("toolbox").combat_start_colorless_pick, 3);
        assert_eq!(fx("orrery").pickup_card_picks, 5);
        assert_eq!(fx("nilrys_codex").end_turn_shuffle_pick, 3);
        assert_eq!(fx("prayer_wheel").extra_card_reward_group, 1);
        assert!(fx("prismatic_shard").prismatic_rewards);
        // 战斗钩子
        assert_eq!(fx("champion_belt").weak_on_vulnerable, 1);
        assert_eq!(fx("lizard_tail").death_save_pct, 50);
        assert_eq!(fx("blue_candle").playable_curses_hp, 1);
        assert!(fx("medical_kit").playable_statuses);
        assert_eq!(fx("hand_drill").vulnerable_on_block_break, 2);
        assert_eq!(fx("chemical_x").x_cost_bonus, 2);
        assert_eq!(fx("strange_spoon").exhaust_to_discard_pct, 50);
        // 地图与显示
        assert_eq!(fx("wing_boots").map_wing_charges, 3);
        assert!(fx("frozen_eye").draw_pile_in_order);
        // 药水
        assert_eq!(fx("sacred_bark").potion_potency_pct, 100);
        // 都不再是空壳
        let batch = [
            "dream_catcher",
            "peace_pipe",
            "shovel",
            "girya",
            "bottled_flame",
            "bottled_lightning",
            "bottled_tornado",
            "toolbox",
            "orrery",
            "nilrys_codex",
            "prayer_wheel",
            "prismatic_shard",
            "champion_belt",
            "lizard_tail",
            "blue_candle",
            "medical_kit",
            "hand_drill",
            "chemical_x",
            "strange_spoon",
            "wing_boots",
            "frozen_eye",
            "sacred_bark",
        ];
        for id in batch {
            let def = relic_def_or_panic(id);
            assert!(!def.fx.is_noop(), "{id} 还是空壳");
            assert!(def.note.is_empty(), "{id} 实现了就不该留 note");
        }
    }

    /// fx 全零的遗物必须写明原因,反之有 fx 的不能留 note
    #[test]
    fn every_relic_is_either_implemented_or_documented() {
        for r in RELICS {
            if r.fx.is_noop() {
                assert!(!r.note.is_empty(), "{} 没有效果也没写原因", r.id);
            } else {
                assert!(r.note.is_empty(), "{} 有 fx 就不该写原因", r.id);
            }
        }
    }

    #[test]
    fn every_relic_has_text() {
        for r in RELICS {
            assert!(!r.desc.is_empty(), "{} 没有描述", r.id);
            assert!(!r.name.is_empty(), "{} 没有名字", r.id);
        }
    }

    #[test]
    fn required_relics_have_expected_fields() {
        let blood = relic_def_or_panic("burning_blood");
        assert_eq!(blood.tier, RelicTier::Starter);
        assert_eq!(blood.fx.post_combat_heal, 6);
        assert_eq!(blood.desc, "At the end of combat, heal 6 HP.");

        let lantern = relic_def_or_panic("lantern");
        assert_eq!(lantern.tier, RelicTier::Common);
        assert_eq!(lantern.fx.combat_start_energy, 1);

        let scales = relic_def_or_panic("bronze_scales");
        assert_eq!(scales.tier, RelicTier::Common);
        assert_eq!(scales.fx.thorns, 3);

        assert_eq!(relic_def_or_panic("membership_card").fx.shop_discount_pct, 50);
        assert_eq!(relic_def_or_panic("golden_idol").fx.gold_reward_pct, 25);
    }

    #[test]
    fn starter_relic_is_burning_blood() {
        assert_eq!(starter_relic().id, "burning_blood");
    }

    #[test]
    fn some_relic_has_non_zero_fx() {
        assert!(RELICS.iter().any(|r| !r.fx.is_noop()));
    }

    #[test]
    fn strings_are_ascii_only() {
        for r in RELICS {
            assert!(r.id.is_ascii(), "non-ascii id: {}", r.id);
            assert!(r.name.is_ascii(), "non-ascii name: {}", r.name);
            assert!(r.desc.is_ascii(), "non-ascii desc: {}", r.desc);
        }
    }

    // ================= 一局流程侧遗物钩子(Run 级断言) =================
    //
    // 这些钩子在战斗沙盒里测不到(拾取/地图/商店/营火/事件/奖励/宝箱),
    // tools/sandbox_relics.ts 的 run 行 oracle 填的就是下面这些测试名。

    use crate::core::card::{CardInstance, CardType, Rarity};
    use crate::core::enemies::encounter_def;
    use crate::core::map::NodeKind;
    use crate::core::potions::POTIONS;
    use crate::core::run::{PickPurpose, RestOption, RewardSlot, Run, ShopItem};

    /// 把胜利后的 2 秒停留一步走完
    fn settle(r: &mut Run) {
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
    }

    /// 直接打赢一场普通战斗并结算到奖励屏
    fn win_plain(r: &mut Run) {
        let enc = encounter_def("jaw_worm_solo").expect("大颚虫遭遇");
        r.debug_start_combat(enc);
        r.debug_win_battle();
        settle(r);
    }

    /// 商店里删牌服务的报价
    fn removal_price(r: &Run) -> i32 {
        r.shop
            .as_ref()
            .expect("在商店里")
            .items
            .iter()
            .find_map(|it| match it {
                ShopItem::Remove(p) => Some(*p),
                _ => None,
            })
            .expect("商店有删牌格")
    }

    /// 连赢 n 场普通战斗,数奖励里出了几张稀有牌(同一颗种子,开关礼物各跑一遍)
    fn rare_rewards(seed: u64, gift: bool, n: usize) -> usize {
        let mut r = Run::new(seed);
        if gift {
            r.debug_add_relic("nloths_gift").unwrap();
        }
        let mut rares = 0;
        for _ in 0..n {
            win_plain(&mut r);
            if let Some(rw) = r.reward.as_ref() {
                rares += rw.cards.iter().filter(|c| c.def.rarity == Rarity::Rare).count();
            }
            r.leave_reward();
        }
        rares
    }

    /// 燃烧之血:战后回 6
    #[test]
    fn burning_blood_post_combat_heal() {
        let mut r = Run::new(5);
        r.player.hp = 30;
        win_plain(&mut r);
        assert_eq!(r.player.hp, 36, "战后回 6");
    }

    /// 黑血:替换燃烧之血,战后回 12
    #[test]
    fn black_blood_replaces_blood_and_heals() {
        let mut r = Run::new(5);
        r.debug_add_relic("black_blood").unwrap();
        assert!(r.has_relic("black_blood"));
        assert!(!r.has_relic("burning_blood"), "黑血替换燃烧之血");
        r.player.hp = 30;
        win_plain(&mut r);
        assert_eq!(r.player.hp, 42, "战后回 12");
    }

    /// 肉骨头:战后血量 <=50% 才回 12
    #[test]
    fn meat_on_the_bone_heal_threshold() {
        let mut low = Run::new(5);
        low.debug_remove_relic("burning_blood").unwrap();
        low.debug_add_relic("meat_on_the_bone").unwrap();
        low.player.hp = 39; // 39*2=78 <= 80
        win_plain(&mut low);
        assert_eq!(low.player.hp, 51, "半血以下回 12");
        let mut high = Run::new(5);
        high.debug_remove_relic("burning_blood").unwrap();
        high.debug_add_relic("meat_on_the_bone").unwrap();
        high.player.hp = 70;
        win_plain(&mut high);
        assert_eq!(high.player.hp, 70, "半血以上不回");
    }

    /// 牧师之面:每场胜利最大生命 +1
    #[test]
    fn face_of_cleric_max_hp_on_victory() {
        let mut r = Run::new(5);
        r.debug_add_relic("face_of_cleric").unwrap();
        let (hp0, max0) = (r.player.hp, r.player.max_hp);
        win_plain(&mut r);
        assert_eq!(r.player.max_hp, max0 + 1);
        assert_eq!(r.player.hp, hp0 + 1);
    }

    /// 陶瓷鱼:每加一张牌得 9 金币(vampires 事件加 5 张 bite -> +45)
    #[test]
    fn ceramic_fish_gold_on_card_add() {
        let mut r = Run::new(5);
        r.debug_add_relic("ceramic_fish").unwrap();
        r.player.hp = r.player.max_hp;
        r.debug_open_event("vampires").unwrap();
        let g0 = r.player.gold;
        r.choose_event(1).unwrap();
        assert_eq!(r.player.gold, g0 + 45, "5 张 bite 每张 9 金");
    }

    /// 十手镯:? 房间不再出普通战斗
    #[test]
    fn juzu_bracelet_skips_normal_combats() {
        let mut r = Run::new(5);
        r.debug_add_relic("juzu_bracelet").unwrap();
        let rooms = r.debug_unknown_rooms(60);
        assert!(
            rooms.iter().all(|n| *n != "Monster"),
            "有十手镯时 ? 房不该出普通战斗:{rooms:?}"
        );
        // 对照组:总有一些种子的 ? 房能出普通战斗
        let control = (0..20u64).any(|seed| {
            let mut c = Run::new(seed);
            c.debug_unknown_rooms(60).iter().any(|n| *n == "Monster")
        });
        assert!(control, "没有十手镯时 ? 房应该能出普通战斗");
    }

    /// 银行家之躯:每爬一层 +12 金;在商店花过钱后失效
    #[test]
    fn maw_bank_gold_per_floor_then_stops() {
        let mut r = Run::new(5);
        r.debug_add_relic("maw_bank").unwrap();
        let g0 = r.player.gold;
        let first = r.reachable()[0];
        r.enter_node(first).unwrap();
        assert_eq!(r.player.gold, g0 + 12, "每层 +12");
        // 在商店花一次钱,下一层就不再给
        r.debug_room("shop").unwrap();
        r.spend_gold(1);
        r.leave_shop();
        let g1 = r.player.gold;
        let next = r.reachable()[0];
        r.enter_node(next).unwrap();
        assert_eq!(r.player.gold, g1, "花过钱就不再给");
    }

    /// 餐券:每次进商店回 15
    #[test]
    fn meal_ticket_heals_on_shop() {
        let mut r = Run::new(5);
        r.debug_add_relic("meal_ticket").unwrap();
        r.player.hp = 40;
        r.debug_room("shop").unwrap();
        assert_eq!(r.player.hp, 55, "进店回 15");
    }

    /// 御守:抵消接下来 2 张诅咒,第 3 张才进来
    #[test]
    fn omamori_negates_two_curses() {
        let mut r = Run::new(5);
        r.debug_add_relic("omamori").unwrap();
        r.debug_add_relic("cursed_key").unwrap();
        let curses = |r: &Run| r.player.deck.iter().filter(|c| c.kind() == CardType::Curse).count();
        let base = curses(&r);
        for i in 0..2 {
            r.debug_room("treasure").unwrap();
            r.take_treasure();
            assert_eq!(curses(&r), base, "第 {} 张诅咒被御守挡下", i + 1);
        }
        r.debug_room("treasure").unwrap();
        r.take_treasure();
        assert_eq!(curses(&r), base + 1, "第 3 张进来了");
    }

    /// 药水腰带:拾取 +2 药水格
    #[test]
    fn potion_belt_adds_two_slots() {
        let mut r = Run::new(5);
        let n0 = r.player.potions.len();
        r.debug_add_relic("potion_belt").unwrap();
        assert_eq!(r.player.potions.len(), n0 + 2);
    }

    /// 帝王枕:休息多回 15
    #[test]
    fn regal_pillow_rest_heal_bonus() {
        let mut a = Run::new(5);
        a.player.hp = 30;
        a.rest_heal();
        let plain = a.player.hp;
        let mut b = Run::new(5);
        b.debug_add_relic("regal_pillow").unwrap();
        b.player.hp = 30;
        b.rest_heal();
        assert_eq!(b.player.hp - plain, 15, "休息多回 15");
    }

    /// 微笑面具:删牌服务固定 50 金
    #[test]
    fn smiling_mask_fixes_removal_price() {
        let mut normal = Run::new(5);
        normal.debug_room("shop").unwrap();
        assert_ne!(removal_price(&normal), 50, "正常报价不是 50");
        let mut masked = Run::new(5);
        masked.debug_add_relic("smiling_mask").unwrap();
        masked.debug_room("shop").unwrap();
        assert_eq!(removal_price(&masked), 50);
    }

    /// 草莓:拾取 +7 最大生命
    #[test]
    fn strawberry_max_hp() {
        let mut r = Run::new(5);
        let m = r.player.max_hp;
        r.debug_add_relic("strawberry").unwrap();
        assert_eq!(r.player.max_hp, m + 7);
    }

    /// 小箱子:每第 4 个 ? 房间必出宝箱
    #[test]
    fn tiny_chest_treasure_every_fourth_unknown() {
        let mut r = Run::new(5);
        r.debug_add_relic("tiny_chest").unwrap();
        let rooms = r.debug_unknown_rooms(12);
        for (i, name) in rooms.iter().enumerate() {
            if (i + 1) % 4 == 0 {
                assert_eq!(*name, "Treasure", "第 {} 个 ? 房该是宝箱", i + 1);
            }
        }
    }

    /// 战争涂装:拾取随机升级 2 张技能
    #[test]
    fn war_paint_upgrades_two_skills() {
        let mut r = Run::new(5);
        r.debug_add_relic("war_paint").unwrap();
        let up = r
            .player
            .deck
            .iter()
            .filter(|c| c.kind() == CardType::Skill && c.upgraded)
            .count();
        assert!(up >= 2, "升级了 {up} 张技能,期望 >=2");
    }

    /// 磨刀石:拾取随机升级 2 张攻击
    #[test]
    fn whetstone_upgrades_two_attacks() {
        let mut r = Run::new(5);
        r.debug_add_relic("whetstone").unwrap();
        let up = r
            .player
            .deck
            .iter()
            .filter(|c| c.kind() == CardType::Attack && c.upgraded)
            .count();
        assert!(up >= 2, "升级了 {up} 张攻击,期望 >=2");
    }

    /// 黑石护符:每拿一张诅咒 +6 最大生命
    #[test]
    fn darkstone_periapt_max_hp_per_curse() {
        let mut r = Run::new(5);
        r.debug_add_relic("darkstone_periapt").unwrap();
        r.debug_add_relic("cursed_key").unwrap();
        let m0 = r.player.max_hp;
        r.debug_room("treasure").unwrap();
        r.take_treasure();
        assert_eq!(r.player.max_hp, m0 + 6);
    }

    /// 熔火之蛋:事件加进来的攻击牌直接升级(vampires -> 5 张 bite)
    #[test]
    fn molten_egg_upgrades_attacks() {
        let mut r = Run::new(5);
        r.debug_add_relic("molten_egg").unwrap();
        r.player.hp = r.player.max_hp;
        r.debug_open_event("vampires").unwrap();
        r.choose_event(1).unwrap();
        let bites: Vec<_> = r.player.deck.iter().filter(|c| c.def.id == "bite").collect();
        assert_eq!(bites.len(), 5);
        assert!(bites.iter().all(|c| c.upgraded), "熔火之蛋该升级 bite");
    }

    /// 剧毒之蛋:事件加进来的技能牌直接升级(ghosts -> 5 张 ghostly_armor)
    #[test]
    fn toxic_egg_upgrades_skills() {
        let mut r = Run::new(5);
        r.debug_add_relic("toxic_egg").unwrap();
        r.player.hp = r.player.max_hp;
        r.debug_open_event("ghosts").unwrap();
        r.choose_event(0).unwrap();
        let cards: Vec<_> = r
            .player
            .deck
            .iter()
            .filter(|c| c.def.id == "ghostly_armor")
            .collect();
        assert_eq!(cards.len(), 5);
        assert!(cards.iter().all(|c| c.upgraded), "剧毒之蛋该升级 ghostly_armor");
    }

    /// 冰冻之蛋:商店买进来的能力牌直接升级(加牌统一钩子在商店路径也生效)
    #[test]
    fn frozen_egg_upgrades_powers() {
        use crate::core::run::ShopKind;
        let mut r = Run::new(5);
        r.debug_add_relic("frozen_egg").unwrap();
        r.player.gold = 99_999;
        r.open_shop();
        let i = r
            .shop
            .as_ref()
            .unwrap()
            .kinds
            .iter()
            .position(|k| *k == ShopKind::ClassCard)
            .expect("商店有职业牌格");
        r.shop.as_mut().unwrap().items[i] = ShopItem::Card(
            CardInstance::new(crate::core::cards::card_def_or_panic("inflame")),
            50,
        );
        r.shop.as_mut().unwrap().index = i;
        r.buy_selected().unwrap();
        let got = r.player.deck.last().expect("牌组末尾是刚买的牌");
        assert!(got.upgraded, "冰冻之蛋该升级买来的 inflame");
    }

    /// 永恒之羽:每进一次营火、每 5 张牌回 3(初始 10 张 -> 回 6)
    #[test]
    fn eternal_feather_rest_heal() {
        let mut a = Run::new(5);
        a.player.hp = 20;
        a.debug_room("rest").unwrap();
        let plain = a.player.hp;
        let mut b = Run::new(5);
        b.debug_add_relic("eternal_feather").unwrap();
        b.player.hp = 20;
        b.debug_room("rest").unwrap();
        assert_eq!(b.player.hp - plain, 6, "10 张牌 / 5 * 3");
    }

    /// 梨:拾取 +10 最大生命
    #[test]
    fn pear_max_hp() {
        let mut r = Run::new(5);
        let m = r.player.max_hp;
        r.debug_add_relic("pear").unwrap();
        assert_eq!(r.player.max_hp, m + 10);
    }

    /// 芒果:拾取 +14 最大生命
    #[test]
    fn mango_max_hp() {
        let mut r = Run::new(5);
        let m = r.player.max_hp;
        r.debug_add_relic("mango").unwrap();
        assert_eq!(r.player.max_hp, m + 14);
    }

    /// 古钱币:拾取 +300 金币
    #[test]
    fn old_coin_gold() {
        let mut r = Run::new(5);
        let g = r.player.gold;
        r.debug_add_relic("old_coin").unwrap();
        assert_eq!(r.player.gold, g + 300);
    }

    /// 李的华夫饼:拾取 +7 最大生命并回满
    #[test]
    fn lees_waffle_max_hp_and_full_heal() {
        let mut r = Run::new(5);
        r.player.hp = 10;
        let m = r.player.max_hp;
        r.debug_add_relic("lees_waffle").unwrap();
        assert_eq!(r.player.max_hp, m + 7);
        assert_eq!(r.player.hp, r.player.max_hp, "回满");
    }

    /// 星盘:拾取时开"转化 3 张"的选牌窗口
    #[test]
    fn astrolabe_transforms_three() {
        let mut r = Run::new(5);
        r.debug_add_relic("astrolabe").unwrap();
        assert_eq!(r.picker.as_ref().unwrap().purpose, PickPurpose::Transform);
        assert_eq!(r.picker.as_ref().unwrap().remaining, 3, "转化 3 张");
    }

    /// 空笼:拾取时开"删 2 张"的选牌窗口
    #[test]
    fn empty_cage_removes_two() {
        let mut r = Run::new(5);
        r.debug_add_relic("empty_cage").unwrap();
        assert_eq!(r.picker.as_ref().unwrap().purpose, PickPurpose::Remove);
        assert_eq!(r.picker.as_ref().unwrap().remaining, 2, "删 2 张");
    }

    /// 达莉的镜子:拾取时开"复制一张"的选牌窗口
    #[test]
    fn dollys_mirror_duplicates_a_card() {
        let mut r = Run::new(5);
        r.debug_add_relic("dollys_mirror").unwrap();
        assert_eq!(r.picker.as_ref().unwrap().purpose, PickPurpose::Duplicate);
    }

    /// 呼叫铃:拾取 3 件遗物并塞一张诅咒
    #[test]
    fn calling_bell_three_relics_and_curse() {
        let mut r = Run::new(5);
        let n = r.player.relics.len();
        let curses = r.player.deck.iter().filter(|c| c.kind() == CardType::Curse).count();
        r.debug_add_relic("calling_bell").unwrap();
        assert_eq!(r.player.relics.len(), n + 1 + 3, "铃铛本身 + 3 件");
        assert_eq!(
            r.player.deck.iter().filter(|c| c.kind() == CardType::Curse).count(),
            curses + 1,
            "附带一张诅咒"
        );
    }

    /// 坩埚:拾取时调满药水格(先戴药水腰带凑 5 格)
    #[test]
    fn cauldron_five_potions() {
        let mut r = Run::new(5);
        r.debug_add_relic("potion_belt").unwrap();
        r.debug_add_relic("cauldron").unwrap();
        let filled = r.player.potions.iter().filter(|p| p.is_some()).count();
        assert_eq!(filled, 5, "5 瓶药水");
    }

    /// 潘多拉魔盒:所有打击与防御都变形,数量不变
    #[test]
    fn pandoras_box_transforms_strikes_and_defends() {
        let mut r = Run::new(5);
        let total = r.player.deck.len();
        r.debug_add_relic("pandoras_box").unwrap();
        let left = r
            .player
            .deck
            .iter()
            .filter(|c| matches!(c.def.id, "strike" | "defend"))
            .count();
        assert_eq!(left, 0, "打击与防御都该被转化");
        assert_eq!(r.player.deck.len(), total, "牌数不变");
    }

    /// 小房子:最大生命 +5、金币 +50、开一组卡牌奖励
    #[test]
    fn tiny_house_pickup_bundle() {
        let mut r = Run::new(5);
        let (m, g) = (r.player.max_hp, r.player.gold);
        r.debug_add_relic("tiny_house").unwrap();
        assert_eq!(r.player.max_hp, m + 5);
        assert_eq!(r.player.gold, g + 50);
        assert!(r.reward.as_ref().is_some_and(|rw| !rw.cards.is_empty()), "开卡牌奖励");
    }

    /// 问号牌:卡牌奖励多 1 张
    #[test]
    fn question_card_extra_card_reward() {
        let mut r = Run::new(5);
        r.debug_add_relic("question_card").unwrap();
        win_plain(&mut r);
        assert_eq!(r.reward.as_ref().unwrap().cards.len(), 4, "3 + 1");
    }

    /// Boss 王冠:卡牌奖励少 2 张
    #[test]
    fn busted_crown_card_reward_minus_two() {
        let mut r = Run::new(5);
        r.debug_add_relic("busted_crown").unwrap();
        win_plain(&mut r);
        assert_eq!(r.reward.as_ref().unwrap().cards.len(), 1, "3 - 2");
    }

    /// 唱歌碗:跳过卡牌奖励换 +2 最大生命
    #[test]
    fn singing_bowl_offers_max_hp_on_skip() {
        let mut r = Run::new(5);
        r.debug_add_relic("singing_bowl").unwrap();
        win_plain(&mut r);
        let m = r.player.max_hp;
        let msg = r.reward_skip_cards();
        assert!(msg.contains("+2"), "提示:{msg}");
        assert_eq!(r.player.max_hp, m + 2);
    }

    /// 白色野兽雕像:战斗奖励必出药水
    #[test]
    fn white_beast_statue_guarantees_potion() {
        let mut r = Run::new(5);
        r.debug_add_relic("white_beast_statue").unwrap();
        let rolls = r.debug_potion_rewards(25);
        assert!(rolls.iter().all(|p| p.is_some()), "雕像下每次必掉:{rolls:?}");
    }

    /// 金像:敌人掉的金币多 25%
    #[test]
    fn golden_idol_gold_reward_bonus() {
        let mut base = Run::new(5);
        win_plain(&mut base);
        let plain = base.reward.as_ref().unwrap().gold;
        let mut idol = Run::new(5);
        idol.debug_add_relic("golden_idol").unwrap();
        win_plain(&mut idol);
        let boosted = idol.reward.as_ref().unwrap().gold;
        assert!(plain > 0);
        assert_eq!(boosted, plain + (plain * 25 + 50) / 100, "多 25%");
    }

    /// 血偶像:每次获得金币回 5
    #[test]
    fn bloody_idol_heals_on_gold_gain() {
        let mut r = Run::new(5);
        r.debug_add_relic("bloody_idol").unwrap();
        r.player.hp = 50;
        r.gain_gold(10);
        assert_eq!(r.player.hp, 55);
    }

    /// Neow 的哀悼:最前面 3 场战斗敌人只剩 1 血
    #[test]
    fn neows_lament_weakens_first_combats() {
        let mut r = Run::new(5);
        r.debug_add_relic("neows_lament").unwrap();
        let enc = encounter_def("jaw_worm_solo").unwrap();
        r.debug_start_combat(enc);
        assert!(
            r.combat.as_ref().unwrap().enemies.iter().all(|e| e.hp == 1),
            "开局敌人 1 血"
        );
    }

    /// 恩洛斯的礼物:稀有牌概率翻三倍
    #[test]
    fn nloths_gift_triples_rare_chance() {
        let plain = rare_rewards(9, false, 120);
        let gift = rare_rewards(9, true, 120);
        assert!(gift > plain, "礼物该提高稀有牌数:{gift} vs {plain}");
    }

    /// 饥肠辘辘之脸:下一个非 Boss 宝箱为空,之后恢复正常
    #[test]
    fn nloths_hungry_face_empties_a_chest() {
        let mut r = Run::new(5);
        r.debug_add_relic("nloths_hungry_face").unwrap();
        let n = r.player.relics.len();
        r.debug_room("treasure").unwrap();
        r.take_treasure();
        assert_eq!(r.player.relics.len(), n, "第一个箱子是空的");
        r.debug_room("treasure").unwrap();
        r.take_treasure();
        assert!(r.player.relics.len() > n, "之后的箱子恢复给遗物");
    }

    /// 蛇头:进 ? 房间 +50 金币
    #[test]
    fn ssserpent_head_gold_on_unknown() {
        for seed in 0..30u64 {
            let mut r = Run::new(seed);
            r.debug_add_relic("ssserpent_head").unwrap();
            // 第 1 层必是普通战斗:先打赢,回到地图
            let first = r.reachable()[0];
            r.enter_node(first).unwrap();
            r.debug_win_battle();
            settle(&mut r);
            r.leave_reward();
            if r.screen != crate::core::run::Screen::Map {
                continue;
            }
            let Some(idx) = r
                .reachable()
                .into_iter()
                .find(|i| r.map.node(*i).kind == NodeKind::Event)
            else {
                continue;
            };
            let g = r.player.gold;
            r.enter_node(idx).unwrap();
            assert_eq!(r.player.gold, g + 50, "seed {seed} 进 ? 房该 +50");
            return;
        }
        panic!("30 个种子里第二层都没找到 ? 房");
    }

    /// 花开彼岸:再也回不了血
    #[test]
    fn mark_of_the_bloom_blocks_healing() {
        let mut r = Run::new(5);
        r.debug_add_relic("mark_of_the_bloom").unwrap();
        r.player.hp = 30;
        assert_eq!(r.heal(10), 0);
        assert_eq!(r.player.hp, 30);
    }

    /// 灵质:再也拿不到金币
    #[test]
    fn ectoplasm_blocks_gold() {
        let mut r = Run::new(5);
        r.debug_add_relic("ectoplasm").unwrap();
        let g = r.player.gold;
        r.gain_gold(50);
        assert_eq!(r.player.gold, g);
    }

    /// 苏族之魂:再也拿不到药水
    #[test]
    fn sozu_blocks_potions() {
        let mut r = Run::new(5);
        r.debug_add_relic("sozu").unwrap();
        assert!(!r.add_potion(&POTIONS[0]), "加不进药水");
    }

    /// 咖啡滤壶:营火不能休息
    #[test]
    fn coffee_dripper_blocks_rest() {
        let mut r = Run::new(5);
        r.debug_add_relic("coffee_dripper").unwrap();
        r.debug_room("rest").unwrap();
        assert!(!r.rest_options().contains(&RestOption::Rest), "不能休息");
    }

    /// 融合锤:营火不能锻造
    #[test]
    fn fusion_hammer_blocks_smith() {
        let mut r = Run::new(5);
        r.debug_add_relic("fusion_hammer").unwrap();
        r.debug_room("rest").unwrap();
        assert!(!r.rest_options().contains(&RestOption::Smith), "不能锻造");
    }

    /// 诅咒钥匙:非 Boss 宝箱附带一张诅咒
    #[test]
    fn cursed_key_curse_on_chest() {
        let mut r = Run::new(5);
        r.debug_add_relic("cursed_key").unwrap();
        let c0 = r.player.deck.iter().filter(|c| c.kind() == CardType::Curse).count();
        r.debug_room("treasure").unwrap();
        r.take_treasure();
        assert_eq!(
            r.player.deck.iter().filter(|c| c.kind() == CardType::Curse).count(),
            c0 + 1
        );
    }

    /// 黑星:精英多掉一件遗物
    #[test]
    fn black_star_extra_elite_relic() {
        let mut r = Run::new(5);
        r.debug_add_relic("black_star").unwrap();
        let n = r.player.relics.len();
        let enc = encounter_def("gremlin_nob_solo").unwrap();
        r.debug_start_combat(enc);
        r.debug_win_battle();
        settle(&mut r);
        assert!(r.player.relics.len() > n, "精英多掉一件");
    }

    /// 会员卡:所有商品降 50%
    #[test]
    fn membership_card_shop_discount() {
        let mut plain = Run::new(5);
        plain.debug_room("shop").unwrap();
        let base = removal_price(&plain);
        let mut member = Run::new(5);
        member.debug_add_relic("membership_card").unwrap();
        member.debug_room("shop").unwrap();
        let discounted = removal_price(&member);
        assert_eq!(discounted, (base as f32 * 0.5).round() as i32, "五折");
    }

    /// 奖励屏取走一张牌要真的进牌组
    #[test]
    fn reward_card_take_adds_to_deck() {
        let mut r = Run::new(5);
        win_plain(&mut r);
        let slots = r.reward_slots();
        let idx = slots
            .iter()
            .position(|s| matches!(s, RewardSlot::Card(_)))
            .expect("有卡牌奖励");
        r.reward.as_mut().unwrap().index = idx;
        let before = r.player.deck.len();
        r.reward_take().unwrap();
        assert_eq!(r.player.deck.len(), before + 1);
    }

}
