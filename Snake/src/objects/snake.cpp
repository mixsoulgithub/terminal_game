#include "snake.hpp"
#include <ncurses.h>
#include <chrono>
#include "food.hpp"
#include <stdexcept>
#include "../collision/collision.hpp"
// 游戏区域尺寸

//TODO bind this with direction
int Snake::DIRECT_STEP[4][2]{ //H, W
    {-1,0},
    {0,1},
    {0,-1},
    {1,0}
};
    
Snake::Snake(int h, int w, const Outlook& default_outlook, DIRECT dir, int speed):Object(default_outlook), dir(dir), speed(speed){
    body.emplace_back(h, w, default_outlook);
}
    

Body Snake::get_head(){
    return body[body.size()-1];
}

int Snake::refresh(World& world){
    int i=0;
    for(auto [h,w]:body){
        // if(frame.pixels)
        // frame.pixels[h][w]=outlook[i]; //放在frame方法里面, 声明和实现分离.
        // frame->get_pixels(h,w,outlook[i]);
        // i++;
    }
    return 0;
}

/*
* @return : -1 is game over, 0 is ok
*/ 
int Snake::update_dir(DIRECT new_dir){
    if((dir^new_dir)==0b11){
        new_dir=dir;//invalid move
    }
    dir=new_dir;
    return 0;
}

int Snake::move(World& world){
    using namespace std::chrono;
    static auto last_update_time = world.now();
    auto current_time = world.now();
    if(5*speed> duration_cast<milliseconds>(current_time - last_update_time).count()){
        //speed is milliseconds cost of a move. 
        unchange();
        return 0;
    }
    change();
    last_update_time=world.now();
    using namespace Collision;
    auto [head_x, head_y]=get_head().get_location();
    int* step=DIRECT_STEP[dir];
    head_x=head_x + step[0];
    head_y=head_y + step[1];
    mvprintw(0, 64, "head_x= %d, head_y = %d", head_x, head_y);
    auto [pattern, color] =  default_outlook;
    mvprintw(7, 64, pattern.c_str());
    body.emplace_back(head_x, head_y, default_outlook);//居然可以这样传递make tuple参数.
    auto [tail_loc, _]=body[0];
    body.erase(body.begin());
    //TODO encapsulate rewirte by hand.
    auto [tail_x,tail_y]=tail_loc;
    mvprintw(tail_x, tail_y, " ");

    mvprintw(1, 64, "bodysize= %ld, ", body.size() );
    //in collison, snake eat or die.
    if(check_collision(world, *this)==UNSOLVABLE||check_collision(*this)==UNSOLVABLE){
        return -1;
    }
    return 0;
}

DIRECT Snake::get_dir(){
    return dir;
}

int Snake::grow(Outlook& outlook){//here default argument must be static to tell linker where it is before runtime.
    int len=body.size();
    if(len<=0) throw std::runtime_error("snake lenth <=0");
    auto [tail_x, tail_y]=body[0].get_location();
    if(len>=2){
        auto [subtail_x, subtail_y]=body[1].get_location();
        body.emplace(body.begin(), subtail_x+2*(tail_x-subtail_x), subtail_y+2*(tail_y-subtail_y), outlook);
    }
    int reverse_x=DIRECT_STEP[4-dir][0];
    int reverse_y=DIRECT_STEP[4-dir][1];
    body.emplace(body.begin(), tail_x+reverse_x, tail_y+reverse_y, outlook);
    return 1;
}

int Snake::grow(){
    grow(default_outlook);
}
