// 语料数据审计:不拿参考实现当基准,直接从原版证据源(语料 corpus JSON,记自
// sts_lightspeed 反编译)抽出每张牌/每瓶药水/每件遗物的数值事实,再用沙盒在
// 受控局面下实测本作的产出,逐条对比。不一致的由人对 wiki 定夺。
//
//   bun tools/audit_corpus.ts                 # 全量审计 + 报告
//   bun tools/audit_corpus.ts --only cards    # 只审某类(cards|potions|relics)
//   bun tools/audit_corpus.ts --seed 12345
//   bun tools/audit_corpus.ts --out tools/golden/audit_report.txt
//
// 语料 text 里的数值与 values/upgrade 字段是同一份来源;本工具从 text 抽事实、
// 用 values 做语料自洽核对,再用 `spire --sandbox-batch` 实测本作行为。

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const REPO = join(ROOT, "..");
const CORPUS = join(REPO, "refs", "slay-the-cli", "data", "corpus");
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
const want = (kind: string) => ONLY === undefined || ONLY === kind;

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
 * 从牌的语料文本抽数值事实。只认直白的 "Deal N damage" / "Gain N Block" /
 * "Apply N X" / "Gain N X" / "Draw N" / "Lose N HP" / "Gain @RE";
 * 带条件(If/Whenever/for each/equal to)或没数值的句子一律不认,
 * 牌会被标成"未覆盖"而不是误判成我们错。
 */
function cardFacts(card: CorpusCard, up: boolean): Fact[] {
  const facts: Fact[] = [];
  for (const line of norm(card.text).split("\n")) {
    const l = line.trim();
    // Deal N damage [to ALL enemies / to a random enemy] [N times | twice]
    let m = l.match(/^Deal\s+(\[[^\]]*\]|\S+)\s+damage(.*)$/);
    if (m) {
      const rest = m[2]!;
      const dmg = pick(m[1]!, up);
      if (dmg !== null && dmg > 0 && !/for each/.test(rest)) {
        let hits = 1;
        const times = rest.match(/(\[[^\]]*\]|\S+)\s+times\b/);
        if (times) hits = pick(times[1]!, up) ?? 1;
        else if (/\btwice\b/.test(rest)) hits = 2;
        facts.push({ k: "damage", v: dmg * hits });
      }
      continue;
    }
    // Gain N Block
    m = l.match(/^Gain\s+(\[[^\]]*\]|\S+)\s+Block\b/);
    if (m) {
      if (!/for each/.test(l)) {
        const b = pick(m[1]!, up);
        if (b !== null) facts.push({ k: "block", v: b });
      }
      continue;
    }
    // Apply N Weak/Vulnerable/Frail(可带 "and ..." 一并处理)
    m = l.match(/^Apply\s+(\[[^\]]*\]|\S+)\s+(Weak|Vulnerable|Frail)\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "enemy_power", name: POWER_KEY[m[2]!]!, v });
      const and = l.match(/and\s+(?:N\s+)?(Weak|Vulnerable|Frail)\b/);
      if (and && m[2] !== and[1] && v !== null) facts.push({ k: "enemy_power", name: POWER_KEY[and[1]!]!, v });
      continue;
    }
    // 句中 apply N Vulnerable(thunderclap)
    m = l.match(/\bapply\s+(\[[^\]]*\]|\S+)\s+(Weak|Vulnerable|Frail)\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "enemy_power", name: POWER_KEY[m[2]!]!, v });
      continue;
    }
    // Enemy loses N Strength(disarm)
    m = l.match(/^Enemy loses\s+(\[[^\]]*\]|\S+)\s+Strength\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "enemy_power", name: "strength", v: -v });
      continue;
    }
    // Gain N Strength/Dexterity/...
    m = l.match(/^Gain\s+(\[[^\]]*\]|\S+)\s+(Strength|Dexterity|Artifact|Thorns|Metallicize|Plated Armor|Regeneration|Intangible|Ritual|Focus)\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "player_power", name: POWER_KEY[m[2]!]!, v });
      continue;
    }
    // Gain @RE(@RE ...) -> 能量
    m = l.match(/^Gain\s+(@RE.*)$/);
    if (m) {
      const base = m[1]!.split("|")[0]!;
      const n = (base.match(/@RE/g) ?? []).length;
      if (n > 0) facts.push({ k: "energy", v: n });
      continue;
    }
    // Draw N cards
    m = l.match(/^Draw\s+(\[[^\]]*\]|\S+)/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "draw", v });
      continue;
    }
    // Lose N HP
    m = l.match(/^Lose\s+(\[[^\]]*\]|\S+)\s+HP\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "self_hp", v: -v });
      continue;
    }
    // Heal N HP
    m = l.match(/^Heal\s+(\[[^\]]*\]|\S+)\s+HP\b/);
    if (m) {
      const v = pick(m[1]!, up);
      if (v !== null) facts.push({ k: "self_hp", v });
    }
  }
  return facts;
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
): void {
  list.push({ name: `cards/${id}/${suffix}`, kind: "cards", id, level, facts, probe, scenario, mode: "delta" });
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
      if (facts.length === 0) continue;
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
  { re: /At the start of each combat, draw (\d+) additional cards?/, fact: (m) => ({ k: "draw", v: Number(m[1]) }), deck: true },
  { re: /Gain (\d+) Energy on the first turn of each combat/, fact: (m) => ({ k: "energy", v: Number(m[1]) }) },
  { re: /At the start of each combat, heal (\d+) HP/, fact: (m) => ({ k: "self_hp", v: Number(m[1]) }) },
];

function relicScenarios(): void {
  for (const id of ourRelics) {
    if (gated.has(`relic/${id}`)) continue;
    const r = relicByGame.get(id);
    if (!r) continue;
    const t = norm(r.text);
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
      break;
    }
  }
}

if (want("cards")) {
  cardScenarios();
  curseStatusScenarios();
  powerScenarios();
  choiceScenarios();
}
if (want("potions")) potionScenarios();
if (want("relics")) relicScenarios();

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

for (const s of list) {
  const rows = got.get(s.name);
  if (!rows) {
    mismatches.push({ name: s.name, verdict: "(a) 我们错", lines: ["沙盒没有输出这一段"] });
    continue;
  }
  // 探针轴:直接读快照算事实(持有/抽到/回合末/多回合/选牌结果集合)
  if (s.probe) {
    const rec = s.probe(rows);
    if (typeof rec === "string") {
      mismatches.push({ name: s.name, verdict: KNOWN[s.name] ?? "(a) 我们错", lines: [rec] });
      continue;
    }
    const lines: string[] = [];
    for (const f of s.facts) {
      if (f.k !== "probe") continue;
      const have = rec[f.key];
      if (have !== f.v) lines.push(`${f.key}: 实测 ${JSON.stringify(have)} vs 期望 ${f.v}`);
    }
    if (lines.length === 0) passed++;
    else mismatches.push({ name: s.name, verdict: KNOWN[s.name] ?? "(a) 我们错", lines });
    continue;
  }
  const o = observed(s, rows);
  if (typeof o === "string") {
    mismatches.push({ name: s.name, verdict: "(a) 我们错", lines: [o] });
    continue;
  }
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
  if (lines.length === 0) passed++;
  else mismatches.push({ name: s.name, verdict: KNOWN[s.name] ?? "(a) 我们错", lines });
}

const coveredCardIds = new Set(list.filter((s) => s.kind === "cards").map((s) => s.id));
const unaudited = ourCards.filter((id) => !coveredCardIds.has(id));

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

// ---- 报告 ----
const totals: Record<string, number> = { cards: 0, potions: 0, relics: 0 };
for (const s of list) totals[s.kind] = (totals[s.kind] ?? 0) + 1;

const report: string[] = [];
report.push(`语料数据审计报告  seed=${SEED}`);
report.push(`命令: bun tools/audit_corpus.ts${ONLY ? ` --only ${ONLY}` : ""}`);
report.push(`证据源: ${CORPUS.replace(REPO + "/", "")}(记自 sts_lightspeed 反编译)`);
report.push("");
report.push(`覆盖: 牌 ${ourCards.length} 张(可审 ${totals.cards} 个场景)/ 药水 ${ourPotions.length} 瓶(${totals.potions})/ 遗物 ${ourRelics.length} 件(${totals.relics})`);
report.push(`结果: 通过 ${passed}/${list.length}, 不一致 ${mismatches.length}, 未覆盖(数值不可当回合直测)牌 ${unaudited.length} 张`);
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
report.push("行为不一致清单:");
if (mismatches.length === 0) report.push("  (无)");
for (const m of mismatches) {
  report.push(`  ${m.name}  [${m.verdict}]`);
  for (const l of m.lines) report.push(`      ${l}`);
}
report.push("");
report.push(`未覆盖的牌(${unaudited.length}): ${unaudited.join(", ")}`);

const text = report.join("\n") + "\n";
process.stdout.write(text);
if (OUT) writeFileSync(OUT, text);
rmSync(DIR, { recursive: true, force: true });
