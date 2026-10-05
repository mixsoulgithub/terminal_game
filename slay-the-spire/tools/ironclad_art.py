#!/usr/bin/env python3
"""20x10 的 ironclad:纯 ASCII 字符画,只用前景色(不动终端背景)。

两张 20x10 网格对着看:上面一张是要画的字符,下面一张是每格的颜色代号。
颜色代号:
    . 空   s 钢   b 蓝缠柄   k 近黑(手套)   d 深棕皮甲
    g 金   e 烬色眼缝   v 灰白发   p 土黄(护肩、腰带、靴)

画法按原图量出来的比例落格:剑刃在 row3 左侧(cols 0-7)、护手在第 8 列并上下
各伸一格、蓝缠柄两格、盔占 cols 11-15 三行、眼缝在盔的右前方、红裤 rows 6-8、
双腿外八(cols 9-10 与 16-17)、靴尖朝外的 (_ _)。

用法:
    python3 tools/ironclad_art.py          # 直接打印(24 位真彩前景色)
    python3 tools/ironclad_art.py --plain  # 只看字符,不上色
    python3 tools/ironclad_art.py --png /tmp/ironclad.png   # 出图核对
"""
import sys

ART = [
    "            _^_     ",
    "           (###)    ",
    "        |~~(#o#)    ",
    "<=======|HH%#%#:    ",
    "        | (#%#%:)   ",
    "          [==#==]   ",
    "         %######%   ",
    "         ###  ###   ",
    "         ##     %%  ",
    "        (__)   (__) ",
]

COL = [
    "            ggg     ",
    "           ggggg    ",
    "        svvggggg    ",
    "sssssssssbbkdddd    ",
    "        s pdddddp   ",
    "          ppppppp   ",
    "         rrrrrrrr   ",
    "         rrr  rrr   ",
    "         rr     rr  ",
    "        pppp   pppp ",
]

RGB = {
    "s": (172, 192, 202),
    "b": (58, 92, 150),
    "k": (28, 22, 20),
    "d": (74, 48, 34),
    "g": (211, 166, 62),
    "e": (255, 132, 45),
    "v": (168, 168, 164),
    "p": (150, 128, 88),
    "r": (168, 52, 48),
}

W, H = 20, 10
BG = (16, 16, 22)  # 仅 --png 用


def cells():
    """把两张网格合成 (字符, 颜色代号) 的二维表。"""
    for y in range(H):
        assert len(ART[y]) == W and len(COL[y]) == W, f"第 {y} 行列数不对"
        for x in range(W):
            char, col = ART[y][x], COL[y][x]
            if char == " ":
                assert col == " ", f"({y},{x}) 空格处不该有颜色"
                yield y, x, " ", " "
            else:
                yield y, x, char, col


def plain() -> str:
    return "\n".join(row.rstrip() for row in ART)


def colored() -> str:
    out = []
    for y in range(H):
        line = []
        for x in range(W):
            char, col = ART[y][x], COL[y][x]
            if char == " ":
                line.append(" ")
            else:
                r, g, b = RGB[col]
                line.append(f"\x1b[38;2;{r};{g};{b}m{char}")
        out.append("".join(line) + "\x1b[0m")
    return "\n".join(out)


def write_png(path: str, cw: int = 16, ch: int = 32) -> None:
    """用真实等宽字体栅格化一份,方便在图像查看器里核对。"""
    from PIL import Image, ImageDraw, ImageFont

    font = ImageFont.truetype(
        "/usr/share/fonts/truetype/hack/Hack-Regular.ttf", int(ch * 0.95)
    )
    img = Image.new("RGB", (W * cw, H * ch), BG)
    d = ImageDraw.Draw(img)
    for y, x, char, col in cells():
        if char == " ":
            continue
        d.text((x * cw, y * ch), char, font=font, fill=RGB[col], anchor="la")
    img.save(path)
    print(path)


def main() -> int:
    list(cells())  # 先做一次对齐检查
    args = sys.argv[1:]
    if args[:1] == ["--plain"]:
        print(plain())
    elif args[:1] == ["--png"]:
        write_png(args[1] if len(args) > 1 else "ironclad_art.png")
    else:
        print(colored())
    return 0


if __name__ == "__main__":
    sys.exit(main())
