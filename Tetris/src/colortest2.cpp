#include <ncurses.h>
#include <unistd.h>
#include <cmath>
#include <string>

//霓虹跑马灯.
int main() {
    initscr();
    noecho();
    curs_set(0);      // 隐藏光标
    nodelay(stdscr, TRUE);

    int HEIGHT, WIDTH;
    getmaxyx(stdscr, HEIGHT, WIDTH);


    // HEIGHT=10;
    // WIDTH=10;
    // const std::string text = " ANSI TRUECOLOR DEMO ";
    const std::string text = "                       ";
    int text_len=text.size();

    double t = 0.0;

    while (true) {
        std::string frame;
        frame.reserve(HEIGHT * WIDTH * 20); // 预分配内存

        frame += "\033[H"; // 移动光标到左上, 这种操作居然也是特殊字符.

        for (int y = 0; y < HEIGHT; y++) {
            for (int x = 0; x < WIDTH; x++) {
                int r = (int)(128 + 127 * sin(x * 0.3 + t));
                int g = (int)(128 + 127 * sin(x * 0.3 + t + 2));
                int b = (int)(128 + 127 * sin(x * 0.3 + t + 4));

                int br = (int)(128 + 127 * sin(x * 0.3 - t + 1));
                int bg = (int)(128 + 127 * sin(x * 0.3 - t + 3));
                int bb = (int)(128 + 127 * sin(x * 0.3 - t + 5));

                frame += 
                // "\033[38;2;" + std::to_string(r) + ";" + std::to_string(g) + ";" + std::to_string(b) + "m" +           
                "\033[48;2;" + std::to_string(br) + ";" + std::to_string(bg) + ";" + std::to_string(bb) +
                "m" + text[(x + y) % text_len] + "\033[0m";
            }
        }

        printf("%s", frame.c_str());
        fflush(stdout);

        usleep(80000); // 80ms -> ~12 FPS
        t += 0.1;

        int ch = getch();
        if (ch != ERR) break;
    }

    endwin();
    return 0;
}
