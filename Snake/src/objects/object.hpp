#pragma once
#include <vector>
#include <string>
#include <tuple>

struct Object{
    virtual void foo(){}//make it polymorphic in runtime.
    std::vector<std::tuple<int, int>> body;
    std::vector<std::string> outlook;
    std::vector<int> color;

    std::vector<std::tuple<int, int>>& get_body();
    std::vector<std::string>& get_outlook();
    std::vector<int>& get_color();
};