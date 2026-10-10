// 全量回归总入口:一条命令跑齐本项目的所有检查,最后打印汇总 PASS/FAIL。
//
//   bun tools/check_all.ts                      # 全跑(build + test + 9 张 e2e 表 + 全部审计 + smoke)
//   bun tools/check_all.ts --skip smoke         # 跳过某项(可重复;名字见下表 step 名)
//   bun tools/check_all.ts --smoke-seeds "7 42" # 自定义 smoke 种子(默认 7 42)
//
// 各项:
//   build         cargo build(要求 0 告警)
//   test          cargo test(基线 819 通过,只增不减)
//   e2e:*         九张端到端对拍表(act1/act2/act3/acts/a20/a20a2/a20a3/a20a4/act4)
//   sandbox       tools/sandbox_diff.ts 逐卡/遗物/药水差分尺(要求 (a) 我们错 = 0)
//   corpus        tools/audit_corpus.ts 语料审计 + 两道守卫(未覆盖即报错 / 新增未登记即报错)
//   selftest      tools/audit_corpus.ts --selftest 历史盲区回归
//   monsters      tools/audit_monsters.ts 怪物对账(65 怪 × 招式/飞升档/血量档/AI 规则)
//   monself       tools/audit_monsters.ts --selftest 怪物对账的改坏/恢复回归
//   events        tools/audit_events.ts 事件效果沙盒审计(逐选项实测)
//   evimpl        tools/audit_events_impl.ts 事件面双向对账(选项/条件/效果/进入条件 ↔ 语料/反编译)
//   evself        tools/audit_events_impl.ts --selftest 事件对账的改坏/恢复回归
//   axes          tools/sandbox_axes.ts 边界轴
//   relics        tools/sandbox_relics.ts 遗物沙盒
//   potions       sandbox_diff --only potions 药水沙盒
//   smoke         python3 tools/smoke.py <少量种子>(要求全 PASS,不卡死)
//
// 基线数字是本轮记录下来的实际值:判据一律"不高于基线"(修好一处就变绿,退化就 FAIL)。

import { spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");

const argv = process.argv.slice(2);
const flag = (name: string) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const skipped = new Set<string>();
for (let i = 0; i < argv.length; i++) if (argv[i] === "--skip" && argv[i + 1]) skipped.add(argv[i + 1]!);
const SMOKE_SEEDS = (flag("--smoke-seeds") ?? "7 42").split(/\s+/).filter((s) => s !== "");

/** 九张 e2e 表的基线"总差异 N 处"(见 src/core/replay.rs 的登记表) */
const E2E_BASELINE: Record<string, number> = { act1: 0, act2: 9, act3: 11, acts: 0, a20: 0, a20a2: 3, a20a3: 25, a20a4: 1, act4: 0 };

interface Step {
  name: string;
  ok: boolean;
  detail: string;
  /** FAIL 时打印出来定位用的片段 */
  fail?: string[];
}

function sh(cmd: string[], cwd: string, extraEnv: Record<string, string> = {}): { status: number; out: string } {
  const r = spawnSync(cmd[0]!, cmd.slice(1), {
    cwd,
    encoding: "utf8",
    maxBuffer: 1 << 28,
    env: { ...process.env, NO_COLOR: "1", ...extraEnv },
  });
  return { status: r.status ?? -1, out: `${r.stdout ?? ""}${r.stderr ?? ""}` };
}

/** 从输出里挑 FAIL 时值得看的行(含关键词的几行) */
function interesting(out: string, words: string[], max = 12): string[] {
  const lines = out.split("\n").filter((l) => words.some((w) => l.includes(w)));
  return lines.slice(0, max);
}

function stepBuild(): Step {
  const { status, out } = sh(["cargo", "build"], ROOT);
  const warnings = out.split("\n").filter((l) => l.includes("warning:") && !l.includes("generated"));
  return {
    name: "build",
    ok: status === 0 && warnings.length === 0,
    detail: `exit ${status},告警 ${warnings.length}`,
    fail: warnings.slice(0, 8),
  };
}

function stepTest(): Step {
  const { status, out } = sh(["cargo", "test"], ROOT);
  const sums = [...out.matchAll(/test result: ok\. (\d+) passed; (\d+) failed/g)];
  const passed = sums.reduce((a, m) => a + Number(m[1]), 0);
  const failed = sums.reduce((a, m) => a + Number(m[2]), 0);
  return {
    name: "test",
    ok: status === 0 && failed === 0 && passed >= 819,
    detail: `通过 ${passed},失败 ${failed}(基线 819)`,
    fail: interesting(out, ["FAILED", "panicked", "error["], 10),
  };
}

function stepE2e(table: string): Step {
  const script = join(HERE, "golden", "e2e", `${table}.script`);
  const { status, out } = sh(["bun", join(HERE, "e2e_diff.ts"), "--all", "--script", script], ROOT);
  const m = out.match(/总差异 (\d+) 处/);
  const actual = m ? Number(m[1]) : -1;
  const base = E2E_BASELINE[table] ?? 0;
  return {
    name: `e2e:${table}`,
    ok: status === 0 && actual >= 0 && actual <= base,
    detail: `总差异 ${actual === -1 ? "?" : actual} 处(基线 ${base})`,
    fail: interesting(out, ["差异", "← 第一处分叉"], 10),
  };
}

function stepSandbox(): Step {
  const { status, out } = sh(["bun", join(HERE, "sandbox_diff.ts")], ROOT);
  const m = out.match(/通过 (\d+)\/(\d+), 不一致 (\d+)/);
  const reasonSection = out.slice(out.indexOf("不一致原因:"));
  const ourFault = [...reasonSection.matchAll(/\(a\)[^\n]*?\s+(\d+)/g)].reduce((a, x) => a + Number(x[1]), 0);
  return {
    name: "sandbox",
    ok: status === 0 && ourFault === 0,
    detail: `${m ? `${m[1]}/${m[2]}` : "?"} 通过,不一致 ${m ? m[3] : "?"},(a) 我们错 ${ourFault}`,
    fail: interesting(out, ["(a)", "不一致清单"], 12),
  };
}

function stepCorpus(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_corpus.ts")], ROOT);
  const m = out.match(/通过 (\d+)\/(\d+)/);
  const guard = out.match(/守卫: (PASS|FAIL)/);
  return {
    name: "corpus",
    ok: status === 0 && guard?.[1] === "PASS",
    detail: `${m ? `${m[1]}/${m[2]}` : "?"} 通过,守卫 ${guard?.[1] ?? "?"}`,
    fail: interesting(out, ["守卫", "守卫一", "守卫二", "口径未覆盖", "登记表", "  实测"], 16),
  };
}

function stepSelftest(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_corpus.ts"), "--selftest"], ROOT);
  const m = out.match(/抓到 (\d+),漏检 (\d+)/);
  return {
    name: "selftest",
    ok: status === 0 && m?.[2] === "0",
    detail: `历史盲区抓到 ${m?.[1] ?? "?"},漏检 ${m?.[2] ?? "?"}`,
    fail: interesting(out, ["[FAIL]"], 12),
  };
}

function stepMonsters(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_monsters.ts")], ROOT);
  const bad = out.match(/不一致 (\d+) 处/);
  const guard = out.match(/守卫: (PASS|FAIL)/);
  return {
    name: "monsters",
    ok: status === 0 && bad?.[1] === "0" && guard?.[1] === "PASS",
    detail: `不一致 ${bad?.[1] ?? "?"} 处,守卫 ${guard?.[1] ?? "?"}`,
    fail: interesting(out, ["不一致", "守卫", "缺少", "没进"], 16),
  };
}

function stepMonsterSelftest(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_monsters.ts"), "--selftest"], ROOT);
  const m = out.match(/抓到 (\d+),漏检 (\d+)/);
  return {
    name: "monself",
    ok: status === 0 && m?.[2] === "0",
    detail: `改坏/恢复抓到 ${m?.[1] ?? "?"},漏检 ${m?.[2] ?? "?"}`,
    fail: interesting(out, ["[FAIL]"], 12),
  };
}

function stepEvents(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_events.ts")], ROOT);
  const m = out.match(/ok=(\d+)\s+mismatch=(\d+)/);
  return {
    name: "events",
    ok: status === 0 && m?.[2] === "0",
    detail: `ok ${m?.[1] ?? "?"},mismatch ${m?.[2] ?? "?"}`,
    fail: interesting(out, ["mismatch"], 12),
  };
}

/** 事件面双向对账(语料/反编译 ↔ 实现声明) */
function stepEventsImpl(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_events_impl.ts")], ROOT);
  const d = out.match(/不一致 (\d+):/);
  const g = out.match(/覆盖守卫 FAIL\((\d+)\)/);
  const cov = out.match(/效果对上的选项: (\d+)\/(\d+)/);
  return {
    name: "evimpl",
    ok: status === 0 && (d?.[1] ?? "0") === "0" && !g,
    detail: `覆盖 ${cov ? `${cov[1]}/${cov[2]}` : "?"},不一致 ${d?.[1] ?? "0"},守卫 ${g ? `FAIL(${g[1]})` : "PASS"}`,
    fail: interesting(out, ["[DIFF]", "[GUARD]"], 12),
  };
}

/** 事件面双向对账的改坏/恢复回归 */
function stepEventsImplSelftest(): Step {
  const { status, out } = sh(["bun", join(HERE, "audit_events_impl.ts"), "--selftest"], ROOT);
  const m = out.match(/抓到 (\d+),漏检 (\d+)/);
  return {
    name: "evself",
    ok: status === 0 && m?.[2] === "0",
    detail: `改坏/恢复抓到 ${m?.[1] ?? "?"},漏检 ${m?.[2] ?? "?"}`,
    fail: interesting(out, ["[FAIL]"], 12),
  };
}

function stepAxes(): Step {
  const { status, out } = sh(["bun", join(HERE, "sandbox_axes.ts")], ROOT);
  const m = out.match(/场景 (\d+) 个,通过 (\d+),失败 (\d+)/);
  return {
    name: "axes",
    ok: status === 0 && m?.[3] === "0",
    detail: `${m ? `${m[2]}/${m[1]}` : "?"} 通过,失败 ${m?.[3] ?? "?"}`,
    fail: interesting(out, ["[fail]", "失败"], 12),
  };
}

function stepRelics(): Step {
  const { status, out } = sh(["bun", join(HERE, "sandbox_relics.ts")], ROOT);
  const m = out.match(/沙盒行通过 (\d+)\/(\d+)/);
  return {
    name: "relics",
    ok: status === 0 && !!m && m[1] === m[2],
    detail: m ? `${m[1]}/${m[2]} 通过` : "?",
    fail: interesting(out, ["失败", "不一致"], 12),
  };
}

function stepPotions(): Step {
  const { status, out } = sh(["bun", join(HERE, "sandbox_diff.ts"), "--only", "potions"], ROOT);
  const m = out.match(/通过 (\d+)\/(\d+), 不一致 (\d+)/);
  const reasonSection = out.slice(out.indexOf("不一致原因:"));
  const ourFault = [...reasonSection.matchAll(/\(a\)[^\n]*?\s+(\d+)/g)].reduce((a, x) => a + Number(x[1]), 0);
  return {
    name: "potions",
    ok: status === 0 && ourFault === 0,
    detail: `${m ? `${m[1]}/${m[2]}` : "?"} 通过,(a) 我们错 ${ourFault}`,
    fail: interesting(out, ["(a)", "不一致清单"], 12),
  };
}

function stepSmoke(): Step {
  // smoke.py 自己会按 pid+随机后缀取独立会话名与独立 XDG_DATA_HOME;这里再显式指定一个
  // 带 pid+随机后缀的会话名,保证并发跑 check_all(或与手动 smoke 同时跑)也互不干扰。
  const session = process.env.SMOKE_SESSION ?? `spire_checkall_${process.pid}_${randomBytes(3).toString("hex")}`;
  const { status, out } = sh(["python3", join(HERE, "smoke.py"), ...SMOKE_SEEDS], ROOT, { SMOKE_SESSION: session });
  const summary = out.match(/smoke: (PASS|FAIL)\s+\((\d+) seeds/);
  const fails = [...out.matchAll(/^FAIL seed (\d+)/gm)].map((m) => m[1]!);
  return {
    name: "smoke",
    ok: status === 0 && summary?.[1] === "PASS" && fails.length === 0,
    detail: `种子 ${summary?.[2] ?? "?"} 个,FAIL ${fails.length}${fails.length ? `: ${fails.join(",")}` : ""}`,
    fail: interesting(out, ["FAIL seed", "smoke:"], 12),
  };
}

// ---- 跑全部 ----
const steps: Step[] = [];
const run = (s: Step, name: string) => {
  if (skipped.has(name)) return;
  process.stderr.write(`… 跑 ${name}\n`);
  steps.push(s);
};

run(stepBuild(), "build");
run(stepTest(), "test");
for (const t of ["act1", "act2", "act3", "acts", "a20", "a20a2", "a20a3", "a20a4", "act4"]) run(stepE2e(t), `e2e:${t}`);
run(stepSandbox(), "sandbox");
run(stepCorpus(), "corpus");
run(stepSelftest(), "selftest");
run(stepMonsters(), "monsters");
run(stepMonsterSelftest(), "monself");
run(stepEvents(), "events");
run(stepEventsImpl(), "evimpl");
run(stepEventsImplSelftest(), "evself");
run(stepAxes(), "axes");
run(stepRelics(), "relics");
run(stepPotions(), "potions");
run(stepSmoke(), "smoke");

const bad = steps.filter((s) => !s.ok);
const pad = (s: string) => s.padEnd(14, " ");
console.log("");
console.log("==== tools/check_all.ts 汇总 ====");
for (const s of steps) console.log(`${s.ok ? "PASS" : "FAIL"}  ${pad(s.name)} ${s.detail}`);
for (const s of bad) {
  console.log("");
  console.log(`---- ${s.name} 详情(FAIL)----`);
  for (const l of s.fail ?? []) console.log(`  ${l}`);
}
console.log("");
console.log(bad.length === 0 ? `全部 PASS(${steps.length} 项)` : `FAIL ${bad.length} 项 / 共 ${steps.length} 项:${bad.map((s) => s.name).join(", ")}`);
if (bad.length > 0) process.exit(1);
