#pragma once
#include <vector>
#include "object.hpp"

class World{
    std::vector<std::shared_ptr<Object>> objs;//todo: lock
};