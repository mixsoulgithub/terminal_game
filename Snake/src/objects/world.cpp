#include "world.hpp"
#include "snake.hpp"
#include "food.hpp"

World::World()
{
    
}

World::~World()
{

}

void World::update()
{
    
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
