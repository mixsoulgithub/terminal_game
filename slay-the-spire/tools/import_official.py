#!/usr/bin/env python3
"""把原版 Slay the Spire 的文案/数值抽成一份中间语料(corpus_src.json).

数据来源(都在本机 golden 目录, 不进仓库):
  <golden>/game/desktop-1.0.jar                  原版游戏(官方 localization 的正本)
  <golden>/out/localization/eng/*.json           上一步 unzip 出来的官方英文文案
  <golden>/decomp/src/com/megacrit/cardcrawl/**  用 CFR 反编译出的类(数值/拼装顺序的正本)

产出:
  <golden>/out/corpus_src.json                   给 gen_corpus.py 当语料源

为什么不是直接读 localization: 官方文案是"片段数组", 片段之间的数值由每个类的代码拼进去
(例如 Vajra 是 DESCRIPTIONS[0] + 1 + DESCRIPTIONS[1]), 而片段顺序也由代码决定
(例如 Ectoplasm 是 DESCRIPTIONS[1] + DESCRIPTIONS[0]). 所以必须把类的方法体也读一遍.

用法:
  python3 tools/import_official.py [golden_dir] [--report]

--report 只打印覆盖率与和当前语料的数值比对, 不写文件.
"""
import json
import os
import re
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_GOLDEN = os.path.expanduser("~/projects/sts-golden")
DEFAULT_IDS = os.path.join(HERE, "..", "src", "core", "corpus.rs")


def log(*a):
    print(*a, file=sys.stderr)


def collapse(s: str) -> str:
    """把源码压成单行(便于正则), 保留字符串字面量内部. 只用于匹配, 不用于求值."""
    return re.sub(r"\s+", " ", s).strip()


def method_body(src: str, pattern: str):
    """按正则找到方法签名, 返回大括号配平后的方法体文本(不含最外层括号)."""
    m = re.search(pattern, src)
    if not m:
        return None
    i = src.find("{", m.end() - 1)
    if i < 0:
        return None
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return src[i + 1 : j]
    return None


def body_at(src: str, pos: int):
    """从 pos 开始找第一个 { 并做括号配平, 取方法体."""
    i = src.find("{", pos)
    if i < 0:
        return None
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return src[i + 1 : j]
    return None


# ---------------------------------------------------------------- 表达式求值
class Dyn:
    """动态量(this.amount / this.counter 之类): 文本上显示成 X, 比较时当成 num."""

    def __init__(self, num=1, text="X"):
        self.num = num
        self.text = text

    def __str__(self):
        return self.text


class EvalError(Exception):
    pass


TOKEN_RE = re.compile(
    r"""
    (?P<str>"(?:[^"\\]|\\.)*")
  | (?P<num>\d+)
  | (?P<ident>[A-Za-z_][A-Za-z0-9_$]*)
  | (?P<op>\?\?|\?\:|==|!=|>=|<=|&&|\|\||[+\-*/?:<>().\[\],!])
    """,
    re.VERBOSE,
)


class Expr:
    """极小的 Java 表达式求值器, 只覆盖 StS 描述串拼装用到的子集."""

    def __init__(self, ctx):
        self.ctx = ctx

    def eval(self, src):
        self.toks = [m for m in TOKEN_RE.finditer(src)]
        self.i = 0
        v = self.parse_ternary()
        if self.i != len(self.toks):
            raise EvalError("剩余 token: " + self.toks[self.i].group(0))
        return v

    def peek(self):
        return self.toks[self.i].group(0) if self.i < len(self.toks) else None

    def next(self):
        t = self.toks[self.i]
        self.i += 1
        return t.group(0)

    def accept(self, s):
        if self.peek() == s:
            self.i += 1
            return True
        return False

    def expect(self, s):
        if not self.accept(s):
            raise EvalError("期望 %r, 实际 %r" % (s, self.peek()))

    def parse_ternary(self):
        cond = self.parse_or()
        if self.accept("?"):
            a = self.parse_ternary()
            self.expect(":")
            b = self.parse_ternary()
            return a if truthy(cond) else b
        return cond

    def parse_or(self):
        v = self.parse_and()
        while self.accept("||"):
            r = self.parse_and()
            v = truthy(v) or truthy(r)
        return v

    def parse_and(self):
        v = self.parse_cmp()
        while self.accept("&&"):
            r = self.parse_cmp()
            v = truthy(v) and truthy(r)
        return v

    def parse_cmp(self):
        v = self.parse_add()
        while True:
            op = self.peek()
            if op in ("==", "!=", ">", "<", ">=", "<="):
                self.next()
                r = self.parse_add()
                v = compare(op, v, r)
            else:
                return v

    def parse_add(self):
        v = self.parse_mul()
        while self.peek() == "+":
            self.next()
            v = add(v, self.parse_mul())
        return v

    def parse_mul(self):
        v = self.parse_unary()
        while self.peek() in ("*", "/"):
            op = self.next()
            r = self.parse_unary()
            a, b = num(v), num(r)
            calc = a * b if op == "*" else (a // b if b else 0)
            # 两边都是普通整数就保持整数, 否则退化成动态量
            if isinstance(v, int) and isinstance(r, int):
                v = calc
            else:
                v = Dyn(calc, "X")
        return v

    def parse_unary(self):
        if self.accept("-"):
            v = self.parse_unary()
            return Dyn(-v.num, v.text) if isinstance(v, Dyn) else -v
        if self.accept("!"):
            return not truthy(self.parse_unary())
        return self.parse_postfix()

    def parse_postfix(self):
        v = self.parse_atom()
        while self.peek() == "[":
            self.next()
            idx = self.parse_ternary()
            self.expect("]")
            if isinstance(v, list):
                n = idx.num if isinstance(idx, Dyn) else int(idx)
                v = v[n] if 0 <= n < len(v) else ""
            else:
                raise EvalError("对非数组取下标")
        return v

    def parse_atom(self):
        t = self.peek()
        if t is None:
            raise EvalError("表达式提前结束")
        if t.startswith('"'):
            self.next()
            return unquote(t)
        if t == "(":
            # (String)xxx 这类强制转换: 括号里只有一个类型名时直接跳过
            if (
                self.i + 2 < len(self.toks)
                and self.toks[self.i + 1].group(0).isidentifier()
                and self.toks[self.i + 2].group(0) == ")"
            ):
                self.i += 3
                return self.parse_unary()
            self.next()
            v = self.parse_ternary()
            self.expect(")")
            return v
        raw = self.next()
        # 纯数字
        if raw.isdigit():
            return int(raw)
        # 关键字
        if raw == "true":
            return True
        if raw == "false":
            return False
        if raw == "null":
            return ""
        # 名字: 可能是 this.xxx / A.B[i] / 常量
        parts = [raw]
        while True:
            if self.accept("."):
                parts.append(self.next())
            elif self.peek() == "(":
                self.next()
                args = []
                if not self.accept(")"):
                    while True:
                        args.append(self.parse_ternary())
                        if self.accept(")"):
                            break
                        self.expect(",")
                return call(".".join(parts), args)
            else:
                break
        return self.resolve(parts)

    def resolve(self, parts):
        name = ".".join(parts)
        ctx = self.ctx
        # this.DESCRIPTIONS / XxxPower.DESCRIPTIONS / DESC 都是那张片段数组
        if parts[-1] in ("DESCRIPTIONS", "EXTENDED_DESCRIPTION", "DESC"):
            return ctx.get("_" + parts[-1], [])
        if name in ctx:
            return ctx[name]
        if name.startswith("this.") and name[5:] in ctx:
            return ctx[name[5:]]
        if name in ("LocalizedStrings.PERIOD",):
            return "."
        if name in ("LocalizedStrings.NL",):
            return " NL "
        # 动态量(owner.name / basePower 等): 显示成 X, 但记一笔便于复核
        if name.startswith("this.") or "." in name:
            UNKNOWN.append(name)
            return Dyn(1, "X")
        raise EvalError("未知符号: " + name)


def unquote(t):
    return json.loads(t)


UNKNOWN = []


def call(name, args):
    """少量已知方法的等价实现; 不认识的返回动态量 X 并记一笔."""
    if name == "String.format":
        tpl = str(args[0])
        vals = [str(a) for a in args[1:]]
        out, i = [], 0
        for m in re.finditer(r"%(\d+\$)?[dDsS]", tpl):
            out.append(tpl[i : m.start()])
            idx = m.group(1)
            if idx:
                n = int(idx[:-1]) - 1
                out.append(vals[n] if 0 <= n < len(vals) else "X")
            else:
                out.append(vals.pop(0) if vals else "X")
            i = m.end()
        out.append(tpl[i:])
        return "".join(out)
    if name in ("FontHelper.colorString", "TipHelper.capitalize", "String.valueOf", "GameDictionary.keywords.get"):
        return args[0] if args else ""
    if name.endswith(".toString"):
        return args[0] if args else ""
    # 有/没有某遗物、某形态的判断: 语料按"没有"取基础文案
    if re.search(r"hasRelic|hasPower|hasPotion|isBeta|isEndless|isDaily|isTrial|isAscension|isDev|isDebug", name):
        return False
    if name.endswith("getPotency"):
        return args[0] if args else Dyn(1, "X")
    UNKNOWN.append(name)
    return Dyn(1, "X")


def truthy(v):
    if isinstance(v, bool):
        return v
    if isinstance(v, Dyn):
        return bool(v.num)
    if isinstance(v, str):
        return v not in ("", "null")
    return bool(v)


def num(v):
    if isinstance(v, Dyn):
        return v.num
    if isinstance(v, bool):
        return int(v)
    if isinstance(v, int):
        return v
    return v


def compare(op, a, b):
    x, y = num(a), num(b)
    if isinstance(a, str) or isinstance(b, str):
        x, y = str(a), str(b)
    return {
        "==": lambda: x == y,
        "!=": lambda: x != y,
        ">": lambda: x > y,
        "<": lambda: x < y,
        ">=": lambda: x >= y,
        "<=": lambda: x <= y,
    }[op]()


def add(a, b):
    """字符串拼接优先: 只要有一边是字符串就按文本拼."""
    if isinstance(a, bool) or isinstance(b, bool):
        raise EvalError("布尔参与拼接")
    if isinstance(a, int) and isinstance(b, int):
        return a + b
    return str(a) + str(b)


# ---------------------------------------------------------------- 文案清洗
ICON_ENERGY = {"E", "R", "G", "B", "W", "P", "T"}


def resolve_markup(text: str) -> str:
    """官方标记 -> 本游戏用的纯文本.

    规则:
      #y #b #r #g #p #d 颜色标记 -> 去掉
      *word 高亮 -> 去掉星号
      NL -> 换行
      [E]/[R]/... 能量图标 -> 连续的同类图标合成一个 (N), 单个就是 (1)
      其余 [xx] (如 [R] 之外的) 保留原样
    """
    text = text.replace(" NL ", "\n").replace(" NL", "\n")
    text = re.sub(r"#[a-zA-Z]", "", text)
    text = re.sub(r"(?<![A-Za-z])\*", "", text)
    # 合并连续的能量图标
    def merge_icons(m):
        n = len(re.findall(r"\[[A-Za-z]\]", m.group(0)))
        return "(%d)" % n if n else m.group(0)

    text = re.sub(r"(?:\[[A-Za-z]\]\s*){2,}", merge_icons, text)
    text = re.sub(r"\[([A-Za-z])\]", lambda m: "(1)" if m.group(1) in ICON_ENERGY else m.group(0), text)
    text = re.sub(r"[ \t]+", " ", text)
    text = re.sub(r" *\n *", "\n", text)
    text = re.sub(r"\s+([.,;:])", r"\1", text)
    return text.strip()


def resolve_pips(text: str) -> str:
    """能力描述里的 [R] 循环拼装(如 Berserk)统一成 (X)."""
    text = re.sub(r"(\(1\)\s*)+", "(X) ", text)
    return text.strip()


# ---------------------------------------------------------------- 类文件解析
def index_classes(root):
    """按 ID 常量索引反编译出来的类: {ID: path}."""
    idx = {}
    for dirpath, _, files in os.walk(root):
        for f in files:
            if not f.endswith(".java"):
                continue
            p = os.path.join(dirpath, f)
            head = open(p, encoding="utf-8").read()
            m = re.search(r'static final String (?:ID|POTION_ID|POWER_ID) = "([^"]+)"', head)
            if not m:
                continue
            idx.setdefault(m.group(1), p)
    return idx


CARD_ENUM = re.compile(
    r",\s*AbstractCard\.CardType\.(\w+),\s*AbstractCard\.CardColor\.(\w+),"
    r"\s*AbstractCard\.CardRarity\.(\w+),\s*AbstractCard\.CardTarget\.(\w+)"
)

TARGET_MAP = {
    "ENEMY": "enemy",
    "ALL_ENEMY": "allenemy",
    "SELF": "self",
    "NONE": "none",
    "ALL": "allenemy",
}

COLOR_MAP = {"RED": "red", "GREEN": "green", "BLUE": "blue", "PURPLE": "purple", "COLORLESS": "colorless", "CURSE": "curse"}

TYPE_MAP = {"ATTACK": "attack", "SKILL": "skill", "POWER": "power", "STATUS": "status", "CURSE": "curse"}


def parse_card(src):
    """解析一张卡的类: 费用/类型/稀有度/目标/基础数值/升级增量/升级后文本规则."""
    flat = collapse(src)
    out = {}
    m = CARD_ENUM.search(flat)
    if not m:
        return None
    out["kind"] = TYPE_MAP.get(m.group(1), m.group(1).lower())
    out["color"] = COLOR_MAP.get(m.group(2), m.group(2).lower())
    out["rarity"] = m.group(3).lower()
    out["target"] = TARGET_MAP.get(m.group(4), m.group(4).lower())
    # 费用: 图片字符串之后的那个参数
    sup = re.search(r"super\((.*?)AbstractCard\.CardType", flat)
    if sup:
        args = split_args(sup.group(1))
        cost = None
        for i, a in enumerate(args):
            if a.startswith('"'):
                cost = args[i + 1] if i + 1 < len(args) else None
                break
        if cost is not None:
            try:
                out["cost"] = cost_str(int(cost))
            except ValueError:
                out["cost"] = cost.strip()
    out["numbers"] = parse_numbers(src)
    out["rawdesc"] = parse_rawdesc(src)
    return out


def cost_str(c):
    """StS 里 -2 是不可打出, -1 是 X 费; 本游戏语料写成 - / X."""
    return "-" if c <= -2 else ("X" if c < 0 else str(c))


def split_args(s):
    out, depth, cur = [], 0, ""
    for ch in s:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    out.append(cur.strip())
    return [a for a in out if a]


ASSIGN_PATTERNS = {
    "dmg": r"this\.baseDamage\s*=\s*([^;]+);",
    "blk": r"this\.baseBlock\s*=\s*([^;]+);",
    "mag": r"this\.baseMagicNumber\s*=\s*([^;]+);",
}


def int_or_none(expr, consts, src=None):
    """从赋值右边取整数: 支持字面量 / 常量名 / 三元里的最后分支 / this.misc 字段."""
    if expr is None:
        return None
    expr = expr.strip()
    if expr in ("this.misc", "this.amount") and src:
        m = re.search(r"%s\s*=\s*(-?\d+)" % re.escape(expr), src)
        if m:
            return int(m.group(1))
    if re.fullmatch(r"-?\d+", expr):
        return int(expr)
    for k in ("this.misc", "this.amount"):
        m = re.search(r"%s\s*=\s*(-?\d+)" % re.escape(k), expr)
        if m:
            return int(m.group(1))
    m = re.search(r":\s*(-?\d+)\s*$", expr)
    if m:
        return int(m.group(1))
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", expr) and expr in consts:
        return consts[expr]
    return None


def parse_numbers(src):
    consts = {m.group(1): int(m.group(2)) for m in re.finditer(r"static final int (\w+) = (\d+)", src)}
    nums = {}
    # 基础值: 只看构造函数(upgrade/applyPowers 里的赋值是动态的).
    # 卡片可能有多个构造器(如 Searing Blow(int)), 数值散在其中, 全部合并.
    bodies = []
    for m in re.finditer(r"public\s+(\w+)\s*\([^)]*\)\s*\{", src):
        b = body_at(src, m.start())
        if b:
            bodies.append(b)
    body = "\n".join(bodies) if bodies else src
    for k, pat in ASSIGN_PATTERNS.items():
        vals = [int_or_none(x, consts, src) for x in re.findall(pat, body)]
        vals = [v for v in vals if v is not None]
        nums[k] = vals[0] if vals else 0
    # 升级增量
    up = method_body(src, r"public\s+void\s+upgrade\s*\(\s*\)")
    deltas = {"dmg_up": 0, "blk_up": 0, "mag_up": 0, "cost_up": None}
    if up:
        for key, fn in (("dmg_up", "upgradeDamage"), ("blk_up", "upgradeBlock"), ("mag_up", "upgradeMagicNumber")):
            m = re.search(r"%s\(([^)]*)\)" % fn, up)
            if m:
                v = int_or_none(m.group(1), consts, src)
                if v is None:
                    # upgradeDamage(4 + this.timesUpgraded) 这种: 调用发生在自增之前
                    v = eval_up_expr(m.group(1), {**nums, **deltas}, consts, times_upgraded=0)
                if v is not None:
                    deltas[key] = v
        m = re.search(r"upgradeBaseCost\(([^)]*)\)", up)
        if m:
            v = int_or_none(m.group(1), consts, src)
            if v is None:
                v = eval_up_expr(m.group(1), {**nums, **deltas}, consts, times_upgraded=0)
            if v is not None:
                deltas["cost_up"] = v
        # 直接赋值的形式(this.baseDamage = 12), 也可能是表达式(Halt 的 baseBlock + 6 ...)
        for k, pat in ASSIGN_PATTERNS.items():
            for x in re.findall(pat, up):
                v = int_or_none(x, consts, src)
                if v is None:
                    v = eval_up_expr(x, {**nums, **deltas}, consts)
                if v is not None:
                    deltas[k + "_up"] = v - nums[k]
        # cost_up 也要考虑表达式
        if deltas["cost_up"] is None:
            for x in re.findall(r"this\.cost\s*=\s*([^;]+);", up):
                v = int_or_none(x, consts, src)
                if v is not None:
                    deltas["cost_up"] = v
    # 升级后在数值上被重置的卡(如 Searing Blow)以 upgrade() 里的绝对值为准
    nums.update(deltas)
    return nums


def eval_up_expr(expr, nums, consts, times_upgraded=1):
    """升级后是算术表达式时求值(用升级后的同伴数值与 timesUpgraded)."""
    ctx = {
        "this.baseDamage": nums["dmg"] + nums.get("dmg_up", 0),
        "this.baseBlock": nums["blk"] + nums.get("blk_up", 0),
        "this.baseMagicNumber": nums["mag"],
        "this.magicNumber": nums["mag"],
        "this.timesUpgraded": times_upgraded,
        "this.upgraded": True,
    }
    ctx.update(consts)
    try:
        v = Expr(ctx).eval(expr)
    except (EvalError, Exception):
        return None
    return v.num if isinstance(v, Dyn) else (v if isinstance(v, int) else None)


def parse_rawdesc(src):
    """升级后描述怎么来的: keep 用 DESCRIPTION, replace 用 UPGRADE_DESCRIPTION, append 拼接.

    只认构造器和 upgrade() 里的赋值: 有些卡在 use() 里临时拼说明文字(Body Slam), 与
    牌面无关.
    """
    scope = ""
    for m in re.finditer(r"public\s+(\w+)\s*\([^)]*\)\s*\{", src):
        b = body_at(src, m.start())
        if b:
            scope += b
    scope += method_body(src, r"public\s+void\s+upgrade\s*\(\s*\)") or ""
    if re.search(r"rawDescription\s*\+\s*\w+\.cardStrings\.UPGRADE_DESCRIPTION", scope):
        return "append"
    if re.search(r"rawDescription\s*=\s*[^;]*UPGRADE_DESCRIPTION", scope) or re.search(
        r"rawDescription\s*=\s*!\s*this\.upgraded\s*\?", scope
    ):
        return "replace"
    return "keep"


# ---------------------------------------------------------------- 单条实体文本
def card_text(desc, up_desc, nums, upgraded):
    """把 !D!/!B!/!M! 换成数值."""
    d = nums["dmg"] + (nums["dmg_up"] if upgraded else 0)
    b = nums["blk"] + (nums["blk_up"] if upgraded else 0)
    m = nums["mag"] + (nums["mag_up"] if upgraded else 0)
    t = up_desc if upgraded else desc
    t = t.replace("!D!", str(d)).replace("!B!", str(b)).replace("!M!", str(m))
    return t


def build_text_from_expr(src, descs, name="", ctx_extra=None):
    """用类里的拼装代码把 DESCRIPTIONS 片段拼起来.

    优先级: setDescription() -> getUpdatedDescription() -> updateDescription() 赋值
            -> 构造函数里的 description 赋值 -> StringBuilder 的 append 串
    """
    ctx = {
        "amount": Dyn(1, "X"),
        "counter": Dyn(3, "X"),
        "hitCount": Dyn(1, "X"),
        "hpLoss": Dyn(1, "X"),
        "damage": Dyn(1, "X"),
        "basePower": Dyn(1, "X"),
        "multiplier": Dyn(1, "X"),
        "Settings.isEndless": False,
        "Settings.isBeta": False,
        "Settings.isDebug": False,
        "Settings.isDev": False,
        "AbstractDungeon.player": None,
        "this.owner": Dyn(1),
        "this.owner.isPlayer": True,
        "this.isPlayer": True,
        "this.onPlayer": False,
        "this.timesUpgraded": 0,
        "this.upgraded": False,
        "NAME": name,
    }
    if ctx_extra:
        ctx.update(ctx_extra)
    ctx["_DESCRIPTIONS"] = descs
    ctx["_DESC"] = descs
    ctx["_EXTENDED_DESCRIPTION"] = descs
    # 类里的 public static final String X = DESCRIPTIONS[i] 这种别名
    for m in re.finditer(r"public static final String (\w+) = ([^;]+);", src):
        if m.group(1) in ctx:
            continue
        try:
            ctx[m.group(1)] = str(Expr(ctx).eval(m.group(2)))
        except EvalError:
            pass

    exprs = []
    for pat in (
        r"String\s+setDescription\s*\([^)]*\)\s*\{",
        r"String\s+getUpdatedDescription\s*\(\s*\)\s*\{",
    ):
        b = method_body(src, pat)
        if b:
            exprs += re.findall(r"return\s+([^;]+);", b)
    b = method_body(src, r"void\s+updateDescription\s*\([^)]*\)\s*\{")
    if b:
        exprs += re.findall(r"this\.description\s*=\s*([^;]+);", b)
    ctor = re.search(r"public\s+(\w+)\s*\([^)]*\)\s*\{", src)
    if ctor:
        cb = method_body(src, r"public\s+%s\s*\([^)]*\)\s*\{" % re.escape(ctor.group(1)))
        if cb:
            exprs += re.findall(r"this\.description\s*=\s*([^;]+);", cb)
            exprs += re.findall(r"return\s+([^;]+);", cb)
    first_err = None
    # 最后兜底: 全类范围内找 this.description = 表达式(药水的 initializeData 等)
    fallback = [e for e in re.findall(r"this\.description\s*=\s*([^;]+);", src) if "description" not in e]
    for group in (exprs, fallback):
        for e in group:
            if "description" in e:
                continue
            try:
                return str(Expr(ctx).eval(e))
            except EvalError as ex:
                if first_err is None:
                    first_err = ex
    # StringBuilder: 把 append 片段按顺序拼起来
    if b:
        appends = re.findall(r"\.append\(([^;]*?)\);", b)
        if appends:
            parts = []
            for a in appends:
                for piece in split_args(a):
                    try:
                        parts.append(str(Expr(ctx).eval(piece)))
                    except EvalError as ex:
                        if first_err is None:
                            first_err = ex
                        parts = None
                        break
                if parts is None:
                    break
            if parts:
                return "".join(parts)
    if first_err:
        raise first_err
    raise EvalError("找不到可求值的描述表达式")


def potency_of(src):
    m = re.search(r"int\s+getPotency\s*\([^)]*\)\s*\{([^}]*)\}", src, re.S)
    if m:
        r = re.search(r"return\s+(\d+)", m.group(1))
        if r:
            return int(r.group(1))
    return None


# ---------------------------------------------------------------- 主流程

def main():
    golden = DEFAULT_GOLDEN
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    report_only = "--report" in sys.argv
    if args:
        golden = os.path.expanduser(args[0])
    loc = os.path.join(golden, "out", "localization", "eng")
    decomp = os.path.join(golden, "decomp", "src", "com", "megacrit", "cardcrawl")
    official = {
        n: json.load(open(os.path.join(loc, n + ".json"), encoding="utf-8"))
        for n in ("cards", "relics", "powers", "potions", "events")
    }
    classes = {
        "cards": index_classes(os.path.join(decomp, "cards")),
        "relics": index_classes(os.path.join(decomp, "relics")),
        "powers": index_classes(os.path.join(decomp, "powers")),
        "potions": index_classes(os.path.join(decomp, "potions")),
    }
    for k, v in classes.items():
        log("%-8s 类 %d 个, 官方条目 %d 个" % (k, len(v), len(official[k])))

    out = {"cards": [], "relics": [], "powers": [], "potions": [], "events": [], "missing": []}
    # 事件: 只用官方标题. 正文是按阶段分片的旁白, 我们的语料是机制摘要, 不能覆盖
    for key, entry in official["events"].items():
        out["events"].append({"key": key, "name": entry.get("NAME", key)})
    # ---- 卡片
    for key, entry in official["cards"].items():
        p = classes["cards"].get(key)
        if not p:
            out["missing"].append(("cards", key, "无对应类"))
            continue
        src = open(p, encoding="utf-8").read()
        info = parse_card(src)
        if not info:
            out["missing"].append(("cards", key, "解析 super() 失败"))
            continue
        d, u = entry.get("DESCRIPTION", ""), entry.get("UPGRADE_DESCRIPTION")
        base = resolve_markup(card_text(d, d, info["numbers"], False))
        if info["rawdesc"] == "replace" and u:
            up = resolve_markup(card_text(u, u, info["numbers"], True))
        elif info["rawdesc"] == "append" and u:
            up = resolve_markup(card_text(d + u, d + u, info["numbers"], True))
        else:
            up = resolve_markup(card_text(d, d, info["numbers"], True))
        cu = info["numbers"].get("cost_up")
        out["cards"].append(
            {
                "key": key,
                "name": entry.get("NAME", key),
                "kind": info["kind"],
                "color": info["color"],
                "rarity": info["rarity"],
                "target": info["target"],
                "cost": info.get("cost", "-"),
                "cost_up": cost_str(cu) if cu is not None else info.get("cost", "-"),
                "text": base,
                "text_up": up,
                "nums": info["numbers"],
            }
        )
    # ---- 遗物
    for key, entry in official["relics"].items():
        p = classes["relics"].get(key)
        if not p:
            out["missing"].append(("relics", key, "无对应类"))
            continue
        src = open(p, encoding="utf-8").read()
        try:
            text = build_text_from_expr(src, entry.get("DESCRIPTIONS", []), entry.get("NAME", key))
            out["relics"].append({"key": key, "name": entry.get("NAME", key), "text": resolve_markup(text)})
        except EvalError as ex:
            out["missing"].append(("relics", key, "求值失败: %s" % ex))
    # ---- 能力
    for key, entry in official["powers"].items():
        p = classes["powers"].get(key)
        if not p:
            out["missing"].append(("powers", key, "无对应类"))
            continue
        src = open(p, encoding="utf-8").read()
        try:
            text = resolve_pips(resolve_markup(build_text_from_expr(src, entry.get("DESCRIPTIONS", []), entry.get("NAME", key))))
            kind = "debuff" if re.search(r"PowerType\.DEBUFF", src) else "buff"
            out["powers"].append({"key": key, "name": entry.get("NAME", key), "kind": kind, "text": text})
        except EvalError as ex:
            out["missing"].append(("powers", key, "求值失败: %s" % ex))
    # ---- 药水
    for key, entry in official["potions"].items():
        p = classes["potions"].get(key)
        if not p:
            out["missing"].append(("potions", key, "无对应类"))
            continue
        src = open(p, encoding="utf-8").read()
        pot = potency_of(src)
        descs = entry.get("DESCRIPTIONS", [])
        try:
            text = build_text_from_expr(
                src, descs, entry.get("NAME", key), ctx_extra={"this.potency": pot if pot is not None else Dyn(1, "X")}
            )
        except EvalError:
            # 兜底: 官方药水文案基本都是 DESCRIPTIONS[0] + potency + DESCRIPTIONS[1]
            text = descs[0] + (str(pot) if pot is not None else "X") + (descs[1] if len(descs) > 1 else "")
        out["potions"].append(
            {
                "key": key,
                "name": entry.get("NAME", key),
                "text": resolve_markup(text),
                "targeted": "targetRequired = true" in collapse(src),
            }
        )
    stats = {k: len(v) for k, v in out.items() if isinstance(v, list)}
    log("抽出: " + ", ".join("%s %d" % (k, v) for k, v in stats.items()))
    log("未处理 %d 项" % len(out["missing"]))
    for t, k, why in out["missing"]:
        log("  [%s] %s: %s" % (t, k, why))
    if report_only:
        return
    dst = os.path.join(golden, "out", "corpus_src.json")
    with open(dst, "w", encoding="utf-8") as f:
        json.dump(out, f, ensure_ascii=False, indent=1)
    log("wrote " + dst)


if __name__ == "__main__":
    main()
