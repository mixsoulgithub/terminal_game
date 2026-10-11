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
    "Weak": "md-sword", "Frail": "md-shield_off", "Entangled": "fa-spider",
    "DemonForm": "fa-fire_flame_curved", "Metallicize": "fa-shield_halved",
    "FeelNoPain": "fa-bandage", "DarkEmbrace": "fa-moon_o", "Evolve": "md-cards",
    "FireBreathing": "fa-fire", "Barricade": "md-wall", "Brutality": "md-blood_bag",
    "Rupture": "fa-droplet_slash", "Regenerate": "md-heart_pulse", "Thorns": "md-cactus",
    "Berserk": "fa-face_grin_stars", "Combust": "fa-fire_flame_simple", "Corruption": "fa-skull",
    "DoubleTap": "md-sword_cross", "Juggernaut": "fa-helmet_safety", "Rage": "fa-face_angry",
    "FlameBarrier": "ple-flame_thick", "Artifact": "fa-gem", "NoBlock": "fa-ban",
    "NoDraw": "cod-circle_slash", "Mayhem": "fa-shuffle", "Magnetism": "fa-magnet",
    "Panache": "fa-feather", "SadisticNature": "fa-face_grin_tongue_squint",
    "PlatedArmor": "md-shield_plus", "Constricted": "md-snake", "Ritual": "fa-hands_praying",
    "Enrage": "fa-bullhorn", "Anger": "fa-face_grimace", "SharpHide": "fa-hand_fist",
    "BeatOfDeath": "fa-heartbeat", "Curiosity": "md-magnify", "CurlUp": "md-shield_plus_outline",
    "Explosive": "md-bomb", "Fading": "fa-hourglass_end", "Flight": "fa-dove", "Invincible": "fa-shield",
    "Malleable": "fa-shapes", "Minion": "fa-users", "MinionLeader": "fa-crown",
    "ModeShift": "fa-rotate", "PainfulStabs": "fa-syringe", "Reactive": "md-bolt",
    "Regrow": "fa-seedling", "Shifting": "fa-arrows_v", "Slow": "fa-clock_o",
    "SporeCloud": "fa-cloud", "Split": "md-source_branch", "Stasis": "fa-box_archive",
    "StrengthUp": "fa-angles_up", "TimeWarp": "fa-hourglass", "Asleep": "md-sleep",
    "Duplication": "fa-clone", "LoseStrength": "fa-arrow_down", "LoseDexterity": "cod-arrow_down",
    "Focus": "fa-bullseye", "Accuracy": "fa-crosshairs", "AfterImage": "fa-images",
    "ThousandCuts": "md-knife", "Envenom": "fa-flask", "InfiniteBlades": "fa-khanda",
    "NoxiousFumes": "fa-smog", "ToolsOfTheTrade": "fa-screwdriver_wrench", "WraithForm": "fa-ghost",
    "Blur": "fa-eye_slash", "PhantasmalKiller": "fa-user_secret", "Burst": "fa-forward",
    "WellLaidPlans": "cod-checklist", "Choke": "md-hand_front_right", "CorpseExplosion": "fa-bomb",
    "DoubleDamage": "fa-check_double", "Poison": "fa-skull_crossbones", "Confused": "md-help_circle",
    "Hex": "fa-hurricane", "DrawReduction": "md-cards_outline", "Surrounded": "md-account_group",
    "Intangible": "md-ghost",
}



# 状态 -> 图标配色键(键到具体颜色的映射在 src/ui/theme.rs 的 icon_color)
COLOR = {
    "Strength": "fire",
    "Dexterity": "good",
    "Vulnerable": "bad",
    "Weak": "bad",
    "Frail": "bad",
    "Entangled": "bad",
    "DemonForm": "fire",
    "Metallicize": "block",
    "FeelNoPain": "good",
    "DarkEmbrace": "good",
    "Evolve": "good",
    "FireBreathing": "fire",
    "Barricade": "block",
    "Brutality": "blood",
    "Rupture": "blood",
    "Regenerate": "good",
    "Thorns": "fire",
    "Berserk": "fire",
    "Combust": "fire",
    "Corruption": "arcane",
    "DoubleTap": "energy",
    "Juggernaut": "fire",
    "Rage": "block",
    "FlameBarrier": "block",
    "Artifact": "good",
    "NoBlock": "bad",
    "NoDraw": "energy",
    "Mayhem": "energy",
    "Magnetism": "good",
    "Panache": "good",
    "SadisticNature": "fire",
    "PlatedArmor": "block",
    "Constricted": "bad",
    "Ritual": "arcane",
    "Enrage": "arcane",
    "Anger": "arcane",
    "SharpHide": "block",
    "BeatOfDeath": "blood",
    "Curiosity": "arcane",
    "CurlUp": "good",
    "Explosive": "fire",
    "Fading": "ghost",
    "Flight": "block",
    "Invincible": "block",
    "Malleable": "block",
    "Minion": "plain",
    "MinionLeader": "plain",
    "ModeShift": "arcane",
    "PainfulStabs": "blood",
    "Reactive": "ghost",
    "Regrow": "good",
    "Shifting": "arcane",
    "Slow": "ghost",
    "SporeCloud": "block",
    "Split": "dim",
    "Stasis": "dim",
    "StrengthUp": "arcane",
    "TimeWarp": "ghost",
    "Asleep": "ghost",
    "Duplication": "energy",
    "LoseStrength": "blood",
    "LoseDexterity": "blood",
    "Focus": "good",
    "Accuracy": "good",
    "AfterImage": "good",
    "ThousandCuts": "fire",
    "Envenom": "poison",
    "InfiniteBlades": "good",
    "NoxiousFumes": "poison",
    "ToolsOfTheTrade": "good",
    "WraithForm": "ghost",
    "Blur": "block",
    "PhantasmalKiller": "fire",
    "Burst": "energy",
    "WellLaidPlans": "good",
    "Choke": "poison",
    "CorpseExplosion": "poison",
    "DoubleDamage": "fire",
    "Poison": "poison",
    "Confused": "arcane",
    "Hex": "arcane",
    "DrawReduction": "energy",
    "Surrounded": "arcane",
    "Intangible": "block",
}

# 状态 -> 候选图标的关键词(按关键词从字体字形名里搜,最多取 10 个;第一个是当前用的)
KEYWORDS = {
    "Strength": ["dumbbell", "weight", "muscle", "fist", "fitness", "strong"],
    "Dexterity": ["shoe_prints", "shoe", "running", "walk", "wind", "feather"],
    "Vulnerable": ["heart_crack", "heart_broken", "shield_halved", "crack", "broken", "target"],
    "Weak": ["sword", "slash", "dagger", "knife", "face_dizzy", "face_tired"],
    "Frail": ["shield", "crutch", "bone", "heart_broken"],
    "Entangled": ["spider", "web", "net", "rope", "chain"],
    "DemonForm": ["fire_flame_curved", "fire", "flame", "demon", "devil", "skull"],
    "Metallicize": ["shield_halved", "shield", "metal", "gear", "cog", "anvil"],
    "FeelNoPain": ["bandage", "heart", "hand_holding", "shield"],
    "DarkEmbrace": ["moon_o", "moon", "cloud_moon", "moon_cloud"],
    "Evolve": ["cards", "card", "arrow_up", "dna", "seedling"],
    "FireBreathing": ["fire", "fire_flame_simple", "dragon", "flame", "smog"],
    "Barricade": ["wall", "brick", "chess_rook", "castle", "shield"],
    "Brutality": ["blood_bag", "blood", "droplet", "heart", "syringe"],
    "Rupture": ["droplet_slash", "droplet", "blood", "syringe", "flask"],
    "Regenerate": ["heart_pulse", "heartbeat", "heart", "plus", "medkit", "bandage"],
    "Thorns": ["cactus", "thorn", "spike", "plant", "star", "sun"],
    "Berserk": ["face_grin_stars", "flame", "fire", "bolt", "skull", "rage"],
    "Combust": ["fire_flame_simple", "fire", "flame", "bomb", "burst", "smog"],
    "Corruption": ["skull", "biohazard", "radiation", "virus"],
    "DoubleTap": ["sword", "swords", "repeat", "copy", "clone"],
    "Juggernaut": ["helmet", "shield", "truck_fast", "bolt"],
    "Rage": ["face_angry", "angry", "face_grimace", "face_grin_stars", "angry_face"],
    "FlameBarrier": ["flame_thick", "flame", "fire", "shield", "wall"],
    "Artifact": ["gem", "diamond", "shield", "star", "shield_halved"],
    "NoBlock": ["ban", "circle_slash", "cancel", "shield_off", "shield"],
    "NoDraw": ["circle_slash", "ban", "hand_dots", "hand", "cards", "lock"],
    "Mayhem": ["shuffle", "dice", "random", "cards", "sparkles"],
    "Magnetism": ["magnet", "circle_dot", "magnet_flat"],
    "Panache": ["feather", "feather_pointed", "hat", "star", "sparkles"],
    "SadisticNature": ["face_grin_tongue_squint", "face_grin", "smile", "skull", "devil"],
    "PlatedArmor": ["shield_plus", "shield", "armor", "shield_halved", "vest"],
    "Constricted": ["snake", "vine", "ring", "compress", "circle"],
    "Ritual": ["hands_praying", "pray", "candle", "church", "star", "moon"],
    "Enrage": ["bullhorn", "megaphone", "angry", "face_angry", "bolt"],
    "Anger": ["face_grimace", "face_angry", "angry", "face_frown", "bolt"],
    "SharpHide": ["hand_fist", "shield_halved", "spike", "sword", "shield"],
    "BeatOfDeath": ["heartbeat", "heart_pulse", "heart", "heart_broken", "skull"],
    "Curiosity": ["magnify", "magnifying_glass", "search", "question", "eye"],
    "CurlUp": ["shield_plus_outline", "shield", "circle_notch", "snail", "shield_halved"],
    "Explosive": ["bomb", "bomb_off", "burst", "fire", "skull", "radiation"],
    "Fading": ["hourglass_end", "hourglass", "ghost", "clock", "hourglass_o"],
    "Flight": ["dove", "feather", "wing", "bird", "plane", "cloud"],
    "Invincible": ["shield", "shield_halved", "shield_plus", "castle", "crown"],
    "Malleable": ["shapes", "dice", "puzzle", "circle", "square"],
    "Minion": ["users", "user", "user_friends", "account_group", "person"],
    "MinionLeader": ["crown", "chess_king", "star", "skull", "flag"],
    "ModeShift": ["rotate", "arrows_rotate", "sync", "refresh", "shield"],
    "PainfulStabs": ["syringe", "injection", "needle", "dagger", "sword", "bolt"],
    "Reactive": ["bolt", "lightning", "bolt_auto", "flash", "zap"],
    "Regrow": ["seedling", "plant", "leaf", "sprout", "recycle", "heart"],
    "Shifting": ["arrows_v", "arrows_up_down", "arrows", "exchange", "refresh"],
    "Slow": ["clock_o", "clock", "hourglass", "snail", "tachometer"],
    "SporeCloud": ["cloud", "smog", "clouds", "fog", "poison", "spa"],
    "Split": ["source_branch", "code_branch", "clone", "fork", "share", "scissors"],
    "Stasis": ["box_archive", "box", "pause", "lock", "clock", "freeze"],
    "StrengthUp": ["angles_up", "arrow_up", "arrow_up_long", "chevron_up", "bolt"],
    "TimeWarp": ["hourglass", "clock", "hourglass_half", "hourglass_end", "stopwatch"],
    "Asleep": ["sleep", "moon", "zzz", "bed", "bell_sleep"],
    "Duplication": ["clone", "copy", "repeat", "files", "layer_group"],
    "LoseStrength": ["arrow_down", "arrow_down_long", "angles_down", "dumbbell"],
    "LoseDexterity": ["arrow_down", "arrow_down_long", "angles_down", "shoe_prints"],
    "Focus": ["bullseye", "crosshairs", "target", "eye", "circle_dot"],
    "Accuracy": ["crosshairs", "bullseye", "target", "dagger", "knife", "feather"],
    "AfterImage": ["images", "image", "ghost", "copy", "clone", "film"],
    "ThousandCuts": ["knife", "blade", "sword", "scissors", "khanda", "slash"],
    "Envenom": ["flask", "poison", "syringe", "skull_crossbones", "biohazard"],
    "InfiniteBlades": ["khanda", "sword", "swords", "dagger", "knife", "infinity"],
    "NoxiousFumes": ["smog", "cloud", "poison", "fog", "spa", "biohazard"],
    "ToolsOfTheTrade": ["screwdriver_wrench", "toolbox", "wrench", "gear", "tools"],
    "WraithForm": ["ghost", "ghost_alt", "spirit", "shield", "cloud"],
    "Blur": ["eye_slash", "low_vision", "eye", "cloud", "wind"],
    "PhantasmalKiller": ["user_secret", "mask", "spy", "dagger", "ghost", "eye"],
    "Burst": ["forward", "bolt", "fast_forward", "rocket", "angles_right"],
    "WellLaidPlans": ["checklist", "list_check", "clipboard", "list", "map", "compass"],
    "Choke": ["hand_front_right", "hand", "hand_fist", "hourglass", "skull"],
    "CorpseExplosion": ["bomb", "skull_crossbones", "fire", "burst", "radiation", "biohazard"],
    "DoubleDamage": ["check_double", "bolt", "dumbbell", "swords", "chevron_double"],
    "Poison": ["skull_crossbones", "flask", "biohazard", "skull", "droplet", "syringe"],
    "Confused": ["help_circle", "question", "face_dizzy", "shuffle", "dice"],
    "Hex": ["hurricane", "swirl", "vortex", "hexagon", "magic", "star"],
    "DrawReduction": ["cards_outline", "cards", "arrow_down", "minus", "hand"],
    "Surrounded": ["account_group", "users", "people", "arrows", "circle"],
    "Intangible": ["ghost", "shield", "spirit", "cloud", "eye_slash"],
}


# 状态 -> 原作的图标长什么样(据资料与观察;带 ? 的是不确定的,评审时可以纠正)
ORIGINAL = {
    "Strength": "屈臂肌肉",
    "Dexterity": "带翼的靴子/脚步",
    "Vulnerable": "裂开的心",
    "Weak": "断剑",
    "Frail": "破盾",
    "Entangled": "蛛网",
    "DemonForm": "恶魔头(带角)",
    "Metallicize": "金属齿轮",
    "FeelNoPain": "绷带",
    "DarkEmbrace": "暗色斗篷?",
    "Evolve": "卡牌+上箭头",
    "FireBreathing": "龙头吐火",
    "Barricade": "铁壁",
    "Brutality": "血滴",
    "Rupture": "血滴+上箭头",
    "Regenerate": "心+加号",
    "Thorns": "荆棘球/仙人球",
    "Berserk": "红色怒面",
    "Combust": "火焰",
    "Corruption": "紫色骷髅",
    "DoubleTap": "两把交叉的剑",
    "Juggernaut": "骑士头盔",
    "Rage": "怒面",
    "FlameBarrier": "火焰盾",
    "Artifact": "宝石/宝珠",
    "NoBlock": "带斜杠的盾",
    "NoDraw": "带斜杠的卡",
    "Mayhem": "混乱漩涡",
    "Magnetism": "磁铁",
    "Panache": "羽饰帽",
    "SadisticNature": "诡异的笑面",
    "PlatedArmor": "板甲盾",
    "Constricted": "缠绕的蛇/藤",
    "Ritual": "图腾/烛火?",
    "Enrage": "怒号/喇叭",
    "Anger": "怒面",
    "SharpHide": "尖刺盾",
    "BeatOfDeath": "心跳/心",
    "Curiosity": "放大镜/问号?",
    "CurlUp": "蜷缩的虫",
    "Explosive": "炸弹",
    "Fading": "沙漏",
    "Flight": "翅膀",
    "Invincible": "盾+锁",
    "Malleable": "变形方块",
    "Minion": "小怪剪影",
    "MinionLeader": "王冠",
    "ModeShift": "齿轮/盾",
    "PainfulStabs": "针/匕首",
    "Reactive": "闪电",
    "Regrow": "嫩芽",
    "Shifting": "双向箭头",
    "Slow": "沙漏",
    "SporeCloud": "孢子云",
    "Split": "分裂的史莱姆",
    "Stasis": "上锁的卡/盒子",
    "StrengthUp": "上箭头",
    "TimeWarp": "时钟/沙漏",
    "Asleep": "沉睡(月亮/zzz)",
    "Duplication": "两张卡",
    "LoseStrength": "向下箭头",
    "LoseDexterity": "向下箭头",
    "Focus": "球/眼睛",
    "Accuracy": "靶心+飞刀",
    "AfterImage": "残影/分身",
    "ThousandCuts": "许多刀刃",
    "Envenom": "毒牙/绿色毒滴?",
    "InfiniteBlades": "无尽的剑",
    "NoxiousFumes": "毒云",
    "ToolsOfTheTrade": "工具",
    "WraithForm": "幽魂",
    "Blur": "残像/抹影?",
    "PhantasmalKiller": "面具/幻影刺客",
    "Burst": "爆发前冲",
    "WellLaidPlans": "地图/计划",
    "Choke": "掐住的手",
    "CorpseExplosion": "爆炸的骷髅",
    "DoubleDamage": "双重影像",
    "Poison": "骷髅+毒",
    "Confused": "问号/漩涡",
    "Hex": "紫色漩涡",
    "DrawReduction": "卡牌+下箭头",
    "Surrounded": "两侧的敌人/人群",
    "Intangible": "幽魂",
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
    miss_c = [v for v in vs if v not in COLOR]
    if miss_c:
        print(f"没配颜色的状态: {miss_c}", file=sys.stderr)
        return 1
    used_keys = set(COLOR.values())
    known_keys = {"good", "bad", "block", "fire", "poison", "energy", "ghost", "arcane", "blood", "dim", "plain"}
    bad_keys = used_keys - known_keys
    if bad_keys:
        print(f"配色键不在 theme::icon_color 支持的集合里: {sorted(bad_keys)}", file=sys.stderr)
        return 1
    unused = [v for v in MAP if v not in vs]
    if unused:
        print(f"映射表里有枚举里不存在的键: {unused}", file=sys.stderr)
        return 1
    lines = [
        f"            {v} => '\\u{{{byname[MAP[v].lower().replace('-', '_')]:x}}}', // {MAP[v]}"
        for v in vs
    ]
    color_lines = [f'            {v} => "{COLOR[v]}",' for v in vs]

    def candidates(status: str):
        out = []
        picked = MAP[status].lower().replace("-", "_")
        # 当前用的排第一,其余按关键词从字形名里搜(名字排序保证稳定)
        out.append(picked)
        for kw in KEYWORDS.get(status, []):
            for n in sorted(byname):
                if kw in n and n not in out:
                    out.append(n)
                    if len(out) >= 10:
                        break
            if len(out) >= 10:
                break
        return out[:10]

    orig_lines = [f'            {v} => "{ORIGINAL.get(v, "?")}",' for v in vs]

    cand_lines = []
    for v in vs:
        items = candidates(v)
        pairs = ", ".join(f"('\\u{{{byname[n]:x}}}', \"{n.replace('_', '-')}\")" for n in items)
        cand_lines.append(f"            {v} => &[{pairs}],")
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
        body += (
            "\n    /// 图标配色键:键到颜色的映射在 theme::icon_color(先在 compendium 里审,\n"
            "    /// 确认后再往 HUD/战斗状态行铺).\n"
            "    pub fn icon_color_key(self) -> &'static str {\n        use Status::*;\n        match self {\n"
            + "\n".join(color_lines)
            + "\n        }\n    }\n"
        )
        body += (
            "\n    /// 原作这条状态的图标长什么样(据资料与观察;带 ? 的不确定,评审时纠正)\n"
            "    pub fn original_icon_note(self) -> &'static str {\n        use Status::*;\n        match self {\n"
            + "\n".join(orig_lines)
            + "\n        }\n    }\n"
        )
        body += (
            "\n    /// 候选图标(最多 10 个,第一个是当前用的):图鉴把这份清单附在说明后面,\n"
            "    /// 评审时按序号或字形名挑.候选也是按字形名从字体 cmap 里搜出来的,挑到的都在字体里.\n"
            "    pub fn glyph_candidates(self) -> &'static [(char, &'static str)] {\n        use Status::*;\n        match self {\n"
            + "\n".join(cand_lines)
            + "\n        }\n    }\n"
        )
        start = src.index("    /// 图鉴里每条状态前显示一个 Nerd Font 图标")
        # 生成区 = 从第一个生成函数的文档注释起,到最后一个生成函数(glyph_candidates)收尾
        end = src.index("\n    }\n", src.index("pub fn glyph_candidates(self)")) + len("\n    }\n")
        src = src[:start] + body + src[end:]
        open(STATUS_RS, "w", encoding="utf-8").write(src)
        print(f"已写回 {STATUS_RS}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
