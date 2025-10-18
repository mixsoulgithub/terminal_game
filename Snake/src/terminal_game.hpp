#pragma once
#include "objects/scoreboard.hpp"
#include "frame.hpp"
#include "objects/world.hpp"
#include "objects/snake.hpp"
#include "objects/food.hpp"

// 所有终端游戏的基类
// 提供基本的游戏循环结构和接口
// 具体游戏逻辑由派生类实现
// 如果需要添加新的游戏类型，只需继承此类并实现纯虚函数
class TerminalGame
{
public:
    TerminalGame();
    virtual ~TerminalGame();

    // 这里是游戏循环的三个核心步骤，派生类必须实现它们
    virtual void processInput() = 0;
    virtual void update() = 0;
    virtual void render() = 0;

    bool getIsGameOver() const { return m_is_game_over; }
protected:
    // 这里存放所有终端游戏都应该有的元素：游戏结束标志，游戏世界，渲染帧等
    bool m_is_game_over;
    Frame m_frame;
    World m_world;
};

