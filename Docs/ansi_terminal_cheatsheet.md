
# 🧭 终端输出速查表：ANSI / ECMA-48

## 🎯 一、控制序列基础语法

| 组件 | 含义 | 示例 |
|------|------|------|
| `ESC` | 转义字符 (`\033` / `0x1B`) | |
| `[` | CSI（Control Sequence Introducer） | |
| `m` | SGR（Select Graphic Rendition）命令尾 | |
| 结构 | `ESC [ 参数1 ; 参数2 ; … m` | `\033[1;31mHello\033[0m` |

---

## ✨ 二、基本文本样式（SGR 参数）

| 参数 | 效果 | 说明 |
|------|------|------|
| `0` | 重置所有属性 | 常用于结尾 |
| `1` | 加粗 (Bold) |
| `2` | 变暗 (Dim) |
| `3` | 斜体 (Italic) |
| `4` | 下划线 (Underline) |
| `5` | 闪烁 (Blink) |
| `7` | 反色 (Reverse video) |
| `8` | 隐藏文字 (Conceal) |
| `9` | 删除线 (Strikethrough) |
| `22` | 取消加粗/变暗 |
| `23` | 取消斜体 |
| `24` | 取消下划线 |
| `25` | 取消闪烁 |
| `27` | 取消反色 |
| `28` | 取消隐藏 |
| `29` | 取消删除线 |

示例：
```bash
echo -e "\033[1;4;31mBold Underlined Red\033[0m"
```

---
## 🌈 五、真彩色（24 位 RGB）

格式：
```
前景: \033[38;2;R;G;Bm
背景: \033[48;2;R;G;Bm
```

示例：
```bash
echo -e "\033[48;2;0;128;255;38;2;255;255;255m Text on Blue BG \033[0m"
```

| 模式 | 说明 | 示例 |
|------|------|------|
| `38;2;R;G;B` | 设置前景色 | `\033[38;2;255;0;128m` |
| `48;2;R;G;B` | 设置背景色 | `\033[48;2;0;128;255m` |
| `0m` | 重置 | `\033[0m` |

---

## 🧩 六、组合样例

```bash
# 加粗 + 下划线 + 黄字 + 蓝底
echo -e "\033[1;4;33;44mStylish Text\033[0m"

# 真彩前景与背景
echo -e "\033[38;2;255;105;180;48;2;25;25;25mPink on Dark Gray\033[0m"

# 反色 + 删除线
echo -e "\033[7;9mReversed Strikethrough\033[0m"
```

---

## 🧪 七、测试终端支持

检测 24 位真彩支持：
```bash
curl -s https://raw.githubusercontent.com/JohnMorales/dotfiles/master/colors/24-bit-color.sh | bash
```

查看当前 `$TERM`：
```bash
echo $TERM
```

若为 `xterm-256color` 或支持 `Tc` 的设置，则通常支持真彩。

---

## ⚙️ 八、兼容性参考

| 终端 | 8/16色 | 256色 | 24位真彩 | 备注 |
|------|---------|--------|------------|------|
| xterm / gnome-terminal / konsole | ✅ | ✅ | ✅ | Linux 主流 |
| alacritty / kitty / wezterm | ✅ | ✅ | ✅ | 推荐使用 |
| Windows Terminal | ✅ | ✅ | ✅ | Win10+ |
| cmd.exe (旧) | ✅ | ⚠️ | ❌ | 部分 ANSI 支持 |
| PowerShell (旧版) | ✅ | ⚠️ | ❌ | 同上 |
| tmux/screen | ✅ | ✅ | ⚙️ 手动开启真彩 |
| VSCode Terminal | ✅ | ✅ | ✅ | Electron 环境 |

---

## 📚 九、参考文献与标准

- [ECMA-48 官方标准 PDF](https://www.ecma-international.org/publications-and-standards/standards/ecma-48/)
- ISO/IEC 6429（同 ECMA-48）
- ANSI X3.64 历史标准
- [Microsoft Console Virtual Terminal Sequences](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences)
- [Wikipedia: ANSI escape code](https://en.wikipedia.org/wiki/ANSI_escape_code)
