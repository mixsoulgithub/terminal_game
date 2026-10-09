// 逐卡/逐遗物/逐药水的差分尺:给全部已实现的牌、遗物、药水各生成一组标准场景,
// 我方(`spire --sandbox-batch`)与参考侧(`bun tools/sandbox_ref.ts --batch`)跑同一份
// scenario.json,逐行逐字段比对,给出可读报告。
//
//   cargo build                                    # 先有 target/debug/spire
//   bun tools/sandbox_diff.ts                      # 全量扫描 + 报告
//   bun tools/sandbox_diff.ts --only cards         # 只扫某类(cards|relics|potions)
//   bun tools/sandbox_diff.ts --limit 20           # 每类只取前 N 个内容,先给报告试跑
//   bun tools/sandbox_diff.ts --seed 12345         # 换种子
//   bun tools/sandbox_diff.ts --keep               # 保留生成的 scenario 目录
//   bun tools/sandbox_diff.ts --out tools/golden/sandbox_report.txt
//
// 每个场景一段 JSONL:第一行 `#名字`,后面每行一步 {"step","op",...}。
// 参考实现是"期望",我方是"实际";两边都报错的场景只比"有没有报错"(错误文本不比)。

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const SPIRE = process.env.SPIRE_BIN ?? join(ROOT, "target", "debug", "spire");

if (!existsSync(SPIRE)) {
  console.error(`找不到 ${SPIRE};先跑 cargo build`);
  process.exit(2);
}

// ---- 命令行 ----

const argv = process.argv.slice(2);
function flag(name: string): string | undefined {
  const i = argv.indexOf(name);
  return i === -1 ? undefined : argv[i + 1];
}
const ONLY = flag("--only");
const LIMIT = flag("--limit") ? Number.parseInt(flag("--limit")!, 10) : 0;
const SEED = flag("--seed") ?? "12345";
const KEEP = argv.includes("--keep");
const OUT = flag("--out");

// ---- 内容清单:直接问本作(它就是被测的那份数据) ----

function dump(what: string): string[][] {
  const r = spawnSync(SPIRE, ["--dump", what], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`spire --dump ${what} 失败: ${r.stderr}`);
  return r.stdout
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l) => l.trim().split(/\s+/));
}

type CardRow = { id: string; kind: string; cost: string };
const cards: CardRow[] = dump("cards").map((c) => ({ id: c[0]!, kind: c[1]!, cost: c[3]! }));
const relics: string[] = dump("relics").map((c) => c[0]!);
const potions: string[] = dump("potions").map((c) => c[0]!);

/** 刻意不实现的机制(别的职业专属 / 本作没有的系统):不参与"必须一致" */
const gated = new Set<string>();
for (const row of dump("gated")) {
  if (row[0] === "relic") gated.add(`relics/${row[1]!}`);
  if (row[0] === "potion") gated.add(`potions/${row[1]!}`);
}

/**
 * 本作把机制记在别处、不挂成一条状态,所以只差一条"能力条目"的算表示差异,
 * 不是玩法差异(逐条核对过:同一个场景里其余字段全部一致)。
 */
const REPRESENTATION_ONLY: Record<string, string[]> = {
  "cards/normality": ["normality"],
  "cards/pain": ["pain"],
  "cards/blood_for_blood": ["blood_for_blood"],
  "cards/battle_trance": ["no_draw"],
};

/**
 * 从池子里随机抽牌的牌:参考实现自己注明它把池子按 id 排序、与原作的牌库顺序
 * 不同(见 IMPL 里的 ENGINE-NOTE),所以"抽到哪张"这一层两边无法对拍 —— 属参考缺口.
 */
const RANDOM_CARD_IDS = [
  "infernal_blade",
  "discovery",
  "jack_of_all_trades",
  "metamorphosis",
  "chrysalis",
  "magnetism",
  "transmutation",
  "white_noise",
];
const RANDOM_CARD_RELICS = ["enchiridion", "warped_tongs", "toolbox", "nilrys_codex", "dead_branch"];

/**
 * (a) 我们错:已经定位到根因、但要动引擎核心才修得干净的,先记在这里。
 * key 是场景前缀(类别/内容 id)。
 */
const OURS_WRONG: Record<string, string> = {};

/** (b) 参考缺口:参考实现自己坏掉/没实现的地方(不是我们的锅) */
const REF_GAP: Record<string, string> = {
  "potions/smoke_bomb": "脱战时参考实现崩了(build 后 state.combat 已清空,还去读 c.monsters)",
};

// ---- 场景生成 ----

const DIR = mkdtempSync(join(tmpdir(), "spire-sandbox-"));
const list: { name: string; path: string }[] = [];

/** 冻住的一块标准局面:手牌给定,其余牌堆固定,一只 50 血的邪教徒 */
function board(opts: {
  hand: string[];
  enemies?: unknown[];
  player?: Record<string, unknown>;
  relics?: string[];
  potions?: (string | null)[];
  draw?: string[];
}) {
  return {
    player: opts.player ?? { hp: 80, max_hp: 80, energy: 9, max_energy: 9 },
    relics: opts.relics ?? [],
    potions: opts.potions ?? [null, null, null],
    hand: opts.hand,
    draw: opts.draw ?? ["strike", "strike", "strike", "strike", "strike"],
    discard: [],
    exhaust: [],
    enemies: opts.enemies ?? [{ id: "cultist", hp: 50, max_hp: 50, move: "Incantation" }],
  };
}

function emit(name: string, scenario: Record<string, unknown>): void {
  const path = join(DIR, `${name.replace(/\//g, "__")}.json`);
  writeFileSync(path, JSON.stringify(scenario));
  list.push({ name, path });
}

const TWO_CULTISTS = [
  { id: "cultist", hp: 50, max_hp: 50, move: "Incantation" },
  { id: "cultist", hp: 50, max_hp: 50, move: "Incantation" },
];

function cardScenarios(card: CardRow): void {
  const id = card.id;
  const others = ["defend", "defend", "defend", "defend"];
  const playBase = [
    { op: "play", hand: 0, target: 0 },
    { op: "end_turn" },
    { op: "play", hand: 0, target: 0 },
  ];
  const axes: [string, Record<string, unknown>][] = [
    ["base", { ...board({ hand: [id, ...others] }), actions: playBase }],
    [
      "up",
      {
        ...board({ hand: [`${id}+`, ...others] }),
        actions: [{ op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
      },
    ],
    [
      "statted",
      {
        ...board({
          hand: [id, ...others],
          player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9, powers: { strength: 3 } },
          enemies: [
            { id: "cultist", hp: 50, max_hp: 50, move: "Incantation", powers: { vulnerable: 3 } },
          ],
        }),
        actions: playBase,
      },
    ],
    [
      "multi",
      {
        ...board({ hand: [id, ...others], enemies: TWO_CULTISTS }),
        actions: [{ op: "play", hand: 0, target: 1 }, { op: "end_turn" }],
      },
    ],
    [
      "no_energy",
      {
        ...board({
          hand: [id, ...others],
          player: { hp: 80, max_hp: 80, energy: 0, max_energy: 9 },
        }),
        actions: [{ op: "play", hand: 0, target: 0 }],
      },
    ],
    [
      "hand10",
      {
        ...board({ hand: [id, ...others, "strike", "strike", "strike", "strike", "strike"] }),
        actions: [{ op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
      },
    ],
  ];
  for (const [axis, s] of axes) emit(`cards/${id}/${axis}`, s);
}

function relicScenarios(id: string): void {
  const hand = ["strike", "defend", "bash", "strike", "defend"];
  emit(`relics/${id}/start`, {
    ...board({ hand, relics: [id], player: { hp: 80, max_hp: 80 } }),
    actions: [{ op: "noop" }],
  });
  emit(`relics/${id}/play`, {
    ...board({ hand, relics: [id], player: { hp: 80, max_hp: 80 } }),
    actions: [
      { op: "play", hand: 0, target: 0 },
      { op: "play", hand: 0 },
      { op: "end_turn" },
      { op: "play", hand: 0, target: 0 },
      { op: "end_turn" },
      { op: "play", hand: 0, target: 0 },
    ],
  });
}

function potionScenarios(id: string): void {
  emit(`potions/${id}`, {
    ...board({ hand: ["strike", "defend", "bash", "strike", "defend"], potions: [id, null, null] }),
    actions: [{ op: "potion", slot: 0, target: 0 }, { op: "end_turn" }],
  });
}

const want = (kind: string) => ONLY === undefined || ONLY === kind;
const take = <T,>(xs: T[]): T[] => (LIMIT > 0 ? xs.slice(0, LIMIT) : xs);
if (want("cards")) for (const c of take(cards)) cardScenarios(c);
if (want("relics")) for (const r of take(relics)) relicScenarios(r);
if (want("potions")) for (const p of take(potions)) potionScenarios(p);

const listPath = join(DIR, "list.txt");
writeFileSync(listPath, list.map((s) => `${s.name}\t${s.path}`).join("\n"));

// ---- 跑两边 ----

function run(cmd: string, args: string[]): string {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 1 << 28 });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")} 失败(${r.status}): ${r.stderr}`);
  return r.stdout;
}

const oursText = run(SPIRE, ["--sandbox-batch", SEED, listPath]);
const refText = run("bun", [join(HERE, "sandbox_ref.ts"), "--batch", SEED, listPath]);

/** 把 `#名字` 分段的 JSONL 拆成 name -> 行数组 */
function segments(text: string): Map<string, unknown[]> {
  const out = new Map<string, unknown[]>();
  let name: string | null = null;
  let rows: unknown[] = [];
  for (const line of text.split("\n")) {
    if (line.startsWith("#")) {
      if (name !== null) out.set(name, rows);
      name = line.slice(1);
      rows = [];
    } else if (line.trim() !== "") {
      rows.push(JSON.parse(line));
    }
  }
  if (name !== null) out.set(name, rows);
  return out;
}

const ours = segments(oursText);
const ref = segments(refText);

// ---- 比对 ----

/** 递归逐字段比,差异写进 out(lines)。两边同时报错只比"有没有报错" */
function walk(a: unknown, b: unknown, path: string, out: string[]): void {
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) {
      out.push(`${path}: 长度 ref=${b.length} ours=${a.length}`);
      return;
    }
    for (let i = 0; i < a.length; i++) walk(a[i], b[i], `${path}[${i}]`, out);
    return;
  }
  if (a !== null && b !== null && typeof a === "object" && typeof b === "object") {
    const keys = new Set([...Object.keys(a as object), ...Object.keys(b as object)]);
    for (const k of [...keys].sort()) {
      const av = (a as Record<string, unknown>)[k];
      const bv = (b as Record<string, unknown>)[k];
      if (k === "error") {
        if (Boolean(av) !== Boolean(bv)) {
          out.push(`${path}.error: ref=${JSON.stringify(bv)} ours=${JSON.stringify(av)}`);
        }
        continue;
      }
      walk(av, bv, path === "" ? k : `${path}.${k}`, out);
    }
    return;
  }
  if (a !== b) {
    out.push(`${path}: ref=${JSON.stringify(b)} ours=${JSON.stringify(a)}`);
  }
}

/** 这一步之后战斗已经分出胜负(两边都全灭) */
function combatOver(row: unknown): boolean {
  const st = (row as { st?: { enemies?: { dead: boolean }[] } })?.st;
  return !!st?.enemies && st.enemies.length > 0 && st.enemies.every((e) => e.dead);
}

type Fail = { name: string; group: string; kind: string; diffs: string[] };
const fails: Fail[] = [];
let passed = 0;
let skipped = 0;

for (const item of list) {
  const group = item.name.split("/")[0]!;
  // 刻意没实现的遗物/药水:单列一类,不算不一致
  if (gated.has(item.name.split("/").slice(0, 2).join("/"))) {
    skipped++;
    continue;
  }
  const a = ours.get(item.name);
  const b = ref.get(item.name);
  if (!a || !b) {
    fails.push({ name: item.name, group, kind: "缺输出", diffs: [`缺少输出: ours=${!!a} ref=${!!b}`] });
    continue;
  }
  const diffs: string[] = [];
  if (a.length !== b.length) {
    diffs.push(`步数 ref=${b.length} ours=${a.length}`);
  }
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    // 战斗已经结束:之后的步数是"打完以后怎么摆",不作数
    if (i > 0 && combatOver(b[i - 1])) break;
    walk(a[i], b[i], `step${i}`, diffs);
    if (diffs.length > 12) break;
  }
  if (diffs.length === 0) {
    passed++;
    continue;
  }
  // 分类:表示差异 / 随机抽牌的参考缺口 / 真不一致
  const stem = item.name.split("/").slice(0, 2).join("/");
  const onlyIgnored = REPRESENTATION_ONLY[stem]?.every((key) =>
    diffs.every((d) => d.includes(`powers.${key}`)),
  );
  const id = item.name.split("/")[1] ?? "";
  const randomish =
    RANDOM_CARD_IDS.includes(id) || (item.name.startsWith("relics/") && RANDOM_CARD_RELICS.includes(id));
  const prefix = item.name.split("/").slice(0, 2).join("/");
  const kind = onlyIgnored
    ? "表示差异(玩法一致)"
    : randomish
      ? "(b) 参考缺口(随机池)"
      : OURS_WRONG[prefix]
        ? "(a) 我们错"
        : REF_GAP[prefix]
          ? "(b) 参考缺口"
          : "(a) 我们错(待判定)";
  const note = OURS_WRONG[prefix] ?? REF_GAP[prefix];
  fails.push({ name: item.name, group, kind, diffs: note ? [`根因: ${note}`, ...diffs] : diffs });
}

// ---- 报告 ----

const counts: Record<string, [number, number]> = {};
for (const item of list) {
  const g = item.name.split("/")[0]!;
  counts[g] ??= [0, 0];
  counts[g]![0]++;
}

const lines: string[] = [];
lines.push(`沙盒差分报告  seed=${SEED}`);
lines.push(`命令: cargo build && bun tools/sandbox_diff.ts${ONLY ? ` --only ${ONLY}` : ""}`);
lines.push(
  `内容: 牌 ${cards.length} / 遗物 ${relics.length} / 药水 ${potions.length}` +
    `(本次生成场景 ${list.length} 个)`,
);
const byKind: Record<string, number> = {};
for (const f of fails) byKind[f.kind] = (byKind[f.kind] ?? 0) + 1;

lines.push("");
lines.push(
  `通过 ${passed}/${list.length}, 不一致 ${fails.length}, 跳过(刻意未实现) ${skipped}`,
);
lines.push("");
lines.push("分类:");
for (const [g, v] of Object.entries(counts).sort()) {
  const bad = fails.filter((f) => f.group === g).length;
  const gate = [...gated].filter((x) => x.startsWith(g)).length;
  lines.push(
    `  ${g.padEnd(8)} 场景 ${v[0]}  通过 ${v[0] - bad - gate}  不一致 ${bad}  跳过(未实现) ${gate}`,
  );
}
lines.push("");
lines.push("不一致原因:");
for (const [k, v] of Object.entries(byKind).sort()) {
  lines.push(`  ${k.padEnd(18)} ${v}`);
}
const byGroupKind: Record<string, number> = {};
for (const f of fails) {
  const key = `${f.group}/${f.kind}`;
  byGroupKind[key] = (byGroupKind[key] ?? 0) + 1;
}
lines.push("");
lines.push("按类别 x 原因:");
for (const [k, v] of Object.entries(byGroupKind).sort()) lines.push(`  ${k.padEnd(28)} ${v}`);

lines.push("");
lines.push("不一致清单:");
for (const kind of [
  "(a) 我们错",
  "(a) 我们错(待判定)",
  "(b) 参考缺口",
  "(b) 参考缺口(随机池)",
  "表示差异(玩法一致)",
  "缺输出",
]) {
  const rows = fails.filter((f) => f.kind === kind);
  if (rows.length === 0) continue;
  lines.push(`  == ${kind} (${rows.length}) ==`);
  for (const f of rows) {
    lines.push(`  ${f.name}`);
    for (const d of f.diffs) lines.push(`      ${d}`);
  }
}

const report = lines.join("\n") + "\n";
process.stdout.write(report);
if (OUT) writeFileSync(OUT, report);
if (!KEEP) rmSync(DIR, { recursive: true, force: true });
else console.error(`场景目录保留在 ${DIR}`);
