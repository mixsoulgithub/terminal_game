#include <unistd.h>
#include <ncurses.h>

#include "terminal_game.hpp"

TerminalGame::TerminalGame():
    m_is_game_over(false),
    m_world()
{
    
}

TerminalGame::~TerminalGame()
{

}

bool TerminalGame::initialize()
{
    bool initialize_result = true;

    initialize_result &= initialize_ncurses();
    initialize_result &= m_frame.initialize();
    initialize_result &= buildFromConfigFile("../config/config.json");

    return initialize_result;
}

void TerminalGame::run()
{
    // 主游戏循环
    while (m_is_game_over == false) {
        update();
        render();
        processInput();
    }
}

void TerminalGame::shutdown()
{
    // 将输入模式改回阻塞模式（无限等待）
    timeout(-1);
    // 等待用户按任意键继续
    getch();
    // 退出NCurses模式，恢复终端原始状态
    endwin();
}

bool TerminalGame::initialize_ncurses()
{
    setlocale(LC_ALL, ""); // 关键：设置区域，启用 UTF-8

    initscr();              // 初始化ncurses
    cbreak();               // 禁用行缓冲，字符立即可用（但保留信号控制字符）
    noecho();               // 关闭输入字符的自动回显
    curs_set(0);            // 隐藏光标（0=隐藏，1=正常，2=高亮）
    keypad(stdscr, TRUE);   // 启用功能键（方向键、F1-F12等）的特殊编码
    timeout(1);             // 设置getch()为非阻塞模式，超时时间为1毫秒

    return true;
}