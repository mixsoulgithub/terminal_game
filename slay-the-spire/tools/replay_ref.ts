// 端到端对拍:参考实现侧的导出器.喂同一个 seed + 同一份路径脚本,
// 输出与 Rust 侧(spire --replay)同 schema 的 JSONL.
//   bun tools/replay_ref.ts <seed> [script] > ref.jsonl
// 依赖 refs/slay-the-cli(只读).
//
// 走法与 src/core/replay.rs 一一对应:
//   Neow 选第 i 项 → 地图每步走"列号最小"的那个可达节点 → 战斗按 autoWinCombat
//   的规则出牌(第一张打得起的攻击牌打第一只活怪) → 奖励按策略拿/跳 →
//   营火休息 → 商店不买东西 → 宝箱开箱 → 事件选第一个能选的选项.
// 走到 Boss 的奖励界面就停(不离开,离开之后两边会分叉:参考进第二章,本作进胜利).

import { createRun, advance, type GameState, type Command } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/game.ts";
import { buildBaseContentBundle } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/content/index.ts";
import { Rng, seedToString, type RngState } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rng.ts";
import { MAP_HEIGHT, MAP_WIDTH } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/mapGen.ts";
import { buildEventScreen } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/eventRuntime.ts";
import { restOptionAvailable, resolveUnknownRoom } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/runFlow.ts";
import { canSmith } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/rest.ts";
import { createCardReward, cardGroupEntries } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/rewards.ts";
import { readFileSync } from "node:fs";
import { ActionQueue } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/queue.ts";
import { executeAction } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/combat/interpreter.ts";
import type { GameAction } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/actions.ts";
import { RngRegistry } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rngRegistry.ts";
import type { EffectCtx } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/content/defs.ts";
import type { CombatState } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/combat/combatState.ts";

type Policy = {
  neow: number;
  rewardTake: boolean;
  card: number;
  event: number;
  rest: "rest" | "smith";
  shopSkip: boolean;
  maxSteps: number;
  /** 最多走到第几幕:在第 acts 幕的 Boss 奖励界面停下(默认 1) */
  acts: number;
  /** 钥匙模式:营火优先回忆拿红钥匙,宝箱优先拿蓝钥匙 */
  keys: boolean;
  /** 智能打牌(默认关).开了才走 smartPlay,不开还是 autoPlay */
  smart: boolean;
  /** 调试钩子:开局直接切到第几幕(默认 1) */
  act: number;
  /** 调试钩子:切幕后先静默走到本章第几行(默认 0) */
  floor: number;
  /** 调试钩子:三把钥匙直接到手(keys all) */
  keysAll: boolean;
  /** 调试钩子:开局把生命与上限设成这个值(hp n);null = 不改 */
  hp: number | null;
  /** 调试钩子:把牌组整个换掉(deck keep/strong/ramp/burst);"keep" = 不改 */
  deck: "keep" | "strong" | "ramp" | "burst";
  /** 飞升等级(0-20,默认 0).`asc 20` 就是 A20 */
  ascension: number;
};

function defaultPolicy(): Policy {
  return {
    neow: 1,
    rewardTake: true,
    card: 0,
    event: 0,
    rest: "rest",
    shopSkip: true,
    maxSteps: 400,
    acts: 1,
    keys: false,
    smart: false,
    act: 1,
    floor: 0,
    keysAll: false,
    hp: null,
    deck: "keep",
    ascension: 0,
  };
}

function parsePolicy(text: string): Policy {
  const p = defaultPolicy();
  for (const raw of text.split("\n")) {
    const line = raw.split("#")[0]!.trim();
    if (!line) continue;
    const [key, val] = line.split(/\s+/);
    switch (key) {
      case "neow": p.neow = Number(val); break;
      case "card": p.card = Number(val); break;
      case "event": p.event = Number(val); break;
      case "steps": p.maxSteps = Number(val); break;
      case "acts": p.acts = Math.max(1, Number(val)); break;
      case "act": p.act = Math.max(1, Number(val)); break;
      case "floor": p.floor = Number(val); break;
      case "hp": p.hp = Number(val); break;
      case "asc": p.ascension = Math.max(0, Math.min(20, Number(val))); break;
      case "deck":
        if (val !== "keep" && val !== "strong" && val !== "ramp" && val !== "burst")
          throw new Error(`deck 只能是 strong/ramp/burst/keep,给的是 ${val}`);
        p.deck = val;
        break;
      case "keys":
        p.keys = val !== "off";
        if (val === "all") p.keysAll = true;
        break;
      case "smart": p.smart = val === "on"; break;
      case "reward": p.rewardTake = val === "take"; break;
      case "rest": p.rest = val === "smith" ? "smith" : "rest"; break;
      case "shop": p.shopSkip = val !== "buy"; break;
      default: throw new Error(`不认识 ${key}`);
    }
  }
  return p;
}

const bundle = buildBaseContentBundle();

// ---- 参考侧缺口的驱动补偿:燃烧精英的"再生" ----
//
// 参考实现 applyBurningEliteBuff(runFlow.ts:352)照原版把 REGEN 挂到了怪身上,
// 但它的战斗解释器里对"怪身上的 REGEN"没有 atStartOfTurn 钩子 —— 参考自己的
// REGEN 能力(src/content/relics/supportPowers.ts:93)是玩家侧"回合末回血、每回合
// 递减"(Regen Potion 用的),钩子里 owner.kind === "player" 才回血,于是燃烧精英
// 抽到 3 号增益时怪物一点血都不回.
// 原版 Monster::applyStartOfTurnPowers 会按层数回血、且**不递减**
// (反编译 refs/sts_lightspeed/src/combat/Monster.cpp:59-60;挂载见 refs/sts_lightspeed/src/combat/MonsterGroup.cpp:622),本作引擎
// 照原版实现(combat.rs 敌人回合开始按 Regenerate 回血、不减层),不动.
// 注意:不能直接 bundle.powers.set("REGEN", ...) —— 那会把玩家侧的 Regen Potion
// 能力一起换掉(misc: 喝过 Regen Potion 的种子在参考侧反向给怪回血,acts 77->161).
// 做法 = 另立一枚怪专用的 BURNING_REGEN(回合开始按层数回血、不递减),并在每次
// advance 之后把怪身上的 "REGEN"(参考侧燃烧精英挂的那枚)改名过去.
const BURNING_REGEN_POWER = {
  id: "BURNING_REGEN",
  name: "Regen",
  kind: "buff" as const,
  stacking: "intensity" as const,
  turnBased: false,
  hooks: {
    atStartOfTurn: (ctx: {
      owner: { idx: number };
      power?: { amount: number };
      combat?: { monsters: { hp: number; maxHp: number; isDead: boolean; isEscaped: boolean }[] };
    }) => {
      const m = ctx.combat?.monsters[ctx.owner.idx];
      if (!m || m.isDead || m.isEscaped) return;
      m.hp = Math.min(m.maxHp, m.hp + (ctx.power?.amount ?? 0));
    },
  },
};
bundle.powers.set("BURNING_REGEN" as never, BURNING_REGEN_POWER as never);

/** 参考侧燃烧精英把怪身上的再生记成 "REGEN"(与玩家 Regen Potion 同名),
 *  改名到怪专用的 BURNING_REGEN,别让它落进玩家侧那条会递减的实现. */
function renameBurningRegen(s: GameState): void {
  if (!s.combat) return;
  for (const m of s.combat.monsters) {
    for (const p of m.powers) {
      if (p.id === "REGEN") p.id = "BURNING_REGEN";
    }
  }
}

// ---- 参考侧缺口的驱动补偿:蛋类遗物 ----
//
// 参考实现的 MOLTEN_EGG/TOXIC_EGG/FROZEN_EGG(src/content/relics/*.ts)都是
// `hooks: {}` 的空壳,只留了一条 "RUN-LAYER" 注释 —— 往牌组里加攻击/技能/能力牌时
// 该给的强化它没给(原版:card.canUpgrade() 就升一级;语料 relics 的 text 与
// 本作 relics.rs 的 egg 实现一致).任其带着走,fixture 会把"参考缺口"当成整串
// 战斗的 hp/抽牌差异(acts seed 12691 的 128 处就是它).这里在驱动侧按原版规则
// 补上:每一步 advance 之后,把这一步新加进牌组的牌按身上的蛋升一级.
// 本作引擎不动.
const EGG_TYPE: Record<string, string> = {
  MOLTEN_EGG: "attack",
  TOXIC_EGG: "skill",
  FROZEN_EGG: "power",
};

/** 新加进牌组(追加在队尾)的牌按蛋升级;返回被升级的卡 id(调试/断言用) */
export function applyEggUpgrades(state: GameState, deckLenBefore: number): string[] {
  const deck = state.run?.deck;
  if (!deck || deck.length <= deckLenBefore) return [];
  const types: Record<string, true> = {};
  for (const r of state.run.relics) {
    const t = EGG_TYPE[r.defId];
    if (t) types[t] = true;
  }
  const upgraded: string[] = [];
  if (Object.keys(types).length === 0) return upgraded;
  for (let i = deckLenBefore; i < deck.length; i++) {
    const card = deck[i]!;
    if (card.upgrades > 0) continue;
    const def = bundle.cards.get(card.defId);
    if (def && types[def.type]) {
      card.upgrades = 1;
      upgraded.push(card.defId);
    }
  }
  return upgraded;
}

// ---- 参考侧缺口的驱动补偿:击杀盗贼/强盗的退赃 ----
//
// 原版战斗结束时(BattleContext::updateMonstersOnExit)把没逃跑的 LOOTER/MUGGER
// 身上的 miscInfo(偷走的金币)汇总成 info.stolenGold,奖励层第一件事就是
// reward.addGold(info.stolenGold) 还给玩家;本作 src/core/combat.rs 的敌死处理
// 也把 state.stolen 加回 player_gold(见 combat.rs 的 thief_steals_gold_and_flees
// "打死就把赃款吐出来").参考实现的 looters.ts/mugger.ts 只在自己的 data.stolenGold
// 上记账,奖励层从不加回 —— 两边金币从第一场盗贼战起就差了赃款(盗贼每偷一次
// 记一笔,一直挂到战斗结束).这里在驱动侧按原版补上:每次 advance 之后,把
// "已死、还没退过账"的 LOOTER/MUGGER 的 data.stolenGold 加回 run.gold 并清零
// (清零即标记;该怪的 getMove 不读这个字段,清掉不影响它的选招).本作引擎不动.

/** 把这一步里"死掉的盗贼/强盗"记着的赃款还给参考侧的金币.
 *  认"死了哪只"靠参考引擎自己的 monsterDeath 事件(advance 会把这一步的事件记进
 *  state.eventLog):最后一只怪倒下时这场战斗已经被收尾(combat 已清空),死掉的那只
 *  不在新状态里,取不到就回 advance 之前的状态里按槽位找(advance 是 structuredClone,
 *  旧状态原样没动).清零 data.stolenGold 即标记,免得重复退. */
function refundStolenGold(before: GameState, after: GameState): void {
  for (const ev of after.eventLog ?? []) {
    if (ev.event !== "monsterDeath") continue;
    const idx = (ev.payload as { idx: number } | undefined)?.idx;
    if (idx === undefined) continue;
    const m =
      after.combat?.monsters.find((x) => x.idx === idx) ??
      before.combat?.monsters.find((x) => x.idx === idx);
    if (!m) continue;
    const n = (m.data.stolenGold as number) ?? 0;
    if (n <= 0) continue;
    m.data.stolenGold = 0;
    after.run.gold += n;
  }
}

/** 与引擎的 advance 同义,只是给新加的牌补上蛋类遗物的强化,并把已死盗贼的赃款还回来 */
function advanceWithEggs(s: GameState, cmd: Command): GameState {
  const n = s.run.deck.length;
  const out = advance(s, cmd, bundle);
  applyEggUpgrades(out, n);
  if (COMPENSATE) {
    refundStolenGold(s, out);
    renameBurningRegen(out);
    ensureFairyWatch(out);
    fixSummonerSlotLayout(out);
  }
  return out;
}

// ---- 参考侧缺口的驱动补偿:仙女在瓶中的保命 ----
//
// 参考实现把 FAIRY_POTION 写成了 ENGINE-GAP(src/content/potions/index.ts:315-324
// "non-drinkable death-save; playerDeath has no hook yet"),onUse 是空函数,它的
// playerDeath(interpreter.ts:335-339)只把 combatOver 置成 defeat —— 玩家一到 0 血
// 就判负,瓶子还攥在手里.原版是死亡拦截:Player::wouldDie(反编译 refs/sts_lightspeed/src/combat/Player.cpp:320-345)
// 先扫药水栏,有 FAIRY_POTION 就丢掉瓶子、按 max HP 的 30%(神圣树皮 60%,至少 1)
// 回血并**继续战斗**;战斗外同一规则在 GameContext::playerOnDie(GameContext.cpp:
// 2132-2151,花开彼岸除外).本作 combat.rs 的 resolve_player_death 照原版实现(见
// run.rs 的 resolve_player_death 与测试 fairy_potion_saves_the_player_once),不动.
//
// 参考引擎没有"将死"这一钩子,但它给玩家侧的伤害管线留了 onLoseHp 折叠
// (interpreter.ts:211/235 的 foldHook(ctx, PLAYER, "onLoseHp", d));这里就借它
// 补上:注册一枚隐藏的 FAIRY_SAVE 玩家能力,每次开战挂到玩家身上,onLoseHp 里
// 发现"这一下会把血打到 0 或以下"且身上还有 FAIRY_POTION 时,就按原版丢掉瓶子、
// 把 run.hp 直接设成回复值、并把这一次的掉血折成 0(等价于原版"先置 0 再回血"
// 的最终值).既不改参考源码,也不碰它自己的 Regen Potion 之类能力.
// 挂能力而不是事后改 outcome:原版是当场继续打,参考侧若先判负再回滚,行动队列
// 剩余的那几段就丢了(seed 9 会陷进打不完的战斗).
// 已知口径差:这一折把致命那一下的掉血整个折成 0,于是"掉血触发"的钩子(Runic Cube
// 这类)在参考侧不会响,原版会响;现有各表没碰到这种组合,碰到时按参考侧登记.

const FAIRY_SAVE_POWER = {
  id: "FAIRY_SAVE",
  name: "Fairy in a Bottle",
  kind: "buff" as const,
  stacking: "none" as const,
  turnBased: false,
  hidden: true, // 不显示成 buff 图标、不写日志
  hooks: {
    onLoseHp: (ctx: EffectCtx, amount: number): number => {
      const run = ctx.run;
      if (amount <= 0) return amount;
      if (run.relics.some((r) => r.defId === "MARK_OF_THE_BLOOM")) return amount;
      const slot = run.potions.findIndex((p) => p === "FAIRY_POTION");
      if (slot === -1) return amount;
      // 这枚能力挂在玩家能力表末尾,先跑的能力(BUFFER)已经改过 amount 了;
      // 但遗物侧的 onLoseHp 折叠(钨钢棒 -1)排在能力之后,这里先自己按它算一遍,
      // 免得"钨钢棒刚好救下"的场合也把仙女瓶吃掉
      const afterRod = run.relics.some((r) => r.defId === "TUNGSTEN_ROD")
        ? amount > 0
          ? amount - 1
          : amount
        : amount;
      if (run.hp - afterRod > 0) return amount; // 这一下打不死,不触发
      run.potions[slot] = null; // 丢掉瓶子,同时当作"用过一次"的标记
      const bark = run.relics.some((r) => r.defId === "SACRED_BARK");
      run.hp = Math.max(1, Math.floor(run.maxHp * (bark ? 0.6 : 0.3)));
      return 0;
    },
  },
};
bundle.powers.set("FAIRY_SAVE" as never, FAIRY_SAVE_POWER as never);

/** 开战时给玩家挂上仙女保命符(不在战斗里或已经挂了就不动) */
function ensureFairyWatch(s: GameState): void {
  const c = s.combat;
  if (!c) return;
  if (c.player.powers.some((p) => p.id === ("FAIRY_SAVE" as never))) return;
  c.player.powers.push({ id: "FAIRY_SAVE" as never, amount: 0, justApplied: false, data: null });
}

// ---- 参考侧时序补丁:怪物"下一招掷点"早于它本招排队的回血结算 ----
//
// 原版怪招是"先把本招效果当场做完,再掷下一招".反编译的 MYSTIC_HEAL
// (refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:600-607)就是
// `heal(knight); heal(self); rollMove();` —— heal 立即改 curHp
// (refs/sts_lightspeed/src/combat/Monster.cpp:269-272),紧接着 rollMove 里 getMove 读到的是
// **补过血之后**的血量(神秘者据此判"自己还差不差血、要不要继续 HEAL").
// CENTURION_DEFEND 同理(mystic.addBlock 当场生效后才 rollMove).
//
// 参考实现把 heal 塞进动作队列(interpreter.ts:68 的 "heal" action 走 applyHeal),
// 而它的 executeMonsterMove(interpreter.ts:748-768)在 move.execute 排完队之后**立刻**
// rollMove,于是 getMove 读到的是补血**之前**的血:神秘者在 14/54 时判"要治",原版补到
// 34/54 后才判"不用治".两边都没漏实现,是参考排队的时点比原版晚了一拍.
//
// 这里在驱动侧把这一拍掰回来:包住每个怪招的 execute,等它把本招效果(含 heal)排进动作
// 队列后,当场把**本招刚排进去的、目标是怪物的** heal 按原顺序先结算掉 —— 走参考自己的
// executeAction → applyHeal,回血钩子照旧,一粒不少;随后参考自己的 rollMove 就与原版
// 一样读到补过血的血量.只把"结算"的时点提前,掷点**次数**不变,其它 action 一律不动;
// 玩家的回血(牌/遗物)不是怪招排的,不在名单里.本作引擎不动.
const patchedMoveExec = new WeakSet<object>();

function patchMonsterRollAfterQueuedHeal(): void {
  for (const def of bundle.monsters.values()) {
    for (const move of Object.values(def.moves)) {
      if (patchedMoveExec.has(move)) continue;
      patchedMoveExec.add(move);
      const exec = move.execute;
      if (!exec) continue;
      move.execute = (ctx, self) => {
        // ActionQueue.items 是 private;驱动侧要按对象身份摘掉刚排队的那几项,这里显式松一把类型(它就在 queue.ts:13 声明)
        const queue = ctx.queue as unknown as { items: GameAction[] };
        const items = queue.items;
        // 本招刚开始时队列里已有的是"别人的";只认本招新排进去的那几项
        const before = new Set<GameAction>(items);
        exec(ctx, self);
        if (!COMPENSATE) return;
        const late = items.filter(
          (a) => !before.has(a) && a.kind === "heal" && a.target.kind === "monster",
        );
        if (late.length === 0) return;
        // 先按原地顺序从队列里摘掉(本招排的都是 addToBottom,原本也排在这些项之后),
        // 再逐项走参考自己的结算,掷点读到的血量就与原版一致.
        const lateSet = new Set<GameAction>(late);
        for (let i = items.length - 1; i >= 0; i--) {
          if (lateSet.has(items[i]!)) items.splice(i, 1);
        }
        for (const a of late) executeAction(ctx, a);
      };
    }
  }
}

patchMonsterRollAfterQueuedHeal();

// ---- 参考侧缺口的驱动补偿:六火幽魂的 Inferno ----
//
// 原版 Inferno 除了 2x6(飞升 3x6)攻击,还要"往弃牌堆塞 3 张 Burn+,并把已有的
// 灼伤全部升级"(语料 refs/slay-the-cli/data/corpus/monsters-act1.json 的
// HEXAGHOST.conflicts 条:"wiki:Hexaghost (Enemies.lua Inferno text): 'Adds 3 Burns+
// into your discard pile. Upgrades all Burns.'";wiki 与反编译同源的 Java
// BurnIncreaseAction + burnUpgraded).参考实现有**意**省掉了这半
//(refs/slay-the-cli/src/content/monsters/act1/hexaghost.ts 的 HEXAGHOST_INFERNO
// 只 attackPlayer,文件头注明"omitted here as in the transcription"),本作引擎照
// 原版实现(enemies/act1.rs 的 Inferno:PlayerCardUpgraded{burn,3,from_turn:None} +
// UpgradePlayerBurns).这里在驱动侧把参考缺的那半补上:包住 HEXAGHOST_INFERNO 的
// execute,攻击照旧(原 exec),再把玩家各堆里已有的灼伤就地升级(升过的跳过)、
// 往弃牌堆塞 3 张 Burn+(与 Sear 用同一个 makeTempCard 动作).不掷点;本作引擎不动.
const patchedInferno = new WeakSet<object>();

function patchHexaghostInferno(): void {
  const def = bundle.monsters.get("HEXAGHOST" as never);
  const move = def?.moves.HEXAGHOST_INFERNO;
  if (!move || patchedInferno.has(move)) return;
  patchedInferno.add(move);
  const exec = move.execute;
  if (!exec) return;
  move.execute = (ctx, self) => {
    exec(ctx, self);
    if (!COMPENSATE || !ctx.combat) return;
    const c = ctx.combat;
    // 已有的灼伤全部升级(参考侧把牌实例放在 combat.cards,piles 里存 iid)
    for (const pile of Object.values(c.player.piles)) {
      for (const iid of pile) {
        const card = c.cards[iid];
        if (card && card.defId === "BURN" && card.upgrades < 1) card.upgrades = 1;
      }
    }
    // 再塞 3 张 Burn+ 进弃牌堆
    ctx.queue.addToBottom({
      kind: "makeTempCard",
      defId: "BURN",
      upgrades: 1,
      dest: "discard",
      n: 3,
    } as never);
  };
}

patchHexaghostInferno();

// ---- 参考侧摆位缺口的驱动补偿:召唤型首领的开战槽位 ----
//
// 反编译按固定槽位摆怪(MonsterGroup::createMonsters):
//   GREMLIN_LEADER 在槽 3、起始随从在 arr[1]/arr[2]、槽 0 空
//   (refs/sts_lightspeed/src/combat/MonsterGroup.cpp:255 把 GREMLIN_LEADER construct 在槽 3);
//   BRONZE_AUTOMATON 在槽 1、槽 0 空,召唤的铜球进槽 0/2
//   (反编译 refs/sts_lightspeed/src/combat/MonsterSpecific.cpp:3393-3399 的 spawnBronzeOrbs 用 arr[0]/arr[2],
//    铜球找空槽见 Actions.cpp 的 SpawnOrbs/SummonGremlins 按从低到高找空槽).
// 本作 encounter 表的摆位与之一致(gremlin_leader_gang = [1,2,3];bronze_automaton = [1]).
// 参考实现把遭遇表的怪按数组下标直接铺成槽 0..N-1
// (refs/slay-the-cli/src/content/acts.ts:78/84 的 monsters:[...]),于是
//   小鬼头目落在槽 2 —— Rally 找空槽(反编译 Actions.cpp:459 的 1,2,0 顺序)候选少了槽 2
//   (a20a2 seed 13 只差 5 hp、seed 33 槽 1 的小鬼已死时参考只找到 1 个空槽、少召唤一只胖小鬼,差 117 hp);
//   铜制自动机落在槽 0 —— 铜球进槽 1/2;更要紧的是"每回合谁先掷下一招 aiRng"按数组顺序走,
//   摆位一挪,首领与两颗铜球各自拿到的掷点就换了人,铜球的随机选招(Beam/Support Beam)因此分叉
//   (a20a2 seed 13/33 各差 8 hp;seed 3 那 10 hp 不是摆位,见下条).
// 这里在驱动侧把参考的开战槽位掰回原版:开战后插一个槽 0 的 GAP(escaped,永不行动、不计数、
// 不可选中,与参考自己 padMonsterSlots 造的同一种对象),再把各只 idx 顺移一位.
// 不掷任何点(HP 掷点仍按原次序落在真实怪身上);导出时 GAP 照旧滤掉.本作引擎不动.
// 每个遭遇只认它"开战那一刻"的阵容,插一次就不再来(之后召唤/替换更不会再插):
// 小鬼头目开战固定三只、首领排最后;铜制自动机开战只有它自己一只.
//
// 附:铜制自动机 seed 3 的整场 10 hp 差曾一并记在这条摆位缺口下,实际另有其因 —— 本作漏了
//   飞升 19 的"光束后接增幅"分支(本作 pick_bronze_automaton 原先一律回到 Stunned;反编译
//   MonsterSpecific.cpp:492-499 是 `asc19 ? BOOST : STUNNED`).已按反编译修好本作引擎,
//   摆位缺口与它两回事,两条都得有(单一改任一条都对不上).
const LEADING_GAP_ENCOUNTERS: Record<string, (ids: string[]) => boolean> = {
  GREMLIN_LEADER: (ids) => ids.length === 3 && ids[2] === "GREMLIN_LEADER",
  AUTOMATON: (ids) => ids.length === 1 && ids[0] === "BRONZE_AUTOMATON",
};

function fixSummonerSlotLayout(s: GameState): void {
  const c = s.combat;
  if (!c) return;
  const enc = c.combatFlags.encounterId;
  const isInitial = LEADING_GAP_ENCOUNTERS[enc];
  if (!isInitial) return;
  const ms = c.monsters;
  if (!isInitial(ms.map((m) => m.id as string))) return;
  ms.unshift({
    id: "GAP" as never,
    idx: 0,
    hp: 0,
    maxHp: 0,
    block: 0,
    powers: [],
    move: null,
    moveHistory: [],
    isDead: false,
    isEscaped: true,
    halfDead: false,
    data: {},
  });
  ms.forEach((m, i) => {
    m.idx = i;
  });
}

// ---- 参考侧判定写反的驱动补偿:Preserved Insect ----
//
// 原版 Preserved Insect 只在"精英房"触发(AbstractDungeon.getCurrRoom().eliteTrigger;
// 反编译 refs/sts_lightspeed/src/combat/BattleContext.cpp 的 eliteTrigger 分支),本作照这个
// (relics.rs 的 elite_hp_reduction_pct + 战斗房间种类).参考实现按"战斗里有 elite 类怪"
// 触发(refs/slay-the-cli/src/content/relics/common.ts:321-330 的
// `monsters.some(category === "elite")`),于是**非精英房里的精英**(如第二幕怪物节点上的
// 燃烧精英,roomKind 仍是 "monster")也被削 25% 血(a20a2 seed 18 的哨卫+球形守卫那场
// 43/20 -> 32/15).这里在驱动侧把判据换成参考自己记的房间种类 roomKind === "elite".
const preservedInsect = bundle.relics.get("PRESERVED_INSECT" as never);
if (preservedInsect) {
  preservedInsect.hooks.atBattleStartPreDraw = (ctx) => {
    const room = ctx.run.room as { roomKind?: string } | undefined;
    if (room?.roomKind !== "elite") return;
    for (const m of ctx.combat!.monsters) m.hp = Math.floor(m.hp * 0.75);
  };
}

/** 调试钩子 `act n`:从当前幕切到第 n 幕开头.
 *  借参考实现自己的幕切换 —— 先把房间换成"Boss 奖励屏",再 skipRewards,
 *  runFlow 的 leaveRewards 就会调用它自己的 actTransition(与 Rust 侧 begin_act
 *  是同一套掷点),这样两边的流位置一致. */
function debugJumpAct(s: GameState): GameState {
  const st = structuredClone(s);
  st.run.room = { kind: "rewards", entries: [], source: "boss" };
  return advance(st, { cmd: "skipRewards" }, bundle);
}

/** 调试钩子 `floor n`:丢掉 init 之后直到第一次走到第 n 行为止的输出,
 *  再从头编号 step(init 仍是 0);最后一行 end 永远保留. */
function trimToRow(lines: string[], row: number): string[] {
  const last = lines.length - 1;
  const kept = lines.filter((l, i) => {
    if (i === 0 || i === last) return true;
    const m = /"row":(\d+)/.exec(l);
    return m ? Number(m[1]) >= row : false;
  });
  return kept.map((l, step) => {
    if (!l.startsWith('{"step":')) return l;
    return `{"step":${step}${l.slice(l.indexOf(","))}`;
  });
}

/** 收尾:按 floor 裁剪后归一化名字,再拼成最终 JSONL. */
function finish(out: string[], policy: Policy): string {
  const trimmed = policy.floor > 0 ? trimToRow(out, policy.floor) : out;
  return normalizeNames(trimmed.join("\n") + "\n");
}


// 只给 buildEventScreen 读用,不行使任何效果
function scratchCtx(state: GameState): EffectCtx {
  return {
    run: state.run,
    combat: state.combat,
    queue: new ActionQueue(),
    bundle,
    rt: { pending: null, currentItem: null, combatOver: null },
    rng: (s) => RngRegistry.fromState(state.rng).get(s),
    asc: state.run.ascension,
    emit: () => {},
    requestChoice: () => {},
  };
}

// 牌堆记号:id、id+、id+2(与 Rust 侧 card_token 一致)
function cardToken(c: { defId: string; upgrades: number }): string {
  if (c.upgrades > 1) return `${c.defId}+${c.upgrades}`;
  if (c.upgrades === 1) return `${c.defId}+`;
  return c.defId;
}

function stateJson(state: GameState): string {
  const r = state.run;
  const row = r.position === null ? 0 : r.position[1];
  return (
    `{"hp":${r.hp},"max_hp":${r.maxHp},"gold":${r.gold},"act":${r.act},"row":${row},` +
    `"deck":[${r.deck.map(cardToken).map((t) => JSON.stringify(t)).join(",")}],` +
    `"relics":[${r.relics.map((x) => JSON.stringify(x.defId)).join(",")}],` +
    `"potions":[${r.potions.map((p) => (p === null ? "null" : JSON.stringify(p))).join(",")}]}`
  );
}

// 地图每格:{k,b,e};edges 按列号升序(两边一致)
function mapJson(state: GameState): string {
  const map = state.run.map!;
  // 第一到三幕的地图只有 0..14 行,Boss 是第 15 行上补出来的那一格;
  // 第四幕的地图本来就把 Boss 摆在 rows 里(第 3 行),这时不再补行 15
  // (本作 src/core/replay.rs 的 map_json 就是照 rows 原样印的).
  const hasBossRow = map.rows.some((r) => r?.some((n) => n && n.kind === "boss"));
  const rows: string[] = [];
  for (let y = 0; y <= MAP_HEIGHT; y++) {
    const cells: string[] = [];
    for (let x = 0; x < MAP_WIDTH; x++) {
      if (y === MAP_HEIGHT) {
        cells.push(!hasBossRow && x === 3 ? `{"k":"boss","b":0,"e":[]}` : "null");
        continue;
      }
      const n = map.rows[y]![x];
      if (!n) {
        cells.push("null");
        continue;
      }
      const kind = n.kind === "unknown" ? "event" : n.kind;
      const edges = [...n.edges].sort((a, b) => a - b);
      cells.push(`{"k":${JSON.stringify(kind)},"b":${n.burningElite ? 1 : 0},"e":[${edges.join(",")}]}`);
    }
    rows.push(`[${cells.join(",")}]`);
  }
  return `[${rows.join(",")}]`;
}

function line(step: number, kind: string, payload: string, state: string): string {
  const mid = payload ? `,${payload}` : "";
  return `{"step":${step},"kind":${JSON.stringify(kind)}${mid},"s":${state}}`;
}

// ---- 战斗:与 autoWinCombat 同规则 ----

/** 当前战斗的遭遇 id(null = 不在战斗里).A20 双 Boss 是"同一个 smartPlay 里
 *  连着两场战斗",驱动层要按"一场战斗一条 fight 行"出料,所以用它认边界:
 *  一旦 run.room 的 encounterId 换了,就停下这次调用,交给驱动层再出一行. */
function combatEncounters(state: GameState): string | null {
  const room = state.run.room;
  return room && room.kind === "combat" ? room.encounterId : null;
}

function autoPlay(state: GameState): GameState {
  let s = state;
  let guard = 0;
  const enc0 = combatEncounters(state);
  while (s.combat && !s.outcome) {
    if (guard++ > 5000) throw new Error("combat did not end");
    if (combatEncounters(s) !== enc0) break; // 又开了一场(双 Boss):这一行到此为止
    // 战斗内挂起的选牌:跟本作一样选前 min 张(位置下标),不给它单独一步
    if (s.pending) {
      const req = s.pending.request;
      const picks =
        req.kind === "cards"
          ? req.iids.map((_, i) => i).slice(0, req.min)
          : [0];
      s = advanceWithEggs(s, { cmd: "choose", indices: picks });
      continue;
    }
    const target = s.combat.monsters.findIndex((m) => !m.isDead && !m.isEscaped);
    const names = s.combat.player.piles.hand.map((iid) => s.combat!.cards[iid]!.defId);
    const energy = s.combat.player.energy;
    const atkIdx = names.findIndex((n) => {
      const def = bundle.cards.get(n)!;
      return def.type === "attack" && def.cost >= 0 && def.cost <= energy;
    });
    if (atkIdx !== -1 && target !== -1) {
      try {
        s = advanceWithEggs(s, { cmd: "playCard", handIdx: atkIdx, target });
      } catch {
        s = advanceWithEggs(s, { cmd: "endTurn" });
      }
    } else {
      s = advanceWithEggs(s, { cmd: "endTurn" });
    }
  }
  return s;
}

// ---- 智能打牌(smart on 才走;与 src/core/replay.rs 的 smart_play 同规则) ----
// SPIRE_TRACE=1 时把每次出牌(手牌/敌人血/意图/选择)打到 stderr,与 Rust 侧同一套,
// 便于把两边的分叉定位到具体回合.

/** 血最少的活敌人(平手取下标小的);攻击与指向敌人的药水都用它当目标 */
function lowestHpEnemy(c: CombatState): number {
  let best = -1;
  for (let i = 0; i < c.monsters.length; i++) {
    const m = c.monsters[i]!;
    if (m.isDead || m.isEscaped || m.halfDead) continue;
    if (best === -1 || m.hp < c.monsters[best]!.hp) best = i;
  }
  return best;
}

/** 按优先级挑一张打得起的牌(与 Rust 的 pick_smart_card 同序):
 *  1. 有敌人快死了(血量 <= 1,或掉到四分之一以下)且自己不会被打死时先补刀,攻击优先;
 *  2. 即将被斩杀或意图总伤 > 格挡时,先用技能补防;
 *  3. 能力牌尽早铺开;
 *  4. 其余打攻击牌,费用从低到高;
 *  5. 再不济打技能. */
function pickSmartCard(c: CombatState, aboutToDie: boolean, threatened: boolean, canKill: boolean): number {
  const hand = c.player.piles.hand.map((iid) => c.cards[iid]!);
  const defAt = (i: number) => bundle.cards.get(hand[i]!.defId)!;
  // 判据是牌面基础费用(与 Rust 一致)
  const affordable = (i: number): boolean => {
    const d = defAt(i);
    return d.cost >= 0 && d.cost <= c.player.energy;
  };
  const group = (t: string): number => {
    let best = -1;
    for (let i = 0; i < hand.length; i++) {
      const d = defAt(i);
      if (d.type !== t || !affordable(i)) continue;
      if (best === -1 || d.cost < defAt(best).cost) best = i;
    }
    return best;
  };
  if (canKill && !aboutToDie) {
    const a = group("attack");
    if (a !== -1) return a;
  }
  if (aboutToDie || threatened) {
    const sk = group("skill");
    if (sk !== -1) return sk;
  }
  const p = group("power");
  if (p !== -1) return p;
  const a = group("attack");
  if (a !== -1) return a;
  const sk = group("skill");
  if (sk !== -1) return sk;
  return -1;
}

/** 危险时按格子顺序找第一瓶能喝的药水;喝到就返回新状态,否则 null */
// 原版里"仙女在瓶中"是被动药水:不能主动喝(onUse 空),只在将死时自动触发
//(FAIRY_SAVE 补偿在下面).参考实现把它写成 `onUse: () => {}` 的空壳,试喝时
// usePotion 会"成功"并吃掉这一格(refs/slay-the-cli/src/content/potions/index.ts:315-324),
// 保命符就这么丢了、后面的药水也跟着整体错位(acts seed 12691 的 59 处).
// 本作引擎 quaff_potion 对 Passive 药水直接回 Err(run.rs 的 quaff_potion),
// 这里让参考侧对齐:试喝时跳过这瓶.
const NON_DRINKABLE_POTIONS: Record<string, true> = { FAIRY_POTION: true };

function tryDrinkOnce(s: GameState): GameState | null {
  const run = s.run;
  for (let slot = 0; slot < run.potions.length; slot++) {
    const id = run.potions[slot];
    if (!id) continue;
    if (NON_DRINKABLE_POTIONS[id]) continue;
    const def = bundle.potions.get(id);
    if (!def) continue;
    let target: number | undefined = undefined;
    if (def.targeted) {
      const t = s.combat ? lowestHpEnemy(s.combat) : -1;
      if (t === -1) continue;
      target = t;
    }
    try {
      return advanceWithEggs(s, { cmd: "usePotion", slot, target });
    } catch {
      continue;
    }
  }
  return null;
}

// 参考实现的 computeIntent(intents.ts)是"拿一个必抛错的 rng 干跑一遍
// move.execute",而且它在内部把 rng 硬换成那个必抛错的版本,外面传什么都拦不住.
// 凡是 execute 里会先掷点/问选择的招(盗贼/强盗的 MUG:先掷台词点、再偷钱、才
// 出手)就会半路抛错,damage=null.本作(Rust)是按静态招式数据算意图的,于是
// 参考侧把"来袭伤害"当 0、不防御,本作算出真伤害、会防御 —— 从第一场盗贼战起
// 走法全线分叉.
//
// 我们自己按招式数据算一遍:只对"会打人"的招(和 UI 的攻击类意图同四种),
// 把它的 execute 干跑在一个"温和 rng"(掷点一律返回 0、选择直接返回)上,
// 读队伍里排队的"对玩家攻击伤害"求和,就是这只怪下一击的来袭总伤.既跟本作
// 看到同一份威胁值,又不改参考源码、也不降低本作保真度.
const ATTACK_INTENTS: Record<string, true> = {
  attack: true,
  attackDefend: true,
  attackDebuff: true,
  attackBuff: true,
};

/** 一只怪下一击对玩家的来袭总伤(不打人就 0) */
function previewIncoming(state: GameState, idx: number): number {
  const c = state.combat!;
  const m = c.monsters[idx]!;
  const move = bundle.monsters.get(m.id)?.moves[m.move];
  if (!move || !ATTACK_INTENTS[move.intent]) return 0;
  const combatClone = structuredClone(c);
  const runClone = structuredClone(state.run);
  const queue = new ActionQueue();
  const rngStream = new Proxy({}, { get: () => () => 0 }) as never;
  const dry: EffectCtx = {
    run: runClone,
    combat: combatClone,
    queue,
    bundle,
    rt: { pending: null, currentItem: null, combatOver: null },
    rng: () => rngStream,
    asc: state.run.ascension,
    emit: () => {},
    requestChoice: () => {},
  };
  try {
    move.execute(dry, combatClone.monsters[idx]!);
  } catch {
    // 干跑半路抛错(要选择/要掷点):读到多少算多少
  }
  let total = 0;
  for (let a = queue.pop(); a !== undefined; a = queue.pop()) {
    if (a.kind === "damage" && a.target.kind === "player" && a.info.type === "attack") {
      total += a.info.amount;
    }
  }
  return total;
}

/** 智能打牌:与 src/core/replay.rs 的 smart_play 同规则 */
function smartPlay(state: GameState): GameState {
  let s = state;
  let guard = 0;
  const enc0 = combatEncounters(state);
  // 已经试过药水的回合号(0 = 还没试过);每回合最多试一次
  let potionTurn = 0;
  while (s.combat && !s.outcome) {
    if (guard++ > 20000) throw new Error("combat did not end");
    if (combatEncounters(s) !== enc0) break; // 又开了一场(双 Boss):这一行到此为止
    // 战斗内挂起的选牌:跟 autoPlay 一样选前 min 张
    if (s.pending) {
      const req = s.pending.request;
      const picks = req.kind === "cards" ? req.iids.map((_, i) => i).slice(0, req.min) : [0];
      s = advanceWithEggs(s, { cmd: "choose", indices: picks });
      continue;
    }
    const c = s.combat;
    if (!c.playerTurn) break;
    const alive: number[] = [];
    for (let i = 0; i < c.monsters.length; i++) {
      const m = c.monsters[i]!;
      if (!m.isDead && !m.isEscaped && !m.halfDead) alive.push(i);
    }
    if (alive.length === 0) {
      s = advanceWithEggs(s, { cmd: "endTurn" });
      continue;
    }
    // 敌方来袭总伤(按招式数据自算,绕开参考侧会抛错的干跑)
    let incoming = 0;
    for (const i of alive) incoming += previewIncoming(s, i);
    const hp = s.run.hp;
    const maxHp = s.run.maxHp;
    const block = c.player.block;
    const aboutToDie = incoming >= hp + block;
    // 来袭总伤已经够把血打空(还没算格挡)也算危险:该喝药水了
    const dangerous = incoming >= hp;
    const threatened = incoming > block;
    const lowHp = hp * 2 <= maxHp;
    // 有敌人快死了:血量 <= 1 这刀必死;掉到四分之一以下也先补刀
    const canKill = alive.some((i) => {
      const m = c.monsters[i]!;
      return m.hp <= 1 || m.hp * 4 <= m.maxHp;
    });
    const turn = c.turn;
    const target = lowestHpEnemy(c);
    const pick = pickSmartCard(c, aboutToDie, threatened, canKill);
    if (process.env.SPIRE_TRACE) {
      const hand = c.player.piles.hand.map((iid) => {
        const x = c.cards[iid]!;
        return `${x.defId}(b${x.misc ?? 0}c${x.costForTurn ?? x.cost})`;
      });
      const foes = c.monsters.map((m, i) => `${m.id}:${m.hp}/${m.maxHp}b${m.block}m${m.move}${m.halfDead ? "HALF" : ""}`);
      console.error(
        `TRACE t${turn} hp${hp} blk${block} in${incoming} atd${aboutToDie} danger${dangerous} cankill${canKill} pick${pick} tgt${target} hand[${hand.join(",")}] foes[${foes.join(",")}]`,
      );
    }
    if ((aboutToDie || dangerous || lowHp) && potionTurn !== turn) {
      const next = tryDrinkOnce(s);
      if (next) {
        potionTurn = next.combat ? next.combat.turn : turn;
        s = next;
        continue;
      }
      potionTurn = turn;
    }
    if (pick !== -1 && target !== -1) {
      // 只有指定敌人的牌才带目标:参考侧 SURROUNDED.onUseCard 会把传进来的 target
      // 记成玩家朝向,给能力牌(火上浇油这种)乱传会把它的朝向掰过去,于是背袭 ×1.5
      // 落错人.本作按牌面 target 判断,不指定敌人的牌一律不看 target(见 combat.rs 的
      // play_card:只有 Target::Enemy 才认 chosen),两边要同规则.
      const pdef = bundle.cards.get(s.combat!.cards[s.combat!.player.piles.hand[pick]!]!.defId);
      const cardTarget =
        pdef && (pdef.target === "enemy" || pdef.target === "selfandenemy") ? target : undefined;
      try {
        s = advanceWithEggs(s, { cmd: "playCard", handIdx: pick, target: cardTarget });
      } catch {
        s = advanceWithEggs(s, { cmd: "endTurn" });
      }
    } else {
      s = advanceWithEggs(s, { cmd: "endTurn" });
    }
  }
  return s;
}

// ---- 奖励条目(与 Rust 侧同顺序:金币 → 遗物 → Boss 三选一 → 药水 → 卡牌 → 绿钥匙) ----

type Entry = { kind: string; id?: string; amount?: number; upgraded?: boolean; group?: number; taken: boolean };

function rewardEntries(state: GameState): string[] {
  const room = state.run.room;
  if (!room || room.kind !== "rewards") return [];
  const es = room.entries as Entry[];
  const out: string[] = [];
  const gold = es.find((e) => e.kind === "gold");
  if (gold) out.push(`{"k":"gold","n":${gold.amount}}`);
  const relic = es.find((e) => e.kind === "relic");
  if (relic) out.push(`{"k":"relic","id":${JSON.stringify(relic.id)}}`);
  for (const e of es.filter((e) => e.kind === "bossRelic")) out.push(`{"k":"boss_relic","id":${JSON.stringify(e.id)}}`);
  for (const e of es.filter((e) => e.kind === "potion")) out.push(`{"k":"potion","id":${JSON.stringify(e.id)}}`);
  for (const e of es.filter((e) => e.kind === "card")) {
    out.push(`{"k":"card","id":${JSON.stringify(e.id)},"up":${e.upgraded === true}}`);
  }
  if (es.some((e) => e.kind === "emeraldKey")) out.push(`{"k":"emerald_key"}`);
  return out;
}

/** 按策略拿奖励:金币 → 遗物 → Boss 三选一 → 药水 → 第 N 张卡 → 绿钥匙 */
function takeRewards(state: GameState, policy: Policy): { state: GameState; taken: string[] } {
  let s = state;
  const taken: string[] = [];
  for (let guard = 0; guard < 16; guard++) {
    if (s.pending) break; // 交给主循环去选
    const room = s.run.room;
    if (!room || room.kind !== "rewards") break;
    const es = room.entries as Entry[];
    const idxOf = (pred: (e: Entry) => boolean): number => es.findIndex((e) => pred(e) && !e.taken);
    let i = idxOf((e) => e.kind === "gold");
    let tag = "gold";
    if (i === -1) { i = idxOf((e) => e.kind === "relic"); tag = "relic"; }
    if (i === -1) { i = idxOf((e) => e.kind === "bossRelic"); tag = "boss_relic"; }
    if (i === -1) { i = idxOf((e) => e.kind === "potion"); tag = "potion"; }
    if (i === -1) {
      const cards = es.map((e, k) => [e, k] as const).filter(([e]) => e.kind === "card" && !e.taken);
      if (cards.length > policy.card) {
        i = cards[policy.card]![1];
        tag = `card${policy.card}`;
      }
    }
    if (i === -1) { i = idxOf((e) => e.kind === "emeraldKey"); tag = "emerald_key"; }
    if (i === -1) break;
    try {
      s = advanceWithEggs(s, { cmd: "takeReward", i });
      taken.push(tag);
    } catch {
      // 药水格满了之类:标记掉这一项,免得卡死
      if (s.run.room?.kind === "rewards") {
        const e = (s.run.room.entries as Entry[])[i];
        if (e) e.taken = true;
      }
    }
  }
  taken.sort((a, b) => rewardRank(a) - rewardRank(b));
  return { state: s, taken };
}

/** 奖励条目的固定顺序:金币 → 遗物 → Boss 三选一 → 药水 → 卡牌 → 绿钥匙 */
function rewardRank(t: string): number {
  return t === "gold" ? 0 : t === "relic" ? 1 : t === "boss_relic" ? 2 : t === "potion" ? 3 : t.startsWith("card") ? 4 : 5;
}

function shopItems(state: GameState): string[] {
  const room = state.run.room;
  if (!room || room.kind !== "shop") return [];
  const out: string[] = [];
  for (const c of room.shop.cards) out.push(`{"k":"card","id":${JSON.stringify(c.id)},"price":${c.price},"sold":${c.sold}}`);
  for (const r of room.shop.relics) out.push(`{"k":"relic","id":${JSON.stringify(r.id)},"price":${r.price},"sold":${r.sold}}`);
  for (const p of room.shop.potions) out.push(`{"k":"potion","id":${JSON.stringify(p.id)},"price":${p.price},"sold":${p.sold}}`);
  out.push(`{"k":"remove","price":${room.shop.removalCost},"sold":${room.shop.removalUsed}}`);
  return out;
}

const REST_KINDS = ["rest", "smith", "recall", "lift", "toke", "dig"] as const;

function restOptions(state: GameState): string[] {
  return REST_KINDS.filter((k) => restOptionAvailable(scratchCtx(state), k));
}

/** 事件屏当前"能选"的选项下标(参考实现的选项表是整个事件平铺的,可用性由 enabled 谓词给;
 *  这里只保留当前这一屏真正能选的那些,折成与本作同粒度) */
function eventEnabledOptions(state: GameState): number[] {
  const room = state.run.room;
  if (!room || room.kind !== "event") return [0];
  const scr = buildEventScreen(scratchCtx(state));
  if (!scr) return [0];
  const out: number[] = [];
  for (let i = 0; i < scr.options.length; i++) {
    let ok = false;
    try {
      ok = scr.options[i]!.enabled(scratchCtx(state));
    } catch {
      ok = false;
    }
    if (ok) out.push(i);
  }
  return out;
}

/** 事件选项:从 preferred 往后找第一个能选的;都没有就退到第一项,再不行就最后一项 */
function firstEventOption(state: GameState, preferred: number): number {
  const room = state.run.room;
  const scr = room && room.kind === "event" ? buildEventScreen(scratchCtx(state)) : null;
  const enabled = eventEnabledOptions(state);
  for (const i of enabled) if (i >= preferred) return i;
  if (enabled.length > 0) return enabled[0]!;
  return Math.max(0, (scr?.options.length ?? 1) - 1);
}

/** 地图上列号最小的可达节点 */
function nextNode(state: GameState): { x: number; y: number } {
  const map = state.run.map!;
  if (state.run.position === null) {
    const row = map.rows[0]!;
    for (let x = 0; x < MAP_WIDTH; x++) if (row[x]) return { x, y: 0 };
    throw new Error("row 0 is empty");
  }
  const [px, py] = state.run.position;
  if (py === MAP_HEIGHT - 1) return { x: 3, y: MAP_HEIGHT };
  const edges = [...map.rows[py]![px]!.edges].sort((a, b) => a - b);
  return { x: edges[0]!, y: py + 1 };
}

function roomKindAfter(state: GameState): string {
  const room = state.run.room!;
  switch (room.kind) {
    case "monster": case "elite": case "boss": return room.roomKind;
    case "combat": return room.roomKind;
    default: return room.kind;
  }
}

/** 参考实现的 id 全是大写;折成小写并补上别名,输出才能跟本作逐字节比 */
function normalizeNames(text: string): string {
  return text.replace(/"([A-Z][A-Z0-9_]*(?:\+\d*)?)"/g, (_m, id: string) => JSON.stringify(normId(id)));
}

function normId(id: string): string {
  const m = id.match(/^(.*?)(\+\d*)?$/);
  const head = (m![1] ?? "").toLowerCase();
  const tail = m![2] ?? "";
  const alias: Record<string, string> = {
    spike_slime_s: "spike_slime_small",
    acid_slime_s: "acid_slime_small",
    spike_slime_m: "spike_slime_medium",
    acid_slime_m: "acid_slime_medium",
    spike_slime_l: "spike_slime_large",
    acid_slime_l: "acid_slime_large",
    strike_red: "strike",
    defend_red: "defend",
    // 第二章三个 Boss:参考实现叫 automaton/champ/collector,本作带 the_/bronze_ 前缀
    automaton: "bronze_automaton",
    champ: "the_champ",
    collector: "the_collector",
  };
  return (alias[head] ?? head) + tail;
}

// ---- 参考侧补掷:原版开战才定阵容的那些遭遇 ----
//
// 原版开战时才把阵容拼出来(MonsterGroup::createMonsters):先按 miscRng 抽签/掷变体
// 挑怪,再逐只构造(构造从 monsterHpRng 掷血,虱子还顺带掷咬伤),连没被选上的候选
// 也要构造、也要掷.参考实现把这些遭遇的阵容写成了固定名单
// (refs/slay-the-cli/src/content/acts.ts 的 TODO(randomized lineups)),于是一个掷点
// 都没消耗 —— 它的流位置从这里开始就跟原版错开,后面所有掷点全错.
//
// 这里照着原版的规则与顺序,用参考实现自己的 miscRng / monsterHpRng 把这些掷点补上,
// 并且把补掷算出来的阵容连同血量/咬伤/卷曲交给参考侧去开战:参考侧的战斗引擎
// (AI 选招、开局钩子、招式数值)照旧跑它自己的代码,我们只补它没做的那部分掷点.
// 不补的话,参考侧就只是一把没有刻度的尺子.`--raw-ref` 关掉补偿,直接比原样的参考.
//
// 原版顺序(sts_lightspeed MonsterGroup.cpp):抽签/变体(全是 miscRng)→ 逐只构造
// (monsterHpRng:先血,虱子再咬伤)→ 开战时 preBattle(虱子卷曲格挡).本作 Rust 侧的
// 同一条顺序写在 src/core/enemies.rs 的 lineup 函数里,两边正好互相验证.

/** 参考侧(大写)与本作(小写)叫法不同的那几只 */
const REF_IDS: Record<string, string> = {
  acid_slime_small: "ACID_SLIME_S",
  acid_slime_medium: "ACID_SLIME_M",
  acid_slime_large: "ACID_SLIME_L",
  spike_slime_small: "SPIKE_SLIME_S",
  spike_slime_medium: "SPIKE_SLIME_M",
  spike_slime_large: "SPIKE_SLIME_L",
};

/** 参考侧的怪 id:除了上面那几只史莱姆的别名,其余就是本作 id 的大写 */
function refId(ours: string): string {
  const id = REF_IDS[ours] ?? ours.toUpperCase();
  if (!bundle.monsters.has(id as never)) throw new Error(`参考实现里没有这只怪: ${ours}`);
  return id;
}

/** 8 只小鬼(带重复),与小鬼团伙/头目抽签一致 */
const GREMLIN_POOL = [
  "mad_gremlin", "mad_gremlin",
  "sneaky_gremlin", "sneaky_gremlin",
  "fat_gremlin", "fat_gremlin",
  "shield_gremlin", "gremlin_wizard",
];
/** 三种"形状"的 6 只池,每只两份 */
const SHAPE_POOL = ["repulsor", "repulsor", "exploder", "exploder", "spiker", "spiker"];
/** 球体守卫那场里两只形状的 3 选 1 池(放回) */
const SHAPE3_POOL = ["spiker", "repulsor", "exploder"];
/** 一堆史莱姆:3 尖刺小 + 2 酸小,抽完为止 */
const LOTS_OF_SLIMES_POOL = [
  "spike_slime_small", "spike_slime_small", "spike_slime_small",
  "acid_slime_small", "acid_slime_small",
];

type Rolled = { ours: string; hp: number; bite: number | null };

/** 补掷出来的阵容,以及补掷完的各流状态 */
type LineupPlan = {
  /** 参考侧 id(大写),顺序与原版一致 */
  ids: string[];
  hp: number[];
  /** 虱子的咬伤(构造时掷),其余 null */
  bite: (number | null)[];
  /** 虱子开局的卷曲格挡(preBattle 掷),其余 null */
  curl: (number | null)[];
  /** 补掷完阵容抽签后的 miscRng */
  miscAfter: RngState;
  /** 补掷完阵容抽签与卷曲后的 monsterHpRng */
  hpAfter: RngState;
};

function isLouse(id: string): boolean {
  return id === "red_louse" || id === "green_louse";
}

/**
 * 按原版规则把一场遭遇的阵容掷出来,顺带把补掷完的流状态交出来.
 * encounterId 用参考侧的叫法(大写,见 refs/.../content/acts.ts;与本作的遭遇 id
 * 有几处不同,例如参考的 THREE_LOUSE 对应本作的 three_louses、GREMLIN_LEADER
 * 对应本作的 gremlin_leader_gang).不在补偿名单里的遭遇返回 null.
 */
function planLineup(encounterId: string, seed: bigint, floor: number, asc: number): LineupPlan | null {
  // 开战这一刻 floor 流刚按 seed+floorNum 重新定种(原版与本作都一样),
  // 所以补掷从计数器 0 开始 —— 这里拿两条定种后的流,而不是当前状态
  const misc = new Rng(seed + BigInt(floor));
  const hpRng = new Rng(seed + BigInt(floor));

  // 构造一只:掷血;虱子再掷咬伤(原版构造时就连着掷).
  // 原版 Monster::initHp 只有球形守卫/巨口/闪现者直接给固定血量(一次掷点都不消耗),
  // 其余怪即使区间塌成一点也照样掷一次(Random.random(min,max) 一定消耗)——
  // 与本作 ascension::roll_hp 同规则.
  const NO_ROLL_HP = new Set(["SPHERIC_GUARDIAN", "THE_MAW", "TRANSIENT"]);
  const construct = (ours: string): Rolled => {
    const def = bundle.monsters.get(refId(ours) as never);
    if (!def) throw new Error(`参考实现里没有这只怪: ${ours}`);
    const [lo, hi] = def.hp(asc);
    const hp = NO_ROLL_HP.has(refId(ours)) ? lo : hpRng.randomRange(lo, hi);
    // 虱子的咬伤随飞升 2 换档(与本作 ascension::louse_bite_range 同档)
    const bite = isLouse(ours) ? (asc >= 2 ? hpRng.randomRange(6, 8) : hpRng.randomRange(5, 7)) : null;
    return { ours, hp, bite };
  };

  // 不放回地抽 n 个:misc.random(len-1),抽中的从池子里删掉
  const draw = (pool: string[], n: number): string[] => {
    const p = pool.slice();
    const out: string[] = [];
    for (let i = 0; i < n; i++) out.push(p.splice(misc.random(p.length - 1), 1)[0]!);
    return out;
  };
  const louse = (): string => (misc.randomBoolean() ? "red_louse" : "green_louse");
  /** 先构造全部候选(候选的血也掷),再 miscRng 从 0..max 里选一只 */
  const choose = (cands: Rolled[], max: number): Rolled => cands[misc.random(max)]!;

  // Exordium 的"弱野生动物":虱子 / 尖刺中史莱姆 / 酸液中史莱姆
  const weakWildlife = (): Rolled =>
    choose([construct(louse()), construct("spike_slime_medium"), construct("acid_slime_medium")], 2);
  // Exordium 的"强人形":邪教徒 / 奴贩(红蓝随机) / 劫掠者
  const strongHumanoid = (): Rolled => {
    const slaver = misc.randomBoolean() ? "red_slaver" : "blue_slaver";
    return choose([construct("cultist"), construct(slaver), construct("looter")], 2);
  };
  // Exordium 的"强野生动物":真菌兽 / 颚虫
  const strongWildlife = (): Rolled => choose([construct("fungi_beast"), construct("jaw_worm")], 1);

  let rolled: Rolled[];
  switch (encounterId) {
    case "GREMLIN_GANG":
      rolled = draw(GREMLIN_POOL, 4).map(construct);
      break;
    case "LOTS_OF_SLIMES":
      rolled = draw(LOTS_OF_SLIMES_POOL, 5).map(construct);
      break;
    case "SMALL_SLIMES":
      rolled = (misc.randomBoolean()
        ? ["spike_slime_small", "acid_slime_medium"]
        : ["acid_slime_small", "spike_slime_medium"]
      ).map(construct);
      break;
    case "LARGE_SLIME":
      rolled = [construct(misc.randomBoolean() ? "acid_slime_large" : "spike_slime_large")];
      break;
    case "TWO_LOUSE":
      rolled = [construct(louse()), construct(louse())];
      break;
    case "THREE_LOUSE":
      rolled = [construct(louse()), construct(louse()), construct(louse())];
      break;
    case "EXORDIUM_THUGS":
      rolled = [weakWildlife(), strongHumanoid()];
      break;
    case "EXORDIUM_WILDLIFE":
      rolled = [strongWildlife(), weakWildlife()];
      break;
    case "GREMLIN_LEADER":
      // 两只小鬼各抽一只(有放回,允许重复),头目自己排在最后
      rolled = [
        construct(GREMLIN_POOL[misc.random(7)]!),
        construct(GREMLIN_POOL[misc.random(7)]!),
        construct("gremlin_leader"),
      ];
      break;
    case "THREE_SHAPES":
      rolled = draw(SHAPE_POOL, 3).map(construct);
      break;
    case "FOUR_SHAPES":
      rolled = draw(SHAPE_POOL, 4).map(construct);
      break;
    case "SPHERE_AND_TWO_SHAPES":
      // 两只形状放回地抽,球体守卫固定排最后
      rolled = [
        construct(SHAPE3_POOL[misc.random(2)]!),
        construct(SHAPE3_POOL[misc.random(2)]!),
        construct("spheric_guardian"),
      ];
      break;
    default:
      return null;
  }
  const ids = rolled.map((r) => refId(r.ours));

  const miscAfter = misc.saveState();
  // 开局卷曲:每只虱子一次(原版 preBattle,槽位顺序);随飞升 7/17 换档
  const curlTier: [number, number] = asc >= 17 ? [9, 12] : asc >= 7 ? [4, 8] : [3, 7];
  const curl: (number | null)[] = rolled.map((r) =>
    isLouse(r.ours) ? hpRng.randomRange(curlTier[0], curlTier[1]) : null,
  );
  const hpAfter = hpRng.saveState();

  return { ids, hp: rolled.map((r) => r.hp), bite: rolled.map((r) => r.bite), curl, miscAfter, hpAfter };
}

/** 补偿开关(--raw-ref 关掉) */
let COMPENSATE = true;

/** 参考侧把 chosen 当"牌组下标"用的选牌 resume(见 emitPendingPick 里的说明). */
const DECK_INDEX_RESUMES: Record<string, true> = {
  __eventChoice: true,
  __runDeckChoice: true,
  __restToke: true,
};

/**
 * 结算一次挂起的选牌并输出 pick 行.
 *
 * 本作的窗口是"每选一张发一行 pick"(空笼删 2 张 = 候选 20 → 19 两行);
 * 参考侧把整个窗口一次 choose 交完,所以要按被选中的张数逐张补出中间态.
 * 这是驱动的导出约定(不是规则分歧):中间态 = 原状态里删掉前 k+1 张被选中的牌,
 * 牌组下标从 resumeArgs 的 indices 拿(参考侧 removeDeckCards 用的同一组下标).
 */
function emitPendingPick(out: string[], step: number, s: GameState): { s: GameState; step: number } {
  const req = s.pending!.request;
  if (req.kind === "option") {
    // the_library 的"读一本书(20 选 1)"屏:参考把这一屏实现成 option 型待选项
    //(refs/slay-the-cli/src/content/events/lib.ts:372-382 的 requestOptionChoice),
    // 本作的导出器认的是"事件第二屏"(event, options=可选项数).两边都选第 0 项,
    // 下一行状态逐字段一致 —— 只是同一屏被摊成了不同的行(旧口径导成 pick candidates 0).
    // 这里按本作的口径补出 event 行.本作引擎不动.
    const args = s.pending!.resumeArgs as { eventId?: string } | undefined;
    const next = advanceWithEggs(s, { cmd: "choose", indices: [0] } as Command);
    out.push(
      line(
        step,
        "event",
        `"id":${JSON.stringify((args?.eventId ?? "").toLowerCase())},"options":${req.options.length},"pick":0`,
        stateJson(next),
      ),
    );
    return { s: next, step: step + 1 };
  }
  const n = req.kind === "cards" ? req.iids.length : 0;
  const positions = req.kind === "cards" ? req.iids.map((_, i) => i).slice(0, req.min) : [0];
  // 参考侧"选牌"屏的 resume 有两种口径:
  //   - content:relicPickupChoice(pickup.ts:91-116)把 chosen 当**候选位置**,再映射回自己的
  //     indices(chosen.map(i => indices[i]));驱动现在传候选位置,正好对上.
  //   - __eventChoice(lib.ts:355 requestDeckChoice 起的屏)/__runDeckChoice(runFlow.ts:570)/
  //     __restToke(runFlow.ts:560)把 chosen 当**牌组下标**直接用(如 designer/upgrade_shrine 的
  //     onResume 里 `upgradeDeckCard(chosen[0])`,参考无守卫,直接给该下标 upgrades++).
  // 驱动原来一律传候选位置,于是事件屏会把"候选位置 0"错当"牌组下标 0",升了一张不该升/已升过的
  // 牌(seed 19/25).这里按 resume 口径把候选位置换成牌组下标(iids[p]),不动本作引擎.
  const deckPick = req.kind === "cards" && DECK_INDEX_RESUMES[String(s.pending!.resume)] === true;
  const picks = deckPick ? positions.map((p) => req.iids[p]!) : positions;
  const deckIdx = (s.pending!.resumeArgs as { indices?: number[] } | undefined)?.indices;
  if (positions.length > 1 && deckIdx && deckIdx.length >= positions.length) {
    for (let k = 0; k < positions.length - 1; k++) {
      const mid = structuredClone(s);
      // 中间态:按牌组下标从大到小删掉前 k+1 张被选中的牌
      for (let j = k; j >= 0; j--) mid.run.deck.splice(deckIdx[positions[j]!]!, 1);
      out.push(line(step, "pick", `"candidates":${n - k},"pick":0`, stateJson(mid)));
      step += 1;
    }
  }
  const next = advanceWithEggs(s, { cmd: "choose", indices: picks });
  out.push(
    line(step, "pick", `"candidates":${n - Math.max(0, picks.length - 1)},"pick":0`, stateJson(next)),
  );
  step += 1;
  return { s: next, step };
}

/** 把补掷出来的阵容写进参考侧的遭遇表,好让它按这个阵容开战 */
function injectLineup(act: number, encounterId: string, ids: string[]): void {
  const actDef = bundle.acts.find((a) => a.act === act);
  if (!actDef) throw new Error(`参考实现没有第 ${act} 幕`);
  const table = [
    ...actDef.weakEncounters,
    ...actDef.strongEncounters,
    ...actDef.elites,
    ...(actDef.bossEncounters ?? []),
  ] as unknown as { id: string; monsters: string[] }[];
  let hit = false;
  for (const e of table) {
    if (e.id === encounterId) {
      e.monsters = ids.slice();
      hit = true;
    }
  }
  if (!hit) throw new Error(`参考实现的第 ${act} 幕里没有遭遇 ${encounterId}`);
}

/** 猜这一格要打哪一场:未知房要按参考实现自己的未知房规则先解析 */
function encounterAt(state: GameState, x: number, y: number): string | null {
  const run = state.run;
  if (run.act === 4) return null; // 第四幕固定阵容
  let kind: string = y === MAP_HEIGHT ? "boss" : run.map!.rows[y]![x]!.kind;
  if (kind === "unknown") {
    // 只借掷点、不动真状态:run 深拷一份,eventRng 用一份新的注册表
    const runClone = structuredClone(run);
    const ctx: EffectCtx = {
      run: runClone,
      combat: null,
      queue: new ActionQueue(),
      bundle,
      rt: { pending: null, currentItem: null, combatOver: null },
      rng: (s) => RngRegistry.fromState(state.rng).get(s),
      asc: run.ascension,
      emit: () => {},
      requestChoice: () => {},
    };
    kind = resolveUnknownRoom(ctx);
  }
  if (kind === "monster") return run.pools.monsterList[0] ?? null;
  if (kind === "elite") return run.pools.eliteList[0] ?? null;
  if (kind === "boss") return run.map!.bossId;
  return null;
}

/**
 * 补掷出来的血量/咬伤/卷曲,按 id 排成队列,开战期间顶替参考侧自己的那几次掷点.
 *
 * 参考侧开战时会按槽位顺序自己掷点(hp 一只一次;虱子的 preBattle 再掷咬伤+卷曲),
 * 掷出来的值跟原版不是一回事(原版是在构造候选时就掷的).这里把 def 的 hp()/preBattle()
 * 临时接过来,让它们按槽位顺序吐出补掷出来的值:于是参考侧的战斗引擎(开局钩子、
 * 水银沙漏那类开局掉血、燃烧精英的加成)全都照旧作用在"正确的那份血"上,
 * 不用我们事后去猜它被改成了多少.同一个 id 出现多只时队列按槽位先后对上.
 *
 * 返回还原函数:开战一结束就还原(召唤出来的同类怪要掷自己的血).
 */
function stagePlan(plan: LineupPlan): () => void {
  const hpQueue: Record<string, number[]> = {};
  const biteQueue: Record<string, number[]> = {};
  const curlQueue: Record<string, number[]> = {};
  plan.ids.forEach((id, i) => {
    (hpQueue[id] ??= []).push(plan.hp[i]!);
    if (plan.bite[i] !== null) (biteQueue[id] ??= []).push(plan.bite[i]!);
    if (plan.curl[i] !== null) (curlQueue[id] ??= []).push(plan.curl[i]!);
  });
  const restore: (() => void)[] = [];
  for (const id of Object.keys(hpQueue)) {
    const def = bundle.monsters.get(id as never) as unknown as {
      hp: (asc: number) => [number, number];
      preBattle?: (ctx: EffectCtx, self: MonsterState) => void;
    };
    const hp = def.hp;
    const queue = hpQueue[id]!;
    // 退化区间:randomRange(v, v) 照样记一次掷点,但一定吐 v
    def.hp = (): [number, number] => {
      const v = queue.shift();
      if (v === undefined) throw new Error(`${id} 的血量队列空了`);
      return [v, v];
    };
    restore.push(() => {
      def.hp = hp;
    });
    const bites = biteQueue[id];
    if (!bites) continue;
    const curls = curlQueue[id]!;
    const preBattle = def.preBattle;
    def.preBattle = (ctx, self): void => {
      preBattle?.(ctx, self);
      self.data.biteDamage = bites.shift();
      const curl = self.powers.find((x) => x.id === "CURL_UP");
      if (curl) curl.amount = curls.shift()!;
    };
    restore.push(() => {
      def.preBattle = preBattle;
    });
  }
  return (): void => {
    for (const f of restore) f();
  };
}

/** 补掷完开战:把两条流推到原版该在的位置(参考侧自己消耗的那些不再算数) */
function fixStreams(state: GameState, plan: LineupPlan): void {
  state.rng.floor.miscRng = plan.miscAfter;
  state.rng.floor.monsterHpRng = plan.hpAfter;
}

export function replayRefl(seedStr: string, policy: Policy): string {
  let s: GameState = createRun({ seed: seedStr, bundle, character: "IRONCLAD", ascension: policy.ascension });
  // 调试钩子:先给钥匙(会影响切幕时地图标不标燃烧精英),再切幕(与 Rust 侧同序)
  if (policy.keysAll) s.run.keys = { emerald: true, ruby: true, sapphire: true };
  if (policy.hp !== null) {
    s.run.maxHp = policy.hp;
    s.run.hp = policy.hp;
  }
  if (policy.deck === "strong") {
    s.run.deck = Array.from({ length: 10 }, () => ({ defId: "BLUDGEON", upgrades: 1, misc: 0, bottled: false }));
  } else if (policy.deck === "ramp") {
    // 与 Rust 侧 set_replay_deck 一致:10 张强化狂暴,纯状态不掷点
    s.run.deck = Array.from({ length: 10 }, () => ({ defId: "RAMPAGE", upgrades: 1, misc: 0, bottled: false }));
  } else if (policy.deck === "burst") {
    // 与 Rust 侧 set_burst_deck 一致:1 张强化重刃 + 24 张强化火上浇油
    const card = (defId: string): { defId: string; upgrades: number; misc: number; bottled: boolean } => ({
      defId,
      upgrades: 1,
      misc: 0,
      bottled: false,
    });
    s.run.deck = [card("HEAVY_BLADE"), ...Array.from({ length: 24 }, () => card("INFLAME"))];
  }
  for (let i = 1; i < policy.act; i++) s = debugJumpAct(s);
  const out: string[] = [];
  let step = 0;
  let combatKind = "monster";

  out.push(
    line(step, "init", `"seed_str":${JSON.stringify(s.seed)},"boss":${JSON.stringify(s.run.map!.bossId)},"map":${mapJson(s)}`, stateJson(s)),
  );
  step += 1;

  for (let guard = 0; guard < policy.maxSteps; guard++) {
    if (s.pending) {
      ({ s, step } = emitPendingPick(out, step, s));
      continue;
    }
    const room = s.run.room;
    if (!room) throw new Error("no run room");
    switch (room.kind) {
      case "neow": {
        const opts = room.options.map((o) => `[${JSON.stringify(o.bonus)},${JSON.stringify(o.drawback)}]`);
        s = advanceWithEggs(s, { cmd: "neowPick", i: policy.neow });
        out.push(line(step, "neow", `"options":[${opts.join(",")}],"pick":${policy.neow}`, stateJson(s)));
        step += 1;
        break;
      }
      case "map": {
        const target = nextNode(s);
        const from = s.run.position === null ? "null" : `[${s.run.position[0]},${s.run.position[1]}]`;
        const node = target.y === MAP_HEIGHT
          ? "boss"
          : s.run.map!.rows[target.y]![target.x]!.kind;
        // 开战前先把原版该掷的阵容掷出来:补上参考实现漏掉的掷点,并把阵容交给它
        const encId = COMPENSATE ? encounterAt(s, target.x, target.y) : null;
        const plan = encId ? planLineup(encId, BigInt(s.rng.seed), s.run.floor + 1, s.run.ascension) : null;
        let unstaged: (() => void) | null = null;
        if (plan && encId) {
          injectLineup(s.run.act, encId, plan.ids);
          unstaged = stagePlan(plan);
        }
        s = advanceWithEggs(s, { cmd: "mapPick", x: target.x, y: target.y });
        unstaged?.();
        if (plan) fixStreams(s, plan);
        const resolved = roomKindAfter(s);
        if (s.run.room?.kind === "combat") combatKind = resolved;
        let payload =
          `"from":${from},"to":[${target.x},${target.y}],"node":${JSON.stringify(node === "unknown" ? "event" : node)},` +
          `"resolved":${JSON.stringify(resolved)}`;
        if (s.combat) {
          // 参考实现用 GAP 占位符铺满原版的定长怪物槽(collector 在 2 号槽、
          // 小鬼头目在 3 号槽这种),_shared.ts 的 padMonsterSlots 插的.
          // 本作导出的是"真实存在的怪",GAP 不是怪(参考自己的 CLI 也过滤它,
          // 见 slay-the-cli/src/cli/state/view.ts:851),对拍时折掉.
          payload +=
            `,"monsters":[${s.combat.monsters
              .filter((m) => m.id !== "GAP")
              .map((m) => `{"id":${JSON.stringify(m.id)},"hp":${m.hp},"max_hp":${m.maxHp}}`)
              .join(",")}]`;
        }
        out.push(line(step, "move", payload, stateJson(s)));
        step += 1;
        break;
      }
      case "combat": {
        s = policy.smart ? smartPlay(s) : autoPlay(s);
        out.push(line(step, "fight", `"result":"end"`, stateJson(s)));
        step += 1;
        break;
      }
      case "rewards": {
        const isBoss = room.source === "boss";
        const source = isBoss ? "boss" : room.source === "elite" ? "elite" : combatKind;
        const entries = rewardEntries(s);
        let taken: string[] = [];
        if (policy.rewardTake) {
          // 拿遗物会挂起一个选牌(瓶装/浑天仪那类):先出 pick 行选完,
          // 再回来接着拿剩下的奖励(本作 src/core/replay.rs 的奖励分支同序)
          for (let guard = 0; guard < 8; guard++) {
            const res = takeRewards(s, policy);
            s = res.state;
            taken.push(...res.taken);
            if (!s.pending) break;
            while (s.pending) {
              ({ s, step } = emitPendingPick(out, step, s));
            }
          }
          taken.sort((a, b) => rewardRank(a) - rewardRank(b));
        }
        const payload = `"source":${JSON.stringify(source)},"entries":[${entries.join(",")}],"taken":[${taken.map((v) => JSON.stringify(v)).join(",")}]`;
        if (isBoss) {
          out.push(line(step, "reward", payload, stateJson(s)));
          step += 1;
          // 到了策略允许的最后一幕就在这里收尾;否则离开奖励屏去下一幕
          if (s.run.act >= policy.acts) {
            out.push(line(step, "end", `"result":"boss"`, stateJson(s)));
            return finish(out, policy);
          }
          s = advanceWithEggs(s, { cmd: "skipRewards" });
          break;
        }
        s = advanceWithEggs(s, { cmd: "skipRewards" });
        out.push(line(step, "reward", payload, stateJson(s)));
        step += 1;
        break;
      }
      case "rest": {
        const opts = restOptions(s);
        // 钥匙模式:营火优先"回忆"拿红钥匙
        if (policy.keys && restOptionAvailable(scratchCtx(s), "recall")) {
          s = advanceWithEggs(s, { cmd: "restOption", kind: "recall" });
          out.push(line(step, "rest", `"options":[${opts.map((v) => JSON.stringify(v)).join(",")}],"pick":"recall"`, stateJson(s)));
          step += 1;
        } else if (policy.rest === "smith") {
          // 参考实现的打铁要当场给牌组下标;挑第一张能升级的,并补一条 pick 步
          // (本作这边走的是"选牌界面",两边步流才对得上)
          const ctx = scratchCtx(s);
          let deckIdx = -1;
          for (let i = 0; i < s.run.deck.length; i++) {
            if (canSmith(ctx, i)) { deckIdx = i; break; }
          }
          const candidates = s.run.deck.filter((_, i) => canSmith(ctx, i)).length;
          s = advanceWithEggs(s, { cmd: "restOption", kind: "smith", deckIdx });
          out.push(line(step, "rest", `"options":[${opts.map((v) => JSON.stringify(v)).join(",")}],"pick":"smith"`, stateJson(s)));
          step += 1;
          out.push(line(step, "pick", `"purpose":"upgrade","candidates":${candidates},"pick":0`, stateJson(s)));
          step += 1;
        } else {
          s = advanceWithEggs(s, { cmd: "restOption", kind: "rest" });
          out.push(line(step, "rest", `"options":[${opts.map((v) => JSON.stringify(v)).join(",")}],"pick":"rest"`, stateJson(s)));
          step += 1;
          // 梦中情网(原版"休息后可以加一张牌")在参考实现里是 hooks: {} 的空实现.
          // 这里按本作引擎的同一套规则补出那一屏:用参考侧自己的 cardRng 跑一遍
          // createCardReward、把推进后的流写回,再按同一策略拿同一张 —— 两边掷点位置
          // 与牌组才对齐;不补的话参考侧就是一把没刻度的尺子(与本文件对阵容补掷同理).
          if (s.run.relics.some((x) => x.defId === "DREAM_CATCHER")) {
            const cardRng = RngRegistry.fromState(s.rng).get("cardRng");
            const cards = createCardReward({ ...scratchCtx(s), rng: () => cardRng }, "monster");
            s.rng.run.cardRng = cardRng.saveState();
            s.run.room = { kind: "rewards", entries: cardGroupEntries(cards), source: "monster" };
            const entries = rewardEntries(s);
            const res = takeRewards(s, policy);
            s = res.state;
            const taken = [...res.taken].sort((a, b) => rewardRank(a) - rewardRank(b));
            s = advanceWithEggs(s, { cmd: "skipRewards" });
            const payload = `"source":"monster","entries":[${entries.join(",")}],"taken":[${taken.map((v) => JSON.stringify(v)).join(",")}]`;
            out.push(line(step, "reward", payload, stateJson(s)));
            step += 1;
          }
        }
        if (s.run.room?.kind === "rest" && !s.pending) s = advanceWithEggs(s, { cmd: "proceed" });
        break;
      }
      case "shop": {
        const items = shopItems(s);
        s = advanceWithEggs(s, { cmd: "proceed" });
        out.push(line(step, "shop", `"items":[${items.join(",")}],"bought":[]`, stateJson(s)));
        step += 1;
        break;
      }
      case "treasure": {
        const chest = room.chest;
        const before = { gold: s.run.gold, relics: s.run.relics.length };
        // 钥匙模式:箱子还没开又还没有蓝钥匙,就拿蓝钥匙(那件遗物就作废了)
        const takeKey = policy.keys && chest.sapphireKeyAvailable && !chest.opened;
        const cmd: Command = chest.opened
          ? { cmd: "proceed" }
          : takeKey
            ? { cmd: "takeSapphireKey" }
            : { cmd: "openChest" };
        s = advanceWithEggs(s, cmd);
        if (s.run.room?.kind === "treasure" && s.run.room.chest.opened && !s.pending) {
          s = advanceWithEggs(s, { cmd: "proceed" });
        }
        const gained = s.run.relics.slice(before.relics).map((r) => JSON.stringify(r.defId));
        const payload =
          `"size":${JSON.stringify(chest.size)},"gold_present":${chest.goldPresent},"tier":${JSON.stringify(chest.relicTier)},` +
          `"gold":${s.run.gold - before.gold},"relics":[${gained.join(",")}]`;
        out.push(line(step, "treasure", payload, stateJson(s)));
        step += 1;
        break;
      }
      case "event": {
        const id = room.eventId;
        // 参考实现的选项表是整个事件平铺的(如 cursed_tome 5 项、colosseum 3 项),
        // 本作导出的是当前屏真正能选的项数;这里也只数"能选"的,把平铺表折成屏粒度.
        // pick 同时换成"可选项里的序号",两边才是同一把尺子.
        const enabled = id === null ? [0] : eventEnabledOptions(s);
        const count = enabled.length;
        const rawPick = id === null ? 0 : firstEventOption(s, policy.event);
        const pick = Math.max(0, enabled.indexOf(rawPick));
        s = advanceWithEggs(s, { cmd: "eventOption", i: rawPick });
        out.push(line(step, "event", `"id":${JSON.stringify(id ?? "invalid")},"options":${count},"pick":${pick}`, stateJson(s)));
        step += 1;
        break;
      }
      case "gameOver":
        out.push(line(step, "end", `"result":${JSON.stringify(room.victory ? "victory" : "death")}`, stateJson(s)));
        return finish(out, policy);
    }
    if (s.outcome) {
      out.push(line(step, "end", `"result":${JSON.stringify(s.outcome.kind)}`, stateJson(s)));
      return finish(out, policy);
    }
  }
  throw new Error(`走了 ${policy.maxSteps} 步还没到 Boss 奖励`);
}

if (import.meta.main) {
  // 用法与 spire --replay 对齐:`--script <file>` 可选,也可以直接给位置参数.
  // 默认按原版补掷(`--raw-ref` 关掉,直接比原样的参考实现)
  const raw = process.argv.includes("--raw-ref");
  COMPENSATE = !raw;
  const args = process.argv.slice(2).filter((a) => a !== "--script" && a !== "--raw-ref");
  const seedArg = args[0];
  if (!seedArg) {
    console.error("usage: bun tools/replay_ref.ts <seed> [script] [--raw-ref]");
    process.exit(2);
  }
  const scriptPath = args[1];
  const policy = scriptPath ? parsePolicy(readFileSync(scriptPath, "utf8")) : defaultPolicy();
  const seed = /^\d+$/.test(seedArg) ? seedToString(BigInt(seedArg)) : seedArg;
  process.stdout.write(replayRefl(seed, policy));
}
