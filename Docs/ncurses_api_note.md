记录 ncurses 库中一些 API 的用法
- mvaddstr
> mvaddstr(y,x,str)
功能：用于在指定位置输出字符串
参数说明：
- y: 行坐标（从0开始）
- x: 列坐标（从0开始）
- str: 要输出的字符串



- start_color
> start_color()
功能: 启用颜色功能, 如果没有, 后面的颜色相关函数既不生效, 也不报错


