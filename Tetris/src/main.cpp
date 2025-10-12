#include <ncurses.h>
#include <iostream>

#include "tetris.h"

// 方块形状定义 - 7种基本形状
const int shapes[SHAPE_COUNT][SHAPE_SIZE][SHAPE_SIZE] = {
    // I
    {
        {0,0,0,0},
        {1,1,1,1},
        {0,0,0,0},
        {0,0,0,0}
    },
    // J
    {
        {0,0,0,0},
        {1,0,0,0},
        {1,1,1,0},
        {0,0,0,0}
    },
    // L
    {
        {0,0,0,0},
        {0,0,1,0},
        {1,1,1,0},
        {0,0,0,0}
    },
    // O
    {
        {0,0,0,0},
        {0,1,1,0},
        {0,1,1,0},
        {0,0,0,0}
    },
    // S
    {
        {0,0,0,0},
        {0,1,1,0},
        {1,1,0,0},
        {0,0,0,0}
    },
    // T
    {
        {0,0,0,0},
        {0,1,0,0},
        {1,1,1,0},
        {0,0,0,0}
    },
    // Z
    {
        {0,0,0,0},
        {1,1,0,0},
        {0,1,1,0},
        {0,0,0,0}
    }
};

// 初始化游戏
void init_game(GameState *game) {
    // 清空游戏板
    for (int y = 0; y < BOARD_HEIGHT; y++) {
        for (int x = 0; x < BOARD_WIDTH; x++) {
            game->board[y][x] = 0;
        }
    }
    
    // 初始化游戏状态
    game->score = 0;
    game->level = 1;
    game->lines_cleared = 0;
    game->game_over = 0;
    game->rotation = 0;
    
    // 生成第一个和下一个方块
    srand(time(NULL));
    game->next_shape_type = rand() % SHAPE_COUNT;
    spawn_shape(game);
}

// 绘制游戏板
void draw_board(GameState *game) {
    // 绘制边框
    attron(COLOR_PAIR(COLOR_WHITE));
    for (int y = 0; y < BOARD_HEIGHT + 2; y++) {
        mvaddch(BOARD_OFFSET_Y + y, BOARD_OFFSET_X - 1, '|');
        mvaddch(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + BOARD_WIDTH, '|');
    }
    for (int x = 0; x < BOARD_WIDTH + 2; x++) {
        mvaddch(BOARD_OFFSET_Y - 1, BOARD_OFFSET_X + x - 1, '-');
        mvaddch(BOARD_OFFSET_Y + BOARD_HEIGHT, BOARD_OFFSET_X + x - 1, '-');
    }
    mvaddch(BOARD_OFFSET_Y - 1, BOARD_OFFSET_X - 1, '+');
    mvaddch(BOARD_OFFSET_Y - 1, BOARD_OFFSET_X + BOARD_WIDTH, '+');
    mvaddch(BOARD_OFFSET_Y + BOARD_HEIGHT, BOARD_OFFSET_X - 1, '+');
    mvaddch(BOARD_OFFSET_Y + BOARD_HEIGHT, BOARD_OFFSET_X + BOARD_WIDTH, '+');
    
    // 绘制已固定的方块
    for (int y = 0; y < BOARD_HEIGHT; y++) {
        for (int x = 0; x < BOARD_WIDTH; x++) {
            if (game->board[y][x]) {
                int color = game->board[y][x];
                attron(COLOR_PAIR(color));
                // mvaddstr(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x, "▇");//▇ is thin in shell than here. and I can't find a better one in utf8.
                mvaddch(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x , '#');
                // mvaddstr(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x , "★");
                // mvaddstr(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x , "口");//bug: out of frame, because '口' is double width in terminal.
                // mvaddstr(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x, "■");// '■' is not better, big gap between lines.
                // mvaddch(BOARD_OFFSET_Y + y, BOARD_OFFSET_X + x + 1, ']');
            } 
        }
    }
}

// 绘制当前方块（draw=1绘制，draw=0擦除）
void draw_shape(GameState *game, int draw) {
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            if (game->current_shape[y][x]) {
                int screen_y = BOARD_OFFSET_Y + game->shape_y + y;
                int screen_x = BOARD_OFFSET_X + game->shape_x + x;
                
                if (draw) {
                    attron(COLOR_PAIR(game->current_shape_type + 1));
                    mvaddch(screen_y, screen_x, '#');
                }
            }
        }
    }
}

// 绘制下一个方块预览
void draw_next_shape(GameState *game) {
    int preview_x = BOARD_OFFSET_X + BOARD_WIDTH + 8;
    int preview_y = BOARD_OFFSET_Y + 2;
    
    // 绘制预览框
    attron(COLOR_PAIR(COLOR_WHITE));
    mvprintw(preview_y - 2, preview_x, "Next Shape:");
    for (int y = 0; y < SHAPE_SIZE + 2; y++) {
        mvaddch(preview_y + y, preview_x - 1, '|');
        mvaddch(preview_y + y, preview_x + SHAPE_SIZE * 2 + 1, '|');
    }
    for (int x = 0; x < SHAPE_SIZE * 2 + 2; x++) {
        mvaddch(preview_y - 1, preview_x + x - 1, '-');
        mvaddch(preview_y + SHAPE_SIZE + 1, preview_x + x - 1, '-');
    }
    
    // 绘制下一个方块
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            int screen_y = preview_y + y;
            int screen_x = preview_x + x;
            
            if (game->next_shape[y][x]) {
                attron(COLOR_PAIR(game->next_shape_type + 1));
                mvaddch(screen_y, screen_x, '#');
            }
        }
    }
}

// 绘制用户界面
void draw_ui(GameState *game) {
    int info_x = BOARD_OFFSET_X + BOARD_WIDTH + 8;
    int info_y = BOARD_OFFSET_Y + 10;
    
    attron(COLOR_PAIR(COLOR_WHITE));
    mvprintw(info_y, info_x, "Score: %d", game->score);
    mvprintw(info_y + 2, info_x, "Level: %d", game->level);
    mvprintw(info_y + 4, info_x, "Lines: %d", game->lines_cleared);
    
    mvprintw(info_y + 8, info_x, "Controls:");
    mvprintw(info_y + 9, info_x, "← → : Move");
    mvprintw(info_y + 10, info_x, "↑ : Rotate");
    mvprintw(info_y + 11, info_x, "↓ : Soft Drop");
    mvprintw(info_y + 12, info_x, "Space : Hard Drop");
    mvprintw(info_y + 13, info_x, "P : Pause");
    mvprintw(info_y + 14, info_x, "Q : Quit");
    
    if (game->game_over) {
        mvprintw(BOARD_OFFSET_Y + BOARD_HEIGHT / 2, 
                BOARD_OFFSET_X + BOARD_WIDTH / 2 - 5, 
                "GAME OVER!");
    }
}

// 检查碰撞
int check_collision(GameState *game) {
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            if (game->current_shape[y][x]) {
                int board_x = game->shape_x + x;
                int board_y = game->shape_y + y;
                
                // 检查边界
                if (board_x < 0 || board_x >= BOARD_WIDTH || 
                    board_y >= BOARD_HEIGHT) {
                    return 1;
                }
                
                // 检查与其他方块的碰撞
                if (board_y >= 0 && game->board[board_y][board_x]) {
                    return 1;
                }
            }
        }
    }
    return 0;
}

// 将当前方块合并到游戏板
void merge_shape(GameState *game) {
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            if (game->current_shape[y][x]) {
                int board_x = game->shape_x + x;
                int board_y = game->shape_y + y;
                
                if (board_y >= 0) {  // 确保在有效范围内
                    game->board[board_y][board_x] = game->current_shape_type + 1;
                }
            }
        }
    }
}

// 消除完整的行并返回消除的行数
int clear_lines(GameState *game) {
    int lines_cleared = 0;
    
    for (int y = BOARD_HEIGHT - 1; y >= 0; y--) {
        int line_full = 1;
        
        // 检查行是否完整
        for (int x = 0; x < BOARD_WIDTH; x++) {
            if (!game->board[y][x]) {
                line_full = 0;
                break;
            }
        }
        
        // 如果行完整，消除它
        if (line_full) {
            lines_cleared++;
            
            // 将上面的行下移
            for (int yy = y; yy > 0; yy--) {
                for (int x = 0; x < BOARD_WIDTH; x++) {
                    game->board[yy][x] = game->board[yy - 1][x];
                }
            }
            
            // 清空顶行
            for (int x = 0; x < BOARD_WIDTH; x++) {
                game->board[0][x] = 0;
            }
            
            // 重新检查当前行（因为可能有新的完整行）
            y++;
        }
    }
    
    return lines_cleared;
}

// 旋转当前方块
void rotate_shape(GameState *game) {
    // 保存原始形状
    int temp_shape[SHAPE_SIZE][SHAPE_SIZE];
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            temp_shape[y][x] = game->current_shape[y][x];
        }
    }
    
    // 旋转形状
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            game->current_shape[x][SHAPE_SIZE - 1 - y] = temp_shape[y][x];
        }
    }
    
    // 如果旋转后发生碰撞，恢复原始形状
    if (check_collision(game)) {
        for (int y = 0; y < SHAPE_SIZE; y++) {
            for (int x = 0; x < SHAPE_SIZE; x++) {
                game->current_shape[y][x] = temp_shape[y][x];
            }
        }
    }
}

// 移动当前方块
void move_shape(GameState *game, int dx, int dy) {
    draw_shape(game, 0);  // 擦除当前方块
    
    game->shape_x += dx;
    game->shape_y += dy;
    
    // 如果移动后发生碰撞，恢复原位置
    if (check_collision(game)) {
        game->shape_x -= dx;
        game->shape_y -= dy;
    }
    
    draw_shape(game, 1);  // 绘制新位置的方块
}

// 硬降（立即落到底部）
void drop_shape(GameState *game) {
    draw_shape(game, 0);  // 擦除当前方块
    
    // 一直下落直到碰撞
    while (!check_collision(game)) {
        game->shape_y++;
    }
    
    // 回退一步到有效位置
    game->shape_y--;
    
    draw_shape(game, 1);  // 绘制新位置的方块
    
    // 合并到游戏板并生成新方块
    merge_shape(game);
    int lines = clear_lines(game);
    
    // 更新分数
    if (lines > 0) {
        game->lines_cleared += lines;
        game->score += lines * lines * 100 * game->level;
        game->level = game->lines_cleared / 10 + 1;
    }
    
    spawn_shape(game);
}

// 生成新方块
void spawn_shape(GameState *game) {
    // 设置当前方块
    game->current_shape_type = game->next_shape_type;
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            game->current_shape[y][x] = shapes[game->current_shape_type][y][x];
        }
    }
    
    // 生成下一个方块
    game->next_shape_type = rand() % SHAPE_COUNT;
    for (int y = 0; y < SHAPE_SIZE; y++) {
        for (int x = 0; x < SHAPE_SIZE; x++) {
            game->next_shape[y][x] = shapes[game->next_shape_type][y][x];
        }
    }
    
    // 设置初始位置（顶部中央）
    game->shape_x = BOARD_WIDTH / 2 - SHAPE_SIZE / 2;
    game->shape_y = 0;
    
    // 检查游戏是否结束
    if (check_collision(game)) {
        game->game_over = 1;
    }
}

// 根据等级获取下落速度
int get_drop_speed(GameState *game) {
    // 等级越高，速度越快（毫秒）
    return 100 - (game->level - 1) * 100;
    if (get_drop_speed(game) < 100) {
        return 100;  // 最小速度
    }
}

int main() {
    setlocale(LC_ALL, ""); // 关键：设置区域，启用 UTF-8
    // 初始化ncurses
    initscr();
    cbreak();
    noecho();
    curs_set(0);
    keypad(stdscr, TRUE);
    timeout(1);//this makes getch non-blocking, with 1ms delay.
    
    // 初始化颜色
    if (has_colors()) {
        start_color();
        //前景色, 背景色.
        init_pair(COLOR_CYAN, COLOR_CYAN, COLOR_BLACK);
        init_pair(COLOR_BLUE, COLOR_BLUE, COLOR_BLACK);
        // init_pair(COLOR_ORANGE, COLOR_WHITE, COLOR_BLACK);  // 橙色用白色替代
        init_pair(COLOR_YELLOW, COLOR_YELLOW, COLOR_BLACK);
        init_pair(COLOR_GREEN, COLOR_GREEN, COLOR_BLACK);
        // init_pair(COLOR_PURPLE, COLOR_MAGENTA, COLOR_BLACK);
        init_pair(COLOR_RED, COLOR_RED, COLOR_BLACK);
        init_pair(COLOR_WHITE, COLOR_WHITE, COLOR_BLACK);
    }
    
    GameState game;
    init_game(&game);
    
    int ch;
    int paused = 0;
    unsigned long last_drop_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
    
    // 主游戏循环
    while (!game.game_over) {
        ch = getch();
        
        // 处理输入
        if (!paused) {
            switch (ch) {
                case 'q':
                case 'Q':
                    game.game_over = 1;
                    break;
                case KEY_LEFT:
                    move_shape(&game, -1, 0);
                    break;
                case KEY_RIGHT:
                    move_shape(&game, 1, 0);
                    break;
                case KEY_UP:
                    rotate_shape(&game);
                    break;
                case KEY_DOWN:
                    move_shape(&game, 0, 1);
                    break;
                case ' ':
                    drop_shape(&game);
                    break;
                case 'p':
                case 'P':
                    paused = 1;
                    break;
            }
            
            // 自动下落, *1000 是增加粒度.
            unsigned long current_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
            if (current_time - last_drop_time > get_drop_speed(&game)) {
                move_shape(&game, 0, 1);
                
                // 检查是否已经到底
                draw_shape(&game, 0);
                game.shape_y++;
                if (check_collision(&game)) {
                    game.shape_y--;
                    merge_shape(&game);
                    int lines = clear_lines(&game);
                    if (lines > 0) {
                        game.lines_cleared += lines;
                        game.score += lines * lines * 100 * game.level;
                        game.level = game.lines_cleared / 10 + 1;
                    }
                    spawn_shape(&game);
                }
                draw_shape(&game, 1);
                
                last_drop_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
            }
        } else {
            // 暂停状态
            if (ch == 'p' || ch == 'P') {
                paused = 0;
            }
        }
        
        // 绘制游戏
        clear();
        draw_board(&game);
        draw_shape(&game, 1);
        draw_next_shape(&game);
        draw_ui(&game);
        
        if (paused) {
            mvprintw(BOARD_OFFSET_Y + BOARD_HEIGHT / 2, 
                    BOARD_OFFSET_X + BOARD_WIDTH / 2 - 3, 
                    "PAUSED");
        }
        
        refresh();
        usleep(10000);  // 10ms延迟，减少CPU使用
    }
    
    // 游戏结束显示
    draw_ui(&game);
    mvprintw(BOARD_OFFSET_Y + BOARD_HEIGHT + 2, BOARD_OFFSET_X, 
             "Game Over! Final Score: %d", game.score);
    mvprintw(BOARD_OFFSET_Y + BOARD_HEIGHT + 3, BOARD_OFFSET_X, 
             "Press any key to exit...");
    refresh();
    
    timeout(-1);  // 阻塞等待按键
    getch();
    
    endwin();
    return 0;
}