// 一局(run)的流程:地图推进、战斗结算、奖励、商店、事件、营火、牌组管理.
// 所有状态都在这里,UI 只读这些字段并调用这里的方法改状态.
use crate::core::card::{CardDef, CardInstance, CardType, Effect as CardEffect, Rarity};
use crate::core::cards;
use crate::core::combat::{Combat, CombatSetup, Phase, RunRelicCounters};
use crate::core::corpus;
use crate::core::enemies;
use crate::core::enemy::{EnemyKind, Encounter};
use crate::core::events::{
    CombatReward, EventDef, FlipResult, MatchKeep, NeowOption, Outcome, RemoveRule,
};
use crate::core::map::{ActMap, NodeKind};
use crate::core::potions::{self, PotionDef, PotionFx};
use crate::core::relics::{self, RelicDef, RelicFx, RelicTier};
use crate::core::roster;
use crate::core::status::Status;
use crate::rng::{java_shuffle, FloorStream, JavaRandom, RngRegistry, RunStream};

/// 药水格子数
pub const POTION_SLOTS: usize = 3;
/// 起始生命(铁甲战士的,只有测试还在用;实际数值来自语料)
#[cfg(test)]
pub const STARTING_HP: i32 = 80;
/// 起始金币(同上)
#[cfg(test)]
pub const STARTING_GOLD: i32 = 99;
/// 营火休息回复比例(百分比)
pub const REST_HEAL_PCT: i32 = 30;
/// 历史记录上限
const HISTORY_LIMIT: usize = 4000;
/// 超限时一次丢掉多少条
const HISTORY_TRIM: usize = 1000;

// ---- 奖励与掉落的常数(照抄参考实现 rewards.ts / shop.ts) ----

/// 卡牌奖励的稀有度保底初值
const CARD_RARITY_PITY_START: i32 = 5;
/// 抽到普通牌时保底值 -1,最低到这里
const CARD_RARITY_PITY_FLOOR: i32 = -40;
/// 普通战斗的稀有概率(百分数),精英另有一套
const CARD_RARE_CHANCE_ELITE: i32 = 10;
const CARD_RARE_CHANCE_NON_ELITE: i32 = 3;
const CARD_UNCOMMON_CHANCE_ELITE: i32 = 40;
const CARD_UNCOMMON_CHANCE_NON_ELITE: i32 = 37;
/// 卡牌奖励固定三选一
const CARD_REWARD_COUNT: usize = 3;
/// the_library 的 Read:候选张数(原版 20 张选一张,不重样)
const LIBRARY_CARD_COUNT: usize = 20;
/// 药水掉落的基础概率与保底步长
const POTION_DROP_BASE_CHANCE: i32 = 40;
const POTION_PITY_STEP: i32 = 10;
/// 精英掉遗物的稀有度分界(参考实现 returnRandomRelicTierElite)
const ELITE_RELIC_COMMON_BELOW: u32 = 50;
const ELITE_RELIC_RARE_ABOVE: u32 = 82;
/// 商店价格:底价与浮动区间(参考实现 shop.ts)
const SHOP_CARD_JITTER: (f32, f32) = (0.9, 1.1);
const SHOP_OTHER_JITTER: (f32, f32) = (0.95, 1.05);
const SHOP_CARD_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 50),
    (Rarity::Uncommon, 75),
    (Rarity::Rare, 150),
];
const SHOP_RELIC_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 150),
    (Rarity::Uncommon, 250),
    (Rarity::Rare, 300),
];
const SHOP_POTION_BASE: [(Rarity, i32); 3] = [
    (Rarity::Common, 50),
    (Rarity::Uncommon, 75),
    (Rarity::Rare, 100),
];
/// 商店掷稀有度的两条线:掷值 + 保底值 <9 稀有,>=46 普通
const SHOP_CARD_RARE_BELOW: i32 = 9;
const SHOP_CARD_COMMON_AT: i32 = 46;
/// 商店的无色牌比同稀有度的职业牌贵 20%
const SHOP_COLORLESS_FACTOR: f32 = 1.2;
/// 信使补一张无色牌:30% 掷稀有,否则罕见(原版 COLORLESS_RARE_CHANCE = 0.30)
const SHOP_COLORLESS_RARE_CHANCE: f32 = 0.30;
/// 七张牌里只有一格打折(掷 merchantRng.random(4))
const SHOP_SALE_SLOTS: u32 = 5;
/// 删牌服务:底价 75,本局每买过一次涨 25
const SHOP_REMOVAL_BASE: i32 = 75;
const SHOP_REMOVAL_STEP: i32 = 25;

/// 商店遗物的档次(比 Rarity 多了"商店档")
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShopTier {
    Common,
    Uncommon,
    Rare,
    Shop,
}

impl ShopTier {
    /// 各档次的底价(商店档 150,与参考实现一致)
    fn base(self) -> i32 {
        match self {
            ShopTier::Common => shop_base(SHOP_RELIC_BASE, Rarity::Common),
            ShopTier::Uncommon => shop_base(SHOP_RELIC_BASE, Rarity::Uncommon),
            ShopTier::Rare => shop_base(SHOP_RELIC_BASE, Rarity::Rare),
            ShopTier::Shop => 150,
        }
    }
}

// ---- 未知房(问号房)与事件池的常数(照抄参考实现 runFlow.ts) ----

/// 未知房判成怪/商店/宝箱的起始概率
const UNKNOWN_BASE: (f32, f32, f32) = (0.1, 0.03, 0.02);
/// 没判中时下一次各涨多少(与起始值相同)
const UNKNOWN_ESCALATION: (f32, f32, f32) = (0.1, 0.03, 0.02);
/// 未知房判成事件后,有 0.25 的概率改抽神龛(或一次性事件)
const SHRINE_CHANCE: f32 = 0.25;

/// 第一章的事件池(顺序就是抽签的顺序,不能乱)
const ACT1_EVENTS: [&str; 11] = [
    "big_fish",
    "the_cleric",
    "dead_adventurer",
    "golden_idol",
    "wing_statue",
    "world_of_goop",
    "the_ssssserpent",
    "living_wall",
    "hypnotizing_colored_mushrooms",
    "scrap_ooze",
    "shining_light",
];
/// 第一章的神龛池
const ACT1_SHRINES: [&str; 6] = [
    "match_and_keep",
    "golden_shrine",
    "transmorgrifier",
    "purifier",
    "upgrade_shrine",
    "wheel_of_change",
];
/// 第二章的事件池(顺序就是抽签的顺序,照参考实现 actDefs[1].events)
const ACT2_EVENTS: [&str; 13] = [
    "pleading_vagrant",
    "ancient_writing",
    "old_beggar",
    "colosseum",
    "cursed_tome",
    "augmenter",
    "forgotten_altar",
    "ghosts",
    "masked_bandits",
    "the_nest",
    "the_library",
    "the_mausoleum",
    "vampires",
];
/// 第三章的事件池
const ACT3_EVENTS: [&str; 7] = [
    "falling",
    "mindbloom",
    "the_moai_head",
    "mysterious_sphere",
    "sensory_stone",
    "tomb_of_lord_red_mask",
    "winding_halls",
];
/// 第二、三章的神龛池:与第一章同一批,但顺序不同(参考实现 actDefs 的原样)
const ACT23_SHRINES: [&str; 6] = [
    "match_and_keep",
    "wheel_of_change",
    "golden_shrine",
    "transmorgrifier",
    "purifier",
    "upgrade_shrine",
];

/// 第 act 章的事件池
fn act_event_pool(act: u32) -> Vec<&'static str> {
    match act {
        1 => ACT1_EVENTS.to_vec(),
        2 => ACT2_EVENTS.to_vec(),
        3 => ACT3_EVENTS.to_vec(),
        _ => Vec::new(),
    }
}

/// 第 act 章的神龛池
fn act_shrine_pool(act: u32) -> Vec<&'static str> {
    match act {
        1 => ACT1_SHRINES.to_vec(),
        _ => ACT23_SHRINES.to_vec(),
    }
}
/// 一次性事件池(抽走即移除).A15 起去掉 note_for_yourself
/// (参考实现 ONE_TIME_EVENTS_ASC15 = ASC0 里去掉 NOTE_FOR_YOURSELF)
const ONE_TIME_EVENTS: [&str; 14] = [
    "ominous_forge",
    "bonfire_spirits",
    "designer_in_spire",
    "duplicator",
    "face_trader",
    "the_divine_fountain",
    "knowing_skull",
    "lab",
    "nloth",
    "note_for_yourself",
    "secret_portal",
    "the_joust",
    "we_meet_again",
    "the_woman_in_blue",
];

/// 本局的一次性事件池:A15 起 note_for_yourself 不进池子(抽签的档位也就少了它)
fn one_time_event_pool(asc: u32) -> Vec<&'static str> {
    ONE_TIME_EVENTS
        .iter()
        .copied()
        .filter(|id| asc < 15 || *id != "note_for_yourself")
        .collect()
}

/// 历史记录里一条的类别,决定它在历史窗口里怎么上色
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistoryKind {
    /// 一局流程自己的记录(进房间、买卖、拿奖励)
    System,
    /// 玩家做了什么
    Player,
    /// 敌人做了什么
    Enemy,
    /// 其他说明
    Info,
}

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub text: String,
    pub kind: HistoryKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// 开始界面:continue / new game / compendium
    Title,
    /// 选角色
    CharSelect,
    /// 图鉴子菜单:卡片库/遗物册/药水间
    Compendium,
    /// 图鉴列表(具体看哪一种由 App::library 决定)
    Library,
    Map,
    Combat,
    Reward,
    Rest,
    Shop,
    Event,
    Treasure,
    /// 选牌界面(升级/移除)
    Pick,
    Victory,
    Death,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Title => "TITLE",
            Screen::CharSelect => "CHARACTER",
            Screen::Compendium => "COMPENDIUM",
            Screen::Library => "LIBRARY",
            Screen::Map => "MAP",
            Screen::Combat => "COMBAT",
            Screen::Reward => "REWARD",
            Screen::Rest => "REST",
            Screen::Shop => "SHOP",
            Screen::Event => "EVENT",
            Screen::Treasure => "TREASURE",
            Screen::Pick => "PICK",
            Screen::Victory => "VICTORY",
            Screen::Death => "DEATH",
        }
    }
}

/// 营火能做的事(参考实现 restOptionAvailable 的位)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestOption {
    Rest,
    Smith,
    Recall,
    /// 吉利亚举铁:永久 +1 力量(有次数上限)
    Lift,
    /// 和平烟斗:从牌组里删一张
    Toke,
    /// 铲子:挖一件遗物
    Dig,
}

impl RestOption {
    pub fn name(self) -> &'static str {
        match self {
            RestOption::Rest => "Rest",
            RestOption::Smith => "Smith",
            RestOption::Recall => "Recall",
            RestOption::Lift => "Lift",
            RestOption::Toke => "Toke",
            RestOption::Dig => "Dig",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickPurpose {
    Upgrade,
    Remove,
    /// 变形:移除这张牌,换成一张随机本职业牌
    Transform,
    /// 复制:往牌组里再塞一张一样的
    Duplicate,
    /// 瓶装(瓶装火焰/闪电/龙卷风):把选中的牌标记成开局进手,牌留在牌组里
    Bottle,
    /// 祭品(篝火精灵):把选中的牌烧掉,赏赐按它的稀有度算
    Offer,
}

impl PickPurpose {
    pub fn title(self) -> &'static str {
        match self {
            PickPurpose::Upgrade => "choose a card to upgrade",
            PickPurpose::Remove => "choose a card to remove",
            PickPurpose::Transform => "choose a card to transform",
            PickPurpose::Duplicate => "choose a card to duplicate",
            PickPurpose::Bottle => "choose a card to bottle",
            PickPurpose::Offer => "choose a card to offer",
        }
    }
}

pub struct Picker {
    pub purpose: PickPurpose,
    pub back: Screen,
    pub index: usize,
    /// 确认时要付的金币(商店的移除服务)
    pub cost_gold: i32,
    /// 付款成功要标记已售的商店下标
    pub shop_slot: Option<usize>,
    /// 还要选几张(Neow 的"移除两张/变形两张"是 2,其余是 1)
    pub remaining: u8,
    /// 瓶装时只让选这一类型的牌(None 表示不限)
    pub bottle_kind: Option<CardType>,
    /// 便条事件:选中的牌要写回存卡文件留给下一局
    pub store_note: bool,
}

pub struct Player {
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub deck: Vec<CardInstance>,
    pub relics: Vec<&'static RelicDef>,
    pub potions: Vec<Option<&'static PotionDef>>,
}

impl Player {
    /// 把所有遗物的某个数值字段加起来
    pub fn relic_fx_sum(&self, f: impl Fn(&RelicDef) -> i32) -> i32 {
        self.relics.iter().map(|r| f(r)).sum()
    }
}

pub struct RewardState {
    pub gold: i32,
    pub gold_taken: bool,
    /// 拿 Boss 遗物时补进来的额外金币(小房子的 50 金):原版是往奖励屏再塞一条,
    /// 所以第一笔已在手时它单独算一条,拿掉才算到手
    pub extra_gold: i32,
    pub extra_gold_taken: bool,
    pub cards: Vec<CardInstance>,
    pub card_taken: bool,
    /// 排队的后续卡牌三选一(浑天仪的五组、祈祷轮的额外一组):
    /// 拿完(或跳过)当前这一组就把下一组顶上来
    pub queued: Vec<Vec<CardInstance>>,
    pub relic: Option<&'static RelicDef>,
    /// Boss 的三件候选(普通奖励/精英/事件用 relic,这里放 Boss 三选一)
    pub relic_choices: Vec<&'static RelicDef>,
    pub relic_taken: bool,
    pub potions: Vec<&'static PotionDef>,
    /// 和 potions 一一对应:哪几瓶已经被拿走(药水栏满时也算处理过)
    pub potion_taken: Vec<bool>,
    /// 打通燃烧精英掉落的绿钥匙(还没拿就是 true)
    pub emerald_key: bool,
    pub index: usize,
    /// 拿完(或跳过)之后去哪个界面
    pub next: Screen,
}

impl RewardState {
    /// 空奖励屏:金币/卡牌/遗物都按"已拿走"处理,只有调用方后来填进去的条目会显示
    fn empty(next: Screen) -> RewardState {
        RewardState {
            gold: 0,
            gold_taken: true,
            extra_gold: 0,
            extra_gold_taken: true,
            cards: Vec::new(),
            card_taken: true,
            queued: Vec::new(),
            relic: None,
            relic_choices: Vec::new(),
            relic_taken: true,
            potions: Vec::new(),
            potion_taken: Vec::new(),
            emerald_key: false,
            index: 0,
            next,
        }
    }
}

/// 奖励行:UI 与选择都按这个顺序来
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RewardSlot {
    Gold,
    /// 额外一条金币(小房子)
    ExtraGold,
    Card(usize),
    /// 单件遗物奖励(精英/事件/宝箱给的那件)
    Relic,
    /// Boss 三选一里的第 i 件
    RelicChoice(usize),
    /// 奖励屏里的第 i 瓶药水
    Potion(usize),
    /// 燃烧精英的绿钥匙
    EmeraldKey,
}

pub enum ShopItem {
    Card(CardInstance, i32),
    Relic(&'static RelicDef, i32),
    Potion(&'static PotionDef, i32),
    Remove(i32),
}

impl ShopItem {
    pub fn price(&self) -> i32 {
        match self {
            ShopItem::Card(_, p)
            | ShopItem::Relic(_, p)
            | ShopItem::Potion(_, p)
            | ShopItem::Remove(p) => *p,
        }
    }
}

pub struct ShopState {
    pub items: Vec<ShopItem>,
    /// 每格的种类:信使补货时按同种类重掷一格
    pub kinds: Vec<ShopKind>,
    pub sold: Vec<bool>,
    pub index: usize,
    pub removes: u32,
}

/// 商店格子的种类(信使 The Courier 补货时决定重掷什么)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShopKind {
    /// 本职业牌格
    ClassCard,
    /// 无色牌格
    ColorlessCard,
    Relic,
    Potion,
    /// 删牌服务不补货
    Remove,
}

/// 三把钥匙:进第四章的门票.燃烧精英给绿钥匙,营火"回忆"给红钥匙,
/// 非 Boss 宝箱里可以拿蓝钥匙(拿走就没了那件遗物).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Keys {
    pub emerald: bool,
    pub ruby: bool,
    pub sapphire: bool,
}

/// 宝箱的大小
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChestSize {
    Small,
    Medium,
    Large,
}

/// 进宝箱房时掷出来的箱子状态
#[derive(Clone, Copy, Debug)]
pub struct Chest {
    pub size: ChestSize,
    pub gold_present: bool,
    pub tier: RelicTier,
    /// 这个箱子是空的(饥肠辘辘之脸)
    pub empty: bool,
    /// 箱子已经开过了:开箱时若插进选牌界面(瓶装类遗物),回来再点一次是"前进"
    pub opened: bool,
}

pub struct EventState {
    pub def: &'static EventDef,
    /// Neow 专用:掷出来的四个选项(普通事件是空表)
    pub neow_options: Vec<NeowOption>,
    pub index: usize,
    /// 已选结果,展示完才能离开
    pub result: Option<String>,
    /// 翻牌小游戏(match_and_keep)的棋盘;其它事件是 None
    pub match_keep: Option<MatchKeep>,
    /// the_library 的 Read:掷好的 20 张候选;选走一张之前一直摆在这里,其余事件是 None
    pub library: Option<LibraryOffer>,
    /// 可反复尝试的事件(废料泥怪)已经试过几次;别的用不上
    pub attempts: u32,
    /// 事件当前在哪一屏(参考实现 room.screen):golden_idol 靠它切换陷阱屏的选项
    pub screen: Option<&'static str>,
    /// dead_adventurer 进房时掷出来的奖池与伏击遭遇;其它事件是 None
    pub adv: Option<DeadAdventurerData>,
    /// 再会事件进房时掷好的交易目标(参考实现 onEnter);其它事件是 None
    pub wma: Option<WeMeetAgainData>,
    /// N'loth 进房时掷好的两件供奉遗物(参考实现 onEnter);其它事件是 None
    pub nloth: Option<NlothData>,
    /// Designer In-Spire 进房时掷好的服务变体(参考实现 onEnter);其它事件是 None
    pub designer: Option<DesignerData>,
    /// 会说话的骷髅:三个购买项各自已经买过几次(涨价步数)
    pub skull: [u32; 3],
}

/// the_library 的 Read 摆出来的 20 张候选(参考实现 requestOptionChoice 的 extra.cards):
/// 挑走一张时把 note 当结果文本显示
#[derive(Clone, Debug)]
pub struct LibraryOffer {
    pub cards: Vec<CardInstance>,
    pub note: &'static str,
}

impl EventState {
    /// 事件屏的最小状态:Neow 的选项、翻牌棋盘、各事件的进房数据由调用方另填
    pub fn new(def: &'static EventDef) -> EventState {
        EventState {
            def,
            neow_options: Vec::new(),
            index: 0,
            result: None,
            match_keep: None,
            library: None,
            attempts: 0,
            screen: None,
            adv: None,
            wma: None,
            nloth: None,
            designer: None,
            skull: [0; 3],
        }
    }
}

/// dead_adventurer 的事件状态(参考实现 room.data):
/// 进房时洗一次的三格奖池、这次伏击用哪种精英、已经搜过几次.
#[derive(Clone, Copy, Debug)]
pub struct DeadAdventurerData {
    pub rewards: [&'static str; 3],
    pub encounter: &'static str,
    pub phase: u32,
}

/// "再会"事件进房时掷好的三个交易目标(参考实现 onEnter):
/// 药水格、金币数额、要交出去的牌下标;对应的选项不可用时是 None.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeMeetAgainData {
    pub potion_slot: Option<usize>,
    pub gold_amount: Option<i32>,
    pub card_idx: Option<usize>,
}

/// N'loth 进房时掷好的两件供奉遗物(参考实现 onEnter):
/// 身上遗物的下标洗一遍后取前两件;身上不足两件时对应项是 None.
#[derive(Clone, Copy, Debug, Default)]
pub struct NlothData {
    pub offer_a: Option<&'static str>,
    pub offer_b: Option<&'static str>,
}

/// Designer In-Spire 进房时掷好的两个服务变体(参考实现 onEnter):
/// true 走"自己选一张",false 走"随机两张".
#[derive(Clone, Copy, Debug, Default)]
pub struct DesignerData {
    pub upgrade_choice: bool,
    pub cleanup_choice: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Stats {
    pub fights: u32,
    pub elites: u32,
    pub bosses: u32,
    pub turns: u32,
    pub damage_dealt: i32,
    pub potions_used: u32,
}

/// 开局按档次洗好的五个遗物池.每个档次一个池子,取走即从池子里删掉,
/// 抽干后按兜底链退到下一档(参考实现 obtainRelicFromPool).
#[derive(Default)]
pub struct RelicPools {
    pub common: Vec<&'static RelicDef>,
    pub uncommon: Vec<&'static RelicDef>,
    pub rare: Vec<&'static RelicDef>,
    pub shop: Vec<&'static RelicDef>,
    pub boss: Vec<&'static RelicDef>,
}

impl RelicPools {
    /// 会进池子的五个档次(起始/事件/兜底档不进池子)
    pub const TIERS: [RelicTier; 5] = [
        RelicTier::Common,
        RelicTier::Uncommon,
        RelicTier::Rare,
        RelicTier::Shop,
        RelicTier::Boss,
    ];

    fn slot(&self, tier: RelicTier) -> Option<&Vec<&'static RelicDef>> {
        match tier {
            RelicTier::Common => Some(&self.common),
            RelicTier::Uncommon => Some(&self.uncommon),
            RelicTier::Rare => Some(&self.rare),
            RelicTier::Shop => Some(&self.shop),
            RelicTier::Boss => Some(&self.boss),
            _ => None,
        }
    }

    fn slot_mut(&mut self, tier: RelicTier) -> Option<&mut Vec<&'static RelicDef>> {
        match tier {
            RelicTier::Common => Some(&mut self.common),
            RelicTier::Uncommon => Some(&mut self.uncommon),
            RelicTier::Rare => Some(&mut self.rare),
            RelicTier::Shop => Some(&mut self.shop),
            RelicTier::Boss => Some(&mut self.boss),
            _ => None,
        }
    }

    fn set(&mut self, tier: RelicTier, pool: Vec<&'static RelicDef>) {
        if let Some(slot) = self.slot_mut(tier) {
            *slot = pool;
        }
    }

    /// 所有池子(读档后排掉已拿到的遗物要用)
    fn all_mut(&mut self) -> [&mut Vec<&'static RelicDef>; 5] {
        [
            &mut self.common,
            &mut self.uncommon,
            &mut self.rare,
            &mut self.shop,
            &mut self.boss,
        ]
    }
}

/// 池子抽干时的兜底链(参考实现 tierChain)
fn tier_chain(tier: RelicTier) -> &'static [RelicTier] {
    match tier {
        RelicTier::Common => &[RelicTier::Common, RelicTier::Uncommon, RelicTier::Rare],
        RelicTier::Uncommon => &[RelicTier::Uncommon, RelicTier::Rare],
        RelicTier::Rare => &[RelicTier::Rare],
        RelicTier::Shop => &[RelicTier::Shop, RelicTier::Uncommon, RelicTier::Rare],
        RelicTier::Boss => &[RelicTier::Boss],
        _ => &[],
    }
}

/// 事件层还在用稀有度表示遗物档次,这里换算一下
fn tier_of_rarity(rarity: Rarity) -> RelicTier {
    match rarity {
        Rarity::Basic => RelicTier::Starter,
        Rarity::Common => RelicTier::Common,
        Rarity::Uncommon => RelicTier::Uncommon,
        Rarity::Rare => RelicTier::Rare,
        Rarity::Special => RelicTier::Special,
    }
}

pub struct Run {
    pub seed: u64,
    /// 开局选的角色的语料 id
    pub character: &'static str,
    /// 飞升等级(0 = 关,1..=20).开局定下来,随存档走
    pub ascension: u32,
    /// 第几场战斗(每次开打 +1),表现层用它判断要不要重新拍快照
    pub fight_seq: u64,
    /// 赢了之后还要在战场上多停几帧(>0 表示正在停,满了才进奖励)
    pub win_hold: u8,
    pub streams: RngRegistry,
    pub player: Player,
    pub map: ActMap,
    /// 现在爬到第几章(1..=4).第四章是钥匙门后的心脏
    pub act: u32,
    /// 全局层号(参考实现的 run.floor):每进一个房间 +1,切幕不重置.
    /// 每层的随机流用它重种(seed + 这个数),所以跨幕必须一直累加.
    floor_num: u32,
    /// 三把钥匙(绿/红/蓝):集齐才能开第三章 Boss 后面的门进第四章
    pub keys: Keys,
    /// 本局这条路的 Boss:开局定下来,地图上直接写名字
    pub boss_enc: &'static Encounter,
    /// 本局的第二个 Boss(飞升 20 的第三章双 Boss 用;平时不用)
    boss2_enc: &'static Encounter,
    /// 飞升 20:第三章的第二个 Boss 是否已经打过(打过就不再触发)
    a20_second_boss: bool,
    /// 这一章还没打的怪房间名单(monsterRng 一次生成,按顺序消耗)
    monster_list: Vec<&'static str>,
    /// 这一章还没打的精英名单
    elite_list: Vec<&'static str>,
    /// 开局用 neowRng 掷出来的四个祝福选项
    neow_options: Vec<NeowOption>,
    /// 未知房判成怪/商店/宝箱的概率(参考实现 blizzard 的三档)
    monster_chance: f32,
    shop_chance: f32,
    treasure_chance: f32,
    /// 上一个房间是商店:紧接着的未知房不再判成商店
    last_room_was_shop: bool,
    /// 本章的事件池(抽走即移除)
    event_pool: Vec<&'static str>,
    /// 本章的神龛池(抽走即移除)
    shrine_pool: Vec<&'static str>,
    /// 本局的一次性事件池(抽走即移除)
    one_time_pool: Vec<&'static str>,
    /// 本局买过几次删牌服务(价格 75 + 25/次)
    removes_purchased: u32,
    /// Neow's Lament 还剩几场战斗(前几场开打时敌人只剩 1 血)
    neow_lament: u8,
    /// 俄罗斯套娃还剩几个宝箱会多给一件
    chest_extra_left: i32,
    /// 饥肠辘辘之脸还剩几个宝箱是空的
    chest_empty_left: i32,
    /// 御守还剩几次挡诅咒
    omamori_charges: i32,
    /// 银行家之躯:在商店花过钱之后就不再每层给钱
    maw_bank_spent: bool,
    /// 开打前刚在营火休息过(古代茶具)
    rested: bool,
    /// 吉利亚已经举过几次铁(上限由遗物的 rest_lift_max 给)
    relic_lifts: i32,
    /// 整局持续的遗物计数器(笔尖/快乐花/薰香/日晷/双节棍/墨水瓶),随存档走
    pub relic_counters: RunRelicCounters,
    /// 羽翼靴还能无视路径飞几次
    wing_boots_left: i32,
    /// 小箱子:已经进过几个 ? 房间
    unknown_rooms_seen: u32,
    /// 药水掉落的保底值(参考实现里的 potionChance:每次没掉 +10,掉了 -10)
    potion_chance: i32,
    /// 卡牌稀有度的保底值(参考实现里的 cardRarityFactor:抽到稀有重置 5,普通 -1,下限 -40)
    card_rarity_factor: i32,
    /// 便条事件存卡的文件路径覆盖(测试用);None 用存档目录里的 note.card
    note_path: Option<std::path::PathBuf>,
    /// 便条存卡是否落盘:headless 对拍/回放关掉,免得读到玩家存档或把回放写进存档
    note_persist: bool,
    /// 玩家当前所在节点;None 表示还没上路
    pub pos: Option<usize>,
    /// 这一局走过的节点(按顺序),地图上走过的房间统一给底色
    pub path: Vec<usize>,
    pub floor_reached: usize,
    pub screen: Screen,
    pub combat: Option<Combat>,
    pub reward: Option<RewardState>,
    pub shop: Option<ShopState>,
    pub event: Option<EventState>,
    pub picker: Option<Picker>,
    pub treasure: Option<&'static RelicDef>,
    /// 当前宝箱房的箱子状态(不在宝箱房就是 None)
    pub chest: Option<Chest>,
    pub rest_index: usize,
    pub stats: Stats,
    /// 一整局发生过的所有事,用 H 翻看
    pub history: Vec<HistoryEntry>,
    /// 已经抄进历史的战斗日志序号
    combat_log_seen: u64,
    /// 本局还没出现过的遗物(按五个档次分别洗好的池子)
    relic_pools: RelicPools,
    last_encounter: &'static str,
    /// 打赢这一场事件战斗后要回到的事件那一屏
    pending_event: Option<&'static EventDef>,
    /// dead_adventurer 的伏击:打赢之后按这份事件状态发奖励屏(参考实现 onCombatVictory)
    pending_adv: Option<DeadAdventurerData>,
    /// 这一场事件战斗的奖励方案(打完即清空)
    combat_reward: Option<&'static CombatReward>,
}

/// 存档里的卡牌记号:id、升级加 "+"、可多次升级的带等级(id+3)
fn card_token(c: &CardInstance) -> String {
    if c.plus > 1 {
        format!("{}+{}", c.def.id, c.plus)
    } else if c.upgraded {
        format!("{}+", c.def.id)
    } else {
        c.def.id.to_string()
    }
}

/// 按 id 找卡牌:先认事件专用牌,再认卡池
fn card_def_any(id: &str) -> &'static CardDef {
    crate::core::events::event_card(id)
        .or_else(|| cards::card_def(id))
        .unwrap_or_else(|| panic!("unknown card id: {id}"))
}

/// 按 id 找遗物:先认事件专用遗物,再认遗物池
fn relic_def_any(id: &str) -> &'static RelicDef {
    relic_def_any_opt(id).unwrap_or_else(|| panic!("unknown relic id: {id}"))
}

/// 存档里的事件 id 换回 'static 的那一份
fn event_id_static(id: &str) -> Option<&'static str> {
    crate::core::events::EVENTS
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.id)
}

/// 同上,但认不出返回 None(读存档用)
fn relic_def_any_opt(id: &str) -> Option<&'static RelicDef> {
    crate::core::events::event_relic(id).or_else(|| relics::relic_def(id))
}

/// 反向解析牌堆
fn parse_cards(text: &str) -> Result<Vec<CardInstance>, String> {
    let mut out = Vec::new();
    for item in text.split(',').filter(|s| !s.is_empty()) {
        let item = item.trim();
        let (id, plus) = match item.split_once('+') {
            Some((id, rest)) => (
                id,
                if rest.is_empty() {
                    1
                } else {
                    rest.parse::<u8>().unwrap_or(1)
                },
            ),
            None => (item, 0),
        };
        let def = crate::core::events::event_card(id)
            .or_else(|| cards::card_def(id))
            .ok_or_else(|| format!("存档里的卡 {id} 不认识"))?;
        let mut inst = CardInstance::new(def);
        for _ in 0..plus {
            inst.upgrade();
        }
        out.push(inst);
    }
    Ok(out)
}

impl Run {
    /// 默认角色(铁甲战士)开一局,测试和 :new 用
    pub fn new(seed: u64) -> Run {
        let ch = roster::find("ironclad").expect("ironclad 必须在语料里");
        Run::new_for(seed, ch).expect("铁甲战士的起始牌组必须是已实现的")
    }

    /// 按角色开一局(飞升 0):起始牌组/血量/金币/遗物都来自语料
    pub fn new_for(seed: u64, ch: &'static corpus::CharacterInfo) -> Result<Run, String> {
        Run::new_for_asc(seed, ch, 0)
    }

    /// 角色在飞升 14 掉的生命上限(参考实现 a14HpLoss:铁甲 -5,其余 -4)
    fn a14_hp_loss(ch: &'static corpus::CharacterInfo) -> i32 {
        if ch.id == "ironclad" {
            5
        } else {
            4
        }
    }

    /// 按角色 + 飞升等级开一局:起始牌组/血量/金币/遗物都来自语料,
    /// 飞升 6/10/11/14 的差异(开局受伤、初始诅咒、药水槽、上限)在这里落地
    pub fn new_for_asc(
        seed: u64,
        ch: &'static corpus::CharacterInfo,
        asc: u32,
    ) -> Result<Run, String> {
        let missing = roster::missing_cards(ch);
        if !missing.is_empty() {
            return Err(format!("{} 还没实现: {}", ch.name, missing.join(" ")));
        }
        if !roster::has_starter_relic(ch) {
            return Err(format!("{} 的起始遗物 {} 还没实现", ch.name, ch.relic));
        }
        let mut streams = RngRegistry::new(seed);
        let mut deck: Vec<CardInstance> = Vec::new();
        for (id, n) in ch.deck {
            for _ in 0..*n {
                deck.push(cards::card(id));
            }
        }
        // 飞升 10:开局多一张"飞升者之灾"(不可移除的诅咒)
        if asc >= 10 {
            deck.push(cards::card("ascenders_bane"));
        }
        let starter = relics::relic_def_or_panic(ch.relic);
        // 遗物池开局按 普通/罕见/稀有/商店/Boss 五个档次各洗一遍,
        // 每档消耗 relicRng 的一个 long(顺序与参考实现一致).
        // 池子内容 = 共享遗物 + 本职业专属遗物,顺序照 RELICS 表.
        let mut relic_pools = RelicPools::default();
        for tier in RelicPools::TIERS {
            let mut pool = relics::pool_for(tier, ch.color);
            let java_seed = streams.run(RunStream::RelicRng).random_long();
            java_shuffle(&mut pool, &mut JavaRandom::new(java_seed));
            relic_pools.set(tier, pool);
        }

        // 遭遇名单(monsterRng)与地图(mapRng)都按参考实现的顺序掷
        let lists = enemies::generate_encounters(1, streams.run(RunStream::MonsterRng));
        streams.reseed_map(1);
        // 第一章一定标一个燃烧精英
        let map = ActMap::generate(streams.map_rng(), true, asc);
        // 本局的 Boss 是 monsterRng 洗出来的那一条;第二条留给飞升 20 的双 Boss
        let boss_enc: &'static Encounter = enemies::resolve(lists.boss[0]);
        let boss2_enc: &'static Encounter =
            enemies::resolve(*lists.boss.get(1).unwrap_or(&lists.boss[0]));
        // 飞升 14 先降上限,飞升 6 再按(降过的)上限扣 10%(参考实现顺序)
        let max_hp = ch.max_hp - if asc >= 14 { Self::a14_hp_loss(ch) } else { 0 };
        let hp = if asc >= 6 {
            (max_hp as f32 * 0.9).round() as i32
        } else {
            max_hp
        };
        let potion_slots = if asc >= 11 { 2 } else { POTION_SLOTS };
        let mut run = Run {
            seed,
            character: ch.id,
            ascension: asc,
            fight_seq: 0,
            win_hold: 0,
            streams,
            player: Player {
                hp,
                max_hp,
                gold: ch.gold,
                deck,
                relics: vec![starter],
                potions: vec![None; potion_slots],
            },
            map,
            act: 1,
            floor_num: 0,
            keys: Keys::default(),
            boss_enc,
            boss2_enc,
            a20_second_boss: false,
            monster_list: lists.monster,
            elite_list: lists.elite,
            neow_options: Vec::new(),
            monster_chance: UNKNOWN_BASE.0,
            shop_chance: UNKNOWN_BASE.1,
            treasure_chance: UNKNOWN_BASE.2,
            last_room_was_shop: false,
            event_pool: ACT1_EVENTS.to_vec(),
            shrine_pool: ACT1_SHRINES.to_vec(),
            one_time_pool: one_time_event_pool(asc),
            removes_purchased: 0,
            neow_lament: 0,
            chest_extra_left: 0,
            chest_empty_left: 0,
            omamori_charges: 0,
            maw_bank_spent: false,
            rested: false,
            relic_lifts: 0,
            relic_counters: RunRelicCounters::default(),
            wing_boots_left: 0,
            unknown_rooms_seen: 0,
            potion_chance: 0,
            card_rarity_factor: CARD_RARITY_PITY_START,
            note_path: None,
            note_persist: true,
            pos: None,
            path: Vec::new(),
            floor_reached: 0,
            screen: Screen::Map,
            combat: None,
            reward: None,
            shop: None,
            event: None,
            picker: None,
            treasure: None,
            chest: None,
            rest_index: 0,
            stats: Stats::default(),
            history: Vec::new(),
            combat_log_seen: 0,
            relic_pools,
            last_encounter: "",
            pending_event: None,
            pending_adv: None,
            combat_reward: None,
        };
        // 起始遗物的拾取效果
        run.apply_relic_pickup(starter);
        // 开局先掷 Neow 的四个选项(neowRng)
        run.neow_options = crate::core::events::neow_options(run.streams.run(RunStream::NeowRng));
        run.say(format!("seed {seed}: climb the spire"));
        Ok(run)
    }

    /// 存档文本:只存"这一层刚开始"的状态(地图界面才存)
    pub fn save_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("seed={}\n", self.seed));
        out.push_str(&format!("char={}\n", self.character));
        out.push_str(&format!("ascension={}\n", self.ascension));
        out.push_str(&format!("a20_second={}\n", self.a20_second_boss));
        out.push_str(&format!("hp={}\n", self.player.hp));
        out.push_str(&format!("max_hp={}\n", self.player.max_hp));
        out.push_str(&format!("gold={}\n", self.player.gold));
        out.push_str(&format!("act={}\n", self.act));
        out.push_str(&format!("boss={}\n", self.boss_enc.id));
        out.push_str(&format!("boss2={}\n", self.boss2_enc.id));
        out.push_str(&format!("floor_num={}\n", self.floor_num));
        out.push_str(&format!(
            "keys={}{}{}\n",
            self.keys.emerald as u8, self.keys.ruby as u8, self.keys.sapphire as u8
        ));
        // 具名流的完整状态(15 条).老存档只有一条 xoshiro 的 rng=,读的时候会明确报错
        out.push_str(&self.streams.save_text());
        out.push_str(&format!("potion_pity={}\n", self.potion_chance));
        out.push_str(&format!("card_factor={}\n", self.card_rarity_factor));
        out.push_str(&format!("monsters={}\n", self.monster_list.join(",")));
        out.push_str(&format!("elites={}\n", self.elite_list.join(",")));
        // 事件池(抽走即移除)与未知房的概率:不存的话读档会把抽过的再抽一遍
        out.push_str(&format!("events={}\n", self.event_pool.join(",")));
        out.push_str(&format!("shrines={}\n", self.shrine_pool.join(",")));
        out.push_str(&format!("one_time={}\n", self.one_time_pool.join(",")));
        out.push_str(&format!(
            "blizzard={},{},{}\n",
            self.monster_chance, self.shop_chance, self.treasure_chance
        ));
        out.push_str(&format!("removes={}\n", self.removes_purchased));
        out.push_str(&format!("last_shop={}\n", self.last_room_was_shop));
        out.push_str(&format!("neow_lament={}\n", self.neow_lament));
        out.push_str(&format!("relic_lifts={}\n", self.relic_lifts));
        out.push_str(&format!("wing_boots={}\n", self.wing_boots_left));
        // 跨战斗的遗物计数器.战斗现场存盘时以战斗里那份为准(Run 上的副本这时还没收回)
        let rc = self
            .combat
            .as_ref()
            .map(|c| c.rs.run_counters())
            .unwrap_or(self.relic_counters);
        out.push_str(&format!(
            "relic_counters={},{},{},{},{},{}\n",
            rc.pen_nib, rc.happy_flower, rc.incense, rc.sundial, rc.attacks_total, rc.cards_total
        ));
        if let Some(chest) = self.chest {
            let size = match chest.size {
                ChestSize::Small => "small",
                ChestSize::Medium => "medium",
                ChestSize::Large => "large",
            };
            out.push_str(&format!(
                "chest={}:{}:{}:{}\n",
                size,
                chest.gold_present,
                chest.tier.name(),
                chest.empty
            ));
        }
        // 宝箱相关的一次性次数(俄罗斯套娃 / 饥肠辘辘之脸):不存读档就白拿了
        out.push_str(&format!(
            "chest_extras={},{}\n",
            self.chest_extra_left, self.chest_empty_left
        ));
        if let Some(t) = self.treasure {
            out.push_str(&format!("treasure={}\n", t.id));
        }
        out.push_str(&format!(
            "pos={}\n",
            self.pos.map(|p| p.to_string()).unwrap_or_else(|| "none".to_string())
        ));
        let path: Vec<String> = self.path.iter().map(|n| n.to_string()).collect();
        out.push_str(&format!("path={}\n", path.join(",")));
        out.push_str(&format!("floor={}\n", self.floor_reached));
        // 每张牌记成 id:升级:是否被封进瓶子
        let deck: Vec<String> = self
            .player
            .deck
            .iter()
            .map(|c| {
                format!(
                    "{}:{}:{}",
                    c.def.id,
                    if c.upgraded { 1 } else { 0 },
                    if c.bottled { 1 } else { 0 }
                )
            })
            .collect();
        out.push_str(&format!("deck={}\n", deck.join(",")));
        let relics: Vec<&str> = self.player.relics.iter().map(|r| r.id).collect();
        out.push_str(&format!("relics={}\n", relics.join(",")));
        let potions: Vec<&str> = self
            .player
            .potions
            .iter()
            .map(|p| p.map(|d| d.id).unwrap_or("-"))
            .collect();
        out.push_str(&format!("potions={}\n", potions.join(",")));
        // 战斗现场(测试存档用):老存档没这几行,读到没有就照旧重建地图
        if let Some(c) = self.combat.as_ref() {
            // 抽牌堆序列化时"顶牌在下标 0";老存档是反的,靠这一行认出来并明确拒绝
            out.push_str("combat_pile_order=top\n");
            out.push_str(&format!("combat_encounter={}\n", c.encounter_id));
            out.push_str(&format!("combat_turn={}\n", c.turn));
            out.push_str(&format!("combat_energy={}\n", c.energy));
            out.push_str(&format!("combat_max_energy={}\n", c.max_energy));
            out.push_str(&format!("combat_hp={}\n", c.player.hp));
            out.push_str(&format!("combat_block={}\n", c.player.block));
            for (k, pile) in [
                ("hand", &c.hand),
                ("draw", &c.draw),
                ("discard", &c.discard),
                ("exhaust", &c.exhaust),
            ] {
                let list: Vec<String> = pile.iter().map(card_token).collect();
                out.push_str(&format!("combat_{k}={}\n", list.join(",")));
            }
            let foes: Vec<String> = c
                .enemies
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}:{}:{}",
                        e.hp,
                        e.block,
                        e.next_move,
                        if e.alive() { 1 } else { 0 }
                    )
                })
                .collect();
            out.push_str(&format!("combat_enemies={}\n", foes.join(",")));
        }
        out
    }

    /// 从存档文本恢复一局
    pub fn from_save(text: &str) -> Result<Run, String> {
        let mut seed = 0u64;
        let mut char_id = "ironclad".to_string();
        let mut ascension = 0u32;
        let mut num: Vec<(&str, &str)> = Vec::new();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            num.push((k, v));
            if k == "seed" {
                seed = v.trim().parse().map_err(|_| "存档里的种子坏了".to_string())?;
            } else if k == "char" {
                char_id = v.trim().to_string();
            } else if k == "ascension" {
                ascension = crate::core::ascension::clamp(v.trim().parse().unwrap_or(0));
            }
        }
        let get = |k: &str| -> Option<&str> { num.iter().find(|(a, _)| *a == k).map(|(_, b)| *b) };
        let int = |k: &str, d: i32| -> i32 { get(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d) };
        let ch = roster::find(&char_id).ok_or_else(|| format!("存档里的角色 {char_id} 不认识"))?;
        let mut run = Run::new_for_asc(seed, ch, ascension)?;
        run.player.hp = int("hp", run.player.hp);
        run.player.max_hp = int("max_hp", run.player.max_hp);
        run.player.gold = int("gold", run.player.gold);
        if let Some(v) = get("keys") {
            let b: Vec<char> = v.trim().chars().collect();
            if b.len() == 3 {
                run.keys = Keys {
                    emerald: b[0] == '1',
                    ruby: b[1] == '1',
                    sapphire: b[2] == '1',
                };
            }
        }
        run.floor_reached = int("floor", 0).max(0) as usize;
        run.act = int("act", 1).clamp(1, 4) as u32;
        run.floor_num = int("floor_num", run.floor_reached as i32 + 1).max(0) as u32;
        if let Some(id) = get("boss") {
            run.boss_enc = enemies::resolve(id.trim());
        }
        if let Some(id) = get("boss2") {
            run.boss2_enc = enemies::resolve(id.trim());
        }
        run.a20_second_boss = get("a20_second").map(|v| v.trim() == "true").unwrap_or(false);
        // 地图按当前章的种子重生成(第一章 seed+1,第二章 seed+200,第三章 seed+600;
        // 第四章是定死的).这一步必须在读随机流之前做:读档会把 mapRng 的状态覆盖回来.
        if run.act == 4 {
            run.map = ActMap::act4();
        } else {
            run.streams.reseed_map(run.act);
            run.map =
                ActMap::generate(run.streams.map_rng(), run.act == 1 || !run.keys.emerald, run.ascension);
        }
        // 旧存档的随机状态是单条 xoshiro 流,和现在的具名流对不上:
        // 直接报错,别静默接着跑出一局错的游戏
        if get("rng").is_some() && !RngRegistry::has_any_stream_line(text) {
            return Err(
                "这份存档记的是旧版随机状态(单条 xoshiro 流),与现在的具名流对不上:请重新开一局".to_string(),
            );
        }
        for (k, v) in &num {
            if crate::rng::is_stream_key(k) {
                run.streams.load_line(k, v)?;
            }
        }
        run.potion_chance = int("potion_pity", 0);
        run.card_rarity_factor = int("card_factor", CARD_RARITY_PITY_START);
        if let Some(v) = get("monsters") {
            run.monster_list = v
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| enemies::resolve(s).id)
                .collect();
        }
        if let Some(v) = get("elites") {
            run.elite_list = v
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| enemies::resolve(s).id)
                .collect();
        }
        if let Some(v) = get("events") {
            run.event_pool = v.split(',').filter_map(event_id_static).collect();
        }
        if let Some(v) = get("shrines") {
            run.shrine_pool = v.split(',').filter_map(event_id_static).collect();
        }
        if let Some(v) = get("one_time") {
            run.one_time_pool = v.split(',').filter_map(event_id_static).collect();
        }
        if let Some(v) = get("blizzard") {
            let parts: Vec<&str> = v.split(',').collect();
            if parts.len() == 3 {
                run.monster_chance = parts[0].trim().parse().unwrap_or(UNKNOWN_BASE.0);
                run.shop_chance = parts[1].trim().parse().unwrap_or(UNKNOWN_BASE.1);
                run.treasure_chance = parts[2].trim().parse().unwrap_or(UNKNOWN_BASE.2);
            }
        }
        run.removes_purchased = int("removes", 0).max(0) as u32;
        run.last_room_was_shop = get("last_shop").map(|v| v.trim() == "true").unwrap_or(false);
        run.neow_lament = int("neow_lament", 0).max(0) as u8;
        run.relic_lifts = int("relic_lifts", 0).max(0);
        run.wing_boots_left = int("wing_boots", 0).max(0);
        // 跨战斗的遗物计数器.老存档没有这一行,那时这些计数器本来是每场清零的,
        // 按 0 起就与旧行为一致(不会静默给出错的局面)
        if let Some(v) = get("relic_counters") {
            let parts: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if parts.len() != 6 {
                return Err("存档里的 relic_counters 字段坏了".to_string());
            }
            run.relic_counters = RunRelicCounters {
                pen_nib: parts[0].max(0),
                happy_flower: parts[1].max(0),
                incense: parts[2].max(0),
                sundial: parts[3].max(0),
                attacks_total: parts[4].max(0),
                cards_total: parts[5].max(0),
            };
        }
        if let Some(v) = get("chest") {
            let parts: Vec<&str> = v.split(':').collect();
            if parts.len() >= 3 {
                run.chest = Some(Chest {
                    empty: parts.get(3).map(|s| s.trim() == "true").unwrap_or(false),
                    size: match parts[0] {
                        "small" => ChestSize::Small,
                        "medium" => ChestSize::Medium,
                        _ => ChestSize::Large,
                    },
                    gold_present: parts[1] == "true",
                    tier: relic_tier_from_name(parts[2]),
                    opened: false,
                });
            }
        }
        if let Some(v) = get("chest_extras") {
            let parts: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if parts.len() == 2 {
                run.chest_extra_left = parts[0].max(0);
                run.chest_empty_left = parts[1].max(0);
            }
        }
        if let Some(v) = get("treasure") {
            run.treasure = relic_def_any_opt(v);
        }
        if let Some(v) = get("deck") {
            let mut deck: Vec<CardInstance> = Vec::new();
            for item in v.split(',').filter(|s| !s.is_empty()) {
                // id:升级[:封装];老存档只有前两段
                let mut bits = item.split(':');
                let id = bits.next().unwrap_or(item);
                let up = bits.next().unwrap_or("0");
                let bottled = bits.next().unwrap_or("0") == "1";
                let Some(def) = crate::core::events::event_card(id).or_else(|| cards::card_def(id))
                else {
                    return Err(format!("存档里的卡 {id} 不认识"));
                };
                let mut inst = cards::card(def.id);
                if up.trim() == "1" {
                    inst.upgrade();
                }
                inst.bottled = bottled;
                deck.push(inst);
            }
            if !deck.is_empty() {
                run.player.deck = deck;
            }
        }
        if let Some(v) = get("relics") {
            let mut relics_out: Vec<&'static RelicDef> = Vec::new();
            for id in v.split(',').filter(|s| !s.is_empty()) {
                let Some(def) = relic_def_any_opt(id) else {
                    return Err(format!("存档里的遗物 {id} 不认识"));
                };
                relics_out.push(def);
            }
            if !relics_out.is_empty() {
                run.player.relics = relics_out;
            }
        }
        if let Some(v) = get("potions") {
            let n = run.player.potions.len();
            let mut slots: Vec<Option<&'static PotionDef>> = vec![None; n];
            for (i, id) in v.split(',').enumerate().take(n) {
                if id == "-" || id.is_empty() {
                    continue;
                }
                let def = potions::by_id(id)
                    .ok_or_else(|| format!("存档里的药水 {id} 不认识"))?;
                slots[i] = Some(def);
            }
            run.player.potions = slots;
        }
        if let Some(v) = get("pos") {
            run.pos = if v.trim() == "none" {
                None
            } else {
                v.trim().parse::<usize>().ok()
            };
        }
        if let Some(v) = get("path") {
            run.path = v
                .split(',')
                .filter(|s| !s.is_empty())
                .filter_map(|x| x.trim().parse().ok())
                .collect();
        }
        // 测试存档:带战斗现场就照原样恢复(不用从种子重放)
        if let Some(enc_id) = get("combat_encounter") {
            // 旧存档的抽牌堆是"顶牌记在末尾",和现在的数组朝向正好反过来.
            // 照旧读会把整堆读反,所以这里直接拒绝,让人重新存一份
            if get("combat_pile_order") != Some("top") {
                return Err(
                    "这份存档的战斗现场是旧版抽牌堆朝向(顶牌记在末尾),直接读会读反:请重新存一份"
                        .to_string(),
                );
            }
            if let Some(enc) = enemies::encounter_def(enc_id) {
                let setup = CombatSetup {
                    rested: run.rested,
                    hp: run.player.hp,
                    max_hp: run.player.max_hp,
                    deck: run.player.deck.clone(),
                    relics: run.player.relics.clone(),
                    gold: run.player.gold,
                    lift_strength: run.relic_lifts,
                    relic_counters: run.relic_counters,
                    curse_negate: run.omamori_charges,
                    asc: run.ascension,
                };
                let mut c = Combat::new(enc, setup, run.streams.clone());
                // 重建时的第一回合与开局的洗牌都会动这些计数器(还带一次日晷),
                // 存档里的那份才是真的,覆盖回去
                c.rs.set_run_counters(run.relic_counters);
                c.turn = int("combat_turn", 1).max(1) as u32;
                c.energy = int("combat_energy", c.energy);
                c.max_energy = int("combat_max_energy", c.max_energy);
                c.player.hp = int("combat_hp", run.player.hp);
                c.player.block = int("combat_block", 0);
                for (key, pile) in [("hand", 0), ("draw", 1), ("discard", 2), ("exhaust", 3)] {
                    let Some(v) = get(&format!("combat_{key}")) else {
                        continue;
                    };
                    let list = parse_cards(v)?;
                    match pile {
                        0 => c.hand = list,
                        1 => c.draw = list,
                        2 => c.discard = list,
                        _ => c.exhaust = list,
                    }
                }
                if let Some(v) = get("combat_enemies") {
                    for (i, part) in v.split(',').enumerate() {
                        let nums: Vec<i32> = part
                            .split(':')
                            .filter_map(|x| x.trim().parse().ok())
                            .collect();
                        if let Some(e) = c.enemies.get_mut(i) {
                            if let Some(hp) = nums.first() {
                                e.hp = *hp;
                            }
                            if let Some(b) = nums.get(1) {
                                e.block = *b;
                            }
                            if let Some(m) = nums.get(2) {
                                e.next_move = (*m).max(0) as usize;
                            }
                            if nums.get(3) == Some(&0) {
                                e.hp = 0;
                            }
                        }
                    }
                }
                if c.enemies.iter().all(|e| !e.alive()) {
                    c.phase = Phase::Won;
                }
                // 战斗的随机状态以存档为准(上面重建时又掷了几次血量与洗牌)
                c.streams = run.streams.clone();
                run.combat = Some(c);
                run.screen = Screen::Combat;
                run.win_hold = 0;
                run.sync_combat();
            }
        }
        // 还没出现过的遗物:在上面这条"开局洗好的池子"上排掉已经拿到的,
        // 不能重新按表重建,否则洗牌顺序就不是本局的了
        for pool in run.relic_pools.all_mut() {
            pool.retain(|r| !run.player.relics.iter().any(|o| o.id == r.id));
        }
        // 有战斗现场的就留在战斗里,别把 screen/combat 清掉
        if run.combat.is_none() {
            run.screen = Screen::Map;
        }
        run.event = None;
        if run.combat.is_none() {
            run.combat = None;
        }
        run.reward = None;
        run.shop = None;
        run.picker = None;
        run.treasure = None;
        run.say(format!("continued run, seed {seed}"));
        Ok(run)
    }

    /// 开局第一件事:Neow 的祝福(四个掷出来的选项,选一个)
    pub fn open_neow(&mut self) {
        let mut st = EventState::new(crate::core::events::neow());
        st.neow_options = self.neow_options.clone();
        self.event = Some(st);
        self.screen = Screen::Event;
    }

    // ---- 金标准测试用的几个观察口 ----

    /// Neow 掷出来的四个选项
    #[cfg(test)]
    pub fn neow_option_list(&self) -> &[NeowOption] {
        &self.neow_options
    }

    /// 连续判定 n 次未知房(每次一个 eventRng float 加一轮概率递增)
    #[cfg(test)]
    pub fn debug_unknown_rooms(&mut self, n: usize) -> Vec<&'static str> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.resolve_unknown_room().name());
        }
        out
    }

    /// 连续抽 n 次事件(抽签掷在 eventRng 副本上,不动主流)
    #[cfg(test)]
    pub fn debug_event_picks(&mut self, n: usize) -> Vec<Option<&'static str>> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.generate_event_id());
        }
        out
    }

    /// 调试钩子(--replay 的 `act n`):从当前幕一路切幕到第 n 幕开头,
    /// 走的就是 `begin_act`(幕切换的那套掷点),参考侧用同样的切幕驱动.
    pub fn debug_jump_act(&mut self, n: u32) {
        while self.act < n.max(1) {
            self.begin_act();
        }
    }

    /// 调试钩子(--replay 的 `keys all`):三把钥匙直接到手,用来开第四章的门.
    pub fn debug_grant_keys(&mut self) {
        self.keys = Keys {
            emerald: true,
            ruby: true,
            sapphire: true,
        };
    }

    /// 调试钩子(--replay 的 `hp n`):把生命与上限直接设成 n,
    /// 好让起手牌组也能在第三/四幕活着走到 Boss(纯状态,不掷点).
    pub fn debug_set_hp(&mut self, hp: i32) {
        self.player.max_hp = hp;
        self.player.hp = hp;
    }

    /// 调试钩子(事件沙盒):把全局层号设到 floor 并重种该层的掷点流.
    /// mindbloom 的 "I am Rich"/"I am Healthy" 按层号开关选项,需要它.
    pub fn debug_set_floor(&mut self, floor: u32) {
        self.floor_num = floor;
        self.floor_reached = floor as usize;
        self.streams.reseed_floor_streams(floor);
    }

    /// 当前全局层号(事件沙盒输出用)
    pub fn debug_floor(&self) -> u32 {
        self.floor_num
    }

    /// 调试钩子(--replay 的 `deck strong`):把牌组换成 10 张强化重锤.
    /// 只有 3 点能量时,最笨的出牌策略(先挑费用最低的)也打得出 32/回合,
    /// 刚好够破开第四章精英(盾与矛)的格挡、一路走到心脏(纯状态,不掷点).
    pub fn debug_set_strong_deck(&mut self) {
        let mut deck = Vec::with_capacity(10);
        for _ in 0..10 {
            let mut c = cards::card("bludgeon");
            c.upgrade();
            deck.push(c);
        }
        self.player.deck = deck;
    }

    /// 记一笔:既进历史记录,也更新界面上的提示
    fn say(&mut self, text: impl Into<String>) {
        self.push_history(text, HistoryKind::System);
    }

    fn push_history(&mut self, text: impl Into<String>, kind: HistoryKind) {
        self.history.push(HistoryEntry {
            text: text.into(),
            kind,
        });
        if self.history.len() > HISTORY_LIMIT {
            self.history.drain(0..HISTORY_TRIM);
        }
    }

    // ---- 地图 ----

    /// 地图上 Boss 那里要写的名字
    pub fn boss_name(&self) -> &'static str {
        enemies::encounter_name(self.boss_enc)
    }

    pub fn reachable(&self) -> Vec<usize> {
        self.map.reachable_from(self.pos)
    }

    pub fn floor(&self) -> usize {
        match self.pos {
            Some(i) => self.map.node(i).floor,
            None => 0,
        }
    }

    /// 进入一个节点
    pub fn enter_node(&mut self, idx: usize) -> Result<(), String> {
        if self.screen != Screen::Map {
            return Err("not on the map".to_string());
        }
        if !self.travel_options().contains(&idx) {
            return Err("that node is not reachable from here".to_string());
        }
        // 羽翼靴:飞过去要花一次充能
        if self.travel_is_fly(idx) {
            if self.wing_boots_left <= 0 {
                return Err("Wing Boots has no charges left".to_string());
            }
            self.wing_boots_left -= 1;
            self.say(format!(
                "Wing Boots flies you there ({} left)",
                self.wing_boots_left
            ));
        }
        let node = self.map.node(idx);
        let (kind, floor) = (node.kind, node.floor);
        // 这个格子在地图上是不是"?"(进房后才判定)
        let was_unknown = kind == NodeKind::Event;
        self.pos = Some(idx);
        self.path.push(idx);
        self.floor_reached = floor;
        // 每进一个房间,全局层号 +1,每层的流(miscRng / aiRng / monsterHpRng /
        // shuffleRng / cardRandomRng)都用 seed + 层号重开
        // (参考实现的 transitionToMapNode).切幕不重置这个数.
        self.floor_num += 1;
        self.streams.reseed_floor_streams(self.floor_num);
        // 地图上的未知房进房时才判定是什么(参考实现 mapPick -> resolveUnknownRoom)
        let kind = if kind == NodeKind::Event {
            self.resolve_unknown_room()
        } else {
            kind
        };
        self.last_room_was_shop = kind == NodeKind::Shop;
        self.say(format!("floor {}: {}", floor + 1, kind.name()));
        // 银行家之躯:每爬一层给钱;在商店花过钱之后就失效
        let per_floor: i32 = self.player.relic_fx_sum(|r| r.fx.gold_per_floor);
        if per_floor > 0 && !self.maw_bank_spent {
            self.gain_gold(per_floor);
        }
        // 九头蛇之首:进 ? 房间就给钱
        let unknown_gold = self.player.relic_fx_sum(|r| r.fx.gold_on_unknown_room);
        if unknown_gold > 0 && was_unknown {
            self.gain_gold(unknown_gold);
        }
        match kind {
            NodeKind::Monster => {
                let enc = self.pick_encounter(EnemyKind::Normal);
                self.start_combat(enc, false);
            }
            NodeKind::Elite => {
                self.stats.elites += 1;
                // 第四章的精英是定死的那一对(参考实现 enterResolvedRoom 的 act===4 分支)
                let enc = if self.act == 4 {
                    enemies::resolve("shield_and_spear")
                } else {
                    self.pick_encounter(EnemyKind::Elite)
                };
                let burning = self.map.node(idx).burning;
                self.start_combat(enc, burning);
            }
            NodeKind::Boss => {
                self.stats.bosses += 1;
                let enc = self.boss_enc;
                self.start_combat(enc, false);
            }
            NodeKind::Rest => self.enter_rest(),
            NodeKind::Shop => self.open_shop(),
            NodeKind::Treasure => self.open_treasure(),
            NodeKind::Event => self.open_event(),
        }
        Ok(())
    }

    /// 从这一章的名单里取下一个遭遇:名单按顺序消耗,抽干了再补一批强怪
    fn pick_encounter(&mut self, kind: EnemyKind) -> &'static Encounter {
        let id = match kind {
            EnemyKind::Normal => {
                if self.monster_list.is_empty() {
                    self.monster_list = enemies::generate_extra_strong(
                        self.act,
                        self.streams.run(RunStream::MonsterRng),
                        12,
                    );
                }
                self.monster_list.remove(0)
            }
            EnemyKind::Elite => {
                assert!(!self.elite_list.is_empty(), "精英名单已经抽干");
                self.elite_list.remove(0)
            }
            EnemyKind::Boss => self.boss_enc.id,
        };
        let pick = enemies::resolve(id);
        self.last_encounter = pick.id;
        pick
    }

    // ---- 战斗 ----

    fn start_combat(&mut self, enc: &'static Encounter, burning: bool) {
        self.say(format!("a fight breaks out: {}", enc.id));
        let setup = CombatSetup {
            rested: self.rested,
            hp: self.player.hp,
            max_hp: self.player.max_hp,
            deck: self.player.deck.clone(),
            relics: self.player.relics.clone(),
            gold: self.player.gold,
            // 吉利亚:营火举过几次铁,开局就给几层力量
            lift_strength: self.relic_lifts,
            // 跨战斗的遗物计数器(笔尖/快乐花/薰香/日晷/双节棍/墨水瓶)
            relic_counters: self.relic_counters,
            // 御守:战斗里被怪物塞进牌组的诅咒也要挡
            curse_negate: self.omamori_charges,
            asc: self.ascension,
        };
        self.combat = Some(Combat::new(enc, setup, self.streams.clone()));
        // 古代茶具的能量只在紧接着的这场战斗里生效,开打就清掉
        self.rested = false;
        // 仙女在瓶中:开局就把保命符挂上
        let fairy = self.has_fairy();
        if let Some(c) = self.combat.as_mut() {
            c.fairy_save = fairy;
        }
        // Neow's Lament:最前面这几场战斗里敌人只剩 1 血(上限不变)
        if self.neow_lament > 0 {
            self.neow_lament -= 1;
            if let Some(c) = self.combat.as_mut() {
                for e in c.enemies.iter_mut() {
                    if e.hp > 1 {
                        e.hp = 1;
                    }
                }
            }
        }
        // 燃烧精英:开局给全体敌人挂上地图掷出来的那个增益
        if burning {
            let buff = self.map.burning_buff;
            self.apply_burning_elite_buff(buff);
            if let Some(c) = self.combat.as_mut() {
                c.burning = true;
            }
        }
        self.win_hold = 0;
        self.fight_seq += 1;
        self.combat_log_seen = 0;
        self.screen = Screen::Combat;
        self.stats.fights += 1;
    }

    /// 燃烧精英的开局增益(照参考实现 applyBurningEliteBuff):
    /// 0 力量 +act,1 生命上限 +25%(四舍五入)并补等量血量,
    /// 2 金属化 act*2+2,3 再生 act*2+1.act 就是当前章号.
    fn apply_burning_elite_buff(&mut self, buff: i32) {
        let act = self.act as i32;
        let Some(c) = self.combat.as_mut() else { return };
        for e in c.enemies.iter_mut() {
            if e.dead() || e.escaped {
                continue;
            }
            match buff {
                0 => e.statuses.add(Status::Strength, act),
                1 => {
                    let inc = (e.max_hp as f32 * 0.25).round() as i32;
                    e.max_hp += inc;
                    e.hp += inc;
                }
                2 => e.statuses.add(Status::Metallicize, act * 2 + 2),
                3 => e.statuses.add(Status::Regenerate, act * 2 + 1),
                _ => {}
            }
        }
    }

    /// 测试与界面自检用:跳过地图,直接打一场指定遭遇
    #[cfg(test)]
    pub fn debug_start_combat(&mut self, enc: &'static Encounter) {
        self.start_combat(enc, false);
    }

    /// 调试用:直接判定这一场战斗胜利,进入奖励结算(不影响整局,跳的是战斗不是本局)
    pub fn debug_win_battle(&mut self) {
        if let Some(c) = self.combat.as_mut() {
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = crate::core::combat::Phase::Won;
        }
        self.sync_combat();
    }

    pub fn combat(&self) -> Option<&Combat> {
        self.combat.as_ref()
    }

    pub fn combat_mut(&mut self) -> Option<&mut Combat> {
        self.combat.as_mut()
    }

    /// 把这场战斗里还没抄过的日志行追加到历史记录
    fn absorb_combat_log(&mut self) {
        let Some(c) = self.combat.as_ref() else {
            return;
        };
        let fresh: Vec<HistoryEntry> = c
            .log
            .iter()
            .filter(|l| l.seq > self.combat_log_seen)
            .map(|l| HistoryEntry {
                text: format!("  {}", l.text),
                kind: match l.kind {
                    crate::core::combat::LogKind::Enemy => HistoryKind::Enemy,
                    crate::core::combat::LogKind::Player => HistoryKind::Player,
                    crate::core::combat::LogKind::Info => HistoryKind::Info,
                },
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        self.combat_log_seen = c.log_seq;
        self.history.extend(fresh);
        if self.history.len() > HISTORY_LIMIT {
            self.history.drain(0..HISTORY_TRIM);
        }
    }

    /// 每次战斗内操作之后调用:同步生命、处理胜负
    pub fn sync_combat(&mut self) {
        self.absorb_combat_log();
        // 跨战斗的遗物计数器:随时收回一局上,存档/界面读到的才是最新的
        let counters = self.combat.as_ref().map(|c| c.rs.run_counters());
        if let Some(counters) = counters {
            self.relic_counters = counters;
        }
        // 战斗里的掷点是在自己的流副本上走的,这里把它收回来
        if let Some(c) = self.combat.as_ref() {
            self.streams = c.streams.clone();
        }
        // 仙女在瓶中:这一场保过命就把那一格扣掉
        if self.combat.as_ref().is_some_and(|c| c.fairy_used) {
            if let Some(c) = self.combat.as_mut() {
                c.fairy_used = false;
            }
            if let Some(slot) = self.player.potions.iter_mut().find(|s| {
                s.as_ref().map(|p| p.id == "fairy_potion").unwrap_or(false)
            }) {
                *slot = None;
            }
            self.say("Fairy in a Bottle is spent");
        }
        // 战斗里永久成长的牌(血祭匕首):把增量写回牌组原件,下一次战斗仍然生效
        let growth: Vec<(usize, i32)> = self
            .combat
            .as_mut()
            .map(|c| std::mem::take(&mut c.deck_growth))
            .unwrap_or_default();
        for (idx, n) in growth {
            if let Some(mc) = self.player.deck.get_mut(idx) {
                mc.bonus += n;
            }
        }
        let fairy = self.has_fairy();
        let Some(c) = self.combat.as_mut() else {
            return;
        };
        c.fairy_save = fairy;
        let phase = c.phase;
        let hp = c.player.hp;
        let max_hp = c.player.max_hp;
        let damage = c.damage_dealt;
        let turns = c.turn;
        // 战斗里赚到的金币(贪婪之手)在这里收走,收一次就清零
        let looted = c.gold_gained;
        c.gold_gained = 0;
        // 抢劫类敌人会当场动玩家身上的金币
        let gold = c.player_gold;
        // 战斗中永久塞进牌组的牌(寄生)在这里并进去;御守挡掉的次数也收回来
        let added: Vec<_> = c.deck_cards.drain(..).collect();
        let omamori = c.curse_negate;
        self.player.hp = hp;
        self.player.max_hp = max_hp;
        self.player.gold = gold;
        self.omamori_charges = omamori;
        for card in added {
            self.say(format!("{} is added to your deck", card.label()));
            // 走统一入口:陶瓷鱼(每加一张 9 金)、蛋、黑石护符这些钩子
            // (反编译 exitBattle 走 Deck::obtain,那里会触发它们)
            self.push_card_to_deck(card);
        }
        if looted > 0 {
            self.gain_gold(looted);
            self.say(format!("you loot {looted} gold"));
        }
        // 战斗里的金币以玩家身上的为准,免得下一次同步把它冲掉
        if let Some(c) = self.combat.as_mut() {
            c.player_gold = self.player.gold;
        }
        match phase {
            // 赢了先留在战场上看 2 秒(死亡/结算动画),由 tick_win_hold 收尾
            Phase::Won => {
                if self.win_hold == 0 {
                    self.win_hold = Self::VICTORY_HOLD;
                }
            }
            Phase::Lost => self.resolve_defeat(),
            _ => {}
        }
        self.stats.turns = self.stats.turns.max(turns);
        let _ = damage;
    }

    /// 战斗胜利后在原地停留的帧数(60ms 一帧,34 帧约 2 秒)
    pub const VICTORY_HOLD: u8 = 34;

    /// 赢了但还没进奖励(这段时间不吃战斗操作)
    pub fn holding_victory(&self) -> bool {
        self.win_hold > 0
    }

    /// 胜利停留的倒计时;归零就把这场战斗结算掉
    pub fn tick_win_hold(&mut self) {
        if self.win_hold == 0 {
            return;
        }
        self.win_hold -= 1;
        if self.win_hold == 0 {
            let won = self
                .combat
                .as_ref()
                .is_some_and(|c| c.phase == Phase::Won);
            if won {
                self.resolve_victory();
            }
        }
    }

    fn post_combat_heal(&mut self) -> i32 {
        let mut heal: i32 = self.player.relic_fx_sum(|r| r.fx.post_combat_heal);
        // 肉骨头:血量在一半以下时额外回血
        let bone = self.player.relic_fx_sum(|r| r.fx.post_combat_heal_if_below_half);
        if bone > 0 && self.player.hp * 2 <= self.player.max_hp {
            heal += bone;
        }
        // 牧师之面:每场战斗提升一点生命上限
        let cleric = self.player.relic_fx_sum(|r| r.fx.max_hp_on_victory);
        if cleric > 0 {
            self.player.max_hp += cleric;
            self.player.hp += cleric;
        }
        if heal > 0 {
            // 魔法花:战斗中的治疗多 50%;战后这一次回血算在战斗里(原版如此)
            let pct = self
                .player
                .relics
                .iter()
                .map(|r| r.fx.combat_heal_pct)
                .max()
                .unwrap_or(0);
            if pct > 100 {
                heal = heal * pct / 100;
            }
            self.heal(heal);
        }
        heal
    }

    fn resolve_victory(&mut self) {
        self.absorb_combat_log();
        let Some(c) = self.combat.take() else {
            return;
        };
        // 跨战斗的遗物计数器写回一局(下一场从这里接着数)
        self.relic_counters = c.rs.run_counters();
        let kind = c.kind;
        let burning = c.burning;
        self.stats.damage_dealt += c.damage_dealt;
        if c.gold_gained > 0 {
            self.gain_gold(c.gold_gained);
        }
        let healed = self.post_combat_heal();
        if healed > 0 {
            self.say(format!("relics heal you for {healed}"));
        }
        // 事件打的那一场:奖励由事件指定,打完可能还要回到事件里接着选
        let plan = self.combat_reward.take();
        let back = self.pending_event.take();
        let adv = self.pending_adv.take();
        if let Some(d) = adv {
            self.say(format!("victory over the {}", c.encounter_id));
            self.open_dead_adventurer_rewards(d);
            return;
        }
        if let Some(def) = back {
            self.say(format!("victory over the {}", c.encounter_id));
            self.event = Some(EventState::new(def));
            self.screen = Screen::Event;
            return;
        }
        if plan.map(|p| p.nothing).unwrap_or(false) {
            self.say(format!("victory over the {} (no rewards)", c.encounter_id));
            self.screen = Screen::Map;
            return;
        }
        // 第三章及以后的 Boss 没有奖励屏(照参考实现 handleCombatVictory):
        // 集齐三把钥匙就把第三章 Boss 后面那扇门打开,直接进第四章;
        // 否则本局到此为止(第四章打倒心脏也是直接结束).
        if kind == EnemyKind::Boss && self.act >= 3 {
            self.say(format!("victory over the {}", c.encounter_id));
            // 飞升 20:第三章的 Boss 要连打两个.第一个倒下后不结算,
            // 直接推进一层、重种流,再开第二个 Boss 的战斗
            if self.act == 3 && self.ascension >= 20 && !self.a20_second_boss {
                self.a20_second_boss = true;
                self.say("A20: a second boss rises to meet you");
                self.floor_num += 1;
                self.streams.reseed_floor_streams(self.floor_num);
                let enc = self.boss2_enc;
                self.start_combat(enc, false);
                return;
            }
            if self.act == 3 && self.keys.emerald && self.keys.ruby && self.keys.sapphire {
                self.say("you hold all three keys: the door opens");
                self.begin_act();
            } else {
                self.screen = Screen::Victory;
            }
            return;
        }
        // 黑星:精英多掉一件遗物(在正常那一件之前先给)
        if kind == EnemyKind::Elite && self.player.relic_fx_sum(|r| r.fx.extra_elite_relic) > 0 {
            let tier = self.roll_elite_relic_tier();
            let extra = self.take_relic_of_tier(tier);
            self.gain_relic(extra);
            self.say(format!("Black Star grants {}", extra.name));
        }
        // 顺序照参考实现的 buildCombatRewards:金币 → 遗物(精英)→ 药水 → 卡牌
        // 区间上下限相同就是"定值金币"(事件战斗的金币,原版直接给这个数):
        // 不能掷点 —— 掷一次 random_range(50,50) 值一样,但会让宝箱与后续奖励
        // 的 treasureRng 掷点整体错位
        let gold_raw = match plan.and_then(|p| p.gold) {
            Some((lo, hi)) if lo == hi => lo,
            Some((lo, hi)) => self.streams.run(RunStream::TreasureRng).random_range(lo, hi),
            None => match kind {
                EnemyKind::Normal => self
                    .streams
                    .run(RunStream::TreasureRng)
                    .random_range(10, 20),
                EnemyKind::Elite => self
                    .streams
                    .run(RunStream::TreasureRng)
                    .random_range(25, 35),
                // Boss 的金币走 miscRng:100 上下浮动 5;飞升 13+ 掉 25%
                EnemyKind::Boss => {
                    let g = 100 + self.streams.floor(FloorStream::MiscRng).random_range(-5, 5);
                    if self.ascension >= 13 {
                        (g as f32 * 0.75).round() as i32
                    } else {
                        g
                    }
                }
            },
        };
        // 金像:敌人掉的金币多 25%
        let idol_pct = self.player.relic_fx_sum(|r| r.fx.gold_reward_pct);
        let gold = if idol_pct > 0 {
            gold_raw + (gold_raw * idol_pct + 50) / 100
        } else {
            gold_raw
        };
        let relic = match plan {
            Some(p) if p.no_relic => None,
            Some(p) if p.relic_id.is_some() => Some(relic_def_any(p.relic_id.unwrap())),
            Some(p) if p.relic_rarity.is_some() => self.take_relic_of(p.relic_rarity.unwrap()),
            Some(_) => None,
            None => match kind {
                EnemyKind::Elite => {
                    let tier = self.roll_elite_relic_tier();
                    Some(self.take_relic_of_tier(tier))
                }
                // Boss 的三件候选在下面单独取(参考实现的 boss 遗物三选一)
                EnemyKind::Boss | EnemyKind::Normal => None,
            },
        };
        // 斗兽场第二场的第二件遗物:奖励屏一次只摆一件,这件打赢就直接进背包
        if let Some(rarity) = plan.and_then(|p| p.relic_rarity2) {
            if let Some(def) = self.take_relic_of(rarity) {
                self.gain_relic(def);
                self.say(format!("{} is yours", def.name));
            }
        }
        // 药水:先掷一次 d100 看掉不掉(带保底),掉了再掷稀有度.
        // 事件战斗(plan)走的也是同一条:参考实现 eventCombatRewards → rollPotionReward(ctx, entries.length),
        // 保底累加与"已有奖励条目 ≥4 就不掉"都照旧,所以这里把已有条目数传进去.
        let potion = match plan {
            Some(p) if p.potion_pct > 0 => {
                let so_far = usize::from(gold > 0) + usize::from(relic.is_some());
                self.roll_potion_reward(so_far)
            }
            Some(_) => None,
            None => {
                let categories = if matches!(kind, EnemyKind::Elite) { 2 } else { 1 };
                self.roll_potion_reward(categories)
            }
        };
        let no_cards = plan.map(|p| p.no_cards).unwrap_or(false);
        let cards: Vec<CardInstance> = if no_cards {
            Vec::new()
        } else {
            self.create_card_reward(kind)
        };
        // 祈祷轮:普通战斗多掉几组卡牌奖励(参考实现 buildCombatRewards)
        let extra_groups = self.player.relic_fx_sum(|r| r.fx.extra_card_reward_group);
        let mut queued: Vec<Vec<CardInstance>> = Vec::new();
        if kind == EnemyKind::Normal && !no_cards {
            for _ in 0..extra_groups.max(0) {
                queued.push(self.create_card_reward(kind));
            }
        }
        let next = if kind == EnemyKind::Boss {
            // Boss 奖励屏:离开时按幕推进(见 leave_reward / after_boss_reward)
            Screen::Victory
        } else {
            Screen::Map
        };
        // Boss 的金币已经用过了 Boss 那一层的流;这里再进"Boss 宝箱房":
        // 全局层号 +1 并重种每层的流(参考实现 enterBossTreasureRoom)
        if kind == EnemyKind::Boss {
            self.floor_num += 1;
            self.streams.reseed_floor_streams(self.floor_num);
        }
        self.say(format!("victory over the {} ({kind:?})", c.encounter_id));
        // Boss 的遗物是三选一(参考实现的 boss relic choice:从 Boss 池里连取三件)
        let relic_choices: Vec<&'static RelicDef> = if kind == EnemyKind::Boss {
            (0..3).map(|_| self.take_relic_of_tier(RelicTier::Boss)).collect()
        } else {
            Vec::new()
        };
        let potions: Vec<&'static PotionDef> = potion.into_iter().collect();
        self.reward = Some(RewardState {
            gold,
            gold_taken: false,
            extra_gold: 0,
            extra_gold_taken: true,
            cards,
            card_taken: false,
            queued,
            relic,
            relic_choices,
            relic_taken: false,
            potion_taken: vec![false; potions.len()],
            potions,
            // 燃烧精英掉绿钥匙:参考实现 burningElite && !keys.emerald 时加一份
            emerald_key: kind == EnemyKind::Elite && burning && !self.keys.emerald,
            index: 0,
            next,
        });
        self.screen = Screen::Reward;
    }

    fn resolve_defeat(&mut self) {
        self.absorb_combat_log();
        // 跨战斗的遗物计数器照样写回(死亡界面上的存档/统计要一致)
        if let Some(c) = self.combat.as_ref() {
            self.relic_counters = c.rs.run_counters();
        }
        self.combat = None;
        self.say("you fell in battle");
        self.screen = Screen::Death;
    }

    // ---- 奖励 ----

    pub fn reward_slots(&self) -> Vec<RewardSlot> {
        let Some(r) = &self.reward else {
            return Vec::new();
        };
        let mut v = Vec::new();
        if !r.gold_taken {
            v.push(RewardSlot::Gold);
        }
        if r.extra_gold > 0 && !r.extra_gold_taken {
            v.push(RewardSlot::ExtraGold);
        }
        if !r.card_taken {
            for i in 0..r.cards.len() {
                v.push(RewardSlot::Card(i));
            }
        }
        if r.relic.is_some() && !r.relic_taken {
            v.push(RewardSlot::Relic);
        }
        if !r.relic_taken {
            for i in 0..r.relic_choices.len() {
                v.push(RewardSlot::RelicChoice(i));
            }
        }
        if r.emerald_key && !self.keys.emerald {
            v.push(RewardSlot::EmeraldKey);
        }
        for (i, _) in r.potions.iter().enumerate() {
            if !r.potion_taken.get(i).copied().unwrap_or(true) {
                v.push(RewardSlot::Potion(i));
            }
        }
        v
    }

    pub fn reward_clamp(&mut self) {
        let n = self.reward_slots().len();
        if let Some(r) = self.reward.as_mut() {
            if n == 0 {
                r.index = 0;
            } else if r.index >= n {
                r.index = n - 1;
            }
        }
    }

    /// 取走当前选中的奖励
    pub fn reward_take(&mut self) -> Result<String, String> {
        let slots = self.reward_slots();
        let index = self.reward.as_ref().map(|r| r.index).unwrap_or(0);
        let Some(slot) = slots.get(index).copied() else {
            return Err("nothing selected".to_string());
        };
        match slot {
            RewardSlot::Gold => {
                let g = self.reward.as_ref().map(|r| r.gold).unwrap_or(0);
                self.gain_gold(g);
                self.mark_reward(|r| r.gold_taken = true);
                Ok(format!("+${g}"))
            }
            RewardSlot::ExtraGold => {
                let g = self.reward.as_ref().map(|r| r.extra_gold).unwrap_or(0);
                self.gain_gold(g);
                self.mark_reward(|r| r.extra_gold_taken = true);
                Ok(format!("+${g}"))
            }
            RewardSlot::Card(i) => {
                let Some(card) = self.reward.as_ref().and_then(|r| r.cards.get(i)).cloned() else {
                    return Err("no such card".to_string());
                };
                let label = card.label();
                self.push_card_to_deck(card);
                self.mark_reward(|r| r.card_taken = true);
                self.next_card_group();
                Ok(format!("{label} added to your deck"))
            }
            RewardSlot::Relic => {
                let Some(def) = self.reward.as_ref().and_then(|r| r.relic) else {
                    return Err("no relic here".to_string());
                };
                self.gain_relic(def);
                self.mark_reward(|r| r.relic_taken = true);
                Ok(format!("relic gained: {}", def.name))
            }
            RewardSlot::RelicChoice(i) => {
                let Some(def) = self.reward.as_ref().and_then(|r| r.relic_choices.get(i)).copied()
                else {
                    return Err("no relic here".to_string());
                };
                self.gain_relic(def);
                self.mark_reward(|r| r.relic_taken = true);
                Ok(format!("relic gained: {}", def.name))
            }
            RewardSlot::Potion(i) => {
                let Some(def) = self.reward.as_ref().and_then(|r| r.potions.get(i)).copied() else {
                    return Err("no potion here".to_string());
                };
                if !self.add_potion(def) {
                    return Err("no free potion slot: press p, then t+1-3 to toss one".to_string());
                }
                self.mark_reward(|r| {
                    if let Some(t) = r.potion_taken.get_mut(i) {
                        *t = true;
                    }
                });
                Ok(format!("potion gained: {}", def.name))
            }
            RewardSlot::EmeraldKey => {
                self.keys.emerald = true;
                self.mark_reward(|r| r.emerald_key = false);
                Ok("the Emerald Key is yours".to_string())
            }
        }
    }

    fn mark_reward(&mut self, f: impl FnOnce(&mut RewardState)) {
        if let Some(r) = self.reward.as_mut() {
            f(r);
        }
    }

    /// 跳过卡牌奖励时可能提升生命上限(唱歌碗)
    pub fn reward_skip_cards(&mut self) -> String {
        let bonus: i32 = self.player.relic_fx_sum(|r| r.fx.max_hp_on_card_skip);
        if let Some(r) = self.reward.as_mut() {
            r.card_taken = true;
        }
        self.next_card_group();
        if bonus > 0 {
            self.player.max_hp += bonus;
            self.player.hp += bonus;
            format!("skip the cards: +{bonus} max HP")
        } else {
            "skipped the cards".to_string()
        }
    }

    /// 离开奖励界面.Boss 的奖励屏离开时按幕推进:第一、二章进下一章;
    /// 第三章看钥匙(有就开门进第四章,没有就本局胜利收尾).
    pub fn leave_reward(&mut self) -> Screen {
        let next = self
            .reward
            .as_ref()
            .map(|r| r.next)
            .unwrap_or(Screen::Map);
        self.reward = None;
        if next == Screen::Victory {
            return self.after_boss_reward();
        }
        self.screen = next;
        next
    }

    /// Boss 奖励屏之后去哪:第一、二章切到下一章,第三章没钥匙就收尾
    /// (有钥匙的情况在 resolve_victory 里就开门了,走不到这里)
    fn after_boss_reward(&mut self) -> Screen {
        if self.act >= 3 {
            self.screen = Screen::Victory;
            return Screen::Victory;
        }
        self.begin_act();
        self.screen
    }

    /// 切到下一章(参考实现的 actTransition).调用前 act 是刚打通的那一章:
    /// 回满血、清掉未知房与药水的保底、cardRng 计数器跳到 250 的边界,
    /// 重新掷遭遇名单与事件池,再按新章的种子重生成地图.
    fn begin_act(&mut self) {
        self.act += 1;
        // 打完 Boss 切幕的回血:飞升 5 以下回满,飞升 5+ 只补缺血的 75%
        if self.ascension >= 5 {
            let missing = (self.player.max_hp - self.player.hp).max(0);
            self.player.hp = (self.player.hp + (missing as f32 * 0.75).round() as i32)
                .min(self.player.max_hp);
        } else {
            self.player.hp = self.player.max_hp;
        }
        // 未知房的保底与药水保底都在切幕时复位
        self.potion_chance = 0;
        self.monster_chance = UNKNOWN_BASE.0;
        self.shop_chance = UNKNOWN_BASE.1;
        self.treasure_chance = UNKNOWN_BASE.2;
        // cardRng 计数器跳到下一个 250 的边界(空烧 randomBoolean)
        let card = self.streams.run(RunStream::CardRng);
        let target = if card.counter() < 250 {
            250
        } else if card.counter() < 500 {
            500
        } else if card.counter() < 750 {
            750
        } else {
            card.counter()
        };
        card.set_counter(target);
        if self.act == 4 {
            // 第四章:没有遭遇/事件池,地图与 Boss 都是定死的
            self.monster_list.clear();
            self.elite_list.clear();
            self.boss_enc = enemies::resolve("the_heart");
            self.boss2_enc = self.boss_enc;
            self.event_pool.clear();
            self.shrine_pool.clear();
            self.map = ActMap::act4();
        } else {
            // 新一章的遭遇名单接着 monsterRng 掷,事件池换成这一章的
            let lists = enemies::generate_encounters(self.act, self.streams.run(RunStream::MonsterRng));
            self.boss_enc = enemies::resolve(lists.boss[0]);
            self.boss2_enc = enemies::resolve(*lists.boss.get(1).unwrap_or(&lists.boss[0]));
            self.monster_list = lists.monster;
            self.elite_list = lists.elite;
            self.event_pool = act_event_pool(self.act);
            self.shrine_pool = act_shrine_pool(self.act);
            // 地图按新章的种子重开;绿钥匙到手了就不再标燃烧精英
            self.streams.reseed_map(self.act);
            self.map =
                ActMap::generate(self.streams.map_rng(), !self.keys.emerald, self.ascension);
        }
        self.pos = None;
        self.path.clear();
        self.floor_reached = 0;
        self.last_room_was_shop = false;
        self.screen = Screen::Map;
    }

    // ---- 营火 ----

    /// 进营火:永恒之羽在这里就结算(参考实现 onEnterRestSite:每 5 张牌回 3 点),
    /// 与之后选不选"休息"无关
    fn enter_rest(&mut self) {
        self.rest_index = 0;
        self.screen = Screen::Rest;
        // 古代茶具:只要进过营火(选不选休息都算),下一场战斗第一回合多 2 点能量
        self.rested = true;
        let feather: i32 = self.player.relic_fx_sum(|r| r.fx.rest_heal_per_5_deck);
        if feather > 0 {
            let amount = (self.player.deck.len() as i32 / 5) * feather;
            if amount > 0 {
                let healed = self.heal(amount);
                self.say(format!("Eternal Feather heals {healed} HP"));
            }
        }
    }

    pub fn rest_heal(&mut self) {
        // 咖啡滴滤器:营火不能休息
        if self.has_relic_fx(|fx| fx.no_rest) {
            self.say("Coffee Dripper: you can no longer rest");
            return;
        }
        let bonus: i32 = self.player.relic_fx_sum(|r| r.fx.rest_heal_bonus);
        let amount = (self.player.max_hp * REST_HEAL_PCT / 100) + bonus;
        let healed = self.heal(amount);
        self.say(format!("you rest and heal {healed} HP"));
        self.screen = Screen::Map;
        // 梦中情网:休息之后可以再挑一张牌进牌组(参考实现 Dream Catcher)
        if self.has_relic_fx(|fx| fx.rest_card_reward) {
            self.add_card_choice(1);
        }
    }

    pub fn rest_smith(&mut self) {
        // 熔火之锤:再也不能锻造
        if self.has_relic_fx(|fx| fx.no_smith) {
            self.say("Fusion Hammer: you can no longer smith");
            return;
        }
        self.open_picker(PickPurpose::Upgrade, Screen::Map, 0, None);
    }

    /// 营火能不能"回忆"拿红钥匙:还没有红钥匙就行(参考实现 canRecall)
    pub fn can_recall(&self) -> bool {
        !self.keys.ruby
    }

    /// 现在营火都能做什么(参考实现 restOptionAvailable 的营火位)
    pub fn rest_options(&self) -> Vec<RestOption> {
        let mut out = Vec::new();
        if !self.has_relic_fx(|fx| fx.no_rest) {
            out.push(RestOption::Rest);
        }
        // 打铁:除了熔火之锤,还得牌组里真有一张能升的(参考实现 canSmith 的条件)
        if !self.has_relic_fx(|fx| fx.no_smith)
            && !self.picker_candidates_is_empty(PickPurpose::Upgrade)
        {
            out.push(RestOption::Smith);
        }
        if self.can_recall() {
            out.push(RestOption::Recall);
        }
        // 吉利亚:举铁,上限由遗物给(参考实现 counter < 3)
        let lift_max = self.player.relic_fx_sum(|r| r.fx.rest_lift_max);
        if lift_max > 0 && self.relic_lifts < lift_max {
            out.push(RestOption::Lift);
        }
        // 和平烟斗:删一张牌
        if self.has_relic_fx(|fx| fx.rest_toke) && !self.player.deck.is_empty() {
            out.push(RestOption::Toke);
        }
        // 铲子:挖一件遗物
        if self.has_relic_fx(|fx| fx.rest_dig) {
            out.push(RestOption::Dig);
        }
        out
    }

    /// 营火选项个数
    pub fn rest_option_count(&self) -> usize {
        self.rest_options().len()
    }

    /// 按选项办事(界面只负责挑一个;不可用的选项返回错误)
    pub fn rest_choose(&mut self, opt: RestOption) -> Result<String, String> {
        if !self.rest_options().contains(&opt) {
            return Err(format!("{} is not available here", opt.name()));
        }
        match opt {
            RestOption::Rest => {
                self.rest_heal();
                Ok("you rest by the fire".to_string())
            }
            RestOption::Smith => {
                self.rest_smith();
                Ok("choose a card to upgrade".to_string())
            }
            RestOption::Recall => {
                self.rest_recall();
                Ok("you recall the Ruby Key".to_string())
            }
            RestOption::Lift => {
                self.rest_lift();
                Ok("Girya: +1 Strength for the next combat".to_string())
            }
            RestOption::Toke => {
                self.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
                Ok("choose a card to remove".to_string())
            }
            RestOption::Dig => Ok(self.rest_dig()),
        }
    }

    /// 吉利亚已经举过几次铁(界面显示用)
    pub fn lifts(&self) -> i32 {
        self.relic_lifts
    }

    /// 羽翼靴还剩几次(界面显示用)
    pub fn wing_boots_charges(&self) -> i32 {
        self.wing_boots_left
    }

    /// 地图上现在能去哪儿:正常可达的节点;羽翼靴有电时同层往后的任意节点也能飞
    pub fn travel_options(&self) -> Vec<usize> {
        let mut out = self.reachable();
        if self.wing_boots_left > 0 {
            let here = self.pos.map(|p| self.map.node(p).floor).unwrap_or(0);
            let boss = self.map.boss;
            for (i, node) in self.map.nodes.iter().enumerate() {
                if i == boss || node.floor <= here || out.contains(&i) {
                    continue;
                }
                out.push(i);
            }
            out.sort_unstable();
        }
        out
    }

    /// 这一趟是不是靠羽翼靴飞的(不在正常可达里)
    pub fn travel_is_fly(&self, idx: usize) -> bool {
        !self.reachable().contains(&idx)
    }

    /// 举铁:计数器 +1,力量在下一场战斗开局结算(参考实现 GIRYA 的 counter)
    pub fn rest_lift(&mut self) {
        self.relic_lifts += 1;
        self.say(format!("Girya: you lift ({}/3)", self.relic_lifts.min(3)));
        self.screen = Screen::Map;
    }

    /// 挖宝:按战斗档的遗物档次挖一件,走正常的拾取流程(参考实现 Shovel 的 dig)
    pub fn rest_dig(&mut self) -> String {
        self.screen = Screen::Map;
        let tier = self.roll_combat_relic_tier();
        let def = self.take_relic_of_tier(tier);
        let name = def.name;
        self.gain_relic(def);
        self.say(format!("Shovel digs up {name}"));
        format!("you dig up {name}")
    }

    /// 回忆:拿走红钥匙(不休息也不锻造)
    pub fn rest_recall(&mut self) {
        if self.keys.ruby {
            self.say("you already have the Ruby Key");
            return;
        }
        self.keys.ruby = true;
        self.say("you recall the Ruby Key");
        self.screen = Screen::Map;
    }

    // ---- 商店 ----

    /// 商店进货(参考实现 generateShop).掷点顺序:
    /// 五张职业牌(稀有度+身份都走 cardRng)→ 两张无色牌(cardRng)→
    /// 七张牌的价格浮动(merchantRng)→ 打折位(merchantRng)→
    /// 两件遗物的档次(merchantRng)+ 一件商店档 → 三件遗物的价格浮动 →
    /// 三瓶药水(potionRng)→ 三瓶药水的价格浮动 → 删牌服务的价格.
    pub(crate) fn open_shop(&mut self) {
        // --- 五张职业牌:攻击两张、技能两张(各自不重样)、能力一张 ---
        let mut class_picks: Vec<(&'static CardDef, Rarity)> = Vec::new();
        for kind in [CardType::Attack, CardType::Skill] {
            let Some((first, first_rarity)) = self.roll_shop_class_card(kind) else {
                continue;
            };
            class_picks.push((first, first_rarity));
            let mut next = self.roll_shop_class_card(kind);
            let mut guard = 0;
            while next.map(|(d, _)| d.id) == Some(first.id) && guard < 1000 {
                guard += 1;
                next = self.roll_shop_class_card(kind);
            }
            if let Some((def, rarity)) = next {
                class_picks.push((def, rarity));
            }
        }
        if let Some((def, rarity)) = self.roll_shop_class_card(CardType::Power) {
            class_picks.push((def, rarity));
        }
        // --- 两张无色牌:一张非普通、一张稀有 ---
        let mut colorless_picks: Vec<(&'static CardDef, Rarity)> = Vec::new();
        for rarity in [Rarity::Uncommon, Rarity::Rare] {
            if let Some(def) = self.roll_shop_colorless(rarity) {
                colorless_picks.push((def, rarity));
            }
        }
        // --- 牌价:底价 × 0.9..1.1,无色牌再 ×1.2;价掷完之后才掷打折位 ---
        let mut card_slots: Vec<(&'static CardDef, i32)> = Vec::new();
        let n_class = class_picks.len();
        for (i, (def, rarity)) in class_picks
            .iter()
            .chain(colorless_picks.iter())
            .map(|(d, r)| (*d, *r))
            .enumerate()
        {
            let base = shop_base(SHOP_CARD_BASE, rarity) as f32;
            let jitter = self
                .streams
                .run(RunStream::MerchantRng)
                .random_float_range(SHOP_CARD_JITTER.0, SHOP_CARD_JITTER.1);
            let mut price = base * jitter;
            if i >= n_class {
                price *= SHOP_COLORLESS_FACTOR;
            }
            card_slots.push((def, price as i32));
        }
        if !card_slots.is_empty() {
            let sale = self.streams.run(RunStream::MerchantRng).random(SHOP_SALE_SLOTS - 1) as usize;
            if let Some((_, price)) = card_slots.get_mut(sale) {
                *price /= 2;
            }
        }
        let mut items: Vec<ShopItem> = Vec::new();
        let mut kinds: Vec<ShopKind> = Vec::new();
        for (i, (def, price)) in card_slots.into_iter().enumerate() {
            items.push(ShopItem::Card(CardInstance::new(def), self.discount(price)));
            kinds.push(if i < n_class {
                ShopKind::ClassCard
            } else {
                ShopKind::ColorlessCard
            });
        }
        // --- 遗物:两件按档次掷、一件商店档(档次掷完才掷价格) ---
        let mut relic_picks: Vec<(&'static RelicDef, ShopTier)> = Vec::new();
        for _ in 0..2 {
            let tier = self.roll_shop_relic_tier();
            let def = self.take_shop_relic(tier).expect("遗物池不会空:兜底发 Circlet");
            relic_picks.push((def, tier));
        }
        let shelf_def = self.take_shop_relic(ShopTier::Shop).expect("遗物池不会空:兜底发 Circlet");
        relic_picks.push((shelf_def, ShopTier::Shop));
        for (def, tier) in relic_picks {
            // 池子空了也把这一掷走掉,后面的流位置才对得上
            let jitter = self
                .streams
                .run(RunStream::MerchantRng)
                .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1);
            let price = (tier.base() as f32 * jitter).round() as i32;
            items.push(ShopItem::Relic(def, self.discount(price)));
            kinds.push(ShopKind::Relic);
        }
        // --- 药水:三瓶(身份走 potionRng),价掷在身份之后 ---
        let mut potion_defs: Vec<&'static PotionDef> = Vec::new();
        for _ in 0..3 {
            if let Some(def) = potions::random_potion(self.streams.run(RunStream::PotionRng), potions::class_color(self.character)) {
                potion_defs.push(def);
            }
        }
        for def in potion_defs {
            let base = shop_base(SHOP_POTION_BASE, def.rarity) as f32;
            let jitter = self
                .streams
                .run(RunStream::MerchantRng)
                .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1);
            let price = (base * jitter).round() as i32;
            items.push(ShopItem::Potion(def, self.discount(price)));
            kinds.push(ShopKind::Potion);
        }
        // --- 删牌服务:75 + 25 × 本局已买过的次数(微笑面具固定 50) ---
        let fixed = self.player.relic_fx_sum(|r| r.fx.removal_cost_fixed);
        let removal = if fixed > 0 {
            fixed
        } else {
            self.discount(SHOP_REMOVAL_BASE + SHOP_REMOVAL_STEP * self.removes_purchased as i32)
        };
        items.push(ShopItem::Remove(removal));
        kinds.push(ShopKind::Remove);
        debug_assert_eq!(items.len(), kinds.len(), "商店格子和种类数要对得上");
        let n = items.len();
        self.shop = Some(ShopState {
            items,
            kinds,
            sold: vec![false; n],
            index: 0,
            removes: 0,
        });
        // 餐券:每次进商店回 15 血
        let ticket = self.player.relic_fx_sum(|r| r.fx.heal_on_shop_enter);
        if ticket > 0 {
            let healed = self.heal(ticket);
            self.say(format!("Meal Ticket heals you for {healed}"));
        }
        self.screen = Screen::Shop;
    }

    /// 一张职业牌:先掷稀有度(读 cardRarityFactor 但不改它),能力位把普通升成非普通,
    /// 再从"该稀有度 + 该类型"的池子里抽一张(参考实现 rollShopClassCard)
    fn roll_shop_class_card(&mut self, kind: CardType) -> Option<(&'static CardDef, Rarity)> {
        let roll = self.streams.run(RunStream::CardRng).random(99) as i32 + self.card_rarity_factor;
        let mut rarity = if roll < SHOP_CARD_RARE_BELOW {
            Rarity::Rare
        } else if roll >= SHOP_CARD_COMMON_AT {
            Rarity::Common
        } else {
            Rarity::Uncommon
        };
        if kind == CardType::Power && rarity == Rarity::Common {
            rarity = Rarity::Uncommon;
        }
        let pool: Vec<&CardDef> = cards::reward_pool(rarity)
            .into_iter()
            .filter(|c| c.kind == kind)
            .collect();
        if pool.is_empty() {
            return None;
        }
        Some((*self.streams.run(RunStream::CardRng).pick(&pool), rarity))
    }

    /// 一张无色牌(参考实现 rollColorlessCard)
    fn roll_shop_colorless(&mut self, rarity: Rarity) -> Option<&'static CardDef> {
        let pool: Vec<&CardDef> = cards::colorless_pool()
            .into_iter()
            .filter(|c| c.rarity == rarity)
            .collect();
        if pool.is_empty() {
            return None;
        }
        Some(*self.streams.run(RunStream::CardRng).pick(&pool))
    }

    /// 商店遗物的档次(merchantRng:<48 普通,<82 罕见,其余稀有)
    fn roll_shop_relic_tier(&mut self) -> ShopTier {
        let roll = self.streams.run(RunStream::MerchantRng).random(99);
        if roll < 48 {
            ShopTier::Common
        } else if roll < 82 {
            ShopTier::Uncommon
        } else {
            ShopTier::Rare
        }
    }

    /// 从池子里取一件该档次的遗物(商店档抽干了退回罕见/稀有)
    fn take_shop_relic(&mut self, tier: ShopTier) -> Option<&'static RelicDef> {
        let tier = match tier {
            ShopTier::Common => RelicTier::Common,
            ShopTier::Uncommon => RelicTier::Uncommon,
            ShopTier::Rare => RelicTier::Rare,
            ShopTier::Shop => RelicTier::Shop,
        };
        Some(self.take_relic_of_tier(tier))
    }

    fn discount(&self, price: i32) -> i32 {
        // 飞升 16:商店一律 +10%(wiki 口径),在遗物折扣之前先算
        let price = if self.ascension >= 16 {
            (price as f32 * 1.1).round() as i32
        } else {
            price
        };
        // 遗物折扣多件相乘:信使 -20% 与会员卡 -50% 叠起来是 -60%(wiki 口径),
        // 不是把百分比相加。最后按最近的整数取整(会员卡的结果 `.5` 进位)
        let mut factor = 1.0f32;
        let mut discounted = false;
        for r in &self.player.relics {
            let pct = r.fx.shop_discount_pct.clamp(0, 100);
            if pct > 0 {
                factor *= (100 - pct) as f32 / 100.0;
                discounted = true;
            }
        }
        if !discounted {
            return price;
        }
        ((price as f32) * factor).round() as i32
    }

    pub fn shop_clamp(&mut self) {
        if let Some(s) = self.shop.as_mut() {
            if s.index >= s.items.len() {
                s.index = s.items.len().saturating_sub(1);
            }
        }
    }

    pub fn buy_selected(&mut self) -> Result<String, String> {
        let Some(shop) = self.shop.as_ref() else {
            return Err("no shop here".to_string());
        };
        let i = shop.index;
        let Some(item) = shop.items.get(i) else {
            return Err("nothing selected".to_string());
        };
        if shop.sold.get(i).copied().unwrap_or(true) {
            return Err("sold out".to_string());
        }
        let price = item.price();
        if self.player.gold < price {
            return Err(format!("needs ${price}"));
        }
        match item {
            ShopItem::Card(card, _) => {
                let card = card.clone();
                let label = card.label();
                self.push_card_to_deck(card);
                self.spend_gold(price);
                self.shop.as_mut().unwrap().sold[i] = true;
                self.restock_shop_slot(i);
                Ok(format!("bought {label} for {price}"))
            }
            ShopItem::Relic(def, _) => {
                let def = *def;
                self.spend_gold(price);
                self.gain_relic(def);
                self.shop.as_mut().unwrap().sold[i] = true;
                self.restock_shop_slot(i);
                Ok(format!("bought {} for {price}", def.name))
            }
            ShopItem::Potion(def, _) => {
                let def = *def;
                if !self.add_potion(def) {
                    return Err("no free potion slot: press p, then t+1-3 to toss one".to_string());
                }
                self.spend_gold(price);
                self.shop.as_mut().unwrap().sold[i] = true;
                self.restock_shop_slot(i);
                Ok(format!("bought {} for {price}", def.name))
            }
            ShopItem::Remove(_) => {
                // 先选牌,确认时再付钱
                self.open_picker(PickPurpose::Remove, Screen::Shop, price, Some(i));
                Ok("choose a card to remove".to_string())
            }
        }
    }

    /// 信使(The Courier)补货:买走一格就按同种类即时重掷一格(原版语义,参考实现未做).
    /// 卡走商店房间的稀有度掷 + mathUtilRng 抽池;无色牌走 30% 稀有掷;
    /// 遗物走 Shop::rollRelicTier(商店档遗物买走后补的是普通档次);药水走 potionRng.
    /// 补出来的价格也照常打折(-20%).删牌服务不补货.
    fn restock_shop_slot(&mut self, i: usize) {
        if !self.has_relic("the_courier") {
            return;
        }
        let Some(kind) = self.shop.as_ref().and_then(|s| s.kinds.get(i).copied()) else {
            return;
        };
        let item = match kind {
            ShopKind::ClassCard => self.roll_courier_class_card(),
            ShopKind::ColorlessCard => self.roll_courier_colorless_card(),
            ShopKind::Relic => self.roll_courier_relic(),
            ShopKind::Potion => self.roll_courier_potion(),
            ShopKind::Remove => None,
        };
        if let Some(item) = item {
            if let Some(shop) = self.shop.as_mut() {
                shop.items[i] = item;
                shop.sold[i] = false;
            }
        }
    }

    /// 补一张本职业牌:稀有度按"商店房间"掷(和普通怪一样 3/37;恩洛斯礼物翻三倍),
    /// 再从该稀有度的职业牌池里用 mathUtilRng 抽一张(稀有位次用本作的池子序).
    fn roll_courier_class_card(&mut self) -> Option<ShopItem> {
        let rarity = self.roll_card_rarity(EnemyKind::Normal);
        let pool = cards::reward_pool(rarity);
        if pool.is_empty() {
            return None;
        }
        let def = *self.streams.math_util().pick(&pool);
        let base = shop_base(SHOP_CARD_BASE, rarity) as f32;
        let jitter = self
            .streams
            .run(RunStream::MerchantRng)
            .random_float_range(SHOP_CARD_JITTER.0, SHOP_CARD_JITTER.1);
        Some(ShopItem::Card(
            CardInstance::new(def),
            self.discount((base * jitter) as i32),
        ))
    }

    /// 补一张无色牌:先掷 30% 决定稀有/罕见,再从无色池抽(cardRng),价 ×1.2
    fn roll_courier_colorless_card(&mut self) -> Option<ShopItem> {
        let rare = self
            .streams
            .run(RunStream::MerchantRng)
            .random_float()
            < SHOP_COLORLESS_RARE_CHANCE;
        let rarity = if rare { Rarity::Rare } else { Rarity::Uncommon };
        let def = self.roll_shop_colorless(rarity)?;
        let base = shop_base(SHOP_CARD_BASE, rarity) as f32;
        let jitter = self
            .streams
            .run(RunStream::MerchantRng)
            .random_float_range(SHOP_CARD_JITTER.0, SHOP_CARD_JITTER.1);
        Some(ShopItem::Card(
            CardInstance::new(def),
            self.discount((base * jitter * SHOP_COLORLESS_FACTOR) as i32),
        ))
    }

    /// 补一件遗物:档次掷 merchantRng(永不掷出商店档),价格照常浮动
    fn roll_courier_relic(&mut self) -> Option<ShopItem> {
        let tier = self.roll_shop_relic_tier();
        let def = self.take_shop_relic(tier)?;
        let jitter = self
            .streams
            .run(RunStream::MerchantRng)
            .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1);
        let price = (tier.base() as f32 * jitter).round() as i32;
        Some(ShopItem::Relic(def, self.discount(price)))
    }

    /// 补一瓶药水:身份走 potionRng,价格照常浮动
    fn roll_courier_potion(&mut self) -> Option<ShopItem> {
        let def =
            potions::random_potion(self.streams.run(RunStream::PotionRng), potions::class_color(self.character))?;
        let base = shop_base(SHOP_POTION_BASE, def.rarity) as f32;
        let jitter = self
            .streams
            .run(RunStream::MerchantRng)
            .random_float_range(SHOP_OTHER_JITTER.0, SHOP_OTHER_JITTER.1);
        let price = (base * jitter).round() as i32;
        Some(ShopItem::Potion(def, self.discount(price)))
    }

    pub fn leave_shop(&mut self) {
        self.say("you leave the shop");
        self.shop = None;
        self.screen = Screen::Map;
    }

    // ---- 事件 ----

    fn open_event(&mut self) {
        match self.generate_event_id() {
            Some(id) => {
                let def = crate::core::events::EVENTS.iter().find(|e| e.id == id);
                match def {
                    Some(def) => self.open_event_def(def),
                    None => {
                        self.say(format!("the room is empty ({id} has no content)"));
                        self.screen = Screen::Map;
                    }
                }
            }
            None => {
                // 事件池抽干了(参考实现的 INVALID):这一格当作空房间
                self.say("the room is empty");
                self.screen = Screen::Map;
            }
        }
    }

    /// 进未知房时掷一次 eventRng,判定这是怪/商店/宝箱/事件
    /// (参考实现 resolveUnknownRoom).没判中的那一档概率涨一份起始值,
    /// 判中的那一档复位;刚逛完商店时商店那档算 0.
    fn resolve_unknown_room(&mut self) -> NodeKind {
        let roll = self.streams.run(RunStream::EventRng).random_float();
        let idx = (roll * 100.0) as i32;
        let monster_size = (self.monster_chance * 100.0) as i32;
        let shop_size = monster_size
            + if self.last_room_was_shop {
                0
            } else {
                (self.shop_chance * 100.0) as i32
            };
        let treasure_size = shop_size + (self.treasure_chance * 100.0) as i32;
        let mut kind = if idx < monster_size {
            NodeKind::Monster
        } else if idx < shop_size {
            NodeKind::Shop
        } else if idx < treasure_size {
            NodeKind::Treasure
        } else {
            NodeKind::Event
        };
        // 十手镯:? 房间不再出普通战斗(反编译是掷出战斗后把房间改成事件,
        // 但那一档概率的复位仍按"掷出来的战斗"算)
        let mut chance_kind = kind;
        if kind == NodeKind::Monster && self.has_relic_fx(|fx| fx.no_normal_combat_in_unknown) {
            kind = NodeKind::Event;
        }
        // 小箱子:每 4 个 ? 房间必出宝箱(这条路径不参与掷点,复位按宝箱算)
        if self.has_relic_fx(|fx| fx.treasure_every_4_unknown) {
            self.unknown_rooms_seen += 1;
            if self.unknown_rooms_seen % 4 == 0 {
                kind = NodeKind::Treasure;
                chance_kind = NodeKind::Treasure;
            }
        }
        self.monster_chance = if chance_kind == NodeKind::Monster {
            UNKNOWN_ESCALATION.0
        } else {
            self.monster_chance + UNKNOWN_ESCALATION.0
        };
        self.shop_chance = if kind == NodeKind::Shop {
            UNKNOWN_ESCALATION.1
        } else {
            self.shop_chance + UNKNOWN_ESCALATION.1
        };
        self.treasure_chance = if kind == NodeKind::Treasure {
            UNKNOWN_ESCALATION.2
        } else {
            self.treasure_chance + UNKNOWN_ESCALATION.2
        };
        kind
    }

    /// 抽一个事件 id(参考实现 generateEvent).抽签掷在 eventRng 的副本上:
    /// 主流的消耗只有"未知房判定"那一次 float,抽签本身不动它.
    /// 抽中的事件从池子里移除(一次性事件池同理).
    fn generate_event_id(&mut self) -> Option<&'static str> {
        let mut ev = self.streams.run(RunStream::EventRng).clone();
        let shrine_roll = ev.random_float() < SHRINE_CHANCE;
        let mut chosen = None;
        if shrine_roll {
            if self.shrine_pool.is_empty() && self.one_time_pool.is_empty() {
                if !self.event_pool.is_empty() {
                    chosen = self.pick_normal_event(&mut ev);
                }
            } else {
                chosen = self.pick_shrine_event(&mut ev);
            }
        } else {
            chosen = self.pick_normal_event(&mut ev);
            if chosen.is_none() {
                chosen = self.pick_shrine_event(&mut ev);
            }
        }
        chosen
    }

    /// 从本章事件池里抽一个(池子里的顺序就是参考实现的顺序)
    fn pick_normal_event(&mut self, ev: &mut crate::rng::Rng) -> Option<&'static str> {
        let eligible: Vec<&'static str> = self
            .event_pool
            .iter()
            .copied()
            .filter(|id| self.event_can_spawn(*id))
            .collect();
        if eligible.is_empty() {
            return None;
        }
        let id = eligible[ev.random(eligible.len() as u32 - 1) as usize];
        if let Some(i) = self.event_pool.iter().position(|x| *x == id) {
            self.event_pool.remove(i);
        }
        Some(id)
    }

    /// 抽一个神龛或一次性事件(参考实现把两张表合成一张抽,抽中的从原表里移除)
    fn pick_shrine_event(&mut self, ev: &mut crate::rng::Rng) -> Option<&'static str> {
        let mut eligible: Vec<&'static str> = self.shrine_pool.clone();
        eligible.extend(
            self.one_time_pool
                .iter()
                .copied()
                .filter(|id| self.event_can_spawn(*id)),
        );
        if eligible.is_empty() {
            return None;
        }
        let id = eligible[ev.random(eligible.len() as u32 - 1) as usize];
        if let Some(i) = self.shrine_pool.iter().position(|x| *x == id) {
            self.shrine_pool.remove(i);
        } else if let Some(i) = self.one_time_pool.iter().position(|x| *x == id) {
            self.one_time_pool.remove(i);
        }
        Some(id)
    }

    /// 这个事件现在能不能出现(参考实现各事件里的 canSpawn).act 是当前章号.
    fn event_can_spawn(&self, id: &str) -> bool {
        let act = self.act;
        // floor_num 进第一房就是 1(每进一房 +1),与参考实现的 run.floor(全局层号,
        // 1 起)是同一个数;这里不能再 +1,否则 dead_adventurer 这类门槛会提前一层
        let floor = self.floor_num as i32;
        match id {
            "the_cleric" => self.player.gold >= 35,
            "dead_adventurer" | "hypnotizing_colored_mushrooms" => floor >= 7,
            // 二、三章的几个事件各有各的门槛(照参考实现各事件里的 canSpawn)
            "old_beggar" => self.player.gold >= 75,
            "colosseum" => self.pos.is_some() && self.floor_reached > 7,
            "the_moai_head" => {
                self.player.hp * 2 <= self.player.max_hp
                    || self.player.relics.iter().any(|r| r.id == "golden_idol")
            }
            "designer_in_spire" => (act == 2 || act == 3) && self.player.gold >= 75,
            "duplicator" => act == 2 || act == 3,
            "face_trader" => act == 1 || act == 2,
            "the_divine_fountain" => self
                .player
                .deck
                .iter()
                .any(|c| c.kind() == crate::core::card::CardType::Curse),
            "knowing_skull" => act == 2 && self.player.hp >= 13,
            "nloth" => act == 2 && self.player.relics.len() >= 2,
            "note_for_yourself" => self.ascension <= 14,
            "secret_portal" => act == 3,
            "the_joust" => act == 2 && self.player.gold >= 50,
            "the_woman_in_blue" => self.player.gold >= 50,
            _ => true,
        }
    }

    /// 打开指定事件;翻牌事件顺手把 12 格棋盘铺好,dead_adventurer 掷一次奖池与伏击遭遇
    fn open_event_def(&mut self, def: &'static EventDef) {
        let match_keep = if def.id == "match_and_keep" {
            Some(MatchKeep::new(&mut self.streams, self.character, self.ascension))
        } else {
            None
        };
        // 参考实现 dead_adventurer.onEnter:洗奖池,再挑这次的伏击精英
        let adv = if def.id == "dead_adventurer" {
            Some(self.roll_dead_adventurer())
        } else {
            None
        };
        // 参考实现 we_meet_again.onEnter:按选项顺序掷三个交易目标(选项不可用就不掷)
        let wma = if def.id == "we_meet_again" {
            Some(self.roll_we_meet_again())
        } else {
            None
        };
        // 参考实现 nloth.onEnter:洗遗物下标,取前两件当 offerA/offerB
        let nloth = if def.id == "nloth" {
            Some(self.roll_nloth())
        } else {
            None
        };
        // 参考实现 designer_in_spire.onEnter:掷两个布尔决定两个服务的变体
        let designer = if def.id == "designer_in_spire" {
            Some(self.roll_designer())
        } else {
            None
        };
        let mut st = EventState::new(def);
        st.match_keep = match_keep;
        st.adv = adv;
        st.wma = wma;
        st.nloth = nloth;
        st.designer = designer;
        self.event = Some(st);
        self.screen = Screen::Event;
    }

    /// "再会"进房时掷点(参考实现 onEnter,顺序即选项顺序):
    /// 有药水就 javaShuffle 取一格,金币 >=50 就掷 50..min(150,gold),有非基础非诅咒牌就再洗一次取一张.
    fn roll_we_meet_again(&mut self) -> WeMeetAgainData {
        let mut d = WeMeetAgainData::default();
        let mut filled: Vec<usize> = self
            .player
            .potions
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_some())
            .map(|(i, _)| i)
            .collect();
        if !filled.is_empty() {
            let seed = self.streams.floor(FloorStream::MiscRng).random_long();
            java_shuffle(&mut filled, &mut JavaRandom::new(seed));
            d.potion_slot = Some(filled[0]);
        }
        if self.player.gold >= 50 {
            let hi = 150.min(self.player.gold);
            d.gold_amount = Some(
                self.streams
                    .floor(FloorStream::MiscRng)
                    .random_range(50, hi),
            );
        }
        let mut eligible: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.rarity() != Rarity::Basic && c.kind() != crate::core::card::CardType::Curse
            })
            .map(|(i, _)| i)
            .collect();
        if !eligible.is_empty() {
            let seed = self.streams.floor(FloorStream::MiscRng).random_long();
            java_shuffle(&mut eligible, &mut JavaRandom::new(seed));
            d.card_idx = Some(eligible[0]);
        }
        d
    }

    /// dead_adventurer 进房时掷一次(参考实现 onEnter):奖池三格用 miscRng 洗一遍,
    /// 再从三种精英里挑一个当伏击.
    fn roll_dead_adventurer(&mut self) -> DeadAdventurerData {
        let mut rewards = crate::core::events::DEAD_ADVENTURER_REWARDS;
        let seed = self.streams.floor(FloorStream::MiscRng).random_long();
        java_shuffle(&mut rewards, &mut JavaRandom::new(seed));
        let idx = self.streams.floor(FloorStream::MiscRng).random(2) as usize;
        DeadAdventurerData {
            rewards,
            encounter: crate::core::events::DEAD_ADVENTURER_ENCOUNTERS[idx],
            phase: 0,
        }
    }

    /// N'loth 进房时掷一次(参考实现 onEnter):把身上遗物的下标洗一遍,取前两件当 offerA/offerB
    /// (不足两件时对应项是 None,那个选项就不可选).
    fn roll_nloth(&mut self) -> NlothData {
        let mut idxs: Vec<usize> = (0..self.player.relics.len()).collect();
        let seed = self.streams.floor(FloorStream::MiscRng).random_long();
        java_shuffle(&mut idxs, &mut JavaRandom::new(seed));
        NlothData {
            offer_a: idxs.first().map(|&i| self.player.relics[i].id),
            offer_b: idxs.get(1).map(|&i| self.player.relics[i].id),
        }
    }

    /// Designer In-Spire 进房时掷两次(参考实现 onEnter,顺序:先 Adjustments 后 Clean up).
    fn roll_designer(&mut self) -> DesignerData {
        DesignerData {
            upgrade_choice: self.streams.floor(FloorStream::MiscRng).random_boolean(),
            cleanup_choice: self.streams.floor(FloorStream::MiscRng).random_boolean(),
        }
    }

    /// 选项当前是否可选(钱够、血够、该有的遗物/药水/牌都有);
    /// 翻牌事件按棋盘的格子算
    pub fn event_choice_available(&self, i: usize) -> bool {
        let Some(st) = &self.event else {
            return false;
        };
        if let Some(mk) = &st.match_keep {
            return st.result.is_none() && mk.available(i);
        }
        if let Some(offer) = &st.library {
            // 20 张候选都能选(原版必须选一张才能离开这张网格界面)
            return st.result.is_none() && i < offer.cards.len();
        }
        let Some(c) = st.def.choices.get(i) else {
            return false;
        };
        let eff = c.effective(self.ascension);
        if self.player.gold < eff.cost_gold || self.player.hp <= eff.cost_hp {
            return false;
        }
        if self.player.gold < c.req_gold {
            return false;
        }
        if let Some(id) = c.req_relic {
            if !self.player.relics.iter().any(|r| r.id == id) {
                return false;
            }
        }
        if let Some(id) = c.req_no_relic {
            if self.player.relics.iter().any(|r| r.id == id) {
                return false;
            }
        }
        if c.req_potion && self.player.potions.iter().all(|p| p.is_none()) {
            return false;
        }
        if c.req_big_attack && !self.has_big_attack() {
            return false;
        }
        if c.req_non_basic && !self.has_non_basic_card() {
            return false;
        }
        // 分期(多屏)事件的选项:只有事件停在这一屏时才可选
        if c.only_screen != st.screen {
            return false;
        }
        // 限次数的选项(dead_adventurer 的搜索最多三次)
        if c.max_uses > 0 && st.attempts >= c.max_uses {
            return false;
        }
        if c.req_removable && self.removable_cards().is_empty() {
            return false;
        }
        if c.req_upgradeable && !self.player.deck.iter().any(|c| c.can_upgrade()) {
            return false;
        }
        // 坠落:某类型一张可移除的牌都没有,对应选项锁住(原版;参考实现没做)
        if let Some(kind) = c.req_card_type {
            if !self.deck_has_removable_of_type(kind) {
                return false;
            }
        }
        // 坠落的保底"Land":只有三类牌都抽不出时才可选
        if c.req_no_card_type && self.deck_has_any_falling_card() {
            return false;
        }
        // 按全局层号开关的选项(mindbloom 的 "I am Rich" / "I am Healthy")
        if c.req_floor_max > 0 && self.floor_num > c.req_floor_max {
            return false;
        }
        if c.req_floor_min > 0 && self.floor_num < c.req_floor_min {
            return false;
        }
        // N'loth:洗到的那件供奉遗物不存在(身上不足两件)时,对应选项不可选
        if i < 2 {
            if let Some(d) = &st.nloth {
                let offer = if i == 0 { d.offer_a } else { d.offer_b };
                if offer.is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// 牌组里有没有该类型的可移除牌(没瓶装、非不可移除);"坠落"选项的可用性
    fn deck_has_removable_of_type(&self, kind: CardType) -> bool {
        self.player
            .deck
            .iter()
            .any(|c| c.kind() == kind && !c.def.unremovable && !c.bottled)
    }

    /// 技能/能力/攻击里有没有任意一张可移除牌("坠落"保底选项的可用性)
    fn deck_has_any_falling_card(&self) -> bool {
        [CardType::Skill, CardType::Power, CardType::Attack]
            .into_iter()
            .any(|k| self.deck_has_removable_of_type(k))
    }

    /// 能当祭品/能删掉的牌的下标(参考实现 removableIndices:没瓶装、不是不可移除的)
    fn removable_cards(&self) -> Vec<usize> {
        // 牌组不能被清空,留最后一张
        if self.player.deck.len() <= 1 {
            return Vec::new();
        }
        self.player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.def.unremovable && !c.bottled)
            .map(|(i, _)| i)
            .collect()
    }

    /// 事件当前有几个选项:翻牌事件按棋盘的 12 格算
    pub fn event_choice_count(&self) -> usize {
        match &self.event {
            Some(st) => match &st.library {
                Some(offer) => offer.cards.len(),
                None => match &st.match_keep {
                    Some(mk) => mk.board.len(),
                    None if !st.neow_options.is_empty() => st.neow_options.len(),
                    None => st.def.choices.len(),
                },
            },
            None => 0,
        }
    }

    /// 选项这一行的文本与要价:翻牌事件给的是棋盘上那一格(朝上就显示牌名);
    /// Neow 的标签是祝福名(带代价)
    pub fn event_choice_row(&self, i: usize) -> Option<(String, i32, i32)> {
        let st = self.event.as_ref()?;
        if let Some(offer) = &st.library {
            return Some((offer.cards.get(i)?.label(), 0, 0));
        }
        if let Some(mk) = &st.match_keep {
            return Some((mk.label(i), 0, 0));
        }
        if let Some(opt) = st.neow_options.get(i) {
            let mut label = crate::core::events::neow_bonus_label(opt.bonus).to_string();
            if opt.drawback != "none" {
                label.push_str(" (");
                label.push_str(crate::core::events::neow_drawback_label(opt.drawback));
                label.push(')');
            }
            return Some((label, 0, 0));
        }
        // N'loth:两个供奉选项的标签按进房时洗到的那件遗物现拼(参考实现 build 里的 offer(key))
        if let Some(d) = &st.nloth {
            let offer = match i {
                0 => Some(d.offer_a),
                1 => Some(d.offer_b),
                _ => None,
            };
            if let Some(offer) = offer {
                let name = offer
                    .and_then(|id| relics::relic_def(id))
                    .map(|r| r.name)
                    .unwrap_or("(none)");
                return Some((
                    format!("Offer {name}: lose that relic, obtain N'loth's Gift"),
                    0,
                    0,
                ));
            }
        }
        // Designer In-Spire:两个服务选项的标签按进房时掷到的变体现拼(参考实现 build)
        if let Some(d) = &st.designer {
            let cost = st
                .def
                .choices
                .get(i)
                .map(|c| c.effective(self.ascension).cost_gold)
                .unwrap_or(0);
            let label = match i {
                0 if d.upgrade_choice => format!("Adjustments: pay {cost} gold; upgrade a chosen card"),
                0 => format!("Adjustments: pay {cost} gold; upgrade 2 random cards"),
                1 if d.cleanup_choice => format!("Clean up: pay {cost} gold; remove a chosen card"),
                1 => format!("Clean up: pay {cost} gold; transform 2 random cards"),
                _ => String::new(),
            };
            if !label.is_empty() {
                return Some((label, cost, 0));
            }
        }
        let c = st.def.choices.get(i)?;
        let eff = c.effective(self.ascension);
        Some((eff.label.to_string(), eff.cost_gold, eff.cost_hp))
    }

    /// 牌组里有没有单次伤害 10 以上的攻击牌(Wing Statue 砸雕像的条件)
    fn has_big_attack(&self) -> bool {
        self.player.deck.iter().any(|c| {
            c.kind() == crate::core::card::CardType::Attack
                && c.effects().iter().any(|e| {
                    // 按"单次伤害"算,多段攻击不累加(语料 WING_STATUE 的
                    // deckHasAttackWithSingleHitDamageAtLeast;参考实现 deckHasBigSingleHit 同)
                    let amount = match *e {
                        CardEffect::Damage { amount, .. }
                        | CardEffect::DamageWithBonus { amount, .. }
                        | CardEffect::DamageAndKillBonusSelf { amount, .. } => amount,
                        _ => return false,
                    };
                    amount + c.bonus >= 10
                })
        })
    }

    /// 牌组里有没有非基础、非诅咒的牌
    fn has_non_basic_card(&self) -> bool {
        self.player
            .deck
            .iter()
            .any(|c| c.rarity() != Rarity::Basic && c.kind() != crate::core::card::CardType::Curse)
    }

    pub fn choose_event(&mut self, i: usize) -> Result<(), String> {
        let Some(st) = self.event.as_ref() else {
            return Err("no event here".to_string());
        };
        if st.result.is_some() {
            return Err("already resolved".to_string());
        }
        if st.match_keep.is_some() {
            return self.flip_event_card(i);
        }
        // the_library 的 Read:候选已经摆出来时,选中的那张进牌组,事件收尾
        if st.library.is_some() {
            return self.take_library_card(i);
        }
        // Neow 的选项不走 outcome:先结算代价,再结算祝福
        if let Some(opt) = st.neow_options.get(i).copied() {
            let text = self.apply_neow(opt.bonus, opt.drawback)?;
            // 发牌/选牌这类会另开一屏的祝福自己把事件界面收掉了
            if self.screen == Screen::Event {
                if let Some(st) = self.event.as_mut() {
                    st.result = Some(text.to_string());
                }
            }
            return Ok(());
        }
        let Some(choice) = st.def.choices.get(i).copied() else {
            return Err("no such choice".to_string());
        };
        if !self.event_choice_available(i) {
            return Err("that choice is not available".to_string());
        }
        // 飞升 15+ 的选项覆盖(代价/效果)
        let choice = choice.effective(self.ascension);
        // the_library 的 Read:先掷好 20 张候选摆在这一屏上,等玩家挑一张
        if choice.outcome.library_read {
            return self.open_library_read(choice.outcome.text);
        }
        // 废料泥怪:"把手伸进去"可反复尝试,自己掷点决定去留,不走"结算完写 result"的流程
        if choice.outcome.ooze {
            return self.ooze_attempt();
        }
        // dead_adventurer 的搜索:掷伏击/领奖池,也自己决定去留
        if choice.outcome.adv_search {
            return self.dead_adventurer_search();
        }
        // 会说话的骷髅:先按这一项当前的价格扣血,死了就到这为止;
        // 买完留在本屏(可以接着买),所以这里结算完直接返回,不写 result
        if choice.outcome.skull_buy > 0 {
            self.skull_buy(choice.outcome.skull_buy as usize);
            if self.player.hp <= 0 {
                return Ok(());
            }
            let _ = self.apply_outcome(&choice.outcome);
            return Ok(());
        }
        if choice.cost_gold > 0 {
            self.spend_gold(choice.cost_gold);
        }
        if choice.cost_hp > 0 {
            self.damage(choice.cost_hp);
        }
        let outcome = choice.outcome;
        let text = self.apply_outcome(&outcome);
        // 多屏事件:这一屏结算完直接换选项,不给"看完结果再按回车"的停顿
        if let Some(next) = outcome.next {
            if let Some(st) = self.event.as_mut() {
                st.def = next;
                st.index = 0;
                st.result = None;
            }
            return Ok(());
        }
        // 换屏但选项表不变(golden_idol 的陷阱屏):留在事件屏,可用性按新屏算
        if let Some(screen) = outcome.set_screen {
            if let Some(st) = self.event.as_mut() {
                st.screen = Some(screen);
                st.index = 0;
                st.result = None;
            }
            return Ok(());
        }
        if let Some(st) = self.event.as_mut() {
            st.result = Some(text.to_string());
        }
        Ok(())
    }

    /// dead_adventurer 的搜索(参考实现 act1.ts 的 search 选项):
    /// 先掷伏击(25% + 25% * 已经搜过的次数),中了就开一场精英战、打完再发奖;
    /// 没中就领当前阶段的那格奖池(30 金币 / 空 / 随机遗物)并把阶段推进一格.
    fn dead_adventurer_search(&mut self) -> Result<(), String> {
        let Some(st) = self.event.as_ref() else {
            return Err("no event here".to_string());
        };
        let Some(mut d) = st.adv else {
            return Err("not the dead adventurer".to_string());
        };
        if d.phase >= crate::core::events::DEAD_ADVENTURER_MAX_SEARCHES {
            return Err("already searched three times".to_string());
        }
        let base = if self.ascension >= 15 { 35 } else { crate::core::events::DEAD_ADVENTURER_AMBUSH_BASE };
        let chance = base + 25 * d.phase as i32;
        if self.streams.floor(FloorStream::MiscRng).random(99) < chance as u32 {
            // 伏击:剩下的奖池在打完之后折成奖励屏(参考实现 onCombatVictory)
            let enc = enemies::resolve(d.encounter);
            self.pending_adv = Some(d);
            self.start_combat(enc, false);
            // 拉格文在事件战斗里是醒着的(参考实现 suppressPreBattle)
            if d.encounter == "lagavulin_solo" {
                self.wake_sleepers();
            }
            return Ok(());
        }
        let reward = d.rewards[d.phase as usize];
        d.phase += 1;
        if let Some(st) = self.event.as_mut() {
            st.adv = Some(d);
            st.attempts += 1;
        }
        match reward {
            "GOLD" => {
                self.gain_gold(crate::core::events::DEAD_ADVENTURER_GOLD);
                self.say("you find a pouch of gold");
            }
            "RELIC" => {
                if let Some(def) = self.take_relic_of_any() {
                    self.gain_relic(def);
                }
            }
            _ => self.say("you find nothing worth taking"),
        }
        Ok(())
    }

    /// 把敌人身上的"睡眠"去掉(事件战斗里拉格文是醒着的):
    /// 参考实现的 suppressPreBattle 会把敌人的 preBattle 抹掉,这里抹掉开局的
    /// 睡眠/金属化与那 8 点格挡.
    fn wake_sleepers(&mut self) {
        if let Some(c) = self.combat.as_mut() {
            for e in c.enemies.iter_mut() {
                if e.def.id == "lagavulin" {
                    e.statuses.set(Status::Asleep, 0);
                    e.statuses.set(Status::Metallicize, 0);
                    e.block = 0;
                }
            }
        }
    }

    /// 废料泥怪:"把手伸进去"可以反复尝试——先扣 3 点血,再掷一次
    /// (25% 起步、每次失败涨 10%);中了给一件随机遗物并收尾,没中就留在这屏,
    /// 下次还能再伸(掷点在扣血之后,和参考实现同序).
    fn ooze_attempt(&mut self) -> Result<(), String> {
        // 飞升 15+:伸手的代价从 3 点血涨到 5 点
        self.damage(if self.ascension >= 15 { 5 } else { 3 });
        if self.player.hp <= 0 {
            return Ok(());
        }
        let attempts = self.event.as_ref().map(|e| e.attempts).unwrap_or(0);
        let chance = 25 + 10 * attempts as i32;
        if self.streams.floor(FloorStream::MiscRng).random(99) >= (99 - chance) as u32 {
            if let Some(def) = self.take_relic_of_any() {
                self.gain_relic(def);
            }
            if let Some(st) = self.event.as_mut() {
                st.result = Some("You pull a relic out of the muck.".to_string());
            }
        } else if let Some(st) = self.event.as_mut() {
            st.attempts += 1;
        }
        Ok(())
    }

    /// 会说话的骷髅:扣血价 = max(6, 10% 生命上限的 floor) + 这一项已经买过的次数
    /// (参考实现 skullBase + 各选项自己的计数器);扣完再把这一个计数加一.
    fn skull_buy(&mut self, key: usize) {
        let idx = (key - 1).min(2);
        let base = crate::core::events::frac_floor_of(self.player.max_hp, 0.1).max(6);
        let extra = self
            .event
            .as_ref()
            .and_then(|s| s.skull.get(idx).copied())
            .unwrap_or(0);
        let price = base + extra as i32;
        if let Some(st) = self.event.as_mut() {
            st.skull[idx] += 1;
        }
        self.damage(price);
    }

    /// 翻牌小游戏(match_and_keep):翻第 i 格.
    /// 第一张只是翻开;翻第二张才算一次尝试,同源的两张进牌组并一直朝上,
    /// 不同源的两张翻回背面.次数用完(或 12 格全配对)事件结束,只剩离开.
    fn flip_event_card(&mut self, i: usize) -> Result<(), String> {
        let Some(mut board) = self.event.as_mut().and_then(|st| st.match_keep.take()) else {
            return Err("no board here".to_string());
        };
        if !board.available(i) {
            if let Some(st) = self.event.as_mut() {
                st.match_keep = Some(board);
            }
            return Err("that card can not be flipped".to_string());
        }
        let first = board.first;
        let result = board.flip(i);
        let name = |id: Option<&'static str>| match id {
            Some(id) => card_def_any(id).name,
            None => "(empty)",
        };
        let mut text = String::new();
        match result {
            FlipResult::First => text.push_str(&format!("you flip {}", name(board.card_at(i)))),
            FlipResult::Miss => {
                let a = name(first.and_then(|f| board.card_at(f)));
                text.push_str(&format!("{a} and {} do not match", name(board.card_at(i))));
            }
            FlipResult::Matched => match board.card_at(i) {
                Some(id) => {
                    self.add_card_id(id, 1);
                    text.push_str(&format!("{} matches: it joins your deck", name(Some(id))));
                }
                None => text.push_str("the pair matches, but there was nothing to take"),
            },
        }
        let done = board.finished();
        board.note = Some(text);
        if let Some(st) = self.event.as_mut() {
            st.match_keep = Some(board);
            if done {
                st.result = Some("The memory game is over; the pairs you matched are yours.".to_string());
            }
        }
        Ok(())
    }

    /// 翻牌棋盘下面那行说明(上一次翻牌的结果);其它事件没有
    pub fn event_note(&self) -> Option<&str> {
        self.event.as_ref()?.match_keep.as_ref()?.note.as_deref()
    }

    /// 事件屏上摆着的候选牌(the_library 的 Read 那 20 张);其它事件是 None
    pub fn event_offer(&self) -> Option<&[CardInstance]> {
        Some(self.event.as_ref()?.library.as_ref()?.cards.as_slice())
    }

    /// the_library 的 Read:按原版"20 张选一张"掷候选(参考实现 requestOptionChoice).
    /// 每张先按事件档掷稀有度(cardRng;恩洛斯的礼物照样把稀有概率翻三倍),
    /// 再从该稀有度的本职业池里抽一张,重样的重掷;候选的展示顺序是生成顺序倒过来
    /// (参考实现同款).note 是挑走时显示的结果文本.
    fn open_library_read(&mut self, note: &'static str) -> Result<(), String> {
        let mut rolled: Vec<&'static CardDef> = Vec::new();
        for _ in 0..LIBRARY_CARD_COUNT {
            let rarity = self.roll_card_rarity(EnemyKind::Normal);
            let pool = cards::reward_pool(rarity);
            if pool.is_empty() {
                continue;
            }
            let mut def = *self.streams.run(RunStream::CardRng).pick(&pool);
            let mut guard = 0;
            while rolled.iter().any(|c| c.id == def.id) {
                guard += 1;
                if guard >= 1000 {
                    break;
                }
                def = *self.streams.run(RunStream::CardRng).pick(&pool);
            }
            rolled.push(def);
        }
        rolled.reverse();
        let offer = LibraryOffer {
            cards: rolled.into_iter().map(CardInstance::new).collect(),
            note,
        };
        let Some(st) = self.event.as_mut() else {
            return Err("no event here".to_string());
        };
        st.index = 0;
        st.library = Some(offer);
        Ok(())
    }

    /// the_library:把候选里的第 i 张收进牌组(蛋遗物照常升级),事件收尾
    fn take_library_card(&mut self, i: usize) -> Result<(), String> {
        let (card, note) = {
            let Some(offer) = self.event.as_ref().and_then(|st| st.library.as_ref()) else {
                return Err("no books here".to_string());
            };
            let card = offer
                .cards
                .get(i)
                .cloned()
                .ok_or_else(|| "no such card".to_string())?;
            (card, offer.note)
        };
        let label = card.label();
        self.push_card_to_deck(card);
        if let Some(st) = self.event.as_mut() {
            st.library = None;
            st.result = Some(format!("{note} ({label})"));
        }
        Ok(())
    }

    /// 事件结果结算;返回给玩家看的文本.
    /// 百分比一律按"结算前的生命上限"算(先加上限再扣血的那几个事件也照原作来).
    fn apply_outcome(&mut self, o: &Outcome) -> &'static str {
        let max_hp0 = self.player.max_hp;
        // 先金币后扣血的顺序(脸商人):把金币提到生命结算之前
        if o.gold_first && o.gold != 0 {
            if o.gold > 0 {
                self.gain_gold(o.gold);
            } else {
                let g = (-o.gold).min(self.player.gold);
                self.spend_gold(g);
            }
        }
        if o.max_hp != 0 {
            self.player.max_hp = (self.player.max_hp + o.max_hp).max(1);
            if o.max_hp > 0 {
                // 上限涨多少就回多少(原版 increaseMaxHp = maxHp += n; heal(n)),
                // 所以花开彼岸下只加上限、不回血
                self.heal(o.max_hp);
            }
        }
        if o.max_hp_pct > 0 {
            let loss = crate::core::events::pct_of(max_hp0, o.max_hp_pct).max(1);
            self.player.max_hp = (self.player.max_hp - loss).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
        if o.max_hp_frac > 0.0 {
            let loss = crate::core::events::frac_floor_of(max_hp0, o.max_hp_frac);
            self.player.max_hp = (self.player.max_hp - loss).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
        if o.max_hp_frac_ceil > 0.0 {
            let loss = crate::core::events::frac_ceil_of(max_hp0, o.max_hp_frac_ceil);
            self.player.max_hp = (self.player.max_hp - loss).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
        let mut delta = o.hp;
        if o.hp_pct > 0 {
            let pct = crate::core::events::pct_of(max_hp0, o.hp_pct).max(o.hp_pct_min.max(1));
            delta -= pct;
        }
        if o.hp_frac > 0.0 {
            let loss = crate::core::events::frac_floor_of(max_hp0, o.hp_frac);
            // 有的调用点给 frac 也带下限(脸商人的"至少 1 点");下限为 0 就不设
            delta -= if o.hp_pct_min > 0 { loss.max(o.hp_pct_min) } else { loss };
        }
        if o.hp_frac_ceil > 0.0 {
            delta -= crate::core::events::frac_ceil_of(max_hp0, o.hp_frac_ceil);
        }
        if delta < 0 {
            self.damage(-delta);
        } else if delta > 0 {
            self.heal(delta);
        }
        if o.heal_pct > 0 {
            self.heal(crate::core::events::pct_of(max_hp0, o.heal_pct));
        }
        if o.heal_frac > 0.0 {
            self.heal(crate::core::events::frac_floor_of(max_hp0, o.heal_frac));
        }
        if o.full_heal {
            // 花开彼岸:回满也归零(原版走 heal())
            self.heal(self.player.max_hp);
        }
        if o.gold != 0 && !o.gold_first {
            if o.gold > 0 {
                self.gain_gold(o.gold);
            } else {
                let g = (-o.gold).min(self.player.gold);
                self.spend_gold(g);
            }
        }
        if let Some((lo, hi)) = o.gold_range {
            let g = self.streams.floor(FloorStream::MiscRng).range_inclusive(lo, hi);
            self.gain_gold(g);
        }
        if o.gold_lose_all {
            let g = self.player.gold;
            self.spend_gold(g);
        }
        if let Some((lo, hi)) = o.gold_lose_range {
            let g = self.streams.floor(FloorStream::MiscRng).range_inclusive(lo, hi).min(self.player.gold);
            self.spend_gold(g);
        }
        if let Some(id) = o.remove_relic {
            self.remove_relic_by_id(id);
        }
        // N'loth:吃掉进房时掷定的那件供奉遗物(参考实现 removeRelic(d[key]))
        if o.nloth_offer > 0 {
            let offer = self.event.as_ref().and_then(|st| st.nloth).and_then(|d| {
                if o.nloth_offer == 1 {
                    d.offer_a
                } else {
                    d.offer_b
                }
            });
            if let Some(id) = offer {
                self.remove_relic_by_id(id);
            }
        }
        if let Some(id) = o.relic_id {
            let def = relic_def_any(id);
            self.gain_relic(def);
        }
        if let Some(rarity) = o.random_relic_rarity {
            if let Some(def) = self.take_relic_of(rarity) {
                self.gain_relic(def);
            }
        }
        if o.random_relic_any {
            if let Some(def) = self.take_relic_of_any() {
                self.gain_relic(def);
            }
        }
        // 从指定遗物表里挑一件自己没有的(脸商人的交易):洗一遍取第一件,都有就给 Circlet
        if let Some(table) = o.pick_relic_from {
            let mut cands: Vec<&'static RelicDef> = table
                .iter()
                .filter(|id| !self.player.relics.iter().any(|r| r.id == **id))
                .map(|id| relic_def_any(id))
                .collect();
            let def = if cands.is_empty() {
                relic_def_any("circlet")
            } else {
                let seed = self.streams.floor(FloorStream::MiscRng).random_long();
                java_shuffle(&mut cands, &mut JavaRandom::new(seed));
                cands[0]
            };
            self.gain_relic(def);
        }
        // 一件随机遗物开成奖励屏(转盘转到遗物那一格):拿不拿由玩家决定
        if o.relic_reward {
            let mut r = RewardState::empty(Screen::Map);
            r.relic = self.take_relic_of_any();
            r.relic_taken = false;
            self.open_event_reward(r);
        }
        if let Some(id) = o.relic_reward_id {
            // 指定遗物摆进奖励屏,不进遗物池
            let mut r = RewardState::empty(Screen::Map);
            r.relic = Some(relic_def_any(id));
            r.relic_taken = false;
            self.open_event_reward(r);
        }
        if let Some(id) = o.add_card {
            self.add_card_id(id, 1);
        }
        if let Some((id, n)) = o.add_cards {
            self.add_card_id(id, n);
        }
        if let Some(id) = o.add_curse {
            self.add_card_id(id, 1);
        }
        if o.add_random_curse {
            let pool = cards::curses();
            if !pool.is_empty() {
                let def = *self.streams.floor(FloorStream::MiscRng).pick(&pool);
                self.push_card_to_deck(CardInstance::new(def));
            }
        }
        if let Some((rarity, n)) = o.add_random_class {
            for _ in 0..n {
                if let Some(def) = self.random_class_card(Some(rarity)) {
                    self.push_card_to_deck(CardInstance::new(def));
                }
            }
        }
        if let Some((rarity, n)) = o.add_random_colorless {
            for _ in 0..n {
                if let Some(def) = self.random_colorless_card(rarity) {
                    self.push_card_to_deck(CardInstance::new(def));
                }
            }
        }
        // sensory_stone 的 Recall:无色牌其实是卡牌奖励(参考实现 extraCardGroups),
        // 每组三张里选一张,选完(或跳过)顶下一组.扣血致死不推开奖励屏.
        if o.colorless_card_rewards > 0 && self.screen != Screen::Death {
            let mut r = RewardState::empty(Screen::Map);
            for i in 0..o.colorless_card_rewards {
                let group = self.create_colorless_card_reward();
                if i == 0 {
                    r.cards = group;
                    r.card_taken = false;
                } else {
                    r.queued.push(group);
                }
            }
            self.open_event_reward(r);
        }
        if o.upgrade_all || o.upgrade_starters {
            let starters = o.upgrade_starters;
            let mut n = 0;
            for c in self.player.deck.iter_mut() {
                if !c.can_upgrade() {
                    continue;
                }
                if starters && !matches!(c.def.id, "strike" | "defend") {
                    continue;
                }
                c.upgrade();
                n += 1;
            }
            if n > 0 {
                self.say(format!("{n} cards are upgraded"));
            }
        }
        // 洗一遍可升级的牌再取前 n 张(参考实现 shining_light)
        if o.upgrade_random_shuffle > 0 {
            let mut cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if !cands.is_empty() {
                let seed = self.streams.floor(FloorStream::MiscRng).random_long();
                java_shuffle(&mut cands, &mut JavaRandom::new(seed));
                for &i in cands.iter().take(o.upgrade_random_shuffle as usize) {
                    self.player.deck[i].upgrade();
                }
            }
        }
        for _ in 0..o.upgrade_random_n {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if cands.is_empty() {
                break;
            }
            let pick = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
            self.player.deck[pick].upgrade();
            let name = self.player.deck[pick].label();
            self.say(format!("{name} is upgraded"));
        }
        if o.remove_base_strikes {
            let mut i = 0;
            while i < self.player.deck.len() {
                // 原版按稀有度吃掉所有起始打击(升级过的也算),这里按 id 认
                let hit = self.player.deck[i].def.id == "strike";
                if hit {
                    let card = self.player.deck.remove(i);
                    self.pay_deck_leave_cost(&card);
                } else {
                    i += 1;
                }
            }
        }
        if o.remove_curses {
            let mut i = 0;
            while i < self.player.deck.len() {
                let c = &self.player.deck[i];
                let hit = c.kind() == crate::core::card::CardType::Curse && !c.def.unremovable;
                if hit {
                    let card = self.player.deck.remove(i);
                    self.pay_deck_leave_cost(&card);
                } else {
                    i += 1;
                }
            }
        }
        if let Some(rule) = o.remove_random {
            // "坠落"随机夺走的那张牌:原版会把封进瓶子的牌排除在外
            // (参考实现这里没做,是它自认的 TODO;本作按原版来)
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    if c.def.unremovable || c.bottled {
                        return false;
                    }
                    match rule {
                        RemoveRule::OfType(kind) => c.kind() == kind,
                    }
                })
                .map(|(i, _)| i)
                .collect();
            if !cands.is_empty() {
                let idx = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
                let card = self.player.deck.remove(idx);
                let name = card.label();
                self.pay_deck_leave_cost(&card);
                self.say(format!("{name} is lost"));
            }
        }
        for _ in 0..o.transform_random_n {
            let cands: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.def.unremovable)
                .map(|(i, _)| i)
                .collect();
            if cands.is_empty() {
                break;
            }
            let idx = cands[self.streams.floor(FloorStream::MiscRng).below(cands.len() as u32) as usize];
            self.transform_deck_card(idx);
        }
        if o.lose_random_potion {
            let slots: Vec<usize> = self
                .player
                .potions
                .iter()
                .enumerate()
                .filter(|(_, p)| p.is_some())
                .map(|(i, _)| i)
                .collect();
            if !slots.is_empty() {
                let slot = slots[self.streams.floor(FloorStream::MiscRng).below(slots.len() as u32) as usize];
                let gone = self.player.potions[slot].take();
                if let Some(def) = gone {
                    self.say(format!("{} is given away", def.name));
                }
            }
        }
        // "再会"事件:目标在进房时就掷好了,这里只照做
        if o.wma > 0 {
            let data = self.event.as_ref().and_then(|st| st.wma);
            match (o.wma, data) {
                (1, Some(d)) => {
                    if let Some(slot) = d.potion_slot {
                        if let Some(def) = self.player.potions.get_mut(slot).and_then(|p| p.take()) {
                            self.say(format!("{} is given away", def.name));
                        }
                    }
                }
                (2, Some(d)) => {
                    if let Some(amount) = d.gold_amount {
                        let g = amount.min(self.player.gold);
                        self.spend_gold(g);
                    }
                }
                (3, Some(d)) => {
                    if let Some(idx) = d.card_idx {
                        if idx < self.player.deck.len() {
                            let card = self.player.deck.remove(idx);
                            let name = card.label();
                            self.pay_deck_leave_cost(&card);
                            self.say(format!("{name} is lost"));
                        }
                    }
                }
                _ => {}
            }
        }
        for _ in 0..o.random_potion_n {
            if let Some(def) = potions::random_potion(self.streams.run(RunStream::PotionRng), potions::class_color(self.character)) {
                self.add_potion(def);
            }
        }
        // 药水奖励屏(实验室/蓝衣女子):掷好 n 瓶摆进奖励屏,拿不拿由玩家决定
        if o.potion_reward_n > 0 {
            self.open_potion_reward(o.potion_reward_n);
        }
        if let Some((table, kind)) = o.roll {
            use crate::core::events::RollKind;
            let i = match kind {
                RollKind::Uniform => self
                    .streams
                    .floor(FloorStream::MiscRng)
                    .random(table.len() as u32 - 1) as usize,
                RollKind::Coin { num, den, true_idx } => {
                    let hit = self
                        .streams
                        .floor(FloorStream::MiscRng)
                        .random_bool_chance(num as f32 / den.max(1) as f32);
                    if hit {
                        true_idx as usize
                    } else {
                        1 - true_idx as usize
                    }
                }
                RollKind::HalfBit => {
                    let hit = self.streams.floor(FloorStream::MiscRng).random_boolean();
                    if hit { 0 } else { 1 }
                }
                RollKind::Always { idx } => idx as usize,
            };
            let sub = table[i];
            self.apply_outcome(&sub);
        }
        if o.jump_to_boss {
            self.jump_to_boss();
            return o.text;
        }
        let fight_id = match (o.fight, o.fight_pool) {
            (Some(id), _) => Some(id),
            (None, Some(pool)) if !pool.is_empty() => {
                // 原版是 Collections.shuffle(候选, new Random(miscRng.randomLong())) 之后取第一个
                // (语料 events.json 的 MINDBLOOM:RANDOM_ACT1_BOSS),不是单抽一个下标.
                let mut order: Vec<&'static str> = pool.to_vec();
                let seed = self.streams.floor(FloorStream::MiscRng).random_long();
                java_shuffle(&mut order, &mut JavaRandom::new(seed));
                Some(order[0])
            }
            _ => None,
        };
        if let Some(enc_id) = fight_id {
            let enc = enemies::encounter_def(enc_id)
                .or_else(|| crate::core::enemy::event_encounter(enc_id))
                .unwrap_or_else(|| panic!("unknown encounter: {enc_id}"));
            self.combat_reward = o.fight_reward;
            self.pending_event = o.fight_next;
            self.start_combat(enc, false);
            return o.text;
        }
        // "A Note For Yourself":取回存档里那张牌(带升级),再开一次选牌界面把它写回存档
        if o.note_swap {
            self.note_recall();
            self.open_picker(PickPurpose::Remove, Screen::Event, 0, None);
            if let Some(p) = self.picker.as_mut() {
                p.store_note = true;
            }
        } else if o.remove_card {
            if !self.picker_would_be_empty(PickPurpose::Remove) {
                self.open_picker(PickPurpose::Remove, Screen::Event, 0, None);
            }
        } else if o.upgrade_card {
            if !self.picker_would_be_empty(PickPurpose::Upgrade) {
                self.open_picker(PickPurpose::Upgrade, Screen::Event, 0, None);
            }
        } else if o.transform_choose_n > 0 {
            // 增强器的"选 2 张变形":和 Neow 的变形两张走同一套选牌界面
            if !self.picker_would_be_empty(PickPurpose::Transform) {
                self.open_picker_n(PickPurpose::Transform, Screen::Event, 0, None, o.transform_choose_n);
            }
        } else if o.transform_card {
            if !self.picker_would_be_empty(PickPurpose::Transform) {
                self.open_picker(PickPurpose::Transform, Screen::Event, 0, None);
            }
        } else if o.duplicate_card {
            if !self.picker_would_be_empty(PickPurpose::Duplicate) {
                self.open_picker(PickPurpose::Duplicate, Screen::Event, 0, None);
            }
        } else if o.offer_card {
            if !self.picker_would_be_empty(PickPurpose::Offer) {
                self.open_picker(PickPurpose::Offer, Screen::Event, 0, None);
            }
        }
        // Designer In-Spire:进房时掷好的变体决定这一项服务怎么结算(参考实现 build/onResume)
        if o.designer_service > 0 {
            let d = self.event.as_ref().and_then(|st| st.designer).unwrap_or_default();
            match o.designer_service {
                1 if d.upgrade_choice => {
                    self.open_picker(PickPurpose::Upgrade, Screen::Event, 0, None);
                }
                1 => {
                    self.apply_outcome(&crate::outcome!(upgrade_random_n: 2, text: ""));
                }
                2 if d.cleanup_choice => {
                    self.open_picker(PickPurpose::Remove, Screen::Event, 0, None);
                }
                2 => {
                    self.apply_outcome(&crate::outcome!(transform_random_n: 2, text: ""));
                }
                _ => {}
            }
        }
        if o.dead {
            self.player.hp = 0;
            self.screen = Screen::Death;
        }
        o.text
    }

    // ---- Neow 的祝福结算(参考实现 neow.ts) ----

    /// 结算 Neow 的一项:先结算代价,再结算祝福(与参考实现同序)
    fn apply_neow(&mut self, bonus: &'static str, drawback: &str) -> Result<String, String> {
        self.apply_neow_drawback(drawback);
        match bonus {
            "three_cards" | "three_rare_cards" => {
                let rare_only = bonus == "three_rare_cards";
                let cards = self.neow_class_card_reward(rare_only);
                self.open_neow_card_reward(cards);
                Ok(String::new())
            }
            "random_colorless" | "random_colorless_2" => {
                let rare_only = bonus == "random_colorless_2";
                let cards = self.neow_colorless_card_reward(rare_only);
                self.open_neow_card_reward(cards);
                Ok(String::new())
            }
            "one_random_rare_card" => {
                let pool = cards::reward_pool(Rarity::Rare);
                if pool.is_empty() {
                    return Err("rare card pool is empty".to_string());
                }
                let idx = self.streams.run(RunStream::NeowRng).random(pool.len() as u32 - 1) as usize;
                let id = pool[idx].id;
                let name = cards::card_def_or_panic(id).name;
                self.add_card_id(id, 1);
                Ok(format!("{name} joins your deck"))
            }
            "remove_card" => {
                self.open_picker_n(PickPurpose::Remove, Screen::Map, 0, None, 1);
                Ok(String::new())
            }
            "remove_two" => {
                self.open_picker_n(PickPurpose::Remove, Screen::Map, 0, None, 2);
                Ok(String::new())
            }
            "upgrade_card" => {
                self.open_picker_n(PickPurpose::Upgrade, Screen::Map, 0, None, 1);
                Ok(String::new())
            }
            "transform_card" => {
                self.open_picker_n(PickPurpose::Transform, Screen::Map, 0, None, 1);
                Ok(String::new())
            }
            "transform_two_cards" => {
                self.open_picker_n(PickPurpose::Transform, Screen::Map, 0, None, 2);
                Ok(String::new())
            }
            "three_small_potions" => {
                let mut got = 0;
                for _ in 0..crate::core::events::NEOW_THREE_SMALL_POTIONS {
                    if let Some(def) = potions::random_potion(self.streams.run(RunStream::PotionRng), potions::class_color(self.character)) {
                        // 药水栏满了就没了(参考实现同样丢掉)
                        if self.add_potion(def) {
                            got += 1;
                        }
                    }
                }
                Ok(format!("{got} potions materialize"))
            }
            "random_common_relic" => Ok(self.neow_relic(RelicTier::Common)),
            "one_rare_relic" => Ok(self.neow_relic(RelicTier::Rare)),
            "boss_relic" => Ok(self.neow_relic(RelicTier::Boss)),
            "ten_percent_hp_bonus" => {
                let gain =
                    (self.player.max_hp as f64 * crate::core::events::NEOW_TEN_PERCENT_HP_BONUS).floor() as i32;
                self.player.max_hp += gain;
                Ok(format!("max HP +{gain}"))
            }
            "twenty_percent_hp_bonus" => {
                let gain =
                    (self.player.max_hp as f64 * crate::core::events::NEOW_TWENTY_PERCENT_HP_BONUS).floor()
                        as i32;
                self.player.max_hp += gain;
                Ok(format!("max HP +{gain}"))
            }
            "hundred_gold" => {
                self.gain_gold(crate::core::events::NEOW_HUNDRED_GOLD);
                Ok(format!("+{} gold", crate::core::events::NEOW_HUNDRED_GOLD))
            }
            "two_fifty_gold" => {
                self.gain_gold(crate::core::events::NEOW_TWO_FIFTY_GOLD);
                Ok(format!("+{} gold", crate::core::events::NEOW_TWO_FIFTY_GOLD))
            }
            "three_enemy_kill" => {
                // 原版/参考实现是把 Neow's Lament 当成一件遗物发下来(onEquip 置 3 次),
                // 而不是只记一个裸计数器:遗物列表本身要对齐,拾取也走同一条路径.
                let def = relics::relic_def_or_panic("neows_lament");
                self.gain_relic(def);
                Ok(format!(
                    "Neow's Lament: the next {} combats start with 1 HP enemies",
                    crate::core::events::NEOW_THREE_ENEMY_KILL
                ))
            }
            other => Err(format!("unknown Neow blessing '{other}'")),
        }
    }

    /// Neow 的代价(先于祝福结算)
    fn apply_neow_drawback(&mut self, drawback: &str) {
        match drawback {
            "ten_percent_hp_loss" => {
                self.player.max_hp -= self.player.max_hp / 10;
                self.player.hp = self.player.hp.min(self.player.max_hp);
            }
            "no_gold" => self.player.gold = 0,
            "curse" => {
                // 随机一张诅咒(参考实现走 cardRng)
                let pool = cards::curses();
                if !pool.is_empty() {
                    let idx =
                        self.streams.run(RunStream::CardRng).random(pool.len() as u32 - 1) as usize;
                    let id = pool[idx].id;
                    self.add_card_id(id, 1);
                }
            }
            "percent_damage" => {
                // 当前生命的 30%,整数除法后再扣,至少留 1 点
                let loss = (self.player.hp / 10) * 3;
                self.player.hp = (self.player.hp - loss).max(1);
            }
            "lose_starter_relic" => {
                if !self.player.relics.is_empty() {
                    let gone = self.player.relics.remove(0);
                    self.say(format!("{} is gone", gone.name));
                }
            }
            _ => {}
        }
    }

    /// Neow 白给一件遗物:从对应档次的池子里取一件,拿到的立刻生效
    fn neow_relic(&mut self, tier: RelicTier) -> String {
        let def = self.take_relic_of_tier(tier);
        self.gain_relic(def);
        format!("{} is yours", def.name)
    }

    /// Neow 的"三张本职业牌":每张先掷稀有度(neowRng 掷 0.33 出非普通),
    /// 再从该档池子里抽一张,同一次不重样(参考实现 neowClassCardReward)
    fn neow_class_card_reward(&mut self, rare_only: bool) -> Vec<CardInstance> {
        let mut ids: Vec<&'static str> = Vec::new();
        for _ in 0..3 {
            let rarity = if rare_only {
                Rarity::Rare
            } else if self
                .streams
                .run(RunStream::NeowRng)
                .random_bool_chance(crate::core::events::NEOW_CARD_UNCOMMON_CHANCE)
            {
                Rarity::Uncommon
            } else {
                Rarity::Common
            };
            let pool = cards::reward_pool(rarity);
            if pool.is_empty() {
                break;
            }
            let mut id = pool[self.streams.run(RunStream::NeowRng).random(pool.len() as u32 - 1) as usize].id;
            let mut guard = 0;
            while ids.contains(&id) && guard < 1000 {
                guard += 1;
                id =
                    pool[self.streams.run(RunStream::NeowRng).random(pool.len() as u32 - 1) as usize].id;
            }
            ids.push(id);
        }
        ids.into_iter().map(cards::card).collect()
    }

    /// Neow 的三张无色牌:稀有度掷点在 neowRng(掷了也不改结果,普通一律升成非普通),
    /// 牌的身份走 cardRng(参考实现 neowColorlessCardReward)
    fn neow_colorless_card_reward(&mut self, rare_only: bool) -> Vec<CardInstance> {
        let mut ids: Vec<&'static str> = Vec::new();
        for _ in 0..3 {
            let rarity = if rare_only {
                Rarity::Rare
            } else {
                // 这一掷只为对齐流位置:掷出来是普通也当非普通发
                self.streams
                    .run(RunStream::NeowRng)
                    .random_bool_chance(crate::core::events::NEOW_CARD_UNCOMMON_CHANCE);
                Rarity::Uncommon
            };
            let pool: Vec<&CardDef> = cards::colorless_pool()
                .into_iter()
                .filter(|c| c.rarity == rarity)
                .collect();
            if pool.is_empty() {
                break;
            }
            let mut id = pool[self.streams.run(RunStream::CardRng).random(pool.len() as u32 - 1) as usize].id;
            let mut guard = 0;
            while ids.contains(&id) && guard < 1000 {
                guard += 1;
                id =
                    pool[self.streams.run(RunStream::CardRng).random(pool.len() as u32 - 1) as usize].id;
            }
            ids.push(id);
        }
        ids.into_iter().map(cards::card).collect()
    }

    /// Neow 发牌那一屏:只摆三张牌,选一张(跳过也行),完了回地图
    fn open_neow_card_reward(&mut self, cards_in: Vec<CardInstance>) {
        self.event = None;
        self.reward = Some(RewardState {
            gold: 0,
            gold_taken: true,
            extra_gold: 0,
            extra_gold_taken: true,
            cards: cards_in,
            card_taken: false,
            queued: Vec::new(),
            relic: None,
            relic_choices: Vec::new(),
            relic_taken: true,
            potions: Vec::new(),
            potion_taken: Vec::new(),
            emerald_key: false,
            index: 0,
            next: Screen::Map,
        });
        self.screen = Screen::Reward;
        self.reward_clamp();
    }

    /// 按 id 加 n 张牌(先认事件专用牌,再认卡池)
    fn add_card_id(&mut self, id: &str, n: u8) {
        for _ in 0..n.max(1) {
            let def = card_def_any(id);
            self.push_card_to_deck(CardInstance::new(def));
        }
    }

    /// 把一张造好的牌加进牌组:御守挡诅咒、黑石护符加生命上限、蛋强制升级、
    /// 陶瓷鱼给金币都在这里.奖励/商店/事件三条加牌路径共用同一条钩子
    /// (原版里这些遗物对所有"把牌加进牌组"的来源都生效).
    fn push_card_to_deck(&mut self, mut inst: CardInstance) {
        // 御守:挡掉接下来的诅咒
        if inst.kind() == CardType::Curse && self.omamori_charges > 0 {
            self.omamori_charges -= 1;
            self.say(format!("Omamori negates {}", inst.def.name));
            return;
        }
        // 黑石护符:拿到诅咒就提升生命上限
        if inst.kind() == CardType::Curse {
            let bonus = self.player.relic_fx_sum(|r| r.fx.max_hp_on_curse);
            if bonus > 0 {
                self.player.max_hp += bonus;
                self.player.hp += bonus;
            }
        }
        // 蛋:拿到对应类型的牌直接升级
        let egg = match inst.kind() {
            CardType::Attack => self.has_relic_fx(|fx| fx.egg_attack_upgrade),
            CardType::Skill => self.has_relic_fx(|fx| fx.egg_skill_upgrade),
            CardType::Power => self.has_relic_fx(|fx| fx.egg_power_upgrade),
            _ => false,
        };
        if egg && inst.upgrade() {
            self.say(format!("{} arrives upgraded", inst.def.name));
        }
        self.player.deck.push(inst);
        // 陶瓷鱼:每加一张牌给 9 金币
        let fish = self.player.relic_fx_sum(|r| r.fx.gold_on_card_add);
        if fish > 0 {
            self.gain_gold(fish);
        }
    }

    /// 便条事件的存卡文件:测试里指到临时文件,正式跑用存档目录里的 note.card
    fn note_file(&self) -> std::path::PathBuf {
        self.note_path
            .clone()
            .unwrap_or_else(crate::core::save::note_path)
    }

    /// 读存档里存的那张牌(id + 升级次数);没有、认不出来或没开持久化就按原版
    /// 给未升级的铁斩波(headless 对拍时参考实现也只认默认的这张)
    fn note_stored_card(&self) -> (&'static CardDef, u8) {
        let stored = if self.note_persist {
            crate::core::save::read_note_at(&self.note_file())
        } else {
            None
        };
        let (id, plus) = stored.unwrap_or_else(|| ("iron_wave".to_string(), 0));
        let def =
            cards::card_def(&id).unwrap_or_else(|| cards::card_def_or_panic("iron_wave"));
        (def, plus)
    }

    /// 取回便条里存的牌:升级次数照旧(灼热攻击保留等级);拿牌算一次加牌,
    /// 触发对应类型的蛋与陶瓷鱼(原版:取回算一次加牌)
    fn note_recall(&mut self) {
        let (def, plus) = self.note_stored_card();
        let mut inst = CardInstance::new(def);
        for _ in 0..plus {
            inst.upgrade();
        }
        let egg = match def.kind {
            CardType::Attack => self.has_relic_fx(|fx| fx.egg_attack_upgrade),
            CardType::Skill => self.has_relic_fx(|fx| fx.egg_skill_upgrade),
            CardType::Power => self.has_relic_fx(|fx| fx.egg_power_upgrade),
            _ => false,
        };
        if !inst.upgraded && egg && inst.upgrade() {
            self.say(format!("{} arrives upgraded", def.name));
        }
        let name = inst.label();
        self.player.deck.push(inst);
        let fish = self.player.relic_fx_sum(|r| r.fx.gold_on_card_add);
        if fish > 0 {
            self.gain_gold(fish);
        }
        self.say(format!("the note gives you {name}"));
    }

    /// 把一张牌写回便条存档,留给下一局(没开持久化就不写)
    fn note_store_card(&self, card: &CardInstance) {
        if !self.note_persist {
            return;
        }
        let plus = if card.plus > 0 {
            card.plus
        } else if card.upgraded {
            1
        } else {
            0
        };
        let _ = crate::core::save::write_note_at(&self.note_file(), card.def.id, plus);
    }

    /// 测试用:把便条存卡指到临时文件,免得读到真实存档目录
    #[cfg(test)]
    pub fn set_note_path(&mut self, path: std::path::PathBuf) {
        self.note_path = Some(path);
    }

    /// 关掉便条存卡的持久化:headless 对拍/回放用,保证不读不写玩家存档
    pub fn set_note_persist(&mut self, on: bool) {
        self.note_persist = on;
    }

    /// 随机一张本职业牌(不限稀有度时从三档里挑)
    fn random_class_card(&mut self, rarity: Option<Rarity>) -> Option<&'static CardDef> {
        let mut pool: Vec<&'static CardDef> = match rarity {
            Some(r) => cards::reward_pool(r),
            None => {
                let mut v = cards::reward_pool(Rarity::Common);
                v.extend(cards::reward_pool(Rarity::Uncommon));
                v.extend(cards::reward_pool(Rarity::Rare));
                v
            }
        };
        if pool.is_empty() {
            return None;
        }
        let idx = self.streams.floor(FloorStream::MiscRng).random(pool.len() as u32 - 1) as usize;
        Some(pool.swap_remove(idx))
    }

    /// 随机一张无色牌
    fn random_colorless_card(&mut self, rarity: Option<Rarity>) -> Option<&'static CardDef> {
        // 原版 returnColorlessCard:整池用 shuffleRng 的 java 洗牌洗一遍,再取第一张
        // 该稀有度(反编译 GameContext.cpp:1682-1687;不是"在子池里 pick")
        let mut pool = cards::colorless_pool();
        if pool.is_empty() {
            return None;
        }
        java_shuffle(
            &mut pool,
            &mut JavaRandom::new(self.streams.floor(FloorStream::ShuffleRng).random_long()),
        );
        pool.into_iter()
            .find(|c| rarity.map(|r| c.rarity == r).unwrap_or(true))
    }

    /// 移除身上的一件指定遗物
    fn remove_relic_by_id(&mut self, id: &str) {
        if let Some(idx) = self.player.relics.iter().position(|r| r.id == id) {
            let gone = self.player.relics.remove(idx);
            self.say(format!("{} is gone", gone.name));
        }
    }

    /// 变形:去掉这张牌,换成一张随机本职业牌
    fn transform_deck_card(&mut self, idx: usize) {
        if idx >= self.player.deck.len() {
            return;
        }
        let card = self.player.deck.remove(idx);
        self.pay_deck_leave_cost(&card);
        if let Some(def) = self.random_class_card(None) {
            self.player.deck.push(CardInstance::new(def));
        }
    }

    /// 直接跳到本层 Boss 房并开打(秘密传送门)
    fn jump_to_boss(&mut self) {
        let boss = self.map.boss;
        self.pos = Some(boss);
        self.path.push(boss);
        self.floor_reached = self.map.node(boss).floor;
        // 照参考实现的 goToBoss:全局层号 +1 再重种每层的流
        self.floor_num += 1;
        self.streams.reseed_floor_streams(self.floor_num);
        self.stats.bosses += 1;
        self.say(format!("floor {}: boss", self.floor_reached + 1));
        let enc = self.boss_enc;
        self.start_combat(enc, false);
    }

    pub fn event_index_set(&mut self, i: usize) {
        let n = self.event_choice_count();
        if let Some(st) = self.event.as_mut() {
            if n > 0 {
                st.index = i.min(n - 1);
            }
        }
    }

    pub fn leave_event(&mut self) {
        self.event = None;
        self.screen = Screen::Map;
    }

    // ---- 宝箱 ----

    /// 宝箱房:先掷箱子的尺寸,再掷一次同时决定"有没有金币"和"遗物档次"
    /// (参考实现的单掷怪癖:金币与档次共用一个掷点)
    fn open_treasure(&mut self) {
        let size_roll = self.streams.run(RunStream::TreasureRng).random(99);
        let size = if size_roll < 50 {
            ChestSize::Small
        } else if size_roll < 83 {
            ChestSize::Medium
        } else {
            ChestSize::Large
        };
        let roll = self.streams.run(RunStream::TreasureRng).random(99);
        let (gold_chance, common_below, uncommon_below) = match size {
            ChestSize::Small => (50, 75, 100),
            ChestSize::Medium => (35, 35, 85),
            ChestSize::Large => (50, 0, 75),
        };
        let tier = if roll < common_below {
            RelicTier::Common
        } else if roll < uncommon_below {
            RelicTier::Uncommon
        } else {
            RelicTier::Rare
        };
        // 饥肠辘辘之脸:接下来这一个非 Boss 宝箱是空的
        let empty = self.chest_empty_left > 0;
        if empty {
            self.chest_empty_left -= 1;
        }
        self.chest = Some(Chest {
            size,
            gold_present: roll < gold_chance,
            tier,
            empty,
            opened: false,
        });
        // 遗物身份进房间时就看得见(参考实现的 peekRelicFromPool)
        self.treasure = if empty {
            self.say("the chest is empty");
            None
        } else {
            Some(self.peek_relic_of_tier(tier))
        };
        self.screen = Screen::Treasure;
    }

    /// 开箱:有金币就先掷金币数,再把遗物从池子里取走
    pub fn take_treasure(&mut self) {
        self.open_chest(false);
    }

    /// 拿蓝钥匙:同样先给金币,但遗物作废(照参考实现的 takeSapphireKey)
    pub fn take_sapphire_key(&mut self) {
        self.open_chest(true);
    }

    /// 这个宝箱房里能不能拿蓝钥匙(箱子还没开、还没有蓝钥匙才行)
    pub fn chest_sapphire_available(&self) -> bool {
        self.chest.is_some_and(|c| !c.opened) && !self.keys.sapphire
    }

    fn open_chest(&mut self, take_sapphire: bool) {
        let Some(chest) = self.chest else {
            self.screen = Screen::Map;
            return;
        };
        // 已经开过了:开箱时插进了选牌界面(瓶装类遗物),这一步是"前进"
        if chest.opened {
            self.chest = None;
            self.treasure = None;
            self.screen = Screen::Map;
            return;
        }
        if take_sapphire && self.keys.sapphire {
            // 已经有一把了:什么都不做
            return;
        }
        if let Some(c) = self.chest.as_mut() {
            c.opened = true;
        }
        if chest.gold_present {
            let base = match chest.size {
                ChestSize::Small => 25.0,
                ChestSize::Medium => 50.0,
                ChestSize::Large => 75.0,
            };
            let gold = self
                .streams
                .run(RunStream::TreasureRng)
                .random_float_range(base * 0.9, base * 1.1)
                .round() as i32;
            self.gain_gold(gold);
            self.say(format!("the chest holds ${gold}"));
        }
        if take_sapphire {
            // 遗物身份照参考实现先定下来(从池子里取走),拿钥匙就等于作废它
            if !chest.empty {
                let _ = self.take_relic_of_tier(chest.tier);
            }
            self.keys.sapphire = true;
            self.treasure = None;
            self.say("you take the Sapphire Key");
            self.finish_chest();
            return;
        }
        if !chest.empty {
            let def = self.take_relic_of_tier(chest.tier);
            self.gain_relic(def);
            self.say(format!("you found {}", def.name));
        } else {
            self.say("the chest is empty");
        }
        // 诅咒钥匙:非 Boss 宝箱里附带一张诅咒
        if self.has_relic_fx(|fx| fx.curse_on_chest) {
            self.add_random_curse();
        }
        // 俄罗斯套娃:还带次数就再给一件.额外那件的档次固定 75% 普通 / 25% 罕见,
        // 与箱子大小无关(参考实现 getMatryoshkaRelicTier);饥肠辘辘之脸吃空的箱子也照给
        if self.chest_extra_left > 0 {
            self.chest_extra_left -= 1;
            let tier = if self
                .streams
                .run(RunStream::RelicRng)
                .random_bool_chance(0.75)
            {
                RelicTier::Common
            } else {
                RelicTier::Uncommon
            };
            let second = self.take_relic_of_tier(tier);
            self.gain_relic(second);
            self.say(format!("the chest also holds {}", second.name));
        }
        self.finish_chest();
    }

    /// 开完箱收尾:没有插进来选牌界面(瓶装类遗物)就直接回地图;
    /// 插了就把箱子留着(已标记 opened),选完牌回来再点一次是"前进".
    fn finish_chest(&mut self) {
        self.treasure = None;
        if self.screen != Screen::Pick {
            self.chest = None;
            self.screen = Screen::Map;
        }
    }

    /// 随机一张诅咒进牌组(诅咒钥匙;走 cardRng,与事件里的诅咒一致)
    fn add_random_curse(&mut self) {
        let pool = cards::curses();
        if pool.is_empty() {
            return;
        }
        let idx = self.streams.run(RunStream::CardRng).random(pool.len() as u32 - 1) as usize;
        let id = pool[idx].id;
        self.add_card_id(id, 1);
        self.say(format!("a curse is added: {id}"));
    }

    // ---- 选牌 ----

    fn open_picker(
        &mut self,
        purpose: PickPurpose,
        back: Screen,
        cost_gold: i32,
        shop_slot: Option<usize>,
    ) {
        self.open_picker_n(purpose, back, cost_gold, shop_slot, 1);
    }

    fn open_picker_n(
        &mut self,
        purpose: PickPurpose,
        back: Screen,
        cost_gold: i32,
        shop_slot: Option<usize>,
        remaining: u8,
    ) {
        self.picker = Some(Picker {
            purpose,
            back,
            index: 0,
            cost_gold,
            shop_slot,
            remaining: remaining.max(1),
            bottle_kind: None,
            store_note: false,
        });
        self.screen = Screen::Pick;
        self.picker_clamp();
    }

    /// 当前可选牌的 deck 下标
    pub fn picker_candidates(&self) -> Vec<usize> {
        let Some(p) = &self.picker else {
            return Vec::new();
        };
        match p.purpose {
            PickPurpose::Upgrade => self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect(),
            PickPurpose::Remove | PickPurpose::Transform => {
                // 不能把牌组删空;带"不可移除"标记的牌(升天者的诅咒等)不进候选
                if self.player.deck.len() <= 1 {
                    Vec::new()
                } else {
                    self.player
                        .deck
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| !c.def.unremovable)
                        .map(|(i, _)| i)
                        .collect()
                }
            }
            // 祭品:能烧掉的牌(不可移除的不行,参考实现 removableIndices 还排掉瓶装的)
            PickPurpose::Offer => self.removable_cards(),
            PickPurpose::Duplicate => (0..self.player.deck.len()).collect(),
            // 瓶装:只列对应类型、还没被封进瓶子的牌
            PickPurpose::Bottle => {
                let want = p.bottle_kind;
                self.player
                    .deck
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.bottled && want.map_or(true, |k| c.kind() == k))
                    .map(|(i, _)| i)
                    .collect()
            }
        }
    }

    pub fn picker_clamp(&mut self) {
        let n = self.picker_candidates().len();
        if let Some(p) = self.picker.as_mut() {
            if n == 0 {
                p.index = 0;
            } else if p.index >= n {
                p.index = n - 1;
            }
        }
    }

    pub fn picker_confirm(&mut self) -> Result<String, String> {
        let Some(p) = self.picker.as_ref() else {
            return Err("no picker open".to_string());
        };
        let cands = self.picker_candidates();
        let Some(deck_idx) = cands.get(p.index).copied() else {
            return Err("nothing selected".to_string());
        };
        let purpose = p.purpose;
        let back = p.back;
        let cost = p.cost_gold;
        let slot = p.shop_slot;
        let remaining = p.remaining;
        let store_note = p.store_note;
        if cost > 0 && self.player.gold < cost {
            return Err("not enough gold".to_string());
        }
        let msg = match purpose {
            PickPurpose::Upgrade => {
                self.player.deck[deck_idx].upgrade();
                let label = self.player.deck[deck_idx].label();
                format!("upgraded to {label}")
            }
            PickPurpose::Remove => {
                let card = self.player.deck.remove(deck_idx);
                // 便条事件:这张牌要留给下一局("A Note For Yourself")
                if store_note {
                    self.note_store_card(&card);
                }
                // 寄生这类"被抽出牌组要付代价"的牌
                self.pay_deck_leave_cost(&card);
                format!("{} removed from your deck", card.label())
            }
            PickPurpose::Transform => {
                let before = self.player.deck[deck_idx].label();
                self.transform_deck_card(deck_idx);
                let after = self.player.deck.last().map(|c| c.label()).unwrap_or_default();
                format!("{before} transformed into {after}")
            }
            PickPurpose::Duplicate => {
                let copy = self.player.deck[deck_idx].clone();
                let label = copy.label();
                self.push_card_to_deck(copy);
                format!("{label} duplicated")
            }
            PickPurpose::Bottle => {
                // 瓶装:牌留在牌组里,只做个记号;战斗开局它会直接进手
                self.player.deck[deck_idx].bottled = true;
                let label = self.player.deck[deck_idx].label();
                format!("{label} is bottled")
            }
            PickPurpose::Offer => {
                let card = self.player.deck.remove(deck_idx);
                // 寄生这类"被抽出牌组要付代价"的牌
                self.pay_deck_leave_cost(&card);
                self.offer_card_bonus(&card)
            }
        };
        if cost > 0 {
            self.spend_gold(cost);
            if let (Some(slot), Some(shop)) = (slot, self.shop.as_mut()) {
                shop.sold[slot] = true;
                shop.removes += 1;
            }
            // 本局买过几次删牌:下一家店的删牌价按这个涨
            self.removes_purchased += 1;
        }
        self.picker = None;
        self.say(msg.clone());
        self.screen = back;
        // 祭品结算完事件就结束了(参考实现 onResume 里的 endEvent)
        if purpose == PickPurpose::Offer {
            if let Some(st) = self.event.as_mut() {
                st.result = Some(msg.clone());
            }
        }
        // Neow 的"移除两张/变形两张":选完一张再开一次(免费的才这样重复)
        if remaining > 1 && cost == 0 && slot.is_none() {
            let left = remaining - 1;
            if !self.picker_candidates_is_empty(purpose) {
                self.open_picker_n(purpose, back, cost, slot, left);
            }
        }
        Ok(msg)
    }

    /// 篝火精灵的赏赐:按烧掉的牌的稀有度给(参考实现 bonfireSpirits.onResume).
    /// 诅咒给"灵便便",基础牌什么也不给,普通/特殊回 5 点,罕见回 10 点,
    /// 稀有给 10 点上限并回满.
    fn offer_card_bonus(&mut self, card: &CardInstance) -> String {
        let name = card.label();
        if card.kind() == CardType::Curse {
            let def = relics::relic_def_or_panic("spirit_poop");
            self.gain_relic(def);
            return format!("{name} is devoured; the spirits leave Spirit Poop");
        }
        match card.rarity() {
            Rarity::Basic => format!("{name} is devoured; the spirits want more"),
            Rarity::Common | Rarity::Special => {
                self.heal(5);
                format!("{name} is devoured; you heal 5 HP")
            }
            Rarity::Uncommon => {
                self.heal(10);
                format!("{name} is devoured; you heal 10 HP")
            }
            Rarity::Rare => {
                // 加上限会顺带回等量的血,再照参考实现 healToFull 回满
                self.player.max_hp += 10;
                self.heal(10);
                let full = self.player.max_hp;
                self.heal(full);
                format!("{name} is devoured; you gain 10 max HP and are healed to full")
            }
        }
    }

    /// 打开选牌界面之前先看一眼:这个用途下一张可选牌都没有,就别开空的界面
    /// (转盘抽到"删一张"但牌组全是不可移除时,参考实现也是直接收尾).
    fn picker_would_be_empty(&self, purpose: PickPurpose) -> bool {
        match purpose {
            PickPurpose::Upgrade => !self.player.deck.iter().any(|c| c.can_upgrade()),
            PickPurpose::Remove | PickPurpose::Transform | PickPurpose::Offer => {
                self.removable_cards().is_empty()
            }
            PickPurpose::Duplicate => self.player.deck.is_empty(),
            PickPurpose::Bottle => false,
        }
    }

    /// 这个用途下还有没有可选的牌(重复开选牌界面前先看一眼)
    fn picker_candidates_is_empty(&self, purpose: PickPurpose) -> bool {
        match purpose {
            PickPurpose::Upgrade => !self.player.deck.iter().any(|c| c.can_upgrade()),
            PickPurpose::Remove | PickPurpose::Transform => {
                self.player.deck.len() <= 1
                    || !self.player.deck.iter().any(|c| !c.def.unremovable)
            }
            PickPurpose::Duplicate => self.player.deck.is_empty(),
            // 瓶装与祭品只选一次,不走"再开一次"的分支
            PickPurpose::Bottle | PickPurpose::Offer => true,
        }
    }

    pub fn picker_cancel(&mut self) {
        if let Some(p) = self.picker.take() {
            self.screen = p.back;
        }
    }

    /// 一张牌离开牌组时要付的代价(寄生:3 点最大生命).
    /// 删牌、变形这类"把原牌从牌组拿走"的入口都要调一次;复制不用调.
    pub fn pay_deck_leave_cost(&mut self, card: &CardInstance) {
        let toll: i32 = card
            .effects()
            .iter()
            .map(|e| match *e {
                CardEffect::LoseMaxHpOnRemoved { n } => n,
                _ => 0,
            })
            .sum();
        if toll > 0 {
            self.player.max_hp = (self.player.max_hp - toll).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
    }

    // ---- 药水 ----

    pub fn add_potion(&mut self, def: &'static PotionDef) -> bool {
        // 苏族之魂:再也拿不到药水
        if self.has_relic_fx(|fx| fx.no_potions) {
            return false;
        }
        for slot in self.player.potions.iter_mut() {
            if slot.is_none() {
                *slot = Some(def);
                return true;
            }
        }
        false
    }

    pub fn quaff_potion(&mut self, slot: usize, target: Option<usize>) -> Result<String, String> {
        let Some(Some(def)) = self.player.potions.get(slot).copied() else {
            return Err("no potion in that slot".to_string());
        };
        // 仙女在瓶中这类被动药水喝不掉:它只在被打死的那一刻自动触发,
        // 允许主动喝只会白白把死亡保护丢掉(反编译里没有这条使用路径).
        if !def.drinkable() {
            return Err(format!("{} only triggers when you would die", def.name));
        }
        let in_combat = self.screen == Screen::Combat;
        if !in_combat && (!def.out_of_combat || def.target.needs_enemy()) {
            return Err(format!("{} only works in combat", def.name));
        }
        // 烟雾弹:从非 Boss 战斗里脱身,不给奖励
        if matches!(def.fx, PotionFx::Escape) {
            if !in_combat {
                return Err(format!("{} only works in combat", def.name));
            }
            if self.combat.as_ref().map(|c| c.kind) == Some(EnemyKind::Boss) {
                return Err("cannot escape a boss fight".to_string());
            }
            if let Some(c) = self.combat.as_ref() {
                self.streams = c.streams.clone();
                // 脱身也算这场打完:跨战斗的遗物计数器写回一局
                self.relic_counters = c.rs.run_counters();
            }
            self.absorb_combat_log();
            self.combat = None;
            self.player.potions[slot] = None;
            self.stats.potions_used += 1;
            self.screen = Screen::Map;
            self.say(format!("you slip away with {}", def.name));
            return Ok(format!("used {}", def.name));
        }
        if in_combat {
            if let Some(c) = self.combat.as_mut() {
                c.use_potion(def, target);
            }
            self.stats.potions_used += 1;
        } else {
            // 神圣树皮:地图上喝的药水数值同样翻倍
            let fx = if self.player.relic_fx_sum(|r| r.fx.potion_potency_pct) > 0 {
                def.fx.doubled()
            } else {
                def.fx
            };
            match fx {
                PotionFx::HealPercent { pct } => {
                    let amount = self.player.max_hp * pct / 100;
                    self.heal(amount);
                }
                PotionFx::MaxHp { n } => {
                    self.player.max_hp += n;
                    self.player.hp += n;
                }
                _ => {}
            }
            self.stats.potions_used += 1;
        }
        self.player.potions[slot] = None;
        // 熵能酿剂:空槽用随机药水填满(走 potionRng,和奖励/商店同一套掷点)
        if matches!(def.fx, PotionFx::FillPotionSlots) {
            let color = potions::class_color(self.character);
            for s in 0..self.player.potions.len() {
                if self.player.potions[s].is_none() {
                    if let Some(p) = potions::random_potion(
                        self.streams.run(RunStream::PotionRng),
                        color,
                    ) {
                        self.player.potions[s] = Some(p);
                    }
                }
            }
        }
        // 仙女在瓶中:战斗里挂着当保命符
        let fairy = self.has_fairy();
        if let Some(c) = self.combat.as_mut() {
            c.fairy_save = fairy;
        }
        if self.screen == Screen::Combat {
            self.sync_combat();
        }
        Ok(format!("used {}", def.name))
    }

    /// 身上挂着仙女在瓶中
    fn has_fairy(&self) -> bool {
        self.player
            .potions
            .iter()
            .flatten()
            .any(|p| p.id == "fairy_potion")
    }

    pub fn toss_potion(&mut self, slot: usize) -> Result<String, String> {
        match self.player.potions.get_mut(slot) {
            Some(slot_ref) if slot_ref.is_some() => {
                let def = slot_ref.take().unwrap();
                Ok(format!("discarded {}", def.name))
            }
            _ => Err("no potion in that slot".to_string()),
        }
    }

    // ---- 资源 ----

    pub fn gain_gold(&mut self, n: i32) {
        // 灵体外质:再也拿不到金币
        if n > 0 && self.has_relic_fx(|fx| fx.no_gold) {
            return;
        }
        self.player.gold += n;
        // 血腥雕像:每次拿到金币就回血
        if n > 0 {
            let idol = self.player.relic_fx_sum(|r| r.fx.heal_on_gold_gain);
            if idol > 0 {
                self.heal(idol);
            }
        }
    }

    pub fn spend_gold(&mut self, n: i32) {
        // 银行家之躯:在商店花过钱就不再每层给钱。买删牌走的是选牌屏
        // (screen 已切到 Pick),所以按"人还在商店里"(self.shop)判
        if n > 0 && (self.screen == Screen::Shop || self.shop.is_some()) {
            self.maw_bank_spent = true;
        }
        self.player.gold = (self.player.gold - n).max(0);
    }

    /// 回血,返回实际回复量
    pub fn heal(&mut self, n: i32) -> i32 {
        // 花开彼岸:再也回不了血
        if n > 0 && self.has_relic_fx(|fx| fx.no_heal) {
            return 0;
        }
        if n <= 0 {
            return 0;
        }
        let before = self.player.hp;
        self.player.hp = (self.player.hp + n).min(self.player.max_hp);
        self.player.hp - before
    }

    /// 直接掉血;死了切到死亡界面
    pub fn damage(&mut self, n: i32) {
        if n <= 0 {
            return;
        }
        self.player.hp -= n;
        if self.player.hp <= 0 {
            self.player.hp = 0;
            self.screen = Screen::Death;
        }
    }

    /// 拾取遗物时的一次性效果(参考实现的 onEquip)
    fn apply_relic_pickup(&mut self, def: &'static RelicDef) {
        // 小房子:顺序与掷点和参考实现 tinyHousePickup 一致 —— 先随机升级一张,
        // 再抬 5 点上限、+50 金、从药水池等概率摸一瓶(miscRng),最后排一组卡牌三选一
        if def.fx.pickup_tiny_house {
            // 小房子:顺序与掷点和参考实现 tinyHousePickup 一致 —— 先随机升级一张,
            // 再抬 5 点上限,然后把 50 金与一瓶药水塞进当前奖励屏,最后排一组卡牌三选一.
            // 原版走的是 addGoldToRewards / addPotionToRewards:这两样是奖励屏上的条目,
            // 不是当场结算(拿了遗物就不管奖励屏的话,这 50 金与原版一样会留在屏上不进口袋).
            self.upgrade_random_any(1);
            self.player.max_hp += 5;
            self.player.hp += 5;
            let color = potions::class_color(self.character);
            let pool = potions::pool(color);
            let potion = if pool.is_empty() {
                None
            } else {
                let i = self
                    .streams
                    .floor(FloorStream::MiscRng)
                    .random(pool.len() as u32 - 1) as usize;
                Some(pool[i])
            };
            let mut gold_left = true;
            if let Some(r) = self.reward.as_mut() {
                if r.gold_taken {
                    r.gold = 50;
                    r.gold_taken = false;
                } else {
                    // 第一条金币还没拿:原版会显示成两条,这里也留成独立一条
                    r.extra_gold = 50;
                    r.extra_gold_taken = false;
                }
                if let Some(p) = potion {
                    r.potions.push(p);
                    r.potion_taken.push(false);
                }
                gold_left = false;
            }
            if gold_left {
                // 不在奖励屏上(理论上不会发生):退回当场结算
                self.gain_gold(50);
                if let Some(p) = potion {
                    self.add_potion(p);
                }
            }
            self.add_card_choice(1);
            return;
        }
        let fx = def.fx;
        // 黑血之类的"替换起始遗物"
        if fx.removes_starter_relic {
            if let Some(starter) = roster::find(self.character).map(|c| c.relic) {
                if starter != def.id {
                    self.remove_relic_by_id(starter);
                }
            }
        }
        if fx.max_hp != 0 {
            self.player.max_hp += fx.max_hp;
            if fx.max_hp > 0 {
                // 原版 increaseMaxHp 里的 heal,花开彼岸下不回血
                self.heal(fx.max_hp);
            }
        }
        if fx.heal > 0 {
            self.heal(fx.heal);
        }
        if fx.full_heal {
            // 花开彼岸:回满也归零(原版走 heal())
            self.heal(self.player.max_hp);
        }
        if fx.gold > 0 {
            self.gain_gold(fx.gold);
        }
        for _ in 0..fx.potion_slots.max(0) {
            self.player.potions.push(None);
        }
        if fx.upgrade_random_attacks > 0 {
            self.upgrade_random_deck(CardType::Attack, fx.upgrade_random_attacks);
        }
        if fx.upgrade_random_skills > 0 {
            self.upgrade_random_deck(CardType::Skill, fx.upgrade_random_skills);
        }
        if fx.upgrade_random_cards > 0 {
            self.upgrade_random_any(fx.upgrade_random_cards);
        }
        if fx.transform_strikes_defends {
            self.transform_starter_strikes_and_defends();
        }
        // 一次性/带次数的效果在这里落成计数器
        if fx.extra_chest_relic_charges > 0 {
            self.chest_extra_left = fx.extra_chest_relic_charges;
        }
        if fx.chest_empty_charges > 0 {
            self.chest_empty_left += fx.chest_empty_charges;
        }
        if fx.neow_lament_combats > 0 {
            self.neow_lament = fx.neow_lament_combats as u8;
        }
        if fx.curse_negate > 0 {
            self.omamori_charges = fx.curse_negate;
        }
        if fx.map_wing_charges > 0 {
            self.wing_boots_left = fx.map_wing_charges;
        }
    }

    /// 随机升级牌组里 n 张指定类型的牌(参考实现走 miscRng)
    fn upgrade_random_deck(&mut self, want: CardType, n: i32) {
        for _ in 0..n {
            let idxs: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.kind() == want && c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            let Some(i) = self.random_index(&idxs) else {
                return;
            };
            self.player.deck[i].upgrade();
        }
    }

    /// 随机升级牌组里 n 张能升级的牌(小房子:把可升级下标用 miscRng 掷出的种子
    /// java 洗牌,取第一张,和参考实现 Deck::getUpgradeableCardIdxs + shuffle 一致)
    fn upgrade_random_any(&mut self, n: i32) {
        for _ in 0..n {
            let mut idxs: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| c.can_upgrade())
                .map(|(i, _)| i)
                .collect();
            if idxs.is_empty() {
                return;
            }
            let seed = self.streams.floor(FloorStream::MiscRng).random_long();
            java_shuffle(&mut idxs, &mut JavaRandom::new(seed));
            self.player.deck[idxs[0]].upgrade();
        }
    }

    /// 从下标表里随机挑一个(miscRng)
    fn random_index(&mut self, idxs: &[usize]) -> Option<usize> {
        if idxs.is_empty() {
            return None;
        }
        let pick = self.streams.floor(FloorStream::MiscRng).random(idxs.len() as u32 - 1) as usize;
        Some(idxs[pick])
    }

    /// 潘多拉魔盒:把牌组里的打击与防御全部变形
    fn transform_starter_strikes_and_defends(&mut self) {
        let mut idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.def.id, "strike" | "defend"))
            .map(|(i, _)| i)
            .collect();
        // 从后往前删,下标才不会错位
        idxs.sort_unstable();
        for i in idxs.into_iter().rev() {
            self.transform_deck_card(i);
        }
    }

    pub fn gain_relic(&mut self, def: &'static RelicDef) {
        self.apply_relic_pickup(def);
        self.player.relics.push(def);
        for pool in self.relic_pools.all_mut() {
            pool.retain(|r| r.id != def.id);
        }
        self.say(format!("relic: {}", def.name));
        self.start_relic_pickup_pickers(def);
    }

    /// 需要选牌/加奖励的拾取效果:拿完遗物接着开选牌界面
    fn start_relic_pickup_pickers(&mut self, def: &'static RelicDef) {
        let fx = def.fx;
        let back = self.screen;
        // 瓶装类:挑一张对应类型的牌封进瓶子(牌留在牌组里)
        let bottle_kind = if fx.bottle_attack {
            Some(CardType::Attack)
        } else if fx.bottle_skill {
            Some(CardType::Skill)
        } else if fx.bottle_power {
            Some(CardType::Power)
        } else {
            None
        };
        if let Some(kind) = bottle_kind {
            let has = self.player.deck.iter().any(|c| c.kind() == kind);
            if has {
                self.open_picker(PickPurpose::Bottle, back, 0, None);
                if let Some(p) = self.picker.as_mut() {
                    p.bottle_kind = Some(kind);
                }
            }
        }
        // 浑天仪:连开五次卡牌三选一
        if fx.pickup_card_picks > 0 {
            self.add_card_choice(fx.pickup_card_picks as usize);
        }
        if fx.remove_cards > 0 {
            self.open_picker_n(PickPurpose::Remove, back, 0, None, fx.remove_cards as u8);
        } else if fx.transform_cards > 0 {
            self.open_picker_n(PickPurpose::Transform, back, 0, None, fx.transform_cards as u8);
        } else if fx.duplicate_cards > 0 {
            self.open_picker_n(PickPurpose::Duplicate, back, 0, None, fx.duplicate_cards as u8);
        }
        if fx.add_relics > 0 {
            // 叫醒铃铛:依次从普通/罕见/稀有的池子里各拿一件
            for tier in [RelicTier::Common, RelicTier::Uncommon, RelicTier::Rare] {
                for _ in 0..(fx.add_relics / 3).max(0) {
                    let extra = self.take_relic_of_tier(tier);
                    self.gain_relic_raw(extra);
                }
            }
            let rest = fx.add_relics % 3;
            for _ in 0..rest {
                let extra = self.take_relic_of_tier(RelicTier::Common);
                self.gain_relic_raw(extra);
            }
        }
        if fx.add_cards > 0 {
            self.add_card_choice(fx.add_cards as usize);
        }
        if let Some(id) = fx.add_card {
            // 原版走 masterDeck.addToTop,不经过御守/黑石护符/蛋那条加牌钩子
            self.player
                .deck
                .push(CardInstance::new(card_def_any(id)));
        }
        if fx.add_curse {
            self.add_random_curse();
        }
        for _ in 0..fx.add_potions.max(0) {
            let color = potions::class_color(self.character);
            if let Some(p) = potions::random_potion(self.streams.run(RunStream::PotionRng), color) {
                self.add_potion(p);
            }
        }
    }

    /// 事件自己开一个奖励屏(参考实现 openRewards):条目由调用方填好,
    /// 事件本身在这一屏开始时就结束了.
    fn open_event_reward(&mut self, r: RewardState) {
        self.event = None;
        self.reward = Some(r);
        self.screen = Screen::Reward;
        self.reward_clamp();
    }

    /// 空奖励屏:只有调用方填进去的条目会显示(金币/遗物/卡牌都按"已拿走"处理)
    fn open_potion_reward(&mut self, n: u8) {
        let mut r = RewardState::empty(Screen::Map);
        for _ in 0..n {
            let color = potions::class_color(self.character);
            if let Some(p) = potions::random_potion(self.streams.run(RunStream::PotionRng), color) {
                r.potions.push(p);
                r.potion_taken.push(false);
            }
        }
        self.open_event_reward(r);
    }

    /// dead_adventurer 的伏击打完后发的事件奖励(参考实现 onCombatVictory):
    /// 金币 = miscRng 摇 25-35,奖池里每份没领到的金币奖再加 30;奖池里还剩遗物
    /// 就摇一件;再照 eventCombatRewards 的顺序掷一次药水、发一组精英牌.
    fn open_dead_adventurer_rewards(&mut self, d: DeadAdventurerData) {
        let remaining = &d.rewards[d.phase.min(3) as usize..];
        let parcels = remaining.iter().filter(|r| **r == "GOLD").count() as i32;
        let (lo, hi) = crate::core::events::DEAD_ADVENTURER_AMBUSH_GOLD;
        let gold = self.streams.floor(FloorStream::MiscRng).random_range(lo, hi)
            + crate::core::events::DEAD_ADVENTURER_GOLD * parcels;
        let relic = if remaining.contains(&"RELIC") {
            self.take_relic_of_any()
        } else {
            None
        };
        let potion = self.roll_potion_reward(1 + usize::from(relic.is_some()));
        let cards = self.create_card_reward(EnemyKind::Elite);
        let mut r = RewardState::empty(Screen::Map);
        r.gold = gold;
        r.gold_taken = false;
        r.relic = relic;
        r.relic_taken = false;
        if let Some(p) = potion {
            r.potions.push(p);
            r.potion_taken.push(false);
        }
        r.cards = cards;
        r.card_taken = false;
        self.open_event_reward(r);
    }

    /// 排 n 组卡牌三选一(小房子的"获得一张牌"/浑天仪的五次/梦中情网的一次).
    /// 第一组直接放进当前(或新建的)奖励屏,其余排队,拿完一组再顶上来一组.
    fn add_card_choice(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let mut groups: Vec<Vec<CardInstance>> = Vec::new();
        for _ in 0..n {
            let cards = self.create_card_reward(EnemyKind::Normal);
            if !cards.is_empty() {
                groups.push(cards);
            }
        }
        if groups.is_empty() {
            return;
        }
        let first = groups.remove(0);
        if let Some(r) = self.reward.as_mut() {
            if r.card_taken || r.cards.is_empty() {
                // 当前这一组已经拿完(或本来就没有):新的一组直接顶上来
                r.cards = first;
                r.card_taken = false;
                r.queued.extend(groups);
            } else {
                // 当前这一组还没拿(小房子的"获得一张牌"):另起一组排在后面,
                // 拿完当前这组再顶上来,不能并进同一组
                r.queued.push(first);
                r.queued.extend(groups);
            }
        } else {
            let back = self.screen;
            self.reward = Some(RewardState {
                gold: 0,
                gold_taken: true,
                extra_gold: 0,
                extra_gold_taken: true,
                cards: first,
                card_taken: false,
                queued: groups,
                relic: None,
                relic_choices: Vec::new(),
                relic_taken: true,
                potions: Vec::new(),
                potion_taken: Vec::new(),
                emerald_key: false,
                index: 0,
                next: back,
            });
            self.screen = Screen::Reward;
            self.reward_clamp();
        }
    }

    /// 当前这一组卡牌奖励拿完(或跳过)之后,把排队的一组顶上来
    fn next_card_group(&mut self) {
        let next = {
            let Some(r) = self.reward.as_mut() else {
                return;
            };
            if r.queued.is_empty() {
                return;
            }
            r.queued.remove(0)
        };
        if let Some(r) = self.reward.as_mut() {
            r.cards = next;
            r.card_taken = false;
        }
        self.reward_clamp();
    }

    /// 拾取结算用:只入库,不再递归触发拾取效果
    fn gain_relic_raw(&mut self, def: &'static RelicDef) {
        self.player.relics.push(def);
        for pool in self.relic_pools.all_mut() {
            pool.retain(|r| r.id != def.id);
        }
        self.say(format!("relic: {}", def.name));
    }

    /// 有没有任何一件遗物的效果满足这个条件
    fn has_relic_fx(&self, f: impl Fn(&RelicFx) -> bool) -> bool {
        self.player.relics.iter().any(|r| f(&r.fx))
    }

    /// 身上有没有这件遗物(按 id)
    pub fn has_relic(&self, id: &str) -> bool {
        self.player.relics.iter().any(|r| r.id == id)
    }

    // ---- 调试命令:按名字加/删遗物与卡牌 ----

    /// 名字归一化:大小写、空格、下划线都不计较("Blood for Blood" = blood_for_blood)
    fn norm(name: &str) -> String {
        name.trim().to_lowercase().replace([' ', '-'], "_")
    }

    pub fn debug_add_relic(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        if want == "all" {
            let ids: Vec<&'static str> = relics::RELICS.iter().map(|r| r.id).collect();
            let mut n = 0;
            for id in ids {
                if !self.player.relics.iter().any(|r| r.id == id) {
                    let def = relics::relic_def_or_panic(id);
                    self.gain_relic(def);
                    n += 1;
                }
            }
            return Ok(format!("added {n} relics"));
        }
        let def = relics::RELICS
            .iter()
            .find(|r| Self::norm(r.id) == want || Self::norm(r.name) == want)
            .ok_or_else(|| format!("no relic named {name}"))?;
        let name = def.name;
        self.gain_relic(def);
        Ok(format!("added relic {name}"))
    }

    pub fn debug_remove_relic(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        let idx = self
            .player
            .relics
            .iter()
            .position(|r| Self::norm(r.id) == want || Self::norm(r.name) == want)
            .ok_or_else(|| format!("no relic named {name}"))?;
        let def = self.player.relics.remove(idx);
        if let Some(pool) = self.relic_pools.slot_mut(def.tier) {
            pool.insert(0, def);
        }
        Ok(format!("removed relic {}", def.name))
    }

    /// 加牌:战斗中加到手牌(满了就不加),平时加进牌组
    pub fn debug_add_card(&mut self, name: &str) -> Result<String, String> {
        let want = Self::norm(name);
        let def = cards::CARDS
            .iter()
            .find(|c| Self::norm(c.id) == want || Self::norm(c.name) == want)
            .ok_or_else(|| format!("no card named {name}"))?;
        let label = def.name;
        if let Some(c) = self.combat.as_mut() {
            if c.hand.len() >= crate::core::combat::HAND_LIMIT {
                return Ok(format!("hand is full, {label} not added"));
            }
            let mut inst = cards::card(def.id);
            c.fix_new_card(&mut inst);
            c.hand.push(inst);
        } else {
            self.player.deck.push(cards::card(def.id));
        }
        Ok(format!("added card {label}"))
    }

    /// 调试用:往战斗里的某个牌堆直接塞牌(:card pile draw Havoc, Havoc)
    pub fn debug_pile_cards(&mut self, pile: &str, args: &str) -> Result<String, String> {
        let Some(c) = self.combat.as_mut() else {
            return Err("not in a battle".to_string());
        };
        let mut n = 0;
        for part in args.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            let want = Self::norm(name);
            let def = cards::CARDS
                .iter()
                .find(|c| Self::norm(c.id) == want || Self::norm(c.name) == want)
                .ok_or_else(|| format!("no card named {name}"))?;
            let mut inst = cards::card(def.id);
            c.fix_new_card(&mut inst);
            match pile {
                "hand" => {
                    if c.hand.len() >= crate::core::combat::HAND_LIMIT {
                        continue;
                    }
                    c.hand.push(inst);
                }
                // 抽牌堆的顶是下标 0:塞进去的牌下一个就抽到(和以前 push 到末尾等效)
                "draw" => c.draw.insert(0, inst),
                "discard" => c.discard.push(inst),
                "exhaust" => c.exhaust.push(inst),
                other => return Err(format!("unknown pile {other}")),
            }
            n += 1;
        }
        Ok(format!("put {n} card(s) into {pile}"))
    }

    /// 删牌:名字 / all(整副) / hand(手牌选择窗口) / hand all(手牌全删)
    pub fn debug_remove_card(&mut self, what: &str) -> Result<String, String> {
        let arg = what.trim().to_lowercase();
        if arg.is_empty() {
            // 没给名字:开牌组里的选牌窗口
            return Err("pick from the deck (see the picker)".to_string());
        }
        if arg == "all" {
            let n = self.player.deck.len();
            self.player.deck.clear();
            return Ok(format!("removed {n} cards from the deck"));
        }
        if let Some(rest) = arg.strip_prefix("hand") {
            let all = rest.trim() == "all";
            if all {
                if let Some(c) = self.combat.as_mut() {
                    let n = c.hand.len();
                    c.hand.clear();
                    return Ok(format!("removed {n} cards from your hand"));
                }
                return Err("not in a battle".to_string());
            }
            if self.combat.is_none() {
                return Err("not in a battle".to_string());
            }
            return Err("pick from your hand".to_string());
        }
        let want = Self::norm(&arg);
        let before = self.player.deck.len();
        self.player.deck.retain(|c| {
            !(Self::norm(c.def.id) == want || Self::norm(c.def.name) == want)
        });
        if let Some(c) = self.combat.as_mut() {
            c.hand.retain(|x| !(Self::norm(x.def.id) == want || Self::norm(x.def.name) == want));
        }
        let removed = before - self.player.deck.len();
        Ok(format!("removed {removed} card(s)"))
    }

    /// 升级牌组里的某张牌(默认 1 次),可无限升级的会一直在牌组里升
    pub fn debug_upgrade_card(&mut self, name: &str, times: usize) -> Result<String, String> {
        let want = Self::norm(name);
        let mut done = 0;
        for card in self.player.deck.iter_mut() {
            if Self::norm(card.def.id) == want || Self::norm(card.def.name) == want {
                for _ in 0..times.max(1) {
                    if card.upgrade() {
                        done += 1;
                    }
                }
            }
        }
        if let Some(c) = self.combat.as_mut() {
            for card in c.hand.iter_mut() {
                if Self::norm(card.def.id) == want || Self::norm(card.def.name) == want {
                    for _ in 0..times.max(1) {
                        if card.upgrade() {
                            done += 1;
                        }
                    }
                }
            }
        }
        if done == 0 {
            return Err(format!("no upgradable card named {name} in your deck"));
        }
        Ok(format!("upgraded {done} time(s)"))
    }

    /// 加遗物:名字用逗号分隔,可以一次加多个("Pear, Vajra")
    pub fn debug_add_relics(&mut self, args: &str) -> Result<String, String> {
        let mut out = Vec::new();
        for part in args.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push(self.debug_add_relic(part)?);
        }
        if out.is_empty() {
            return Err("no relic name given".to_string());
        }
        Ok(out.join("; "))
    }

    /// 加牌:同样用逗号分隔
    pub fn debug_add_cards(&mut self, args: &str) -> Result<String, String> {
        let mut out = Vec::new();
        for part in args.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            out.push(self.debug_add_card(part)?);
        }
        if out.is_empty() {
            return Err("no card name given".to_string());
        }
        Ok(out.join("; "))
    }

    /// 调试用:开牌组里的"删一张"窗口
    pub fn debug_open_remove_picker(&mut self) {
        self.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
    }

    /// 调试用:开"从手牌删一张"的选择(战斗里)
    pub fn debug_begin_hand_remove(&mut self) -> Result<(), String> {
        match self.combat.as_mut() {
            Some(c) => {
                c.debug_begin_hand_remove();
                Ok(())
            }
            None => Err("not in a battle".to_string()),
        }
    }

    /// 调试用:直接进某个房间。不改地图、不动位置,退出后照旧回到原来的地图。
    /// what: shop / event [事件id] / battle(随机 boss|elite|enemy) / boss / elite / enemy
    pub fn debug_room(&mut self, what: &str) -> Result<String, String> {
        let (kind, arg) = match what.split_once(' ') {
            Some((k, a)) => (k, Some(a.trim())),
            None => (what, None),
        };
        match kind {
            "shop" => {
                self.open_shop();
                Ok(format!("debug room: {}", self.last_encounter_or("shop")))
            }
            "event" => match arg {
                Some(id) if !id.is_empty() => self.debug_open_event(id),
                _ => {
                    self.open_event();
                    Ok(format!("debug room: {}", self.last_encounter_or("event")))
                }
            },
            "battle" => {
                let kind = match self.streams.floor(FloorStream::MiscRng).below(3) {
                    0 => EnemyKind::Boss,
                    1 => EnemyKind::Elite,
                    _ => EnemyKind::Normal,
                };
                let enc = self.pick_encounter(kind);
                let id = enc.id;
                self.start_combat(enc, false);
                Ok(format!("debug room: battle {id}"))
            }
            // 燃烧精英(调试用:直接跳到地图上那个节点开打,增益照常生效)
            "burning" => {
                let Some(idx) = self.map.burning_node() else {
                    return Err("这张地图没有燃烧精英".to_string());
                };
                self.pos = Some(idx);
                self.path.push(idx);
                self.floor_reached = self.map.node(idx).floor;
                self.floor_num = self.floor_reached as u32 + 1;
                self.streams.reseed_floor_streams(self.floor_num);
                self.stats.elites += 1;
                let enc = self.pick_encounter(EnemyKind::Elite);
                let id = enc.id;
                let buff = self.map.burning_buff;
                self.start_combat(enc, true);
                Ok(format!("debug room: burning elite {id} (buff {buff})"))
            }
            // 宝箱房(调试用:不进地图也能看箱子)
            "treasure" => {
                self.open_treasure();
                Ok(format!("debug room: treasure {:?}", self.treasure.map(|d| d.name)))
            }
            // 营火(调试用:直接看营火选项,含回忆拿红钥匙)
            "rest" => {
                self.enter_rest();
                Ok("debug room: rest".to_string())
            }
            "boss" | "elite" | "enemy" => {
                // 点名一只怪(:room enemy jaw_worm)或一场遭遇(:room enemy slime_boss)
                if let Some(name) = arg {
                    let wanted = name.trim().to_lowercase();
                    let by_id = |id: &str| id == wanted;
                    let enc = enemies::all_encounters()
                        .find(|e| by_id(e.id))
                        .or_else(|| {
                            enemies::ENEMIES
                                .iter()
                                .find(|e| by_id(e.id) || e.name.to_lowercase() == wanted)
                                .and_then(|e| enemies::encounter_with_enemy(e.id))
                        });
                    match enc {
                        Some(enc) => {
                            let id = enc.id;
                            self.start_combat(enc, false);
                            return Ok(format!("debug room: battle {id}"));
                        }
                        None => {
                            return Err(format!("unknown enemy or encounter '{name}'"));
                        }
                    }
                }
                let kind = match kind {
                    "boss" => EnemyKind::Boss,
                    "elite" => EnemyKind::Elite,
                    _ => EnemyKind::Normal,
                };
                let enc = self.pick_encounter(kind);
                let id = enc.id;
                self.start_combat(enc, false);
                Ok(format!("debug room: battle {id}"))
            }
            other => Err(format!(
                "unknown room '{other}', try: shop, event, battle, boss, elite, enemy, treasure, burning, rest"
            )),
        }
    }

    /// 调试用:直接打开某个事件(翻牌事件会把棋盘铺好)
    pub fn debug_open_event(&mut self, id: &str) -> Result<String, String> {
        let def = crate::core::events::EVENTS
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("unknown event '{id}'"))?;
        self.open_event_def(def);
        Ok(format!("debug event: {}", def.id))
    }

    /// 调试用的名字:拿不到就退回默认
    fn last_encounter_or(&self, fallback: &str) -> String {
        if self.last_encounter.is_empty() {
            fallback.to_string()
        } else {
            self.last_encounter.to_string()
        }
    }

    /// 按稀有度取遗物(接口给 events.rs 留着;换算成档次再走池子)
    fn take_relic_of(&mut self, rarity: Rarity) -> Option<&'static RelicDef> {
        Some(self.take_relic_of_tier(tier_of_rarity(rarity)))
    }

    /// 从指定档次的池子里取一件;抽干了按兜底链退档,再没有就给兜底遗物
    fn take_relic_of_tier(&mut self, tier: RelicTier) -> &'static RelicDef {
        for t in tier_chain(tier) {
            if let Some(pool) = self.relic_pools.slot_mut(*t) {
                if !pool.is_empty() {
                    return pool.remove(0);
                }
            }
        }
        if tier == RelicTier::Boss {
            relics::relic_def_or_panic("red_circlet")
        } else {
            relics::relic_def_or_panic("circlet")
        }
    }

    /// 只看不取(宝箱房在开箱前就要把遗物名字显示出来)
    fn peek_relic_of_tier(&self, tier: RelicTier) -> &'static RelicDef {
        for t in tier_chain(tier) {
            if let Some(pool) = self.relic_pools.slot(*t) {
                if let Some(def) = pool.first() {
                    return def;
                }
            }
        }
        if tier == RelicTier::Boss {
            relics::relic_def_or_panic("red_circlet")
        } else {
            relics::relic_def_or_panic("circlet")
        }
    }

    /// 无界面随机遗物(事件用):先滚档次再从池子里抽.
    /// 抽到"瓶装/磨刀石"这类还要再开一次选牌界面的遗物就接着再抽一件(参考实现同规则)
    fn take_relic_of_any(&mut self) -> Option<&'static RelicDef> {
        let tier = self.roll_combat_relic_tier();
        loop {
            let def = self.take_relic_of_tier(tier);
            if !matches!(
                def.id,
                "bottled_flame" | "bottled_lightning" | "bottled_tornado" | "whetstone"
            ) {
                return Some(def);
            }
        }
    }

    /// 战斗奖励的遗物档次:<50 普通,<83 罕见,其余稀有(都走 relicRng)
    fn roll_combat_relic_tier(&mut self) -> RelicTier {
        let roll = self.streams.run(RunStream::RelicRng).random_range(0, 99);
        if roll < 50 {
            RelicTier::Common
        } else if roll < 83 {
            RelicTier::Uncommon
        } else {
            RelicTier::Rare
        }
    }

    /// 精英奖励的遗物档次:<50 普通,>82 稀有,其余罕见(参考实现 returnRandomRelicTierElite)
    fn roll_elite_relic_tier(&mut self) -> RelicTier {
        let roll = self.streams.run(RunStream::RelicRng).random(99);
        if roll < ELITE_RELIC_COMMON_BELOW {
            RelicTier::Common
        } else if roll > ELITE_RELIC_RARE_ABOVE {
            RelicTier::Rare
        } else {
            RelicTier::Uncommon
        }
    }

    /// 药水掉落:先掷 d100(带保底 ±10),掉出来再掷是哪瓶
    fn roll_potion_reward(&mut self, rewards_so_far: usize) -> Option<&'static PotionDef> {
        let mut chance = POTION_DROP_BASE_CHANCE + self.potion_chance;
        // 白色野兽雕像:药水必掉
        if self.has_relic_fx(|fx| fx.potions_always) {
            chance = 100;
        }
        // 苏族之魂:再也拿不到药水
        if self.has_relic_fx(|fx| fx.no_potions) {
            chance = 0;
        }
        if rewards_so_far >= 4 {
            chance = 0;
        }
        if self.streams.run(RunStream::PotionRng).random(99) as i32 >= chance {
            self.potion_chance += POTION_PITY_STEP;
            return None;
        }
        self.potion_chance -= POTION_PITY_STEP;
        potions::random_potion(self.streams.run(RunStream::PotionRng), potions::class_color(self.character))
    }

    /// 金标准用:连掷 n 次战斗/事件档(或精英档)的遗物掉落,返回(档次, 身份)
    #[cfg(test)]
    pub fn debug_relic_drops(&mut self, n: usize, elite: bool) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for _ in 0..n {
            let tier = if elite {
                self.roll_elite_relic_tier()
            } else {
                self.roll_combat_relic_tier()
            };
            let def = self.take_relic_of_tier(tier);
            out.push((tier.name().to_lowercase(), def.id.to_string()));
        }
        out
    }

    /// 金标准用:Boss 遗物三选一,连取 sets 组(每组三件)
    #[cfg(test)]
    pub fn debug_relic_boss_choices(&mut self, sets: usize) -> Vec<Vec<String>> {
        (0..sets)
            .map(|_| {
                (0..3)
                    .map(|_| self.take_relic_of_tier(RelicTier::Boss).id.to_string())
                    .collect()
            })
            .collect()
    }

    /// 金标准用:连开 n 个宝箱,返回(尺寸, 有没有金币, 档次, 身份)
    #[cfg(test)]
    pub fn debug_relic_chests(&mut self, n: usize) -> Vec<(String, bool, String, String)> {
        let mut out = Vec::new();
        for _ in 0..n {
            self.open_treasure();
            let chest = self.chest.expect("刚开的箱子");
            let size = match chest.size {
                ChestSize::Small => "small",
                ChestSize::Medium => "medium",
                ChestSize::Large => "large",
            };
            let id = self.treasure.map(|d| d.id.to_string()).unwrap_or_default();
            out.push((size.to_string(), chest.gold_present, chest.tier.name().to_lowercase(), id));
            self.take_treasure();
        }
        out
    }

    /// 金标准用:连续按"普通怪的第一件奖励"掷 n 次药水,返回身份序列
    #[cfg(test)]
    pub fn debug_potion_rewards(&mut self, n: usize) -> Vec<Option<&'static str>> {
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(self.roll_potion_reward(1).map(|p| p.id));
        }
        out
    }

    /// 金标准用:连续从 potionRng 抽 n 瓶药水(不带掉落判定)
    #[cfg(test)]
    pub fn debug_potion_draws(&mut self, n: usize) -> Vec<&'static str> {
        let color = potions::class_color(self.character);
        let mut out = Vec::new();
        for _ in 0..n {
            if let Some(p) = potions::random_potion(self.streams.run(RunStream::PotionRng), color) {
                out.push(p.id);
            }
        }
        out
    }

    /// 金标准用:药水保底值与 potionRng 的步数
    #[cfg(test)]
    pub fn debug_potion_state(&mut self) -> (i32, u32) {
        (
            self.potion_chance,
            self.streams.run(RunStream::PotionRng).state().counter,
        )
    }

    /// 卡牌奖励:三张,每张先掷稀有度(带动保底)再从对应池子里抽一张,同一次不重样
    fn create_card_reward(&mut self, kind: EnemyKind) -> Vec<CardInstance> {
        let mut out: Vec<CardInstance> = Vec::new();
        let bonus = self.player.relic_fx_sum(|r| r.fx.card_reward_bonus);
        let count = (CARD_REWARD_COUNT as i32 + bonus).max(0) as usize;
        // 棱彩碎片:奖励池混入无色牌与其它颜色
        let prismatic = self.has_relic_fx(|fx| fx.prismatic_rewards);
        for _ in 0..count {
            let rarity = self.roll_card_rarity(kind);
            match rarity {
                Rarity::Rare => self.card_rarity_factor = CARD_RARITY_PITY_START,
                Rarity::Common => {
                    self.card_rarity_factor =
                        (self.card_rarity_factor - 1).max(CARD_RARITY_PITY_FLOOR)
                }
                _ => {}
            }
            let pool = if prismatic {
                cards::prismatic_reward_pool(rarity)
            } else {
                cards::reward_pool(rarity)
            };
            if pool.is_empty() {
                break;
            }
            let mut id = self.streams.run(RunStream::CardRng).pick(&pool).id;
            let mut guard = 0;
            loop {
                if !out.iter().any(|c| c.def.id == id) {
                    break;
                }
                id = self.streams.run(RunStream::CardRng).pick(&pool).id;
                guard += 1;
                if guard >= 1000 {
                    break;
                }
            }
            out.push(CardInstance::new(cards::card_def_or_panic(id)));
            // 非稀有牌按幕数掷一次升级(第一幕 0%,第二幕 25%,第三幕起 50%);
            // 这一掷无论成败都要消耗(只有概率为 0 或稀有牌才不掷)
            let chance = reward_upgrade_chance(self.act, self.ascension);
            if rarity != Rarity::Rare && chance > 0.0 {
                let roll = self
                    .streams
                    .run(RunStream::CardRng)
                    .random_bool_chance(chance);
                if roll {
                    out.last_mut().expect("刚推入一张").upgrade();
                }
            }
        }
        out
    }

    /// sensory_stone 的无色牌奖励:一组三张(受问号卡/残缺王冠那类奖励件数加成),
    /// 稀有度按事件档掷(参考实现 createColorlessCardReward),无色池里没有普通牌,
    /// 掷到普通就抬成非普通;抽到稀有把保底压回初值,普通保底 -1;非稀有按幕数掷升级.
    fn create_colorless_card_reward(&mut self) -> Vec<CardInstance> {
        let mut out: Vec<CardInstance> = Vec::new();
        let bonus = self.player.relic_fx_sum(|r| r.fx.card_reward_bonus);
        let count = (CARD_REWARD_COUNT as i32 + bonus).max(0) as usize;
        for _ in 0..count {
            let mut rarity = self.roll_card_rarity(EnemyKind::Normal);
            match rarity {
                Rarity::Rare => self.card_rarity_factor = CARD_RARITY_PITY_START,
                Rarity::Common => {
                    self.card_rarity_factor =
                        (self.card_rarity_factor - 1).max(CARD_RARITY_PITY_FLOOR)
                }
                _ => {}
            }
            if rarity == Rarity::Common {
                rarity = Rarity::Uncommon;
            }
            let pool: Vec<&'static CardDef> = cards::colorless_pool()
                .into_iter()
                .filter(|c| c.rarity == rarity)
                .collect();
            if pool.is_empty() {
                break;
            }
            let mut def = *self.streams.run(RunStream::CardRng).pick(&pool);
            let mut guard = 0;
            loop {
                if !out.iter().any(|c| c.def.id == def.id) {
                    break;
                }
                def = *self.streams.run(RunStream::CardRng).pick(&pool);
                guard += 1;
                if guard >= 1000 {
                    break;
                }
            }
            out.push(CardInstance::new(def));
            let chance = reward_upgrade_chance(self.act, self.ascension);
            if rarity != Rarity::Rare && chance > 0.0 {
                let roll = self
                    .streams
                    .run(RunStream::CardRng)
                    .random_bool_chance(chance);
                if roll {
                    out.last_mut().expect("刚推入一张").upgrade();
                }
            }
        }
        out
    }

    /// 抽稀有度:Boss 直接稀有,其余 d100 + 保底值比 3/37(精英 10/40)
    fn roll_card_rarity(&mut self, kind: EnemyKind) -> Rarity {
        if kind == EnemyKind::Boss {
            return Rarity::Rare;
        }
        let roll = self.streams.run(RunStream::CardRng).random(99) as i32 + self.card_rarity_factor;
        let (mut rare, uncommon) = if kind == EnemyKind::Elite {
            (CARD_RARE_CHANCE_ELITE, CARD_UNCOMMON_CHANCE_ELITE)
        } else {
            (CARD_RARE_CHANCE_NON_ELITE, CARD_UNCOMMON_CHANCE_NON_ELITE)
        };
        // 恩洛斯的礼物:稀有牌概率翻三倍(营火之外)
        if self.has_relic_fx(|fx| fx.rare_card_chance_x3)
            && self.screen != Screen::Rest
        {
            rare *= 3;
        }
        if roll < rare {
            Rarity::Rare
        } else if roll < rare + uncommon {
            Rarity::Uncommon
        } else {
            Rarity::Common
        }
    }

}

/// 卡牌奖励的升级概率:第一幕不给(0),第二幕 25%,第三幕及以后 50%;
/// 飞升 12+ 减半(参考实现 UPGRADE_CHANCES)
fn reward_upgrade_chance(act: u32, asc: u32) -> f32 {
    let base = match act {
        0 | 1 => 0.0,
        2 => 0.25,
        _ => 0.5,
    };
    if asc >= 12 {
        base / 2.0
    } else {
        base
    }
}

/// 商店底价(按稀有度查表)
fn shop_base(table: [(Rarity, i32); 3], rarity: Rarity) -> i32 {
    table
        .iter()
        .find(|(r, _)| *r == rarity)
        .map(|(_, v)| *v)
        .unwrap_or(50)
}

/// 存档里记的稀有度名字换回枚举
/// 存档里的遗物档次名字换回枚举(宝箱档)
fn relic_tier_from_name(name: &str) -> RelicTier {
    match name {
        "Common" => RelicTier::Common,
        "Uncommon" => RelicTier::Uncommon,
        "Rare" => RelicTier::Rare,
        "Shop" => RelicTier::Shop,
        "Boss" => RelicTier::Boss,
        "Event" => RelicTier::Event,
        "Special" => RelicTier::Special,
        _ => RelicTier::Common,
    }
}

#[cfg(test)]
mod tests {
    /// 测试里把"胜利后停留 2 秒"一步走完(真实流程由事件循环逐帧 tick)
    fn settle(r: &mut Run) {
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
    }

    use super::*;
    use crate::core::map::FLOORS;

    fn run(seed: u64) -> Run {
        Run::new(seed)
    }

    // ---- 本批新实现的遗物:一局流程侧的断言 ----

    /// 事件门槛用的是全局层号本身(原版 dead_adventurer 要 floorNum > 6,
    /// 本作的 floor_num 进第一房就是 1,不能再 +1)
    #[test]
    fn event_spawn_floor_uses_the_global_floor() {
        let mut r = run(7);
        r.debug_set_floor(6);
        assert!(!r.event_can_spawn("dead_adventurer"), "第 6 层还不出");
        assert!(!r.event_can_spawn("hypnotizing_colored_mushrooms"));
        r.debug_set_floor(7);
        assert!(r.event_can_spawn("dead_adventurer"), "第 7 层才出");
        assert!(r.event_can_spawn("hypnotizing_colored_mushrooms"));
    }

    /// 梦中情网:休息之后多一次卡牌三选一
    #[test]
    fn dream_catcher_gives_a_card_pick_after_resting() {
        let mut r = run(31);
        r.debug_add_relic("dream_catcher").unwrap();
        r.rest_heal();
        let reward = r.reward.as_ref().expect("休息后要开卡牌三选一");
        assert_eq!(reward.cards.len(), CARD_REWARD_COUNT);
        assert!(!reward.card_taken);
        let before = r.player.deck.len();
        r.reward_take().unwrap();
        assert_eq!(r.player.deck.len(), before + 1, "挑中的进牌组");
    }

    /// 和平烟斗:营火多出"删牌",选完那张从牌组里消失
    #[test]
    fn peace_pipe_adds_a_rest_removal() {
        let mut r = run(32);
        assert!(!r.rest_options().contains(&RestOption::Toke), "没烟斗就没这项");
        r.debug_add_relic("peace_pipe").unwrap();
        assert!(r.rest_options().contains(&RestOption::Toke));
        let before = r.player.deck.len();
        r.rest_choose(RestOption::Toke).unwrap();
        assert_eq!(r.screen, Screen::Pick);
        assert_eq!(r.picker.as_ref().unwrap().purpose, PickPurpose::Remove);
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), before - 1, "删掉一张");
    }

    /// 铲子:营火能挖出一件遗物
    #[test]
    fn shovel_digs_up_a_relic() {
        let mut r = run(33);
        r.debug_add_relic("shovel").unwrap();
        let before: Vec<&str> = r.player.relics.iter().map(|x| x.id).collect();
        let msg = r.rest_choose(RestOption::Dig).unwrap();
        assert!(r.player.relics.len() > before.len(), "挖到一件:{msg}");
        assert_eq!(r.player.relics.len(), before.len() + 1, "只多一件");
    }

    /// 吉利亚:营火举铁三次,下一场战斗开局多 3 点力量
    #[test]
    fn girya_lifts_and_pays_out_in_combat() {
        let mut r = run(34);
        r.debug_add_relic("girya").unwrap();
        for _ in 0..3 {
            r.rest_choose(RestOption::Lift).unwrap();
        }
        assert_eq!(r.lifts(), 3, "举了三次");
        assert!(!r.rest_options().contains(&RestOption::Lift), "举满就不给了");
        assert!(r.rest_choose(RestOption::Lift).is_err(), "不能再举");
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").unwrap();
        r.debug_start_combat(enc);
        let c = r.combat.as_ref().unwrap();
        assert_eq!(
            c.player.statuses.get(crate::core::status::Status::Strength),
            3,
            "开局就带 3 点力量"
        );
    }

    /// 跨战斗的遗物计数器:薰香的无形回合在整局里连着数,存档往返后接着数
    #[test]
    fn incense_burner_counter_carries_across_combats_and_saves() {
        let mut r = run(36);
        r.player.hp = 200;
        r.player.max_hp = 200;
        r.debug_add_relic("incense_burner").unwrap();
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").unwrap();
        r.debug_start_combat(enc);
        r.sync_combat();
        assert_eq!(r.relic_counters.incense, 1, "开局第 1 回合数到 1");
        for _ in 0..2 {
            r.combat.as_mut().unwrap().end_turn();
            r.sync_combat();
        }
        assert_eq!(r.relic_counters.incense, 3, "第 3 回合数到 3");

        // 打到一半存档:计数器与战斗现场一起回来
        let back_text = r.save_text();
        let mut r2 = Run::from_save(&back_text).expect("读回存档");
        assert_eq!(r2.relic_counters, r.relic_counters, "计数器随存档走");
        assert_eq!(
            r2.combat.as_ref().unwrap().rs.incense,
            3,
            "战斗现场里也还是 3"
        );
        // 读档重建时会走一遍开局的第 1 回合与洗牌,不能把存档里的数再带高一格
        assert!(
            r2.save_text().contains("relic_counters=0,0,3,0,0,0"),
            "存-读-再存,计数器照旧:\n{}",
            r2.save_text()
        );

        // 下一场从停下的 3 接着数:第 6 个回合触发无形,然后归零重数
        r2.debug_start_combat(enc);
        r2.sync_combat();
        assert_eq!(r2.relic_counters.incense, 4, "新一场从上一场的 3 接着数");
        for _ in 0..2 {
            r2.combat.as_mut().unwrap().end_turn();
            r2.sync_combat();
        }
        assert_eq!(
            r2.combat
                .as_ref()
                .unwrap()
                .player
                .statuses
                .get(Status::Intangible),
            1,
            "第 6 个回合给 1 层无形"
        );
        assert_eq!(r2.relic_counters.incense, 0, "触发后归零,下一轮重新数");
        // 无形只护这一回合:怪物走完就减掉(参考实现里 turnBased 的能力在回合末 tick)
        r2.combat.as_mut().unwrap().end_turn();
        r2.sync_combat();
        assert_eq!(
            r2.combat
                .as_ref()
                .unwrap()
                .player
                .statuses
                .get(Status::Intangible),
            0,
            "无形只管当回合"
        );
    }

    /// 跨战斗的遗物计数器只在自己这件遗物在手时累加(参考实现挂在遗物上):
    /// 没拿到之前打再多牌也不数,中途拿到就从 0 起
    #[test]
    fn relic_counters_only_advance_while_the_relic_is_held() {
        let mut r = run(37);
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").unwrap();
        r.debug_start_combat(enc);
        let play_a_strike = |r: &mut Run| {
            let c = r.combat.as_mut().unwrap();
            let idx = c
                .hand
                .iter()
                .position(|x| x.def.id == "strike")
                .expect("起手该有打击");
            c.play_card(idx, Some(0)).unwrap();
            r.sync_combat();
        };
        play_a_strike(&mut r);
        assert_eq!(r.relic_counters.pen_nib, 0, "没笔尖就不数");
        assert_eq!(r.relic_counters.attacks_total, 0, "没双节棍就不数");
        assert_eq!(r.relic_counters.cards_total, 0, "没墨水瓶就不数");

        // 中途入手笔尖:计数器没被之前的攻击带偏,第一张攻击就从 1 起
        r.debug_add_relic("pen_nib").unwrap();
        r.debug_start_combat(enc);
        assert_eq!(r.relic_counters.pen_nib, 0, "刚拿到手是 0");
        play_a_strike(&mut r);
        assert_eq!(r.relic_counters.pen_nib, 1, "拿到手后从 0 数起");
    }

    /// 羽翼靴:可以无视路径飞三次,飞完只剩正常可达
    #[test]
    fn wing_boots_fly_anywhere_three_times() {
        let mut r = run(35);
        r.debug_add_relic("wing_boots").unwrap();
        assert_eq!(r.wing_boots_charges(), 3, "拾取时给三次");
        for flights in 1..=3 {
            let here = r.pos.map(|p| r.map.node(p).floor).unwrap_or(0);
            let far = (0..r.map.nodes.len())
                .find(|i| {
                    *i != r.map.boss && r.map.node(*i).floor > here && !r.reachable().contains(i)
                })
                .expect("还有更远的、走不到的节点");
            assert!(r.travel_options().contains(&far), "有羽翼靴就能去");
            assert!(r.travel_is_fly(far));
            r.screen = Screen::Map; // 上一个节点可能是战斗/营火,这里只看穿越
            r.enter_node(far).unwrap();
            assert_eq!(r.wing_boots_charges(), 3 - flights, "飞一次少一次");
        }
        assert_eq!(r.travel_options(), r.reachable(), "没充能就只剩正常可达");
        let normal: Vec<usize> = r.reachable();
        let far = (0..r.map.nodes.len())
            .find(|i| *i != r.map.boss && r.map.node(*i).floor > 0 && !normal.contains(i));
        if let Some(far) = far {
            assert!(r.enter_node(far).is_err(), "没充能就不能乱飞");
        }
    }

    /// 浑天仪:拾取时连开五次三选一
    #[test]
    fn orrery_offers_five_card_picks() {
        let mut r = run(36);
        let before = r.player.deck.len();
        r.debug_add_relic("orrery").unwrap();
        for i in 0..5 {
            {
                let reward = r.reward.as_mut().expect("每一组都要在");
                assert_eq!(reward.cards.len(), CARD_REWARD_COUNT, "第 {i} 组三张");
                reward.index = 0;
            }
            r.reward_take().unwrap();
            r.reward_clamp();
        }
        assert_eq!(r.player.deck.len(), before + 5, "五次各拿一张");
        assert!(
            r.reward.as_ref().unwrap().queued.is_empty(),
            "五组都开完了"
        );
    }

    /// 祈祷轮:普通战斗多一组卡牌奖励
    #[test]
    fn prayer_wheel_adds_a_second_card_group() {
        let mut r = run(37);
        r.debug_add_relic("prayer_wheel").unwrap();
        let before = r.player.deck.len();
        let enc = crate::core::enemies::encounter_def("jaw_worm_solo").unwrap();
        r.debug_start_combat(enc);
        r.debug_win_battle();
        settle(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        assert_eq!(r.reward.as_ref().unwrap().queued.len(), 1, "多排了一组");
        for _ in 0..2 {
            let card_slot = r
                .reward_slots()
                .iter()
                .position(|s| matches!(s, RewardSlot::Card(_)))
                .expect("这一屏有卡牌奖励");
            r.reward.as_mut().unwrap().index = card_slot;
            r.reward_take().unwrap();
            r.reward_clamp();
        }
        assert_eq!(r.player.deck.len(), before + 2, "两组各拿一张");
    }

    /// 棱彩碎片:卡牌奖励的池子里混进了无色牌
    #[test]
    fn prismatic_shard_mixes_colorless_into_rewards() {
        // 无色牌全是罕见/稀有档,拿罕见档比
        let plain = cards::reward_pool(Rarity::Uncommon).len();
        let prismatic = cards::prismatic_reward_pool(Rarity::Uncommon).len();
        assert!(prismatic > plain, "棱彩池要比普通池大");
        assert!(
            cards::prismatic_reward_pool(Rarity::Uncommon)
                .iter()
                .any(|c| cards::pool_of(c) == "colorless"),
            "棱彩池里要有无色牌"
        );
        let mut r = run(38);
        r.debug_add_relic("prismatic_shard").unwrap();
        let mut colorless = 0;
        for _ in 0..40 {
            for c in r.create_card_reward(EnemyKind::Normal) {
                if cards::pool_of(c.def) == "colorless" {
                    colorless += 1;
                }
            }
        }
        assert!(colorless > 0, "四十组奖励里必须出现无色牌");
    }

    /// 瓶装火焰:拾取时只能挑攻击牌,选中的被封进瓶子
    #[test]
    fn bottled_flame_pickup_bottles_an_attack() {
        let mut r = run(40);
        r.debug_add_relic("bottled_flame").unwrap();
        assert_eq!(r.screen, Screen::Pick, "要开选牌界面");
        let cands = r.picker_candidates();
        assert!(!cands.is_empty());
        assert!(
            cands.iter().all(|i| r.player.deck[*i].kind() == CardType::Attack),
            "只给攻击牌"
        );
        let idx = cands[0];
        r.picker_confirm().unwrap();
        assert!(r.player.deck[idx].bottled, "被封进瓶子");
        assert_eq!(r.screen, Screen::Map);
    }

    /// 瓶装闪电:拾取时只能挑技能牌
    #[test]
    fn bottled_lightning_pickup_bottles_a_skill() {
        let mut r = run(41);
        r.debug_add_relic("bottled_lightning").unwrap();
        let cands = r.picker_candidates();
        assert!(!cands.is_empty());
        assert!(
            cands.iter().all(|i| r.player.deck[*i].kind() == CardType::Skill),
            "只给技能牌"
        );
        let idx = cands[0];
        r.picker_confirm().unwrap();
        assert!(r.player.deck[idx].bottled);
    }

    /// 瓶装龙卷风:拾取时只能挑能力牌(牌组里没有能力牌就不开界面)
    #[test]
    fn bottled_tornado_pickup_bottles_a_power() {
        let mut r = run(42);
        assert!(
            !r.player.deck.iter().any(|c| c.kind() == CardType::Power),
            "起始牌组没有能力牌"
        );
        // 牌组里没有对应类型:不开界面
        r.debug_add_relic("bottled_tornado").unwrap();
        assert_ne!(r.screen, Screen::Pick, "没能力牌就不开选牌");

        let mut r = run(42);
        r.player.deck.push(cards::card("inflame"));
        r.debug_add_relic("bottled_tornado").unwrap();
        assert_eq!(r.screen, Screen::Pick);
        let cands = r.picker_candidates();
        assert_eq!(cands.len(), 1, "只有刚加的那张能力牌");
        assert!(r.player.deck[cands[0]].kind() == CardType::Power);
        r.picker_confirm().unwrap();
        assert!(r.player.deck[cands[0]].bottled);
    }

    /// 神圣树皮:地图上喝的药水也翻倍(果汁 +5 -> +10 最大生命)
    #[test]
    fn sacred_bark_doubles_out_of_combat_potions() {
        let juice = potions::by_id("fruit_juice").unwrap();
        let mut r = run(43);
        assert!(r.add_potion(juice));
        let max_before = r.player.max_hp;
        r.quaff_potion(0, None).unwrap();
        assert_eq!(r.player.max_hp, max_before + 5, "本来 +5");

        r.debug_add_relic("sacred_bark").unwrap();
        assert!(r.add_potion(juice));
        let slot = r.player.potions.iter().position(|p| p.is_some()).unwrap();
        let max_before = r.player.max_hp;
        r.quaff_potion(slot, None).unwrap();
        assert_eq!(r.player.max_hp, max_before + 10, "神圣树皮 +10");
    }

    /// 封装的牌写进存档要能读回来
    #[test]
    fn bottled_flag_survives_a_save_round_trip() {
        let mut r = run(44);
        r.player.deck[0].bottled = true;
        let text = r.save_text();
        let back = Run::from_save(&text).unwrap();
        assert!(back.player.deck[0].bottled, "读回来还封着");
        assert!(
            back.player.deck[1..].iter().all(|c| !c.bottled),
            "别的牌没被顺带标记"
        );
    }

    /// 存档要带着章号、本局 Boss 与全局层号走:读回来还得在同一章,
    /// 地图按这一章的种子重生成,遭遇名单/事件池/钥匙一个不少
    #[test]
    fn act_and_floor_number_survive_a_save_round_trip() {
        for act in [2u32, 3, 4] {
            let mut r = run(13);
            // 钥匙先给好:第二章以后"绿钥匙到手就不标燃烧精英"这条会改地图
            r.keys = Keys {
                emerald: true,
                ruby: act >= 3,
                sapphire: false,
            };
            while r.act < act {
                r.begin_act();
            }
            // 站到本章第一层的第一个节点上(只摆位置,不进房间)
            let start = r.travel_options()[0];
            r.pos = Some(start);
            r.floor_reached = r.map.node(start).floor;
            let back = Run::from_save(&r.save_text()).expect("读回存档");
            assert_eq!(back.act, act, "章号要读回来");
            assert_eq!(back.pos, r.pos);
            assert_eq!(back.boss_enc.id, r.boss_enc.id, "本局 Boss 要读回来");
            assert_eq!(back.map.nodes.len(), r.map.nodes.len(), "地图要按章重生成");
            assert_eq!(back.monster_list, r.monster_list);
            assert_eq!(back.elite_list, r.elite_list);
            assert_eq!(back.event_pool, r.event_pool);
            assert_eq!(back.shrine_pool, r.shrine_pool);
            assert_eq!(back.keys, r.keys);
            assert_eq!(back.floor_reached, r.floor_reached);
        }
    }

    /// 战斗现场的存档要带"顶牌在下标 0"的标记;旧格式(没有这行)必须明确拒绝
    #[test]
    fn combat_save_marks_the_pile_order_and_rejects_the_old_format() {
        let base = run(44).save_text();
        let combat = "combat_encounter=jaw_worm_solo\n\
                      combat_turn=1\n\
                      combat_energy=3\n\
                      combat_max_energy=3\n\
                      combat_hp=80\n\
                      combat_block=0\n\
                      combat_hand=strike,defend\n\
                      combat_draw=defend,strike\n\
                      combat_discard=bash\n\
                      combat_exhaust=\n\
                      combat_enemies=40:0:0:1\n";
        // 老格式的抽牌堆是"顶牌记在末尾":直接拒绝,不能静默读反
        let err = match Run::from_save(&format!("{base}{combat}")) {
            Ok(_) => panic!("旧格式的战斗存档应该被拒绝"),
            Err(e) => e,
        };
        assert!(err.contains("旧版抽牌堆朝向"), "要说清楚为什么拒绝: {err}");
        // 新格式读回来,牌堆顺序照原样
        let back = Run::from_save(&format!("{base}combat_pile_order=top\n{combat}")).unwrap();
        let c = back.combat.as_ref().expect("战斗现场要读回来");
        let ids: Vec<&str> = c.draw.iter().map(|x| x.def.id).collect();
        assert_eq!(ids, vec!["defend", "strike"], "顶牌在下标 0");
        let ids: Vec<&str> = c.hand.iter().map(|x| x.def.id).collect();
        assert_eq!(ids, vec!["strike", "defend"]);
        let ids: Vec<&str> = c.discard.iter().map(|x| x.def.id).collect();
        assert_eq!(ids, vec!["bash"]);
        assert!(
            back.save_text().contains("combat_pile_order=top"),
            "存回去还带着标记"
        );
    }

    #[test]
    fn new_run_has_starter_pack() {
        let r = run(1);
        assert_eq!(r.player.hp, STARTING_HP);
        assert_eq!(r.player.max_hp, STARTING_HP);
        assert_eq!(r.player.gold, STARTING_GOLD);
        assert_eq!(r.player.deck.len(), 10);
        assert_eq!(r.player.potions.len(), POTION_SLOTS);
        assert!(r.player.potions.iter().all(|p| p.is_none()));
        assert_eq!(r.screen, Screen::Map);
        assert!(r.pos.is_none());
        assert_eq!(r.player.relics.len(), 1);
    }

    #[test]
    fn unreachable_node_is_rejected() {
        let mut r = run(2);
        let far = r.map.row(FLOORS - 1)[0];
        assert!(r.enter_node(far).is_err(), "没出发就不能跳到最后一层");
    }

    /// "不可移除"的诅咒不进删牌候选,普通牌照旧能删
    #[test]
    fn unremovable_curses_stay_out_of_the_removal_picker() {
        let mut r = run(11);
        let deck_len = r.player.deck.len();
        for id in ["ascenders_bane", "curse_of_the_bell", "necronomicurse"] {
            r.player.deck.push(cards::card(id));
        }
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let cands = r.picker_candidates();
        assert_eq!(cands.len(), deck_len, "三张不可移除的牌都不该出现在候选里");
        assert!(cands.iter().all(|i| !r.player.deck[*i].def.unremovable));

        // 选中第一张仍然能正常删掉,删完牌组少一张
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), deck_len + 2);
        assert_eq!(
            r.player
                .deck
                .iter()
                .filter(|c| c.def.unremovable)
                .count(),
            3
        );
    }

    /// 寄生:从牌组里删掉要付 3 点最大生命(它本身不是"不可移除",只是有代价)
    #[test]
    fn removing_parasite_costs_three_max_hp() {
        // 满血时删:当前生命要跟着新上限一起降
        let mut r = run(12);
        r.player.deck.push(cards::card("parasite"));
        let max0 = r.player.max_hp;
        assert_eq!(r.player.hp, max0, "满血开局");
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let slot = r
            .picker_candidates()
            .iter()
            .position(|i| r.player.deck[*i].def.id == "parasite")
            .expect("寄生应该出现在删牌候选里");
        r.picker.as_mut().unwrap().index = slot;
        r.picker_confirm().unwrap();
        assert_eq!(r.player.max_hp, max0 - 3, "最大生命 -3");
        assert_eq!(r.player.hp, max0 - 3, "当前生命夹到新的上限");
        assert!(
            !r.player.deck.iter().any(|c| c.def.id == "parasite"),
            "牌组里没有它了"
        );

        // 残血时删:当前生命不受影响,只掉上限
        let mut r = run(13);
        r.player.deck.push(cards::card("parasite"));
        let max0 = r.player.max_hp;
        r.player.hp = 20;
        r.open_picker(PickPurpose::Remove, Screen::Map, 0, None);
        let slot = r
            .picker_candidates()
            .iter()
            .position(|i| r.player.deck[*i].def.id == "parasite")
            .unwrap();
        r.picker.as_mut().unwrap().index = slot;
        r.picker_confirm().unwrap();
        assert_eq!(r.player.max_hp, max0 - 3);
        assert_eq!(r.player.hp, 20, "当前生命没到上限就不动");
    }

    #[test]
    fn cannot_enter_when_not_on_map() {
        let mut r = run(3);
        r.screen = Screen::Death;
        let start = r.reachable()[0];
        assert!(r.enter_node(start).is_err());
    }

    #[test]
    fn walking_the_map_reaches_the_boss() {
        // 一路往上点第一个可达节点,顺手把每个界面都走一遍:整局流程不能卡死
        let mut r = run(7);
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 400, "整局流程死循环");
            match r.screen {
                Screen::Map => {
                    let next = r.reachable();
                    assert!(!next.is_empty(), "无路可走");
                    let target = *next.last().unwrap();
                    r.enter_node(target).unwrap();
                }
                Screen::Combat => {
                    let c = r.combat.as_mut().unwrap();
                    for e in c.enemies.iter_mut() {
                        e.hp = 0;
                    }
                    c.phase = Phase::Won;
                    settle(&mut r);
                }
                Screen::Reward => {
                    // 药水奖励在格子满时拿不走,所以要给循环一个上限
                    for _ in 0..12 {
                        if r.reward_slots().is_empty() {
                            break;
                        }
                        let _ = r.reward_take();
                        r.reward_clamp();
                    }
                    r.leave_reward();
                }
                Screen::Pick => {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                Screen::Event => {
                    if r.event.as_ref().map(|e| e.result.is_none()).unwrap_or(false) {
                        let _ = r.choose_event(0);
                    }
                    if r.screen == Screen::Event {
                        r.leave_event();
                    }
                }
                Screen::Shop => r.leave_shop(),
                Screen::Rest => r.rest_heal(),
                Screen::Treasure => r.take_treasure(),
                Screen::Victory => break,
                Screen::Death => break,
                // 开始界面那几个不会出现在这条流程里
                Screen::Title | Screen::CharSelect | Screen::Compendium | Screen::Library => {
                    panic!("整局流程里不该出现开始界面")
                }
            }
        }
        assert_eq!(r.screen, Screen::Victory, "走到 Boss 应获胜");
        let last = r.pos.expect("应该有落点");
        assert_eq!(r.map.node(last).kind, NodeKind::Boss);
        assert!(r.floor_reached >= FLOORS - 1);
    }

    /// 商店不卖重复的卡与遗物, 也不卖已经拿到的遗物;
    /// 药水按参考实现是连抽三瓶, 可能重复
    #[test]
    fn shop_has_no_duplicates_and_no_owned_relics() {
        for seed in 0..80u64 {
            let mut r = run(seed);
            r.open_shop();
            let mut keys: Vec<String> = Vec::new();
            for it in &r.shop.as_ref().unwrap().items {
                let key = match it {
                    ShopItem::Card(c, _) => format!("card:{}", c.def.id),
                    ShopItem::Relic(d, _) => format!("relic:{}", d.id),
                    // 药水不去重(参考实现 generateShop 就是抽三次)
                    ShopItem::Potion(_, _) => continue,
                    ShopItem::Remove(_) => "remove".to_string(),
                };
                assert!(!keys.contains(&key), "seed {seed}: 商店里重复了 {key}");
                keys.push(key);
            }
            for it in &r.shop.as_ref().unwrap().items {
                if let ShopItem::Relic(d, _) = it {
                    assert!(
                        !r.player.relics.iter().any(|x| x.id == d.id),
                        "seed {seed}: 卖了已经有的遗物 {}",
                        d.id
                    );
                }
            }
            // 池子里不该留着已经拿到的遗物(全局不重复)
            for pool in r.relic_pools.all_mut() {
                assert!(pool
                    .iter()
                    .all(|p| !r.player.relics.iter().any(|o| o.id == p.id)));
            }
        }
    }

    /// 事件战斗奖励的药水走的也是带保底的掉落(参考实现 eventCombatRewards →
    /// rollPotionReward):保底叠到 60 时必掉,掉完把保底减回去
    #[test]
    fn potion_reward_roll_gets_the_pity_bonus() {
        let mut r = Run::new(9);
        assert_eq!(r.potion_chance, 0);
        r.potion_chance = 60;
        assert!(r.roll_potion_reward(2).is_some(), "保底叠满必掉药水");
        assert_eq!(r.potion_chance, 50, "掉了要把保底减一格");
    }

    /// 三选一不该出现同一张牌(抽很多局来看)
    #[test]
    fn reward_cards_never_repeat() {
        for seed in 0..120u64 {
            let mut r = run(seed);
            let start = r.reachable()[0];
            r.enter_node(start).unwrap();
            {
                let c = r.combat.as_mut().unwrap();
                for e in c.enemies.iter_mut() {
                    e.hp = 0;
                }
                c.phase = Phase::Won;
            }
            settle(&mut r);
            let cards = &r.reward.as_ref().unwrap().cards;
            let mut ids: Vec<&str> = cards.iter().map(|c| c.def.id).collect();
            let before = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(before, ids.len(), "seed {seed}: 奖励里出现了重复的牌");
        }
    }

    #[test]
    fn combat_victory_gives_reward_and_keeps_hp() {
        let mut r = run(11);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        let hp_before = r.player.hp;
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        let reward = r.reward.as_ref().unwrap();
        assert!(reward.gold > 0);
        assert_eq!(reward.cards.len(), 3);
        assert!(r.player.hp >= hp_before.min(r.player.max_hp));
        // 领金币
        let mut got_gold = false;
        for i in 0..r.reward_slots().len() {
            if let Some(rw) = r.reward.as_mut() {
                rw.index = i;
            }
            if matches!(r.reward_slots().get(i), Some(RewardSlot::Gold)) {
                r.reward_take().unwrap();
                got_gold = true;
            }
        }
        assert!(got_gold);
        assert!(r.player.gold > STARTING_GOLD);
    }

    /// 贪婪之手赚到的金币要真的进到这一局的钱包里,而且只进一次
    #[test]
    fn hand_of_greed_gold_reaches_the_run() {
        let mut r = run(17);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        let gold_before = r.player.gold;
        {
            let c = r.combat.as_mut().unwrap();
            c.enemies[0].hp = 1;
            c.energy = 9;
        }
        r.debug_add_card("Hand of Greed").unwrap();
        let idx = r
            .combat
            .as_ref()
            .unwrap()
            .hand
            .iter()
            .position(|c| c.def.id == "hand_of_greed")
            .expect("调试加的牌该在手牌里");
        r.combat
            .as_mut()
            .unwrap()
            .play_card(idx, Some(0))
            .unwrap();
        r.sync_combat();
        assert_eq!(r.player.gold, gold_before + 20, "致命一击的金币要落袋");
        r.sync_combat();
        assert_eq!(r.player.gold, gold_before + 20, "同一笔钱不能入账两次");
    }

    /// 把玩家放到本章最后一层的第一个节点上,再走进 Boss 房
    fn enter_act_boss(r: &mut Run) {
        let pre = r.map.row(FLOORS - 1)[0];
        r.pos = Some(pre);
        r.floor_reached = FLOORS - 1;
        let boss = r.map.boss;
        r.enter_node(boss).unwrap();
    }

    /// 把当前这场战斗直接判赢并结算
    fn win_combat(r: &mut Run) {
        let c = r.combat.as_mut().expect("要在战斗里才能判胜");
        for e in c.enemies.iter_mut() {
            e.hp = 0;
        }
        c.phase = Phase::Won;
        settle(r);
    }

    /// 第一、二幕的 Boss 奖励屏离开之后进下一幕
    #[test]
    fn boss_victory_leads_to_victory_screen() {
        let mut r = run(13);
        // 直接把玩家放到 Boss 前一层的节点上
        let boss_floor = FLOORS;
        assert_eq!(r.map.node(r.map.boss).floor, boss_floor);
        enter_act_boss(&mut r);
        win_combat(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        // 奖励屏的 next 标成 Victory 只是"这是 Boss 奖励"的记号:
        // 真正离开时按幕推进(第一幕之后是第二幕,不是终局)
        assert_eq!(r.reward.as_ref().unwrap().next, Screen::Victory);
        assert_eq!(r.act, 1);
        let next = r.leave_reward();
        assert_eq!(next, Screen::Map, "第一幕 Boss 之后该进第二幕");
        assert_eq!(r.act, 2);
        assert_eq!(r.player.hp, r.player.max_hp, "切幕回满血");
        assert!(r.pos.is_none());
        assert!(!r.monster_list.is_empty());
        assert_eq!(r.event_pool, act_event_pool(2));
    }

    /// 第三幕 Boss:没集齐三把钥匙就是本局胜利;集齐了开门直接进第四幕
    #[test]
    fn act_three_boss_opens_the_door_only_with_all_three_keys() {
        for keys in [false, true] {
            let mut r = run(13);
            r.keys = Keys {
                emerald: keys,
                ruby: keys,
                sapphire: keys,
            };
            while r.act < 3 {
                r.begin_act();
            }
            assert_eq!(r.act, 3);
            assert_eq!(r.screen, Screen::Map);
            enter_act_boss(&mut r);
            win_combat(&mut r);
            if keys {
                assert_eq!(r.act, 4, "三把钥匙齐了就把门打开");
                assert_eq!(r.screen, Screen::Map, "开门后直接进第四章地图");
                assert_eq!(r.player.hp, r.player.max_hp);
            } else {
                assert_eq!(r.act, 3);
                assert_eq!(r.screen, Screen::Victory, "没钥匙就是本局胜利");
                assert!(r.reward.is_none(), "第三幕 Boss 没有奖励屏");
            }
        }
    }

    /// 第四章:固定 4 层(休息点 → 商店 → 精英 → 心脏)的一列,
    /// 精英定死是盾与矛,Boss 是心脏,打倒心脏本局结束
    #[test]
    fn act_four_runs_the_elite_then_the_heart() {
        let mut r = run(13);
        r.keys = Keys {
            emerald: true,
            ruby: true,
            sapphire: true,
        };
        while r.act < 4 {
            r.begin_act();
        }
        assert_eq!(r.act, 4);
        assert_eq!(r.map.total_floors(), 4);
        let kinds: Vec<NodeKind> = (0..4)
            .map(|f| {
                let i = r.map.row(f)[0];
                assert_eq!(r.map.node(i).col, 3, "第四章在第 3 列一竖条");
                r.map.node(i).kind
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                NodeKind::Rest,
                NodeKind::Shop,
                NodeKind::Elite,
                NodeKind::Boss
            ]
        );
        assert_eq!(r.boss_enc.id, "the_heart");
        // 休息点
        r.enter_node(r.map.row(0)[0]).unwrap();
        assert_eq!(r.screen, Screen::Rest);
        r.rest_choose(RestOption::Rest).unwrap();
        assert_eq!(r.screen, Screen::Map);
        // 商店
        r.enter_node(r.map.row(1)[0]).unwrap();
        assert_eq!(r.screen, Screen::Shop);
        r.leave_shop();
        // 精英:定死的一对
        r.enter_node(r.map.row(2)[0]).unwrap();
        assert_eq!(r.screen, Screen::Combat);
        assert_eq!(r.combat().unwrap().encounter_id, "shield_and_spear");
        win_combat(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        // 奖励全拿:药水格满了就跳过那一格(与对拍脚本同规则)
        for _ in 0..20 {
            if r.reward_slots().is_empty() {
                break;
            }
            if r.reward_take().is_err() {
                let i = r.reward.as_ref().unwrap().index;
                let slot = r.reward_slots().get(i).copied();
                if let (Some(RewardSlot::Potion(p)), Some(rw)) = (slot, r.reward.as_mut()) {
                    rw.potion_taken[p] = true;
                } else {
                    break;
                }
            }
        }
        r.leave_reward();
        assert_eq!(r.screen, Screen::Map);
        // 心脏
        r.enter_node(r.map.boss).unwrap();
        assert_eq!(r.combat().unwrap().encounter_id, "the_heart");
        win_combat(&mut r);
        assert_eq!(r.screen, Screen::Victory, "打倒心脏就是终局");
        assert_eq!(r.act, 4);
    }

    #[test]
    fn defeat_ends_the_run() {
        let mut r = run(17);
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        {
            let c = r.combat.as_mut().unwrap();
            c.player.hp = 0;
            c.phase = Phase::Lost;
        }
        settle(&mut r);
        assert_eq!(r.screen, Screen::Death);
        assert_eq!(r.player.hp, 0);
    }

    #[test]
    fn shop_prices_lose_money_and_add_cards() {
        let mut r = run(19);
        // 找一个商店节点,站到它上一层
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .expect("地图应有商店");
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.floor_reached = r.map.node(parent).floor;
        r.player.gold = 500;
        r.enter_node(shop).unwrap();
        assert_eq!(r.screen, Screen::Shop);
        let gold_before = r.player.gold;
        let deck_before = r.player.deck.len();
        // 买第一件商品(一定是卡牌)
        let msg = r.buy_selected().unwrap();
        assert!(msg.starts_with("bought"));
        assert!(r.player.gold < gold_before);
        assert_eq!(r.player.deck.len(), deck_before + 1);
    }

    #[test]
    fn shop_rejects_purchase_without_gold() {
        let mut r = run(23);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 0;
        assert!(r.buy_selected().is_err());
    }

    /// 蛋与陶瓷鱼对"卡牌奖励"也生效:奖励的加牌要过同一条加牌钩子
    /// (以前只有事件那条路走钩子,奖励/商店直接 push 牌组)
    #[test]
    fn molten_egg_and_ceramic_fish_apply_to_card_rewards() {
        let mut r = run(71);
        r.debug_add_relic("molten_egg").unwrap();
        r.debug_add_relic("ceramic_fish").unwrap();
        r.player.gold = 0;
        r.reward = Some(RewardState::empty(Screen::Map));
        {
            let rw = r.reward.as_mut().unwrap();
            rw.cards = vec![CardInstance::new(cards::card_def_or_panic("clothesline"))];
            rw.card_taken = false;
        }
        let idx = r
            .reward_slots()
            .iter()
            .position(|s| matches!(s, RewardSlot::Card(_)))
            .expect("奖励里应有卡牌格");
        r.reward.as_mut().unwrap().index = idx;
        r.reward_take().unwrap();
        let got = r.player.deck.last().expect("牌组末尾是刚拿的牌");
        assert!(got.upgraded, "熔火之蛋要把奖励里的攻击牌升级");
        assert_eq!(r.player.gold, 9, "陶瓷鱼每加一张牌给 9 金币");
    }

    /// 蛋与陶瓷鱼对"商店买牌"也生效(同一条加牌钩子)
    #[test]
    fn molten_egg_and_ceramic_fish_apply_to_shop_purchases() {
        let mut r = run(72);
        r.debug_add_relic("molten_egg").unwrap();
        r.debug_add_relic("ceramic_fish").unwrap();
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
        r.shop.as_mut().unwrap().items[i] =
            ShopItem::Card(CardInstance::new(cards::card_def_or_panic("clothesline")), 50);
        r.shop.as_mut().unwrap().index = i;
        let gold_before = r.player.gold;
        r.buy_selected().unwrap();
        let got = r.player.deck.last().expect("牌组末尾是刚买的牌");
        assert!(got.upgraded, "熔火之蛋要把买来的攻击牌升级");
        assert_eq!(r.player.gold, gold_before - 50 + 9, "付 50,陶瓷鱼再补 9");
    }

    /// 信使 -20% 与会员卡 -50% 相乘(-60%),不是相加(-70%)(wiki 口径);
    /// 会员卡的 `.5` 进位
    #[test]
    fn courier_and_membership_card_discounts_multiply() {
        let mut both = run(73);
        both.debug_add_relic("the_courier").unwrap();
        both.debug_add_relic("membership_card").unwrap();
        assert_eq!(both.discount(100), 40, "0.8 × 0.5 = 四折");
        let mut card = run(73);
        card.debug_add_relic("membership_card").unwrap();
        assert_eq!(card.discount(101), 51, "101 折半的 50.5 进位到 51");
        let mut courier = run(73);
        courier.debug_add_relic("the_courier").unwrap();
        assert_eq!(courier.discount(100), 80, "单信使打八折");
    }

    /// 魔法花把战后回血也抬 50%(原版:Burning Blood 6 -> 9,肉骨头 12 -> 18)
    #[test]
    fn magic_flower_boosts_post_combat_heal() {
        let mut plain = run(74);
        plain.debug_remove_relic("burning_blood").unwrap();
        plain.debug_add_relic("meat_on_the_bone").unwrap();
        plain.player.hp = 40;
        assert_eq!(plain.post_combat_heal(), 12, "半血以下肉骨头回 12");
        let mut flower = run(74);
        flower.debug_remove_relic("burning_blood").unwrap();
        flower.debug_add_relic("meat_on_the_bone").unwrap();
        flower.debug_add_relic("magic_flower").unwrap();
        flower.player.hp = 40;
        assert_eq!(flower.post_combat_heal(), 18, "魔法花 12 × 1.5 = 18");
        assert_eq!(flower.player.hp, 58);
    }

    /// 信使(The Courier):买走卡/遗物/药水后该格按同种类补货,且不再标已售
    #[test]
    fn courier_restocks_cards_relics_and_potions() {
        let mut r = run(37);
        r.debug_add_relic("the_courier").unwrap();
        r.player.gold = 99_999;
        r.open_shop();
        let (card_i, relic_i, potion_i) = {
            let shop = r.shop.as_ref().unwrap();
            let find = |k: ShopKind| shop.kinds.iter().position(|x| *x == k).unwrap();
            (find(ShopKind::ClassCard), find(ShopKind::Relic), find(ShopKind::Potion))
        };

        r.shop.as_mut().unwrap().index = card_i;
        r.buy_selected().unwrap();
        {
            let shop = r.shop.as_ref().unwrap();
            assert!(!shop.sold[card_i], "卡格买走后要补货");
            assert!(matches!(shop.items[card_i], ShopItem::Card(..)), "补的还是牌");
        }

        r.shop.as_mut().unwrap().index = relic_i;
        r.buy_selected().unwrap();
        {
            let shop = r.shop.as_ref().unwrap();
            assert!(!shop.sold[relic_i], "遗物格买走后要补货");
            match &shop.items[relic_i] {
                // 商店档遗物买走后补的是普通档次(原版/wiki)
                ShopItem::Relic(d, _) => assert_ne!(d.tier, RelicTier::Shop, "不补商店档"),
                _ => panic!("补的还是遗物"),
            }
        }

        r.shop.as_mut().unwrap().index = potion_i;
        r.buy_selected().unwrap();
        {
            let shop = r.shop.as_ref().unwrap();
            assert!(!shop.sold[potion_i], "药水格买走后要补货");
            assert!(matches!(shop.items[potion_i], ShopItem::Potion(..)), "补的还是药水");
        }
    }

    /// 补货价也带 20% 折扣:浮动上限 1.1(遗物/药水 1.05)乘 0.8 后低于原价
    #[test]
    fn courier_restocked_prices_are_discounted() {
        let mut r = run(41);
        r.debug_add_relic("the_courier").unwrap();
        r.player.gold = 99_999;
        r.open_shop();
        let (class_i, colorless_i, relic_i, potion_i) = {
            let shop = r.shop.as_ref().unwrap();
            let find = |k: ShopKind| shop.kinds.iter().position(|x| *x == k).unwrap();
            (
                find(ShopKind::ClassCard),
                find(ShopKind::ColorlessCard),
                find(ShopKind::Relic),
                find(ShopKind::Potion),
            )
        };
        for (i, expected_base) in [
            (class_i, None),
            (colorless_i, None),
            (relic_i, Some(SHOP_RELIC_BASE)),
            (potion_i, Some(SHOP_POTION_BASE)),
        ] {
            r.shop.as_mut().unwrap().index = i;
            r.buy_selected().unwrap();
            let shop = r.shop.as_ref().unwrap();
            let price = shop.items[i].price();
            let base = match expected_base {
                Some(table) => {
                    // 遗物/药水按稀有度查底价
                    let rarity = match &shop.items[i] {
                        ShopItem::Relic(d, _) => match d.tier {
                            RelicTier::Uncommon => Rarity::Uncommon,
                            RelicTier::Rare => Rarity::Rare,
                            _ => Rarity::Common,
                        },
                        ShopItem::Potion(d, _) => d.rarity,
                        _ => unreachable!(),
                    };
                    shop_base(table, rarity)
                }
                None => {
                    let rarity = match &shop.items[i] {
                        ShopItem::Card(c, _) => c.rarity(),
                        _ => unreachable!(),
                    };
                    shop_base(SHOP_CARD_BASE, rarity)
                }
            };
            assert!(
                price < base,
                "补货价 {price} 应低于底价 {base}(带 -20% 折扣)"
            );
        }
    }

    /// 没有信使就不补货:格子买走后保持已售
    #[test]
    fn purchases_without_courier_leave_the_slot_sold() {
        let mut r = run(43);
        r.player.gold = 99_999;
        r.open_shop();
        let card_i = r
            .shop
            .as_ref()
            .unwrap()
            .kinds
            .iter()
            .position(|k| *k == ShopKind::ClassCard)
            .unwrap();
        r.shop.as_mut().unwrap().index = card_i;
        r.buy_selected().unwrap();
        assert!(r.shop.as_ref().unwrap().sold[card_i], "没有信使就维持已售");
    }

    /// 未知房判定在 eventRng 上走一次;事件抽签掷的是副本,不动主流
    /// (参考实现 generateEvent 把 eventRng 按值传下去)
    #[test]
    fn unknown_room_advances_event_rng_but_event_pick_does_not() {
        let mut r = run(7);
        let before = r.streams.run(RunStream::EventRng).state().counter;
        let _ = r.resolve_unknown_room();
        assert_eq!(
            r.streams.run(RunStream::EventRng).state().counter,
            before + 1,
            "未知房判定该走一次 eventRng"
        );
        let _ = r.generate_event_id();
        assert_eq!(
            r.streams.run(RunStream::EventRng).state().counter,
            before + 1,
            "事件抽签不该动 eventRng"
        );
    }

    /// 未知房没判中的那一档概率涨一份起始值,判中的那一档复位
    #[test]
    fn unknown_room_chances_escalate_and_reset() {
        let mut r = run(11);
        assert_eq!(r.monster_chance, UNKNOWN_BASE.0);
        assert_eq!(r.shop_chance, UNKNOWN_BASE.1);
        assert_eq!(r.treasure_chance, UNKNOWN_BASE.2);
        let mut seen: Vec<&'static str> = Vec::new();
        for _ in 0..40 {
            let kind = r.resolve_unknown_room();
            let name = kind.name();
            if !seen.contains(&name) {
                seen.push(name);
            }
            if kind == NodeKind::Monster {
                assert_eq!(r.monster_chance, UNKNOWN_ESCALATION.0, "判中要复位");
            } else {
                assert!(r.monster_chance > UNKNOWN_BASE.0, "没判中要涨");
            }
            assert!(r.monster_chance <= 1.0);
            // 概率封顶前一直在涨,判中那一档必须回到起始值
            if kind == NodeKind::Shop {
                assert_eq!(r.shop_chance, UNKNOWN_ESCALATION.1);
            }
            if kind == NodeKind::Treasure {
                assert_eq!(r.treasure_chance, UNKNOWN_ESCALATION.2);
            }
        }
        assert!(seen.len() >= 2, "40 次判定该出现不止一种房间");
    }

    /// 抽过的事件从池子里移除:同一个事件不会再抽到,池子总数减少
    #[test]
    fn drawn_events_leave_their_pool() {
        let mut r = run(11);
        let total = r.event_pool.len() + r.shrine_pool.len() + r.one_time_pool.len();
        let picks: Vec<&str> = r
            .debug_event_picks(8)
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(picks.len(), 8, "池子够抽 8 个");
        let mut uniq = picks.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 8, "抽过的事件不该再出现");
        assert_eq!(
            r.event_pool.len() + r.shrine_pool.len() + r.one_time_pool.len(),
            total - 8,
            "抽走的要从池子里离开"
        );
    }

    #[test]
    fn removal_service_charges_on_confirm() {
        let mut r = run(29);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 500;
        let idx = r
            .shop
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|i| matches!(i, ShopItem::Remove(_)))
            .unwrap();
        r.shop.as_mut().unwrap().index = idx;
        let price = r.shop.as_ref().unwrap().items[idx].price();
        r.buy_selected().unwrap();
        assert_eq!(r.screen, Screen::Pick);
        let deck_before = r.player.deck.len();
        r.picker_confirm().unwrap();
        assert_eq!(r.player.deck.len(), deck_before - 1);
        assert_eq!(r.player.gold, 500 - price);
        assert_eq!(r.screen, Screen::Shop);
    }

    #[test]
    fn picker_cancel_leaves_deck_alone() {
        let mut r = run(31);
        let shop = (0..r.map.nodes.len())
            .find(|i| r.map.node(*i).kind == NodeKind::Shop)
            .unwrap();
        let parent = r.map.node(shop).prev[0];
        r.pos = Some(parent);
        r.enter_node(shop).unwrap();
        r.player.gold = 500;
        let idx = r
            .shop
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|i| matches!(i, ShopItem::Remove(_)))
            .unwrap();
        r.shop.as_mut().unwrap().index = idx;
        r.buy_selected().unwrap();
        let before = r.player.deck.len();
        r.picker_cancel();
        assert_eq!(r.player.deck.len(), before);
        assert_eq!(r.player.gold, 500, "取消不该扣钱");
    }

    #[test]
    fn rest_heals_thirty_percent() {
        let mut r = run(37);
        r.player.hp = 10;
        r.rest_heal();
        assert_eq!(r.player.hp, 10 + STARTING_HP * REST_HEAL_PCT / 100);
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn smith_upgrades_a_card() {
        let mut r = run(41);
        r.rest_smith();
        assert_eq!(r.screen, Screen::Pick);
        let before = r.player.deck.iter().filter(|c| c.upgraded).count();
        r.picker_confirm().unwrap();
        let after = r.player.deck.iter().filter(|c| c.upgraded).count();
        assert_eq!(after, before + 1);
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn potion_slots_are_finite() {
        let mut r = run(43);
        let Some(def) = crate::core::potions::POTIONS.first() else {
            return;
        };
        assert!(r.add_potion(def));
        assert!(r.add_potion(def));
        assert!(r.add_potion(def));
        assert!(!r.add_potion(def), "第四个格子不该存在");
        assert!(r.toss_potion(0).is_ok());
        assert!(r.add_potion(def));
    }

    #[test]
    fn potion_use_respects_context() {
        let mut r = run(47);
        // 战斗外只能喝能在地图上用的药水
        let combat_only = crate::core::potions::POTIONS
            .iter()
            .find(|p| !p.out_of_combat);
        if let Some(def) = combat_only {
            r.player.potions[0] = Some(def);
            assert!(r.quaff_potion(0, None).is_err());
        }
        let out = crate::core::potions::POTIONS
            .iter()
            .find(|p| p.out_of_combat);
        if let Some(def) = out {
            r.player.potions[1] = Some(def);
            r.player.hp = 1;
            assert!(r.quaff_potion(1, None).is_ok());
            assert!(r.player.potions[1].is_none());
        }
    }

    #[test]
    fn targeting_potion_through_run_flips_the_facing() {
        // 整局入口 run.quaff_potion 走的也是 Combat::use_potion 那条:朝向要跟着改,
        // 于是夹击的 1.5 倍落到另一只身上(原版 Surrounded:牌或药水都能改朝向).
        let mut r = run(44);
        let enc = crate::core::enemies::encounter_def("shield_and_spear").unwrap();
        r.debug_start_combat(enc);
        let fear = potions::POTIONS
            .iter()
            .find(|p| p.id == "fear_potion")
            .expect("恐惧药剂应该在池子里");
        r.player.potions[0] = Some(fear);
        assert_eq!(r.combat().unwrap().facing, 1, "开局朝向右边的长矛(slot 1)");
        assert!(r.quaff_potion(0, Some(0)).is_ok());
        assert_eq!(r.combat().unwrap().facing, 0, "药水命中谁就朝向谁");
        assert!(r.player.potions[0].is_none(), "喝完该腾出格子");
    }

    #[test]
    fn events_apply_outcomes() {
        let mut r = run(53);
        r.open_event();
        assert_eq!(r.screen, Screen::Event);
        // 随机抽到的事件可能是多屏或翻牌那种,结算挑一个选项 0 就直接给结果的
        r.debug_open_event("big_fish").unwrap();
        let before = r.player.gold;
        r.choose_event(0).unwrap();
        assert!(r.event.as_ref().unwrap().result.is_some());
        // 选项可能给钱也可能要钱,只要求不 panic 且状态自洽
        assert!(r.player.gold >= 0 || before >= 0);
        r.leave_event();
        assert_eq!(r.screen, Screen::Map);
    }

    #[test]
    fn history_records_the_run() {
        let mut r = run(67);
        let first = r.history.len();
        assert!(first > 0, "开局就该有一条");
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        assert!(r.history.len() > first, "进入房间要记一笔");
        {
            let c = r.combat.as_mut().unwrap();
            c.play_card(0, Some(0)).ok();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        let joined = r
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("-- Turn 1 --"), "战斗日志要抄进历史");
        assert!(joined.contains("dealt") || joined.contains("you play"));
    }

    #[test]
    fn fruit_juice_works_on_the_map() {
        let mut r = run(83);
        let def = potions::POTIONS
            .iter()
            .find(|p| p.id == "fruit_juice")
            .expect("果汗药水应该在池子里");
        assert!(def.out_of_combat, "果汗要能在地图上喝");
        r.player.potions[0] = Some(def);
        let (hp, max_hp) = (r.player.hp, r.player.max_hp);
        r.quaff_potion(0, None).unwrap();
        assert_eq!(r.player.max_hp, max_hp + 5);
        assert_eq!(r.player.hp, hp + 5);
        assert!(r.player.potions[0].is_none(), "喝完该腾出格子");
    }

    /// 血药按最大生命的百分比回,地图上也能喝
    #[test]
    fn blood_potion_heals_a_percentage_on_the_map() {
        let mut r = run(83);
        let def = potions::by_id("blood_potion").expect("血药在池子里");
        r.player.potions[0] = Some(def);
        r.player.hp = 10;
        let want = 10 + r.player.max_hp * 20 / 100;
        r.quaff_potion(0, None).unwrap();
        assert_eq!(r.player.hp, want);
        assert!(r.player.potions[0].is_none());
    }

    /// 烟雾弹从普通战斗里脱身:不给奖励,直接回地图
    #[test]
    fn smoke_bomb_leaves_a_normal_fight() {
        let mut r = run(89);
        r.debug_room("enemy cultist").unwrap();
        r.player.potions[0] = Some(potions::by_id("smoke_bomb").unwrap());
        r.quaff_potion(0, None).unwrap();
        assert!(r.combat.is_none(), "脱身后不该还挂着战斗");
        assert_eq!(r.screen, Screen::Map);
        assert!(r.player.potions[0].is_none());
        assert!(r.reward.is_none(), "脱身没有奖励");
    }

    /// Boss 战走不掉:药水不该被喝掉
    #[test]
    fn smoke_bomb_refuses_a_boss_fight() {
        let mut r = run(89);
        r.debug_room("boss").unwrap();
        r.player.potions[0] = Some(potions::by_id("smoke_bomb").unwrap());
        assert!(r.quaff_potion(0, None).is_err());
        assert!(r.combat.is_some());
        assert!(r.player.potions[0].is_some(), "拒绝了就不该消耗");
    }

    /// 仙女在瓶中:致命伤改成回到 30% 生命,并消耗那一格
    #[test]
    fn fairy_potion_saves_the_player_once() {
        let mut r = run(61);
        r.player.potions[0] = Some(potions::by_id("fairy_potion").unwrap());
        r.debug_room("enemy cultist").unwrap();
        {
            let c = r.combat_mut().unwrap();
            c.player.hp = 5;
            for e in c.enemies.iter_mut() {
                if let Some(i) = e
                    .def
                    .moves
                    .iter()
                    .position(|m| matches!(m.intent, crate::core::enemy::Intent::Attack { .. }))
                {
                    e.next_move = i;
                }
            }
            c.end_turn();
            assert_eq!(c.player.hp, 24, "保命符把血拉回最大生命的 30%");
        }
        r.sync_combat();
        assert_eq!(r.screen, Screen::Combat, "保住了就还在战斗里");
        assert!(r.player.potions[0].is_none(), "仙女该被消耗掉");
    }

    /// 仙女在瓶中喝不掉:地图与战斗里主动喝都报错,药水留在格子里
    #[test]
    fn fairy_potion_cannot_be_drunk() {
        let mut r = run(61);
        r.player.potions[0] = Some(potions::by_id("fairy_potion").unwrap());
        assert!(r.quaff_potion(0, None).is_err(), "地图上喝不掉");
        assert!(r.player.potions[0].is_some(), "拒绝了就不该消耗");
        r.debug_room("enemy cultist").unwrap();
        assert!(r.quaff_potion(0, None).is_err(), "战斗里也喝不掉");
        assert!(r.player.potions[0].is_some(), "拒绝了就该留在格子里");
    }

    /// 熵能酿剂:空槽用随机药水填满,走 potionRng
    #[test]
    fn entropic_brew_fills_the_empty_slots() {
        let mut r = run(93);
        let before = r.debug_potion_state().1;
        r.player.potions[0] = Some(potions::by_id("entropic_brew").unwrap());
        r.quaff_potion(0, None).unwrap();
        assert!(r.player.potions.iter().all(|s| s.is_some()), "三个槽都该填上");
        assert!(
            r.debug_potion_state().1 > before,
            "填槽要走 potionRng"
        );
    }

    #[test]
    fn potion_reward_needs_a_free_slot() {
        let mut r = run(71);
        // 三格占满时拿不下药水,腾出一格就能拿
        let def = crate::core::potions::POTIONS.first().unwrap();
        let start = r.reachable()[0];
        r.enter_node(start).unwrap();
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        settle(&mut r);
        let reward = r.reward.as_mut().unwrap();
        reward.potions = vec![def];
        reward.potion_taken = vec![false];
        for slot in r.player.potions.iter_mut() {
            *slot = Some(def);
        }
        r.reward_clamp();
        // 选中药水那一行
        let slots = r.reward_slots();
        let idx = slots
            .iter()
            .position(|s| matches!(s, RewardSlot::Potion(_)))
            .unwrap();
        r.reward.as_mut().unwrap().index = idx;
        assert!(r.reward_take().is_err(), "满格时不该拿得下");
        assert!(
            r.reward_slots().iter().any(|s| matches!(s, RewardSlot::Potion(_))),
            "拿不下就该还在"
        );
        // 腾一格
        r.player.potions[0] = None;
        let msg = r.reward_take().unwrap();
        assert!(msg.starts_with("potion gained"), "腾出格子后应该能拿走");
        assert!(r.player.potions.iter().flatten().count() == 3);
    }

    #[test]
    fn gold_never_goes_negative() {
        let mut r = run(59);
        r.player.gold = 5;
        r.spend_gold(50);
        assert_eq!(r.player.gold, 0);
    }

    #[test]
    fn damage_can_kill_the_player() {
        let mut r = run(61);
        r.player.hp = 3;
        r.damage(10);
        assert_eq!(r.player.hp, 0);
        assert_eq!(r.screen, Screen::Death);
    }

    /// 简单策略机器人:有威胁就挡,挡得住就砍,用最省的办法打完一只手牌
    pub(super) fn bot_take_turn(c: &mut Combat) {
        loop {
            let incoming: i32 = c
                .alive_enemies()
                .iter()
                .map(|i| {
                    let (per, times) = c.predicted_damage(*i);
                    per * times.max(1) as i32
                })
                .sum();
            let need_block = incoming > c.player.block;
            let mut best: Option<(usize, i32)> = None;
            for idx in 0..c.hand.len() {
                if c.blocked_reason(idx).is_some() {
                    continue;
                }
                let card = &c.hand[idx];
                let mut dmg = 0;
                let mut blk = 0;
                for e in card.effects() {
                    match *e {
                        crate::core::card::Effect::Damage { amount, times }
                        | crate::core::card::Effect::DamageAll { amount, times }
                        | crate::core::card::Effect::DamageRandom { amount, times }
                        | crate::core::card::Effect::DamageWithBonus { amount, times } => {
                            dmg += amount * times.max(1) as i32
                        }
                        crate::core::card::Effect::DamageAllX { per } => dmg += per * c.energy,
                        crate::core::card::Effect::Reaper { amount } => dmg += amount,
                        crate::core::card::Effect::Block { amount } => blk += amount,
                        _ => {}
                    }
                }
                let score = if need_block {
                    blk * 3 + dmg
                } else {
                    dmg * 3 + blk
                };
                if score > 0 && best.map(|(_, s)| score > s).unwrap_or(true) {
                    best = Some((idx, score));
                }
            }
            let Some((idx, _)) = best else { return };
            let target = c
                .alive_enemies()
                .into_iter()
                .min_by_key(|i| c.enemies[*i].hp);
            if c.play_card(idx, target).is_err() {
                return;
            }
        }
    }

    /// 把非战斗界面推进到地图:只有战斗中才需要机器人动脑
    fn resolve_non_combat(r: &mut Run) {
        match r.screen {
            Screen::Shop => r.leave_shop(),
            Screen::Rest => r.rest_heal(),
            Screen::Treasure => r.take_treasure(),
            Screen::Event => {
                let _ = r.choose_event(0);
                while r.screen == Screen::Pick {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                if r.screen == Screen::Event {
                    r.leave_event();
                }
            }
            _ => {}
        }
    }

    /// 让机器人把一局打完(死了也算走完),返回结束时的状态
    fn bot_play(mut r: Run, seed: u64) -> Run {
        let mut guard = 0;
        while r.screen != Screen::Victory && r.screen != Screen::Death {
            guard += 1;
            assert!(guard < 80, "seed {seed}: 流程卡在 {:?}", r.screen);
            match r.screen {
                Screen::Map => {
                    let next = r.reachable();
                    assert!(!next.is_empty(), "seed {seed}: 无路可走");
                    let node = next[next.len() / 2];
                    r.enter_node(node).unwrap();
                }
                Screen::Combat => {
                    let mut turns = 0;
                    loop {
                        turns += 1;
                        assert!(turns <= 40, "seed {seed} 单场战斗超过 40 回合");
                        bot_take_turn(r.combat.as_mut().unwrap());
                        if r.combat.as_ref().unwrap().phase != Phase::PlayerTurn {
                            break;
                        }
                        r.combat.as_mut().unwrap().end_turn();
                    }
                    settle(&mut r);
                }
                Screen::Reward => {
                    for _ in 0..12 {
                        if r.reward_slots().is_empty() {
                            break;
                        }
                        let _ = r.reward_take();
                        r.reward_clamp();
                    }
                    r.leave_reward();
                }
                Screen::Rest => {
                    // 血少就睡,否则打铁升级
                    if r.player.hp * 2 < r.player.max_hp {
                        r.rest_heal();
                    } else {
                        r.rest_smith();
                    }
                }
                Screen::Pick => {
                    let _ = r.picker_confirm();
                    if r.screen == Screen::Pick {
                        r.picker_cancel();
                    }
                }
                _ => resolve_non_combat(&mut r),
            }
        }
        r
    }

    #[test]
    fn a_simple_bot_can_clear_the_first_half_of_the_act() {
        // 机器人只会"有威胁就挡、否则砍",奖励全拿、血少就睡.
        // 它要连前半段都上不去,说明数值或流程有硬伤
        let mut best = 0usize;
        let mut report = Vec::new();
        for seed in [1u64, 2, 3, 5, 8, 13, 21] {
            let r = bot_play(run(seed), seed);
            report.push((seed, r.floor_reached + 1, r.player.hp, r.stats.fights));
            best = best.max(r.floor_reached);
        }
        assert!(
            best >= 12,
            "机器人最好的成绩只到第 {} 层,报告 {:?}",
            best + 1,
            report
        );
    }

    // ---- 燃烧精英与三把钥匙 ----

    /// 燃烧精英开局挂的增益照地图掷出来的编号:
    /// 0 力量 +1,1 生命上限 +25%(四舍五入)并补等量血,2 金属化 4,3 再生 3(act=1)
    #[test]
    fn burning_elite_buff_applies_per_map_roll() {
        // 敌人自带的开局状态(有些精英开局就有力量),增益要迭加在它上面
        fn innate(e: &crate::core::combat::Enemy, s: Status) -> i32 {
            e.def.innate.iter().find(|(k, _)| *k == s).map(|(_, n)| *n).unwrap_or(0)
        }
        let buff_case = |buff: i32| -> Combat {
            let mut r = run(3);
            r.debug_room("enemy jaw_worm").expect("打一场");
            r.apply_burning_elite_buff(buff);
            r.combat.take().expect("战斗还在")
        };
        let c = buff_case(0);
        assert_eq!(
            c.enemies[0].statuses.get(Status::Strength),
            innate(&c.enemies[0], Status::Strength) + 1,
            "增益 0 = 力量 +1"
        );
        // 1:先把基准血量设成 40,加 25%(10)后变成 50,血也跟着补满
        let mut r = run(3);
        r.debug_room("enemy jaw_worm").expect("打一场");
        {
            let c = r.combat_mut().unwrap();
            c.enemies[0].hp = 40;
            c.enemies[0].max_hp = 40;
        }
        r.apply_burning_elite_buff(1);
        let c = r.combat().unwrap();
        assert_eq!(c.enemies[0].max_hp, 50, "增益 1 = 上限 +25%");
        assert_eq!(c.enemies[0].hp, 50, "血量一起补上");
        let c = buff_case(2);
        assert_eq!(
            c.enemies[0].statuses.get(Status::Metallicize),
            innate(&c.enemies[0], Status::Metallicize) + 4,
            "增益 2 = 金属化 4"
        );
        let c = buff_case(3);
        assert_eq!(
            c.enemies[0].statuses.get(Status::Regenerate),
            innate(&c.enemies[0], Status::Regenerate) + 3,
            "增益 3 = 再生 3"
        );
    }

    /// 通过地图进燃烧精英:标记与增益都要生效(seed 3 的增益是 0)
    #[test]
    fn entering_the_burning_node_carries_its_buff() {
        let mut r = run(3);
        assert_eq!(r.map.burning_buff, 0);
        r.debug_room("burning").expect("跳到燃烧精英");
        let c = r.combat().expect("燃烧精英该开打");
        assert!(c.burning, "这一场要标成燃烧精英");
        for e in &c.enemies {
            let innate = e.def.innate.iter().find(|(k, _)| *k == Status::Strength).map(|(_, n)| *n).unwrap_or(0);
            assert!(e.statuses.get(Status::Strength) > innate, "地图掷出的增益要落到敌人身上");
        }
    }

    /// 普通精英不带燃烧标记,也就没有增益
    #[test]
    fn a_plain_elite_gets_no_burning_flag() {
        let mut r = run(1);
        let burning = r.map.burning_node().expect("第一章有燃烧精英");
        assert!(r.map.node(burning).burning, "标记节点的 burning 应为真");
        assert_eq!(
            r.map.nodes.iter().filter(|n| n.burning).count(),
            1,
            "整张图只标一个燃烧精英"
        );
        let plain = r
            .map
            .nodes
            .iter()
            .position(|n| n.kind == NodeKind::Elite && !n.burning);
        if let Some(p) = plain {
            let enc = r.pick_encounter(EnemyKind::Elite);
            r.start_combat(enc, r.map.node(p).burning);
            let c = r.combat().expect("该开打");
            assert!(!c.burning, "普通精英不该标成燃烧");
            // 没有增益:力量不该超过它自带的
            for e in &c.enemies {
                let innate = e.def.innate.iter().find(|(k, _)| *k == Status::Strength).map(|(_, n)| *n).unwrap_or(0);
                assert_eq!(e.statuses.get(Status::Strength), innate, "普通精英没有额外力量");
            }
        }
    }

    /// 打通燃烧精英掉绿钥匙;普通精英不掉
    #[test]
    fn burning_elite_drops_the_emerald_key() {
        let mut r = run(3);
        r.debug_room("burning").expect("跳到燃烧精英");
        r.debug_win_battle();
        settle(&mut r);
        assert_eq!(r.screen, Screen::Reward);
        assert!(
            r.reward_slots().contains(&RewardSlot::EmeraldKey),
            "燃烧精英的奖励里要有绿钥匙"
        );
        assert!(!r.keys.emerald, "还没拿之前不该有绿钥匙");
        // 取走它
        let idx = r
            .reward_slots()
            .iter()
            .position(|s| *s == RewardSlot::EmeraldKey)
            .unwrap();
        r.reward.as_mut().unwrap().index = idx;
        r.reward_take().expect("拿绿钥匙");
        assert!(r.keys.emerald, "拿完要有绿钥匙");
        assert!(
            !r.reward_slots().contains(&RewardSlot::EmeraldKey),
            "拿过就不该再出现"
        );
    }

    /// 营火回忆拿红钥匙:拿过一次后不再出现
    #[test]
    fn rest_recall_grants_the_ruby_key_once() {
        let mut r = run(1);
        assert!(r.can_recall(), "一开始没有红钥匙");
        assert_eq!(r.rest_option_count(), 3);
        r.rest_recall();
        assert!(r.keys.ruby, "回忆后要有红钥匙");
        assert!(!r.can_recall(), "有了就不再提供回忆");
        assert_eq!(r.rest_option_count(), 2);
    }

    /// 宝箱拿蓝钥匙:拿走钥匙就没了那件遗物,而且每个箱子只提供一次机会
    #[test]
    fn chest_sapphire_key_forfeits_the_relic() {
        let mut r = run(2);
        r.debug_room("treasure").expect("开宝箱房");
        assert!(r.chest_sapphire_available(), "还没有蓝钥匙时应该能拿");
        let before_relics = r.player.relics.len();
        r.take_sapphire_key();
        assert!(r.keys.sapphire, "拿完要有蓝钥匙");
        assert_eq!(r.player.relics.len(), before_relics, "拿钥匙不给遗物");
        assert!(r.treasure.is_none(), "箱子已开");
        assert!(!r.chest_sapphire_available(), "已经有一把就不再提供");
        // 再开一个箱子:不该再给第二把
        r.debug_room("treasure").expect("再开一个宝箱房");
        assert!(!r.chest_sapphire_available(), "已有蓝钥匙就不再提供");
    }

    /// 俄罗斯套娃:接下来两个非 Boss 宝箱各多给一件,之后就只剩原有的那件;
    /// 剩余次数会进存档
    #[test]
    fn matryoshka_adds_an_extra_relic_to_two_chests() {
        let mut r = run(7);
        r.debug_add_relic("matryoshka").expect("装上套娃");
        let base = r.player.relics.len();
        r.debug_room("treasure").expect("开宝箱房 1");
        r.take_treasure();
        assert_eq!(r.player.relics.len(), base + 2, "第一个箱子给两件");
        r.debug_room("treasure").expect("开宝箱房 2");
        r.take_treasure();
        assert_eq!(r.player.relics.len(), base + 4, "第二个箱子给两件");
        assert!(r.save_text().contains("chest_extras=0,"), "两次用光");
        r.debug_room("treasure").expect("开宝箱房 3");
        r.take_treasure();
        assert_eq!(r.player.relics.len(), base + 5, "第三个箱子只给一件");
    }

    /// 血祭匕首的永久成长会写回牌组原件(击杀非随从时 +3)
    #[test]
    fn ritual_dagger_growth_is_written_back_to_the_deck() {
        let mut r = run(31);
        let def = crate::core::events::event_card("ritual_dagger").expect("血祭匕首在事件牌里");
        r.player.deck = vec![CardInstance::new(def)];
        r.debug_room("enemy jaw_worm").expect("打一场");
        let c = r.combat_mut().expect("该开打");
        let pos = c
            .hand
            .iter()
            .position(|x| x.def.id == "ritual_dagger")
            .or_else(|| c.draw.iter().position(|x| x.def.id == "ritual_dagger"))
            .expect("匕首要在手或抽牌堆里");
        let from_hand = c.hand.iter().any(|x| x.def.id == "ritual_dagger");
        if !from_hand {
            let card = c.draw.remove(pos);
            c.hand.push(card);
        }
        c.enemies[0].hp = 5;
        c.enemies[0].max_hp = 5;
        let idx = c
            .hand
            .iter()
            .position(|x| x.def.id == "ritual_dagger")
            .unwrap();
        c.play_card(idx, Some(0)).unwrap();
        assert!(c.enemies[0].dead(), "15 伤打死 5 血的虫子");
        r.sync_combat();
        assert_eq!(r.player.deck[0].bonus, 3, "击杀要写回牌组原件");
    }

    /// 第三幕未知房只从第三幕事件池(外加神龛/一次性事件)抽;清空神龛池后
    /// 走的必然是本章池,七个第三幕事件全都要能抽到.
    #[test]
    fn act3_unknown_rooms_pull_from_the_act3_pool() {
        let mut r = run(7);
        r.act = 3;
        r.event_pool = act_event_pool(3);
        r.shrine_pool = act_shrine_pool(3);
        r.one_time_pool = ONE_TIME_EVENTS.to_vec();
        let mut got: Vec<&str> = Vec::new();
        for _ in 0..60 {
            if let Some(ev) = r.debug_event_picks(1)[0] {
                got.push(ev);
            }
        }
        let act3: Vec<&str> = ACT3_EVENTS.to_vec();
        let shrines: Vec<&str> = ACT23_SHRINES.to_vec();
        let onetime: Vec<&str> = ONE_TIME_EVENTS.to_vec();
        for ev in &got {
            assert!(
                act3.contains(ev) || shrines.contains(ev) || onetime.contains(ev),
                "第三幕未知房抽到外池事件 {ev}"
            );
        }
        // 清空神龛/一次性池,强制走本章池:可抽到的第三幕事件要全部抽到
        r.event_pool = act_event_pool(3);
        r.shrine_pool.clear();
        r.one_time_pool.clear();
        let spawnable: Vec<&str> = act3
            .iter()
            .copied()
            .filter(|id| r.event_can_spawn(id))
            .collect();
        assert!(spawnable.len() >= 6, "第三幕事件池太小: {spawnable:?}");
        let mut normal: Vec<&str> = Vec::new();
        for _ in 0..spawnable.len() {
            if let Some(ev) = r.debug_event_picks(1)[0] {
                normal.push(ev);
            }
        }
        assert_eq!(normal.len(), spawnable.len(), "第三幕本章池没抽干净");
        for ev in spawnable {
            assert!(normal.contains(&ev), "第三幕事件 {ev} 一次都没抽到");
        }
    }
}




#[cfg(test)]
mod ascension_tests {
    //! 飞升难度:等级表各条在 run 层的落地断言(硬约束:A0 逐字节不变)
    use super::*;

    fn run_asc(asc: u32) -> Run {
        let ch = roster::find("ironclad").unwrap();
        Run::new_for_asc(7, ch, asc).unwrap()
    }

    /// A0 与老行为一致:满血满槽、无诅咒、等级记 0
    #[test]
    fn a0_run_matches_the_old_baseline() {
        let r = Run::new(7);
        assert_eq!(r.ascension, 0);
        assert_eq!(r.player.hp, 80);
        assert_eq!(r.player.max_hp, 80);
        assert_eq!(r.player.gold, 99);
        assert_eq!(r.player.deck.len(), 10);
        assert_eq!(r.player.potions.len(), POTION_SLOTS);
        assert!(r.save_text().contains("ascension=0\n"));
    }

    /// A14 先降上限,再按降过的上限做 A6 的 10% 扣血
    #[test]
    fn a14_lowers_max_hp_then_a6_damages() {
        // 飞升 5 仍满血
        let r = run_asc(5);
        assert_eq!((r.player.hp, r.player.max_hp), (80, 80));
        // 飞升 6:80 的 90% = 72
        let r = run_asc(6);
        assert_eq!((r.player.hp, r.player.max_hp), (72, 80));
        // 飞升 14:先 -5 到 75,再 90% 四舍五入 = 68(75*0.9=67.5 -> 68)
        let r = run_asc(14);
        assert_eq!((r.player.hp, r.player.max_hp), (68, 75));
        // 13 只降血不降上限
        let r = run_asc(13);
        assert_eq!((r.player.hp, r.player.max_hp), (72, 80));
    }

    /// A10 开局带一张飞升者之灾
    #[test]
    fn a10_starts_with_ascenders_bane() {
        assert_eq!(run_asc(9).player.deck.len(), 10);
        let r = run_asc(10);
        assert_eq!(r.player.deck.len(), 11);
        assert!(r.player.deck.iter().any(|c| c.def.id == "ascenders_bane"));
    }

    /// A11 少一个药水槽
    #[test]
    fn a11_fewer_potion_slots() {
        assert_eq!(run_asc(10).player.potions.len(), 3);
        assert_eq!(run_asc(11).player.potions.len(), 2);
        assert_eq!(run_asc(20).player.potions.len(), 2);
    }

    /// A5 打完 Boss 切幕:飞升 5 以下回满,5+ 只补缺血的 75%
    #[test]
    fn a5_boss_transition_heals_less() {
        let mut r = run_asc(4);
        r.player.hp = 40;
        r.begin_act();
        assert_eq!(r.player.hp, r.player.max_hp, "A4 回满");

        let mut r = run_asc(5);
        r.player.hp = 40; // 缺血 40,补 75% = 30
        r.begin_act();
        assert_eq!(r.player.hp, 70);
    }

    /// A12 卡牌奖励升级率在第二/三幕减半
    #[test]
    fn a12_halves_card_upgrade_chance() {
        assert_eq!(reward_upgrade_chance(2, 11), 0.25);
        assert_eq!(reward_upgrade_chance(2, 12), 0.125);
        assert_eq!(reward_upgrade_chance(3, 12), 0.25);
        // 第一幕本来就不升级,减半后仍是 0
        assert_eq!(reward_upgrade_chance(1, 20), 0.0);
    }

    /// A16 商店一律 +10%(在遗物折扣之前)
    #[test]
    fn a16_shop_prices_are_higher() {
        assert_eq!(run_asc(15).discount(100), 100);
        assert_eq!(run_asc(16).discount(100), 110);
    }

    /// 存档带飞升等级,读回来还是同一级
    #[test]
    fn save_roundtrips_the_ascension_level() {
        let r = run_asc(17);
        let text = r.save_text();
        assert!(text.contains("ascension=17\n"));
        let back = Run::from_save(&text).unwrap();
        assert_eq!(back.ascension, 17);
        assert_eq!(back.player.deck.len(), r.player.deck.len());
    }

    /// A20 第三章要连打两个 Boss:第一个倒下后接着第二个,不给奖励屏
    #[test]
    fn a20_act3_has_two_bosses() {
        let mut r = run_asc(20);
        r.act = 3;
        r.boss_enc = enemies::resolve("time_eater");
        r.boss2_enc = enemies::resolve("donu_and_deca");
        r.player.hp = 200;
        r.player.max_hp = 200;
        r.debug_start_combat(r.boss_enc);
        assert_eq!(r.screen, Screen::Combat);
        // 第一个 Boss 倒下
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
        assert_eq!(r.screen, Screen::Combat, "要接着打第二个 Boss");
        assert_eq!(r.combat().unwrap().encounter_id, "donu_and_deca");
        // 第二个也倒下:这时没有钥匙,本局胜利
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
        assert_eq!(r.screen, Screen::Victory);
    }

    /// A0 的第三章 Boss 只有一场
    #[test]
    fn a0_act3_has_one_boss() {
        let mut r = run_asc(0);
        r.act = 3;
        r.boss_enc = enemies::resolve("time_eater");
        r.boss2_enc = enemies::resolve("donu_and_deca");
        r.player.hp = 200;
        r.player.max_hp = 200;
        r.debug_start_combat(r.boss_enc);
        {
            let c = r.combat.as_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = Phase::Won;
        }
        r.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            r.tick_win_hold();
        }
        assert_eq!(r.screen, Screen::Victory, "A0 打完一个 Boss 就收尾");
    }
}
