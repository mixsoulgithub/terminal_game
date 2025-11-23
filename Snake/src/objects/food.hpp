#pragma once
#include "object.hpp"
#include "world.hpp"
#include <limits.h>
#include <random>

struct Food : public Object {
private:
  std::mt19937 seed;
  std::uniform_int_distribution<int> dist;

public:
  Food(const char *logo);
  Food(int x, int y, const char *logo);
  int generate(World &world);
};
