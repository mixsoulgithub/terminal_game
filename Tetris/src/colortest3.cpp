#include <ncurses.h>
#include <unistd.h>
#include <vector>
#include <cstdlib>
#include <ctime>
#include <string>

int main() {
    // 初始化 ncurses
    initscr();
    noecho();
    curs_set(0);
    nodelay(stdscr, TRUE); // 非阻塞输入
    start_color();

    // 初始化一些霓虹颜色对
    init_pair(1, COLOR_RED, COLOR_BLACK);
    init_pair(2, COLOR_YELLOW, COLOR_BLACK);
    init_pair(3, COLOR_GREEN, COLOR_BLACK);
    init_pair(4, COLOR_CYAN, COLOR_BLACK);
    init_pair(5, COLOR_BLUE, COLOR_BLACK);
    init_pair(6, COLOR_MAGENTA, COLOR_BLACK);
    int color_count = 6;

    srand(time(nullptr));

    // 获取屏幕大小
    int max_y, max_x;
    getmaxyx(stdscr, max_y, max_x);

    std::string text = " 霓虹跑马灯效果酷炫C++ ncurses Demo ";
    int text_len = text.size();
    int pos = 0;
    int direction = 1; // 1: 右, -1: 左

    while (true) {
        clear();

        for (int i = 0; i < text_len; ++i) {
            int color = 1 + rand() % color_count;
            attron(COLOR_PAIR(color));
            mvaddch(max_y / 2, (pos + i) % max_x, text[i]);
            attroff(COLOR_PAIR(color));
        }

        refresh();
        usleep(70000); // 控制速度

        pos += direction;
        if (pos + text_len >= max_x || pos <= 0) direction *= -1;

        // 退出条件: 按下 q
        int ch = getch();
        if (ch == 'q' || ch == 'Q') break;
    }

    // 恢复终端
    endwin();
    return 0;
}
