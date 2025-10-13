#include "snake.hpp"

// 游戏区域尺寸

int Snake::DIRECT_STEP[4][2]{ //H, W
    {-1,0},
    {0,-1},
    {0,1},
    {1,0}
};
    
Snake::Snake(int h, int w, const char* logo="#"){
    body.emplace_back(std::make_tuple(h,w));
    outlook.emplace_back(logo);
    dir=RIGHT;
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
int Snake::move(DIRECT new_dir, World& world){
    if((dir^new_dir)==0b11){
        new_dir=dir;//invalid move
    }
    dir=new_dir;
    auto [head_x, head_y]=body[body.size()-1];//get head
    int* step=DIRECT_STEP[new_dir];
    head_x=head_x + step[0];
    head_y=head_y + step[1];
    body.emplace_back(head_x, head_y);//居然可以这样传递make tuple参数.
    if(world.check_collision(*this)||check_collision()){
        return -1;
    }
    // auto foods=frame.search<Food>(1);//search food
    // if(foods.size()){
    //     for(auto&& food:foods){
    //         if(food->body[0]==;){
    //             //eat food
    //             // frame.objs.erase(std::remove(frame.objs.begin(), frame.objs.end(), food), frame.objs.end());
    //             return 1;
    //         }
    //     }
    // }
    return 0;
}

int Snake::check_collision(){
    for(int i=1;i<body.size();i++){
        if(body[0]==body[i]){
            return 1;
        }
    }
    return 0;
}


