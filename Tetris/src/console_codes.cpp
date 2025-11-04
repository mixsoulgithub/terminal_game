// https://man7.org/linux/man-pages/man4/console_codes.4.html


#include <ncurses.h>
#include <unistd.h>
#include <cmath>
#include <string>
#include <vector>

//霓虹跑马灯.
int main() {
    initscr();
    noecho();
    // curs_set(0);      // 隐藏光标
    nodelay(stdscr, TRUE);
    while (true) {
    
    printw("%s", "hello,world");
    fflush(stdout);


    usleep(2000000); // 80ms -> ~12 FPS

    printw("\033[H");
    fflush(stdout);
    usleep(2000000); // 80ms -> ~12 FPS

    printw("\n");
    fflush(stdout);
    usleep(2000000); // 80ms -> ~12 FPS
    }
    endwin();
    return 0;
}
