// 全量回归总入口:一条命令跑齐本项目的所有检查,最后打印汇总 PASS/FAIL。
//
//   bun tools/check_all.ts                      # 全跑(build + test + 9 张 e2e 表 + 全部审计 + smoke)
//   bun tools/check_all.ts --skip smoke         # 跳过某项(可重复;名字见下表 step 名)
//   bun tools/check_all.ts --smoke-seeds "7 42" # 自定义 smoke 种子(默认 7 42)
//   bun tools/check_all.ts --timing             # 每项耗时(汇总行 + 末端降序表)
//   bun tools/check_all.ts --serial             # 关掉并发(逐项串行;排查并发互扰用)
//   bun tools/check_all.ts --jobs N             # 并发度(默认 min(8, 核数);build/test 恒串行在最前)
//
// 各项:
//   build         cargo build(要求 0 告警)
//   test          cargo test(基线 822 通过,只增不减)
//   e2e:*         九张端到端对拍表(act1/act2/act3/acts/a20/a20a2/a20a3/a20a4/act4)
//                 —— 九张表合在同一个 e2e_diff 进程里跑到完(表内种子并发),汇总仍是九项
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

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { availableParallelism } from "node:os";
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
/** 并发度:--serial 退回逐项串行,--jobs N 显式指定,默认 min(8, 核数) */
const LIMIT = argv.includes("--serial")
  ? 1
  : Math.max(1, Number(flag("--jobs") ?? Math.min(8, availableParallelism())));
const START = performance.now();

/** 九张 e2e 表的基线"总差异 N 处"(见 src/core/replay.rs 的登记表) */
const E2E_BASELINE: Record<string, number> = { act1: 0, act2: 9, act3: 11, acts: 0, a20: 0, a20a2: 3, a20a3: 25, a20a4: 1, act4: 0 };

interface Step {
  name: string;
  ok: boolean;
  detail: string;
  /** FAIL 时打印出来定位用的片段 */
  fail?: string[];
  /** 该项自己花掉的墙钟毫秒(--timing 汇总用) */
  ms?: number;
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

/** 同 sh,但不阻塞事件循环:并发池里的各项用它,才能真的同时在跑 */
function shAsync(
  cmd: string[],
  cwd: string,
  extraEnv: Record<string, string> = {},
): Promise<{ status: number; out: string; stdout: string }> {
  return new Promise((resolve) => {
    const child = spawn(cmd[0]!, cmd.slice(1), {
      cwd,
      env: { ...process.env, NO_COLOR: "1", ...extraEnv },
    });
    // 两路各自收全再拼(与同步 sh 的 `${stdout}${stderr}` 一致):边收边拼会把
    // 交错到达的两路数据掐断在半行里,靠行首匹配解析的地方就会漏掉。
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (d: string) => { stdout += d; });
    child.stderr.on("data", (d: string) => { stderr += d; });
    child.on("error", (e) => resolve({ status: -1, out: `${stdout}${stderr}${e.message}`, stdout }));
    child.on("close", (code) => resolve({ status: code ?? -1, out: `${stdout}${stderr}`, stdout }));
  });
}

/** 一批任务最多 limit 个同时跑,结果按输入下标回填(与顺序跑的语义一致) */
async function mapPool<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const out = new Array<R>(items.length);
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i]!);
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, () => worker()));
  return out;
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
  // 两种结尾都认("ok." 与 "FAILED."),否则有失败时解析出来的数字全是 0
  const sums = [...out.matchAll(/test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed/g)];
  const passed = sums.reduce((a, m) => a + Number(m[1]), 0);
  const failed = sums.reduce((a, m) => a + Number(m[2]), 0);
  return {
    name: "test",
    ok: status === 0 && failed === 0 && passed >= 822,
    detail: `通过 ${passed},失败 ${failed}(基线 822)`,
    fail: interesting(out, ["FAILED", "panicked", "error["], 10),
  };
}

/** e2e_diff --tables 每张表打的一行汇总 */
interface E2eSummary {
  table: string;
  seeds: number;
  /** 全表"总差异 N 处"(与逐颗串行的口径一致) */
  diffs: number;
  /** 一边没跑起来的 seed;非空即该表失败 */
  failed: string[];
  /** 该表全部种子的耗时之和(表内并发,不是墙钟) */
  ms: number;
}

/** 九张 e2e 表放在同一个 e2e_diff 进程里跑到完,再逐表按基线判定(汇总仍是九项) */
async function stepE2eBatch(tables: string[]): Promise<Step[]> {
  const { status, out, stdout } = await shAsync(
    [
      "bun",
      join(HERE, "e2e_diff.ts"),
      "--tables",
      "--json",
      "--only-tables",
      tables.join(","),
      "--jobs",
      String(Math.max(1, LIMIT)),
    ],
    ROOT,
  );
  const summaries = new Map<string, E2eSummary>();
  // 汇总行在 stdout(明细在 stderr),只按 stdout 解析,免得被两路交错掐断
  for (const m of stdout.matchAll(/^E2E_SUMMARY (.*)$/gm)) {
    const v = JSON.parse(m[1]!) as E2eSummary;
    summaries.set(v.table, v);
  }
  return tables.map((table) => {
    const base = E2E_BASELINE[table] ?? 0;
    const s = summaries.get(table);
    if (!s) {
      return {
        name: `e2e:${table}`,
        ok: false,
        detail: "没拿到汇总(整批没跑起来?)",
        fail: interesting(out, ["error", "跑不通", "总差异", "E2E"], 12),
      };
    }
    const crashed = s.failed.length > 0;
    const only = out
      .split("\n")
      .filter((l) => l.startsWith(`[${table}] `))
      .join("\n");
    return {
      name: `e2e:${table}`,
      ok: status === 0 && !crashed && s.diffs <= base,
      detail: `总差异 ${s.diffs} 处(基线 ${base},种子 ${s.seeds})${crashed ? `,跑不通 ${s.failed.join(",")}` : ""}`,
      fail: interesting(only || out, ["差异", "← 第一处分叉", "跑不通"], 10),
      ms: s.ms,
    };
  });
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

/** smoke 一局要一分多钟:每个种子开一个独立进程并行跑,再把结果并成一项 */
async function stepSmoke(limit: number): Promise<Step> {
  // smoke.py 自己会按 pid+随机后缀取独立会话名与独立 XDG_DATA_HOME;这里再给每个种子
  // 一个带 pid+随机后缀的会话名,保证并发跑 check_all(或与手动 smoke 同时跑)也互不干扰。
  const base = process.env.SMOKE_SESSION ?? `spire_checkall_${process.pid}_${randomBytes(3).toString("hex")}`;
  const parts = await mapPool(SMOKE_SEEDS, Math.min(Math.max(1, limit), 4), (seed) =>
    shAsync(["python3", join(HERE, "smoke.py"), seed], ROOT, { SMOKE_SESSION: `${base}_${seed}` }),
  );
  const out = parts.map((p) => p.out).join("\n");
  // 逐进程只认 stdout(报告都在它上面),避免两路交错把行掐断
  const fails: string[] = [];
  let done = 0;
  let reported = 0;
  let allPass = true;
  for (const p of parts) {
    const m = p.stdout.match(/smoke: (PASS|FAIL)\s+\((\d+) seeds/);
    if (!m || m[1] !== "PASS") allPass = false;
    else reported++;
    done += Number(m?.[2] ?? 0);
    for (const f of p.stdout.matchAll(/^FAIL seed (\d+)/gm)) fails.push(f[1]!);
  }
  return {
    name: "smoke",
    ok: parts.every((p) => p.status === 0) && allPass && reported === SMOKE_SEEDS.length && fails.length === 0,
    detail: `种子 ${done}/${SMOKE_SEEDS.length} 个跑完(逐种子一进程),FAIL ${fails.length}${fails.length ? `: ${fails.join(",")}` : ""}`,
    fail: interesting(out, ["FAIL seed", "smoke:", "Traceback", "AssertionError"], 12),
  };
}

// ---- 跑全部 ----
// build -> test 串行在最前:后面所有项都要用 target/debug/spire;两项都只跑一次。
// 其余各项互不依赖,丢进并发池(mapPool),任一步失败都照常进汇总。
const steps: Step[] = [];
const runSync = (name: string, fn: () => Step) => {
  if (skipped.has(name)) return;
  process.stderr.write(`… 跑 ${name}\n`);
  const t0 = performance.now();
  const s = fn();
  s.name = name;
  s.ms = performance.now() - t0;
  steps.push(s);
};

runSync("build", stepBuild);
runSync("test", stepTest);

interface Job { name: string; fn: () => Promise<Step[]> }
const jobs: Job[] = [];
const addStep = (name: string, fn: () => Promise<Step>) => {
  if (skipped.has(name)) return;
  jobs.push({ name, fn: async () => [await fn()] });
};
// smoke 最长的那个种子要一分多钟,是全流程的临界路径:排在第一个,开跑就先占一格,
// 其余各项(每项几秒)填满剩下的格子在它这段时间里跑完。
addStep("smoke", () => stepSmoke(LIMIT));
const E2E_TABLES = Object.keys(E2E_BASELINE).filter((t) => !skipped.has(`e2e:${t}`));
if (E2E_TABLES.length > 0) {
  jobs.push({ name: `e2e(${E2E_TABLES.length}表)`, fn: () => stepE2eBatch(E2E_TABLES) });
}
addStep("sandbox", stepSandbox);
addStep("corpus", stepCorpus);
addStep("selftest", stepSelftest);
addStep("monsters", stepMonsters);
addStep("monself", stepMonsterSelftest);
addStep("events", stepEvents);
addStep("evimpl", stepEventsImpl);
addStep("evself", stepEventsImplSelftest);
addStep("axes", stepAxes);
addStep("relics", stepRelics);
addStep("potions", stepPotions);

// 每一批自己的墙钟(并发下会重叠;--timing 时单独列出来看临界路径在哪)
const jobWalls: { name: string; ms: number }[] = [];
const batches = await mapPool(jobs, LIMIT, async (job) => {
  process.stderr.write(`… 跑 ${job.name}\n`);
  const t0 = performance.now();
  const wall = () => performance.now() - t0;
  try {
    const got = await job.fn();
    jobWalls.push({ name: job.name, ms: wall() });
    // 批内自己报了耗时(e2e 逐表)就保留,其余按这一批的墙钟算
    for (const s of got) if (s.ms === undefined) s.ms = wall();
    return got;
  } catch (e) {
    jobWalls.push({ name: job.name, ms: wall() });
    return [{ name: job.name, ok: false, detail: `抛异常: ${String(e)}`, ms: wall() }];
  }
});
for (const b of batches) steps.push(...b);
const bad = steps.filter((s) => !s.ok);
const pad = (s: string) => s.padEnd(14, " ");
const TIMING = argv.includes("--timing");
console.log("");
console.log("==== tools/check_all.ts 汇总 ====");
for (const s of steps) {
  const ms = s.ms ?? 0;
  console.log(`${s.ok ? "PASS" : "FAIL"}  ${pad(s.name)} ${s.detail}${TIMING ? `  [${(ms / 1000).toFixed(2)}s]` : ""}`);
}
if (TIMING) {
  console.log("");
  console.log("==== 逐项耗时(降序)====");
  console.log("注:并发下各项 ms 会互相重叠(墙钟看末行);e2e:* 是该表种子耗时之和(表内并发)。");
  const total = steps.reduce((a, s) => a + (s.ms ?? 0), 0);
  const maxMs = Math.max(1, ...steps.map((s) => s.ms ?? 0));
  for (const s of [...steps].sort((a, b) => (b.ms ?? 0) - (a.ms ?? 0))) {
    const ms = s.ms ?? 0;
    console.log(`${pad(s.name)} ${String(ms.toFixed(0)).padStart(6)}ms ${"#".repeat(Math.max(1, Math.round((ms / maxMs) * 40)))}`);
  }
  console.log(`${pad("各项之和")} ${String(total.toFixed(0)).padStart(6)}ms`);
  console.log(`${pad("墙钟总计")} ${String((performance.now() - START).toFixed(0)).padStart(6)}ms(并发度 ${LIMIT})`);
  console.log("");
  console.log("==== 各批墙钟(并发池里同时起跑)====");
  for (const j of [...jobWalls].sort((a, b) => b.ms - a.ms)) {
    console.log(`${pad(j.name)} ${String(j.ms.toFixed(0)).padStart(6)}ms`);
  }
}
for (const s of bad) {
  console.log("");
  console.log(`---- ${s.name} 详情(FAIL)----`);
  for (const l of s.fail ?? []) console.log(`  ${l}`);
}
console.log("");
console.log(bad.length === 0 ? `全部 PASS(${steps.length} 项)` : `FAIL ${bad.length} 项 / 共 ${steps.length} 项:${bad.map((s) => s.name).join(", ")}`);
if (bad.length > 0) process.exit(1);
