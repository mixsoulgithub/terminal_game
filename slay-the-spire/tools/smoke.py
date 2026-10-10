#!/usr/bin/env python3
"""用 tmux 驱动真实二进制跑一整局:每个界面都进一遍,确认不会卡死。

用法:
    python3 tools/smoke.py [二进制路径] [种子...]
    python3 tools/smoke.py 7 42          # 不给二进制就用 ./target/debug/spire
    python3 tools/smoke.py 1-30          # 区间写法:一次跑一批种子
    SMOKE_SESSION=batch2 python3 tools/smoke.py 31-40   # 换会话名,可并行跑几批

它读取底栏的 `-- SCREEN --` 判断当前界面,然后发对应的按键,最后要求这一局
落到 VICTORY 或 DEATH(不做策略,死了也算通过:重点是流程不能卡住).
跑完打印逐种子 PASS/FAIL 表(含走过的界面、按键数、耗时)与总耗时.

`--smart`(或 SMOKE_SMART=1)打开"轻量推进策略":不再乱按键,而是按屏幕文本
决策,目标是把一局尽量打赢、推进到第二/三幕(打开后不再是"活得久"):
  - 战斗:对齐 src/core/replay.rs 的 smart_play/pick_smart_card —— 能补刀先攻击,
    要被斩杀或意图总伤超过格挡时先用技能,能力牌尽早铺,其余攻击优先、技能兜底;
    手牌里读不出的"类型"才把光标挪过去从说明区认出来并缓存(按牌名);
    打不出去的牌(状态限制等)自动换成下一张,都不行就结束回合;
    每回合危险时喝一瓶药(指向敌人的补一次回车确认目标).
  - 奖励:金币/遗物/药水直接拿,卡牌按 伤害/格挡+类型 打分挑最好的(诅咒跳过).
  - 地图:采样所有岔路,血少优先营火、金币多优先商店,避开精英,问号/宝箱其次.
  - 营火:血少就休息,否则锻造升级.
  - 商店:买得起的遗物/删牌优先(不买诅咒).
  - 事件/宝箱/选牌:事件按序号找第一个能选的选项(不可用的项会报"不可用",自动跳过);
    翻牌小游戏一格一格翻;宝箱回车拿遗物;选牌确认第一张.
默认不开策略,行为与原来完全一致(20 种子批不受影响).

后几幕的 UI 覆盖(都依赖真实 TUI 驱动,只是用游戏自带的调试命令开路;TUI 没有 :act,
只有 --replay 脚本能直接切幕,所以这里用 :room boss + :win 一幕后一幕后推):
  --act N        先用 `:room boss`+`:win` 把进度推到第 N 幕,再用策略继续打;
  --room <spec>  先 `:room <spec>` 直接进某个房间(如 `event mindbloom`、`boss`、`shop`);
  --win          战斗一律用 `:win` 判定打赢(能顺着看完 Boss 房/双 Boss/奖励那一串 UI);
  --asc N        传 --ascension N(A20 第三幕 Boss 后直接接第二只 Boss:双 Boss).
"""
import atexit
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time

# tmux 会话名与存档目录都必须每次进程独立:否则并行跑 smoke(或 check_all 与手动 smoke
# 同时跑)会互相 kill 会话、互相覆盖 ~/.local/share/slay-the-spire 下的存档与便条。
# 外部显式给了 SMOKE_SESSION 就沿用(留给需要固定名字的场景)。
SESSION = os.environ.get("SMOKE_SESSION") or f"spire_smoke_{os.getpid()}_{secrets.token_hex(3)}"
# 本进程专用的游戏数据目录(存档/便条都落在它下面),跑完删掉。
DATA_DIR = tempfile.mkdtemp(prefix=f"spire_smoke_data_{os.getpid()}_")
atexit.register(shutil.rmtree, DATA_DIR, ignore_errors=True)
# tmux 伪终端的尺寸.默认比真实终端(107x24)大一点,可用 SMOKE_SIZE=WxH 覆盖来测小屏.
SIZE = os.environ.get("SMOKE_SIZE", "110x34")
# 按键之后最短等多久再抓屏(等界面稳定下来用的采样间隔)
SAMPLE = 0.03

# 策略模式:开着一路尽量打赢,目标是推进到第二/三幕.
# 默认关(不设 --smart / SMOKE_SMART 就是原来的"乱按"行为,不能被破坏).
SMART = os.environ.get("SMOKE_SMART", "") not in ("", "0", "no", "false")

# 飞升等级(0-20):非 0 时二进制多带一个 --ascension.默认 0,与原来完全一致.
ASC = int(os.environ.get("SMOKE_ASC", "0") or "0")


def tmux(*args: str) -> str:
    out = subprocess.run(["tmux", *args], capture_output=True, text=True)
    return out.stdout


def start(binary: str, seed: int) -> None:
    stop()
    w, _, h = SIZE.partition("x")
    # XDG_DATA_HOME 指到本进程专属目录:并行跑时各写各的 run.save / note.card,不互相污染.
    cmd = f"env XDG_DATA_HOME={DATA_DIR} {binary} --seed {seed}" + (f" --ascension {ASC}" if ASC else "")
    subprocess.run(
        ["tmux", "new-session", "-d", "-s", SESSION, "-x", w, "-y", h, cmd],
        check=True,
    )
    time.sleep(0.9)


def stop() -> None:
    subprocess.run(["tmux", "kill-session", "-t", SESSION], capture_output=True)


# 进程退出时收掉本进程的 tmux 会话:随机会话名不能被留成僵尸会话。
atexit.register(stop)


def screen() -> str:
    # 抓屏偶尔会抓到空内容(tmux/渲染的瞬时状态):重试几次,别把一次空屏当成"认不出的界面".
    out = ""
    for _ in range(5):
        out = tmux("capture-pane", "-t", SESSION, "-p")
        if out.strip():
            return out
        time.sleep(0.05)
    return out


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


# ==== 策略模式(--smart)====
# 打法对齐 src/core/replay.rs 的 smart_play / pick_smart_card:
#   能补刀就先攻击,要被斩杀或意图总伤超过格挡就先用技能,能力牌尽早铺,其余攻击优先、技能兜底.
# 屏幕文本能直接读出来的东西(血量/格挡/能量/手牌费用/敌人意图)就照读;
# 只读不出来的(每张牌的"类型")才把光标挪过去,从说明区那一行认出来并缓存.

# 顶栏 血量/上限/格挡 与 Act/Floor
TOPBAR_RE = re.compile(r"^\s*(\d+)/(\d+)/(\d+)\b")
ACT_RE = re.compile(r"Act (\d+)\s+Floor (\d+)/")
# 敌人意图里的攻击记号:ATTACK 5 / ATTACK 5x2
ATTACK_RE = re.compile(r"ATTACK (\d+)(?:x(\d+))?")
# 一格手牌的首 token(费用:X / 数字 / -),后面是牌名
CELL_RE = re.compile(r"^(\S+)\s+(.*)$")

# 已知的基础牌(避免每次开局都去读;其余牌运行时学):牌名 -> (类型, 伤害, 格挡, 是否群攻)
CARD_INFO: dict[str, tuple[str, int, int, bool]] = {
    "Strike": ("Attack", 6, 0, False),
    "Defend": ("Skill", 0, 5, False),
    "Bash": ("Attack", 8, 0, False),
}


def parse_top(text: str) -> tuple[int, int, int] | None:
    first = text.splitlines()[0] if text.splitlines() else ""
    m = TOPBAR_RE.match(first)
    return (int(m.group(1)), int(m.group(2)), int(m.group(3))) if m else None


def parse_act_floor(text: str) -> tuple[int, int]:
    m = ACT_RE.search(text)
    return (int(m.group(1)), int(m.group(2))) if m else (0, 0)


def parse_energy(text: str) -> int:
    m = re.search(r"\((\d+)\)/\(\d+\) energy", text)
    return int(m.group(1)) if m else 0


def parse_hand(text: str) -> tuple[list[tuple[str, str]], int]:
    """读出手牌:返回 [(费用字符串, 牌名), ...] 与光标那张的下标.

    手牌在能量行下面那一行(可能折成两行),分隔符是 │,光标那张左右换成 { }.
    """
    lines = text.splitlines()
    hi = None
    for i, l in enumerate(lines):
        if "in hand " in l:
            hi = i
            break
    if hi is None:
        return [], 0
    hand: list[tuple[str, str]] = []
    sel = 0
    for l in lines[hi + 1:]:
        if not l.strip() or l.strip().startswith("─"):
            break
        parts = re.split(r"([{}│])", l)
        for k in range(1, len(parts), 2):
            body = parts[k + 1] if k + 1 < len(parts) else ""
            if not body.strip():
                continue
            m = CELL_RE.match(body.strip())
            if not m:
                continue
            hand.append((m.group(1), m.group(2).strip()))
            if parts[k] == "{":
                sel = len(hand) - 1
    return hand, sel


def selected_info(text: str) -> tuple[str | None, str]:
    """光标那张牌的(类型, 说明文本):说明区那一行带 <Attack>/[Skill]/(Power)."""
    lines = text.splitlines()
    for i, l in enumerate(lines):
        if l.startswith("─") and ") energy" in l and "in hand" not in l:
            m = KIND_RE.search(l)
            desc = []
            j = i + 1
            while j < len(lines) and not lines[j].strip().startswith("─"):
                desc.append(lines[j].strip())
                j += 1
            return (m.group(1) if m else None), " ".join(desc).strip()
    return None, ""


def enemy_hp(text: str) -> list[tuple[int, int, int]]:
    """敌人的 血量/上限/格挡;按屏幕从左到右的顺序(不含顶栏那一行)."""
    out: list[tuple[int, int, int]] = []
    for i, l in enumerate(text.splitlines()):
        if i == 0 or "$" in l or "Act " in l or "in hand" in l:
            continue
        for m in re.finditer(r"(\d+)/(\d+)/(\d+)", l):
            out.append((int(m.group(1)), int(m.group(2)), int(m.group(3))))
    return out


def incoming_damage(text: str) -> int:
    total = 0
    for m in ATTACK_RE.finditer(text):
        total += int(m.group(1)) * int(m.group(2) or 1)
    return total


def parse_card_info(desc: str) -> tuple[str | None, int, int, bool]:
    """从一张牌的说明文本里估出 (类型, 单次伤害x次数, 格挡, 是否群攻)."""
    d = re.search(r"Deal\s+(\d+)\s+damage", desc)
    dmg = int(d.group(1)) if d else 0
    if dmg:
        if "twice" in desc:
            dmg *= 2
        else:
            n = re.search(r"(\d+)\s+times", desc)
            if n:
                dmg *= int(n.group(1))
    b = re.search(r"Gain\s+(\d+)\s+Block", desc)
    blk = int(b.group(1)) if b else 0
    return None, dmg, blk, "ALL enemies" in desc


def ensure_info(text: str, hand: list[tuple[str, str]], sel: int) -> list[tuple[str, int, int, bool]]:
    """保证手牌每张的 (类型, 伤害, 格挡, 群攻) 都在缓存里:缺的就把光标挪过去读说明区."""
    out: list[tuple[str, int, int, bool]] = []
    cur = sel
    for i, (_cost, name) in enumerate(hand):
        if name not in CARD_INFO:
            delta = (i - cur) % len(hand)
            if delta:
                send(*(["l"] * delta))
                text = screen()
                cur = i
            kind, desc = selected_info(text)
            _k, dmg, blk, aoe = parse_card_info(desc)
            CARD_INFO[name] = (kind or "Skill", dmg, blk, aoe)
        out.append(CARD_INFO[name])
    return out


def move_hand_to(target: int, text: str) -> None:
    """把光标挪到第 target 张(按屏幕上的 {} 认出当前位置)."""
    hand, sel = parse_hand(text)
    if not hand:
        return
    target = target % len(hand)
    delta = (target - sel) % len(hand)
    if delta:
        send(*(["l"] * delta))


def occupied_potions(text: str) -> list[int]:
    """顶栏药水区里非空的槽位(空槽画成 ())."""
    first = text.splitlines()[0] if text.splitlines() else ""
    slots = []
    for i, m in enumerate(re.finditer(r"\(([^)]*)\)", first)):
        if m.group(1).strip():
            slots.append(i)
    return slots


def available_potion_slot(text: str) -> bool:
    first = text.splitlines()[0] if text.splitlines() else ""
    return "()" in first


def combat_action(text: str) -> None:
    """战斗里做一步(打一张牌/结束回合),规则对齐 replay 的 pick_smart_card."""
    energy = parse_energy(text)
    hand, sel = parse_hand(text)
    if not hand:
        send("e")
        return
    top = parse_top(text)
    hp, max_hp, block = top if top else (0, 1, 0)
    foes = [(h, m) for (h, m, _b) in enemy_hp(text) if h > 0]
    if not foes:
        time.sleep(0.2)  # 已经赢了,等结算屏
        return
    incoming = incoming_damage(text)
    about_to_die = incoming >= hp + block
    threatened = incoming > block
    min_hp = min(h for (h, _m) in foes)
    needed = max(0, incoming - block)
    infos = ensure_info(text, hand, sel)

    def score(i: int) -> int:
        kind, dmg, blk, aoe = infos[i]
        if kind == "Attack":
            s = dmg * (len(foes) if aoe and len(foes) > 1 else 1)
            # 能补刀就先补(要死了另算):少一只怪就少一份来袭
            if not about_to_die and 0 < min_hp <= dmg:
                s += 1000
            return s
        if kind == "Power":
            return 40
        if blk > 0:
            if about_to_die:
                return blk * 3
            return min(blk, needed) * 2 if needed > 0 else 3
        return 6

    def choose(blocked: set[int]) -> int | None:
        cands = [
            (-score(i), i)
            for i in range(len(hand))
            if i not in blocked and hand[i][0].isdigit() and int(hand[i][0]) <= energy
        ]
        return min(cands)[1] if cands else None

    names = tuple(n for _c, n in hand)
    blocked: set[int] = set()
    while True:
        pick = choose(blocked)
        if pick is None:
            send("e")
            return
        move_hand_to(pick, screen())
        send("Enter")
        after = screen()
        ahand, _ = parse_hand(after)
        # 打出去了手牌一定变少(费用也变);没变就是这张当前打不出去,换一张
        if (tuple(n for _c, n in ahand), parse_energy(after)) == (names, energy):
            blocked.add(pick)
            if len(blocked) >= len(hand):
                send("e")
                return
            continue
        return


def maybe_drink(text: str) -> bool:
    """危险时喝一瓶:挑第一个非空药水槽,用 p + 数字直接喝;指向敌人的还要回车确认目标."""
    slots = occupied_potions(text)
    if not slots:
        return False
    send("p", str(slots[0] + 1))
    if "select a target with j/k" in screen():
        send("Enter")
    return True


def reward_cards(text: str) -> list[tuple[str | None, int, int]]:
    """奖励屏的卡牌框:按从左到右给出每张的 (类型, 伤害数字, 格挡数字)."""
    lines = text.splitlines()
    boxes = None
    for l in lines:
        if "┌" in l:
            xs = [i for i, ch in enumerate(l) if ch == "┌"]
            boxes = [(x, l.find("┐", x) + 1) for x in xs]
            break
    if not boxes:
        return []
    out = []
    for x0, x1 in boxes:
        blob = "\n".join(l[x0:x1] for l in lines)
        m = KIND_RE.search(blob)
        d = re.search(r"Deal\s+(\d+)\s+damage", blob)
        b = re.search(r"Gain\s+(\d+)\s+Block", blob)
        out.append((m.group(1) if m else None, int(d.group(1)) if d else 0, int(b.group(1)) if b else 0))
    return out


def act_reward(text: str) -> None:
    """奖励屏:金币/遗物/药水直接拿,卡牌挑最好的拿,拿完就离开."""
    lines = text.splitlines()
    marked = next((l.strip()[2:] for l in lines if l.lstrip().startswith("> ")), None)
    if marked is not None:
        # 药水格满了拿不下:直接离开,否则会一直卡在同一个槽位上
        if marked.startswith("Potion") and not available_potion_slot(text):
            send("Escape")
            return
        send("Enter")
        return
    cards = reward_cards(text)
    if cards:
        best, score = None, -10 ** 6
        for i, (kind, dmg, blk) in enumerate(cards):
            if kind in ("Curse", "Status"):
                continue
            val = max(dmg, blk) + {"Attack": 20, "Power": 25}.get(kind, 10)
            if val > score:
                best, score = i, val
        if best is None:
            send("c")  # 三张都是诅咒/状态:跳过卡牌
        else:
            send("g", *(["l"] * best), "Enter")
        return
    send("Escape")


def parse_rest_options(text: str) -> list[str]:
    opts = []
    for l in text.splitlines():
        m = re.match(r"\s*(Rest|Smith|Recall|Lift|Toke|Dig)\b", l)
        if m:
            opts.append(m.group(1))
    return opts


def act_rest(text: str) -> None:
    top = parse_top(text)
    hp, max_hp, _ = top if top else (0, 1, 0)
    opts = parse_rest_options(text)
    if not opts:
        send("Enter")
        return
    if hp * 2 < max_hp and "Rest" in opts:
        idx = opts.index("Rest")
    elif "Smith" in opts:
        idx = opts.index("Smith")
    else:
        idx = 0
    send(str(idx + 1), "Enter")


def act_pick(text: str) -> None:
    """选牌屏(升级/移除/变形):有候选就确认第 0 张,没有就取消."""
    has = any(re.match(r"\s*\((\d+|X|-)\)\s+\S", l) for l in text.splitlines())
    send("g", "Enter") if has else send("Escape")


def parse_shop(text: str) -> list[tuple[str, bool]]:
    """商店条目:(名字, 能不能买).带 () 的整名是药水,带 (数字) 前缀的是卡牌,其余是遗物."""
    items = []
    for l in text.splitlines():
        s = l.strip().strip("│").strip()
        m = re.match(r"^(.*?)\s+(\[(?:sold out|can't afford)\]\s*)?\$(\d+)\s*$", s)
        if not m:
            continue
        name = m.group(1).strip()
        dead = bool(m.group(2) and m.group(2).strip())
        items.append((name, not dead))
    return items


def act_shop(text: str) -> None:
    """商店:买得起的遗物/删牌优先,其次直接离开(不买诅咒)."""
    for i, (name, ok) in enumerate(parse_shop(text)):
        if not ok:
            continue
        is_potion = re.match(r"^\([^)]*\)$", name)
        is_card = re.match(r"^\(\d+\)\s+\S", name)
        if name == "Card Removal Service" or not (is_potion or is_card):
            send("g", *(["j"] * i), "Enter")
            return
    send("Escape")


def bracket_symbol(text: str) -> str | None:
    """地图上被 [] 框住的当前节点符号(图例的 [] you 是空的,匹配不到)."""
    m = re.search(r"\[([^\]]+)\]", text)
    return m.group(1) if m else None


def act_map(text: str) -> None:
    """地图:把岔路都看一遍(用 j/k 采样),按优先级挑一个再走."""
    first = text.splitlines()[0] if text.splitlines() else ""
    top = parse_top(text)
    hp, max_hp, _ = top if top else (0, 1, 0)
    m = re.search(r"\$(\d+)", first)
    gold = int(m.group(1)) if m else 0
    syms = []
    t = text
    for _ in range(6):
        s = bracket_symbol(t)
        if s is None:
            break
        syms.append(s)
        send("j")
        t = screen()
    if not syms:
        send("Enter")
        return
    if len(syms) > 1:
        send(*(["k"] * len(syms)))  # 采样多少步就退多少步,回到起点

    def rank(sym: str) -> int:
        if sym == "R":
            return 0 if hp * 2 < max_hp else 4
        if sym == "$":
            return 0 if gold >= 200 else 4
        if sym in ("T", "?"):
            return 2
        if sym == "e":
            return 3
        if sym == "E":
            return 9
        return 6  # Boss 名字:没得选时才走

    best = min(range(len(syms)), key=lambda i: (rank(syms[i]), i))
    if best:
        send(*(["j"] * best))
    send("Enter")


def event_step(text: str, tried: set[int]) -> None:
    """事件界面走一步.

    结果屏(有"press enter or esc to continue")回车离开;翻牌小游戏一格一格试;
    其余事件按序号一个个试:不可用的项会报 "that choice is not available",试到能选为止
    (数字键直接选中第 N 项,选中的才生效).全试过还不行就 esc 试着离开.
    """
    if "press enter or esc to continue" in text:
        send("Enter")
    elif "Flip card" in text:
        send("j", "Enter")
    else:
        for i in range(5):  # 事件最多 4 个选项,多试一格兜底
            if i not in tried:
                tried.add(i)
                send(str(i + 1))
                return
        send("Escape")


def send_cmd(c: str) -> None:
    """在命令行里敲一条命令并回车."""
    send(":", c, "Enter")


def jump_to_act(target: int, max_tries: int = 8) -> None:
    """用游戏自带的调试命令(:room boss + :win)把当前进度推到第 target 幕.

    TUI 没有 `:act`,只有 --replay 脚本能直接切幕;这里走 :room boss(进本局真 Boss)
    + :win(判定打赢) + 离开奖励屏,tmux 驱动的是真实界面,用来给后几幕的 UI 覆盖开路.
    """
    for _ in range(max_tries):
        a, _f = parse_act_floor(screen())
        if a >= target:
            return
        send_cmd("room boss")
        for _ in range(40):
            if current(screen()) == "COMBAT":
                break
            time.sleep(0.1)
        send_cmd("win")
        for _ in range(80):
            if current(screen()) == "REWARD":
                break
            time.sleep(0.1)
        send("Escape")
        for _ in range(40):
            if current(screen()) == "MAP":
                break
            time.sleep(0.1)


def play_smart(binary: str, seed: int, steps: int = 4000, target_act: int = 1,
               room: str | None = None, win: bool = False) -> dict:
    """策略模式跑一整局:尽量打赢,目标是推进到第二/三幕.

    room/win 是给后几幕做 UI 覆盖用的:room 先用 --act 推幕,再 `:room <spec>` 直接进某个房间;
    win 打开后战斗一律用 `:win` 判定打赢(可以顺着看 Boss 房/双 Boss/奖励那串 UI).
    """
    start(binary, seed)
    enter_the_game()
    seen: set[str] = set()
    events: set[str] = set()
    boss_battles: set[str] = set()
    text = wait_for_map()
    if not map_legend_present(text):
        raise AssertionError(f"seed {seed}: 地图没有图例,首屏如下:\n{text}")
    seen.add("MAP")
    # --act N:先用调试命令把进度推到第 N 幕(给后几幕的 UI 覆盖开路),再正常打
    if target_act > 1:
        jump_to_act(target_act)
        text = wait_for_map()
    # --room <spec>:直接进某个房间(:room shop / event mindbloom / boss / ...)
    if room:
        send_cmd(f"room {room}")
        for _ in range(60):
            text = screen()
            if current(text) != "MAP":
                break
            time.sleep(0.1)
    act, floor = parse_act_floor(text)
    max_act, max_floor = act, floor
    # 药水:本场战斗里连续两次喝了没反应就认为喝不动(仙女瓶那类),别再试
    drink_disabled = False
    drink_stall = 0
    # 战斗里看不到活敌人(比如觉醒者的半死阶段):等一会儿还没走就按 e 推它一下
    no_foe = 0
    # 覆盖模式下(f>=16 的)获胜次数:A20 第三幕打赢第一只 Boss 会直接接第二只(双 Boss)
    boss_wins = 0
    # 事件:记住当前事件标题与试过的选项序号,换事件就重置
    ev_title = None
    ev_tried: set[int] = set()
    # 连续抓到空屏的次数(抓屏瞬时抖动)
    empty = 0
    for _ in range(steps):
        text = screen()
        where = current(text)
        if where == "?":
            for _ in range(8):
                time.sleep(0.15)
                text = screen()
                where = current(text)
                if where != "?":
                    break
        # 还是空的:多半是抓屏的瞬时抖动,歇一下重来;连着太多次才算会话没了
        if not text.strip():
            empty += 1
            if empty > 60:
                raise AssertionError(f"seed {seed}: 连续抓到空屏,会话可能已经退出")
            time.sleep(0.2)
            continue
        empty = 0
        seen.add(where)
        a, f = parse_act_floor(text)
        if a:
            if a > max_act:
                max_act, max_floor = a, f
            elif a == max_act:
                max_floor = max(max_floor, f)
        if where == "EVENT":
            m = re.search(r"┌─ (.*?) ─", text)
            title = m.group(1).strip() if m else None
            if title:
                events.add(title)
            if title != ev_title:
                ev_title = title
                ev_tried = set()
        if where == "COMBAT" and f >= 16:
            boss_battles.add(f"Act {a} boss")
        if where in ("VICTORY", "DEATH"):
            stop()
            return {"where": where, "seen": seen, "text": text,
                    "act": max_act, "floor": max_floor,
                    "events": events, "bosses": boss_battles}
        if where != "COMBAT":
            drink_disabled = drink_stall = no_foe = 0
        if where == "MAP":
            act_map(text)
        elif where == "COMBAT":
            top = parse_top(text)
            hp, max_hp, _b = top if top else (0, 0, 0)
            live = [h for (h, _mm, _bb) in enemy_hp(text) if h > 0]
            if win and live:
                send_cmd("win")  # 覆盖模式:直接判定打赢(双 Boss 会顺势接上第二只)
                if f >= 16:
                    boss_wins += 1
                    tag = f"Act {a} boss" + (f" #{boss_wins}" if boss_wins > 1 else "")
                    boss_battles.add(tag)
            elif not live:
                # 已经赢了(结算要停一会儿)或半死阶段:等一下,太久就按 e 推一把
                time.sleep(0.2)
                no_foe += 1
                if no_foe >= 25:
                    send("e")
                    no_foe = 0
            elif (not drink_disabled and incoming_damage(text) >= hp
                    and occupied_potions(text)):
                before = text
                maybe_drink(text)
                if screen() == before:
                    drink_stall += 1
                    drink_disabled = drink_stall >= 2
                else:
                    drink_stall = 0
            else:
                combat_action(text)
        elif where == "CHOICE":
            if HAND_CHOICE in text:
                send(" ", "Enter")
            else:
                send("Enter")
        elif where == "REWARD":
            act_reward(text)
        elif where == "REST":
            act_rest(text)
        elif where == "SHOP":
            act_shop(text)
        elif where == "EVENT":
            event_step(text, ev_tried)
        elif where == "TREASURE":
            send("Enter")
        elif where == "PICK":
            act_pick(text)
        else:
            raise AssertionError(f"seed {seed}: 认不出界面,屏幕如下:\n{text}")
    stuck = screen()
    stop()
    raise AssertionError(
        f"seed {seed}: {steps} 步之内没有结束,卡在 {current(stuck)},屏幕如下:\n{stuck}"
    )


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
            elif "that choice is not available" in text:
                # 光标停在一个刚失效的选项上(金像拿完后的陷阱屏:第 0/1 项已不可选).
                # 往后挪一格再试,挪到可选项上就能选出去.
                send("j")
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


def expand_seeds(tokens: list[str]) -> list[int]:
    """把种子参数展开:普通数字原样,`N-M` 展开成区间(闭区间)。"""
    seeds: list[int] = []
    for tok in tokens:
        if "-" in tok:
            lo, _, hi = tok.partition("-")
            if lo.isdigit() and hi.isdigit():
                a, b = int(lo), int(hi)
                seeds.extend(range(a, b + 1))
                continue
        if not tok.isdigit():
            raise SystemExit(f"认不出种子参数: {tok}")
        seeds.append(int(tok))
    return seeds


def main() -> int:
    global ASC
    args = sys.argv[1:]
    smart = SMART
    if "--smart" in args:
        args.remove("--smart")
        smart = True
    # --asc N:飞升等级,直接透传给二进制
    if "--asc" in args:
        i = args.index("--asc")
        ASC = int(args[i + 1])
        del args[i:i + 2]
    # --act N:策略模式下先用调试命令把进度推到第 N 幕,再正常打(后几幕 UI 覆盖用)
    target_act = int(os.environ.get("SMOKE_ACT", "1") or "1")
    if "--act" in args:
        i = args.index("--act")
        target_act = int(args[i + 1])
        del args[i:i + 2]
        smart = True
    # --room <spec>:直接进某个房间(:room shop / event mindbloom / boss / ...),用来覆盖后几幕界面
    room = None
    if "--room" in args:
        i = args.index("--room")
        room = args[i + 1]
        del args[i:i + 2]
        smart = True
    # --win:覆盖模式,战斗一律用 :win 判定打赢(看 Boss 房/双 Boss/奖励那一串)
    win = False
    if "--win" in args:
        args.remove("--win")
        win = True
        smart = True
    # 第一个参数是种子(单个数字或 `N-M` 区间)就当简写,否则按 [二进制路径] [种子...] 解析.
    first_seedish = bool(args) and (
        args[0].isdigit()
        or (
            "-" in args[0]
            and all(p.isdigit() for p in args[0].split("-", 1))
        )
    )
    if first_seedish:
        binary = "./target/debug/spire"
        seeds = expand_seeds(args)
    else:
        binary = args[0] if args else "./target/debug/spire"
        seeds = expand_seeds(args[1:])
    seeds = seeds or [7, 42]
    ok = True
    rows: list[tuple[int, bool, str, list[str], int, float, int, int, str]] = []
    t_all = time.time()
    for seed in seeds:
        t0 = time.time()
        act, floor, notes = 0, 0, ""
        try:
            if smart:
                res = play_smart(binary, seed, target_act=target_act, room=room, win=win)
                where, seen, text = res["where"], res["seen"], res["text"]
                act, floor = res["act"], res["floor"]
                ev = ",".join(sorted(res.get("events", ()))) or "-"
                bo = ",".join(sorted(res.get("bosses", ()))) or "-"
                notes = f"events[{ev}] boss[{bo}]"
                if target_act > 1:
                    notes = f"jump->act{target_act} " + notes
                if room:
                    notes += f" room[{room}]"
                if win:
                    notes += " win"
            else:
                where, seen, text = play(binary, seed)
                act, floor = parse_act_floor(text)
        except AssertionError as e:
            print(f"FAIL seed {seed}: {e}")
            rows.append((seed, False, "-", [], KEYS_SENT, time.time() - t0, act, floor, notes))
            ok = False
            continue
        if not smart and "REWARD" not in seen:
            print(f"FAIL seed {seed}: 整局没出现过奖励界面")
            rows.append((seed, False, "-", sorted(seen), KEYS_SENT, time.time() - t0, act, floor, notes))
            ok = False
            continue
        print(
            f"ok seed {seed}: 结局 {where},最远 Act {act} 第 {floor} 层,"
            f"走过的界面 {sorted(seen)},按了 {KEYS_SENT} 次键"
        )
        if notes:
            print("   ", notes)
        print("   最后一行:", [l for l in text.splitlines() if l.strip()][-1][:100])
        rows.append((seed, True, where, sorted(seen), KEYS_SENT, time.time() - t0, act, floor, notes))
    print()
    print("seed   result  end      act/floor  keys  secs  screens")
    for seed, good, where, seen, keys, secs, act, floor, notes in rows:
        print(
            f"{seed:<6} {'PASS' if good else 'FAIL':<7} {where:<8} "
            f"a{act}f{floor:<4} {keys:<5} {secs:5.1f} {','.join(seen)}"
        )
        if notes:
            print(f"       {notes}")
    print(f"\nsmoke: {'PASS' if ok else 'FAIL'}  "
          f"({len(rows)} seeds, {time.time() - t_all:.1f}s total)")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
