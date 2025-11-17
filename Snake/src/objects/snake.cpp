#include "snake.hpp"
#include <memory>
#include <ncurses.h>
#include <chrono>
#include "food.hpp"
#include <stdexcept>
#include "ncursesw/curses.h"
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

/*
* @return : -1 is game over, 0 is ok
*/ 
int Snake::update_dir(DIRECT input_dir){
    new_dir=input_dir;
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
    if((dir^new_dir)!=0b11 || body.size()<=1){
        dir=new_dir;
    }
    //move head.
    auto [head_x, head_y]=get_head().get_location();
    int* step=DIRECT_STEP[dir];
    head_x=head_x + step[0];
    head_y=head_y + step[1];
    mvprintw(0, 64, "head_x= %d, head_y = %d", head_x, head_y);
    auto [patltern, color] =  default_outlook;
    mvprintw(7, 64, "snake body len %ld", body.size());
    body.emplace_back(head_x, head_y, default_outlook);//居然可以这样传递make tuple参数.
    //remove tail.
    auto [tail_loc, _]=body[0];
    body.erase(body.begin());

    //TODO encapsulate rewirte by hand.
    auto [tail_x,tail_y]=tail_loc;
    mvprintw(tail_x, tail_y, " ");

    mvprintw(1, 64, "bodysize= %ld, ", body.size() );
    //in collison, snake eat or die.
    //first check collision with world, it may grow, then check collision with self.
    if(collision_with(world)==UNSOLVABLE||collision_with_self()==UNSOLVABLE){
        return -1;
    }
    return 0;
}

COLLISION_TYPE Snake::collision_with(World& world){
    COLLISION_TYPE ans=NONE;
    auto objs=world.get_objects();
    for(auto&& obj: objs){
        if(collision_with(obj)==UNSOLVABLE){
            return UNSOLVABLE;
        }else if(collision_with(obj)==SOLVABLE){
            ans=SOLVABLE;
        }
    }
    return ans;
}
COLLISION_TYPE Snake::collision_with(std::shared_ptr<Object> obj) {
    COLLISION_TYPE ans = NONE;
    if (std::addressof(*obj) == std::addressof(*this)) {
        ans = collision_with_self();
    } else if (typeid(*obj) == typeid(Food)) {
        ans = collision_with(std::dynamic_pointer_cast<Food>(obj));
    }
    return ans;
}

COLLISION_TYPE Snake::collision_with(std::shared_ptr<Food> food){
        static int called_times=0;
        auto& food_body=food->get_body();
        int snake_len=body.size();
        //only head of snake can eat something.
        auto&& [s_location, _]=body[snake_len-1];
        int food_len=food_body.size();
        for(int j=0;j<food_len;++j){
            auto&& [f_location, _]=food_body[j];
            if(s_location==f_location){
                grow();
                mvprintw(12, 64, "snake grow %d", called_times++);
                food->delete_body(j);
                return SOLVABLE;
            }
        }
        return NONE;
}

COLLISION_TYPE Snake::collision_with_self(){
        int len=body.size();
        auto&& [head_location, _]=body[0];
        for(int i=1;i<len;i++){
            auto&& [body_location, _]=body[i];
            if(head_location==body_location){
                return UNSOLVABLE;
            }
        }
        return NONE;
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
    }else{
        int reverse_x=DIRECT_STEP[0b11^dir][0];
        int reverse_y=DIRECT_STEP[0b11^dir][1];
        body.emplace(body.begin(), tail_x+reverse_x, tail_y+reverse_y, outlook);
    }
    return 1;
}

int Snake::grow(){
    return grow(default_outlook);
}
