#pragma once
#include "frame.hpp"

//一帧是由若干个图层组成的.不过按渲染顺序来. 逻辑和渲染分离.

Frame::Frame(const int H=30, const int W=40, const int OH=2, const int OW=5):
    HEIGHT(H), WIDTH(W), OFFSET_H(OH), OFFSET_W(OW){
    pixels=std::move(std::vector<std::vector<std::string>>(HEIGHT, std::vector<std::string>(WIDTH,std::string(" "))));
}


    //show pixels on screen.
    auto flush_to_screen(World& world) {
        using namespace Frame;
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