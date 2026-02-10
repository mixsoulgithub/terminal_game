#include <cassert>
#include <iostream>
#include <string>
#include "terminal.hpp"
int main() {
  struct termios raw = terminal::config_raw_mode();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
  terminal::cursor_off();
  terminal::clear_screen();
  std::cout << terminal::color_it_all(terminal::string("🎉新年快樂🎉"), terminal::color(0x00FF00, 0xFF0000)) << "\r\n";
  terminal::couplet("凱歌送舊歲", "駿馬迎新春", "馬到成功");
  char c;
  while (true) {
    if (read(STDIN_FILENO, &c, 1) == 1) {
      if (c == 'q') break;
    }
  }
  terminal::clear_screen();
  terminal::cursor_on();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &terminal::original_termios);
  return 0;
}