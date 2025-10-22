#include <ncurses.h>
#include "color/color_system.hpp"

ColorSystem::ColorSystem()
{

}

ColorSystem::~ColorSystem()
{

}

void ColorSystem::initialize()
{
    if (has_colors())
    {
        start_color();
        init_pair((short)ColorMode::FRONT_WHITE_BACK_BLACK, COLOR_WHITE, COLOR_BLACK);
        init_pair((short)ColorMode::FRONT_YELLOW_BACK_WHITE, COLOR_YELLOW, COLOR_WHITE);
    }
}

void ColorSystem::set_color_mode(ColorMode color_mode)
{
    attron(COLOR_PAIR(static_cast<int>(color_mode)));
}

void ColorSystem::unset_color_mode(ColorMode color_mode)
{
    attroff(COLOR_PAIR(static_cast<int>(color_mode)));
}
