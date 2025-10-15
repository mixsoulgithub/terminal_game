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
    std::vector<std::shared_ptr<Object>>& get_objects();//Function 'get_objects' with deduced return type cannot be used before it is defined
    int add_object(std::shared_ptr<Object> obj);
    int check_collision(Object& it);

    template<typename T>
    int search(std::vector<std::shared_ptr<Object>>& ans, int all);
protected:
    std::vector<std::shared_ptr<Object>> m_objs;//todo: lock
};