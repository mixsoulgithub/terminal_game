#pragma once
#include <vector>
#include <string>
#include <tuple>
#include "../frame.hpp"

struct Object{
    virtual void foo(){}//make it polymorphic in runtime.
    std::vector<std::tuple<int, int>> body;
    std::vector<std::string> outlook;
    std::vector<int> color;

};