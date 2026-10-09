// 遗物钩子审计:对 155 件已实现遗物逐件核对"触发时点 + 数值".
//
//   bun tools/sandbox_relics.ts
//   bun tools/sandbox_relics.ts --seed 12345 --out tools/golden/relic_coverage.txt
//
// 战斗内钩子用 `spire --sandbox-batch` 实测;一局流程侧的钩子不能在战斗沙盒里测,
// 表里 oracle 填 src/core/relics.rs(或 run.rs)里 Run 级的断言测试名。
// 清单来源 = tools/golden/relic_fx_map.txt 的 155 件已实现遗物(工具会自校验全覆盖)。

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const HERE = dirname(new URL(import.meta.url).pathname);
const SPIRE = process.env.SPIRE_BIN ?? join(HERE, "..", "target", "debug", "spire");
if (!existsSync(SPIRE)) throw new Error(`找不到 ${SPIRE},先 cargo build`);

const argv = process.argv.slice(2);
const flag = (name: string) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};
const SEED = flag("--seed") ?? "12345";
const OUT = flag("--out");

// ---- 沙盒输出 schema ----
interface EnemyOut {
  id: string;
  hp: number;
  max_hp: number;
  block: number;
  dead: boolean;
  move: string;
  powers: Record<string, number>;
}
interface St {
  turn: number;
  energy: number;
  max_energy: number;
  player: { hp: number; max_hp: number; block: number; powers: Record<string, number> };
  hand: string[];
  draw: string[];
  discard: string[];
  exhaust: string[];
  potions: (string | null)[];
  enemies: EnemyOut[];
  counters?: Record<string, number>;
}
interface Row {
  step: number;
  op: string;
  c?: number;
  error?: string;
  st?: St;
  report?: { candidates: string[]; costs: number[] };
}

// ---- 场景构造 ----
const HI = 999;
const ENEMY = (over: Record<string, unknown> = {}) => ({ id: "cultist", hp: HI, max_hp: HI, move: "Incantation", ...over });
// 显式牌堆:会覆盖开局抽牌,用来把局面摆死
const board = (extra: Record<string, unknown>) => ({
  player: { hp: 80, max_hp: 80 },
  relics: [],
  potions: [null, null, null],
  hand: ["defend"],
  draw: [],
  discard: [],
  exhaust: [],
  enemies: [ENEMY()],
  actions: [],
  ...extra,
});
// 牌组模式:不写显式牌堆,让开局抽牌(以及开战加牌的遗物)按真实流程走
const deckboard = (extra: Record<string, unknown>) => ({
  player: { hp: 80, max_hp: 80 },
  relics: [],
  potions: [null, null, null],
  enemies: [ENEMY()],
  actions: [],
  ...extra,
});
const deckOf = (n: number, id: string) => Array.from({ length: n }, () => id);
const playFirst = [{ op: "play", hand: 0, target: 0 }];
const plays = (n: number) => Array.from({ length: n }, () => ({ op: "play", hand: 0, target: 0 }));

// ---- 输出读取 ----
const withSt = (r: Row[]) => r.filter((x) => x.st);
const init = (r: Row[]) => withSt(r)[0]!.st!;
const last = (r: Row[]) => withSt(r)[withSt(r).length - 1]!.st!;
const dealt = (r: Row[], i = 0) => init(r).enemies[i]!.hp - last(r).enemies[i]!.hp;
const hurt = (r: Row[]) => init(r).player.hp - last(r).player.hp;
const has = (powers: Record<string, number>, key: string) => Object.prototype.hasOwnProperty.call(powers, key);
const playsOk = (r: Row[]) => r.filter((x) => x.op === "play" && x.st).length;

type Check = (rows: Row[], all: Map<string, Row[]>) => string | null;
interface RelicRow {
  id: string;
  hook: string;
  expected: string;
  kind: "sandbox" | "run" | "both" | "untestable";
  oracle: string;
  scenario?: Record<string, unknown>;
  baseScenario?: Record<string, unknown>;
  check?: Check;
  test?: string;
  reason?: string;
}
const rows: RelicRow[] = [];
const S = (id: string, hook: string, expected: string, scenario: Record<string, unknown>, check: Check, over: Partial<RelicRow> = {}) =>
  rows.push({ id, hook, expected, kind: "sandbox", oracle: id, scenario, check, ...over });
const B = (id: string, hook: string, expected: string, scenario: Record<string, unknown>, check: Check, test: string) =>
  rows.push({ id, hook, expected, kind: "both", oracle: `${id} + ${test}`, scenario, check, test });
const R = (id: string, hook: string, expected: string, test: string) =>
  rows.push({ id, hook, expected, kind: "run", oracle: test, test });
const U = (id: string, hook: string, expected: string, reason: string) =>
  rows.push({ id, hook, expected, kind: "untestable", oracle: "(无)", reason });

// ================= 战斗开始 / 开局 =================
S("ring_of_the_snake", "战斗开始(开局抽牌)", "开局多抽 2 张(手牌 5+2=7)", deckboard({ relics: ["ring_of_the_snake"], deck: deckOf(20, "strike"), actions: [{ op: "noop" }] }),
  (r) => (init(r).hand.length === 7 ? null : `开局手牌 ${init(r).hand.length},期望 7`));
S("bag_of_preparation", "战斗开始(开局抽牌)", "开局多抽 2 张(手牌 5+2=7)", deckboard({ relics: ["bag_of_preparation"], deck: deckOf(20, "strike"), actions: [{ op: "noop" }] }),
  (r) => (init(r).hand.length === 7 ? null : `开局手牌 ${init(r).hand.length},期望 7`));
S("akabeko", "战斗开始(振奋 8)", "本场第一张攻击 +8:strike 6->14", board({ relics: ["akabeko"], hand: ["strike"], actions: playFirst }),
  (r) => (dealt(r) === 14 ? null : `首击 ${dealt(r)},期望 14`));
S("anchor", "战斗开始(格挡)", "开局 10 格挡", board({ relics: ["anchor"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.block === 10 ? null : `开局格挡 ${init(r).player.block},期望 10`));
S("vajra", "战斗开始(力量)", "开局 1 力量", board({ relics: ["vajra"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.strength === 1 ? null : `开局力量 ${init(r).player.powers.strength},期望 1`));
S("oddly_smooth_stone", "战斗开始(敏捷)", "开局 1 敏捷", board({ relics: ["oddly_smooth_stone"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.dexterity === 1 ? null : `开局敏捷 ${init(r).player.powers.dexterity},期望 1`));
S("thread_and_needle", "战斗开始(镀甲)", "开局 4 镀甲", board({ relics: ["thread_and_needle"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.plated_armor === 4 ? null : `开局镀甲 ${init(r).player.powers.plated_armor},期望 4`));
S("bronze_scales", "战斗开始(荆棘)/被打", "开局 3 荆棘,被打时反伤 3", board({ relics: ["bronze_scales"], enemies: [ENEMY({ move: "Dark Strike" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (HI - last(r).enemies[0]!.hp === 3 ? null : `荆棘反伤 ${HI - last(r).enemies[0]!.hp},期望 3`));
S("clockwork_souvenir", "战斗开始(人工制品)", "开局 1 人工制品", board({ relics: ["clockwork_souvenir"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.artifact === 1 ? null : `开局人工制品 ${init(r).player.powers.artifact},期望 1`));
S("lantern", "战斗第一回合(能量)", "第一回合 +1 能量:3->4", board({ relics: ["lantern"], actions: [{ op: "noop" }] }),
  (r) => (init(r).energy === 4 && init(r).max_energy === 3 ? null : `第一回合能量 ${init(r).energy}/${init(r).max_energy},期望 4/3`));
S("ancient_tea_set", "营火后第一场(能量)", "休息过则第一回合 +2 能量:3->5", board({ relics: ["ancient_tea_set"], rested: true, actions: [{ op: "noop" }] }),
  (r) => (init(r).energy === 5 ? null : `休息后第一回合能量 ${init(r).energy},期望 5`));
S("blood_vial", "战斗开始(回血)", "开局回 2:40->42", board({ player: { hp: 40, max_hp: 80 }, relics: ["blood_vial"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.hp === 42 ? null : `开局血量 ${init(r).player.hp},期望 42`));
S("bag_of_marbles", "战斗开始(敌方易伤)", "所有敌人开局 1 易伤", board({ relics: ["bag_of_marbles"], actions: [{ op: "noop" }] }),
  (r) => (init(r).enemies[0]!.powers.vulnerable === 1 ? null : `敌方易伤 ${init(r).enemies[0]!.powers.vulnerable},期望 1`));
S("red_mask", "战斗开始(敌方虚弱)", "所有敌人开局 1 虚弱", board({ relics: ["red_mask"], actions: [{ op: "noop" }] }),
  (r) => (init(r).enemies[0]!.powers.weak === 1 ? null : `敌方虚弱 ${init(r).enemies[0]!.powers.weak},期望 1`));
S("gremlin_visage", "战斗开始(自身虚弱)", "开局自身 1 虚弱", board({ relics: ["gremlin_visage"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.weak === 1 ? null : `自身虚弱 ${init(r).player.powers.weak},期望 1`));
S("red_skull", "战斗开始(半血力量)", "血量 <=50% 时 3 力量", board({ player: { hp: 40, max_hp: 80 }, relics: ["red_skull"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.strength === 3 ? null : `半血力量 ${init(r).player.powers.strength},期望 3`));
S("du_vu_doll", "战斗开始(每诅咒力量)", "牌组每张诅咒 +1 力量:2 诅咒->2", board({ relics: ["du_vu_doll"], hand: ["defend"], draw: ["injury", "injury"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.powers.strength === 2 ? null : `诅咒力量 ${init(r).player.powers.strength},期望 2`));
S("philosophers_stone", "战斗开始(能量/敌力)", "每回合 +1 能量且敌人 +1 力量", board({ relics: ["philosophers_stone"], actions: [{ op: "noop" }] }),
  (r) => {
    const s = init(r);
    if (s.max_energy !== 4 || s.energy !== 4) return `能量 ${s.energy}/${s.max_energy},期望 4/4`;
    return s.enemies[0]!.powers.strength === 1 ? null : `敌方力量 ${s.enemies[0]!.powers.strength},期望 1`;
  });
S("brimstone", "回合开始(力量)", "每回合自身 +2、敌人 +1 力量", board({ relics: ["brimstone"], actions: [{ op: "noop" }] }),
  (r) => {
    const s = init(r);
    if (s.player.powers.strength !== 2) return `自身力量 ${s.player.powers.strength},期望 2`;
    return s.enemies[0]!.powers.strength === 1 ? null : `敌方力量 ${s.enemies[0]!.powers.strength},期望 1`;
  });
S("mark_of_pain", "战斗开始(能量/伤口)", "每回合 +1 能量,开局洗 2 张伤口", deckboard({ relics: ["mark_of_pain"], deck: deckOf(20, "strike"), actions: [{ op: "noop" }] }),
  (r) => {
    const s = init(r);
    if (s.max_energy !== 4) return `最大能量 ${s.max_energy},期望 4`;
    const wounds = [...s.hand, ...s.draw].filter((c) => c === "wound").length;
    return wounds === 2 ? null : `伤口张数 ${wounds},期望 2`;
  });
S("enchiridion", "战斗开始(随机能力牌)", "仅开局加 1 张能力牌(第 2 回合不再加)", deckboard({ relics: ["enchiridion"], deck: deckOf(25, "strike"), actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => {
    if (init(r).hand.length !== 6) return `开局手牌 ${init(r).hand.length},期望 6(5+1)`;
    const t2 = last(r);
    return t2.hand.length === 5 ? null : `第 2 回合手牌 ${t2.hand.length},期望 5(战斗开始才加)`;
  });
S("snecko_eye", "战斗开始(混乱/抽牌)", "每回合多抽 2,开局混乱", deckboard({ relics: ["snecko_eye"], deck: deckOf(20, "strike"), actions: [{ op: "noop" }] }),
  (r) => {
    const s = init(r);
    if (s.hand.length !== 7) return `开局手牌 ${s.hand.length},期望 7`;
    return has(s.player.powers, "confused") ? null : "开局没有混乱";
  });
S("ring_of_the_serpent", "每回合开始(抽牌)", "每回合多抽 1:开局 6 张", deckboard({ relics: ["ring_of_the_serpent"], deck: deckOf(20, "strike"), actions: [{ op: "noop" }] }),
  (r) => (init(r).hand.length === 6 ? null : `开局手牌 ${init(r).hand.length},期望 6`));
B("runic_dome", "战斗开始(能量)", "每回合 +1 能量(4/4)", board({ relics: ["runic_dome"], actions: [{ op: "noop" }] }),
  (r) => (init(r).energy === 4 && init(r).max_energy === 4 ? null : `能量 ${init(r).energy}/${init(r).max_energy},期望 4/4`),
  "runic_dome_energy");
B("busted_crown", "战斗开始(能量)/奖励", "每回合 +1 能量;奖励少 2 张", board({ relics: ["busted_crown"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "busted_crown_card_reward_minus_two");
B("cursed_key", "战斗开始(能量)/宝箱", "每回合 +1 能量;开箱得诅咒", board({ relics: ["cursed_key"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "cursed_key_curse_on_chest");
B("ectoplasm", "战斗开始(能量)/金币", "每回合 +1 能量;不能再获得金币", board({ relics: ["ectoplasm"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "ectoplasm_blocks_gold");
B("sozu", "战斗开始(能量)/药水", "每回合 +1 能量;不能再获得药水", board({ relics: ["sozu"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "sozu_blocks_potions");
B("coffee_dripper", "战斗开始(能量)/营火", "每回合 +1 能量;不能再休息", board({ relics: ["coffee_dripper"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "coffee_dripper_blocks_rest");
B("fusion_hammer", "战斗开始(能量)/营火", "每回合 +1 能量;不能再锻造", board({ relics: ["fusion_hammer"], actions: [{ op: "noop" }] }),
  (r) => (init(r).max_energy === 4 ? null : `最大能量 ${init(r).max_energy},期望 4`),
  "fusion_hammer_blocks_smith");
S("slavers_collar", "精英/首领战斗能量", "仅精英/首领开战 +1 能量", {
  player: { hp: 80, max_hp: 80 },
  relics: ["slavers_collar"],
  potions: [null, null, null],
  combats: [
    { encounter: "gremlin_nob_solo", enemies: [ENEMY({ id: "gremlin_nob", hp: 200, max_hp: 200, move: null })], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "noop" }] },
    { encounter: "cultist_solo", enemies: [ENEMY({ id: "cultist", hp: 200, max_hp: 200 })], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "noop" }] },
  ],
},
  (r) => {
    const elite = withSt(r).find((x) => x.c === 0 && x.op === "init")!.st!;
    const normal = withSt(r).find((x) => x.c === 1 && x.op === "init")!.st!;
    if (elite.max_energy !== 4) return `精英战最大能量 ${elite.max_energy},期望 4`;
    return normal.max_energy === 3 ? null : `普通战最大能量 ${normal.max_energy},期望 3(不应加成)`;
  });
S("sling_of_courage", "精英战斗力量", "仅精英开战 +2 力量", {
  player: { hp: 80, max_hp: 80 },
  relics: ["sling_of_courage"],
  potions: [null, null, null],
  combats: [
    { encounter: "gremlin_nob_solo", enemies: [ENEMY({ id: "gremlin_nob", hp: 200, max_hp: 200, move: null })], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "noop" }] },
    { encounter: "cultist_solo", enemies: [ENEMY({ id: "cultist", hp: 200, max_hp: 200 })], hand: ["defend"], draw: [], discard: [], exhaust: [], actions: [{ op: "noop" }] },
  ],
},
  (r) => {
    const elite = withSt(r).find((x) => x.c === 0 && x.op === "init")!.st!;
    const normal = withSt(r).find((x) => x.c === 1 && x.op === "init")!.st!;
    if (elite.player.powers.strength !== 2) return `精英战力量 ${elite.player.powers.strength},期望 2`;
    return normal.player.powers.strength === undefined ? null : `普通战力量 ${normal.player.powers.strength},期望无`;
  });
S("pantograph", "首领战斗回血", "首领战开局回 25:40->65", board({ player: { hp: 40, max_hp: 80 }, relics: ["pantograph"], encounter: "the_guardian", enemies: [ENEMY({ id: "the_guardian", hp: 400, max_hp: 400, move: null })], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.hp === 65 ? null : `首领战开局血量 ${init(r).player.hp},期望 65`));
S("preserved_insect", "精英战斗减血", "精英敌人血量 -25%", board({ relics: ["preserved_insect"], encounter: "gremlin_nob_solo", enemies: [{ id: "gremlin_nob", move: null }], actions: [{ op: "noop" }] }),
  (r, all) => {
    const base = all.get("preserved_insect/base")!;
    const b = init(base).enemies[0]!;
    const want = b.hp - Math.floor(b.max_hp * 25 / 100);
    const got = init(r).enemies[0]!.hp;
    return got === want ? null : `精英血量 ${got},期望 ${want}(基数 ${b.hp}/${b.max_hp})`;
  }, { baseScenario: board({ relics: [], encounter: "gremlin_nob_solo", enemies: [{ id: "gremlin_nob", move: null }], actions: [{ op: "noop" }] }) });

// ================= 回合开始 / 回合末 =================
S("art_of_war", "回合开始(无攻击补能量)", "上回合没打攻击则 +1 能量:3->4", board({ relics: ["art_of_war"], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).energy === 4 ? null : `第 2 回合能量 ${last(r).energy},期望 4`));
S("art_of_war", "回合开始(打过攻击则不补)", "上回合打过攻击就没有额外能量", board({ relics: ["art_of_war"], hand: ["strike"], actions: [{ op: "play", hand: 0, target: 0 }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).energy === 3 ? null : `打过攻击后第 2 回合能量 ${last(r).energy},期望 3`));
S("happy_flower", "每 3 回合(能量)", "第 3 回合 +1 能量", board({ relics: ["happy_flower"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 3 && last(r).energy === 4 ? null : `第 3 回合能量 ${last(r).energy},期望 4`));
S("mercury_hourglass", "回合开始(全体伤害)", "每回合开始对全体敌人 3 伤害", board({ relics: ["mercury_hourglass"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (dealt(r) === 3 ? null : `回合开始伤害 ${dealt(r)},期望 3`));
S("horn_cleat", "第 2 回合(格挡)", "第 2 回合开始 +14 格挡", board({ relics: ["horn_cleat"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).player.block === 14 ? null : `第 2 回合格挡 ${last(r).player.block},期望 14`));
S("captains_wheel", "第 3 回合(格挡)", "第 3 回合开始 +18 格挡", board({ relics: ["captains_wheel"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 3 && last(r).player.block === 18 ? null : `第 3 回合格挡 ${last(r).player.block},期望 18`));
S("incense_burner", "每 6 回合(无形)", "第 6 回合开始 1 层无形", board({ relics: ["incense_burner"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 6 && last(r).player.powers.intangible === 1 ? null : `第 6 回合无形 ${last(r).player.powers.intangible},期望 1`));
S("stone_calendar", "第 7 回合末(全体伤害)", "第 7 回合结束对全体 52 伤害", board({ player: { hp: 999, max_hp: 999 }, relics: ["stone_calendar"], enemies: [ENEMY({ hp: 500, max_hp: 500, powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }, { op: "end_turn" }] }),
  (r) => (500 - last(r).enemies[0]!.hp === 52 ? null : `第 7 回合末伤害 ${500 - last(r).enemies[0]!.hp},期望 52`));
S("orichalcum", "回合末(无格挡补)", "回合末无格挡 +6 格挡吃掉 6 点攻击", board({ relics: ["orichalcum"], enemies: [ENEMY({ move: "Dark Strike" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (hurt(r) === 0 ? null : `掉血 ${hurt(r)},期望 0(6 格挡挡下 6 点)`));
S("cloak_clasp", "回合末(手牌格挡)", "手牌每张 +1 格挡,2 张挡下 2 点", board({ relics: ["cloak_clasp"], hand: ["strike", "defend"], enemies: [ENEMY({ move: "Dark Strike", powers: { strength: -4 } })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (hurt(r) === 0 ? null : `掉血 ${hurt(r)},期望 0(2 张手牌 2 格挡挡下 2 点)`));
S("calipers", "回合开始(格挡上限)", "回合开始只掉 15 格挡:40->25", board({ player: { hp: 80, max_hp: 80, block: 40 }, relics: ["calipers"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (last(r).player.block === 25 ? null : `第 2 回合格挡 ${last(r).player.block},期望 25`));
S("ice_cream", "回合开始(能量保留)", "未用完的能量留到下回合:3 打 1 剩 2 -> 下回合 5", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["ice_cream"], actions: [{ op: "play", hand: 0 }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).energy === 5 ? null : `第 2 回合能量 ${last(r).energy},期望 5`));
S("warped_tongs", "回合开始(升级手牌)", "回合开始随机升级手里一张牌", deckboard({ relics: ["warped_tongs"], deck: [...deckOf(10, "strike"), ...deckOf(10, "defend")], actions: [{ op: "noop" }] }),
  (r) => (init(r).hand.some((c) => c.includes("+")) ? null : `开局没有升级牌:[${init(r).hand.join(",")}]`));
S("pocketwatch", "回合开始(少出牌补抽)", "出 <=3 张牌的下回合多抽 3:第 2 回合 5+3=8;第 1 回合不抽", deckboard({ relics: ["pocketwatch"], deck: deckOf(25, "strike"), actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => {
    if (init(r).hand.length !== 5) return `第 1 回合手牌 ${init(r).hand.length},期望 5(不该多抽)`;
    return last(r).turn === 2 && last(r).hand.length === 8 ? null : `第 2 回合手牌 ${last(r).hand.length},期望 8`;
  });
S("pocketwatch", "回合开始(出牌多则不补)", "出 4 张牌的下回合不多抽", deckboard({ player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9 }, relics: ["pocketwatch"], deck: deckOf(25, "strike"), actions: [{ op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "play", hand: 0, target: 0 }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).hand.length === 5 ? null : `出 4 张后第 2 回合手牌 ${last(r).hand.length},期望 5`));
S("runic_pyramid", "回合末(留手牌)", "回合末不再弃手牌", board({ relics: ["runic_pyramid"], hand: ["strike", "defend"], draw: deckOf(5, "strike"), actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => {
    const t2 = last(r);
    if (t2.turn !== 2) return `回合 ${t2.turn},期望 2`;
    if (!t2.hand.includes("strike") || !t2.hand.includes("defend")) return `保留失败:[${t2.hand.join(",")}]`;
    return t2.hand.length === 7 ? null : `第 2 回合手牌 ${t2.hand.length},期望 7(2 保留 + 5 抽)`;
  });
S("mutagenic_strength", "战斗开始/首回合末(力量)", "首回合 +3 力量,回合末收回", board({ relics: ["mutagenic_strength"], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => {
    if (init(r).player.powers.strength !== 3) return `开局力量 ${init(r).player.powers.strength},期望 3`;
    return !has(last(r).player.powers, "strength") ? null : `首回合末力量 ${last(r).player.powers.strength},期望无`;
  });

// ================= 打出牌触发 =================
S("strike_dummy", "攻击牌结算(打击加伤)", "名字带 Strike 的牌 +3:strike 6->9", board({ relics: ["strike_dummy"], hand: ["strike"], actions: playFirst }),
  (r) => (dealt(r) === 9 ? null : `打击 ${dealt(r)},期望 9`));
S("wrist_blade", "攻击牌结算(0 费加伤)", "0 费攻击 +4:swift_strike 7->11", board({ relics: ["wrist_blade"], hand: ["swift_strike"], actions: playFirst }),
  (r) => (dealt(r) === 11 ? null : `0 费攻击 ${dealt(r)},期望 11`));
S("the_boot", "攻击结算(低伤抬到 5)", "4 点以下未格挡攻击抬到 5:虚弱 strike 4->5", board({ relics: ["the_boot"], player: { hp: 80, max_hp: 80, powers: { weak: 10 } }, hand: ["strike"], actions: playFirst }),
  (r) => (dealt(r) === 5 ? null : `低伤抬升后 ${dealt(r)},期望 5`));
S("pen_nib", "每 10 张攻击(翻倍)", "第 10 张攻击伤害翻倍:10*6 中第 10 张 12,合计 66", board({ player: { hp: 80, max_hp: 80, energy: 10, max_energy: 10 }, relics: ["pen_nib"], hand: deckOf(10, "strike"), actions: plays(10) }),
  (r) => (dealt(r) === 66 ? null : `10 张打击合计 ${dealt(r)},期望 66`));
S("nunchaku", "每 10 张攻击(能量)", "第 10 张攻击 +1 能量", board({ player: { hp: 80, max_hp: 80, energy: 10, max_energy: 10 }, relics: ["nunchaku"], hand: deckOf(10, "strike"), actions: plays(10) }),
  (r) => (last(r).energy === 1 ? null : `10 张攻击后能量 ${last(r).energy},期望 1`));
S("ink_bottle", "每 10 张牌(抽牌)", "第 10 张牌后抽 1 张", board({ player: { hp: 80, max_hp: 80, energy: 10, max_energy: 10 }, relics: ["ink_bottle"], hand: deckOf(10, "defend"), draw: ["strike"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: plays(10) }),
  (r) => (last(r).hand.length === 1 ? null : `10 张牌后手牌 ${last(r).hand.length},期望 1(抽了 1)`));
S("kunai", "每 3 张攻击(敏捷)", "单回合 3 张攻击 +1 敏捷", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["kunai"], hand: deckOf(3, "strike"), actions: plays(3) }),
  (r) => (last(r).player.powers.dexterity === 1 ? null : `3 张攻击后敏捷 ${last(r).player.powers.dexterity},期望 1`));
S("shuriken", "每 3 张攻击(力量)", "单回合 3 张攻击 +1 力量", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["shuriken"], hand: deckOf(3, "strike"), actions: plays(3) }),
  (r) => (last(r).player.powers.strength === 1 ? null : `3 张攻击后力量 ${last(r).player.powers.strength},期望 1`));
S("ornamental_fan", "每 3 张攻击(格挡)", "单回合 3 张攻击 +4 格挡", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["ornamental_fan"], hand: deckOf(3, "strike"), actions: plays(3) }),
  (r) => (last(r).player.block === 4 ? null : `3 张攻击后格挡 ${last(r).player.block},期望 4`));
S("letter_opener", "每 3 张技能(全体伤害)", "单回合 3 张技能对全体 5 伤害", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["letter_opener"], hand: deckOf(3, "defend"), actions: plays(3) }),
  (r) => (dealt(r) === 5 ? null : `3 张技能后伤害 ${dealt(r)},期望 5`));
S("bird_faced_urn", "打出能力牌(回血)", "每张能力牌回 2:40->42", board({ player: { hp: 40, max_hp: 80 }, relics: ["bird_faced_urn"], hand: ["inflame", "defend"], actions: playFirst }),
  (r) => (last(r).player.hp === 42 ? null : `能力牌回血后 ${last(r).player.hp},期望 42`));
S("mummified_hand", "打出能力牌(手牌 0 费)", "能力牌后手里一张随机牌本回合 0 费", board({ player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9 }, relics: ["mummified_hand"], hand: ["inflame", "strike", "defend"], actions: playFirst, report: true }),
  (r) => {
    const rep = withSt(r).find((x) => x.op === "play")!.report!;
    return rep.costs.includes(0) ? null : `手牌费用 [${rep.costs.join(",")}],期望有 0 费`;
  });
S("orange_pellets", "三种类型都打出(清减益)", "同回合打出能力/攻击/技能后清自身减益", board({ player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9, powers: { vulnerable: 5, weak: 5 } }, relics: ["orange_pellets"], hand: ["inflame", "strike", "defend"], draw: [], actions: plays(3) }),
  (r) => {
    const p = last(r).player.powers;
    return !has(p, "vulnerable") && !has(p, "weak") ? null : `减益未清:[${JSON.stringify(p)}]`;
  });
S("chemical_x", "X 费牌结算(+2)", "X 费效果 +2:3 能量旋风斩 5*(3+2)=25", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 9 }, relics: ["chemical_x"], hand: ["whirlwind"], actions: playFirst }),
  (r) => (dealt(r) === 25 ? null : `旋风斩 ${dealt(r)},期望 25`));
S("sacred_bark", "药水结算(翻倍)", "药水数值翻倍:火焰药水 20->40", board({ relics: ["sacred_bark"], potions: ["fire_potion", null, null], hand: ["defend"], actions: [{ op: "potion", slot: 0, target: 0 }] }),
  (r) => (dealt(r) === 40 ? null : `圣树皮火焰药水 ${dealt(r)},期望 40`));
S("blue_candle", "打出诅咒", "可打诅咒,打出掉 1 血并消耗", board({ relics: ["blue_candle"], hand: ["injury", "defend"], actions: playFirst }),
  (r) => {
    const s = last(r);
    if (s.player.hp !== 79) return `血量 ${s.player.hp},期望 79`;
    return s.exhaust.includes("injury") ? null : `消耗堆 [${s.exhaust.join(",")}],期望含 injury`;
  });
S("medical_kit", "打出状态牌", "可打状态牌并消耗", board({ relics: ["medical_kit"], hand: ["wound", "defend"], actions: playFirst }),
  (r) => {
    const s = last(r);
    if (s.player.hp !== 80) return `血量 ${s.player.hp},期望 80`;
    return s.exhaust.includes("wound") ? null : `消耗堆 [${s.exhaust.join(",")}],期望含 wound`;
  });
S("necronomicon", "首张 >=2 费攻击(双打)", "第一张 2 费以上攻击再打一次:bludgeon 32->64", board({ player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9 }, relics: ["necronomicon"], hand: ["bludgeon"], actions: playFirst }),
  (r) => (dealt(r) === 64 ? null : `死灵之书 bludgeon ${dealt(r)},期望 64`));
S("champion_belt", "上易伤(附带虚弱)", "上易伤时附带 1 虚弱", board({ relics: ["champion_belt"], hand: ["bash"], actions: playFirst }),
  (r) => {
    const p = last(r).enemies[0]!.powers;
    return p.vulnerable === 2 && p.weak === 1 ? null : `敌方 [${JSON.stringify(p)}],期望 vuln2+weak1`;
  });
S("hand_drill", "击破格挡(上易伤)", "击破敌人格挡时上 2 易伤", board({ relics: ["hand_drill"], enemies: [ENEMY({ block: 6 })], hand: ["strike"], actions: playFirst }),
  (r) => (last(r).enemies[0]!.powers.vulnerable === 2 ? null : `破格挡后易伤 ${last(r).enemies[0]!.powers.vulnerable},期望 2`));

// ================= 掉血 / 受伤触发 =================
S("centennial_puzzle", "本场首次掉血(抽牌)", "首次掉血抽 3 张", board({ relics: ["centennial_puzzle"], hand: ["hemokinesis"], draw: deckOf(5, "strike"), enemies: [ENEMY()], actions: playFirst }),
  (r) => {
    const s = last(r);
    if (s.player.hp !== 78) return `掉血后血量 ${s.player.hp},期望 78`;
    return s.hand.length === 3 ? null : `手牌 ${s.hand.length},期望 3(抽了 3)`;
  });
S("runic_cube", "每次掉血(抽牌)", "每次掉血抽 1 张", board({ relics: ["runic_cube"], hand: ["hemokinesis"], draw: deckOf(5, "strike"), enemies: [ENEMY()], actions: playFirst }),
  (r) => (last(r).hand.length === 1 ? null : `掉血后手牌 ${last(r).hand.length},期望 1`));
S("self_forming_clay", "掉血(下回合格挡)", "掉血后下回合 +3 格挡", board({ relics: ["self_forming_clay"], hand: ["hemokinesis"], enemies: [ENEMY({ powers: { strength: -20 } })], actions: [{ op: "play", hand: 0, target: 0 }, { op: "end_turn" }] }),
  (r) => (last(r).turn === 2 && last(r).player.block === 3 ? null : `第 2 回合格挡 ${last(r).player.block},期望 3`));
S("fossilized_helix", "本场首次掉血(免伤)", "首次掉血完全免掉", board({ player: { hp: 40, max_hp: 80 }, relics: ["fossilized_helix"], hand: ["hemokinesis"], enemies: [ENEMY()], actions: playFirst }),
  (r) => (last(r).player.hp === 40 ? null : `血量 ${last(r).player.hp},期望 40(免掉 2 点)`));
S("tungsten_rod", "每次掉血(-1)", "每次掉血少掉 1:6->5", board({ relics: ["tungsten_rod"], enemies: [ENEMY({ move: "Dark Strike" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (hurt(r) === 5 ? null : `掉血 ${hurt(r)},期望 5`));
S("torii", "<=5 点攻击(减为 1)", "5 点以下未格挡攻击降为 1:5->1", board({ relics: ["torii"], enemies: [ENEMY({ move: "Dark Strike", powers: { strength: -1 } })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (hurt(r) === 1 ? null : `掉血 ${hurt(r)},期望 1`));
S("lizard_tail", "致命伤(免死)", "致命伤改为回到最大生命 50%", board({ player: { hp: 4, max_hp: 80 }, relics: ["lizard_tail"], enemies: [ENEMY({ move: "Dark Strike" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (last(r).player.hp === 40 ? null : `免死后血量 ${last(r).player.hp},期望 40`));
S("odd_mushroom", "自身易伤(减伤)", "自身易伤只多受 25%:6->7", board({ relics: ["odd_mushroom"], player: { hp: 80, max_hp: 80, powers: { vulnerable: 10 } }, enemies: [ENEMY({ move: "Dark Strike" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (hurt(r) === 7 ? null : `掉血 ${hurt(r)},期望 7(6*1.25 向下取整)`));
S("ginger", "免疫虚弱", "不会陷入虚弱(蓝奴隶贩子的 Rake 后仍无虚弱)", board({ relics: ["ginger"], enemies: [ENEMY({ id: "blue_slaver", hp: HI, max_hp: HI, move: "Rake" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (has(last(r).player.powers, "weak") ? `仍中了虚弱:${JSON.stringify(last(r).player.powers)}` : null));
S("turnip", "免疫脆弱", "不会陷入脆弱(尖刺史莱姆 Lick 后仍无脆弱)", board({ relics: ["turnip"], enemies: [ENEMY({ id: "spike_slime_medium", hp: HI, max_hp: HI, move: "Lick" })], actions: [{ op: "noop" }, { op: "end_turn" }] }),
  (r) => (has(last(r).player.powers, "frail") ? `仍中了脆弱:${JSON.stringify(last(r).player.powers)}` : null));
S("magic_flower", "战斗中治疗(增幅)", "战斗内治疗 +50%:血瓶 2->3", board({ player: { hp: 40, max_hp: 80 }, relics: ["magic_flower", "blood_vial"], actions: [{ op: "noop" }] }),
  (r) => (init(r).player.hp === 43 ? null : `开局血量 ${init(r).player.hp},期望 43(2*1.5)`));
S("toy_ornithopter", "使用药水(回血)", "每瓶药水回 5:40->45", board({ player: { hp: 40, max_hp: 80 }, relics: ["toy_ornithopter"], potions: ["fire_potion", null, null], actions: [{ op: "potion", slot: 0, target: 0 }] }),
  (r) => (last(r).player.hp === 45 ? null : `喝药后血量 ${last(r).player.hp},期望 45`));

// ================= 消耗 / 弃牌 / 洗牌 =================
S("charons_ashes", "消耗牌(全体伤害)", "每消耗一张对全体 3 伤害", board({ relics: ["charons_ashes"], hand: ["slimed"], actions: playFirst }),
  (r) => (dealt(r) === 3 ? null : `消耗触发伤害 ${dealt(r)},期望 3`));
S("dead_branch", "消耗牌(加牌)", "每消耗一张向手里加一张随机牌", board({ relics: ["dead_branch"], hand: ["slimed"], actions: playFirst }),
  (r) => (last(r).hand.length === 1 ? null : `消耗后手牌 ${last(r).hand.length},期望 1(枯枝 +1)`));
S("tingsha", "主动弃牌(随机伤害)", "每弃一张对随机敌人 3 伤害", board({ relics: ["tingsha"], potions: ["gamblers_brew", null, null], hand: ["defend"], enemies: [ENEMY()], actions: [{ op: "potion", slot: 0, choose: [0] }] }),
  (r) => (dealt(r) === 3 ? null : `弃牌触发伤害 ${dealt(r)},期望 3`));
S("tough_bandages", "主动弃牌(格挡)", "每弃一张 +3 格挡", board({ relics: ["tough_bandages"], potions: ["gamblers_brew", null, null], hand: ["defend"], actions: [{ op: "potion", slot: 0, choose: [0] }] }),
  (r) => (last(r).player.block === 3 ? null : `弃牌后格挡 ${last(r).player.block},期望 3`));
S("hovering_kite", "每回合首次弃牌(能量)", "每回合首次弃牌 +1 能量:3->4", board({ relics: ["hovering_kite"], potions: ["gamblers_brew", null, null], hand: ["defend"], actions: [{ op: "potion", slot: 0, choose: [0] }] }),
  (r) => (last(r).energy === 4 ? null : `弃牌后能量 ${last(r).energy},期望 4`));
S("the_abacus", "洗牌(格挡)", "每次洗牌 +6 格挡(3 次 -> 18)", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["the_abacus"], hand: deckOf(3, "pommel_strike"), draw: [], discard: ["strike"], actions: plays(3) }),
  (r) => (last(r).player.block === 18 ? null : `3 次洗牌后格挡 ${last(r).player.block},期望 18`));
S("sundial", "每 3 次洗牌(能量)", "每 3 次洗牌 +2 能量:3 打 3 张后 +2 -> 2+2=4", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["sundial"], hand: deckOf(3, "pommel_strike"), draw: [], discard: ["strike"], actions: plays(3) }),
  (r) => (last(r).energy === 2 ? null : `3 次洗牌后能量 ${last(r).energy},期望 2(3-3+2)`));
S("unceasing_top", "手牌空(抽牌)", "手里没牌时抽 1", board({ relics: ["unceasing_top"], hand: ["strike"], draw: ["defend"], actions: playFirst }),
  (r) => (last(r).hand.length === 1 ? null : `手牌 ${last(r).hand.length},期望 1(打完空手补 1)`));

// ================= 击杀 / 其他战斗钩子 =================
S("gremlin_horn", "敌人死亡(能量+抽牌)", "每死一只 +1 能量并抽 1", board({ player: { hp: 80, max_hp: 80, energy: 3, max_energy: 3 }, relics: ["gremlin_horn"], hand: ["strike"], draw: ["defend"], enemies: [ENEMY({ hp: 1, max_hp: 1 })], actions: playFirst }),
  (r) => {
    const s = last(r);
    if (s.energy !== 3) return `击杀后能量 ${s.energy},期望 3(3-1+1)`;
    return s.hand.length === 1 ? null : `击杀后手牌 ${s.hand.length},期望 1(抽了 1)`;
  });
S("velvet_choker", "每回合出牌上限", "一回合最多 6 张,第 7 张报错", board({ player: { hp: 80, max_hp: 80, energy: 9, max_energy: 9 }, relics: ["velvet_choker"], hand: deckOf(7, "strike"), actions: plays(7) }),
  (r) => {
    if (playsOk(r) !== 6) return `成功打出 ${playsOk(r)} 张,期望 6`;
    return r.some((x) => x.error) ? null : "第 7 张没被拦下";
  });
S("paper_phrog", "易伤增伤", "易伤多受 75%:strike 6->10", board({ relics: ["paper_phrog"], enemies: [ENEMY({ powers: { vulnerable: 10 } })], hand: ["strike"], actions: playFirst }),
  (r) => (dealt(r) === 10 ? null : `易伤打击 ${dealt(r)},期望 10(6*1.75)`));
S("paper_krane", "虚弱减伤", "虚弱少受 40%:strike 6->3", board({ relics: ["paper_krane"], player: { hp: 80, max_hp: 80, powers: { weak: 10 } }, hand: ["strike"], actions: playFirst }),
  (r) => (dealt(r) === 3 ? null : `虚弱打击 ${dealt(r)},期望 3(6*0.6)`));

// ================= 一局流程侧(在 relics.rs 的 Run 级测试里断言) =================
R("burning_blood", "战斗结束(回血)", "战后回 6", "burning_blood_post_combat_heal");
R("black_blood", "战斗结束(回血)", "替换燃烧之血,战后回 12", "black_blood_replaces_blood_and_heals");
R("meat_on_the_bone", "战斗结束(半血回血)", "战后血量 <=50% 时回 12", "meat_on_the_bone_heal_threshold");
R("face_of_cleric", "战斗结束(最大生命)", "每次胜利最大生命 +1", "face_of_cleric_max_hp_on_victory");
R("ceramic_fish", "加入牌组(金币)", "每次加牌得 9 金币", "ceramic_fish_gold_on_card_add");
R("dream_catcher", "营火休息(卡牌奖励)", "休息后可再挑一张牌", "dream_catcher_gives_a_card_pick_after_resting");
R("juzu_bracelet", "未知房间(不遇普通战)", "? 房间不再出普通战斗", "juzu_bracelet_skips_normal_combats");
R("maw_bank", "每上一层(金币)", "每层 +12 金币,花过钱后失效", "maw_bank_gold_per_floor_then_stops");
R("meal_ticket", "进入商店(回血)", "进店回 15", "meal_ticket_heals_on_shop");
R("omamori", "获得诅咒(抵消)", "抵消接下来 2 张诅咒", "omamori_negates_two_curses");
R("potion_belt", "拾取(药水格)", "拾取 +2 药水格", "potion_belt_adds_two_slots");
R("regal_pillow", "营火休息(回血加成)", "休息多回 15", "regal_pillow_rest_heal_bonus");
R("smiling_mask", "商店删牌(定价)", "删牌固定 50 金", "smiling_mask_fixes_removal_price");
R("strawberry", "拾取(最大生命)", "拾取 +7 最大生命", "strawberry_max_hp");
R("tiny_chest", "未知房间(宝箱)", "每第 4 个 ? 房间是宝箱", "tiny_chest_treasure_every_fourth_unknown");
R("war_paint", "拾取(升级技能)", "随机升级 2 张技能", "war_paint_upgrades_two_skills");
R("whetstone", "拾取(升级攻击)", "随机升级 2 张攻击", "whetstone_upgrades_two_attacks");
R("darkstone_periapt", "获得诅咒(最大生命)", "每张诅咒 +6 最大生命", "darkstone_periapt_max_hp_per_curse");
R("bottled_flame", "拾取(封装攻击)/开局", "封装一张攻击,开局进手", "bottled_flame_pickup_bottles_an_attack");
R("bottled_lightning", "拾取(封装技能)/开局", "封装一张技能,开局进手", "bottled_lightning_pickup_bottles_a_skill");
R("bottled_tornado", "拾取(封装能力)/开局", "封装一张能力,开局进手", "bottled_tornado_pickup_bottles_a_power");
R("eternal_feather", "营火休息(每 5 张回血)", "每 5 张牌回 3", "eternal_feather_rest_heal");
R("frozen_egg", "加入能力牌(升级)", "加入的能力牌自动升级(加牌统一钩子,商店/奖励/事件都生效)", "frozen_egg_upgrades_powers");
R("molten_egg", "加入攻击牌(升级)", "加入的攻击牌自动升级", "molten_egg_upgrades_attacks");
R("molten_egg", "奖励/商店加牌(升级)", "统一加牌钩子覆盖奖励与商店", "molten_egg_and_ceramic_fish_apply_to_card_rewards");
R("toxic_egg", "加入技能牌(升级)", "加入的技能牌自动升级", "toxic_egg_upgrades_skills");
R("ceramic_fish", "奖励/商店加牌(金币)", "奖励/商店加牌也各给 9 金", "molten_egg_and_ceramic_fish_apply_to_card_rewards");
R("matryoshka", "宝箱(额外遗物)", "接下来 2 个非 Boss 宝箱各 2 件遗物", "matryoshka_adds_an_extra_relic_to_two_chests");
R("pear", "拾取(最大生命)", "拾取 +10 最大生命", "pear_max_hp");
R("question_card", "卡牌奖励(多一张)", "奖励多 1 张候选", "question_card_extra_card_reward");
R("singing_bowl", "跳过卡牌奖励(最大生命)", "跳过奖励可换 +2 最大生命", "singing_bowl_offers_max_hp_on_skip");
R("membership_card", "商店(折扣)", "所有商品降 50%", "membership_card_shop_discount");
R("membership_card", "商店折扣(相乘)", "会员卡 -50% 与信使 -20% 相乘(共约 -60%)", "courier_and_membership_card_discounts_multiply");
R("white_beast_statue", "战斗奖励(药水)", "战斗奖励必出药水", "white_beast_statue_guarantees_potion");
R("golden_idol", "金币奖励(+25%)", "敌人掉落金币 +25%", "golden_idol_gold_reward_bonus");
R("bloody_idol", "获得金币(回血)", "每次获得金币回 5", "bloody_idol_heals_on_gold_gain");
R("neows_lament", "前 3 场战斗(敌人 1 血)", "前 3 场战斗敌人 1 血", "neows_lament_weakens_first_combats");
R("nloths_gift", "卡牌奖励(稀有度)", "稀有牌概率 x3", "nloths_gift_triples_rare_chance");
R("nloths_hungry_face", "宝箱(空箱)", "下一个非 Boss 宝箱为空", "nloths_hungry_face_empties_a_chest");
R("ssserpent_head", "未知房间(金币)", "每进 ? 房间 +50 金币", "ssserpent_head_gold_on_unknown");
R("mark_of_the_bloom", "全局(禁止治疗)", "不能再治疗", "mark_of_the_bloom_blocks_healing");
R("ectoplasm", "全局(禁止金币)", "不能再获得金币", "ectoplasm_blocks_gold");
R("sozu", "全局(禁止药水)", "不能再获得药水", "sozu_blocks_potions");
R("coffee_dripper", "全局(禁止休息)", "营火不能休息", "coffee_dripper_blocks_rest");
R("fusion_hammer", "全局(禁止锻造)", "营火不能锻造", "fusion_hammer_blocks_smith");
R("cursed_key", "非 Boss 宝箱(诅咒)", "开箱得一张诅咒", "cursed_key_curse_on_chest");
R("busted_crown", "卡牌奖励(-2 张)", "奖励候选少 2 张", "busted_crown_card_reward_minus_two");
R("black_star", "精英奖励(额外遗物)", "精英多掉一件遗物", "black_star_extra_elite_relic");
R("lees_waffle", "拾取(最大生命/满血)", "拾取 +7 最大生命并回满", "lees_waffle_max_hp_and_full_heal");
R("mango", "拾取(最大生命)", "拾取 +14 最大生命", "mango_max_hp");
R("old_coin", "拾取(金币)", "拾取 +300 金币", "old_coin_gold");
R("astrolabe", "拾取(转化 3 张并升级)", "转化 3 张牌并升级", "astrolabe_transforms_three");
R("calling_bell", "拾取(3 遗物 + 诅咒)", "拾取 3 件遗物和 1 张诅咒", "calling_bell_three_relics_and_curse");
R("cauldron", "拾取(5 瓶药水)", "拾取时调出 5 瓶药水", "cauldron_five_potions");
R("dollys_mirror", "拾取(复制一张)", "复制牌组里一张牌", "dollys_mirror_duplicates_a_card");
R("empty_cage", "拾取(删 2 张)", "删掉牌组里 2 张", "empty_cage_removes_two");
R("pandoras_box", "拾取(转化)", "转化所有打击与防御", "pandoras_box_transforms_strikes_and_defends");
R("tiny_house", "拾取(综合)", "药水+50 金+5 最大生命+一张牌+升级一张", "tiny_house_pickup_bundle");
R("peace_pipe", "营火(删牌)", "营火可删牌", "peace_pipe_adds_a_rest_removal");
R("shovel", "营火(挖遗物)", "营火可挖遗物", "shovel_digs_up_a_relic");
R("girya", "营火(举铁)", "营火可举铁,上限 3 次", "girya_lifts_and_pays_out_in_combat");
R("wing_boots", "地图(无视路径)", "可无视路径 3 次", "wing_boots_fly_anywhere_three_times");
R("orrery", "拾取(5 次三选一)", "拾取时连开 5 次卡牌三选一", "orrery_offers_five_card_picks");
R("prayer_wheel", "普通战奖励(多一组)", "普通战多一组卡牌奖励", "prayer_wheel_adds_a_second_card_group");
R("prismatic_shard", "卡牌奖励(混色)", "奖励混入无色与其它颜色", "prismatic_shard_mixes_colorless_into_rewards");
R("sacred_bark", "药水(翻倍)", "药水数值翻倍(局外)", "sacred_bark_doubles_out_of_combat_potions");
R("the_courier", "商店(补货)", "商店补货", "courier_restocks_cards_relics_and_potions");
R("the_courier", "商店折扣(相乘)", "信使 -20% 与会员卡 -50% 相乘", "courier_and_membership_card_discounts_multiply");

// ================= 战斗沙盒测不到的(选牌/显示) =================
R("gambling_chip", "战斗开始(弃牌重抽)", "开局可弃任意张再抽等量张(选牌窗口沙盒按不选收掉)", "gambling_chip_discards_then_draws");
R("nilrys_codex", "回合末(洗牌选择)", "回合末亮 3 张挑一张洗进抽牌堆(选牌窗口沙盒按跳过收掉)", "nilrys_codex_shuffles_a_chosen_card_into_the_draw_pile");
R("strange_spoon", "消耗牌(50% 改弃牌)", "该消耗的牌 50% 概率改弃牌(按概率测,沙盒不确定)", "strange_spoon_redirects_exhausting_cards");
R("toolbox", "战斗开始(无色三选一)", "开局亮 3 张无色牌挑一张进手(选牌窗口沙盒按不选收掉)", "toolbox_offers_three_colorless_cards");

U("frozen_eye", "抽牌堆显示(顺序)", "查看抽牌堆时按抽取顺序显示", "仅影响 UI 里抽牌堆的显示顺序,战斗状态与沙盒输出里不可观测");

// ================= 自校验:155 件全覆盖 =================
const mapPath = join(HERE, "golden", "relic_fx_map.txt");
const implemented: string[] = readFileSync(mapPath, "utf8")
  .split("\n")
  .filter((l) => l.trim() !== "" && !l.startsWith("#") && !l.includes("(gated)"))
  .map((l) => l.split("\t")[0]!.trim());
const wanted = [...new Set(implemented)].sort();
const inTable = [...new Set(rows.map((r) => r.id))].sort();
const missing = wanted.filter((id) => !inTable.includes(id));
const extra = inTable.filter((id) => !wanted.includes(id));
if (missing.length || extra.length) {
  throw new Error(`遗物清单不匹配:缺 [${missing.join(",")}],多 [${extra.join(",")}]`);
}
if (rows.some((r) => r.kind === "run" && !r.test)) throw new Error("run 行缺 test 名");
if (rows.some((r) => (r.kind === "sandbox" || r.kind === "both") && !r.scenario)) throw new Error("sandbox 行缺 scenario");
{
  const uncovered = inTable.filter(
    (id) => !rows.some((r) => r.id === id && (r.scenario || r.test || r.kind === "untestable")),
  );
  if (uncovered.length) throw new Error(`这些遗物既没沙盒也没测试:[${uncovered.join(",")}]`);
}

// ================= 跑沙盒 =================
const DIR = mkdtempSync(join(tmpdir(), "spire-relics-"));
const bath = rows.filter((r) => r.scenario);
// 同一件遗物可能有多行场景:每行给一个唯一的名字,免得批量输出互相覆盖
const keyOf = new Map<RelicRow, string>();
{
  const seen = new Map<string, number>();
  for (const r of bath) {
    const n = seen.get(r.id) ?? 0;
    seen.set(r.id, n + 1);
    keyOf.set(r, n === 0 ? r.id : `${r.id}~${n}`);
  }
}
const list: string[] = [];
bath.forEach((r, i) => {
  const key = keyOf.get(r)!;
  const p = join(DIR, `s${i}.json`);
  writeFileSync(p, JSON.stringify(r.scenario));
  list.push(`${key}\t${p}`);
  if (r.baseScenario) {
    const bp = join(DIR, `b${i}.json`);
    writeFileSync(bp, JSON.stringify(r.baseScenario));
    list.push(`${key}/base\t${bp}`);
  }
});
const listPath = join(DIR, "list.txt");
writeFileSync(listPath, list.join("\n"));
const run = spawnSync(SPIRE, ["--sandbox-batch", SEED, listPath], { encoding: "utf8", maxBuffer: 1 << 27 });
if (run.status !== 0) throw new Error(`沙盒跑不动(${run.status}): ${run.stderr}`);
const seg = new Map<string, Row[]>();
{
  let name: string | null = null;
  let cur: Row[] = [];
  for (const line of run.stdout.split("\n")) {
    if (line.startsWith("#")) {
      if (name !== null) seg.set(name, cur);
      name = line.slice(1);
      cur = [];
    } else if (line.trim() !== "") cur.push(JSON.parse(line) as Row);
  }
  if (name !== null) seg.set(name, cur);
}

// ================= 结论 =================
interface Verdict {
  row: RelicRow;
  status: "ok" | "mismatch" | "run-test" | "untestable";
  detail: string;
}
const verdicts: Verdict[] = [];
for (const r of rows) {
  if (r.kind === "untestable") {
    verdicts.push({ row: r, status: "untestable", detail: r.reason ?? "" });
    continue;
  }
  if (!r.scenario) {
    verdicts.push({ row: r, status: "run-test", detail: r.test ?? "" });
    continue;
  }
  const key = keyOf.get(r) ?? r.id;
  const buf = seg.get(key);
  if (!buf) {
    verdicts.push({ row: r, status: "mismatch", detail: "沙盒没有输出" });
    continue;
  }
  const fail = [...buf, ...(seg.get(`${key}/base`) ?? [])].find((x) => x.op === "fail");
  if (fail) {
    verdicts.push({ row: r, status: "mismatch", detail: `场景错误:${fail.error}` });
    continue;
  }
  let msg: string | null = null;
  try {
    msg = r.check ? r.check(buf, seg) : null;
  } catch (e) {
    msg = `断言异常:${(e as Error).message}`;
  }
  if (msg) verdicts.push({ row: r, status: "mismatch", detail: msg });
  else verdicts.push({ row: r, status: r.test ? "ok" : "ok", detail: r.test ? `真跑通;流程侧见 ${r.test}` : "" });
}

const sandboxRows = rows.filter((r) => r.scenario).length;
const runOnly = rows.filter((r) => !r.scenario && r.kind === "run").length;
const untestable = rows.filter((r) => r.kind === "untestable").length;
const idsSandbox = new Set(rows.filter((r) => r.scenario).map((r) => r.id));
const idsRun = new Set(rows.filter((r) => r.test).map((r) => r.id));
const idsUntestable = new Set(rows.filter((r) => r.kind === "untestable").map((r) => r.id));
const idsRunOnly = new Set([...idsRun].filter((id) => !idsSandbox.has(id)));
const uniqueIds = new Set(rows.map((r) => r.id)).size;
const mismatches = verdicts.filter((v) => v.status === "mismatch");
const ok = verdicts.filter((v) => v.status === "ok").length;

const report: string[] = [];
report.push("遗物钩子审计报告");
report.push(`种子 seed=${SEED}`);
report.push("命令: bun tools/sandbox_relics.ts");
report.push("");
report.push("覆盖统计:");
report.push(`  已实现遗物 ${uniqueIds} 件,表行 ${rows.length} 条`);
report.push(`  - 沙盒实测(战斗内钩子):${idsSandbox.size} 件(沙盒行 ${sandboxRows} 条,通过 ${ok},不一致 ${mismatches.length})`);
report.push(`  - 纯 run-level 断言测试(一局流程侧):${idsRunOnly.size} 件(再加上与沙盒互补的 ${[...idsRun].filter((id) => idsSandbox.has(id)).length} 件战斗内遗物的流程侧钩子)`);
report.push(`  - 不可测:${idsUntestable.size} 件 [${[...idsUntestable].join(", ")}]`);
report.push(`  沙盒行通过 ${ok}/${sandboxRows}`);
report.push("");
report.push("每件遗物(沙盒 = 实测结论;run-test = 见对应 Rust 断言测试;untest = 不可测+理由):");
for (const v of verdicts) {
  const tag = v.status === "ok" ? "ok      " : v.status === "mismatch" ? "MISMATCH" : v.status === "run-test" ? "run-test" : "untest  ";
  report.push(`  [${tag}] ${v.row.id.padEnd(24)} hook=${v.row.hook}`);
  report.push(`             期望 ${v.row.expected}`);
  report.push(`             oracle ${v.row.oracle}${v.detail ? "  -> " + v.detail : ""}`);
}
if (mismatches.length) {
  report.push("");
  report.push("不一致明细:");
  for (const m of mismatches) report.push(`  ${m.row.id} [${m.row.kind}] ${m.detail}`);
}
report.push("");
report.push("已知 (a) 类不一致(全部已修,均配断言):");
report.push("  pocketwatch(combat.rs): 第 1 回合不该多抽 3(cards_last_turn 初值 0 被当成\"上回合没出牌\");");
report.push("    修法 start_turn 里加 turn > 1 判断;断言 = 沙盒 pocketwatch 行(第 1 回合手牌 5,第 2 回合 8)。");
report.push("  enchiridion(combat.rs): 随机能力牌只在战斗开始加,不该每回合加;修法 turn == 1 才加;");
report.push("    断言 = 沙盒 enchiridion 行(开局手牌 6,第 2 回合 5)。");
report.push("  蛋/陶瓷鱼(run.rs,主 agent 修): 卡牌奖励与商店买牌绕过加牌钩子,蛋升级与陶瓷鱼金币不生效;");
report.push("    新增 push_card_to_deck 统一钩子,断言 = molten_egg_and_ceramic_fish_apply_to_card_rewards /");
report.push("    _shop_purchases,以及本表的 frozen_egg_upgrades_powers(商店买能力牌)。");
report.push("  商店折扣(run.rs,主 agent 修): 信使 -20% 与会员卡 -50% 由相加改相乘(约 -60%,wiki 口径);");
report.push("    断言 = courier_and_membership_card_discounts_multiply。");
const text = report.join("\n") + "\n";
process.stdout.write(text);
if (OUT) writeFileSync(OUT, text);
rmSync(DIR, { recursive: true, force: true });
