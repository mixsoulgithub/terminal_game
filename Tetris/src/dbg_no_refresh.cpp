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
//TODO get input
    int HEIGHT=1, WIDTH=1;

    const std::string text = " ";
    std::string oneline=" ";
    for(int i =1;i<WIDTH;i++){
        oneline+=" ";
    }
    std::vector<std::string> square(HEIGHT, oneline);
    double t = 0.0;

    while (true) {
        clear();//remember to clear
        std::string frame;
        frame.reserve(HEIGHT * WIDTH * 200); // 预分配内存, 200 is for special str.

        frame += "\033[H"; // 移动光标到左上, 这种操作居然也是特殊字符.here is at first avaliable location, after bash prompt.

        int r = (int)(128 + 127 * sin(t * 0.3 ));
        int g = (int)(128 + 127 * sin(t * 0.3 ));
        int b = (int)(128 + 127 * sin(t * 0.3 ));
        for (int h = 0; h < HEIGHT; h++) {
            // frame += 
            //     "\033[48;2;" + std::to_string(r) + ";" + std::to_string(g) + ";" + std::to_string(b) +
            //     "m" + square[h] + "\033[0m" 
            //     + "\n"
            //     ;

            frame = "\033[48;2;" + std::to_string(r) + ";" + std::to_string(g) + ";" + std::to_string(b) +
                 "m" + square[h] + "\033[0m" 
                 + "\n"
                 ;
        printf("%s", frame.c_str());
        fflush(stdout);
        }


        usleep(80000); // 80ms -> ~12 FPS
        t += 0.1;

        int ch = getch();// ohhh! getch have a side effect to go in a no prompt environment.
        if (ch == 'q') break;
    }

    endwin();
    return 0;
}
