#!/usr/bin/env python3
"""从 refs/slay-the-cli 的语料生成 src/core/corpus.rs.

用法: python3 tools/gen_corpus.py [语料目录] [输出文件]
默认语料目录 ../refs/slay-the-cli/data/corpus, 默认输出 src/core/corpus.rs.

生成的是纯展示数据(名字/类型/稀有度/费用/描述文本), 供图鉴和角色选择读取;
可玩逻辑仍然在 cards.rs / relics.rs / potions.rs / events.rs 里手写.
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SRC = os.path.join(HERE, "..", "..", "refs", "slay-the-cli", "data", "corpus")
DEFAULT_OUT = os.path.join(HERE, "..", "src", "core", "corpus.rs")

# 语料里的角色后缀 -> 本游戏 id 前缀(去掉后缀后就是本游戏的基础牌 id)
SUFFIXES = ("_red", "_green", "_blue", "_purple")


def game_id(corpus_id: str) -> str:
    """语料 id 转成本游戏 id: STRIKE_RED -> strike, BASH -> bash.

    只有基础牌才去掉颜色后缀(STRIKE_RED/DEFEND_GREEN...);
    SEEING_RED 这种名字里带 _RED 的不能动.
    """
    s = corpus_id.lower()
    for suf in SUFFIXES:
        if s.endswith(suf) and s[: -len(suf)] in ("strike", "defend"):
            s = s[: -len(suf)]
            break
    return s


def pick_upgrade(text: str, upgraded: bool) -> str:
    """wiki 链接和 [base|upgraded] 两级选择, 保留 $Keyword."""
    s = re.sub(r"<br\s*/?>", "\n", text, flags=re.I)
    s = re.sub(r"\[\[([^\[\]]*)\]\]", lambda m: m.group(1).split("|")[-1], s)
    return re.sub(
        r"\[([^\[\]|]*)\|([^\[\]]*)\]",
        lambda m: m.group(2) if upgraded else m.group(1),
        s,
    )


def resolve_markup(text: str, upgraded: bool) -> str:
    """按参考实现 src/cli/text/markup.ts 把标记解析成纯文本."""
    s = pick_upgrade(text, upgraded)
    s = re.sub(r"\{\{([^{}]*)\}\}", lambda m: m.group(1).split("|")[-1], s)
    s = re.sub(r"\$([A-Za-z-]+)", r"\1", s)
    # 连续的 @XX(@RE/@GE...)是能量符号, 合并成 (N): 得到几点能量就写几
    s = re.sub(
        r"@[A-Z]+(?:\s*@[A-Z]+)*",
        lambda m: "(%d)" % len(re.findall(r"@[A-Z]+", m.group(0))),
        s,
    )
    lines = [re.sub(r"\s+", " ", ln).strip() for ln in s.split("\n")]
    return "\n".join(ln for ln in lines if ln)


def rs(s) -> str:
    """Rust 字符串字面量."""
    if s is None:
        s = ""
    s = str(s).replace("\\", "\\\\").replace('"', '\\"')
    s = s.replace("\n", "\\n").replace("\t", " ")
    # 非 ascii 一律转义, 保证生成文件可读且不会被编码搞坏
    out = []
    for ch in s:
        out.append(ch if 32 <= ord(ch) < 127 else "\\u{%x}" % ord(ch))
    return '"' + "".join(out) + '"'


def load(src: str, name: str):
    with open(os.path.join(src, name + ".json"), encoding="utf-8") as f:
        return json.load(f)


def cost_str(c) -> str:
    if c is None:
        return "-"
    if isinstance(c, str):
        return c
    if isinstance(c, (int, float)):
        n = int(c)
        if n == -1:
            return "X"
        if n < 0:
            return "-"
        return str(n)
    return str(c)


def gen_cards(src: str) -> str:
    rows = []
    for c in load(src, "cards"):
        up = c.get("upgrade") or {}
        text_up = resolve_markup(c.get("text", ""), True) if c.get("upgrade") else ""
        rows.append(
            "    CardInfo {{ id: {}, corpus_id: {}, name: {}, color: {}, kind: {}, "
            "rarity: {}, pool: {}, cost: {}, cost_up: {}, target: {}, text: {}, text_up: {} }},".format(
                rs(game_id(c["id"])),
                rs(c["id"]),
                rs(c["name"]),
                rs(c.get("color", "")),
                rs(c.get("type", "")),
                rs(c.get("rarity", "")),
                rs(c.get("pool", "")),
                rs(cost_str(c.get("cost"))),
                rs(cost_str(up.get("cost", c.get("cost"))) if up else "-"),
                rs(c.get("target", "")),
                rs(resolve_markup(c.get("text", ""), False)),
                rs(text_up),
            )
        )
    return "\n".join(rows)


def gen_relics(src: str) -> str:
    rows = []
    for r in load(src, "relics"):
        pool = r.get("pool", "")
        rows.append(
            "    RelicInfo {{ id: {}, corpus_id: {}, name: {}, tier: {}, pool: {}, text: {} }},".format(
                rs(game_id(r["id"])),
                rs(r["id"]),
                rs(r["name"]),
                rs(r.get("tier", "")),
                rs(pool),
                rs(resolve_markup(r.get("text", ""), False)),
            )
        )
    return "\n".join(rows)


def gen_potions(src: str) -> str:
    rows = []
    for p in load(src, "potions"):
        rows.append(
            "    PotionInfo {{ id: {}, corpus_id: {}, name: {}, color: {}, rarity: {}, targeted: {}, text: {} }},".format(
                rs(game_id(p["id"])),
                rs(p["id"]),
                rs(p["name"]),
                rs(p.get("class", "")),
                rs(p.get("rarity", "")),
                "true" if p.get("targeted") else "false",
                rs(resolve_markup(p.get("text", ""), False)),
            )
        )
    return "\n".join(rows)


def gen_events(src: str) -> str:
    rows = []
    for e in load(src, "events")["events"]:
        opts = [
            resolve_markup(o.get("label", ""), False)
            for o in e.get("options", [])
        ]
        opt_lit = "&[{}]".format(", ".join(rs(o) for o in opts))
        acts = e.get("acts") or []
        rows.append(
            "    EventInfo {{ id: {}, name: {}, acts: {}, pool: {}, options: {}, text: {} }},".format(
                rs(game_id(e["id"])),
                rs(e["name"]),
                rs(",".join(str(a) for a in acts)),
                rs(e.get("pool", "")),
                opt_lit,
                rs(resolve_markup(e.get("summary", ""), False)),
            )
        )
    return "\n".join(rows)


# 语料里的史莱姆用 _S/_M/_L 表示小/中/大型, 本游戏的敌人 id 写全
SLIME_SIZES = {"_S": "_small", "_M": "_medium", "_L": "_large"}


def monster_id(corpus_id: str) -> str:
    """语料怪物 id 转成本游戏敌人 id: CULTIST -> cultist, ACID_SLIME_S -> acid_slime_small.

    只有史莱姆的体型后缀要展开, 和 enemies.rs 里的 id 对齐.
    """
    s = corpus_id.lower()
    if s.startswith(("acid_slime_", "spike_slime_")):
        for suf, full in SLIME_SIZES.items():
            if s.endswith(suf.lower()):
                s = s[: -len(suf)] + full
                break
    return s


def gen_monsters(src: str) -> str:
    ms = load(src, "monsters-act1") + load(src, "monsters-act2")
    ms += load(src, "monsters-act34")["monsters"]
    rows = []
    for m in ms:
        hp = m.get("hp") or {}
        base = hp.get("base") or [0, 0]
        asc = hp.get("asc") or base
        moves = list((m.get("moves") or {}).keys())
        mv = "&[{}]".format(", ".join(rs(k) for k in moves))
        rows.append(
            "    MonsterInfo {{ id: {}, corpus_id: {}, name: {}, category: {}, acts: {}, "
            "hp_lo: {}, hp_hi: {}, hp_asc_lo: {}, hp_asc_hi: {}, moves: {} }},".format(
                rs(monster_id(m["id"])),
                rs(m["id"]),
                rs(m["name"]),
                rs(m.get("category", "")),
                rs(",".join(str(a) for a in (m.get("acts") or []))),
                int(base[0]),
                int(base[1]),
                int(asc[0]),
                int(asc[1]),
                mv,
            )
        )
    return "\n".join(rows)


def gen_characters(src: str) -> str:
    rows = []
    for c in load(src, "characters"):
        deck = ", ".join(
            "({}, {})".format(rs(game_id(d["card"])), int(d.get("count", 1)))
            for d in c.get("startingDeck", [])
        )
        rel = (c.get("startingRelic") or {}).get("id", "")
        rows.append(
            "    CharacterInfo {{ id: {}, color: {}, name: {}, max_hp: {}, gold: {}, "
            "orb_slots: {}, relic: {}, relic_name: {}, deck: &[{}] }},".format(
                rs(game_id(c["id"])),
                rs(c.get("color", "")),
                rs(c["name"]),
                int(c.get("maxHp", 0)),
                int(c.get("startingGold", 0)),
                int(c.get("orbSlots", 0)),
                rs(game_id(rel) if rel else ""),
                rs((c.get("startingRelic") or {}).get("name", "")),
                deck,
            )
        )
    return "\n".join(rows)


HEADER = '''#![allow(dead_code)]
// 全量语料(卡牌/遗物/药水/事件/怪物/角色), 由 tools/gen_corpus.py 从
// refs/slay-the-cli/data/corpus/*.json 生成, 不要手改.
// 这里只有展示数据; 能不能打/能不能用由 cards.rs relics.rs potions.rs events.rs 决定.

pub struct CardInfo {
    /// 本游戏的 id(语料 id 去掉 _RED/_GREEN/_BLUE/_PURPLE 后缀并转小写)
    pub id: &\'static str,
    pub corpus_id: &\'static str,
    pub name: &\'static str,
    /// red / green / blue / purple / colorless / curse ...
    pub color: &\'static str,
    /// attack / skill / power / status / curse
    pub kind: &\'static str,
    pub rarity: &\'static str,
    /// class / colorless / curse / basic / special
    pub pool: &\'static str,
    pub cost: &\'static str,
    pub cost_up: &\'static str,
    pub target: &\'static str,
    pub text: &\'static str,
    pub text_up: &\'static str,
}

pub struct RelicInfo {
    pub id: &\'static str,
    pub corpus_id: &\'static str,
    pub name: &\'static str,
    pub tier: &\'static str,
    pub pool: &\'static str,
    pub text: &\'static str,
}

pub struct PotionInfo {
    pub id: &\'static str,
    pub corpus_id: &\'static str,
    pub name: &\'static str,
    pub color: &\'static str,
    pub rarity: &\'static str,
    pub targeted: bool,
    pub text: &\'static str,
}

pub struct EventInfo {
    pub id: &\'static str,
    pub name: &\'static str,
    pub acts: &\'static str,
    pub pool: &\'static str,
    pub options: &\'static [&\'static str],
    pub text: &\'static str,
}

pub struct MonsterInfo {
    /// 本游戏的敌人 id(语料 id 转小写, 史莱姆的 _S/_M/_L 展开成 _small/_medium/_large)
    pub id: &\'static str,
    pub corpus_id: &\'static str,
    pub name: &\'static str,
    /// normal / elite / boss / minion / event
    pub category: &\'static str,
    /// 出现的层, 逗号分隔(如 "1,2,3")
    pub acts: &\'static str,
    pub hp_lo: i32,
    pub hp_hi: i32,
    pub hp_asc_lo: i32,
    pub hp_asc_hi: i32,
    /// 招式 id(名字里带怪物 id 前缀, 展示时再裁)
    pub moves: &\'static [&\'static str],
}

pub struct CharacterInfo {
    pub id: &\'static str,
    /// red / green / blue / purple
    pub color: &\'static str,
    pub name: &\'static str,
    pub max_hp: i32,
    pub gold: i32,
    pub orb_slots: i32,
    pub relic: &\'static str,
    pub relic_name: &\'static str,
    /// 起始牌组: (本游戏卡牌 id, 张数)
    pub deck: &\'static [(&\'static str, u8)],
}

'''


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SRC
    out = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_OUT
    parts = [HEADER]
    for name, ty, fn in [
        ("cards", "CardInfo", gen_cards),
        ("relics", "RelicInfo", gen_relics),
        ("potions", "PotionInfo", gen_potions),
        ("events", "EventInfo", gen_events),
        ("monsters", "MonsterInfo", gen_monsters),
        ("characters", "CharacterInfo", gen_characters),
    ]:
        body = fn(src)
        n = body.count("\n") + 1
        parts.append(f"pub static {name.upper()}: &[{ty}] = &[\n{body}\n];\n")
        print(f"{name}: {n} 条", file=sys.stderr)
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(parts))
    print(f"wrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
