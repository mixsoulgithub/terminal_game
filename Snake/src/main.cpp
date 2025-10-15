#include <memory>
#include <ncurses.h>
#include <iostream>
#include <unistd.h>
#include "frame.hpp"
#include "objects/snake.hpp"
#include "objects/food.hpp"
#include "objects/scoreboard.hpp"
#include "objects/world.hpp"
#include "game.hpp"

void processInput();

int main() {
    setlocale(LC_ALL, ""); // 关键：设置区域，启用 UTF-8

    initscr();              // 初始化ncurses
    cbreak();               // 禁用行缓冲，字符立即可用（但保留信号控制字符）
    noecho();               // 关闭输入字符的自动回显
    curs_set(0);            // 隐藏光标（0=隐藏，1=正常，2=高亮）
    keypad(stdscr, TRUE);   // 启用功能键（方向键、F1-F12等）的特殊编码
    timeout(1);             // 设置getch()为非阻塞模式，超时时间为1毫秒
    
    // 初始化颜色
    if (has_colors())   // 检查终端是否支持颜色
    {
        start_color();  // 启用颜色功能
        //前景色, 背景色.
        init_pair(1, COLOR_CYAN, COLOR_BLACK);         // 定义颜色对1：青色前景，黑色背景
        init_pair(2, COLOR_BLUE, COLOR_BLACK);         // 定义颜色对2：蓝色前景，黑色背景
        init_pair(3, COLOR_YELLOW, COLOR_BLACK);     // 定义颜色对3：黄色前景，黑色背景
        init_pair(4, COLOR_GREEN, COLOR_BLACK);       // 定义颜色对4：绿色前景，黑色背景
        init_pair(5, COLOR_RED, COLOR_BLACK);           // 定义颜色对5：红色前景，黑色背景
        init_pair(6, COLOR_WHITE, COLOR_BLACK);       // 定义颜色对6：白色前景，黑色背景
    }
    
    unsigned long last_drop_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
    
    Game game;

    // 主游戏循环
    while (game.getIsGameOver() == false) {
        game.processInput();
        game.update();
        game.render();
    }
    
    timeout(-1);  // 阻塞等待按键
    getch();
    
    endwin();
    return 0;
}
