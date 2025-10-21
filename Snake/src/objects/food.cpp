#include "food.hpp"
#include <string>
#include "color/color_system.hpp"
Food::Food(int x , int y, const char* logo="🍎"):Object(Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK)){
    default_outlook=Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK);
    body.emplace_back(x, y, default_outlook);
}
