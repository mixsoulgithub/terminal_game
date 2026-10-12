#!/usr/bin/env python3
"""从 tools/import_official.py 抽出的中间语料生成 src/core/corpus.rs.

来源(权威顺序): 原版游戏 desktop-1.0.jar 的官方 localization + CFR 反编译出的类.
中间文件默认在 ~/projects/sts-golden/out/corpus_src.json(游戏文件不进仓库).

设计:
  - id / corpus_id 完全沿用现有 corpus.rs 里的值, 只刷新文案与可核对的元数据.
    这样 compendium / 各处按 id 查语料的地方不受影响.
  - 中间语料里找不到的条目(改名卡/内部能力/已删内容)保留旧文案, 并在报告里列出.
  - EVENTS / MONSTERS / CHARACTERS 三段暂时原样保留(官方事件文案是按阶段分片的,
    要按事件类的代码拼, 还没做).

用法:
  python3 tools/gen_corpus.py [--src 中间json] [--out corpus.rs] [--report]
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SRC = os.path.expanduser("~/projects/sts-golden/out/corpus_src.json")
DEFAULT_OUT = os.path.join(HERE, "..", "src", "core", "corpus.rs")

# struct 字段顺序(与现有文件一致)
FIELDS = {
    "CardInfo": ["id", "corpus_id", "name", "color", "kind", "rarity", "pool", "cost", "cost_up", "target", "text", "text_up"],
    "RelicInfo": ["id", "corpus_id", "name", "tier", "pool", "text"],
    "PotionInfo": ["id", "corpus_id", "name", "color", "rarity", "targeted", "text"],
    "PowerInfo": ["id", "corpus_id", "name", "kind", "text"],
}

# 各段起止标记
SECTIONS = ["CARDS", "POWERS", "RELICS", "POTIONS"]


def rs(s: str) -> str:
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def split_top(s: str):
    """按顶层逗号切分(忽略引号/括号内部)."""
    out, cur, depth, instr = [], "", 0, False
    i = 0
    while i < len(s):
        ch = s[i]
        if instr:
            cur += ch
            if ch == "\\":
                cur += s[i + 1]
                i += 2
                continue
            if ch == '"':
                instr = False
        elif ch == '"':
            instr = True
            cur += ch
        elif ch in "[{(":
            depth += 1
            cur += ch
        elif ch in "]})":
            depth -= 1
            cur += ch
        elif ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
        i += 1
    if cur.strip():
        out.append(cur.strip())
    return out


def parse_rows(text: str):
    """把一段 `Type { ... },` 解析成 [dict], 顺序保留(每行一条, 末行没有换行也要收)."""
    rows = []
    for line in text.split("\n"):
        m = re.match(r"\s*(\w+Info) \{ (.*) \},?$", line.rstrip())
        if not m:
            continue
        fields = {}
        for kv in split_top(m.group(2)):
            k, _, v = kv.partition(":")
            fields[k.strip()] = v.strip()
        rows.append((m.group(1), fields))
    return rows


def nk(s: str) -> str:
    return re.sub(r"[^a-z0-9]", "", s.lower())


SUF = {"RED": "R", "GREEN": "G", "BLUE": "B", "PURPLE": "P"}


def corpus_key(cid: str) -> str:
    """语料 id(如 STRIKE_RED) -> 官方键的归一形式(如 striker)."""
    s = cid.lower()
    for k, v in SUF.items():
        if s.endswith("_" + k.lower()):
            s = s[: -len(k) - 1] + "_" + v
    return nk(s)


def load_src(path):
    if not os.path.exists(path):
        sys.exit(
            "缺少中间语料 %s\n先跑: python3 tools/import_official.py (需要 ~/projects/sts-golden 里的游戏文件)" % path
        )
    d = json.load(open(path, encoding="utf-8"))
    out = {}
    for name in ("cards", "relics", "powers", "potions", "events", "monsters"):
        by_key, by_name = {}, {}
        for e in d[name]:
            by_key.setdefault(nk(e["key"]), e)
            by_name.setdefault(nk(e.get("name", "")), e)
        out[name] = (by_key, by_name)
    return out


def join(entry_lists, fields):
    """按 corpus_id 或旧显示名找对应的官方条目."""
    by_key, by_name = entry_lists
    cid = unquote(fields["corpus_id"])
    e = by_key.get(corpus_key(cid))
    if e:
        return e
    return by_name.get(nk(unquote(fields["name"])))


def unquote(v: str) -> str:
    v = v.strip()
    if v.startswith('"'):
        return json.loads(v)
    return v


def emit(name, fields):
    parts = []
    for f in FIELDS[name]:
        parts.append("%s: %s" % (f, fields[f]))
    return "    %s { %s }," % (name, ", ".join(parts))


def main():
    args = sys.argv[1:]
    report_only = "--report" in args
    src = DEFAULT_SRC
    out = DEFAULT_OUT
    if "--src" in args:
        src = os.path.expanduser(args[args.index("--src") + 1])
    if "--out" in args:
        out = os.path.expanduser(args[args.index("--out") + 1])
    data = load_src(src)
    cur = open(DEFAULT_OUT, encoding="utf-8").read()
    # 定位各段
    spans = {}
    TYPE_OF = {s: s[:-1].title() + "Info" for s in SECTIONS + ["EVENTS", "MONSTERS", "CHARACTERS"]}
    for s in SECTIONS + ["EVENTS", "MONSTERS", "CHARACTERS"]:
        m = re.search(r"pub static %s: &\[%s\] = &\[\n(.*?)\n\];" % (s, TYPE_OF[s]), cur, re.S)
        if m:
            spans[s] = m.span(1)
    missing_sections = [s for s in SECTIONS + ["EVENTS", "MONSTERS", "CHARACTERS"] if s not in spans]
    if missing_sections:
        sys.exit("找不到这些段: %s" % missing_sections)

    stats = {}
    new_sections = {}
    # 事件: 只对齐官方标题, 机制摘要/选项保持我们自己写的
    ev_by_key, ev_by_name = data.get("events", ({}, {}))
    ev = {k: v["name"] for k, v in ev_by_key.items()}
    evn = {k: v["name"] for k, v in ev_by_name.items()}
    # 怪物: 名字/常规血量/飞升档血量对齐官方(招式的名字与效果本作自己有一套, 不动)
    mo_by_key, mo_by_name = data.get("monsters", ({}, {}))
    seg = cur[spans["MONSTERS"][0] : spans["MONSTERS"][1]].rstrip("\n")
    mlines, mupd, mmiss = [], [], []
    for line in seg.split("\n"):
        m = re.match(r'\s*MonsterInfo \{ id: "([^"]+)", corpus_id: "([^"]+)", name: "([^"]+)"', line)
        if not m:
            mlines.append(line)
            continue
        e = mo_by_key.get(corpus_key(m.group(2))) or mo_by_name.get(nk(m.group(3))) or mo_by_key.get(nk(m.group(1)))
        if not e:
            mmiss.append(m.group(1))
            mlines.append(line)
            continue
        asc = e["asc"][0] if e["asc"] else None
        fields = dict(re.findall(r'(\w+): ("(?:[^"\\]|\\.)*"|&\[[^\]]*\]|-?\d+)', line))
        if not fields:
            mlines.append(line)
            continue
        fields["name"] = rs(e["name"])
        fields["hp_lo"] = str(e["hp"][0])
        fields["hp_hi"] = str(e["hp"][1])
        fields["hp_asc_lo"] = str(asc[1] if asc else e["hp"][0])
        fields["hp_asc_hi"] = str(asc[2] if asc else e["hp"][1])
        keys = ["id", "corpus_id", "name", "category", "acts", "hp_lo", "hp_hi", "hp_asc_lo", "hp_asc_hi", "moves"]
        if all(k in fields for k in keys):
            before = line
            line = "    MonsterInfo { %s }," % ", ".join(
                "%s: %s" % (k, fields[k]) for k in keys
            )
            if line != before:
                mupd.append(m.group(1))
        mlines.append(line)
    new_sections["MONSTERS"] = "\n".join(mlines)
    print("MONSTERS %3d 行 | 更新 %d | 官方无 %d" % (len(mlines), len(mupd), len(mmiss)), file=sys.stderr)

    ev_seg = cur[spans["EVENTS"][0] : spans["EVENTS"][1]].rstrip("\n")
    lines, renamed, evmiss = [], [], []
    for line in ev_seg.split("\n"):
        m = re.match(r'\s*EventInfo \{ id: "([^"]+)", name: "([^"]+)"(.*)', line, re.S)
        if not m:
            lines.append(line)
            continue
        nk_id = re.sub(r"[^a-z0-9]", "", m.group(1).lower())
        nk_nm = re.sub(r"[^a-z0-9]", "", m.group(2).lower())
        new_name = ev.get(nk_id) or evn.get(nk_nm)
        if not new_name:
            evmiss.append(m.group(1))
            lines.append(line)
            continue
        if new_name != m.group(2):
            renamed.append((m.group(1), m.group(2), new_name))
        lines.append('    EventInfo { id: "%s", name: %s%s' % (m.group(1), rs(new_name), m.group(3)))
    new_sections["EVENTS"] = "\n".join(lines)
    print("EVENTS   %3d 行 | 名字对齐 %d %s | 官方无 %d %s" % (
        len(lines) and sum(1 for l in lines if "EventInfo {" in l), len(renamed),
        ("(" + ", ".join("%s -> %s" % (a, c) for a, b, c in renamed) + ")") if renamed else "",
        len(evmiss), ("(" + ", ".join(evmiss) + ")") if evmiss else ""), file=sys.stderr)
    for sec, srcname, type_ in (
        ("CARDS", "cards", "CardInfo"),
        ("POWERS", "powers", "PowerInfo"),
        ("RELICS", "relics", "RelicInfo"),
        ("POTIONS", "potions", "PotionInfo"),
    ):
        seg = cur[spans[sec][0] : spans[sec][1]]
        rows = parse_rows(seg)
        kept, refreshed, miss = 0, 0, []
        lines = []
        for typename, fields in rows:
            assert typename == type_, (typename, type_)
            e = join(data[srcname], fields)
            if not e:
                miss.append(unquote(fields["name"]))
                lines.append(emit(typename, fields))
                kept += 1
                continue
            before = dict(fields)
            if type_ == "CardInfo":
                fields["name"] = rs(e["name"])
                fields["kind"] = rs(e["kind"])
                fields["rarity"] = rs(e["rarity"])
                fields["color"] = rs(e["color"])
                fields["target"] = rs(e["target"])
                fields["cost"] = rs(e["cost"])
                fields["cost_up"] = rs(e["cost_up"])
                fields["text"] = rs(e["text"])
                fields["text_up"] = rs(e["text_up"])
            elif type_ == "RelicInfo":
                fields["name"] = rs(e["name"])
                fields["text"] = rs(e["text"])
            elif type_ == "PowerInfo":
                fields["name"] = rs(e["name"])
                fields["kind"] = rs(e["kind"])
                fields["text"] = rs(e["text"])
            else:
                fields["name"] = rs(e["name"])
                fields["text"] = rs(e["text"])
            if fields != before:
                refreshed += 1
            lines.append(emit(typename, fields))
        stats[sec] = (len(rows), refreshed, miss)
        new_sections[sec] = "\n".join(lines)

    # 报告
    for sec, (n, ch, miss) in stats.items():
        print(
            "%-8s %3d 行 | 更新 %3d | 保留旧文案 %2d%s"
            % (sec, n, ch, len(miss), (" : " + ", ".join(miss[:12])) if miss else ""),
            file=sys.stderr,
        )
    if report_only:
        return
    # 拼回整份文件
    pieces = []
    last = 0
    for sec in SECTIONS + ["EVENTS", "MONSTERS"]:
        a, b = spans[sec]
        pieces.append(cur[last:a])
        pieces.append(new_sections[sec] + "\n")
        last = b
    pieces.append(cur[last:])
    text = "".join(pieces)
    open(out, "w", encoding="utf-8").write(text)
    print("wrote " + out, file=sys.stderr)


if __name__ == "__main__":
    main()
