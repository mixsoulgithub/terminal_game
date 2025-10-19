#include <ncurses.h>

int show_color(int fore_color, int back_color, int pair){
    static int cur_index=0;//初始化之进行一次.
    int cur_w=cur_index % 32;
    int cur_h=cur_index / 32;
    
    attron(COLOR_PAIR(pair));
    mvaddch(cur_h, cur_w, '#');
    attroff(COLOR_PAIR(pair));
    cur_index++;
    return 0;
}

int main() {
    initscr();
    if (!has_colors()) {
        endwin();
        printf("Your terminal does not support colors.\n");
        return 1;
    }
    start_color();

    for(int i=0; i< 65536; ++i){
        attron(COLOR_PAIR(i));
        printw("TEST COLOR PAIR\n");
        attroff(COLOR_PAIR(i));
    }
    // if (can_change_color()) {
    //     // 定义自定义颜色 RGB (0~1000)
    //     init_color(10, 1000, 500, 0);  // 橙色
    //     init_color(11, 0, 1000, 0);    // 绿色
    // }

    // // 创建颜色对
    // init_pair(256, 10, COLOR_BLACK);
    // init_pair(0, 11, COLOR_BLACK);
    // init_pair(3, 11, 10);

    // // 打印
    // attron(COLOR_PAIR(256));
    // printw("This is orange text\n");
    // attroff(COLOR_PAIR(256));

    // attron(COLOR_PAIR(0));
    // printw("This is green text\n");
    // attroff(COLOR_PAIR(0));

    // attron(COLOR_PAIR(3));
    // printw("This is green text\n");
    // attroff(COLOR_PAIR(3));

    // printw("Colors: %d\n", COLORS);//256
    // printw("Color pairs: %d\n", COLOR_PAIRS);//65536


    refresh();
    getch();
    endwin();
    return 0;
}
