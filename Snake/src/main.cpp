#include <memory>
#include "snake_game.hpp"
int debug_h=0;
int main()
{
    // 当新增游戏种类时，只需要将下面的 std::make_shared<SnakeGame>() 替换成 NewGameType 即可
    std::unique_ptr<TerminalGame> game = std::make_unique<SnakeGame>();
    
    if (game->initialize())
    {
        game->run();
    }

    game->shutdown();
    
    return 0;
}
