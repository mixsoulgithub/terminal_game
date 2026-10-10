// 端到端对拍器:同一个 seed + 同一份路径脚本,把本作(spire --replay)与参考实现
// (refs/slay-the-cli,由 tools/replay_ref.ts 驱动)的两份 JSONL 逐行逐字段比对.
//
//   bun tools/e2e_diff.ts 54 39 12345            # 打印差异报告
//   bun tools/e2e_diff.ts 54 --write             # 顺手把参考侧的 JSONL 落成 fixture
//   bun tools/e2e_diff.ts --all                  # 用 fixture 里登记的全部 seed
//   bun tools/e2e_diff.ts --tables --jobs 6      # 九张表一次跑完(种子并发),给 check_all 用
//                                                #   --only-tables a20,act4 只跑其中几张
//   bun tools/e2e_diff.ts --tables --json        # 上者再加机器可读的 E2E_SUMMARY(每表一行)
//   bun tools/e2e_diff.ts 13 --script tools/golden/e2e/act3.script --seed-timeout 30
//                                                # 单颗种子单侧跑超 30s 就 SIGKILL 当"跑不通"(默认 60s,0=不限)
//
// 归一化:参考实现的 id 是大写,本作是小写;这里统一小写,并补一张别名表
// (史莱姆的 S/M/L 与铁甲战士的两张基础牌).归一化只改"名字",不改数值.

import { readFileSync, mkdirSync, writeFileSync, readdirSync } from "node:fs";
import { seedToString } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/core/rng.ts";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const FIXTURE_DIR = join(HERE, "golden", "e2e");
const DEFAULT_SCRIPT = join(FIXTURE_DIR, "act1.script");

// 路径脚本可以用 --script 指定;fixture 文件名跟着脚本走:
//   act1.script -> seed<N>.ref.jsonl(既有的一套,不许改名)
//   其它脚本     -> seed<N>.<脚本名>.ref.jsonl
let SCRIPT = DEFAULT_SCRIPT;
/** --raw-ref:参考侧不做补掷(直接比原样的参考实现) */
let RAW_REF = false;
/** --seed-timeout <秒>:单颗种子本作/参考任一侧跑超这个时间就 SIGKILL 当"跑不通"(默认 60) */
let SEED_TIMEOUT_MS = 60_000;
function fixturePath(seed: string): string {
  const stem = SCRIPT.split("/").pop()!.replace(/\.script$/, "");
  const name = stem === "act1" ? `seed${seed}.ref.jsonl` : `seed${seed}.${stem}.ref.jsonl`;
  return join(FIXTURE_DIR, name);
}

/** 名字归一:两边叫法不同但指的是同一个东西 */
function normId(value: string): string {
  const plus = value.endsWith("+") || /\+\d+$/.test(value);
  const [head, tail] = plus ? [value.replace(/\+\d*$/, ""), value.match(/\+\d*$/)![0]] : [value, ""];
  const lower = head.toLowerCase();
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
  return (alias[lower] ?? lower) + tail;
}

/** 递归归一:所有字符串都过一遍名字表 */
function normalize(v: unknown): unknown {
  if (typeof v === "string") return normId(v);
  if (Array.isArray(v)) return v.map(normalize);
  if (v && typeof v === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, val] of Object.entries(v as Record<string, unknown>)) out[k] = normalize(val);
    return out;
  }
  return v;
}

type Diff = { path: string; ours: unknown; ref: unknown };

/** 逐字段比两份 JSON,给出点分路径.数组按下标比. */
function diffValue(ours: unknown, ref: unknown, path: string, out: Diff[]): void {
  if (Array.isArray(ours) && Array.isArray(ref)) {
    if (ours.length !== ref.length) {
      out.push({ path: `${path}.length`, ours: ours.length, ref: ref.length });
    }
    for (let i = 0; i < Math.max(ours.length, ref.length); i++) {
      diffValue(ours[i], ref[i], `${path}[${i}]`, out);
    }
    return;
  }
  if (ours && ref && typeof ours === "object" && typeof ref === "object") {
    const keys = [...new Set([...Object.keys(ours), ...Object.keys(ref)])].sort();
    for (const k of keys) {
      diffValue(
        (ours as Record<string, unknown>)[k],
        (ref as Record<string, unknown>)[k],
        path ? `${path}.${k}` : k,
        out,
      );
    }
    return;
  }
  if (JSON.stringify(ours) !== JSON.stringify(ref)) out.push({ path, ours, ref });
}

/** 差异归类:先看是不是随机流错位,再看是不是内容缺失,否则算规则差异 */
function classify(d: Diff): string {
  if (d.ours === undefined || d.ref === undefined) return "内容缺失";
  if (/\.(deck|relics|potions)\[/.test(d.path)) return "随机流错位";
  if (/entries\[\d+\]\.id|monsters\[\d+\]\.id|\.id$/.test(d.path)) return "随机流错位";
  if (/\.length$/.test(d.path)) return "内容缺失";
  if (/resolved|kind$|\.k$/.test(d.path)) return "规则差异";
  return "规则差异";
}

function show(v: unknown): string {
  const s = JSON.stringify(v);
  return s === undefined ? "undefined" : s.length > 60 ? s.slice(0, 57) + "..." : s;
}

type Line = Record<string, any>;

/** 跑一个子进程并收全 stdout/stderr(逐颗串行与 --tables 并发都走这里).
 *  timeoutMs > 0 时到点 SIGKILL 并返回 status -2:单颗种子卡死/参考侧跑飞时,
 *  不让它把整批拖住(原因写进 stderr,由调用方当成"跑不通"登记). */
function spawnCollect(cmd: string[], timeoutMs = 0): Promise<{ status: number; stdout: string; stderr: string }> {
  const { promise, resolve } = Promise.withResolvers<{ status: number; stdout: string; stderr: string }>();
  const child = spawn(cmd[0]!, cmd.slice(1), { env: { ...process.env, NO_COLOR: "1" } });
  let stdout = "";
  let stderr = "";
  let timedOut = false;
  const timer = timeoutMs > 0
    ? setTimeout(() => {
        timedOut = true;
        child.kill("SIGKILL");
      }, timeoutMs)
    : undefined;
  const done = (status: number): void => {
    clearTimeout(timer);
    resolve({ status, stdout, stderr: timedOut ? `${stderr}\n(timeout ${timeoutMs}ms, killed)`.trim() : stderr });
  };
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (d: string) => { stdout += d; });
  child.stderr.on("data", (d: string) => { stderr += d; });
  child.on("error", (e) => { stderr += e.message; done(-1); });
  child.on("close", (code) => done(timedOut ? -2 : (code ?? -1)));
  return promise;
}

async function runOurs(seed: string, script: string): Promise<{ ok: boolean; lines: Line[]; err: string }> {
  const bin = join(ROOT, "target", "debug", "spire");
  const r = await spawnCollect([bin, "--replay", seed, "--script", script], SEED_TIMEOUT_MS);
  if (r.status !== 0) return { ok: false, lines: [], err: r.stderr.trim() };
  return { ok: true, lines: parseJsonl(r.stdout), err: "" };
}

function parseJsonl(text: string): Line[] {
  return text
    .split("\n")
    .filter((l) => l.trim().length > 0)
    .map((l) => JSON.parse(l));
}

async function runReference(seed: string, script: string): Promise<{ ok: boolean; lines: Line[]; err: string }> {
  // 默认让参考侧按原版补掷;--raw-ref 直接比原样的参考实现(不补偿)
  const args = [join(HERE, "replay_ref.ts"), seed, script];
  if (RAW_REF) args.push("--raw-ref");
  const r = await spawnCollect(["bun", ...args], SEED_TIMEOUT_MS);
  if (r.status !== 0) return { ok: false, lines: [], err: r.stderr.trim().split("\n").slice(0, 6).join("\n") };
  return { ok: true, lines: parseJsonl(r.stdout), err: "" };
}

/** 一整个 seed 的对拍:返回可打印的报告行与是否有差异 */
type Pin = { seed: string; lines: number; refLines: number; aligned: number; diffSteps: number[]; digest: bigint };

/** 本作与参考实现的这一次对拍结果;failed = 有一边没跑起来(此时 diffs 无意义) */
type SeedResult = { lines: string[]; diffs: number; aligned: number; pin: Pin | null; failed: boolean };

/** 打印行组装:tag 非空时给每行加上来源标签(--tables 并发跑时用) */
function tagLines(lines: string[], tag: string): string[] {
  return tag === "" ? lines : lines.map((l) => `[${tag}] ${l}`);
}

async function compareSeed(seed: string, scriptPath: string = SCRIPT, tag = ""): Promise<SeedResult> {
  const out: string[] = [];
  const script = readFileSync(scriptPath, "utf8");
  // 两边互不依赖:同一个 seed 的本作与参考实现同时跑,少一半等待
  const [ours, ref] = await Promise.all([runOurs(seed, scriptPath), runReference(seed, scriptPath)]);
  const policy = script
    .split("\n")
    .map((l) => l.split("#")[0]!.trim())
    .filter((l) => l.length > 0)
    .map((l) => l.split(/\s+/).join("="))
    .join(" ");
  if (!ours.ok) {
    out.push(`seed ${seed}: 本作跑不通: ${ours.err}`);
    return { lines: tagLines(out, tag), diffs: -1, aligned: 0, pin: null, failed: true };
  }
  if (!ref.ok) {
    out.push(`seed ${seed}: 参考实现跑不通: ${ref.err}`);
    return { lines: tagLines(out, tag), diffs: -1, aligned: 0, pin: null, failed: true };
  }
  const a = ours.lines.map((l) => normalize(l) as Line);
  const b = ref.lines.map((l) => normalize(l) as Line);
  out.push(`seed ${seed}: 本作 ${a.length} 步 / 参考 ${b.length} 步   脚本: ${policy}`);
  const byClass: Record<string, number> = {};
  const stepDiffs: { step: number; kind: string; diffs: Diff[] }[] = [];
  let total = 0;
  let aligned = 0;
  let prefix = 0;
  let firstDiffStep = -1;
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const la = a[i];
    const lb = b[i];
    if (!la || !lb) {
      out.push(`  步 ${i}: 只有一边有(${la ? "本作" : "参考"})`);
      total++;
      if (firstDiffStep < 0) firstDiffStep = i;
      stepDiffs.push({ step: i, kind: (la ?? lb)!.kind, diffs: [{ path: "(整行)", ref: lb ?? null, ours: la ?? null }] });
      continue;
    }
    const diffs: Diff[] = [];
    diffValue(la, lb, "", diffs);
    if (diffs.length === 0) {
      aligned++;
      if (firstDiffStep < 0) prefix++;
      continue;
    }
    if (firstDiffStep < 0) firstDiffStep = i;
    total += diffs.length;
    for (const d of diffs) {
      const c = classify(d);
      byClass[c] = (byClass[c] ?? 0) + 1;
    }
    stepDiffs.push({ step: i, kind: la.kind, diffs });
  }
  out.push(`  前 ${prefix} 步完全一致(逐字段相同)`);
  const kindsA = a.map((l) => l.kind).join(" ");
  const kindsB = b.map((l) => l.kind).join(" ");
  if (kindsA !== kindsB) {
    out.push(`  本作步流: ${kindsA}`);
    out.push(`  参考步流: ${kindsB}`);
  }
  // 第一处分叉整段列出,其余每步只列前 3 条,免得报告糊成一片
  const listed: string[] = [];
  for (const sd of stepDiffs.slice(0, 12)) {
    const first = sd.step === firstDiffStep;
    const shown = first ? sd.diffs : sd.diffs.slice(0, 3);
    listed.push(`  步 ${sd.step} (${sd.kind}): ${sd.diffs.length} 处差异${first ? " ← 第一处分叉" : ""}`);
    for (const d of shown) {
      listed.push(`      ${d.path}: 参考 ${show(d.ref)} / 本作 ${show(d.ours)} [${classify(d)}]`);
    }
    if (shown.length < sd.diffs.length) listed.push(`      ...还有 ${sd.diffs.length - shown.length} 处`);
  }
  out.push(...listed);
  if (stepDiffs.length > 12) out.push(`  ...后面还有 ${stepDiffs.length - 12} 步有差异(不再逐个列出)`);
  out.push(
    `  合计:完全一致 ${aligned}/${Math.max(a.length, b.length)} 步,差异 ${total} 处 ${JSON.stringify(byClass)}`,
  );
  // 差异行的内容指纹:哪一行变了都会让指纹变(与 Rust 测试里的 FNV 一致)
  const FNV_OFFSET = 0xcbf29ce484222325n;
  const FNV_PRIME = 0x100000001b3n;
  let digest = FNV_OFFSET;
  const bump = (text: string): void => {
    for (const b of new TextEncoder().encode(text)) {
      digest = ((digest ^ BigInt(b)) * FNV_PRIME) & 0xffffffffffffffffn;
    }
  };
  const diffSteps = stepDiffs.map((sd) => sd.step);
  for (const i of diffSteps) {
    bump(`${i}\t${JSON.stringify(a[i] ?? null)}\t${JSON.stringify(b[i] ?? null)}\n`);
  }
  return {
    lines: tagLines(out, tag),
    diffs: total,
    aligned,
    pin: { seed, lines: a.length, refLines: b.length, aligned: prefix, diffSteps, digest },
    failed: false,
  };
}

async function writeFixture(seed: string): Promise<void> {
  const ref = await runReference(seed, SCRIPT);
  if (!ref.ok) throw new Error(`参考实现跑不通: ${ref.err}`);
  mkdirSync(FIXTURE_DIR, { recursive: true });
  const path = fixturePath(seed);
  // 参考侧的名字归一化会把 init 行的 seed_str 也折成小写,本作那边是原样大写.
  // fixture 要跟本作的原始输出逐字节对齐(act1_walk_matches_reference 逐行比原始行),
  // 所以这里把这一格改回定种时的大小写.
  const seedStr = seedToString(BigInt(seed));
  const lines = ref.lines.map((l) => JSON.stringify("seed_str" in l ? { ...l, seed_str: seedStr } : l));
  writeFileSync(path, lines.join("\n") + "\n");
  console.log(`wrote ${path.slice(ROOT.length + 1)}`);
}

function fixtureSeeds(scriptPath: string = SCRIPT): string[] {
  const stem = scriptPath.split("/").pop()!.replace(/\.script$/, "");
  const re = stem === "act1" ? /^seed(\d+)\.ref\.jsonl$/ : new RegExp(`^seed(\\d+)\\.${stem}\\.ref\\.jsonl$`);
  try {
    return readdirSync(FIXTURE_DIR)
      .map((f) => f.match(re)?.[1])
      .filter((s): s is string => s !== undefined)
      .sort((x, y) => Number(x) - Number(y));
  } catch {
    return [];
  }
}

/** 每张表的汇总(--json 下打成一行 E2E_SUMMARY,给 check_all 解析) */
interface TableSummary {
  table: string;
  seeds: number;
  /** 全表"总差异 N 处"之和,与逐颗串行跑出来的口径一致 */
  diffs: number;
  /** 一边没跑起来的 seed(非空即该表失败) */
  failed: string[];
  /** 该表全部 seed 的耗时之和(并发下不是墙钟) */
  ms: number;
}

/** 九张表一次跑完:把各表登记的 seed 全丢进 jobs 条并发的池子,逐表汇总后打印 */
async function runTables(tables: string[], jobs: number, json: boolean): Promise<number> {
  interface Item { table: string; seed: string; script: string }
  const items: Item[] = [];
  for (const t of tables) {
    const script = join(FIXTURE_DIR, `${t}.script`);
    for (const s of fixtureSeeds(script)) items.push({ table: t, seed: s, script });
  }
  const acc = new Map<string, TableSummary>(
    tables.map((t) => [t, { table: t, seeds: 0, diffs: 0, failed: [], ms: 0 }]),
  );
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < items.length) {
      const it = items[next++]!;
      const t0 = performance.now();
      const r = await compareSeed(it.seed, it.script, it.table);
      const a = acc.get(it.table)!;
      a.seeds++;
      a.ms += performance.now() - t0;
      for (const l of r.lines) (json ? console.error : console.log)(l);
      if (r.failed) a.failed.push(it.seed);
      else a.diffs += Math.max(0, r.diffs);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(jobs, items.length)) }, () => worker()));
  let failures = 0;
  for (const t of tables) {
    const a = acc.get(t)!;
    failures += a.failed.length;
    console.log(`E2E_SUMMARY ${JSON.stringify(a)}`);
  }
  console.log(`\n共 ${tables.length} 张表 / ${items.length} 颗种子,跑不通 ${failures} 颗`);
  return failures;
}

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const write = argv.includes("--write");
  RAW_REF = argv.includes("--raw-ref");
  const pin = argv.includes("--pin");
  const all = argv.includes("--all");
  const tablesMode = argv.includes("--tables");
  const json = argv.includes("--json");
  const ji = argv.indexOf("--jobs");
  const jobs = ji !== -1 ? Number(argv[ji + 1]) : 6;
  // --script <file> 选路径脚本(默认 act1.script);fixture 的名字跟着脚本走
  const si = argv.indexOf("--script");
  if (si !== -1) {
    const p = argv[si + 1];
    if (!p) {
      console.error("usage: --script <path>");
      process.exit(2);
    }
    SCRIPT = p.startsWith("/") ? p : join(ROOT, p);
  }
  // --seed-timeout <秒>:单侧跑超就杀(0 = 不限时);默认 60 秒
  const sti = argv.indexOf("--seed-timeout");
  if (sti !== -1) {
    const secs = Number(argv[sti + 1]);
    SEED_TIMEOUT_MS = Number.isFinite(secs) && secs > 0 ? secs * 1000 : 0;
  }
  if (tablesMode) {
    // 九张表 = fixture 目录下的全部 *.script(表名就是脚本干名);--only-tables a,b 只跑其中几张
    const oi = argv.indexOf("--only-tables");
    const only = oi !== -1 ? (argv[oi + 1] ?? "").split(",").filter((s) => s !== "") : null;
    const tables = readdirSync(FIXTURE_DIR)
      .filter((f) => f.endsWith(".script"))
      .map((f) => f.replace(/\.script$/, ""))
      .filter((t) => only === null || only.includes(t))
      .sort();
    if (tables.length === 0) {
      console.error("--tables 里一张表都没选中");
      process.exit(2);
    }
    const failures = await runTables(tables, Number.isFinite(jobs) && jobs > 0 ? jobs : 6, json);
    if (failures > 0) process.exit(1);
  } else {
    const seeds = all
      ? fixtureSeeds()
      : argv.filter((a) => /^\d+$/.test(a));
    if (seeds.length === 0) {
      console.error("usage: bun tools/e2e_diff.ts <seed...> [--write] [--all] [--script <file>] [--raw-ref]");
      process.exit(2);
    }
    let diffTotal = 0;
    const pins: Pin[] = [];
    for (const seed of seeds) {
      if (write) await writeFixture(seed);
      const r = await compareSeed(seed);
      if (!pin) for (const l of r.lines) console.log(l);
      if (r.pin) pins.push(r.pin);
      diffTotal += Math.max(0, r.diffs);
    }
    if (pin) {
      // 给 src/core/replay.rs 的端到端测试用的常量表
      console.log("const CASES: &[Expected] = &[");
      for (const p of pins) {
        console.log(
          `    Expected { seed: ${p.seed}, lines: ${p.lines}, ref_lines: ${p.refLines}, aligned: ${p.aligned}, ` +
            `diff_steps: &[${p.diffSteps.join(", ")}], diff_digest: 0x${p.digest.toString(16)} },`,
        );
      }
      console.log("];");
    } else {
      console.log(`\n总差异 ${diffTotal} 处`);
    }
  }
}
