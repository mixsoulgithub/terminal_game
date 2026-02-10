#include "terminal.hpp"
#include <cassert>
#include <iostream>
#include <string>
#include <sys/select.h>
int main() {
  struct termios raw = terminal::config_raw_mode();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
  terminal::cursor_off();
  terminal::clear_screen();
  terminal::string pieces("♔♚♕♛♖♜♗♝♘♞");
  terminal::string line3_6;
  for(int i=0; i<16; i++) {
    line3_6.emplace_back(" ");
  }
  terminal::string line7("♟ ♟ ♟ ♟ ♟ ♟ ♟ ♟ ");
  terminal::string line8("♜ ♞ ♝ ♛ ♚ ♝ ♞ ♜ ");
  terminal::string line1("♖ ♘ ♗ ♕ ♔ ♗ ♘ ♖ ");
  terminal::string line2("♙ ♙ ♙ ♙ ♙ ♙ ♙ ♙ ");
  terminal::color dark_square(0x000000, 0x4C2B20);
  terminal::color light_square(0x000000, 0xAF804F);
  std::vector<terminal::color> colors1;
  for(int i=0; i<4; i++) {
    colors1.push_back(dark_square);
    colors1.push_back(dark_square);
    colors1.push_back(light_square);
    colors1.push_back(light_square);
  }
  std::vector<terminal::color> colors2;
  for(int i=0; i<4; i++) {
    colors2.push_back(light_square);
    colors2.push_back(light_square);
    colors2.push_back(dark_square);
    colors2.push_back(dark_square);
  }
  // terminal::color light_square(0xFFFFFF, 0x000000);
  // std::vector<terminal::color> colors = {dark_square, light_square,dark_square, light_square,dark_square,
  // light_square,dark_square, light_square}; 
  std::cout<<terminal::color_it_all(line8, colors1)<<"\r\n";
  std::cout<<terminal::color_it_all(line7, colors2)<<"\r\n";
  for(int i=0; i<4; i++) {
    if(i%2==0) {
      std::cout<<terminal::color_it_all(line3_6, colors1)<<"\r\n";
    } else {
      std::cout<<terminal::color_it_all(line3_6, colors2)<<"\r\n";
    }
  }
  std::cout<<terminal::color_it_all(line2, colors1)<<"\r\n";
  std::cout<<terminal::color_it_all(line1, colors2)<<"\r\n";
  std::cout.flush();
  // ♔	王	King	♚
  //  ♕	后	Queen	♛
  //  ♖	车	Rook	♜
  //  ♗	象	Bishop	♝
  //  ♘	马	Knight	♞
  //  ♙	兵	Pawn	♟
  // ♔♚  ♕♛  ♖♜  ♗♝  ♘♞  ♙♟
  char c;
  fd_set readfds;
  while (true) {
    FD_ZERO(&readfds);
    FD_SET(STDIN_FILENO, &readfds);
    select(STDIN_FILENO + 1, &readfds, nullptr, nullptr, nullptr);
    if (read(STDIN_FILENO, &c, 1) == 1) {
      if (c == 'q')
        break;
    }
  }
  terminal::clear_screen();
  terminal::cursor_on();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &terminal::original_termios);
  return 0;
}