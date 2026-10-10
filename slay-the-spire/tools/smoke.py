#!/usr/bin/env python3
"""用 tmux 驱动真实二进制跑一整局:每个界面都进一遍,确认不会卡死。

用法:
    python3 tools/smoke.py [二进制路径] [种子...]
    python3 tools/smoke.py 7 42     # 不给二进制就用 ./target/debug/spire

它读取底栏的 `-- SCREEN --` 判断当前界面,然后发对应的按键,最后要求这一局
落到 VICTORY 或 DEATH(不做策略,死了也算通过:重点是流程不能卡住)。
"""
import os
import re
import subprocess
import sys
import time

SESSION = "spire_smoke"
# 按键之后最短等多久再抓屏(等界面稳定下来用的采样间隔)
SAMPLE = 0.03


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


KEYS_SENT = 0


def send(*keys: str) -> None:
    """发一次键,然后等到界面稳定再返回。

    原来是无脑 sleep 0.28s,一局要按两百次键,光等就一分钟;
    现在改成每 50ms 抓一次屏,连续两次一样就当画完了(不动的键大约 0.1s 就走人),
    最慢 2s 兜底。
    """
    global KEYS_SENT
    KEYS_SENT += len(keys)
    subprocess.run(["tmux", "send-keys", "-t", SESSION, *keys], check=True)
    # 至少给游戏一帧的时间,然后等画面稳定(不再无脑 0.28s)
    time.sleep(0.03)
    deadline = time.time() + 2.0
    prev = screen()
    settled = 0
    while time.time() < deadline:
        time.sleep(SAMPLE)
        cur = screen()
        settled = settled + 1 if cur == prev else 0
        prev = cur
        if settled >= 3:
            return


CARD_RE = re.compile(r"\|\s*(\d+)\.\s+([^|]*?)\s*\|")
TYPE_RE = re.compile(r"\|\s*(Attack|Skill|Power|Status|Curse)\s+c\S*")


# 战斗界面里选中的牌的类型写成 <Attack> / [Skill] / (Power)
KIND_RE = re.compile(r"[<\[(](Attack|Skill|Power|Status|Curse)[>\])]")
ENERGY_RE = re.compile(r"\(\d+\)/\(\d+\) energy")

# 战斗里挂起的选牌窗口(从弃牌/消耗/抽牌堆挑牌,或从手牌挑):
# 信息行会写"it applies right away",从牌堆挑时窗口标题还会带"j/k pick, space or enter take".
# 手牌来源没有窗口,信息行的措辞是"space to choose"(所以要靠它区分按键).
CHOICE_RE = re.compile(r"it applies right away|j/k pick, space or enter take")
HAND_CHOICE = "space to choose"


def selected_kind(text: str) -> str | None:
    """从说明区那一行读出选中那张牌的类型(找不到就返回 None)。"""
    for line in text.splitlines():
        m = KIND_RE.search(line)
        if m:
            return m.group(1)
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
    for name in ("VICTORY", "DEATH", "REWARD", "SHOP", "REST", "EVENT",
                 "TREASURE", "PICK", "MAP"):
        if f"-- {name} --" in screen_text:
            return name
    # 战斗里挂起的选牌:窗口把能量行盖住了,认不出来就会误判成"?".
    # 放在能量行前面,因为它优先级更高(手牌来源没有窗口、能量行还在).
    if CHOICE_RE.search(screen_text):
        return "CHOICE"
    # 战斗界面自带信息行和命令栏,没有 -- COMBAT -- 标记,靠能量行认
    if ENERGY_RE.search(screen_text):
        return "COMBAT"
    return "?"


def map_legend_present(text: str) -> bool:
    """地图右上角的图例画出来了没(当前渲染:每个符号一行,竖排在右侧)。

    不看底栏的 `-- MAP --`(别处也可能出现),而是认图例里现在真实存在的两行:
    当前位置的 "[] you" 与燃烧精英的 "E  Burning"(宽度不够时图例退化成
    底栏一行,那两行就对不上,也算没到位)。
    """
    return "[] you" in text and re.search(r"E\s+Burning", text) is not None


def enter_the_game() -> None:
    """从开始界面进一局:new game -> 第一个角色 -> Neow 的第一个祝福。

    抓屏偶尔会抓到半帧,所以认不出界面时不要直接返回,歇一下重来。
    Neow 的祝福有的会开奖励屏(三张牌)或选牌屏(升级/移除/变形),
    这些也要按掉才能走到地图。
    """
    for step in range(80):
        text = screen()
        if os.environ.get("SMOKE_DEBUG"):
            head = [l for l in text.splitlines() if l.strip()][:2]
            print(f"  [enter {step}] {head}", file=sys.stderr)
        if map_legend_present(text):
            return
        if "slay the spire" in text and "new game" in text:
            send("j")          # 光标默认停在 continue,挪到 new game
            send("Enter")
        elif "choose a character" in text:
            send("Enter")      # 第一个角色
        elif "Neow's Blessing" in text:
            send("Enter")      # 第一个祝福
        elif "press enter" in text:
            send("Enter")
        elif current(text) == "REWARD":
            send("Escape")     # Neow 发的牌可跳过(esc leave),跳过后回地图
        elif current(text) == "PICK":
            send("Enter")      # 确认选中的那张(两选的会再进一次)
        else:
            time.sleep(0.2)


def wait_for_map(timeout: float = 8.0) -> str:
    """等第一帧画出来:启动瞬间抓屏可能抓到空屏或半帧。"""
    deadline = time.time() + timeout
    text = screen()
    while not map_legend_present(text) and time.time() < deadline:
        time.sleep(0.2)
        text = screen()
    return text


def play(binary: str, seed: int, steps: int = 800) -> tuple[str, set[str], str]:
    start(binary, seed)
    enter_the_game()
    seen: set[str] = set()
    text = wait_for_map()
    if not map_legend_present(text):
        raise AssertionError(f"seed {seed}: 地图没有图例,首屏如下:\n{text}")
    seen.add("MAP")
    # 战斗里的节奏:打最多 4 张攻击牌,找不到攻击牌连续挪 5 次就结束回合
    plays = 0
    moves = 0
    checked_history = False
    for _ in range(steps):
        text = screen()
        where = current(text)
        if where == "?":
            # 抓屏有概率抓到半帧,重抓几次再判定
            for _ in range(8):
                time.sleep(0.15)
                text = screen()
                where = current(text)
                if where != "?":
                    break
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
        elif where == "CHOICE":
            # 战斗里挂起的选牌.从牌堆挑:回车就是"选中光标下那张"(窗口已自动打开、
            # 光标在第 0 个).从手牌挑:先空格选中(选够张数会立刻生效),再回车确认
            # (不限张数的那种要回车收工).
            if HAND_CHOICE in text:
                send(" ", "Enter")
            else:
                send("Enter")
        elif where == "REWARD":
            send("Enter", "Enter", "c", "Escape")
        elif where == "REST":
            send("Enter")
        elif where == "SHOP":
            send("Escape")
        elif where == "EVENT":
            # 翻牌小游戏(match_and_keep):已经翻开的/本次刚翻的那格不能再选,
            # 光标停原地时按回车只会报 "that card can not be flipped".
            # 先用 j 挪到下一格再回车;光标是循环的,一格一格往下走正好凑齐 5 次尝试.
            if "Flip card" in text:
                send("j", "Enter")
            else:
                send("Enter")
        elif where == "TREASURE":
            send("Enter")
        elif where == "PICK":
            send("Enter")
        else:
            raise AssertionError(f"seed {seed}: 认不出界面,屏幕如下:\n{text}")
    # 先抓屏再杀会话:stop() 之后 tmux 已经没了,再抓就是空的
    stuck = screen()
    stop()
    raise AssertionError(
        f"seed {seed}: {steps} 步之内没有结束,卡在 {current(stuck)},屏幕如下:\n{stuck}"
    )


def main() -> int:
    args = sys.argv[1:]
    # 第一个参数全是数字就当它是种子(留出 `smoke.py 7` 这种简写),
    # 否则按 [二进制路径] [种子...] 解析.
    if args and args[0].isdigit():
        binary = "./target/debug/spire"
        seeds = [int(a) for a in args]
    else:
        binary = args[0] if args else "./target/debug/spire"
        seeds = [int(a) for a in args[1:]]
    seeds = seeds or [7, 42]
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
        print(
            f"ok seed {seed}: 结局 {where},走过的界面 {sorted(seen)},"
            f"按了 {KEYS_SENT} 次键"
        )
        print("   最后一行:", [l for l in text.splitlines() if l.strip()][-1][:100])
    print("smoke:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
