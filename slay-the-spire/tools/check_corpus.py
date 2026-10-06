#!/usr/bin/env python3
"""把生成的 src/core/corpus.rs 和参考实现自己的解析结果逐字对账。

参考那边用 refs/slay-the-cli 的 markup 解析器(tools/corpus_check.ts, bun 跑),
这里比 id / 名字 / 解析后的正文(含升级后),任何一条对不上就报出来。

用法:
    python3 tools/check_corpus.py            # 自动跑 bun 取参考结果
    python3 tools/check_corpus.py ref.tsv    # 用现成的 TSV
"""
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
GAME = os.path.abspath(os.path.join(HERE, ".."))

FIELD_RE = re.compile(r'(\w+): "((?:[^"\\]|\\.)*)"')


def unescape(s: str) -> str:
    out = []
    i = 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            if n == "n":
                out.append("\n")
                i += 2
                continue
            if n == "t":
                out.append("\t")
                i += 2
                continue
            if n == '"':
                out.append('"')
                i += 2
                continue
            if n == "\\":
                out.append("\\")
                i += 2
                continue
            if n == "u" and s[i + 2] == "{":
                end = s.index("}", i + 3)
                out.append(chr(int(s[i + 3 : end], 16)))
                i = end + 1
                continue
        out.append(c)
        i += 1
    return "".join(out)


def parse_corpus(path: str):
    """-> {"card": {corpus_id: {field: value}}, "relic": ..., "potion": ...}"""
    out = {"card": {}, "relic": {}, "potion": {}}
    kind = None
    for line in open(path, encoding="utf-8"):
        m = re.match(r"pub static (\w+):", line)
        if m:
            name = m.group(1)
            kind = {
                "CARDS": "card",
                "RELICS": "relic",
                "POTIONS": "potion",
            }.get(name)
            continue
        if kind is None or "{" not in line:
            continue
        fields = {k: unescape(v) for k, v in FIELD_RE.findall(line)}
        if "corpus_id" in fields:
            out[kind][fields["corpus_id"]] = fields
    return out


def reference(tsv: str):
    out = {"card": {}, "relic": {}, "potion": {}}
    for line in open(tsv, encoding="utf-8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 6:
            continue
        kind, cid, name, base, up, meta = parts[:6]
        out.setdefault(kind, {})[cid] = {
            "name": name,
            "base": base.replace("\\n", "\n"),
            "up": up.replace("\\n", "\n"),
            "meta": json.loads(meta),
        }
    return out


def cost_text(c) -> str:
    """语料里的费用 -> 图鉴里显示的字: -1 = X, -2 = 不能打, 其余原样"""
    if c is None:
        return "-"
    if isinstance(c, str):
        return c
    n = int(c)
    if n == -1:
        return "X"
    if n < 0:
        return "-"
    return str(n)


def main() -> int:
    if len(sys.argv) > 1:
        tsv = sys.argv[1]
    else:
        tsv = "/tmp/corpus_ref.tsv"
        r = subprocess.run(
            ["bun", "run", os.path.join(HERE, "corpus_check.ts")],
            cwd=os.path.join(GAME, ".."),
            capture_output=True,
            text=True,
        )
        if r.returncode != 0:
            print("bun 跑参考解析器失败:", r.stderr[-500:])
            return 2
        open(tsv, "w", encoding="utf-8").write(r.stdout)

    mine = parse_corpus(os.path.join(GAME, "src/core/corpus.rs"))
    ref = reference(tsv)
    bad = 0
    checked = 0
    for kind in ("card", "relic", "potion"):
        for cid, r in ref[kind].items():
            m = mine[kind].get(cid)
            checked += 1
            if m is None:
                print(f"缺条目 {kind} {cid}")
                bad += 1
                continue
            if m["name"] != r["name"]:
                print(f"名字不同 {kind} {cid}: 我 {m['name']!r} / 参考 {r['name']!r}")
                bad += 1
            if m["text"] != r["base"]:
                print(f"正文不同 {kind} {cid}:\n  我   {m['text']!r}\n  参考 {r['base']!r}")
                bad += 1
            if kind == "card" and m.get("text_up", "") != r["up"]:
                print(f"升级正文不同 {kind} {cid}:\n  我   {m.get('text_up')!r}\n  参考 {r['up']!r}")
                bad += 1
            # 其余字段逐项对: cost 是唯一要换算的
            want = dict(r["meta"])
            if kind == "card":
                want["cost"] = cost_text(want.get("cost"))
                want["cost_up"] = cost_text(want.pop("costUp", None))
            # Rust 那边的字段名: 卡片的 type 叫 kind, 药水的 class 叫 color
            alias = {
                "card": {"type": "kind", "costUp": "cost_up"},
                "relic": {},
                "potion": {"class": "color"},
            }[kind]
            got = {}
            for key in want:
                got[key] = m.get(alias.get(key, key), "")
            for key, wv in want.items():
                if got[key] != wv:
                    print(f"字段不同 {kind} {cid} {key}: 我 {got[key]!r} / 参考 {wv!r}")
                    bad += 1
    print(f"checked {checked} 条, 不一致 {bad} 条")
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
