// 沙盒差分尺(参考侧):吃同一份 scenario.json,吐出与
// `spire --sandbox <seed> <scenario.json>` 同 schema 的 JSONL。
// 直接用 bun 驱动参考实现的战斗内核(createCombatGame + advance),绕过它的 CLI 与 run 层。
//
//   bun tools/sandbox_ref.ts <seed> <scenario.json>
//
// scenario 的字段、牌记号("strike+"、"strike+2")、状态键与动作语义,
// 都与 src/core/replay.rs 的 sandbox 模块一致;两边对拍由 tools/sandbox_diff.ts 做。
// 依赖 refs/slay-the-cli(只读)。

import {
  createCombatGame,
  advance,
  type GameState,
  type Command,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/game.ts";
import { buildBaseContentBundle } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/content/index.ts";
import { seedToString } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rng.ts";
import { RngRegistry } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rngRegistry.ts";
import type { CardInstance, CombatState, MonsterState } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/combat/combatState.ts";
import type { ContentBundle } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/content/defs.ts";
import type { RunState } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/runState.ts";
import { readFileSync } from "node:fs";

// ---- 名字归一:本作的 id 与参考实现的 id 互转 ----

/** 本作(小写)→ 参考实现(大写) */
const TO_REF_CARD: Record<string, string> = {
  strike: "STRIKE_RED",
  defend: "DEFEND_RED",
};

const TO_REF_MONSTER: Record<string, string> = {
  spike_slime_small: "SPIKE_SLIME_S",
  acid_slime_small: "ACID_SLIME_S",
  spike_slime_medium: "SPIKE_SLIME_M",
  acid_slime_medium: "ACID_SLIME_M",
  spike_slime_large: "SPIKE_SLIME_L",
  acid_slime_large: "ACID_SLIME_L",
};

/** 参考实现(大写)→ 本作(小写);differ 只比这一层 */
const FROM_REF: Record<string, string> = {
  strike_red: "strike",
  defend_red: "defend",
  spike_slime_s: "spike_slime_small",
  acid_slime_s: "acid_slime_small",
  spike_slime_m: "spike_slime_medium",
  acid_slime_m: "acid_slime_medium",
  spike_slime_l: "spike_slime_large",
  acid_slime_l: "acid_slime_large",
};

function toRefCard(id: string): string {
  return TO_REF_CARD[id] ?? id.toUpperCase();
}

function toRefMonster(id: string): string {
  return TO_REF_MONSTER[id] ?? id.toUpperCase();
}

/** 参考实现的 id → 本作的小写 id(differ 直接比字符串) */
function normId(id: string): string {
  const lower = id.toLowerCase();
  return FROM_REF[lower] ?? lower;
}

/** 能力名两边叫法不同的:参考实现 → 本作 */
const FROM_REF_POWER: Record<string, string> = {
  sadistic: "sadistic_nature",
  regen: "regenerate",
};

function normPower(id: string): string {
  const lower = id.toLowerCase();
  return FROM_REF_POWER[lower] ?? lower;
}

/** 本作的状态键 → 参考实现的 power id(参考实现那边没有再归一) */
const TO_REF_POWER: Record<string, string> = {
  anger: "ANGRY",
};

function toRefPower(key: string): string {
  const k = key.trim().toLowerCase().replace(/[\s-]+/g, "_");
  return TO_REF_POWER[k] ?? k.toUpperCase();
}

// ---- scenario 类型 ----

type Scenario = {
  encounter?: string;
  player?: {
    hp?: number;
    max_hp?: number;
    block?: number;
    energy?: number;
    max_energy?: number;
    powers?: Record<string, number>;
  };
  relics?: string[];
  potions?: (string | null)[];
  deck?: string[];
  hand?: string[];
  draw?: string[];
  discard?: string[];
  exhaust?: string[];
  enemies: {
    id: string;
    hp?: number;
    max_hp?: number;
    block?: number;
    powers?: Record<string, number>;
    move?: string;
    slot?: number;
  }[];
  actions: {
    op: string;
    hand?: number;
    target?: number | null;
    slot?: number;
    choose?: number[];
  }[];
};

// ---- 牌记号 ----

function splitToken(tok: string): { id: string; upgrades: number } {
  const m = /^(.*?)\+(\d*)$/.exec(tok);
  if (!m) return { id: tok, upgrades: 0 };
  const n = m[2] === "" ? 1 : Number.parseInt(m[2]!, 10);
  return { id: m[1]!, upgrades: Number.isFinite(n) ? n : 1 };
}

function tokenOf(c: CardInstance): string {
  const id = normId(c.defId);
  if (c.upgrades > 1) return `${id}+${c.upgrades}`;
  if (c.upgrades === 1) return `${id}+`;
  return id;
}

// ---- 状态拼装 ----

function powersJson(powers: { id: string; amount: number }[]): string {
  const items = powers
    .filter((p) => p.amount !== 0)
    .map((p) => `${JSON.stringify(normPower(p.id))}:${p.amount}`);
  return `{${items.join(",")}}`;
}

function pileJson(c: CombatState, ids: number[]): string {
  const items = ids.map((iid) => JSON.stringify(tokenOf(c.cards[iid]!)));
  return `[${items.join(",")}]`;
}

/** 参考实现的招式 id 带怪物前缀(如 CULTIST_INCANTATION),剥成与本能一致的写法 */
function moveName(m: MonsterState): string {
  if (!m.move) return "";
  const lower = m.move.toLowerCase();
  const prefix = `${m.id.toLowerCase()}_`;
  return lower.startsWith(prefix) ? lower.slice(prefix.length) : lower;
}

function phaseOf(s: GameState): string {
  if (s.outcome?.kind === "death") return "lost";
  const c = s.combat;
  if (c && c.monsters.length > 0 && c.monsters.every((m) => m.isDead || m.isEscaped)) {
    return "won";
  }
  return c && c.playerTurn ? "player" : "enemy";
}

function stateJson(s: GameState): string {
  const c = s.combat!;
  const run: RunState = s.run;
  const enemies = c.monsters
    .map(
      (m) =>
        `{"id":${JSON.stringify(normId(m.id))},"hp":${m.hp},"max_hp":${m.maxHp},` +
        `"block":${m.block},"dead":${m.isDead || m.isEscaped},` +
        `"move":${JSON.stringify(moveName(m))},"powers":${powersJson(m.powers)}}`,
    )
    .join(",");
  const pots = run.potions.map((p) => (p ? JSON.stringify(normId(p)) : "null")).join(",");
  return (
    `{"turn":${c.turn},"phase":${JSON.stringify(phaseOf(s))},` +
    `"energy":${c.player.energy},"max_energy":${c.player.energyPerTurn},` +
    `"player":{"hp":${run.hp},"max_hp":${run.maxHp},"block":${c.player.block},` +
    `"powers":${powersJson(c.player.powers)}},` +
    `"hand":${pileJson(c, c.player.piles.hand)},` +
    `"draw":${pileJson(c, c.player.piles.draw)},` +
    `"discard":${pileJson(c, c.player.piles.discard)},` +
    `"exhaust":${pileJson(c, c.player.piles.exhaust)},` +
    `"potions":[${pots}],"enemies":[${enemies}]}`
  );
}

function line(step: number, op: string, rest: string): string {
  return `{"step":${step},"op":${JSON.stringify(op)},${rest}}\n`;
}

// ---- 构造与覆盖 ----

/** 按 scenario 重建牌堆(顶牌在数组开头),masterIdx 与掷点取数与本作一致 */
function rebuildPiles(
  c: CombatState,
  order: [("hand" | "draw" | "discard" | "exhaust"), string[]][],
  bundle: ContentBundle,
): void {
  c.cards = {};
  c.player.piles = { draw: [], hand: [], discard: [], exhaust: [], limbo: [] };
  let iid = 1;
  let masterIdx = 0;
  for (const [pile, tokens] of order) {
    for (const tok of tokens) {
      const { id, upgrades } = splitToken(tok);
      const def = bundle.cards.get(toRefCard(id));
      if (!def) throw new Error(`unknown card in scenario: ${tok}`);
      const cost =
        upgrades > 0 && def.upgradeValues.cost !== undefined ? def.upgradeValues.cost : def.cost;
      c.cards[iid] = {
        iid,
        defId: def.id,
        upgrades,
        cost,
        costForTurn: cost,
        freeToPlayOnce: false,
        masterIdx,
        misc: 0,
        retainOnce: false,
      };
      c.player.piles[pile].push(iid);
      iid++;
      masterIdx++;
    }
  }
  c.nextCardInstanceId = iid;
  c.turnFlags = {
    cardsPlayedThisTurn: 0,
    attacksPlayedThisTurn: 0,
    skillsPlayedThisTurn: 0,
    endTurnQueued: false,
    manualDiscardsThisTurn: 0,
  };
  c.combatFlags = {
    cardsPlayedThisCombat: 0,
    attacksPlayedThisCombat: 0,
    skillsPlayedThisCombat: 0,
    powersPlayedThisCombat: 0,
    turnsTaken: 0,
    hpLostThisCombat: 0,
    encounterId: c.combatFlags.encounterId,
  };
  c.turn = 1;
  c.playerTurn = true;
}

function build(
  sc: Scenario,
  seedStr: string,
  seedBigInt: bigint,
  bundle: ContentBundle,
): GameState {
  const explicit =
    sc.hand !== undefined || sc.draw !== undefined || sc.discard !== undefined || sc.exhaust !== undefined;
  const deckTokens = explicit
    ? [...(sc.hand ?? []), ...(sc.draw ?? []), ...(sc.discard ?? []), ...(sc.exhaust ?? [])]
    : (sc.deck ?? []);
  let state = createCombatGame({
    seed: seedStr,
    bundle,
    character: "IRONCLAD",
    deck: deckTokens.map((t) => {
      const s = splitToken(t);
      return { defId: toRefCard(s.id), upgrades: s.upgrades };
    }),
    relics: (sc.relics ?? []).map((id) => id.toUpperCase()),
    monsters: sc.enemies.map((e) => toRefMonster(e.id)),
    encounterId: sc.encounter ?? "SANDBOX",
    hp: sc.player?.hp ?? 80,
    maxHp: sc.player?.max_hp ?? 80,
  });
  // 初始化时挂起的选牌(赌徒筹码)先按"一张不选"收掉:必须在重建牌堆之前,
  // 否则选牌里记的还是旧 iid
  let guard = 0;
  while (state.pending && guard < 8) {
    guard++;
    state = advance(state, { cmd: "choose", indices: [] }, bundle);
  }
  const c = state.combat!;
  const run = state.run;

  if (sc.player) {
    if (sc.player.hp !== undefined) run.hp = sc.player.hp;
    if (sc.player.max_hp !== undefined) run.maxHp = sc.player.max_hp;
    if (sc.player.block !== undefined) c.player.block = sc.player.block;
    if (sc.player.energy !== undefined) c.player.energy = sc.player.energy;
    if (sc.player.max_energy !== undefined) c.player.energyPerTurn = sc.player.max_energy;
    if (sc.player.powers !== undefined) {
      c.player.powers = Object.entries(sc.player.powers).map(([k, v]) => ({
        id: toRefPower(k),
        amount: v,
        justApplied: false,
        data: null,
      }));
    }
  }

  if (explicit) {
    rebuildPiles(
      c,
      [
        ["hand", sc.hand ?? []],
        ["draw", sc.draw ?? []],
        ["discard", sc.discard ?? []],
        ["exhaust", sc.exhaust ?? []],
      ],
      bundle,
    );
  }

  // 药水格
  const pots = [...(sc.potions ?? [])];
  while (pots.length < 3) pots.push(null);
  run.potions = pots.map((p) => (p ? p.toUpperCase() : null));

  // 敌人:血/格挡/状态/招式全按 scenario 摆
  sc.enemies.forEach((spec, i) => {
    const m = c.monsters[i];
    if (!m) return;
    if (spec.hp !== undefined) m.hp = spec.hp;
    if (spec.max_hp !== undefined) m.maxHp = spec.max_hp;
    if (spec.hp !== undefined && spec.max_hp === undefined) m.maxHp = spec.hp;
    if (spec.block !== undefined) m.block = spec.block;
    if (spec.powers !== undefined) {
      m.powers = Object.entries(spec.powers).map(([k, v]) => ({
        id: toRefPower(k),
        amount: v,
        justApplied: false,
        data: null,
      }));
    }
    if (spec.move !== undefined) {
      const mid = toRefMonster(spec.id);
      const def = bundle.monsters.get(mid);
      const candidate = `${mid}_${spec.move.toUpperCase().replace(/[\s-]+/g, "_")}`;
      if (def && def.moves[candidate]) {
        m.move = candidate;
        // 这一招当"初始化时掷出来的首招":moveHistory 里放着它,
        // 与对方 state.last = Some(next_move) 表示同一件事(firstTurn 两边一致)
        m.moveHistory = [candidate];
      }
    }
    m.data = {};
    m.isDead = false;
    m.isEscaped = false;
    m.halfDead = false;
    m.idx = i;
  });

  // 掷点流重置:初始化阶段两边消耗的掷点数可能不同,重置成同一颗种子之后,
  // 动作阶段的随机(洗牌/随机目标/随机卡)才能逐步对齐(与本作一致)。
  state.rng = new RngRegistry(seedBigInt).saveState();
  return state;
}

// ---- 主流程 ----

/** 跑一段 scenario,返回它那一段 JSONL(与 `spire --sandbox` 逐行同 schema) */
function runOne(sc: Scenario, seedStr: string, seedBigInt: bigint, bundle: ContentBundle): string {
  let state = build(sc, seedStr, seedBigInt, bundle);
  const out: string[] = [];
  out.push(line(0, "init", `"st":${stateJson(state)}`));

  const n = sc.actions.length;
  for (let i = 0; i < n; i++) {
    const a = sc.actions[i]!;
    const step = i + 1;
    const choose = a.choose ?? [0];
    let cmd: Command;
    let op: string;
    if (a.op === "play") {
      op = "play";
      cmd = { cmd: "playCard", handIdx: a.hand ?? 0, target: a.target ?? undefined };
    } else if (a.op === "end_turn" || a.op === "endTurn") {
      op = "end_turn";
      cmd = { cmd: "endTurn" };
    } else if (a.op === "potion") {
      op = "potion";
      cmd = { cmd: "usePotion", slot: a.slot ?? 0, target: a.target ?? undefined };
    } else if (a.op === "noop" || a.op === "snapshot") {
      out.push(line(step, "noop", `"st":${stateJson(state)}`));
      continue;
    } else {
      out.push(line(step, "unknown", `"error":${JSON.stringify(`unknown op ${a.op}`)}`));
      break;
    }
    try {
      // 开战就挂起的选牌(赌徒之骰/工具箱)先收掉,否则下一步会被挡住
      let guard = 0;
      while (state.pending && guard < 16) {
        guard++;
        state = advance(state, { cmd: "choose", indices: choose }, bundle);
      }
      state = advance(state, cmd, bundle);
      while (state.pending && guard < 16) {
        guard++;
        state = advance(state, { cmd: "choose", indices: choose }, bundle);
      }
    } catch (e) {
      out.push(line(step, op, `"error":${JSON.stringify(String((e as Error).message ?? e))}`));
      break;
    }
    out.push(line(step, op, `"st":${stateJson(state)}`));
  }
  return out.join("");
}

function main(): void {
  const argv = process.argv.slice(2);
  const batch = argv[0] === "--batch";
  if (batch ? argv.length < 3 : argv.length < 2) {
    console.error(
      "usage: bun tools/sandbox_ref.ts <seed> <scenario.json>\n" +
        "       bun tools/sandbox_ref.ts --batch <seed> <list>   # list 每行 `name<TAB>path`",
    );
    process.exit(2);
  }
  const seedArg = batch ? argv[1]! : argv[0]!;
  const seedStr = /^\d+$/.test(seedArg) ? seedToString(BigInt(seedArg)) : seedArg;
  const seedBigInt = /^\d+$/.test(seedArg) ? BigInt(seedArg) : BigInt(seedStr);
  const bundle = buildBaseContentBundle();

  if (!batch) {
    const sc: Scenario = JSON.parse(readFileSync(argv[1]!, "utf8"));
    process.stdout.write(runOne(sc, seedStr, seedBigInt, bundle));
    return;
  }

  const out: string[] = [];
  for (const raw of readFileSync(argv[2]!, "utf8").split("\n")) {
    const lineText = raw.trim();
    if (!lineText || lineText.startsWith("#")) continue;
    const tab = lineText.indexOf("\t");
    const name = tab === -1 ? lineText : lineText.slice(0, tab).trim();
    const path = tab === -1 ? lineText : lineText.slice(tab + 1).trim();
    out.push(`#${name}\n`);
    try {
      const sc: Scenario = JSON.parse(readFileSync(path, "utf8"));
      out.push(runOne(sc, seedStr, seedBigInt, bundle));
    } catch (e) {
      out.push(
        `{"step":-1,"op":"fail","error":${JSON.stringify(String((e as Error).message ?? e))}}\n`,
      );
    }
  }
  process.stdout.write(out.join(""));
}

main();
