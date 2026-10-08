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
import { makeTestCtx } from "/home/mix/projects/terminal_game/refs/slay-the-cli/tests/run/runCtx.ts";
import { resolveUnknownRoom, generateEventId } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/runFlow.ts";
import { generateShop } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/shop.ts";
import {
  combatRelicTier,
  eliteRelicTier,
  obtainRelicFromPool,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/rewards.ts";
import {
  setupTreasureRoom,
  openChestContents,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/treasure.ts";
import {
  returnRandomPotion,
  rollPotionReward,
} from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/rewards.ts";
import { getNeowOptions } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/neow.ts";
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

// 5) Neow 掷出来的四个选项(neowRng)
const neowReg = new RngRegistry(BigInt(n));
const neowOptions = getNeowOptions(neowReg.get("neowRng")).map((o) => [o.bonus, o.drawback]);
const neowState = neowReg.get("neowRng").saveState();

const mkState = () => createRun({ seed: seedStr, bundle, character: "IRONCLAD" });

// 6) 未知房判定:连续判定 12 次(每次一个 eventRng float + 递增概率)
const unknownRooms = (() => {
  const st = mkState();
  const { ctx } = makeTestCtx(st, bundle);
  const out: string[] = [];
  for (let i = 0; i < 12; i++) out.push(resolveUnknownRoom(ctx));
  return out;
})();

// 7) 事件抽取:连续抽 12 次(神龛 0.25 掷点、一次性事件抽走即移除)
const eventPicks = (() => {
  const st = mkState();
  const { ctx } = makeTestCtx(st, bundle);
  const out: (string | null)[] = [];
  for (let i = 0; i < 12; i++) out.push(generateEventId(ctx));
  return out;
})();

// 7b) 判定与抽取交替:每次判定完,判成事件才抽一个
// (未知房判定推进 eventRng,抽签掷在副本上,主流只走那一次判定)
const eventRolls = (() => {
  const st = mkState();
  const { ctx } = makeTestCtx(st, bundle);
  const out: string[] = [];
  for (let i = 0; i < 8; i++) {
    const kind = resolveUnknownRoom(ctx);
    const id = kind === "event" ? generateEventId(ctx) : null;
    out.push(id === null ? kind : `${kind}:${id}`);
  }
  return out;
})();

// 8) 商店货架与价格
const shopDump = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const shop = generateShop(ctx);
  saveRng();
  return {
    cards: shop.cards.map((c) => ({ id: c.id, rarity: c.rarity, price: c.price, colorless: c.colorless })),
    relics: shop.relics.map((r) => ({ id: r.id, tier: r.tier, price: r.price })),
    potions: shop.potions.map((p) => ({ id: p.id, price: p.price })),
    removalCost: shop.removalCost,
    streams: {
      card: st.rng.run.cardRng.counter,
      merchant: st.rng.run.merchantRng.counter,
      potion: st.rng.run.potionRng.counter,
    },
  };
})();

// 9) Neow 领第 2 项(无代价那一档)之后的状态
const neowPick = (() => {
  let st = createRun({ seed: seedStr, bundle, character: "IRONCLAD" });
  st = advance(st, { cmd: "neowPick", i: 1 }, bundle);
  return {
    bonus: neowOptions[1]![0],
    drawback: neowOptions[1]![1],
    hp: st.run.hp,
    maxHp: st.run.maxHp,
    gold: st.run.gold,
    deck: st.run.deck.length,
    relics: st.run.relics.map((r) => r.defId),
    pending: st.pending !== null,
    roomKind: st.run.room?.kind ?? null,
  };
})();


// 10) 药水身份序列:奖励路径(d100 掉落 + 保底 ±10 + 稀有度掷点 + 池内逐瓶抽)
//     与商店路径(只走稀有度掷点 + 池内抽签)
const potionRewardSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const ids: (string | null)[] = [];
  for (let i = 0; i < 12; i++) ids.push(rollPotionReward(ctx, 1));
  saveRng();
  return { ids, counter: st.rng.run.potionRng.counter, pity: ctx.run.blizzard.potionChance };
})();

const potionShopSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const ids: string[] = [];
  for (let i = 0; i < 12; i++) ids.push(returnRandomPotion(ctx)!);
  saveRng();
  return { ids, counter: st.rng.run.potionRng.counter };
})();


// 11) 遗物掉落身份序列
//     一局开局会把 普通/罕见/稀有/商店/Boss 五个池子各洗一次(各耗 relicRng 一个 long),
//     之后抽哪一件完全由池子顺序决定(战斗中不再掷点).
//     战斗/事件档:<50 普通,<83 罕见,其余稀有(relicRng)
const relicCombatSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const out: { tier: string; id: string }[] = [];
  for (let i = 0; i < 12; i++) {
    const tier = combatRelicTier(ctx);
    out.push({ tier, id: obtainRelicFromPool(ctx.run, tier) });
  }
  saveRng();
  return out;
})();

// 精英档:<50 普通,>82 稀有,其余罕见(relicRng)
const relicEliteSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const out: { tier: string; id: string }[] = [];
  for (let i = 0; i < 12; i++) {
    const tier = eliteRelicTier(ctx);
    out.push({ tier, id: obtainRelicFromPool(ctx.run, tier) });
  }
  saveRng();
  return out;
})();

// Boss 三选一:一次连取三件(不掷点)
const relicBossSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const sets: string[][] = [];
  for (let s = 0; s < 4; s++) {
    const three: string[] = [];
    for (let i = 0; i < 3; i++) three.push(obtainRelicFromPool(ctx.run, "boss"));
    sets.push(three);
  }
  saveRng();
  return sets;
})();

// 宝箱:尺寸一掷,金币/档次共用第二掷(treasureRng 的单掷怪癖)
const relicChestSeq = (() => {
  const st = mkState();
  const { ctx, saveRng } = makeTestCtx(st, bundle);
  const out: { size: string; goldPresent: boolean; tier: string; id: string }[] = [];
  for (let i = 0; i < 6; i++) {
    const chest = setupTreasureRoom(ctx);
    const contents = openChestContents(ctx, chest, false);
    out.push({ size: chest.size, goldPresent: chest.goldPresent, tier: chest.relicTier, id: contents.relicId! });
  }
  saveRng();
  return out;
})();

const out = {
  seedNumeric: n,
  seedString: seedStr,
  mapHeight: MAP_HEIGHT,
  map: mapString,
  burningElite: { x: gm.burningEliteX, y: gm.burningEliteY, buff: gm.burningEliteBuff },
  monsterList: gen.monsterList,
  monsterListMobs: gen.monsterList.map(lineup),
  eliteList: gen.eliteList,
  eliteListMobs: gen.eliteList.map(lineup),
  bossOrder: gen.bossOrder,
  bossOrderMobs: gen.bossOrder.map(lineup),
  firstRoom,
  rewards,
  neowOptions,
  neowState,
  neowPick,
  unknownRooms,
  eventPicks,
  eventRolls,
  shop: shopDump,
  potionRewardSeq,
  potionShopSeq,
  relicCombatSeq,
  relicEliteSeq,
  relicBossSeq,
  relicChestSeq,
};

// --- 多颗种子的同一批数值:第一个怪房间的阵容与血量 + 第一场战斗的奖励 ---
const SEEDS = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89];
interface Case {
  seed: number;
  burning: [number, number, number];
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
  const bm = generateMap(BigInt(s), 0, 1, true);
  cases.push({
    seed: s,
    burning: [bm.burningEliteX, bm.burningEliteY, bm.burningEliteBuff],
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
rust.push("/// 第一章的燃烧精英:(列, 行, 增益编号)");
rust.push(
  `pub const BURNING_ELITE: (i32, i32, i32) = (${gm.burningEliteX}, ${gm.burningEliteY}, ${gm.burningEliteBuff});`,
);
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
    `    Case { seed: ${c.seed}, burning: (${c.burning[0]}, ${c.burning[1]}, ${c.burning[2]}), monsters: &[${c.monsters
      .map((m) => `("${m[0]}", ${m[1]})`)
      .join(", ")}], gold: ${c.gold}, potion: ${
      c.potion === null ? "None" : `Some("${c.potion}")`
    }, cards: &[${c.cards.map((x) => `("${x[0]}", ${x[1]})`).join(", ")}] },`,
  );
}
rust.push("];");
rust.push("");
rust.push("/// Neow 掷出来的四个选项(祝福, 代价)");
rust.push(
  `pub const NEOW_OPTIONS: &[(&str, &str)] = &[${neowOptions
    .map((o) => `("${o[0]}", "${o[1]}")`)
    .join(", ")}];`,
);
rust.push("/// Neow 掷完选项后 neowRng 的计数器");
rust.push(`pub const NEOW_RNG_COUNTER: u32 = ${neowState.counter};`);
rust.push("/// Neow 领第 2 项之后的局面");
rust.push(`pub const NEOW_PICK_BONUS: &str = "${neowPick.bonus}";`);
rust.push(`pub const NEOW_PICK_DRAWBACK: &str = "${neowPick.drawback}";`);
rust.push(`pub const NEOW_PICK_HP: i32 = ${neowPick.hp};`);
rust.push(`pub const NEOW_PICK_MAX_HP: i32 = ${neowPick.maxHp};`);
rust.push(`pub const NEOW_PICK_GOLD: i32 = ${neowPick.gold};`);
rust.push(`pub const NEOW_PICK_DECK: usize = ${neowPick.deck};`);
rust.push("/// 未知房判定:连续 12 次的结果");
rust.push(`pub const UNKNOWN_ROOMS: &[&str] = &[${unknownRooms.map((s) => `"${s}"`).join(", ")}];`);
rust.push("/// 事件抽取:连续 12 次抽到的事件 id");
rust.push(
  `pub const EVENT_PICKS: &[Option<&str>] = &[${eventPicks
    .map((s) => (s === null ? "None" : `Some("${s}")`))
    .join(", ")}];`,
);
rust.push("/// 判定与抽取交替 8 轮:判定结果,判成事件时带上抽到的 id");
rust.push(
  `pub const EVENT_ROLLS: &[&str] = &[${eventRolls.map((s) => `"${s}"`).join(", ")}];`,
);
rust.push("/// 商店货架的牌(稀有度, 价格, 是否无色)");
rust.push(
  `pub const SHOP_CARDS: &[(&str, &str, i32, bool)] = &[${shopDump.cards
    .map((c) => `("${c.id}", "${c.rarity}", ${c.price}, ${c.colorless})`)
    .join(", ")}];`,
);
rust.push("/// 商店货架的遗物(档次, 价格)");
rust.push(
  `pub const SHOP_RELICS: &[(&str, &str, i32)] = &[${shopDump.relics
    .map((r) => `("${r.id}", "${r.tier}", ${r.price})`)
    .join(", ")}];`,
);
rust.push("/// 商店货架的药水(价格)");
rust.push(`pub const SHOP_POTIONS: &[(&str, i32)] = &[${shopDump.potions.map((p) => `("${p.id}", ${p.price})`).join(", ")}];`);
rust.push("/// 商店的删牌服务价格");
rust.push(`pub const SHOP_REMOVAL: i32 = ${shopDump.removalCost};`);
rust.push("/// 商店生成后的三个流计数器(card, merchant, potion)");
rust.push(
  `pub const SHOP_STREAM_COUNTERS: (u32, u32, u32) = (${shopDump.streams.card}, ${shopDump.streams.merchant}, ${shopDump.streams.potion});`,
);
rust.push("/// 奖励路径连续 12 次的药水身份(没有掉落就是 None)");
rust.push(
  `pub const POTION_REWARD_SEQ: &[Option<&str>] = &[${potionRewardSeq.ids
    .map((s) => (s === null ? "None" : `Some("${s}")`))
    .join(", ")}];`,
);
rust.push("/// 掷完这一批之后 potionRng 的位置与药水保底值");
rust.push(`pub const POTION_REWARD_COUNTER: u32 = ${potionRewardSeq.counter};`);
rust.push(`pub const POTION_REWARD_PITY: i32 = ${potionRewardSeq.pity};`);
rust.push("/// 商店路径连续 12 次(四家店 × 三瓶)抽到的药水身份");
rust.push(
  `pub const POTION_SHOP_SEQ: &[&str] = &[${potionShopSeq.ids.map((s) => `"${s}"`).join(", ")}];`,
);
rust.push(`pub const POTION_SHOP_COUNTER: u32 = ${potionShopSeq.counter};`);
rust.push("/// 战斗/事件档的遗物掉落(档次, 身份):连掷 12 次");
rust.push(
  `pub const RELIC_COMBAT_SEQ: &[(&str, &str)] = &[${relicCombatSeq
    .map((r) => `("${r.tier}", "${r.id}")`)
    .join(", ")}];`,
);
rust.push("/// 精英档的遗物掉落(档次, 身份):连掷 12 次");
rust.push(
  `pub const RELIC_ELITE_SEQ: &[(&str, &str)] = &[${relicEliteSeq
    .map((r) => `("${r.tier}", "${r.id}")`)
    .join(", ")}];`,
);
rust.push("/// Boss 遗物三选一:四组,每组三件");
rust.push(
  `pub const RELIC_BOSS_CHOICES: &[[&str; 3]] = &[${relicBossSeq
    .map((set) => `["${set[0]}", "${set[1]}", "${set[2]}"]`)
    .join(", ")}];`,
);
rust.push("/// 宝箱(尺寸, 有没有金币, 档次, 身份):连开 6 个");
rust.push(
  `pub const RELIC_CHESTS: &[(&str, bool, &str, &str)] = &[${relicChestSeq
    .map((c) => `("${c.size}", ${c.goldPresent}, "${c.tier}", "${c.id}")`)
    .join(", ")}];`,
);
rust.push("");
await Bun.write(`src/core/golden_fixture.rs`, rust.join("\n"));
console.error(`wrote src/core/golden_fixture.rs (seed ${n})`);
