// 金标准 fixture 生成器:用 bun 直接跑参考实现,把同一 seed 下的几个具体数值
// 落成 JSON,供 Rust 侧的金标准测试逐位比对.
//   bun tools/gen_golden.ts 12345 > tools/golden/seed12345.json
// 依赖 refs/slay-the-cli(只读),不修改它.
import { createRun, advance, type GameState } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/game.ts";
import { buildBaseContentBundle } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/content/index.ts";
import { RngRegistry } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rngRegistry.ts";
import { seedToString } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rng.ts";
import {
  generateMap,
  mapToString,
  MAP_HEIGHT,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/mapGen.ts";
import {
  generateEncounters,
  resolveEncounter,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/encounters.ts";
import { autoWinCombat } from "/home/mix/projects/terminal_game/refs/slay-the-cli/tests/run/runCtx.ts";
import type { RewardEntry } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/runState.ts";

const n = Number(process.argv[2] ?? "12345");
const seedStr = seedToString(BigInt(n));
const bundle = buildBaseContentBundle();

// 1) 第一章地图布局
const gm = generateMap(BigInt(n), 0, 1, true);
const mapString = mapToString(gm);

// 2) 开局生成的遭遇表(monsterRng)
const reg = new RngRegistry(BigInt(n));
const act1 = bundle.acts.find((a) => a.act === 1)!;
const gen = generateEncounters(act1, reg.get("monsterRng"));

// 3) 第一个怪房间:遭遇、怪物与掷出来的血量
let state = createRun({ seed: seedStr, bundle, character: "IRONCLAD" });
state = advance(state, { cmd: "neowPick", i: 1 }, bundle);
const row0 = state.run.map!.rows[0]!;
const startX = row0.findIndex((x) => x !== null);
state = advance(state, { cmd: "mapPick", x: startX, y: 0 }, bundle);
const room = state.run.room!;
const firstRoom = {
  x: startX,
  roomKind: room.kind,
  encounterId: room.kind === "combat" ? room.encounterId : null,
  monsters: state.combat!.monsters.map((m) => [m.id, m.maxHp]),
  floor: state.run.floor,
};

// 4) 第一场战斗打赢后的奖励(金币 / 药水 / 三张牌)
state = autoWinCombat(state, bundle);
const rroom = state.run.room!;
const entries = rroom.kind === "rewards" ? rroom.entries : [];
const gold = entries.find((e) => e.kind === "gold");
const potion = entries.find((e) => e.kind === "potion");
const cards = entries.filter((e) => e.kind === "card");
const rewards = {
  gold: gold ? gold.amount : null,
  potion: potion ? potion.id : null,
  cards: cards.map((c) => [c.id, c.upgraded === true]),
};

const lineup = (id: string): string[] => resolveEncounter(act1, id).slice();

const out = {
  seedNumeric: n,
  seedString: seedStr,
  mapHeight: MAP_HEIGHT,
  map: mapString,
  monsterList: gen.monsterList,
  monsterListMobs: gen.monsterList.map(lineup),
  eliteList: gen.eliteList,
  eliteListMobs: gen.eliteList.map(lineup),
  bossOrder: gen.bossOrder,
  bossOrderMobs: gen.bossOrder.map(lineup),
  firstRoom,
  rewards,
};

// --- 多颗种子的同一批数值:第一个怪房间的阵容与血量 + 第一场战斗的奖励 ---
const SEEDS = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89];
interface Case {
  seed: number;
  monsters: [string, number][];
  gold: number;
  potion: string | null;
  cards: [string, boolean][];
}
const cases: Case[] = [];
const rewardStreams = (st: GameState): string =>
  JSON.stringify([st.rng.run.cardRng, st.rng.run.potionRng, st.rng.run.treasureRng, st.rng.run.relicRng]);

for (const s of SEEDS) {
  // Neow 的祝福各不相同,有的会动到奖励那几条流.本作的 Neow 是简化实现,
  // 所以这里只挑"领完不动奖励流"的那个选项,保证两边的流位置是同一个
  const proto = createRun({ seed: seedToString(BigInt(s)), bundle, character: "IRONCLAD" });
  const before = rewardStreams(proto);
  let st: GameState | null = null;
  // 固定领第 2 个祝福(只有奖励没有代价),但它对每条流的影响因种子而异,
  // 所以只在"领完奖励流没动"时才把这个种子收进 fixture
  {
    const cand = advance(createRun({ seed: seedToString(BigInt(s)), bundle, character: "IRONCLAD" }), { cmd: "neowPick", i: 1 }, bundle);
    if (!cand.pending && cand.run.room?.kind === "map" && rewardStreams(cand) === before) {
      st = cand;
    } else {
      console.error(`seed ${s}: Neow 那一步动了奖励流,跳过`);
      continue;
    }
  }
  if (st === null) {
    console.error(`seed ${s}: 没有不消耗奖励流的 Neow 选项,跳过`);
    continue;
  }
  const r0 = st.run.map!.rows[0]!;
  const x0 = r0.findIndex((v) => v !== null);
  st = advance(st, { cmd: "mapPick", x: x0, y: 0 }, bundle);
  const monsters: [string, number][] = st.combat!.monsters.map((m) => [m.id, m.maxHp]);
  st = autoWinCombat(st, bundle);
  const room = st.run.room;
  const ents: RewardEntry[] = room !== null && room.kind === "rewards" ? room.entries : [];
  const g = ents.find((e) => e.kind === "gold");
  const p = ents.find((e) => e.kind === "potion");
  const cs = ents.filter((e) => e.kind === "card");
  cases.push({
    seed: s,
    monsters,
    gold: g !== undefined && g.kind === "gold" ? g.amount : 0,
    potion: p !== undefined && p.kind === "potion" ? p.id : null,
    cards: cs.map((c) => (c.kind === "card" ? [c.id, c.upgraded === true] : ["", false]) as [string, boolean]),
  });
}

console.log(JSON.stringify(out, null, 2));

// 同一批数值再落一份 Rust 常量,给 src/core/golden.rs 逐位比对用
const rust: string[] = [];
rust.push("// 由 tools/gen_golden.ts 从参考实现(refs/slay-the-cli)跑出来,不要手改.");
rust.push("// 生成命令:bun tools/gen_golden.ts " + n + " > tools/golden/seed" + n + ".json");
rust.push("");
rust.push("/// 数字种子");
rust.push(`pub const SEED: u64 = ${n};`);
rust.push("/// 同一颗种子在参考实现里的 base-35 写法");
rust.push(`pub const SEED_STRING: &str = "${seedStr}";`);
rust.push("/// 第一章地图布局(行间用 \\n,怪物格是 M)");
rust.push(`pub const MAP: &str = "${mapString.replace(/\n/g, "\\n")}";`);
rust.push("/// 开局生成的怪名单(monsterRng)");
rust.push(`pub const MONSTER_LIST: &[&str] = &[${gen.monsterList.map((s) => `"${s}"`).join(", ")}];`);
rust.push("/// 开局生成的精英名单");
rust.push(`pub const ELITE_LIST: &[&str] = &[${gen.eliteList.map((s) => `"${s}"`).join(", ")}];`);
rust.push("/// 本局 Boss 顺序");
rust.push(`pub const BOSS_ORDER: &[&str] = &[${gen.bossOrder.map((s) => `"${s}"`).join(", ")}];`);
rust.push("/// 开局生成的怪名单(monsterRng):每一场的怪物阵容");
rust.push(`pub const MONSTER_LINEUPS: &[&[&str]] = &[${gen.monsterList
  .map((id) => `&[${lineup(id).map((m) => `"${m}"`).join(", ")}]`)
  .join(", ")}];`);
rust.push("/// 开局生成的精英名单的阵容");
rust.push(`pub const ELITE_LINEUPS: &[&[&str]] = &[${gen.eliteList
  .map((id) => `&[${lineup(id).map((m) => `"${m}"`).join(", ")}]`)
  .join(", ")}];`);
rust.push("/// 本局 Boss 顺序的阵容");
rust.push(`pub const BOSS_LINEUPS: &[&[&str]] = &[${gen.bossOrder
  .map((id) => `&[${lineup(id).map((m) => `"${m}"`).join(", ")}]`)
  .join(", ")}];`);
rust.push("/// 第一个怪房间在哪一列");
rust.push(`pub const FIRST_ROOM_X: usize = ${firstRoom.x};`);
rust.push("/// 第一个怪房间的遭遇 id");
rust.push(`pub const FIRST_ROOM_ENCOUNTER: &str = "${firstRoom.encounterId}";`);
rust.push("/// 第一个怪房间的怪物与掷出来的血量");
rust.push(
  `pub const FIRST_ROOM_MONSTERS: &[(&str, i32)] = &[${firstRoom.monsters
    .map((m) => `("${m[0]}", ${m[1]})`)
    .join(", ")}];`,
);
rust.push("/// 第一场战斗的金币奖励");
rust.push(`pub const REWARD_GOLD: i32 = ${rewards.gold};`);
rust.push("/// 第一场战斗的药水奖励(没有就是 None)");
rust.push(
  `pub const REWARD_POTION: Option<&str> = ${rewards.potion === null ? "None" : `Some("${rewards.potion}")`};`,
);
rust.push("/// 第一场战斗的三张卡牌(升级标记)");
rust.push(
  `pub const REWARD_CARDS: &[(&str, bool)] = &[${rewards.cards
    .map((c) => `("${c[0]}", ${c[1]})`)
    .join(", ")}];`,
);
rust.push("/// 多颗种子的第一个怪房间与第一场战斗奖励");
rust.push("pub const CASES: &[Case] = &[");
for (const c of cases) {
  rust.push(
    `    Case { seed: ${c.seed}, monsters: &[${c.monsters
      .map((m) => `("${m[0]}", ${m[1]})`)
      .join(", ")}], gold: ${c.gold}, potion: ${
      c.potion === null ? "None" : `Some("${c.potion}")`
    }, cards: &[${c.cards.map((x) => `("${x[0]}", ${x[1]})`).join(", ")}] },`,
  );
}
rust.push("];");
rust.push("");
await Bun.write(`src/core/golden_fixture.rs`, rust.join("\n"));
console.error(`wrote src/core/golden_fixture.rs (seed ${n})`);
