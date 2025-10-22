#pragma once
#include <vector>
#include <string>
#include <tuple>
#include "color/color_system.hpp"

using Outlook=std::tuple<std::string, ColorMode>;//pattern, color

//Body 用于存储物体的每个部分的位置和外观
struct Body{
    std::tuple<int, int> location;
    Outlook outlook;//花纹, pattern, 也就是字符. 以及颜色, 严格来讲是颜色对.

    //重载了两份构造函数.
    Body(int x, int y, const std::string& pattern, const ColorMode color_mode = ColorMode::FRONT_WHITE_BACK_BLACK):
        location(std::make_tuple(x,y)), outlook(std::make_tuple(pattern, color_mode)){}

    Body(int x, int y, const Outlook& pattern_color):
        location(std::make_tuple(x,y)), outlook(pattern_color){}
    
    //获取位置和外观. 由于这几个函数比较简单, 就直接内联在这里了.
    const std::tuple<int, int>& get_location() const { return location; }
    const Outlook& get_outlook() const { return outlook; }
};

struct Object{
    
    std::vector<Body> body;//一个物体由多个部分组成, 逻辑上是链表.
    static Outlook default_outlook;//默认外观, 用于初始化物体的各个部分.

    Outlook& get_default_outlook(){ return default_outlook; }

    //提供一些操作 body 的接口.
    const std::vector<Body>& get_body() const;
    const Body& get_body(int i) const;
    int set_body(int i, Body& body_part);
    int delete_body(int i);
    int insert_body(int i, Body& body_part);

    Object(Outlook default_outlook);//需要传入默认外观.
    // 由于很多地方要用:Object(Outlook(std::string(logo),1)), 所以不用左值引用了.
    Object()=default;//给出默认构造函数. 此时default outloock为默认值. 见cpp文件.
    virtual void foo(){}//make it polymorphic in runtime.
};