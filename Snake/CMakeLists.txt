cmake_minimum_required(VERSION 3.10)
project(Snake)
set(CMAKE_CXX_STANDARD 17)
find_package(Curses REQUIRED)
# target_link_libraries(Tetris ncurses)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

function(compile_it target_name srcs)
    add_executable(${target_name} ${srcs})
    target_include_directories(${target_name} PRIVATE ${CURSES_INCLUDE_DIRS})
    target_link_libraries(${target_name} PRIVATE ${CURSES_LIBRARIES})
endfunction()

file(GLOB_RECURSE SOURCES "*.cpp")
file(GLOB_RECURSE HEADERS "*.h")

compile_it(Snake ${SOURCES} ${HEADERS})

target_compile_options(Snake PUBLIC -g)

