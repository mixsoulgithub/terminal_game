#include <ncurses.h>
#include <unistd.h>
#include <string>
#include <vector>

int pair_id_loop(){
    clear();
    init_pair(42, COLOR_CYAN, COLOR_BLACK);
    // attron(COLOR_PAIR(42+256));
    // printw("before set 42, 42+256 is like this\n");
    // attroff(COLOR_PAIR(42+256));
    attron(COLOR_PAIR(42));
    printw("before set 42, 42 is like this\n");
    attroff(COLOR_PAIR(42));


    init_pair(42, COLOR_CYAN, COLOR_WHITE);
    

    attron(COLOR_PAIR(42));
    printw("after set 42, 42 is like this\n");
    attroff(COLOR_PAIR(42));

    if(!can_change_color()){
        printw("can't change color!\n");
    }

    //这样不行, 也许不支持这么大?
    // init_color(1+256, 1000, 0,0);
    // init_color(2+256, 0, 1000,0);
    // init_extended_pair(42+256, 1+256, 2+256);
    // attron(COLOR_PAIR(42+256));
    // printw("after set 42, 42+256 is like this\n");
    // attroff(COLOR_PAIR(42+256));
    
    int color_f=30;
    int color_b=31;
    int color_pair=42+256;
    init_color(color_f, 500, 0, 500);
    init_color(color_b, 0, 1000,0);
    init_extended_pair(color_pair, color_f, color_b);
    attron(COLOR_PAIR(color_pair));
    printw("after set 42, 42+256 is like this\n");
    attroff(COLOR_PAIR(color_pair));

    refresh();
    sleep(5);
    return 0;
}

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
    c='#';
    //都不激活, 则par_id mod 256=0时为白+黑, 其余为灰+黑.

    // 激活下面的代码, 则得到在默认设置下, 背景色先是几个纯色, 然后是一些主色带领的渐变色. 以256为一个循环.
    // for (int bg = 0; bg < colors*colors; ++bg) {
    //     int pair_id = bg;
    //     if (pair_id < pairs) {
    //         init_pair(pair_id, COLOR_WHITE, bg);
    //     }
    // }
    // c=' ';

    //激活下面的代码, 则得到在默认设置下, 前景色和背景色一个规律. 
    // for (int fg = 0; fg < colors*colors; ++fg) {
    //     int pair_id = fg;
    //     if (pair_id < pairs) {
    //         init_pair(pair_id, fg, COLOR_BLACK);
    //     }
    // }

    // 激活下面的代码, 则得排列组合.依旧以256为循环，这是因为同时最多存在256个颜色对, 最后定义的覆盖了最前面定义的. 
    //也就是说, init_pair(pair_id) , 则所有id==pair_id(mod 256)的都是一个颜色对了.
    for (int fg = 0; fg < colors; ++fg) {
        for (int bg = 0; bg < colors; ++bg) {
            int pair_id = fg * colors + bg;
            if (pair_id < pairs) {
                init_pair(pair_id, bg, fg);
            }
        }
    }
    
    
    pair_id_loop();
    //结论: 我们同时最多只能有256个颜色对.不过够用了. 
    int height, width;
    getmaxyx(stdscr, height, width);
    // height=colors;
    // width=colors;

    // int pairs_per_page = height * width;
    // int total_pages = (pairs + pairs_per_page - 1) / pairs_per_page;

    // for (int page = 0; page < total_pages; ++page) {
    //     clear();

    //     int start_pair = page * pairs_per_page;
    //     int end_pair = std::min(start_pair + pairs_per_page, pairs);

    //     int idx = 0;
    //     for (int i = start_pair; i < end_pair; ++i) {
    //         int y = idx / width;
    //         int x = idx % width;
    //         if (y >= height) break;
    //         attron(COLOR_PAIR(i));
    //         mvaddch(y, x, c);
    //         attroff(COLOR_PAIR(i));
    //         ++idx;
    //     }

    //     mvprintw(height - 1, 0, "Page %d / %d  (Pairs %d - %d)  COLORS=%d  COLOR_PAIRS=%d",
    //              page + 1, total_pages, start_pair, end_pair - 1, colors, pairs);

    //     refresh();
    //     sleep(10);
    // }

    endwin();
    return 0;

}