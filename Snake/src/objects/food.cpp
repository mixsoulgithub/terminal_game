#include "food.hpp"
#include <limits.h>
#include <string>
#include "color/color_system.hpp"

Food::Food(const char* logo)
:Object(Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK)){
    default_outlook=Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK);
    seed=std::mt19937 (42);
    dist=std::uniform_int_distribution<int> (0,std::numeric_limits<int>::max());
}

Food::Food(int x , int y, const char* logo="🍎"):Object(Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK)){
    default_outlook=Outlook(std::string(logo), ColorMode::FRONT_WHITE_BACK_BLACK);
    body.emplace_back(x, y, default_outlook);
    seed=std::mt19937 (42);
    dist=std::uniform_int_distribution<int> (0,std::numeric_limits<int>::max());
}

//1 means success.
int Food::generate(World& world){
    if(body.size()!=0)return 0;//there are still food;
    std::vector<std::tuple<int,int>> spare_space(std::move(world.get_spare_space()));
    int spare_space_len=spare_space.size();
    if(spare_space_len==0) return 1;
    int i = dist(seed)%spare_space_len;
    body.emplace_back(spare_space[i], default_outlook);
    return 0;
}
