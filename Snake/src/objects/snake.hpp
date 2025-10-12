#pragma once
#include "object.hpp"
#include "world.hpp"

class Snake: public Object{

    typedef enum {
            UP=0b00,
            RIGHT=0b01,
            LEFT=0b10,
            DOWN=0b11
    }DIRECT;

    public:
    DIRECT dir;
    
    Snake(int h, int w, const char* logo);
    

    int refresh(World& world);

    /*
    * @return : -1 is game over, 0 is ok
    */ 
    int move(DIRECT new_dir, World& world);

    int check_collision();
    private:

        static int DIRECT_STEP[4][2];
};