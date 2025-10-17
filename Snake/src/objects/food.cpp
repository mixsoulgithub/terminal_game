#include "food.hpp"
#include <string>

Food::Food(int x , int y, const char* logo="🍎"):Object(Outlook(std::string(logo),1)){
    default_outlook=Outlook(std::string(logo), 0);
    body.emplace_back(x, y, default_outlook);
}
