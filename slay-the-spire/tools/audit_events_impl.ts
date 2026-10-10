// 事件面双向对账:把语料(refs/slay-the-cli/data/corpus/events.json,记自
// sts_lightspeed 反编译)里 51 个事件的每个选项/可用性条件/效果/进入条件,与本作
// 实现(src/core/events.rs 的 EventDef/EventChoice + src/core/run.rs 的事件分支)
// 逐条双向核对。
//
//   bun tools/audit_events_impl.ts                 # 全量审计(守卫失败或不一致即 exit != 0)
//   bun tools/audit_events_impl.ts --only big_fish # 只审一个事件
//   bun tools/audit_events_impl.ts --selftest      # 改坏/恢复回归:逐条确认工具还抓得到
//   bun tools/audit_events_impl.ts --impl <文件>   # 用保存下来的 --dump events-json 输出(默认现跑)
//   bun tools/audit_events_impl.ts --corpus <目录> # 换语料目录(自检用)
//   bun tools/audit_events_impl.ts --src <目录>    # 换实现源码目录(默认 src/core,自检用)
//
// 实现侧走 `spire --dump events-json`(src/core/events.rs::dump_json),导出每个
// EventDef 的每个 EventChoice:label + 全部 req_* 条件字段 + outcome 的每个非默认
// 字段 + outcome_a15 + cost_gold_a15,并把多屏后半段(next/fight_next 可达)一并带上。
// 进入条件与事件池直接读 src/core/run.rs 文本对账。
//
// 对账口径:
//   1 选项文本:语料的每个选项都要在实现里找到对应选项(多屏事件按 stage 映射),
//     且选项名(冒号前的短名)与数值锚点要能对上;
//   2 可用性条件:语料 requires(minGold/hasRelic/deckHasType/minPotions/maxFloor…)
//     ↔ 实现 EventChoice 的 cost_gold/req_*/only_screen/req_floor_*,并与反编译
//     GameAction.cpp 的可用性位掩码逐事件核对(把"阈值方向 / >= vs > / 是否计入上限"
//     这类最易错的点钉死);
//   3 效果:语料 outcomes(healHp/loseHp/gainMaxHp/loseMaxHp/loseGold/gainGold/
//     obtainRelic/obtainCurse/obtainCard/obtainPotion/removeCard/upgradeCard/
//     transformCard/startCombat/other)↔ 实现 outcome 字段,取整方式(floor/round/ceil)
//     也逐条对(用"对任意生命上限结果等价"的判据,避免把 0.5 的 ceil/round 误报);
//     A15 变体按"基础效果 + 覆盖"合并后再与 outcome_a15 对;
//   4 进入条件:语料 canSpawn(pool/acts/minGold/minFloor/minCurHp/minRelics/
//     maxAscension/…)↔ run.rs::event_can_spawn + 各事件池,含 A15 起 note_for_yourself 出池;
//   5 覆盖守卫:语料里的选项/效果/字段没进口径又没登记 -> FAIL;反过来实现里多出来的
//     选项/条件/字段没登记也 -> FAIL;登记表里用不上的条目同样 -> FAIL(防登记表烂掉)。
//
// 已接进 tools/check_all.ts 的两步:events(全量审计)/ evself(改坏-恢复回归)。

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const ROOT = join(HERE, "..");
const REPO = join(ROOT, "..");
const DEFAULT_CORPUS = join(REPO, "refs", "slay-the-cli", "data", "corpus");
const DEFAULT_SRC = join(ROOT, "src", "core");
const SPIRE = process.env.SPIRE_BIN ?? join(ROOT, "target", "debug", "spire");

const argv = process.argv.slice(2);
const flag = (name: string): string | undefined => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const SELFTEST = argv.includes("--selftest");
/** 只列"事件 × 选项"覆盖清单(维护登记表/人看用) */
const LIST = argv.includes("--list");
const ONLY = flag("--only");
const CORPUS_DIR = flag("--corpus") ?? DEFAULT_CORPUS;
const SRC_DIR = flag("--src") ?? DEFAULT_SRC;
const IMPL_FILE = flag("--impl");

// ---- 语料类型 ----

interface CorpusOutcome {
  effect: string;
  amount?: number | string | number[] | null;
  id?: string | null;
  rounding?: string | null;
  kind?: string | null;
  detail?: string | null;
  combatRewards?: unknown;
}
interface CorpusOption {
  label: string;
  requires?: Record<string, unknown> | null;
  outcomes?: CorpusOutcome[];
  random?: { rng?: string; roll?: string } | null;
  a15?: Record<string, unknown> | null;
}
interface CorpusEvent {
  id: string;
  pool: string;
  acts: number[];
  canSpawn?: Record<string, unknown> | null;
  options: CorpusOption[];
}

// ---- 实现类型(--dump events-json) ----

type ImplOutcome = Record<string, unknown>;
interface ImplChoice {
  label: string;
  [k: string]: unknown;
}
interface ImplDef {
  i: number;
  id: string;
  root: boolean;
  choices: ImplChoice[];
}

function loadCorpus(): CorpusEvent[] {
  const raw = JSON.parse(readFileSync(join(CORPUS_DIR, "events.json"), "utf8")) as {
    events: CorpusEvent[];
  };
  return raw.events;
}

function loadImpl(): ImplDef[] {
  let text: string;
  if (IMPL_FILE) text = readFileSync(IMPL_FILE, "utf8");
  else {
    const r = spawnSync(SPIRE, ["--dump", "events-json"], { encoding: "utf8", maxBuffer: 1 << 26 });
    if (r.status !== 0) throw new Error(`spire --dump events-json 失败: ${r.stderr}`);
    text = r.stdout;
  }
  return (JSON.parse(text) as { events: ImplDef[] }).events;
}

/** 读实现源码(run.rs 事件分支 + events.rs);--selftest 会传入改坏的副本 */
function loadSources(dir = SRC_DIR): { run: string; events: string } {
  return {
    run: readFileSync(join(dir, "run.rs"), "utf8"),
    events: readFileSync(join(dir, "events.rs"), "utf8"),
  };
}

// ---- 多屏事件的选项映射(语料 option 下标 -> 实现 [def 第 n 个, choice 下标]) ----
//
// 其余事件都是根 EventDef 的选项与语料选项同序;只有这两个多屏事件语料把后半段
// 折成了几个选项,必须显式点名。cursed_tome 的 root[1] "Leave" 被语料并进了
// option[0] 的文本里(反编译 GameAction.cpp:724-739 CURSED_TOME phase0 返回 0x3,确实是两项)。

interface Loc {
  id: string;
  n: number;
  choice: number;
}
const MULTI_STAGE: Record<string, Loc[][]> = {
  cursed_tome: [
    [{ id: "cursed_tome", n: 0, choice: 0 }], // Read
    [
      { id: "cursed_tome", n: 1, choice: 0 },
      { id: "cursed_tome", n: 2, choice: 0 },
      { id: "cursed_tome", n: 3, choice: 0 },
    ], // Continue 三页
    [{ id: "cursed_tome", n: 4, choice: 0 }], // Take
    [{ id: "cursed_tome", n: 4, choice: 1 }], // Stop
  ],
  colosseum: [
    [{ id: "colosseum", n: 0, choice: 0 }],
    [{ id: "colosseum", n: 1, choice: 0 }],
    [{ id: "colosseum", n: 1, choice: 1 }],
  ],
};

/** 每个语料 option 对应几个实现位置(默认 1 个) */
function locGroups(ev: CorpusEvent): Loc[][] {
  const multi = MULTI_STAGE[ev.id.toLowerCase()];
  if (multi) return multi;
  return ev.options.map((_, i) => [{ id: ev.id.toLowerCase(), n: 0, choice: i }]);
}

function choicesOf(defs: ImplDef[], id: string): ImplChoice[] {
  const def = defs.filter((d) => d.id === id).sort((a, b) => a.i - b.i)[0];
  return def ? def.choices : [];
}

function nthChoices(defs: ImplDef[], id: string, n: number): ImplChoice[] {
  const def = defs.filter((d) => d.id === id).sort((a, b) => a.i - b.i)[n];
  return def ? def.choices : [];
}

// ============================================================================
// 规范化:两侧都产出同一套 token,逐项对账
// ============================================================================

/** 小数规范化:0.10 -> "0.1";0.3333 -> "0.3333" */
function fr(x: number): string {
  return String(Number(x.toFixed(6)));
}
/** 语料 pct:0.25 -> 0.25 */
function pct(s: string): number {
  return Number(s.slice(4));
}
function cap(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1).toLowerCase();
}

/**
 * 生命上限 x 在两种取整下的结果是否始终一致(一致就不区分取整方式,避免把
 * ghosts 的 ceil(50%) 与实现的 round(50%) 误报)。取 x 到 400 足够覆盖真实上限。
 */
function roundingEq(f: number, a: string, b: string): boolean {
  if (a === b) return true;
  const perMille = Math.round(f * 1000);
  const method = (x: number, m: string): number => {
    if (m === "round") return ((x * perMille + 500) / 1000) | 0;
    const prod = Math.fround(Math.fround(x) * Math.fround(f));
    return m === "floor" ? Math.floor(prod) : Math.ceil(prod);
  };
  for (let x = 1; x <= 400; x++) if (method(x, a) !== method(x, b)) return false;
  return true;
}

interface Sig {
  facts: string[];
  unresolved: string[];
  unused: string[];
}
function mk(): Sig {
  return { facts: [], unresolved: [], unused: [] };
}

// ---- 实现 outcome -> 效果 facts ----

function implOutcomeFacts(o: ImplOutcome, s: Sig): void {
  const f = (t: string) => s.facts.push(t);
  const num = (k: string): number | undefined => (typeof o[k] === "number" ? (o[k] as number) : undefined);
  const str = (k: string): string | undefined => (typeof o[k] === "string" ? (o[k] as string) : undefined);
  const used = new Set<string>();
  const known = Object.keys(o).filter((k) => k !== "text");

  const gold = num("gold");
  if (gold) {
    used.add("gold");
    f(gold > 0 ? `gold:+${gold}` : `gold:-${-gold}`);
  }
  if (Array.isArray(o.gold_range)) {
    used.add("gold_range");
    f(`gold:+${(o.gold_range as number[])[0]}..${(o.gold_range as number[])[1]}`);
  }
  if (Array.isArray(o.gold_lose_range)) {
    used.add("gold_lose_range");
    f(`gold:-${(o.gold_lose_range as number[])[0]}..${(o.gold_lose_range as number[])[1]}`);
  }
  if (o.gold_lose_all) {
    used.add("gold_lose_all");
    f(`gold:-all`);
  }
  if (num("gold_per_act")) {
    used.add("gold_per_act");
    f(`gold_act:+${num("gold_per_act")}`);
  }
  if (o.gold_first) used.add("gold_first");

  const hp = num("hp");
  if (hp) {
    used.add("hp");
    f(hp < 0 ? `hp:-${-hp}` : `hp:+${hp}`);
  }
  if (num("cost_hp")) {
    used.add("cost_hp");
    f(`hp:-${num("cost_hp")}`);
  }
  if (num("hp_frac")) {
    used.add("hp_frac");
    f(`hp_frac:-${fr(num("hp_frac")!)}:floor`);
  }
  if (num("hp_frac_ceil")) {
    used.add("hp_frac_ceil");
    f(`hp_frac:-${fr(num("hp_frac_ceil")!)}:ceil`);
  }
  if (num("hp_pct")) {
    used.add("hp_pct");
    f(`hp_frac:-${fr(num("hp_pct")! / 1000)}:round`);
  }
  if (num("hp_pct_min")) used.add("hp_pct_min");

  if (num("heal_frac")) {
    used.add("heal_frac");
    f(`heal_frac:+${fr(num("heal_frac")!)}:floor`);
  }
  if (num("heal_pct")) {
    used.add("heal_pct");
    f(`heal_frac:+${fr(num("heal_pct")! / 1000)}:round`);
  }
  if (o.full_heal) {
    used.add("full_heal");
    f(`heal_full`);
  }
  const mh = num("max_hp");
  if (mh) {
    used.add("max_hp");
    f(mh < 0 ? `max_hp:-${-mh}` : `max_hp:+${mh}`);
  }
  if (num("max_hp_frac")) {
    used.add("max_hp_frac");
    f(`max_hp_frac:-${fr(num("max_hp_frac")!)}:floor`);
  }
  if (num("max_hp_frac_ceil")) {
    used.add("max_hp_frac_ceil");
    f(`max_hp_frac:-${fr(num("max_hp_frac_ceil")!)}:ceil`);
  }
  if (num("max_hp_pct")) {
    used.add("max_hp_pct");
    f(`max_hp_frac:-${fr(num("max_hp_pct")! / 1000)}:round`);
  }

  if (str("relic_id")) {
    used.add("relic_id");
    f(`relic:${str("relic_id")}`);
  }
  if (o.random_relic_any) {
    used.add("random_relic_any");
    f(`relic:random_tier`);
  }
  if (str("random_relic_rarity")) {
    used.add("random_relic_rarity");
    f(`relic:random_${str("random_relic_rarity")}`);
  }
  if (o.pick_relic_from) {
    used.add("pick_relic_from");
    f(`relic_pick_from`);
  }
  if (o.relic_reward) {
    used.add("relic_reward");
    f(`relic_reward_random`);
  }
  if (str("relic_reward_id")) {
    used.add("relic_reward_id");
    f(`relic_reward:${str("relic_reward_id")}`);
  }
  if (str("remove_relic")) {
    used.add("remove_relic");
    f(`lose_relic:${str("remove_relic")}`);
  }

  if (str("add_card")) {
    used.add("add_card");
    f(`card:${str("add_card")}x1`);
  }
  if (Array.isArray(o.add_cards)) {
    used.add("add_cards");
    const [id, n] = o.add_cards as [string, number];
    f(`card:${id}x${n}`);
  }
  if (str("add_curse")) {
    used.add("add_curse");
    f(`card:${str("add_curse")}x1`);
  }
  if (o.add_random_curse) {
    used.add("add_random_curse");
    f(`card_random_curse`);
  }
  if (Array.isArray(o.add_random_class)) {
    used.add("add_random_class");
    const [r, n] = o.add_random_class as [string, number];
    f(`card_random_class:${r}x${n}`);
  }
  if (Array.isArray(o.add_random_colorless)) {
    used.add("add_random_colorless");
    const [r, n] = o.add_random_colorless as [string | null, number];
    f(`card_random_colorless:${r ?? "any"}x${n}`);
  }
  if (num("colorless_card_rewards")) {
    used.add("colorless_card_rewards");
    f(`colorless_reward_x${num("colorless_card_rewards")}`);
  }
  if (o.library_read) {
    used.add("library_read");
    f(`library_20`);
  }

  if (o.remove_card) {
    used.add("remove_card");
    f(`remove_card`);
  }
  if (o.remove_curses) {
    used.add("remove_curses");
    f(`remove_curses`);
  }
  if (o.remove_base_strikes) {
    used.add("remove_base_strikes");
    f(`remove_strikes`);
  }
  if (o.remove_random) {
    used.add("remove_random");
    f(`remove_random:${(o.remove_random as { of_type: string }).of_type}`);
  }
  if (o.offer_card) {
    used.add("offer_card");
    f(`offer_card`);
  }
  if (o.note_swap) {
    used.add("note_swap");
    f(`note_swap`);
  }
  if (o.duplicate_card) {
    used.add("duplicate_card");
    f(`duplicate_card`);
  }
  if (o.transform_card) {
    used.add("transform_card");
    f(`transform_one`);
  }
  if (num("transform_random_n")) {
    used.add("transform_random_n");
    f(`transform_random_x${num("transform_random_n")}`);
  }
  if (num("transform_choose_n")) {
    used.add("transform_choose_n");
    f(`transform_choose_x${num("transform_choose_n")}`);
  }

  if (o.upgrade_card) {
    used.add("upgrade_card");
    f(`upgrade_one`);
  }
  if (num("upgrade_random_n")) {
    used.add("upgrade_random_n");
    f(`upgrade_random_x${num("upgrade_random_n")}`);
  }
  if (num("upgrade_random_shuffle")) {
    used.add("upgrade_random_shuffle");
    f(`upgrade_random_shuffle_x${num("upgrade_random_shuffle")}`);
  }
  if (o.upgrade_all) {
    used.add("upgrade_all");
    f(`upgrade_all`);
  }
  if (o.upgrade_starters) {
    used.add("upgrade_starters");
    f(`upgrade_starters`);
  }

  if (o.lose_random_potion) {
    used.add("lose_random_potion");
    f(`lose_potion_random`);
  }
  if (num("random_potion_n")) {
    used.add("random_potion_n");
    f(`potion_random_x${num("random_potion_n")}`);
  }
  if (num("potion_reward_n")) {
    used.add("potion_reward_n");
    f(`potion_reward_x${num("potion_reward_n")}`);
  }

  if (str("fight")) {
    used.add("fight");
    f(`fight:${str("fight")}`);
  }
  if (o.fight_pool) {
    used.add("fight_pool");
    f(`fight_pool`);
  }
  if (o.fight_reward) {
    used.add("fight_reward");
    f(`fight_reward:${fightRewardToken(o.fight_reward as Record<string, unknown>)}`);
  }
  if (o.fight_next !== undefined) used.add("fight_next");

  if (o.roll) {
    used.add("roll");
    const roll = o.roll as { kind: Record<string, unknown>; outcomes: ImplOutcome[] };
    f(`roll:${rollKind(roll.kind)}`);
    roll.outcomes.forEach((ro, i) => {
      const sub = mk();
      implOutcomeFacts(ro, sub);
      if (sub.facts.length === 0) f(`rollout:${i}:none`);
      for (const t of sub.facts) f(`rollout:${i}:${t}`);
    });
  }
  if (o.ooze) {
    used.add("ooze");
    f(`ooze`);
  }
  if (o.adv_search) {
    used.add("adv_search");
    f(`adv_search`);
  }
  if (num("wma")) {
    used.add("wma");
    f(`wma:${num("wma")}`);
  }
  if (num("nloth_offer")) {
    used.add("nloth_offer");
    f(`nloth:${num("nloth_offer")}`);
  }
  if (num("designer_service")) {
    used.add("designer_service");
    f(`designer:${num("designer_service")}`);
  }
  if (num("skull_buy")) {
    used.add("skull_buy");
    f(`skull:${num("skull_buy")}`);
  }
  if (o.jump_to_boss) {
    used.add("jump_to_boss");
    f(`jump_to_boss`);
  }
  if (o.set_screen) {
    used.add("set_screen");
    f(`screen:${o.set_screen}`);
  }
  if (o.next !== undefined) used.add("next");
  if (o.dead) {
    used.add("dead");
    f(`dead`);
  }

  for (const k of known) if (!used.has(k)) s.unused.push(`outcome.${k}`);
}

function rollKind(k: Record<string, unknown>): string {
  if (k.kind === "Uniform") return "Uniform";
  if (k.kind === "HalfBit") return "HalfBit";
  if (k.kind === "Always") return `Always(${k.idx})`;
  return `Coin(${k.num}/${k.den},true=${k.true_idx})`;
}

function fightRewardToken(r: Record<string, unknown>): string {
  const parts: string[] = [];
  if (r.nothing) parts.push("nothing");
  if (r.gold) parts.push(`gold:${(r.gold as number[]).join("-")}`);
  if (r.relic_id) parts.push(`relic:${r.relic_id}`);
  if (r.relic_rarity) parts.push(`relic:${r.relic_rarity}`);
  if (r.relic_rarity2) parts.push(`relic2:${r.relic_rarity2}`);
  if (r.no_relic) parts.push("no_relic");
  if (r.no_cards) parts.push("no_cards");
  if (typeof r.potion_pct === "number" && r.potion_pct > 0) parts.push("potions");
  return parts.join(",");
}

// ---- 实现 choice -> 条件 facts ----

function implCondFacts(c: ImplChoice, s: Sig): void {
  const num = (k: string) => (typeof c[k] === "number" ? (c[k] as number) : 0);
  const str = (k: string) => (typeof c[k] === "string" ? (c[k] as string) : undefined);
  const gold = Math.max(num("cost_gold"), num("req_gold"));
  if (gold > 0) s.facts.push(`req_gold:>=${gold}`);
  if (str("req_relic")) s.facts.push(`req_relic:${str("req_relic")}`);
  if (str("req_no_relic")) s.facts.push(`req_no_relic:${str("req_no_relic")}`);
  if (c.req_potion) s.facts.push(`req_potion`);
  if (c.req_big_attack) s.facts.push(`req_big_attack`);
  if (c.req_non_basic) s.facts.push(`req_non_basic`);
  if (str("only_screen")) s.facts.push(`screen:${str("only_screen")}`);
  if (num("max_uses") > 0) s.facts.push(`max_uses:${num("max_uses")}`);
  if (c.req_removable) s.facts.push(`req_removable`);
  if (num("req_removable_min") > 0) s.facts.push(`req_removable_min:${num("req_removable_min")}`);
  if (c.req_upgradeable) s.facts.push(`req_upgradeable`);
  if (str("req_card_type")) s.facts.push(`req_card_type:${str("req_card_type")}`);
  if (c.req_no_card_type) s.facts.push(`req_no_card_type`);
  if (num("req_floor_max") > 0) s.facts.push(`req_floor:<=${num("req_floor_max")}`);
  if (num("req_floor_min") > 0) s.facts.push(`req_floor:>=${num("req_floor_min")}`);
}

// ============================================================================
// 语料特例:自由文本 other / 复合效果按反编译逐条登记(带依据)
// ============================================================================

interface Special {
  facts: string[];
  ref: string;
  /** A15 变体的显式 facts;给了就用它(与 outcome_a15 对) */
  a15?: string[];
  /** A15 差异实现在代码里(run.rs 常量/分支),不做数据对账,改用源码断言 */
  a15src?: { pat: RegExp; ref: string };
}

const SPECIAL: Record<string, Special> = {
  "DEAD_ADVENTURER[0]": {
    facts: ["adv_search"],
    ref: "GameContext.cpp:867-878 onEnter + GameAction.cpp:677;伏击 25/A15 35 在 run.rs:3743",
    a15src: { pat: /if self\.ascension >= 15 \{ 35 \} else \{ crate::core::events::DEAD_ADVENTURER_AMBUSH_BASE \}/, ref: "run.rs:3743" },
  },
  "SCRAP_OOZE[0]": {
    facts: ["ooze"],
    ref: "GameContext.cpp:3179-3199 SCRAP_OOZE",
    a15src: { pat: /self\.damage\(if self\.ascension >= 15 \{ 5 \} else \{ 3 \}\)/, ref: "run.rs:3797" },
  },
  "MATCH_AND_KEEP[0]": {
    facts: [],
    ref: "GameContext.cpp:963-997;棋盘与牌堆在 run.rs MatchKeep::new,A15 换第二张诅咒",
    a15src: { pat: /MatchKeep::new/, ref: "run.rs" },
  },
  "DEAD_ADVENTURER_FIGHT": { facts: [], ref: "" },
  "FORGOTTEN_ALTAR[0]": {
    facts: ["relic:bloody_idol", "lose_relic:golden_idol"],
    ref: "GameContext.cpp:2785-2810 FORGOTTEN_ALTAR",
  },
  "VAMPIRES[0]": {
    facts: ["lose_relic:blood_vial", "remove_strikes", "card:bitex5"],
    ref: "GameContext.cpp:1029-1032 + VAMPIRES",
  },
  "VAMPIRES[1]": {
    facts: ["max_hp_frac:-0.3:ceil", "remove_strikes", "card:bitex5"],
    ref: "GameContext.cpp VAMPIRES 接受:ceil(30%) 上限、删全部起始打击",
  },
  "THE_MOAI_HEAD[1]": {
    facts: ["lose_relic:golden_idol", "gold:+333"],
    ref: "GameContext.cpp:3343+ THE_MOAI_HEAD",
  },
  "WHEEL_OF_CHANGE[0]": {
    facts: [
      "roll:Uniform",
      "rollout:0:gold_act:+100",
      "rollout:1:relic_reward_random",
      "rollout:2:heal_full",
      "rollout:3:card:decayx1",
      "rollout:4:remove_card",
      "rollout:5:hp_frac:-0.1:floor",
    ],
    ref: "GameContext.cpp WHEEL_OF_CHANGE;WHEEL 数组 events.rs",
    a15: [
      "roll:Uniform",
      "rollout:0:gold_act:+100",
      "rollout:1:relic_reward_random",
      "rollout:2:heal_full",
      "rollout:3:card:decayx1",
      "rollout:4:remove_card",
      "rollout:5:hp_frac:-0.15:floor",
    ],
  },
  "BONFIRE_SPIRITS[0]": { facts: ["offer_card"], ref: "GameContext.cpp:859-862;按稀有度结算在 run.rs" },
  "DESIGNER_IN_SPIRE[1]": {
    facts: ["gold:-60", "designer:2"],
    ref: "GameContext.cpp:2657-2697 + GameAction.cpp:740-773",
  },
  "DESIGNER_IN_SPIRE[0]": {
    facts: ["gold:-40", "designer:1"],
    ref: "GameContext.cpp:2657-2697 + GameAction.cpp:740-773",
  },
  "SECRET_PORTAL[0]": { facts: ["jump_to_boss"], ref: "GameContext.cpp:3200-3213 SECRET_PORTAL" },
  "WE_MEET_AGAIN[0]": {
    facts: ["relic:random_tier", "wma:1"],
    ref: "GameContext.cpp:1033-1043 WE_MEET_AGAIN;丢哪瓶药在代码里掷",
  },
  "WE_MEET_AGAIN[1]": {
    facts: ["relic:random_tier", "wma:2"],
    ref: "GameContext.cpp:1033-1043;gold = miscRng.random(50, min(150, gold))",
  },
  "WE_MEET_AGAIN[2]": {
    facts: ["relic:random_tier", "wma:3"],
    ref: "GameContext.cpp:1033-1043;随机丢一张非基础非诅咒牌",
  },
  "NLOTH[0]": { facts: ["relic:nloths_gift", "nloth:1"], ref: "GameContext.cpp:998-1008 NLOTH onEnter" },
  "NLOTH[1]": { facts: ["relic:nloths_gift", "nloth:2"], ref: "GameContext.cpp:998-1008" },
  "KNOWING_SKULL[0]": { facts: ["skull:1", "gold:+90"], ref: "GameContext.cpp:2920-2947 KNOWING_SKULL" },
  "KNOWING_SKULL[1]": {
    facts: ["skull:2", "card_random_colorless:Uncommonx1"],
    ref: "GameContext.cpp:2920-2947;shuffleRng 洗无色池取第一张罕见",
  },
  "KNOWING_SKULL[2]": { facts: ["skull:3", "potion_random_x1"], ref: "GameContext.cpp:2920-2947" },
  "KNOWING_SKULL[3]": {
    facts: ["hp_frac:-0.1:floor"],
    ref: "GameContext.cpp:2920-2947;离开只付基础价 max(6, floor(10% 上限))",
  },
  "CURSED_TOME[1]": { facts: ["hp:-1", "hp:-2", "hp:-3"], ref: "GameContext.cpp:2558-2598 三页" },
  "CURSED_TOME[2]": {
    facts: ["hp:-10", "roll:Uniform", "rollout:0:relic_reward:necronomicon", "rollout:1:relic_reward:enchiridion", "rollout:2:relic_reward:nilrys_codex"],
    ref: "GameContext.cpp:2558-2598;miscRng.random(2) 三选一,摆进奖励屏",
    a15: ["hp:-15", "roll:Uniform", "rollout:0:relic_reward:necronomicon", "rollout:1:relic_reward:enchiridion", "rollout:2:relic_reward:nilrys_codex"],
  },
  "NOTE_FOR_YOURSELF[0]": { facts: ["note_swap"], ref: "GameContext.cpp:3155-3165 NOTE_FOR_YOURSELF" },
  "THE_LIBRARY[0]": { facts: ["library_20"], ref: "GameContext.cpp:3300-3325 THE_LIBRARY" },
  "SENSORY_STONE[0]": { facts: ["colorless_reward_x1"], ref: "GameContext.cpp:3214-3232 SENSORY_STONE" },
  "SENSORY_STONE[1]": { facts: ["hp:-5", "colorless_reward_x2"], ref: "GameContext.cpp:3214-3232" },
  "SENSORY_STONE[2]": { facts: ["hp:-10", "colorless_reward_x3"], ref: "GameContext.cpp:3214-3232" },
  "DUPLICATOR[0]": { facts: ["duplicate_card"], ref: "GameContext.cpp:2721-2733 DUPLICATOR" },
  "FALLING[0]": { facts: ["remove_random:Skill"], ref: "GameContext.cpp:2757-2784 FALLING" },
  "FALLING[1]": { facts: ["remove_random:Power"], ref: "GameContext.cpp:2757-2784" },
  "FALLING[2]": { facts: ["remove_random:Attack"], ref: "GameContext.cpp:2757-2784" },
  "THE_DIVINE_FOUNTAIN[0]": { facts: ["remove_curses"], ref: "GameContext.cpp:2811-2828" },
  "THE_JOUST[0]": {
    facts: ["gold:-50", "roll:Coin(3/10,true=1)", "rollout:0:gold:+100", "rollout:1:none"],
    ref: "GameContext.cpp:3280-3297 THE_JOUST",
  },
  "THE_JOUST[1]": {
    facts: ["gold:-50", "roll:Coin(3/10,true=0)", "rollout:0:gold:+250", "rollout:1:none"],
    ref: "GameContext.cpp:3280-3297",
  },
  "THE_MAUSOLEUM[0]": {
    facts: ["relic:random_tier", "roll:HalfBit", "rollout:0:card:writhex1", "rollout:1:none"],
    ref: "GameContext.cpp:3326-3342 THE_MAUSOLEUM",
    a15: ["relic:random_tier", "roll:Always(0)", "rollout:0:card:writhex1", "rollout:1:none"],
  },
  "HYPNOTIZING_COLORED_MUSHROOMS[0]": {
    facts: ["fight:event_three_fungi", "fight_reward:gold:20-30,relic:odd_mushroom,potions"],
    ref: "GameContext.cpp:3069-3092",
  },
  "MASKED_BANDITS[1]": {
    facts: ["fight:event_bandits", "fight_reward:gold:25-35,relic:red_mask"],
    ref: "GameContext.cpp:2987-3009;原版不掉药水",
  },
  "MYSTERIOUS_SPHERE[0]": {
    facts: ["fight:event_two_orbs", "fight_reward:gold:45-55,relic:Rare,potions"],
    ref: "GameContext.cpp:3093-3114",
  },
  "COLOSSEUM[0]": {
    facts: ["fight:event_colosseum_slavers", "fight_reward:nothing,no_relic,no_cards"],
    ref: "GameContext.cpp:2554-2557(stub)/MonsterGroup.cpp:208-211",
  },
  "COLOSSEUM[2]": {
    facts: ["fight:event_colosseum_nobs", "fight_reward:gold:100-100,relic:Rare,relic2:Uncommon,potions"],
    ref: "GameContext.cpp:2554-2557;MonsterGroup COLOSSEUM_EVENT_NOBS",
  },
  "MINDBLOOM[0]": {
    facts: ["fight_pool", "fight_reward:gold:50-50,relic:Rare,potions"],
    ref: "GameContext.cpp:3015-3068 MINDBLOOM",
    a15: ["fight_pool", "fight_reward:gold:25-25,relic:Rare,potions"],
  },
  "LAB[0]": {
    facts: ["potion_reward_x3"],
    ref: "GameContext.cpp:2948-2952 LAB(走奖励屏)",
    a15: ["potion_reward_x2"],
  },
  "BIG_FISH[2]": { facts: ["relic:random_tier", "card:regretx1"], ref: "GameContext.cpp:2526-2552 BIG_FISH" },
  "GOLDEN_IDOL[2]": { facts: ["card:injuryx1"], ref: "GameContext.cpp:2844-2874 GOLDEN_IDOL" },
};
delete SPECIAL["DEAD_ADVENTURER_FIGHT"];

// 语料 detail 里点名的固定扣血下限(实现的 hp_pct_min);这些数很小,单独点名核对
const HP_MIN_CHECKS: Record<string, { min: number; ref: string }> = {
  "FACE_TRADER[0]": { min: 1, ref: "GameContext.cpp:2734-2756 FACE_TRADER max(1, floor(10%))" },
  "KNOWING_SKULL[3]": { min: 6, ref: "GameContext.cpp:2920-2947 基础价 max(6, floor(10%))" },
};

// 语料没有、实现多出来的条件:全部登记(牌组完整度/限次数/位掩码互斥),新种类必须登记
const EXTRA_COND_TOKENS: Record<string, string[]> = {
  "THE_JOUST[0]": ["req_gold:>=50"],
  "THE_JOUST[1]": ["req_gold:>=50"],
};

const EXTRA_COND: Record<string, string> = {
  req_removable: "牌组完整度:没有可移除牌就锁选项(反编译只给 CardSelectScreen,未门控)",
  req_upgradeable: "牌组完整度:没有可升级牌就锁选项",
  req_removable_min: "增强器/设计师变形张数门槛(反编译 GameAction getTransformableCount(2)>=2)",
  req_no_card_type: "坠落的保底项只在三类型都抽不出时可点(反编译 FALLING bits==0 -> 8)",
  req_no_relic: "两张位掩码互斥(反编译 TOMB_OF_LORD_RED_MASK)",
  max_uses: "限次数(反编译 dead_adventurer phase<3)",
  screen: "多屏事件的分屏门控(反编译 golden_idol 位掩码)",
};

// 语料没建模、实现多出来的效果 token:分屏/顺序/独立计数等,新种类必须登记
const EXTRA_FX: Record<string, string> = {
  screen: "set_screen 分屏(界面流程)",
  gold_first: "金先于血的结算顺序(脸商人)",
  hp_min: "扣血下限(单独在 HP_MIN_CHECKS 核对)",
  wma: "再会交易类型标记(数值在代码里掷)",
  nloth: "两件供奉遗物标记",
  designer: "设计师服务编号",
  skull: "会说话的骷髅选项计数",
};

/** 条件 token 的种类(冒号前) */
function condKind(t: string): string {
  return t.split(":")[0]!;
}

// 多屏事件的"进入这一屏"条件:由结构保证(见 AVAIL),不作为数据条件对账
const STRUCTURE_CONDS = new Set(["stage_after_fight", "stage_reading", "stage_read_all"]);

// ============================================================================
// 进入条件(语料 canSpawn / pool ↔ run.rs event_can_spawn + 事件池)
// ============================================================================

interface EntryRule {
  /** run.rs 里必须出现的源码片段(每条一个) */
  needles: string[];
  ref: string;
}
const ENTRY: Record<string, EntryRule> = {
  THE_CLERIC: { needles: [`"the_cleric" => self.player.gold >= 35`], ref: "run.rs the_cleric" },
  DEAD_ADVENTURER: { needles: [`"dead_adventurer" | "hypnotizing_colored_mushrooms" => floor >= 7`], ref: "run.rs dead_adventurer" },
  HYPNOTIZING_COLORED_MUSHROOMS: { needles: [`"dead_adventurer" | "hypnotizing_colored_mushrooms" => floor >= 7`], ref: "run.rs hypnotizing_colored_mushrooms" },
  OLD_BEGGAR: { needles: [`"old_beggar" => self.player.gold >= 75`], ref: "run.rs old_beggar" },
  COLOSSEUM: { needles: [`"colosseum" => self.pos.is_some() && self.floor_reached > 7`], ref: "run.rs colosseum" },
  THE_MOAI_HEAD: {
    needles: [`"the_moai_head" => {`, `self.player.hp * 2 <= self.player.max_hp`, `golden_idol`],
    ref: "run.rs the_moai_head",
  },
  DESIGNER_IN_SPIRE: { needles: [`"designer_in_spire" => (act == 2 || act == 3) && self.player.gold >= 75`], ref: "run.rs designer_in_spire" },
  DUPLICATOR: { needles: [`"duplicator" => act == 2 || act == 3`], ref: "run.rs duplicator" },
  FACE_TRADER: { needles: [`"face_trader" => act == 1 || act == 2`], ref: "run.rs face_trader" },
  THE_DIVINE_FOUNTAIN: { needles: [`"the_divine_fountain" => self`, `CardType::Curse`], ref: "run.rs the_divine_fountain" },
  KNOWING_SKULL: { needles: [`"knowing_skull" => act == 2 && self.player.hp >= 13`], ref: "run.rs knowing_skull" },
  NLOTH: { needles: [`"nloth" => act == 2 && self.player.relics.len() >= 2`], ref: "run.rs nloth" },
  NOTE_FOR_YOURSELF: { needles: [`"note_for_yourself" => self.ascension <= 14`, `*id != "note_for_yourself"`], ref: "run.rs note_for_yourself + one_time_event_pool" },
  SECRET_PORTAL: { needles: [`"secret_portal" => act == 3`], ref: "run.rs secret_portal;反编译 speedrunPace 是搜索开关不建模" },
  THE_JOUST: { needles: [`"the_joust" => act == 2 && self.player.gold >= 50`], ref: "run.rs the_joust" },
  THE_WOMAN_IN_BLUE: { needles: [`"the_woman_in_blue" => self.player.gold >= 50`], ref: "run.rs the_woman_in_blue" },
};

// ============================================================================
// 反编译可用性位掩码表(GameAction.cpp):把阈值方向/>= vs > 钉死
// ============================================================================

interface AvailRule {
  ref: string;
  check: (defs: ImplDef[]) => string | null;
}
const AVAIL: Record<string, AvailRule> = {
  THE_CLERIC: {
    ref: "GameAction.cpp:842-847 gold>=75/50 -> 0x7 else 0b101",
    check: (d) => {
      const c = choicesOf(d, "the_cleric");
      return c[1]?.cost_gold === 50 && c[1]?.cost_gold_a15 === 75
        ? null
        : `THE_CLERIC Purify 金币门槛应 50/A15 75,实测 ${c[1]?.cost_gold}/${c[1]?.cost_gold_a15}`;
    },
  },
  GOLDEN_IDOL: {
    ref: "GameAction.cpp:806-811 有金像 -> 0b11100,没有 -> 0b11",
    check: (d) => {
      const c = choicesOf(d, "golden_idol");
      const trap = [2, 3, 4].every((i) => c[i]?.only_screen === "trap");
      const take = c[0]?.only_screen === undefined;
      return trap && take ? null : `GOLDEN_IDOL 陷阱屏门控应 only_screen=trap`;
    },
  },
  MINDBLOOM: {
    ref: "GameAction.cpp:827-832 floorNum<=40 -> 0x7 else 0b1011",
    check: (d) => {
      const c = choicesOf(d, "mindbloom");
      return c[2]?.req_floor_max === 40 && c[3]?.req_floor_min === 41
        ? null
        : `MINDBLOOM 层号门控应 <=40 / >=41`;
    },
  },
  THE_MOAI_HEAD: {
    ref: "GameAction.cpp:849-854 hasRelic GOLDEN_IDOL -> 0x7",
    check: (d) => (choicesOf(d, "the_moai_head")[1]?.req_relic === "golden_idol" ? null : `THE_MOAI_HEAD 献金像应 req_relic=golden_idol`),
  },
  TOMB_OF_LORD_RED_MASK: {
    ref: "GameAction.cpp:856-861 有红面具 -> 0b101 else 0b110",
    check: (d) => {
      const c = choicesOf(d, "tomb_of_lord_red_mask");
      return c[0]?.req_relic === "red_mask" && c[1]?.req_no_relic === "red_mask"
        ? null
        : `TOMB 红面具两选项应 req_relic/req_no_relic 互斥`;
    },
  },
  VAMPIRES: {
    ref: "GameAction.cpp:867-871 hasRelic BLOOD_VIAL -> 0x7",
    check: (d) => (choicesOf(d, "vampires")[0]?.req_relic === "blood_vial" ? null : `VAMPIRES 献血瓶应 req_relic=blood_vial`),
  },
  FALLING: {
    ref: "GameAction.cpp:781-798 每类型有可移除牌亮对应位;都没有 -> 8",
    check: (d) => {
      const c = choicesOf(d, "falling");
      return c[0]?.req_card_type === "Skill" && c[1]?.req_card_type === "Power" && c[2]?.req_card_type === "Attack" && c[3]?.req_no_card_type === true
        ? null
        : `FALLING 三类型 + 保底 req_no_card_type 配置不符`;
    },
  },
  LIVING_WALL: {
    ref: "GameAction.cpp:820-825 有可升级牌 -> 0x7 else 0x3",
    check: (d) => (choicesOf(d, "living_wall")[2]?.req_upgradeable === true ? null : `LIVING_WALL Grow 应 req_upgradeable`),
  },
  PURIFIER: {
    ref: "GameAction.cpp:834-840 有可移除牌 -> 0x3 else 0x2",
    check: (d) => (choicesOf(d, "purifier")[0]?.req_removable === true ? null : `PURIFIER Pray 应 req_removable`),
  },
  UPGRADE_SHRINE: {
    ref: "GameAction.cpp:863-868 有可升级牌 -> 0x3 else 0x2",
    check: (d) => (choicesOf(d, "upgrade_shrine")[0]?.req_upgradeable === true ? null : `UPGRADE_SHRINE Pray 应 req_upgradeable`),
  },
  AUGMENTER: {
    ref: "GameAction.cpp:774-779 transformableCount(2)>=2 -> 0x7 else 0b101",
    check: (d) => (choicesOf(d, "augmenter")[1]?.req_removable_min === 2 ? null : `AUGMENTER 变形两张应 req_removable_min=2`),
  },
  DESIGNER_IN_SPIRE: {
    ref: "GameAction.cpp:740-773 三档金币 + 变形张数要求",
    check: (d) => {
      const c = choicesOf(d, "designer_in_spire");
      const g = [40, 60, 90].every((x, i) => c[i]?.cost_gold === x);
      const a15 = [50, 75, 110].every((x, i) => c[i]?.cost_gold_a15 === x);
      return g && a15 ? null : `DESIGNER 三档金币门槛应 40/60/90(50/75/110)`;
    },
  },
  WE_MEET_AGAIN: {
    ref: "GameAction.cpp:873-882 potionIdx/gold/cardIdx 各自非 -1 才亮位",
    check: (d) => {
      const c = choicesOf(d, "we_meet_again");
      return c[0]?.req_potion === true && c[1]?.req_gold === 50 && c[2]?.req_non_basic === true
        ? null
        : `WE_MEET_AGAIN 三项应 req_potion/req_gold:50/req_non_basic`;
    },
  },
  THE_JOUST: {
    ref: "反编译归常亮 0x3(GameAction.cpp:691);真机要 50 金(语料 minGold 50)",
    check: (d) => {
      const c = choicesOf(d, "the_joust");
      return c[0]?.req_gold === 50 && c[1]?.req_gold === 50 ? null : `THE_JOUST 押注应 req_gold=50`;
    },
  },
  SCRAP_OOZE: {
    ref: "GameAction.cpp:688 常亮 0x3",
    check: (d) => (choicesOf(d, "scrap_ooze")[0]?.max_uses === undefined ? null : `SCRAP_OOZE 不应有 max_uses`),
  },
  DEAD_ADVENTURER: {
    ref: "GameAction.cpp:677 常亮,靠 phase<3 自灭",
    check: (d) => (choicesOf(d, "dead_adventurer")[0]?.max_uses === 3 ? null : `DEAD_ADVENTURER Search 应 max_uses=3`),
  },
  CURSED_TOME: {
    ref: "GameAction.cpp:724-739 phase 位掩码:0->0x3, 1..3->单 Continue, 4->Take/Stop",
    check: (d) => {
      const n = d.filter((x) => x.id === "cursed_tome").length;
      const total = d.filter((x) => x.id === "cursed_tome").reduce((a, x) => a + x.choices.length, 0);
      return n === 5 && total === 7 ? null : `CURSED_TOME 应有 5 个 def / 7 项,实测 ${n}/${total}`;
    },
  },
};

// ============================================================================
// 主对账
// ============================================================================

// 登记:反编译/语料与实现之间已知的、本工具不改的差异(附依据),打印在报告末尾
const NOTED: { item: string; why: string }[] = [
  {
    item: "SECRET_PORTAL 的 800 秒 playtime 门槛不建模",
    why: "反编译只是一个 speedrunPace 搜索开关(canAddOneTimeEvent: act==3 && !speedrunPace),没有现实时间模型;实现按 act==3 放行(run.rs the_joust 附近)",
  },
  {
    item: "事件遗物档次不随章变(act4 的 0/100/0)",
    why: "反编译 returnRandomRelicTier(relicRng, act) 只对 act4 特殊;事件只出现在 1~3 章,档位都是 50/33/17",
  },
  {
    item: "COLOSSEUM 流程按 wiki/corpus",
    why: "反编译 GameContext.cpp:2554-2556 是 stub(disableColosseum),只有第一场阵容(MonsterGroup.cpp:208-211)与第二场奖励有依据",
  },
  {
    item: "语料在若干处按 wiki 纠正了反编译的疑似 copy-paste bug",
    why: "蘑菇 Eat(反编译给 99 金)、神秘球体 Leave(99 金)、会说话的骷髅基础价(反编译恒 6)、蓝衣女子/设计师的金币花费(反编译不扣)。实现一律跟语料:heal+Parasite / Leave 无效果 / max(6,floor(10%)) / cost_gold 20/30/40 与 40/60/90",
  },
  {
    item: "MASKED_BANDITS 战斗不进药水掉落",
    why: "语料取反编译(addPotionRewards 在该场省略);实现 REWARD_BANDITS potion_pct = 0",
  },
  {
    item: "FALLING 排除瓶装牌",
    why: "反编译自标 todo(GameContext.cpp:886);实现按真机(deck_has_removable_of_type 排 bottled)",
  },
];

export interface Report {
  diffs: string[];
  guards: string[];
  stats: {
    events: number;
    options: number;
    a15: number;
    conds: number;
    entry: number;
    avail: number;
    covered: number;
    structure: number;
  };
}

/** 取整方式等价的 frac 事实是否匹配 */
function fracMatch(x: string, y: string): boolean {
  const mx = /^(hp_frac|max_hp_frac|heal_frac):(-?[\d.]+):(\w+)$/.exec(x);
  const my = /^(hp_frac|max_hp_frac|heal_frac):(-?[\d.]+):(\w+)$/.exec(y);
  if (!mx || !my) return false;
  if (mx[1] !== my[1] || Math.abs(Number(mx[2]) - Number(my[2])) > 1e-6) return false;
  return roundingEq(Math.abs(Number(mx[2])), mx[3]!, my[3]!);
}

function sigDiff(a: string[], b: string[]): { missing: string[]; extra: string[] } {
  const bb = [...b];
  const missing: string[] = [];
  for (const x of a) {
    let i = bb.indexOf(x);
    if (i < 0) i = bb.findIndex((y) => fracMatch(x, y));
    if (i >= 0) bb.splice(i, 1);
    else missing.push(x);
  }
  return { missing, extra: bb };
}

export function audit(
  corpus: CorpusEvent[],
  defs: ImplDef[],
  src: { run: string; events: string } = loadSources(),
  full = true,
): Report {
  const diffs: string[] = [];
  const guards: string[] = [];
  const stats = { events: 0, options: 0, a15: 0, conds: 0, entry: 0, avail: 0, covered: 0, structure: 0 };
  const usedSpecial = new Set<string>();
  const usedEntry = new Set<string>();

  const corpusIds = new Set(corpus.map((e) => e.id.toLowerCase()));
  const implRoots = new Set(defs.filter((d) => d.root).map((d) => d.id));
  if (full) {
    for (const id of implRoots) if (!corpusIds.has(id)) guards.push(`实现里的事件 ${id} 不在语料`);
    for (const id of corpusIds) if (!implRoots.has(id)) guards.push(`语料里的事件 ${id} 没实现`);
  }

  for (const ev of corpus) {
    stats.events++;
    const groups = locGroups(ev);
    if (groups.length !== ev.options.length) guards.push(`${ev.id}: 语料 ${ev.options.length} 选项,映射 ${groups.length}`);
    const defsFor = (loc: Loc) => nthChoices(defs, loc.id, loc.n)[loc.choice];

    for (let i = 0; i < ev.options.length; i++) {
      const opt = ev.options[i]!;
      const group = groups[i]!;
      const tag = `${ev.id}[${i}]`;
      const primary = defsFor(group[0]!);
      if (!primary) {
        guards.push(`${tag}: 实现里找不到对应选项`);
        continue;
      }
      stats.options++;

      // 1) 效果(多屏 option 汇总所有 loc 的 outcome)
      const spec = SPECIAL[tag];
      const cs = mk();
      if (spec) {
        usedSpecial.add(tag);
        cs.facts.push(...spec.facts);
      } else {
        corpusEffects(ev, i, opt, cs);
      }
      const is = mk();
      for (const loc of group) {
        const c = defsFor(loc);
        if (!c) continue;
        implOutcomeFacts(c.outcome as ImplOutcome, is);
        const cost = typeof c.cost_gold === "number" ? (c.cost_gold as number) : 0;
        if (cost > 0) is.facts.push(`gold:-${cost}`);
      }
      for (const u of is.unused) guards.push(`${tag}: 实现 outcome 字段 ${u} 没进口径`);
      for (const u of cs.unresolved) guards.push(`${tag}: 语料片段 ${u} 没进口径(未登记)`);
      const ef = sigDiff(cs.facts, is.facts);
      for (const m of ef.missing) diffs.push(`${tag} 效果: 语料有而实现没有/不符 -> ${m}`);
      for (const m of ef.extra) {
        const kind = m.split(":")[0]!;
        // screen:/wma: 等实现多出的标记类 token 需登记
        const kindKey = EXTRA_FX[`${tag}#${kind}`] ?? EXTRA_FX[kind];
        if (kindKey) continue;
        diffs.push(`${tag} 效果: 实现多出/不符 -> ${m}`);
      }
      if (cs.facts.length && ef.missing.length === 0) stats.covered++;

      // 2) A15 变体
      if (opt.a15) {
        stats.a15++;
        if (spec?.a15src) {
          if (!spec.a15src.pat.test(src.run) && !spec.a15src.pat.test(src.events))
            diffs.push(`${tag}@a15: 代码里的 A15 分支没找到(${spec.a15src.ref})`);
        } else {
          const as = spec?.a15 ? spec.a15.slice() : corpusA15Facts(ev, opt, cs.facts);
          const isA: string[] = [];
          const c0 = primary;
          const a15Cost = typeof c0.cost_gold_a15 === "number" ? (c0.cost_gold_a15 as number) : 0;
          if (c0.outcome_a15) {
            // outcome_a15 是整份替换
            if (a15Cost > 0) isA.push(`gold:-${a15Cost}`);
            const s2 = mk();
            implOutcomeFacts(c0.outcome_a15 as ImplOutcome, s2);
            isA.push(...s2.facts);
          } else {
            // 只有花费变:基础效果 + 覆盖后的花费
            const s2 = mk();
            implOutcomeFacts(c0.outcome as ImplOutcome, s2);
            isA.push(...s2.facts);
            const baseCost = typeof c0.cost_gold === "number" ? (c0.cost_gold as number) : 0;
            if (baseCost > 0) isA.push(`gold:-${baseCost}`);
            if (a15Cost > 0) {
              const i = isA.indexOf(`gold:-${baseCost}`);
              if (i >= 0) isA[i] = `gold:-${a15Cost}`;
              else isA.push(`gold:-${a15Cost}`);
            }
          }
          if (!c0.outcome_a15 && a15Cost === 0)
            guards.push(`${tag}@a15: 语料有 A15 变体,实现没有 outcome_a15/cost_gold_a15`);
          const af = sigDiff(as, isA);
          for (const m of af.missing) diffs.push(`${tag}@a15 效果: 语料有而实现没有/不符 -> ${m}`);
          for (const m of af.extra) {
            const kind = m.split(":")[0]!;
            if (EXTRA_FX[`${tag}#${kind}`] ?? EXTRA_FX[kind]) continue;
            diffs.push(`${tag}@a15 效果: 实现多出/不符 -> ${m}`);
          }
        }
      }

      // 3) 可用性条件
      const cc = corpusCond(ev, opt);
      stats.structure += cc.facts.filter((x) => STRUCTURE_CONDS.has(x)).length;
      const ic = mk();
      for (const loc of group) {
        const c = defsFor(loc);
        if (c) implCondFacts(c, ic);
      }
      for (const u of cc.unresolved) guards.push(`${tag}: ${u} 没进口径`);
      const structural = cc.facts.filter((x) => STRUCTURE_CONDS.has(x));
      const condFacts = cc.facts.filter((x) => !STRUCTURE_CONDS.has(x));
      stats.conds += condFacts.length;
      const cf = sigDiff(condFacts, ic.facts);
      for (const m of cf.missing) diffs.push(`${tag} 条件: 语料有而实现没有/不符 -> ${m}`);
      for (const m of cf.extra) {
        if (EXTRA_COND[condKind(m)] || EXTRA_COND_TOKENS[tag]?.includes(m)) continue;
        diffs.push(`${tag} 条件: 实现多出/不符 -> ${m}`);
      }
      void structural;

      // 4) 标签锚点
      const lab = labelCheck(ev, i, opt, primary);
      if (lab) diffs.push(`${tag} 选项文本: ${lab}`);
    }

    // 5) 进入条件
    const rule = ENTRY[ev.id];
    if (!rule) {
      if (ev.canSpawn) guards.push(`${ev.id}: 有 canSpawn 却没登记 ENTRY`);
    } else {
      usedEntry.add(ev.id);
      stats.entry += rule.needles.length;
      for (const n of rule.needles) if (!src.run.includes(n)) diffs.push(`${ev.id} 进入条件: run.rs 缺少判据 "${n}"(${rule.ref})`);
    }
    // 池一致性
    const poolGuard = poolCheck(ev, src.run);
    if (poolGuard) diffs.push(poolGuard);
  }

  // 6) 反编译可用性位掩码
  for (const [id, rule] of Object.entries(AVAIL)) {
    if (!corpusIds.has(id.toLowerCase())) {
      if (full) guards.push(`AVAIL 登记了不存在的事件 ${id}`);
      continue;
    }
    stats.avail++;
    const msg = rule.check(defs);
    if (msg) diffs.push(`反编译可用性 ${id}: ${msg}(${rule.ref})`);
  }

  // 7) 固定扣血下限
  for (const [tag, chk] of Object.entries(HP_MIN_CHECKS)) {
    const [eid, idx] = tag.split("[") as [string, string];
    const i = Number(idx.replace("]", ""));
    const ev = corpus.find((e) => e.id === eid);
    const group = ev ? locGroups(ev)[i] : undefined;
    const c = group ? nthChoices(defs, group[0]!.id, group[0]!.n)[group[0]!.choice] : undefined;
    const oc = (c?.outcome ?? {}) as Record<string, unknown>;
    const got = typeof oc.hp_pct_min === "number" ? (oc.hp_pct_min as number) : 0;
    if (got < chk.min) diffs.push(`${tag} 扣血下限: 应 >= ${chk.min},实测 ${got}(${chk.ref})`);
  }

  // 8) 登记表用得上(全量审计时才查,避免 --only 误报)
  if (full) {
    for (const k of Object.keys(SPECIAL)) {
      if (!usedSpecial.has(k)) guards.push(`SPECIAL 登记了用不上的条目 ${k}`);
    }
    for (const id of Object.keys(ENTRY)) if (!usedEntry.has(id)) guards.push(`ENTRY 登记了语料里没有的事件 ${id}`);
  }

  return { diffs, guards, stats };
}

// ---- 语料 -> facts ----

function corpusCond(ev: CorpusEvent, opt: CorpusOption): Sig {
  const s = mk();
  for (const [k, v] of Object.entries(opt.requires ?? {})) {
    if (k === "minGold") s.facts.push(`req_gold:>=${v}`);
    else if (k === "hasRelic") s.facts.push(`req_relic:${String(v).toLowerCase()}`);
    else if (k === "tookIdol") s.facts.push(`screen:trap`);
    else if (k === "deckHasAttackWithSingleHitDamageAtLeast") s.facts.push(`req_big_attack`);
    else if (k === "wonFirstFight") s.facts.push(`stage_after_fight`);
    else if (k === "reading") s.facts.push(`stage_reading`);
    else if (k === "readAllPages") s.facts.push(`stage_read_all`);
    else if (k === "deckHasType") s.facts.push(`req_card_type:${cap(String(v))}`);
    else if (k === "noEligibleCards") s.facts.push(`req_no_card_type`);
    else if (k === "maxFloor") s.facts.push(`req_floor:<=${v}`);
    else if (k === "minFloor") s.facts.push(`req_floor:>=${v}`);
    else if (k === "minPotions") s.facts.push(`req_potion`);
    else if (k === "deckHasNonBasicNonCurseCard") s.facts.push(`req_non_basic`);
    else s.unresolved.push(`${ev.id} requires.${k}`);
  }
  return s;
}

function effectFacts(list: CorpusOutcome[], tag: string, random: { roll?: string } | null | undefined, s: Sig): void {
  for (const o of list) {
    switch (o.effect) {
      case "healHp":
        if (typeof o.amount === "string" && o.amount.startsWith("pct:")) s.facts.push(`heal_frac:+${fr(pct(o.amount))}:${o.rounding ?? "floor"}`);
        else if (o.amount === null || o.amount === undefined) {
          if ((o.detail ?? "").includes("full")) s.facts.push("heal_full");
          else s.unresolved.push(`${tag} healHp(${o.detail})`);
        } else s.facts.push(`hp:+${o.amount}`);
        break;
      case "gainMaxHp":
        s.facts.push(`max_hp:+${o.amount}`);
        break;
      case "loseHp":
        if (typeof o.amount === "string" && o.amount.startsWith("pct:")) s.facts.push(`hp_frac:-${fr(pct(o.amount))}:${o.rounding ?? "floor"}`);
        else if (typeof o.amount === "number") s.facts.push(`hp:-${o.amount}`);
        else s.unresolved.push(`${tag} loseHp(${o.detail})`);
        break;
      case "loseMaxHp":
        if (typeof o.amount === "string" && o.amount.startsWith("pct:")) s.facts.push(`max_hp_frac:-${fr(pct(o.amount))}:${o.rounding ?? "floor"}`);
        else s.unresolved.push(`${tag} loseMaxHp(${o.detail})`);
        break;
      case "loseGold":
        if (typeof o.amount === "number") s.facts.push(`gold:-${o.amount}`);
        else if ((o.detail ?? "").includes("all current gold")) s.facts.push("gold:-all");
        else {
          const m = /(\d+)-(\d+)/.exec(`${random?.roll ?? ""} ${o.detail ?? ""}`);
          if (m) s.facts.push(`gold:-${m[1]}..${m[2]}`);
          else s.unresolved.push(`${tag} loseGold(${o.detail})`);
        }
        break;
      case "gainGold":
        if (typeof o.amount === "number") s.facts.push(`gold:+${o.amount}`);
        else if (Array.isArray(o.amount)) s.facts.push(`gold:+${o.amount[0]}..${o.amount[1]}`);
        else {
          const m = /(\d+)-(\d+)/.exec(random?.roll ?? "");
          if (m) s.facts.push(`gold:+${m[1]}..${m[2]}`);
          else s.unresolved.push(`${tag} gainGold(${random?.roll})`);
        }
        break;
      case "obtainRelic": {
        const idc = String(o.id);
        if (idc === "RANDOM") s.facts.push((o.detail ?? "").includes("reward screen") ? "relic_reward_random" : "relic:random_tier");
        else if (idc === "RANDOM_FACE") s.facts.push("relic_pick_from");
        else if (idc === "RANDOM_RARE" || (o.detail ?? "").includes("RARE")) s.facts.push("relic:random_Rare");
        else s.facts.push(`relic:${idc.toLowerCase()}`);
        break;
      }
      case "obtainCurse":
      case "obtainCard": {
        const idc = o.id == null ? null : String(o.id);
        const n = typeof o.amount === "number" ? o.amount : 1;
        if (idc) s.facts.push(`card:${idc.toLowerCase()}x${n}`);
        else s.unresolved.push(`${tag} ${o.effect}(${o.detail})`);
        break;
      }
      case "obtainPotion":
        s.facts.push((o.detail ?? "").includes("reward") ? `potion_reward_x${o.amount}` : `potion_random_x${o.amount}`);
        break;
      case "removeCard":
        if (typeof o.amount === "number") s.facts.push("remove_card");
        else s.unresolved.push(`${tag} removeCard(${o.detail})`);
        break;
      case "transformCard":
        s.facts.push(o.amount === 2 ? "transform_choose_x2" : "transform_one");
        break;
      case "upgradeCard": {
        const d = o.detail ?? "";
        if (o.amount === 2) s.facts.push("upgrade_random_shuffle_x2");
        else if (d.includes("all upgradeable")) s.facts.push("upgrade_all");
        else if (d.includes("Strike/Defend")) s.facts.push("upgrade_starters");
        else if (d.includes("variant")) s.unresolved.push(`${tag} upgradeCard(variant)`);
        else if (d.includes("random")) s.facts.push("upgrade_random_x1");
        else if (o.amount === 1 || o.amount === null || o.amount === undefined) s.facts.push("upgrade_one");
        else s.unresolved.push(`${tag} upgradeCard(${d})`);
        break;
      }
      case "startCombat":
      case "other":
        s.unresolved.push(`${tag} ${o.effect}`);
        break;
      default:
        s.unresolved.push(`${tag} 未知效果 ${o.effect}`);
    }
  }
}

function corpusEffects(ev: CorpusEvent, i: number, opt: CorpusOption, s: Sig): void {
  effectFacts(opt.outcomes ?? [], `${ev.id}[${i}]`, opt.random, s);
}

/** A15 = 基础效果上覆盖若干数值(与 outcome_a15 的"整份替换"对账) */
function corpusA15Facts(ev: CorpusEvent, opt: CorpusOption, baseFacts: string[]): string[] {
  const facts = [...baseFacts];
  const a = opt.a15 ?? {};
  const replace = (re: RegExp, rep: string) => {
    const i = facts.findIndex((x) => re.test(x));
    if (i >= 0) facts[i] = rep;
    else facts.push(rep);
  };
  if (typeof a.loseGold === "number") replace(/^gold:-/, `gold:-${a.loseGold}`);
  if (typeof a.gainGold === "number") replace(/^gold:\+/, `gold:+${a.gainGold}`);
  if (typeof a.loseHp === "number") replace(/^hp:-\d+$/, `hp:-${a.loseHp}`);
  if (typeof a.obtainPotion === "number") replace(/^potion_reward_x\d+/, `potion_reward_x${a.obtainPotion}`);
  if (typeof a.obtainCard === "number") {
    const i = facts.findIndex((x) => /^card:.+x\d+$/.test(x));
    if (i >= 0) {
      const id = facts[i]!.slice(5).split("x")[0]!;
      facts[i] = `card:${id}x${a.obtainCard}`;
    } else facts.push(`card_n:${a.obtainCard}`);
  }
  if (typeof a.amount === "string" && a.amount.startsWith("pct:")) {
    const base = (opt.outcomes ?? []).find((o) => typeof o.amount === "string" && o.amount.startsWith("pct:"));
    if (base) {
      const kind = base.effect === "loseHp" ? "hp_frac" : base.effect === "loseMaxHp" ? "max_hp_frac" : "heal_frac";
      const r = base.rounding ?? "floor";
      const sign = base.effect === "healHp" ? "+" : "-";
      replace(new RegExp(`^${kind}:`), `${kind}:${sign}${fr(pct(a.amount))}:${r}`);
    }
  }
  if (typeof a.amount === "number") replace(/^hp:-\d+$/, `hp:-${a.amount}`);
  if (typeof a.roll === "string") {
    const m = /(\d+)-(\d+)/.exec(a.roll);
    if (m) replace(/^gold:-/, `gold:-${m[1]}..${m[2]}`);
  }
  if (Array.isArray(a.outcomes)) {
    // 整组替换的 A15(如蓝衣女子"离开"新增扣血):先清掉基础里同类事实,再加入
    const as = mk();
    effectFacts(a.outcomes as CorpusOutcome[], `${ev.id}[a15]`, opt.random, as);
    for (const f of as.facts) {
      const kind = f.split(":")[0];
      for (let i = facts.length - 1; i >= 0; i--) if (facts[i]!.startsWith(`${kind}:`)) facts.splice(i, 1);
      facts.push(f);
    }
    for (const u of as.unresolved) facts.push(`__a15_unresolved__:${u}`);
  }
  const known = new Set(["loseGold", "gainGold", "loseHp", "obtainCard", "obtainPotion", "amount", "roll", "requires", "detail", "outcomes"]);
  for (const k of Object.keys(a)) if (!known.has(k)) facts.push(`__a15_unresolved__:${k}`);
  return facts;
}

// ---- 标签锚点 ----

const LABEL_ALIAS: Record<string, string[]> = {
  "THE_CLERIC[2]": ["leave"],
  "CURSED_TOME[0]": ["read"],
  "CURSED_TOME[1]": ["continue"],
  "CURSED_TOME[2]": ["take"],
  "CURSED_TOME[3]": ["stop"],
  "COLOSSEUM[0]": ["fight"],
  "COLOSSEUM[1]": ["cowardice"],
  "COLOSSEUM[2]": ["victory"],
  "KNOWING_SKULL[3]": ["how do i leave"],
  "FALLING[3]": ["land on your head"],
  "MATCH_AND_KEEP[0]": ["leave", "play"],
  "THE_JOUST[0]": ["murderer"],
  "THE_JOUST[1]": ["owner"],
};

function labelCheck(ev: CorpusEvent, i: number, opt: CorpusOption, choice: ImplChoice): string | null {
  const cl = opt.label;
  const il = choice.label;
  const alias = LABEL_ALIAS[`${ev.id}[${i}]`];
  const anchor = cl.split(":")[0]!.trim().toLowerCase();
  const ilLower = il.toLowerCase();
  const anchorOk = ilLower.includes(anchor) || anchor.includes(ilLower.split(":")[0]!.trim()) || alias?.some((x) => ilLower.includes(x));
  if (anchor.length >= 3 && !anchorOk) return `选项名锚点 "${anchor}" 对不上实现文本 "${il}"`;
  for (const m of cl.matchAll(/\b(\d{2,4})\b/g)) {
    const n = m[1]!;
    const inOutcome = (opt.outcomes ?? []).some((o) => String(o.amount ?? "").includes(n) || String(o.id ?? "").includes(n));
    if (!ilLower.includes(n) && !inOutcome && !alias?.some((x) => ilLower.includes(x))) return `语料数值 ${n} 没在实现文本里`;
  }
  return null;
}

// ---- 池一致性 ----

function poolCheck(ev: CorpusEvent, runSrc: string): string | null {
  const id = ev.id.toLowerCase();
  const arr = (name: string): string[] => {
    const m = new RegExp(`const ${name}(?:: \\[&(?:'static )?str; \\d+\\])? = \\[([\\s\\S]*?)\\];`).exec(runSrc);
    return m ? [...m[1]!.matchAll(/"([^"]+)"/g)].map((x) => x[1]!) : [];
  };
  if (ev.pool === "act1" || ev.pool === "act2" || ev.pool === "act3") {
    const name = `ACT${Number(ev.pool.slice(3))}_EVENTS`;
    return arr(name).includes(id) ? null : `${ev.id} 应在 ${name} 里`;
  }
  if (ev.pool === "shrine") {
    const ok = arr("ACT1_SHRINES").includes(id) && arr("ACT23_SHRINES").includes(id);
    return ok ? null : `${ev.id} 应在两个神龛池里`;
  }
  if (ev.pool === "oneTime") {
    return arr("ONE_TIME_EVENTS").includes(id) ? null : `${ev.id} 应在 ONE_TIME_EVENTS 里`;
  }
  return `${ev.id}: 未知池 ${ev.pool}`;
}

// ============================================================================
// 报告
// ============================================================================

function printReport(r: Report, title: string): void {
  const { stats } = r;
  console.log(`事件面双向对账 (${title})`);
  console.log(
    `事件 ${stats.events}  选项 ${stats.options}  A15 变体 ${stats.a15}  条件 ${stats.conds}  进入条件 ${stats.entry}  反编译可用性 ${stats.avail}  结构条件 ${stats.structure}`,
  );
  console.log(`效果对上的选项: ${stats.covered}/${stats.options}`);
  console.log(
    `登记规则: 特例 ${Object.keys(SPECIAL).length}  多出条件种类 ${Object.keys(EXTRA_COND).length}  进入条件表 ${Object.keys(ENTRY).length}  反编译可用性表 ${Object.keys(AVAIL).length}  扣血下限 ${Object.keys(HP_MIN_CHECKS).length}`,
  );
  console.log("");
  if (r.guards.length) {
    console.log(`覆盖守卫 FAIL(${r.guards.length}):`);
    for (const g of r.guards) console.log(`  [GUARD] ${g}`);
  } else console.log("覆盖守卫 PASS");
  if (r.diffs.length) {
    console.log(`不一致 ${r.diffs.length}:`);
    for (const d of r.diffs) console.log(`  [DIFF] ${d}`);
  } else console.log("不一致: 0");
  console.log("");
  console.log("登记(不改;附依据):");
  for (const n of NOTED) console.log(`  [登记] ${n.item}: ${n.why}`);
}

// ============================================================================
// 自检:改坏/恢复
// ============================================================================

interface Mutation {
  name: string;
  want: string;
  corpus?: (c: CorpusEvent[]) => void;
  impl?: (d: ImplDef[]) => void;
  src?: (s: { run: string; events: string }) => void;
}

const MUTATIONS: Mutation[] = [
  {
    name: "改条件阈值(语料):THE_CLERIC minGold 35->30",
    want: "THE_CLERIC[0] 条件",
    corpus: (c) => {
      c.find((e) => e.id === "THE_CLERIC")!.options[0]!.requires = { minGold: 30 };
    },
  },
  {
    name: "改效果数值(语料):BIG_FISH Donut gainMaxHp 5->7",
    want: "BIG_FISH[1] 效果",
    corpus: (c) => {
      c.find((e) => e.id === "BIG_FISH")!.options[1]!.outcomes = [{ effect: "gainMaxHp", amount: 7, id: null }];
    },
  },
  {
    name: "改飞升变体(语料):THE_CLERIC A15 loseGold 75->80",
    want: "@a15",
    corpus: (c) => {
      c.find((e) => e.id === "THE_CLERIC")!.options[1]!.a15 = { loseGold: 80, requires: { minGold: 80 } };
    },
  },
  {
    name: "改取整方式(语料):SHINING_LIGHT round->floor",
    want: "SHINING_LIGHT[0] 效果",
    corpus: (c) => {
      c.find((e) => e.id === "SHINING_LIGHT")!.options[0]!.outcomes = [{ effect: "loseHp", kind: "damage", amount: "pct:0.20", rounding: "floor", id: null }];
    },
  },
  {
    name: "改掷点流(实现):THE_JOUST 押凶手的赔金 100->999",
    want: "THE_JOUST[0] 效果",
    impl: (d) => {
      const oc = d.filter((x) => x.id === "the_joust")[0]!.choices[0]!.outcome as Record<string, unknown>;
      (oc.roll as { outcomes: Record<string, unknown>[] }).outcomes[0]!.gold = 999;
    },
  },
  {
    name: "改掷点流(实现):MAUSOLEUM A15 必中改成 HalfBit",
    want: "@a15",
    impl: (d) => {
      const oc = d.filter((x) => x.id === "the_mausoleum")[0]!.choices[0]!.outcome_a15 as Record<string, unknown>;
      (oc.roll as Record<string, unknown>).kind = { kind: "HalfBit" };
    },
  },
  {
    name: "改取整方式(实现):WINDING_HALLS hp_pct 125->130",
    want: "WINDING_HALLS[0] 效果",
    impl: (d) => {
      (d.filter((x) => x.id === "winding_halls")[0]!.choices[0]!.outcome as Record<string, unknown>).hp_pct = 130;
    },
  },
  {
    name: "改进入门槛(实现源码):dead_adventurer floor>=7 -> >=8",
    want: "进入条件",
    src: (s) => {
      s.run = s.run.replace('"dead_adventurer" | "hypnotizing_colored_mushrooms" => floor >= 7', '"dead_adventurer" | "hypnotizing_colored_mushrooms" => floor >= 8');
    },
  },
  {
    name: "改进入门槛(实现):MINDBLOOM req_floor_max 40->39",
    want: "MINDBLOOM",
    impl: (d) => {
      d.filter((x) => x.id === "mindbloom").sort((a, b) => a.i - b.i)[0]!.choices[2]!.req_floor_max = 39;
    },
  },
  {
    name: "改可用性(实现):GOLDEN_IDOL 陷阱去掉 only_screen",
    want: "GOLDEN_IDOL",
    impl: (d) => {
      delete d.filter((x) => x.id === "golden_idol").sort((a, b) => a.i - b.i)[0]!.choices[2]!.only_screen;
    },
  },
  {
    name: "改效果(实现):BIG_FISH Donut max_hp 5->6",
    want: "BIG_FISH[1] 效果",
    impl: (d) => {
      (d.filter((x) => x.id === "big_fish")[0]!.choices[1]!.outcome as Record<string, unknown>).max_hp = 6;
    },
  },
  {
    name: "改效果(实现):THE_SSSSSERPENT gold 175->170",
    want: "THE_SSSSSERPENT[0] 效果",
    impl: (d) => {
      (d.filter((x) => x.id === "the_ssssserpent")[0]!.choices[0]!.outcome as Record<string, unknown>).gold = 170;
    },
  },
  {
    name: "改反编译可用性(实现):VAMPIRES 去掉 req_relic",
    want: "VAMPIRES",
    impl: (d) => {
      delete d.filter((x) => x.id === "vampires")[0]!.choices[0]!.req_relic;
    },
  },
  {
    name: "改金币标度(实现):WHEEL 金币格 gold_per_act 100->50",
    want: "WHEEL_OF_CHANGE[0] 效果",
    impl: (d) => {
      const oc = d.filter((x) => x.id === "wheel_of_change")[0]!.choices[0]!.outcome as Record<string, unknown>;
      ((oc.roll as { outcomes: Record<string, unknown>[] }).outcomes[0] as Record<string, unknown>).gold_per_act = 50;
    },
  },
  {
    name: "改覆盖(实现):给 THE_NEST[1] 塞没登记的 outcome 字段",
    want: "没进口径",
    impl: (d) => {
      (d.filter((x) => x.id === "the_nest")[0]!.choices[1]!.outcome as Record<string, unknown>).made_up_field = 1;
    },
  },
  {
    name: "改覆盖(语料):给 WING_STATUE[0] 塞没登记的 effect",
    want: "没进口径",
    corpus: (c) => {
      const o = c.find((e) => e.id === "WING_STATUE")!.options[0]!;
      o.outcomes = [...(o.outcomes ?? []), { effect: "telekinesis", amount: 1, id: null }];
    },
  },
];

function selftest(): void {
  const corpus = loadCorpus();
  const defs = loadImpl();
  const src = loadSources();
  const base = audit(corpus, defs, src);
  if (base.diffs.length || base.guards.length) {
    console.log("自检基准不干净:先让全量审计通过再跑 --selftest");
    printReport(base, "基准");
    process.exit(1);
  }
  let caught = 0;
  let missed = 0;
  for (const mut of MUTATIONS) {
    const c = structuredClone(corpus);
    const d = structuredClone(defs);
    const s = { run: src.run, events: src.events };
    mut.corpus?.(c);
    mut.impl?.(d);
    mut.src?.(s);
    const r = audit(c, d, s);
    const all = [...r.diffs, ...r.guards];
    if (all.some((x) => x.includes(mut.want))) caught++;
    else {
      missed++;
      console.log(`[FAIL] ${mut.name}:没抓到(关键词 "${mut.want}")`);
      if (all.length) console.log(`    实际: ${all.slice(0, 3).join(" | ")}`);
    }
  }
  const restore = audit(corpus, defs, src);
  if (restore.diffs.length || restore.guards.length) {
    console.log("[FAIL] 恢复后不干净");
    missed++;
  }
  console.log(`恢复:基准重跑 -> 不一致 ${restore.diffs.length} 处,守卫 ${restore.guards.length ? "FAIL" : "PASS"}`);
  console.log(`自检:抓到 ${caught},漏检 ${missed}(共 ${MUTATIONS.length} 个模式 + 1 次恢复)`);
  process.exit(missed === 0 ? 0 : 1);
}

// ---- 覆盖清单(--list) ----

function printList(corpus: CorpusEvent[], defs: ImplDef[]): void {
  let totalOpts = 0;
  let specialOpts = 0;
  for (const ev of corpus) {
    const groups = locGroups(ev);
    console.log(`${ev.id.toLowerCase()}  [${ev.pool}]`);
    for (let i = 0; i < ev.options.length; i++) {
      const opt = ev.options[i]!;
      const group = groups[i]!;
      const impl = nthChoices(defs, group[0]!.id, group[0]!.n)[group[0]!.choice];
      const tag = `${ev.id}[${i}]`;
      const how = SPECIAL[tag] ? "登记" : "机械";
      if (SPECIAL[tag]) specialOpts++;
      totalOpts++;
      const a15 = opt.a15 ? " +A15" : "";
      console.log(`  [${i}] ${how}${a15}  ${opt.label.slice(0, 46).padEnd(46)} -> ${impl ? impl.label.slice(0, 46) : "?"}`);
    }
  }
  console.log(`\n共 ${corpus.length} 事件 / ${totalOpts} 选项(${specialOpts} 个走登记表,其余机械对账)`);
}

// ============================================================================
// main
// ============================================================================

if (LIST) {
  printList(loadCorpus(), loadImpl());
} else if (SELFTEST) {
  selftest();
} else {
  const corpus = loadCorpus();
  const defs = loadImpl();
  const filtered = ONLY ? corpus.filter((e) => e.id.toLowerCase() === ONLY.toLowerCase()) : corpus;
  if (ONLY && filtered.length === 0) {
    console.error(`没有匹配的事件:${ONLY}`);
    process.exit(2);
  }
  const r = audit(filtered, defs, loadSources(), !ONLY);
  printReport(r, ONLY ? `--only ${ONLY}` : `${corpus.length} 个事件`);
  process.exit(r.diffs.length || r.guards.length ? 1 : 0);
}

