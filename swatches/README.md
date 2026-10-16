# swatches

一块终端色卡:整屏色块网格,用 vim 键位在上面挪光标,`yy` 把当前色的 `#RRGGBB` 塞进剪贴板。

色块之间的空隙、光标、信息栏都由 ratatui 的 Buffer diff 处理,静止时一帧只写一次,不动就不再向终端写字节。

## 为什么这么做

挑颜色时最烦的是"看得见但要手抄"。这里把两件事压在一起:一块能一眼扫完的色域,和一个只按两次 `y` 就能拿走的十六进制串。

`Docs/Color.md` 记过结论:终端对整屏彩色背景是有压力的。swatches 的网格是静态的,没有动画,所以只有按键时才产生输出。

## 技术栈

- Rust 2021(rustc 1.98)
- ratatui 0.30 / crossterm 0.29
- 颜色走 24 位真彩(`38;2;r;g;b` / `48;2;r;g;b`),不经终端调色板。需要 `COLORTERM=truecolor`
- 没有额外依赖:base64 编码自己写了 20 行

## 构建与测试

```bash
cd swatches
cargo build --release
cargo test
./target/release/swatches
```

## 用法

```bash
swatches [palette]
```

| 调色板 | 内容 |
| --- | --- |
| `hue`(默认) | 横轴色相、纵轴明度,随终端大小铺满整屏 |
| `sat` | 横轴色相、纵轴饱和度,底行接近白 |
| `ansi16` | 传统 16 色,8x2 居中 |
| `ansi256` | xterm 256 色表,16x16 居中 |
| `gray` | 32 级灰阶,一行居中 |

参数可以写短名(`hue` / `sat` / `ansi16` / `ansi256` / `gray`)或编号(`1` 到 `5`)。

## 键位

| 键 | 作用 | 键 | 作用 |
| --- | --- | --- | --- |
| `h` / `l`,左右方向键 | 左右挪一格色块 | `j` / `k`,上下方向键 | 上下挪一格 |
| `0` / `$` | 本行第一个 / 最后一个色块 | `g` `g` | 本列最上面那一格(列不动) |
| `G` | 本列最下面那一格(列不动) | `y` `y` | 复制当前色的 `#RRGGBB` |
| `tab` / `shift-tab` | 下一张 / 上一张色卡 | `1` .. `5` | 直接切到某张色卡 |
| `?` | 开关帮助 | `q` / `esc` / `ctrl-c` | 退出 |

光标画成一对方括号 `[]`,套在色块上,底色一点没挡住。`g`、`y` 都是两键序列:第一次按只是待命(底栏显示 `g- press g again...`),第二次按同一个键才动;此时按别的键是取消,按 `esc` 也是取消而不是退出。

## 复制是怎么送出去的

两条路一起走,谁成算谁:

1. **OSC 52**:往 stderr 写 `ESC ] 52 ; c ; <base64> BEL`。tmux 会把它转给外层终端(也就是真正设剪贴板的那一端),不在 tmux 里则终端自己处理。写 stderr 是为了不和 ratatui 写在 stdout 的帧交错。
2. **tmux load-buffer**:检测到 `$TMUX` 时直接 `tmux load-buffer -`,顺手把内容放进 tmux 自己的 paste buffer(`prefix + ]` 就能粘)。

底栏会显示实际用了哪条路,例如 `copied #FF8800 (osc52+tmux)`。

## 注意

- 环境里有 `NO_COLOR` 时,crossterm 会把所有颜色序列吞掉,色卡只剩空格子。swatches 检测到这种情况会在底栏直说:`NO_COLOR is set: colors are suppressed, unset it to see the swatches`。
- 终端小于 24x8 时只显示 `terminal too small`,不会画花。
- 固定尺寸的色卡放不下时贴着左上角裁,光标会自动贴到最近的实心色块上。
