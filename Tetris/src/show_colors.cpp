#include <ncurses.h>
#include <unistd.h>
#include <string>
#include <vector>

int main() {
    initscr();
    noecho();
    curs_set(0);
    start_color();

    if (!has_colors()) {
        endwin();
        printf("This terminal does not support colors.\n");
        return 1;
    }

    int colors = COLORS;
    int pairs  = COLOR_PAIRS;

    char c;
    // 激活下面的代码, 则得到在默认设置下, 背景色先是几个纯色, 然后是一些主色带领的渐变色. 以256为一个循环.
    // for (int bg = 0; bg < colors*colors; ++bg) {
    //     int pair_id = bg;
    //     if (pair_id < pairs) {
    //         init_pair(pair_id, COLOR_WHITE, bg);
    //     }
    // }
    // c=' ';

    //激活下面的代码, 则得到在默认设置下, 前景色和背景色一个规律. 
    for (int fg = 0; fg < colors*colors; ++fg) {
        int pair_id = fg;
        if (pair_id < pairs) {
            init_pair(pair_id, fg, COLOR_BLACK);
        }
    }
    c='#';

    int height, width;
    // getmaxyx(stdscr, height, width);
    height=colors;
    width=colors;

    int pairs_per_page = height * width;
    int total_pages = (pairs + pairs_per_page - 1) / pairs_per_page;

    for (int page = 0; page < total_pages; ++page) {
        clear();

        int start_pair = page * pairs_per_page;
        int end_pair = std::min(start_pair + pairs_per_page, pairs);

        int idx = 0;
        for (int i = start_pair; i < end_pair; ++i) {
            int y = idx / width;
            int x = idx % width;
            if (y >= height) break;
            attron(COLOR_PAIR(i));
            mvaddch(y, x, c);
            attroff(COLOR_PAIR(i));
            ++idx;
        }

        mvprintw(height - 1, 0, "Page %d / %d  (Pairs %d - %d)  COLORS=%d  COLOR_PAIRS=%d",
                 page + 1, total_pages, start_pair, end_pair - 1, colors, pairs);

        refresh();
        sleep(10);
    }

    endwin();
    return 0;

}