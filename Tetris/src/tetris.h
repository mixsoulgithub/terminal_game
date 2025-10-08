#ifndef TETRIS_H
#define TETRIS_H

#include <ncurses.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

// 游戏区域尺寸
#define BOARD_WIDTH 10
#define BOARD_HEIGHT 20
#define BOARD_OFFSET_X 5
#define BOARD_OFFSET_Y 2

// 方块形状定义
#define SHAPE_COUNT 7
#define SHAPE_SIZE 4

// 方块形状数据
extern const int shapes[SHAPE_COUNT][SHAPE_SIZE][SHAPE_SIZE];

// 颜色定义

// 游戏状态结构体
typedef struct {
    int board[BOARD_HEIGHT][BOARD_WIDTH];
    int current_shape[SHAPE_SIZE][SHAPE_SIZE];
    int next_shape[SHAPE_SIZE][SHAPE_SIZE];
    int shape_x, shape_y;
    int current_shape_type;
    int next_shape_type;
    int rotation;
    int score;
    int level;
    int lines_cleared;
    int game_over;
} GameState;

// 函数声明
void init_game(GameState *game);
void draw_board(GameState *game);
void draw_shape(GameState *game, int draw);
void draw_next_shape(GameState *game);
void draw_ui(GameState *game);
int check_collision(GameState *game);
void merge_shape(GameState *game);
int clear_lines(GameState *game);
void rotate_shape(GameState *game);
void move_shape(GameState *game, int dx, int dy);
void drop_shape(GameState *game);
void spawn_shape(GameState *game);
int get_drop_speed(GameState *game);

#endif