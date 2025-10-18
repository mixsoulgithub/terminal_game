#include <unistd.h>
#include <ncurses.h>

#include "terminal_game.hpp"

TerminalGame::TerminalGame():
    m_is_game_over(false),
    m_world()
{
    
}

TerminalGame::~TerminalGame()
{

}