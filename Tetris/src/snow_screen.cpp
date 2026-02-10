#include "terminal.hpp"
#include <cassert>
#include <iostream>
#include <string>
#include <sys/select.h>
#include <random>

void frame(){
  auto [width,height]=terminal::get_winsize();
  std::random_device rd;
  std::mt19937 gen(rd());
  std::uniform_int_distribution<int> posdis(0, width-1);
  std::uniform_int_distribution<int> numdis(0, (int)width/3);
  std::string snow(width, ' ');//0xFFFFFF, 0x000000 for space is empty
  for(int h=0; h<height; h++) {
  std::vector<terminal::color> colors(width, terminal::color(0x000000, 0x000000));
    for(int i=0; i<numdis(gen); i++) {
      colors[posdis(gen)] = terminal::color(0xFFFFFF, 0xFF9000);
    }
    std::cout<<terminal::color_it_all(snow, colors);
  }
  std::cout.flush();
}

int main() {
  struct termios raw = terminal::config_raw_mode();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
  terminal::cursor_off();
  terminal::clear_screen();
  char c;
  fd_set readfds;
  struct timeval timeout;
  while (true) {
    frame();
    FD_ZERO(&readfds);
    FD_SET(STDIN_FILENO, &readfds);
    timeout.tv_sec = 0;
    timeout.tv_usec = 0;  // 非阻塞：立即返回，只检查是否有输入
    if (select(STDIN_FILENO + 1, &readfds, nullptr, nullptr, &timeout) > 0) {
      if (read(STDIN_FILENO, &c, 1) == 1) {
        if (c == 'q')
          break;
      }
    }
    usleep(300000);
    std::cout << "\033[H"; //\033[H, 移动光标到左上角
    std::cout.flush();
  }
  terminal::clear_screen();
  terminal::cursor_on();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &terminal::original_termios);
  return 0;
}