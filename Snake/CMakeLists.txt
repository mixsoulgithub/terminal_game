cmake_minimum_required(VERSION 3.10)
project(Snake)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# 查找 ncurses 库
find_package(Curses REQUIRED)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# 设置源文件路径
set(SRC_DIR src)
set(OBJECTS_DIR ${SRC_DIR}/objects)

# 收集源文件（不包括头文件）
file(GLOB_RECURSE SOURCES 
    "${SRC_DIR}/*.cpp"
    "${OBJECTS_DIR}/*.cpp"
)

# 收集头文件路径用于显示（不用于编译）
file(GLOB_RECURSE HEADERS 
    "${SRC_DIR}/*.hpp"
    "${SRC_DIR}/*.h"
    "${OBJECTS_DIR}/*.hpp" 
    "${OBJECTS_DIR}/*.h"
)

message(STATUS "Sources: ${SOURCES}")
message(STATUS "Headers: ${HEADERS}")

# 创建可执行文件（只包含源文件）
add_executable(Snake ${SOURCES})

# 设置包含目录
target_include_directories(Snake PRIVATE 
    ${SRC_DIR}
    ${OBJECTS_DIR}
    ${CURSES_INCLUDE_DIRS}
)

# 链接库
target_link_libraries(Snake PRIVATE ${CURSES_LIBRARIES})

# 编译选项
target_compile_options(Snake PUBLIC -g)

# cmake_minimum_required(VERSION 3.10)
# project(Snake)
# set(CMAKE_CXX_STANDARD 17)
# find_package(Curses REQUIRED)
# # target_link_libraries(Tetris ncurses)
# set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# function(compile_it target_name srcs)
#     add_executable(${target_name} ${srcs})
#     target_include_directories(${target_name} PRIVATE ${CURSES_INCLUDE_DIRS})
#     target_link_libraries(${target_name} PRIVATE ${CURSES_LIBRARIES})
# endfunction()

# file(GLOB_RECURSE SOURCES "*.cpp")
# file(GLOB_RECURSE HEADERS "*.h")

# message(STATUS "Sources: ${SOURCES}")
# compile_it(Snake ${SOURCES} ${HEADERS})

# target_compile_options(Snake PUBLIC -g)

