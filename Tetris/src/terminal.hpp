#pragma once
//https://www.qtccolor.com/secaiku/
#include <cassert>
#include <iostream>
#include <stdexcept>
#include <string>
#include <termios.h>
#include <tuple>
#include <unistd.h>
#include <vector>
#include <sys/ioctl.h>
#include <unistd.h>
namespace terminal {
inline void cursor_on() {
  std::cout << "\033[?25h";
  std::cout.flush();
}
inline void cursor_off() {
  std::cout << "\033[?25l";
  std::cout.flush();
}
inline void clear_screen() {
  std::cout << "\033[2J\033[H"; //\033[2J, 清屏; \033[H, 移动光标到左上角
  std::cout.flush();
}
inline std::tuple<int, int> get_winsize() {
  struct winsize w;
  if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == -1) {
    throw std::runtime_error(__FUNCTION__); // 默认宽度
  }
  return std::make_tuple(w.ws_col,w.ws_row);
}
inline struct termios original_termios;
inline struct termios config_raw_mode() {
  tcgetattr(STDIN_FILENO, &original_termios);

  struct termios raw = original_termios;

  // 禁用以下功能:
  raw.c_iflag &= ~(BRKINT | INPCK | ISTRIP | IXON);
  // BRKINT: 中断信号
  // ICRNL: 将 CR 转换为 NL
  // INPCK: 奇偶校验
  // ISTRIP: 剥离第8位
  // IXON: 软件流控 (Ctrl-S/Ctrl-Q)

  raw.c_oflag &= ~(OPOST);
  // OPOST: 输出后处理, 禁用后, \n 不会自动移动到行首

  raw.c_cflag |= (CS8);
  // CS8: 8位字符大小

  raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);
  // ECHO: 回显输入字符 (关键! 禁用后输入不会显示)
  // ICANON: 规范模式 (禁用行缓冲, 立即读取每个字符)
  // IEXTEN: 扩展输入处理
  // ISIG: 信号字符 (Ctrl-C, Ctrl-Z 等)

  raw.c_cc[VMIN] = 0;  // 最小读取字符数 (0 = 非阻塞)
  raw.c_cc[VTIME] = 0; // 超时时间 (0 = 立即返回)

  return raw;
  // tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
}

class string : public std::vector<std::string> {
public:
  string() {}
  string(const char *str) : string(std::string(str)) {}
  string(const std::string &str) {
    int i = 0;
    int len = str.size();
    while (i < len) {
      unsigned char byte = static_cast<unsigned char>(str[i]);
      int charLen = 1;

      // 判断 UTF-8 字符的字节数
      if ((byte & 0x80) == 0) {
        // ASCII 字符 (0xxxxxxx)
        charLen = 1;
      } else if ((byte & 0xE0) == 0xC0) {
        // 2 字节字符 (110xxxxx)
        charLen = 2;
      } else if ((byte & 0xF0) == 0xE0) {
        // 3 字节字符 (1110xxxx)
        charLen = 3;
      } else if ((byte & 0xF8) == 0xF0) {
        // 4 字节字符 (11110xxx)
        charLen = 4;
      }

      // 提取完整的 UTF-8 字符
      this->emplace_back(str.substr(i, charLen));
      i += charLen;
    }
  }
  friend std::ostream &operator<<(std::ostream &os, const string &str) {
    for (int i = 0; i < str.size(); i++) {
      os << str[i];
    }
    return os;
  }

  friend string operator+(std::string str, const string str2) {
    string tmp(str);
    tmp = tmp + str2;
    return tmp;
  }
  // friend string operator+(const char *str, const string str2) {
  //   string tmp(str);
  //   tmp = tmp + str2;
  //   return tmp;
  // }
  string operator+(const string str) {
    for (int i = 0; i < str.size(); i++) {
      this->emplace_back(str[i]);
    }
    return *this;
  }
  string operator+(const std::string str) {
    string tmp(str);
    *this = *this + tmp;
    return *this;
  }
  string operator+(const char *str) { return *this + std::string(str); }
};
class color {
public:
  uint8_t fr;
  uint8_t fg;
  uint8_t rb;
  uint8_t br;
  uint8_t bg;
  uint8_t bb;
  constexpr color(const uint8_t fr, const uint8_t fg, const uint8_t rb, const uint8_t br, const uint8_t bg,
                  const uint8_t bb)
      : fr(fr), fg(fg), rb(rb), br(br), bg(bg), bb(bb) {}
  constexpr color(const int fc, const int bc) {
    fr = (uint8_t)((fc >> 16) & 0xFF);
    fg = (uint8_t)((fc >> 8) & 0xFF);
    rb = (uint8_t)((fc) & 0xFF);
    br = (uint8_t)((bc >> 16) & 0xFF);
    bg = (uint8_t)((bc >> 8) & 0xFF);
    bb = (uint8_t)((bc) & 0xFF);
  }
  friend std::ostream &operator<<(std::ostream &os, const color &c) {
    // os<<"debug"<<std::endl;
    // os<<"fr,fg,rb,br,bg,bb:"<<(int)c.fr<<","<<(int)c.fg<<","<<(int)c.rb<<","<<(int)c.br<<","<<(int)c.bg<<","<<(int)c.bb<<std::endl;
    return os;
  }
};
inline std::string color_it_all(std::string str, color c) {
  std::string tmp = "\033[38;2;" + std::to_string((int)c.fr) + ";" + std::to_string((int)c.fg) + ";" +
                    std::to_string((int)c.rb) + ";" + "48;2;" + std::to_string((int)c.br) + ";" +
                    std::to_string((int)c.bg) + ";" + std::to_string((int)c.bb) + "m" + str + "\033[0m";
  return tmp;
}
inline string color_it(string str, color c, int index) {
  assert(index >= 0 && index < str.size() && "index out of range");
  std::cout << c << "\r\n";
  string tmp(str);
  tmp[index] = "\033[38;2;" + std::to_string((int)c.fr) + ";" + std::to_string((int)c.fg) + ";" +
               std::to_string((int)c.rb) + ";" + "48;2;" + std::to_string((int)c.br) + ";" + std::to_string((int)c.bg) +
               ";" + std::to_string((int)c.bb) + "m" + str[index] + "\033[0m";
  return tmp;
}
inline string color_it_all(string str, color c) {
  string tmp = "\033[38;2;" + std::to_string((int)c.fr) + ";" + std::to_string((int)c.fg) + ";" +
               std::to_string((int)c.rb) + ";" + "48;2;" + std::to_string((int)c.br) + ";" + std::to_string((int)c.bg) +
               ";" + std::to_string((int)c.bb) + "m" + str + "\033[0m";
  return tmp;
}
// 检测字符是否是国际象棋字符（或其他需要特殊处理的 1.5 宽字符）
inline bool is_chess_piece(const std::string& ch) {
  // 国际象棋字符的 Unicode 范围: U+2654-U+265F
  // ♔ U+2654, ♕ U+2655, ♖ U+2656, ♗ U+2657, ♘ U+2658, ♙ U+2659 (白棋)
  // ♚ U+265A, ♛ U+265B, ♜ U+265C, ♝ U+265D, ♞ U+265E, ♟ U+265F (黑棋)
  if (ch.size() == 3) {  // 这些字符是 3 字节 UTF-8
    unsigned char b0 = static_cast<unsigned char>(ch[0]);
    unsigned char b1 = static_cast<unsigned char>(ch[1]);
    unsigned char b2 = static_cast<unsigned char>(ch[2]);
    // 检查是否是 U+2654-U+265F 范围 (UTF-8: E2 99 [94-9F])
    if (b0 == 0xE2 && b1 == 0x99 && b2 >= 0x94 && b2 <= 0x9F) {
      return true;
    }
  }
  return false;
}

inline string color_it_all(string chars, std::vector<color> c) {
  assert(chars.size() == c.size());
  string tmp;
  for (int i = 0; i < chars.size(); i++) {
    std::string color_seq = "\033[38;2;" + std::to_string(c[i].fr) + ";" + std::to_string(c[i].fg) + ";" + 
                            std::to_string(c[i].rb) + ";48;2;" + std::to_string(c[i].br) + ";" + 
                            std::to_string(c[i].bg) + ";" + std::to_string(c[i].bb) + "m";
    
    // 对于国际象棋字符（1.5 宽），在字符后添加一个空格，使其占据完整的 2 个 cell
    // 空格也需要相同的背景色来保持颜色连续性
    // if (is_chess_piece(chars[i])) {
    //   // 空格在颜色序列内，这样空格也会有相同的背景色
    //   tmp.emplace_back(color_seq + chars[i] + " " + "\033[0m");
    // } else {
      tmp.emplace_back(color_seq + chars[i] + "\033[0m");
    // }
  }
  return tmp;
}

inline void move_cursor(int down, int right) {
  if (down > 0) {
    std::cout << "\033[" << down << "B";
  } else if (down < 0) {
    std::cout << "\033[" << -down << "A";
  }
  if (right > 0) {
    std::cout << "\033[" << right << "C";
  } else if (right < 0) {
    std::cout << "\033[" << -right << "D";
  }
}
inline void draw_box(int width, int height) {
  std::cout << "┌";
  for (int i = 0; i < width - 2; i++) std::cout << "─";
  std::cout << "┐";
  move_cursor(1, -width);
  for (int i = 0; i < height - 2; i++) {
    std::cout << "│" << std::string(width - 2, ' ') << "│";
    move_cursor(1, -width);
  }
  std::cout << "└";
  for (int i = 0; i < width - 2; i++)
    std::cout << "─";
  std::cout << "┘";
  std::cout.flush();
  move_cursor(-width, -height);
}
inline void couplet(std::string left, std::string right, std::string middle) {
    //┌， ┬， ┐， ├， ┼， ┤， └， ┴， ┘ 
  assert(left.size() == right.size());
  string u8left(left);
  string u8right(right);
  string u8middle(middle);
  int len = u8left.size();
  int door_width = 8;
  int couplet_width = 2;
  color c(0xFFC800, 0xFF0000);
  std::cout << std::string(couplet_width, ' ') << color_it_all(u8middle, c) << std::string(couplet_width, ' ')
            << "\r\n";
  for (int i = 0; i < len; i++) {
    std::cout << color_it_all(u8left[i], c) << std::string(door_width, ' ') << color_it_all(u8right[i], c) << "\r\n";
  }
  move_cursor(-len, 2);
  draw_box(door_width/2, len);
  move_cursor(0, door_width/2+1);
  draw_box(door_width/2, len);
  move_cursor(len/2, 1);
  std::cout<<"├";
  std::cout.flush();
//   std::cout << "┌";
//   for (int i = 0; i < door_width - 2; i++) std::cout << "─";
//   std::cout << "┐";
//   move_cursor(1, -door_width);
//   for (int i = 0; i < len - 2; i++) {
//     std::cout << "│" << std::string(door_width - 2, ' ') << "│";
//     move_cursor(1, -door_width);
//   }
//   std::cout << "└";
//   for (int i = 0; i < door_width - 2; i++)
//     std::cout << "─";
//   std::cout << "┘";
//   std::cout.flush();
}
} // namespace terminal