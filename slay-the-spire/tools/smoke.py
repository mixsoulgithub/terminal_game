#!/usr/bin/env python3
"""用 tmux 驱动真实二进制跑一整局:每个界面都进一遍,确认不会卡死。

用法:
    python3 tools/smoke.py [二进制路径] [种子...]

它读取底栏的 `-- SCREEN --` 判断当前界面,然后发对应的按键,最后要求这一局
落到 VICTORY 或 DEATH(不做策略,死了也算通过:重点是流程不能卡住)。
"""
import re
import subprocess
import sys
import time

SESSION = "spire_smoke"


def tmux(*args: str) -> str:
    out = subprocess.run(["tmux", *args], capture_output=True, text=True)
    return out.stdout


def start(binary: str, seed: int) -> None:
    stop()
    subprocess.run(
        ["tmux", "new-session", "-d", "-s", SESSION, "-x", "110", "-y", "34",
         f"{binary} --seed {seed}"],
        check=True,
    )
    time.sleep(0.9)


def stop() -> None:
    subprocess.run(["tmux", "kill-session", "-t", SESSION], capture_output=True)


def screen() -> str:
    return tmux("capture-pane", "-t", SESSION, "-p")


def send(*keys: str) -> None:
    subprocess.run(["tmux", "send-keys", "-t", SESSION, *keys], check=True)
    time.sleep(0.28)


CARD_RE = re.compile(r"\|\s*(\d+)\.\s+([^|]*?)\s*\|")
TYPE_RE = re.compile(r"\|\s*(Attack|Skill|Power|Status|Curse)\s+c\S*")


def hand_attacks(text: str) -> list[str]:
    """从战斗界面里读出每张手牌的序号与类型,返回攻击牌的按键序列。

    手牌一行里并排放着好几张卡,所以名字行与类型行都要整行取全部匹配,
    再按位置配对。
    """
    slot_re = re.compile(r"\|\s*(\d+)\.\s+([^|]*?)\s*\|")
    type_re = re.compile(r"\|\s*(Attack|Skill|Power|Status|Curse)\s+c\S*")
    slots: list[int] = []
    kinds: list[str] = []
    for line in text.splitlines():
        slots += [int(m.group(1)) for m in slot_re.finditer(line)]
        kinds += [m.group(1) for m in type_re.finditer(line)]
    return [
        "0" if slot == 10 else str(slot)
        for slot, kind in zip(slots, kinds)
        if kind == "Attack"
    ]


def current(screen_text: str) -> str:
    for name in ("VICTORY", "DEATH", "COMBAT", "REWARD", "SHOP", "REST", "EVENT",
                 "TREASURE", "PICK", "MAP"):
        if f"-- {name} --" in screen_text:
            return name
    return "?"


def play(binary: str, seed: int, steps: int = 120) -> tuple[str, set[str], str]:
    start(binary, seed)
    seen: set[str] = set()
    text = screen()
    if "M monster" not in text:
        raise AssertionError(f"seed {seed}: 地图没有图例,首屏如下:\n{text}")
    seen.add("MAP")
    for _ in range(steps):
        text = screen()
        where = current(text)
        seen.add(where)
        if where in ("VICTORY", "DEATH"):
            stop()
            return where, seen, text
        if where == "MAP":
            send("Enter")
        elif where == "COMBAT":
            # 从屏幕里读出攻击牌,最多打三张(正好 3 点能量),然后结束回合
            keys = hand_attacks(text)[:3]
            send(*(keys + ["e"]))
        elif where == "REWARD":
            send("Enter", "Enter", "c", "Escape")
        elif where == "REST":
            send("Enter")
        elif where == "SHOP":
            send("Escape")
        elif where == "EVENT":
            send("Enter")
        elif where == "TREASURE":
            send("Enter")
        elif where == "PICK":
            send("Enter")
        else:
            raise AssertionError(f"seed {seed}: 认不出界面,屏幕如下:\n{text}")
    stop()
    raise AssertionError(f"seed {seed}: {steps} 步之内没有结束,卡在 {current(screen())}")


def main() -> int:
    binary = sys.argv[1] if len(sys.argv) > 1 else "./target/debug/spire"
    seeds = [int(a) for a in sys.argv[2:]] or [7, 42]
    ok = True
    for seed in seeds:
        try:
            where, seen, text = play(binary, seed)
        except AssertionError as e:
            print(f"FAIL seed {seed}: {e}")
            ok = False
            continue
        if "REWARD" not in seen:
            print(f"FAIL seed {seed}: 整局没出现过奖励界面")
            ok = False
            continue
        print(f"ok seed {seed}: 结局 {where},走过的界面 {sorted(seen)}")
        print("   最后一行:", [l for l in text.splitlines() if l.strip()][-1][:100])
    print("smoke:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
