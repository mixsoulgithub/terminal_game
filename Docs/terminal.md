# ncurse的颜色机制.
`printw`等输出函数不会立马将字符显示, 而是带着颜色属性`color_pair`, 也就是一个数字编号, 放在缓冲区里, 由`refresh()`将其一次性输出. 而输出时的颜色绑定, 是由最后一次`init_color(color_pair, A,B)`决定的.   

默认的颜色, 无论前景还是背景, 都是以256为周期循环. 先是几个纯色, 然后是一些主色带领的渐变色. 而可以组合出的颜色数量, 当然是`65536`, 但是默认的color_pair最多只能同时存在256种.

我进行了一系列测试, 当然它可以支持很多颜色, 但对于我希望的"现代终端显示能力的极限"来说, 这是非常混乱和糟糕的接口设计. ncurses或许更倾向于跨平台和稳定, 但游戏追求多姿多彩.


我有点想用自带的颜色输出了. 它所有的信息都在当前字符串信息里, 且组合无穷!
```bash
#检测终端的颜色支持情况
curl -s https://raw.githubusercontent.com/JohnMorales/dotfiles/master/colors/24-bit-color.sh | bash

printf "\033[48;2;0;128;255mText on blue background\033[0m\n"
# ESC → 转义字符，八进制 \033，十六进制 0x1B。

# [ → 表示进入“控制序列引导”（CSI, Control Sequence Introducer）。

# 48(背景颜色设置的代号); 2(RGB 3*8 = 24位真彩色模式);;0(R);128(G);255(B);m(结束);

# m → 表示这是一个“选择图形再现 (SGR, Select Graphic Rendition)”命令，即控制样式/颜色。

# 0(重置);
```


说白了, 我们只需要做到刷新就好. 
但有一个问题, `colortest2`实验表明, 终端的渲染能力有限. 彩色背景+彩色文字, 很快就卡卡的. 但单独背景或文字, 可以霓虹灯一样跑一段时间. 是内存还是计算的原因? 是终端的能力不足还是我程序优化不好? 终端显示的理论上限在哪? 

对于字符, 终端有以下分类.
| 类型             | 宽度 (cell) | 示例                   |
| -------------- | --------- | -------------------- |
| 半宽 (halfwidth) | 1         | ASCII, Latin, digits |
| 全宽 (fullwidth) | 2         | 中文、日文、韩文字符           |
| 宽 (wide)       | 2         | 部分 emoji             |
| 窄 (narrow)     | 1         | 其他符号                 |
| 组合 (combining) | 0         | 音标、重音符号等             |

https://www.qtccolor.com/secaiku/

## 清屏和光标控制

### 1. 清理屏幕内容

要获得一个完全干净的终端用于输出, 可以使用以下 ANSI 转义序列:

```bash
# 清屏 - 清除整个屏幕并移动光标到左上角 (0,0)
printf "\033[2J"

# 或者使用更完整的组合: 清屏 + 移动光标到左上角
printf "\033[2J\033[H"

# 清除从光标位置到屏幕末尾的所有内容
printf "\033[0J"  # 或 \033[J

# 清除从光标位置到行尾的内容
printf "\033[0K"  # 或 \033[K

# 清除从行首到光标位置的内容
printf "\033[1K"

# 清除整行
printf "\033[2K"
```

**常用组合:**
- `\033[2J\033[H` - 完全清屏并重置光标位置, 这是最常用的清屏方式
- `\033[2J` - 只清屏, 光标位置不变

### 2. 光标定位

将光标移动到指定位置:

```bash
# 移动光标到指定行列 (行号, 列号, 从1开始计数)
# 格式: \033[行号;列号H 或 \033[行号;列号f
printf "\033[10;20H"  # 移动到第10行第20列
printf "\033[10;20f"  # 同上, f 和 H 功能相同

# 移动光标到左上角 (1,1)
printf "\033[H"

# 相对移动
printf "\033[nA"  # 光标向上移动 n 行
printf "\033[nB"  # 光标向下移动 n 行
printf "\033[nC"  # 光标向右移动 n 列
printf "\033[nD"  # 光标向左移动 n 列

# 保存和恢复光标位置
printf "\033[s"   # 保存当前光标位置
printf "\033[u"   # 恢复到保存的光标位置

# 查询光标位置 (Device Status Report - DSR)
printf "\033[6n"  # 查询光标位置, 终端会返回 \033[row;columnR
```

**示例:**
```bash
# 清屏后在第5行第10列输出文字
printf "\033[2J\033[5;10HHello World\n"
```

### 2.1 获取当前光标位置

要获取当前光标所在的行和列, 可以使用 ANSI 转义序列查询:

```cpp
#include <iostream>
#include <unistd.h>
#include <termios.h>
#include <cstdio>
#include <string>

// 获取光标位置
bool get_cursor_position(int& row, int& col) {
    // 发送查询命令: Device Status Report (DSR)
    std::cout << "\033[6n";
    std::cout.flush();
    
    // 读取响应: 格式为 \033[row;columnR
    std::string response;
    char c;
    
    // 读取直到遇到 'R' (响应结束标志)
    while (true) {
        if (read(STDIN_FILENO, &c, 1) != 1) {
            return false;
        }
        response += c;
        if (c == 'R') {
            break;
        }
        // 防止无限循环
        if (response.length() > 32) {
            return false;
        }
    }
    
    // 解析响应: \033[row;columnR
    if (sscanf(response.c_str(), "\033[%d;%dR", &row, &col) == 2) {
        return true;
    }
    return false;
}

// 使用示例
int main() {
    // 注意: 在原始模式下更容易读取响应
    int row, col;
    if (get_cursor_position(row, col)) {
        std::cout << "当前光标位置: 行=" << row << ", 列=" << col << std::endl;
    } else {
        std::cout << "无法获取光标位置" << std::endl;
    }
    return 0;
}
```

**注意事项:**
- 查询命令 `\033[6n` 会立即返回响应, 需要从标准输入读取
- 响应格式为 `\033[row;columnR`, 其中 row 和 column 从 1 开始计数
- 在原始模式下更容易读取响应, 因为不会进行行缓冲
- 某些终端可能不支持此功能, 需要检查返回值

**重要问题处理:**

1. **输入缓冲区混淆问题**: 如果用户在查询前输入了字符, 这些字符会与响应混在一起. **解决方案**: 在查询前清空输入缓冲区:
   ```cpp
   #include <termios.h>
   void flush_input() {
       tcflush(STDIN_FILENO, TCIFLUSH);  // 清空输入缓冲区
   }
   
   // 在查询前调用
   flush_input();
   std::cout << "\033[6n";
   std::cout.flush();
   ```

2. **超时机制**: 如果终端不支持查询或响应丢失, 可能会无限等待. **解决方案**: 使用 `select()` 添加超时:
   ```cpp
   #include <sys/select.h>
   #include <sys/time.h>
   
   struct timeval timeout;
   timeout.tv_sec = 0;
   timeout.tv_usec = 500000;  // 500ms 超时
   
   fd_set readfds;
   FD_ZERO(&readfds);
   FD_SET(STDIN_FILENO, &readfds);
   
   int result = select(STDIN_FILENO + 1, &readfds, nullptr, nullptr, &timeout);
   if (result == 0) {
       // 超时
       return false;
   }
   ```

3. **回车键处理**: 在原始模式下, 回车键可能是 `\r` 或 `\n`, 或者两者都有. 需要正确处理:
   ```cpp
   // 等待回车时, 需要处理可能的 \r\n 组合
   char c;
   if (read(STDIN_FILENO, &c, 1) == 1) {
       if (c == '\r') {
           // 可能后面还有 \n, 需要检查
           // ...
       } else if (c == '\n') {
           // 通常 \n 已经足够
           // ...
       }
   }
   ```

### 3. 隐藏/显示光标

```bash
# 隐藏光标
printf "\033[?25l"  # 小写字母 l (lowercase L)

# 显示光标
printf "\033[?25h"  # 小写字母 h

# 示例: 隐藏光标, 清屏, 输出内容, 最后显示光标
printf "\033[?25l\033[2J\033[H"
# ... 你的输出代码 ...
printf "\033[?25h"
```

**注意事项:**
- 光标位置从 (1,1) 开始计数, 不是 (0,0)
- 隐藏光标后记得在程序退出前恢复显示, 否则终端会一直隐藏光标
- 清屏操作不会清除终端的滚动缓冲区, 只是清除当前可见区域
- 在 C++ 中可以使用 `std::cout << "\033[2J\033[H";` 来清屏

## 完全干净的终端环境

要获得一个完全干净的屏幕, 只有你自己的光标, 不显示bash提示符和任何系统信息, 需要进入**原始模式 (raw mode)**.

### 使用 termios 进入原始模式 (C++)

```cpp
#include <termios.h>
#include <unistd.h>
#include <iostream>

struct termios original_termios;

// 进入原始模式
void enable_raw_mode() {
    // 保存原始终端设置
    tcgetattr(STDIN_FILENO, &original_termios);
    
    struct termios raw = original_termios;
    
    // 禁用以下功能:
    raw.c_iflag &= ~(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    // BRKINT: 中断信号
    // ICRNL: 将 CR 转换为 NL
    // INPCK: 奇偶校验
    // ISTRIP: 剥离第8位
    // IXON: 软件流控 (Ctrl-S/Ctrl-Q)
    
    raw.c_oflag &= ~(OPOST);
    // OPOST: 输出后处理
    
    raw.c_cflag |= (CS8);
    // CS8: 8位字符大小
    
    raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);
    // ECHO: 回显输入字符 (关键! 禁用后输入不会显示)
    // ICANON: 规范模式 (禁用行缓冲, 立即读取每个字符)
    // IEXTEN: 扩展输入处理
    // ISIG: 信号字符 (Ctrl-C, Ctrl-Z 等)
    
    raw.c_cc[VMIN] = 0;   // 最小读取字符数 (0 = 非阻塞)
    raw.c_cc[VTIME] = 0;  // 超时时间 (0 = 立即返回)
    
    // 应用新设置
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
}

// 恢复原始终端设置
void disable_raw_mode() {
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &original_termios);
}

int main() {
    // 进入原始模式
    enable_raw_mode();
    
    // 清屏并隐藏光标
    std::cout << "\033[2J\033[H\033[?25l";
    std::cout.flush();
    
    // 现在你有一个完全干净的屏幕
    // 输入不会回显, 每个按键立即读取
    // 只有你自己控制的光标和输出
    
    // ... 你的游戏循环 ...
    
    // 退出前恢复
    std::cout << "\033[?25h";  // 显示光标
    disable_raw_mode();        // 恢复终端设置
    
    return 0;
}
```

### 关键设置说明

- **`ECHO` 禁用**: 输入字符不会回显到屏幕, 完全由你控制显示
- **`ICANON` 禁用**: 禁用行缓冲, 每个按键立即读取, 不需要按回车
- **`ISIG` 禁用**: Ctrl-C 等信号不会终止程序 (你可能需要手动处理退出)
- **`VMIN = 0, VTIME = 0`**: 非阻塞读取, 如果没有输入立即返回

### 简单的使用示例

```cpp
#include <termios.h>
#include <unistd.h>
#include <iostream>

struct termios original_termios;

void setup_terminal() {
    tcgetattr(STDIN_FILENO, &original_termios);
    struct termios raw = original_termios;
    raw.c_lflag &= ~(ECHO | ICANON);
    raw.c_cc[VMIN] = 0;
    raw.c_cc[VTIME] = 0;
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
    
    // 清屏 + 隐藏光标
    std::cout << "\033[2J\033[H\033[?25l";
    std::cout.flush();
}

void restore_terminal() {
    std::cout << "\033[?25h";  // 显示光标
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &original_termios);
}

int main() {
    setup_terminal();
    
    // 现在屏幕完全干净, 只有你的输出
    std::cout << "\033[10;20HHello, clean terminal!\n";
    
    // 读取单个字符 (非阻塞)
    char c;
    while (read(STDIN_FILENO, &c, 1) == 1 && c != 'q') {
        // 处理输入
    }
    
    restore_terminal();
    return 0;
}
```

### 注意事项

1. **必须恢复终端设置**: 程序退出前必须调用 `disable_raw_mode()`, 否则终端会一直处于原始模式
2. **信号处理**: 考虑添加信号处理器 (SIGINT, SIGTERM) 来确保异常退出时也能恢复终端
3. **Ctrl-C 处理**: 在原始模式下, Ctrl-C 不会自动终止程序, 需要手动检测并处理
4. **窗口大小变化**: 如果终端窗口大小改变, 需要重新获取并处理 (使用 SIGWINCH 信号)
5. **回车键和换行处理**: 
   - 如果**禁用了 `ICRNL`**: 回车键会返回 `\r` (CR) 而不是 `\n` (NL), 需要手动转换
   - 如果**启用了 `ICRNL`**: 回车键会被转换为 `\n`, 但由于禁用了 `OPOST` (输出后处理), `\n` 只会换行而不会移动到行首
   - **解决方案**: 检测到 `\n` 或 `\r` 时, 输出 `\r\n` 组合来实现换行并移动到行首:
   ```cpp
   if (c == '\n' || c == '\r') {
       std::cout << "\r\n";  // 回车(移动到行首) + 换行(移动到下一行)
   } else {
       std::cout << c;
   }
   ```
6. **退格键 (Backspace) 处理**: 
   - 在原始模式下, 由于禁用了 `ICANON` (规范模式), 退格键不会自动删除字符
   - **Backspace 键通常发送**: ASCII 127 (DEL) 或 ASCII 8 (BS, `\b`)
   - **解决方案**: 检测到退格键时, 手动实现删除功能:
   ```cpp
   else if (c == 127 || c == '\b') {
       // 退格: 光标左移 -> 输出空格覆盖字符 -> 光标再左移
       std::cout << "\b \b";
   }
   ```
   - 注意: 这只是简单的视觉删除, 如果需要在内存中维护输入缓冲区, 需要额外的逻辑

## 获取和改变终端大小

### 1. 获取终端大小

要获取当前终端窗口的大小 (行数和列数), 可以使用 `ioctl()` 系统调用:

```cpp
#include <sys/ioctl.h>
#include <unistd.h>
#include <iostream>

// 获取终端大小
bool get_terminal_size(int& rows, int& cols) {
    struct winsize w;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == -1) {
        return false;
    }
    rows = w.ws_row;
    cols = w.ws_col;
    return true;
}

// 使用示例
int main() {
    int rows, cols;
    if (get_terminal_size(rows, cols)) {
        std::cout << "终端大小: " << rows << " 行 x " << cols << " 列" << std::endl;
    } else {
        std::cerr << "无法获取终端大小" << std::endl;
    }
    return 0;
}
```

**`winsize` 结构体说明:**

```cpp
struct winsize {
    unsigned short ws_row;    // 行数 (高度)
    unsigned short ws_col;    // 列数 (宽度)
    unsigned short ws_xpixel; // 水平像素数 (通常不使用)
    unsigned short ws_ypixel; // 垂直像素数 (通常不使用)
};
```

**注意事项:**
- `TIOCGWINSZ` 是 "Terminal I/O Control Get Window Size" 的缩写
- 如果终端不支持此功能, `ioctl()` 会返回 -1
- 也可以使用 `STDIN_FILENO` 或 `STDERR_FILENO`, 但通常使用 `STDOUT_FILENO`
- 某些情况下, 如果终端大小未知, `ws_row` 和 `ws_col` 可能为 0

### 2. 改变终端大小

**注意**: 程序通常**不能直接改变终端窗口的大小**, 因为终端大小是由终端模拟器 (如 xterm, gnome-terminal, Windows Terminal 等) 控制的, 而不是由运行在其中的程序控制.

但是, 你可以:

#### 2.1 通过 ANSI 转义序列请求改变大小 (部分终端支持)

某些终端模拟器支持通过 ANSI 转义序列来请求改变窗口大小:

```cpp
// 请求终端窗口改变为指定大小 (行数 x 列数)
// 注意: 这只是一个请求, 终端可能忽略此命令
std::cout << "\033[8;" << rows << ";" << cols << "t";
std::cout.flush();
```

**示例:**
```cpp
// 请求终端窗口改变为 30 行 x 80 列
std::cout << "\033[8;30;80t";
std::cout.flush();
```

**重要提示:**
- 此功能**不是标准功能**, 许多终端不支持
- 即使支持, 终端也可能因为窗口管理器的限制而无法改变大小
- 建议仅用于兼容性测试, 不要依赖此功能

#### 2.2 通过环境变量获取默认大小

如果无法通过 `ioctl()` 获取大小, 可以尝试从环境变量获取:

```cpp
#include <cstdlib>
#include <iostream>

void get_terminal_size_from_env(int& rows, int& cols) {
    const char* rows_str = std::getenv("LINES");
    const char* cols_str = std::getenv("COLUMNS");
    
    if (rows_str) {
        rows = std::atoi(rows_str);
    }
    if (cols_str) {
        cols = std::atoi(cols_str);
    }
}
```

**注意**: 环境变量可能不存在或不准确, 应作为备选方案.

### 3. 处理窗口大小变化信号 (SIGWINCH)

当用户调整终端窗口大小时, 系统会向进程发送 `SIGWINCH` 信号. 程序应该捕获此信号并重新获取终端大小:

```cpp
#include <sys/ioctl.h>
#include <signal.h>
#include <unistd.h>
#include <iostream>

// 全局变量存储终端大小
int terminal_rows = 24;
int terminal_cols = 80;

// 信号处理函数
void handle_winch(int sig) {
    (void)sig;  // 避免未使用参数警告
    struct winsize w;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) != -1) {
        terminal_rows = w.ws_row;
        terminal_cols = w.ws_col;
        std::cout << "\033[2J\033[H";  // 清屏
        std::cout << "终端大小已改变: " << terminal_rows 
                  << " 行 x " << terminal_cols << " 列" << std::endl;
        std::cout.flush();
    }
}

int main() {
    // 注册信号处理器
    signal(SIGWINCH, handle_winch);
    
    // 初始化时获取终端大小
    struct winsize w;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) != -1) {
        terminal_rows = w.ws_row;
        terminal_cols = w.ws_col;
    }
    
    std::cout << "当前终端大小: " << terminal_rows 
              << " 行 x " << terminal_cols << " 列" << std::endl;
    std::cout << "请调整终端窗口大小, 程序会自动检测..." << std::endl;
    
    // 主循环
    while (true) {
        // 你的游戏逻辑...
        sleep(1);
    }
    
    return 0;
}
```

**使用 `sigaction` 的改进版本** (推荐):

```cpp
#include <sys/ioctl.h>
#include <signal.h>
#include <unistd.h>
#include <iostream>

int terminal_rows = 24;
int terminal_cols = 80;
volatile sig_atomic_t window_resized = 0;

void handle_winch(int sig) {
    (void)sig;
    window_resized = 1;  // 设置标志, 在主循环中处理
}

int main() {
    // 使用 sigaction 注册信号处理器 (比 signal() 更可靠)
    struct sigaction sa;
    sa.sa_handler = handle_winch;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = 0;
    sigaction(SIGWINCH, &sa, nullptr);
    
    // 初始化终端大小
    struct winsize w;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) != -1) {
        terminal_rows = w.ws_row;
        terminal_cols = w.ws_col;
    }
    
    // 主循环
    while (true) {
        // 检查窗口大小是否改变
        if (window_resized) {
            window_resized = 0;
            struct winsize w;
            if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) != -1) {
                terminal_rows = w.ws_row;
                terminal_cols = w.ws_col;
                // 重新绘制界面
                std::cout << "\033[2J\033[H";
                std::cout << "终端大小: " << terminal_rows 
                          << " 行 x " << terminal_cols << " 列" << std::endl;
                std::cout.flush();
            }
        }
        
        // 你的游戏逻辑...
        usleep(100000);  // 100ms
    }
    
    return 0;
}
```

**注意事项:**
- `SIGWINCH` 信号在窗口大小改变时发送, 但信号处理函数中不应执行复杂操作
- 使用 `volatile sig_atomic_t` 类型的标志变量在主循环中处理窗口大小变化
- 在信号处理函数中调用 `ioctl()` 通常是安全的, 但应避免调用非异步信号安全的函数 (如 `printf`, `malloc` 等)
- 某些系统可能不支持 `SIGWINCH` 信号, 需要检查返回值

### 4. 完整的终端大小管理示例

```cpp
#include <sys/ioctl.h>
#include <signal.h>
#include <unistd.h>
#include <iostream>
#include <termios.h>

class TerminalSize {
private:
    int rows_;
    int cols_;
    volatile sig_atomic_t resized_;
    
    static void winch_handler(int sig) {
        (void)sig;
        // 使用静态成员或全局变量来设置标志
        // 这里简化处理, 实际使用时需要访问实例
    }
    
public:
    TerminalSize() : rows_(24), cols_(80), resized_(0) {
        update();
        setup_signal_handler();
    }
    
    bool update() {
        struct winsize w;
        if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == -1) {
            return false;
        }
        rows_ = w.ws_row;
        cols_ = w.ws_col;
        return true;
    }
    
    void setup_signal_handler() {
        struct sigaction sa;
        sa.sa_handler = [](int sig) {
            (void)sig;
            // 注意: 在信号处理函数中不能直接访问非静态成员
            // 需要使用全局变量或静态成员
        };
        sigemptyset(&sa.sa_mask);
        sa.sa_flags = 0;
        sigaction(SIGWINCH, &sa, nullptr);
    }
    
    int rows() const { return rows_; }
    int cols() const { return cols_; }
    bool was_resized() const { return resized_ != 0; }
    void clear_resized_flag() { resized_ = 0; }
};

int main() {
    TerminalSize term_size;
    
    std::cout << "终端大小: " << term_size.rows() 
              << " 行 x " << term_size.cols() << " 列" << std::endl;
    
    // 主循环
    while (true) {
        if (term_size.was_resized()) {
            term_size.update();
            term_size.clear_resized_flag();
            std::cout << "\033[2J\033[H";
            std::cout << "新终端大小: " << term_size.rows() 
                      << " 行 x " << term_size.cols() << " 列" << std::endl;
            std::cout.flush();
        }
        
        // 你的游戏逻辑...
        usleep(100000);
    }
    
    return 0;
}
```

## Termios 控制位详细说明

以下内容基于 `/home/mix/miniconda3/x86_64-conda-linux-gnu/sysroot/usr/include/bits/termios-*.h` 文件中的定义.

### c_iflag (输入模式标志)

控制输入字符的处理方式:

| 标志 | 值 | 说明 |
|------|-----|------|
| **IGNBRK** | 0000001 | **忽略中断条件 (break condition)**. 当设置为1时, 忽略接收到的 break 信号 |
| **BRKINT** | 0000002 | **中断时发送信号**. 当接收到 break 信号时, 向进程发送 SIGINT 信号. 如果同时设置了 IGNBRK, 则此标志无效 |
| **IGNPAR** | 0000004 | **忽略奇偶校验错误**. 忽略带有奇偶校验错误的字符 |
| **PARMRK** | 0000010 | **标记奇偶校验和帧错误**. 当检测到奇偶校验或帧错误时, 在字符前插入特殊标记序列 (通常为 \377\0) |
| **INPCK** | 0000020 | **启用输入奇偶校验检查**. 启用对接收字符的奇偶校验检查 |
| **ISTRIP** | 0000040 | **剥离第8位**. 将输入字符的第8位 (最高位) 剥离, 只保留7位 |
| **INLCR** | 0000100 | **将 NL 映射为 CR**. 将输入中的换行符 (NL, \n) 转换为回车符 (CR, \r) |
| **IGNCR** | 0000200 | **忽略 CR**. 忽略输入中的回车符 (CR, \r) |
| **ICRNL** | 0000400 | **将 CR 映射为 NL**. 将输入中的回车符 (CR, \r) 转换为换行符 (NL, \n). **在原始模式下通常禁用此标志, 所以回车键会返回 \r 而不是 \n** |
| **IUCLC** | 0001000 | **将大写字母映射为小写** (非 POSIX). 将输入中的大写字母转换为小写 |
| **IXON** | 0002000 | **启用输出流控**. 启用 XON/XOFF 流控, 允许使用 Ctrl-S (XOFF) 暂停输出, Ctrl-Q (XON) 恢复输出. **在原始模式下通常禁用** |
| **IXANY** | 0004000 | **允许任意字符重启输出**. 允许任何字符 (不仅仅是 XON) 来重启被暂停的输出 |
| **IXOFF** | 0010000 | **启用输入流控**. 启用输入流的 XON/XOFF 流控 |
| **IMAXBEL** | 0020000 | **输入队列满时响铃** (非 POSIX). 当输入队列满时, 终端会响铃 |
| **IUTF8** | 0040000 | **输入为 UTF-8** (非 POSIX). 指示输入使用 UTF-8 编码 |

### c_oflag (输出模式标志)

控制输出字符的处理方式:

| 标志 | 值 | 说明 |
|------|-----|------|
| **OPOST** | 0000001 | **输出后处理**. 启用输出后处理, 允许进行字符映射和延迟处理. **在原始模式下通常禁用, 输出直接发送到终端. 这意味着 `\n` 只会换行而不会移动到行首, 需要手动输出 `\r\n` 来实现完整的换行功能** |
| **OLCUC** | 0000002 | **将小写字母映射为大写** (非 POSIX). 将输出中的小写字母转换为大写 |
| **ONLCR** | 0000004 | **将 NL 映射为 CR-NL**. 输出换行符 (NL) 时, 先输出回车符 (CR), 再输出换行符. **注意: 此功能需要 OPOST 启用才有效** |
| **OCRNL** | 0000010 | **将 CR 映射为 NL**. 将输出中的回车符 (CR) 转换为换行符 (NL). **注意: 此功能需要 OPOST 启用才有效** |
| **ONOCR** | 0000020 | **第0列不输出 CR**. 当光标在第0列时, 不输出回车符. **注意: 此功能需要 OPOST 启用才有效** |
| **ONLRET** | 0000040 | **NL 执行 CR 功能**. 换行符 (NL) 同时执行回车符的功能 (将光标移到行首). **注意: 此功能需要 OPOST 启用才有效** |
| **OFILL** | 0000100 | **使用填充字符延迟**. 使用填充字符来实现延迟, 而不是使用时间延迟 |
| **OFDEL** | 0000200 | **填充字符为 DEL**. 如果设置了 OFILL, 使用 DEL 字符作为填充字符, 否则使用 NULL 字符 |

**延迟标志** (需要 OPOST 启用):

| 标志 | 值 | 说明 |
|------|-----|------|
| **NLDLY** | 0000400 | **换行延迟选择**: NL0 (无延迟), NL1 (延迟) |
| **CRDLY** | 0003000 | **回车延迟选择**: CR0 (无延迟), CR1/CR2/CR3 (不同延迟类型) |
| **TABDLY** | 0014000 | **制表符延迟选择**: TAB0 (无延迟), TAB1/2 (延迟), TAB3 (扩展为空格) |
| **BSDLY** | 0020000 | **退格延迟选择**: BS0 (无延迟), BS1 (延迟) |
| **VTDLY** | 0040000 | **垂直制表符延迟选择**: VT0 (无延迟), VT1 (延迟) |
| **FFDLY** | 0100000 | **换页延迟选择**: FF0 (无延迟), FF1 (延迟) |

### c_cflag (控制模式标志)

控制硬件相关的设置:

| 标志 | 值 | 说明 |
|------|-----|------|
| **CSIZE** | 0000060 | **字符大小掩码**. 用于选择字符大小: CS5 (5位), CS6 (6位), CS7 (7位), CS8 (8位) |
| **CS5** | 0000000 | **5位字符** |
| **CS6** | 0000020 | **6位字符** |
| **CS7** | 0000040 | **7位字符** |
| **CS8** | 0000060 | **8位字符**. **通常使用此设置以支持完整的 ASCII 和扩展字符集** |
| **CSTOPB** | 0000100 | **发送两个停止位**. 如果设置, 发送两个停止位, 否则发送一个 |
| **CREAD** | 0000200 | **启用接收器**. 允许接收字符 |
| **PARENB** | 0000400 | **启用奇偶校验**. 启用奇偶校验位的生成和检测 |
| **PARODD** | 0001000 | **奇校验**. 如果设置了 PARENB, 使用奇校验, 否则使用偶校验 |
| **HUPCL** | 0002000 | **挂断时关闭**. 当最后一个进程关闭终端时, 挂断调制解调器 |
| **CLOCAL** | 0004000 | **本地连接**. 忽略调制解调器控制线, 假设是本地连接 |

**扩展标志** (非 POSIX):

| 标志 | 值 | 说明 |
|------|-----|------|
| **CMSPAR** | 010000000000 | **标记或空格奇偶校验** |
| **CRTSCTS** | 020000000000 | **硬件流控**. 启用 RTS/CTS 硬件流控 |

### c_lflag (本地模式标志)

控制终端驱动程序的本地处理:

| 标志 | 值 | 说明 |
|------|-----|------|
| **ISIG** | 0000001 | **启用信号**. 当接收到特殊字符 (INTR, QUIT, SUSP) 时, 向进程发送相应的信号. **在原始模式下通常禁用** |
| **ICANON** | 0000002 | **规范模式**. 启用规范输入模式, 允许行编辑 (退格、删除等) 和行缓冲. **在原始模式下必须禁用, 以实现逐字符读取** |
| **ECHO** | 0000010 | **回显输入字符**. 将输入的字符回显到终端. **在原始模式下通常禁用, 由程序控制显示** |
| **ECHOE** | 0000020 | **回显删除字符为退格**. 在规范模式下, 删除字符时回显为退格-空格-退格序列 |
| **ECHOK** | 0000040 | **回显 KILL**. 在规范模式下, 接收到 KILL 字符时回显换行 |
| **ECHONL** | 0000100 | **回显 NL**. 即使禁用了 ECHO, 也回显换行符 |
| **NOFLSH** | 0000200 | **禁用刷新**. 在接收到 INTR 或 QUIT 信号后, 不清空输入和输出队列 |
| **TOSTOP** | 0000400 | **后台输出时发送 SIGTTOU**. 当后台进程尝试写入终端时, 发送 SIGTTOU 信号 |
| **IEXTEN** | 0100000 | **启用扩展输入处理**. 启用实现定义的输入处理扩展. **在原始模式下通常禁用** |

**扩展标志** (非 POSIX):

| 标志 | 值 | 说明 |
|------|-----|------|
| **ECHOCTL** | 0001000 | **回显控制字符为 ^X**. 如果同时设置了 ECHO, 将控制字符回显为 ^X 格式 (例如 Ctrl-A 显示为 ^A) |
| **ECHOPRT** | 0002000 | **打印删除的字符**. 在规范模式下, 删除字符时打印它们 |
| **ECHOKE** | 0004000 | **KILL 字符的视觉删除**. 在规范模式下, KILL 字符通过视觉删除每个字符来回显 |
| **FLUSHO** | 0010000 | **输出正在刷新**. 此标志由 DISCARD 字符切换, 表示输出正在被刷新 |
| **PENDIN** | 0040000 | **重新打印输入队列**. 当下一个字符被读取时, 重新打印输入队列中的所有字符 |
| **EXTPROC** | 0200000 | **扩展处理** |

### c_cc[] (控制字符数组)

定义特殊控制字符的值. 数组索引定义如下:

| 索引 | 宏名 | 说明 |
|------|------|------|
| 0 | **VINTR** | **中断字符** (通常是 Ctrl-C). 发送 SIGINT 信号 |
| 1 | **VQUIT** | **退出字符** (通常是 Ctrl-\). 发送 SIGQUIT 信号 |
| 2 | **VERASE** | **删除字符** (通常是 Backspace 或 Ctrl-H). 在规范模式下删除前一个字符 |
| 3 | **VKILL** | **删除行字符** (通常是 Ctrl-U). 在规范模式下删除整行 |
| 4 | **VEOF** | **文件结束字符** (通常是 Ctrl-D). 在规范模式下表示输入结束 |
| 5 | **VTIME** | **非规范模式下的超时时间** (以 0.1 秒为单位). 与 VMIN 配合使用 |
| 6 | **VMIN** | **非规范模式下的最小字符数**. 与 VTIME 配合使用:
- VMIN=0, VTIME=0: 非阻塞, 立即返回
- VMIN>0, VTIME=0: 阻塞直到读取到 VMIN 个字符
- VMIN=0, VTIME>0: 阻塞直到读取到至少1个字符或超时
- VMIN>0, VTIME>0: 阻塞直到读取到 VMIN 个字符或超时 |
| 7 | **VSWTC** | **软件流控字符** (通常未使用) |
| 8 | **VSTART** | **开始字符** (通常是 Ctrl-Q). 用于 XON/XOFF 流控 |
| 9 | **VSTOP** | **停止字符** (通常是 Ctrl-S). 用于 XON/XOFF 流控 |
| 10 | **VSUSP** | **挂起字符** (通常是 Ctrl-Z). 发送 SIGTSTP 信号 |
| 11 | **VEOL** | **行结束字符** (通常是 0). 在规范模式下表示行结束 |
| 12 | **VREPRINT** | **重新打印字符** (通常是 Ctrl-R). 重新打印当前输入行 |
| 13 | **VDISCARD** | **丢弃字符** (通常是 Ctrl-O). 切换 FLUSHO 标志 |
| 14 | **VWERASE** | **单词删除字符** (通常是 Ctrl-W). 删除前一个单词 |
| 15 | **VLNEXT** | **字面量下一个字符** (通常是 Ctrl-V). 使下一个字符按字面意思解释 |
| 16 | **VEOL2** | **第二个行结束字符** (通常是 0) |

### 原始模式典型配置

在原始模式下, 通常的配置是:

```cpp
// c_iflag: 禁用大部分输入处理
raw.c_iflag &= ~(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
// 注意: ICRNL 被禁用, 所以回车键返回 \r 而不是 \n

// c_oflag: 禁用输出后处理
raw.c_oflag &= ~(OPOST);

// c_cflag: 设置8位字符
raw.c_cflag |= (CS8);

// c_lflag: 禁用规范模式、回显、信号处理
raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);

// c_cc: 非阻塞读取
raw.c_cc[VMIN] = 0;
raw.c_cc[VTIME] = 0;
```

**重要提示**: 由于禁用了 `ICRNL`, 在原始模式下读取回车键会得到 `\r` (ASCII 13) 而不是 `\n` (ASCII 10). 如果需要在输出时换行, 需要手动转换:

```cpp
char c;
if (read(STDIN_FILENO, &c, 1) == 1) {
    if (c == '\r') {
        c = '\n';  // 将回车转换为换行
    }
    // 处理字符...
}
```

