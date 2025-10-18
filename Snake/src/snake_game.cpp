#include <unistd.h>

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
                attron(COLOR_PAIR(6));  // 启用颜色对6：白色前景，黑色背景
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
                break;
            case KEY_RIGHT:
                break;
            case KEY_UP:
                break;
            case KEY_DOWN:
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
        refresh();
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