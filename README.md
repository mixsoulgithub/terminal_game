# Terminal Games

一个基于 ncurses 库开发的终端游戏合集。正在抽象出可复用的框架.

## 项目结构

```
terminal_game
├── README.md
├── Snake/              # 正在开发
│   ├── CMakeLists.txt
│   └── src/
└── Tetris/             # 已实现
    ├── CMakeLists.txt
    └── src/
```

## 游戏列表

### 1. Snake (贪吃蛇)
- 经典贪吃蛇游戏
- 使用方向键控制蛇的移动
- 吃到食物增长身体长度
- 碰撞墙壁或自身游戏结束

### 2. Tetris (俄罗斯方块)
- 经典俄罗斯方块游戏
- 使用键盘控制方块移动和旋转
- 消除完整的行获得分数
- 方块堆到顶部游戏结束

## 构建要求
- 类Unix系统. 如Linux, MacOS.
- CMake (版本 3.10 或更高)
- C++ 编译器 (支持 C++17, 建议gcc11.3.0及以上)
- ncurses 库

### 安装依赖 


强烈推荐使用conda管理环境. Conda 是一个开源的包管理和环境管理系统，可以轻松地安装、运行和更新软件包及其依赖项，并能创建独立的项目环境，避免不同项目间的库版本冲突。 安装教程 https://www.anaconda.com/docs/getting-started/miniconda/install

安装完成后, 配置环境如下
```bash
conda create --name terminal_game
conda activate terminal_game
conda install gcc>=11.3.0 #如果系统gcc满足要求可跳过
conda install cmake>=3.10.0 #如果系统cmake满足要求可跳过
conda install ncurses=6.5
```

如果不想使用conda, 可以按如下配置. 

**Ubuntu/Debian:**
```bash
sudo apt-get install build-essential cmake libncurses5-dev
```
## 构建说明

```bash
# 创建构建目录
cd Snake #任意一个游戏目录
mkdir build
cd build

# 配置项目
cmake ..

# 构建游戏
cmake --build .
```


## 运行游戏

构建完成后，可执行文件将位于 `build/` 目录下：

```bash
# 运行贪吃蛇游戏
./Snake/snake_game

# 运行俄罗斯方块游戏
./Tetris/tetris_game
```


## 特性

- 跨平台支持 (Linux, macOS)
- 基于终端的图形界面
- 简单的构建系统
- 清晰的代码结构

## 许可证

本项目采用 MIT 许可证。详见 LICENSE 文件。

## 贡献

欢迎提交 Issue 和 Pull Request 来改进这个项目！

## 故障排除

如果遇到构建问题，请确保：
1. 已安装所有必需的依赖项
2. CMake 版本符合要求
3. ncurses 库已正确安装

如有其他问题，请在项目的 Issue 页面提交问题报告。