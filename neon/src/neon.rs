// 该文件实现霓虹配色:HSL 色相环、流动相位、正文上的高斯光带.
// 所有颜色都是 24 位真彩,由 crossterm 直接输出 38;2;r;g;b,不走终端调色板.
use ratatui::style::Color;

// 正文底色
pub const BG: Color = Color::Rgb(8, 9, 16);
// 正文基色(未被光带扫到时的颜色)
pub const TEXT: (u8, u8, u8) = (208, 214, 228);
// 底栏文字色,靠深色压在流动底色上
pub const BAR_TEXT: (u8, u8, u8) = (6, 8, 14);
// 当前匹配的高亮
pub const HL_CUR_BG: Color = Color::Rgb(255, 240, 130);
pub const HL_CUR_FG: Color = Color::Rgb(12, 12, 18);
// 其余匹配的高亮
pub const HL_BG: Color = Color::Rgb(58, 40, 96);
pub const HL_FG: Color = Color::Rgb(232, 224, 255);

// 色相 h 以 1.0 为一圈,s/l 取 0..1;返回 24 位 RGB
pub fn hsl(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let q = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (q(r1), q(g1), q(b1))
}

pub fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color::Rgb(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

// 环形高斯:距中心 phase 越近越亮,用于正文上那条流动的光带
pub fn wave(pos: f32, phase: f32, sigma: f32) -> f32 {
    let d = ((pos - phase).rem_euclid(1.0)).min((phase - pos).rem_euclid(1.0));
    (-(d * d) / (2.0 * sigma * sigma)).exp()
}

// 色相台阶数:每 1/STEPS 圈才算一次变化.
// 这是性能的关键:颜色只在跨台阶时才变,于是绝大多数格子在帧间完全相同,
// ratatui 的 Buffer diff 就不会为它们写任何字节.
const HUE_STEPS: f32 = 128.0;
// 光带强度的台阶数
const TINT_STEPS: f32 = 16.0;

fn step(v: f32, n: f32) -> f32 {
    (v * n).floor() / n
}

// 流动状态:phase 让色相整体旋转,sweep 让光带横穿屏幕
pub struct Flow {
    pub phase: f32,
    pub sweep: f32,
    pub enabled: bool,
    speed: f32,
}

impl Flow {
    pub fn new(speed: f32) -> Flow {
        Flow {
            phase: 0.0,
            sweep: 0.0,
            enabled: true,
            speed: speed.max(0.01),
        }
    }

    // dt 为秒;长时间挂起后不做大跳
    pub fn tick(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        let dt = dt.clamp(0.0, 0.25) * self.speed;
        self.phase = (self.phase + dt * 0.05).rem_euclid(1.0);
        self.sweep = (self.sweep + dt * 0.10).rem_euclid(1.0);
    }

    // 在 0.05..=8.0 之间缩放流速
    pub fn speed_up(&mut self, factor: f32) {
        self.speed = (self.speed * factor).clamp(0.05, 8.0);
    }

    // 色相环上的一个点
    pub fn hue(&self, t: f32) -> Color {
        let c = hsl(step(t + self.phase, HUE_STEPS), 1.0, 0.55);
        Color::Rgb(c.0, c.1, c.2)
    }

    // 底栏的底色:沿宽度铺半圈色相
    pub fn bar_bg(&self, x: u16, w: u16) -> Color {
        self.hue(0.5 * x as f32 / w.max(1) as f32)
    }

    // 正文颜色:x/y 是视口内的坐标,所以滚动时屏幕上的颜色图案不动,只有内容从下面滑过
    pub fn body_fg(&self, x: u16, y: u16, w: u16, h: u16, rainbow: bool) -> Color {
        let wf = w.max(1) as f32;
        let hf = h.max(1) as f32;
        let pos = (x as f32 + y as f32 * 2.0) / (wf + hf * 2.0);
        if rainbow {
            return self.hue(pos);
        }
        let g = step(wave(pos, self.sweep, 0.09), TINT_STEPS);
        let c = hsl(step(pos + self.phase, HUE_STEPS), 1.0, 0.62);
        mix(TEXT, c, g * 0.85)
    }

    // 行号列:暗霓虹,沿竖直方向流动
    pub fn gutter_fg(&self, y: u16, h: u16) -> Color {
        let t = y as f32 / h.max(1) as f32;
        let c = hsl(step(t * 0.5 + self.phase, HUE_STEPS), 1.0, 0.6);
        mix((88, 96, 128), c, 0.85)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_covers_the_ring() {
        assert_eq!(hsl(0.0, 1.0, 0.5), (255, 0, 0));
        assert_eq!(hsl(1.0 / 3.0, 1.0, 0.5), (0, 255, 0));
        assert_eq!(hsl(2.0 / 3.0, 1.0, 0.5), (0, 0, 255));
        // 负值与超过一圈都应回到同一色相
        assert_eq!(hsl(-1.0, 1.0, 0.5), hsl(0.0, 1.0, 0.5));
        assert_eq!(hsl(2.0, 1.0, 0.5), hsl(1.0, 1.0, 0.5));
    }

    #[test]
    fn wave_peaks_at_phase() {
        assert!((wave(0.25, 0.25, 0.09) - 1.0).abs() < 1e-6);
        assert!(wave(0.25, 0.25, 0.09) > wave(0.35, 0.25, 0.09));
        // 环形:0.99 与 0.01 都是"离 0.0 很近"
        assert!(wave(0.01, 0.0, 0.09) > wave(0.5, 0.0, 0.09));
    }

    #[test]
    fn tick_only_when_enabled() {
        let mut f = Flow::new(1.0);
        f.tick(1.0);
        assert!(f.phase > 0.0 && f.sweep > 0.0);
        let (p, s) = (f.phase, f.sweep);
        f.enabled = false;
        f.tick(1.0);
        assert_eq!((f.phase, f.sweep), (p, s));
    }
}
