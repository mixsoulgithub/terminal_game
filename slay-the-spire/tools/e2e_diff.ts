// 端到端对拍器:同一个 seed + 同一份路径脚本,把本作(spire --replay)与参考实现
// (refs/slay-the-cli,由 tools/replay_ref.ts 驱动)的两份 JSONL 逐行逐字段比对.
//
//   bun tools/e2e_diff.ts 54 39 12345            # 打印差异报告
//   bun tools/e2e_diff.ts 54 --write             # 顺手把参考侧的 JSONL 落成 fixture
//   bun tools/e2e_diff.ts --all                  # 用 fixture 里登记的全部 seed
//
// 归一化:参考实现的 id 是大写,本作是小写;这里统一小写,并补一张别名表
// (史莱姆的 S/M/L 与铁甲战士的两张基础牌).归一化只改"名字",不改数值.

import { readFileSync, mkdirSync, writeFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const FIXTURE_DIR = join(HERE, "golden", "e2e");
const SCRIPT = join(FIXTURE_DIR, "act1.script");

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

function runOurs(seed: string, script: string): { ok: boolean; lines: Line[]; err: string } {
  const bin = join(ROOT, "target", "debug", "spire");
  const args = ["--replay", seed, "--script", script];
  const r = spawnSync(bin, args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) return { ok: false, lines: [], err: (r.stderr || "").trim() };
  return { ok: true, lines: parseJsonl(r.stdout), err: "" };
}

function parseJsonl(text: string): Line[] {
  return text
    .split("\n")
    .filter((l) => l.trim().length > 0)
    .map((l) => JSON.parse(l));
}

function runReference(seed: string, script: string): { ok: boolean; lines: Line[]; err: string } {
  const r = spawnSync("bun", [join(HERE, "replay_ref.ts"), seed, script], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (r.status !== 0) return { ok: false, lines: [], err: (r.stderr || "").trim().split("\n").slice(0, 6).join("\n") };
  return { ok: true, lines: parseJsonl(r.stdout), err: "" };
}

/** 一整个 seed 的对拍:返回可打印的报告行与是否有差异 */
type Pin = { seed: string; lines: number; refLines: number; aligned: number; diffSteps: number[]; digest: bigint };

function compareSeed(seed: string): { lines: string[]; diffs: number; aligned: number; pin: Pin | null } {
  const out: string[] = [];
  const script = readFileSync(SCRIPT, "utf8");
  const ours = runOurs(seed, SCRIPT);
  const ref = runReference(seed, SCRIPT);
  const policy = script
    .split("\n")
    .map((l) => l.split("#")[0]!.trim())
    .filter((l) => l.length > 0)
    .map((l) => l.split(/\s+/).join("="))
    .join(" ");
  if (!ours.ok) {
    out.push(`seed ${seed}: 本作跑不通: ${ours.err}`);
    return { lines: out, diffs: -1, aligned: 0, pin: null };
  }
  if (!ref.ok) {
    out.push(`seed ${seed}: 参考实现跑不通: ${ref.err}`);
    return { lines: out, diffs: -1, aligned: 0, pin: null };
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
    lines: out,
    diffs: total,
    aligned,
    pin: { seed, lines: a.length, refLines: b.length, aligned: prefix, diffSteps, digest },
  };
}

function writeFixture(seed: string): void {
  const script = readFileSync(SCRIPT, "utf8");
  const ref = runReference(seed, SCRIPT);
  if (!ref.ok) throw new Error(`参考实现跑不通: ${ref.err}`);
  mkdirSync(FIXTURE_DIR, { recursive: true });
  writeFileSync(join(FIXTURE_DIR, `seed${seed}.ref.jsonl`), ref.lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
  console.log(`wrote tools/golden/e2e/seed${seed}.ref.jsonl`);
}

function fixtureSeeds(): string[] {
  try {
    return readdirSync(FIXTURE_DIR)
      .filter((f) => /^seed\d+\.ref\.jsonl$/.test(f))
      .map((f) => f.replace(/^seed/, "").replace(/\.ref\.jsonl$/, ""))
      .sort((x, y) => Number(x) - Number(y));
  } catch {
    return [];
  }
}

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const write = argv.includes("--write");
  const pin = argv.includes("--pin");
  const all = argv.includes("--all");
  const seeds = all
    ? fixtureSeeds()
    : argv.filter((a) => /^\d+$/.test(a));
  if (seeds.length === 0) {
    console.error("usage: bun tools/e2e_diff.ts <seed...> [--write] [--all]");
    process.exit(2);
  }
  let diffTotal = 0;
  const pins: Pin[] = [];
  for (const seed of seeds) {
    if (write) writeFixture(seed);
    const r = compareSeed(seed);
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
