#include "object.hpp"


std::vector<std::tuple<int, int>>& Object::get_body(){
    return body;
}
std::vector<std::string>& Object::get_outlook(){
    return outlook;
}
std::vector<int>& Object::get_color(){
    return color;
}