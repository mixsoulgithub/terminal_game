#pragma once

enum class ColorMode : int
{
    FRONT_WHITE_BACK_BLACK,
    FRONT_YELLOW_BACK_WHITE,
    COLOR_MODE_COUNT
};

class ColorSystem
{
public:
    ColorSystem();
    ~ColorSystem();

    void initialize();
    void set_color_mode(ColorMode color_mode);
    void unset_color_mode(ColorMode color_mode);
};