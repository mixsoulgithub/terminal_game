#pragma once
#include <ncurses.h>
//COLOR_A_B, 则A为前景, B为背景. 
#define COLOR_WHITE_BLACK 1

//因为这里只是声明, 所以不用inline也没关系.
void init_all_colors();