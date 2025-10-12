#include <memory>
#include <ncurses.h>
#include <iostream>
#include <unistd.h>
#include "frame.hpp"
#include "objects/snake.hpp"
#include "objects/food.hpp"
#include "objects/scoreboard.hpp"
#include "objects/world.hpp"

int main() {
    setlocale(LC_ALL, ""); // 关键：设置区域，启用 UTF-8
    // 初始化ncurses
    initscr();
    cbreak();
    noecho();
    curs_set(0);
    keypad(stdscr, TRUE);
    timeout(1);//this makes getch non-blocking, with 1ms delay.
    
    // 初始化颜色
    if (has_colors()) {
        start_color();
        //前景色, 背景色.
        init_pair(COLOR_CYAN, COLOR_CYAN, COLOR_BLACK);
        init_pair(COLOR_BLUE, COLOR_BLUE, COLOR_BLACK);
        // init_pair(COLOR_ORANGE, COLOR_WHITE, COLOR_BLACK);  // 橙色用白色替代
        init_pair(COLOR_YELLOW, COLOR_YELLOW, COLOR_BLACK);
        init_pair(COLOR_GREEN, COLOR_GREEN, COLOR_BLACK);
        // init_pair(COLOR_PURPLE, COLOR_MAGENTA, COLOR_BLACK);
        init_pair(COLOR_RED, COLOR_RED, COLOR_BLACK);
        init_pair(COLOR_WHITE, COLOR_WHITE, COLOR_BLACK);
    }
    
    Frame frame;
    std::shared_ptr<Snake> snake = std::make_shared<Snake>(0, 0, "@");
    std::shared_ptr<Food> food = std::make_shared<Food>(5, 5, "*");
    ScoreBoard scoreboard;//计分板
    int ch;
    int paused = 0;
    unsigned long last_drop_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
    
    World world;

    // 主游戏循环
    while (1) {
        // frame.flush_to_screen(std::vector<std::shared_ptr<Object>>({snake,food}));
        frame.flush_to_screen(world);
        ch = getch();
        
        // 处理输入
        if (!paused) {
            switch (ch) {
                case 'q':
                case 'Q':
                    attron(COLOR_PAIR(COLOR_WHITE));
                    mvprintw(0, 0, "Game Over! Final Score: %d", scoreboard.get_score());
                    timeout(-1);
                    getch();
                    endwin();
                    return 0;
                case KEY_LEFT:
                    snake->move(DIRECT::LEFT, world);
                    break;
                case KEY_RIGHT:
                    
                    break;
                case KEY_UP:
                    
                    break;
                case KEY_DOWN:
                    
                    break;
                case ' ':
                    
                    break;
                case 'p':
                case 'P':
                    paused = 1;
                    break;
            }
        
        refresh();
        usleep(10000);  // 10ms延迟，减少CPU使用
        }
    }
    
    // 游戏结束显示
    // refresh();
    
    timeout(-1);  // 阻塞等待按键
    getch();
    
    endwin();
    return 0;
}