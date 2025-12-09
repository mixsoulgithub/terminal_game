#pragma once
#include "color/color_system.hpp"
#include <string>
#include <tuple>
#include <vector>

enum COLLISION_TYPE { NONE = 0, UNSOLVABLE = 1, SOLVABLE = 2 };
using Outlook = std::tuple<std::string, ColorMode>; // pattern, color

// Body 用于存储物体的每个部分的位置和外观
struct Body {
  std::tuple<int, int> location;
  // 花纹, pattern, 也就是字符. 以及颜色, 严格来讲是颜色对.
  Outlook outlook; 

  // 重载了两份构造函数.
  Body(int x, int y, const std::string &pattern, const ColorMode color_mode = ColorMode::FRONT_WHITE_BACK_BLACK)
      : location(std::make_tuple(x, y)), outlook(std::make_tuple(pattern, color_mode)) {}

  Body(int x, int y, const Outlook &pattern_color) : location(std::make_tuple(x, y)), outlook(pattern_color) {}

  // 获取位置和外观. 由于这几个函数比较简单, 就直接内联在这里了.
  Body(std::tuple<int, int> location, const Outlook &pattern_color) : location(location), outlook(pattern_color) {}

  const std::tuple<int, int> &get_location() const { return location; }
  const Outlook &get_outlook() const { return outlook; }
};

struct Object {

protected:
//head of sneak is at body.size()-1
  std::vector<Body> body;  
  Outlook default_outlook; 
  int m_is_changed;

public:
  // 不用左值引用, 方便传入临时对象.
  Object(Outlook default_outlook); 
  // because different object need their own m_is_changed, so m_is_changed is
  // not static, and so we need non-default Object constructor to initialize.
  Object();

  Outlook &get_default_outlook() { return default_outlook; }
  int is_changed() { return m_is_changed; } 
  void change() { m_is_changed = 1; }       
  void unchange() { m_is_changed = 0; }

  const std::vector<Body> &get_body() const;
  const Body &get_body(int i) const;
  int set_body(int i, Body &body_part);
  int delete_body(int i);
  int insert_body(int i, Body &body_part);

  virtual void foo() {} // make it polymorphic in runtime.
};
