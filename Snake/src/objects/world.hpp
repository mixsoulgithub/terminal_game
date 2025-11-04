#pragma once
#include <vector>
#include <memory>
#include <typeinfo>
#include "object.hpp"
#include <chrono>

//TODO: use singleton pattern.
class World{
public:
    World();
    ~World();
    void update();
    //return reference of objs
    const std::vector<std::shared_ptr<Object>>& get_objects() const;//Function 'get_objects' with deduced return type cannot be used before it is defined, 用auto不行.
    //shared_ptr不用引用, 不过是多了一个引用计数而已, 开销不大. 且这样可以接受左值和右值, 更加灵活. 也不用完美转发的冗长代码.

    int add_object(std::shared_ptr<Object> obj);

    template<typename T>
    int search(std::vector<std::shared_ptr<Object>> ans, int all);
    //为了多态, 这里一定不能只用Object作为vector对象. 静态多态是内存切片, 这样只能使用Object本身的方法(因为其它子类方法都被切掉了.)
    

    int set_limits(int up, int down, int left, int right);
    std::tuple<int,int,int,int> get_limits();

    std::vector<std::tuple<int,int>> get_spare_space();
    
    std::chrono::steady_clock::time_point now();
protected:
    const std::chrono::steady_clock::time_point m_start_time; 
    std::vector<std::shared_ptr<Object>> m_objs;//todo: lock
    static int m_up_limit;
    static int m_down_limit;
    static int m_right_limit;
    static int m_left_limit;
};
