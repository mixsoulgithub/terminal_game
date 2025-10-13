#include "food.hpp"

Food::Food(int x , int y, const char* logo="🍎"):logo(logo){
    body.emplace_back(x, y);
    outlook.emplace_back(logo);
}
