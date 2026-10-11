#!/usr/bin/env python3
"""给每条状态(Status)挑 Nerd Font 图标,并生成 src/core/status.rs 里的 glyph() 函数.

用法: python3 tools/nf_status_glyphs.py [字体路径] [--write]

默认字体取 kitty 装的 Nerd Font symbols;映射用**字形名**(fa-dumbbell / md_shield_plus 这类),
从字体的 cmap 里查出码点,再写成 \\u{...} 转义 —— 源码保持 ASCII,换图标只改下面这张表.
不加 --write 只打印当前映射核对(挑不到的名字会报错).
"""
import os
import re
import sys

DEFAULT_FONT = os.path.expanduser("~/.local/lib/kitty/fonts/SymbolsNerdFontMono-Regular.ttf")
STATUS_RS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "core", "status.rs")

# 状态 -> 字形名(Nerd Fonts 的字形名,大小写与连字符标点都不计较)
MAP = {
    "Strength": "fa-dumbbell", "Dexterity": "fa-shoe_prints", "Vulnerable": "fa-heart_crack",
    "Weak": "fa-face_dizzy", "Frail": "fa-crutch", "Entangled": "fa-spider",
    "DemonForm": "fa-fire_flame_curved", "Metallicize": "fa-shield_halved",
    "FeelNoPain": "fa-hand_holding_heart", "DarkEmbrace": "fa-moon_o", "Evolve": "fa-dna",
    "FireBreathing": "fa-fire", "Barricade": "fa-chess_rook", "Brutality": "md-blood_bag",
    "Rupture": "fa-droplet_slash", "Regenerate": "md-heart_pulse", "Thorns": "md-cactus",
    "Berserk": "fa-face_grin_stars", "Combust": "fa-fire_flame_simple", "Corruption": "fa-biohazard",
    "DoubleTap": "fa-repeat", "Juggernaut": "fa-truck_fast", "Rage": "fa-face_angry",
    "FlameBarrier": "ple-flame_thick", "Artifact": "fa-gem", "NoBlock": "fa-ban",
    "NoDraw": "cod-circle_slash", "Mayhem": "fa-shuffle", "Magnetism": "fa-magnet",
    "Panache": "fa-feather", "SadisticNature": "fa-face_grin_tongue_squint",
    "PlatedArmor": "md-shield_plus", "Constricted": "fa-ring", "Ritual": "fa-hands_praying",
    "Enrage": "fa-bullhorn", "Anger": "fa-face_grimace", "SharpHide": "fa-hand_fist",
    "BeatOfDeath": "fa-heartbeat", "Curiosity": "md-magnify", "CurlUp": "md-shield_plus_outline",
    "Explosive": "md-bomb", "Fading": "fa-hourglass_end", "Flight": "fa-dove", "Invincible": "fa-shield",
    "Malleable": "fa-shapes", "Minion": "fa-users", "MinionLeader": "fa-crown",
    "ModeShift": "fa-rotate", "PainfulStabs": "fa-syringe", "Reactive": "md-bolt",
    "Regrow": "fa-seedling", "Shifting": "fa-arrows_v", "Slow": "fa-clock_o",
    "SporeCloud": "fa-cloud", "Split": "md-source_branch", "Stasis": "fa-box_archive",
    "StrengthUp": "fa-angles_up", "TimeWarp": "fa-hourglass", "Asleep": "fa-bed_pulse",
    "Duplication": "fa-clone", "LoseStrength": "fa-arrow_down", "LoseDexterity": "cod-arrow_down",
    "Focus": "fa-bullseye", "Accuracy": "fa-crosshairs", "AfterImage": "fa-images",
    "ThousandCuts": "fa-utensils", "Envenom": "fa-flask", "InfiniteBlades": "fa-khanda",
    "NoxiousFumes": "fa-smog", "ToolsOfTheTrade": "fa-screwdriver_wrench", "WraithForm": "fa-ghost",
    "Blur": "fa-eye_slash", "PhantasmalKiller": "fa-user_secret", "Burst": "fa-forward",
    "WellLaidPlans": "cod-checklist", "Choke": "md-hand_front_right", "CorpseExplosion": "fa-bomb",
    "DoubleDamage": "fa-check_double", "Poison": "fa-skull_crossbones", "Confused": "md-help_circle",
    "Hex": "md-hexagon", "DrawReduction": "md-cards_outline", "Surrounded": "md-account_group",
    "Intangible": "md-ghost",
}


def variants(src: str):
    """status.rs 里 Status 枚举的变体(按声明顺序)"""
    i = src.index("pub enum Status {")
    j = src.index("\n}", i)
    return re.findall(r"^    ([A-Z][A-Za-z0-9]*),?$", src[i:j], re.M)


def glyphs(font_path: str):
    from fontTools.ttLib import TTFont

    f = TTFont(font_path, fontNumber=0)
    out = {}
    for cp, name in f.getBestCmap().items():
        key = name.lower().replace("-", "_")
        if key not in out:
            out[key] = cp
    return out


def main():
    args = [a for a in sys.argv[1:] if a != "--write"]
    font_path = args[0] if args else DEFAULT_FONT
    byname = glyphs(font_path)
    src = open(STATUS_RS, encoding="utf-8").read()
    vs = variants(src)
    miss = [(v, MAP.get(v)) for v in vs if (MAP.get(v) or "").lower().replace("-", "_") not in byname]
    if miss:
        for v, n in miss:
            print(f"字形名挑不到: {v} -> {n}", file=sys.stderr)
        return 1
    unused = [v for v in MAP if v not in vs]
    if unused:
        print(f"映射表里有枚举里不存在的键: {unused}", file=sys.stderr)
        return 1
    lines = [
        f"            {v} => '\\u{{{byname[MAP[v].lower().replace('-', '_')]:x}}}', // {MAP[v]}"
        for v in vs
    ]
    print("\n".join(lines))
    if "--write" in sys.argv:
        body = (
            "\n    /// 图鉴里每条状态前显示一个 Nerd Font 图标.图标按**字形名**从字体 cmap 里挑\n"
            "    /// (映射表见 tools/nf_status_glyphs.py),源码里一律写 \\u{...} 转义,文件保持 ASCII.\n"
            "    /// 换图标只改那张表再重跑;先在 compendium 里看效果,确认后再往 HUD/战斗界面铺.\n"
            "    pub fn glyph(self) -> char {\n        use Status::*;\n        match self {\n"
            + "\n".join(lines)
            + "\n        }\n    }\n"
        )
        start = src.index("    /// 图鉴里每条状态前显示一个 Nerd Font 图标")
        end = src.index("\n    }\n", src.index("pub fn glyph(self) -> char {")) + len("\n    }\n")
        src = src[:start] + body + src[end:]
        open(STATUS_RS, "w", encoding="utf-8").write(src)
        print(f"已写回 {STATUS_RS}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
