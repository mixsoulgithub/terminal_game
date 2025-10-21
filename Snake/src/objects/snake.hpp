#pragma once
#include "object.hpp"
#include "world.hpp"

typedef enum {
            UP=0b00,
            RIGHT=0b01,
            LEFT=0b10,
            DOWN=0b11
}DIRECT;

class Snake: public Object{
    // the body of snake is tail->middle->head.
    public:
    Snake(int h, int w, const Outlook& default_outlook, DIRECT dir);

    int refresh(World& world);

    int update_dir(DIRECT new_dir);
    int move(World& world);
    // int check_collision();
    DIRECT get_dir();

    int grow(Outlook& outlook=default_outlook);//when eat something, snake will grow.

    // test functions
    void test_move_offset_foce(int w_offset, int h_offset){
        for (int i = 0; i < body.size(); i++){
            auto& [x,y]=body[i].location;
            x+=h_offset;
            y+=w_offset;
        }
    }

    private:
        DIRECT dir;
        static int DIRECT_STEP[4][2];
};