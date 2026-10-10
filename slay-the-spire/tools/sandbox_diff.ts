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
// 描述里带换行的牌在 dump 里会多出续行(没有前 4 列),按列形状滤掉
const cards: CardRow[] = dump("cards")
  .filter((c) => c[3] === "cost")
  .map((c) => ({ id: c[0]!, kind: c[1]!, cost: c[4]! }));
const relics: string[] = dump("relics").map((c) => c[0]!);
const potions: string[] = dump("potions").map((c) => c[0]!);

/** 刻意不实现的机制(别的职业专属 / 本作没有的系统):不参与"必须一致" */
const gated = new Set<string>();
for (const row of dump("gated")) {
  if (row[0] === "relic") gated.add(`relics/${row[1]!}`);
  if (row[0] === "potion") gated.add(`potions/${row[1]!}`);
}

/**
 * 本作把机制记在别处、不挂成一条"能力条目",所以只差一条"能力条目"的算表示差异,
 * 不是玩法差异(逐条核对过:同一个场景里其余字段全部一致)。
 *   - 反常/痛苦:反编译记成**手牌计数**(refs/sts_lightspeed/src/combat/CardManager.cpp:280-310
 *     的 handNormalityCount / handPainCount),闸门在 BattleContext.cpp:714 与 2656-2661;
 *     参考实现把它们建模成玩家 power,于是输出里多一条 powers.normality / powers.pain。
 *     本作照反编译用计数,故没有那条 power。断言:combat.rs curse_hand_counters_match_the_decompile。
 *   - 嗜血:反编译用 CardInstance::tookDamage(CardInstance.cpp:175-181)每掉一次血把三堆里的
 *     每张嗜血都降 1 费;参考实现自认 ENGINE-GAP(cards/ironclad/uncommon.ts:55-62:dynamicCost
 *     不被引擎读取,改用一张隐藏 power 在"抽到时"才开始计数,首张抽到前的掉血全丢),
 *     输出里多一条 powers.blood_for_blood。本作照反编译,故没有那条 power。
 */
const REPRESENTATION_ONLY: Record<string, string[]> = {
  "cards/normality": ["normality"],
  "cards/pain": ["pain"],
  "cards/blood_for_blood": ["blood_for_blood"],
};

/**
 * 从池子里随机抽牌的卡牌。这里的"抽到哪张"分三层,本轮逐条核对后的结论:
 *
 *  1) 牌面规则(张数/费用/消耗/时点/剔重)以反编译 refs/sts_lightspeed/ 为准,
 *     已在不依赖参考的自证里固化(见 src/core/combat.rs 的
 *     "colorless_random_pool_is_the_decompiled_35" 一段,共 8 条断言);
 *  2) 池子顺序:反编译里战斗内随机无色牌走 CombatColorlessCardPool —— 一张写死的 34 项
 *     数组(CardPools.h:189-196),内容是 Java HashMap 的打散序;它比 ColorlessRarityCardPool
 *     的 35 张(CardPools.h:133-138)少一张 BANDAGE_UP,CombatTypeCardPool(CardPools.h:150-156)
 *     同样缺 FEED / REAPER。**为什么无法复现**:按原序复刻时这些缺牌会让池成员与真实游戏
 *     对不上、分布也歪,而"缺的那几张该插在哪"反编译里没有(它是运行期从卡牌库拼的)。
 *     **已尝试**:照 CardPools.h 数组原序建池比对,发现成员数/内容不一致。**采用口径**:
 *     照参考实现——战斗内随机一律把池子按 id 排序(slay-the-cli colorless/effects.ts:34-56
 *     与 ironclad/uncommon.ts:300-306 的 ENGINE-NOTE)。万事通/磁力/嬗变/发现四处此前漏了
 *     排序(炼狱之刃早就排了),已补齐,这批场景现在通过;
 *  3) 抽/落位的**时点**:化茧/变形要先把 3(5)张一次抽完、再逐张落位
 *     (refs/sts_lightspeed/src/combat/Actions.cpp:546-561),参考实现是"抽一张落一张"交替
 *     (colorless/effects.ts:97-110),两条流的掷点次序不同 —— 这是参考实现与反编译的差别,
 *     本作按反编译;所以 chrysalis/metamorphosis 这两个 id 仍会出现在下面的清单里。
 *
 * 状态计数(seed=12345;本类内容 34 = 卡池卡 31 + 表示差异 3):
 *   - 卡池卡 31:此前 21 条(万事通 6 + 磁力 5 + 嬗变 5 + 发现 5)已自证并通过;
 *     剩 10 条(化茧 5 + 变形 5)是参考实现掷点次序不同(见上第 3 点),本作按反编译;
 *   - 表示差异 3:反常 / 痛苦 / 嗜血,规则已自证、只差一条能力条目(见 REPRESENTATION_ONLY)。
 *   已自证 34 条,无法自证 0 条。
 * 断言位置:src/core/combat.rs 的 "colorless_random_pool_is_the_decompiled_35" 一段(8 条测试)
 * 与 chrysalis_and_metamorphosis_pick_then_place_like_the_decompile /
 * curse_hand_counters_match_the_decompile;每条注释里都写了反编译出处行号。
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
/** 从池子里随机抽牌的遗物(抽到哪张可能因池序不同而不同);这三件的场景目前全通过 */
const RANDOM_CARD_RELICS = ["enchiridion", "toolbox", "dead_branch"];

/** 随机类卡牌失败场景的精确口径(前缀 + 原因 + 自证位置) */
const RANDOM_CARD_NOTES: Record<string, string> = {
  "cards/chrysalis":
    "参考是抽一张落一张(colorless/effects.ts:97-110),反编译是先抽完 3 张再逐张随机落位(Actions.cpp:546-561),掷点次序不同;本作按反编译。掷点账见 combat.rs chrysalis_and_metamorphosis_pick_then_place_like_the_decompile(2n 次 cardRandomRng、shuffleRng 不动、抽牌堆原序不变)",
  "cards/metamorphosis":
    "参考是抽一张落一张(colorless/effects.ts:97-110),反编译是先抽完 3(5)张再逐张随机落位(Actions.cpp:546-561),掷点次序不同;本作按反编译。掷点账见 combat.rs chrysalis_and_metamorphosis_pick_then_place_like_the_decompile",
};

/**
 * (a) 我们错:已经定位到根因、但要动引擎核心才修得干净的,先记在这里。
 * key 是场景前缀(类别/内容 id)。
 */
const OURS_WRONG: Record<string, string> = {};

/**
 * (b) 参考缺口:参考实现自己坏掉/没实现的地方(不是我们的锅)。
 * key 是场景前缀(类别/内容 id),值写清"参考在哪、反编译在哪、本作照哪边"。
 */
const REF_GAP: Record<string, string> = {
  "potions/smoke_bomb":
    "脱身是**跑局层**结果(本作 run.rs smoke_bomb_leaves_a_normal_fight / smoke_bomb_refuses_a_boss_fight,只对非 Boss 战生效),沙盒只建模单场战斗、看不到;参考在 onUse 里设 combatOver=escape(content/potions/index.ts:529-541),沙盒渲染时 combat 已被清空、去读 c.monsters 崩掉。反编译 BattleContext.cpp:2398-2400 对 SMOKE_BOMB 只写 // todo,给不出口径",
  "relics/warped_tongs":
    "参考把 WARPED_TONGS 挂在 atStartOfTurn(抽牌前,content/relics/event.ts:237-249),那一刻手里还没有牌、挑不出候选,于是永不升级;反编译放在抽牌后的 applyStartOfTurnPostDrawRelics(Player.cpp:669-671 + Actions.cpp:940-962),本作照反编译",
  "relics/nilrys_codex":
    "参考的 NILRY_CODEX hooks 为空、根本没实现(content/relics/event.ts:146-153);反编译有完整实现(BattleContext.cpp:2046-2047 + Actions.cpp:964-969 + CardManager.cpp:215-223),本作照反编译",
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
  const note = OURS_WRONG[prefix] ?? REF_GAP[prefix] ?? (randomish ? RANDOM_CARD_NOTES[prefix] : undefined);
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
