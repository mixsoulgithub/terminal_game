#include "frame.hpp"
#include <cstdio>
#include <vector>

//一帧是由若干个图层组成的.不过按渲染顺序来. 逻辑和渲染分离.
Frame::Frame(int H,  int W,  int OH,  int OW):
    m_height(H), m_width(W), m_offset_h(OH), m_offset_w(OW){
    m_pixels = std::move(std::vector<std::vector<std::string>>(m_height, std::vector<std::string>(m_width,std::string(" "))));
}

Frame::~Frame() {
    // No dynamic memory to free, but if there were, it would be done here.
}

//show pixels on screen.
bool Frame::flush_to_screen(const World& world)
{
    // 这里 get_objects 的返回值类型稳定，可以不使用 auto 自动推导
    const std::vector<std::shared_ptr<Object>>& objs = world.get_objects();//copy or move?
    for(const std::shared_ptr<Object>& obj : objs) //template delays type makes it's harder to find objs/obj typo.
    {
        const std::vector<Body>& obj_body = obj->get_body();//no copy cost. Function 'get_body' with deduced return type cannot be used before it is defined, this is because of speration of declaration and definition.
        for(auto&& [location, outlook] : obj_body)
        {
            auto [H, W] = location;
            auto [pattern, color] = outlook;
            // std::printf("H: %d, W: %d, outlook: %s\n", H, W, (obj_outlook[i]).c_str());
            attron(color);
            mvaddstr(/*m_height + */H, /*m_width + */W, pattern.c_str());
            attroff(color);//here
        }
    }
    return true;
}
//detect if it is clash with other object
//only sometimes it will check. 
// template<typename Container>
// auto check_collision(Object it) -> decltype(std::forward<Container>(objs), int()) {
//     using ValueType = typename std::decay_t<Container>::value_type;
//     static_assert(
//         std::is_same_v<ValueType, std::shared_ptr<Object>>,
//         "Container must hold shared_ptr<Object>"
//     );
//     static_assert(
//         std::is_same_v<std::decay_t<Container>, std::vector<ValueType>>,
//         "Container must be a vector"
//     );
//     for(auto&& obj : objs){
//         if(obj.get()==&it) continue;//skip self
//         for(auto [h1,w1]:it.body){
//             for(auto [h2,w2]:(*obj).body){
//                 if(h1==h2 && w1==w2){
//                     return 1;
//                 }
//             }
//         }
//     }
//     return 0;
// }

//if all =1, return all as vector. if all =0, return first found.
int Frame::set_pixels(std::vector<std::tuple<int,int>>& body , std::vector<std::string>& pixels) {
    int len = body.size();
    for(int i = 0; i < len; i++){
        auto [h,w] = body[i];
        set_pixel(h, w, pixels[i]);
    }
    return 0;
}

int Frame::set_pixel(int h, int w, const std::string& pixel){
    if(h < 0 || h >= m_height || w < 0 || w >= m_width) return -1;
    this->m_pixels[h][w] = pixel;
    return 0;
}
