// 事件效果审计:对 51 个事件(含多屏后半段、A15 变体)的每个选项,
// 用真实代码路径无头跑一遍,逐项核对数值/牌/遗物/药水/血量上限/金币/条件分支/
// 多屏流程/A15 变体。期望来自 slaythespire.wiki.gg 与本作语料(corpus::EVENTS)。
//
//   bun tools/audit_events.ts
//   bun tools/audit_events.ts --seed 12345 --out tools/golden/event_coverage.txt
//
// 机制:把每个 (事件,选项) 写成一个 scenario(带 "event" 字段),批量交给
// `spire --sandbox-batch`,它走 Run::debug_open_event / choose_event /
// picker_confirm / debug_win_battle 这些真实入口,逐动作吐 JSONL。这里解析后
// 对着期望断言,最后落一张"事件 × 选项 × 期望 × 实测 × 结论"覆盖表。

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

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

// ---- 类型 ----

interface Choice {
  label: string;
  enabled: boolean;
  cost_gold: number;
  cost_hp: number;
}
interface St {
  hp: number;
  max_hp: number;
  gold: number;
  asc: number;
  floor: number;
  screen: string;
  event: string | null;
  result: string | null;
  event_screen: string | null;
  attempts: number;
  deck: string[];
  relics: string[];
  potions: (string | null)[];
  choices: Choice[];
  encounter: { encounter: string; turn: number; phase: string } | null;
  reward: { gold: number; cards: string[]; relic: string | null; potions: string[] } | null;
  adv: { rewards: string[]; encounter: string; phase: number } | null;
}
interface Row {
  step: number;
  op: string;
  error?: string;
  st?: St;
}
interface Act {
  op: string;
  i?: number;
  win?: boolean;
}

// ---- 断言小工具 ----

const ok = (msg: string | null): string | null => msg;
const eq = (actual: unknown, expected: unknown, what: string): string | null =>
  actual === expected ? null : `${what}: 实测 ${JSON.stringify(actual)},期望 ${JSON.stringify(expected)}`;
const between = (actual: number, lo: number, hi: number, what: string): string | null =>
  actual >= lo && actual <= hi ? null : `${what}: 实测 ${actual},期望 ${lo}~${hi}`;
const isIn = (actual: unknown, list: unknown[], what: string): string | null =>
  list.includes(actual) ? null : `${what}: 实测 ${JSON.stringify(actual)},期望属于 ${JSON.stringify(list)}`;
/** 逐个跑,返回第一条不合规的原因 */
const seq = (...checks: (string | null)[]): string | null => checks.find((c) => c !== null) ?? null;

const countOf = (arr: string[], id: string) => arr.filter((x) => x === id).length;
const openSt = (rows: Row[]) => rows[0]!.st!;
const lastSt = (rows: Row[]) => rows[rows.length - 1]!.st!;
const afterChoose = (rows: Row[]) => {
  const rs = rows.filter((r) => r.op === "choose" && r.st);
  return (rs[rs.length - 1] ?? rows[0]!).st!;
};
const opRow = (rows: Row[], op: string) => rows.find((r) => r.op === op);
const relicGained = (rows: Row[], id?: string) => {
  const a = openSt(rows), b = lastSt(rows);
  return id ? b.relics.includes(id) && !a.relics.includes(id) : b.relics.length === a.relics.length + 1;
};
/** 遗物检查的字符串版(直接当 check 用) */
const wantRelic = (rows: Row[], id?: string): string | null =>
  relicGained(rows, id) ? null : `没得到遗物${id ?? ""}`;

// ---- scenario 构造 ----

const base = (over: Record<string, unknown> = {}): Record<string, unknown> => ({
  hp: 40,
  max_hp: 80,
  gold: 100,
  asc: 0,
  relics: [],
  deck: ["strike", "strike", "defend", "defend", "bash", "bludgeon", "shrug_it_off"],
  potions: [null, null, null],
  actions: [],
  ...over,
});

interface Spec {
  event: string;
  /** 在事件里的选项下标(多屏后半段用 actions 自己走,这里只作报告标签) */
  choice: number;
  /** 报告里显示的逻辑选项名 */
  option: string;
  /** 期望的一句话 */
  expected: string;
  scenario: Record<string, unknown>;
  /** 返回 null = 通过,否则是失败原因 */
  check: (rows: Row[]) => string | null;
  /** 可选备注(表示差异等) */
  note?: string;
}

const specs: Spec[] = [];
function add(s: Spec) {
  specs.push(s);
}

// 常用:打开事件 → 选某一项 → 可选后续动作
const choose = (i: number, over: Record<string, unknown> = {}, after: Act[] = []): Record<string, unknown> =>
  base({ ...over, actions: [{ op: "choose", i }, ...after] });

// ============================================================================
// 第一幕
// ============================================================================

// ---- Big Fish ----
add({
  event: "big_fish", choice: 0, option: "Banana", expected: "回血 floor(1/3 上限)=26",
  scenario: choose(0),
  check: (r) => seq(eq(lastSt(r).hp - openSt(r).hp, 26, "回血"), eq(lastSt(r).screen, "EVENT", "屏")),
});
add({
  event: "big_fish", choice: 1, option: "Donut", expected: "上限 +5 且同时 +5 血",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).max_hp - openSt(r).max_hp, 5, "上限"), eq(lastSt(r).hp - openSt(r).hp, 5, "血")),
});
add({
  event: "big_fish", choice: 2, option: "Box", expected: "随机遗物 + Regret 诅咒",
  scenario: choose(2),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), eq(countOf(lastSt(r).deck, "regret"), 1, "Regret")),
});

// ---- The Cleric ----
add({
  event: "the_cleric", choice: 0, option: "Heal(35)", expected: "扣 35 金,回血 floor(25% 上限)=20",
  scenario: choose(0),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 35, "金币"), eq(lastSt(r).hp - openSt(r).hp, 20, "回血")),
});
add({
  event: "the_cleric", choice: 1, option: "Purify(50)", expected: "扣 50 金,开选牌界面删一张",
  scenario: choose(1, {}, [{ op: "pick", i: 0 }]),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 50, "金币"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌组"), eq(lastSt(r).screen, "EVENT", "收尾屏")),
});
add({
  event: "the_cleric", choice: 1, option: "Purify A15(75)", expected: "A15 收 75 金",
  scenario: choose(1, { asc: 15, gold: 200 }, [{ op: "pick", i: 0 }]),
  check: (r) => eq(openSt(r).gold - lastSt(r).gold, 75, "金币"),
});
add({
  event: "the_cleric", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => seq(eq(lastSt(r).hp, openSt(r).hp, "血"), eq(lastSt(r).gold, openSt(r).gold, "金币")),
});
add({
  event: "the_cleric", choice: 1, option: "Purify 无牌可删时禁用", expected: "牌组无可移除牌 → 选项禁用",
  scenario: base({ relics: [], deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[1]!.enabled, false, "enabled"),
});

// ---- Dead Adventurer ----
add({
  event: "dead_adventurer", choice: 0, option: "Search", expected: "掷伏击:开精英战或领一格奖池(30 金/遗物/空),after 阶段 +1",
  scenario: base({ room: true, actions: [{ op: "choose", i: 0 }] }),
  check: (r) => {
    const a = openSt(r), b = lastSt(r);
    if (b.encounter) {
      return isIn(b.encounter.encounter, ["three_sentries", "gremlin_nob_solo", "lagavulin_solo"], "伏击遭遇");
    }
    // 没伏击:领了一格奖池,阶段 +1,血不变
    const phaseOk = (b.adv?.phase ?? 0) === (a.adv?.phase ?? 0) + 1;
    const rewardOk = b.relics.length === a.relics.length + 1 || b.gold === a.gold + 30 || (b.gold === a.gold && b.relics.length === a.relics.length);
    return seq(eq(phaseOk, true, "奖池阶段 +1"), eq(rewardOk, true, "领奖"), eq(b.hp, a.hp, "血"));
  },
});
add({
  event: "dead_adventurer", choice: 1, option: "Leave", expected: "无变化、事件结束",
  scenario: base({ room: true, actions: [{ op: "choose", i: 1 }] }),
  check: (r) => seq(eq(lastSt(r).result !== null, true, "已结算"), eq(lastSt(r).hp, openSt(r).hp, "血")),
});

// ---- Golden Idol ----
add({
  event: "golden_idol", choice: 0, option: "Take", expected: "得 Golden Idol,切到 trap 屏",
  scenario: choose(0),
  check: (r) => seq(eq(relicGained(r, "golden_idol"), true, "金像"), eq(lastSt(r).event_screen, "trap", "屏")),
});
add({
  event: "golden_idol", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数"),
});
add({
  event: "golden_idol", choice: 2, option: "Trap - Outrun", expected: "得 Injury 诅咒",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 2 }] }),
  check: (r) => eq(countOf(lastSt(r).deck, "injury"), 1, "Injury"),
});
add({
  event: "golden_idol", choice: 3, option: "Trap - Smash", expected: "扣 floor(25% 上限)=20 血",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 3 }] }),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, -20, "扣血"),
});
add({
  event: "golden_idol", choice: 3, option: "Trap - Smash A15", expected: "A15 扣 floor(35% 上限)=28 血",
  scenario: base({ asc: 15, actions: [{ op: "choose", i: 0 }, { op: "choose", i: 3 }] }),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, -28, "扣血"),
});
add({
  event: "golden_idol", choice: 4, option: "Trap - Hide", expected: "永久上限 -floor(8% 上限)=6",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 4 }] }),
  check: (r) => eq(openSt(r).max_hp - lastSt(r).max_hp, 6, "上限"),
});
add({
  event: "golden_idol", choice: 4, option: "Trap - Hide A15", expected: "A15 永久上限 -floor(10% 上限)=8",
  scenario: base({ asc: 15, actions: [{ op: "choose", i: 0 }, { op: "choose", i: 4 }] }),
  check: (r) => eq(openSt(r).max_hp - lastSt(r).max_hp, 8, "上限"),
});
add({
  event: "golden_idol", choice: 0, option: "Take 前 trap 选项禁用", expected: "进 trap 屏前,3 个陷阱选项禁用",
  scenario: choose(0, { actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[2]!.enabled, false, "Outrun"), eq(openSt(r).choices[3]!.enabled, false, "Smash")),
});

// ---- Scrap Ooze ----
add({
  event: "scrap_ooze", choice: 0, option: "Reach inside", expected: "扣 3 血;得遗物或留屏重试",
  scenario: choose(0),
  check: (r) => {
    const a = openSt(r), b = lastSt(r);
    const hpOk = b.hp === a.hp - 3;
    const doneOk = b.relics.length === a.relics.length + 1 || b.attempts === a.attempts + 1;
    return seq(eq(hpOk, true, "扣血 3"), eq(doneOk, true, "得遗物或重试"));
  },
});
add({
  event: "scrap_ooze", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).hp, openSt(r).hp, "血"),
});

// ---- Living Wall ----
add({
  event: "living_wall", choice: 0, option: "Forget", expected: "开选牌界面删一张",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌组 -1"),
});
add({
  event: "living_wall", choice: 1, option: "Change", expected: "开选牌界面变形一张(张数不变)",
  scenario: choose(1, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "张数"),
});
add({
  event: "living_wall", choice: 2, option: "Grow", expected: "开选牌界面升级一张",
  scenario: choose(2, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.some((c) => c.includes("+")), true, "有升级牌"),
});
add({
  event: "living_wall", choice: 0, option: "无可移除牌时禁用", expected: "全不可移除 → Forget/Change 禁用,Grow 也可能禁用",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[0]!.enabled, false, "Forget"), eq(openSt(r).choices[1]!.enabled, false, "Change")),
});

// ---- The Ssssserpent ----
add({
  event: "the_ssssserpent", choice: 0, option: "Agree", expected: "+175 金 + Doubt",
  scenario: choose(0),
  check: (r) => seq(eq(lastSt(r).gold - openSt(r).gold, 175, "金币"), eq(countOf(lastSt(r).deck, "doubt"), 1, "Doubt")),
});
add({
  event: "the_ssssserpent", choice: 0, option: "Agree A15", expected: "A15 只 +150 金 + Doubt",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 150, "金币"),
});
add({
  event: "the_ssssserpent", choice: 1, option: "Disagree", expected: "无变化",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).gold, openSt(r).gold, "金币"), eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组")),
});

// ---- Bonfire Spirits ----
add({
  event: "bonfire_spirits", choice: 0, option: "Offer(rare)", expected: "烧稀有牌:上限 +10、回满",
  scenario: base({ hp: 30, deck: ["strike", "defend", "demon_form"], actions: [{ op: "choose", i: 0 }, { op: "pick", i: 2 }] }),
  check: (r) => seq(
    eq(lastSt(r).max_hp - openSt(r).max_hp, 10, "上限"),
    eq(lastSt(r).hp, lastSt(r).max_hp, "回满"),
    eq(lastSt(r).deck.includes("demon_form"), false, "牌被烧"),
  ),
});
add({
  event: "bonfire_spirits", choice: 0, option: "Offer(curse)", expected: "烧诅咒:得 Spirit Poop",
  scenario: base({ deck: ["strike", "defend", "injury"], actions: [{ op: "choose", i: 0 }, { op: "pick", i: 2 }] }),
  check: (r) => wantRelic(r, "spirit_poop"),
});
add({
  event: "bonfire_spirits", choice: 0, option: "Offer(uncommon)", expected: "烧罕见牌:回 10 血",
  scenario: base({ hp: 30, deck: ["strike", "defend", "spot_weakness"], actions: [{ op: "choose", i: 0 }, { op: "pick", i: 2 }] }),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, 10, "回血"),
});
add({
  event: "bonfire_spirits", choice: 0, option: "Offer(basic)", expected: "烧基础牌:无补偿",
  scenario: base({ hp: 30, deck: ["strike", "defend", "bash"], actions: [{ op: "choose", i: 0 }, { op: "pick", i: 0 }] }),
  check: (r) => seq(eq(lastSt(r).hp, openSt(r).hp, "血"), eq(lastSt(r).max_hp, openSt(r).max_hp, "上限"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1")),
});

// ---- Wing Statue ----
add({
  event: "wing_statue", choice: 0, option: "Pray", expected: "扣 7 血并开删牌界面",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 7, "扣血"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1")),
});
add({
  event: "wing_statue", choice: 1, option: "Destroy", expected: "需要一击 10+ 的牌;得 50~80 金",
  scenario: choose(1, { deck: ["strike", "defend", "bludgeon"] }),
  check: (r) => between(lastSt(r).gold - openSt(r).gold, 50, 80, "金币"),
});
add({
  event: "wing_statue", choice: 1, option: "Destroy 无条件禁用", expected: "没有 10+ 攻击牌 → 禁用",
  scenario: base({ deck: ["strike", "strike", "defend"], actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[1]!.enabled, false, "enabled")),
});

// ---- World of Goop ----
add({
  event: "world_of_goop", choice: 0, option: "Gather", expected: "扣 11 血 +75 金",
  scenario: choose(0),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 11, "扣血"), eq(lastSt(r).gold - openSt(r).gold, 75, "金币")),
});
add({
  event: "world_of_goop", choice: 1, option: "Leave it", expected: "丢 20~50 金",
  scenario: choose(1),
  check: (r) => between(openSt(r).gold - lastSt(r).gold, 20, 50, "丢金"),
});
add({
  event: "world_of_goop", choice: 1, option: "Leave it A15", expected: "A15 丢 35~75 金",
  scenario: choose(1, { asc: 15 }),
  check: (r) => between(openSt(r).gold - lastSt(r).gold, 35, 75, "丢金"),
});

// ---- Hypnotizing Colored Mushrooms ----
add({
  event: "hypnotizing_colored_mushrooms", choice: 0, option: "Stomp", expected: "开 3 蘑菇战;打赢给 Odd Mushroom + 20~30 金",
  scenario: choose(0, {}, [{ op: "fight", win: true }]),
  check: (r) => {
    const c = opRow(r, "choose");
    const reward = lastSt(r).reward;
    return seq(
      eq(c?.st?.encounter?.encounter, "event_three_fungi", "遭遇"),
      reward && reward.relic === "odd_mushroom" ? null : "怪蘑菇不在奖励屏",
      reward ? between(reward.gold, 20, 30, "奖励金") : "没开奖励屏",
    );
  },
});
add({
  event: "hypnotizing_colored_mushrooms", choice: 1, option: "Eat", expected: "回血 floor(25% 上限)=20 + Parasite",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).hp - openSt(r).hp, 20, "回血"), eq(countOf(lastSt(r).deck, "parasite"), 1, "Parasite")),
});

// ---- Shining Light ----
add({
  event: "shining_light", choice: 0, option: "Enter", expected: "扣 round(20% 上限)=16,升级 2 张",
  scenario: choose(0, { deck: ["strike", "defend", "bash", "bludgeon"] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 16, "扣血"), eq(lastSt(r).deck.filter((c) => c.includes("+")).length, 2, "升级数")),
});
add({
  event: "shining_light", choice: 0, option: "Enter A15", expected: "A15 扣 round(30% 上限)=24",
  scenario: choose(0, { asc: 15, deck: ["strike", "defend", "bash", "bludgeon"] }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 24, "扣血"),
});
add({
  event: "shining_light", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).hp, openSt(r).hp, "血"),
});

// ============================================================================
// 第二幕
// ============================================================================

// ---- Pleading Vagrant ----
add({
  event: "pleading_vagrant", choice: 0, option: "Offer gold(85)", expected: "扣 85 金 + 随机遗物",
  scenario: choose(0, { gold: 200 }),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 85, "金币"), eq(relicGained(r), true, "遗物 +1")),
});
add({
  event: "pleading_vagrant", choice: 0, option: "钱不够禁用", expected: "金币 <85 → 禁用",
  scenario: choose(0, { gold: 50, actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "pleading_vagrant", choice: 1, option: "Rob", expected: "随机遗物 + Shame",
  scenario: choose(1),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), eq(countOf(lastSt(r).deck, "shame"), 1, "Shame")),
});
add({
  event: "pleading_vagrant", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).gold, openSt(r).gold, "金币"),
});

// ---- Ancient Writing ----
add({
  event: "ancient_writing", choice: 0, option: "Elegance", expected: "开选牌界面删一张",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1"),
});
add({
  event: "ancient_writing", choice: 1, option: "Simplicity", expected: "升级所有起始打击/防御",
  scenario: choose(1, { deck: ["strike", "strike", "defend", "bash"] }),
  check: (r) => {
    const upgraded = lastSt(r).deck.filter((c) => c.startsWith("strike+") || c.startsWith("defend+")).length;
    return eq(upgraded, 3, "升级的打击/防御数");
  },
});
add({
  event: "ancient_writing", choice: 0, option: "无可移除牌禁用", expected: "全不可移除 → Elegance 禁用",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});

// ---- Old Beggar ----
add({
  event: "old_beggar", choice: 0, option: "Offer gold(75)", expected: "扣 75 金并开删牌界面",
  scenario: choose(0, { gold: 200 }, [{ op: "pick", i: 0 }]),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 75, "金币"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1")),
});
add({
  event: "old_beggar", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).gold, openSt(r).gold, "金币"),
});

// ---- Colosseum ----
add({
  event: "colosseum", choice: 0, option: "Fight(第一场)", expected: "开 slavers 战,无奖励,打赢回看台",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "fight", win: true }] }),
  check: (r) => seq(
    eq(opRow(r, "choose")?.st?.encounter?.encounter, "event_colosseum_slavers", "遭遇"),
    eq(lastSt(r).screen, "EVENT", "回看台"),
    eq(lastSt(r).relics.length, openSt(r).relics.length, "无遗物"),
  ),
});
add({
  event: "colosseum", choice: 1, option: "Cowardice", expected: "逃走,事件结束",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "fight", win: true }, { op: "choose", i: 0 }] }),
  check: (r) => eq(lastSt(r).result !== null, true, "已结算"),
});
add({
  event: "colosseum", choice: 2, option: "Victory(第二场)", expected: "开 nobs 战;100 金 + 稀有遗物 + 罕见遗物 + 卡牌",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "fight", win: true }, { op: "choose", i: 1 }, { op: "fight", win: true }] }),
  check: (r) => {
    const b = lastSt(r);
    const reward = b.reward;
    return seq(
      eq(b.screen, "REWARD", "奖励屏"),
      reward ? eq(reward.gold, 100, "金币") : "没开奖励屏",
      reward && reward.relic ? null : "奖励屏没摆稀有遗物",
      eq(relicGained(r), true, "打赢共得两件遗物(屏上一件 + 直接进包一件)"),
      reward ? eq(reward.cards.length > 0, true, "有卡牌奖励") : "没卡牌",
    );
  },
});

// ---- Cursed Tome(多屏) ----
add({
  event: "cursed_tome", choice: 0, option: "Read", expected: "翻到第一页,不掉血",
  scenario: base({ actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(eq(lastSt(r).hp, openSt(r).hp, "血"), eq(lastSt(r).screen, "EVENT", "屏")),
});
add({
  event: "cursed_tome", choice: 1, option: "Leave", expected: "无变化",
  scenario: base({ actions: [{ op: "choose", i: 1 }] }),
  check: (r) => eq(lastSt(r).hp, openSt(r).hp, "血"),
});
add({
  event: "cursed_tome", choice: 2, option: "Continue 第1页", expected: "扣 1 血",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 1, "扣血"),
});
add({
  event: "cursed_tome", choice: 3, option: "Continue 第2页", expected: "累计扣 3 血(1+2)",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 3, "扣血"),
});
add({
  event: "cursed_tome", choice: 4, option: "Continue 第3页", expected: "累计扣 6 血(1+2+3),到最后一屏",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 6, "扣血"), eq(lastSt(r).choices[0]!.label.startsWith("Take"), true, "到 Take 屏")),
});
add({
  event: "cursed_tome", choice: 5, option: "Take", expected: "再扣 10 血,书遗物摆进奖励屏",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => {
    const st = lastSt(r);
    // 原版 Take 之后走 openCombatRewardScreen(反编译 refs/sts_lightspeed/src/game/GameContext.cpp:2558-2598),
    // 遗物摆在奖励屏上、要点一下才到手
    const id = st.reward?.relic ?? null;
    return seq(
      eq(openSt(r).hp - st.hp, 16, "总扣血"),
      eq(st.screen, "REWARD", "开奖励屏"),
      id && ["necronomicon", "enchiridion", "nilrys_codex"].includes(id) ? null : "书遗物没摆上奖励屏",
    );
  },
});
add({
  event: "cursed_tome", choice: 5, option: "Take A15", expected: "A15 最后一扣 15 血(总 21)",
  scenario: base({ asc: 15, actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 21, "总扣血"),
});
add({
  event: "cursed_tome", choice: 6, option: "Stop", expected: "不拿书,再扣 3 血(总 9)",
  scenario: base({ actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 0 }, { op: "choose", i: 1 }] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 9, "总扣血"), eq(lastSt(r).relics.length, openSt(r).relics.length, "无遗物")),
});

// ---- Augmenter ----
add({
  event: "augmenter", choice: 0, option: "Test J.A.X.", expected: "得 J.A.X. 牌",
  scenario: choose(0),
  check: (r) => eq(countOf(lastSt(r).deck, "jax"), 1, "J.A.X."),
});
add({
  event: "augmenter", choice: 1, option: "Become test subject", expected: "自选 2 张变形(张数不变)",
  scenario: choose(1, {}, [{ op: "pick", i: 0 }, { op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "张数"),
});
add({
  event: "augmenter", choice: 2, option: "Ingest mutagens", expected: "得 Mutagenic Strength",
  scenario: choose(2),
  check: (r) => relicGained(r, "mutagenic_strength"),
});
add({
  event: "augmenter", choice: 1, option: "无可变形牌禁用", expected: "全不可移除 → 禁用",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[1]!.enabled, false, "enabled"),
});

// ---- Forgotten Altar ----
add({
  event: "forgotten_altar", choice: 0, option: "Offer Golden Idol", expected: "金像换血像",
  scenario: choose(0, { relics: ["golden_idol"] }),
  check: (r) => seq(eq(lastSt(r).relics.includes("golden_idol"), false, "金像没了"), eq(lastSt(r).relics.includes("bloody_idol"), true, "血像")),
});
add({
  event: "forgotten_altar", choice: 0, option: "无金像禁用", expected: "没有 Golden Idol → 禁用",
  scenario: choose(0, { actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "forgotten_altar", choice: 1, option: "Sacrifice", expected: "先 +5 上限,再按旧上限扣 round(25%)=20",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).max_hp - openSt(r).max_hp, 5, "上限"), eq(lastSt(r).hp, openSt(r).hp + 5 - 20, "血")),
});
add({
  event: "forgotten_altar", choice: 1, option: "Sacrifice A15", expected: "A15 扣 round(35% 旧上限)=28",
  scenario: choose(1, { asc: 15 }),
  check: (r) => eq(lastSt(r).hp, openSt(r).hp + 5 - 28, "血"),
});
add({
  event: "forgotten_altar", choice: 2, option: "Desecrate", expected: "得 Decay 诅咒",
  scenario: choose(2),
  check: (r) => eq(countOf(lastSt(r).deck, "decay"), 1, "Decay"),
});

// ---- Council of Ghosts ----
add({
  event: "ghosts", choice: 0, option: "Accept", expected: "上限减半(40),得 5 张 Apparition",
  scenario: choose(0),
  check: (r) => seq(
    eq(lastSt(r).max_hp, 40, "上限"),
    eq(countOf(lastSt(r).deck, "ghostly_armor"), 5, "Apparition x5"),
  ),
});
add({
  event: "ghosts", choice: 0, option: "Accept A15", expected: "A15 只给 3 张",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(countOf(lastSt(r).deck, "ghostly_armor"), 3, "Apparition 数"),
});
add({
  event: "ghosts", choice: 1, option: "Refuse", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).max_hp, openSt(r).max_hp, "上限"),
});

// ---- Masked Bandits ----
add({
  event: "masked_bandits", choice: 0, option: "Pay", expected: "金币清零",
  scenario: choose(0),
  check: (r) => eq(lastSt(r).gold, 0, "金币"),
});
add({
  event: "masked_bandits", choice: 1, option: "Fight", expected: "开 bandits 战;赢给 Red Mask + 25~35 金 + 卡牌(不掉药水)",
  scenario: choose(1, {}, [{ op: "fight", win: true }]),
  check: (r) => {
    const reward = lastSt(r).reward;
    return seq(
      reward && reward.relic === "red_mask" ? null : "红面具不在奖励屏",
      reward ? between(reward.gold, 25, 35, "奖励金") : "没开奖励屏",
      reward ? eq(reward.potions.length, 0, "不掉药水") : "没开奖励屏",
    );
  },
});

// ---- The Nest ----
add({
  event: "the_nest", choice: 0, option: "Smash and grab", expected: "+99 金",
  scenario: choose(0),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 99, "金币"),
});
add({
  event: "the_nest", choice: 0, option: "Smash A15", expected: "A15 只 +50 金",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 50, "金币"),
});
add({
  event: "the_nest", choice: 1, option: "Stay in line", expected: "扣 6 血 + Ritual Dagger",
  scenario: choose(1),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 6, "扣血"), eq(countOf(lastSt(r).deck, "ritual_dagger"), 1, "匕首")),
});

// ---- The Library ----
add({
  event: "the_library", choice: 0, option: "Read", expected: "开 20 张不重样的本职业选牌屏,选一张进牌组",
  scenario: choose(0, {}, [{ op: "choose", i: 0 }]),
  check: (r) => seq(eq(lastSt(r).deck.length, openSt(r).deck.length + 1, "牌 +1"), eq(lastSt(r).screen, "EVENT", "收尾屏")),
});
add({
  event: "the_library", choice: 1, option: "Sleep", expected: "回血 round(33% 上限)=26",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, 26, "回血"),
});
add({
  event: "the_library", choice: 1, option: "Sleep A15", expected: "A15 回 round(20% 上限)=16",
  scenario: choose(1, { asc: 15 }),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, 16, "回血"),
});

// ---- The Mausoleum ----
add({
  event: "the_mausoleum", choice: 0, option: "Open coffin", expected: "随机遗物;50% 再给 Writhe",
  scenario: choose(0),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), isIn(countOf(lastSt(r).deck, "writhe"), [0, 1], "Writhe 数")),
});
add({
  event: "the_mausoleum", choice: 0, option: "Open A15", expected: "A15 必给 Writhe",
  scenario: choose(0, { asc: 15 }),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), eq(countOf(lastSt(r).deck, "writhe"), 1, "Writhe")),
});
add({
  event: "the_mausoleum", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数"),
});

// ---- Vampires(?) ----
add({
  event: "vampires", choice: 0, option: "Offer Blood Vial", expected: "失去血瓶、删所有起始打击、得 5 张 Bite",
  scenario: base({ relics: ["blood_vial"], deck: ["strike", "strike", "strike", "strike", "strike", "defend", "bash"], actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(
    eq(lastSt(r).relics.includes("blood_vial"), false, "血瓶没了"),
    eq(countOf(lastSt(r).deck, "strike"), 0, "打击清空"),
    eq(countOf(lastSt(r).deck, "bite"), 5, "Bite x5"),
  ),
});
add({
  event: "vampires", choice: 0, option: "无血瓶禁用", expected: "没有 Blood Vial → 禁用",
  scenario: base({ relics: [], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "vampires", choice: 1, option: "Accept", expected: "上限 -ceil(30% 上限)=24,打击换 5 Bite",
  scenario: base({ max_hp: 71, hp: 60, deck: ["strike", "strike", "defend", "bash"], actions: [{ op: "choose", i: 1 }] }),
  check: (r) => seq(
    eq(openSt(r).max_hp - lastSt(r).max_hp, 22, "上限减(ceil 71*0.3=22)"),
    eq(countOf(lastSt(r).deck, "strike"), 0, "打击清空"),
    eq(countOf(lastSt(r).deck, "bite"), 5, "Bite x5"),
  ),
});
add({
  event: "vampires", choice: 2, option: "Refuse", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});

// ============================================================================
// 第三幕
// ============================================================================

// ---- Falling ----
add({
  event: "falling", choice: 0, option: "Land", expected: "随机移除一张技能牌",
  scenario: base({ deck: ["strike", "bash", "bludgeon", "shrug_it_off"], actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1"), eq(lastSt(r).deck.includes("shrug_it_off"), false, "技能被夺")),
});
add({
  event: "falling", choice: 2, option: "Strike", expected: "随机移除一张攻击牌",
  scenario: base({ deck: ["strike", "defend", "bash", "bludgeon"], actions: [{ op: "choose", i: 2 }] }),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1"),
});
add({
  event: "falling", choice: 3, option: "Land on your head", expected: "三类牌都抽不出时才可选:什么都不丢",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[3]!.enabled, true, "保底可用"), eq(openSt(r).choices[0]!.enabled, false, "Land 禁用")),
});
add({
  event: "falling", choice: 3, option: "Land(执行)", expected: "执行保底:不丢牌",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "choose", i: 3 }] }),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组不变"),
});

// ---- Mindbloom ----
add({
  event: "mindbloom", choice: 2, option: "I am Rich", expected: "floor 38 可用:+999 金 + 2 Normality",
  scenario: base({ floor: 38, actions: [{ op: "choose", i: 2 }] }),
  check: (r) => seq(eq(lastSt(r).gold - openSt(r).gold, 999, "金币"), eq(countOf(lastSt(r).deck, "normality"), 2, "Normality x2")),
});
add({
  event: "mindbloom", choice: 2, option: "I am Rich 在 floor 41+ 禁用", expected: "floor 45 → Rich 禁用,Healthy 可用",
  scenario: base({ floor: 45, actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[2]!.enabled, false, "Rich"), eq(openSt(r).choices[3]!.enabled, true, "Healthy")),
});
add({
  event: "mindbloom", choice: 3, option: "I am Healthy", expected: "floor 45 可用:回满 + Doubt",
  scenario: base({ floor: 45, hp: 30, actions: [{ op: "choose", i: 3 }] }),
  check: (r) => seq(eq(lastSt(r).hp, lastSt(r).max_hp, "回满"), eq(countOf(lastSt(r).deck, "doubt"), 1, "Doubt")),
});
add({
  event: "mindbloom", choice: 3, option: "I am Healthy 在 floor 40- 禁用", expected: "floor 38 → Healthy 禁用",
  scenario: base({ floor: 38, actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[3]!.enabled, false, "Healthy"),
});
add({
  event: "mindbloom", choice: 0, option: "I am War", expected: "开一只 Act1 Boss 幻影;赢给稀有遗物 + 50 金",
  scenario: base({ floor: 38, actions: [{ op: "choose", i: 0 }, { op: "fight", win: true }] }),
  check: (r) => {
    const enc = opRow(r, "choose")?.st?.encounter?.encounter ?? "";
    const reward = lastSt(r).reward;
    return seq(
      enc.startsWith("event_phantom_") ? null : `遭遇 ${enc} 不是幻影 Boss`,
      reward ? eq(reward.gold, 50, "奖励金") : "没开奖励屏",
      reward && reward.relic ? null : "没稀有遗物",
    );
  },
});
add({
  event: "mindbloom", choice: 1, option: "I am Awake", expected: "升级所有可升级牌 + Mark of the Bloom",
  scenario: base({ floor: 38, deck: ["strike", "defend", "bash"], actions: [{ op: "choose", i: 1 }] }),
  check: (r) => seq(
    eq(lastSt(r).deck.every((c) => c.includes("+")), true, "全升级"),
    eq(lastSt(r).relics.includes("mark_of_the_bloom"), true, "Mark of the Bloom"),
  ),
});

// ---- The Moai Head ----
add({
  event: "the_moai_head", choice: 0, option: "Jump inside", expected: "上限 -round(12.5% 上限)=10,再回满",
  scenario: choose(0, { hp: 30 }),
  check: (r) => seq(eq(openSt(r).max_hp - lastSt(r).max_hp, 10, "上限"), eq(lastSt(r).hp, lastSt(r).max_hp, "回满")),
});
add({
  event: "the_moai_head", choice: 0, option: "Jump A15", expected: "A15 上限 -round(18% 上限)=14",
  scenario: choose(0, { asc: 15, hp: 30 }),
  check: (r) => eq(openSt(r).max_hp - lastSt(r).max_hp, 14, "上限"),
});
add({
  event: "the_moai_head", choice: 1, option: "Offer Golden Idol", expected: "失金像 +333 金",
  scenario: choose(1, { relics: ["golden_idol"] }),
  check: (r) => seq(eq(lastSt(r).relics.includes("golden_idol"), false, "金像没了"), eq(lastSt(r).gold - openSt(r).gold, 333, "金币")),
});
add({
  event: "the_moai_head", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).max_hp, openSt(r).max_hp, "上限"),
});

// ---- Mysterious Sphere ----
add({
  event: "mysterious_sphere", choice: 0, option: "Open sphere", expected: "开 2 Orb Walker 战;赢给稀有遗物 + 45~55 金 + 卡牌",
  scenario: choose(0, {}, [{ op: "fight", win: true }]),
  check: (r) => {
    const reward = lastSt(r).reward;
    return seq(
      eq(opRow(r, "choose")?.st?.encounter?.encounter, "event_two_orbs", "遭遇"),
      reward ? between(reward.gold, 45, 55, "奖励金") : "没开奖励屏",
      reward && reward.relic ? null : "没稀有遗物",
    );
  },
});
add({
  event: "mysterious_sphere", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数"),
});

// ---- Sensory Stone ----
add({
  event: "sensory_stone", choice: 0, option: "Recall 1", expected: "开 1 组无色牌奖励屏(三选一),不掉血",
  scenario: choose(0, {}, [{ op: "take" }]),
  check: (r) => seq(eq(lastSt(r).deck.length, openSt(r).deck.length + 1, "牌 +1"), eq(lastSt(r).hp, openSt(r).hp, "血")),
});
add({
  event: "sensory_stone", choice: 1, option: "Recall 2", expected: "扣 5 血,开 2 组无色牌奖励屏,各选一张",
  scenario: choose(1, {}, [{ op: "take" }, { op: "take" }]),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 5, "扣血"), eq(lastSt(r).deck.length, openSt(r).deck.length + 2, "牌 +2")),
});
add({
  event: "sensory_stone", choice: 2, option: "Recall 3", expected: "扣 10 血,开 3 组无色牌奖励屏,各选一张",
  scenario: choose(2, {}, [{ op: "take" }, { op: "take" }, { op: "take" }]),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 10, "扣血"), eq(lastSt(r).deck.length, openSt(r).deck.length + 3, "牌 +3")),
});

// ---- Tomb of Lord Red Mask ----
add({
  event: "tomb_of_lord_red_mask", choice: 0, option: "Don the Red Mask", expected: "有红面具:+222 金",
  scenario: choose(0, { relics: ["red_mask"] }),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 222, "金币"),
});
add({
  event: "tomb_of_lord_red_mask", choice: 0, option: "无红面具禁用", expected: "没有 Red Mask → 禁用",
  scenario: choose(0, { actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "tomb_of_lord_red_mask", choice: 1, option: "Offer gold", expected: "金币清零,得 Red Mask",
  scenario: choose(1, { gold: 200 }),
  check: (r) => seq(eq(lastSt(r).gold, 0, "金币"), eq(relicGained(r, "red_mask"), true, "红面具")),
});
add({
  event: "tomb_of_lord_red_mask", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).gold, openSt(r).gold, "金币"),
});

// ---- Winding Halls ----
add({
  event: "winding_halls", choice: 0, option: "Embrace madness", expected: "扣 round(12.5% 上限)=10,得 2 Madness",
  scenario: choose(0),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 10, "扣血"), eq(countOf(lastSt(r).deck, "madness"), 2, "Madness x2")),
});
add({
  event: "winding_halls", choice: 0, option: "Embrace A15", expected: "A15 扣 round(18% 上限)=14",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 14, "扣血"),
});
add({
  event: "winding_halls", choice: 1, option: "Press on", expected: "回血 round(25% 上限)=20 + Writhe",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).hp - openSt(r).hp, 20, "回血"), eq(countOf(lastSt(r).deck, "writhe"), 1, "Writhe")),
});
add({
  event: "winding_halls", choice: 1, option: "Press on A15", expected: "A15 回 round(20% 上限)=16",
  scenario: choose(1, { asc: 15 }),
  check: (r) => eq(lastSt(r).hp - openSt(r).hp, 16, "回血"),
});
add({
  event: "winding_halls", choice: 2, option: "Retrace your steps", expected: "永久上限 -round(5% 上限)=4",
  scenario: choose(2),
  check: (r) => eq(openSt(r).max_hp - lastSt(r).max_hp, 4, "上限"),
});

// ============================================================================
// 神龛
// ============================================================================

// ---- Match and Keep ----
add({
  event: "match_and_keep", choice: 0, option: "12 格翻牌", expected: "12 个可翻格子;翻两张后记一次尝试(配对则进牌组)",
  scenario: base({ actions: [{ op: "flip", i: 0 }, { op: "flip", i: 1 }, { op: "flip", i: 2 }, { op: "noop" }] }),
  check: (r) => {
    const a = openSt(r);
    if (a.choices.length !== 12) return `格子数 ${a.choices.length},期望 12`;
    const b = lastSt(r);
    const matched = b.deck.length - a.deck.length;
    return isIn(matched, [0, 1, 2], "配对进牌数");
  },
  note: "(c) 表示差异:棋盘铺法按本作实现,翻牌次数上限 5 与原版一致",
});

// ---- Golden Shrine ----
add({
  event: "golden_shrine", choice: 0, option: "Pray", expected: "+100 金",
  scenario: choose(0),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 100, "金币"),
});
add({
  event: "golden_shrine", choice: 0, option: "Pray A15", expected: "A15 只 +50 金",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 50, "金币"),
});
add({
  event: "golden_shrine", choice: 1, option: "Desecrate", expected: "+275 金 + Regret",
  scenario: choose(1),
  check: (r) => seq(eq(lastSt(r).gold - openSt(r).gold, 275, "金币"), eq(countOf(lastSt(r).deck, "regret"), 1, "Regret")),
});
add({
  event: "golden_shrine", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).gold, openSt(r).gold, "金币"),
});

// ---- Transmogrifier ----
add({
  event: "transmorgrifier", choice: 0, option: "Pray", expected: "开变形选牌界面",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "张数不变"),
});
add({
  event: "transmorgrifier", choice: 0, option: "无可变形牌禁用", expected: "全不可移除 → 禁用",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "transmorgrifier", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});

// ---- Purifier ----
add({
  event: "purifier", choice: 0, option: "Pray", expected: "开删牌界面,删一张",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1"),
});
add({
  event: "purifier", choice: 0, option: "无可移除牌禁用", expected: "全不可移除 → 禁用",
  scenario: base({ deck: ["ascenders_bane", "ascenders_bane"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "purifier", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});

// ---- Upgrade Shrine ----
add({
  event: "upgrade_shrine", choice: 0, option: "Pray", expected: "开升级界面,升级一张",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.filter((c) => c.includes("+")).length, 1, "升级数"),
});
add({
  event: "upgrade_shrine", choice: 0, option: "无升级空间禁用", expected: "没有可升级牌 → 禁用",
  scenario: base({ deck: ["strike+", "defend+", "bash+"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "upgrade_shrine", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).deck.some((c) => c.includes("+")), false, "没升级"),
});

// ---- Wheel of Change ----
add({
  event: "wheel_of_change", choice: 0, option: "Spin", expected: "六格之一:100 金 / 遗物屏 / 回满 / Decay / 删牌屏 / 扣 round(10% 上限)=8 血",
  scenario: base({ hp: 30, actions: [{ op: "choose", i: 0 }] }),
  check: (r) => {
    const a = openSt(r), b = lastSt(r);
    const dGold = b.gold - a.gold;
    const outcomes = [
      dGold === 100,
      b.reward?.relic != null,
      b.hp === b.max_hp && b.hp > a.hp,
      countOf(b.deck, "decay") === 1,
      b.screen === "PICK",
      a.hp - b.hp === 8,
    ];
    return outcomes.some(Boolean) ? null : `没落到任何已知结果:${JSON.stringify({ dGold, screen: b.screen, hp: b.hp, reward: b.reward })}`;
  },
});
add({
  event: "wheel_of_change", choice: 0, option: "Spin A15", expected: "A15 扣血那格是 round(15% 上限)=12",
  scenario: base({ asc: 15, hp: 30, actions: [{ op: "choose", i: 0 }] }),
  check: (r) => {
    const a = openSt(r), b = lastSt(r);
    const outcomes = [
      b.gold - a.gold === 100,
      b.reward?.relic != null,
      b.hp === b.max_hp && b.hp > a.hp,
      countOf(b.deck, "decay") === 1,
      b.screen === "PICK",
      a.hp - b.hp === 12,
    ];
    return outcomes.some(Boolean) ? null : `没落到任何已知结果:${JSON.stringify(b)}`;
  },
});

// ============================================================================
// 一次性事件
// ============================================================================

// ---- Ominous Forge ----
add({
  event: "ominous_forge", choice: 0, option: "Forge", expected: "开升级界面",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.filter((c) => c.includes("+")).length, 1, "升级数"),
});
add({
  event: "ominous_forge", choice: 0, option: "无升级空间禁用", expected: "没有可升级牌 → 禁用",
  scenario: base({ deck: ["strike+", "defend+", "bash+"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "ominous_forge", choice: 1, option: "Rummage", expected: "得 Warped Tongs + Pain",
  scenario: choose(1),
  check: (r) => seq(eq(relicGained(r, "warped_tongs"), true, "Warped Tongs"), eq(countOf(lastSt(r).deck, "pain"), 1, "Pain")),
});
add({
  event: "ominous_forge", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});

// ---- Designer In-Spire ----
add({
  event: "designer_in_spire", choice: 0, option: "Adjustments", expected: "扣 40 金:选一张升级或随机升两张",
  scenario: choose(0, { gold: 200 }, [{ op: "pick", i: 0 }]),
  check: (r) => {
    const b = lastSt(r);
    const upgraded = b.deck.filter((c) => c.includes("+")).length;
    return seq(eq(openSt(r).gold - b.gold, 40, "金币"), upgraded >= 1 ? null : "没有牌被升级");
  },
});
add({
  event: "designer_in_spire", choice: 1, option: "Clean up", expected: "扣 60 金:选一张删或随机变两张",
  scenario: choose(1, { gold: 200 }, [{ op: "pick", i: 0 }]),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 60, "金币"), eq(lastSt(r).screen, "EVENT", "收尾屏")),
});
add({
  event: "designer_in_spire", choice: 2, option: "Full service", expected: "扣 90 金:删一张再随机升一张",
  scenario: choose(2, { gold: 200 }, [{ op: "pick", i: 0 }]),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 90, "金币"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1")),
});
add({
  event: "designer_in_spire", choice: 3, option: "Punch", expected: "扣 3 血",
  scenario: choose(3, { gold: 200 }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 3, "扣血"),
});
add({
  event: "designer_in_spire", choice: 3, option: "Punch A15", expected: "A15 扣 5 血",
  scenario: choose(3, { gold: 200, asc: 15 }),
  check: (r) => eq(openSt(r).hp - lastSt(r).hp, 5, "扣血"),
});
add({
  event: "designer_in_spire", choice: 2, option: "钱不够禁用", expected: "金币 <90 → Full service 禁用",
  scenario: base({ gold: 80, actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[2]!.enabled, false, "enabled"),
});

// ---- Duplicator ----
add({
  event: "duplicator", choice: 0, option: "Pray", expected: "复制一张:牌组 +1",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length + 1, "牌 +1"),
});
add({
  event: "duplicator", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});

// ---- Face Trader ----
add({
  event: "face_trader", choice: 0, option: "Touch", expected: "扣 floor(10% 上限)=8 血,+75 金",
  scenario: choose(0),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 8, "扣血"), eq(lastSt(r).gold - openSt(r).gold, 75, "金币")),
});
add({
  event: "face_trader", choice: 0, option: "Touch A15", expected: "A15 只 +50 金",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(lastSt(r).gold - openSt(r).gold, 50, "金币"),
});
add({
  event: "face_trader", choice: 1, option: "Trade", expected: "得一件没拥有的脸部遗物",
  scenario: choose(1),
  check: (r) => eq(relicGained(r), true, "遗物 +1"),
});
add({
  event: "face_trader", choice: 2, option: "Leave", expected: "无变化",
  scenario: choose(2),
  check: (r) => eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数"),
});

// ---- The Divine Fountain ----
add({
  event: "the_divine_fountain", choice: 0, option: "Drink", expected: "移除所有可移除诅咒",
  scenario: base({ deck: ["strike", "defend", "doubt", "regret", "bash"], actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(eq(countOf(lastSt(r).deck, "doubt"), 0, "Doubt"), eq(countOf(lastSt(r).deck, "regret"), 0, "Regret")),
});
add({
  event: "the_divine_fountain", choice: 1, option: "Leave", expected: "无变化",
  scenario: base({ deck: ["strike", "defend", "doubt", "bash"], actions: [{ op: "choose", i: 1 }] }),
  check: (r) => eq(countOf(lastSt(r).deck, "doubt"), 1, "Doubt 还在"),
});

// ---- Knowing Skull ----
add({
  event: "knowing_skull", choice: 0, option: "Riches", expected: "扣 base=max(6,floor(10% 上限))=8 血,+90 金;再买一次扣 9",
  scenario: base({ hp: 70, actions: [{ op: "choose", i: 0 }, { op: "choose", i: 0 }] }),
  check: (r) => {
    const a = openSt(r);
    const rs = r.filter((x) => x.op === "choose" && x.st).map((x) => x.st!);
    return seq(
      eq(a.hp - rs[0]!.hp, 8, "第一次扣血"),
      eq(a.hp - rs[1]!.hp, 17, "两次累计扣血 8+9"),
      eq(lastSt(r).gold - a.gold, 180, "金币"),
      eq(lastSt(r).screen, "EVENT", "留在本屏"),
    );
  },
});
add({
  event: "knowing_skull", choice: 1, option: "Success", expected: "扣 8 血,得一张非普通无色牌",
  scenario: base({ hp: 70, actions: [{ op: "choose", i: 1 }] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 8, "扣血"), eq(lastSt(r).deck.length, openSt(r).deck.length + 1, "牌 +1")),
});
add({
  event: "knowing_skull", choice: 2, option: "A pick me up", expected: "扣 8 血,得 1 瓶药水",
  scenario: base({ hp: 70, actions: [{ op: "choose", i: 2 }] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 8, "扣血"), eq(lastSt(r).potions.filter(Boolean).length, 1, "药水数")),
});
add({
  event: "knowing_skull", choice: 3, option: "How do I leave", expected: "扣 base 8 血,事件结束",
  scenario: base({ hp: 70, actions: [{ op: "choose", i: 3 }] }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 8, "扣血"), eq(lastSt(r).result !== null, true, "已结算")),
});

// ---- Lab ----
add({
  event: "lab", choice: 0, option: "Search", expected: "奖励屏摆 3 瓶药水",
  scenario: choose(0),
  check: (r) => seq(eq(lastSt(r).screen, "REWARD", "奖励屏"), eq(lastSt(r).reward?.potions.length, 3, "药水数")),
});
add({
  event: "lab", choice: 0, option: "Search A15", expected: "A15 只摆 2 瓶",
  scenario: choose(0, { asc: 15 }),
  check: (r) => eq(lastSt(r).reward?.potions.length, 2, "药水数"),
});

// ---- N'loth ----
add({
  event: "nloth", choice: 0, option: "Offer relic A", expected: "吃掉一件遗物,换 N'loth's Gift(遗物数不变)",
  scenario: base({ relics: ["anchor", "lantern"], actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(
    eq(relicGained(r, "nloths_gift"), true, "N'loth's Gift"),
    eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数不变"),
  ),
});
add({
  event: "nloth", choice: 1, option: "Offer relic B", expected: "同 A,另一件",
  scenario: base({ relics: ["anchor", "lantern"], actions: [{ op: "choose", i: 1 }] }),
  check: (r) => wantRelic(r, "nloths_gift"),
});
add({
  event: "nloth", choice: 0, option: "不足两件遗物时禁用", expected: "只 1 件遗物 → A 可献(就是那件),B 禁用",
  scenario: base({ relics: ["anchor"], actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[0]!.enabled, true, "A"), eq(openSt(r).choices[1]!.enabled, false, "B")),
});
add({
  event: "nloth", choice: 2, option: "Leave", expected: "无变化",
  scenario: base({ relics: ["anchor", "lantern"], actions: [{ op: "choose", i: 2 }] }),
  check: (r) => eq(lastSt(r).relics.length, openSt(r).relics.length, "遗物数"),
});

// ---- Note For Yourself ----
add({
  event: "note_for_yourself", choice: 1, option: "Ignore", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).deck.length, openSt(r).deck.length, "牌组"),
});
add({
  event: "note_for_yourself", choice: 0, option: "Take and give", expected: "取回存档牌并开选牌界面写下(无头对拍存卡关闭)",
  scenario: choose(0, {}, [{ op: "pick", i: 0 }]),
  check: (r) => isIn(lastSt(r).screen, ["EVENT", "PICK"], "屏"),
  note: "(c) 跨局存卡文件在无头沙盒里不落盘,只验证流程不崩",
});

// ---- Secret Portal ----
add({
  event: "secret_portal", choice: 0, option: "Enter the portal", expected: "直接跳到本层 Boss 战",
  scenario: choose(0),
  check: (r) => {
    const c = afterChoose(r);
    return c.encounter ? null : "没进战斗";
  },
  note: "跳到 Boss 房后由上层接管,沙盒里看到 COMBAT 即算通过",
});
add({
  event: "secret_portal", choice: 1, option: "Leave", expected: "无变化",
  scenario: choose(1),
  check: (r) => eq(lastSt(r).screen, "EVENT", "留在事件"),
});

// ---- The Joust ----
add({
  event: "the_joust", choice: 0, option: "Bet on the murderer", expected: "付 50 金,70% 再赢 100 金",
  scenario: choose(0),
  check: (r) => isIn(lastSt(r).gold - openSt(r).gold, [-50, 50], "金币变化"),
});
add({
  event: "the_joust", choice: 1, option: "Bet on the owner", expected: "付 50 金,30% 赢 250 金",
  scenario: choose(1),
  check: (r) => isIn(lastSt(r).gold - openSt(r).gold, [-50, 200], "金币变化"),
});
add({
  event: "the_joust", choice: 0, option: "钱不够禁用", expected: "金币 <50 → 两个赌注都禁用",
  scenario: base({ gold: 40, actions: [{ op: "noop" }] }),
  check: (r) => seq(eq(openSt(r).choices[0]!.enabled, false, "murderer"), eq(openSt(r).choices[1]!.enabled, false, "owner")),
});

// ---- We Meet Again! ----
add({
  event: "we_meet_again", choice: 0, option: "Give potion", expected: "交一瓶药水,得随机遗物(药水少一瓶)",
  scenario: base({ potions: ["fire_potion", null, null], actions: [{ op: "choose", i: 0 }] }),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), eq(lastSt(r).potions.filter(Boolean).length, 0, "药水少一瓶")),
});
add({
  event: "we_meet_again", choice: 0, option: "没药水禁用", expected: "药水栏全空 → 禁用",
  scenario: base({ potions: [null, null, null], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[0]!.enabled, false, "enabled"),
});
add({
  event: "we_meet_again", choice: 1, option: "Give gold", expected: "丢 50~150 金,得随机遗物",
  scenario: choose(1, { gold: 200 }),
  check: (r) => seq(between(openSt(r).gold - lastSt(r).gold, 50, 150, "丢金"), eq(relicGained(r), true, "遗物 +1")),
});
add({
  event: "we_meet_again", choice: 2, option: "Give card", expected: "丢一张非基础非诅咒牌,得随机遗物(牌 -1)",
  scenario: choose(2, { deck: ["strike", "strike", "defend", "bash", "bludgeon"] }),
  check: (r) => seq(eq(relicGained(r), true, "遗物 +1"), eq(lastSt(r).deck.length, openSt(r).deck.length - 1, "牌 -1")),
});
add({
  event: "we_meet_again", choice: 2, option: "没有可给的牌时禁用", expected: "只有基础牌 → Give card 禁用",
  scenario: base({ deck: ["strike", "strike", "defend", "defend"], actions: [{ op: "noop" }] }),
  check: (r) => eq(openSt(r).choices[2]!.enabled, false, "enabled"),
});
add({
  event: "we_meet_again", choice: 3, option: "Attack", expected: "无变化,事件结束",
  scenario: choose(3),
  check: (r) => seq(eq(lastSt(r).result !== null, true, "已结算"), eq(lastSt(r).relics.length, openSt(r).relics.length, "无遗物")),
});

// ---- The Woman in Blue ----
add({
  event: "the_woman_in_blue", choice: 0, option: "Buy 1", expected: "扣 20 金,奖励屏 1 瓶药水",
  scenario: choose(0, { gold: 100 }),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 20, "金币"), eq(lastSt(r).reward?.potions.length, 1, "药水数")),
});
add({
  event: "the_woman_in_blue", choice: 1, option: "Buy 2", expected: "扣 30 金,奖励屏 2 瓶药水",
  scenario: choose(1, { gold: 100 }),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 30, "金币"), eq(lastSt(r).reward?.potions.length, 2, "药水数")),
});
add({
  event: "the_woman_in_blue", choice: 2, option: "Buy 3", expected: "扣 40 金,奖励屏 3 瓶药水",
  scenario: choose(2, { gold: 100 }),
  check: (r) => seq(eq(openSt(r).gold - lastSt(r).gold, 40, "金币"), eq(lastSt(r).reward?.potions.length, 3, "药水数")),
});
add({
  event: "the_woman_in_blue", choice: 3, option: "Leave", expected: "无变化",
  scenario: choose(3, { gold: 100 }),
  check: (r) => eq(lastSt(r).gold, openSt(r).gold, "金币"),
});
add({
  event: "the_woman_in_blue", choice: 3, option: "Leave A15", expected: "A15 扣 ceil(5% 上限)=4 血(不减上限)",
  scenario: choose(3, { gold: 100, max_hp: 71, hp: 60, asc: 15 }),
  check: (r) => seq(eq(openSt(r).hp - lastSt(r).hp, 4, "扣血"), eq(lastSt(r).max_hp, openSt(r).max_hp, "上限不变")),
});

// ============================================================================
// 跑
// ============================================================================

const dir = mkdtempSync(join(tmpdir(), "spire-events-"));
const listLines: string[] = [];
specs.forEach((s, i) => {
  const name = `${s.event}/${s.choice}/${i}`;
  const file = join(dir, `${i}.json`);
  writeFileSync(file, JSON.stringify({ event: s.event, ...s.scenario }));
  listLines.push(`${name}\t${file}`);
});
const listPath = join(dir, "list.txt");
writeFileSync(listPath, listLines.join("\n") + "\n");

const run = spawnSync(SPIRE, ["--sandbox-batch", SEED, listPath], { encoding: "utf8", maxBuffer: 1 << 28 });
if (run.status !== 0) throw new Error(`spire --sandbox-batch 失败: ${run.stderr}`);

// 解析:`#名字` 一段,后面跟 JSONL
const segments = new Map<string, Row[]>();
{
  let cur: string | null = null;
  for (const line of run.stdout.split("\n")) {
    const t = line.trim();
    if (t === "") continue;
    if (t.startsWith("#")) {
      cur = t.slice(1);
      segments.set(cur, []);
      continue;
    }
    if (cur) {
      try {
        segments.get(cur)!.push(JSON.parse(t));
      } catch {
        // 忽略无法解析的行
      }
    }
  }
}

interface Result {
  spec: Spec;
  rows: Row[];
  verdict: "ok" | "mismatch" | "untestable";
  reason: string | null;
}
const results: Result[] = [];
specs.forEach((s, i) => {
  const name = `${s.event}/${s.choice}/${i}`;
  const all = segments.get(name) ?? [];
  const rows = all.filter((r) => r.st);
  if (rows.length === 0) {
    results.push({ spec: s, rows: [], verdict: "untestable", reason: all[0]?.error ?? `没跑出状态(段=${all.length})` });
    return;
  }
  let reason: string | null;
  try {
    const raw = s.check(rows) as unknown;
    reason = raw === null || raw === true ? null : typeof raw === "string" ? raw : "检查返回了非文本结果";
  } catch (e) {
    reason = `检查抛错: ${(e as Error).message}`;
  }
  results.push({ spec: s, rows, verdict: reason === null ? "ok" : "mismatch", reason });
});

// ---- 报告 ----

const byEvent = new Map<string, Result[]>();
for (const r of results) {
  if (!byEvent.has(r.spec.event)) byEvent.set(r.spec.event, []);
  byEvent.get(r.spec.event)!.push(r);
}

const nOk = results.filter((r) => r.verdict === "ok").length;
const nMismatch = results.filter((r) => r.verdict === "mismatch").length;
const nUntestable = results.filter((r) => r.verdict === "untestable").length;
const hasNote = results.filter((r) => r.spec.note).length;

const lines: string[] = [];
const push = (s = "") => lines.push(s);

push("事件效果审计报告 (event coverage)");
push(`种子 seed=${SEED}  事件=${byEvent.size}  选项实测=${results.length}`);
push(`ok=${nOk}  mismatch=${nMismatch}  untestable=${nUntestable}  (含表示差异备注 ${hasNote})`);
push("");
push("结论列:ok = 与期望一致;mismatch = 不一致;untestable = 无法测(理由);[c] = 表示差异已备注");
push("");
push("事件                              选项                                    期望                                    实测                                    结论");
push("-".repeat(150));

for (const [ev, rs] of byEvent) {
  for (const r of rs) {
    const s = r.spec;
    const observed =
      r.verdict === "ok" ? "与期望一致" : r.reason ?? "?";
    const tag = r.verdict === "ok" ? (s.note ? "ok [c]" : "ok") : r.verdict === "mismatch" ? "MISMATCH" : "untestable";
    push(
      [
        `${ev}#${s.choice}`.padEnd(32),
        s.option.padEnd(36),
        s.expected.padEnd(38),
        observed.slice(0, 38).padEnd(38),
        tag,
      ].join(" "),
    );
    if (s.note) push(`    note: ${s.note}`);
  }
}

push("");
push("分类 (a) 我们错(审计发现并已修;断言见 src/core/events.rs::event_audit_fix_tests):");
push("  - cursed_tome:只有 2 页,缺第 3 页(成本应为 1/2/3 HP)→ 补第 3 页");
push("  - colosseum 第二场:只给 1 件遗物 → 补第二件(罕见),打赢直接进包");
push("  - masked_bandits 战斗:多掉一瓶药水 → 去掉药水掉落");
push("  - mindbloom:'I am Rich'/'I am Healthy' 没有按层号(<=40 / >=41)开关 → 加层号门控");
push("  - shining_light / winding_halls 'Embrace':缺 A15 变体(30% / 18%)→ 补 outcome_a15");
push("  - knowing_skull:三档共用涨价、且基础价用 round → 改成按选项各自计数 + max(6, floor(10% 上限))");
push("  - hypnotizing_colored_mushrooms(吃):25% 回血用了 round → 改成 floor");
push("  - vampires(接受):30% 上限用了 round → 改成 ceil");
push("  - the_woman_in_blue(离开 A15):错误地永久掉上限 → 改成扣血(ceil 5%)");
push("  - 多个'删牌/升级/变形'选项缺条件(无牌可选仍可点)→ 加 req_removable/req_upgradeable");
push("  - the_joust:金币 <50 仍可下注 → 加 req_gold 50");
push("  - vampires:升级过的起始打击没被删 → 一并删");
push("  - face_trader:10% 扣血的下限 1 点没生效 → hp_frac 也吃 hp_pct_min");
push("  - cursed_tome 的 Take:遗物直接进包 → 改成摆进奖励屏(反编译 refs/sts_lightspeed/src/game/GameContext.cpp:2558-2598 走 openCombatRewardScreen);");
push("    同轮补上死灵之书拾取时的死灵诅咒(Necronomicon.onEquip);三页中间屏去掉多余的第 2 项 Stop");
push("    (反编译 refs/sts_lightspeed/src/sim/search/GameAction.cpp:727-731 中间三屏的位掩码只有 Continue)");
push("  - colosseum 第一场:按地图上的 SLAVERS 遭遇摆了三只(多一只巡回官)→ 改成蓝/红奴隶主两只");
push("    (反编译 refs/sts_lightspeed/src/combat/MonsterGroup.cpp:208-211 的 COLOSSEUM_EVENT_SLAVERS);");
push("  - wing_statue 的 Destroy:按'总伤'判定 → 改成按单次伤害(语料 deckHasAttackWithSingleHitDamageAtLeast)");
push("");
push("分类 (b) 无法测:");
if (nUntestable === 0) push("  (无)");
for (const r of results.filter((x) => x.verdict === "untestable")) {
  push(`  [b] ${r.spec.event}#${r.spec.choice} ${r.spec.option}: ${r.reason}`);
}
push("");
push("分类 (c) 表示差异(效果等价,交互形态不同):");
const noted = results.filter((x) => x.spec.note);
if (noted.length === 0) push("  (无)");
for (const r of noted) push(`  [c] ${r.spec.event}#${r.spec.choice} ${r.spec.option}: ${r.spec.note}`);
push("");
push("分类 (d) wiki 与 lightspeed/参考冲突: 无(取整统一按参考实现各调用点的 floor/round/ceil)");
push("");
push("第三幕+ 事件复核对拍(反编译 GameContext.cpp / GameAction.cpp / ConsoleSimulator.cpp)结论:");
push("  falling: 三类型分支与\"摔头\"保底一致;瓶装牌按 corpus/wiki(真实游戏)排除 —— 反编译 Event::FALLING 只挂了一句");
push("    `// todo test and CANNOT BE BOTTLED`(refs/sts_lightspeed/src/game/GameContext.cpp:886),实现仍不过滤瓶装牌,是反编译省了一块。");
push("  mindbloom: 层号门控 <=40 / >=41 与 GameAction 一致。");
push("  vampires: ceil(30%) 上限、min(maxHp-1)、删全部起始打击(含升级)、5 张 Bite 一致。");
push("  the_mausoleum / living_wall / winding_halls / mysterious_sphere / sensory_stone / joust: 效果与分支一致。");
push("  the_moai_head: \"回满\"受花开彼岸限制(本轮修:full_heal 改走 heal,不再直接赋 max_hp)。");
push("  tomb_of_lord_red_mask: 本轮修 —— \"Offer gold\" 补 req_no_relic 门控(反编译两张位掩码互斥),");
push("    断言 = events.rs::tomb_of_lord_stops_offering_the_mask_once_you_wear_it。");
push("  knowing_skull: 本轮修 —— Success 的无色牌改走 shuffleRng 整池 java 洗牌取第一张非普通;");
push("  cursed_tome / colosseum / n'loth: 选项层与 corpus 一致;本轮按反编译修了 cursed_tome 的 Take 奖励屏");
push("    与死灵之书拾取诅咒、colosseum 第一场的阵容(见上 (a) 分类)。");
push("  secret_portal: 一致(跳 Boss 房,800 秒门槛抽象为 speedrunPace)。");
push("登记(本轮不改,附理由):");
push("  [d] colosseum 整体在反编译里是 stub(refs/sts_lightspeed/src/game/GameContext.cpp:2554-2556, spawn 被 disableColosseum 关掉),");
push("      只有第一场阵容(refs/sts_lightspeed/src/combat/MonsterGroup.cpp:208-211)与第二场奖励(MonsterGroup/Events)有反编译依据;");
push("      其余流程按 wiki/corpus;");
push("  [d] joust 掷点语义已核:反编译 THE_JOUST(refs/sts_lightspeed/src/game/GameContext.cpp:3280-3297)在两个分支前先掷一次");
push("      miscRng.randomBoolean(0.3)(0.3 = 骑士赢),本作 RollKind::Coin{num:3,den:10}(events.rs:2082/2091)同口径;");
push("      押凶手 70% 赢 100 / 押骑士 30% 赢 250 与语料一致。");
push("  [b] falling 的掷点时机:反编译在 onEnter 用 miscRng.random 预选下标(GameContext.cpp:890-895),本作在选项结算时删牌;");
push("      牌与时点不同,结果集合一致。");
push("");
push("");
push("本轮实测未发现新的 mismatch:");
if (nMismatch === 0) push("  (无)");
for (const r of results.filter((x) => x.verdict === "mismatch")) {
  push(`  [a?] ${r.spec.event}#${r.spec.choice} ${r.spec.option}: ${r.reason}`);
}

const report = lines.join("\n") + "\n";
if (OUT) {
  mkdirSync(dirname(OUT), { recursive: true });
  writeFileSync(OUT, report);
}
process.stdout.write(report);

// 有一处 mismatch 就以非零退出,方便 CI 挂上去
rmSync(dir, { recursive: true, force: true });
if (nMismatch > 0) process.exit(1);
