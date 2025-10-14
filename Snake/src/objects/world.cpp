#include "world.hpp"

std::vector<Object>& World::get_objects() {
    return objs;
}

int World::add_object(Object& obj) {
    objs.emplace_back(obj);
    return 0;
}

template<typename T>
int World::search(std::vector<Object>& ans, int all){
    for(auto&& obj : objs){
        if(typeid(obj)==typeid(T)){
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