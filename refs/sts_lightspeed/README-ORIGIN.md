# refs/sts_lightspeed —— 来源与用途

本目录是**只读的审计依据**:原版《Slay the Spire》机制的 C++ 反编译/重实现源码,
用来核对本项目各条数值与流程断言的出处。**不参与本仓构建**(本仓是 `../..`/`slay-the-spire` 的 Rust 工程)。

本目录就是各处注释里 `sts_lightspeed src/...`、`refs/sts_lightspeed/src/...` 这类引用所指的目标。

## 来源

| 项 | 值 |
|---|---|
| 上游 | https://github.com/gamerpuppy/sts_lightspeed |
| 分支 | `master` |
| commit | `7476a81954020087da31d41d16fddf475746ec2d` |
| tree hash | `670b2c3c8924b07944a741fa4e8e9a0cc29677d4` |
| commit 日期 | 2024-08-10 14:30:50 -0700 |
| commit 标题 | Merge pull request #5 from daniel-ziegler/fixes |
| 许可 | MIT,见同目录 `LICENSE.md` |

即该仓库在 2024-08 时间点上的 `master` 顶端(PR #5 之后)。上游后续若有新提交,本目录不会自动跟进。

## 获取时间与方式

- 复制时间:2026-10-10(UTC 2026-10-09T23:34)。
- 方式:`git archive HEAD`(在一份完整 clone 上执行)后解包到本目录,即**逐字节等于**上述 commit 的树;
  已用 `diff -r --exclude=.git` 与 clone 工作树比对,结果一致。
- 之所以不带 `.git`:本仓 `refs/` 下的其它参考是用 **submodule gitlink** 引入的;若这里嵌一个 `.git`,
  本仓只会记录一个 gitlink 提交号而**不存内容**,离线时依据链仍然断裂。因此这里存的是被追踪的普通文件,
  版本信息以本文件的 commit/tree hash 为准。

## 包含 / 排除

包含(上游全部 90 个文件;加上本说明文件共 91 个,约 1.4 MiB 磁盘 / 1.1 MiB 内容):

- `src/**`(29 个 .cpp:`combat/`、`game/`、`sim/`、`sim/search/`)
- `include/**`(50 个头文件:`combat/`、`constants/`、`data_structure/`、`game/`、`sim/`)
- `apps/`(3)、`bindings/`(3)
- 仓库根:`CMakeLists.txt`、`README.md`、`LICENSE.md`、`.gitignore`、`.gitmodules`

排除:

- `.git/`(380 KiB 历史对象)——原因见上"方式"一节。
- `json/`、`pybind11/`——上游是 submodule,在此 clone 中为空目录,无内容可存。

## 已知的引用入口(审计用)

以下符号/位置是本仓已在用的断言依据,路径均相对本目录,现已可复现:

- `src/combat/BattleContext.cpp` —— 药水使用(如 `Potion::FAIRY_POTION` 落到 `assert(false)` 的 `case`)、
  遗物钩子(如 `R::PRESERVED_INSECT` 的 `curHp = (int)(maxHp * .75)`)。
- `src/combat/Monster.cpp` —— `Monster::attackedUnblockedHelper`(受击结算顺序:格挡/目标侧之后)。
- `src/combat/MonsterGroup.cpp` —— `MonsterGroup::createMonsters`(开战阵容抽签与逐只构造,含血掷顺序)。
- `include/constants/Cards.h`、`Potions.h`、`Relics.h`、`RelicPools.h`、`Events.h`、
  `MonsterIds.h`、`CharacterClasses.h`、`PlayerStatusEffects.h` —— 语料/枚举 id 与稀有度表的事实来源。

## 使用约束

- 只作**查阅依据**,不抄进 `slay-the-spire/src` 以外的构建流程、不加进 `Cargo.toml`、不参与测试。
- 修改审计结论时,引用请写清 **本目录内的相对路径 + 行号**(如 `refs/sts_lightspeed/src/combat/Monster.cpp:339`)。
