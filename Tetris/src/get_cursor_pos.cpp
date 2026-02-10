#include <iostream>
#include <unistd.h>
#include <termios.h>
#include <cstdio>
#include <string>
#include <sys/select.h>
#include <sys/time.h>

struct termios original_termios;

// 进入原始模式 (简化版, 用于读取光标位置)
void enable_raw_mode() {
    tcgetattr(STDIN_FILENO, &original_termios);
    struct termios raw = original_termios;
    raw.c_lflag &= ~(ECHO | ICANON);
    raw.c_cc[VMIN] = 0;
    raw.c_cc[VTIME] = 0;
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
}

// 恢复终端设置
void disable_raw_mode() {
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &original_termios);
}

// 清空输入缓冲区
void flush_input() {
    tcflush(STDIN_FILENO, TCIFLUSH);
}

// 获取光标位置
bool get_cursor_position(int& row, int& col) {
    // 先清空输入缓冲区, 避免用户输入和响应混淆
    flush_input();
    
    // 发送查询命令: Device Status Report (DSR)
    std::cout << "\033[6n";
    std::cout.flush();
    
    // 读取响应: 格式为 \033[row;columnR
    std::string response;
    char c;
    
    // 使用 select 添加超时机制 (500ms)
    struct timeval timeout;
    timeout.tv_sec = 0;
    timeout.tv_usec = 500000;  // 500ms
    
    fd_set readfds;
    FD_ZERO(&readfds);
    FD_SET(STDIN_FILENO, &readfds);
    
    // 读取直到遇到 'R' (响应结束标志)
    while (true) {
        fd_set tempfds = readfds;
        int result = select(STDIN_FILENO + 1, &tempfds, nullptr, nullptr, &timeout);
        
        if (result == 0) {
            // 超时
            return false;
        } else if (result < 0) {
            // 错误
            return false;
        }
        
        // 有数据可读
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
        
        // 重置超时
        timeout.tv_sec = 0;
        timeout.tv_usec = 500000;
    }
    
    // 解析响应: \033[row;columnR
    if (sscanf(response.c_str(), "\033[%d;%dR", &row, &col) == 2) {
        return true;
    }
    
    // 如果解析失败, 打印调试信息
    std::cerr << "解析失败, 响应: ";
    for (char ch : response) {
        if (ch >= 32 && ch < 127) {
            std::cerr << ch;
        } else {
            std::cerr << "\\x" << std::hex << (unsigned char)ch << std::dec;
        }
    }
    std::cerr << std::endl;
    
    return false;
}

int main() {
    // 进入原始模式以便读取响应
    enable_raw_mode();
    
    std::cout << "请移动光标到任意位置, 然后按回车键查询位置..." << std::endl;
    std::cout << "当前位置: ";
    
    // 等待用户按回车
    // 注意: 在原始模式下, 如果启用了 ICRNL, 回车会被转换为 \n
    // 但我们需要消耗所有可能的字符 (\r 和 \n)
    char c;
    bool got_newline = false;
    while (true) {
        if (read(STDIN_FILENO, &c, 1) == 1) {
            if (c == '\n') {
                got_newline = true;
                // 继续读取, 看是否还有 \r
                // 但通常 \n 已经足够了
                break;
            } else if (c == '\r') {
                // 如果先收到 \r, 可能后面还有 \n
                got_newline = true;
                // 尝试再读一个字符, 看是否是 \n
                fd_set readfds;
                FD_ZERO(&readfds);
                FD_SET(STDIN_FILENO, &readfds);
                struct timeval timeout;
                timeout.tv_sec = 0;
                timeout.tv_usec = 10000;  // 10ms 超时
                if (select(STDIN_FILENO + 1, &readfds, nullptr, nullptr, &timeout) > 0) {
                    char next_c;
                    if (read(STDIN_FILENO, &next_c, 1) == 1 && next_c == '\n') {
                        // 消耗了 \n
                    }
                }
                break;
            }
        }
    }
    
    // 获取光标位置
    int row, col;
    if (get_cursor_position(row, col)) {
        std::cout << "行=" << row << ", 列=" << col << std::endl;
    } else {
        std::cout << "无法获取光标位置" << std::endl;
    }
    
    // 恢复终端设置
    disable_raw_mode();
    
    return 0;
}

