
#include <ncurses.h>

int main() {
    initscr();            // 初始化 ncurses
    cbreak();
    noecho();
    keypad(stdscr, TRUE);
    refresh();

    // 创建两个窗口, height, width, x, y.
    WINDOW* win1 = newwin(30, 30, 0, 0);
    WINDOW* win2 = newwin(30, 30, 0, 30);

    // 窗口 1
    box(win1, 0, 0);
    mvwprintw(win1, 1, 2, "Window 1: Logs");
    wrefresh(win1);

    // 窗口 2
    box(win2, 0, 0);
    mvwprintw(win2, 1, 2, "Window 2: Input");
    wrefresh(win2);

    // 等待按键
    mvprintw(23, 1, "Press any key to exit...");
    refresh();
    getch();

    // 清理
    delwin(win1);
    delwin(win2);
    endwin();
    return 0;
}

