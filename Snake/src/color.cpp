#include "color.hpp"
void init_all_colors(){
    // 初始化颜色
    if (has_colors())   // 检查终端是否支持颜色
    {
        start_color();  // 启用颜色功能
        //前景色, 背景色.
        init_pair(COLOR_WHITE_BLACK, COLOR_WHITE, COLOR_BLACK);         // 定义颜色对1：白色前景，黑色背景
        init_pair(COLOR_WHITE_NONE, COLOR_WHITE, COLOR_BLACK); 

    }
}