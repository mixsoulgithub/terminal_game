#include "world.hpp"
#include "snake.hpp"
#include "food.hpp"

World::World()
{
    std::shared_ptr<Snake> snake = std::make_shared<Snake>(0, 0, "@");
    add_object(snake);

    std::shared_ptr<Food> food = std::make_shared<Food>(5, 5, "*");
    add_object(food);
}

World::~World()
{

}

void World::update()
{
    
}

std::vector<std::shared_ptr<Object>>& World::get_objects() {
    return m_objs;
}

int World::add_object(std::shared_ptr<Object> obj) {
    m_objs.emplace_back(obj);
    return 0;
}

template<typename T>
int World::search(std::vector<std::shared_ptr<Object>>& ans, int all){
    for(auto&& obj : m_objs){
        if(typeid(*obj)==typeid(T)){
            ans.emplace_back(obj);
            if(!all) return ans.size();
        }
    }
    return ans.size();
}


int World::check_collision(Object &it){
    // for(){

    // }
    return 0;
}