#pragma once
#include <vector>
#include <memory>
#include <typeinfo>
#include "object.hpp"

class World{
public:
    World();
    ~World();
    void update();
    //return reference of objs
    std::vector<Object>& get_objects();//Function 'get_objects' with deduced return type cannot be used before it is defined
    int add_object(Object& obj);

    template<typename T>
    int search(std::vector<Object>& ans, int all);
protected:
    std::vector<Object> m_objs;//todo: lock
};