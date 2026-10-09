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
import { seedToString } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rng.ts";
import { MAP_HEIGHT, MAP_WIDTH } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/mapGen.ts";
import { buildEventScreen } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/eventRuntime.ts";
import { restOptionAvailable } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/runFlow.ts";
import { canSmith } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/rest.ts";
import { readFileSync } from "node:fs";
import { ActionQueue } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/queue.ts";
import { RngRegistry } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rngRegistry.ts";
import type { EffectCtx } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/content/defs.ts";

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
      case "keys": p.keys = val === "on"; break;
      case "reward": p.rewardTake = val === "take"; break;
      case "rest": p.rest = val === "smith" ? "smith" : "rest"; break;
      case "shop": p.shopSkip = val !== "buy"; break;
      default: throw new Error(`不认识 ${key}`);
    }
  }
  return p;
}

const bundle = buildBaseContentBundle();

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
  const rows: string[] = [];
  for (let y = 0; y <= MAP_HEIGHT; y++) {
    const cells: string[] = [];
    for (let x = 0; x < MAP_WIDTH; x++) {
      if (y === MAP_HEIGHT) {
        cells.push(x === 3 ? `{"k":"boss","b":0,"e":[]}` : "null");
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

function autoPlay(state: GameState): GameState {
  let s = state;
  let guard = 0;
  while (s.combat && !s.outcome) {
    if (guard++ > 5000) throw new Error("combat did not end");
    // 战斗内挂起的选牌:跟本作一样选前 min 张(位置下标),不给它单独一步
    if (s.pending) {
      const req = s.pending.request;
      const picks =
        req.kind === "cards"
          ? req.iids.map((_, i) => i).slice(0, req.min)
          : [0];
      s = advance(s, { cmd: "choose", indices: picks }, bundle);
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
        s = advance(s, { cmd: "playCard", handIdx: atkIdx, target }, bundle);
      } catch {
        s = advance(s, { cmd: "endTurn" }, bundle);
      }
    } else {
      s = advance(s, { cmd: "endTurn" }, bundle);
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
      s = advance(s, { cmd: "takeReward", i }, bundle);
      taken.push(tag);
    } catch {
      // 药水格满了之类:标记掉这一项,免得卡死
      if (s.run.room?.kind === "rewards") {
        const e = (s.run.room.entries as Entry[])[i];
        if (e) e.taken = true;
      }
    }
  }
  const rank = (t: string): number =>
    t === "gold" ? 0 : t === "relic" ? 1 : t === "boss_relic" ? 2 : t === "potion" ? 3 : t.startsWith("card") ? 4 : 5;
  taken.sort((a, b) => rank(a) - rank(b));
  return { state: s, taken };
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

/** 事件选项:从 preferred 往后找第一个能选的;都没有就退到第一项,再不行就最后一项 */
function firstEventOption(state: GameState, preferred: number): number {
  const room = state.run.room;
  if (!room || room.kind !== "event") return 0;
  const scr = buildEventScreen(scratchCtx(state));
  if (!scr) return 0;
  const can = (i: number): boolean => {
    try {
      return scr.options[i]!.enabled(scratchCtx(state));
    } catch {
      return false;
    }
  };
  for (let i = preferred; i < scr.options.length; i++) if (can(i)) return i;
  for (let i = 0; i < scr.options.length; i++) if (can(i)) return i;
  return Math.max(0, scr.options.length - 1);
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
  };
  return (alias[head] ?? head) + tail;
}

export function replayRefl(seedStr: string, policy: Policy): string {
  let s: GameState = createRun({ seed: seedStr, bundle, character: "IRONCLAD" });
  const out: string[] = [];
  let step = 0;
  let combatKind = "monster";

  out.push(
    line(step, "init", `"seed_str":${JSON.stringify(s.seed)},"boss":${JSON.stringify(s.run.map!.bossId)},"map":${mapJson(s)}`, stateJson(s)),
  );
  step += 1;

  for (let guard = 0; guard < policy.maxSteps; guard++) {
    if (s.pending) {
      const req = s.pending.request;
      const n = req.kind === "cards" ? req.iids.length : 0;
      // choose 的 indices 是 iids 的下标(见 chosenIid:iids[chosen[0]]),
      // 所以取前 min 个位置,不是前 min 个 iid
      const picks =
        req.kind === "cards"
          ? req.iids.map((_, i) => i).slice(0, req.min)
          : [0];
      s = advance(s, { cmd: "choose", indices: picks }, bundle);
      out.push(line(step, "pick", `"candidates":${n},"pick":0`, stateJson(s)));
      step += 1;
      continue;
    }
    const room = s.run.room;
    if (!room) throw new Error("no run room");
    switch (room.kind) {
      case "neow": {
        const opts = room.options.map((o) => `[${JSON.stringify(o.bonus)},${JSON.stringify(o.drawback)}]`);
        s = advance(s, { cmd: "neowPick", i: policy.neow }, bundle);
        out.push(line(step, "neow", `"options":[${opts.join(",")}],"pick":${policy.neow}`, stateJson(s)));
        step += 1;
        break;
      }
      case "map": {
        const target = nextNode(s);
        const from = s.run.position === null ? "null" : `[${s.run.position[0]},${s.run.position[1]}]`;
        const node = s.run.position !== null && target.y !== MAP_HEIGHT
          ? s.run.map!.rows[target.y]![target.x]!.kind
          : (target.y === MAP_HEIGHT ? "boss" : "monster");
        s = advance(s, { cmd: "mapPick", x: target.x, y: target.y }, bundle);
        const resolved = roomKindAfter(s);
        if (s.run.room?.kind === "combat") combatKind = resolved;
        let payload =
          `"from":${from},"to":[${target.x},${target.y}],"node":${JSON.stringify(node === "unknown" ? "event" : node)},` +
          `"resolved":${JSON.stringify(resolved)}`;
        if (s.combat) {
          payload +=
            `,"monsters":[${s.combat.monsters
              .map((m) => `{"id":${JSON.stringify(m.id)},"hp":${m.hp},"max_hp":${m.maxHp}}`)
              .join(",")}]`;
        }
        out.push(line(step, "move", payload, stateJson(s)));
        step += 1;
        break;
      }
      case "combat": {
        s = autoPlay(s);
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
          const res = takeRewards(s, policy);
          s = res.state;
          taken = res.taken;
        }
        const payload = `"source":${JSON.stringify(source)},"entries":[${entries.join(",")}],"taken":[${taken.map((v) => JSON.stringify(v)).join(",")}]`;
        if (isBoss) {
          out.push(line(step, "reward", payload, stateJson(s)));
          step += 1;
          // 到了策略允许的最后一幕就在这里收尾;否则离开奖励屏去下一幕
          if (s.run.act >= policy.acts) {
            out.push(line(step, "end", `"result":"boss"`, stateJson(s)));
            return normalizeNames(out.join("\n") + "\n");
          }
          s = advance(s, { cmd: "skipRewards" }, bundle);
          break;
        }
        s = advance(s, { cmd: "skipRewards" }, bundle);
        out.push(line(step, "reward", payload, stateJson(s)));
        step += 1;
        break;
      }
      case "rest": {
        const opts = restOptions(s);
        // 钥匙模式:营火优先"回忆"拿红钥匙
        if (policy.keys && restOptionAvailable(scratchCtx(s), "recall")) {
          s = advance(s, { cmd: "restOption", kind: "recall" }, bundle);
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
          s = advance(s, { cmd: "restOption", kind: "smith", deckIdx }, bundle);
          out.push(line(step, "rest", `"options":[${opts.map((v) => JSON.stringify(v)).join(",")}],"pick":"smith"`, stateJson(s)));
          step += 1;
          out.push(line(step, "pick", `"purpose":"upgrade","candidates":${candidates},"pick":0`, stateJson(s)));
          step += 1;
        } else {
          s = advance(s, { cmd: "restOption", kind: "rest" }, bundle);
          out.push(line(step, "rest", `"options":[${opts.map((v) => JSON.stringify(v)).join(",")}],"pick":"rest"`, stateJson(s)));
          step += 1;
        }
        if (s.run.room?.kind === "rest" && !s.pending) s = advance(s, { cmd: "proceed" }, bundle);
        break;
      }
      case "shop": {
        const items = shopItems(s);
        s = advance(s, { cmd: "proceed" }, bundle);
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
        s = advance(s, cmd, bundle);
        if (s.run.room?.kind === "treasure" && s.run.room.chest.opened && !s.pending) {
          s = advance(s, { cmd: "proceed" }, bundle);
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
        const scr = id === null ? null : buildEventScreen(scratchCtx(s));
        const count = scr ? scr.options.length : 1;
        const pick = id === null ? 0 : firstEventOption(s, policy.event);
        s = advance(s, { cmd: "eventOption", i: pick }, bundle);
        out.push(line(step, "event", `"id":${JSON.stringify(id ?? "invalid")},"options":${count},"pick":${pick}`, stateJson(s)));
        step += 1;
        break;
      }
      case "gameOver":
        out.push(line(step, "end", `"result":${JSON.stringify(room.victory ? "victory" : "death")}`, stateJson(s)));
        return normalizeNames(out.join("\n") + "\n");
    }
    if (s.outcome) {
      out.push(line(step, "end", `"result":${JSON.stringify(s.outcome.kind)}`, stateJson(s)));
      return normalizeNames(out.join("\n") + "\n");
    }
  }
  throw new Error(`走了 ${policy.maxSteps} 步还没到 Boss 奖励`);
}

if (import.meta.main) {
  // 用法与 spire --replay 对齐:`--script <file>` 可选,也可以直接给位置参数
  const args = process.argv.slice(2).filter((a) => a !== "--script");
  const seedArg = args[0];
  if (!seedArg) {
    console.error("usage: bun tools/replay_ref.ts <seed> [script]");
    process.exit(2);
  }
  const scriptPath = args[1];
  const policy = scriptPath ? parsePolicy(readFileSync(scriptPath, "utf8")) : defaultPolicy();
  const seed = /^\d+$/.test(seedArg) ? seedToString(BigInt(seedArg)) : seedArg;
  process.stdout.write(replayRefl(seed, policy));
}
