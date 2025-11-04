#pragma once
#include <random>
#include <limits.h>
#include "object.hpp"
#include "world.hpp"

struct Food:public Object{
private:
    std::mt19937 seed;
    std::uniform_int_distribution<int> dist;
public:
    
    Food(const char* logo);
    Food(int x , int y, const char* logo);
    int generate(World& world);
};
