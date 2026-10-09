// 沙盒边界场景轴:把原版里容易踩坑的极端情形摆进沙盒,直接对着规则断言结果。
// 覆盖:极端叠加(力量/易伤/虚弱/脆弱/格挡/中毒 10+)、能量 0 与 X 费、
// 手牌上限、多敌全打与分裂、跨战斗遗物计数器。
//
//   bun tools/sandbox_axes.ts
//   bun tools/sandbox_axes.ts --seed 12345 --out tools/golden/axes_report.txt
//
// 每个场景一段 JSONL:第一行 `#名字`,后面每行一步。断言写在 EXPECT 里。

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const SPIRE = process.env.SPIRE_BIN ?? join(HERE, "..", "target", "debug", "spire");
if (!existsSync(SPIRE)) throw new Error(`找不到 ${SPIRE},先 cargo build`);

const argv = process.argv.slice(2);
const flag = (name: string) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const SEED = flag("--seed") ?? "12345";
const OUT = flag("--out");

interface Enemy {
  id: string;
  hp: number;
  max_hp: number;
  block?: number;
  move?: string | null;
  powers?: Record<string, number>;
}
interface St {
  turn: number;
  energy: number;
  max_energy: number;
  player: { hp: number; max_hp: number; block: number; powers: Record<string, number> };
  hand: string[];
  draw: string[];
  discard: string[];
  enemies: { id: string; hp: number; max_hp: number; dead: boolean; powers: Record<string, number> }[];
  counters?: Record<string, number>;
}
interface Row {
  step: number;
  op: string;
  c?: number;
  error?: string;
  st?: St;
}

const HI = 999; // 高血量靶子,免得被一击打死影响观察
const CULTIST: Enemy = { id: "cultist", hp: HI, max_hp: HI, move: "Incantation" };
const board = (extra: Record<string, unknown>) => ({
  player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9 },
  relics: [],
  potions: [null, null, null],
  draw: ["strike", "strike", "strike", "strike", "strike"],
  discard: [],
  exhaust: [],
  enemies: [CULTIST],
  hand: ["defend"],
  actions: [],
  ...extra,
});
const playFirst = [{ op: "play", hand: 0, target: 0 }];
const dmgTo = (rows: Row[], i: number) => {
  const init = rows[0]!.st!;
  const last = rows.filter((r) => r.st).pop()!.st!;
  return init.enemies[i]!.hp - last.enemies[i]!.hp;
};
const lastSt = (rows: Row[]) => rows.filter((r) => r.st).pop()!.st!;
const afterPlay = (rows: Row[]) => rows.find((r) => r.op === "play" && r.st)!.st!;

interface Check {
  name: string;
  scenario: Record<string, unknown>;
  expect: (rows: Row[]) => string | null;
}
const checks: Check[] = [];
const add = (name: string, scenario: Record<string, unknown>, expect: Check["expect"]) =>
  checks.push({ name, scenario, expect });

// ---- 极端叠加 ----
add("stacks/strength_10", board({ player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, powers: { strength: 10 } }, hand: ["strike"], actions: playFirst }), (r) =>
  dmgTo(r, 0) === 16 ? null : `strike+力量10 打了 ${dmgTo(r, 0)},期望 16`,
);
add("stacks/vulnerable_10", board({ enemies: [{ ...CULTIST, powers: { vulnerable: 10 } }], hand: ["strike"], actions: playFirst }), (r) =>
  dmgTo(r, 0) === 9 ? null : `strike 打易伤10 打了 ${dmgTo(r, 0)},期望 9`,
);
add("stacks/strength10_vuln10", board({ player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, powers: { strength: 10 } }, enemies: [{ ...CULTIST, powers: { vulnerable: 10 } }], hand: ["strike"], actions: playFirst }), (r) =>
  dmgTo(r, 0) === 24 ? null : `力量10+易伤10 打了 ${dmgTo(r, 0)},期望 24`,
);
add("stacks/frail_10", board({ player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, powers: { frail: 10 } }, hand: ["defend"], actions: playFirst }), (r) =>
  afterPlay(r).player.block === 3 ? null : `脆弱10 的 defend 给了 ${afterPlay(r).player.block} 格挡,期望 3`,
);
add("stacks/weak_10", board({ player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, powers: { weak: 10 } }, hand: ["strike"], actions: playFirst }), (r) =>
  dmgTo(r, 0) === 4 ? null : `虚弱10 的 strike 打了 ${dmgTo(r, 0)},期望 4`,
);
add("stacks/block_10", board({ player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, block: 10 }, hand: ["defend"], actions: playFirst }), (r) =>
  afterPlay(r).player.block === 15 ? null : `已有 10 格挡再 defend 是 ${afterPlay(r).player.block},期望 15`,
);
add("stacks/poison_10_tick", board({ enemies: [{ ...CULTIST, powers: { poison: 10 } }], hand: ["defend"], actions: [{ op: "end_turn" }] }), (r) => {
  const st = lastSt(r);
  if (st.enemies[0]!.hp !== HI - 10) return `中毒10 掉血不对: ${HI - st.enemies[0]!.hp},期望 10`;
  if (st.enemies[0]!.powers.poison !== 9) return `中毒层数没减: ${st.enemies[0]!.powers.poison},期望 9`;
  return null;
});

// ---- 能量 ----
add("energy/zero_cannot_play", board({ player: { hp: 40, max_hp: 80, energy: 0, max_energy: 3 }, hand: ["strike"], actions: playFirst }), (r) => {
  const p = r.find((x) => x.op === "play")!;
  return p.error === "not enough energy" ? null : `0 能量打 strike 没报错: ${JSON.stringify(p.error)}`;
});
add("energy/x_cost", board({ player: { hp: 40, max_hp: 80, energy: 3, max_energy: 9 }, hand: ["whirlwind"], enemies: [CULTIST, CULTIST], actions: playFirst }), (r) => {
  const st = afterPlay(r);
  if (st.energy !== 0) return `X 费没吃光能量: ${st.energy}`;
  const d0 = HI - st.enemies[0]!.hp;
  return d0 === 15 ? null : `3 能量的旋风斩打了 ${d0},期望 15(5×3)`;
});

// ---- 手牌上限 ----
add("hand/limit_10", board({ hand: ["pommel_strike", "defend", "defend", "defend", "defend", "defend", "defend", "defend", "defend", "defend"], actions: playFirst }), (r) => {
  const st = afterPlay(r);
  return st.hand.length <= 10 ? null : `手牌超上限: ${st.hand.length}`;
});

// ---- 多敌:全打与分裂 ----
add("enemies/cleave_all", board({ hand: ["cleave"], enemies: [CULTIST, CULTIST, CULTIST], actions: playFirst }), (r) => {
  const st = afterPlay(r);
  const bad = st.enemies.filter((e) => HI - e.hp !== 8);
  return bad.length === 0 ? null : `cleave 没对每只都打 8: ${st.enemies.map((e) => HI - e.hp).join(",")}`;
});
add(
  "enemies/split",
  board({
    encounter: "large_slime",
    hand: ["bludgeon", "strike", "defend", "defend", "defend"],
    draw: ["strike", "strike", "strike"],
    enemies: [{ id: "acid_slime_large", hp: 65, max_hp: 65 }],
    actions: [{ op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
  }),
  (r) => {
    const st = lastSt(r);
    const ids = st.enemies.map((e) => e.id).join(",");
    if (st.enemies.length !== 2 || ids !== "acid_slime_medium,acid_slime_medium")
      return `大史莱姆没裂成两只中史莱姆: ${st.enemies.length} 只 [${ids}]`;
    return null;
  },
);

// ---- 跨战斗遗物计数器 ----
add(
  "counters/incense_carries",
  {
    player: { hp: 40, max_hp: 80 },
    relics: ["incense_burner"],
    potions: [null, null, null],
    combats: [
      { enemies: [CULTIST], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "end_turn" }, { op: "end_turn" }] },
      { enemies: [CULTIST], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "noop" }] },
    ],
  },
  (r) => {
    const end0 = r.filter((x) => x.c === 0).filter((x) => x.st).pop()!.st!;
    const start1 = r.find((x) => x.c === 1 && x.op === "init")!.st!;
    if (end0.counters!.incense !== 3) return `第一场结束薰香计数是 ${end0.counters!.incense},期望 3`;
    if (start1.counters!.incense !== 4) return `第二场开局薰香计数没接上: ${start1.counters!.incense},期望 4`;
    return null;
  },
);
add(
  "counters/pen_nib_carries",
  {
    player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9 },
    relics: ["pen_nib"],
    potions: [null, null, null],
    combats: [
      { enemies: [CULTIST], hand: ["strike", "strike", "strike", "strike", "strike"], draw: [], discard: [], exhaust: [], actions: [{ op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }] },
      { enemies: [CULTIST], hand: ["strike"], draw: [], discard: [], exhaust: [], actions: [{ op: "play", hand: 0, target: 0 }] },
    ],
  },
  (r) => {
    const start1 = r.find((x) => x.c === 1 && x.op === "init")!.st!;
    if (start1.counters!.pen_nib !== 5) return `第二场开局笔尖计数是 ${start1.counters!.pen_nib},期望 5`;
    const last = r.filter((x) => x.c === 1 && x.op === "play").pop()!.st!;
    const d = HI - last.enemies[0]!.hp;
    return d === 6 ? null : `跨场后的第 6 张攻击打了 ${d},期望 6`;
  },
);
add(
  "counters/nunchaku_10_attacks",
  board({
    relics: ["nunchaku"],
    hand: ["strike", "strike", "strike", "strike", "strike", "strike", "strike", "strike", "strike", "defend"],
    actions: [{ op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
  }),
  (r) => {
    // 第 9 张攻击把能量花光,回合结束时回到 9;双节棍的第 10 张在第 2 回合
    const turn2 = r.find((x) => x.st && x.st.turn === 2 && x.op === "end_turn")!.st!;
    return turn2.energy === 9 ? null : `双节棍没在跨回合第 10 次攻击时补能量: ${turn2.energy}`;
  },
);

// ---- 遗物数值(原版语料给的具体数,参考实现也未必覆盖) ----
const relicBoard = (relic: string, hand: string[], enemies: Enemy[], actions: Record<string, unknown>[], player?: Record<string, unknown>) =>
  board({ relics: [relic], hand, enemies, actions, ...(player ? { player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, ...player } } : {}) });

add("relics/akabeko_first_attack", relicBoard("akabeko", ["strike"], [CULTIST], playFirst), (r) =>
  dmgTo(r, 0) === 14 ? null : `akabeko 首击打了 ${dmgTo(r, 0)},期望 14(6+8)`,
);
add("relics/strike_dummy", relicBoard("strike_dummy", ["strike"], [CULTIST], playFirst), (r) =>
  dmgTo(r, 0) === 9 ? null : `strike_dummy 的 strike 打了 ${dmgTo(r, 0)},期望 9(6+3)`,
);
add("relics/wrist_blade_zero_cost", relicBoard("wrist_blade", ["swift_strike"], [CULTIST], playFirst), (r) =>
  dmgTo(r, 0) === 11 ? null : `wrist_blade 的 0 费 swift_strike 打了 ${dmgTo(r, 0)},期望 11(7+4)`,
);
add("relics/the_boot_min_5", relicBoard("the_boot", ["strike"], [CULTIST], playFirst, { powers: { weak: 10 } }), (r) =>
  dmgTo(r, 0) === 5 ? null : `the_boot 没把 4 点抬到 5: ${dmgTo(r, 0)}`,
);
add(
  "relics/letter_opener_3_skills",
  relicBoard("letter_opener", ["defend", "defend", "defend"], [CULTIST], [{ op: "play", hand: 0 }, { op: "play", hand: 0 }, { op: "play", hand: 0 }]),
  (r) => (dmgTo(r, 0) === 5 ? null : `letter_opener 三张技能后打了 ${dmgTo(r, 0)},期望 5`),
);
add("relics/mercury_hourglass", relicBoard("mercury_hourglass", ["defend"], [CULTIST], [{ op: "end_turn" }]), (r) =>
  dmgTo(r, 0) === 3 ? null : `mercury_hourglass 回合开始打了 ${dmgTo(r, 0)},期望 3`,
);
add("relics/champion_belt", relicBoard("champion_belt", ["bash"], [CULTIST], playFirst), (r) => {
  const p = lastSt(r).enemies[0]!.powers;
  return p.vulnerable === 2 && p.weak === 1 ? null : `champion_belt 后敌人是 ${JSON.stringify(p)},期望 vuln2+weak1`;
});
add(
  "relics/runic_cube_draw_on_hp_loss",
  relicBoard("runic_cube", ["hemokinesis"], [CULTIST], playFirst),
  (r) => (lastSt(r).hand.length === 1 ? null : `runic_cube 掉血后手牌 ${lastSt(r).hand.length},期望 1(抽了 1 张)`),
);
add(
  "relics/chemical_x_plus_2",
  relicBoard("chemical_x", ["whirlwind"], [CULTIST], playFirst, { energy: 3, max_energy: 9 }),
  (r) => (dmgTo(r, 0) === 25 ? null : `chemical_x 的旋风斩打了 ${dmgTo(r, 0)},期望 25(5×(3+2))`),
);
add("relics/paper_phrog_vuln_75", board({ relics: ["paper_phrog"], hand: ["strike"], enemies: [{ ...CULTIST, powers: { vulnerable: 10 } }], actions: playFirst }), (r) =>
  dmgTo(r, 0) === 10 ? null : `paper_phrog 下易伤 strike 打了 ${dmgTo(r, 0)},期望 10(6×1.75)`,
);
add("relics/paper_krane_weak_40", relicBoard("paper_krane", ["strike"], [CULTIST], playFirst, { powers: { weak: 10 } }), (r) =>
  dmgTo(r, 0) === 3 ? null : `paper_krane 下虚弱 strike 打了 ${dmgTo(r, 0)},期望 3(6×0.6)`,
);
add(
  "relics/sacred_bark_doubles_potion",
  board({ relics: ["sacred_bark"], hand: ["defend"], potions: ["fire_potion", null, null], enemies: [CULTIST], actions: [{ op: "potion", slot: 0, target: 0 }] }),
  (r) => (dmgTo(r, 0) === 40 ? null : `圣树皮的火焰药水打了 ${dmgTo(r, 0)},期望 40(20×2)`),
);
const hurtBy = (r: Row[]) => r[0]!.st!.player.hp - lastSt(r).player.hp;
add(
  "relics/torii_5_to_1",
  board({ relics: ["torii"], hand: ["defend"], enemies: [{ ...CULTIST, move: "Dark Strike", powers: { strength: -1 } }], actions: [{ op: "end_turn" }] }),
  (r) => (hurtBy(r) === 1 ? null : `torii 下 5 点攻击打了 ${hurtBy(r)},期望 1`),
);
add(
  "relics/tungsten_rod_less_1",
  board({ relics: ["tungsten_rod"], hand: ["defend"], enemies: [{ ...CULTIST, move: "Dark Strike" }], actions: [{ op: "end_turn" }] }),
  (r) => (hurtBy(r) === 5 ? null : `tungsten_rod 下 6 点攻击打了 ${hurtBy(r)},期望 5`),
);

// ---- 选牌轴:升级真言/燃烧契约是"自选消耗",吃 choose 不吃随机流 ----
add(
  "choices/true_grit_up_chooses",
  board({ hand: ["true_grit+", "defend", "bash"], enemies: [CULTIST], actions: [{ op: "play", hand: 0, target: 0, choose: [1] }] }),
  (r) => {
    const st = lastSt(r);
    if (st.exhaust.join(",") !== "bash") return `true_grit+ 消耗的是 [${st.exhaust.join(",")}],期望 [bash]`;
    if (st.hand.join(",") !== "defend") return `没被选中的没留下,手牌 [${st.hand.join(",")}]`;
    return null;
  },
);

// ---- 跑 ----
const DIR = mkdtempSync(join(tmpdir(), "spire-axes-"));
const listPath = join(DIR, "list.txt");
writeFileSync(listPath, checks.map((c, i) => `${c.name}\t${join(DIR, `s${i}.json`)}`).join("\n"));
checks.forEach((c, i) => writeFileSync(join(DIR, `s${i}.json`), JSON.stringify(c.scenario)));

const run = spawnSync(SPIRE, ["--sandbox-batch", SEED, listPath], { encoding: "utf8", maxBuffer: 1 << 26 });
if (run.status !== 0) throw new Error(`沙盒跑不动(${run.status}): ${run.stderr}`);
const seg = new Map<string, Row[]>();
{
  let name: string | null = null;
  let rows: Row[] = [];
  for (const line of run.stdout.split("\n")) {
    if (line.startsWith("#")) {
      if (name !== null) seg.set(name, rows);
      name = line.slice(1);
      rows = [];
    } else if (line.trim() !== "") rows.push(JSON.parse(line) as Row);
  }
  if (name !== null) seg.set(name, rows);
}

const failed: string[] = [];
for (const c of checks) {
  const rows = seg.get(c.name);
  if (!rows) {
    failed.push(`${c.name}: 沙盒没有输出`);
    continue;
  }
  const msg = c.expect(rows);
  if (msg) failed.push(`${c.name}: ${msg}`);
}

const report: string[] = [];
report.push(`沙盒边界场景轴报告  seed=${SEED}`);
report.push(`命令: bun tools/sandbox_axes.ts`);
report.push(`场景 ${checks.length} 个,通过 ${checks.length - failed.length},失败 ${failed.length}`);
report.push("");
for (const c of checks) report.push(`  [${failed.some((f) => f.startsWith(c.name + ":")) ? "FAIL" : " ok "}] ${c.name}`);
if (failed.length) {
  report.push("");
  report.push("失败明细:");
  for (const f of failed) report.push(`  ${f}`);
}
const text = report.join("\n") + "\n";
process.stdout.write(text);
if (OUT) writeFileSync(OUT, text);
rmSync(DIR, { recursive: true, force: true });
