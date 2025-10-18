#pragma once

#include <ncurses.h>
#include <vector>
#include <string>
#include <tuple>
#include <memory>

#include "objects/object.hpp"
#include "objects/world.hpp"

//一帧是由若干个图层组成的.不过按渲染顺序来. 逻辑和渲染分离.
class Frame
{
public:
    Frame(int h = 30, int w = 40, int oh = 2, int ow = 5);//todo: const
    ~Frame();

    bool flush_to_screen(const World& world);//world is only one in most cases, so I don't want use xxx_ptr here.

    //detect if it is clash with other object
    //only sometimes it will check. 
    // auto check_collision(Object& it, World& world);

    //if all =1, return all as vector. if all =0, return first found.
    auto search(int all);

    int set_pixels(std::vector<std::tuple<int,int>>& body , std::vector<std::string>& pixel);
    int set_pixel(int h, int w, const std::string& pixel);

protected:
    int m_height;
    int m_width;
    int m_offset_h;
    int m_offset_w;
    std::vector<std::vector<std::string>> m_pixels;
};