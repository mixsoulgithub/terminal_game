#pragma once

#include <ncurses.h>
#include <vector>
#include <string>
#include <tuple>
#include <memory>

#include "objects/object.hpp"
#include "objects/world.hpp"

//一帧是由若干个图层组成的.不过按渲染顺序来. 逻辑和渲染分离.
class Frame{
    int HEIGHT;
    int WIDTH;
    int OFFSET_H;
    int OFFSET_W;
    std::vector<std::vector<std::string>> pixels;
private:

public:
    Frame(int H=30, int W=40, int OH=2, int OW=5);//todo: const

    bool flush_to_screen(World& world);//world is only one in most cases, so I don't want use xxx_ptr here.

    //detect if it is clash with other object
    //only sometimes it will check. 
    // auto check_collision(Object& it, World& world);

    //if all =1, return all as vector. if all =0, return first found.
    auto search(int all);

    int set_pixels(std::vector<std::tuple<int,int>>& body , std::vector<std::string>& pixel);
    int set_pixel(int h, int w, std::string& pixel);
};