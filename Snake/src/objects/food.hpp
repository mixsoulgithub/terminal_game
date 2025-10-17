#pragma once

#include "object.hpp"
#include "world.hpp"

struct Food:public Object{
    public:

    Food(int x , int y, const char* logo);
};