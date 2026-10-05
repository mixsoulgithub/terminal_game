#!/usr/bin/env python3
"""给角色立绘生成亮度/对比度候选图,供终端预览从中挑一张。

defect.png 不动(预览里已经够清楚),默认做 watcher/silent/ironclad 三张。
ironclad 先按 CROP_RATIO 裁掉左边 2/5(那把剑放弃了),其余角色原尺寸不裁。

用法:
    python3 tools/tune_sprites.py                  # 处理默认三张
    python3 tools/tune_sprites.py ironclad         # 只处理指定的

输出:assets/characters/tuned/<名字>-<阶段>.png,外加同目录 manifest.csv。

档位:
  s1-b*      只提亮度:RGB 乘系数,alpha 原样保留
  s2-c*      只提对比度:以该图灰度自身均值为支点拉伸(PIL ImageEnhance 口径)
  s3-b*-c*   先亮度后对比度
  s4-auto*   对非透明像素做 1%/99% 分位线性拉伸(自动找黑白场),可再叠亮度

manifest.csv 记了每张候选的系数、改动前后非透明像素的平均亮度,以及画布尺寸。
"""
import csv
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from PIL import Image, ImageEnhance

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "characters"
OUT = SRC / "tuned"

DEFAULT_NAMES = ["watcher", "silent", "ironclad"]
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)

# 角色 -> 从左侧裁掉的比例(0/缺省表示不裁)。
# ironclad 那把横在左边的剑放弃了,留着只会把人物挤到右半边;watcher 整幅都要,不裁。
CROP_RATIO = {"ironclad": 0.4}

# manifest 里每个候选的参数列,缺的留空,保证每行列数一致
PARAM_COLS = ["b", "c", "auto"]


@dataclass
class Op:
    tag: str
    note: str
    bright: float = 1.0
    contrast: float = 1.0
    auto: bool = False


OPS = [
    Op("s1-b1.15", "只提亮 15%", bright=1.15),
    Op("s1-b1.30", "只提亮 30%", bright=1.30),
    Op("s1-b1.50", "只提亮 50%", bright=1.50),
    Op("s2-c1.15", "只提对比 15%", contrast=1.15),
    Op("s2-c1.30", "只提对比 30%", contrast=1.30),
    Op("s2-c1.50", "只提对比 50%", contrast=1.50),
    Op("s3-b1.15-c1.15", "提亮 15% 再提对比 15%", bright=1.15, contrast=1.15),
    Op("s3-b1.30-c1.30", "提亮 30% 再提对比 30%", bright=1.30, contrast=1.30),
    Op("s3-b1.15-c1.50", "提亮 15% 再提对比 50%", bright=1.15, contrast=1.50),
    Op("s3-b1.50-c1.15", "提亮 50% 再提对比 15%", bright=1.50, contrast=1.15),
    Op("s4-auto", "非透明像素 1%/99% 分位自动找黑白场", auto=True),
    Op("s4-auto-b1.30", "自动找黑白场后再提亮 30%", bright=1.30, auto=True),
]


def luma_of(rgb: np.ndarray) -> np.ndarray:
    return rgb @ LUMA


def apply_auto_levels(rgb: np.ndarray, mask: np.ndarray) -> np.ndarray:
    """只用非透明像素求 1%/99% 分位,把黑白场拉到 0/255。"""
    pix = rgb[mask].reshape(-1, 3)
    lo = np.percentile(pix, 1.0, axis=0)
    hi = np.percentile(pix, 99.0, axis=0)
    span = np.maximum(hi - lo, 1.0)
    return np.clip((rgb - lo) / span * 255.0, 0, 255)


def load(name: str) -> tuple[Image.Image, str]:
    """读原图;该裁的角色按 CROP_RATIO 去左侧,返回 RGBA 图和尺寸说明。"""
    im = Image.open(SRC / f"{name}.png").convert("RGBA")
    ratio = CROP_RATIO.get(name, 0.0)
    if not ratio:
        return im, f"{im.size[0]}x{im.size[1]}"
    w, h = im.size
    cut = round(w * ratio)
    im = im.crop((cut, 0, w, h))
    return im, f"{im.size[0]}x{im.size[1]}(左侧裁 {cut}px)"


def main() -> int:
    names = sys.argv[1:] or DEFAULT_NAMES
    OUT.mkdir(parents=True, exist_ok=True)

    rows: list[list[str]] = []
    for name in names:
        src = SRC / f"{name}.png"
        if not src.exists():
            print(f"跳过 {name}:没有 {src}", file=sys.stderr)
            continue

        im, size_note = load(name)
        rgb_img = im.convert("RGB")
        rgb0 = np.asarray(rgb_img, dtype=np.float32)
        a0 = np.asarray(im.getchannel("A"))
        opaque = a0 > 8
        before = float(luma_of(rgb0[opaque].reshape(-1, 3)).mean())

        for op in OPS:
            work = rgb_img
            if op.auto:
                work = Image.fromarray(apply_auto_levels(rgb0, opaque).astype(np.uint8), "RGB")
            if op.bright != 1.0:
                work = ImageEnhance.Brightness(work).enhance(op.bright)
            if op.contrast != 1.0:
                work = ImageEnhance.Contrast(work).enhance(op.contrast)

            work_rgb = np.asarray(work, dtype=np.float32)
            work_a = a0 > 8
            dst = OUT / f"{name}-{op.tag}.png"
            out_im = work.convert("RGBA")
            out_im.putalpha(im.getchannel("A"))  # alpha 原样,亮度/对比只作用在 RGB
            out_im.save(dst)

            after = float(luma_of(work_rgb[work_a].reshape(-1, 3)).mean())
            rows.append([dst.name, name, op.tag.split("-")[0], op.note,
                         *[{"b": f"{op.bright:.2f}", "c": f"{op.contrast:.2f}",
                            "auto": "yes" if op.auto else "no"}[k] for k in PARAM_COLS],
                         size_note, f"{before:.1f}", f"{after:.1f}"])

    manifest = OUT / "manifest.csv"
    with manifest.open("w", newline="") as f:
        wr = csv.writer(f)
        wr.writerow(["file", "sprite", "stage", "note", *PARAM_COLS, "size",
                     "luma_before", "luma_after"])
        wr.writerows(rows)

    print(f"生成 {len(rows)} 张候选到 {OUT}")
    print(f"清单 {manifest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
