#include <ncurses.h>
#include <locale.h>

int main() {
    setlocale(LC_ALL, "");  // 关键：设置区域，启用 UTF-8
    initscr();
    
    int info_y = 1, info_x = 1;
    mvprintw(info_y + 9, info_x, "← → : Move");  // 现在应该正常显示
    
    refresh();
    getch();
    endwin();
    return 0;
}