#include <ncurses.h>
#include <locale.h>

int main() {
    setlocale(LC_ALL, "");  // 关键：设置区域，启用 UTF-8
    initscr();
    
    int info_y = 1, info_x = 1;
    mvprintw(info_y + 9, info_x, "← → : Move");  // 现在应该正常显示
    mvprintw(1, 1, "█");  // 实心方块 U+2588
    mvprintw(2, 1, "▓");  // 75% 填充 U+2593
    mvprintw(3, 1, "▒");  // 50% 填充 U+2592
    mvprintw(4, 1, "░你好");  // 25% 填充 U+2591
    
    // 边框字符（用于游戏区域）
    mvprintw(6, 1, "┌ ─ ┐");  // 上边框
    mvprintw(7, 1, "│ │");  // 竖边框
    mvprintw(8, 1, "└─┘");  // 下边框
    
    mvprintw(0, 0, "    GAME OVERq  ");
    refresh();
    getch();
    endwin();
    return 0;
}