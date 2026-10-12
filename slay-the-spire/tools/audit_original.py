#!/usr/bin/env python3
"""把本作的实现参数与原版反编译出的类逐条比对(只关心铁甲战士/静默猎手).

原版侧: ~/projects/sts-golden/decomp/src/com/megacrit/cardcrawl/**
         (CFR 反编译 desktop-1.0.jar 得到, 见 tools/import_official.py 的说明)
本作侧: src/core/cards.rs / relics.rs / enemies.rs (用一份只给审计用的小解析器读)

比对的是"能机械对上"的东西: 费用/类型/稀有度/目标/标签/伤害/格挡/命中次数/血量/
招式伤害与升升值. 语义对不上的地方(效果顺序、随机调用次数)在这里只能提示, 要人看.

用法: python3 tools/audit_original.py [cards|relics|monsters|moves|all]

已知"本作与参照一致、但与原版不同"的一处(这一轮查实, 还没改, 改之前要先动对拍器):
  - 卡牌奖励的升级掷点顺序. 原版 AbstractDungeon.getRewardCards 是两遍制: 先把
    numCards 张(稀有度 + 抽牌)全定下来, 之后单独一轮跑
    `c.rarity != RARE && cardRng.randomBoolean(cardUpgradedChance) && c.canUpgrade()`;
    randomBoolean 无条件 ++counter, 所以第一幕(概率 0)也要吃这 numCards 次.
    独立佐证: refs/sts_lightspeed/src/game/GameContext.cpp:1805-1831(先有 rewardRarities[]
    再统一掷升级).
    本作 src/core/run.rs 的 create_card_reward 目前是"每张牌跟着掷 + 概率 0 不掷"
    —— 与参照实现 refs/slay-the-cli 的 rewards.ts:195-208 一致, 与原版不同.
    修之前必须给 tools/replay_ref.ts 加驱动侧补偿(把参考侧的奖励也重排成两遍制),
    否则 e2e 对拍表会从第一处卡牌奖励起全面分家(实测 act2 差异 9 -> 11748 处).
    补偿试过三轮(第三轮把配方全做齐了). 配方(四条, 缺一条就会分家):
      a. 快照取"把奖励屏拉起来的那一次 advance 之前"的 cardRng(在推进漏斗里记);
         取战斗开始前的更早快照会丢掉整场战斗消耗的掷点;
      b. 还要还原 run.blizzard.cardRarityFactor(奖励生成会改保底);
      c. 稀有度档位按真实房间来源传("monster"/"elite"/"boss"/"event");
      d. 换掉的那组卡牌沿用原 group 号.
    按用户口径"只看 a20"用现场参照做同基准 A/B(逐颗对齐前缀, 合计):
      a20    46 颗: 好转 6 / 不变 37 / 变差 3,  908 -> 927
      a20a2  12 颗: 好转 1 / 不变  9 / 变差 2,  553 -> 551
      a20a3   7 颗: 好转 2 / 不变  4 / 变差 1,  251 -> 282(seed 510: 11 -> 41)
      a20a4  45 颗: 全不变 492;  act4 16 颗: 全不变 176
      五表合计 2380 -> 2428(+48, 净收益)
    而 A0 的 act1/act2/act3 是净负(前缀和 +32 / -22 / -326), 用户已明确只管 a20.
    之所以还没落定: 改成两遍制会同时动到 A0 那几张表的 fixture 与基线 —— 重录 fixture
    之后 cargo test 有 12 项要重钉(9 张 CASE 表 + fixtures_cover_the_whole_first_act /
    the_walk_crosses_into_the_second_act / act4_walk_covers_the_whole_fourth_act 三个
    不变量测试), 属于纯机械但不能半途的活. 落定顺序:
      (1) 重新加补偿(replay_ref.ts)+ 本作改两遍制(run.rs) —— 配方见上四条;
      (2) 逐表重录 fixture: --all --script <表>.script --write;
      (3) cargo test 列出需重钉的 12 项, 逐张 --all --script <表>.script --pin 后按种子集
          替换 replay.rs 里的 CASE 表(三个不变量测试也要看它断言的是结构还是写死的步号);
      (4) 用 --tables --json 更新 check_all 的 E2E_BASELINE, 再跑 cargo test + check_all.
    落定时撞到的结构性问题, 定性了(下轮第一件事): 覆盖测试 the_walk_crosses_into_the_second_act
    断言 `crossed >= 15`(参考侧要有足够多种子切进第二幕), 而补偿之后只剩 4 —— 不是"少一颗",
    是参考侧的走法被截断了. 最可疑的是 fixRewardCards 里 `s.run.room = { ...room, entries }`
    这个整体替换: 参考后续某个 advance 可能按房间对象的形态/身份走了另一条分支而提前收尾.
    下轮先做: 施加补偿后单颗种子跑 acts.script, 看参考侧 JSONL 最后停在哪一步、停在哪里,
    把根因找出来再谈落定(而不是去调测试阈值 —— 阈值 15 是"尺子还在量东西"的守卫).
 重录 fixture 之后,
    `the_walk_crosses_into_the_second_act` 报"只有 4 个种子参考走到了第二幕, 切幕没被
    量到" —— 也就是补偿改变了参考侧走法的长度(不是只改奖励内容), 而该测试要求"有足
    够多的种子切到第二幕". 另一条 `fixtures_cover_the_whole_first_act` 报的是覆盖计数
    36 vs 16(纯数字可更新). 下次落定前先查: 补偿之后参考侧为什么走得短了(怀疑点: 重排
    奖励时改了 s.run.room / rng 的形态, 让参考后续某些分支提前结束; 或重录时的步数上限
    与原来不同).
    方法学提醒(踩过的坑): tools/golden/e2e/*.ref.jsonl 这些 fixture 与"现场跑参照"不是
    同一份(同种子两边 md5 不同) —— cargo test 比冻结 fixture, tools/e2e_diff.ts /
    check_all 比现场参照, 两者不能混着做 A/B, 否则结论会反.
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import import_official as io  # noqa: E402  (同目录的抽取器, 复用类解析)

GOLDEN = os.path.expanduser("~/projects/sts-golden")
DECOMP = os.path.join(GOLDEN, "decomp", "src", "com", "megacrit", "cardcrawl")
OURS = os.path.join(HERE, "..", "src", "core")

SUF = {"RED": "R", "GREEN": "G", "BLUE": "B", "PURPLE": "P"}
TYPE_MAP = {
    "CardType::Attack": "attack",
    "CardType::Skill": "skill",
    "CardType::Power": "power",
    "CardType::Status": "status",
    "CardType::Curse": "curse",
}
RARITY_MAP = {
    "Rarity::Basic": "basic",
    "Rarity::Common": "common",
    "Rarity::Uncommon": "uncommon",
    "Rarity::Rare": "rare",
    "Rarity::Special": "special",
}
# 原版的特殊稀有度: 诅咒卡在原版是 CURSE, 本作归到 special
RARITY_ALIAS = {"curse": "special", "deprecated": "special"}
# 本作的 Target 只有四种, 原版把"打自己/无目标"分开(SELF/NONE), 本作都归 none
TARGET_MAP = {
    "Target::None": "none",
    "Target::Enemy": "enemy",
    "Target::All": "all",
    # 本作把"随机打敌人"单列成 Random(不用选目标), 原版那类卡标的是 ALL_ENEMY
    "Target::Random": "all",
}
# 原版目标 -> 本作的说法
ORIG_TARGET = {
    "enemy": "enemy",
    "self_and_enemy": "enemy",
    "allenemy": "all",
    "all": "allenemy",
    "self": "none",
    "none": "none",
}


def nk(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


def corpus_key(cid):
    s = cid.lower()
    for k, v in SUF.items():
        if s.endswith("_" + k.lower()):
            s = s[: -len(k) - 1] + "_" + v
    return nk(s)


# ---------------------------------------------------------------- 本作侧
def our_entries(path, type_name):
    """把 `Type { ... },` 逐条切开(字段都是 8 空格缩进, 条目以 4 空格 `},` 收尾)."""
    text = open(path, encoding="utf-8").read()
    out = []
    for m in re.finditer(r"\n    %s \{\n(.*?)\n    \}," % type_name, text, re.S):
        out.append(m.group(1))
    return out


def field(block, name, pattern=r'([^\n]+)'):
    m = re.search(r"(?:^|\n)        %s: %s," % (name, pattern), block)
    return m.group(1).strip() if m else None


def ints(text):
    return sorted(int(x) for x in re.findall(r"\b(\d+)\b", text))


def load_ours_cards():
    cards = {}
    for block in our_entries(os.path.join(OURS, "cards.rs"), "CardDef"):
        cid = field(block, "id", r'"([^"]+)"')
        if not cid:
            continue
        cid = cid.strip('"')
        cost = field(block, "cost")
        cost = {
            "Cost::X": "X",
            "Cost::Unplayable": "-",
        }.get(cost) or re.sub(r"Cost::Fixed\((\d+)\)", r"\1", cost or "")
        kind = TYPE_MAP.get(field(block, "kind") or "", "")
        rarity = RARITY_MAP.get(field(block, "rarity") or "", "")
        target = TARGET_MAP.get(field(block, "target") or "", "")
        flags = {
            f: field(block, f) == "true"
            for f in ("exhaust", "ethereal", "innate", "retain")
        }
        # 基础效果: effects: &[ ... ], 升级效果在 up!( ... ) 里
        base = re.search(r"\n        effects: &\[\n(.*?)\n        \],", block, re.S)
        up = re.search(r"\n        upgrade: up!\(\n(.*?)\n        \),", block, re.S)
        base_txt = base.group(1) if base else ""
        up_txt = up.group(1) if up else ""
        # 第一个带 amount 的伤害类效果 / 格挡效果 / 命中次数
        def first(pattern, text):
            m = re.search(pattern, text)
            return int(m.group(1)) if m else None

        cards[cid] = {
            "cost": cost,
            "kind": kind,
            "rarity": rarity,
            "target": target,
            "flags": flags,
            "dmg": first(r"Effect::Damage\w* \{[^}]*?amount: (\d+)", base_txt),
            "dmg_up": first(r"Effect::Damage\w* \{[^}]*?amount: (\d+)", up_txt),
            "block": first(r"Effect::Block \{[^}]*?amount: (\d+)", base_txt),
            "block_up": first(r"Effect::Block \{[^}]*?amount: (\d+)", up_txt),
            "times": first(r"Effect::Damage\w* \{[^}]*?times: (\d+)", base_txt),
            "ints": set(ints(base_txt)),
            "ints_up": set(ints(up_txt)),
            "up_innate": "up_innate!" in block or "innate: Some(true)" in block,
            "multi_upgrade": "multi_upgrade: true" in block,
        }
    return cards


# ---------------------------------------------------------------- 原版侧
def original_cards():
    idx = io.index_classes(os.path.join(DECOMP, "cards"))
    out = {}
    for key, path in idx.items():
        src = open(path, encoding="utf-8").read()
        info = io.parse_card(src)
        if not info:
            continue
        # 基础标签只看构造函数; upgrade() 里的单独记(本作是 up_innate! 那种)
        ctor = re.search(r"public\s+\w+\s*\([^)]*\)\s*\{", src)
        ctor_body = io.body_at(src, ctor.start()) if ctor else ""
        up_body = io.method_body(src, r"public\s+void\s+upgrade\s*\(\s*\)") or ""

        def flags_in(body):
            return {
                f: bool(re.search(r"this\.%s = true" % f, body))
                for f in ("exhaust", "isEthereal", "isInnate", "selfRetain")
            }

        flags = flags_in(ctor_body)
        up_flags = flags_in(up_body)
        out[key] = {
            "src": src,
            "cost": info.get("cost", ""),
            "kind": info["kind"],
            "rarity": info["rarity"],
            "target": info["target"],
            "color": info["color"],
            "nums": info["numbers"],
            "flags": {
                "exhaust": flags["exhaust"],
                "ethereal": flags["isEthereal"],
                "innate": flags["isInnate"],
                "retain": flags["selfRetain"],
            },
            "up_innate": up_flags["isInnate"],
        }
    return out


def load_corpus_ids():
    """本作 id -> 语料 id(用于找原版类)."""
    text = open(os.path.join(OURS, "corpus.rs"), encoding="utf-8").read()
    out = {}
    for m in re.finditer(r'CardInfo \{ id: "([^"]+)", corpus_id: "([^"]+)"', text):
        out.setdefault(m.group(1), m.group(2))
    return out


def audit_cards():
    ours = load_ours_cards()
    orig = original_cards()
    cids = load_corpus_ids()
    # 只看铁甲战士/静默猎手: 颜色从原版类拿
    rows = []
    checked = 0
    for cid, o in sorted(ours.items()):
        key = None
        mine = cids.get(cid)
        for cand in ([mine] if mine else []) + [cid]:
            if not cand:
                continue
            for k in orig:
                if nk(k) == corpus_key(cand) or nk(k) == nk(cand):
                    key = k
                    break
            if key:
                break
        if not key:
            continue
        e = orig[key]
        if e["color"] not in ("red", "green"):
            continue
        checked += 1
        add = lambda f, a, b: rows.append((cid, f, str(a), str(b)))
        if o["cost"] != e["cost"]:
            add("cost", o["cost"], e["cost"])
        if o["kind"] != e["kind"]:
            add("kind", o["kind"], e["kind"])
        if o["rarity"] != RARITY_ALIAS.get(e["rarity"], e["rarity"]):
            add("rarity", o["rarity"], e["rarity"])
        if o["target"] != ORIG_TARGET.get(e["target"], e["target"]):
            add("target", o["target"], e["target"])
        for f, v in o["flags"].items():
            if v != e["flags"][f]:
                add("flag:" + f, v, e["flags"][f])
        if o["up_innate"] != e["up_innate"]:
            add("upgrade:innate", o["up_innate"], e["up_innate"])
        n = e["nums"]
        if o["dmg"] is not None and n["dmg"] and o["dmg"] != n["dmg"]:
            add("dmg", o["dmg"], n["dmg"])
        if o["block"] is not None and n["blk"] and o["block"] != n["blk"]:
            add("block", o["block"], n["blk"])
        if o["dmg_up"] is not None and n["dmg"] and not o["multi_upgrade"]:
            want = n["dmg"] + n["dmg_up"]
            if o["dmg_up"] != want:
                add("dmg_up", o["dmg_up"], want)
        if o["block_up"] is not None and n["blk"]:
            want = n["blk"] + n["blk_up"]
            if o["block_up"] != want:
                add("block_up", o["block_up"], want)
        # 魔法数只在"文本里明写 !M! 次"这一类才机械可比, 其余只做提示
        if n["mag"] and re.search(r"!M! times|!M! time\b", e["src"]):
            if o["times"] != n["mag"]:
                add("times", o["times"], n["mag"])
    print("=== 卡牌审计: 铁甲战士/静默猎手 %d 张, 机械可比的项对不上 %d 处" % (checked, len(rows)))
    print("卡牌,字段,本作,原版")
    for r in rows:
        print("%s,%s,%s,%s" % r)
    return rows


# ---------------------------------------------------------------- 遗物
# 原版的遗物有两类实现面: (a) 覆写 AbstractRelic 的钩子, (b) 核心代码里按名字查
# (hasRelic("Calipers") 这种). 两边都算"实现面"; 我们这边靠 fx/note/核心代码里的 id 引用.
NOISE_HOOKS = {
    "flash", "makeCopy", "update", "render", "obtain", "instantObtain", "loadLargeImg",
    "beginPulse", "beginLongPulse", "stopPulse", "setCounter", "getUpdatedDescription",
    "updateDescription", "getRelicTip", "canSpawn", "compareTo", "discard", "relicTips",
    "initializeTips", "bossObtainLogic", "gameDataUploadData", "toString",
}


def original_relic_surface():
    """每件遗物在原版里的"实现面": 覆写的钩子 + 核心代码里提到它(类名或 ID 字面量).

    原版不少遗物是核心按名字查的(hasRelic("Calipers") / Omamori.ID 这种), 所以只看
    类自己覆写了什么会漏掉一半.
    """
    ab = open(os.path.join(DECOMP, "relics", "AbstractRelic.java"), encoding="utf-8").read()
    allm = set(re.findall(r"public\s+[\w<>\[\].]+\s+(\w+)\s*\(", ab))
    hooks = {
        h
        for h in allm
        # 注意: canUseCampfireOption / addCampfireOption 是真钩子(铲子/烟斗/碗), 别当噪音
        if h not in NOISE_HOOKS and not h.startswith(("get", "set", "is", "has"))
    }
    surface = {}
    stems = {}
    relic_dir = os.path.join(DECOMP, "relics")
    for dirpath, _, files in os.walk(relic_dir):
        for f in files:
            if not f.endswith(".java") or f == "AbstractRelic.java":
                continue
            src = open(os.path.join(dirpath, f), encoding="utf-8").read()
            m = re.search(r'static final String ID = "([^"]+)"', src)
            if not m:
                continue
            key = m.group(1)
            stems[key] = f[:-5]
            got = {h for h in hooks if re.search(r"public\s+[\w<>\[\].]+\s+%s\s*\(" % h, src)}
            surface.setdefault(key, set()).update(got)
    # 核心代码里提到这件遗物(类名), 说明它的效果是在那儿实现的
    blob = []
    for pkg in ("dungeons", "rooms", "actions", "core", "cards", "monsters", "powers", "events", "shop", "rewards", "screens", "helpers", "neow", "ui"):
        base = os.path.join(DECOMP, pkg)
        if not os.path.isdir(base):
            continue
        for dirpath, _, files in os.walk(base):
            for f in files:
                if f.endswith(".java"):
                    blob.append(open(os.path.join(dirpath, f), encoding="utf-8").read())
    core = "\n".join(blob)
    for key, stem in stems.items():
        if re.search(r"\b%s\b" % re.escape(stem), core):
            surface.setdefault(key, set()).add("core:" + stem)
    return surface


def our_relic_entries():
    out = {}
    for block in our_entries(os.path.join(OURS, "relics.rs"), "RelicDef"):
        rid = field(block, "id", r'"([^"]+)"')
        if not rid:
            continue
        rid = rid.strip('"')
        fx = re.search(r"fx: RelicFx \{\n(.*?)\n        \},", block, re.S)
        body = fx.group(1) if fx else ""
        non_zero = any(
            line.strip() and not line.strip().startswith("..RelicFx::ZERO")
            for line in body.split("\n")
        )
        note = field(block, "note", r'"([^"]*)"') or '""'
        out[rid] = {"fx": non_zero, "note": note.strip('"')}
    return out


def our_core_mentions():
    """本作核心代码里出现过的遗物 id(说明按名字特判过)."""
    text = []
    for dirpath, _, files in os.walk(OURS):
        for f in files:
            if f.endswith(".rs") and f not in ("relics.rs", "corpus.rs"):
                text.append(open(os.path.join(dirpath, f), encoding="utf-8").read())
    blob = "\n".join(text)
    return blob


def audit_relics():
    surface = original_relic_surface()
    ours = our_relic_entries()
    blob = our_core_mentions()
    # 本作 id -> 原版键: 走官方显示名(Paper Krane 的类 ID 其实是 Paper Crane)
    src_json = os.path.join(GOLDEN, "out", "corpus_src.json")
    by_name = {}
    if os.path.exists(src_json):
        for e in json.load(open(src_json, encoding="utf-8"))["relics"]:
            by_name[nk(e["name"])] = e["key"]
    gaps, invented = [], []
    for rid, info in sorted(ours.items()):
        key = by_name.get(nk(rid)) or next((k for k in surface if nk(k) == nk(rid)), None)
        orig = surface.get(key, set()) if key else set()
        mentioned = ('"%s"' % rid) in blob
        mine = info["fx"] or info["note"] or mentioned
        if orig and not mine:
            gaps.append((rid, key, " ".join(sorted(orig))[:90]))
        if mine and not orig:
            invented.append((rid, key))
    print("=== 遗物审计: 本作 %d 件, 原版有实现面而我们什么都没写 %d 件, 我们有而原版没有 %d 件"
          % (len(ours), len(gaps), len(invented)))
    print("遗物,字段,说明")
    for rid, key, what in gaps:
        print("%s,silent-gap,%s" % (rid, what))
    for rid, key in invented:
        print("%s,no-original,%s" % (rid, key))
    return gaps, invented


# ---------------------------------------------------------------- 怪物
def our_enemies():
    out = {}
    for f in ("act1", "act2", "act34"):
        path = os.path.join(OURS, "enemies", f + ".rs")
        if not os.path.exists(path):
            continue
        text = open(path, encoding="utf-8").read()
        for m in re.finditer(r"EnemyDef \{\n(.*?)\n    \}(?:,|\n)", text, re.S):
            b = m.group(1)
            rid = re.search(r'id: "([a-z0-9_]+)"', b)
            hp = re.search(r"hp: \((\d+), (\d+)\)", b)
            moves = re.findall(r'MoveDef \{[^}]*?name: "([^"]+)"', b)
            if not rid:
                continue
            out[rid.group(1)] = {
                "hp": (int(hp.group(1)), int(hp.group(2))) if hp else None,
                "moves": moves,
                "block": b,
            }
    return out


def _asc_chain(body):
    """把 if (asc>=N) {...} / else if / else 链摊成 [(门槛, 代码块)] + else 块.

    原版血量两档写法最常见的形状; else if 链(守卫者 A19/A9)也要吃下.
    """
    marks = []
    for m in re.finditer(r"(?:else\s+)?if \(AbstractDungeon\.ascensionLevel >= (\d+)\)\s*\{", body):
        marks.append((int(m.group(1)), body.find("{", m.end() - 1)))
    out = []
    for i, (lvl, pos) in enumerate(marks):
        seg = io.body_at(body, pos)
        if seg is None:
            continue
        out.append((lvl, seg))
    tail = None
    m = re.search(r"\}\s*else\s*\{", body)
    if m:
        tail = io.body_at(body, body.find("{", m.end() - 1))
    return out, tail


def _hp_in(text):
    """(下限, 上限, 是否两参形式). 原版单参 setHp(N) 是定值: 连一次掷点都不消耗."""
    m = re.search(r"setHp\((\d+)(?:,\s*(\d+))?\)", text or "")
    if not m:
        return None
    lo = int(m.group(1))
    if m.group(2):
        return (lo, int(m.group(2)), True)
    return (lo, lo, False)


def original_monsters():
    out = {}
    base = os.path.join(DECOMP, "monsters")
    for dirpath, _, files in os.walk(base):
        for f in files:
            if not f.endswith(".java") or f.startswith("Abstract"):
                continue
            src = open(os.path.join(dirpath, f), encoding="utf-8").read()
            mid = re.search(r'public static final String ID = "([^"]+)"', src)
            if not mid:
                continue
            cls = f[:-5]
            # 合并所有构造器: 有的类 public 构造器只是转发给私有构造器(Searing Blow 那种)
            bodies = []
            for m in re.finditer(r"(?:public|private|protected)\s+%s\s*\([^)]*\)\s*\{" % re.escape(cls), src):
                b = io.body_at(src, m.start())
                if b:
                    bodies.append(b)
            body = "\n".join(bodies) if bodies else src
            chain, tail = _asc_chain(body)
            # 每档的实际血量: 门槛从高到低, 第一个 <= asc 的分支生效
            asc_hp = []
            rolled_asc = {}
            for lvl, seg in chain:
                hp = _hp_in(seg)
                if hp:
                    asc_hp.append((lvl, hp[0], hp[1]))
                    rolled_asc[lvl] = hp[2]
            rolled_base = None
            base_hp = _hp_in(tail) if tail else None
            if base_hp is not None:
                rolled_base = base_hp[2]
                base_hp = (base_hp[0], base_hp[1])
            if base_hp is None:
                # 没有 else 时: 取不在任何飞升分支里的 setHp(单参形式)
                for m in re.finditer(r"setHp\((\d+)(?:,\s*(\d+))?\)", body):
                    inside = any(m.start() >= seg_start and m.start() < seg_start + len(seg)
                                 for _, seg in chain for seg_start in [body.find(seg)])
                    if inside:
                        continue
                    lo = int(m.group(1))
                    rolled_base = bool(m.group(2))
                    base_hp = (lo, int(m.group(2)) if m.group(2) else lo)
                    break
            moves = sorted(set(re.findall(r'moveName = "([^"]+)"', src)))
            out[mid.group(1)] = {
                "src": src,
                "hp": base_hp,
                "draws": bool(re.search(r"setHp\(", body)),
                "rolled": rolled_base,
                "rolled_asc": rolled_asc,
                "asc_hp": asc_hp,
                "move_names": moves,
            }
    return out


def our_fixed_no_roll():
    """本作 ascension::hp_fixed_no_roll 列出的"定值血量、不掷点"的怪."""
    text = open(os.path.join(OURS, "ascension.rs"), encoding="utf-8").read()
    body = io.method_body(text, r"pub fn hp_fixed_no_roll\(def_id: &str\) -> bool \{")
    if not body:
        return set()
    start = body.find("matches!(")
    if start < 0:
        return set()
    depth = 0
    for i in range(body.index("(", start), len(body)):
        if body[i] == "(":
            depth += 1
        elif body[i] == ")":
            depth -= 1
            if depth == 0:
                return set(re.findall(r'"([a-z0-9_]+)"', body[start:i]))
    return set()


def our_hp_asc():
    """本作 ascension::HP_ASC 表: id -> (门槛, 区间)."""
    text = open(os.path.join(OURS, "ascension.rs"), encoding="utf-8").read()
    out = {}
    for m in re.finditer(r'\("([a-z0-9_]+)", (\d+), \((\d+), (\d+)\)\)', text):
        out[m.group(1)] = (int(m.group(2)), (int(m.group(3)), int(m.group(4))))
    return out


def corpus_monster_ids():
    """本作怪物 id -> 语料 id(史莱姆的 _small 在语料里是 _S)."""
    text = open(os.path.join(OURS, "corpus.rs"), encoding="utf-8").read()
    out = {}
    for m in re.finditer(r'MonsterInfo \{ id: "([^"]+)", corpus_id: "([^"]+)"', text):
        out[m.group(1)] = m.group(2)
    return out


def audit_monsters():
    ours = our_enemies()
    orig = original_monsters()
    cids = corpus_monster_ids()
    rows = []
    checked = 0
    by_nk = {nk(k): (k, v) for k, v in orig.items()}
    for rid, mine in sorted(ours.items()):
        cand = cids.get(rid)
        hit = None
        if cand:
            for alt in (cand, cand.replace("_S", "_small").replace("_M", "_medium").replace("_L", "_large"),
                        cand.replace("_SMALL", "_S").replace("_MEDIUM", "_M").replace("_LARGE", "_L")):
                if nk(alt) in by_nk:
                    hit = by_nk[nk(alt)]
                    break
        hit = hit or by_nk.get(nk(rid))
        if not hit:
            continue
        key, e = hit
        checked += 1
        if mine["hp"] and e["hp"] and mine["hp"] != e["hp"]:
            rows.append((rid, "hp", "%d-%d" % mine["hp"], "%d-%d" % e["hp"]))
        ours_asc = our_hp_asc().get(rid)
        # 定值血量的怪原版不掷点: 我们这边必须也在 hp_fixed_no_roll 名单里, 否则会多掷一次
        ours_fixed = rid in our_fixed_no_roll()
        theirs_fixed = not e["draws"]
        if theirs_fixed is not None and ours_fixed != theirs_fixed and e["hp"]:
            rows.append((rid, "hp-roll", "不掷" if ours_fixed else "掷点", "不掷" if theirs_fixed else "掷点"))

        def theirs_at(level):
            """原版在这档飞升时实际用的血量(门槛从高到低, 第一个 <= level 的生效)."""
            best = None
            for lvl, lo, hi in sorted(e["asc_hp"], reverse=True):
                if level >= lvl:
                    best = (lvl, (lo, hi))
                    break
            return best

        # 本作有表的档位 -> 与原版该档实际值比
        if ours_asc:
            lvl, rng = ours_asc
            t = theirs_at(lvl)
            if t is None:
                rows.append((rid, "asc-hp 多余", "A%d:%d-%d" % (lvl, rng[0], rng[1]), "-"))
            elif t[1] != rng:
                rows.append((rid, "asc-hp", "A%d:%d-%d" % (lvl, rng[0], rng[1]),
                             "A%d:%d-%d" % (t[0], t[1][0], t[1][1])))
        # 原版有而本作没写的档位
        for lvl, lo, hi in e["asc_hp"]:
            if not ours_asc or lvl < ours_asc[0]:
                rows.append((rid, "asc-hp 缺", "-", "A%d:%d-%d" % (lvl, lo, hi)))
        # 招式数: 原版的 moveName 是内部 id, 本作用自己的显示名, 只比个数
        theirs_n = len(set(e["move_names"]))
        if theirs_n and len(mine["moves"]) != theirs_n:
            rows.append((rid, "move-count", len(mine["moves"]), theirs_n))
    print("=== 怪物审计: 对上 %d 只, 提示 %d 条" % (checked, len(rows)))
    print("怪物,字段,本作,原版")
    for r in rows:
        print("%s,%s,%s,%s" % r)
    return rows


# ---------------------------------------------------------------- 招式伤害
def _resolve_int(src, expr):
    """把构造函数里的数值参数解析成 (常规值, 飞升档, 飞升值).

    支持: 字面量 / this.字段(字段可能是 asc 三元).
    """
    expr = (expr or "").strip()
    m = re.fullmatch(r"-?\d+", expr)
    if m:
        return int(expr), None, None
    m = re.fullmatch(r"this\.(\w+)", expr)
    if m:
        f = m.group(1)
        fm = re.search(r"this\.%s = ([^;]+);" % f, src)
        if fm:
            body = fm.group(1)
            tm = re.search(r"ascensionLevel >= (\d+)\s*\?\s*(\d+)\s*:\s*(\d+)", body)
            if tm:
                return int(tm.group(3)), int(tm.group(1)), int(tm.group(2))
            nm = re.search(r"-?\d+", body)
            if nm:
                return int(nm.group(0)), None, None
    return None, None, None


def original_move_damage(src):
    """原版一只怪的攻击招式集合: {(伤害, 命中次数)}.

    写法: setMove([名字,] (byte)k, Intent.ATTACK[_XXX], 伤害[, 命中次数[, true]])
    伤害可能是字面量、this.字段(还可能是 asc 三元), 同一招常在不同分支里重复设置, 去重.
    """
    out = set()
    for m in re.finditer(r"setMove\(((?:[^()]|\([^()]*\))*)\)", src):
        args = [a.strip() for a in _split_args(m.group(1))]
        idx = next((i for i, a in enumerate(args) if "Intent.ATTACK" in a), None)
        if idx is None or idx + 1 >= len(args):
            continue
        base, _lvl, _ascv = _resolve_int(src, args[idx + 1])
        if base is None:
            continue
        hits = 1
        if idx + 2 < len(args):
            hm = re.fullmatch(r"(\d+)", args[idx + 2])
            if hm:
                hits = int(hm.group(1))
        out.add((base, hits))
    return sorted(out)


def _split_args(s):
    out, cur, depth = [], "", 0
    for ch in s:
        if ch == "," and depth == 0:
            out.append(cur)
            cur = ""
            continue
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        cur += ch
    out.append(cur)
    return [x for x in out if x.strip()]


def our_monster_dump():
    import subprocess

    bin_path = os.path.join(HERE, "..", "target", "debug", "spire")
    r = subprocess.run([bin_path, "--dump", "monsters"], capture_output=True, text=True)
    if r.returncode != 0:
        return {}
    return {m["id"]: m for m in json.loads(r.stdout)["monsters"]}


def _orig_move_raw(src):
    """带飞升信息的原始版本(上面那个去重集合丢了档位)."""
    out = []
    for m in re.finditer(r"setMove\(((?:[^()]|\([^()]*\))*)\)", src):
        args = [a.strip() for a in _split_args(m.group(1))]
        idx = next((i for i, a in enumerate(args) if "Intent.ATTACK" in a), None)
        if idx is None or idx + 1 >= len(args):
            continue
        base, lvl, ascv = _resolve_int(src, args[idx + 1])
        if base is not None:
            out.append((base, 1, lvl, ascv))
    return out


# 招式伤害的"表示不同"登记: ids -> 原因(不参与机械比对)
MOVE_REP_DIFF = {
    # 越拖越痛: 原版在构造器里排了 startingDeathDmg+{0,5,...,30} 一整张表,
    # 本作建成 AttackScaling(每回合 +5, 上限 30), 两者的取值序列一致但形状不同
    "giant_head": "成长型伤害: 表 vs AttackScaling",
}


def audit_move_damage():
    ours = our_monster_dump()
    if not ours:
        print("=== 招式伤害审计: 先 cargo build(要 target/debug/spire)")
        return []
    cids = corpus_monster_ids()
    orig = original_monsters()
    by_nk = {nk(k): (k, v) for k, v in orig.items()}
    rows = []
    checked = 0
    for rid, dump in sorted(ours.items()):
        hit = None
        cands = [cids.get(rid), rid]
        if cands[0]:
            cands.append(cands[0].replace("_S", "_small").replace("_M", "_medium").replace("_L", "_large"))
        for cand in cands:
            if cand and nk(cand) in by_nk:
                hit = by_nk[nk(cand)]
                break
        if not hit:
            continue
        src = hit[1]["src"]
        want = original_move_damage(src)
        got = sorted(
            set(
                (m["base"]["damage"], m["base"]["times"] or 1)
                for m in dump["moves"]
                if m["base"].get("damage") is not None
            )
        )
        checked += 1
        if rid in MOVE_REP_DIFF:
            continue
        if want and got != want:
            rows.append((rid, "招伤", " ".join("%dx%d" % g for g in got), " ".join("%dx%d" % w for w in want)))
        # 原版飞升会改伤害的, 本作也必须有一档变化
        asc_want = sorted(
            set((lvl, ascv) for _d, _t, lvl, ascv in _orig_move_raw(src) if lvl and ascv is not None)
        )
        asc_got = sorted(set(
            (c["asc"], c["damage"])
            for m in dump["moves"]
            for c in m.get("changes", [])
            if c.get("damage") is not None
        ))
        if asc_want and not asc_got:
            rows.append((rid, "招伤飞升档", "无", str(asc_want)))
        elif asc_want and asc_got and [d for _, d in asc_got] != [d for _, d in asc_want]:
            rows.append((rid, "招伤飞升档", str(asc_got), str(asc_want)))
    print("=== 招式伤害审计: 对上 %d 只, 提示 %d 条" % (checked, len(rows)))
    print("怪物,字段,本作,原版")
    for r in rows:
        print("%s,%s,%s,%s" % r)
    return rows


def main():
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    if what in ("cards", "all"):
        audit_cards()
    if what in ("relics", "all"):
        audit_relics()
    if what in ("monsters", "all"):
        audit_monsters()
    if what in ("moves", "all"):
        audit_move_damage()


if __name__ == "__main__":
    main()
