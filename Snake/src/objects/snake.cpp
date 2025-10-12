#pragma once

#include "snake.hpp"

// 游戏区域尺寸


struct Snake;
struct Food;
struct Frame;
struct ScoreBoard;
struct BackGrouand;


struct Snake: public Object{
    public:
    DIRECT dir;
    
    Snake(int h, int w, const char* logo="#"){
        body.emplace_back(std::make_tuple(h,w));
        outlook.emplace_back(logo);
        dir=RIGHT;
    }
    
    int flush(Frame* frame){
        int i=0;
        for(auto [h,w]:body){
            // if(frame.pixels)
            // frame.pixels[h][w]=outlook[i]; //放在frame方法里面, 声明和实现分离.
            frame->get_pixels(h,w,outlook[i]);
            i++;
        }
        return 0;
    }

    /*
    * @return : -1 is game over, 0 is ok
    */ 
    template<typename Frame>
    int move(DIRECT new_dir, Frame& frame){
        if((dir^new_dir)==0b11){
            new_dir=dir;//invalid move
        }
        dir=new_dir;
        auto [head_x, head_y]=body[body.size()-1];//get head
        int* step=DIRECT_STEP[new_dir];
        head_x=head_x + step[0];
        head_y=head_y + step[1];
        body.emplace_back(head_x, head_y);//居然可以这样传递make tuple参数.
        if(frame.check_collision(*this)||check_collision()){
            return -1;
        }
        auto foods=frame.search<Food>(1);//search food
        if(foods.size()){
            for(auto&& food:foods){
                if(food->body[0]==;){
                    //eat food
                    frame.objs.erase(std::remove(frame.objs.begin(), frame.objs.end(), food), frame.objs.end());
                    return 1;
                }
            }
        }
        return 0;
    }

    int check_collision(){
        for(int i=1;i<body.size();i++){
            if(body[0]==body[i]){
                return 1;
            }
        }
        return 0;
    }
};
struct Food:public Object{
    public:
    const char* logo;

    Food(int x , int y, const char* logo="🍎"):logo(logo){
        body.emplace_back(x, y);
        outlook.emplace_back("🍎");
    }
};
// 一帧

struct ScoreBoard:public Object{
    public:
    int score;
    int level;
    int lines_cleared;

    ScoreBoard():score(0),level(1),lines_cleared(0){
    }

    int flush(){
        return 0;
    }
    int flush(int x, int y){
        mvprintw(y, x, "Score: %d", score);
        mvprintw(y+1, x, "Level: %d", level);
        mvprintw(y+2, x, "Lines: %d", lines_cleared);
        return 0;
    }
};

struct BackGrouand:public Object{
    int HEIGHT;
    int WIDTH;
    int COLOR;
};

//一帧是由若干个图层组成的.不过按渲染顺序来. 逻辑和渲染分离.
struct Frame{
    int HEIGHT;
    int WIDTH;
    int OFFSET_H;
    int OFFSET_W;
    std::vector<std::vector<std::string>> pixels;
    std::vector<std::shared_ptr<Object>> objs;//todo: lock
private:

public:
    Frame(const int H=30, const int W=40, const int OH=2, const int OW=5):
        HEIGHT(H), WIDTH(W), OFFSET_H(OH), OFFSET_W(OW){
        pixels=std::move(std::vector<std::vector<std::string>>(HEIGHT, std::vector<std::string>(WIDTH,std::string(" "))));
    }


    //show pixels on screen.
    template<typename Container>
    auto flush_to_screen(Container&& objs) {
        for(auto&& obj : std::forward<Container>(objs)){//template delays type, it's harder to find objs/obj typo.
            int i=0;
            for(auto [H,W]:(*obj).body){
                mvaddstr(OFFSET_H + H, OFFSET_W + W, (obj->outlook[i]).c_str());
                i++;
            }
        }
        return 0;
    }

    //detect if it is clash with other object
    //only sometimes it will check. 
    template<typename Container>
    auto check_collision(Object it) -> decltype(std::forward<Container>(objs), int()) {
        using ValueType = typename std::decay_t<Container>::value_type;
        static_assert(
            std::is_same_v<ValueType, std::shared_ptr<Object>>,
            "Container must hold shared_ptr<Object>"
        );
        static_assert(
            std::is_same_v<std::decay_t<Container>, std::vector<ValueType>>,
            "Container must be a vector"
        );
        for(auto&& obj : objs){
            if(obj.get()==&it) continue;//skip self
            for(auto [h1,w1]:it.body){
                for(auto [h2,w2]:(*obj).body){
                    if(h1==h2 && w1==w2){
                        return 1;
                    }
                }
            }
        }
        return 0;
    }

    //if all =1, return all as vector. if all =0, return first found.
    template<typename T>
    auto search(int all){
        std::vector<std::shared_ptr<T>> res;
        for(auto&& obj : objs){
            if(typeid(obj) == typeid(T)){
                if(!all) return std::dynamic_pointer_cast<T>(obj);
                res.emplace_back(std::dynamic_pointer_cast<T>(obj));//use obj to initialize shared_ptr<T>
            }
        }
        return res;
    }

    auto get_pixels(int h, int w, std::string& pixel) {
        return pixels;
    }
};
