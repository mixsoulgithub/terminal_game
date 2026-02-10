#include <iostream>

int main() {
    int width = 10;   // 宽度（以全角字符为单位）
    int height = 10;  // 高度（以全角字符为单位）
    
    // 上边框: 使用全角水平线字符 ─
    std::cout << "┌";
    for (int i = 0; i < width - 2; i++) {
        std::cout << "─";
    }
    std::cout << "┐" << std::endl;
    
    // 中间行: 使用全角空格
    for (int i = 0; i < height - 2; i++) {
        std::cout << "│";
        for (int j = 0; j < width - 2; j++) {
            std::cout << " ";  // 全角空格
        }
        std::cout << "│" << std::endl;
    }
    
    // 下边框: 使用全角水平线字符 ─
    std::cout << "└";
    for (int i = 0; i < width - 2; i++) {
        std::cout << "─";
    }
    std::cout << "┘" << std::endl;
    
    return 0;
}