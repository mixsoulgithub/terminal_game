#include "collision.hpp"
#include <memory>

namespace Collision{

    template<typename TA, typename TB>
        COLLISION_TYPE check_collision(TA& A, TB& B){
        // 默认行为：当作两个Object处理, 注意, 这个实现是必须的, Object, Snake没有实现并不会去找Objet, Object的特化版本, 而是lookup到这里.
        //如果没有, 则会链接错误.
        return check_collision<Object, Object>(
            static_cast<Object&>(A), 
            static_cast<Object&>(B)
    );
}

    template<>
    COLLISION_TYPE check_collision<Object, Object>(Object& A, Object&B){
        auto& Abody=A.get_body();
        auto& Bbody=B.get_body();
        int Alen=Abody.size();
        int Blen=Bbody.size();
        for(int i=0;i<Alen;++i){
            for(int j=0;j<Blen;++j){
                auto&& [A_location, A_outlook]=Abody[i];
                auto&& [B_location, B_outlook]=Bbody[j];
                if(A_location==B_location){
                    return SOLVABLE;
                }
            }
        }
        return NONE;
    }

    template<>
    COLLISION_TYPE check_collision<Snake, Food>(Snake& snake, Food& food){
        auto& snake_body=snake.get_body();
        auto& food_body=food.get_body();
        int snake_len=snake_body.size();
        //only head of snake can eat something.
        auto&& [s_location, s_outlook]=snake_body[snake_len-1];
        int food_len=food_body.size();
        for(int j=0;j<food_len;++j){
            auto&& [f_location, f_outlook]=food_body[j];
            if(s_location==f_location){
                snake.grow();
                food.delete_body(j);
                return SOLVABLE;
            }
        }
        return NONE;
    }
    //not used so far
    template<>
    COLLISION_TYPE check_collision<Food, Snake>(Food& food, Snake& snake){
        auto& snake_body=snake.get_body();
        auto& food_body=food.get_body();
        int snake_len=snake_body.size();
        int food_len=food_body.size();
        for(int i=0;i<snake_len;i++){
        for(int j=0;j<food_len;++j){
            auto&& [s_location,s_outlook]=snake_body[i];
            auto&& [f_location, f_outlook]=food_body[j];
            if(s_location==f_location){
                return UNSOLVABLE;
            }
        }
        }
        return NONE;
    
    }
    
    template<>
    COLLISION_TYPE check_collision<World, std::tuple<int,int>>(World& world, std::tuple<int,int>& location){
        auto objs=world.get_objects();
        for(auto&& obj: objs){
            for(auto&& [obj_location, _]:obj->get_body()){
               if(obj_location==location){
                   return UNSOLVABLE;
               } 
            }
        }
        return NONE;
    }

    template<typename T>
    COLLISION_TYPE check_collision(T& A){
        auto& body=A.get_body();
        int len=body.size();
        auto& head_location=body[0].get_location();
        for(int i=1;i<len;i++){
            if(head_location==body[i].get_location()){
                return UNSOLVABLE;
            }
        }
        return NONE;
    }

    template<>
    COLLISION_TYPE check_collision<Snake>(Snake& snake){
        auto& body=snake.get_body();
        int len=body.size();
        auto& head_location=snake.get_head().get_location();
        for(int i=0;i<len-1;i++){
            if(head_location==body[i].get_location()){
                return UNSOLVABLE;
            }
        }
        return NONE;
    }

    template<>
    COLLISION_TYPE check_collision<World, Snake>(World& world, Snake& snake){
        COLLISION_TYPE ans=NONE;
        auto objs=world.get_objects();
        for(auto&& obj: objs){
            if(std::addressof(*obj)==std::addressof(snake)){
                continue;
            }
            COLLISION_TYPE tmp = check_collision(*obj,snake);
            if(tmp==UNSOLVABLE){
                return UNSOLVABLE;
            }else if(tmp==SOLVABLE){
                ans=SOLVABLE;
            }
        }
        return NONE;
    }

    
}
