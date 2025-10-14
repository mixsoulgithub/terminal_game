#pragma once
#include <vector>
#include <memory>
#include "object.hpp"

class World{
public:
    //return reference of objs
    std::vector<Object>& get_objects();//Function 'get_objects' with deduced return type cannot be used before it is defined
    int add_object(Object& obj);
    int check_collision(Object& it);

    template<typename T>
    int search(std::vector<Object>& ans, int all);
private:
    std::vector<Object> objs;//todo: lock
};