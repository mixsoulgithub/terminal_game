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


DETAIL_RE = re.compile(r"^(\S[^|]*?)\s{2,}(Attack|Skill|Power|Status|Curse)\s{2,}cost\s+(\S+)\s*$")


def selected_kind(text: str) -> str | None:
    """从手牌区右侧的详情行里读出选中那张牌的类型(找不到就返回 None)。"""
    for line in text.splitlines():
        m = DETAIL_RE.match(line.strip())
        if m:
            return m.group(2)
    return None


def play_one_turn(text: str) -> None:
    """一次只做一件事:选中的是攻击牌就打出去,否则往右挪一格。

    调用方每次都要重新抓屏(打出一张牌后手牌会左移)。
    """
    kind = selected_kind(text)
    if kind == "Attack":
        send("Enter")
    else:
        send("l")


def current(screen_text: str) -> str:
    for name in ("VICTORY", "DEATH", "COMBAT", "REWARD", "SHOP", "REST", "EVENT",
                 "TREASURE", "PICK", "MAP"):
        if f"-- {name} --" in screen_text:
            return name
    return "?"


def wait_for_map(timeout: float = 8.0) -> str:
    """等第一帧画出来:启动瞬间抓屏可能抓到空屏或半帧。"""
    deadline = time.time() + timeout
    text = screen()
    while "M monster" not in text and time.time() < deadline:
        time.sleep(0.2)
        text = screen()
    return text


def play(binary: str, seed: int, steps: int = 220) -> tuple[str, set[str], str]:
    start(binary, seed)
    seen: set[str] = set()
    text = wait_for_map()
    if "M monster" not in text:
        raise AssertionError(f"seed {seed}: 地图没有图例,首屏如下:\n{text}")
    seen.add("MAP")
    # 战斗里的节奏:打最多 4 张攻击牌,找不到攻击牌连续挪 5 次就结束回合
    plays = 0
    moves = 0
    checked_history = False
    for _ in range(steps):
        text = screen()
        where = current(text)
        seen.add(where)
        if where in ("VICTORY", "DEATH"):
            stop()
            return where, seen, text
        if where != "COMBAT":
            plays = moves = 0
        if where == "MAP":
            # 打第一场之后翻一次历史记录,确认叠加层开了也能正常关掉
            if not checked_history and seen >= {"MAP", "COMBAT", "REWARD"}:
                send("H")
                hist = screen()
                # 历史记录默认停在最新一条,所以用标题而不是"条目数"那一行来判断
                if "history  (j/k scroll, H or esc close)" not in hist:
                    raise AssertionError(f"seed {seed}: H 没打开历史记录:\n{hist}")
                send("Escape")
                checked_history = True
            send("Enter")
        elif where == "COMBAT":
            kind = selected_kind(text)
            if kind == "Attack" and plays < 4:
                send("Enter")
                plays += 1
                moves = 0
            elif moves >= 5:
                send("e")
                plays = moves = 0
            else:
                send("l")
                moves += 1
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
