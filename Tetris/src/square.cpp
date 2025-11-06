#include <ncurses.h>
#include <unistd.h>
#include <cmath>
#include <string>
#include <vector>

//霓虹跑马灯.
int main() {
    initscr();
    noecho();
    curs_set(0);      // 隐藏光标
    nodelay(stdscr, TRUE);
    start_color();
    timeout(0);  // 非阻塞输入
//TODO get input
    int HEIGHT=5, WIDTH=5;

    double t = 0.0;
    if(!can_change_color()){
        mvprintw(1,2*WIDTH,"can't change color");
    }
    while (true) {
        clear();//remember to clear, otherwise it will be a endless slash
        
        int r = (int)(500 + 500 * sin(t * 3 ));
        int g = (int)(500 + 500* sin(t * 3 +4));
        int b = (int)(500 + 500* sin(t * 3 +8));
        init_color(2, r, g, b);
        
        init_pair(3, COLOR_BLACK, 2);
        attron(COLOR_PAIR(3));
        for (int h = 0; h < HEIGHT; h++) {
         mvprintw(3, 5*WIDTH, "RGB: (%3d, %3d, %3d) - t: %.2f", 
                    r, g, b, t);  
         mvprintw(h,WIDTH,"test output");
        }
        attroff(COLOR_PAIR(3));

        usleep(80000); // 80ms -> ~12 FPS
        t += 0.1;
        refresh();
        int ch = getch();
        if (ch == 'q') break;
    }

    endwin();
    return 0;
}
