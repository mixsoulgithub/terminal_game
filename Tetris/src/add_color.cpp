#include <ncurses.h>
#include <stdlib.h>
#include <cmath>
#include <unistd.h>
#include <locale.h> //TODO why some times we need it by hand?

// 将0-255的RGB值转换为0-1000
int rgb_to_ncurses(int rgb) {
    return (rgb * 1000) / 255;
}

// 创建自定义颜色
int create_color(int color_id, int r, int g, int b) {
    if (color_id < 0 || color_id >= COLORS) {
        return -1;
    }
    init_color(color_id, rgb_to_ncurses(r), rgb_to_ncurses(g), rgb_to_ncurses(b));
    return 0;
}

// 创建颜色对
int create_color_pair(int pair_id, int fg_color_id, int bg_color_id) {
    if (pair_id < 1 || pair_id >= COLOR_PAIRS) {
        return -1;
    }
    init_pair(pair_id, fg_color_id, bg_color_id);
    return 0;
}

int main() {
    setlocale(LC_ALL, ""); // 关键：设置区域，启用 UTF-8
    initscr();
    start_color();
    noecho();
    curs_set(0); // 隐藏光标
    timeout(0);  // 非阻塞输入
    
    if (!can_change_color()) {
        endwin();
        printf("无法修改颜色\n");
        return 1;
    }
    
    double t = 0.0;
    int width, height;
    getmaxyx(stdscr, height, width);
    
    while(true) {
        clear(); // 清屏
        
        // 创建多个霓虹颜色
        int r1 = (int)(128 + 127 * sin(t * 3));
        int g1 = (int)(128 + 127 * sin(t * 3 + 2));
        int b1 = (int)(128 + 127 * sin(t * 3 + 4));
        
        int r2 = (int)(128 + 127 * sin(t * 2 + 1));
        int g2 = (int)(128 + 127 * sin(t * 2 + 3));
        int b2 = (int)(128 + 127 * sin(t * 2 + 5));
        
        // 更新颜色
        create_color(8, r1, g1, b1);
        create_color(9, r2, g2, b2);
        
        // 创建颜色对
        create_color_pair(1, COLOR_BLACK, 8);
        create_color_pair(2, COLOR_BLACK, 9);
        
        // 在屏幕不同位置显示霓虹文本
        attron(COLOR_PAIR(1));
        mvprintw(height/2 - 2, width/2 - 6, "╔══════════╗");
        mvprintw(height/2 - 1, width/2 - 6, "║ 霓虹效果 ║");
        mvprintw(height/2, width/2 - 6, "╚══════════╝");
        
        attron(COLOR_PAIR(2));
        for (int i = 0; i < width; i++) {
            int y = height/2 + 2 + (int)(3 * sin(i * 0.3 + t * 5));
            if (y >= 0 && y < height) {
                mvprintw(y, i, "*");
            }
        }
        
        // 显示RGB值
        attron(COLOR_PAIR(1));
        mvprintw(height/2 + 6, width/2 - 10, "RGB1: (%3d, %3d, %3d)", r1, g1, b1);
        mvprintw(height/2 + 7, width/2 - 10, "RGB2: (%3d, %3d, %3d)", r2, g2, b2);
        
        refresh();
        
        // 检查退出
        int c = getch();
        if(c == 'q') {
            break;
        }
        
        t += 0.05;
        usleep(50000); // 50ms延迟，控制动画速度
    }
    
    endwin();
    return 0;
}
