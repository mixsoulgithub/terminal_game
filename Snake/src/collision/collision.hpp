#pragma once
#include "../objects/object.hpp"
#include "../objects/snake.hpp"
#include "../objects/food.hpp"
#include "../objects/scoreboard.hpp"
#include "../objects/world.hpp"

namespace Collision{
    enum COLLISION_TYPE{
        NONE=0,
        UNSOLVABLE=1,
        SOLVABLE=2
    };
    //those are template partial specialization, somehow a little triky. We can use RTTI instead. But now let it go. 
    template<typename TA, typename TB>
    COLLISION_TYPE check_collision(TA& A, TB&B);

    template<>
    COLLISION_TYPE check_collision<Object, Object>(Object& A, Object& B);
    

    //这里还是有一个对称问题. 也有Food, Snake的组合. 目前想到的比较直观的方法是把所有组合都写出来.
    template<>
    COLLISION_TYPE check_collision<Snake, Food>(Snake& A, Food&B);

    template<>
    COLLISION_TYPE check_collision<Food, Snake>(Food& B, Snake& A);


    template<>
    COLLISION_TYPE check_collision<World, Snake>(World& world, Snake& snake);

    template<typename T>
    COLLISION_TYPE check_collision(T& A);

    template<>
    COLLISION_TYPE check_collision<Snake>(Snake& snake);

    
}