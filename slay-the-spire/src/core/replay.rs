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
//!     rest rest     营火休息(rest)还是打铁(smith)
//!     shop skip     商店什么都不买
//!     steps 400     最多走多少步
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

use crate::core::card::{CardInstance, CardType};
use crate::core::combat::Phase;
use crate::core::enemy::EnemyKind;
use crate::core::map::{NodeKind, COLS, FLOORS};
use crate::core::relics::RelicTier;
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
    format!(
        "{{\"hp\":{},\"max_hp\":{},\"gold\":{},\"act\":1,\"row\":{},\"deck\":[{}],\"relics\":[{}],\"potions\":[{}]}}",
        run.player.hp,
        run.player.max_hp,
        run.player.gold,
        run.floor_reached,
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
            let cands = c.choice_candidates();
            if let Some((i, _)) = cands.first().copied() {
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
        let pick = (0..c.hand.len()).find(|&i| {
            c.hand[i].kind() == CardType::Attack
                && c.hand[i]
                    .fixed_cost()
                    .is_some_and(|k| k >= 0 && k <= c.energy)
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
pub fn run_jsonl(seed: u64, policy: &Policy) -> Result<String, String> {
    let mut run = Run::new(seed);
    run.open_neow();
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
                auto_play(&mut run)?;
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
                let taken = if policy.reward_take {
                    take_rewards(&mut run, policy)?
                } else {
                    Vec::new()
                };
                let tags: Vec<String> = taken.iter().map(|t| js(t)).collect();
                let payload = format!(
                    "\"source\":{},\"entries\":[{}],\"taken\":[{}]",
                    js(source),
                    entries.join(","),
                    tags.join(",")
                );
                // Boss 的奖励界面就是这一趟的终点:不离开(离开后两边会分叉)
                if is_boss {
                    out.push(line(step, "reward", &payload, &state_json(&run)));
                    step += 1;
                    out.push(line(step, "end", "\"result\":\"boss\"", &state_json(&run)));
                    return Ok(out.join("\n") + "\n");
                }
                run.leave_reward();
                out.push(line(step, "reward", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 营火 ----
            Screen::Rest => {
                let opts: Vec<String> = run
                    .rest_options()
                    .iter()
                    .map(|o| js(&o.name().to_lowercase()))
                    .collect();
                let pick = policy.rest;
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
                run.take_treasure();
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
                let n = run.picker_candidates().len();
                run.picker_confirm()
                    .map_err(|e| format!("选牌确认失败:{e}"))?;
                let payload = format!("\"candidates\":{n},\"pick\":0");
                out.push(line(step, "pick", &payload, &state_json(&run)));
                step += 1;
                continue;
            }

            // ---- 收尾 ----
            Screen::Victory => {
                out.push(line(step, "end", "\"result\":\"victory\"", &state_json(&run)));
                return Ok(out.join("\n") + "\n");
            }
            Screen::Death => {
                out.push(line(step, "end", "\"result\":\"death\"", &state_json(&run)));
                return Ok(out.join("\n") + "\n");
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
        Expected {
            seed: 3,
            lines: 45,
            ref_lines: 45,
            aligned: 30,
            diff_steps: &[30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44],
            diff_digest: 0x12703eae9af9546e,
        },
        Expected {
            seed: 42,
            lines: 43,
            ref_lines: 43,
            aligned: 36,
            diff_steps: &[36, 37, 38, 39, 40, 41, 42],
            diff_digest: 0x74091caeac0de459,
        },
        Expected {
            seed: 54,
            lines: 43,
            ref_lines: 43,
            aligned: 25,
            diff_steps: &[25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42],
            diff_digest: 0xabde3528579e7910,
        },
    ];

    /// 扫荡集合:seed 1..40 的逐字段对拍登记表(与 tools/e2e_diff.ts 的归一化一致,
    /// 比较时两侧都转小写 —— 现在唯一的大小写差异是 init 行的 seed_str).
    const SWEEP: &[Expected] = &[
    Expected { seed: 1, lines: 31, ref_lines: 16, aligned: 11, diff_steps: &[11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30], diff_digest: 0x118af3bb7b6aa795 },
    Expected { seed: 2, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 3, lines: 45, ref_lines: 45, aligned: 30, diff_steps: &[30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44], diff_digest: 0x12703eae9af9546e },
    Expected { seed: 4, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 5, lines: 21, ref_lines: 21, aligned: 21, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 6, lines: 29, ref_lines: 29, aligned: 23, diff_steps: &[23, 24, 25, 26, 27, 28], diff_digest: 0x275af80e7d7076a8 },
    Expected { seed: 7, lines: 23, ref_lines: 23, aligned: 23, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 8, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 9, lines: 37, ref_lines: 34, aligned: 32, diff_steps: &[32, 33, 34, 35, 36], diff_digest: 0x7e495006a3dcd207 },
    Expected { seed: 10, lines: 37, ref_lines: 21, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36], diff_digest: 0x8d76dee0c2a4f8e8 },
    Expected { seed: 11, lines: 21, ref_lines: 21, aligned: 14, diff_steps: &[14, 15, 16, 17, 18, 19, 20], diff_digest: 0x31589fa2901fcc99 },
    Expected { seed: 12, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 13, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 14, lines: 23, ref_lines: 23, aligned: 23, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 15, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0x7b05b6a7f79bf07b },
    Expected { seed: 16, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 17, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 18, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 19, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 20, lines: 34, ref_lines: 34, aligned: 15, diff_steps: &[15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33], diff_digest: 0x76cde42d54ccab0f },
    Expected { seed: 21, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 22, lines: 44, ref_lines: 44, aligned: 19, diff_steps: &[19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0x4cb30134673753c8 },
    Expected { seed: 23, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 24, lines: 17, ref_lines: 17, aligned: 15, diff_steps: &[15, 16], diff_digest: 0xb3e49f4f3735e49c },
    Expected { seed: 25, lines: 33, ref_lines: 33, aligned: 33, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 26, lines: 19, ref_lines: 19, aligned: 19, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 27, lines: 44, ref_lines: 44, aligned: 12, diff_steps: &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43], diff_digest: 0xee37ffdd8a894de },
    Expected { seed: 28, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 29, lines: 17, ref_lines: 17, aligned: 12, diff_steps: &[12, 13, 14, 15, 16], diff_digest: 0xdf8e599cc5fa8477 },
    Expected { seed: 30, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 31, lines: 38, ref_lines: 38, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37], diff_digest: 0x4ee49a0608b9e4d6 },
    Expected { seed: 32, lines: 22, ref_lines: 22, aligned: 22, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 33, lines: 17, ref_lines: 17, aligned: 17, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 34, lines: 28, ref_lines: 28, aligned: 28, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 35, lines: 36, ref_lines: 36, aligned: 27, diff_steps: &[27, 28, 29, 30, 31, 32, 33, 34, 35], diff_digest: 0x39a75410bfd5721a },
    Expected { seed: 36, lines: 18, ref_lines: 18, aligned: 18, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 37, lines: 29, ref_lines: 34, aligned: 26, diff_steps: &[26, 27, 28, 29, 30, 31, 32, 33], diff_digest: 0x9a2ec73e8ca154ea },
    Expected { seed: 38, lines: 33, ref_lines: 33, aligned: 33, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 39, lines: 20, ref_lines: 20, aligned: 20, diff_steps: &[], diff_digest: 0xcbf29ce484222325 },
    Expected { seed: 40, lines: 43, ref_lines: 43, aligned: 18, diff_steps: &[18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42], diff_digest: 0xbd2e0139416d1b53 },
];

    fn fixture_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/golden/e2e")
    }

    fn fixture_text(seed: u64) -> String {
        std::fs::read_to_string(fixture_dir().join(format!("seed{seed}.ref.jsonl")))
            .unwrap_or_else(|e| panic!("seed {seed}: 读不到 fixture: {e}"))
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
            assert!(
                lines.iter().any(|l| l.contains("\"row\":15")),
                "seed {}: fixture 没走到 Boss 那一层",
                case.seed
            );
            assert!(lines.len() >= 30, "seed {}: fixture 太短", case.seed);
        }
    }
}
