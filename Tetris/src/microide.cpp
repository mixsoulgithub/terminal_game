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
    raw.c_iflag &= ~(BRKINT   | INPCK | ISTRIP | IXON);
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
    std::cout << "\033[?25h";
    std::cout.flush();
    
    // 读取输入并处理特殊键
    char c;
    while (true) {
        if (read(STDIN_FILENO, &c, 1) == 1) {
            if (c == 'q') break;
            
            // 处理回车键: 由于禁用了 OPOST, \n 不会自动移动到行首
            // \r移动到行首, \n 移动到下一行
            if (c == '\n' || c == '\r') {
                std::cout << "\r\n"; 
            }
            // 处理退格键: backspace 通常发送 ASCII 127 (DEL) 或 ASCII 8 (BS)
            else if (c == 127 || c == '\b') {
                // 退格: 光标左移 -> 输出空格覆盖 -> 光标再左移
                std::cout << "\b \b";
            } else {
                std::cout << c;
            }
            std::cout.flush();
        }
    }
    std::cout << "\033[?25h";  // 显示光标
    disable_raw_mode();        // 恢复终端设置
    
    return 0;
}