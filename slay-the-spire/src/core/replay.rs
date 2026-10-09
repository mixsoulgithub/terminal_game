//! 无头脚本化运行:同一颗种子 + 同一套策略,把每一步的状态落成一行 JSON,
//! 供 tools/e2e_diff.ts 与参考实现(tools/replay_ref.ts)逐行对拍.
//!
//! 用法(CLI 见 main.rs 的 --replay):`spire --replay <seed> [<script>]`
//!
//! 路径脚本是若干行 `键 值`(空格分隔,# 开头是注释):
//!     neow 1        第几个 Neow 祝福(0 起)
//!     reward take   奖励界面全拿(take)还是全跳过(skip)
//!     card 0        卡牌三选一拿第几张
//!     event 0       事件选第几个选项
//!     rest rest     营火休息(rest)还是打铁(smith);给 recall 就是拿红钥匙
//!     shop skip     商店什么都不买
//!     steps 400     最多走多少步
//!     acts 1        最多走到第几幕(默认 1,即停在第一幕 Boss 奖励界面)
//!     act 3         调试钩子:开局直接切到第 3 幕开头(走的是幕切换的掷点)
//!     floor 7       调试钩子:切幕后先静默走到本章第 7 行,再从那里开始输出
//!     keys on       钥匙模式:开的话营火优先回忆、宝箱优先拿蓝钥匙
//!     keys all      调试钩子:三把钥匙直接到手(用来进第四章)
//!     hp 9999       调试钩子:把生命与上限设成这个数(第三/四幕才活得到 Boss)
//!     deck strong   调试钩子:牌组换成 10 张强化重锤(最笨的策略也打得出 32/回合)
//!     deck ramp     调试钩子:牌组换成 10 张强化狂暴(每打一次自己 +8,越打越重)
//!     deck burst    调试钩子:1 张强化重刃 + 24 张强化火上浇油(力量越堆越高,量心脏的无敌)
//!     smart off     智能打牌:开的话按 smart_play 的策略出牌(默认关,act1 序列不变)
//!     asc 20        飞升等级(0-20,默认 0;A0 与之前逐字节一致)
//!
//! 环境变量 SPIRE_TRACE=1 会把智能打牌的每一次出牌(手牌/敌人血/意图/选择)打到
//! stderr:tools/replay_ref.ts 有同一份,两边对着看就能定出分叉在第几回合.\n
//!
//! `act`/`floor`/`keys all` 是给"第三/四幕 seed 级对拍"用的调试钩子:
//! 参考侧(tools/replay_ref.ts)用同一套语义驱动(切幕走它自己的 actTransition),
//! 两边的掷点位置才一致.
//!
//! 输出的每一行字段:
//!   step   从 0 开始的步号
//!   kind   这一步在哪个界面做了什么(init/neow/move/fight/reward/rest/
//!          shop/treasure/event/pick/end)
//!   ...    该界面的内容(遭遇阵容、奖励条目、事件选项、货架……)
//!   s      这一步结束后的局面:hp/max_hp/gold/act/row/deck/relics/potions
//!
//! 牌堆记号与存档一致:`strike`、`strike+`、`strike+2`.
//! 参考实现的 id 是大写,对拍器(不在这里)负责大小写与少数别名归一.

use crate::core::card::{CardInstance, CardType, Cost};
use crate::core::combat::Phase;
use crate::core::enemy::EnemyKind;
use crate::core::map::{NodeKind, COLS, FLOORS};
use crate::core::relics::RelicTier;
use crate::core::roster;
use crate::core::run::{ChestSize, RestOption, RewardSlot, Run, Screen, ShopItem};
use crate::rng::seed_to_string;

/// 对拍用的路径脚本.默认值 = 奖励全拿、营火休息、商店不买东西.
#[derive(Clone, Debug)]
pub struct Policy {
    pub neow: usize,
    pub reward_take: bool,
    pub card: usize,
    pub event: usize,
    pub rest: RestOption,
    pub shop_skip: bool,
    pub max_steps: usize,
    /// 最多走到第几幕:在第 acts 幕的 Boss 奖励界面停下(默认 1,即只走第一幕).
    /// 设成 4 就一直走到第四章(集齐三把钥匙才会开门).
    pub acts: u32,
    /// 钥匙模式:营火优先"回忆"拿红钥匙,宝箱优先拿蓝钥匙,燃烧精英的绿钥匙照常拿
    pub keys: bool,
    /// 智能打牌(默认关).开了才走 smart_play,不开还是 auto_play,
    /// 所以 act1 的既有 fixture 逐位不变.
    pub smart: bool,
    /// 调试钩子:开局直接切到第几幕(默认 1,即不切).`act 3` 就是切到第三幕开头.
    /// 切幕走的是 begin_act,参考侧用同样的切幕驱动,两边的掷点位置一致.
    pub act: u32,
    /// 调试钩子:切到第 act 幕后先静默走到本章第几行(row),再从那一行开始输出.
    /// 默认 0(不静默).`floor 7` 就是先走到第 7 行.
    pub floor: u32,
    /// 调试钩子:三把钥匙直接到手(默认关).`keys all` 打开它,
    /// 用来开第三幕 Boss 后面的门进第四章.
    pub keys_all: bool,
    /// 调试钩子:开局把生命(与上限)设成这个值(默认不改).`hp 999` 打开它,
    /// 好让起手牌组也能在第三/四幕活着走到 Boss,对拍才有料.
    pub hp: Option<i32>,
    /// 调试钩子:把牌组整个换掉(默认不改).`deck strong` = 10 张强化重锤
    /// (只有 3 点能量时最笨的策略也打得出 32/回合),`deck ramp` = 10 张强化狂暴
    /// (每打一次这张牌自己 +8,越打越重,第三幕那些血厚的遭遇才破得开).
    pub deck: Deck,
    /// 飞升等级(0-20,默认 0).`asc 20` 就是 A20;0 时与既有 A0 fixture 逐字节一致.
    pub asc: u32,
}

/// 调试钩子 `deck ...` 能换的那几套牌(默认不改牌组).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deck {
    /// 不换牌组,用当前牌组
    Keep,
    /// 10 张强化重锤(bludgeon+):3 点能量下每回合 32 点
    Strong,
    /// 10 张强化狂暴(rampage+):每打一次自己 +8,越打越重
    Ramp,
    /// 1 张强化重刃 + 24 张强化火上浇油:力量越堆越高,重刃那一刀会顶到
    /// 心脏的"无敌"(一回合最多掉 300)
    Burst,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            neow: 1,
            reward_take: true,
            card: 0,
            event: 0,
            rest: RestOption::Rest,
            shop_skip: true,
            max_steps: 400,
            acts: 1,
            keys: false,
            smart: false,
            act: 1,
            floor: 0,
            keys_all: false,
            hp: None,
            deck: Deck::Keep,
            asc: 0,
        }
    }
}

impl Policy {
    /// 逐行解析路径脚本;空行与 `#` 注释跳过.
    pub fn parse(text: &str) -> Result<Policy, String> {
        let mut p = Policy::default();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let key = it.next().unwrap_or("");
            let val = it.next().unwrap_or("");
            let num = |v: &str| {
                v.parse::<usize>()
                    .map_err(|_| format!("第 {} 行:{key} 需要一个数字,给的是 {v}", i + 1))
            };
            match key {
                "neow" => p.neow = num(val)?,
                "card" => p.card = num(val)?,
                "event" => p.event = num(val)?,
                "steps" => p.max_steps = num(val)?,
                "acts" => p.acts = num(val)?.max(1) as u32,
                "act" => p.act = num(val)?.max(1) as u32,
                "floor" => p.floor = num(val)? as u32,
                "hp" => p.hp = Some(num(val)? as i32),
                "deck" => {
                    p.deck = match val {
                        "strong" => Deck::Strong,
                        "ramp" => Deck::Ramp,
                        "burst" => Deck::Burst,
                        "keep" => Deck::Keep,
                        _ => return Err(format!("第 {} 行:deck 只能是 strong/ramp/burst/keep", i + 1)),
                    }
                }
                "keys" => {
                    p.keys = match val {
                        "on" => true,
                        "off" => false,
                        "all" => {
                            p.keys_all = true;
                            true
                        }
                        _ => return Err(format!("第 {} 行:keys 只能是 on/off/all", i + 1)),
                    }
                }
                "smart" => {
                    p.smart = match val {
                        "on" => true,
                        "off" => false,
                        _ => return Err(format!("第 {} 行:smart 只能是 on/off", i + 1)),
                    }
                }
                "reward" => {
                    p.reward_take = match val {
                        "take" => true,
                        "skip" => false,
                        _ => return Err(format!("第 {} 行:reward 只能是 take/skip", i + 1)),
                    }
                }
                "rest" => {
                    p.rest = match val {
                        "rest" => RestOption::Rest,
                        "smith" => RestOption::Smith,
                        _ => return Err(format!("第 {} 行:rest 只能是 rest/smith", i + 1)),
                    }
                }
                "asc" => p.asc = crate::core::ascension::clamp(num(val)? as i64),
                "shop" => {
                    p.shop_skip = match val {
                        "skip" => true,
                        "buy" => false,
                        _ => return Err(format!("第 {} 行:shop 只能是 skip/buy", i + 1)),
                    }
                }
                other => return Err(format!("第 {} 行:不认识 {other}", i + 1)),
            }
        }
        Ok(p)
    }
}

// ---- JSON 拼装(只有这一处需要,不引依赖) ----

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn js(s: &str) -> String {
    format!("\"{}\"", esc(s))
}

/// 牌堆记号:id、id+、id+2
fn card_token(c: &CardInstance) -> String {
    if c.plus > 1 {
        js(&format!("{}+{}", c.def.id, c.plus))
    } else if c.upgraded {
        js(&format!("{}+", c.def.id))
    } else {
        js(c.def.id)
    }
}

fn node_kind_name(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Monster => "monster",
        NodeKind::Elite => "elite",
        NodeKind::Event => "event",
        NodeKind::Rest => "rest",
        NodeKind::Shop => "shop",
        NodeKind::Treasure => "treasure",
        NodeKind::Boss => "boss",
    }
}

/// 事件的选项:从策略给的下标往后找第一个"当前能选"的;都没有就退到第一项.
fn first_available_choice(run: &Run, preferred: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    for i in preferred..count {
        if run.event_choice_available(i) {
            return i;
        }
    }
    for i in 0..count {
        if run.event_choice_available(i) {
            return i;
        }
    }
    count - 1
}

fn chest_size_name(s: ChestSize) -> &'static str {
    match s {
        ChestSize::Small => "small",
        ChestSize::Medium => "medium",
        ChestSize::Large => "large",
    }
}

fn relic_tier_name(t: RelicTier) -> &'static str {
    match t {
        RelicTier::Starter => "starter",
        RelicTier::Common => "common",
        RelicTier::Uncommon => "uncommon",
        RelicTier::Rare => "rare",
        RelicTier::Boss => "boss",
        RelicTier::Shop => "shop",
        RelicTier::Event => "event",
        RelicTier::Special => "special",
    }
}

fn room_kind_after(run: &Run) -> String {
    match run.screen {
        Screen::Combat => match run.combat().map(|c| c.kind) {
            Some(EnemyKind::Elite) => "elite".to_string(),
            Some(EnemyKind::Boss) => "boss".to_string(),
            _ => "monster".to_string(),
        },
        Screen::Rest => "rest".to_string(),
        Screen::Shop => "shop".to_string(),
        Screen::Treasure => "treasure".to_string(),
        Screen::Event => "event".to_string(),
        Screen::Map => "empty".to_string(),
        other => other.name().to_lowercase(),
    }
}

/// 这一步结束后的局面
fn state_json(run: &Run) -> String {
    let deck: Vec<String> = run.player.deck.iter().map(card_token).collect();
    let relics: Vec<String> = run.player.relics.iter().map(|r| js(r.id)).collect();
    let potions: Vec<String> = run
        .player
        .potions
        .iter()
        .map(|p| p.map(|d| js(d.id)).unwrap_or_else(|| "null".to_string()))
        .collect();
    // row 是本章地图上的行号;还没上路(null 位置)时是 0(与参考实现一致)
    let row = if run.pos.is_some() { run.floor_reached } else { 0 };
    format!(
        "{{\"hp\":{},\"max_hp\":{},\"gold\":{},\"act\":{},\"row\":{},\"deck\":[{}],\"relics\":[{}],\"potions\":[{}]}}",
        run.player.hp,
        run.player.max_hp,
        run.player.gold,
        run.act,
        row,
        deck.join(","),
        relics.join(","),
        potions.join(",")
    )
}

/// 整张地图:16 行(第 15 行是 Boss),每行 7 格;空格是 null.
fn map_json(run: &Run) -> String {
    let mut rows: Vec<String> = Vec::new();
    for floor in 0..=FLOORS {
        let mut cells: Vec<String> = Vec::new();
        for col in 0..COLS {
            let idx = run
                .map
                .row(floor)
                .iter()
                .copied()
                .find(|&i| run.map.node(i).col == col);
            match idx {
                None => cells.push("null".to_string()),
                Some(i) => {
                    let n = run.map.node(i);
                    let edges: Vec<String> = n
                        .next
                        .iter()
                        .map(|&j| run.map.node(j).col.to_string())
                        .collect();
                    cells.push(format!(
                        "{{\"k\":{},\"b\":{},\"e\":[{}]}}",
                        js(node_kind_name(n.kind)),
                        n.burning as u8,
                        edges.join(",")
                    ));
                }
            }
        }
        rows.push(format!("[{}]", cells.join(",")));
    }
    format!("[{}]", rows.join(","))
}

fn line(step: usize, kind: &str, payload: &str, state: &str) -> String {
    if payload.is_empty() {
        format!("{{\"step\":{step},\"kind\":{},\"s\":{state}}}", js(kind))
    } else {
        format!(
            "{{\"step\":{step},\"kind\":{},{payload},\"s\":{state}}}",
            js(kind)
        )
    }
}

// ---- 战斗:照参考实现的 autoWinCombat ----

/// 手牌里第一张"基础费用在 0..=能量 的攻击牌"打向第一只活着的敌人,
/// 没有就结束回合;直到战斗结束.与参考实现 autoWinCombat 的规则一致.
fn auto_play(run: &mut Run) -> Result<(), String> {
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 5000 {
            return Err("战斗打不完(超过 5000 次操作)".to_string());
        }
        let Some(c) = run.combat_mut() else { break };
        if c.choice.is_some() {
            // 参考实现导出器对"选 N 张"的请求只选前 min 张;本作 need==1 才是
            // 必须选一张,其余(0/不限张数)都是可选,跟参考实现一样一张不选.
            let need = c.choice.as_ref().map(|ch| ch.need).unwrap_or(0);
            for _ in 0..if need == 1 { 1 } else { 0 } {
                let Some((i, _)) = c.choice_candidates().first().copied() else {
                    break;
                };
                let _ = c.choose(i);
            }
            c.finish_choice();
            run.sync_combat();
            continue;
        }
        if c.phase != Phase::PlayerTurn {
            break;
        }
        let target = c.first_alive();
        // 判据是牌面基础费用(升级/动态降费都不算),与参考实现的 autoWinCombat 一致
        let pick = (0..c.hand.len()).find(|&i| {
            c.hand[i].kind() == CardType::Attack
                && matches!(c.hand[i].def.cost, crate::core::card::Cost::Fixed(n) if (n as i32) <= c.energy)
        });
        match (pick, target) {
            (Some(i), Some(t)) => {
                if c.play_card(i, Some(t)).is_err() {
                    c.end_turn();
                }
            }
            _ => c.end_turn(),
        }
        run.sync_combat();
        if run.screen == Screen::Death {
            break;
        }
    }
    // 赢了要在战场上停 VICTORY_HOLD 帧才结算
    if run.holding_victory() {
        for _ in 0..=Run::VICTORY_HOLD {
            run.tick_win_hold();
            if !run.holding_victory() {
                break;
            }
        }
    }
    Ok(())
}

// ---- 智能打牌(acts.script 的 smart on 才走这条;默认关) ----

/// 血最少的活敌人(平手取下标小的);攻击与指向敌人的药水都用它当目标
fn lowest_hp_enemy(c: &crate::core::combat::Combat) -> Option<usize> {
    (0..c.enemies.len())
        .filter(|&i| c.enemies[i].alive())
        .min_by_key(|&i| (c.enemies[i].hp, i))
}

/// 按优先级挑一张打得起的牌:
///   1. 有敌人快死了(血量 <= 1,或掉到四分之一以下)而且自己不会被打死时先补刀,攻击优先;
///   2. 即将被斩杀或意图总伤 > 格挡时,先用技能补防;
///   3. 能力牌尽早铺开;
///   4. 其余打攻击牌,费用从低到高;
///   5. 再不济打技能(抽牌/降费之类).
/// 同序实现见 tools/replay_ref.ts 的 pickSmartCard,两边必须一字不差.
fn pick_smart_card(
    c: &crate::core::combat::Combat,
    about_to_die: bool,
    threatened: bool,
    can_kill: bool,
) -> Option<usize> {
    // 判据是牌面基础费用(升级/动态降费都不算),与 auto_play 一致
    let affordable = |i: usize| matches!(c.hand[i].def.cost, Cost::Fixed(n) if (n as i32) <= c.energy);
    let base_cost = |i: usize| match c.hand[i].def.cost {
        Cost::Fixed(n) => n as i32,
        _ => i32::MAX,
    };
    let group = |kind: CardType| -> Option<usize> {
        (0..c.hand.len())
            .filter(|&i| c.hand[i].kind() == kind && affordable(i))
            .min_by_key(|&i| (base_cost(i), i))
    };
    if can_kill && !about_to_die {
        if let Some(i) = group(CardType::Attack) {
            return Some(i);
        }
    }
    if about_to_die || threatened {
        if let Some(i) = group(CardType::Skill) {
            return Some(i);
        }
    }
    if let Some(i) = group(CardType::Power) {
        return Some(i);
    }
    if let Some(i) = group(CardType::Attack) {
        return Some(i);
    }
    if let Some(i) = group(CardType::Skill) {
        return Some(i);
    }
    None
}

/// 危险时按格子顺序找第一瓶能喝的药水;喝到就返回 true.
/// 指向敌人的药水打血最少的活敌人.
fn try_drink_once(run: &mut Run) -> bool {
    let n = run.player.potions.len();
    for slot in 0..n {
        let Some(Some(def)) = run.player.potions.get(slot).copied() else {
            continue;
        };
        let target = if def.target.needs_enemy() {
            match run.combat().and_then(lowest_hp_enemy) {
                Some(t) => Some(t),
                None => continue,
            }
        } else {
            None
        };
        if run.quaff_potion(slot, target).is_ok() {
            return true;
        }
    }
    false
}

/// 智能打牌:与参考实现的 smartPlay 同规则(见 tools/replay_ref.ts).
fn smart_play(run: &mut Run) -> Result<(), String> {
    let mut guard = 0;
    // 已经试过药水的回合号(0 = 还没试过);每回合最多试一次
    let mut potion_turn: u32 = 0;
    loop {
        guard += 1;
        if guard > 20000 {
            return Err("智能战斗打不完(超过 20000 次操作)".to_string());
        }
        // 战斗内挂起的选牌
        let had_choice = {
            let Some(c) = run.combat_mut() else { break };
            if c.choice.is_some() {
                // 参考实现导出器对"选 N 张"的请求只选前 min 张;本作 need==1
                // 才是"必须选一张",其余(0/不限张数)都是可选,跟参考实现一样一张不选.
                let need = c.choice.as_ref().map(|ch| ch.need).unwrap_or(0);
                for _ in 0..if need == 1 { 1 } else { 0 } {
                    let Some((i, _)) = c.choice_candidates().first().copied() else {
                        break;
                    };
                    let _ = c.choose(i);
                }
                c.finish_choice();
                true
            } else {
                false
            }
        };
        if had_choice {
            run.sync_combat();
            continue;
        }
        // 快照本回合的局势(先读后写,避免同时借用 run 的 combat 与一局)
        let (about_to_die, dangerous, low_hp, turn, pick, target) = {
            let Some(c) = run.combat() else { break };
            if c.phase != Phase::PlayerTurn {
                break;
            }
            let incoming: i32 = (0..c.enemies.len())
                .filter(|&i| c.enemies[i].alive() && c.enemies[i].intent().attacks())
                .map(|i| {
                    let (d, t) = c.predicted_damage(i);
                    d * t as i32
                })
                .sum();
            let hp = c.player.hp;
            let max_hp = c.player.max_hp;
            let block = c.player.block;
            let about_to_die = incoming >= hp + block;
            // 来袭总伤已经够把血打空(还没算格挡)也算危险:该喝药水了
            let dangerous = incoming >= hp;
            let threatened = incoming > block;
            let low_hp = hp * 2 <= max_hp;
            // 有敌人快死了:血量 <= 1 这刀必死;掉到四分之一以下也先补刀,
            // 少一个活着的敌人就少一份来袭
            let can_kill = (0..c.enemies.len()).any(|i| {
                let e = &c.enemies[i];
                e.alive() && (e.hp <= 1 || e.hp * 4 <= e.max_hp)
            });
            let pick = pick_smart_card(c, about_to_die, threatened, can_kill);
            let target = lowest_hp_enemy(c);
            if std::env::var("SPIRE_TRACE").is_ok() {
                let hand: Vec<String> = c
                    .hand
                    .iter()
                    .map(|x| format!("{}{}(b{})", x.def.id, if x.upgraded { "+" } else { "" }, x.bonus))
                    .collect();
                let foes: Vec<String> = c
                    .enemies
                    .iter()
                    .map(|e| {
                        format!(
                            "{}:{}/{}b{}m{}{}",
                            e.def.id,
                            e.hp,
                            e.max_hp,
                            e.block,
                            e.def.moves[e.next_move].name,
                            if e.state.half_dead { "HALF" } else { "" }
                        )
                    })
                    .collect();
                eprintln!(
                    "TRACE t{} hp{} blk{} in{} atd{} danger{} cankill{} pick{:?} tgt{:?} hand[{}] foes[{}]",
                    c.turn,
                    c.player.hp,
                    c.player.block,
                    incoming,
                    about_to_die,
                    dangerous,
                    can_kill,
                    pick,
                    target,
                    hand.join(","),
                    foes.join(",")
                );
            }
            (about_to_die, dangerous, low_hp, c.turn, pick, target)
        };
        // 4) 危险或残血时先喝药水,每回合最多试一次
        if (about_to_die || dangerous || low_hp) && potion_turn != turn {
            if try_drink_once(run) {
                potion_turn = run.combat().map(|c| c.turn).unwrap_or(turn);
                run.sync_combat();
                if run.screen == Screen::Death {
                    break;
                }
                continue;
            }
            potion_turn = turn;
        }
        {
            let Some(c) = run.combat_mut() else { break };
            match (pick, target) {
                (Some(i), Some(t)) => {
                    if c.play_card(i, Some(t)).is_err() {
                        c.end_turn();
                    }
                }
                _ => c.end_turn(),
            }
        }
        run.sync_combat();
        if run.screen == Screen::Death {
            break;
        }
    }
    // 赢了要在战场上停 VICTORY_HOLD 帧才结算
    if run.holding_victory() {
        for _ in 0..=Run::VICTORY_HOLD {
            run.tick_win_hold();
            if !run.holding_victory() {
                break;
            }
        }
    }
    Ok(())
}

// ---- 奖励 ----

/// tag 的固定次序,两边一致
fn tag_rank(tag: &str) -> usize {
    match tag {
        "gold" => 0,
        "relic" => 1,
        "boss_relic" => 2,
        "potion" => 3,
        _ if tag.starts_with("card") => 4,
        "emerald_key" => 5,
        _ => 6,
    }
}

/// 奖励条目,顺序固定为 金币 → 遗物 → Boss 三选一 → 药水 → 卡牌 → 绿钥匙,
/// 这样两边条目顺序不同也能逐条对上.
fn reward_entries(run: &Run) -> Vec<String> {
    let Some(r) = run.reward.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if !r.gold_taken {
        out.push(format!("{{\"k\":\"gold\",\"n\":{}}}", r.gold));
    }
    if let Some(d) = r.relic.filter(|_| !r.relic_taken) {
        out.push(format!("{{\"k\":\"relic\",\"id\":{}}}", js(d.id)));
    }
    if !r.relic_taken {
        for d in &r.relic_choices {
            out.push(format!("{{\"k\":\"boss_relic\",\"id\":{}}}", js(d.id)));
        }
    }
    // 事件的奖励屏可能有好几瓶药水(实验室的三瓶),逐瓶列出
    for (i, d) in r.potions.iter().enumerate() {
        if !r.potion_taken.get(i).copied().unwrap_or(true) {
            out.push(format!("{{\"k\":\"potion\",\"id\":{}}}", js(d.id)));
        }
    }
    if !r.card_taken {
        for c in &r.cards {
            let up = if c.upgraded { "true" } else { "false" };
            out.push(format!(
                "{{\"k\":\"card\",\"id\":{},\"up\":{up}}}",
                js(c.def.id)
            ));
        }
    }
    if r.emerald_key && !run.keys.emerald {
        out.push("{\"k\":\"emerald_key\"}".to_string());
    }
    out
}

/// 按策略拿奖励,返回拿走的条目 tag(金币/遗物/药水/卡牌…).
fn take_rewards(run: &mut Run, policy: &Policy) -> Result<Vec<String>, String> {
    let mut taken: Vec<String> = Vec::new();
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 16 {
            return Err("奖励界面拿不完".to_string());
        }
        // 拿遗物时开的选牌界面(瓶装/浑天仪那类)要等选完才能接着拿
        if run.screen == Screen::Pick {
            break;
        }
        let slots = run.reward_slots();
        if slots.is_empty() {
            break;
        }
        // 金币 → 遗物 → Boss 三选一 → 药水 → 第 N 张卡 → 绿钥匙
        let want = slots
            .iter()
            .position(|s| matches!(s, RewardSlot::Gold))
            .map(|i| (i, "gold".to_string()))
            .or_else(|| {
                slots
                    .iter()
                    .position(|s| matches!(s, RewardSlot::Relic))
                    .map(|i| (i, "relic".to_string()))
            })
            .or_else(|| {
                slots
                    .iter()
                    .position(|s| matches!(s, RewardSlot::RelicChoice(_)))
                    .map(|i| (i, "boss_relic".to_string()))
            })
            .or_else(|| {
                slots
                    .iter()
                    .position(|s| matches!(s, RewardSlot::Potion(_)))
                    .map(|i| (i, "potion".to_string()))
            })
            .or_else(|| {
                slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| matches!(s, RewardSlot::Card(_)))
                    .nth(policy.card)
                    .map(|(i, _)| (i, format!("card{}", policy.card)))
            })
            .or_else(|| {
                slots
                    .iter()
                    .position(|s| matches!(s, RewardSlot::EmeraldKey))
                    .map(|i| (i, "emerald_key".to_string()))
            });
        let Some((idx, tag)) = want else { break };
        if let Some(r) = run.reward.as_mut() {
            r.index = idx;
        }
        match run.reward_take() {
            Ok(_) => taken.push(tag),
            // 药水格满了:把这一瓶标成已处理,免得卡死;其它槽位出错就直接收手
            Err(_) => {
                let slot = run.reward_slots().get(idx).copied();
                match (slot, run.reward.as_mut()) {
                    (Some(RewardSlot::Potion(i)), Some(r)) => {
                        if let Some(t) = r.potion_taken.get_mut(i) {
                            *t = true;
                        }
                    }
                    _ => break,
                }
            }
        }
    }
    taken.sort_by_key(|t| tag_rank(t));
    Ok(taken)
}

fn shop_items(run: &Run) -> Vec<String> {
    let Some(shop) = run.shop.as_ref() else {
        return Vec::new();
    };
    shop.items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let sold = shop.sold.get(i).copied().unwrap_or(false);
            match item {
                ShopItem::Card(c, p) => format!(
                    "{{\"k\":\"card\",\"id\":{},\"price\":{p},\"sold\":{sold}}}",
                    js(c.def.id)
                ),
                ShopItem::Relic(d, p) => format!(
                    "{{\"k\":\"relic\",\"id\":{},\"price\":{p},\"sold\":{sold}}}",
                    js(d.id)
                ),
                ShopItem::Potion(d, p) => format!(
                    "{{\"k\":\"potion\",\"id\":{},\"price\":{p},\"sold\":{sold}}}",
                    js(d.id)
                ),
                ShopItem::Remove(p) => {
                    format!("{{\"k\":\"remove\",\"price\":{p},\"sold\":{sold}}}")
                }
            }
        })
        .collect()
}

// ---- 走一整局 ----

/// 跑一局,返回 JSONL(每行一步).出错就返回 Err(而不是半截输出).
/// `floor n` 的静默前缀在这里裁掉(见 trim_to_row).
pub fn run_jsonl(seed: u64, policy: &Policy) -> Result<String, String> {
    let mut out = run_raw(seed, policy)?;
    if policy.floor > 0 {
        out = trim_to_row(out, policy.floor);
    }
    Ok(out.join("\n") + "\n")
}

/// 从某一行读出行号(每行末尾的 state 里有 `"row":N`)
fn line_row(l: &str) -> usize {
    match l.find("\"row\":") {
        None => 0,
        Some(i) => {
            let rest = &l[i + 6..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            rest[..end].parse().unwrap_or(0)
        }
    }
}

/// `floor n`:丢掉 init 之后直到第一次走到第 n 行为止的那些输出,
/// 再重新编号 step(init 仍是 0).最后那行 end 永远保留(半路阵亡也要有收尾).
fn trim_to_row(lines: Vec<String>, row: u32) -> Vec<String> {
    let row = row as usize;
    let last = lines.len().saturating_sub(1);
    let mut kept: Vec<String> = Vec::with_capacity(lines.len());
    for (i, l) in lines.into_iter().enumerate() {
        let keep = i == 0 || i == last || line_row(&l) >= row;
        if keep {
            kept.push(l);
        }
    }
    let mut out: Vec<String> = Vec::with_capacity(kept.len());
    for (step, l) in kept.into_iter().enumerate() {
        // 只改行首那个 "step":N
        if let Some(rest) = l.strip_prefix("{\"step\":") {
            let after = rest.find(',').map(|j| &rest[j..]).unwrap_or("");
            out.push(format!("{{\"step\":{step}{after}"));
        } else {
            out.push(l);
        }
    }
    out
}

/// 调试钩子 `deck ramp`:把牌组换成 10 张强化狂暴.
/// 狂暴每打一次这张牌自己 +8(只在本场战斗内),越打越重,
/// 第三幕那些血厚/会复生的遭遇(暗灵三连、颚虫三连)才破得开.
/// 纯状态,不掷点;参考侧(tools/replay_ref.ts)用同一套换上.
fn set_replay_deck(run: &mut Run, id: &str, n: usize) {
    let mut deck = Vec::with_capacity(n);
    for _ in 0..n {
        let mut c = crate::core::cards::card(id);
        c.upgrade();
        deck.push(c);
    }
    run.player.deck = deck;
}

/// 调试钩子 `deck burst`:1 张强化重刃 + 24 张强化火上浇油.
/// 火上浇油(能力牌)每打一张 +3 力量,重刃(力量算 5 次)那一刀会越砍越重,
/// 到后面一刀就顶到心脏的"无敌"(一回合最多掉 300 血)——
/// 第四幕那条尺子靠它量心脏的死亡律动与无敌.纯状态,不掷点.
fn set_burst_deck(run: &mut Run) {
    let mut deck = Vec::with_capacity(25);
    let mut blade = crate::core::cards::card("heavy_blade");
    blade.upgrade();
    deck.push(blade);
    for _ in 0..24 {
        let mut c = crate::core::cards::card("inflame");
        c.upgrade();
        deck.push(c);
    }
    run.player.deck = deck;
}

/// 处理一次选牌屏:候选非空就确认第 0 张(picker_confirm 在 remaining > 1 时会自己
/// 再开一次,外层循环接着选),候选为空就取消 —— 否则 picker_confirm 会报
/// "nothing selected",把整局打断(少数 seed 卡在奖励屏就是这个原因).
/// 返回候选数(pick 行的 candidates 字段).
fn confirm_pick(run: &mut Run) -> Result<usize, String> {
    let n = run.picker_candidates().len();
    if n == 0 {
        run.picker_cancel();
    } else {
        run.picker_confirm()
            .map_err(|e| format!("选牌确认失败:{e}"))?;
    }
    Ok(n)
}

/// 跑一局,返回没裁过的输出行.
fn run_raw(seed: u64, policy: &Policy) -> Result<Vec<String>, String> {
    let ch = roster::find("ironclad").expect("ironclad 必须在语料里");
    let mut run = Run::new_for_asc(seed, ch, policy.asc)?;
    // headless 对拍:便条事件不读也不写真实存档(参考实现没有持久化,按默认铁斩波)
    run.set_note_persist(false);
    run.open_neow();
    // 调试钩子:先给钥匙(会影响切幕时地图标不标燃烧精英),再切幕.
    if policy.keys_all {
        run.debug_grant_keys();
    }
    if let Some(hp) = policy.hp {
        run.debug_set_hp(hp);
    }
    match policy.deck {
        Deck::Keep => {}
        Deck::Strong => run.debug_set_strong_deck(),
        Deck::Ramp => set_replay_deck(&mut run, "rampage", 10),
        Deck::Burst => set_burst_deck(&mut run),
    }
    if policy.act > 1 {
        run.debug_jump_act(policy.act);
    }
    let mut out: Vec<String> = Vec::new();
    let mut step = 0usize;
    // 上一个战斗房间的类型:奖励界面靠它区分普通/精英/Boss
    let mut combat_kind = "monster".to_string();

    let init = format!(
        "\"seed_str\":{},\"boss\":{},\"map\":{}",
        js(&seed_to_string(seed)),
        js(run.boss_enc.id),
        map_json(&run)
    );
    out.push(line(step, "init", &init, &state_json(&run)));
    step += 1;

    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > policy.max_steps {
            return Err(format!("走了 {} 步还没到 Boss 奖励", policy.max_steps));
        }
        match run.screen {
            // ---- Neow 与事件 ----
            Screen::Event => {
                let is_neow = run
                    .event
                    .as_ref()
                    .is_some_and(|e| !e.neow_options.is_empty());
                if is_neow {
                    let opts: Vec<String> = run
                        .event
                        .as_ref()
                        .unwrap()
                        .neow_options
                        .iter()
                        .map(|o| format!("[{},{}]", js(o.bonus), js(o.drawback)))
                        .collect();
                    let pick = policy.neow;
                    run.choose_event(pick)
                        .map_err(|e| format!("Neow 第 {pick} 项选不了:{e}"))?;
                    let resolved = run.event.as_ref().map(|e| e.result.is_some()).unwrap_or(true);
                    let payload = format!("\"options\":[{}],\"pick\":{pick}", opts.join(","));
                    out.push(line(step, "neow", &payload, &state_json(&run)));
                    step += 1;
                    // 结算完就离开;祝福自己另开一屏(选牌/奖励)时留在那一屏
                    if resolved {
                        run.leave_event();
                    }
                    continue;
                }
                // 事件屏却没有事件(异常):回地图,别卡死
                if run.event.is_none() {
                    run.leave_event();
                    continue;
                }
                // 已经结算过的事件屏(选完牌/打完架回到这一屏):直接离开,不再发步
                if run.event.as_ref().is_some_and(|e| e.result.is_some()) {
                    run.leave_event();
                    continue;
                }
                let id = run.event.as_ref().map(|e| e.def.id).unwrap_or("invalid");
                let count = run.event_choice_count();
                let pick = first_available_choice(&run, policy.event, count);
                run.choose_event(pick)
                    .map_err(|e| format!("事件 {id} 第 {pick} 项选不了:{e}"))?;
                let resolved = run.event.as_ref().is_some_and(|e| e.result.is_some());
                out.push(line(
                    step,
                    "event",
                    &format!("\"id\":{},\"options\":{count},\"pick\":{pick}", js(id)),
                    &state_json(&run),
                ));
                step += 1;
                // 选项开出选牌窗口/战斗时停在那一屏(下一步再处理);单屏事件结算完就离开,
                // 多屏事件(结果文本还没定)继续留在事件屏
                if resolved && run.screen == Screen::Event {
                    run.leave_event();
                }
                continue;
            }

            // ---- 地图:走下一个节点 ----
            Screen::Map => {
                let Some(idx) = run.travel_options().first().copied() else {
                    return Err("地图上没有可走的下一步".to_string());
                };
                let from = run
                    .pos
                    .map(|p| format!("[{},{}]", run.map.node(p).col, run.map.node(p).floor));
                let to = format!("[{},{}]", run.map.node(idx).col, run.map.node(idx).floor);
                let node = node_kind_name(run.map.node(idx).kind).to_string();
                run.enter_node(idx)
                    .map_err(|e| format!("进不了节点 {idx}:{e}"))?;
                let resolved = room_kind_after(&run);
                if run.screen == Screen::Combat {
                    combat_kind = resolved.clone();
                }
                let mut payload = format!(
                    "\"from\":{},\"to\":{to},\"node\":{},\"resolved\":{}",
                    from.unwrap_or_else(|| "null".to_string()),
                    js(&node),
                    js(&resolved)
                );
                if let Some(c) = run.combat() {
                    let mons: Vec<String> = c
                        .enemies
                        .iter()
                        .map(|e| {
                            format!(
                                "{{\"id\":{},\"hp\":{},\"max_hp\":{}}}",
                                js(e.def.id),
                                e.hp,
                                e.max_hp
                            )
                        })
                        .collect();
                    payload.push_str(&format!(",\"monsters\":[{}]", mons.join(",")));
                }
                out.push(line(step, "move", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 战斗 ----
            Screen::Combat => {
                if policy.smart {
                    smart_play(&mut run)?;
                } else {
                    auto_play(&mut run)?;
                }
                out.push(line(step, "fight", "\"result\":\"end\"", &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 奖励 ----
            Screen::Reward => {
                let is_boss = run
                    .reward
                    .as_ref()
                    .is_some_and(|r| r.next == Screen::Victory);
                let source = if is_boss {
                    "boss"
                } else {
                    combat_kind.as_str()
                };
                let entries = reward_entries(&run);
                let mut taken: Vec<String> = Vec::new();
                if policy.reward_take {
                    // 拿遗物会开选牌界面(瓶装/浑天仪那类):先出 pick 行选完,
                    // 再回来接着拿剩下的奖励(离开奖励屏就没这一步了)
                    loop {
                        taken.extend(take_rewards(&mut run, policy)?);
                        if run.screen != Screen::Pick {
                            break;
                        }
                        let n = confirm_pick(&mut run)?;
                        out.push(line(
                            step,
                            "pick",
                            &format!("\"candidates\":{n},\"pick\":0"),
                            &state_json(&run),
                        ));
                        step += 1;
                    }
                    taken.sort_by_key(|t| tag_rank(t));
                }
                let tags: Vec<String> = taken.iter().map(|t| js(t)).collect();
                let payload = format!(
                    "\"source\":{},\"entries\":[{}],\"taken\":[{}]",
                    js(source),
                    entries.join(","),
                    tags.join(",")
                );
                // Boss 的奖励界面:到了策略允许的最后一幕就在这里收尾
                // (奖励行按离开前的局面打)
                if is_boss && run.act >= policy.acts {
                    out.push(line(step, "reward", &payload, &state_json(&run)));
                    step += 1;
                    out.push(line(step, "end", "\"result\":\"boss\"", &state_json(&run)));
                    return Ok(out);
                }
                if is_boss {
                    // 第一、二幕的 Boss 奖励离开时切下一幕:奖励行按切幕前的局面打
                    out.push(line(step, "reward", &payload, &state_json(&run)));
                    step += 1;
                    run.leave_reward();
                    continue;
                }
                run.leave_reward();
                out.push(line(step, "reward", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 营火 ----
            Screen::Rest => {
                let options = run.rest_options();
                let opts: Vec<String> = options
                    .iter()
                    .map(|o| js(&o.name().to_lowercase()))
                    .collect();
                // 钥匙模式:营火优先"回忆"拿红钥匙;策略要的那项不在就退到休息
                let mut pick = policy.rest;
                if policy.keys && run.can_recall() {
                    pick = RestOption::Recall;
                }
                if !options.contains(&pick) {
                    pick = options.first().copied().unwrap_or(RestOption::Rest);
                }
                run.rest_choose(pick)
                    .map_err(|e| format!("营火 {} 做不了:{e}", pick.name()))?;
                let payload = format!(
                    "\"options\":[{}],\"pick\":{}",
                    opts.join(","),
                    js(&pick.name().to_lowercase())
                );
                out.push(line(step, "rest", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 商店 ----
            Screen::Shop => {
                let items = shop_items(&run);
                // 购物策略还没做细,买空手(与参考实现的规范走法一致)
                let bought: Vec<String> = Vec::new();
                run.leave_shop();
                let payload = format!(
                    "\"items\":[{}],\"bought\":[{}]",
                    items.join(","),
                    bought.join(",")
                );
                out.push(line(step, "shop", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 宝箱 ----
            Screen::Treasure => {
                let chest = match run.chest {
                    Some(c) => format!(
                        "\"size\":{},\"gold_present\":{},\"tier\":{}",
                        js(chest_size_name(c.size)),
                        c.gold_present,
                        js(relic_tier_name(c.tier))
                    ),
                    None => "\"size\":null,\"gold_present\":false,\"tier\":null".to_string(),
                };
                let gold_before = run.player.gold;
                let relics_before = run.player.relics.len();
                // 钥匙模式:箱子还没开又还没有蓝钥匙,就拿蓝钥匙(那件遗物就作废了)
                if policy.keys && run.chest_sapphire_available() {
                    run.take_sapphire_key();
                } else {
                    run.take_treasure();
                }
                let gained: Vec<String> = run.player.relics[relics_before..]
                    .iter()
                    .map(|r| js(r.id))
                    .collect();
                let gold = run.player.gold - gold_before;
                let payload = format!(
                    "{chest},\"gold\":{gold},\"relics\":[{}]",
                    gained.join(",")
                );
                out.push(line(step, "treasure", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 选牌(Neow 移除/商店删牌/事件选牌/营火打铁) ----
            Screen::Pick => {
                let n = confirm_pick(&mut run)?;
                let payload = format!("\"candidates\":{n},\"pick\":0");
                out.push(line(step, "pick", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 收尾 ----
            Screen::Victory => {
                out.push(line(step, "end", "\"result\":\"victory\"", &state_json(&run)));
                return Ok(out);
            }
            Screen::Death => {
                out.push(line(step, "end", "\"result\":\"death\"", &state_json(&run)));
                return Ok(out);
            }
            other => {
                return Err(format!("走到没法自动处理的界面:{}", other.name()));
            }
        }
    }
}

/// CLI 入口:解析脚本(没有就用默认策略)
pub fn policy_from_script(text: Option<&str>) -> Result<Policy, String> {
    match text {
        None => Ok(Policy::default()),
        Some(t) => Policy::parse(t),
    }
}


#[cfg(test)]
mod e2e {
    //! 端到端对拍的固化:同 seed + 同路径脚本,本作的 JSONL 与参考实现
    //! (fixture 由 `bun tools/e2e_diff.ts <seed> --write` 从 refs/slay-the-cli 生成)
    //! 逐行比对.
    //!
    //! 现在两边还没完全一致,所以这里分两段断言:
    //!   1. 前面 `aligned` 步必须逐字节相同 —— 这部分坏了就是回归;
    //!   2. 剩下的差异步集合与内容指纹必须与登记的完全一致 ——
    //!      多一处差异、少一处差异、或某一行内容变了都会失败.
    //! 修好一条差异就重新跑 `bun tools/e2e_diff.ts <seed> --pin` 更新下面这张表.

    use super::*;

    /// 候选为空的选牌屏要"取消",不是"空手确认":旧实现直接 picker_confirm,
    /// 会报 "nothing selected" 把整局打断(奖励屏上的升级/移除/变形选牌就踩这个).
    #[test]
    fn empty_picker_is_cancelled_not_confirmed() {
        use crate::core::run::{PickPurpose, Picker};
        let mut run = Run::new(7);
        // 牌组全部升级后,"升级一张牌"这个用途就没有候选
        for c in run.player.deck.iter_mut() {
            c.upgrade();
        }
        assert!(!run.player.deck.iter().any(|c| c.can_upgrade()));
        run.screen = Screen::Pick;
        run.picker = Some(Picker {
            purpose: PickPurpose::Upgrade,
            back: Screen::Map,
            index: 0,
            cost_gold: 0,
            shop_slot: None,
            remaining: 1,
            bottle_kind: None,
            store_note: false,
        });
        let n = confirm_pick(&mut run).expect("空候选不该报错");
        assert_eq!(n, 0);
        assert!(run.picker.is_none(), "空候选应取消选牌屏");
        assert_eq!(run.screen, Screen::Map, "取消后要回到打开选牌前的界面");
    }

    struct Expected {
        seed: u64,
        /// 本作走出来的步数
        lines: usize,
        /// 参考实现 fixture 的步数(两边可能差一步)
        ref_lines: usize,
        /// 前面多少步与参考实现逐字节相同
        aligned: usize,
        /// 对不上的步号
        diff_steps: &'static [usize],
        /// 这些步的内容指纹(FNV-1a 64,覆盖本作与参考两侧的行文本)
        diff_digest: u64,
    }

    const CASES: &[Expected] = &[
    Expected { seed: 1, lines: 16, ref_lines: 16, aligned: 16, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 2, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 3, lines: 45, ref_lines: 45, aligned: 27, diff_steps: &[27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44], diff_digest: 0x310a57ffe7a44709 },
    Expected { seed: 4, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 5, lines: 14, ref_lines: 14, aligned: 14, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 29, ref_lines: 29, aligned: 29, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 7, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 9, lines: 34, ref_lines: 31, aligned: 29, diff_steps: &[29, 30, 31, 32, 33], diff_digest: 0xd995a3f88e81266b },
    Expected { seed: 10, lines: 41, ref_lines: 21, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40], diff_digest: 0xd746f059d1f3a5c8 },
    Expected { seed: 11, lines: 21, ref_lines: 21, aligned: 14, diff_steps: &[14, 15, 16, 17, 18, 19, 20], diff_digest: 0xd2bdcb27709405db },
    Expected { seed: 12, lines: 54, ref_lines: 54, aligned: 41, diff_steps: &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53], diff_digest: 0x2cb7bedae3c72c37 },
    Expected { seed: 13, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 14, lines: 23, ref_lines: 23, aligned: 23, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 15, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0x5bf166c09026e52d },
    Expected { seed: 16, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 17, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 18, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 19, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 20, lines: 22, ref_lines: 22, aligned: 15, diff_steps: &[15, 16, 17, 18, 19, 20, 21], diff_digest: 0x3cf8ea7500cb5abf },
    Expected { seed: 21, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 22, lines: 44, ref_lines: 44, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xe29b71bba304ea2 },
    Expected { seed: 23, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 24, lines: 17, ref_lines: 17, aligned: 15, diff_steps: &[15, 16], diff_digest: 0xb3e49f4f3735e49c },
    Expected { seed: 25, lines: 33, ref_lines: 33, aligned: 33, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 26, lines: 19, ref_lines: 19, aligned: 19, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 27, lines: 44, ref_lines: 44, aligned: 12, diff_steps: &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xc11e7d50c86f558e },
    Expected { seed: 28, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 29, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0x720614be46209ef3 },
    Expected { seed: 30, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 31, lines: 38, ref_lines: 38, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37], diff_digest: 0x18f1eabae1e7e606 },
    Expected { seed: 32, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 33, lines: 25, ref_lines: 25, aligned: 25, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 34, lines: 37, ref_lines: 37, aligned: 37, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 35, lines: 29, ref_lines: 29, aligned: 12, diff_steps: &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28], diff_digest: 0x10dc5af3c3590af8 },
    Expected { seed: 36, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 37, lines: 34, ref_lines: 34, aligned: 34, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 38, lines: 39, ref_lines: 39, aligned: 29, diff_steps: &[29, 30, 31, 32, 33, 34, 35, 36, 37, 38], diff_digest: 0x90c10984bb725557 },
    Expected { seed: 39, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 40, lines: 43, ref_lines: 43, aligned: 23, diff_steps: &[23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42], diff_digest: 0x486665cfaa299e93 },
    Expected { seed: 42, lines: 43, ref_lines: 43, aligned: 36, diff_steps: &[36, 37, 38, 39, 40, 41, 42], diff_digest: 0xf7e5bbf37042caf5 },
    Expected { seed: 54, lines: 43, ref_lines: 43, aligned: 25, diff_steps: &[25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42], diff_digest: 0xabde3528579e7910 },
];

    /// 扫荡集合:seed 1..40 的逐字段对拍登记表(与 tools/e2e_diff.ts 的归一化一致,
    /// 比较时两侧都转小写 —— 现在唯一的大小写差异是 init 行的 seed_str).
    const SWEEP: &[Expected] = &[
    Expected { seed: 1, lines: 16, ref_lines: 16, aligned: 16, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 2, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 3, lines: 45, ref_lines: 45, aligned: 27, diff_steps: &[27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44], diff_digest: 0x310a57ffe7a44709 },
    Expected { seed: 4, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 5, lines: 14, ref_lines: 14, aligned: 14, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 29, ref_lines: 29, aligned: 29, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 7, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 9, lines: 34, ref_lines: 31, aligned: 29, diff_steps: &[29, 30, 31, 32, 33], diff_digest: 0xd995a3f88e81266b },
    Expected { seed: 10, lines: 41, ref_lines: 21, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40], diff_digest: 0xd746f059d1f3a5c8 },
    Expected { seed: 11, lines: 21, ref_lines: 21, aligned: 14, diff_steps: &[14, 15, 16, 17, 18, 19, 20], diff_digest: 0xd2bdcb27709405db },
    Expected { seed: 12, lines: 54, ref_lines: 54, aligned: 41, diff_steps: &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53], diff_digest: 0x2cb7bedae3c72c37 },
    Expected { seed: 13, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 14, lines: 23, ref_lines: 23, aligned: 23, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 15, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0x5bf166c09026e52d },
    Expected { seed: 16, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 17, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 18, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 19, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 20, lines: 22, ref_lines: 22, aligned: 15, diff_steps: &[15, 16, 17, 18, 19, 20, 21], diff_digest: 0x3cf8ea7500cb5abf },
    Expected { seed: 21, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 22, lines: 44, ref_lines: 44, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xe29b71bba304ea2 },
    Expected { seed: 23, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 24, lines: 17, ref_lines: 17, aligned: 15, diff_steps: &[15, 16], diff_digest: 0xb3e49f4f3735e49c },
    Expected { seed: 25, lines: 33, ref_lines: 33, aligned: 33, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 26, lines: 19, ref_lines: 19, aligned: 19, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 27, lines: 44, ref_lines: 44, aligned: 12, diff_steps: &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xc11e7d50c86f558e },
    Expected { seed: 28, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 29, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0x720614be46209ef3 },
    Expected { seed: 30, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 31, lines: 38, ref_lines: 38, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37], diff_digest: 0x18f1eabae1e7e606 },
    Expected { seed: 32, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 33, lines: 25, ref_lines: 25, aligned: 25, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 34, lines: 37, ref_lines: 37, aligned: 37, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 35, lines: 29, ref_lines: 29, aligned: 12, diff_steps: &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28], diff_digest: 0x10dc5af3c3590af8 },
    Expected { seed: 36, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 37, lines: 34, ref_lines: 34, aligned: 34, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 38, lines: 39, ref_lines: 39, aligned: 29, diff_steps: &[29, 30, 31, 32, 33, 34, 35, 36, 37, 38], diff_digest: 0x90c10984bb725557 },
    Expected { seed: 39, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 40, lines: 43, ref_lines: 43, aligned: 23, diff_steps: &[23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42], diff_digest: 0x486665cfaa299e93 },
];

    fn fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/golden/e2e")
    }

    /// fixture 文件名跟脚本走:act1.script 是 seed<N>.ref.jsonl,
    /// 其它脚本是 seed<N>.<脚本名>.ref.jsonl(与 tools/e2e_diff.ts 一致)
    fn fixture_text_named(seed: u64, stem: &str) -> String {
        let name = if stem == "act1" {
            format!("seed{seed}.ref.jsonl")
        } else {
            format!("seed{seed}.{stem}.ref.jsonl")
        };
        std::fs::read_to_string(fixture_dir().join(&name))
            .unwrap_or_else(|e| panic!("seed {seed}: 读不到 {name}: {e}"))
    }

    fn fixture_text(seed: u64) -> String {
        fixture_text_named(seed, "act1")
    }

    fn fnv1a(bytes: &[u8], mut h: u64) -> u64 {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    fn shorten(line: &str) -> String {
        if line.len() > 400 {
            format!("{}…", &line[..400])
        } else {
            line.to_string()
        }
    }

    #[test]
    fn act1_walk_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("act1.script"))
            .expect("tools/golden/e2e/act1.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        for case in CASES {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完第一章: {e}", case.seed));
            let a: Vec<&str> = ours.lines().collect();
            let ref_text = fixture_text(case.seed);
            let b: Vec<&str> = ref_text.lines().collect();

            assert_eq!(
                a.len(),
                case.lines,
                "seed {}: 本作步数与登记的不同({} vs {})",
                case.seed,
                a.len(),
                case.lines
            );
            assert_eq!(
                b.len(),
                case.ref_lines,
                "seed {}: 参考 fixture 步数变了({} vs {})",
                case.seed,
                b.len(),
                case.ref_lines
            );

            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).copied().unwrap_or("null");
                let ref_line = b.get(i).copied().unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(
                aligned, case.aligned,
                "seed {}: 对齐前缀从 {} 步变成 {} 步{}",
                case.seed,
                case.aligned,
                aligned,
                if aligned < a.len().max(b.len()) {
                    format!(
                        "\n  首次分叉在第 {aligned} 步:\n    本作 {}\n    参考 {}",
                        shorten(a.get(aligned).copied().unwrap_or("(没有这一行)")),
                        shorten(b.get(aligned).copied().unwrap_or("(没有这一行)"))
                    )
                } else {
                    " (差异全消了,用 --pin 更新这张表)".to_string()
                }
            );
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 差异步集合变了(登记 {} 步,实际 {} 步)",
                case.seed,
                case.diff_steps.len(),
                diff_steps.len()
            );

            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).copied().unwrap_or("null"),
                    b.get(i).copied().unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h, case.diff_digest,
                "seed {}: 差异步的内容变了(指纹 {h:#x},登记 {:#x})",
                case.seed, case.diff_digest
            );
        }
    }

    /// 扫荡:seed 1..40 的逐字段对拍.两侧先转小写再比,与 tools/e2e_diff.ts 的归一化
    /// 一致(参考侧导出器会把所有字符串转小写,本作只有 init 行的 seed_str 是大写字母).
    #[test]
    fn sweep_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("act1.script"))
            .expect("tools/golden/e2e/act1.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        for case in SWEEP {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完第一章: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text(case.seed)
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).map(String::as_str).unwrap_or("null");
                let ref_line = b.get(i).map(String::as_str).unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(
                aligned,
                case.aligned,
                "seed {}: 对齐前缀从 {} 步变成 {} 步",
                case.seed,
                case.aligned,
                aligned
            );
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 差异步集合变了",
                case.seed
            );
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).map(String::as_str).unwrap_or("null"),
                    b.get(i).map(String::as_str).unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h,
                case.diff_digest,
                "seed {}: 差异步的内容变了(指纹 {h:#x}, 登记 {:#x})",
                case.seed,
                case.diff_digest
            );
        }
    }

    /// 端到端这把尺子要真的量到第一章主干:地图 → 普通怪 → 精英 → Boss 战.
    /// 参考实现在这三个 seed 上都是打到 Boss 就阵亡,没有 Boss 奖励那一屏
    #[test]
    fn the_walk_covers_the_whole_act1_spine() {
        let script = std::fs::read_to_string(fixture_dir().join("act1.script")).unwrap();
        let policy = Policy::parse(&script).unwrap();
        let mut seen: Vec<&str> = Vec::new();
        let record = |kind: &'static str, seen: &mut Vec<&'static str>| {
            if !seen.contains(&kind) {
                seen.push(kind);
            }
        };
        let mut elite = false;
        let mut boss_fight = false;
        let mut side_room = false;
        for case in CASES {
            let text = run_jsonl(case.seed, &policy).unwrap();
            for kind in ["init", "neow", "move", "fight", "reward", "end"] {
                if text.contains(&format!("\"kind\":\"{kind}\"")) {
                    record(kind, &mut seen);
                }
            }
            for kind in ["rest", "shop", "treasure", "event"] {
                if text.contains(&format!("\"kind\":\"{kind}\"")) {
                    side_room = true;
                    record(kind, &mut seen);
                }
            }
            if text.contains("\"resolved\":\"elite\"") {
                elite = true;
            }
            // 主线要量到 Boss 战(第 15 层那一场)
            if text
                .lines()
                .any(|l| l.contains("\"kind\":\"fight\"") && l.contains("\"row\":15"))
            {
                boss_fight = true;
            }
        }
        for kind in ["init", "neow", "move", "fight", "reward", "end"] {
            assert!(seen.contains(&kind), "整条路没走到 {kind}");
        }
        assert!(elite, "三个 seed 都没打过精英");
        assert!(side_room, "三个 seed 都没进过营火/商店/宝箱/事件");
        assert!(boss_fight, "三个 seed 都没打到 Boss 战");
    }

    /// fixture 本身要像样的:每个 seed 都要走到 Boss 那一层,步数不能太少
    #[test]
    fn fixtures_cover_the_whole_first_act() {
        for case in CASES {
            let text = fixture_text(case.seed);
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines.len(), case.ref_lines);
            assert!(
                lines.last().unwrap().contains("\"kind\":\"end\""),
                "seed {}: fixture 最后一行不是 end",
                case.seed
            );
        }
        // 有些 seed 会半路阵亡(fixture 短),所以"走到 Boss 那一层"是对整套的要求:
        // 整套 fixture 必须覆盖第一章的全貌,并且至少有一份是完整的长跑
        assert!(
            CASES.iter().any(|c| fixture_text(c.seed).contains("\"row\":15")),
            "整套 fixture 里没有走到 Boss 那一层的"
        );
        assert!(
            CASES.iter().any(|c| c.ref_lines >= 30),
            "整套 fixture 都太短"
        );
    }

    // ---- 多幕(第一幕 → 第二幕;能活到后面就继续) ----

    /// 多幕扫荡的登记表:acts.script 下的一整局,字段与第一章那张表相同.
    /// 这 19 个种子在 1..30000 里参考实现都能靠智能打牌走到第二幕 Boss(第二幕 Boss
    /// 是堵墙,过了它的目前没有).本作按原版重掷了开战随机阵容之后,战斗走向与参考
    /// 分叉,其中 4 个种子活不到第二幕,其余依次列出各自的差异步.
    const ACTS_CASES: &[Expected] = &[
    Expected { seed: 8, lines: 52, ref_lines: 52, aligned: 47, diff_steps: &[47, 48, 49, 50, 51], diff_digest: 0xc785cdc1b1cf221 },
    Expected { seed: 1815, lines: 52, ref_lines: 52, aligned: 47, diff_steps: &[47, 48, 49, 50, 51], diff_digest: 0xf28cd9b0aa241e08 },
    Expected { seed: 2474, lines: 50, ref_lines: 50, aligned: 42, diff_steps: &[42, 43], diff_digest: 0x68589ce8642de114 },
    Expected { seed: 3605, lines: 45, ref_lines: 45, aligned: 45, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 4327, lines: 51, ref_lines: 51, aligned: 40, diff_steps: &[40, 41, 46, 47, 48, 49, 50], diff_digest: 0x56e51b5536e5ee25 },
    Expected { seed: 7140, lines: 51, ref_lines: 51, aligned: 51, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 10242, lines: 48, ref_lines: 48, aligned: 48, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 11535, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 12691, lines: 51, ref_lines: 51, aligned: 20, diff_steps: &[20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50], diff_digest: 0x25dd779eeff1a1b1 },
    Expected { seed: 12835, lines: 53, ref_lines: 53, aligned: 53, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 20703, lines: 54, ref_lines: 54, aligned: 54, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 21075, lines: 49, ref_lines: 49, aligned: 49, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 22882, lines: 55, ref_lines: 55, aligned: 41, diff_steps: &[41, 42], diff_digest: 0x19960778821c2228 },
    Expected { seed: 23808, lines: 55, ref_lines: 55, aligned: 34, diff_steps: &[34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xb819deb2cb8b5d08 },
    Expected { seed: 24873, lines: 53, ref_lines: 53, aligned: 53, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 25365, lines: 47, ref_lines: 47, aligned: 47, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 26848, lines: 58, ref_lines: 58, aligned: 53, diff_steps: &[53, 54, 55], diff_digest: 0xe7553c4d81a8c618 },
    Expected { seed: 26951, lines: 63, ref_lines: 63, aligned: 43, diff_steps: &[43, 44], diff_digest: 0x664f19f5cdacbe10 },
    Expected { seed: 28104, lines: 50, ref_lines: 50, aligned: 50, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
];
    /// 多幕对拍:同一颗种子 + acts.script,本作与参考实现逐行比对(两侧都转小写).
    /// 「击杀盗贼退还赃款」的差额会一直带着,所以和第一章一样分两段断言:
    /// 前缀逐字节相同 + 差异步集合与内容指纹固定.
    #[test]
    fn acts_walk_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("acts.script"))
            .expect("tools/golden/e2e/acts.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        for case in ACTS_CASES {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完多幕: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text_named(case.seed, "acts")
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).map(String::as_str).unwrap_or("null");
                let ref_line = b.get(i).map(String::as_str).unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(
                aligned, case.aligned,
                "seed {}: 对齐前缀从 {} 步变成 {} 步",
                case.seed, case.aligned, aligned
            );
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 差异步集合变了",
                case.seed
            );
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).map(String::as_str).unwrap_or("null"),
                    b.get(i).map(String::as_str).unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h,
                case.diff_digest,
                "seed {}: 差异步的内容变了(指纹 {h:#x}, 登记 {:#x})",
                case.seed,
                case.diff_digest
            );
        }
    }

    // ---- 第二幕(调试钩子 act 2 推过去) ----

    /// 第二幕的登记表:act2.script 下的一局,字段与第一章那张表相同.
    /// 开局就切到第二幕(`act 2`)、生命 9999(`hp 9999`)、牌组换成 10 张强化狂暴
    /// (deck ramp),于是量到的是第二幕整条主干:第二幕自己的地图、遭遇名单(抢劫三连/
    /// 三哨兵/头目小鬼/蛇形植物那些)、事件池、精英与第 15 行的第二幕 Boss
    /// (铜制自动机/收集者/勇士)以及它的奖励屏.
    ///
    /// 12 个种子按 Boss 分成三组、每组 4 个:铜制自动机 3/13/19/33、勇士 6/17/18/25、
    /// 收集者 4/11/15/16.第二幕两边分叉比第一/三幕多(战斗里的 hp、被偷的金币、
    /// 事件选项数、召唤物的血量与站位),所以这张尺子和 acts 那张一样,连差异步与
    /// 内容指纹一起登记.分类见 report.
    const ACT2_CASES: &[Expected] = &[
    Expected { seed: 3, lines: 46, ref_lines: 46, aligned: 14, diff_steps: &[14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 43, 44, 45], diff_digest: 0x35c048c12f3a91fd },
    Expected { seed: 13, lines: 44, ref_lines: 44, aligned: 22, diff_steps: &[22, 23, 24, 25, 26, 27, 28, 33, 41, 42, 43], diff_digest: 0x1b3fe4bdbca217c5 },
    Expected { seed: 19, lines: 44, ref_lines: 44, aligned: 28, diff_steps: &[28, 29, 30, 40], diff_digest: 0x4cadbd46ae889299 },
    Expected { seed: 33, lines: 43, ref_lines: 43, aligned: 32, diff_steps: &[32, 33, 34, 35, 36, 37, 40, 41, 42], diff_digest: 0x2aec5ce78b4a798e },
    Expected { seed: 6, lines: 46, ref_lines: 46, aligned: 29, diff_steps: &[29, 30, 31, 32, 33, 34, 35], diff_digest: 0xe67781a970da6824 },
    Expected { seed: 17, lines: 43, ref_lines: 43, aligned: 17, diff_steps: &[17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32], diff_digest: 0x9ee3dc12d09ebb12 },
    Expected { seed: 18, lines: 43, ref_lines: 43, aligned: 2, diff_steps: &[2, 3, 4, 5, 6, 7, 35, 36, 37, 40, 41, 42], diff_digest: 0xc172d08ffcdc326c },
    Expected { seed: 25, lines: 47, ref_lines: 46, aligned: 32, diff_steps: &[32, 33, 34, 35, 36, 37, 38, 39, 43, 44, 45, 46], diff_digest: 0x988b6e1c6f0aa4f1 },
    Expected { seed: 4, lines: 46, ref_lines: 45, aligned: 29, diff_steps: &[29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 41, 42, 43, 44, 45], diff_digest: 0xb5adcab9de0b2df5 },
    Expected { seed: 11, lines: 47, ref_lines: 48, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47], diff_digest: 0x11abdc8473601d2 },
    Expected { seed: 15, lines: 44, ref_lines: 44, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 40, 41, 42, 43], diff_digest: 0x6c12df903dcb8e9d },
    Expected { seed: 16, lines: 44, ref_lines: 44, aligned: 26, diff_steps: &[26, 27, 28, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43], diff_digest: 0x436132dc5db675f9 },
];

    /// 第二幕对拍:同一颗种子 + act2.script,逐行比对(两侧都转小写).
    /// 与 acts.script 那张表一样分两段断言:前缀逐字节相同 + 差异步集合与内容指纹固定.
    #[test]
    fn act2_walk_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("act2.script"))
            .expect("tools/golden/e2e/act2.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        assert_eq!(policy.act, 2, "act2.script 要起手切到第二幕");
        assert_eq!(policy.deck, Deck::Ramp, "act2.script 要换成 10 张强化狂暴");
        assert_eq!(policy.acts, 2, "act2.script 要在第二幕 Boss 奖励屏停下");
        for case in ACT2_CASES {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完第二幕: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text_named(case.seed, "act2")
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).map(String::as_str).unwrap_or("null");
                let ref_line = b.get(i).map(String::as_str).unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(
                aligned, case.aligned,
                "seed {}: 对齐前缀从 {} 步变成 {} 步",
                case.seed, case.aligned, aligned
            );
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 差异步集合变了",
                case.seed
            );
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).map(String::as_str).unwrap_or("null"),
                    b.get(i).map(String::as_str).unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h,
                case.diff_digest,
                "seed {}: 差异步的内容变了(指纹 {h:#x}, 登记 {:#x})",
                case.seed,
                case.diff_digest
            );
        }
    }

    /// 第二幕这条尺子要真的量到第二幕:整条路都在 act 2,三个第二幕 Boss 都被量到过,
    /// 而且每个种子都走到了 Boss 奖励屏(不是半路阵亡).
    #[test]
    fn act2_walk_covers_the_second_act() {
        let script = std::fs::read_to_string(fixture_dir().join("act2.script")).unwrap();
        let policy = Policy::parse(&script).unwrap();
        let mut acts: Vec<i64> = Vec::new();
        let mut kinds: Vec<String> = Vec::new();
        let mut bosses: Vec<String> = Vec::new();
        for case in ACT2_CASES {
            let text = run_jsonl(case.seed, &policy).unwrap();
            for line in text.lines() {
                let kind = line
                    .split("\"kind\":\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or("")
                    .to_string();
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
                for boss in ["bronze_automaton", "the_collector", "the_champ"] {
                    if line.contains(&format!("\"id\":\"{boss}\"")) && !bosses.contains(&boss.to_string()) {
                        bosses.push(boss.to_string());
                    }
                }
                let act: i64 = line
                    .split("\"act\":")
                    .nth(1)
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if !acts.contains(&act) {
                    acts.push(act);
                }
            }
            assert!(
                text.lines().any(|l| l.contains("\"node\":\"boss\"")),
                "seed {}: 没走到第二幕 Boss",
                case.seed
            );
            assert!(
                text.lines().last().unwrap().contains("\"result\":\"boss\""),
                "seed {}: 没停在第二幕 Boss 奖励屏",
                case.seed
            );
        }
        assert_eq!(acts, vec![2], "整条路都该在第二幕");
        for kind in ["init", "move", "fight", "reward", "event"] {
            assert!(kinds.iter().any(|k| k == kind), "第二幕没走到 {kind}");
        }
        assert_eq!(bosses.len(), 3, "三个第二幕 Boss 没都量到:{bosses:?}");
    }

    // ---- 第三幕 / 第四幕(调试钩子 act/floor/keys all/hp/deck 推过去) ----

    /// 第三幕的登记表:act3.script 下的一局,字段与第一章那张表相同.
    /// 开局就切到第三幕(`act 3`)、生命 9999(`hp 9999`)、牌组换成 10 张强化狂暴
    /// (deck ramp),钥匙不拿(keys off),于是量到的是第三幕整条主干:第三幕自己的地图、
    /// 遭遇名单(放回/不放回的各种阵容)、事件池、精英,以及第 15 行的第三幕 Boss
    /// (觉醒者/时间吞噬者/顿努与德卡)与它的奖励屏(acts 3 就停在那一屏).
    ///
    /// 狂暴每打一次自己 +8、越打越重,所以最笨的出牌策略也破得开暗灵三连/颚虫三连/
    /// 瞬变体这些血厚或会复生的遭遇:下面 7 个种子全都从第 0 层走到第 15 行、把 Boss 打掉
    /// (此前那套 10 张强化重锤没有一张防守牌,走 0..4 层就阵亡,只量到开局那几层).
    ///
    /// 这 7 个种子是从 1..1200 里挑出来**两边逐字节一致**的(从 init 到本局结束一行不差,
    /// 每一步的 hp/金币/牌堆/遗物/药水都对得上),三个 Boss 都有.剩下的种子会在下面这些
    /// 分叉上分道扬镳(分类与最小修法见 report):
    ///   (b) 参考未实现:颚虫部落那只怪的预置状态(力量 3/格挡 5/已行动一回合),参考侧
    ///       既没有这套预置、也不按"已行动过"重掷第一招;
    ///   (b) 参考未实现:第三幕事件 Mind Bloom 的"打一个 Boss"选项,参考侧开战时抛
    ///       `unknown monster DONU_AND_DECA`,整局跑不完(这些种子连 fixture 都落不了);
    ///   (d) 未定论:暗灵半死复活之后,"重咬不能连续两次"那条历史算不算复活期间摆的
    ///       再生/转生,两边不一致.
    const ACT3_CASES: &[Expected] = &[
    Expected { seed: 29, lines: 43, ref_lines: 43, aligned: 43, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 30, lines: 43, ref_lines: 43, aligned: 43, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 121, lines: 42, ref_lines: 42, aligned: 42, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 237, lines: 41, ref_lines: 41, aligned: 41, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 284, lines: 44, ref_lines: 44, aligned: 44, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 494, lines: 41, ref_lines: 41, aligned: 41, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 510, lines: 44, ref_lines: 44, aligned: 44, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
];

    /// 第三幕对拍:同一颗种子 + act3.script,逐行比对(两侧都转小写).
    #[test]
    fn act3_walk_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("act3.script"))
            .expect("tools/golden/e2e/act3.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        assert_eq!(policy.act, 3, "act3.script 要起手切到第三幕");
        assert_eq!(policy.deck, Deck::Ramp, "act3.script 要换成 10 张强化狂暴");
        assert!(!policy.keys, "act3.script 不拿钥匙(拿了会直接从 Boss 后面进门去第四幕)");
        for case in ACT3_CASES {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完第三幕: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text_named(case.seed, "act3")
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).map(String::as_str).unwrap_or("null");
                let ref_line = b.get(i).map(String::as_str).unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 第三幕出现差异(第 {:?} 步)\n  本作 {}\n  参考 {}",
                case.seed,
                diff_steps.first(),
                a.get(diff_steps.first().copied().unwrap_or(0)).map(String::as_str).unwrap_or(""),
                b.get(diff_steps.first().copied().unwrap_or(0)).map(String::as_str).unwrap_or("")
            );
        }
    }

    /// 第三幕这条尺子要真的量到第三幕:整条路都在 act 3(不是从第一幕走上去的),
    /// 并且真的打过第三幕的怪、拿过奖励.
    #[test]
    fn act3_walk_covers_the_third_act() {
        let script = std::fs::read_to_string(fixture_dir().join("act3.script")).unwrap();
        let policy = Policy::parse(&script).unwrap();
        let mut kinds: Vec<String> = Vec::new();
        let mut acts: Vec<i64> = Vec::new();
        for case in ACT3_CASES {
            let text = run_jsonl(case.seed, &policy).unwrap();
            for line in text.lines() {
                let kind = line
                    .split("\"kind\":\"")
                    .nth(1)
                    .and_then(|s| s.split('"').next())
                    .unwrap_or("")
                    .to_string();
                if !kinds.contains(&kind) {
                    kinds.push(kind);
                }
                let act: i64 = line
                    .split("\"act\":")
                    .nth(1)
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if !acts.contains(&act) {
                    acts.push(act);
                }
            }
        }
        assert_eq!(acts, vec![3], "整条路都该在第三幕");
        for kind in ["init", "move", "fight", "reward"] {
            assert!(kinds.iter().any(|k| k == kind), "第三幕没走到 {kind}");
        }
    }

    /// 第四幕(钥匙门后)的登记表:act4.script 下的一局,字段与第一章那张表相同.
    /// 开局三把钥匙直接到手(keys all)、生命 9999(hp 9999)、牌组换成
    /// 1 张强化重刃 + 24 张强化火上浇油(deck burst),起手就切到第四幕(`act 4`),
    /// 于是量到的正是那条定死的营火 → 商店 → 精英(盾与矛 110/160)→ Boss(心脏 750)四层.
    ///
    /// 心脏的两条机制都量到了:死亡律动(每打一张牌挨 1 点)落在每一回合的 hp 轨迹里
    /// (这套牌每回合至少打 3 张,hp 一次掉 3~4),无敌(一回合最多掉 300)则靠火上浇油
    /// 堆起来的力量:16 个种子里 13 个都出现过"单次出牌正好掉 300"的那一刀
    /// (即伤害被上限截掉),逐字节与参考一致.16 个种子里 15 个是打过心脏收尾,
    /// seed 1 是被心脏打死(两边同样):胜利与阵亡两条路都登记在案.
    const ACT4_CASES: &[Expected] = &[
    Expected { seed: 1, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 2, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 3, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 4, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 5, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 7, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 9, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 10, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 11, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 12, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 13, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 14, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 15, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 16, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
];

    /// 第四幕对拍:同一颗种子 + act4.script,逐行比对(两侧都转小写).
    /// 这条尺子量到的是第四幕整条主干:定死的四层、精英的固定阵容(盾与矛
    /// 110/160)、心脏的 750 血与死亡律动/无敌.两边步数与差异集合都登记在案.
    #[test]
    fn act4_walk_matches_reference() {
        let script = std::fs::read_to_string(fixture_dir().join("act4.script"))
            .expect("tools/golden/e2e/act4.script 应该在");
        let policy = Policy::parse(&script).expect("路径脚本要能解析");
        assert_eq!(policy.act, 4, "act4.script 要起手切到第四幕");
        assert!(policy.keys_all, "act4.script 要三把钥匙直接到手");
        assert_eq!(
            policy.deck,
            Deck::Burst,
            "act4.script 要换成 1 张强化重刃 + 24 张强化火上浇油(才顶得到心脏的无敌)"
        );
        for case in ACT4_CASES {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完第四幕: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text_named(case.seed, "act4")
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let ours_line = a.get(i).map(String::as_str).unwrap_or("null");
                let ref_line = b.get(i).map(String::as_str).unwrap_or("null");
                if ours_line != ref_line {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(
                aligned, case.aligned,
                "seed {}: 对齐前缀从 {} 步变成 {} 步",
                case.seed, case.aligned, aligned
            );
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: 差异步集合变了",
                case.seed
            );
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).map(String::as_str).unwrap_or("null"),
                    b.get(i).map(String::as_str).unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h,
                case.diff_digest,
                "seed {}: 差异步的内容变了(指纹 {h:#x}, 登记 {:#x})",
                case.seed,
                case.diff_digest
            );
        }
    }

    // ---- 飞升(A20)三条尺子 ----
    //
    // 飞升 1-20 的数值/规则刚落地,但历史上只验证过 A0 逐字节不变,没跑过 A20 对拍.
    // 这三张表把 A20 的第一幕 / 第三幕 / 第四幕各钉住一版:
    //   a20.script   = act1.script + `asc 20`(auto_play,不换牌组/hp,量起始状态与第一幕规则)
    //   a20a3.script = act3.script + `asc 20`(smart on,hp 9999,deck ramp,量第三幕与 A20 双 Boss)
    //   a20a4.script = act4.script + `asc 20`(smart on,hp 9999,deck burst,量飞升 18/19/20 的
    //                  精英盾与矛 / 心脏)
    //
    // A20 第一幕现已逐字节全对齐(11/11):修掉了飞升 17 的敌人选招分支(史莱姆/虱子/
    // 奴隶主/小鬼巫师)、飞升 15 的一次性事件池(去掉 note_for_yourself)、飞升 2+ 的虱子
    // 咬伤区间,以及"自己回合内死掉的怪也要照常掷下一招"(参考实现里自爆的死亡是排队生效的).
    // 第三幕还有分叉(42 → 31 处):seed 30 已全对齐(尖刺外壳在击杀那一击也照常反伤,
    // 依据反编译源码:BattleContext 里 SHARP_HIDE 的伤害排在牌之后,而胜利清空动作队列
    // 只清 clearOnCombatVictory=true 的动作,DamagePlayer 恰好是 false).剩下三颗:
    //   seed 237(3 处)顿努与德卡的"团队护盾"在 A20 下的格挡记账;
    //   seed 284(14 处)爬虫法师+匕首一战的 1 点伤害差;
    //   seed 510(14 处)暗灵复活后的选招掷点;
    // 表里把当前的对齐前缀、差异步与内容指纹登记下来,修好一条就重跑
    // `bun tools/e2e_diff.ts <seed> --script <脚本> --pin`.
    // 第四幕(A20)16 个种子逐字节全对齐(飞升 18/19/20 的盾矛与心脏数值都过了).
    const ASC_CASES: &[Expected] = &[
    Expected { seed: 1, lines: 14, ref_lines: 14, aligned: 14, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 4, lines: 19, ref_lines: 19, aligned: 19, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 21, ref_lines: 21, aligned: 21, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 19, lines: 14, ref_lines: 14, aligned: 14, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 23, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 25, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 33, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 34, lines: 31, ref_lines: 31, aligned: 31, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 37, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 39, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
];

    const ASC3_CASES: &[Expected] = &[
    Expected { seed: 29, lines: 44, ref_lines: 44, aligned: 44, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 30, lines: 45, ref_lines: 45, aligned: 45, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 121, lines: 43, ref_lines: 43, aligned: 43, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 237, lines: 43, ref_lines: 43, aligned: 40, diff_steps: &[40, 41, 42], diff_digest: 0xaa19366d72d4fbbb },
    Expected { seed: 284, lines: 46, ref_lines: 46, aligned: 29, diff_steps: &[29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 44, 45], diff_digest: 0x6c96de07441f5089 },
    Expected { seed: 494, lines: 42, ref_lines: 42, aligned: 42, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 510, lines: 45, ref_lines: 45, aligned: 11, diff_steps: &[11, 12, 13, 14, 15, 34, 35, 36, 37, 38, 39, 42, 43, 44], diff_digest: 0xf382ade1687f98c5 },
];

    const ASC4_CASES: &[Expected] = &[
    Expected { seed: 1, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 2, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 3, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 4, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 5, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 7, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 9, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 10, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 11, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 12, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 13, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 14, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 15, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 16, lines: 11, ref_lines: 11, aligned: 11, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
];

    /// A20 三条尺子共用:按 <script> 解析策略(必须 asc 20),逐种子对 fixture.
    fn check_asc_table(script: &str, stem: &str, cases: &[Expected], label: &str) {
        let text = std::fs::read_to_string(fixture_dir().join(script))
            .unwrap_or_else(|_| panic!("tools/golden/e2e/{script} 应该在"));
        let policy = Policy::parse(&text).expect("路径脚本要能解析");
        assert_eq!(policy.asc, 20, "{script} 要把飞升抬到 20");
        for case in cases {
            let ours = run_jsonl(case.seed, &policy)
                .unwrap_or_else(|e| panic!("seed {}: 跑不完{label}: {e}", case.seed));
            let a: Vec<String> = ours.lines().map(|l| l.to_lowercase()).collect();
            let b: Vec<String> = fixture_text_named(case.seed, stem)
                .lines()
                .map(|l| l.to_lowercase())
                .collect();
            assert_eq!(a.len(), case.lines, "seed {}: 本作步数与登记的不同", case.seed);
            assert_eq!(b.len(), case.ref_lines, "seed {}: 参考 fixture 步数变了", case.seed);
            let mut diff_steps: Vec<usize> = Vec::new();
            for i in 0..a.len().max(b.len()) {
                let o = a.get(i).map(String::as_str).unwrap_or("null");
                let r = b.get(i).map(String::as_str).unwrap_or("null");
                if o != r {
                    diff_steps.push(i);
                }
            }
            let aligned = diff_steps.first().copied().unwrap_or(a.len().max(b.len()));
            assert_eq!(aligned, case.aligned, "seed {}: {label} 对齐前缀变了", case.seed);
            assert_eq!(
                diff_steps.as_slice(),
                case.diff_steps,
                "seed {}: {label} 差异步集合变了",
                case.seed
            );
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &i in &diff_steps {
                let line = format!(
                    "{i}\t{}\t{}\n",
                    a.get(i).map(String::as_str).unwrap_or("null"),
                    b.get(i).map(String::as_str).unwrap_or("null")
                );
                h = fnv1a(line.as_bytes(), h);
            }
            assert_eq!(
                h, case.diff_digest,
                "seed {}: {label} 差异步的内容变了(指纹 {h:#x}, 登记 {:#x})",
                case.seed, case.diff_digest
            );
        }
    }

    /// 飞升 20 第一幕:起始状态(A6 掉血/A10 诅咒/A11 药水槽/A14 上限)与第一幕规则.
    #[test]
    fn asc20_act1_matches_reference() {
        check_asc_table("a20.script", "a20", ASC_CASES, "A20 第一幕");
    }

    /// 飞升 20 第三幕:第三幕规则与 A20 的双 Boss(第一个 Boss 倒下后不结算,直接开第二个).
    #[test]
    fn asc20_act3_matches_reference() {
        check_asc_table("a20a3.script", "a20a3", ASC3_CASES, "A20 第三幕");
    }

    /// 飞升 20 第四幕:飞升 18(盾与矛)/19(心脏数值)在 A20 下的表现.
    #[test]
    fn asc20_act4_matches_reference() {
        check_asc_table("a20a4.script", "a20a4", ASC4_CASES, "A20 第四幕");
    }

    /// 第四幕这条尺子要真的量到"钥匙门后"的全貌:定死的四层、
    /// 精英(盾与矛)与心脏(死亡律动/无敌)都在步流里,并且最后是胜利收尾.
    #[test]
    fn act4_walk_covers_the_whole_fourth_act() {
        let script = std::fs::read_to_string(fixture_dir().join("act4.script")).unwrap();
        let policy = Policy::parse(&script).unwrap();
        // seed 1 是被心脏打死的(两边一致),所以"整幕走完"这条用 seed 2 量
        let text = run_jsonl(2, &policy).unwrap();
        assert!(text.contains("\"boss\":\"the_heart\""), "第四幕的 Boss 应该是心脏");
        for node in ["\"node\":\"rest\"", "\"node\":\"shop\"", "\"node\":\"elite\"", "\"node\":\"boss\""] {
            assert!(text.contains(node), "第四幕没走到 {node}");
        }
        assert!(text.contains("\"id\":\"spire_shield\""), "精英应该是盾卫");
        assert!(text.contains("\"id\":\"spire_spear\""), "精英应该是矛手");
        assert!(text.contains("\"id\":\"corrupt_heart\""), "Boss 应该是腐化心脏");
        assert!(
            text.lines().last().unwrap().contains("\"result\":\"victory\""),
            "第四幕应该是打过心脏收尾"
        );
        // 整套尺子要同时量到"打过心脏"与"被心脏打死"两条路
        let ends: Vec<String> = ACT4_CASES
            .iter()
            .map(|c| {
                run_jsonl(c.seed, &policy)
                    .unwrap()
                    .lines()
                    .last()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        assert!(
            ends.iter().any(|l| l.contains("\"result\":\"victory\"")),
            "没有一个种子打过心脏"
        );
        assert!(
            ends.iter().any(|l| l.contains("\"result\":\"death\"")),
            "没有种子是被心脏打死的(阵亡那条路没量到)"
        );
    }

    /// 多幕这条尺子要真的量到第二幕:两边步流(各个界面出现的先后)完全一致,
    /// 而且每一步 move 的 from/to/node/resolved/怪物阵容逐字节相同 ——
    /// 这等于把切幕时机、第二幕地图生成与第二幕遭遇名单都比了一遍.
    #[test]
    fn the_walk_crosses_into_the_second_act() {
        let script = std::fs::read_to_string(fixture_dir().join("acts.script")).unwrap();
        let policy = Policy::parse(&script).unwrap();
        // move 行只比动作部分(from/to/node/resolved),不比结束后的局面.
        // 遭遇阵容也不比:参考实现把随机阵容写死了(虱子颜色/史莱姆变体/小鬼团伙/
        // Exordium 配对),本作按原版开战掷签,两边阵容本来就会不同;
        // 阵容的内容由 ACTS_CASES 那张登记表(逐行指纹)盯着.
        let moves = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|l| l.contains("\"kind\":\"move\""))
                .map(|l| {
                    let start = l.find("\"from\"").expect("move 行里有 from");
                    let end = l.find(",\"s\":").expect("move 行里有 s");
                    let body = match l[start..end].find(",\"monsters\":") {
                        Some(i) => &l[start..start + i],
                        None => &l[start..end],
                    };
                    body.to_lowercase()
                })
                .collect()
        };
        let mut crossed = 0;
        let mut ours_crossed = 0;
        let mut compared = 0usize;
        for case in ACTS_CASES {
            let ours = run_jsonl(case.seed, &policy).unwrap();
            let ref_text = fixture_text_named(case.seed, "acts");
            let ours_reaches_act2 = ours.contains("\"act\":2");
            if ours_reaches_act2 {
                ours_crossed += 1;
                assert!(ours.contains("\"row\":15"), "seed {}: 本作没打满第一幕", case.seed);
            }
            // 参考侧不变:这 19 个种子都是参考能走到第二幕的
            if ref_text.contains("\"act\":2") {
                crossed += 1;
            }
            // 两边步数可能不同(有一边死得早,后面的界面序列自然对不上),
            // 但共同走过的那些"移动"必须一步不差 —— 这就是地图与遭遇名单的比对
            let (om, rm) = (moves(&ours), moves(&ref_text));
            let n = om.len().min(rm.len());
            if ours_reaches_act2 {
                assert!(
                    n >= 16,
                    "seed {}: 本作走到第二幕却只比了 {n} 次移动,量不到第一幕",
                    case.seed
                );
            }
            assert_eq!(
                &om[..n],
                &rm[..n],
                "seed {}: 前 {n} 次移动(from/to/node/遭遇)不一致",
                case.seed
            );
            compared += n;
        }
        assert!(
            crossed >= 15,
            "只有 {crossed} 个种子参考走到了第二幕,切幕没被量到"
        );
        // 本作按原版重掷了随机阵容,战斗走向与原版更近、与参考实现更远:
        // 有 4 个种子在这颗种子下活不到第二幕(阵容/咬伤数值变了).
        // 这里只要求大多数种子仍能跨幕,这样"切幕"这条尺子还在量东西.
        assert!(
            ours_crossed >= 15,
            "本作只有 {ours_crossed} 个种子走到第二幕,切幕没被量到"
        );
        assert!(compared >= 260, "只比过 {compared} 次移动,量得太少");
    }

    /// 第二、三章的地图排版与燃烧精英:fixture 由 `bun tools/gen_act_maps.ts`
    /// 从参考实现的 generateMap 直接掷出来.顺带验了按幕重种 mapRng 的偏移
    /// (第二章 seed+200,第三章 seed+600).
    #[test]
    fn act_maps_match_reference() {
        use crate::core::map::ActMap;
        use crate::rng::RngRegistry;
        let text = std::fs::read_to_string(fixture_dir().join("act_maps.tsv"))
            .expect("tools/golden/e2e/act_maps.tsv 应该在(跑 bun tools/gen_act_maps.ts 生成)");
        let mut n = 0;
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let c: Vec<&str> = line.split('\t').collect();
            assert_eq!(c.len(), 7, "act_maps.tsv 的列数不对: {line}");
            let seed: u64 = c[0].parse().expect("seed 要是数字");
            let act: u32 = c[1].parse().expect("act 要是数字");
            let set_burning = c[2] == "1";
            let bx: i32 = c[3].parse().unwrap();
            let by: i32 = c[4].parse().unwrap();
            let buff: i32 = c[5].parse().unwrap();
            let want = c[6].split('|').collect::<Vec<_>>().join("\n");
            let mut reg = RngRegistry::new(seed);
            reg.reseed_map(act);
            let map = ActMap::generate(reg.map_rng(), set_burning, 0);
            assert_eq!(
                map.to_rows_string(),
                want,
                "seed {seed} act {act} set_burning {set_burning}: 地图对不上"
            );
            match map.burning_node() {
                Some(i) => assert_eq!(
                    (
                        map.node(i).col as i32,
                        map.node(i).floor as i32,
                        map.burning_buff
                    ),
                    (bx, by, buff),
                    "seed {seed} act {act}: 燃烧精英对不上"
                ),
                None => assert!(
                    !set_burning,
                    "seed {seed} act {act}: 该标燃烧精英却没标"
                ),
            }
            n += 1;
        }
        assert!(n >= 90, "act_maps.tsv 的用例太少: {n}");
    }
}

// ---- 沙盒:单张牌 / 单件遗物 / 单瓶药水的差分尺 ----
//
// 用法:`spire --sandbox <seed> <scenario.json>`,逐步状态落成 JSONL。
// 参考侧(tools/sandbox_ref.ts)吃同一份 scenario,吐同一套 schema,
// 由 tools/sandbox_diff.ts 逐字段比对。
//
// scenario 字段(都可省,省了就保持引擎初始化的原状):
//   encounter  我方占位遭遇(只用来定怪物数量;scenario 自己的敌人覆盖它)
//   player     {hp,max_hp,block,energy,max_energy,powers:{strength:2,...}}
//   relics     ["burning_blood"]         遗物 id(我方小写)
//   potions    ["fire_potion",null,null] 药水格
//   deck       初始牌组(给 hand/draw/discard/exhaust 时不用)
//   hand/draw/discard/exhaust  ["strike","defend+"] 显式牌堆,顶牌在数组开头
//   enemies    [{id,hp,max_hp,block,powers,move}]
//   actions    [{"op":"play","hand":0,"target":0},{"op":"end_turn"},...]
//   report     开了以后每行额外带 "report":{candidates,costs}(这次选牌亮出的候选、
//              手牌当前实际费用);审计核对选牌池大小与"本回合 0 费"用,默认关
//   combats    [{"encounter":...,"hand":...,"enemies":...,"actions":...}, ...]
//              多场连打:血量与跨战斗遗物计数器接着上一场走;给了 combats 就
//              忽略根上的那场,每行多带 "c"(场次号)与 st.counters(计数器)
//
// 输出每行:`{"step":n,"op":"...",<state>}`;动作出错就 `{"step":n,"op":"...","error":"..."}`。
// state:turn/phase/energy/max_energy/player{hp,max_hp,block,powers}/
//        hand/draw/discard/exhaust(牌记号)/enemies[{id,hp,max_hp,block,dead,move,powers}]
//        (多场连打时另带 counters{pen_nib,happy_flower,incense,sundial,attacks_total,cards_total})
pub mod sandbox {
    use crate::core::card::CardInstance;
    use crate::core::combat::{Combat, CombatSetup, Phase};
    use crate::core::enemies;
    use crate::core::combat::Enemy;
    use crate::core::enemy::EnemyState;
    use crate::core::potions::{PotionDef, POTIONS};
    use crate::core::relics::RelicDef;
    use crate::core::status::{Status, Statuses};
    use crate::rng::RngRegistry;

    use super::js;

    // ---- 极简 JSON(只为读 scenario,不引依赖) ----

    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Json>),
        Obj(Vec<(String, Json)>),
    }

    impl Json {
        fn get(&self, key: &str) -> Option<&Json> {
            match self {
                Json::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }

        fn as_str(&self) -> Option<&str> {
            match self {
                Json::Str(s) => Some(s),
                _ => None,
            }
        }

        fn as_i64(&self) -> Option<i64> {
            match self {
                Json::Num(n) => Some(*n as i64),
                _ => None,
            }
        }

        fn as_arr(&self) -> Option<&[Json]> {
            match self {
                Json::Arr(a) => Some(a),
                _ => None,
            }
        }
    }

    pub fn parse_json(text: &str) -> Result<Json, String> {
        let b = text.as_bytes();
        let mut i = 0usize;
        let v = parse_value(b, &mut i)?;
        skip_ws(b, &mut i);
        if i != b.len() {
            return Err(format!("JSON 尾部有多余内容(第 {i} 字节)"));
        }
        Ok(v)
    }

    fn skip_ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && (b[*i] as char).is_whitespace() {
            *i += 1;
        }
    }

    fn parse_value(b: &[u8], i: &mut usize) -> Result<Json, String> {
        skip_ws(b, i);
        match b.get(*i) {
            None => Err("JSON 意外结束".to_string()),
            Some(b'{') => {
                *i += 1;
                let mut out: Vec<(String, Json)> = Vec::new();
                skip_ws(b, i);
                if b.get(*i) == Some(&b'}') {
                    *i += 1;
                    return Ok(Json::Obj(out));
                }
                loop {
                    skip_ws(b, i);
                    let k = parse_string(b, i)?;
                    skip_ws(b, i);
                    if b.get(*i) != Some(&b':') {
                        return Err(format!("JSON 对象缺冒号(第 {i} 字节)"));
                    }
                    *i += 1;
                    let v = parse_value(b, i)?;
                    out.push((k, v));
                    skip_ws(b, i);
                    match b.get(*i) {
                        Some(b',') => *i += 1,
                        Some(b'}') => {
                            *i += 1;
                            break;
                        }
                        _ => return Err(format!("JSON 对象缺逗号/右括号(第 {i} 字节)")),
                    }
                }
                Ok(Json::Obj(out))
            }
            Some(b'[') => {
                *i += 1;
                let mut out: Vec<Json> = Vec::new();
                skip_ws(b, i);
                if b.get(*i) == Some(&b']') {
                    *i += 1;
                    return Ok(Json::Arr(out));
                }
                loop {
                    let v = parse_value(b, i)?;
                    out.push(v);
                    skip_ws(b, i);
                    match b.get(*i) {
                        Some(b',') => *i += 1,
                        Some(b']') => {
                            *i += 1;
                            break;
                        }
                        _ => return Err(format!("JSON 数组缺逗号/右括号(第 {i} 字节)")),
                    }
                }
                Ok(Json::Arr(out))
            }
            Some(b'"') => Ok(Json::Str(parse_string(b, i)?)),
            Some(b't') => {
                expect(b, i, "true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                expect(b, i, "false")?;
                Ok(Json::Bool(false))
            }
            Some(b'n') => {
                expect(b, i, "null")?;
                Ok(Json::Null)
            }
            _ => {
                let start = *i;
                while *i < b.len()
                    && matches!(b[*i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    *i += 1;
                }
                let s = std::str::from_utf8(&b[start..*i]).map_err(|_| "JSON 数字不是 utf8")?;
                s.parse::<f64>()
                    .map(Json::Num)
                    .map_err(|_| format!("JSON 数字读不动: {s}"))
            }
        }
    }

    fn expect(b: &[u8], i: &mut usize, lit: &str) -> Result<(), String> {
        if b.len() >= *i + lit.len() && &b[*i..*i + lit.len()] == lit.as_bytes() {
            *i += lit.len();
            Ok(())
        } else {
            Err(format!("JSON 字面量不对(第 {} 字节,想要 {lit})", *i))
        }
    }

    fn parse_string(b: &[u8], i: &mut usize) -> Result<String, String> {
        if b.get(*i) != Some(&b'"') {
            return Err(format!("JSON 字符串缺引号(第 {} 字节)", *i));
        }
        *i += 1;
        let mut out = String::new();
        loop {
            let c = *b.get(*i).ok_or("JSON 字符串没闭合")?;
            *i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *b.get(*i).ok_or("JSON 转义没闭合")?;
                    *i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = std::str::from_utf8(
                                b.get(*i..*i + 4).ok_or("\\u 缺 4 位十六进制")?,
                            )
                            .map_err(|_| "\\u 不是 utf8")?;
                            let n = u32::from_str_radix(hex, 16)
                                .map_err(|_| format!("\\u 读不动: {hex}"))?;
                            *i += 4;
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("不认识的转义 \\{}", other as char)),
                    }
                }
                _ => {
                    // 逐字节收;多字节 UTF-8 直接复制原始字节
                    let start = *i - 1;
                    let mut end = *i;
                    while end < b.len() && (b[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    let s = std::str::from_utf8(&b[start..end]).map_err(|_| "JSON 字符串不是 utf8")?;
                    out.push_str(s);
                    *i = end;
                }
            }
        }
        Ok(out)
    }

    // ---- 牌记号与状态名的规范化 ----

    /// "strike"、"strike+"、"strike+2" -> 牌实例
    fn card_from_token(tok: &str) -> Result<CardInstance, String> {
        let mut id = tok;
        let mut plus = 0u8;
        if let Some(pos) = tok.rfind('+') {
            let tail = &tok[pos + 1..];
            if tail.is_empty() {
                plus = 1;
                id = &tok[..pos];
            } else if let Ok(n) = tail.parse::<u8>() {
                plus = n;
                id = &tok[..pos];
            }
        }
        let def = crate::core::cards::card_def(id)
            .ok_or_else(|| format!("不认识的牌 id: {tok}"))?;
        let mut c = CardInstance::new(def);
        if plus >= 1 {
            if c.multi_upgrade() {
                // 灼热攻击这类:每升一级的伤害加成记在 bonus 上,得走 upgrade()
                for _ in 0..plus {
                    c.upgrade();
                }
            } else {
                c.upgraded = true;
            }
        }
        Ok(c)
    }

    /// 状态键:全小写,空格换下划线(与参考实现的 power id 对齐后再比)
    fn status_key(s: Status) -> String {
        s.name().to_lowercase().replace(' ', "_")
    }

    fn status_from_key(key: &str) -> Option<Status> {
        let k = key.trim().to_lowercase().replace([' ', '-'], "_");
        Some(match k.as_str() {
            "strength" => Status::Strength,
            "dexterity" => Status::Dexterity,
            "vulnerable" => Status::Vulnerable,
            "weak" => Status::Weak,
            "frail" => Status::Frail,
            "entangled" => Status::Entangled,
            "demon_form" => Status::DemonForm,
            "metallicize" => Status::Metallicize,
            "feel_no_pain" => Status::FeelNoPain,
            "dark_embrace" => Status::DarkEmbrace,
            "evolve" => Status::Evolve,
            "fire_breathing" => Status::FireBreathing,
            "barricade" => Status::Barricade,
            "brutality" => Status::Brutality,
            "rupture" => Status::Rupture,
            "regenerate" => Status::Regenerate,
            "thorns" => Status::Thorns,
            "berserk" => Status::Berserk,
            "combust" => Status::Combust,
            "corruption" => Status::Corruption,
            "double_tap" => Status::DoubleTap,
            "juggernaut" => Status::Juggernaut,
            "rage" => Status::Rage,
            "flame_barrier" => Status::FlameBarrier,
            "artifact" => Status::Artifact,
            "no_block" => Status::NoBlock,
            "no_draw" => Status::NoDraw,
            "mayhem" => Status::Mayhem,
            "magnetism" => Status::Magnetism,
            "panache" => Status::Panache,
            "sadistic_nature" => Status::SadisticNature,
            "plated_armor" => Status::PlatedArmor,
            "constricted" => Status::Constricted,
            "confused" => Status::Confused,
            "hex" => Status::Hex,
            "draw_reduction" => Status::DrawReduction,
            "surrounded" => Status::Surrounded,
            "intangible" => Status::Intangible,
            "ritual" => Status::Ritual,
            "enrage" => Status::Enrage,
            // 参考实现叫 ANGRY,本作叫 Anger
            "angry" | "anger" => Status::Anger,
            "sharp_hide" => Status::SharpHide,
            "beat_of_death" => Status::BeatOfDeath,
            "curiosity" => Status::Curiosity,
            "curl_up" => Status::CurlUp,
            "explosive" => Status::Explosive,
            "fading" => Status::Fading,
            "flight" => Status::Flight,
            "invincible" => Status::Invincible,
            "malleable" => Status::Malleable,
            "minion" => Status::Minion,
            "minion_leader" => Status::MinionLeader,
            "mode_shift" => Status::ModeShift,
            "painful_stabs" => Status::PainfulStabs,
            "reactive" => Status::Reactive,
            "regrow" => Status::Regrow,
            "shifting" => Status::Shifting,
            "slow" => Status::Slow,
            "spore_cloud" => Status::SporeCloud,
            "split" => Status::Split,
            "stasis" => Status::Stasis,
            "strength_up" => Status::StrengthUp,
            "time_warp" => Status::TimeWarp,
            "asleep" => Status::Asleep,
            "duplication" => Status::Duplication,
            "lose_strength" => Status::LoseStrength,
            "lose_dexterity" => Status::LoseDexterity,
            "focus" => Status::Focus,
            "poison" => Status::Poison,
            _ => return None,
        })
    }

    fn powers_from_json(v: &Json) -> Result<Vec<(Status, i32)>, String> {
        let mut out = Vec::new();
        if let Json::Obj(m) = v {
            for (k, val) in m {
                let s = status_from_key(k).ok_or_else(|| format!("不认识的状态: {k}"))?;
                let n = val.as_i64().ok_or_else(|| format!("状态 {k} 的层数要是数字"))? as i32;
                out.push((s, n));
            }
        } else if !matches!(v, Json::Null) {
            return Err("powers 要是个对象".to_string());
        }
        Ok(out)
    }

    fn apply_powers(st: &mut Statuses, list: &[(Status, i32)]) {
        for (s, n) in list {
            if *n == 0 {
                st.mark(*s);
            } else {
                st.add(*s, *n);
            }
        }
    }

    // ---- scenario 模型 ----

    struct EnemySpec {
        id: String,
        /// None = 沿用引擎初始化掷出来的血量/格挡
        hp: Option<i32>,
        max_hp: Option<i32>,
        block: Option<i32>,
        powers: Vec<(Status, i32)>,
        move_id: Option<String>,
        slot: Option<usize>,
    }

    enum Action {
        Play {
            hand: usize,
            target: Option<usize>,
            choose: Vec<usize>,
        },
        EndTurn,
        Potion {
            slot: usize,
            target: Option<usize>,
            choose: Vec<usize>,
        },
        Noop,
    }

    /// 单场战斗的现场(多场连打时按顺序来)
    struct CombatSc {
        encounter: String,
        hand: Option<Vec<CardInstance>>,
        draw: Option<Vec<CardInstance>>,
        discard: Option<Vec<CardInstance>>,
        exhaust: Option<Vec<CardInstance>>,
        enemies: Vec<EnemySpec>,
        actions: Vec<Action>,
    }

    struct Scenario {
        hp: i32,
        max_hp: i32,
        gold: i32,
        rested: bool,
        player: Option<Json>,
        relics: Vec<&'static RelicDef>,
        potions: Vec<Option<&'static PotionDef>>,
        deck: Vec<CardInstance>,
        /// 一场或多场战斗:写了 combats 就按数组顺序连打,否则根上那场单独打
        combats: Vec<CombatSc>,
        /// 每行额外带上"这次选牌亮出的候选"与"手牌当前实际费用"(审计用);
        /// 默认关,免得改动 sandbox 与参考侧逐字段比对的 schema
        report: bool,
    }

    fn need_str<'a>(v: &'a Json, key: &str) -> Result<&'a str, String> {
        v.get(key)
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("scenario 缺字符串字段 {key}"))
    }

    fn num_or(v: &Json, key: &str, dflt: i32) -> Result<i32, String> {
        match v.get(key) {
            None | Some(Json::Null) => Ok(dflt),
            Some(x) => x
                .as_i64()
                .map(|n| n as i32)
                .ok_or_else(|| format!("scenario.{key} 要是数字")),
        }
    }

    fn cards_from_arr(v: &Json, key: &str) -> Result<Option<Vec<CardInstance>>, String> {
        match v.get(key) {
            None | Some(Json::Null) => Ok(None),
            Some(Json::Arr(a)) => {
                let mut out = Vec::new();
                for x in a {
                    let s = x
                        .as_str()
                        .ok_or_else(|| format!("scenario.{key} 里要放牌名字符串"))?;
                    out.push(card_from_token(s)?);
                }
                Ok(Some(out))
            }
            Some(_) => Err(format!("scenario.{key} 要是数组")),
        }
    }

    /// 解析一场战斗的现场(encounter/enemies/牌堆/动作)
    fn parse_combat(v: &Json) -> Result<CombatSc, String> {
        let mut cs = CombatSc {
            encounter: String::new(),
            hand: None,
            draw: None,
            discard: None,
            exhaust: None,
            enemies: Vec::new(),
            actions: Vec::new(),
        };
        if let Some(e) = v.get("encounter") {
            cs.encounter = e.as_str().ok_or("encounter 要是字符串")?.to_string();
        }
        cs.hand = cards_from_arr(v, "hand")?;
        cs.draw = cards_from_arr(v, "draw")?;
        cs.discard = cards_from_arr(v, "discard")?;
        cs.exhaust = cards_from_arr(v, "exhaust")?;
        if let Some(arr) = v.get("enemies").and_then(|x| x.as_arr()) {
            for x in arr {
                let id = need_str(x, "id")?.to_string();
                let hp = x.get("hp").and_then(|y| y.as_i64()).map(|n| n as i32);
                let max_hp = x.get("max_hp").and_then(|y| y.as_i64()).map(|n| n as i32);
                let block = x.get("block").and_then(|y| y.as_i64()).map(|n| n as i32);
                let powers = match x.get("powers") {
                    Some(p) => powers_from_json(p)?,
                    None => Vec::new(),
                };
                let move_name = match x.get("move") {
                    None | Some(Json::Null) => None,
                    Some(m) => Some(m.as_str().ok_or("enemy.move 要是字符串")?.to_string()),
                };
                let slot = match x.get("slot") {
                    None | Some(Json::Null) => None,
                    Some(s) => Some(s.as_i64().ok_or("enemy.slot 要是数字")? as usize),
                };
                cs.enemies.push(EnemySpec {
                    id,
                    hp,
                    max_hp,
                    block,
                    powers,
                    move_id: move_name,
                    slot,
                });
            }
        }
        if let Some(arr) = v.get("actions").and_then(|x| x.as_arr()) {
            for x in arr {
                let op = need_str(x, "op")?;
                let choose: Vec<usize> = match x.get("choose").and_then(|y| y.as_arr()) {
                    None => vec![0],
                    Some(a) => a
                        .iter()
                        .map(|y| y.as_i64().unwrap_or(0) as usize)
                        .collect(),
                };
                match op {
                    "play" => {
                        let hand = num_or(x, "hand", 0)? as usize;
                        let target = match x.get("target") {
                            None | Some(Json::Null) => None,
                            Some(t) => Some(t.as_i64().ok_or("action.target 要是数字")? as usize),
                        };
                        cs.actions.push(Action::Play { hand, target, choose });
                    }
                    "end_turn" | "endTurn" => cs.actions.push(Action::EndTurn),
                    "potion" => {
                        let slot = num_or(x, "slot", 0)? as usize;
                        let target = match x.get("target") {
                            None | Some(Json::Null) => None,
                            Some(t) => Some(t.as_i64().ok_or("action.target 要是数字")? as usize),
                        };
                        cs.actions.push(Action::Potion { slot, target, choose });
                    }
                    "noop" | "snapshot" => cs.actions.push(Action::Noop),
                    other => return Err(format!("不认识的动作: {other}")),
                }
            }
        }
        Ok(cs)
    }

    fn parse_scenario(root: &Json) -> Result<Scenario, String> {
        if !matches!(root, Json::Obj(_)) {
            return Err("scenario 根要是个对象".to_string());
        }
        let mut sc = Scenario {
            hp: 80,
            max_hp: 80,
            gold: 99,
            rested: false,
            player: root.get("player").cloned(),
            relics: Vec::new(),
            potions: Vec::new(),
            deck: Vec::new(),
            combats: Vec::new(),
            report: matches!(root.get("report"), Some(Json::Bool(true))),
        };
        if let Some(p) = root.get("player") {
            sc.hp = num_or(p, "hp", sc.hp)?;
            sc.max_hp = num_or(p, "max_hp", sc.max_hp)?;
        }
        sc.gold = num_or(root, "gold", sc.gold)?;
        if let Some(v) = root.get("rested") {
            sc.rested = matches!(v, Json::Bool(true));
        }
        if let Some(arr) = root.get("relics").and_then(|v| v.as_arr()) {
            for x in arr {
                let id = x.as_str().ok_or("relics 里要放字符串")?;
                let d = crate::core::relics::relic_def(id)
                    .ok_or_else(|| format!("不认识的遗物: {id}"))?;
                sc.relics.push(d);
            }
        }
        if let Some(arr) = root.get("potions").and_then(|v| v.as_arr()) {
            for x in arr {
                if matches!(x, Json::Null) {
                    sc.potions.push(None);
                } else {
                    let id = x.as_str().ok_or("potions 里要放字符串或 null")?;
                    let d = potion_def(id).ok_or_else(|| format!("不认识的药水: {id}"))?;
                    sc.potions.push(Some(d));
                }
            }
        }
        if let Some(arr) = cards_from_arr(root, "deck")? {
            sc.deck = arr;
        }
        sc.combats = match root.get("combats") {
            Some(Json::Arr(a)) => {
                let mut v = Vec::new();
                for x in a {
                    v.push(parse_combat(x)?);
                }
                v
            }
            Some(_) => return Err("combats 要是数组".to_string()),
            None => vec![parse_combat(root)?],
        };
        if sc.combats.is_empty() {
            return Err("scenario 至少要有一场战斗".to_string());
        }
        Ok(sc)
    }

    fn potion_def(id: &str) -> Option<&'static PotionDef> {
        POTIONS.iter().find(|p| p.id == id)
    }

    // ---- 局面构造与覆盖 ----

    /// 把 scenario 里的敌人摆出来:以引擎初始化那只怪为底(遗物在开战时挂上的
    /// 减益/格挡要留着),再按 scenario 覆盖血量、追加状态、指定招式.
    fn build_enemies(specs: &[EnemySpec], init: &[Enemy]) -> Result<Vec<Enemy>, String> {
        let mut out = Vec::new();
        for (i, s) in specs.iter().enumerate() {
            let def = enemies::enemy_def_or_panic(&s.id);
            // 只有占位遭遇那一格本来就是同一种怪时才沿用它的现场:
            // 这样遗物开战挂的减益留得住,又不会把占位怪的出生掷点
            // (比如小虱子的蜷缩)带到 scenario 指定的其它怪身上。
            let base = init.get(i).filter(|b| b.def.id == def.id);
            let mut statuses = Statuses::new();
            if let Some(b) = base {
                for (st, n) in b.statuses.iter() {
                    if n == 0 {
                        statuses.mark(st);
                    } else {
                        statuses.add(st, n);
                    }
                }
            }
            apply_powers(&mut statuses, &s.powers);
            let next_move = match &s.move_id {
                // 没点名招式时,如果占位遭遇那一格本来就是同一种怪,就用引擎开局替它
                // 掷出来的那一招(参考侧沙盒也是开局 getMove 掷首招,不是固定第 0 招);
                // 不同种怪的那一格没有这种现场,只能退回第 0 招.
                None => base.map(|b| b.next_move).unwrap_or(0),
                Some(name) => def
                    .move_index(name)
                    .ok_or_else(|| format!("{} 没有叫 {name} 的招", s.id))?,
            };
            // 参考实现的"上过几招"记在 moveHistory 里(初始化时把首招也压进去),
            // 这边用 state.turns/state.last 表示同一件事:把这一招当成
            // "初始化时掷出来的首招",两边的 AI 历史才对得上(否则邪教徒会
            // 一直放充能,因为它以为还没出过手)。
            let mut state = EnemyState::default();
            state.last = Some(next_move);
            // 参考实现里首招一掷出 moveHistory 就非空:之后不再走"开局三选一"分支
            state.move_rolled = true;
            let hp = s.hp.or(base.map(|b| b.hp)).unwrap_or_else(|| def.hp.0);
            out.push(Enemy {
                def,
                name: def.name.to_string(),
                hp,
                max_hp: s.max_hp.or(base.map(|b| b.max_hp)).unwrap_or(hp),
                block: s.block.or(base.map(|b| b.block)).unwrap_or(0),
                statuses,
                next_move,
                death_done: false,
                temp_strength: 0,
                fresh_powers: Vec::new(),
                escaped: false,
                slot: s.slot.or(base.map(|b| b.slot)).unwrap_or(i),
                uid: i as u64,
                // 对拍/回放脚本按 A0 构造:飞升等级固定 0
                asc: 0,
                state,
            });
        }
        Ok(out)
    }

    /// 打地鼠:给 n 只敌人找一个固定阵容的遭遇当占位(数量只影响 RNG 消耗,
    /// 数量对上以后两边的掷点流也一致;之后掷点流还会被重置,见下)
    fn placeholder_encounter(n: usize) -> &'static crate::core::enemy::Encounter {
        let id = match n {
            0 => "cultist_solo",
            1 => "cultist_solo",
            2 => "two_louses",
            3 => "three_louses",
            4 => "gremlin_gang",
            _ => "lots_of_slimes",
        };
        enemies::encounter_def(id).expect("占位遭遇要在表里")
    }

    // ---- 状态输出 ----

    fn pile_json(pile: &[CardInstance]) -> String {
        let items: Vec<String> = pile
            .iter()
            .map(|c| super::card_token(c))
            .collect();
        format!("[{}]", items.join(","))
    }

    fn move_name(c: &Combat, i: usize) -> String {
        let e = &c.enemies[i];
        let raw = e.def.moves[e.next_move].name;
        let key = raw.to_lowercase().replace([' ', '-'], "_");
        // 参考实现的招式 id 带怪物前缀(e.g. gladiator_incantation),统一剥掉
        let prefix = format!("{}_", e.def.id.to_lowercase());
        key.strip_prefix(&prefix).unwrap_or(&key).to_string()
    }

    /// 玩家身上的能力值,含本作不挂 statuses、改记在别处的那些
    /// (赤牛的振奋、铜鳞的荆棘、化石螺壳的缓冲、诱变力量的回合末扣力、定时炸弹)
    fn player_powers_json(c: &Combat) -> String {
        let mut items: Vec<(String, i32)> = c
            .player
            .statuses
            .iter()
            .filter(|(_, n)| *n != 0)
            .map(|(s, n)| (status_key(s), n))
            .collect();
        let mut bump = |key: &str, d: i32| {
            if d == 0 {
                return;
            }
            match items.iter_mut().find(|(k, _)| k == key) {
                Some(e) => e.1 += d,
                None => items.push((key.to_string(), d)),
            }
        };
        bump("thorns", c.relic_thorns());
        bump("vigor", c.rs.vigor);
        bump("buffer", c.rs.helix);
        bump("lose_strength", c.rs.strength_turn1);
        bump("the_bomb", c.bomb_turns());
        items.retain(|(_, n)| *n != 0);
        let body: Vec<String> = items
            .iter()
            .map(|(k, n)| format!("{}:{}", js(k), n))
            .collect();
        format!("{{{}}}", body.join(","))
    }

    /// 敌人身上的能力值:回合末回补的力量记在 temp_strength 上,报成参考实现里的
    /// 同名 power —— 黑暗镣铐是 generic_strength_up,瞬变体的移形换影是 shackled.
    fn enemy_powers_json(e: &Enemy) -> String {
        let mut items: Vec<(String, i32)> = e
            .statuses
            .iter()
            .filter(|(_, n)| *n != 0)
            .map(|(s, n)| (status_key(s), n))
            .collect();
        if e.temp_strength != 0 {
            let key = if e.statuses.holds(Status::Shifting) {
                "shackled"
            } else {
                "generic_strength_up"
            };
            items.push((key.to_string(), e.temp_strength));
        }
        items.retain(|(_, n)| *n != 0);
        let body: Vec<String> = items
            .iter()
            .map(|(k, n)| format!("{}:{}", js(k), n))
            .collect();
        format!("{{{}}}", body.join(","))
    }

    /// 跨战斗的遗物计数器(多场连打时随输出走,单场不输出以保持与参考侧同 schema)
    fn counters_json(c: &Combat) -> String {
        let rc = c.rs.run_counters();
        format!(
            ",\"counters\":{{\"pen_nib\":{},\"happy_flower\":{},\"incense\":{},\"sundial\":{},\"attacks_total\":{},\"cards_total\":{}}}",
            rc.pen_nib, rc.happy_flower, rc.incense, rc.sundial, rc.attacks_total, rc.cards_total
        )
    }

    fn state_json(c: &Combat, potions: &[Option<&'static PotionDef>], extra: &str) -> String {
        let enemies: Vec<String> = (0..c.enemies.len())
            .map(|i| {
                let e = &c.enemies[i];
                format!(
                    "{{\"id\":{},\"hp\":{},\"max_hp\":{},\"block\":{},\"dead\":{},\"move\":{},\"powers\":{}}}",
                    js(e.def.id),
                    e.hp,
                    e.max_hp,
                    e.block,
                    if e.alive() { "false" } else { "true" },
                    js(&move_name(c, i)),
                    enemy_powers_json(e)
                )
            })
            .collect();
        let pots: Vec<String> = potions
            .iter()
            .map(|p| p.map(|d| js(d.id)).unwrap_or_else(|| "null".to_string()))
            .collect();
        let phase = match c.phase {
            Phase::PlayerTurn => "player",
            Phase::EnemyTurn => "enemy",
            Phase::Won => "won",
            Phase::Lost => "lost",
        };
        format!(
            "{{\"turn\":{},\"phase\":{},\"energy\":{},\"max_energy\":{},\
             \"player\":{{\"hp\":{},\"max_hp\":{},\"block\":{},\"powers\":{}}},\
             \"hand\":{},\"draw\":{},\"discard\":{},\"exhaust\":{},\
             \"potions\":[{}],\"enemies\":[{}]{extra}}}",
            c.turn,
            js(phase),
            c.energy,
            c.max_energy,
            c.player.hp,
            c.player.max_hp,
            c.player.block,
            player_powers_json(c),
            pile_json(&c.hand),
            pile_json(&c.draw),
            pile_json(&c.discard),
            pile_json(&c.exhaust),
            pots.join(","),
            enemies.join(",")
        )
    }

    fn line(step: usize, op: &str, ci: Option<usize>, rest: &str) -> String {
        let tag = ci.map(|i| format!("\"c\":{i},")).unwrap_or_default();
        format!("{{\"step\":{step},\"op\":{},{tag}{rest}}}\n", js(op))
    }

    /// ci 为 Some 时输出多场连打的场次号与遗物计数器
    fn snapshot(
        step: usize,
        op: &str,
        c: &Combat,
        potions: &[Option<&'static PotionDef>],
        ci: Option<usize>,
        tail: &str,
    ) -> String {
        let extra = if ci.is_some() { counters_json(c) } else { String::new() };
        line(step, op, ci, &format!("\"st\":{}{tail}", state_json(c, potions, &extra)))
    }

    /// scenario 开了 report 时,每行额外带"这次选牌亮出的候选"与"手牌当前实际费用"
    /// (不可打出记 -1).审计靠它核对选牌池大小与"本回合 0 费"这类不在状态里的约束。
    fn report_tail(report: bool, c: &Combat, offered: &[String]) -> String {
        if !report {
            return String::new();
        }
        // 候选 token 由 card_token 生成时已带 JSON 引号,直接拼
        let cands: Vec<String> = offered.to_vec();
        let costs: Vec<String> = c
            .hand
            .iter()
            .map(|x| match x.fixed_cost() {
                Some(n) => n.to_string(),
                None => "-1".to_string(),
            })
            .collect();
        format!(
            ",\"report\":{{\"candidates\":[{}],\"costs\":[{}]}}",
            cands.join(","),
            costs.join(",")
        )
    }

    // ---- 跑一段 scenario ----

    /// 选牌窗口:按 scenario 给的 indices 依次选,然后收尾(给空数组就是不选).
    /// indices 是"候选表里的第几个"(与参考实现的 chosen 语义一致),
    /// 这里翻译成本作使用的牌堆下标.返回第一轮亮出来的候选(审计报告用).
    fn resolve_choice(c: &mut Combat, indices: &[usize]) -> Vec<String> {
        let mut offered: Vec<String> = Vec::new();
        let mut guard = 0;
        while c.choice.is_some() && guard < 16 {
            guard += 1;
            if guard == 1 {
                offered = c
                    .choice_candidates()
                    .iter()
                    .map(|(_, x)| super::card_token(x))
                    .collect();
            }
            for &rel in indices {
                if c.choice.is_none() {
                    break;
                }
                let picked = c
                    .choice_candidates()
                    .get(rel)
                    .map(|(i, _)| *i);
                if let Some(i) = picked {
                    let _ = c.choose(i);
                }
            }
            if c.choice.is_some() {
                c.finish_choice();
            }
        }
        offered
    }

    /// 按 scenario 摆好一场战斗:牌堆 / 玩家 / 敌人覆盖,并把掷点流重置到同一颗种子.
    fn build_combat(
        cs: &CombatSc,
        sc: &Scenario,
        hp: i32,
        max_hp: i32,
        counters: crate::core::combat::RunRelicCounters,
        seed: u64,
    ) -> Result<Combat, String> {
        if cs.enemies.is_empty() {
            return Err("scenario 至少要有一只敌人".to_string());
        }
        let enc = if cs.encounter.is_empty() {
            placeholder_encounter(cs.enemies.len())
        } else {
            enemies::encounter_def(&cs.encounter)
                .ok_or_else(|| format!("不认识的遭遇: {}", cs.encounter))?
        };
        // 显式牌堆存在时,牌组原件 = hand+draw+discard+exhaust(顺序与参考侧一致)
        let explicit = cs.hand.is_some() || cs.draw.is_some() || cs.discard.is_some() || cs.exhaust.is_some();
        let deck: Vec<CardInstance> = if explicit {
            let mut d = Vec::new();
            for part in [&cs.hand, &cs.draw, &cs.discard, &cs.exhaust] {
                if let Some(p) = part {
                    d.extend(p.iter().cloned());
                }
            }
            d
        } else {
            sc.deck.clone()
        };
        let setup = CombatSetup {
            hp,
            max_hp,
            deck,
            relics: sc.relics.clone(),
            gold: sc.gold,
            rested: sc.rested,
            lift_strength: 0,
            relic_counters: counters,
            curse_negate: 0,
            asc: 0,
        };
        let mut c = Combat::new(enc, setup, RngRegistry::new(seed));

        // 覆盖:显式牌堆
        if explicit {
            c.hand = cs.hand.clone().unwrap_or_default();
            c.draw = cs.draw.clone().unwrap_or_default();
            c.discard = cs.discard.clone().unwrap_or_default();
            c.exhaust = cs.exhaust.clone().unwrap_or_default();
        }
        // 覆盖:玩家(hp/max_hp 由 setup 带进来,多场连打时才能接着上一场)
        if let Some(p) = sc.player.as_ref() {
            c.player.block = num_or(p, "block", c.player.block)?;
            if let Some(e) = p.get("energy") {
                c.energy = e.as_i64().ok_or("player.energy 要是数字")? as i32;
            }
            if let Some(e) = p.get("max_energy") {
                c.max_energy = e.as_i64().ok_or("player.max_energy 要是数字")? as i32;
            }
            if p.get("powers").is_some() {
                // scenario 里的 powers 是覆盖,不是叠加
                c.player.statuses = Statuses::new();
                let list = powers_from_json(p.get("powers").unwrap())?;
                apply_powers(&mut c.player.statuses, &list);
            }
        }
        // 覆盖:敌人(数量要对上)
        if !cs.enemies.is_empty() {
            let want = cs.enemies.len();
            if c.enemies.len() != want {
                return Err(format!(
                    "敌人数量对不上:遭遇给了 {},scenario 要 {want}",
                    c.enemies.len()
                ));
            }
            let init = c.enemies.clone();
            c.enemies = build_enemies(&cs.enemies, &init)?;
        }
        // 初始化时挂起的选牌(赌徒筹码这类开战就选牌的遗物)一律按"一张不选"收掉:
        // scenario 已经把牌堆摆成想要的样子了,构造期的选择只是初始化副作用.
        if c.choice.is_some() {
            let _ = resolve_choice(&mut c, &[]);
        }
        // 掷点流重置:初始化阶段两边消耗的掷点数可能不同,重置成同一颗种子
        // 之后,动作阶段的随机(洗牌/随机目标/随机卡)才能逐步对齐。
        c.streams = RngRegistry::new(seed);
        Ok(c)
    }

    pub fn run(seed: u64, text: &str) -> Result<String, String> {
        let root = parse_json(text)?;
        // scenario 带 "event" 字段就是事件沙盒(战斗沙盒的 scenario 没有这个键,
        // 现有 schema 与输出都不变)
        if root.get("event").is_some() {
            return run_event(seed, &root);
        }
        let sc = parse_scenario(&root)?;
        // 写了 combats 就是多场连打:输出多带场次号与遗物计数器,血量与计数器跨场继承
        let multi = root.get("combats").is_some();
        let mut carried = crate::core::combat::RunRelicCounters::default();
        let mut hp = sc.hp;
        let mut max_hp = sc.max_hp;
        let mut potions = sc.potions.clone();
        while potions.len() < 3 {
            potions.push(None);
        }
        let mut out = String::new();
        let mut step = 0usize;
        let report = sc.report;
        for (ci, cs) in sc.combats.iter().enumerate() {
            let tag = if multi { Some(ci) } else { None };
            let mut c = build_combat(cs, &sc, hp, max_hp, carried.clone(), seed)?;
            let tail = report_tail(report, &c, &[]);
            out.push_str(&snapshot(step, "init", &c, &potions, tag, &tail));
            for a in cs.actions.iter() {
                step += 1;
                // 开战就挂起的选牌(赌徒之骰/工具箱)先按动作给的 choose 收掉,
                // 否则下一步打牌会被"还有选牌没选"挡住,与参考侧对不上。
                let action_choose: Vec<usize> = match a {
                    Action::Play { choose, .. } | Action::Potion { choose, .. } => choose.clone(),
                    _ => vec![0],
                };
                let mut offered: Vec<String> = Vec::new();
                if c.choice.is_some() {
                    offered = resolve_choice(&mut c, &action_choose);
                }
                match a {
                    Action::Play { hand, target, choose } => match c.play_card(*hand, *target) {
                        Ok(()) => {
                            let more = resolve_choice(&mut c, choose);
                            if offered.is_empty() {
                                offered = more;
                            }
                            let tail = report_tail(report, &c, &offered);
                            out.push_str(&snapshot(step, "play", &c, &potions, tag, &tail));
                        }
                        Err(e) => {
                            out.push_str(&line(step, "play", tag, &format!("\"error\":{}", js(e))));
                            break;
                        }
                    },
                    Action::EndTurn => {
                        c.end_turn();
                        let more = resolve_choice(&mut c, &[0]);
                        if offered.is_empty() {
                            offered = more;
                        }
                        let tail = report_tail(report, &c, &offered);
                        out.push_str(&snapshot(step, "end_turn", &c, &potions, tag, &tail));
                    }
                    Action::Potion { slot, target, choose } => {
                        let Some(Some(def)) = potions.get(*slot).copied() else {
                            out.push_str(&line(
                                step,
                                "potion",
                                tag,
                                "\"error\":\"no potion in slot\"",
                            ));
                            break;
                        };
                        c.use_potion(def, *target);
                        potions[*slot] = None;
                        let more = resolve_choice(&mut c, choose);
                        if offered.is_empty() {
                            offered = more;
                        }
                        let tail = report_tail(report, &c, &offered);
                        out.push_str(&snapshot(step, "potion", &c, &potions, tag, &tail));
                    }
                    Action::Noop => {
                        let tail = report_tail(report, &c, &offered);
                        out.push_str(&snapshot(step, "noop", &c, &potions, tag, &tail));
                    }
                }
            }
            // 结算这一场:血量与跨战斗遗物计数器带走,下一场接着来
            carried = c.rs.run_counters();
            hp = c.player.hp;
            max_hp = c.player.max_hp;
            step += 1;
        }
        Ok(out)
    }

    /// 批量跑:清单每行 `名字<TAB>scenario.json 路径`(空行与 # 注释跳过).
    /// 输出每段前面一行 `#名字` 作分隔,省得为一千个 scenario 起一千个进程.
    pub fn run_batch(seed: u64, list_text: &str) -> String {
        let mut out = String::new();
        for raw in list_text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, path) = match line.split_once('\t') {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (line, line),
            };
            out.push_str(&format!("#{name}\n"));
            match std::fs::read_to_string(path) {
                Err(e) => out.push_str(&format!(
                    "{{\"step\":-1,\"op\":\"fail\",\"error\":{}}}\n",
                    js(&format!("读不了 {path}: {e}"))
                )),
                Ok(text) => match run(seed, &text) {
                    Ok(jsonl) => out.push_str(&jsonl),
                    Err(e) => out.push_str(&format!(
                        "{{\"step\":-1,\"op\":\"fail\",\"error\":{}}}\n",
                        js(&e)
                    )),
                },
            }
        }
        out
    }

    // ---- 事件沙盒 ----
    //
    // scenario 里带 "event" 字段时走这里:按给出的一局摆好局面,打开该事件,
    // 再按 actions 逐条驱动真实代码路径(choosing/picker/fight/...),每步一行 JSONL。
    // 与战斗沙盒共用同一套 JSON 解析与牌记号,输出 schema 另一套,互不影响。

    enum EvAction {
        Choose(usize),
        Pick(usize),
        Fight { win: bool },
        RewardTake,
        RewardLeave,
        Noop,
    }

    struct EvScenario {
        event: String,
        /// true 走 debug_room("event <id>")(要 onEnter 掷点的事件)
        room: bool,
        floor: u32,
        asc: u32,
        character: String,
        hp: i32,
        max_hp: i32,
        gold: i32,
        relics: Option<Vec<&'static RelicDef>>,
        deck: Option<Vec<CardInstance>>,
        potions: Option<Vec<Option<&'static PotionDef>>>,
        actions: Vec<EvAction>,
    }

    fn parse_ev_scenario(root: &Json) -> Result<EvScenario, String> {
        let event = root
            .get("event")
            .and_then(|x| x.as_str())
            .ok_or("scenario 缺 event 字段")?
            .to_string();
        let room = matches!(root.get("room"), Some(Json::Bool(true)));
        let floor = root.get("floor").and_then(|x| x.as_i64()).unwrap_or(0).max(0) as u32;
        let asc = root.get("asc").and_then(|x| x.as_i64()).unwrap_or(0).max(0) as u32;
        let character = root
            .get("character")
            .and_then(|x| x.as_str())
            .unwrap_or("ironclad")
            .to_string();
        let mut hp = 70;
        let mut max_hp = 80;
        if let Some(p) = root.get("player") {
            hp = num_or(p, "hp", hp)?;
            max_hp = num_or(p, "max_hp", max_hp)?;
        }
        hp = num_or(root, "hp", hp)?;
        max_hp = num_or(root, "max_hp", max_hp)?;
        let gold = num_or(root, "gold", 99)?;
        let relics = match root.get("relics") {
            None | Some(Json::Null) => None,
            Some(Json::Arr(a)) => {
                let mut v = Vec::new();
                for x in a {
                    let id = x.as_str().ok_or("relics 里要放字符串")?;
                    let d = crate::core::relics::relic_def(id)
                        .ok_or_else(|| format!("不认识的遗物: {id}"))?;
                    v.push(d);
                }
                Some(v)
            }
            Some(_) => return Err("relics 要是数组".to_string()),
        };
        let deck = cards_from_arr(root, "deck")?;
        let potions = match root.get("potions") {
            None | Some(Json::Null) => None,
            Some(Json::Arr(a)) => {
                let mut v: Vec<Option<&'static PotionDef>> = Vec::new();
                for x in a {
                    if matches!(x, Json::Null) {
                        v.push(None);
                    } else {
                        let id = x.as_str().ok_or("potions 里要放字符串或 null")?;
                        let d = potion_def(id).ok_or_else(|| format!("不认识的药水: {id}"))?;
                        v.push(Some(d));
                    }
                }
                Some(v)
            }
            Some(_) => return Err("potions 要是数组".to_string()),
        };
        let mut actions = Vec::new();
        if let Some(arr) = root.get("actions").and_then(|x| x.as_arr()) {
            for x in arr {
                let op = need_str(x, "op")?;
                match op {
                    "choose" | "flip" => actions.push(EvAction::Choose(num_or(x, "i", 0)? as usize)),
                    "pick" => actions.push(EvAction::Pick(num_or(x, "i", 0)? as usize)),
                    "fight" => actions.push(EvAction::Fight {
                        win: !matches!(x.get("win"), Some(Json::Bool(false))),
                    }),
                    "reward_take" | "take" => actions.push(EvAction::RewardTake),
                    "reward_leave" | "leave" => actions.push(EvAction::RewardLeave),
                    "noop" | "snapshot" => actions.push(EvAction::Noop),
                    other => return Err(format!("不认识的事件动作: {other}")),
                }
            }
        }
        Ok(EvScenario {
            event,
            room,
            floor,
            asc,
            character,
            hp,
            max_hp,
            gold,
            relics,
            deck,
            potions,
            actions,
        })
    }

    /// 事件沙盒当前状态一行 JSON:血/钱/飞升/层/屏/事件与结果与当前 str screen、
    /// 每选项 {label,enabled}、牌组/遗物/药水、开战时的遭遇、开奖励屏时的内容。
    fn ev_state_json(run: &crate::core::run::Run) -> String {
        let deck: Vec<String> = run.player.deck.iter().map(super::card_token).collect();
        let relics: Vec<String> = run.player.relics.iter().map(|r| js(r.id)).collect();
        let potions: Vec<String> = run
            .player
            .potions
            .iter()
            .map(|p| p.map(|d| js(d.id)).unwrap_or_else(|| "null".to_string()))
            .collect();
        let (eid, result, escreen, attempts) = match &run.event {
            Some(st) => (
                js(st.def.id),
                st.result
                    .as_deref()
                    .map(js)
                    .unwrap_or_else(|| "null".to_string()),
                st.screen.map(js).unwrap_or_else(|| "null".to_string()),
                st.attempts,
            ),
            None => (
                "null".to_string(),
                "null".to_string(),
                "null".to_string(),
                0,
            ),
        };
        let n = run.event_choice_count();
        let mut choices: Vec<String> = Vec::new();
        for i in 0..n {
            let (label, cg, ch) = run
                .event_choice_row(i)
                .unwrap_or_else(|| (String::new(), 0, 0));
            choices.push(format!(
                "{{\"label\":{},\"cost_gold\":{},\"cost_hp\":{},\"enabled\":{}}}",
                js(&label),
                cg,
                ch,
                run.event_choice_available(i)
            ));
        }
        let combat = match run.combat() {
            Some(c) => {
                let phase = match c.phase {
                    Phase::PlayerTurn => "player",
                    Phase::EnemyTurn => "enemy",
                    Phase::Won => "won",
                    Phase::Lost => "lost",
                };
                format!(
                    "{{\"encounter\":{},\"turn\":{},\"phase\":{}}}",
                    js(c.encounter_id),
                    c.turn,
                    js(phase)
                )
            }
            None => "null".to_string(),
        };
        let reward = match run.reward.as_ref() {
            Some(r) => {
                let cards: Vec<String> = r.cards.iter().map(super::card_token).collect();
                let pots: Vec<String> = r.potions.iter().map(|p| js(p.id)).collect();
                format!(
                    "{{\"gold\":{},\"cards\":[{}],\"relic\":{},\"potions\":[{}]}}",
                    r.gold,
                    cards.join(","),
                    r.relic.map(|d| js(d.id)).unwrap_or_else(|| "null".to_string()),
                    pots.join(",")
                )
            }
            None => "null".to_string(),
        };
        let adv = match run.event.as_ref().and_then(|s| s.adv) {
            Some(d) => format!(
                "{{\"rewards\":[{},{},{}],\"encounter\":{},\"phase\":{}}}",
                js(d.rewards[0]),
                js(d.rewards[1]),
                js(d.rewards[2]),
                js(d.encounter),
                d.phase
            ),
            None => "null".to_string(),
        };
        format!(
            "{{\"hp\":{},\"max_hp\":{},\"gold\":{},\"asc\":{},\"floor\":{},\"screen\":{},\
             \"event\":{},\"result\":{},\"event_screen\":{},\"attempts\":{},\
             \"deck\":[{}],\"relics\":[{}],\"potions\":[{}],\"choices\":[{}],\
             \"encounter\":{},\"reward\":{},\"adv\":{}}}",
            run.player.hp,
            run.player.max_hp,
            run.player.gold,
            run.ascension,
            run.debug_floor(),
            js(run.screen.name()),
            eid,
            result,
            escreen,
            attempts,
            deck.join(","),
            relics.join(","),
            potions.join(","),
            choices.join(","),
            combat,
            reward,
            adv
        )
    }

    fn ev_line(step: usize, op: &str, run: &crate::core::run::Run) -> String {
        format!("{{\"step\":{step},\"op\":{},\"st\":{}}}\n", js(op), ev_state_json(run))
    }

    fn ev_line_err(step: usize, op: &str, err: &str, run: &crate::core::run::Run) -> String {
        format!(
            "{{\"step\":{step},\"op\":{},\"error\":{},\"st\":{}}}\n",
            js(op),
            js(err),
            ev_state_json(run)
        )
    }

    /// 跑一段事件 scenario
    pub fn run_event(seed: u64, root: &Json) -> Result<String, String> {
        let sc = parse_ev_scenario(root)?;
        let ch = crate::core::corpus::CHARACTERS
            .iter()
            .find(|c| c.id == sc.character)
            .ok_or_else(|| format!("不认识的角色: {}", sc.character))?;
        let mut run = crate::core::run::Run::new_for_asc(seed, ch, sc.asc)?;
        if let Some(r) = sc.relics.as_ref() {
            run.player.relics = r.clone();
        }
        if let Some(d) = sc.deck.as_ref() {
            run.player.deck = d.clone();
        }
        if let Some(p) = sc.potions.as_ref() {
            let mut v = p.clone();
            while v.len() < 3 {
                v.push(None);
            }
            run.player.potions = v;
        }
        run.player.hp = sc.hp;
        run.player.max_hp = sc.max_hp;
        run.player.gold = sc.gold;
        run.debug_set_floor(sc.floor);
        if sc.room {
            run.debug_room(&format!("event {}", sc.event))?;
        } else {
            run.debug_open_event(&sc.event)?;
        }
        let mut out = String::new();
        let mut step = 0usize;
        out.push_str(&ev_line(0, "open", &run));
        for a in sc.actions.iter() {
            step += 1;
            match a {
                EvAction::Choose(i) => match run.choose_event(*i) {
                    Ok(()) => out.push_str(&ev_line(step, "choose", &run)),
                    Err(e) => {
                        out.push_str(&ev_line_err(step, "choose", &e, &run));
                        break;
                    }
                },
                EvAction::Pick(i) => {
                    if let Some(p) = run.picker.as_mut() {
                        p.index = *i;
                    }
                    match run.picker_confirm() {
                        Ok(_) => out.push_str(&ev_line(step, "pick", &run)),
                        Err(e) => {
                            out.push_str(&ev_line_err(step, "pick", &e, &run));
                            break;
                        }
                    }
                }
                EvAction::Fight { win } => {
                    if !*win {
                        out.push_str(&ev_line_err(step, "fight", "只支持 win:true", &run));
                        break;
                    }
                    run.debug_win_battle();
                    let mut guard = 0;
                    while run.holding_victory() && guard < 200 {
                        run.tick_win_hold();
                        guard += 1;
                    }
                    out.push_str(&ev_line(step, "fight", &run));
                }
                EvAction::RewardTake => {
                    let _ = run.reward_take();
                    out.push_str(&ev_line(step, "reward_take", &run));
                }
                EvAction::RewardLeave => {
                    run.leave_reward();
                    out.push_str(&ev_line(step, "reward_leave", &run));
                }
                EvAction::Noop => out.push_str(&ev_line(step, "noop", &run)),
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn rows(seed: u64, text: &str) -> Vec<Json> {
            run(seed, text)
                .expect("沙盒要能跑")
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| parse_json(l).expect("每行都要是 JSON"))
                .collect()
        }

        fn dig<'a>(v: &'a Json, keys: &[&str]) -> &'a Json {
            let mut cur = v;
            for k in keys {
                cur = cur.get(k).unwrap_or_else(|| panic!("少了字段 {k}"));
            }
            cur
        }

        fn num(v: &Json, keys: &[&str]) -> i64 {
            dig(v, keys).as_i64().unwrap_or_else(|| panic!("{keys:?} 不是数字"))
        }

        fn find<'a>(rows: &'a [Json], op: &str) -> &'a Json {
            rows.iter()
                .find(|r| dig(r, &["op"]).as_str() == Some(op))
                .unwrap_or_else(|| panic!("没有 op={op} 的行"))
        }

        fn enemy0_hp(v: &Json) -> i64 {
            dig(v, &["st", "enemies"]).as_arr().unwrap()[0]
                .get("hp")
                .unwrap()
                .as_i64()
                .unwrap()
        }

        /// 跨战斗:遗物计数器与血量带着走(combats 模式下才输出场次号与 counters)
        #[test]
        fn multi_combat_carries_relic_counters() {
            let text = r#"{"player":{"hp":80,"max_hp":80},
                "relics":["incense_burner","pen_nib"],
                "combats":[
                  {"enemies":[{"id":"cultist","hp":50,"max_hp":50}],"hand":["strike"],"draw":["defend"],
                   "actions":[{"op":"play","hand":0,"target":0},{"op":"end_turn"},{"op":"end_turn"}]},
                  {"enemies":[{"id":"cultist","hp":50,"max_hp":50}],"hand":["defend"],"draw":[],
                   "actions":[{"op":"noop"}]}
                ]}"#;
            let rows = rows(7, text);
            let first_end = rows
                .iter()
                .filter(|r| r.get("c").and_then(|c| c.as_i64()) == Some(0))
                .filter(|r| r.get("st").is_some())
                .next_back()
                .expect("第一场要有快照");
            assert_eq!(num(first_end, &["st", "counters", "incense"]), 3, "第一场数到第 3 回合");
            let second = rows
                .iter()
                .find(|r| r.get("c").and_then(|c| c.as_i64()) == Some(1))
                .expect("要有第二场的行");
            assert_eq!(num(second, &["st", "counters", "incense"]), 4, "薰香从上一场的 3 接上");
            assert_eq!(num(second, &["st", "counters", "pen_nib"]), 1, "笔尖计数也跨场");
        }

        /// 极端叠加:力量 10 + 易伤 10 是 (6+10)*1.5 向下取整
        #[test]
        fn extreme_stack_math() {
            let text = r#"{"player":{"hp":40,"max_hp":80,"energy":9,"max_energy":9,"powers":{"strength":10}},
                "hand":["strike","defend"],"draw":[],"discard":[],"exhaust":[],
                "enemies":[{"id":"cultist","hp":999,"max_hp":999,"move":"Incantation","powers":{"vulnerable":10}}],
                "actions":[{"op":"play","hand":0,"target":0}]}"#;
            let rows = rows(1, text);
            let dealt = enemy0_hp(&rows[0]) - enemy0_hp(find(&rows, "play"));
            assert_eq!(dealt, 24, "力量10+易伤10 该打 24");
        }

        /// X 费:3 点能量打旋风斩,打 3 次各 5 点,能量清零
        #[test]
        fn x_cost_uses_all_energy() {
            let text = r#"{"player":{"hp":40,"max_hp":80,"energy":3,"max_energy":9},
                "hand":["whirlwind"],"draw":[],"discard":[],"exhaust":[],
                "enemies":[{"id":"cultist","hp":999,"max_hp":999,"move":"Incantation"}],
                "actions":[{"op":"play","hand":0,"target":0}]}"#;
            let rows = rows(1, text);
            let play = find(&rows, "play");
            assert_eq!(enemy0_hp(&rows[0]) - enemy0_hp(play), 15, "3 能量的旋风斩该打 5×3");
            assert_eq!(num(play, &["st", "energy"]), 0, "X 费吃光能量");
        }

        /// 分裂:大史莱姆掉到半血以下,敌方回合结束时裂成两只中史莱姆
        #[test]
        fn large_slime_splits_on_its_turn() {
            let text = r#"{"encounter":"large_slime","player":{"hp":40,"max_hp":80,"energy":9,"max_energy":9},
                "hand":["bludgeon","strike","defend","defend","defend"],"draw":["strike","strike","strike"],
                "discard":[],"exhaust":[],"enemies":[{"id":"acid_slime_large","hp":65,"max_hp":65}],
                "actions":[{"op":"play","hand":0,"target":0},{"op":"play","hand":0,"target":0},{"op":"end_turn"}]}"#;
            let rows = rows(1, text);
            let enemies = dig(find(&rows, "end_turn"), &["st", "enemies"]).as_arr().unwrap();
            assert_eq!(enemies.len(), 2, "该裂成两只");
            for e in enemies {
                assert_eq!(e.get("id").unwrap().as_str(), Some("acid_slime_medium"));
            }
        }

        /// 手牌上限:手上 10 张时打出抽牌牌,手牌仍是 10 张
        #[test]
        fn hand_limit_caps_draws() {
            let text = r#"{"player":{"hp":40,"max_hp":80,"energy":9,"max_energy":9},
                "hand":["pommel_strike","defend","defend","defend","defend","defend","defend","defend","defend","defend"],
                "draw":["strike","strike"],"discard":[],"exhaust":[],
                "enemies":[{"id":"cultist","hp":999,"max_hp":999,"move":"Incantation"}],
                "actions":[{"op":"play","hand":0,"target":0}]}"#;
            let rows = rows(1, text);
            let play = find(&rows, "play");
            assert_eq!(dig(play, &["st", "hand"]).as_arr().unwrap().len(), 10, "手牌不能超 10 张");
        }

        /// 升级真言是"自选一张消耗",不是随机消耗:选第 2 张就消耗第 2 张
        #[test]
        fn true_grit_up_exhausts_the_chosen_card() {
            let text = r#"{"player":{"hp":40,"max_hp":80,"energy":9,"max_energy":9},
                "hand":["true_grit+","defend","bash"],"draw":[],"discard":[],"exhaust":[],
                "enemies":[{"id":"cultist","hp":999,"max_hp":999,"move":"Incantation"}],
                "actions":[{"op":"play","hand":0,"target":0,"choose":[1]}]}"#;
            let rows = rows(1, text);
            let play = find(&rows, "play");
            let exhaust = dig(play, &["st", "exhaust"]).as_arr().unwrap();
            assert_eq!(exhaust.len(), 1, "该消耗一张");
            assert_eq!(exhaust[0].as_str(), Some("bash"), "消耗的是选中的那张");
            let hand = dig(play, &["st", "hand"]).as_arr().unwrap();
            assert_eq!(hand.len(), 1);
            assert_eq!(hand[0].as_str(), Some("defend"), "没被选中的留在手里");
        }
    }
}
