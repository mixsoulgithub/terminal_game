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
  | { k: "energy"; v: number };

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

if (want("cards")) cardScenarios();
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
}
interface StateJson {
  energy: number;
  player: { hp: number; block: number; powers: Record<string, number> };
  hand: unknown[];
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
const KNOWN: Record<string, string> = {};
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
