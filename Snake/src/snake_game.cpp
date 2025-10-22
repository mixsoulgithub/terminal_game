#include <unistd.h>
#include <fstream>
#include "tool/json.hpp"
#include "snake_game.hpp"

SnakeGame::SnakeGame(): TerminalGame(), m_is_paused(false), m_score(0), m_scoreboard()
{
    
}

SnakeGame::~SnakeGame()
{

}

bool SnakeGame::buildFromConfigFile(const std::string& configFilePath)
{
    // 读取配置文件并初始化游戏参数
    nlohmann::json configJson;
    
    std::ifstream file(configFilePath);
    if (!file.is_open())
    {
        return false;
    }
    file >> configJson;
    file.close();

    // 根据配置文件内容初始化游戏元素
    // 例如，创建蛇和食物对象并添加到游戏世界中
    /* Snake */
    auto snake_element = configJson["snake"];
    std::string snake_symbol = snake_element["symbol"];
    int snake_length = snake_element["initial_length"];
    int snake_speed = snake_element["initial_speed"];
    auto snake_position = snake_element["initial_position"];
    int w = snake_position["row"];
    int h = snake_position["col"];
    std::string snake_direction = snake_element["initial_direction"];
    DIRECT dir = RIGHT; //default
    if (snake_direction == "UP") {
        dir = UP;
    } else if (snake_direction == "DOWN") {
        dir = DOWN;
    } else if (snake_direction == "LEFT") {
        dir = LEFT;
    } else if (snake_direction == "RIGHT") {
        dir = RIGHT;
    }
    Outlook snake_outlook = Outlook(snake_symbol, ColorMode::FRONT_YELLOW_BACK_WHITE);
    m_snake = std::make_shared<Snake>(h, w, snake_outlook, dir);
    m_world.add_object(m_snake);

    /* Food */
    auto food_element = configJson["food"];
    std::string food_symbol = food_element["symbol"];
    auto food_points = food_element["points"];
    for (const auto& point : food_points) {
        int x = point["row"];
        int y = point["col"];

        std::shared_ptr<Food> food = std::make_shared<Food>(x, y, food_symbol.c_str());
        m_world.add_object(food);
        m_foods.push_back(food);
    }
    
    return true;
}

void SnakeGame::processInput()
{
    int ch = getch();
        // 处理输入
        switch (ch)
        {
            case 'q':
            case 'Q':
                // 在屏幕顶部显示游戏结束信息和最终分数
                mvprintw(0, 0, "Game Over! Final Score: %d", m_scoreboard.get_score());
                // 将输入模式改回阻塞模式（无限等待）
                timeout(-1);
                // 等待用户按任意键继续
                getch();
                // 退出NCurses模式，恢复终端原始状态
                endwin();
                m_is_game_over = true;
                break;
            case KEY_LEFT:
                m_snake->test_move_offset_foce(-1, 0); //测试用, 强制向下移动
                break;
            case KEY_RIGHT:
                m_snake->test_move_offset_foce(1, 0); //测试用, 强制向上移动
                break;
            case KEY_UP:
                m_snake->test_move_offset_foce(0, -1); //测试用, 强制向左移动
                break;
            case KEY_DOWN:
                m_snake->test_move_offset_foce(0, 1); //测试用, 强制向左移动
                break;
            case ' ':
                break;
            case 'p':
            case 'P':
                m_is_paused = true;
                mvprintw(0, 0, "Game is Paused. Press 'c' to continue.");
                break;
            case 'c':
            case 'C':
                m_is_paused = false;
                mvprintw(0, 0, "                                      "); // Clear the pause message
                break;
        }
        usleep(10000);  // 10ms延迟，减少CPU使用
}

void SnakeGame::update()
{
    if(m_is_paused) return;
    m_world.update();
}
void SnakeGame::render()
{
    m_frame.flush_to_screen(m_world);
}