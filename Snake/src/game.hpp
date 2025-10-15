#pragma once
#include "objects/scoreboard.hpp"
#include "frame.hpp"
#include "objects/world.hpp"
#include "objects/snake.hpp"
#include "objects/food.hpp"

class Game
{
public:
    Game();
    ~Game();
    void processInput();
    void update();
    void render();

    bool getIsGameOver() const { return m_is_game_over; }
protected:
    bool m_is_paused;
    bool m_is_game_over;
    int m_score;
    ScoreBoard m_scoreboard;
    Frame m_frame;
    World m_world;
    Snake m_snake;
    Food m_food;
};

