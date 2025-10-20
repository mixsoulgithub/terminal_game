#include <unistd.h>
#include <ncurses.h>

#include "terminal_game.hpp"
#include "color.hpp"

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
    initialize_result &= buildFromConfigFile("../config/config.json");

    return initialize_result;
}

void TerminalGame::run()
{
    // 主游戏循环
    mvprintw(0, 10, "before while");//在这里没有问题
        int debug_h=1;
    mvprintw(debug_h++, 10, "before while");
        update();
    mvprintw(debug_h++, 10, "before while");
        render();
    mvprintw(debug_h++, 10, "before while");
        processInput();
}

void TerminalGame::shutdown()
{
    timeout(-1);  // 阻塞等待按键
    getch();
    
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
    
    // mvprintw(10, 10, "Game Over! Final Score: ");//在这里没问题
    init_all_colors();

    //mvprintw(10, 10, "Game Over! Final Score: ");//在这里没有问题
    return true;
}