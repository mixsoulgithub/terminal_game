// 语料数据审计:不拿参考实现当基准,直接从原版证据源(语料 corpus JSON,记自
// sts_lightspeed 反编译)抽出每张牌/每瓶药水/每件遗物的数值事实,再用沙盒在
// 受控局面下实测本作的产出,逐条对比。不一致的由人对 wiki 定夺。
//
//   bun tools/audit_corpus.ts                 # 全量审计 + 报告(守卫失败或不一致即 exit != 0)
//   bun tools/audit_corpus.ts --only cards    # 只审某类(cards|potions|relics)
//   bun tools/audit_corpus.ts --seed 12345
//   bun tools/audit_corpus.ts --out tools/golden/audit_report.txt
//   bun tools/audit_corpus.ts --selftest      # 历史盲区回归:逐条确认工具还抓得到(漏检即 exit != 0)
//   bun tools/audit_corpus.ts --corpus <目录>  # 换语料目录(自检用它跑改坏的临时副本)
//   bun tools/audit_corpus.ts --write-registry # 按当前状态重写 tools/audit_registry.txt
//
// 语料 text 里的数值与 values/upgrade 字段是同一份来源;本工具从 text 抽事实、
// 用 values 做语料自洽核对,再用 `spire --sandbox-batch` 实测本作行为。
//
// 文本里条件/触发式/每张类效果进口径的方式:能直测的写进 cardFacts;需要额外触发的
// 用探针(conditionalScenarios / blindSpotScenarios 等);测不了的登记进
// CARD_NOT_COMPARED / POTION_NOT_COMPARED / RELIC_NOT_COMPARED。
//
// 两道守卫(任一不满足即 exit != 0):
//   守卫一 口径覆盖:有数值效果却没进口径、又没登记的内容直接判失败。
//   守卫二 内容登记表:本作或语料里新增了卡/遗物/药水却没登记(audit_registry.txt)判失败。
// 这两道守卫把"哨卫消耗回能"那类静默漏检从"人肉发现"变成"工具拦住"。
//
// 另有"文案 ↔ 实现"双向静态校验(不依赖沙盒):遗物/药水/状态诅咒的数值较早加入;
// 战斗/技能/能力牌由 CARD_RULES 覆盖——数值(伤害/格挡/层数/抽牌/回能/掉血/回血/塞牌/
// 循环次数)、触发词(Whenever / 回合开始末 / When drawn / While in hand / If… / for each /
// X times / twice)、关键词标记(Exhaust/Ethereal/Innate/Retain/无限升级)、升级差异(基础档
// 查基础 effects,升级档查 upgrade 里的 effects)四处对账,跑在语料文案与实现文案两侧。
// 文案里出现数字或条件词却没被任何规则命中、也没登记 CARD_TEXT_SKIP -> 守卫失败。

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const REPO = join(ROOT, "..");
const DEFAULT_CORPUS = join(REPO, "refs", "slay-the-cli", "data", "corpus");
const REGISTRY = join(HERE, "audit_registry.txt");
const SPIRE = process.env.SPIRE_BIN ?? join(ROOT, "target", "debug", "spire");
if (!existsSync(SPIRE)) throw new Error(`找不到 ${SPIRE},先 cargo build`);

// ---- 命令行 ----
const argv = process.argv.slice(2);
function flag(name: string): string | undefined {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
}
const ONLY = flag("--only");
const SEED = flag("--seed") ?? "12345";
const OUT = flag("--out");
/** 语料目录(默认 refs/slay-the-cli/data/corpus);--selftest 会指向改坏的临时副本 */
const CORPUS = flag("--corpus") ?? DEFAULT_CORPUS;
/** 自检模式:把历史盲区逐条喂回工具,确认每条都被拦下;任一漏检即 exit != 0 */
const SELFTEST = argv.includes("--selftest");
/** 内容登记表:新增卡/遗物/药水必须登记,否则守卫二报错 */
const WRITE_REGISTRY = argv.includes("--write-registry");
/** 只跑自检/只写登记表时不打印主报告;--selftest 需要全套场景,忽略 --only */
const want = (kind: string) => SELFTEST || ONLY === undefined || ONLY === kind;

// ---- 语料(外部 JSON:用类型守卫验形状后再当领域类型用) ----
interface CorpusValues {
  damage: number | null;
  block: number | null;
  magic: number | null;
  hits: number | null;
}
interface CorpusCard {
  id: string;
  type: string;
  cost: number | null;
  values: CorpusValues;
  upgrade: CorpusValues & { cost: number | null };
  text: string;
  /** 颜色(red/green/blue/purple/colorless)与卡池归属,选牌结果集合要用 */
  color?: string;
  pool?: string;
}
interface CorpusPotion {
  id: string;
  text: string;
  potency?: { base: number | null; sacredBarkDoubles?: boolean };
}
interface CorpusRelic {
  id: string;
  text: string;
  counter?: string;
}

const asRecord = (x: unknown): Record<string, unknown> | null =>
  typeof x === "object" && x !== null ? (x as Record<string, unknown>) : null;

function isCard(x: unknown): x is CorpusCard {
  const o = asRecord(x);
  return !!o && typeof o.id === "string" && typeof o.text === "string" && !!asRecord(o.values);
}
function isPotion(x: unknown): x is CorpusPotion {
  const o = asRecord(x);
  return !!o && typeof o.id === "string" && typeof o.text === "string";
}
function isRelic(x: unknown): x is CorpusRelic {
  const o = asRecord(x);
  return !!o && typeof o.id === "string" && typeof o.text === "string";
}

function readCorpus(name: string): unknown[] {
  const raw: unknown = JSON.parse(readFileSync(join(CORPUS, `${name}.json`), "utf8"));
  if (!Array.isArray(raw)) throw new Error(`${name}.json 不是数组`);
  return raw;
}
const corpusCards = readCorpus("cards").filter(isCard);
const corpusPotions = readCorpus("potions").filter(isPotion);
const corpusRelics = readCorpus("relics").filter(isRelic);

/** 语料 id -> 本作 id(STRIKE_RED -> strike);只有基础牌才剥颜色后缀,SEEING_RED 不能动 */
function gid(cid: string): string {
  const s = cid.toLowerCase();
  for (const suf of ["_red", "_green", "_blue", "_purple"]) {
    if (s.endsWith(suf) && ["strike", "defend"].includes(s.slice(0, -suf.length))) return s.slice(0, -suf.length);
  }
  return s;
}
const cardByGame = new Map<string, CorpusCard>();
for (const c of corpusCards) cardByGame.set(gid(c.id), c);
const potionByGame = new Map<string, CorpusPotion>();
for (const p of corpusPotions) potionByGame.set(p.id.toLowerCase(), p);
const relicByGame = new Map<string, CorpusRelic>();
for (const r of corpusRelics) relicByGame.set(r.id.toLowerCase(), r);

// ---- 本作已实现的内容清单 ----
function dump(what: string): string[][] {
  const r = spawnSync(SPIRE, ["--dump", what], { encoding: "utf8", maxBuffer: 1 << 26 });
  return r.stdout
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map((l) => l.split(/\s+/));
}
const ourCards = dump("cards").filter((c) => c[3] === "cost").map((c) => c[0]!);
const ourPotions = dump("potions").map((c) => c[0]!);
const ourRelics = dump("relics").map((c) => c[0]!);
/** 刻意未实现的内容(其它职业专属机制):不参与审计 */
const gated = new Set<string>();
for (const row of dump("gated")) gated.add(`${row[0]}/${row[1]}`);

// ---- 文本归一 ----
/** 去掉 {模板}/[[链接]]/$关键字 标记,<br> 换行;保留 [a|b] 两级数值 */
function norm(text: string): string {
  return text
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/\{\{([^}]*)\}\}/g, (_m, inner: string) => inner.split("|").pop()!.trim())
    .replace(/\[\[([^\]]*)\]\]/g, (_m, inner: string) => inner.split("|").pop()!.trim())
    .replace(/\$(\w+)/g, "$1");
}
/** 取 [base|up] / [1 card|2 cards] 里的对应级数字;纯数字原样返回 */
function pick(tok: string, up: boolean): number | null {
  const inner = tok.startsWith("[") && tok.endsWith("]") ? tok.slice(1, -1) : tok;
  const parts = inner.split("|");
  const chosen = up && parts.length > 1 ? parts[1]! : parts[0]!;
  const m = chosen.match(/\d+/);
  return m ? Number(m[0]) : null;
}
/**
 * 取 [@RE @RE|@RE @RE @RE] 里的能量图标个数。能量在语料里用 @RE 表示、不带数字,
 * `pick` 取不到;两级值只需数对应一级里的 @RE 个数。
 */
function reCount(tok: string, up: boolean): number {
  const inner = tok.startsWith("[") && tok.endsWith("]") ? tok.slice(1, -1) : tok;
  const parts = inner.split("|");
  const chosen = up && parts.length > 1 ? parts[1]! : parts[0]!;
  return (chosen.match(/@RE/g) ?? []).length;
}

// ---- 事实模型 ----
type Fact =
  | { k: "damage"; v: number }
  | { k: "block"; v: number }
  | { k: "enemy_power"; name: string; v: number }
  | { k: "player_power"; name: string; v: number }
  | { k: "draw"; v: number }
  | { k: "self_hp"; v: number }
  | { k: "energy"; v: number }
  /** 探针轴(持有/抽到/回合末/多回合/选牌):key 是 probe 返回记录的字段名 */
  | { k: "probe"; key: string; v: number };

const POWER_KEY: Record<string, string> = {
  Vulnerable: "vulnerable",
  Weak: "weak",
  Frail: "frail",
  Poison: "poison",
  Strength: "strength",
  Dexterity: "dexterity",
  Artifact: "artifact",
  Thorns: "thorns",
  Metallicize: "metallicize",
  "Plated Armor": "plated_armor",
  Regeneration: "regenerate",
  Intangible: "intangible",
  Ritual: "ritual",
  Focus: "focus",
};

/**
 * 解析单行文本得到数值事实。
 * 返回 null 表示这一行不匹配任何已知模式(口径外:需登记或加探针,由报告
 * "口径覆盖"一节暴露);返回 [] 表示匹配但属于"条件/每张"类,本行不直接产生
 * 可比数值。
 */
function lineFacts(l: string, up: boolean): Fact[] | null {
  // Deal N damage [to ALL enemies / to a random enemy] [N times | twice]
  let m = l.match(/^Deal\s+(\[[^\]]*\]|\S+)\s+damage(.*)$/);
  if (m) {
    const rest = m[2]!;
    const dmg = pick(m[1]!, up);
    if (dmg === null || dmg <= 0 || /for each/.test(rest)) return [];
    let hits = 1;
    const times = rest.match(/(\[[^\]]*\]|\S+)\s+times\b/);
    if (times) hits = pick(times[1]!, up) ?? 1;
    else if (/\btwice\b/.test(rest)) hits = 2;
    return [{ k: "damage", v: dmg * hits }];
  }
  // Gain N Block
  m = l.match(/^Gain\s+(\[[^\]]*\]|\S+)\s+Block\b/);
  if (m) {
    if (/for each/.test(l)) return [];
    const b = pick(m[1]!, up);
    return b !== null ? [{ k: "block", v: b }] : [];
  }
  // Apply N Weak/Vulnerable/Frail(可带 "and ..." 一并处理)
  m = l.match(/^Apply\s+(\[[^\]]*\]|\S+)\s+(Weak|Vulnerable|Frail)\b/);
  if (m) {
    const out: Fact[] = [];
    const v = pick(m[1]!, up);
    if (v !== null) out.push({ k: "enemy_power", name: POWER_KEY[m[2]!]!, v });
    const and = l.match(/and\s+(?:N\s+)?(Weak|Vulnerable|Frail)\b/);
    if (and && m[2] !== and[1] && v !== null) out.push({ k: "enemy_power", name: POWER_KEY[and[1]!]!, v });
    return out;
  }
  // 句中 apply N Vulnerable(thunderclap)
  m = l.match(/\bapply\s+(\[[^\]]*\]|\S+)\s+(Weak|Vulnerable|Frail)\b/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "enemy_power", name: POWER_KEY[m[2]!]!, v }] : [];
  }
  // Enemy loses N Strength(disarm)
  m = l.match(/^Enemy loses\s+(\[[^\]]*\]|\S+)\s+Strength\b/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "enemy_power", name: "strength", v: -v }] : [];
  }
  // Gain N Strength/Dexterity/...
  m = l.match(/^Gain\s+(\[[^\]]*\]|\S+)\s+(Strength|Dexterity|Artifact|Thorns|Metallicize|Plated Armor|Regeneration|Intangible|Ritual|Focus)\b/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "player_power", name: POWER_KEY[m[2]!]!, v }] : [];
  }
  // Gain @RE(@RE ...) / Gain [@RE@RE|@RE@RE@RE] -> 能量(两级值按 @RE 个数;句末可有标点)
  m = l.match(/^Gain\s+(\[@RE[^\]]*\][^\n]*|@RE.*)$/);
  if (m) {
    const n = reCount(m[1]!, up);
    return n > 0 ? [{ k: "energy", v: n }] : [];
  }
  // Draw N cards
  m = l.match(/^Draw\s+(\[[^\]]*\]|\S+)/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "draw", v }] : [];
  }
  // Lose N HP
  m = l.match(/^Lose\s+(\[[^\]]*\]|\S+)\s+HP\b/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "self_hp", v: -v }] : [];
  }
  // Heal N HP
  m = l.match(/^Heal\s+(\[[^\]]*\]|\S+)\s+HP\b/);
  if (m) {
    const v = pick(m[1]!, up);
    return v !== null ? [{ k: "self_hp", v }] : [];
  }
  return null;
}

/**
 * 从牌的语料文本抽数值事实。只认行首直白的 "Deal N damage" / "Gain N Block" /
 * "Apply N X" / "Gain N X" / "Draw N" / "Lose N HP" / "Gain @RE"(含 [@RE|@RE])。
 * 句中条件句(If/Whenever/At the ...)、每张/等于类、没数值的句子不产生事实:
 * 这些交给 CARD_SPECIAL / POWER_STACK / 探针,或登记进 CARD_NOT_COMPARED,
 * 由报告"口径覆盖"一节显式暴露,不静默跳过。
 */
function cardFacts(card: CorpusCard, up: boolean): Fact[] {
  const facts: Fact[] = [];
  for (const line of norm(card.text).split("\n")) {
    const f = lineFacts(line.trim(), up);
    if (f) facts.push(...f);
  }
  return facts;
}

/** 效果句判据:含效果动词 + 数字/@RE,用来找"有数值效果但没进口径"的文本 */
const EFFECT_VERB = /(deal|deals|gain|draw|apply|lose|heal|exhaust|add|shuffle|put|increase|double|costs|reduce|play)/i;
/** 该牌文本里"有数值效果但 lineFacts 认不出"的行(口径盲区候选) */
function unparsedEffectLines(card: CorpusCard): string[] {
  const out: string[] = [];
  for (const raw of norm(card.text).split("\n")) {
    const l = raw.trim();
    if (!l || !EFFECT_VERB.test(l) || !/\d|@RE/.test(l)) continue;
    if (lineFacts(l, false) === null) out.push(l);
  }
  return out;
}

/** 抽出来的数值事实与 values/upgrade 字段对不对得上(语料自洽检查) */
function corpusSelfCheck(card: CorpusCard): string[] {
  const out: string[] = [];
  const t = norm(card.text);
  const chk = (label: string, tok: RegExp, base: number | null, up: number | null) => {
    const m = t.match(tok);
    if (!m) return;
    const a = pick(m[1]!, false);
    const b = pick(m[1]!, true);
    if (base !== null && a !== null && a !== base) out.push(`${label} 基础: 文本 ${a} vs values ${base}`);
    if (up !== null && b !== null && b !== up) out.push(`${label} 升级: 文本 ${b} vs values ${up}`);
  };
  chk("damage", /Deal\s+(\S+)\s+damage/, card.values.damage, card.upgrade.damage);
  chk("block", /Gain\s+(\S+)\s+Block/, card.values.block, card.upgrade.block);
  return out;
}

// ---- 沙盒场景 ----
interface Spec {
  name: string;
  kind: "cards" | "potions" | "relics";
  id: string;
  level: "base" | "up";
  facts: Fact[];
  scenario: Record<string, unknown>;
  /** delta: 打完后减开局;init: 开局绝对值 */
  mode: "delta" | "init";
  /** 探针轴专用:直接读快照算出一组事实(返回错误字符串表示沙盒炸了) */
  probe?: (rows: Row[]) => Record<string, number> | string;
}

const list: Spec[] = [];
const FILLER = Array.from({ length: 10 }, () => "strike");
/** 出牌/用药的受控局面:9 点能量、手牌给定、抽牌堆塞满(抽得动才测得出抽几张) */
const playBoard = (extra: Record<string, unknown>): Record<string, unknown> => {
  const { player: playerOverride, ...rest } = extra;
  return {
    player: { hp: 40, max_hp: 80, energy: 9, max_energy: 9, ...(asRecord(playerOverride) ?? {}) },
    relics: [],
    potions: [null, null, null],
    draw: FILLER,
    discard: [],
    exhaust: [],
    enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Incantation" }],
    ...rest,
  };
};
/** 开局快照的局面:不覆盖能量/牌堆,让遗物自己生效 */
const initBoard = (extra: Record<string, unknown>): Record<string, unknown> => ({
  // 血量留缺口,开局回血类遗物才测得出
  player: { hp: 40, max_hp: 80 },
  relics: [],
  potions: [null, null, null],
  enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Incantation" }],
  ...extra,
});

/** 少数牌的数值不在文本里(等于格挡/抽牌堆数/X 费),按语义写死期望 */
type CardSpec = { facts: (card: CorpusCard, up: boolean) => Fact[]; board?: Record<string, unknown> };
const CARD_SPECIAL: Record<string, CardSpec> = {
  body_slam: { facts: () => [{ k: "damage", v: 5 }], board: { player: { block: 5 } } },
  mind_blast: {
    facts: () => [{ k: "damage", v: 7 }],
    board: { draw: ["strike", "strike", "strike", "strike", "strike", "strike", "strike"] },
  },
  whirlwind: { facts: (c, up) => [{ k: "damage", v: (up ? c.upgrade.damage! : c.values.damage!) * 3 }], board: { player: { energy: 3, max_energy: 9 } } },
  entrench: { facts: () => [{ k: "block", v: 5 }], board: { player: { block: 5 } } },
  clash: { facts: (c, up) => [{ k: "damage", v: up ? c.upgrade.damage! : c.values.damage! }] },
  // berserk 的数值是挂在自己身上的易伤,不是能力层数
  berserk: { facts: (c, up) => [{ k: "player_power", name: "vulnerable", v: up ? c.upgrade.magic! : c.values.magic! }] },
  // 手牌里 4 张非攻击牌被消耗/放逐,数值按每张算
  second_wind: { facts: (c, up) => [{ k: "block", v: (up ? c.upgrade.block! : c.values.block!) * 4 }] },
  fiend_fire: { facts: (c, up) => [{ k: "damage", v: (up ? c.upgrade.damage! : c.values.damage!) * 4 }] },
  // 敌人这回合要攻击,条件满足
  spot_weakness: {
    facts: (c, up) => [{ k: "player_power", name: "strength", v: up ? c.upgrade.magic! : c.values.magic! }],
    board: { enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Dark Strike" }] },
  },
  impatience: { facts: (c, up) => [{ k: "draw", v: up ? c.upgrade.magic! : c.values.magic! }] },
  // limit_break 把已有的力量翻倍:5 -> 10,差额 5
  limit_break: { facts: () => [{ k: "player_power", name: "strength", v: 5 }], board: { player: { powers: { strength: 5 } } } },
  // 炸弹:第三回合末结算
  the_bomb: {
    facts: (c, up) => [{ k: "damage", v: up ? c.upgrade.magic! : c.values.magic! }],
    board: { actions: [{ op: "play", hand: 0, target: 0 }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }] },
  },
};

/** 能力牌:打出后挂一条与牌同名、层数 = 语料 magic 的能力 */
const POWER_STACK: Record<string, string> = {
  demon_form: "demon_form",
  metallicize: "metallicize",
  feel_no_pain: "feel_no_pain",
  fire_breathing: "fire_breathing",
  rupture: "rupture",
  evolve: "evolve",
  combust: "combust",
  juggernaut: "juggernaut",
  panache: "panache",
  sadistic_nature: "sadistic_nature",
  magnetism: "magnetism",
  mayhem: "mayhem",
  double_tap: "double_tap",
  rage: "rage",
};

/** 条件/依赖牌堆组成,数值不能按字面直测的牌(不是我们错,是审计口径外) */
const CARD_SKIP: Record<string, string> = {
  perfected_strike: "伤害随全牌组含 Strike 的牌数变(含自身),字面 6 不是实测期望",
};

/** 除打出这张牌外,还有几张手牌会离手(消耗/置顶),抽几张要按它折算 */
const CARD_HAND_LOSS: Record<string, number> = {
  burning_pact: 1,
  warcry: 1,
  thinking_ahead: 1,
};

/**
 * 不参与自动比对的牌:文本里确有数值效果,但当前沙盒/审计口径量不出(战斗外结算、
 * 随机交互、需要重复打出或敌方时序等)。显式登记原因,避免静默跳过。
 * 这些牌仍有其它事实被比对(如格挡/伤害);这里只登记"没被比对"的那部分效果。
 */
const CARD_NOT_COMPARED: Record<string, string> = {
  parasite: "被变形/移出牌组时掉 3 最大 HP:发生在战斗外,沙盒只跑一场战斗",
  true_grit: "消耗 1 张手牌(基础随机/升级指定):只核对格挡,张数交互不比对",
  rampage: "伤害按该牌本场已打出次数递增:单次打出量不出成长",
  feed: "若致命则 +3/4 最大 HP:需致命一击,普通局面量不出",
  hand_of_greed: "若致命则 +20/25 金币:需致命一击,且金币不在战斗快照里",
  flame_barrier: "被攻击时反伤 4/6:需要同一回合内敌方先攻击,当前探针未建模敌方攻击时序",
  berserk: "每回合开始 +@RE:回合开始会重置能量,单场快照量不出增量",
  burning_pact: "消耗 1 张手牌:张数交互不比对",
  blood_for_blood: "费用按本场掉血次数递减:需要打出前的费用探针,未建模",
  // 以下为无数字的造牌/洗牌/置顶/升级/抽牌限制类效果,数值审计无法直测
  armaments: "升级手牌[a card|all cards]:升级交互不比对(仅核对格挡)",
  headbutt: "把弃牌堆一张放回抽牌堆顶:选牌交互不比对(仅核对伤害)",
  warcry: "把手牌一张放回抽牌堆顶:选牌交互不比对(仅核对抽牌)",
  sever_soul: "消耗全部非攻击手牌:手牌交互不比对(仅核对伤害)",
  anger: "把本牌副本塞进弃牌堆:造牌不比对(仅核对伤害)",
  wild_strike: "洗一张伤口进抽牌堆:造牌不比对(仅核对伤害)",
  immolate: "塞一张燃烧进弃牌堆:造牌不比对(仅核对伤害)",
  reckless_charge: "洗一张眩晕进抽牌堆:造牌不比对(仅核对伤害)",
  reaper: "回复量等于未被格挡的伤害:按战况变,不比对(仅核对伤害)",
  battle_trance: "本回合不能再抽牌:限制类不比对(仅核对抽牌数)",
  panic_button: "打出后 2 回合不能再获得格挡(NoBlock):限制类不比对(仅核对格挡)",
};

function cardScenarios(): void {
  for (const id of ourCards) {
    const c = cardByGame.get(id);
    if (!c || CARD_SKIP[id]) continue;
    for (const level of ["base", "up"] as const) {
      const up = level === "up";
      const special = CARD_SPECIAL[id];
      const powerKey = POWER_STACK[id];
      const magic = up ? c.upgrade.magic : c.values.magic;
      const facts = special
        ? special.facts(c, up)
        : powerKey && magic !== null
          ? [{ k: "player_power", name: powerKey, v: magic } as Fact]
          : cardFacts(c, up);
      if (facts.length === 0) continue;
      const token = up ? `${id}+` : id;
      const hand = id === "clash" ? [token, "strike", "strike", "strike", "strike"] : [token, "defend", "defend", "defend", "defend"];
      const extra: Record<string, unknown> = { hand, actions: [{ op: "play", hand: 0, target: 0 }] };
      if (special?.board) Object.assign(extra, special.board);
      list.push({ name: `cards/${id}/${level}`, kind: "cards", id, level, facts, scenario: playBoard(extra), mode: "delta" });
    }
  }
}

// ---- 追加轴:诅咒/状态(持有/抽到/回合末)、能力牌(多回合)、随机选牌(结果集合) ----
//
// 这 41 张牌的数值不在"当回合打一下"的口径里,分三种轴实测;期望值仍从语料
// 文本(或 values)抽,选牌类只对"结果集合与约束"(可选张数/张数/是否 0 费/
// 是否消耗/类型与颜色限制)对账,不比对随机出来的具体身份。

const tokenBase = (tok: string) => tok.replace(/\+.*$/, "");
const tokenType = (tok: string) => cardByGame.get(tokenBase(tok))?.type.toLowerCase() ?? "?";
const tokenColor = (tok: string) => cardByGame.get(tokenBase(tok))?.color?.toLowerCase() ?? "?";
const tokenUpgraded = (tok: string) => (tok.includes("+") ? 1 : 0);
const handOf = (st: StateJson) => st.hand as string[];
const firstSt = (rows: Row[]) => rows.filter((r) => r.st)[0]!.st!;
const lastSt = (rows: Row[]) => {
  const snaps = rows.filter((r) => r.st);
  return snaps[snaps.length - 1]!.st!;
};
const playRows = (rows: Row[]) => rows.filter((r) => r.op === "play" && r.st);
/** 最后一行带 report 的快照(通常是刚打完/刚回合末那一步) */
const lastReport = (rows: Row[]) => {
  const withReport = rows.filter((r) => r.report);
  return withReport.length > 0 ? withReport[withReport.length - 1]!.report! : { candidates: [], costs: [] };
};
const errOf = (rows: Row[]) => rows.find((r) => r.error)?.error ?? null;
const candsOf = (rows: Row[]) => lastReport(rows).candidates;
const lastCosts = (rows: Row[]) => lastReport(rows).costs;
const inPile = (st: StateJson, pile: "exhaust" | "draw" | "discard", tok: string) =>
  (st[pile] as string[]).includes(tok) ? 1 : 0;
/** after 里减去 before 的多重集,before 全被消掉后剩下的就是新加的牌 */
function multisetDiff(after: string[], before: string[]): string[] {
  const m = new Map<string, number>();
  for (const t of before) m.set(t, (m.get(t) ?? 0) + 1);
  const out: string[] = [];
  for (const t of after) {
    const n = m.get(t) ?? 0;
    if (n > 0) m.set(t, n - 1);
    else out.push(t);
  }
  return out;
}
/** 打出一张牌后手牌新增的牌(打出的那张从手牌移走,新增的都堆在手牌末尾) */
const addedToHand = (rows: Row[]) => {
  const before = handOf(firstSt(rows));
  const after = handOf(lastSt(rows));
  return after.slice(Math.max(0, before.length - 1));
};
/** 洗进抽牌堆的新牌(顺序会被重洗,只比多重集) */
const addedToDrawOf = (rows: Row[]) =>
  multisetDiff(lastSt(rows).draw as string[], firstSt(rows).draw as string[]);
/** 打出后最后一张手牌(选牌选中的那张)的当前费用 */
const lastHandCost = (rows: Row[]) => {
  const costs = lastCosts(rows);
  return costs.length > 0 ? costs[costs.length - 1]! : null;
};

function pushProbe(
  id: string,
  level: "base" | "up",
  suffix: string,
  facts: Fact[],
  probe: (rows: Row[]) => Record<string, number> | string,
  scenario: Record<string, unknown>,
  kind: Spec["kind"] = "cards",
): void {
  list.push({ name: `${kind}/${id}/${suffix}`, kind, id, level, facts, probe, scenario, mode: "delta" });
}

/** 诅咒/状态:持有(不可打出/手牌限制/掉血)、抽到(掉能量)、回合末(掉血/状态/回手)、天生 */
function curseStatusScenarios(): void {
  for (const id of ourCards) {
    const c = cardByGame.get(id);
    if (!c || !["curse", "status"].includes(c.type.toLowerCase())) continue;
    const t = norm(c.text).replace(/\s+/g, " ").trim();
    const unplayable = /Unplayable\./.test(t);

    if (unplayable) {
      pushProbe(
        id,
        "base",
        "unplayable",
        [{ k: "probe", key: "play_error", v: 1 }],
        (rows) => ({ play_error: errOf(rows) ? 1 : 0 }),
        playBoard({ report: true, hand: [id, "defend"], actions: [{ op: "play", hand: 0, target: 0 }] }),
      );
    }
    if (/Ethereal\./.test(t)) {
      pushProbe(
        id,
        "base",
        "ethereal",
        [{ k: "probe", key: "in_exhaust", v: 1 }],
        (rows) => ({ in_exhaust: inPile(lastSt(rows), "exhaust", id) }),
        playBoard({ hand: [id, "defend"], actions: [{ op: "noop" }, { op: "end_turn" }] }),
      );
    }
    const dmg = t.match(/At the end of your turn, take (\d+) damage/);
    if (dmg) {
      pushProbe(
        id,
        "base",
        "end_turn_hp",
        [{ k: "probe", key: "hp_loss", v: Number(dmg[1]) }],
        (rows) => ({ hp_loss: firstSt(rows).player.hp - lastSt(rows).player.hp }),
        playBoard({ hand: [id, "defend"], actions: [{ op: "noop" }, { op: "end_turn" }] }),
      );
    }
    const pw = t.match(/At the end of your turn, gain (\d+) (Weak|Frail)/);
    if (pw) {
      pushProbe(
        id,
        "base",
        "end_turn_power",
        [{ k: "probe", key: "gain", v: Number(pw[1]) }],
        (rows) => ({ gain: lastSt(rows).player.powers[POWER_KEY[pw[2]!]!] ?? 0 }),
        playBoard({ hand: [id, "defend"], actions: [{ op: "noop" }, { op: "end_turn" }] }),
      );
    }
    if (/At the end of your turn, lose HP equal to the number of cards in your hand/.test(t)) {
      const hand = [id, "defend", "strike"];
      pushProbe(
        id,
        "base",
        "end_turn_regret",
        [{ k: "probe", key: "hp_loss", v: hand.length }],
        (rows) => ({ hp_loss: firstSt(rows).player.hp - lastSt(rows).player.hp }),
        playBoard({ hand, actions: [{ op: "noop" }, { op: "end_turn" }] }),
      );
    }
    const drawn = t.match(/Whenever this card is drawn, lose (\d+) Energy/);
    if (drawn) {
      pushProbe(
        id,
        "base",
        "on_draw",
        [{ k: "probe", key: "energy_loss", v: Number(drawn[1]) }],
        // 减去 pommel_strike 自己的 1 费,剩下的就是抽到这张牌扣的
        (rows) => ({ energy_loss: firstSt(rows).energy - lastSt(rows).energy - 1 }),
        playBoard({
          hand: ["pommel_strike", "defend", "strike"],
          draw: [id, "strike", "strike", "strike"],
          actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
        }),
      );
    }
    const limit = t.match(/While in hand, you cannot play more than (\d+) cards this turn/);
    if (limit) {
      const max = Number(limit[1]);
      pushProbe(
        id,
        "base",
        "play_limit",
        [{ k: "probe", key: "played_ok", v: max }, { k: "probe", key: "blocked", v: 1 }],
        (rows) => ({ played_ok: playRows(rows).length, blocked: errOf(rows) ? 1 : 0 }),
        playBoard({
          report: true,
          hand: [id, ...Array.from({ length: max + 1 }, () => "strike")],
          actions: [
            { op: "noop" },
            ...Array.from({ length: max + 1 }, () => ({ op: "play", hand: 1, target: 0 })),
          ],
        }),
      );
    }
    const pain = t.match(/While in hand, lose (\d+) HP when other cards are played/);
    if (pain) {
      const per = Number(pain[1]);
      pushProbe(
        id,
        "base",
        "in_hand_pain",
        [{ k: "probe", key: "hp_loss", v: per * 2 }],
        (rows) => ({ hp_loss: firstSt(rows).player.hp - lastSt(rows).player.hp }),
        playBoard({
          hand: [id, "strike", "defend"],
          actions: [{ op: "noop" }, { op: "play", hand: 1, target: 0 }, { op: "play", hand: 1, target: 0 }],
        }),
      );
    }
    if (/There is no escape from this curse/.test(t)) {
      pushProbe(
        id,
        "base",
        "returns_on_exhaust",
        [{ k: "probe", key: "back_in_hand", v: 1 }],
        (rows) => ({ back_in_hand: handOf(lastSt(rows)).includes(id) ? 1 : 0 }),
        playBoard({
          hand: [id, "purity", "strike", "defend", "strike"],
          actions: [{ op: "noop" }, { op: "play", hand: 1, target: 0, choose: [0, 0, 0] }],
        }),
      );
    }
    if (/Innate\./.test(t)) {
      pushProbe(
        id,
        "base",
        "innate",
        [{ k: "probe", key: "in_opening_hand", v: 1 }],
        (rows) => ({ in_opening_hand: handOf(firstSt(rows)).includes(id) ? 1 : 0 }),
        {
          player: { hp: 40, max_hp: 80, energy: 5, max_energy: 5 },
          relics: [],
          potions: [null, null, null],
          deck: [...Array.from({ length: 9 }, () => "strike"), id],
          enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Incantation" }],
          actions: [{ op: "noop" }],
        },
      );
    }
    if (/put a copy of this card on top of your draw pile/.test(t)) {
      pushProbe(
        id,
        "base",
        "copy_on_end_turn",
        [{ k: "probe", key: "in_hand_next_turn", v: 1 }],
        (rows) => ({ in_hand_next_turn: handOf(lastSt(rows)).includes(id) ? 1 : 0 }),
        playBoard({
          hand: [id, "defend", "strike"],
          actions: [{ op: "noop" }, { op: "end_turn" }],
        }),
      );
    }
    // 可打出且自带 Exhaust.(史莱姆粘液 / 傲慢):打出后进消耗堆
    if (!unplayable && /\bExhaust\./.test(t)) {
      pushProbe(
        id,
        "base",
        "exhaust_on_play",
        [{ k: "probe", key: "self_exhausted", v: 1 }],
        (rows) => ({ self_exhausted: inPile(lastSt(rows), "exhaust", id) }),
        playBoard({ hand: [id, "defend"], actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }] }),
      );
    }
  }
}

/** 能力牌:多回合滚动(每回合抽牌/加力量/格挡不清空/技能 0 费且消耗) */
function powerScenarios(): void {
  const darkEmbrace = (rows: Row[]) => {
    const p = playRows(rows);
    const before = handOf(p[0]!.st!).length;
    const after = handOf(p[1]!.st!).length;
    return { draw_on_exhaust: after - (before - 1) };
  };
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "dark_embrace+" : "dark_embrace";
    pushProbe(
      "dark_embrace",
      level,
      level,
      [{ k: "probe", key: "draw_on_exhaust", v: 1 }],
      darkEmbrace,
      playBoard({
        hand: [tok, "limit_break", "defend", "defend", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "barricade+" : "barricade";
    pushProbe(
      "barricade",
      level,
      level,
      [{ k: "probe", key: "block_kept", v: 5 }],
      (rows) => ({ block_kept: lastSt(rows).player.block }),
      playBoard({
        hand: [tok, "defend", "strike", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
      }),
    );
  }
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "brutality+" : "brutality";
    pushProbe(
      "brutality",
      level,
      level,
      [{ k: "probe", key: "hp_loss", v: 1 }, { k: "probe", key: "extra_draw", v: 1 }],
      (rows) => ({
        hp_loss: firstSt(rows).player.hp - lastSt(rows).player.hp,
        extra_draw: handOf(lastSt(rows)).length - 5,
      }),
      playBoard({
        hand: [tok, "defend", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
      }),
    );
  }
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "corruption+" : "corruption";
    pushProbe(
      "corruption",
      level,
      level,
      [{ k: "probe", key: "skill_free", v: 1 }, { k: "probe", key: "skill_exhausted", v: 1 }],
      (rows) => {
        const p = playRows(rows);
        const e0 = p[0]!.st!.energy;
        const e1 = p[1]!.st!.energy;
        return { skill_free: e0 - e1 === 0 ? 1 : 0, skill_exhausted: inPile(p[1]!.st!, "exhaust", "defend") };
      },
      playBoard({
        hand: [tok, "defend", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
}

/** 随机/选牌类:只对结果集合与约束对账(候选张数/加几张/0 费/消耗/类型与颜色限制) */
function choiceScenarios(): void {
  // 发现:亮 3 张选 1 张,选中的本回合 0 费;升级后不再消耗
  for (const level of ["base", "up"] as const) {
    const up = level === "up";
    const tok = up ? "discovery+" : "discovery";
    pushProbe(
      "discovery",
      level,
      level,
      [
        { k: "probe", key: "options", v: 3 },
        { k: "probe", key: "added", v: 1 },
        { k: "probe", key: "added_free", v: 1 },
        { k: "probe", key: "self_exhausted", v: up ? 0 : 1 },
      ],
      (rows) => ({
        options: candsOf(rows).length,
        added: addedToHand(rows).length,
        added_free: addedToHand(rows).length === 1 && lastHandCost(rows) === 0 ? 1 : 0,
        self_exhausted: inPile(lastSt(rows), "exhaust", tok),
      }),
      playBoard({
        report: true,
        hand: [tok, "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [1] }],
      }),
    );
  }
  // 双重施法:选一张攻击/能力,加 1 份(升级 2 份);技能不能被选.
  // 手里摆两张攻击:候选只剩一张时原版会直接结算、不开屏(ChoiceMode::Mandatory),
  // 那样 options 就看不到候选池了,所以留两张攻击 + 一张技能来验过滤.
  for (const level of ["base", "up"] as const) {
    const up = level === "up";
    const tok = up ? "dual_wield+" : "dual_wield";
    const strikesIn = (xs: string[]) => xs.filter((x) => tokenBase(x) === "strike").length;
    pushProbe(
      "dual_wield",
      level,
      level,
      [
        { k: "probe", key: "options", v: 2 },
        { k: "probe", key: "copies", v: up ? 2 : 1 },
        { k: "probe", key: "skill_offered", v: 0 },
      ],
      (rows) => ({
        options: candsOf(rows).length,
        copies: strikesIn(handOf(lastSt(rows))) - strikesIn(handOf(firstSt(rows))),
        skill_offered: candsOf(rows).some((x) => tokenType(x) === "skill") ? 1 : 0,
      }),
      playBoard({
        report: true,
        hand: [tok, "strike", "strike", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }],
      }),
    );
  }
  // 地狱之刃:随机加 1 张攻击,本回合 0 费,消耗
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "infernal_blade+" : "infernal_blade";
    pushProbe(
      "infernal_blade",
      level,
      level,
      [
        { k: "probe", key: "added", v: 1 },
        { k: "probe", key: "added_is_attack", v: 1 },
        { k: "probe", key: "added_free", v: 1 },
        { k: "probe", key: "self_exhausted", v: 1 },
      ],
      (rows) => {
        const added = addedToHand(rows);
        return {
          added: added.length,
          added_is_attack: added.length === 1 && tokenType(added[0]!) === "attack" ? 1 : 0,
          added_free: added.length === 1 && lastHandCost(rows) === 0 ? 1 : 0,
          self_exhausted: inPile(lastSt(rows), "exhaust", tok),
        };
      },
      playBoard({
        report: true,
        hand: [tok, "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 万事通:随机加 1(升级 2)张无色牌,消耗
  for (const level of ["base", "up"] as const) {
    const up = level === "up";
    const tok = up ? "jack_of_all_trades+" : "jack_of_all_trades";
    pushProbe(
      "jack_of_all_trades",
      level,
      level,
      [
        { k: "probe", key: "added", v: up ? 2 : 1 },
        { k: "probe", key: "all_colorless", v: 1 },
        { k: "probe", key: "self_exhausted", v: 1 },
      ],
      (rows) => {
        const added = addedToHand(rows);
        return {
          added: added.length,
          all_colorless: added.length > 0 && added.every((x) => tokenColor(x) === "colorless") ? 1 : 0,
          self_exhausted: inPile(lastSt(rows), "exhaust", tok),
        };
      },
      playBoard({
        report: true,
        hand: [tok, "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 嬗变:X 张随机无色牌,本回合 0 费;升级后给的是升级版
  for (const level of ["base", "up"] as const) {
    const up = level === "up";
    const tok = up ? "transmutation+" : "transmutation";
    pushProbe(
      "transmutation",
      level,
      level,
      [
        { k: "probe", key: "added", v: 3 },
        { k: "probe", key: "all_colorless", v: 1 },
        { k: "probe", key: "all_free", v: 1 },
        { k: "probe", key: "all_upgraded", v: up ? 1 : 0 },
        { k: "probe", key: "self_exhausted", v: 1 },
      ],
      (rows) => {
        const added = addedToHand(rows);
        const costs = lastCosts(rows);
        const addedCosts = costs.slice(Math.max(0, costs.length - added.length));
        return {
          added: added.length,
          all_colorless: added.length > 0 && added.every((x) => tokenColor(x) === "colorless") ? 1 : 0,
          all_free: added.length > 0 && addedCosts.every((x) => x === 0) ? 1 : 0,
          all_upgraded: added.length > 0 && added.every(tokenUpgraded) ? 1 : 0,
          self_exhausted: inPile(lastSt(rows), "exhaust", tok),
        };
      },
      playBoard({
        report: true,
        player: { energy: 3, max_energy: 9 },
        hand: [tok, "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 暴力:从抽牌堆抓 3(升级 4)张随机攻击,消耗
  for (const level of ["base", "up"] as const) {
    const up = level === "up";
    const tok = up ? "violence+" : "violence";
    pushProbe(
      "violence",
      level,
      level,
      [
        { k: "probe", key: "added", v: up ? 4 : 3 },
        { k: "probe", key: "all_attacks", v: 1 },
        { k: "probe", key: "self_exhausted", v: 1 },
      ],
      (rows) => {
        const added = addedToHand(rows);
        return {
          added: added.length,
          all_attacks: added.length > 0 && added.every((x) => tokenType(x) === "attack") ? 1 : 0,
          self_exhausted: inPile(lastSt(rows), "exhaust", tok),
        };
      },
      playBoard({
        report: true,
        hand: [tok, "defend"],
        draw: ["strike", "strike", "strike", "strike", "bash", "cleave"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 秘密技巧/秘密武器:从抽牌堆抓 1 张技能/攻击;升级后不再消耗
  for (const [id, kindTok] of [["secret_technique", "skill"], ["secret_weapon", "attack"]] as const) {
    for (const level of ["base", "up"] as const) {
      const up = level === "up";
      const tok = up ? `${id}+` : id;
      pushProbe(
        id,
        level,
        level,
        [
          { k: "probe", key: "added", v: 1 },
          { k: "probe", key: `added_is_${kindTok}`, v: 1 },
          { k: "probe", key: "self_exhausted", v: up ? 0 : 1 },
        ],
        (rows) => {
          const added = addedToHand(rows);
          return {
            added: added.length,
            [`added_is_${kindTok}`]: added.length === 1 && tokenType(added[0]!) === kindTok ? 1 : 0,
            self_exhausted: inPile(lastSt(rows), "exhaust", tok),
          };
        },
        playBoard({
          report: true,
          hand: [tok, "defend"],
          draw: ["defend", "strike", "bash", "defend"],
          actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }],
        }),
      );
    }
  }
  // 浩劫:打出抽牌堆顶那张并消耗它;浩劫自己不消耗
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "havoc+" : "havoc";
    pushProbe(
      "havoc",
      level,
      level,
      [{ k: "probe", key: "top_exhausted", v: 1 }, { k: "probe", key: "self_exhausted", v: 0 }],
      (rows) => ({
        top_exhausted: inPile(lastSt(rows), "exhaust", "strike"),
        self_exhausted: inPile(lastSt(rows), "exhaust", tok),
      }),
      playBoard({
        hand: [tok, "defend"],
        draw: ["strike", "strike", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 茧/变形:洗 3(升级 5)张随机技能/攻击进抽牌堆,本场 0 费,消耗
  for (const [id, kindTok] of [["chrysalis", "skill"], ["metamorphosis", "attack"]] as const) {
    for (const level of ["base", "up"] as const) {
      const up = level === "up";
      const tok = up ? `${id}+` : id;
      pushProbe(
        id,
        level,
        level,
        [
          { k: "probe", key: "shuffled", v: up ? 5 : 3 },
          { k: "probe", key: `all_${kindTok}s`, v: 1 },
          { k: "probe", key: "self_exhausted", v: 1 },
        ],
        (rows) => {
          const added = addedToDrawOf(rows);
          return {
            shuffled: added.length,
            [`all_${kindTok}s`]: added.length > 0 && added.every((x) => tokenType(x) === kindTok) ? 1 : 0,
            self_exhausted: inPile(lastSt(rows), "exhaust", tok),
          };
        },
        playBoard({
          report: true,
          hand: [tok, "defend"],
          draw: FILLER,
          actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
        }),
      );
    }
  }
  // 预谋:把手牌放到抽牌堆底,0 费直到被打出;升级可放任意张
  {
    pushProbe(
      "forethought",
      "base",
      "base",
      [{ k: "probe", key: "moved", v: 1 }, { k: "probe", key: "at_bottom", v: 1 }],
      (rows) => {
        const moved = addedToDrawOf(rows);
        const draw = lastSt(rows).draw as string[];
        return { moved: moved.length, at_bottom: moved.length > 0 && draw[draw.length - 1] === moved[0] ? 1 : 0 };
      },
      playBoard({
        report: true,
        hand: ["forethought", "bash", "defend"],
        draw: ["strike", "strike", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }],
      }),
    );
    pushProbe(
      "forethought",
      "base",
      "cost0",
      [{ k: "probe", key: "cost0_in_hand", v: 1 }],
      (rows) => {
        const costs = lastCosts(rows);
        const hand = handOf(lastSt(rows));
        const i = hand.indexOf("bash");
        return { cost0_in_hand: i >= 0 && costs[i] === 0 ? 1 : 0 };
      },
      playBoard({
        report: true,
        hand: ["forethought", "bash"],
        draw: [],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }, { op: "end_turn" }],
      }),
    );
    pushProbe(
      "forethought",
      "up",
      "up",
      [{ k: "probe", key: "moved", v: 2 }],
      (rows) => ({ moved: addedToDrawOf(rows).length }),
      playBoard({
        report: true,
        hand: ["forethought+", "bash", "defend", "strike"],
        draw: ["strike", "strike", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0, 0] }],
      }),
    );
  }
  // 启蒙:手牌费用降到 1
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "enlightenment+" : "enlightenment";
    pushProbe(
      "enlightenment",
      level,
      level,
      [{ k: "probe", key: "capped", v: 1 }],
      (rows) => {
        const costs = lastCosts(rows);
        return { capped: costs.length > 0 && costs.every((x) => x === 1) ? 1 : 0 };
      },
      playBoard({
        report: true,
        player: { energy: 3, max_energy: 9 },
        hand: [tok, "bash", "bludgeon"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 疯狂:随机一张手牌费用变 0(本场),消耗
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "madness+" : "madness";
    pushProbe(
      "madness",
      level,
      level,
      [{ k: "probe", key: "zeroed", v: 1 }, { k: "probe", key: "self_exhausted", v: 1 }],
      (rows) => {
        const costs = lastCosts(rows);
        return { zeroed: costs.filter((x) => x === 0).length, self_exhausted: inPile(lastSt(rows), "exhaust", tok) };
      },
      playBoard({
        report: true,
        hand: [tok, "bash", "bludgeon"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 净化:最多消耗 3(升级 5)张手牌,自身也消耗
  {
    pushProbe(
      "purity",
      "base",
      "base",
      [{ k: "probe", key: "exhausted", v: 3 }],
      (rows) => ({ exhausted: (lastSt(rows).exhaust as string[]).length - 1 }),
      playBoard({
        report: true,
        hand: ["purity", "strike", "defend", "bash", "wound"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0, 0, 0] }],
      }),
    );
    pushProbe(
      "purity",
      "up",
      "up",
      [{ k: "probe", key: "exhausted", v: 5 }],
      (rows) => ({ exhausted: (lastSt(rows).exhaust as string[]).length - 1 }),
      playBoard({
        report: true,
        hand: ["purity+", "strike", "defend", "bash", "wound", "strike", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0, 0, 0, 0, 0] }],
      }),
    );
  }
  // 神化:本场所有牌升级,自身消耗
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "apotheosis+" : "apotheosis";
    pushProbe(
      "apotheosis",
      level,
      level,
      [{ k: "probe", key: "all_upgraded", v: 1 }],
      (rows) => {
        const st = lastSt(rows);
        const toks = [...handOf(st), ...(st.draw as string[]), ...(st.discard as string[])];
        return { all_upgraded: toks.length > 0 && toks.every(tokenUpgraded) ? 1 : 0 };
      },
      playBoard({
        hand: [tok, "strike", "defend", "bash"],
        draw: ["strike", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 完美打击:6 + 每张含 "Strike" 的牌 ×2/×3(受控牌组:1 张自身 + 2 张打击)
  for (const level of ["base", "up"] as const) {
    const c = cardByGame.get("perfected_strike")!;
    const up = level === "up";
    const tok = up ? "perfected_strike+" : "perfected_strike";
    const strikes = 3; // 牌组 = 完美打击 + 2 张 strike + 3 张 defend,含 "Strike" 的共 3 张
    const dmg = c.values.damage! + (up ? c.upgrade.magic! : c.values.magic!) * strikes;
    pushProbe(
      "perfected_strike",
      level,
      level,
      [{ k: "probe", key: "damage", v: dmg }],
      (rows) => ({ damage: firstSt(rows).enemies[0]!.hp - lastSt(rows).enemies[0]!.hp }),
      playBoard({
        hand: [tok],
        draw: ["strike", "strike", "defend", "defend", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 发掘:从消耗堆拿一张回手;不能把消耗掉的发掘自己拿回来(原版限制)
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "exhume+" : "exhume";
    pushProbe(
      "exhume",
      level,
      level,
      [
        { k: "probe", key: "options", v: 2 },
        { k: "probe", key: "offered_exhume", v: 0 },
        { k: "probe", key: "took_bash", v: 1 },
        { k: "probe", key: "self_exhausted", v: 1 },
      ],
      (rows) => {
        const cands = candsOf(rows);
        return {
          options: cands.length,
          offered_exhume: cands.some((x) => tokenBase(x) === "exhume") ? 1 : 0,
          took_bash: handOf(lastSt(rows)).includes("bash") ? 1 : 0,
          self_exhausted: inPile(lastSt(rows), "exhaust", tok),
        };
      },
      // 消耗堆里留两张别的牌:候选只剩一张时原版会直接结算、不开屏
      // (ChoiceMode::Mandatory),那样 options 就看不到候选池里的"掘出自己"排除了.
      playBoard({
        report: true,
        hand: [tok, "defend"],
        exhaust: ["exhume", "bash", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }],
      }),
    );
  }
}

/**
 * 条件/触发式效果的探针:数值要靠额外触发(被消耗/敌人带易伤/回合末/造牌)才量得出,
 * "打一下"的口径测不到,单独摆局面实测。期望值仍从语料文本抽。
 */
function conditionalScenarios(): void {
  // 哨卫:被消耗时回能 [@RE@RE|@RE@RE@RE]。用净化(0 费、指定消耗)把它消耗掉,读能量净增。
  for (const level of ["base", "up"] as const) {
    const c = cardByGame.get("sentinel")!;
    const up = level === "up";
    const tok = up ? "sentinel+" : "sentinel";
    const line = norm(c.text).split("\n").find((l) => /@RE/.test(l))!;
    const gain = reCount(line.match(/(\[@RE[^\]]*\]|@RE\S*)/)![1]!, up);
    pushProbe(
      "sentinel",
      level,
      `${level}_exhaust_energy`,
      [{ k: "probe", key: "energy_gain", v: gain }],
      (rows) => ({ energy_gain: lastSt(rows).energy - firstSt(rows).energy }),
      playBoard({
        hand: ["purity", tok],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0, choose: [0] }],
      }),
    );
  }
  // 冲刺踢:敌人带易伤时回 1 能并抽 1 张。摆好易伤,读能量净增(1-费用)与抽牌数。
  for (const level of ["base", "up"] as const) {
    const c = cardByGame.get("dropkick")!;
    const up = level === "up";
    const tok = up ? "dropkick+" : "dropkick";
    const cost = (up ? c.upgrade.cost : c.cost) ?? 0;
    pushProbe(
      "dropkick",
      level,
      `${level}_vulnerable_bonus`,
      [{ k: "probe", key: "energy_gain", v: 1 - cost }, { k: "probe", key: "drawn", v: 1 }],
      (rows) => ({
        energy_gain: lastSt(rows).energy - firstSt(rows).energy,
        drawn: lastSt(rows).hand.length - (firstSt(rows).hand.length - 1),
      }),
      playBoard({
        hand: [tok, "defend"],
        enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Incantation", powers: { vulnerable: 2 } }],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 屈伸:本回合 +N 力量,回合末收回。打完后过一回合,力量应回到 0。
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "flex+" : "flex";
    pushProbe(
      "flex",
      level,
      `${level}_temporary_strength`,
      [{ k: "probe", key: "strength_after_end_turn", v: 0 }],
      (rows) => ({ strength_after_end_turn: lastSt(rows).player.powers.strength ?? 0 }),
      playBoard({
        hand: [tok, "defend", "strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }],
      }),
    );
  }
  // 力劈华山:往手里塞 2 张伤口。打完手牌 = 原手牌 -1 + 伤口数。
  {
    const c = cardByGame.get("power_through")!;
    const wounds = Number(norm(c.text).match(/Add (\d+)/)![1]);
    pushProbe(
      "power_through",
      "base",
      "add_wounds",
      [{ k: "probe", key: "hand", v: wounds + 1 }],
      (rows) => ({ hand: lastSt(rows).hand.length }),
      playBoard({
        hand: ["power_through", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
}

/**
 * 历史盲区回归场景:这些效果此前靠人肉发现、工具当时抓不到,现在逐个补成可实测的
 * 探针(期望值仍从语料文本或语料 values 抽),并由 `--selftest` 固化成回归用例。
 * 每个场景对应报告"口径覆盖"里的一类历史漏检:条件效果、状态/诅咒、目标变更、
 * 数值在别的机制里相乘、X 费例外、升级差异、作用范围、遗物文案数值。
 */
function blindSpotScenarios(): void {
  // 吐火:抽到状态牌或诅咒牌都打全体(语料 "Status or Curse"),此前实现只认状态牌。
  // 期望直接读语料:"Curse"/"Status" 任一被从文本里去掉,对应的期望就变 0,守卫自检时
  // 改坏语料这一侧就能看到工具报错。战斗冥思一次抽 3 张,命中几张就打几个 magic。
  for (const level of ["base", "up"] as const) {
    const c = cardByGame.get("fire_breathing")!;
    const magic = level === "up" ? c.upgrade.magic! : c.values.magic!;
    const t = norm(c.text);
    const draws: [string, string[], boolean][] = [
      ["curse_draw", ["decay", "decay", "decay"], /Curse/.test(t)],
      ["status_draw", ["wound", "wound", "wound"], /Status/.test(t)],
    ];
    for (const [suffix, pile, fires] of draws) {
      pushProbe(
        "fire_breathing",
        level,
        `${level}_${suffix}`,
        [{ k: "probe", key: "damage", v: fires ? magic * pile.length : 0 }],
        (rows) => ({ damage: firstSt(rows).enemies[0]!.hp - lastSt(rows).enemies[0]!.hp }),
        playBoard({
          player: { powers: { fire_breathing: magic } },
          hand: ["battle_trance", "defend"],
          draw: pile,
          actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
        }),
      );
    }
  }
  // 致盲/绊倒:基础只打单个目标,升级后打全体(升级感知目标)。两个敌人各看一层;
  // 升级是否打全体直接读语料 "[...| to ALL enemies]" 那半句,改坏语料就会被抓到。
  for (const [id, power] of [
    ["blind", "weak"],
    ["trip", "vulnerable"],
  ] as const) {
    const c = cardByGame.get(id)!;
    const allOnUpgrade = /\[\s*\|[^\]]*ALL enemies/.test(norm(c.text));
    for (const level of ["base", "up"] as const) {
      const up = level === "up";
      const tok = up ? `${id}+` : id;
      const n = up ? c.upgrade.magic! : c.values.magic!;
      pushProbe(
        id,
        level,
        `${level}_target_count`,
        [
          { k: "probe", key: "target_hit", v: n },
          { k: "probe", key: "other_hit", v: up && allOnUpgrade ? 1 : 0 },
        ],
        (rows) => {
          const es = lastSt(rows).enemies;
          return { target_hit: es[0]!.powers[power] ?? 0, other_hit: (es[1]!.powers[power] ?? 0) > 0 ? 1 : 0 };
        },
        playBoard({
          hand: [tok, "defend"],
          enemies: [
            { id: "cultist", hp: 999, max_hp: 999, move: "Incantation" },
            { id: "cultist", hp: 999, max_hp: 999, move: "Incantation" },
          ],
          actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
        }),
      );
    }
  }
  // 神化:只升"打出那一刻"的四个牌堆,之后再造出来的牌不升(反编译 ApotheosisAction)。
  // 用献祭造一张燃烧进弃牌堆,燃烧必须是基础版(若实现连后续新牌也升,会看到 burn+)。
  for (const level of ["base", "up"] as const) {
    const tok = level === "up" ? "apotheosis+" : "apotheosis";
    pushProbe(
      "apotheosis",
      level,
      `${level}_not_later_cards`,
      [
        { k: "probe", key: "burn_base", v: 1 },
        { k: "probe", key: "burn_up", v: 0 },
      ],
      (rows) => {
        const d = lastSt(rows).discard as string[];
        return {
          burn_base: d.filter((t) => tokenBase(t) === "burn" && !t.includes("+")).length,
          burn_up: d.filter((t) => tokenBase(t) === "burn" && t.includes("+")).length,
        };
      },
      playBoard({
        hand: [tok, "immolate"],
        draw: ["defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 重击:伤害在别的机制里相乘 —— 吃力量 3 次(升级 5 次)。摆 10 点力量按语料 values 算期望。
  for (const level of ["base", "up"] as const) {
    const c = cardByGame.get("heavy_blade")!;
    const up = level === "up";
    const str = 10;
    const mult = up ? c.upgrade.magic! : c.values.magic!;
    const dmg = (up ? c.upgrade.damage! : c.values.damage!) + mult * str;
    pushProbe(
      "heavy_blade",
      level,
      `${level}_strength_multiplier`,
      [{ k: "probe", key: "damage", v: dmg }],
      (rows) => ({ damage: firstSt(rows).enemies[0]!.hp - lastSt(rows).enemies[0]!.hp }),
      playBoard({
        player: { powers: { strength: str } },
        hand: [up ? "heavy_blade+" : "heavy_blade", "defend"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
    );
  }
  // 笔尖:每第 10 张攻击翻倍,数值在遗物机制里相乘。第 10 张故意用狂暴(DamageWithBonus)
  // —— 历史 bug 是翻倍只写在 Effect::Damage/DamageAll 分支里,狂暴这类攻击效果漏翻倍。
  // 9 张打击 + 第 10 张狂暴,期望 = 9*打击伤害 + 2*狂暴伤害(都从语料 values 抽,改语料就能看到)。
  {
    const strikeDmg = cardByGame.get("strike")!.values.damage!;
    const rampageDmg = cardByGame.get("rampage")!.values.damage!;
    pushProbe(
      "pen_nib",
      "base",
      "doubles_tenth_attack_effect",
      [{ k: "probe", key: "damage", v: 9 * strikeDmg + 2 * rampageDmg }],
      (rows) => ({ damage: firstSt(rows).enemies[0]!.hp - lastSt(rows).enemies[0]!.hp }),
      playBoard({
        relics: ["pen_nib"],
        player: { energy: 10, max_energy: 10 },
        hand: [...Array.from({ length: 9 }, () => "strike"), "rampage"],
        actions: [
          { op: "noop" },
          ...Array.from({ length: 9 }, () => ({ op: "play", hand: 0, target: 0 })),
          { op: "play", hand: 0, target: 0 },
        ],
      }),
      "relics",
    );
  }
  // 靴子:遗物文案 "4 or less unblocked → 5"(阈值直接读语料文案数字)。虚弱打击 6→4,抬到 5。
  {
    const bootText = norm(relicByGame.get("the_boot")!.text);
    const floor = Number(bootText.match(/increase it to (\d+)/)![1]);
    pushProbe(
      "the_boot",
      "base",
      "raises_low_hit_to_5",
      [{ k: "probe", key: "damage", v: floor }],
      (rows) => ({ damage: firstSt(rows).enemies[0]!.hp - lastSt(rows).enemies[0]!.hp }),
      playBoard({
        relics: ["the_boot"],
        player: { powers: { weak: 10 } },
        hand: ["strike"],
        actions: [{ op: "noop" }, { op: "play", hand: 0, target: 0 }],
      }),
      "relics",
    );
  }
  // 液态记忆:取回的牌本回合 0 费,但 X 费牌除外(反编译里 X 费的 cost 不走这条)。
  pushProbe(
    "liquid_memories",
    "base",
    "returns_card_free",
    [
      { k: "probe", key: "returned", v: 1 },
      { k: "probe", key: "cost", v: 0 },
    ],
    (rows) => ({
      returned: handOf(lastSt(rows)).includes("strike") ? 1 : 0,
      cost: lastHandCost(rows) ?? -99,
    }),
    playBoard({
      report: true,
      hand: ["defend"],
      discard: ["strike"],
      potions: ["liquid_memories", null, null],
      actions: [{ op: "noop" }, { op: "potion", slot: 0, target: null, choose: [0] }],
    }),
    "potions",
  );
  pushProbe(
    "liquid_memories",
    "base",
    "keeps_x_cost",
    [{ k: "probe", key: "cost", v: -1 }],
    (rows) => ({ cost: lastHandCost(rows) ?? -99 }),
    playBoard({
      report: true,
      hand: ["defend"],
      discard: ["whirlwind"],
      potions: ["liquid_memories", null, null],
      actions: [{ op: "noop" }, { op: "potion", slot: 0, target: null, choose: [0] }],
    }),
    "potions",
  );
  // 混乱:随机化后的费用必须落在 0..3,升级降费的牌也不能被再减一档(havoc+ 当前 0 费,
  // 若拿牌面基础费 1 当基线,掷出的值会被减成 -1)。蛇眼每回合抽牌都随机化,多抽几轮
  // 凑够 30+ 个样本(固定种子下即确定性输出)。
  pushProbe(
    "snecko_eye",
    "base",
    "confusion_cost_range",
    [
      { k: "probe", key: "min_cost", v: 0 },
      { k: "probe", key: "max_cost", v: 3 },
    ],
    (rows) => {
      const cs = rows.flatMap((r) => r.report?.costs ?? []);
      return { min_cost: Math.min(...cs), max_cost: Math.max(...cs) };
    },
    {
      player: { hp: 200, max_hp: 200, energy: 9, max_energy: 9 },
      relics: ["snecko_eye"],
      potions: [null, null, null],
      deck: Array.from({ length: 40 }, () => "havoc+"),
      enemies: [{ id: "cultist", hp: 999, max_hp: 999, move: "Incantation" }],
      report: true,
      actions: [
        { op: "noop" },
        ...Array.from({ length: 6 }, () => ({ op: "end_turn" as const })),
      ],
    },
    "relics",
  );
}

function potionFacts(p: CorpusPotion, mul: number): Fact[] {
  const facts: Fact[] = [];
  const t = norm(p.text);
  const base = p.potency?.base ?? null;
  if (base === null) return facts;
  const pot = base * mul;
  let m: RegExpMatchArray | null;
  if (t.match(/^Gain\s+\S+\s+Block/)) facts.push({ k: "block", v: pot });
  else if (t.match(/^Deal\s+\S+\s+damage/)) facts.push({ k: "damage", v: pot });
  else if ((m = t.match(/^Apply\s+\S+\s+(Vulnerable|Weak|Frail|Poison)/))) facts.push({ k: "enemy_power", name: POWER_KEY[m[1]!]!, v: pot });
  else if ((m = t.match(/^Gain\s+\S+\s+(Strength|Dexterity|Artifact|Thorns|Metallicize|Plated Armor|Regeneration|Intangible|Ritual|Focus)/)))
    facts.push({ k: "player_power", name: POWER_KEY[m[1]!]!, v: pot });
  else if (/Gain \S+ Energy/.test(t)) facts.push({ k: "energy", v: pot });
  else if (/^Heal for/.test(t)) facts.push({ k: "self_hp", v: Math.floor((80 * pot) / 100) });
  else if (/^Draw/.test(t)) facts.push({ k: "draw", v: pot });
  return facts;
}

/**
 * 不参与自动比对的药水:文本里没有可当次用药直测的数值效果(随机池、选牌、姿态、
 * 战斗外结算等)。显式登记原因,避免静默跳过。
 */
const POTION_NOT_COMPARED: Record<string, string> = {
  ambrosia: "进入神性(姿态):姿态系统不在本口径",
  attack_potion: "三选一随机攻击牌并加入手牌:随机池/选牌交互",
  blessing_of_the_forge: "升级整手牌:无当次用药的稳定数值",
  bottled_miracle: "加入 2/4 张奇迹牌:手牌构成交互",
  colorless_potion: "三选一随机无色牌:随机池/选牌交互",
  cunning_potion: "加入 3/6 张 Shivs+:手牌构成交互",
  distilled_chaos: "打出抽牌堆顶 3/6 张:随机/连锁结算",
  duplication_potion: "下 1/2 张牌打两次:触发式,非当次数值",
  elixir_potion: "消耗任意张手牌:交互式(无固定张数)",
  entropic_brew: "填满空药水槽为随机药水:随机池/战斗外",
  essence_of_darkness: "按球槽数引导黑暗:充能球机制",
  fairy_potion: "致死时回血 30%/60%:需致死局面",
  fruit_juice: "增加 5/10 最大 HP:战斗外结算",
  gamblers_brew: "弃任意张再抽等量:交互式(无固定张数)",
  potion_of_capacity: "增加 2/4 球槽:充能球机制",
  power_potion: "三选一随机能力牌:随机池/选牌交互",
  skill_potion: "三选一随机技能牌:随机池/选牌交互",
  smoke_bomb: "逃离战斗:战斗外结算",
  stance_potion: "进入平静/愤怒(姿态):姿态系统不在本口径",
};

/** 没有抽出任何数值事实的药水 id(potionScenarios 里记录,供"口径覆盖"检查) */
const potionNoFacts = new Set<string>();

/**
 * 数值效果由探针场景(blindSpotScenarios)覆盖的药水:文本抽不出"当次直测"的数值,
 * 但已经被 probe 逐字段比对,不再算"未覆盖"。
 */
const POTION_PROBED: Record<string, string> = {
  liquid_memories: "取回牌本回合 0 费(probe 比对返回牌的当前费用;X 费牌除外)",
};

function potionScenarios(): void {
  for (const id of ourPotions) {
    if (gated.has(`potion/${id}`)) continue;
    const p = potionByGame.get(id);
    if (!p) continue;
    const variants: [string, number, string[]][] = [[id, 1, []]];
    // 圣树皮让药效翻倍:语料 potency.sacredBarkDoubles 说了哪些药水吃这个
    if (p.potency?.sacredBarkDoubles) variants.push([`${id}/bark`, 2, ["sacred_bark"]]);
    for (const [name, mul, relics] of variants) {
      const facts = potionFacts(p, mul);
      if (facts.length === 0) {
        if (!POTION_PROBED[id]) potionNoFacts.add(id);
        continue;
      }
      list.push({
        name: `potions/${name}`,
        kind: "potions",
        id,
        level: "base",
        facts,
        // 血量留缺口(治疗药水测得出回血)、手牌留空(抽牌药水测得出抽几张)
        scenario: playBoard({
          hand: [],
          player: { hp: 40 },
          relics,
          potions: [id, null, null],
          actions: [{ op: "potion", slot: 0, target: 0 }],
        }),
        mode: "delta",
      });
    }
  }
}

// 遗物:只认"开局就在身上挂着"的固定效果,从语料文本抽数值,开局快照比绝对值。
type RelicRule = { re: RegExp; fact: (m: RegExpMatchArray) => Fact; deck?: boolean };
const RELIC_RULES: RelicRule[] = [
  { re: /Start each combat with (\d+) Block/, fact: (m) => ({ k: "block", v: Number(m[1]) }) },
  { re: /Start each combat with (\d+) Thorns/, fact: (m) => ({ k: "player_power", name: "thorns", v: Number(m[1]) }) },
  { re: /Start each combat with (\d+) Artifact/, fact: (m) => ({ k: "player_power", name: "artifact", v: Number(m[1]) }) },
  { re: /Start each combat with (\d+) (Strength|Dexterity|Focus)/, fact: (m) => ({ k: "player_power", name: POWER_KEY[m[2]!] ?? m[2]!.toLowerCase(), v: Number(m[1]) }) },
  { re: /At the start of each combat, gain (\d+) (Strength|Dexterity|Plated Armor|Focus)/, fact: (m) => ({ k: "player_power", name: POWER_KEY[m[2]!] ?? m[2]!.toLowerCase(), v: Number(m[1]) }) },
  { re: /At the start of each combat, apply (\d+) (Vulnerable|Weak|Poison) to ALL enemies/, fact: (m) => ({ k: "enemy_power", name: POWER_KEY[m[2]!]!, v: Number(m[1]) }) },
  { re: /Start each combat with (\d+) (Weak|Vulnerable)/, fact: (m) => ({ k: "player_power", name: POWER_KEY[m[2]!]!, v: Number(m[1]) }) },
  { re: /At the start of each combat, draw (\d+) additional cards?/, fact: (m) => ({ k: "draw", v: Number(m[1]) }), deck: true },
  { re: /Gain (\d+) Energy on the first turn of each combat/, fact: (m) => ({ k: "energy", v: Number(m[1]) }) },
  { re: /At the start of each combat, heal (\d+) HP/, fact: (m) => ({ k: "self_hp", v: Number(m[1]) }) },
];

/**
 * 遗物的"不参与自动比对"归类:RELIC_RULES 只覆盖"开局就在场"的固定数值效果,
 * 其余按文本归入下列类别(显式登记,不静默跳过)。仅对未被 RELIC_RULES 匹配的
 * 遗物调用。
 */
function relicSkipReason(t: string): string {
  if (/Orb|Channel|Focus/i.test(t)) return "充能球机制(未建模)";
  if (/Scry/i.test(t)) return "预见(未建模)";
  if (/Divinity|Calm|Wrath|Stance/i.test(t)) return "姿态(未建模)";
  if (/At the start of each combat|Start each combat|first turn of each combat/i.test(t))
    return "开局触发但含选择/随机/交互(未建模)";
  if (/Whenever|Every time|Every \d+|At the (start|end) of|When you|If you|While/i.test(t))
    return "战斗内触发式(需时序探针,未建模)";
  if (/Gold|shop|Merchant|Card Reward|pick ?up|obtain|add a card to your deck|Chest|\broom\b|Map/i.test(t))
    return "战斗外(金币/地图/奖励/牌组)";
  if (/Rest|heal|Max HP|die/i.test(t)) return "战斗外(营火/治疗/最大生命/致死)";
  if (/Double the effectiveness|% more|% less|rather than/i.test(t)) return "数值修正(替换基准百分比)";
  return "其它(未分类)";
}

/**
 * RELIC_RULES / relicSkipReason 都盖不住的遗物:显式登记原因(与 CARD_NOT_COMPARED 同理)。
 * 新增遗物若既没规则、relicSkipReason 也归不了类,口径守卫会当"未登记"报错。
 */
const RELIC_NOT_COMPARED: Record<string, string> = {
  akabeko: "本场第一张攻击 +8:战斗内触发式(需攻击伤害探针,见 Heavy Blade 同轴)",
  centennial_puzzle: "本场首次掉血抽 3:战斗内触发式(需掉血时序)",
  juzu_bracelet: "? 房不再遇普通战:地图/房间生成机制",
  strike_dummy: "含 Strike 的牌 +3 伤害:战斗内数值修正(需攻击伤害探针)",
  white_beast_statue: "战利品必出药水:奖励屏机制",
  fossilized_helix: "本场首次掉血免疫:战斗内触发式",
  ginger: "免疫虚弱:减益免疫(需施加时序)",
  ice_cream: "能量跨回合保留:回合结算机制",
  turnip: "免疫脆弱:减益免疫(需施加时序)",
  hovering_kite: "每回合首次弃牌 +1 能:战斗内触发式",
  wrist_blade: "0 费攻击 +4:战斗内数值修正(需攻击伤害探针)",
  black_star: "精英多掉一件遗物:奖励屏机制",
  chemical_x: "X 费牌效果 +2:X 费修正(需 X 费探针)",
  frozen_eye: "抽牌堆按序显示:界面机制,不可在战斗沙盒观测",
  membership_card: "商店 50% 折扣:商店机制",
  sling_of_courage: "精英战开局 +2 力量:开局触发,但只限精英战",
  strange_spoon: "消耗改弃牌 50%:随机触发式",
  prismatic_shard: "奖励屏含无色/他色牌:奖励屏机制",
  neows_lament: "前三场敌人 1 HP:战斗外流程机制",
  nloths_gift: "稀有牌概率三倍:奖励屏掷点机制",
};

/** RELIC_RULES / 遗物探针覆盖的遗物 id(供登记表判"audited") */
const relicAudited = new Set<string>();

/** RELIC_RULES 未覆盖、也没登记原因的遗物 id(记录,供"口径覆盖"检查) */
const relicNoMatch = new Set<string>();

function relicScenarios(): void {
  for (const id of ourRelics) {
    if (gated.has(`relic/${id}`)) continue;
    const r = relicByGame.get(id);
    if (!r) continue;
    const t = norm(r.text);
    let matched = false;
    for (const rule of RELIC_RULES) {
      const m = t.match(rule.re);
      if (!m) continue;
      const extra: Record<string, unknown> = rule.deck
        ? { deck: Array.from({ length: 20 }, () => "strike") }
        : { hand: ["strike", "defend", "defend", "defend", "defend"] };
      list.push({
        name: `relics/${id}`,
        kind: "relics",
        id,
        level: "base",
        facts: [rule.fact(m)],
        scenario: initBoard({ relics: [id], ...extra }),
        mode: "init",
      });
      matched = true;
      break;
    }
    if (matched) relicAudited.add(id);
    else relicNoMatch.add(id);
  }
}

if (want("cards")) {
  cardScenarios();
  curseStatusScenarios();
  powerScenarios();
  choiceScenarios();
  conditionalScenarios();
}
if (want("potions")) potionScenarios();
if (want("relics")) relicScenarios();
// 历史盲区场景跨卡/药水/遗物三类(笔尖与靴子是遗物、液态记忆是药水),单独一组
blindSpotScenarios();

// ---- 跑沙盒 ----
const DIR = mkdtempSync(join(tmpdir(), "spire-audit-"));
const listPath = join(DIR, "list.txt");
const fileOf = (name: string) => join(DIR, `${name.replace(/[^a-z0-9]+/gi, "__")}.json`);
writeFileSync(listPath, list.map((s) => `${s.name}\t${fileOf(s.name)}`).join("\n"));
for (const s of list) writeFileSync(fileOf(s.name), JSON.stringify(s.scenario));

const run = spawnSync(SPIRE, ["--sandbox-batch", SEED, listPath], { encoding: "utf8", maxBuffer: 1 << 26 });
if (run.status !== 0) throw new Error(`沙盒跑不动(${run.status}): ${run.stderr}`);

interface Row {
  step: number;
  op: string;
  error?: string;
  st?: StateJson;
  /** scenario 开了 report 才有:这次选牌亮出的候选与手牌当前实际费用 */
  report?: { candidates: string[]; costs: number[] };
}
interface StateJson {
  energy: number;
  player: { hp: number; block: number; powers: Record<string, number> };
  hand: unknown[];
  draw: unknown[];
  discard: unknown[];
  exhaust: unknown[];
  enemies: { hp: number; powers: Record<string, number> }[];
}

const got = new Map<string, Row[]>();
{
  let name: string | null = null;
  let rows: Row[] = [];
  for (const line of run.stdout.split("\n")) {
    if (line.startsWith("#")) {
      if (name !== null) got.set(name, rows);
      name = line.slice(1);
      rows = [];
    } else if (line.trim() !== "") rows.push(JSON.parse(line) as Row);
  }
  if (name !== null) got.set(name, rows);
}

const powerDelta = (after: Record<string, number> | undefined, before: Record<string, number> | undefined) => {
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(before ?? {})) out[k] = (after?.[k] ?? 0) - v;
  for (const [k, v] of Object.entries(after ?? {})) if (!(k in (before ?? {}))) out[k] = v;
  return out;
};

interface Obs {
  damage: number;
  block: number;
  self_hp: number;
  energy: number;
  draw: number;
  player_power: Record<string, number>;
  enemy_power: Record<string, number>;
}

function observed(s: Spec, rows: Row[]): Obs | string {
  const err = rows.find((r) => r.error);
  if (err) return `动作报错: ${err.error}`;
  const snaps = rows.filter((r) => r.st);
  if (snaps.length === 0) return "沙盒没有产生快照";
  const init = snaps[0]!;
  const st = init.st!;
  if (s.mode === "init") {
    return {
      damage: 0,
      block: st.player.block,
      self_hp: st.player.hp,
      energy: st.energy,
      draw: st.hand.length,
      player_power: st.player.powers,
      enemy_power: st.enemies[0]!.powers,
    };
  }
  // 取最后一个快照:单动作场景就是这一步,多动作场景(炸弹)是跑完之后的现场
  const after = snaps[snaps.length - 1]!;
  const a = after.st!;
  return {
    damage: st.enemies[0]!.hp - a.enemies[0]!.hp,
    block: a.player.block - st.player.block,
    self_hp: a.player.hp - st.player.hp,
    energy: a.energy - st.energy,
    draw: a.hand.length - st.hand.length + (after.op === "play" ? 1 : 0),
    player_power: powerDelta(a.player.powers, st.player.powers),
    enemy_power: powerDelta(a.enemies[0]!.powers, st.enemies[0]!.powers),
  };
}

// ---- 比对 ----
const KNOWN: Record<string, string> = {
  // 引擎核心(combat.rs,归属另一个 agent)里的偏差:期望按原版/wiki 写,
  // 最小修法见报告,这里只归类,不算"审计口径外"。
  "cards/dual_wield/up":
    "(a) 我们错 [combat.rs:双重施法+ 应一次选择加 2 份;本作要选两次(Effect::CopyFromHand 两次 begin_choice)," +
    "修法:Effect::CopyFromHand 带 copies,choose() 的 Hand/Copy 分支按 ch.copies 复制]",
  "cards/exhume/base":
    "(a) 我们错 [combat.rs:发掘的候选池不该包含消耗掉的发掘自己(原版限制)," +
    "修法:FromExhaustToHand 的 begin_choice 用排除 exhume 的过滤器]",
  "cards/exhume/up":
    "(a) 我们错 [combat.rs:同上,发掘+ 的候选池不该包含消耗掉的发掘]",
};
interface Mismatch {
  name: string;
  verdict: string;
  lines: string[];
}
const mismatches: Mismatch[] = [];
let passed = 0;

interface CompareResult {
  lines: string[];
  /** 沙盒没输出这一段 / 探针自己炸了:整段判失败 */
  hard?: string;
}

/** 比对单个场景。返回 lines(空 = 通过)或 hard(整段失败);--selftest 复用同一份逻辑。 */
function compareSpec(s: Spec, rows: Row[]): CompareResult {
  // 探针轴:直接读快照算事实(持有/抽到/回合末/多回合/选牌结果集合)
  if (s.probe) {
    const rec = s.probe(rows);
    if (typeof rec === "string") return { lines: [], hard: rec };
    const lines: string[] = [];
    for (const f of s.facts) {
      if (f.k !== "probe") continue;
      if (rec[f.key] !== f.v) lines.push(`${f.key}: 实测 ${JSON.stringify(rec[f.key])} vs 期望 ${f.v}`);
    }
    return { lines };
  }
  const o = observed(s, rows);
  if (typeof o === "string") return { lines: [], hard: o };
  const lines: string[] = [];
  for (const f of s.facts) {
    let have: unknown;
    let want: unknown;
    if (f.k === "enemy_power") {
      have = o.enemy_power[f.name] ?? 0;
      want = f.v;
    } else if (f.k === "player_power") {
      have = o.player_power[f.name] ?? 0;
      want = f.v;
    } else if (s.mode === "init") {
      have = o[f.k === "self_hp" ? "self_hp" : f.k === "block" ? "block" : f.k === "energy" ? "energy" : "draw"];
      // 开局绝对值:手牌 5+多抽 / 血 80+X / 能量 3+X(默认最大能量 3 + 遗物)
      want = f.k === "draw" ? 5 + f.v : f.k === "self_hp" ? 40 + f.v : f.k === "energy" ? 3 + f.v : f.v;
    } else {
      have = o[f.k];
      const cost = (() => {
        const c = cardByGame.get(s.id);
        if (!c) return 0;
        const v = s.level === "up" ? c.upgrade.cost : c.cost;
        return typeof v === "number" ? v : 0;
      })();
      want = f.k === "draw" ? f.v - (CARD_HAND_LOSS[s.id] ?? 0) : f.k === "energy" ? f.v - cost : f.v;
    }
    if (have !== want) lines.push(`${f.k}${"name" in f ? ":" + f.name : ""}: 实测 ${JSON.stringify(have)} vs 期望 ${JSON.stringify(want)}`);
  }
  return { lines };
}

for (const s of list) {
  const rows = got.get(s.name);
  if (!rows) {
    mismatches.push({ name: s.name, verdict: "(a) 我们错", lines: ["沙盒没有输出这一段"] });
    continue;
  }
  const r = compareSpec(s, rows);
  if (r.hard !== undefined) {
    mismatches.push({ name: s.name, verdict: KNOWN[s.name] ?? "(a) 我们错", lines: [r.hard] });
    continue;
  }
  if (r.lines.length === 0) passed++;
  else mismatches.push({ name: s.name, verdict: KNOWN[s.name] ?? "(a) 我们错", lines: r.lines });
}

const coveredCardIds = new Set(list.filter((s) => s.kind === "cards").map((s) => s.id));
const unaudited = ourCards.filter((id) => !coveredCardIds.has(id));

// 口径覆盖检查:已实现牌的文本里"有数值效果但 lineFacts 认不出"的行,必须被
// hand-written 场景(CARD_SPECIAL/POWER_STACK/探针)覆盖,或登记进 CARD_NOT_COMPARED;
// 否则列进 blind —— 这正是哨卫"消耗回能"漏检的那类盲区。
const handWritten = new Set<string>([
  ...Object.keys(CARD_SPECIAL),
  ...Object.keys(POWER_STACK),
  ...Object.keys(CARD_SKIP),
  ...list.filter((s) => s.probe).map((s) => s.id),
]);
const blind: string[] = [];
for (const id of ourCards) {
  const c = cardByGame.get(id);
  if (!c || handWritten.has(id) || CARD_NOT_COMPARED[id]) continue;
  const lines = unparsedEffectLines(c);
  if (lines.length > 0) blind.push(`${id}: ${lines.join(" | ")}`);
}
// 数值字段级覆盖:没被探针/登记接管的牌,语料 values/upgrade 里每个非空数值字段
// (damage/block/magic)都必须能在抽出来的事实里找到落点:damage 按"事实值是它的整数倍"
// (多段/×times 会翻倍)、block 与 magic 按绝对值相等或"magic 是伤害事实的倍数"(times 类)。
// 语料自身文本 vs values 不一致的牌(见 corpusSelfCheck)跳过,那是语料问题不是口径问题。
const fieldBlind: string[] = [];
for (const id of ourCards) {
  const c = cardByGame.get(id);
  if (!c || handWritten.has(id) || CARD_NOT_COMPARED[id]) continue;
  if (corpusSelfCheck(c).length > 0) continue;
  for (const up of [false, true]) {
    const v = up ? c.upgrade : c.values;
    const facts = cardFacts(c, up);
    const eq = (n: number) => facts.some((f) => "v" in f && f.v !== 0 && Math.abs(f.v) === Math.abs(n));
    const dmg = facts.filter((f) => f.k === "damage").map((f) => f.v);
    const multOf = (n: number) => n > 0 && dmg.some((d) => d > 0 && d % n === 0);
    const lvl = up ? "升级" : "基础";
    if (v.damage !== null && !multOf(v.damage)) fieldBlind.push(`${id} ${lvl}: damage ${v.damage} 没被任何事实覆盖`);
    if (v.block !== null && !eq(v.block)) fieldBlind.push(`${id} ${lvl}: block ${v.block} 没被任何事实覆盖`);
    if (v.magic !== null && !eq(v.magic) && !multOf(v.magic)) fieldBlind.push(`${id} ${lvl}: magic ${v.magic} 没被任何事实覆盖`);
  }
}
const potionBlind = ourPotions.filter((id) => potionNoFacts.has(id) && !POTION_NOT_COMPARED[id]);
const relicReasons: Record<string, string[]> = {};
for (const id of relicNoMatch) {
  const r = relicByGame.get(id);
  const reason = RELIC_NOT_COMPARED[id] ? "已登记(RELIC_NOT_COMPARED)" : r ? relicSkipReason(norm(r.text)) : "语料里没有";
  (relicReasons[reason] ??= []).push(id);
}

const selfIssues: string[] = [];
for (const id of ourCards) {
  const c = cardByGame.get(id);
  if (!c) continue;
  for (const issue of corpusSelfCheck(c)) selfIssues.push(`${id}: ${issue}`);
}
for (const id of ourPotions) {
  const p = potionByGame.get(id);
  if (!p?.potency || p.potency.base === null) continue;
  const m = norm(p.text).match(/\[(\d+)\s*(?:[^|\]]*)\|\s*(\d+)/);
  if (m && Number(m[1]) !== p.potency.base) selfIssues.push(`药水 ${id}: 文本 ${m[1]} vs potency ${p.potency.base}`);
  else if (m && p.potency.sacredBarkDoubles && Number(m[2]) !== p.potency.base * 2)
    selfIssues.push(`药水 ${id}: 圣树皮档 文本 ${m[2]} vs 2×potency ${p.potency.base * 2}`);
}

// 静态数据(费用/类型/稀有度)对语料
const dataIssues: string[] = [];
for (const r of dump("cards").filter((c) => c[3] === "cost")) {
  const c = cardByGame.get(r[0]!);
  if (!c) {
    dataIssues.push(`牌 ${r[0]}: 语料里没有`);
    continue;
  }
  const cc = c.cost;
  const exp = cc === -2 ? "-" : cc === -1 ? "X" : cc === null ? "?" : String(cc);
  if (r[4] !== exp) dataIssues.push(`牌 ${r[0]} 费用: 我们 ${r[4]} vs 语料 ${exp}`);
  if (r[1]!.toLowerCase() !== c.type.toLowerCase()) dataIssues.push(`牌 ${r[0]} 类型: 我们 ${r[1]} vs 语料 ${c.type}`);
  if (["attack", "skill", "power"].includes(c.type) && r[2]!.toLowerCase() !== c.rarity.toLowerCase())
    dataIssues.push(`牌 ${r[0]} 稀有度: 我们 ${r[2]} vs 语料 ${c.rarity}`);
}
for (const r of dump("potions")) {
  const p = potionByGame.get(r[0]!);
  if (p && p.rarity && r[1]!.toLowerCase() !== String(p.rarity).toLowerCase())
    dataIssues.push(`药水 ${r[0]} 稀有度: 我们 ${r[1]} vs 语料 ${p.rarity}`);
}

// ---- 文案 ↔ 实现 双向校验(静态) ----
//
// 语料题面(JSON text)里的数值与触发描述,必须与本作源码里的常量一一对上:两边任一
// 侧被改动都会在这里报错。数字/标志只从"两边各自抽取",规则表只记"文案正则 + 字段名",
// 不重复写数字:
//   ①语料侧:corpus 的 text(归一 + 取基础档)
//   ②实现侧:src/core/relics.rs 的 desc + RelicFx 字段;src/core/potions.rs 的
//      desc + PotionFx 载荷;src/core/cards.rs 的 text + cost/标志位/Effect 载荷。
// 覆盖守卫:已实现内容里"文案含数字却没进表、也没登记"的,直接判失败,防止新内容静默漏检。
const rustStr = (s: string) =>
  s.replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\"/g, '"').replace(/\\\\/g, "\\");
/** 取语料 [基础|升级] 的基础档,折叠空白:同一份正则可同时匹配语料与实现两边文案 */
const flatBase = (text: string) =>
  text
    .replace(/\[([^\]|]*)\|[^\]]*\]/g, "$1")
    .replace(/\[([^\]]*)\]/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
/** 取语料 [基础|升级] 的升级档(升级档没有两级值时与基础档相同) */
const flatUp = (text: string) =>
  text
    .replace(/\[([^\]|]*)\|([^\]]*)\]/g, "$2")
    .replace(/\[([^\]]*)\]/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
const anyDigits = (t: string) => /\d/.test(t);

interface RelicImpl {
  desc: string;
  fx: Map<string, string>;
}
function parseRelicImpls(raw: string): Map<string, RelicImpl> {
  const out = new Map<string, RelicImpl>();
  for (const block of raw.split("\n    RelicDef {").slice(1)) {
    const id = block.match(/id: "([^"]+)"/)?.[1];
    if (!id) continue;
    const desc = rustStr(block.match(/desc: "((?:[^"\\]|\\.)*)"/)?.[1] ?? "");
    const fx = new Map<string, string>();
    const body = block.match(/fx: RelicFx \{([\s\S]*?)\n        \}/);
    if (body)
      for (const line of body[1]!.split("\n")) {
        const m = line.match(/^\s*([a-z_0-9]+):\s*(.+?),\s*$/);
        if (m) fx.set(m[1]!, m[2]!);
      }
    out.set(id, { desc, fx });
  }
  return out;
}

interface PotionImpl {
  desc: string;
  variant: string;
  payload: Map<string, string>;
}
function parsePotionImpls(raw: string): Map<string, PotionImpl> {
  const out = new Map<string, PotionImpl>();
  for (const block of raw.split("\n    PotionDef {").slice(1)) {
    const id = block.match(/id: "([^"]+)"/)?.[1];
    if (!id) continue;
    const desc = rustStr(block.match(/desc: "((?:[^"\\]|\\.)*)"/)?.[1] ?? "");
    const fx = block.match(/fx: PotionFx::(\w+)(?:\s*\{([^}]*)\})?/);
    const payload = new Map<string, string>();
    if (fx?.[2])
      for (const part of fx[2].split(",")) {
        const m = part.match(/^\s*([a-z_0-9]+):\s*(.+?)\s*$/);
        if (m) payload.set(m[1]!, m[2]!);
      }
    out.set(id, { desc, variant: fx?.[1] ?? "?", payload });
  }
  return out;
}

/** 解析出来的 Effect 引用:变体名、括号参数(DoubleSelfStatus(Status::X))、{ 字段: 值 } 表 */
interface EffRef {
  variant: string;
  arg: string;
  fields: Map<string, string>;
}
/** 升级宏带来的标记覆盖(沿用基础值的字段留 undefined) */
interface UpFlags {
  exhaust?: boolean;
  innate?: boolean;
  retain?: boolean;
  ethereal?: boolean;
  target?: string;
}
interface CardImpl {
  text: string;
  block: string;
  kind: string;
  cost: string;
  target: string;
  exhaust: boolean;
  ethereal: boolean;
  innate: boolean;
  retain: boolean;
  unremovable: boolean;
  multiUpgrade: boolean;
  effects: EffRef[];
  onDraw: EffRef[];
  onEndTurn: EffRef[];
  inHand: EffRef[];
  /** 升级宏名:up / up_innate / up_no_exhaust / up_all,不可升级是 None */
  upMacro: string;
  upText: string;
  upEffects: EffRef[] | null;
  upFlags: UpFlags;
}
/** 从 src[i](必须是 '[')起取配对括号内的内容 */
function bracketInner(src: string, i: number): { inner: string; end: number } | null {
  let d = 0;
  for (let k = i; k < src.length; k++) {
    if (src[k] === "[") d++;
    else if (src[k] === "]") {
      d--;
      if (d === 0) return { inner: src.slice(i + 1, k), end: k };
    }
  }
  return null;
}
/** 解析一段 &[Effect::..., ...] 的内容(不含外层括号) */
function parseEffects(inner: string): EffRef[] {
  const out: EffRef[] = [];
  let depth = 0;
  let cur = "";
  const push = (s: string) => {
    const t = s.trim();
    if (t === "") return;
    const m = t.match(/^Effect::(\w+)(.*)$/s);
    if (!m) return;
    const fields = new Map<string, string>();
    const body = m[2]!.match(/\{([^}]*)\}/);
    if (body)
      for (const part of body[1]!.split(",")) {
        const kv = part.match(/^\s*([a-z_0-9]+):\s*(.+?)\s*$/);
        if (kv) fields.set(kv[1]!, kv[2]!);
      }
    out.push({ variant: m[1]!, arg: m[2]!.match(/^\s*\(([^)]*)\)/)?.[1]?.trim() ?? "", fields });
  };
  for (const ch of inner) {
    if (ch === "{" || ch === "(") depth++;
    else if (ch === "}" || ch === ")") depth--;
    if (ch === "," && depth === 0) {
      push(cur);
      cur = "";
      continue;
    }
    cur += ch;
  }
  push(cur);
  return out;
}
/** 取 CardDef 里某个 &[…] 切片(按要求字段名定位,如 on_draw / effects) */
function sliceEffects(block: string, name: string): EffRef[] {
  const i = block.indexOf(`${name}: &[`);
  if (i < 0) return [];
  const b = bracketInner(block, block.indexOf("[", i));
  return b ? parseEffects(b.inner) : [];
}
function parseCardImpls(raw: string): Map<string, CardImpl> {
  const out = new Map<string, CardImpl>();
  for (const block of raw.split("\n    CardDef {").slice(1)) {
    const id = block.match(/id: "([^"]+)"/)?.[1];
    if (!id) continue;
    const flag = (n: string) => block.match(new RegExp(`\\n\\s*${n}: (true|false),`))?.[1] === "true";
    const up = block.match(/\n\s{8}upgrade: (up(?:_[a-z_]+)?!|None)/);
    const upMacro = (up?.[1] ?? "None").replace(/!$/, "");
    let upText = "";
    let upEffects: EffRef[] | null = null;
    const upFlags: UpFlags = {};
    if (up && upMacro !== "None") {
      const after = block.slice(up.index!);
      upText = rustStr(after.match(/up(?:_[a-z_]+)?!\([^"]*"((?:[^"\\]|\\.)*)"/)?.[1] ?? "");
      const txt = after.match(/up(?:_[a-z_]+)?!\([^"]*"(?:[^"\\]|\\.)*"\s*,\s*\[/);
      if (txt) {
        const b = bracketInner(after, after.indexOf("[", txt.index! + txt[0]!.length - 1));
        if (b) upEffects = parseEffects(b.inner);
      }
      if (upMacro === "up_innate") upFlags.innate = true;
      if (upMacro === "up_no_exhaust") upFlags.exhaust = false;
      if (upMacro === "up_all") upFlags.target = "All";
    }
    out.set(id, {
      text: rustStr(block.match(/\n\s{8}text: "((?:[^"\\]|\\.)*)"/)?.[1] ?? ""),
      block,
      kind: block.match(/kind: CardType::(\w+)/)?.[1] ?? "?",
      cost: block.match(/\n\s*cost: (Cost::\w+(?:\([^)]*\))?)/)?.[1] ?? "?",
      target: block.match(/target: Target::(\w+)/)?.[1] ?? "?",
      exhaust: flag("exhaust"),
      ethereal: flag("ethereal"),
      innate: flag("innate"),
      retain: flag("retain"),
      unremovable: flag("unremovable"),
      multiUpgrade: flag("multi_upgrade"),
      effects: sliceEffects(block, "effects"),
      onDraw: sliceEffects(block, "on_draw"),
      onEndTurn: sliceEffects(block, "on_end_turn"),
      inHand: sliceEffects(block, "in_hand"),
      upMacro,
      upText,
      upEffects,
      upFlags,
    });
  }
  return out;
}

const SRC_RELICS_RS = readFileSync(join(ROOT, "src/core/relics.rs"), "utf8");
const SRC_POTIONS_RS = readFileSync(join(ROOT, "src/core/potions.rs"), "utf8");
const SRC_CARDS_RS = readFileSync(join(ROOT, "src/core/cards.rs"), "utf8");
const SRC_COMBAT_RS = readFileSync(join(ROOT, "src/core/combat.rs"), "utf8");
const SRC_RUN_RS = readFileSync(join(ROOT, "src/core/run.rs"), "utf8");
/** 数值落在字面量里的遗物规则要在这些源码里查常量 */
const RELIC_SRC_FILES: Record<string, string> = { "combat.rs": SRC_COMBAT_RS, "run.rs": SRC_RUN_RS };
const relicImpl = parseRelicImpls(SRC_RELICS_RS);
const potionImpl = parsePotionImpls(SRC_POTIONS_RS);
const cardImpl = parseCardImpls(SRC_CARDS_RS);

/** 遗物文案数值规则:re 在(语料 + 实现)两份文案上都要命中;捕获组 v 的数值经 scale
 *  换算后必须等于 RelicFx 里 field 的常量。period 额外要求该数字出现在字段名里
 *  (如 energy_every_3_turns / block_turn3);bool 表示字段是布尔,只校验为 true。 */
interface RelicNumRule {
  id: string;
  re: RegExp;
  field: string;
  v?: number;
  period?: number;
  scale?: (n: number) => number;
  bool?: boolean;
}
const R = (
  id: string,
  re: RegExp,
  field: string,
  extra: Omit<RelicNumRule, "id" | "re" | "field"> = {},
): RelicNumRule => ({ id, re, field, ...extra });
const RELIC_NUM_RULES: RelicNumRule[] = [
  R("akabeko", /first Attack each combat deals (\d+) additional damage/, "combat_start_vigor"),
  R("anchor", /with (\d+) Block/, "combat_start_block"),
  R("ancient_tea_set", /enter a Rest Site, start the next combat with (\d+) extra Energy/, "energy_turn1_if_rested"),
  R("astrolabe", /Transform (\d+) cards/, "transform_cards"),
  R("bag_of_marbles", /apply (\d+) Vulnerable to ALL enemies/, "combat_start_enemy_vulnerable"),
  R("bag_of_preparation", /each combat, draw (\d+) additional cards/, "combat_start_draw"),
  R("bird_faced_urn", /play a Power card, heal (\d+) HP/, "heal_on_power_card"),
  R("black_blood", /end of combat, heal (\d+) HP/, "post_combat_heal"),
  R("blood_vial", /each combat, heal (\d+) HP/, "combat_start_heal"),
  R("bloody_idol", /gain Gold, heal (\d+) HP/, "heal_on_gold_gain"),
  R("blue_candle", /lose (\d+) HP and Exhaust/, "playable_curses_hp"),
  R("bronze_scales", /with (\d+) Thorns/, "thorns"),
  R("brimstone", /gain (\d+) Strength and ALL enemies gain (\d+) Strength/, "brimstone_self"),
  R("brimstone", /gain (\d+) Strength and ALL enemies gain (\d+) Strength/, "brimstone_enemy", { v: 2 }),
  R("burning_blood", /end of combat, heal (\d+) HP/, "post_combat_heal"),
  R("busted_crown", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("busted_crown", /rewards have (\d+) less cards/, "card_reward_bonus", { scale: (n) => -n }),
  R("calipers", /lose (\d+) Block/, "block_loss_cap"),
  R("calling_bell", /and (\d+) relics/, "add_relics"),
  R("captains_wheel", /At the start of your (\d+)\w* turn, gain (\d+) Block/, "block_turn3", { period: 1, v: 2 }),
  R("cauldron", /brews (\d+) random potions/, "add_potions"),
  R("centennial_puzzle", /draw (\d+) cards/, "draw_on_first_hp_loss"),
  R("ceramic_fish", /add a card to your deck, gain (\d+) Gold/, "gold_on_card_add"),
  R("champion_belt", /also apply (\d+) Weak/, "weak_on_vulnerable"),
  R("charons_ashes", /Exhaust a card, deal (\d+) damage/, "damage_all_on_exhaust"),
  R("chemical_x", /increased by (\d+)/, "x_cost_bonus"),
  R("cloak_clasp", /gain (\d+) Block for each card/, "block_per_card_in_hand_at_end"),
  R("clockwork_souvenir", /with (\d+) Artifact/, "combat_start_artifact"),
  R("coffee_dripper", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("cursed_key", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("darkstone_periapt", /increase your Max HP by (\d+)/, "max_hp_on_curse"),
  R("discerning_monocle", /reduced by (\d+)%/, "shop_discount_pct"),
  R("du_vu_doll", /For each Curse in your deck, start each combat with (\d+) Strength/, "combat_start_strength_per_curse"),
  R("ectoplasm", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("empty_cage", /remove (\d+) cards/, "remove_cards"),
  R("eternal_feather", /For every (\d+) cards in your deck, heal (\d+) HP/, "rest_heal_per_5_deck", { period: 1, v: 2 }),
  R("face_of_cleric", /end of combat, raise your Max HP by (\d+)/, "max_hp_on_victory"),
  R("fusion_hammer", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("girya", /up to (\d+) times/, "rest_lift_max"),
  R("golden_idol", /drop (\d+)% more Gold/, "gold_reward_pct"),
  R("gremlin_horn", /enemy dies, gain (\d+) Energy/, "energy_on_kill"),
  R("gremlin_horn", /and draw (\d+) card/, "draw_on_kill"),
  R("gremlin_visage", /each combat with (\d+) Weak/, "combat_start_self_weak"),
  R("hand_drill", /break an enemy's Block, apply (\d+) Vulnerable/, "vulnerable_on_block_break"),
  R("happy_flower", /Every (\d+) turns, gain (\d+) Energy/, "energy_every_3_turns", { period: 1, v: 2 }),
  R("horn_cleat", /At the start of your (\d+)\w* turn, gain (\d+) Block/, "block_turn2", { period: 1, v: 2 }),
  R("hovering_kite", /discard a card each turn, gain (\d+) Energy/, "gain_energy_first_discard_per_turn"),
  R("incense_burner", /Every (\d+) turns, gain (\d+) Intangible/, "intangible_every_6_turns", { period: 1, v: 2 }),
  R("ink_bottle", /play (\d+) cards, draw (\d+) card/, "draw_per_10_cards", { period: 1, v: 2 }),
  R("kunai", /play (\d+) Attacks in a single turn, gain (\d+) Dexterity/, "dexterity_per_3_attacks", { period: 1, v: 2 }),
  R("lantern", /Gain (\d+) Energy on the first turn/, "combat_start_energy"),
  R("lees_waffle", /raise your Max HP by (\d+)/, "max_hp"),
  R("letter_opener", /play (\d+) Skills in a single turn, deal (\d+) damage/, "damage_all_per_3_skills", { period: 1, v: 2 }),
  R("lizard_tail", /heal to (\d+)% of your Max HP/, "death_save_pct"),
  R("magic_flower", /Healing is (\d+)% more effective/, "combat_heal_pct", { scale: (n) => 100 + n }),
  R("mango", /raise your Max HP by (\d+)/, "max_hp"),
  R("mark_of_pain", /shuffle (\d+) Wounds/, "combat_start_wounds"),
  R("matryoshka", /The next (\d+) non-boss chests/, "extra_chest_relic_charges"),
  R("maw_bank", /climb a floor, gain (\d+) Gold/, "gold_per_floor"),
  R("meal_ticket", /enter a shop, heal (\d+) HP/, "heal_on_shop_enter"),
  R("meat_on_the_bone", /end of combat, heal (\d+) HP/, "post_combat_heal_if_below_half"),
  R("membership_card", /(\d+)% discount/, "shop_discount_pct"),
  R("mercury_hourglass", /start of your turn, deal (\d+) damage/, "damage_all_turn_start"),
  R("mutagenic_strength", /Start each combat with (\d+) Strength/, "combat_start_strength_turn1"),
  R("neows_lament", /first (\d+) combats/, "neow_lament_combats"),
  R("nilrys_codex", /shuffle 1 of (\d+) random cards/, "end_turn_shuffle_pick"),
  R("nunchaku", /play (\d+) Attacks, gain (\d+) Energy/, "energy_per_10_attacks", { period: 1, v: 2 }),
  R("odd_mushroom", /take (\d+)% more attack damage rather than/, "vulnerable_taken_pct", { scale: (n) => 100 + n }),
  R("oddly_smooth_stone", /each combat, gain (\d+) Dexterity/, "combat_start_dexterity"),
  R("old_coin", /Upon pickup, gain (\d+) Gold/, "gold"),
  R("omamori", /next (\d+) Curses/, "curse_negate"),
  R("orichalcum", /end your turn without Block, gain (\d+) Block/, "block_if_no_block_at_end"),
  R("ornamental_fan", /play (\d+) Attacks in a single turn, gain (\d+) Block/, "block_per_3_attacks", { period: 1, v: 2 }),
  R("orrery", /add (\d+) cards/, "pickup_card_picks"),
  R("pantograph", /Boss combats, heal (\d+) HP/, "boss_combat_heal"),
  R("paper_krane", /deal (\d+)% less damage rather than/, "weak_damage_pct", { scale: (n) => 100 - n }),
  R("paper_phrog", /take (\d+)% more damage rather than/, "vulnerable_damage_pct", { scale: (n) => 100 + n }),
  R("pear", /raise your Max HP by (\d+)/, "max_hp"),
  R("pen_nib", /Every (\d+)th Attack you play deals double damage/, "double_damage_per_10_attacks", { period: 1, bool: true }),
  R("philosophers_stone", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("philosophers_stone", /enemies start combat with (\d+) Strength/, "combat_start_enemy_strength"),
  R("pocketwatch", /draw (\d+) additional cards at the start of your next turn/, "draw_next_turn_if_low_play"),
  R("potion_belt", /gain (\d+) Potion slots/, "potion_slots"),
  R("preserved_insect", /have (\d+)% less HP/, "elite_hp_reduction_pct"),
  R("question_card", /card rewards have (\d+) additional card/, "card_reward_bonus"),
  R("red_mask", /apply (\d+) Weak to ALL enemies/, "combat_start_enemy_weak"),
  R("red_skull", /have (\d+) additional Strength/, "strength_when_bloodied"),
  R("regal_pillow", /Whenever you Rest, heal an additional (\d+) HP/, "rest_heal_bonus"),
  R("ring_of_the_snake", /each combat, draw (\d+) additional cards/, "combat_start_draw"),
  R("ring_of_the_serpent", /draw (\d+) additional card/, "draw_per_turn"),
  R("runic_cube", /lose HP, draw (\d+) card/, "draw_on_hp_loss"),
  R("runic_dome", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("self_forming_clay", /gain (\d+) Block next turn/, "block_next_turn_on_hp_loss"),
  R("shuriken", /play (\d+) Attacks in a single turn, gain (\d+) Strength/, "strength_per_3_attacks", { period: 1, v: 2 }),
  R("singing_bowl", /raise your Max HP by (\d+) instead/, "max_hp_on_card_skip"),
  R("slavers_collar", /gain (\d+) Energy at the start of your turn/, "combat_start_energy_elite_only"),
  R("sling_of_courage", /Start each Elite combat with (\d+) Strength/, "combat_start_strength_elite"),
  R("smiling_mask", /costs (\d+) Gold/, "removal_cost_fixed"),
  R("snecko_eye", /start of your turn, draw (\d+) additional cards/, "draw_per_turn"),
  R("sozu", /Gain (\d+) Energy at the start of your turn/, "combat_start_energy_per_turn"),
  R("ssserpent_head", /enter a \? room, gain (\d+) Gold/, "gold_on_unknown_room"),
  R("stone_calendar", /At the end of turn (\d+), deal (\d+) damage/, "damage_all_turn7", { period: 1, v: 2 }),
  R("strange_spoon", /discard (\d+)% of the time/, "exhaust_to_discard_pct"),
  R("strawberry", /raise your Max HP by (\d+)/, "max_hp"),
  R("strike_dummy", /containing "Strike" deal (\d+) additional damage/, "strike_damage_bonus"),
  R("sundial", /Every (\d+) times you shuffle your draw pile, gain (\d+) Energy/, "energy_per_3_shuffles", { period: 1, v: 2 }),
  R("the_abacus", /shuffle your draw pile, gain (\d+) Block/, "block_on_shuffle"),
  R("the_boot", /deal (\d+) or less unblocked/, "small_attack_boost_to", { scale: (n) => n + 1 }),
  R("the_boot", /increase it to (\d+)/, "small_attack_boost_to"),
  R("the_courier", /reduced by (\d+)%/, "shop_discount_pct"),
  R("thread_and_needle", /each combat, gain (\d+) Plated Armor/, "combat_start_plated_armor"),
  R("tingsha", /discard a card during your turn, deal (\d+) damage/, "damage_random_on_discard"),
  R("tiny_chest", /Every (\d+)th \? room is a Treasure room/, "treasure_every_4_unknown", { period: 1, bool: true }),
  R("toolbox", /choose 1 of (\d+) random Colorless/, "combat_start_colorless_pick"),
  R("torii", /reduce it to (\d+)/, "small_attack_reduce_to"),
  R("tough_bandages", /discard a card during your turn, gain (\d+) Block/, "block_on_discard"),
  R("toy_ornithopter", /use a potion, heal (\d+) HP/, "heal_on_potion_use"),
  R("tungsten_rod", /lose (\d+) less/, "hp_loss_reduction"),
  R("vajra", /each combat, gain (\d+) Strength/, "combat_start_strength"),
  R("velvet_choker", /more than (\d+) cards per turn/, "card_play_cap"),
  R("war_paint", /Upgrade (\d+) random Skills/, "upgrade_random_skills"),
  R("whetstone", /Upgrade (\d+) random Attacks/, "upgrade_random_attacks"),
  R("wing_boots", /travel to (\d+) times/, "map_wing_charges"),
  R("wrist_blade", /cost 0 deal (\d+) additional damage/, "zero_cost_attack_bonus"),
];

/** 数值落在源码字面量里(不在 RelicFx 字段)的遗物:文案里的阈值必须等于源码常量 */
interface RelicLitRule {
  id: string;
  re: RegExp;
  v?: number;
  file: string;
  lit: RegExp;
}
const RELIC_LIT_RULES: RelicLitRule[] = [
  // 鸟居阈值:文案 "5 or less ... reduce it to 1",源码 combat.rs `taken <= 5`
  { id: "torii", re: /(\d+) or less unblocked/, file: "combat.rs", lit: /taken <= (\d+)/ },
  // 死灵之书:文案 "costs 2 or more",源码 combat.rs `cost >= 2`
  { id: "necronomicon", re: /costs (\d+) or more/, file: "combat.rs", lit: /cost >= (\d+)/ },
  // "每 X"类:文案周期必须等于实现里计数器的阈值/模数(字段名里的数字只证明命名,这里证明真在用它)
  { id: "pen_nib", re: /Every (\d+)th Attack you play deals double damage/, file: "combat.rs", lit: /pen_nib >= (\d+)/ },
  { id: "nunchaku", re: /play (\d+) Attacks, gain (\d+) Energy/, file: "combat.rs", lit: /attacks_total >= (\d+)/ },
  { id: "ink_bottle", re: /play (\d+) cards, draw (\d+) card/, file: "combat.rs", lit: /cards_total >= (\d+)/ },
  { id: "kunai", re: /play (\d+) Attacks in a single turn/, file: "combat.rs", lit: /attacks_this_turn % (\d+)/ },
  { id: "shuriken", re: /play (\d+) Attacks in a single turn/, file: "combat.rs", lit: /attacks_this_turn % (\d+)/ },
  { id: "ornamental_fan", re: /play (\d+) Attacks in a single turn/, file: "combat.rs", lit: /attacks_this_turn % (\d+)/ },
  { id: "letter_opener", re: /play (\d+) Skills in a single turn/, file: "combat.rs", lit: /skills_this_turn % (\d+)/ },
  { id: "happy_flower", re: /Every (\d+) turns, gain (\d+) Energy/, file: "combat.rs", lit: /happy_flower >= (\d+)/ },
  { id: "incense_burner", re: /Every (\d+) turns, gain (\d+) Intangible/, file: "combat.rs", lit: /incense >= (\d+)/ },
  { id: "sundial", re: /Every (\d+) times you shuffle your draw pile/, file: "combat.rs", lit: /sundial >= (\d+)/ },
  { id: "tiny_chest", re: /Every (\d+)th \? room is a Treasure room/, file: "run.rs", lit: /unknown_rooms_seen % (\d+)/ },
  { id: "eternal_feather", re: /For every (\d+) cards in your deck/, file: "run.rs", lit: /deck\.len\(\) as i32 \/ (\d+)/ },
];

/** 文案含数字但数值由别处机制承载(布尔字段 / run.rs 特判 / 隐含),显式登记 */
const RELIC_NUM_SKIP: Record<string, string> = {
  tiny_house: "拾取五连效果(1 药水/50 金/5 上限/1 牌/1 升级)由 run.rs 的 pickup_tiny_house 特判",
  mummified_hand: "文案的 0 是'费用变 0'的机制描述,字段 zero_hand_card_on_power 是布尔",
  enchiridion: "文案的 0 是'费用变 0'的机制描述,字段 add_random_power_card 是布尔",
  matryoshka: "第二个 '2 Relics' 由基础箱 1 件 + 额外 1 件的机制隐含(第一个 'next 2 chests' 已入表)",
};

interface TextSource {
  label: string;
  text: string;
}
function checkRelicText(
  impls: Map<string, RelicImpl>,
  corpus: (id: string) => string | undefined,
  srcFiles: Record<string, string>,
): { fails: string[]; covered: number } {
  const fails: string[] = [];
  let covered = 0;
  const sourcesOf = (id: string, impl?: RelicImpl): TextSource[] => {
    const out: TextSource[] = [];
    const ct = corpus(id);
    if (ct !== undefined) out.push({ label: "语料", text: flatBase(norm(ct)) });
    if (impl) out.push({ label: "实现文案", text: flatBase(impl.desc) });
    return out;
  };
  for (const rule of RELIC_NUM_RULES) {
    const impl = impls.get(rule.id);
    if (!impl) {
      fails.push(`遗物 ${rule.id}: relics.rs 里没有该 RelicDef`);
      continue;
    }
    const raw = impl.fx.get(rule.field);
    if (raw === undefined) {
      fails.push(`遗物 ${rule.id}: RelicFx 里没有字段 ${rule.field}`);
      continue;
    }
    for (const s of sourcesOf(rule.id, impl)) {
      const m = s.text.match(rule.re);
      if (!m) {
        fails.push(`遗物 ${rule.id}[${s.label}]: 文案与规则 ${rule.re} 对不上`);
        continue;
      }
      if (rule.period && !rule.field.includes(String(m[rule.period])))
        fails.push(`遗物 ${rule.id}[${s.label}]: 文案周期 ${m[rule.period]} vs 字段名 ${rule.field}`);
      if (rule.bool) {
        if (raw !== "true") fails.push(`遗物 ${rule.id}[${s.label}]: 字段 ${rule.field}=${raw},文案说会触发`);
      } else {
        const n = Number(m[rule.v ?? 1]);
        const want = rule.scale ? rule.scale(n) : n;
        if (want !== Number(raw)) fails.push(`遗物 ${rule.id}[${s.label}]: 文案数值 ${n}${rule.scale ? `(→${want})` : ""} vs 实现 ${rule.field}=${raw}`);
      }
    }
    covered++;
  }
  for (const rule of RELIC_LIT_RULES) {
    const lit = (srcFiles[rule.file] ?? "").match(rule.lit);
    if (!lit) {
      fails.push(`遗物 ${rule.id}: ${rule.file} 里找不到常量 ${rule.lit}`);
      continue;
    }
    for (const s of sourcesOf(rule.id, impls.get(rule.id))) {
      const m = s.text.match(rule.re);
      if (!m) {
        fails.push(`遗物 ${rule.id}[${s.label}]: 文案与规则 ${rule.re} 对不上`);
        continue;
      }
      if (Number(m[rule.v ?? 1]) !== Number(lit[1])) fails.push(`遗物 ${rule.id}[${s.label}]: 文案阈值 ${m[rule.v ?? 1]} vs ${rule.file} 常量 ${lit[1]}`);
    }
    covered++;
  }
  const ruled = new Set([...RELIC_NUM_RULES.map((r) => r.id), ...RELIC_LIT_RULES.map((r) => r.id)]);
  for (const id of ourRelics) {
    if (gated.has(`relic/${id}`)) continue;
    if (ruled.has(id) || RELIC_NUM_SKIP[id]) continue;
    const impl = impls.get(id);
    const ct = corpus(id);
    const txt = `${ct ? flatBase(norm(ct)) : ""}\n${impl ? flatBase(impl.desc) : ""}`;
    if (anyDigits(txt)) fails.push(`遗物 ${id}: 文案含数字却没进双向校验表(加规则或登记 RELIC_NUM_SKIP)`);
  }
  return { fails, covered };
}

/** 药水文案数值 ↔ PotionFx 载荷(amount/n/pct);两边文案里的基础数值都要等于载荷 */
const POTION_NUM_SKIP: Record<string, string> = {
  duplication_potion: "文案用 'card is/... played twice' 描述,基础档没有数字;基础数值 1 取自语料 potency.base",
  liquid_memories: "文案 'a card|2 cards' 的基础档是文字,没有数字;基础数值 1 取自语料 potency.base",
  fairy_potion: "致死回血 30%/60% 由战斗致死保护实现,不是 PotionFx 常量(已在 POTION_NOT_COMPARED 登记)",
};
function potionPayload(p: PotionImpl): number | null {
  for (const k of ["amount", "n", "pct"]) {
    const v = p.payload.get(k);
    if (v !== undefined) return Number(v);
  }
  return null;
}
function checkPotionText(
  impls: Map<string, PotionImpl>,
  corpus: (id: string) => CorpusPotion | undefined,
): { fails: string[]; covered: number } {
  const fails: string[] = [];
  let covered = 0;
  for (const id of ourPotions) {
    const impl = impls.get(id);
    if (!impl) {
      fails.push(`药水 ${id}: potions.rs 里没有该 PotionDef`);
      continue;
    }
    covered++;
    const p = corpus(id);
    const payload = potionPayload(impl);
    const ct = p ? flatBase(norm(p.text)) : undefined;
    const dt = flatBase(impl.desc);
    if (payload === null) {
      if (gated.has(`potion/${id}`) || POTION_NUM_SKIP[id]) continue;
      const hasDigit = (ct !== undefined && anyDigits(ct)) || anyDigits(dt);
      const base = p?.potency?.base ?? null;
      if (hasDigit || base !== null) fails.push(`药水 ${id}: 文案含数字/potency 但 PotionFx 没有数值载荷,也没登记 POTION_NUM_SKIP`);
      continue;
    }
    if (p?.potency && p.potency.base !== null && p.potency.base !== payload)
      fails.push(`药水 ${id}: 语料 potency.base=${p.potency.base} vs 实现载荷=${payload}`);
    const texts: TextSource[] = [];
    if (ct !== undefined) texts.push({ label: "语料", text: ct });
    texts.push({ label: "实现文案", text: dt });
    for (const s of texts) {
      const m0 = s.text.match(/\d+/);
      const n = m0 ? Number(m0[0]) : null;
      if (n === null) {
        if (!POTION_NUM_SKIP[id]) fails.push(`药水 ${id}[${s.label}]: 文案里没有数值,无法与实现载荷 ${payload} 对照`);
      } else if (n !== payload && !POTION_NUM_SKIP[id]) {
        fails.push(`药水 ${id}[${s.label}]: 文案数值 ${n} vs 实现载荷=${payload}`);
      }
    }
  }
  return { fails, covered };
}

/** 状态/诅咒卡:文案里的标志词 ↔ CardDef 标志位;文案里的数值 ↔ Effect 载荷 */
interface CurseRule {
  label: string;
  re: RegExp;
  check: (block: string, m: RegExpMatchArray) => string | null;
}
const effectNum = (block: string, names: string[], key: string, want: number) => {
  const re = new RegExp(`Effect::(?:${names.join("|")}) \\{\\s*${key}:\\s*(-?\\d+)`);
  const m = block.match(re);
  return m && Number(m[1]) === want ? null : `实现里没有 Effect::{${names.join("|")}} ${key}=${want}`;
};
const CURSE_RULES: CurseRule[] = [
  { label: "不可打出", re: /\bUnplayable\b/, check: (b) => (/\bcost: Cost::Unplayable,/.test(b) ? null : "实现不是 Cost::Unplayable") },
  { label: "虚无", re: /\bEthereal\b/, check: (b) => (/\bethereal: true,/.test(b) ? null : "实现 ethereal 不是 true") },
  { label: "天生", re: /\bInnate\b/, check: (b) => (/\binnate: true,/.test(b) ? null : "实现 innate 不是 true") },
  { label: "消耗", re: /\bExhaust\b/, check: (b) => (/\bexhaust: true,/.test(b) ? null : "实现 exhaust 不是 true") },
  { label: "不可移除", re: /Cannot be removed from your deck/, check: (b) => (/\bunremovable: true,/.test(b) ? null : "实现 unremovable 不是 true") },
  { label: "回合末掉血", re: /At the end of your turn, take (\d+) damage/, check: (b, m) => effectNum(b, ["DamageSelf", "LoseHp"], "amount", Number(m[1])) },
  {
    label: "回合末上状态",
    re: /At the end of your turn, gain (\d+) (Weak|Frail)/,
    check: (b, m) => {
      const re = new RegExp(`Effect::AddSelfStatus \\{\\s*status: Status::${m[2]},\\s*n: (-?\\d+)`);
      const mm = b.match(re);
      return mm && Number(mm[1]) === Number(m[1]) ? null : `实现里没有对己方上 ${m[2]} ${m[1]} 层的 AddSelfStatus`;
    },
  },
  {
    label: "抽到掉能",
    re: /drawn, lose (\d+) Energy/,
    check: (b, m) => (new RegExp(`Effect::GainEnergy \\{\\s*n: -${m[1]} \\}`).test(b) ? null : `实现里没有 GainEnergy n=-${m[1]}`),
  },
  {
    label: "出牌上限",
    re: /cannot play more than (\d+) cards this turn/,
    check: (b, m) => (new RegExp(`Effect::PlayLimitWhileInHand \\{\\s*max: ${m[1]}`).test(b) ? null : `实现里没有 PlayLimitWhileInHand max=${m[1]}`),
  },
  { label: "手牌掉血", re: /While in hand, lose (\d+) HP when other cards are played/, check: (b, m) => effectNum(b, ["LoseHpOnOtherCardPlayed"], "amount", Number(m[1])) },
  { label: "按手牌掉血", re: /lose HP equal to the number of cards in your hand/, check: (b) => (b.includes("Effect::LoseHpPerHandCard") ? null : "实现里没有 LoseHpPerHandCard") },
  { label: "复制回牌堆", re: /put a copy of this card on top of your draw pile/, check: (b) => (b.includes("Effect::CopySelfToDrawTop") ? null : "实现里没有 CopySelfToDrawTop") },
  { label: "无法逃脱", re: /There is no escape from this curse/, check: (b) => (b.includes("Effect::SelfToHandOnExhaust") ? null : "实现里没有 SelfToHandOnExhaust") },
  { label: "失去生命上限", re: /lose (\d+) Max HP/, check: (b, m) => effectNum(b, ["LoseMaxHpOnRemoved"], "n", Number(m[1])) },
];
function checkCurseText(
  impls: Map<string, CardImpl>,
  corpus: (id: string) => string | undefined,
): { fails: string[]; covered: number } {
  const fails: string[] = [];
  let covered = 0;
  for (const id of ourCards) {
    const c = cardByGame.get(id);
    if (!c || !["curse", "status"].includes(c.type.toLowerCase())) continue;
    covered++;
    const impl = impls.get(id);
    if (!impl) {
      fails.push(`状态/诅咒 ${id}: cards.rs 里没有该 CardDef`);
      continue;
    }
    const ct0 = corpus(id);
    const sources: TextSource[] = [];
    if (ct0 !== undefined) sources.push({ label: "语料", text: flatBase(norm(ct0)) });
    sources.push({ label: "实现文案", text: flatBase(impl.text) });
    let matched = false;
    for (const rule of CURSE_RULES) {
      const hits = sources.map((s) => ({ s, m: s.text.match(rule.re) })).filter((h) => h.m !== null);
      if (hits.length === 0) continue;
      matched = true;
      if (hits.length !== sources.length) fails.push(`状态/诅咒 ${id}: 规则[${rule.label}]只在一侧文案命中(${hits.map((h) => h.s.label).join(",")}),两边不一致`);
      for (const h of hits) {
        const err = rule.check(impl.block, h.m!);
        if (err) fails.push(`状态/诅咒 ${id}[${h.s.label}] 规则[${rule.label}]: ${err}`);
      }
    }
    if (!matched) fails.push(`状态/诅咒 ${id}: 文案没被任何规则覆盖(补规则或登记)`);
    if (ct0 !== undefined) {
      const ct = flatBase(norm(ct0));
      const explained = new Set<string>();
      for (const rule of CURSE_RULES) {
        const m = ct.match(rule.re);
        if (m?.[1] !== undefined) explained.add(String(Number(m[1])));
      }
      for (const tok of ct.match(/\d+/g) ?? []) if (!explained.has(String(Number(tok)))) fails.push(`状态/诅咒 ${id}: 语料文案里的数字 ${tok} 没有对应实现规则`);
    }
  }
  return { fails, covered };
}

// ---- 卡牌(战斗/技能/能力)文案 ↔ 实现 ----
//
// 与遗物/药水/状态同一套路:规则表只记"文案正则 + 实现断言",数字两边各自抽取。
// 一份正则同时跑在 语料文案 与 实现文案 上,每条命中都要与实现载荷对账,覆盖四个面:
//   ①数值(伤害/格挡/层数/抽牌/回能/掉血/回血/塞牌/循环次数)↔ Effect 载荷;
//   ②触发词(Whenever / At the start of your turn / At the end of your turn / When drawn /
//     While in hand / If … / for each / X times / twice)↔ 触发点(effects / on_draw /
//     on_end_turn / in_hand / 能力载荷);
//   ③关键词标记(Exhaust / Ethereal / Innate / Retain / 无限升级)↔ CardDef 字段;
//   ④升级差异:基础档查基础 effects,升级档查 upgrade 里的 effects(未给则沿用基础).
// 覆盖守卫:范围内卡的文案里出现数字或条件词,却没被任何规则命中、也没登记 -> FAIL。

type Level = "base" | "up";

/** 从效果表里取 (变体, 字段) 的整数;status 给定时只认该状态的能力载荷 */
function effVal(effs: EffRef[], variant: string, field: string, status?: string): number | null {
  for (const e of effs) {
    if (e.variant !== variant) continue;
    if (status !== undefined && e.arg !== `Status::${status}` && e.fields.get("status") !== `Status::${status}`) continue;
    const v = e.fields.get(field);
    if (v !== undefined && /^-?\d+$/.test(v)) return Number(v);
  }
  return null;
}
function hasEff(effs: EffRef[], variant: string, status?: string): boolean {
  return effs.some(
    (e) => e.variant === variant && (status === undefined || e.arg === `Status::${status}` || e.fields.get("status") === `Status::${status}`),
  );
}
/** 取"能量个数":语料用 @RE 重复,实现用 (N) */
const capEnergy = (m: RegExpMatchArray) => (m[2] !== undefined ? Number(m[2]) : (m[3]!.match(/@RE/g) ?? []).length);
/** 取第一个存在的捕获组整数 */
const capAny = (m: RegExpMatchArray) => {
  for (let i = 1; i < m.length; i++) if (m[i] !== undefined) return Number(m[i]);
  return null;
};

/**
 * 一条卡面规则。re 带 g,在两份文案上逐个命中;num 取文案数值(undefined = 纯触发/标记规则);
 * needs 列出"本规则只在这些效果变体存在时才适用",不满足则不报错(数字交给覆盖守卫兜底)。
 */
interface CardRule {
  label: string;
  re: RegExp;
  num?: (m: RegExpMatchArray) => number | null;
  needs?: string[];
  skip?: string[];
  check: (effs: EffRef[], want: number | null, m: RegExpMatchArray, impl: CardImpl, level: Level) => string | null;
}
/** 构造"文案数值 = 载荷字段"的规则:pairs 里任一 (变体, 字段) 命中且不等于文案值即报错 */
function numRule(
  label: string,
  re: RegExp,
  pairs: [string, string][],
  opts: {
    status?: (m: RegExpMatchArray) => string | undefined;
    needs?: string[];
    cap?: (m: RegExpMatchArray) => number | null;
    scale?: (n: number) => number;
    skip?: string[];
    check?: (effs: EffRef[], want: number | null, m: RegExpMatchArray) => string | null;
  } = {},
): CardRule {
  return {
    label,
    re,
    needs: opts.needs ?? [...new Set(pairs.map((p) => p[0]))],
    skip: opts.skip,
    num: opts.cap ?? ((m) => (m[1] !== undefined ? Number(m[1]) : null)),
    check:
      opts.check ??
      ((effs, want, m) => {
        if (want === null) return null;
        const status = opts.status?.(m);
        const sc = opts.scale ? opts.scale(want) : want;
        for (const [v, f] of pairs) {
          const got = effVal(effs, v, f, status);
          if (got !== null) return got === sc ? null : `实现 ${v}.${f}=${got},文案 ${want}${opts.scale ? `(→${sc})` : ""}`;
        }
        return `实现里找不到 ${pairs.map((p) => `${p[0]}.${p[1]}`).join("/")}=${sc}${status ? `(${status})` : ""}`;
      }),
  };
}
const flagRule = (
  label: string,
  re: RegExp,
  want: (impl: CardImpl, level: Level) => boolean,
  what: string,
): CardRule => ({
  label,
  re,
  check: (_e, _w, _m, impl, level) => (want(impl, level) ? null : `${what}=${want(impl, level)}`),
});

const CARD_RULES: CardRule[] = [
  // ①数值
  numRule(
    "伤害",
    /Deal (?:(\d+)|\{d\}) damage/gi,
    [
      ["Damage", "amount"],
      ["DamageAll", "amount"],
      ["DamageRandom", "amount"],
      ["DamageWithBonus", "amount"],
      ["Reaper", "amount"],
      ["DamageStrengthMult", "amount"],
      ["DamageAndKillMaxHp", "amount"],
      ["DamageAndGoldOnKill", "amount"],
      ["DamageIfVulnerable", "amount"],
      ["DamagePerStrike", "base"],
      ["DamagePerExhausted", "per"],
      ["DamageAllX", "per"],
      ["ExhaustNonAttacks", "damage"],
      ["Bomb", "damage"],
      ["AddSelfStatus", "n"],
    ],
    { skip: ["searing_blow"] },
  ),
  numRule("格挡", /Gain (\d+) Block/gi, [["Block", "amount"], ["BlockPerExhausted", "per"], ["AddSelfStatus", "n"]]),
  numRule("施加状态", /(?:Apply |and )(\d+) (Weak|Vulnerable|Frail)/gi, [["AddTargetStatus", "n"], ["AddAllEnemiesStatus", "n"]], {
    status: (m) => m[2],
  }),
  numRule("获得力量", /Gain (\d+) Strength/gi, [["AddSelfStatus", "n"], ["StrengthIfTargetAttacks", "n"]]),
  numRule("失去力量", /\blose (\d+) Strength/gi, [["AddSelfStatus", "n"]]),
  numRule("敌方失去力量", /[Ee]nemy loses (\d+) Strength(?! this turn)/gi, [["AddTargetStatus", "n"]], { scale: (n) => -n }),
  numRule("敌方本回合失去力量", /[Ee]nemy loses (\d+) Strength this turn/gi, [["TargetLoseStrengthThisTurn", "n"]]),
  numRule("抽牌", /Draw (\d+) cards?/gi, [["Draw", "n"], ["DrawIfNoAttacks", "n"], ["DamageIfVulnerable", "draw"], ["AddSelfStatus", "n"]]),
  numRule("回能", /Gain (\((\d+)\)|((?:@RE\s*)+))/gi, [["GainEnergy", "n"], ["EnergyOnExhaust", "n"], ["DamageIfVulnerable", "energy"]], {
    cap: capEnergy,
  }),
  numRule("掉血", /Lose (\d+) HP/gi, [["LoseHp", "amount"]]),
  numRule("回血", /Heal (\d+) HP/gi, [["Heal", "amount"]]),
  numRule("塞牌进手", /Add (\d+) (Wounds?|Burns?|Dazed)/gi, [["AddCardToHand", "n"]]),
  numRule(
    "次数",
    /(\d+) times/gi,
    [
      ["Damage", "times"],
      ["DamageAll", "times"],
      ["DamageRandom", "times"],
      ["DamageWithBonus", "times"],
      ["DamageAndKillMaxHp", "times"],
      ["DamageAndGoldOnKill", "times"],
      ["DamageStrengthMult", "mult"],
    ],
  ),
  numRule("定时炸弹", /At the end of (\d+) turns/gi, [["Bomb", "turns"]]),
  numRule("本回合末失去力量", /At the end of this turn, lose (\d+) Strength/gi, [["AddSelfStatus", "n"]], {
    status: () => "LoseStrength",
  }),
  numRule("按 Strike 加成", /(?:plus (\d+) damage for each Strike|Deals (\d+) additional damage for ALL your cards)/gi, [
    ["DamagePerStrike", "per"],
  ], { cap: capAny }),
  numRule("复制份数", /Add (?:a copy|(\d+) copies) of that card into your hand/gi, [["CopyFromHand", "copies"]], { cap: capAny }),
  numRule("成长此卡伤害", /Increase this card's damage by (\d+)/gi, [["BonusSelf", "n"]]),
  numRule("击杀加最大生命", /(?:gain (\d+) Max HP|raise your Max HP by (\d+))/gi, [["DamageAndKillMaxHp", "max_hp"]], { cap: capAny }),
  numRule("击杀加金币", /gain (\d+) Gold/gi, [["DamageAndGoldOnKill", "gold"]]),
  numRule("自获易伤", /Gain (\d+) Vulnerable/gi, [["AddSelfStatus", "n"]]),
  numRule("疲惫反伤", /they take (\d+) damage/gi, [["AddSelfStatus", "n"]]),
  numRule("随机无色牌", /Add (\d+) random Colorless/gi, [["AddRandomColorlessToHand", "n"]]),
  numRule("洗入随机牌", /Shuffle (\d+) random (?:Skills|Attacks) into your draw pile/gi, [["AddRandomToDrawFree", "n"]]),
  numRule("消耗至多", /Exhaust up to (\d+) cards/gi, [["ExhaustUpTo", "n"]]),
  numRule("消耗若干手牌", /Exhaust (\d+) cards?/gi, [["ExhaustRandomInHand", "n"], ["ExhaustUpTo", "n"]], {
    needs: ["ExhaustRandomInHand", "ExhaustUpTo", "ExhaustFromHand"],
    check: (effs, want) =>
      want === null
        ? null
        : effVal(effs, "ExhaustRandomInHand", "n") === want ||
            effVal(effs, "ExhaustUpTo", "n") === want ||
            (want === 1 && hasEff(effs, "ExhaustFromHand"))
          ? null
          : `实现里没有消耗 ${want} 张手牌的效果`,
  }),
  numRule("随机取攻", /Put (\d+) random Attacks from your draw pile/gi, [["RandomFromDrawToHand", "n"]]),
  numRule("发现选牌", /Choose (\d+) of (\d+) random/gi, [["OfferRandomCardsFromClass", "n"]], { cap: (m) => Number(m[2]) }),
  numRule("获得人造制品", /Gain (\d+) Artifact/gi, [["AddSelfStatus", "n"]]),
  numRule("禁格挡回合", /cannot gain Block from cards for (\d+) turns/gi, [["AddSelfStatus", "n"]]),
  numRule("双发次数", /your next (\d+) Attacks?/gi, [["AddSelfStatus", "n"]]),
  numRule("费用上限", /Reduce the cost of (?:all|a random) cards? in your hand to (\d+)/gi, [["CapHandCost", "cap"]]),
  {
    label: "升级费用",
    re: /Costs (\d+)\./gi,
    num: (m) => Number(m[1]),
    check: (_e, want, _m, impl, level) => {
      if (level !== "up" || want === null) return null;
      const c = impl.block.match(/\n\s{8}upgrade: up(?:_[a-z_]+)?!\(\s*Some\(Cost::(\w+)(?:\((\d+)\))?\)/);
      if (!c) return null;
      if (!c[2]) return `升级文案说 Costs ${want},实现升级费用 Cost::${c[1]}(非固定值)`;
      return Number(c[2]) === want ? null : `实现升级费用 Cost::Fixed(${c[2]}),文案 ${want}`;
    },
  },
  // ②触发词 ↔ 触发点
  {
    label: "抽到触发",
    re: /(?:Whenever this card is drawn|When drawn)/g,
    check: (_e, _w, _m, impl) => (impl.onDraw.length > 0 ? null : "文案说抽到触发,实现 on_draw 为空"),
  },
  {
    label: "手牌持续",
    re: /While in hand/g,
    check: (_e, _w, _m, impl) => (impl.inHand.length > 0 ? null : "文案说在手牌持续生效,实现 in_hand 为空"),
  },
  {
    label: "回合末触发",
    re: /At the end of your turn/g,
    check: (e, _w, _m, impl) =>
      impl.onEndTurn.length > 0 || ["DamageSelf", "LoseHp", "LoseHpPerHandCard", "AddSelfStatus", "CopySelfToDrawTop"].some((v) => hasEff(e, v))
        ? null
        : "文案说回合末结算,实现 on_end_turn 为空且没有对应载荷",
  },
  {
    label: "本回合末触发",
    re: /At the end of this turn/g,
    check: (e, _w, _m, impl) => (impl.onEndTurn.length > 0 || hasEff(e, "AddSelfStatus", "LoseStrength") ? null : "文案说本回合末结算,实现里没有 LoseStrength"),
  },
  {
    label: "回合开始触发",
    re: /At the start of your turn/g,
    check: (e) => (hasEff(e, "AddSelfStatus") ? null : "文案说回合开始结算,实现里没有对应能力载荷"),
  },
  {
    label: "每当触发",
    re: /Whenever /g,
    check: (e, _w, _m, impl) =>
      hasEff(e, "AddSelfStatus") || impl.onDraw.length > 0 || impl.inHand.length > 0 ? null : "文案说'每当',实现里没有对应能力载荷",
  },
  {
    label: "每个",
    re: /for each/g,
    skip: ["blood_for_blood"],
    check: (e) =>
      ["DamagePerExhausted", "BlockPerExhausted", "DamagePerStrike", "DamagePerDrawPile", "LoseHpPerHandCard"].some((v) => hasEff(e, v))
        ? null
        : "文案说'每个/每张',实现里没有对应按数量结算的效果",
  },
  { label: "X 次", re: /\bX times/g, check: (e) => (hasEff(e, "DamageAllX") ? null : "文案说 X 次,实现里没有 DamageAllX") },
  {
    label: "两次",
    re: /\btwice\b/g,
    check: (e) =>
      e.some((x) => x.fields.get("times") === "2") || hasEff(e, "CopyFromHand") || hasEff(e, "AddSelfStatus", "DoubleTap")
        ? null
        : "文案说'两次',实现里没有对应语义",
  },
  {
    label: "全体",
    re: /to ALL enemies/g,
    check: (e, _w, _m, impl) =>
      ["DamageAll", "DamageAllX", "AddAllEnemiesStatus", "Reaper", "Bomb"].some((v) => hasEff(e, v)) ||
      hasEff(e, "AddSelfStatus", "FireBreathing") ||
      hasEff(e, "AddSelfStatus", "Combust") ||
      hasEff(e, "AddSelfStatus", "Panache") ||
      impl.target === "All" ||
      impl.upFlags.target === "All"
        ? null
        : "文案说对全体敌人,实现里没有全体效果",
  },
  { label: "若易伤", re: /If the enemy (?:has|is) Vulnerable/g, check: (e) => (hasEff(e, "DamageIfVulnerable") ? null : "文案说若易伤,实现里没有 DamageIfVulnerable") },
  {
    label: "若击杀",
    re: /If (?:this kills|Fatal)/g,
    check: (e) => (hasEff(e, "DamageAndKillMaxHp") || hasEff(e, "DamageAndGoldOnKill") ? null : "文案说若击杀,实现里没有击杀收益效果"),
  },
  { label: "若被消耗", re: /If this card is Exhausted/g, check: (e) => (hasEff(e, "EnergyOnExhaust") ? null : "文案说若被消耗,实现里没有 EnergyOnExhaust") },
  {
    label: "若敌人将攻击",
    re: /If the enemy intends to attack/g,
    check: (e) => (hasEff(e, "StrengthIfTargetAttacks") ? null : "文案说若敌人将攻击,实现里没有 StrengthIfTargetAttacks"),
  },
  { label: "若无攻击牌", re: /If you have no Attacks in your hand/g, check: (e) => (hasEff(e, "DrawIfNoAttacks") ? null : "文案说若无攻击牌,实现里没有 DrawIfNoAttacks") },
  { label: "洗牌进抽牌堆", re: /(?:Shuffle a|Add a) (Wound|Dazed|Burn) into your draw pile/g, check: (e, _w, m) => (e.some((x) => x.variant === "AddCardToDraw" && x.fields.get("id") === `"${m[1]!.toLowerCase()}"`) ? null : `实现里没有 AddCardToDraw id=${m[1]!.toLowerCase()}`) },
  { label: "塞牌进弃牌堆", re: /Add a (Burn) to your discard pile/g, check: (e, _w, m) => (e.some((x) => x.variant === "AddCardToDiscard" && x.fields.get("id") === `"${m[1]!.toLowerCase()}"`) ? null : `实现里没有 AddCardToDiscard id=${m[1]!.toLowerCase()}`) },
  { label: "塞自身副本进弃牌堆", re: /Add a copy of this card into your discard pile/g, check: (e) => (hasEff(e, "AddSelfToDiscard") ? null : "实现里没有 AddSelfToDiscard") },
  { label: "洗回抽牌堆", re: /Shuffle your discard pile into your draw pile/g, check: (e) => (hasEff(e, "ShuffleDiscardIntoDraw") ? null : "实现里没有 ShuffleDiscardIntoDraw") },
  { label: "置顶手牌", re: /Put a card from your hand (?:onto|on) top of your draw pile|Put a card from your hand onto the top of your draw pile/g, check: (e) => (hasEff(e, "TopFromHand") ? null : "实现里没有 TopFromHand") },
  { label: "掘出消耗堆", re: /Put a card from your [Ee]xhaust pile into your hand/g, check: (e) => (hasEff(e, "FromExhaustToHand") ? null : "实现里没有 FromExhaustToHand") },
  { label: "弃牌堆置顶", re: /Put a card from your discard pile on top of your draw pile/g, check: (e) => (hasEff(e, "FromDiscardToDrawTop") ? null : "实现里没有 FromDiscardToDrawTop") },
  { label: "复制手牌", re: /Copy an Attack or Power card in your hand/g, check: (e) => (hasEff(e, "CopyFromHand") ? null : "实现里没有 CopyFromHand") },
  // ③关键词标记
  flagRule("消耗标记", /(?:^| )Exhaust\./g, (i, l) => (l === "up" ? i.upFlags.exhaust ?? i.exhaust : i.exhaust), "实现 exhaust"),
  flagRule("不再消耗", /No longer Exhausts\./g, (i) => i.upFlags.exhaust === false, "实现升级 exhaust"),
  flagRule("虚无标记", /\bEthereal\./g, (i, l) => (l === "up" ? i.upFlags.ethereal ?? i.ethereal : i.ethereal), "实现 ethereal"),
  flagRule("天生标记", /\bInnate\./g, (i, l) => (l === "up" ? i.upFlags.innate ?? i.innate : i.innate), "实现 innate"),
  flagRule("保留标记", /\bRetain\./g, (i, l) => (l === "up" ? i.upFlags.retain ?? i.retain : i.retain), "实现 retain"),
  flagRule("无限升级", /Can be upgraded any number of times\./g, (i) => i.multiUpgrade, "实现 multi_upgrade"),
];
/** 关键词的反向核对:实现标记为真,文案里却没有关键词 */
const CARD_KEYWORDS: { label: string; re: RegExp; flag: (i: CardImpl, l: Level) => boolean }[] = [
  { label: "Exhaust", re: /(?:^| )Exhaust\./, flag: (i, l) => (l === "up" ? i.upFlags.exhaust ?? i.exhaust : i.exhaust) },
  { label: "Ethereal", re: /\bEthereal\./, flag: (i, l) => (l === "up" ? i.upFlags.ethereal ?? i.ethereal : i.ethereal) },
  { label: "Innate", re: /\bInnate\./, flag: (i, l) => (l === "up" ? i.upFlags.innate ?? i.innate : i.innate) },
  { label: "Retain", re: /\bRetain\./, flag: (i, l) => (l === "up" ? i.upFlags.retain ?? i.retain : i.retain) },
];
/** 文案里出现即必须被某条规则覆盖的条件词 */
const TRIGGER_SCAN: RegExp[] = [
  /\bWhenever\b/g,
  /At the start of /g,
  /At the end of /g,
  /At the start of your turn/g,
  /At the end of your turn/g,
  /At the end of this turn/g,
  /When drawn/g,
  /While in hand/g,
  /for each/g,
  /\bX times/g,
  /\btwice\b/g,
  /to ALL enemies/g,
  /\bIf\b/g,
  /Can only be played if/g,
  /Can be upgraded any number of times/g,
  /No longer Exhausts/g,
];
/** 文案里"实现了但无法用载荷直测"的数字/条件词:显式登记原因(与遗物 RELIC_NUM_SKIP 同理) */
const CARD_TEXT_SKIP: Record<string, { sub: RegExp; reason: string }[]> = {
  combust: [{ sub: /lose 1 HP/, reason: "每回合掉 1 HP 由 Status::Combust 的结算常量承载(combat.rs),不在 CardDef 载荷里" }],
  brutality: [{ sub: /lose 1 HP/, reason: "每回合掉 1 HP 由 Status::Brutality 的结算常量承载(combat.rs),不在 CardDef 载荷里" }],
  panache: [{ sub: /play 5 cards in a single turn/, reason: "每 5 张的阈值由 Status::Panache 的结算常量承载(combat.rs),载荷 n 是伤害值" }],
  corruption: [{ sub: /Skills cost 0/, reason: "技能降为 0 费由 Status::Corruption 的结算承载,载荷 n 是层数" }],
  infernal_blade: [{ sub: /costs? 0 this turn/, reason: "送给的牌 0 费由 AddRandomAttackToHand 的结算承载" }],
  discovery: [{ sub: /costs? 0 this turn/, reason: "选到的牌 0 费由 OfferRandomCardsFromClass 的结算承载" }],
  transmutation: [{ sub: /costs? 0 this turn/, reason: "送到的牌 0 费由 AddRandomColorlessXToHand 的结算承载" }],
  chrysalis: [{ sub: /costs? 0 this combat/, reason: "送到的牌 0 费由 AddRandomToDrawFree 的结算承载" }],
  metamorphosis: [{ sub: /costs? 0 this combat/, reason: "送到的牌 0 费由 AddRandomToDrawFree 的结算承载" }],
  forethought: [{ sub: /costs? 0 until played/, reason: "放到牌堆底的牌 0 费由 ToDrawBottomFromHand 的结算承载" }],
  madness: [{ sub: /to 0 this combat/, reason: "随机一张降为 0 费由 FreeRandomInHand 的结算承载,载荷里没有数值" }],
  blood_for_blood: [
    { sub: /Costs \(?1\)? less/, reason: "按本场掉血次数降费由 combat.rs 的费用修正承载,不在 CardDef 载荷里" },
    { sub: /for each time you lose HP/, reason: "同上:降费节奏由 combat.rs 的费用修正承载,载荷里没有 per" },
  ],
  berserk: [{ sub: /gain \(1\)/, reason: "回合开始回能由 Status::Berserk 的结算承载,载荷 n=1 是层数" }],
  searing_blow: [{ sub: /Deal (?:\(?\d+\)?|\{d\}) damage/, reason: "无限升级(multi_upgrade):升级档文案是累计加成后的显示值,载荷由 multi_upgrade 增量承载" }],
  clash: [{ sub: /Can only be played if every card in your hand is an Attack/, reason: "只能在手牌全为攻击时打出由 combat.rs 的 can_play 判定承载" }],
};

function checkCardText(impls: Map<string, CardImpl>): { fails: string[]; sides: number; cards: number; rules: number } {
  const fails: string[] = [];
  let sides = 0;
  let cards = 0;
  for (const id of ourCards) {
    const c = cardByGame.get(id);
    const impl = impls.get(id);
    if (!c || !impl) continue;
    const type = c.type.toLowerCase();
    if (type === "curse" || type === "status") continue;
    cards++;
    for (const level of ["base", "up"] as const) {
      if (level === "up" && impl.upMacro === "None") continue;
      const effs = level === "up" ? impl.upEffects ?? impl.effects : impl.effects;
      const implText = flatBase(level === "up" ? impl.upText : impl.text);
      const corpusText = level === "up" ? flatUp(norm(c.text)) : flatBase(norm(c.text));
      for (const [side, text] of [["实现", implText], ["语料", corpusText]] as const) {
        if (text === "") continue;
        sides++;
        const covered = new Array<boolean>(text.length).fill(false);
        for (const rule of CARD_RULES) {
          if (rule.skip?.includes(id)) continue;
          for (const m of text.matchAll(rule.re)) {
            const want = rule.num ? rule.num(m) : null;
            if (rule.needs && !rule.needs.some((v) => effs.some((e) => e.variant === v))) continue;
            for (let k = m.index ?? 0; k < (m.index ?? 0) + m[0]!.length; k++) covered[k] = true;
            const err = rule.check(effs, want, m, impl, level);
            if (err) fails.push(`卡牌 ${id}/${level}[${side}] 规则[${rule.label}]: ${err}  <- "${m[0]}"`);
          }
        }
        for (const kw of CARD_KEYWORDS) {
          const inText = kw.re.test(text);
          const flag = kw.flag(impl, level);
          if (inText !== flag) fails.push(`卡牌 ${id}/${level}[${side}]: 关键词 ${kw.label} 文案${inText ? "有" : "无"} vs 实现${flag ? "有" : "无"}`);
        }
        const allowed = (CARD_TEXT_SKIP[id] ?? []).flatMap((s) =>
          [...text.matchAll(new RegExp(s.sub.source, "g"))].map((m) => [m.index ?? 0, (m.index ?? 0) + m[0]!.length] as const),
        );
        const excused = (at: number) => allowed.some(([a, b]) => at >= a && at < b);
        for (const m of text.matchAll(/\d+/g)) {
          const at = m.index ?? 0;
          if (!covered[at] && !excused(at)) fails.push(`卡牌 ${id}/${level}[${side}]: 数字 ${m[0]} 没有对应规则(补规则或登记 CARD_TEXT_SKIP)`);
        }
        for (const scan of TRIGGER_SCAN) {
          for (const m of text.matchAll(scan)) {
            const at = m.index ?? 0;
            if (!covered[at] && !excused(at)) fails.push(`卡牌 ${id}/${level}[${side}]: 条件词 "${m[0]}" 没有对应规则(补规则或登记)`);
          }
        }
      }
    }
  }
  return { fails, sides, cards, rules: CARD_RULES.length };
}

const textImplFails: string[] = [];
let textImplChecks = 0;
let cardTextSummary = "";
if (want("relics")) {
  const r = checkRelicText(relicImpl, (id) => relicByGame.get(id)?.text, RELIC_SRC_FILES);
  textImplFails.push(...r.fails);
  textImplChecks += r.covered;
}
if (want("potions")) {
  const r = checkPotionText(potionImpl, (id) => potionByGame.get(id));
  textImplFails.push(...r.fails);
  textImplChecks += r.covered;
}
if (want("cards")) {
  const r = checkCurseText(cardImpl, (id) => cardByGame.get(id)?.text);
  textImplFails.push(...r.fails);
  textImplChecks += r.covered;
  const cr = checkCardText(cardImpl);
  textImplFails.push(...cr.fails);
  textImplChecks += cr.sides * cr.rules;
  cardTextSummary = `战斗/技能/能力牌 ${cr.cards} 张 x 基础/升级两档 x ${cr.rules} 条规则(${cr.sides} 侧文案),命中 ${cr.sides * cr.rules}`;
}

// ---- 守卫一:口径覆盖(未覆盖即报错) ----
// 任何"有数值效果却没人比对、又没明确登记"的内容都直接判失败,不再只是报告里列一行。
// 这正是哨卫"消耗回能"那类静默漏检的根源:改坏一处口径,守卫必须响。
const guardFails: string[] = [];
if (want("cards")) {
  if (blind.length > 0) {
    guardFails.push(`卡片口径未覆盖 ${blind.length} 张(加探针或在 CARD_NOT_COMPARED 登记):`);
    for (const b of blind) guardFails.push(`  ${b}`);
  }
  if (fieldBlind.length > 0) {
    guardFails.push(`语料数值字段没进口径 ${fieldBlind.length} 处:`);
    for (const b of fieldBlind) guardFails.push(`  ${b}`);
  }
  if (unaudited.length > 0) guardFails.push(`没有任何场景的牌: ${unaudited.join(", ")}`);
}
if (want("potions") && potionBlind.length > 0)
  guardFails.push(`药水口径未覆盖(加场景或登记 POTION_NOT_COMPARED): ${potionBlind.join(", ")}`);
if (want("relics")) {
  const unclassifiedRelics = relicReasons["其它(未分类)"] ?? [];
  if (unclassifiedRelics.length > 0) guardFails.push(`遗物既没被 RELIC_RULES 覆盖也没登记原因: ${unclassifiedRelics.join(", ")}`);
}
if (dataIssues.length > 0) guardFails.push(`静态数据(费用/类型/稀有度)对语料不一致 ${dataIssues.length} 处`);
if (textImplFails.length > 0) {
  guardFails.push(`文案↔实现 双向校验不一致 ${textImplFails.length} 处:`);
  for (const f of textImplFails) guardFails.push(`  ${f}`);
}

// ---- 守卫二:内容登记表(新增未登记即报错) ----
// 本作 --dump 或语料里出现了登记表没有的卡/遗物/药水,或者登记表里的 tag 与工具当前
// 状态(audited/registered/gated/corpus)对不上,都判失败。新增内容必须显式登记。
type RegTag = "audited" | "registered" | "gated" | "corpus";
const regKey = (kind: string, id: string) => `${kind}/${id}`;
/** 工具当前状态:每个 kind 的 id -> tag */
const computedTags = new Map<string, RegTag>();
{
  const cardsWithScenario = coveredCardIds;
  for (const id of ourCards) {
    const tag: RegTag = cardsWithScenario.has(id) ? "audited" : CARD_NOT_COMPARED[id] ? "registered" : "corpus";
    computedTags.set(regKey("cards", id), tag);
  }
  for (const id of cardByGame.keys()) if (!computedTags.has(regKey("cards", id))) computedTags.set(regKey("cards", id), "corpus");

  for (const id of ourPotions) {
    if (gated.has(`potion/${id}`)) continue;
    const tag: RegTag = POTION_PROBED[id] || !potionNoFacts.has(id) ? "audited" : "registered";
    computedTags.set(regKey("potions", id), tag);
  }
  for (const id of potionByGame.keys()) if (!computedTags.has(regKey("potions", id))) computedTags.set(regKey("potions", id), "corpus");

  for (const id of ourRelics) {
    if (gated.has(`relic/${id}`)) continue;
    const probed = list.some((s) => s.kind === "relics" && s.probe && s.id === id);
    computedTags.set(regKey("relics", id), relicAudited.has(id) || probed ? "audited" : "registered");
  }
  for (const id of relicByGame.keys()) if (!computedTags.has(regKey("relics", id))) computedTags.set(regKey("relics", id), "corpus");

  const gatedKind: Record<string, string> = { card: "cards", potion: "potions", relic: "relics" };
  for (const g of gated) {
    const [kind, id] = g.split("/");
    const k = gatedKind[kind ?? ""];
    if (k && id) computedTags.set(regKey(k, id), "gated");
  }
}
/** 登记表里的一行:kind id tag */
function readRegistry(): Map<string, RegTag> {
  const m = new Map<string, RegTag>();
  if (!existsSync(REGISTRY)) return m;
  for (const raw of readFileSync(REGISTRY, "utf8").split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const [kind, id, tag] = line.split(/\s+/);
    if (!kind || !id || !tag) continue;
    m.set(regKey(kind, id), tag as RegTag);
  }
  return m;
}
const registry = readRegistry();
if (ONLY === undefined) {
  if (WRITE_REGISTRY) {
    const lines = ['# 内容登记表:审计守卫二用它判定"新增未登记"',
      "# 格式:<kind> <id> <tag>,tag = audited(有断言) | registered(已登记不比对) | gated(刻意未实现) | corpus(语料有本作未实现)",
      "# 新增卡/遗物/药水后:补断言 -> audited;登记不比对 -> registered;跑 --write-registry 重写本表。",
      ...Array.from(computedTags.entries()).sort().map(([k, t]) => `${k.split("/")[0]} ${k.split("/").slice(1).join("/")} ${t}`)];
    writeFileSync(REGISTRY, lines.join("\n") + "\n");
  } else {
    for (const [k, tag] of computedTags) {
      const have = registry.get(k);
      if (!have) guardFails.push(`登记表缺少 ${k}(新增未登记;先加断言/登记,再补进 audit_registry.txt,tag=${tag})`);
      else if (have !== tag) guardFails.push(`登记表 ${k} 记的是 ${have},工具当前是 ${tag}(内容状态变了,更新登记表)`);
    }
    for (const k of registry.keys()) if (!computedTags.has(k)) guardFails.push(`登记表里的 ${k} 本作与语料都没有(已过期,删掉)`);
  }
}

// ---- 自检:历史盲区回归(--selftest) ----
//
// 把本项目此前"靠人肉发现、工具当时抓不到"的盲区逐条喂回工具,确认每条现在都被拦下。
// 两条路线:
//  - 语料侧(真·端到端):在临时副本里改坏语料的那半句(数字/目标/条件),用真实审计
//    子进程跑一遍,要求 exit != 0 且输出里点名对应场景;
//  - 实现侧:对真实沙盒输出做定向篡改(模拟当年那份有 bug 的实现),要求 compareSpec
//    对它报出不一致 —— 冻结"场景 + 比对逻辑"这条链,工具口径退化时同样会 FAIL。
// 任一漏检 => FAIL 行,进程 exit != 0。

interface LooseCard {
  id: string;
  text?: string;
  values?: Record<string, number | null>;
}
interface LooseRelic {
  id: string;
  text?: string;
}
interface LoosePotion {
  id: string;
  text?: string;
}
interface SpotCase {
  spot: string;
  expect: string;
  run: () => string | null;
}

/** 在临时语料副本上改坏一处,再跑真实审计子进程,要求它 exit != 0 且点名 marker。 */
function corpusCase(
  spot: string,
  mutate: (c: LooseCard[], r: LooseRelic[], p: LoosePotion[]) => void,
  marker: string,
): SpotCase {
  return {
    spot,
    expect: marker,
    run: () => {
      const dir = mkdtempSync(join(tmpdir(), "spire-corpus-"));
      for (const f of ["cards", "potions", "relics"]) copyFileSync(join(DEFAULT_CORPUS, `${f}.json`), join(dir, `${f}.json`));
      const cards = JSON.parse(readFileSync(join(dir, "cards.json"), "utf8")) as LooseCard[];
      const relics = JSON.parse(readFileSync(join(dir, "relics.json"), "utf8")) as LooseRelic[];
      const potions = JSON.parse(readFileSync(join(dir, "potions.json"), "utf8")) as LoosePotion[];
      mutate(cards, relics, potions);
      writeFileSync(join(dir, "cards.json"), JSON.stringify(cards));
      writeFileSync(join(dir, "relics.json"), JSON.stringify(relics));
      writeFileSync(join(dir, "potions.json"), JSON.stringify(potions));
      const r = spawnSync("bun", [join(HERE, "audit_corpus.ts"), "--corpus", dir], { encoding: "utf8", maxBuffer: 1 << 26 });
      rmSync(dir, { recursive: true, force: true });
      const out = `${r.stdout ?? ""}${r.stderr ?? ""}`;
      if (r.status === 0) return "改坏语料后审计仍然 exit 0(期望非 0)";
      if (!out.includes(marker)) return `审计已报错,但输出里没点名 ${marker}`;
      return null;
    },
  };
}

/** 对真实沙盒输出做定向篡改(模拟实现侧回归),要求工具报出 marker 那条不一致。 */
function implCase(spot: string, specName: string, mutate: (rows: Row[]) => void, marker: string): SpotCase {
  return {
    spot,
    expect: specName,
    run: () => {
      const spec = list.find((s) => s.name === specName);
      const rows0 = got.get(specName);
      if (!spec || !rows0) return `沙盒里没有 ${specName} 这一段`;
      const clean = compareSpec(spec, rows0);
      if (clean.hard !== undefined || clean.lines.length > 0) return `未篡改前这条就没过: ${JSON.stringify(clean)}`;
      const rows = structuredClone(rows0);
      mutate(rows);
      const after = compareSpec(spec, rows);
      const text = after.lines.join("\n");
      if (after.hard === undefined && after.lines.length === 0) return `模拟实现回归后工具没报错(期望点出 ${marker})`;
      if (!text.includes(marker) && !(after.hard ?? "").includes(marker)) return `报的错里没有 ${marker}: ${text || after.hard}`;
      return null;
    },
  };
}

/** 对解析出来的 CardImpl 做定向篡改(模拟实现侧回归),要求 checkCardText 报出 marker。 */
function cardImplCase(spot: string, mutate: (m: Map<string, CardImpl>) => void, marker: string): SpotCase {
  return {
    spot,
    expect: marker,
    run: () => {
      const copy = new Map<string, CardImpl>();
      for (const [k, v] of cardImpl)
        copy.set(k, {
          ...v,
          effects: v.effects.map((e) => ({ ...e, fields: new Map(e.fields) })),
          upEffects: v.upEffects?.map((e) => ({ ...e, fields: new Map(e.fields) })) ?? null,
          upFlags: { ...v.upFlags },
        });
      mutate(copy);
      const { fails } = checkCardText(copy);
      return fails.some((f) => f.includes(marker)) ? null : `改坏实现后卡牌文案校验没点名 ${marker}: ${fails.join(" | ") || "(无失败)"}`;
    },
  };
}

function spotCases(): SpotCase[] {
  return [
    // 1 哨卫:升级后消耗回能 2->3(条件效果 [@RE...]);语料写的是 [@RE@RE|@RE@RE@RE]
    corpusCase(
      "哨卫消耗回能(条件效果 [@RE...])",
      (cards) => {
        for (const c of cards) if (c.id === "SENTINEL") c.text = c.text?.replace("[@RE@RE|@RE@RE@RE]", "[@RE@RE|@RE@RE]");
      },
      "cards/sentinel/up_exhaust_energy",
    ),
    // 2 吐火:文本 "Status or Curse",实现曾只认 Status(抽到诅咒白丢伤害)
    corpusCase(
      "吐火认诅咒(文本 Status or Curse)",
      (cards) => {
        for (const c of cards)
          if (c.id === "FIRE_BREATHING") c.text = c.text?.replace(" or {{QueryLink|Cards|type:Curse|Curse}}", "");
      },
      "cards/fire_breathing/base_curse_draw",
    ),
    // 3/4 升级版差异:致盲/绊倒升级后目标从单体变全体
    corpusCase(
      "致盲升级目标变全体",
      (cards) => {
        for (const c of cards) if (c.id === "BLIND") c.text = c.text?.replace("[| to ALL enemies]", "");
      },
      "cards/blind/up_target_count",
    ),
    corpusCase(
      "绊倒升级目标变全体",
      (cards) => {
        for (const c of cards) if (c.id === "TRIP") c.text = c.text?.replace("[| to ALL enemies]", "");
      },
      "cards/trip/up_target_count",
    ),
    // 5 数值在别的机制里相乘:重击吃力量 3 次(升级 5 次)
    corpusCase(
      "重击力量倍率",
      (cards) => {
        for (const c of cards) if (c.id === "HEAVY_BLADE") c.values!.magic = 4;
      },
      "cards/heavy_blade/base_strength_multiplier",
    ),
    // 6 笔尖:第 10 张攻击翻倍(且狂暴这类 DamageWithBonus 也要翻)
    corpusCase(
      "笔尖第 10 张攻击翻倍",
      (cards) => {
        for (const c of cards) if (c.id === "RAMPAGE") c.values!.damage = 9;
      },
      "relics/pen_nib/doubles_tenth_attack_effect",
    ),
    // 7 遗物文案里的数值:靴子 "4 or less unblocked → 5"
    corpusCase(
      "靴子遗物文案阈值",
      (_cards, relics) => {
        for (const r of relics) if (r.id === "THE_BOOT") r.text = r.text?.replace("increase it to 5", "increase it to 6");
      },
      "relics/the_boot/raises_low_hit_to_5",
    ),
    // 8 神化的作用范围:只升打出那一刻的四个牌堆,后续新造的牌不升
    implCase(
      "神化作用范围(不升后续新牌)",
      "cards/apotheosis/base_not_later_cards",
      (rows) => {
        const last = rows[rows.length - 1]!;
        const d = last.st!.discard as string[];
        last.st!.discard = d.map((t) => (t === "burn" ? "burn+" : t));
      },
      "burn_up",
    ),
    // 9 液态记忆:取回的牌本回合 0 费,但 X 费牌除外
    implCase(
      "液态记忆 X 费牌不被免费",
      "potions/liquid_memories/keeps_x_cost",
      (rows) => {
        const last = rows[rows.length - 1]!;
        const costs = last.report!.costs;
        costs[costs.length - 1] = 0;
      },
      "cost",
    ),
    // 10 混乱/费用:随机化后的费用必须落在 0..3(升级降费的牌不能被再减一档)
    implCase(
      "混乱费用落 0..3(升级基线)",
      "relics/snecko_eye/confusion_cost_range",
      (rows) => {
        for (const r of rows) if (r.report && r.report.costs.length > 0) r.report.costs[0] = -1;
      },
      "min_cost",
    ),
    // 守卫一(未覆盖即报错):给一张没有探针的牌塞一句"有数值但解析不了"的文本
    corpusCase(
      "守卫一 未覆盖即报错",
      (cards) => {
        for (const c of cards) if (c.id === "STRIKE_RED") c.text = `${c.text}<br>Whenever you draw this, gain [1|2] Block.`;
      },
      "卡片口径未覆盖",
    ),
    // 守卫二(新增未登记即报错):往语料里塞一张本作没有、登记表也没有的牌
    corpusCase(
      "守卫二 新增未登记即报错",
      (cards) => {
        cards.push({ id: "GUARD_PROBE_CARD", text: "Deal 1 damage.", values: { damage: 1, block: null, magic: null, hits: null } });
      },
      "登记表缺少 cards/guard_probe_card",
    ),
    // 守卫一(数值字段级):改掉语料 values 里一个 magic,让"有字段没进口径"暴露
    corpusCase(
      "守卫一 数值字段未覆盖即报错",
      (cards) => {
        for (const c of cards) if (c.id === "SWORD_BOOMERANG") c.values!.magic = 7;
      },
      "语料数值字段没进口径",
    ),
    // 守卫一(药水侧):把一瓶药水的文本改成解析不出的数值效果
    corpusCase(
      "守卫一 药水未覆盖即报错",
      (_cards, _relics, potions) => {
        for (const p of potions) if (p.id === "FIRE_POTION") p.text = "Whenever you drink this, gain [1|2] Block.";
      },
      "药水口径未覆盖",
    ),
    // 守卫一(遗物侧):把一件已实现遗物的文本改成归不了类的,要求强制登记
    corpusCase(
      "守卫一 遗物未归类即报错",
      (_cards, relics) => {
        for (const r of relics) if (r.id === "BURNING_BLOOD") r.text = "Mysterious aura.";
      },
      "遗物既没被",
    ),
    // 文案↔实现 双向校验:语料侧改坏(遗物/药水/状态各一),静态表必须报出并点名
    corpusCase(
      "静态双向校验 遗物数值(改语料)",
      (_cards, relics) => {
        for (const r of relics) if (r.id === "STONE_CALENDAR") r.text = r.text?.replace("deal 52 damage", "deal 53 damage");
      },
      "遗物 stone_calendar",
    ),
    corpusCase(
      "静态双向校验 药水数值(改语料)",
      (_cards, _relics, potions) => {
        for (const p of potions) if (p.id === "FIRE_POTION") p.text = p.text?.replace("20", "30");
      },
      "药水 fire_potion",
    ),
    corpusCase(
      "静态双向校验 状态数值(改语料)",
      (cards) => {
        for (const c of cards) if (c.id === "BURN") c.text = c.text?.replace("take [2|4]", "take [3|6]");
      },
      "状态/诅咒 burn",
    ),
    // 实现侧改坏(在内存里篡改解析出来的常量),静态表同样必须报出 —— 冻结"实现侧抽取"这条链
    {
      spot: "静态双向校验 遗物常量(改实现)",
      expect: "anchor",
      run: () => {
        const impls = new Map([...relicImpl].map(([k, v]) => [k, { desc: v.desc, fx: new Map(v.fx) }] as const));
        impls.get("anchor")!.fx.set("combat_start_block", "11");
        const { fails } = checkRelicText(impls, (id) => relicByGame.get(id)?.text, RELIC_SRC_FILES);
        return fails.some((f) => f.includes("anchor")) ? null : `改坏实现常量后静态校验没点名 anchor: ${fails.join(" | ") || "(无失败)"}`;
      },
    },
    {
      spot: "静态双向校验 遗物计数器常量(改实现)",
      expect: "pen_nib",
      run: () => {
        const { fails } = checkRelicText(relicImpl, (id) => relicByGame.get(id)?.text, {
          ...RELIC_SRC_FILES,
          "combat.rs": SRC_COMBAT_RS.replace("pen_nib >= 10", "pen_nib >= 9"),
        });
        return fails.some((f) => f.includes("pen_nib")) ? null : `改坏计数器常量后静态校验没点名 pen_nib: ${fails.join(" | ") || "(无失败)"}`;
      },
    },
    {
      spot: "静态双向校验 药水载荷(改实现)",
      expect: "fire_potion",
      run: () => {
        const impls = new Map([...potionImpl].map(([k, v]) => [k, { desc: v.desc, variant: v.variant, payload: new Map(v.payload) }] as const));
        impls.get("fire_potion")!.payload.set("amount", "21");
        const { fails } = checkPotionText(impls, (id) => potionByGame.get(id));
        return fails.some((f) => f.includes("fire_potion")) ? null : `改坏实现载荷后静态校验没点名 fire_potion: ${fails.join(" | ") || "(无失败)"}`;
      },
    },
    {
      spot: "静态双向校验 状态载荷(改实现)",
      expect: "burn",
      run: () => {
        const impls = new Map(cardImpl);
        const b = impls.get("burn")!;
        impls.set("burn", { ...b, block: b.block.replace("Effect::DamageSelf { amount: 2 }", "Effect::DamageSelf { amount: 5 }") });
        const { fails } = checkCurseText(impls, (id) => cardByGame.get(id)?.text);
        return fails.some((f) => f.includes("burn")) ? null : `改坏实现载荷后静态校验没点名 burn: ${fails.join(" | ") || "(无失败)"}`;
      },
    },
    // ---- 卡牌(战斗/技能/能力)文案 ↔ 实现:改坏/恢复(语料侧 + 实现侧) ----
    // 语料侧:基础档数值被改坏,静态表必须点名
    corpusCase(
      "卡牌 数值(改语料基础档)",
      (cards) => {
        for (const c of cards) if (c.id === "STRIKE_RED") c.text = c.text?.replace("Deal [6|9]", "Deal [7|9]");
      },
      "卡牌 strike/base[语料]",
    ),
    // 语料侧:升级档数值被改坏(升级差异)
    corpusCase(
      "卡牌 升级差异(改语料升级档)",
      (cards) => {
        for (const c of cards) if (c.id === "BLUDGEON") c.text = c.text?.replace("[32|42]", "[32|43]");
      },
      "卡牌 bludgeon/up[语料]",
    ),
    // 语料侧:循环次数被改坏(times 字段)
    corpusCase(
      "卡牌 循环次数(改语料)",
      (cards) => {
        for (const c of cards) if (c.id === "SWORD_BOOMERANG") c.text = c.text?.replace("[3|4] times", "[3|5] times");
      },
      "规则[次数]",
    ),
    // 语料侧:关键词标记被抹掉(Exhaust)
    corpusCase(
      "卡牌 关键词标记(改语料)",
      (cards) => {
        for (const c of cards) if (c.id === "IMPERVIOUS") c.text = c.text?.replace("$Exhaust.", "");
      },
      "关键词 Exhaust",
    ),
    // 语料侧:给牌塞一个没有载荷支撑的数字 -> 覆盖守卫必须响
    corpusCase(
      "卡牌 覆盖守卫 数字(改语料)",
      (cards) => {
        for (const c of cards) if (c.id === "STRIKE_RED") c.text = `${c.text}<br>Gain 3 Block.`;
      },
      "没有对应规则",
    ),
    // 语料侧:给牌塞一个没进表的条件词 -> 覆盖守卫必须响
    corpusCase(
      "卡牌 覆盖守卫 条件词(改语料)",
      (cards) => {
        for (const c of cards) if (c.id === "STRIKE_RED") c.text = `${c.text}<br>At the end of time, gain 1 Block.`;
      },
      "条件词",
    ),
    // 实现侧:伤害载荷被改坏(数值 ↔ Effect)
    cardImplCase(
      "卡牌 数值(改实现载荷)",
      (m) => {
        const e = m.get("strike")!.effects.find((x) => x.variant === "Damage")!;
        e.fields.set("amount", "7");
      },
      "卡牌 strike",
    ),
    // 实现侧:消耗标记被改坏(关键词 ↔ CardDef 字段)
    cardImplCase(
      "卡牌 关键词标记(改实现字段)",
      (m) => {
        m.get("offering")!.exhaust = false;
      },
      "Exhaust",
    ),
    // 实现侧:能力载荷被拿掉(触发词 ↔ 触发点)
    cardImplCase(
      "卡牌 触发点(改实现载荷)",
      (m) => {
        const c = m.get("combust")!;
        c.effects = c.effects.filter((e) => e.variant !== "AddSelfStatus");
      },
      "回合末触发",
    ),
  ];
}

function selftestReport(): string[] {
  const cases = spotCases();
  const out: string[] = [];
  out.push(`历史盲区自检(--selftest)  seed=${SEED}`);
  out.push(`证据源: ${CORPUS.replace(REPO + "/", "")}(记自 sts_lightspeed 反编译)`);
  out.push("");
  let caught = 0;
  let missed = 0;
  for (const c of cases) {
    const fail = c.run();
    if (fail === null) {
      caught++;
      out.push(`  [ok]   ${c.spot} -> ${c.expect}`);
    } else {
      missed++;
      out.push(`  [FAIL] ${c.spot}: ${fail}`);
    }
  }
  out.push("");
  out.push(`历史盲区 ${cases.length} 条:抓到 ${caught},漏检 ${missed}`);
  return out;
}

// ---- 报告 ----
const totals: Record<string, number> = { cards: 0, potions: 0, relics: 0 };
for (const s of list) totals[s.kind] = (totals[s.kind] ?? 0) + 1;

const report: string[] = [];
report.push(`语料数据审计报告  seed=${SEED}`);
report.push(`命令: bun tools/audit_corpus.ts${ONLY ? ` --only ${ONLY}` : ""}`);
report.push(`证据源: ${CORPUS.replace(REPO + "/", "")}(记自 sts_lightspeed 反编译)`);
report.push("");
report.push(`覆盖: 牌 ${ourCards.length} 张(可审 ${totals.cards} 个场景)/ 药水 ${ourPotions.length} 瓶(${totals.potions})/ 遗物 ${ourRelics.length} 件(${totals.relics})`);
report.push(`结果: 通过 ${passed}/${list.length}, 不一致 ${mismatches.length}, 未覆盖(数值不可当回合直测)牌 ${want("cards") ? unaudited.length : "-"} 张`);
{
  const probeSpecs = list.filter((s) => s.probe);
  const probeIds = new Set(probeSpecs.map((s) => s.id));
  report.push(
    `新增轴(持有/抽到/回合末/多回合/选牌结果集合): ${probeIds.size} 张牌,${probeSpecs.length} 个场景 —— ` +
      `补上此前"数值不可当回合直测"的缺口牌(诅咒/状态、能力牌、随机/选牌类)`,
  );
}
report.push("");
if (selfIssues.length) {
  report.push(`== 语料自洽(文本 vs values)不一致 ${selfIssues.length} ==`);
  for (const s of selfIssues) report.push(`  ${s}`);
  report.push("");
}
report.push(`静态数据(费用/类型/稀有度)对语料: ${dataIssues.length === 0 ? "全一致" : `${dataIssues.length} 处不一致`}`);
for (const s of dataIssues) report.push(`  ${s}`);
report.push("");
report.push(`文案↔实现 双向校验(静态): ${textImplFails.length === 0 ? "全一致" : `${textImplFails.length} 处不一致`}(${textImplChecks} 条规则,数字只从语料与源码各自抽取)`);
if (cardTextSummary !== "") report.push(`  ${cardTextSummary}`);
for (const s of textImplFails) report.push(`  ${s}`);
report.push("");
report.push("行为不一致清单:");
if (mismatches.length === 0) report.push("  (无)");
for (const m of mismatches) {
  report.push(`  ${m.name}  [${m.verdict}]`);
  for (const l of m.lines) report.push(`      ${l}`);
}
report.push("");
if (want("cards")) report.push(`未覆盖的牌(${unaudited.length}): ${unaudited.join(", ")}`);

report.push("");
report.push("口径覆盖(防未来盲区):");
if (want("cards")) {
  report.push(`  卡片侧未登记盲区: ${blind.length === 0 ? "无" : `${blind.length} 张`}`);
  for (const b of blind) report.push(`    ${b}`);
  report.push(`  卡片数值字段级未覆盖: ${fieldBlind.length === 0 ? "无" : `${fieldBlind.length} 处`}`);
  for (const b of fieldBlind) report.push(`    ${b}`);
  report.push(`  卡片不参与自动比对(CARD_NOT_COMPARED): ${Object.keys(CARD_NOT_COMPARED).length} 张`);
  for (const [k, v] of Object.entries(CARD_NOT_COMPARED)) report.push(`    ${k}: ${v}`);
}
if (want("potions")) {
  report.push(`  药水侧未登记盲区: ${potionBlind.length === 0 ? "无" : `${potionBlind.length} 瓶`}`);
  for (const b of potionBlind) report.push(`    ${b}`);
  report.push(`  药水不参与自动比对(POTION_NOT_COMPARED): ${Object.keys(POTION_NOT_COMPARED).length} 瓶`);
  for (const [k, v] of Object.entries(POTION_NOT_COMPARED)) report.push(`    ${k}: ${v}`);
}
if (want("relics")) {
  report.push(
    `  遗物不参与自动比对: ${relicNoMatch.size} 件(Relic 规则/探针覆盖 ${totals.relics} 件;gated 已单独排除),按原因分组:`,
  );
  for (const [reason, ids] of Object.entries(relicReasons)) {
    const suffix = reason.startsWith("语料") ? ` -> ${ids.join(", ")}` : "";
    report.push(`    ${reason}: ${ids.length} 件${suffix}`);
  }
  report.push(`  遗物显式登记(RELIC_NOT_COMPARED): ${Object.keys(RELIC_NOT_COMPARED).length} 件`);
  for (const [k, v] of Object.entries(RELIC_NOT_COMPARED)) report.push(`    ${k}: ${v}`);
}

const guardOk = guardFails.length === 0;
const mismatchUnknown = mismatches.filter((m) => !KNOWN[m.name]);
report.push("");
report.push(`守卫: ${guardOk && mismatchUnknown.length === 0 ? "PASS" : "FAIL"}`);
report.push(`  守卫一 口径覆盖: ${guardOk ? "PASS" : `FAIL(${guardFails.length} 条)`}`);
report.push(`  守卫二 内容登记表: ${ONLY === undefined ? `${registry.size} 条已登记 / 工具当前 ${computedTags.size} 条` : "跳过(--only 模式)"}`);
for (const f of guardFails) report.push(`    - ${f}`);
report.push(`  行为不一致: ${mismatchUnknown.length === 0 ? "无" : `${mismatchUnknown.length} 条(见上)`}`);

if (SELFTEST) {
  const st = selftestReport();
  const text = st.join("\n") + "\n";
  process.stdout.write(text);
  if (OUT) writeFileSync(OUT, text);
  rmSync(DIR, { recursive: true, force: true });
  process.exit(st.some((l) => l.includes("[FAIL]")) ? 1 : 0);
}

const text = report.join("\n") + "\n";
process.stdout.write(text);
if (OUT) writeFileSync(OUT, text);
rmSync(DIR, { recursive: true, force: true });
if (!guardOk || mismatchUnknown.length > 0) process.exit(1);
