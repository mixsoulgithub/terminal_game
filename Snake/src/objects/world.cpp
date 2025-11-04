#include "world.hpp"
#include "snake.hpp"
#include "food.hpp"
#include <limits.h>
World::World():m_start_time(std::chrono::steady_clock::now())
{
}

int World::m_up_limit=0;    
int World::m_down_limit=std::numeric_limits<int>::max();    
int World::m_left_limit=0;    
int World::m_right_limit=std::numeric_limits<int>::max();    
World::~World()
{

}

void World::update()
{
    
}

std::chrono::steady_clock::time_point World::now(){
    return std::chrono::steady_clock::now();
}
int World::set_limits(int up, int down, int left, int right){
    m_up_limit=up;
    m_down_limit=down;
    m_left_limit=left;
    m_right_limit=right;
    return 0;
}

std::tuple<int,int,int,int> World::get_limits(){
    return std::make_tuple(m_up_limit,m_down_limit,m_left_limit,m_right_limit);
}

const std::vector<std::shared_ptr<Object>>& World::get_objects() const {
    return m_objs;
}

int World::add_object(std::shared_ptr<Object> obj) {
    m_objs.emplace_back(obj);
    return 0;
}

template<typename T>
int World::search(std::vector<std::shared_ptr<Object>> ans, int all){
    for(auto&& obj : m_objs){
        if(typeid(obj)==typeid(T)){
            ans.emplace_back(obj);
            if(!all) return ans.size();
        }
    }
    return ans.size();
}

//here we can use next fit, search from last stop, and we can also use cache, determinate next n foods.
//if world become large, search from big block, then small block.
std::vector<std::tuple<int,int>> World::get_spare_space(){
    //static variable in function will be initialized when this func was first called.
    //differ with class or normal static variable, they initialized in random order before main func.
    static int w = m_left_limit;
    static int h = m_up_limit;
    std::vector<std::tuple<int,int>> spare_space;//this 100 should scale with w,h.
    long loop_count=0;
    //TODO: obstacle decrese all_space.
    long all_space = (m_right_limit-m_left_limit)*(m_down_limit-m_up_limit);
    for(;;w=((w+1)%(m_right_limit-m_left_limit)-m_left_limit)){
        for (;;h=((h+1)%(m_down_limit-m_up_limit)-m_up_limit)) {
            int collision_flag =0;
            for(auto&& obj:m_objs){
                for(auto&& [location,_]:obj->get_body()){
                    auto [_h,_w]=location;
                    if(_h==h&&_w==w){
                        collision_flag=1;
                        break;
                    }
                }
                if (collision_flag==1) {
                    break;
                }
            }
            if (collision_flag==0) {
                spare_space.emplace_back(std::move(std::make_tuple(h,w)));
            }
            loop_count++;
            //TODO: long >= int? 
            if(loop_count>=all_space){
                //empty means succeed!
                return std::vector<std::tuple<int,int>>();
            }
        }
    }
    return spare_space;
}
