#pragma once

#include "object.hpp"
#include "world.hpp"

struct ScoreBoard:public Object{
    public:
    ScoreBoard();

    int flush();
    int flush(int x, int y);
    int get_score() const { return score; }
    private:
        int score;
        int level;
        int lines_cleared;
};