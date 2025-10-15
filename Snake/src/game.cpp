#include <unistd.h>
#include <ncurses.h>

#include "game.hpp"

Game::Game():
    m_is_paused(false),
    m_is_game_over(false),
    m_score(0),
    m_snake(0, 0, "@"),
    m_food(5, 5, "*"),
    m_world(),
    m_scoreboard()
{
    // unsigned long last_drop_time = (unsigned long)clock() * 1000 / CLOCKS_PER_SEC;
    m_world.add_object(m_snake);
    m_world.add_object(m_food);
}

Game::~Game()
{

}

void Game::processInput()
{
    int ch = getch();
        // 处理输入
        if (!m_is_paused)
        {
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
                    break;
            }
            refresh();
            usleep(10000);  // 10ms延迟，减少CPU使用
        }
}

void Game::update()
{

}
void Game::render()
{
    m_frame.flush_to_screen(m_world);
}