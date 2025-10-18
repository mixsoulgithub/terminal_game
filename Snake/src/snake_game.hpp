#pragma once
#include "terminal_game.hpp"

// 贪吃蛇游戏的具体实现，继承自 TerminalGame
class SnakeGame : public TerminalGame
{
public:
    SnakeGame();
    ~SnakeGame();
    virtual void processInput() override;
    virtual void update() override;
    virtual void render() override;

protected:
    // 这里存放蛇游戏特有的元素：暂停标志，分数，记分板等
    bool m_is_paused;
    int m_score;
    ScoreBoard m_scoreboard;
};