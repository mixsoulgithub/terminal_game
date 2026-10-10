// 怪物审计:把"意图/数值 ↔ 实现招式表"做成双向对账(此前唯一没有系统对账的大内容面)。
//
//   bun tools/audit_monsters.ts                 # 全量审计(守卫失败或不一致即 exit != 0)
//   bun tools/audit_monsters.ts --only <怪id>   # 只审一只
//   bun tools/audit_monsters.ts --selftest      # 改坏/恢复回归:逐条确认工具还抓得到
//   bun tools/audit_monsters.ts --impl <文件>   # 用保存下来的 --dump monsters 输出(默认现跑)
//   bun tools/audit_monsters.ts --src <目录>    # 换实现源码目录(默认 src/core/enemies,自检用)
//   bun tools/audit_monsters.ts --list-ai       # 只列"没解析也没登记"的 AI 规则(维护登记表用)
//
// 语料侧(refs/slay-the-cli/data/corpus/monsters-*.json)是权威:每个怪的
// 血量基础区间 + 飞升换档、每个招式的意图/伤害/段数/格挡/效果(含飞升覆盖),
// 以及开局预置(prebattle)。实现侧走 `spire --dump monsters`(见
// src/core/enemies.rs 的 dump_json):每只怪只在"分辨率变化"的飞升档打一个快照,
// 于是档位边界(>= N 还是 > N)错一位就会被抓到。
//
// 对账口径(六项,逐条登记):
//   1 招式数值:伤害/段数/格挡/增减益层数/塞牌/召唤/回能…  ↔ MoveDef 常量;
//   2 飞升档位:语料 asc 覆盖 ↔ 实现 move 的 changes(逐档比较,含档位本身);
//   3 血量档:语料 hp.base/hp.asc/ascLevel ↔ 实现 hp/hpChanges;
//   4 意图类型:语料 intent ↔ 实现 Intent 枚举;
//   5 触发条件:语料 ai.firstTurn / ai.historyRules ↔ 实现 pick 函数源码
//     (roll 阈值、概率、飞升阈值、firstTurn、只看前一招/前两招、硬接下一招);
//   6 覆盖守卫:语料里出现的数值/招式/预置/字段没进口径又没登记 -> 失败;
//     反过来实现里多出来的怪/招式/效果没登记也 -> 失败;
//     登记表里已经用不上的条目同样 -> 失败(防止登记表烂掉)。
//
// 数值确实对不上、但仓内已有裁决的(如 WRITHING_MASS 的 MALLEABLE 3 vs 4,
// 反编译 3 / wiki 4 / 本作按 wiki)走 PREBATTLE_CONFLICTS 登记,写明依据文件:行;
// 工具不解析的自由描述走 *_SKIP 登记。守卫把"静默漏检"变成工具拦住。
//
// 已接进 tools/check_all.ts 的两步:monsters(全量审计)/ monself(改坏-恢复回归)。

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const REPO = join(ROOT, "..");
const DEFAULT_CORPUS = join(REPO, "refs", "slay-the-cli", "data", "corpus");
const SPIRE = process.env.SPIRE_BIN ?? join(ROOT, "target", "debug", "spire");
const DEFAULT_SRC = join(ROOT, "src", "core", "enemies");

const argv = process.argv.slice(2);
const flag = (name: string): string | undefined => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const SELFTEST = argv.includes("--selftest");
/** 打印"没登记"的 AI 规则清单(生成登记表用),不跑别的检查 */
const LIST_AI = argv.includes("--list-ai");
const ONLY = flag("--only");
const CORPUS_DIR = flag("--corpus") ?? DEFAULT_CORPUS;
const IMPL_FILE = flag("--impl");
const SRC_DIR = flag("--src") ?? DEFAULT_SRC;

// ---- 语料类型 ----

interface CorpusEffect {
  power: string;
  amount?: number | null | [number, number];
  target?: string | null;
}
interface CorpusAsc {
  damage?: number | [number, number] | null;
  hits?: number | null;
  block?: number | null;
  effects?: CorpusEffect[];
}
interface CorpusMove {
  intent: string;
  damage?: number | [number, number] | null;
  hits?: number | null;
  block?: number | null;
  effects?: CorpusEffect[];
  asc?: Record<string, CorpusAsc>;
}
interface CorpusPrebattle {
  power: string;
  amount?: number | null | number[];
  target?: string | null;
  asc?: Record<string, number | number[]>;
}
interface CorpusMonster {
  id: string;
  name: string;
  category: string;
  acts: number[];
  hp: { base: [number, number]; asc: [number, number]; ascLevel: number };
  prebattle?: CorpusPrebattle[];
  moves: Record<string, CorpusMove>;
  ai: { roll?: string; firstTurn?: string; spec?: string; historyRules?: string[] };
  conflicts?: unknown[];
  sources?: string[];
}

// ---- 实现侧类型(--dump monsters) ----

interface ImplFx {
  v: string;
  amount?: number;
  times?: number;
  perTurn?: number;
  cap?: number;
  status?: string;
  n?: number;
  scope?: string;
  card?: string;
  spot?: string;
  fromTurn?: number | null;
  ids?: string[];
  slots?: number[];
  pool?: string[];
  count?: number;
  div?: number;
  add?: number;
  first?: number;
  idx?: number;
  atTurn?: number;
  num?: number;
  den?: number;
  turn?: number;
  dynamic?: boolean;
  lo?: number;
  hi?: number;
}
interface ImplSnap {
  asc: number;
  intent: string;
  damage: number | null;
  times: number | null;
  block: number | null;
  fx: ImplFx[];
}
interface ImplMove {
  name: string;
  base: ImplSnap;
  rolled: (ImplFx & { asc: number })[];
  changes: ImplSnap[];
}
interface ImplMonster {
  id: string;
  name: string;
  kind: string;
  hp: [number, number];
  hpChanges: { asc: number; hp: [number, number] }[];
  startBlock: number;
  special: string;
  moves: ImplMove[];
  innate: { asc: number; block: number; statuses: [string, number | null][] };
  innateChanges: { asc: number; block: number; statuses: [string, number | null][] }[];
  onDeath: ImplFx[];
}

// ---- 小工具 ----

/** 归一化招式名:小写去非字母数字(与 ascension.rs 的 norm 一致) */
const norm = (s: string): string => s.toLowerCase().replace(/[^a-z0-9]/g, "");
/** 归一化状态/效果名:大写去非字母数字 */
const une = (s: string): string => s.toUpperCase().replace(/[^A-Z0-9]/g, "");
const show = (x: unknown): string => JSON.stringify(x);

// ---- 语料 id / 招式名 ↔ 实现 id / 招式名 ----
// 语料里史莱姆按 _S/_M/_L 记,实现按 _small/_medium/_large;以下招式两边叫法不同。
const ID_ALIAS: Record<string, string> = {
  ACID_SLIME_S: "acid_slime_small",
  ACID_SLIME_M: "acid_slime_medium",
  ACID_SLIME_L: "acid_slime_large",
  SPIKE_SLIME_S: "spike_slime_small",
  SPIKE_SLIME_M: "spike_slime_medium",
  SPIKE_SLIME_L: "spike_slime_large",
};
const MOVE_ALIAS: Record<string, string> = {
  "NEMESIS/NEMESIS_ATTACK": "Tri Attack",
  "NEMESIS/NEMESIS_DEBUFF": "Tri Burn",
};

// ---- 效果规范化 ----
// 两边都折成四元组 (kind, detail, n, scope):kind 是效果类别,detail 是状态/卡/召唤物名,
// n 是层数/张数/伤害,scope 是作用范围。攻击与格挡的"招式字段"部分另算(见 compareMove)。
type Tok = [string, string | null, number | null, string | null];
const T = (k: string, d: string | null = null, n: number | null = null, s: string | null = null): Tok => [k, d, n, s];

/** 实现里不进口径的 EnemyFx(纯随机流对齐 / AI 机制 / 已由别的字段表达) */
const IMPL_FX_EXCL: Record<string, string> = {
  Attack: "攻击数值走意图的 damage/times 字段",
  AttackScaling: "回合递增攻击,基础数值走意图,递增算式见 MOVES_EXTRA",
  AttackGrowing: "段数随回合增长,基础数值走意图",
  AttackStabCount: "段数=连刺计数,基础数值走意图",
  AttackRolled: "开局抽签伤害,走 rolled 区间对账",
  ParityCoin: "只为对齐 aiRng 掷点(台词),语料 spec 文本已记",
  ParityRand: "只为对齐 aiRng 掷点(台词),语料 spec 文本已记",
  Charge: "小鬼巫师充能计数,语料 spec 文本已记",
  WakeUp: "拉格文醒来时机,语料 spec/预置已记",
  MarkImplantUsed: "扭动巨物寄生记账,语料 spec 文本已记",
  ForceNext: "硬接下一招,走 AI 规则(硬接)对账",
  RollDamage: "分裂伤害按玩家生命现算,语料 damage 记 null",
  LoseStatus: "双拳合击打散尖刺外壳,语料 spec 文本已记",
  UpgradePlayerBurns: "把玩家灼伤升级,语料 spec 文本已记",
  Split: "分裂走特殊机制(Special::Split)+语料 prebattle SPLIT",
  Escape: "逃离走意图 ESCAPE + special",
};

/** 语料效果 power -> 规范化 token;null 表示登记(不比对,原因见 FX_POWER_SKIP) */
function corpusEffTok(power: string, amount: number | null | undefined, target: string | null | undefined): Tok {
  const raw = power.trim();
  const low = raw.toLowerCase();
  const key = une(raw.replace(/\(.*/, ""));
  const t = target ?? "self";
  const n = amount === undefined ? null : (amount as number | null);
  const nB = n === null && BINARY.has(key) ? 1 : n;
  if (key === "BLOCK") return T("block", null, n, t);
  if (key === "BLOCKEQUALTODAMAGEDEALT") return T("blockfromdmg", null, null, "self");
  if (key === "HEAL") return T("heal", null, n, t);
  if (key === "HEALFORUNBLOCKEDDAMAGE") return T("healfromdmg", null, null, "self");
  if (key === "HEALTOHALFMAXHP") return T("healtohalf", null, null, "self");
  if (key === "REMOVEDEBUFFS" || key === "REMOVEOWNDEBUFFS") return T("clear", null, null, "self");
  if (key === "DRAWREDUCTION") return T("draw", null, n, "player");
  if (key === "STASISSTEALCARD") return T("stealcard", null, null, "player");
  if (key === "NONATTACKDAMAGE") return T("plaindamage", null, n, "player");
  if (key === "SUICIDE") return T("suicide", null, null, "self");
  if (low.startsWith("addcard:")) {
    const parts = raw.split(":");
    // 语料写 masterDeck,实现叫 deck
    const spot = (parts[2] ?? "").toLowerCase();
    return T("card", une(parts[1] ?? ""), n, spot === "masterdeck" ? "deck" : spot || null);
  }
  if (low.startsWith("card:")) return T("card", une(raw.split(":")[1] ?? ""), n, null);
  if (key === "ADDWOUNDTODISCARD") return T("card", "WOUND", n, "discard");
  if (key === "SUMMONBRONZEORBS") return T("summon", "BRONZEORB", n);
  if (key === "SUMMONTORCHHEADS") return T("summon", "TORCHHEAD", n);
  if (key === "SUMMONDAGGERS") return T("summon", "DAGGER", n);
  if (key === "SUMMONGREMLINS") return T("summonrand", "GREMLINS", n);
  if (key === "CLEARNEGATIVESTRENGTHTHENSTRENGTH") return T("resetstrength", null, n, "self");
  if (key.startsWith("ESCALATINGBUFF")) return T("escalate", null, null, "self");
  if (key === "STEALGOLD") return T("steal", null, n, "player");
  // 高塔盾卫的撞击:语料把 FOCUS(-1)|STRENGTH(-1) 写成一条,本作只落力量(集中是蓝职机制)
  if (une(raw) === "FOCUS1STRENGTH1") return T("status", "STRENGTH", -1, "player");
  if (CORP_STATUS.has(key)) return T("status", key, nB, t);
  return T("UNMAPPED", raw, n);
}
/** 只在语料里成对出现的二元状态:层数记 null,实现记 1 */
const BINARY = new Set(["CONFUSED", "HEX", "ENTANGLED"]);
const CORP_STATUS = new Set([
  "RITUAL", "STRENGTH", "DEXTERITY", "WEAK", "FRAIL", "VULNERABLE", "CONSTRICTED", "ENRAGE",
  "METALLICIZE", "FLIGHT", "SHARPHIDE", "PLATEDARMOR", "ANGER", "THORNS", "ARTIFACT", "MODESHIFT",
  "CONFUSED", "HEX", "ENTANGLED",
]);
/** 语料的 prebattle asc 覆盖值:可能是裸数字,也可能是 {amount: N} */
function preAmount(v: number | number[] | { amount?: number } | undefined): number | null {
  if (v === undefined || v === null) return null;
  if (typeof v === "number") return v;
  if (Array.isArray(v)) return null;
  return v.amount ?? null;
}

/** 语料 effect 里出现但只作"点名/占位"的 power(数值由别的招式承载) */
function implEffTok(fx: ImplFx): Tok | null {
  const v = fx.v;
  if (IMPL_FX_EXCL[v]) {
    USED_REG.add(`fx:${v}`);
    return null;
  }
  switch (v) {
    case "Block":
      return T("block", null, fx.amount ?? null, fx.scope ?? null);
    case "BlockFromDamage":
      return T("blockfromdmg", null, null, "self");
    case "GainStatus":
      return T("status", une(fx.status ?? ""), fx.n === 1 && BINARY.has(une(fx.status ?? "")) ? 1 : fx.n ?? null, fx.scope ?? null);
    case "PlayerStatus":
      return T("status", une(fx.status ?? ""), fx.n === 1 && BINARY.has(une(fx.status ?? "")) ? 1 : fx.n ?? null, "player");
    case "Heal":
      return T("heal", null, fx.n ?? null, fx.scope ?? null);
    case "HealFromDamage":
      return T("healfromdmg", null, null, "self");
    case "HealToHalf":
      return T("healtohalf", null, null, "self");
    case "ClearDebuffs":
      return T("clear", null, null, "self");
    case "ResetStrength":
      return T("resetstrength", null, fx.n ?? null, "self");
    case "Escalate":
      return T("escalate", null, null, "self");
    case "PlayerCard":
    case "PlayerCardUpgraded":
      return T("card", une(fx.card ?? ""), fx.n ?? null, (fx.spot ?? "").toLowerCase() || null);
    case "StealGold":
      return T("steal", null, fx.n ?? null, "player");
    case "StealCard":
      return T("stealcard", null, null, "player");
    case "Summon":
      return T("summon", une(fx.ids?.[0] ?? ""), fx.ids?.length ?? null);
    case "SummonRandom":
      return T("summonrand", "GREMLINS", fx.count ?? null);
    case "DrawReduction":
      return T("draw", null, fx.n ?? null, "player");
    case "PlainDamage":
      return T("plaindamage", null, fx.amount ?? null, "player");
    case "Suicide":
      return T("suicide", null, null, "self");
    case "RearmModeShift":
      return T("status", "MODESHIFT", null, "self");
    default:
      return T("UNMAPPED_IMPL", v);
  }
}

/** 作用范围相容:语料的 allies 可由实现的 team/random/leader 满足;team 也满足 self */
function scopeOk(kind: string, c: string | null, i: string | null): boolean {
  if (kind === "card") return c === null || c === i;
  if (c === "player") return i === "player";
  if (c === "self") return i === "self" || i === "team";
  if (c === "allies") return i === "allies" || i === "team" || i === "random" || i === "leader";
  return c === i;
}

/** 多重集匹配:返回 (语料里没被认领的, 实现里多出来的) */
function matchToks(corpus: Tok[], impl: Tok[]): { miss: Tok[]; extra: Tok[] } {
  const rest = impl.slice();
  const miss: Tok[] = [];
  for (const c of corpus) {
    const i = rest.findIndex((t) => t[0] === c[0] && t[1] === c[1] && t[2] === c[2] && scopeOk(c[0], c[3], t[3]));
    if (i < 0) miss.push(c);
    else rest.splice(i, 1);
  }
  return { miss, extra: rest.filter((t) => t[0] !== "EXCL") };
}

/** 招式的"格挡"片段:实现优先用 fx 里的 Block,没有才用意图上的 block */
function implBlocks(snap: ImplSnap): Tok[] {
  const bt = snap.fx.map(implEffTok).filter((t): t is Tok => !!t && t[0] === "block");
  if (bt.length === 0 && snap.block) return [T("block", null, snap.block, "self")];
  return bt;
}
/** 招式的"格挡"片段(语料侧) */
function corpusBlocks(snap: CorpusSnap): Tok[] {
  const bt = snap.fx.map((e) => corpusEffTok(e.power, e.amount as number, e.target)).filter((t) => t[0] === "block");
  if (bt.length === 0 && snap.block) return [T("block", null, snap.block, "self")];
  return bt;
}
function implOthers(snap: ImplSnap): Tok[] {
  return snap.fx.map(implEffTok).filter((t): t is Tok => !!t && t[0] !== "block");
}
function corpusOthers(snap: CorpusSnap): Tok[] {
  return snap.fx.map((e) => corpusEffTok(e.power, e.amount as number, e.target)).filter((t) => t[0] !== "block");
}

// ---- 语料快照(按飞升解析后) ----

interface CorpusSnap {
  intent: string;
  damage: number | null | [number, number];
  times: number | null;
  block: number | null;
  fx: CorpusEffect[];
}
function resolveMove(mv: CorpusMove, asc: number): CorpusSnap {
  const out: CorpusSnap = {
    intent: (mv.intent ?? "").toLowerCase(),
    damage: (mv.damage ?? null) as number | [number, number] | null,
    times: mv.hits ?? null,
    block: mv.block ?? null,
    fx: mv.effects ?? [],
  };
  for (const lvl of Object.keys(mv.asc ?? {}).sort((a, b) => Number(a) - Number(b))) {
    if (asc >= Number(lvl)) {
      const ov = mv.asc![lvl]!;
      if ("damage" in ov) out.damage = (ov.damage ?? null) as number | [number, number] | null;
      if ("hits" in ov) out.times = ov.hits ?? null;
      if ("block" in ov) out.block = ov.block ?? null;
      if ("effects" in ov) out.fx = ov.effects ?? [];
    }
  }
  return out;
}
/** 语料侧某招的"变化档"清单(内容真的变了才算,0 是基础档) */
function corpusLevels(mv: CorpusMove, keyOf: (s: CorpusSnap) => string): number[] {
  const levels = [0, ...Object.keys(mv.asc ?? {}).map(Number)].sort((a, b) => a - b);
  const out = [0];
  for (const l of levels.slice(1)) {
    if (keyOf(resolveMove(mv, l)) !== keyOf(resolveMove(mv, out[out.length - 1]!))) out.push(l);
  }
  return out;
}
function implAt(mv: ImplMove, asc: number): ImplSnap {
  let cur = mv.base;
  for (const c of mv.changes) if (c.asc <= asc) cur = c;
  return cur;
}
/** 实现侧某招的"变化档"清单(把内容没变的原始档过滤掉) */
function implLevels(mv: ImplMove, keyOf: (s: ImplSnap) => string): number[] {
  const raw = [0, ...mv.changes.map((c) => c.asc)].sort((a, b) => a - b);
  const out = [0];
  for (const l of raw.slice(1)) {
    if (keyOf(implAt(mv, l)) !== keyOf(implAt(mv, out[out.length - 1]!))) out.push(l);
  }
  return out;
}

// ---- 招式级登记 ----
// key = `<实现 id>/<归一化实现招式名>`
interface MoveReg {
  /** 语料 damage 与实现意图 damage 对不上:登记原因(如抽签伤害/公式伤害/段数动态) */
  damage?: string;
  /** 语料 hits 与实现意图 times 对不上:登记原因 */
  times?: string;
  /** 效果段整体不比对:登记原因 */
  fx?: string;
  /** 走 rolled 区间对账(语料 damage 是区间/实现开局抽签) */
  rolled?: string;
  /** 实现内部还有语料没记的变化档:登记原因(档号在 stableLevels) */
  stableLevels?: number[];
}
const MOVES: Record<string, MoveReg> = {
  // 抽签伤害:语料给区间,实现开局掷一次(区间随飞升换档,走 rolled 对比)
  "red_louse/bite": { rolled: "开局 monsterHpRng 抽签,区间随 A2 换档(Monster.cpp:117-123)" },
  "green_louse/bite": { rolled: "同红虱子:开局抽签 5..7 / A2 起 6..8" },
  "darkling/nip": { rolled: "开局抽签 7..11(asc2+: 9..13 再 +2),语料 damage 记 null" },
  "hexaghost/divider": { rolled: "分裂伤害 = 玩家生命/12+1 现算,语料 damage 记 null(MonsterSpecific.cpp:822-826)" },
  // 段数动态
  "book_of_stabbing/multistab": { times: "段数=连刺计数(AttackStabCount),语料 hits 记 null" },
  "the_maw/nom": { times: "段数随回合增长(AttackGrowing),语料 hits 记 null;NOM 后硬接 DROOL 走 AI 规则" },
  // 效果段:语料未记(写在 spec 文本里)
  "hexaghost/inferno": { fx: "语料 fx 为空;原作 Inferno 再塞 3 张 Burn+ 并升级已有灼伤,语料 spec 已记" },
  "awakened_one/rebirth": { fx: "语料 fx 为空;半血复活回满是机制(Special::Rebirth),语料 spec 已记" },
  // 双拳合击装回的形态切换额度:语料只记层数 null,实现按"当前门槛+10"给(A9 45/A19 50)
  "the_guardian/twinslam": { stableLevels: [9, 19] },
  // 实现内部变化档(语料没记数值)
  "giant_head/itistime": {
    stableLevels: [18],
  },
};
/** 开局预置里数值与实现差在"占位状态"上的登记 */
const PREBATTLE_CONFLICTS: Record<string, string> = {
  "awakened_one/STRENGTH":
    "语料 prebattle STRENGTH 0 是占位(0 层力量没有效果);实现基础档不挂,只在 A4 起挂 2 层(ascension.rs innate_amount)," +
    "结果等价,数值对账跳过",
  "writhing_mass/MALLEABLE":
    "语料(反编译 MonsterSpecific.cpp:210-215)记 3,wiki 记 4,本作按 wiki 取 4(src/core/enemies/act34.rs:591);" +
    "参考实现同样取 4(refs/slay-the-cli/src/content/monsters/act34/writhingMass.ts:3-5);语料 conflicts 该条 = unresolved",
};
/** 语料怪物对象的顶层字段:没进逐字段对账的必须登记 */
const CORPUS_FIELDS_SKIP: Record<string, string> = {
  acts: "出现层数记在语料的遭遇/地图侧,实现按 encounters 表组织,不逐怪存 acts(由遭遇生成器与 e2e 覆盖)",
  sources: "语料出处清单,不是实现数据",
  conflicts: "语料的冲突裁决记录(如 WRITHING_MASS 的 MALLEABLE),不是实现数据",
};
/** 语料 hp.asc 与 base 相同的"空档"登记:key = 实现 id。
 * 语料仍写了 ascLevel,但区间一样 -> 换档后数值不变,实现的 hpChanges 为空。 */
const HP_NOOP: Record<string, string> = {
  spheric_guardian: "语料 base/asc 都是 [20,20](固定血,连掷点都不消耗)",
  transient: "语料 base/asc 都是 [999,999](固定血)",
  the_maw: "语料 base/asc 都是 [300,300](固定血)",
  dagger: "语料 base/asc 都是 [20,25](随从,飞升不加血)",
};

/** 开局预置里实现落在别的机制上的登记:power -> 原因 */
const PREBATTLE_SKIP: Record<string, string> = {
  CURLUP: "虱子的卷曲在 spawn 钩子里掷(spawn: louse_spawn / curl_up_range),区间由 ascension.rs 单测钉住",
  THIEVERY: "偷取数额落在 MUG/LUNGE 的 STEAL_GOLD 效果上(已逐档对账),原版也不存独立状态",
  SURROUNDED: "夹击挂在玩家身上(遭遇预置),不在怪的 innate",
  SPORECLOUD: "孢子云走 on_death(死亡时给玩家易伤),base 档同时挂在 innate 上",
};
/** 首回合规则落在 if/else 回退分支上的登记:key = 实现 id */
const AI_FIRST_SKIP: Record<string, string> = {
  exploder: "首招落在 last_two_is(SLAM) 的 else 分支(反编译里首招也走这条),工具不解析回退分支",
  dagger: "首招落在 last_is(STAB) 的 else 分支(同上)",
  "acid_slime_small":
    "自由描述的首招规则,由 pick 的状态分支与 combat 单测覆盖(原文:asc>=17: always LICK; otherwise 50/50 TACKLE or LICK via aiRng.randomBoolean())",
  "chosen":
    "自由描述的首招规则,由 pick 的状态分支与 combat 单测覆盖(原文:A<17: POKE; A17+: HEX)",
  "shelled_parasite":
    "自由描述的首招规则,由 pick 的状态分支与 combat 单测覆盖(原文:A<17: 50/50 DOUBLE_STRIKE or SUCK (aiRng.randomBoolean(): true -> DOUBLE_STRIKE); A17+: always FELL. Never FELL on turn 1 below A17.)",
  "torch_head":
    "首招由遭遇预置/召唤动作写死(pick 不会被调用),工具不解析(原文:move preset to TACKLE by The Collector's spawn action (getMove is never invoked for Torch Heads))",
  "darkling":
    "首招按掷点分流(roll/randomBoolean),工具不解析多分支掷点写法(原文:roll < 50 -> HARDEN, else NIP (applies to all three Darklings))",
  "writhing_mass":
    "首招按掷点分流(roll/randomBoolean),工具不解析多分支掷点写法(原文:roll < 33 -> MULTI_STRIKE; roll < 66 -> FLAIL; else WITHER (~1/3 each))",
  "nemesis":
    "首招按掷点分流(roll/randomBoolean),工具不解析多分支掷点写法(原文:roll < 50 -> ATTACK (Tri Attack), else DEBUFF (Tri Burn); never Scythe turn 1)",
  "awakened_one":
    "阶段化首招,由 pick 的 phase 分支实现(原文:phase 1 always opens with SLASH; phase 2 always opens with DARK_ECHO)",
  "spire_shield":
    "首招按掷点分流(roll/randomBoolean),工具不解析多分支掷点写法(原文:aiRng.randomBoolean(): true -> FORTIFY, false -> BASH (50/50; the aiRng.random(99) roll itself is consumed but ignored))",
};
/** 这次审计真正用到的登记项(用来抓"登记表里已经用不上的条目") */
const USED_REG = new Set<string>();

/** 工具没解析、也没登记的 AI 规则(生成登记表用;--list-ai 打印) */
const AI_UNCOVERED: { id: string; rule: string }[] = [];

/** 历史规则(语料 ai.historyRules)工具的登记表:key = `<实现 id>|<规则原文>`,值是原因。
 * 工具机械解析的是"never/cannot be used N times in a row""always followed by""every turn"
 * "per roll: X N%""turn 1 always X""alternates A/B starting X""X then Y, always"
 * "X never after itself or after Y""X every Nth turn"这些形式;自由描述的循环/条件概率/
 * 阶段与半血分支/次数上限只登记,判据落在 pick 的状态分支、combat 单测与 e2e 对拍上。
 * 新增规则若没被解析也没登记 -> 守卫失败(不会静默漏检)。 */
const AI_RULE_SKIP: Record<string, string> = {
  "chosen|A17+: turn 1 HEX, then the same strict debuff/attack alternation starting with a debuff turn":
    "自由描述的触发条件,由 pick 的状态分支与 combat 单测覆盖",
  "gremlin_nob|asc<18: 33% SKULL_BASH / 67% RUSH; RUSH never 3x in a row (forced SKULL_BASH after two RUSHes); SKULL_BASH has no repeat restriction":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  "gremlin_wizard|asc<17: pattern CHARGING, CHARGING, BLAST, then repeating [CHARGING x3, BLAST]":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  "gremlin_wizard|asc>=17: CHARGING, CHARGING, BLAST, then BLAST every turn":
    "自由描述的触发条件,由 pick 的状态分支与 combat 单测覆盖",
  "lagavulin|asleep: SLEEP turns 1-3 (or until damaged), then attacks":
    "自由描述的触发条件,由 pick 的状态分支与 combat 单测覆盖",
  "the_champ|phase 1: TAUNT forced on turns 4, 8, 12, ... (roll made when (turnNumber+1)%4==0)":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // acid_slime_large
  "acid_slime_large|asc>=17 (as coded): TACKLE never 3x in a row, LICK never twice in a row, CORROSIVE_SPIT unconstrained (see conflicts)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  // acid_slime_small
  "acid_slime_small|strict LICK/TACKLE alternation after the first move":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // awakened_one
  "awakened_one|while halfDead the rolled move is always REBIRTH":
    "条件分支(阶段/半血/回合计数)由 pick 的状态分支实现,规则为自由描述",
  // bear
  "bear|fixed deterministic script: BEAR_HUG, LUNGE, MAUL, LUNGE, MAUL, ...":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // blue_slaver
  "blue_slaver|asc>=17 per wiki/real game: RAKE never twice in a row (NOT reproduced by lightspeed's expression)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  // bronze_automaton
  "bronze_automaton|fully deterministic script (one aiRng.random(99) burned per turn)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "bronze_automaton|HYPER BEAM lands every 6th turn below A19; from A19 the post-first-beam loop is 4 turns (BOOST, FLAIL, BOOST, HYPER_BEAM)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // bronze_orb
  "bronze_orb|STASIS at most once per orb; until used it has a 75% chance per roll":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  "bronze_orb|after stasis is used: BEAM 70%, SUPPORT_BEAM 30% per roll":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // byrd
  "byrd|airborne probabilities per roll: PECK 50%, SWOOP 20%, CAW 30%":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  "byrd|while grounded the move sequence is fixed: STUNNED -> HEADBUTT -> FLY -> (normal pattern)":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // centurion
  "centurion|per roll: 65% SLASH-side, 35% support-side; the support move is DEFEND while the Mystic lives, FURY once it is dead":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // chosen
  "chosen|A<17: turn 1 POKE, turn 2 HEX, then strict alternation: debuff turn (DEBILITATE 50% / DRAIN 50%) then attack turn (ZAP 40% / POKE 60%), repeating":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // corrupt_heart
  "corrupt_heart|on non-buff turns the two attacks alternate; after DEBILITATE or BUFF the next attack is chosen 50/50":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // cultist
  "cultist|INCANTATION exactly once, on turn 1":
    "一次性/次数上限记在 EnemyState(或 pick 的分支计数)里,工具不解析自由描述",
  // darkling
  "darkling|CHOMP is never used by the middle Darkling (spawn idx 1)":
    "条件化的重复限制,工具未解析条件部分(基础档的 never/cannot 已单独对账)",
  "darkling|while halfDead the rolled move is always REINCARNATE":
    "条件分支(阶段/半血/回合计数)由 pick 的状态分支实现,规则为自由描述",
  // exploder
  "exploder|explodes on its 3rd turn, always (Attack, Attack, Explode)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // giant_head
  "giant_head|from turn 5 on, always IT_IS_TIME":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // green_louse
  "green_louse|SPIT_WEB never three times in a row (asc<17)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // gremlin_leader
  "gremlin_leader|move table depends on living gremlin count: 0 gremlins: RALLY 75%/STAB 25%; 1 gremlin: RALLY 50%/ENCOURAGE 30%/STAB 20% (with reroll redistribution); 2-3 gremlins: ENCOURAGE 66%/STAB 34%":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // gremlin_nob
  "gremlin_nob|BELLOW exactly once, on turn 1":
    "一次性/次数上限记在 EnemyState(或 pick 的分支计数)里,工具不解析自由描述",
  "gremlin_nob|asc>=18 per wiki/real game: SKULL_BASH turn 2, then RUSH, RUSH, SKULL_BASH, RUSH, RUSH, ... (lightspeed as coded: RUSH every turn -- see conflicts)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  // gremlin_wizard
  // hexaghost
  "hexaghost|turn 1 ACTIVATE, turn 2 DIVIDER, then the fixed repeating loop [SEAR, TACKLE, SEAR, INFLAME, TACKLE, SEAR, INFERNO]":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "hexaghost|no randomness in move selection at any point":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // lagavulin
  "lagavulin|SIPHON_SOUL never twice in a row (always followed by ATTACK)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // looter
  "looter|MUG on turns 1 and 2":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "looter|turn 3: 50% LUNGE / 50% SMOKE_BOMB":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // mugger
  "mugger|fixed script: MUG, MUG, then (50/50) either SMOKE_BOMB -> ESCAPE, or LUNGE -> SMOKE_BOMB -> ESCAPE":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  "mugger|escapes with stolen gold on the ESCAPE turn; killing it first recovers the gold":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // mystic
  "mystic|HEAL has priority over everything whenever any of the pair is missing >= 16 HP (A17: >= 21); no history restriction on HEAL in lightspeed (see conflicts)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  "mystic|per roll when not healing: ATTACK_DEBUFF 60%, BUFF 40%":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // nemesis
  "nemesis|DEBUFF (Tri Burn) cannot be used twice in a row":
    "条件化的重复限制,工具未解析条件部分(基础档的 never/cannot 已单独对账)",
  "nemesis|ATTACK (Tri Attack) cannot be used three times in a row":
    "条件化的重复限制,工具未解析条件部分(基础档的 never/cannot 已单独对账)",
  "nemesis|intangible every other turn (gains Intangible 2 after acting on any turn it is not intangible)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // pointy
  "pointy|uses POINTY_ATTACK (5x2 / 6x2) every turn; no variation":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // red_louse
  "red_louse|GROW never three times in a row (asc<17)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // red_slaver
  "red_slaver|always STAB on turn 1":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "red_slaver|asc>=17 per wiki/real game: SCRAPE never twice in a row after Entangle (NOT reproduced by lightspeed's expression)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  // reptomancer
  "reptomancer|SUMMON is skipped (Snake Strike instead) when the spawn cap is reached":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // romeo
  "romeo|fixed deterministic script: MOCK, AGONIZING_SLASH, CROSS_SLASH, AGONIZING_SLASH, CROSS_SLASH, ... (see conflicts for the wiki A17 variant)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
  // sentry
  "sentry|strict BOLT/BEAM alternation, phase fixed by spawn position":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // slime_boss
  "slime_boss|fixed loop GOOP_SPRAY -> PREPARING -> SLAM":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  "slime_boss|SPLIT interrupts at <= 50% HP (exactly once; boss leaves combat)":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // snake_plant
  "snake_plant|A<17: ENFEEBLING_SPORES never twice in a row":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // snecko
  "snecko|per roll after turn 1: BITE 60%, TAIL_WHIP 40%":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // spheric_guardian
  "spheric_guardian|fixed deterministic script; one aiRng.random(99) is still burned every turn (initial rollMove, then noOpRollMove each turn)":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // spiker
  "spiker|SPIKE can be used at most 6 times per combat; afterwards CUT is used every turn":
    "一次性/次数上限记在 EnemyState(或 pick 的分支计数)里,工具不解析自由描述",
  // spire_growth
  "spire_growth|CONSTRICT never used while the player is already Constricted":
    "条件化的重复限制,工具未解析条件部分(基础档的 never/cannot 已单独对账)",
  // spire_shield
  "spire_shield|between Smashes: BASH and FORTIFY once each, in an order chosen 50/50":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // spire_spear
  "spire_spear|between Skewers: BURN_STRIKE and PIERCER once each, in an order chosen 50/50":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // the_champ
  "the_champ|phase 2 entered on the first roll made while curHp < maxHp/2: ANGER once (removes debuffs), then EXECUTE unless it was used in either of the last two moves (net: EXECUTE every 3rd turn)":
    "条件分支(阶段/半血/回合计数)由 pick 的状态分支实现,规则为自由描述",
  "the_champ|DEFENSIVE_STANCE: at most 2 uses per combat, never twice in a row; roll <= 15 (A19: <= 30)":
    "一次性/次数上限记在 EnemyState(或 pick 的分支计数)里,工具不解析自由描述",
  "the_champ|approximate per-roll odds below A19: STANCE 16%, GLOAT 15%, FACE_SLAP 25%, HEAVY_SLASH 44% (thresholds are inclusive <=)":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // the_collector
  "the_collector|per roll (0-1 heads alive): SPAWN 26%, FIREBALL 45%, BUFF 29%; (2 heads alive): FIREBALL 71%, BUFF 29% (thresholds are <=25 / <=70 inclusive)":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // the_guardian
  "the_guardian|fixed offensive loop CHARGING_UP -> FIERCE_BASH -> VENT_STEAM -> WHIRLWIND":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  "the_guardian|mode shift interrupts the current intent with DEFENSIVE_MODE and grants 20 block":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "the_guardian|fixed defensive sequence DEFENSIVE_MODE -> ROLL_ATTACK -> TWIN_SLAM -> WHIRLWIND (then offensive loop resumes)":
    "固定循环/脚本写在 pick 的 last_is 分支链上,规则为自由描述,工具不逐条解析",
  // the_maw
  "the_maw|after NOM the next move is always DROOL (hard-coded)":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  // time_eater
  "time_eater|HASTE is used exactly once, on the first roll after dropping below 50% HP":
    "概率表/条件概率,工具只核对单点概率(其余由 pick 的阈值分支实现)",
  // writhing_mass
  "writhing_mass|no move can be used (or Reactive-rerolled into) twice in a row":
    "规则为自由描述的触发条件,工具不逐条解析;由 combat 单测与 e2e 对拍覆盖",
  "writhing_mass|IMPLANT can only be used once per combat":
    "一次性/次数上限记在 EnemyState(或 pick 的分支计数)里,工具不解析自由描述",
  "writhing_mass|wiki conditional table (percent chance of NEXT move given last used move) - Parasite row: after Block ~14%, after Debuff 12.5%, after MultiHit ~14%, after BigHit ~11%; Block row: after Parasite ~33%, after Debuff 37.5%, after MultiHit ~43%, after BigHit ~33%; Debuff row: after Parasite ~22%, after Block ~29%, after MultiHit ~29%, after BigHit ~22%; MultiHit row: after Parasite ~33%, after Block ~43%, after Debuff 37.5%, after BigHit ~33%; BigHit row: after Parasite ~11%, after Block ~14%, after Debuff 12.5%, after MultiHit ~14% (approximations from the reroll cascade above)":
    "语料 conflicts/wiki 变体已裁决:本作按 lightspeed 反编译实现",
};

const PREBATTLE_STATUS: Record<string, string> = {
  STRENGTH: "STRENGTH", DEXTERITY: "DEXTERITY", PLATEDARMOR: "PLATEDARMOR", THORNS: "THORNS",
  ARTIFACT: "ARTIFACT", FLIGHT: "FLIGHT", MALLEABLE: "MALLEABLE", PAINFULSTABS: "PAINFULSTABS",
  MINION: "MINION", MINIONLEADER: "MINIONLEADER", REGROW: "REGROW", GENERICSTRENGTHUP: "STRENGTHUP",
  EXPLOSIVE: "EXPLOSIVE", SHIFTING: "SHIFTING", FADING: "FADING", REACTIVE: "REACTIVE", SLOW: "SLOW",
  TIMEWARP: "TIMEWARP", CURIOSITY: "CURIOSITY", REGEN: "REGENERATE", BEATOFDEATH: "BEATOFDEATH",
  INVINCIBLE: "INVINCIBLE", MODESHIFT: "MODESHIFT", ASLEEP: "ASLEEP", METALLICIZE: "METALLICIZE",
  BARRICADE: "BARRICADE", SPORECLOUD: "SPORECLOUD", SPLIT: "SPLIT", ANGRY: "ANGER",
  CURLUP: "CURLUP", THIEVERY: "THIEVERY", SURROUNDED: "SURROUNDED",
};
/** 语料 prebattle 里空层数按 1 记的二元状态 */
const PRE_BINARY = new Set(["SPLIT", "REACTIVE", "SHIFTING", "MINION", "MINIONLEADER", "REGROW", "PAINFULSTABS", "BARRICADE", "SURROUNDED", "ASLEEP"]);

// ---- AI(触发条件)登记 ----
/** 实现 pick 里多出来的飞升阈值(语料文本没提):key = `<实现 id>`,值 = Map<档, 原因> */
/** 实现 pick 里多出来的掷点阈值:key = 实现 id */
const AI_ROLL_EXTRA: Record<string, Record<number, string>> = {};
const AI_ASC_EXTRA: Record<string, Record<number, string>> = {
  giant_head: { 18: "A18 起首次 It Is Time 从第 5 回合提前到第 4 回合(ascension.rs hard_replace;语料只在 spec 里描述)" },
};

// ---- 载入 ----

function loadCorpus(dir: string): CorpusMonster[] {
  const out: CorpusMonster[] = [];
  for (const f of readdirSync(dir).filter((f) => /^monsters-.*\.json$/.test(f)).sort()) {
    const data = JSON.parse(readFileSync(join(dir, f), "utf8")) as unknown;
    const list = Array.isArray(data) ? data : (data as { monsters: unknown[] }).monsters;
    out.push(...(list as CorpusMonster[]));
  }
  return out;
}

function loadImpl(): ImplMonster[] {
  let text: string;
  if (IMPL_FILE) {
    text = readFileSync(IMPL_FILE, "utf8");
  } else {
    if (!existsSync(SPIRE)) throw new Error(`找不到 ${SPIRE},先 cargo build`);
    const r = spawnSync(SPIRE, ["--dump", "monsters"], { encoding: "utf8", maxBuffer: 1 << 26 });
    if (r.status !== 0) throw new Error(`spire --dump monsters 失败: ${r.stderr}`);
    text = r.stdout;
  }
  return (JSON.parse(text) as { monsters: ImplMonster[] }).monsters;
}

function loadSources(dir: string): string {
  return readdirSync(dir).filter((f) => f.endsWith(".rs")).sort().map((f) => readFileSync(join(dir, f), "utf8")).join("\n");
}

/** 语料招式 key -> 实现招式(名字后缀匹配 + 少数别名) */
function matchMoveByName(cid: string, ckey: string, moves: ImplMove[]): ImplMove | null {
  const alias = MOVE_ALIAS[`${cid}/${ckey}`];
  if (alias) return moves.find((m) => m.name === alias) ?? null;
  const n = norm(ckey);
  const cands = moves.filter((m) => n.endsWith(norm(m.name)) || norm(m.name).endsWith(n));
  cands.sort((a, b) => norm(b.name).length - norm(a.name).length);
  return cands[0] ?? null;
}

// ---- 审计主体 ----

interface Report {
  diffs: string[];
  guards: string[];
  counts: Record<string, number>;
  ai: { matched: number; registered: number };
}

function audit(corpus: CorpusMonster[], implList: ImplMonster[], src: string): Report {
  USED_REG.clear(); // 每次审计都从零统计"用到了哪些登记项"
  const diffs: string[] = [];
  const guards: string[] = [];
  const counts: Record<string, number> = {
    怪物: 0, 招式: 0, 飞升档: 0, 血量档: 0, 效果段: 0, 开局预置: 0, AI规则: 0,
  };
  let aiMatched = 0;
  let aiRegistered = 0;

  const implById = new Map(implList.map((m) => [m.id, m]));
  const usedImpl = new Map<string, Set<string>>();

  for (const cm of corpus) {
    const iid = ID_ALIAS[cm.id] ?? cm.id.toLowerCase();
    const im = implById.get(iid);
    if (!im) {
      guards.push(`实现缺少语料怪 ${cm.id}(期望 id ${iid})`);
      continue;
    }
    counts.怪物 = counts.怪物! + 1;
    const used = usedImpl.get(iid) ?? new Set<string>();
    usedImpl.set(iid, used);

    // --- 名字 / 类别 ---
    if (cm.name !== im.name) diffs.push(`${iid} 名字:语料 ${cm.name} vs 实现 ${im.name}`);
    const kindByCategory: Record<string, string> = { normal: "Normal", elite: "Elite", boss: "Boss", minion: "Normal", event: "Normal" };
    if (kindByCategory[cm.category] !== im.kind) {
      diffs.push(`${iid} 类别(${cm.category}):期望 ${kindByCategory[cm.category]} vs 实现 ${im.kind}`);
    }

    // --- 血量:基础区间 + 飞升档 ---
    if (cm.hp.base[0] !== im.hp[0] || cm.hp.base[1] !== im.hp[1]) {
      diffs.push(`${iid} 血量基础区间:语料 [${cm.hp.base}] vs 实现 [${im.hp}]`);
    }
    const hpNoop = cm.hp.asc[0] === cm.hp.base[0] && cm.hp.asc[1] === cm.hp.base[1];
    const expectHpChanges = hpNoop ? [] : [{ asc: cm.hp.ascLevel, hp: cm.hp.asc }];
    if (hpNoop) {
      if (!HP_NOOP[iid]) {
        guards.push(`${iid} 的语料飞升血量与基础相同(A${cm.hp.ascLevel}),这种空档得登记(HP_NOOP)`);
      } else {
        USED_REG.add(`hpnoop:${iid}`);
      }
    } else if (HP_NOOP[iid]) {
      guards.push(`HP_NOOP 里的 ${iid} 现在有真实飞升档了(登记过期)`);
    }
    if (show(expectHpChanges) !== show(im.hpChanges)) {
      diffs.push(`${iid} 血量飞升档:语料 ${show(expectHpChanges)} vs 实现 ${show(im.hpChanges)}`);
    } else if (expectHpChanges.length) {
      counts.血量档 = counts.血量档! + 1;
    }

    // --- 开局预置(prebattle / innate) ---
    comparePrebattle(cm, iid, im, diffs, counts, guards);

    // --- 招式 ---
    for (const ckey of Object.keys(cm.moves)) {
      const mv = cm.moves[ckey]!;
      const imv = matchMoveByName(cm.id, ckey, im.moves);
      if (!imv) {
        guards.push(`${iid} 招式 ${ckey} 在实现里找不到对应招式`);
        continue;
      }
      used.add(imv.name);
      counts.招式 = counts.招式! + 1;
      compareMove(cm, iid, ckey, mv, imv, diffs, counts);
    }

    // --- AI 触发条件 ---
    const aiSrc = pickSource(iid, src);
    if (!aiSrc) {
      guards.push(`${iid} 找不到 pick 函数源码(实现招式表里 pick: 与 fn 对不上?)`);
    } else {
      compareAi(cm, iid, im, aiSrc, diffs, (n) => (n === "m" ? aiMatched++ : aiRegistered++));
    }
  }

  // --- 反向守卫:实现里多出来的怪/招式 ---
  for (const im of implList) {
    // --only 时只审这一只,别拿没审的怪报缺
    if (ONLY && im.id !== ONLY.toLowerCase()) continue;
    if (!corpus.some((c) => (ID_ALIAS[c.id] ?? c.id.toLowerCase()) === im.id)) {
      guards.push(`实现里的怪 ${im.id} 在语料里没有`);
      continue;
    }
    const used = usedImpl.get(im.id)!;
    for (const m of im.moves) {
      if (!used.has(m.name)) guards.push(`${im.id} 的招式 ${m.name} 没被语料对账(语料里没有或名字对不上)`);
    }
  }

  // --- 守卫:效果 power 覆盖 ---
  const fxSeen = new Set<string>();
  for (const cm of corpus) {
    for (const ck of Object.keys(cm.moves)) {
      for (const fx of [...(cm.moves[ck]!.effects ?? []), ...Object.values(cm.moves[ck]!.asc ?? {}).flatMap((a) => a.effects ?? [])]) {
        fxSeen.add(fx.power.trim());
      }
    }
    for (const pb of cm.prebattle ?? []) fxSeen.add(`PRE:${pb.power}`);
  }
  for (const p of fxSeen) {
    if (p.startsWith("PRE:")) {
      const power = p.slice(4);
      if (power === "BLOCK") continue; // 开局格挡走 startBlock 对账
      if (!(une(power) in PREBATTLE_STATUS)) guards.push(`语料 prebattle 的 ${power} 没登记`);
      continue;
    }
    if (corpusEffTok(p, 1, "self")[0] === "UNMAPPED") guards.push(`语料效果 ${p} 没进口径也没登记`);
  }
  // --- 守卫:实现侧效果变体覆盖 ---
  for (const im of implList) {
    for (const m of im.moves) {
      for (const fx of [...m.base.fx, ...m.changes.flatMap((c) => c.fx), ...m.rolled.filter((r) => "v" in r).map((r) => r as ImplFx)]) {
        const tok = implEffTok(fx);
        if (tok && tok[0] === "UNMAPPED_IMPL") guards.push(`实现效果 ${fx.v} 没进对账口径也没登记(${im.id}/${m.name})`);
      }
    }
  }

  // 守卫:语料怪物的顶层字段必须都进口径或登记
  const coveredFields = new Set(["id", "name", "category", "hp", "prebattle", "moves", "ai"]);
  // ai 的四个键都由 AI 口径消费:firstTurn/historyRules 进规则对账,spec 是规则原文,
  // roll 由引擎统一保证(每回合必掷 aiRng.random(99))
  const aiCovered = new Set(["firstTurn", "historyRules", "spec", "roll"]);
  const checkField = (full: string, covered: boolean): void => {
    if (covered) return;
    if (full in CORPUS_FIELDS_SKIP) USED_REG.add(`field:${full}`);
    else guards.push(`语料字段 ${full} 没进口径也没登记`);
  };
  for (const cm of corpus) {
    for (const k of Object.keys(cm)) checkField(k, coveredFields.has(k));
    for (const k of Object.keys(cm.ai ?? {})) checkField(`ai.${k}`, aiCovered.has(k));
  }

  // 守卫:登记表里已经用不上的条目(报名与新内容对不上)也要报出来,防止登记表烂掉
  const allReg: string[] = [
    ...Object.keys(MOVES).map((k) => `move:${k}`),
    ...Object.keys(AI_RULE_SKIP).map((k) => `rule:${k}`),
    ...Object.keys(AI_FIRST_SKIP).map((k) => `first:${k}`),
    ...Object.keys(PREBATTLE_SKIP).map((k) => `pre:${k}`),
    ...Object.keys(PREBATTLE_CONFLICTS).map((k) => `prec:${k}`),
    ...Object.keys(IMPL_FX_EXCL).map((k) => `fx:${k}`),
    ...Object.entries(AI_ASC_EXTRA).flatMap(([id, m]) => Object.keys(m).map((n) => `asc:${id}:${n}`)),
    ...Object.entries(AI_ROLL_EXTRA).flatMap(([id, m]) => Object.keys(m).map((n) => `roll:${id}:${n}`)),
    ...Object.keys(HP_NOOP).map((k) => `hpnoop:${k}`),
    ...Object.keys(CORPUS_FIELDS_SKIP).map((k) => `field:${k}`),
  ];
  // --only 只看一只,别的登记项这轮不会被访问到,这里就不查"用不上"了
  if (!ONLY) for (const k of allReg) if (!USED_REG.has(k)) guards.push(`登记表里的 ${k} 已经用不上了(内容改了?)`);

  counts["AI规则"] = aiMatched + aiRegistered;
  return { diffs, guards, counts, ai: { matched: aiMatched, registered: aiRegistered } };
}

/** 招式逐档对账:意图 / 伤害 / 段数 / 格挡 / 效果段 */
function compareMove(
  cm: CorpusMonster,
  iid: string,
  ckey: string,
  mv: CorpusMove,
  imv: ImplMove,
  diffs: string[],
  counts: Record<string, number>,
): void {
  const regKey = `${iid}/${norm(imv.name)}`;
  const reg = MOVES[regKey] ?? {};
  if (MOVES[regKey]) USED_REG.add(`move:${regKey}`);
  // 登记为"抽签/动态"的字段不参与逐档数值比较(改走 rolled 区间或直接登记原因)
  const nullDamage = !!(reg.damage || reg.rolled);
  const nullTimes = !!reg.times;
  const baseRange = (mv.damage ?? null) as number | [number, number] | null;
  const ckeyOf = (s: CorpusSnap): string =>
    show([s.intent, nullDamage ? null : s.damage, nullTimes ? null : s.times, corpusBlocks(s), corpusOthers(s)]);
  const ikeyOf = (s: ImplSnap): string =>
    show([s.intent, nullDamage ? null : s.damage, nullTimes ? null : s.times, implBlocks(s), implOthers(s)]);
  const clevels = corpusLevels(mv, ckeyOf);
  const ilevels = implLevels(imv, ikeyOf);
  counts.飞升档 = counts.飞升档! + Math.max(clevels.length, ilevels.length) - 1;
  if (show(clevels) !== show(ilevels)) {
    diffs.push(`${iid}/${ckey} 飞升变化档:语料 ${show(clevels)} vs 实现 ${show(ilevels)}`);
  }
  // 实现内部的原始变化档:语料没记的必须登记
  for (const l of imv.changes.map((c) => c.asc)) {
    if (!ilevels.includes(l) && !(reg.stableLevels ?? []).includes(l)) {
      diffs.push(`${iid}/${ckey} 实现在 A${l} 有内部变化但语料没记(未登记 stableLevels)`);
    }
  }
  if (reg.rolled) compareRolled();

  for (const l of sortedUnion(clevels, ilevels)) checkSnap(l);

  /** 抽签伤害:语料区间(基础 + asc 覆盖) ↔ 实现的开局抽签区间 */
  function compareRolled(): void {
    const ascLevels = Object.keys(mv.asc ?? {}).map(Number).sort((a, b) => a - b);
    const cChanges: string[] = [show([0, baseRange])];
    let last: unknown = baseRange;
    for (const l of ascLevels) {
      const r = resolveMove(mv, l).damage;
      if (show(r) !== show(last)) {
        cChanges.push(show([l, r]));
        last = r;
      }
    }
    const iChanges = imv.rolled.map((r) =>
      r.dynamic ? show([r.asc, "dynamic"]) : show([r.asc, [r.lo, r.hi]]),
    );
    counts.飞升档 = counts.飞升档! + Math.max(cChanges.length, iChanges.length) - 1;
    if (imv.rolled.length === 0) {
      diffs.push(`${iid}/${ckey} 语料记了抽签区间 ${show(cChanges)},实现没有抽签伤害效果`);
    } else if (baseRange === null) {
      // 语料没给区间(只在 spec 文本里),实现的区间由 ascension.rs 单测钉住:登记
    } else if (show(cChanges) !== show(iChanges)) {
      diffs.push(`${iid}/${ckey} 抽签区间档:语料 ${show(cChanges)} vs 实现 ${show(iChanges)}`);
    }
    if (Array.isArray(baseRange) && !imv.rolled.some((r) => r.dynamic) && imv.base.damage !== baseRange[0]) {
      diffs.push(`${iid}/${ckey} 抽签招显示伤害:语料下限 ${baseRange[0]} vs 实现意图 ${imv.base.damage}`);
    }
  }

  function checkSnap(l: number): void {
    const a = resolveMove(mv, l);
    const b = implAt(imv, l);
    const at = l === 0 ? "" : ` @A${l}`;
    if (a.intent !== b.intent) diffs.push(`${iid}/${ckey}${at} 意图:语料 ${a.intent} vs 实现 ${b.intent}`);
    if (!nullDamage && show(a.damage) !== show(b.damage)) {
      diffs.push(`${iid}/${ckey}${at} 伤害:语料 ${show(a.damage)} vs 实现 ${show(b.damage)}`);
    }
    if (!nullTimes && show(a.times) !== show(b.times)) {
      diffs.push(`${iid}/${ckey}${at} 段数:语料 ${show(a.times)} vs 实现 ${show(b.times)}`);
    }
    const blk = matchToks(corpusBlocks(a), implBlocks(b));
    if (blk.miss.length || blk.extra.length) {
      diffs.push(`${iid}/${ckey}${at} 格挡:语料没认领 ${show(blk.miss)},实现多出 ${show(blk.extra)}`);
    } else {
      counts.效果段 = counts.效果段! + corpusBlocks(a).length;
    }
    if (!reg.fx) {
      const r = matchToks(corpusOthers(a), implOthers(b));
      if (r.miss.length || r.extra.length) {
        diffs.push(`${iid}/${ckey}${at} 效果:语料没认领 ${show(r.miss)},实现多出 ${show(r.extra)}`);
      }
      counts.效果段 = counts.效果段! + corpusOthers(a).length;
    }
  }
}

const sortedUnion = (a: number[], b: number[]): number[] => [...new Set([...a, ...b])].sort((x, y) => x - y);

/** 开局预置的逐档对账 */
function comparePrebattle(
  cm: CorpusMonster,
  iid: string,
  im: ImplMonster,
  diffs: string[],
  counts: Record<string, number>,
  guards: string[],
): void {
  const pb = cm.prebattle ?? [];
  if (pb.length === 0) return;
  // 语料侧逐档解析
  const corpusAt = (asc: number): { st: Map<string, number | null>; block: number | null } => {
    const st = new Map<string, number | null>();
    let block: number | null = null;
    for (const e of pb) {
      const key = une(e.power);
      if (key === "BLOCK") {
        let v = (e.amount ?? null) as number | null;
        for (const lvl of Object.keys(e.asc ?? {}).sort((a, b) => Number(a) - Number(b))) if (asc >= Number(lvl)) v = preAmount(e.asc![lvl]);
        block = v;
        continue;
      }
      if (!PREBATTLE_STATUS[key]) {
        guards.push(`语料 prebattle 的 ${e.power} 没登记(映射表)`);
        continue;
      }
      if (PREBATTLE_SKIP[key]) {
        USED_REG.add(`pre:${key}`);
        continue;
      }
      let v = (e.amount ?? null) as number | null;
      for (const lvl of Object.keys(e.asc ?? {}).sort((a, b) => Number(a) - Number(b))) if (asc >= Number(lvl)) v = preAmount(e.asc![lvl]);
      if (v === null && PRE_BINARY.has(key)) v = 1;
      st.set(PREBATTLE_STATUS[key]!, v);
    }
    return { st, block };
  };
  const implAt = (asc: number): { st: Map<string, number | null>; block: number } => {
    let snap = im.innate;
    for (const c of im.innateChanges) if (c.asc <= asc) snap = c;
    return { st: new Map(snap.statuses.map(([s, n]) => [une(s), n])), block: snap.block };
  };
  const levels = [0, ...new Set(pb.flatMap((e) => Object.keys(e.asc ?? {}).map(Number)))].sort((a, b) => a - b);
  for (const asc of levels) {
    const c = corpusAt(asc);
    const i = implAt(asc);
    const at = asc === 0 ? "" : ` @A${asc}`;
    counts.开局预置 = counts.开局预置! + c.st.size + (c.block !== null ? 1 : 0);
    if (c.block !== null && c.block !== i.block) {
      diffs.push(`${iid} 开局格挡${at}:语料 ${c.block} vs 实现 ${i.block}`);
    }
    const seen = new Set<string>();
    for (const [k, v] of c.st) {
      seen.add(k);
      if (!i.st.has(k)) {
        if (PREBATTLE_CONFLICTS[`${iid}/${k}`]) {
          USED_REG.add(`prec:${iid}/${k}`);
          continue;
        }
        if (PREBATTLE_SKIP[k]) continue;
        diffs.push(`${iid} 开局状态${at}:语料有 ${k}(${show(v)}),实现没有`);
        continue;
      }
      if (i.st.get(k) !== v) {
        if (PREBATTLE_CONFLICTS[`${iid}/${k}`]) {
          USED_REG.add(`prec:${iid}/${k}`);
          continue;
        }
        diffs.push(`${iid} 开局状态${at}:${k} 语料 ${show(v)} vs 实现 ${show(i.st.get(k))}`);
      }
    }
    for (const k of i.st.keys()) {
      if (!seen.has(k)) {
        if (k === "SPORECLOUD" || k === "SPLIT" || k === "REACTIVE") continue;
        if (PREBATTLE_CONFLICTS[`${iid}/${k}`]) continue;
        diffs.push(`${iid} 开局状态${at}:实现多出 ${k}(${show(i.st.get(k))})`);
      }
    }
  }
}

// ---- AI(触发条件)对账 ----

/** 取某只怪的 pick 函数源码:实现里 `pick: pick_xxx` -> fn 块 */
function pickSource(iid: string, src: string): string | null {
  // 在源码里找 `id: "<iid>"` 那一块里的 pick:
  const blocks = src.split("pub const ").slice(1);
  let fn: string | null = null;
  for (const blk of blocks) {
    if (new RegExp(`id: "${iid}"`).test(blk)) {
      const m = /pick: (\w+)/.exec(blk);
      if (m) fn = m[1]!;
      break;
    }
  }
  if (!fn) return null;
  const i = src.indexOf(`fn ${fn}(`);
  if (i < 0) return null;
  const j = src.indexOf("\n}\n", i);
  return src.slice(i, j);
}

/** pick 函数里声明的招式下标常量:index -> 常量名 */
function constsOf(body: string): Map<number, string> {
  const m = new Map<number, string>();
  for (const mm of body.matchAll(/const\s+([A-Z_0-9]+):\s*usize\s*=\s*(\d+)/g)) {
    m.set(Number(mm[2]), mm[1]!);
  }
  return m;
}

function compareAi(
  cm: CorpusMonster,
  iid: string,
  im: ImplMonster,
  body: string,
  diffs: string[],
  tally: (kind: "m" | "r") => void,
): void {
  const ft = cm.ai.firstTurn ?? "";
  // 首回合分支的两种写法:first_turn(),以及 match ctx.last() { None => ... }
  const implFirst = body.includes("first_turn()") || /None\s*=>/.test(body);
  // 1) 首回合固定招:`always X`
  const ftMatch = /^always\s+([A-Z_0-9]+)/.exec(ft.trim());
  if (ftMatch) {
    const tok = moveToken(cm, im, body, ftMatch[1]!);
    if (tok === null) {
      diffs.push(`${iid} 首回合固定招 ${ftMatch[1]}:pick 里找不到对应招式下标`);
    } else if (!implFirst && im.moves.length > 1) {
      if (!AI_FIRST_SKIP[iid]) diffs.push(`${iid} 首回合固定招 ${ftMatch[1]}:pick 没写首回合分支(first_turn/None =>)`);
      else {
        USED_REG.add(`first:${iid}`);
        tally("r");
      }
    } else if (tok === "" || body.includes(tok)) {
      tally("m");
    } else {
      diffs.push(`${iid} 首回合固定招 ${ftMatch[1]}:pick 里没出现 ${tok}`);
    }
  } else if (/^(none special|no special rule|rollMove|by position|roll consumed|no randomness)/.test(ft.trim())) {
    tally("m"); // 语料明确写了"没有特殊首招规则"
  } else if (ft.trim() === "") {
    // 没有 firstTurn 字段
  } else if (AI_FIRST_SKIP[iid]) {
    USED_REG.add(`first:${iid}`);
    tally("r");
  } else {
    AI_UNCOVERED.push({ id: iid, rule: `firstTurn: ${ft}` });
    diffs.push(`${iid} firstTurn 未覆盖:${ft}`);
  }

  // 2) 历史规则(先剥掉"A17+: / phase 1: / asc<17: "这类条件前缀;带飞升前缀的额外核对阈值)
  for (const raw of cm.ai.historyRules ?? []) {
    let r = raw.trim();
    let condAsc: number | null = null;
    const pre = /^(A(\d+)\+?|asc\s*[<>]=?\s*(\d+)|phase\s+\d+|asleep|airborne)\s*:\s*/.exec(r);
    if (pre) {
      const n = pre[2] ?? pre[3];
      if (n) condAsc = Number(n);
      r = r.slice(pre[0].length).trim();
    }
    if (condAsc !== null && !new RegExp(`asc\\s*>=\\s*${condAsc}`).test(body) && !new RegExp(`ctx\\.asc\\s*<\\s*${condAsc}`).test(body) && !body.includes(`turn() >= ${condAsc}`)) {
      if (!AI_RULE_SKIP[`${iid}|${raw}`]) diffs.push(`${iid} 历史规则 [${raw}] 的飞升条件 A${condAsc} 没在 pick 里出现`);
    }
    const first = /^turn 1 always\s+([A-Z_0-9]+)/.exec(r);
    if (first) {
      const tok = moveToken(cm, im, body, first[1]!);
      if (tok && (body.includes(tok) || tok === "")) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到`); }
      continue;
    }
    const alt = /^alternates\s+([A-Z_0-9]+)\s*\/\s*([A-Z_0-9]+) forever, starting with\s+([A-Z_0-9]+)/.exec(r);
    if (alt) {
      const a = moveToken(cm, im, body, alt[1]!);
      const b = moveToken(cm, im, body, alt[2]!);
      const s0 = moveToken(cm, im, body, alt[3]!);
      if (a && b && s0 && body.includes(a) && body.includes(b) && (implFirst || im.moves.length === 1 || body.includes(s0))) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到交替的两招/首招`); }
      continue;
    }
    const lastTwo = /^([A-Z_0-9]+) cannot be used if it was either of the last two moves/.exec(r);
    if (lastTwo) {
      const c = moveToken(cm, im, body, lastTwo[1]!);
      if (c && body.includes(`last_two_has(${c})`)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到 last_two_has`); }
      continue;
    }
    const then = /^([A-Z_0-9]+) then\s+([A-Z_0-9]+), always/.exec(r);
    if (then) {
      const a = moveToken(cm, im, body, then[1]!);
      const b = moveToken(cm, im, body, then[2]!);
      if (a && b && body.includes(a) && body.includes(b)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到先后两招`); }
      continue;
    }
    const neverAfter = /^([A-Z_0-9]+) never after itself or after\s+([A-Z_0-9]+)/.exec(r);
    if (neverAfter) {
      const a = moveToken(cm, im, body, neverAfter[1]!);
      const b = moveToken(cm, im, body, neverAfter[2]!);
      if (a && b && body.includes(a) && body.includes(b)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到这两招`); }
      continue;
    }
    const nth = /^([A-Z_0-9]+) every\s+(\d+)(?:st|nd|rd|th) turn starting turn/.exec(r);
    if (nth) {
      const c = moveToken(cm, im, body, nth[1]!);
      if (c && body.includes(c)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到`); }
      continue;
    }
    const never = /^([A-Z][A-Z_0-9]*)\s+(?:never|cannot be used)\s+(twice|three times|3x)\s+in a row/.exec(r);
    if (never) {
      const c = moveToken(cm, im, body, never[1]!);
      const prim = never[2] === "twice" ? "last_is" : "last_two_is";
      if (c && body.includes(`${prim}(${c})`)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到 ${prim}(${c ?? "?"})`); }
      continue;
    }
    const follow = /^([A-Z][A-Z_0-9]*)\s+(?:is )?always followed by\s+([A-Z_0-9]+)/.exec(r);
    if (follow) {
      const c = moveToken(cm, im, body, follow[2]!);
      if (c && body.includes(c)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到后继招式`); }
      continue;
    }
    const every = /^(?:uses\s+)?([A-Z][A-Z_0-9]*)\s+every turn/.exec(r);
    if (every) {
      const c = moveToken(cm, im, body, every[1]!);
      if (c && body.includes(c)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 未在 pick 里找到`); }
      continue;
    }
    const prob = /^(?:per roll|probabilities per roll)[^:]*:\s*([A-Z][A-Z_0-9]*)\s+(\d+)%/.exec(r);
    if (prob) {
      const p = Number(prob[2]) / 100;
      const probs = [
        ...[...body.matchAll(/flip\((\d+),\s*(\d+)\)/g)].map((m) => Number(m[1]) / Number(m[2])),
        // 概率也可能写成 roll 阈值(roll < 65 即 65%)
        ...[...body.matchAll(/<\s*(\d+)/g)].map((m) => Number(m[1]) / 100),
      ];
      if (probs.some((x) => Math.abs(x - p) < 1e-6)) tally("m");
      else if (AI_RULE_SKIP[`${iid}|${raw}`]) {
        USED_REG.add(`rule:${iid}|${raw}`);
        tally("r");
      }
      else { AI_UNCOVERED.push({ id: iid, rule: raw }); diffs.push(`${iid} 历史规则 [${raw}] 的 ${p} 没在 pick 的 flip/roll 阈值里出现`); }
      continue;
    }
    if (AI_RULE_SKIP[`${iid}|${raw}`]) {
      USED_REG.add(`rule:${iid}|${raw}`);
      tally("r");
    } else {
      AI_UNCOVERED.push({ id: iid, rule: raw });
      diffs.push(`${iid} 历史规则未覆盖:[${raw}]`);
    }
  }

  // 3) 反向:实现 pick 用的飞升阈值必须在语料提过(或登记)
  const text = `${cm.ai.spec ?? ""} ${(cm.ai.historyRules ?? []).join(" ")} ${ft}`;
  const corpusAsc = new Set<number>();
  for (const m of text.matchAll(/asc\s*>=\s*(\d+)/g)) corpusAsc.add(Number(m[1]));
  for (const m of text.matchAll(/asc\s*<\s*(\d+)/g)) corpusAsc.add(Number(m[1]));
  for (const m of text.matchAll(/A(\d+)\+?/g)) corpusAsc.add(Number(m[1]));
  const extra = AI_ASC_EXTRA[iid] ?? {};
  for (const m of body.matchAll(/asc\s*>=\s*(\d+)/g)) {
    const n = Number(m[1]);
    if (n in extra) USED_REG.add(`asc:${iid}:${n}`);
    if (!corpusAsc.has(n) && !(n in extra)) {
      diffs.push(`${iid} pick 用了语料没提的飞升阈值 A${n}(未登记)`);
    }
  }
  // 4) 掷点分档:实现 pick 里的 roll 阈值必须都在语料文本里提过(或登记)
  const corpusRolls = new Set<number>();
  for (const m of text.matchAll(/roll\s*<\s*(\d+)/g)) corpusRolls.add(Number(m[1]));
  const rollExtra = AI_ROLL_EXTRA[iid] ?? {};
  for (const m of body.matchAll(/\broll\w*\s*<\s*(\d+)/g)) {
    const n = Number(m[1]);
    if (n in rollExtra) USED_REG.add(`roll:${iid}:${n}`);
    if (!corpusRolls.has(n) && !(n in rollExtra)) {
      diffs.push(`${iid} pick 用了语料没提的掷点阈值 roll<${n}(未登记)`);
    }
  }
}

/** 语料里的招式名 -> pick 源码里对应的下标常量名(找不到就退化成下标字面量) */
function moveToken(cm: CorpusMonster, im: ImplMonster, body: string, token: string): string | null {
  const t = norm(token);
  const keys = Object.keys(cm.moves);
  let ci = keys.findIndex((k) => norm(k) === t);
  if (ci < 0) ci = keys.findIndex((k) => norm(k).endsWith(t));
  if (ci < 0) ci = keys.findIndex((k) => t.endsWith(norm(k)));
  if (ci < 0) return null;
  const imv = matchMoveByName(cm.id, keys[ci]!, im.moves);
  if (!imv) return null;
  const idx = im.moves.indexOf(imv);
  return constsOf(body).get(idx) ?? String(idx);
}

// ---- 报告 ----

function printReport(r: Report, label: string): void {
  const c = r.counts;
  console.log(`怪物审计(${label}):`);
  console.log(
    `  怪 ${c.怪物} / 招式 ${c.招式} / 飞升变化档 ${c.飞升档} / 血量档 ${c.血量档} / 效果段 ${c.效果段} / 开局预置 ${c.开局预置} / AI 规则 ${c["AI规则"]}(对账 ${r.ai.matched} + 登记 ${r.ai.registered})`,
  );
  if (r.diffs.length) {
    console.log(`不一致 ${r.diffs.length} 处:`);
    for (const d of r.diffs) console.log(`  - ${d}`);
  } else {
    console.log("不一致 0 处");
  }
  if (r.guards.length) {
    console.log(`守卫: FAIL(${r.guards.length})`);
    for (const g of r.guards) console.log(`  ! ${g}`);
  } else {
    console.log("守卫: PASS");
  }
}

// ---- 自检:改坏一方 -> 必须报错;恢复 -> 必须通过 ----
// 每个模式:改一处 -> 断言报错里出现指定关键词。基准(不改)必须是 0 不一致 + 守卫 PASS。

interface Mutation {
  name: string;
  /** 期望报错里出现的关键词 */
  want: string;
  corpus?: (c: CorpusMonster[]) => void;
  impl?: (m: ImplMonster[]) => void;
  src?: (s: string) => string;
}

const MUTATIONS: Mutation[] = [
  {
    name: "改伤害(语料):CULTIST 黑暗打击 6->7",
    want: "CULTIST_DARK_STRIKE",
    corpus: (c) => {
      c.find((m) => m.id === "CULTIST")!.moves.CULTIST_DARK_STRIKE!.damage = 7;
    },
  },
  {
    name: "改飞升档位(语料):CULTIST 咒语 A2 -> A3",
    want: "CULTIST_INCANTATION",
    corpus: (c) => {
      const mv = c.find((m) => m.id === "CULTIST")!.moves.CULTIST_INCANTATION!;
      mv.asc!["3"] = mv.asc!["2"]!;
      delete mv.asc!["2"];
    },
  },
  {
    name: "改飞升数值(语料):JAW_WORM 咆哮 A17 力量 5->6",
    want: "JAW_WORM_BELLOW",
    corpus: (c) => {
      const mv = c.find((m) => m.id === "JAW_WORM")!.moves.JAW_WORM_BELLOW!;
      mv.asc!["17"]!.effects = [{ power: "STRENGTH", amount: 6, target: "self" }];
    },
  },
  {
    name: "改血量档(语料):CULTIST ascLevel 7->8",
    want: "cultist 血量飞升档",
    corpus: (c) => {
      c.find((m) => m.id === "CULTIST")!.hp.ascLevel = 8;
    },
  },
  {
    name: "改血量区间(语料):CULTIST asc [50,56]->[51,57]",
    want: "cultist 血量飞升档",
    corpus: (c) => {
      c.find((m) => m.id === "CULTIST")!.hp.asc = [51, 57];
    },
  },
  {
    name: "删招式(语料):JAW_WORM 去掉 BELLOW -> 反向守卫",
    want: "BELLOW",
    corpus: (c) => {
      delete c.find((m) => m.id === "JAW_WORM")!.moves.JAW_WORM_BELLOW;
    },
  },
  {
    name: "改多段(语料):CENTURION 狂怒 6x3 -> 6x4",
    want: "CENTURION_FURY",
    corpus: (c) => {
      c.find((m) => m.id === "CENTURION")!.moves.CENTURION_FURY!.hits = 4;
    },
  },
  {
    name: "改格挡(语料):MUGGER 烟雾弹 block 11->12",
    want: "MUGGER_SMOKE_BOMB",
    corpus: (c) => {
      c.find((m) => m.id === "MUGGER")!.moves.MUGGER_SMOKE_BOMB!.block = 12;
    },
  },
  {
    name: "改召唤数(语料):THE_COLLECTOR 召唤 2 只火炬头 -> 3",
    want: "THE_COLLECTOR_SPAWN",
    corpus: (c) => {
      c.find((m) => m.id === "THE_COLLECTOR")!.moves.THE_COLLECTOR_SPAWN!.effects = [
        { power: "SUMMON_TORCH_HEADS", amount: 3, target: "allies" },
      ];
    },
  },
  {
    name: "改塞牌张数(语料):SENTRY 光束 CARD:DAZED 2->1",
    want: "SENTRY_BOLT",
    corpus: (c) => {
      c.find((m) => m.id === "SENTRY")!.moves.SENTRY_BOLT!.effects = [{ power: "CARD:DAZED", amount: 1, target: "player" }];
    },
  },
  {
    name: "改意图(语料):BLUE_SLAVER 耙击 ATTACK_DEBUFF->ATTACK",
    want: "BLUE_SLAVER_RAKE",
    corpus: (c) => {
      c.find((m) => m.id === "BLUE_SLAVER")!.moves.BLUE_SLAVER_RAKE!.intent = "ATTACK";
    },
  },
  {
    name: "改增减益层数(语料):FUNGI_BEAST 生长 力量 3->4",
    want: "FUNGI_BEAST_GROW",
    corpus: (c) => {
      c.find((m) => m.id === "FUNGI_BEAST")!.moves.FUNGI_BEAST_GROW!.effects = [{ power: "STRENGTH", amount: 4, target: "self" }];
    },
  },
  {
    name: "改基础血量(语料):GUARDIAN 240->241",
    want: "the_guardian 血量基础区间",
    corpus: (c) => {
      const g = c.find((m) => m.id === "THE_GUARDIAN")!;
      g.hp.base = [241, 241];
    },
  },
  {
    name: "改实现(impl dump):CULTIST 黑暗打击 6->7",
    want: "CULTIST_DARK_STRIKE",
    impl: (ms) => {
      const m = ms.find((x) => x.id === "cultist")!;
      const mv = m.moves.find((x) => x.name === "Dark Strike")!;
      mv.base.damage = 7;
    },
  },
  {
    name: "改实现飞升档(impl dump):CULTIST 咒语变化档 2->3",
    want: "CULTIST_INCANTATION",
    impl: (ms) => {
      const m = ms.find((x) => x.id === "cultist")!;
      const mv = m.moves.find((x) => x.name === "Incantation")!;
      mv.changes = mv.changes.map((c) => (c.asc === 2 ? { ...c, asc: 3 } : c));
    },
  },
  {
    name: "改实现意图(impl dump):BLUE_SLAVER 耙击 attack_debuff->attack",
    want: "BLUE_SLAVER_RAKE",
    impl: (ms) => {
      const m = ms.find((x) => x.id === "blue_slaver")!;
      const mv = m.moves.find((x) => x.name === "Rake")!;
      mv.base.intent = "attack";
    },
  },
  {
    name: "删实现招式(impl dump):MUGGER 去掉 Smoke Bomb -> 反向守卫",
    want: "MUGGER_SMOKE_BOMB",
    impl: (ms) => {
      const m = ms.find((x) => x.id === "mugger")!;
      m.moves = m.moves.filter((x) => x.name !== "Smoke Bomb");
    },
  },
  {
    name: "改实现血量档(impl dump):CULTIST A7 -> A8",
    want: "cultist 血量飞升档",
    impl: (ms) => {
      const m = ms.find((x) => x.id === "cultist")!;
      m.hpChanges = m.hpChanges.map((c) => ({ ...c, asc: 8 }));
    },
  },
  {
    name: "改开局预置(语料):SPIKER 尖刺 A2 层数 4->5",
    want: "spiker 开局状态",
    corpus: (c) => {
      const e = c.find((m) => m.id === "SPIKER")!.prebattle![0]!;
      e.asc!["2"] = { amount: 5 };
    },
  },
  {
    name: "改塞牌牌堆(语料):SPIRE_SPEAR 灼伤 A18 drawTop->discard",
    want: "SPIRE_SPEAR_BURN_STRIKE",
    corpus: (c) => {
      const mv = c.find((m) => m.id === "SPIRE_SPEAR")!.moves.SPIRE_SPEAR_BURN_STRIKE!;
      mv.asc!["18"]!.effects = [{ power: "AddCard:BURN:discard", amount: 2, target: "player" }];
    },
  },
  {
    name: "加怪(语料):凭空多出一只 NOT_A_MONSTER -> 覆盖守卫",
    want: "NOT_A_MONSTER",
    corpus: (c) => {
      c.push({ ...structuredClone(c[0]!), id: "NOT_A_MONSTER", name: "Not A Monster" });
    },
  },
  {
    name: "加招式(语料):CULTIST 多出一招 -> 覆盖守卫",
    want: "COVERAGE_EXTRA",
    corpus: (c) => {
      c.find((m) => m.id === "CULTIST")!.moves.CULTIST_COVERAGE_EXTRA = {
        intent: "ATTACK",
        damage: 99,
        hits: 1,
        block: null,
        effects: [],
      };
    },
  },
  {
    name: "改效果名(语料):JAW_WORM 咆哮 STRENGTH->NOT_A_POWER -> 覆盖守卫",
    want: "NOT_A_POWER",
    corpus: (c) => {
      c.find((m) => m.id === "JAW_WORM")!.moves.JAW_WORM_BELLOW!.effects = [
        { power: "NOT_A_POWER", amount: 1, target: "self" },
      ];
    },
  },
  {
    name: "改实现 pick(源码):虱子的 roll 阈值 25->26",
    want: "掷点阈值",
    src: (s) => s.replace("if roll < 25 {", "if roll < 26 {"),
  },
  {
    name: "改实现 pick(源码):Cultist 首回合分支去掉 first_turn",
    want: "首回合固定招",
    src: (s) => s.replace("if ctx.first_turn() {\n        0\n    } else {\n        1\n    }", "if ctx.turn() == 0 {\n        0\n    } else {\n        1\n    }"),
  },
];

function selftest(): void {
  const corpus = loadCorpus(CORPUS_DIR);
  const impl = loadImpl();
  const src = loadSources(SRC_DIR);
  const base = audit(corpus, impl, src);
  if (base.diffs.length || base.guards.length) {
    console.log("自检基准不干净:先让 --only/全量审计通过再跑 --selftest");
    printReport(base, "基准");
    process.exit(1);
  }
  let caught = 0;
  let missed = 0;
  for (const mut of MUTATIONS) {
    // 每个模式都从干净副本出发,互不影响
    const c = structuredClone(corpus);
    const im = structuredClone(impl);
    let s = src;
    mut.corpus?.(c);
    mut.impl?.(im);
    if (mut.src) s = mut.src(s);
    const r = src === s ? audit(c, im, src) : audit(c, im, s);
    const hit = [...r.diffs, ...r.guards].some((d) => d.includes(mut.want));
    if (hit) caught++;
    else {
      missed++;
      console.log(`[FAIL] ${mut.name}:没抓到(关键词 "${mut.want}")`);
    }
  }
  // 恢复:原始输入必须仍然干净(改坏->报错 之后,恢复->通过)
  const restore = audit(corpus, impl, src);
  if (restore.diffs.length || restore.guards.length) {
    console.log("[FAIL] 恢复后不干净");
    missed++;
  }
  console.log(
    `恢复:基准重跑 -> 不一致 ${restore.diffs.length} 处,守卫 ${restore.guards.length ? "FAIL" : "PASS"}`,
  );
  console.log(`自检:抓到 ${caught},漏检 ${missed}(共 ${MUTATIONS.length} 个模式 + 1 次恢复)`);
  process.exit(missed === 0 ? 0 : 1);
}

// ---- main ----

if (LIST_AI) {
  // 维护登记表用:把"没解析也没登记"的 AI 规则打成 JSON
  audit(loadCorpus(CORPUS_DIR), loadImpl(), loadSources(SRC_DIR));
  console.log(JSON.stringify(AI_UNCOVERED, null, 1));
  process.exit(0);
} else if (SELFTEST) {
  selftest();
} else {
  const corpus = loadCorpus(CORPUS_DIR);
  const impl = loadImpl();
  const src = loadSources(SRC_DIR);
  const filtered = ONLY ? corpus.filter((m) => (ID_ALIAS[m.id] ?? m.id.toLowerCase()) === ONLY.toLowerCase()) : corpus;
  if (ONLY && filtered.length === 0) {
    console.error(`没有匹配的怪:${ONLY}`);
    process.exit(2);
  }
  const r = audit(filtered, impl, src);
  printReport(r, ONLY ? `--only ${ONLY}` : `${corpus.length} 只怪`);
  process.exit(r.diffs.length || r.guards.length ? 1 : 0);
}
