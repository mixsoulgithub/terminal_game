#include "terminal.hpp"
#include <cassert>
#include <chrono>
#include <iostream>
#include <string>
#include <sys/select.h>
#include <sys/ioctl.h>
#include <unistd.h>

// 获取终端宽度
int get_terminal_width() {
  struct winsize w;
  if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &w) == -1) {
    return 80; // 默认宽度
  }
  return w.ws_col;
}

// 将 terminal::string 转换为 std::string
std::string terminal_string_to_std_string(const terminal::string& ts) {
  std::string result;
  for (const auto& ch : ts) {
    result += ch;
  }
  return result;
}

// 计算文本显示宽度（考虑中文字符占2个cell）
int calculate_text_width(const terminal::string& text) {
  int width = 0;
  for (const auto& ch : text) {
    if (ch.size() > 1) {
      width += 2; // UTF-8 多字节字符（中文、emoji）占2个cell
    } else {
      width += 1; // ASCII 字符占1个cell
    }
  }
  return width;
}

// 居中文本（接受 terminal::string）
std::string center_text(const terminal::string& text, int term_width) {
  int text_width = calculate_text_width(text);
  int padding = (term_width - text_width) / 2;
  if (padding < 0) padding = 0;
  return std::string(padding, ' ') + terminal_string_to_std_string(text);
}

// 居中文本（接受 std::string）
std::string center_text(const std::string& text, int term_width) {
  terminal::string u8text(text);
  return center_text(u8text, term_width);
}

int main() {
  struct termios raw = terminal::config_raw_mode();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);
  terminal::cursor_off();
  terminal::clear_screen();
  auto tp_utc{std::chrono::system_clock::now()};
  char c;
  bool new_year_started = false;
  int flash_state = 0; // 用于交替闪烁
  int last_year = -1;
  
  // 新年快乐的配色方案（温暖喜庆的配色）
  std::vector<terminal::color> new_year_colors = {
    terminal::color(0xFF6B6B, 0xFFE66D), // 红-黄
    terminal::color(0x4ECDC4, 0xFFE66D), // 青-黄
    terminal::color(0xFF6B6B, 0x95E1D3), // 红-青
    terminal::color(0xFFA07A, 0xFFE66D), // 橙-黄
    terminal::color(0xFF6B6B, 0xFFA07A), // 红-橙
  };
  
  // 闪烁时的配色（较暗）
  std::vector<terminal::color> flash_colors = {
    terminal::color(0xCC5555, 0xCCB855), // 暗红-暗黄
    terminal::color(0x3E9E96, 0xCCB855), // 暗青-暗黄
    terminal::color(0xCC5555, 0x77B4A6), // 暗红-暗青
    terminal::color(0xCC8062, 0xCCB855), // 暗橙-暗黄
    terminal::color(0xCC5555, 0xCC8062), // 暗红-暗橙
  };
  
  while (true) {
    tp_utc = std::chrono::system_clock::now();
    auto now_time = std::chrono::current_zone()->to_local(tp_utc);
    auto now_time_s = std::chrono::time_point_cast<std::chrono::seconds>(std::chrono::current_zone()->to_local(tp_utc));
    
    // 获取当前年份
    auto year_str = std::format("{:%Y}", now_time_s);
    int current_year = std::stoi(year_str);
    
    // 检测新年时刻（1月1日00:00:00或年份变化）
    auto month_day = std::format("{:%m-%d}", now_time_s);
    auto hour_minute_second = std::format("{:%H:%M:%S}", now_time_s);
    
    // 检测是否到了新年（1月1日00:00:00）
    if (!new_year_started && month_day == "01-01" && hour_minute_second == "00:00:00") {
      new_year_started = true;
    }
    
    // 如果年份变化了，也触发新年显示（用于测试）
    if (last_year != -1 && current_year != last_year) {
      new_year_started = true;
    }
    last_year = current_year;
    
    // 显示时间
    auto year = std::format("  {:%Y} ", now_time_s);
    auto month_day_display = std::format("{:%m-%d} ", now_time_s);
    auto hour_minute_second_display = std::format("{:%H:%M:%S}   ", now_time_s);
    terminal::color year_color(0x2C3E50, 0xE8ECE3);
    terminal::color month_day_color(0x7E8C69, 0xE8ECE3);
    terminal::color hour_minute_second_color(0xD4A15E, 0xE8ECE3);
    
    int term_width = get_terminal_width();
    terminal::string time_year(year);
    terminal::string time_month_day(month_day_display);
    terminal::string time_hms(hour_minute_second_display);
    terminal::string time_line = terminal::color_it_all(time_year, year_color) + 
                                 terminal::color_it_all(time_month_day, month_day_color) +
                                 terminal::color_it_all(time_hms, hour_minute_second_color);
    
    std::cout << "\033[H"; // 移动到左上角
    std::cout << time_line;
    
    // 如果新年已开始，显示5行"新年快乐"并交替闪烁
    if (true) {
      std::vector<std::string> new_year_lines = {
        "      🎉 新年快乐 🎉    ",
        "      ✨ 新年快乐 ✨    ",
        "      🎊 新年快乐 🎊    ",
        "      🎈 新年快乐  🎈   ",
        "      🎁 新年快乐 🎁    "
      };
      
      // 每0.5秒切换一次闪烁状态，实现交替闪烁效果
      // 奇数行和偶数行交替显示不同亮度
      bool flash_phase = (flash_state / 2) % 2 == 0;
      
      for (size_t i = 0; i < new_year_lines.size(); i++) {
        std::cout << "\r\n"; // 换行
        terminal::string line_text(new_year_lines[i]);
        // 奇数行和偶数行交替闪烁
        bool should_flash = (i % 2 == 0) ? flash_phase : !flash_phase;
        auto& line_color = should_flash ? new_year_colors[i] : flash_colors[i];
        std::vector<terminal::color> line_colors(line_text.size(), line_color);
        terminal::string colored_line = terminal::color_it_all(line_text, line_colors);
        std::cout << center_text(colored_line, term_width);
      }
      
      flash_state++;
    }
    
    std::cout.flush();
    
    // 非阻塞输入检测
    fd_set readfds;
    FD_ZERO(&readfds);
    FD_SET(STDIN_FILENO, &readfds);
    struct timeval timeout;
    timeout.tv_sec = 0;
    timeout.tv_usec = 500000; // 0.5秒超时，用于闪烁
    
    if (select(STDIN_FILENO + 1, &readfds, nullptr, nullptr, &timeout) > 0) {
      if (read(STDIN_FILENO, &c, 1) == 1) {
        if (c == 'q')
          break;
      }
    }
  }
  
  std::cout.flush();
  terminal::clear_screen();
  terminal::cursor_on();
  tcsetattr(STDIN_FILENO, TCSAFLUSH, &terminal::original_termios);
  return 0;
}