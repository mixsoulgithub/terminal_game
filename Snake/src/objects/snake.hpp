#pragma once
#include "object.hpp"

class Snake: public Object{

    typedef enum {
        UP=0b00,
        RIGHT=0b01,
        LEFT=0b10,
        DOWN=0b11
    }DIRECT;

    int DIRECT_STEP[4][2]{ //H, W
        {-1,0},
        {0,-1},
        {0,1},
        {1,0}
    };
    public:
    Snake::DIRECT dir;
    
    Snake(int h, int w, const char* logo="#");
    

    int flush(Frame* frame);

    /*
    * @return : -1 is game over, 0 is ok
    */ 
    int move(DIRECT new_dir, Frame& frame);

    int check_collision();
};