# refs

只读参考:别人做的"终端杀戮尖塔"及其近亲。按 submodule 引入,方便 diff 与抄设计,
不参与本仓构建。看中某一份,再决定 fork 还是继续写 `../slay-the-spire`。

拉取/更新:

```fish
git submodule update --init --recursive
```

## 清单

| 目录 | 上游 | 固定在 | 技术栈 | 定位 |
|---|---|---|---|---|
| `end_of_eden` | BigJk/end_of_eden | v0.1.12-30 | Go + bubbletea/lipgloss | 完成度最高的"类尖塔纯控制台 roguelike",205 星,含 Lua mod 系统;2024-09 停更 |
| `slay-the-cli` | anthonykrivonos/slay-the-cli | v0.3.0 | TypeScript(零运行时依赖) | 原版机制的忠实复刻,49 星;结构最干净,适合当规则表查 |
| `slay-rust` | chloebrett/slay | main | Rust + ratatui 0.28 | **与本项目同栈**,分 `slay-core` / `slay-tui` / `slay-wasm` 三层;一天写完的实验品 |
| `sts2-cli` | wuhao21/sts2-cli | main | C# + Python 驱动 | 把真·杀戮尖塔2 引擎跑成 headless CLI;逻辑是原版,需自备 Steam 游戏 |
| `sts-textual-py` | q107580018/Slay-the-Spire | main | Python + textual/rich | 中文原型,已做到 act1-3、存读档、180 遗物录入;内容覆盖可参考 |

## 怎么跑

### slay-rust

```fish
cd refs/slay-rust
cargo run -p slay-tui        # 本机 rustc 1.98 已验证可编译
```

`crates/slay-core` 不含任何终端依赖,`crates/slay-tui` 才是 ratatui 层;
`crates/slay-wasm` 走浏览器。分层方式与 `../slay-the-spire/src/{core,ui}` 可直接对照。

### slay-the-cli

```fish
cd refs/slay-the-cli
bun install
bun run start:bun
bun test                     # 自带用例
```

### sts2-cli(要游戏本体)

需要 Steam 版杀戮尖塔2 + .NET 9 SDK:

```fish
cd refs/sts2-cli
./setup.sh                   # 从 Steam 目录拷 DLL 并打补丁
python3 python/play.py --lang zhs
```

### sts-textual-py

需要 Python >= 3.12(本机 3.14 可用),`uv` 未装,直接用 venv:

```fish
cd refs/sts-textual-py
python3 -m venv .venv
.venv/bin/pip install -e .
.venv/bin/slay-the-spire new
```

### end_of_eden

Go 1.27.1 已装在 `~/go`(GOROOT),与 `~/.bashrc` 里的 `GOROOT=$HOME/go` 对齐;
fish 侧在 `~/.config/fish/config.fish` 的 PATH 段补了 `$GOROOT/bin` 与 `$GOPATH/bin`。

拉依赖走 `GOPROXY=https://goproxy.cn,direct`(写在 `~/.config/go/env`),默认的
proxy.golang.org 在这台机器上直连超时。

本机没有 `libasound2-dev`,音频依赖 `hajimehoshi/oto` 编不过,用仓库自带的
`no_audio` 构建标签绕过(要声音就先 `sudo apt install libasound2-dev` 再普通构建):

```fish
cd refs/end_of_eden
go build -tags no_audio -o bin/end_of_eden ./cmd/game/
./bin/end_of_eden -audio=false   # 资源走相对路径 ./assets,cwd 必须是仓根
```

窗口版 `./cmd/game_win/` 还要 GL 环境,终端版不需要。
