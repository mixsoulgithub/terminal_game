// palette.rs:颜色类型和几张色卡.
// 色卡是纯数据:给定可用的行列数,产出一张 rows x cols 的格子表,空位是 None.

// 一块色卡的尺寸(单元格).终端一格的宽高比约 1:2,所以 2x1 看起来是方的.
pub const SWATCH_W: usize = 2;
pub const SWATCH_H: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    // 复制给用户的就是这个串
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    // 铺在这块底色上的字该用黑还是白
    pub fn ink(&self) -> Rgb {
        let y = (299 * self.r as u32 + 587 * self.g as u32 + 114 * self.b as u32) / 1000;
        if y > 140 {
            Rgb::new(0, 0, 0)
        } else {
            Rgb::new(255, 255, 255)
        }
    }

    // (h 0..360, s 0..100, l 0..100)
    pub fn hsl(&self) -> (u16, u8, u8) {
        let (r, g, b) = (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
        );
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let l = (max + min) / 2.0;
        if max == min {
            return (0, 0, (l * 100.0).round() as u8);
        }
        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (
            (h * 60.0).round() as u16 % 360,
            (s * 100.0).round() as u8,
            (l * 100.0).round() as u8,
        )
    }

    // 直接塞进 SGR 的参数部分:38;2;R;G;B
    pub fn sgr(&self) -> String {
        format!("38;2;{};{};{}", self.r, self.g, self.b)
    }

    // xterm 256 调色板里最接近的编号
    pub fn ansi256(&self) -> u8 {
        let (r, g, b) = (self.r as i32, self.g as i32, self.b as i32);
        let d2 = |c: Rgb| {
            let (dr, dg, db) = (c.r as i32 - r, c.g as i32 - g, c.b as i32 - b);
            dr * dr + dg * dg + db * db
        };
        // 灰色不走色立方,和黑/白/24 级灰阶比距离,取最近的那个
        if r == g && g == b {
            let k = ((r - 8) as f64 / 10.0).round().clamp(0.0, 23.0) as i32;
            let gray = (8 + 10 * k) as u8;
            let mut best = (16u8, d2(Rgb::new(0, 0, 0)));
            for (i, c) in [
                (231u8, Rgb::new(255, 255, 255)),
                (232 + k as u8, Rgb::new(gray, gray, gray)),
            ] {
                let d = d2(c);
                if d < best.1 {
                    best = (i, d);
                }
            }
            return best.0;
        }
        let q = |v: i32| {
            if v < 48 {
                0
            } else if v < 115 {
                1
            } else {
                ((v - 35) / 40).min(5)
            }
        };
        (16 + 36 * q(r) + 6 * q(g) + q(b)) as u8
    }

    // 最接近的传统 16 色:(编号, 名字)
    pub fn ansi16(&self) -> (u8, &'static str) {
        let mut best = (0u8, 0i64);
        for (i, (c, _)) in ANSI16.iter().enumerate() {
            let d = (c.r as i64 - self.r as i64).pow(2)
                + (c.g as i64 - self.g as i64).pow(2)
                + (c.b as i64 - self.b as i64).pow(2);
            if i == 0 || d < best.1 {
                best = (i as u8, d);
            }
        }
        (best.0, ANSI16[best.0 as usize].1)
    }
}

// xterm 的 16 个基础色,以及它们的实际 RGB(终端默认值,各终端略有差异)
pub const ANSI16: [(Rgb, &str); 16] = [
    (Rgb::new(0, 0, 0), "black"),
    (Rgb::new(205, 0, 0), "red"),
    (Rgb::new(0, 205, 0), "green"),
    (Rgb::new(205, 205, 0), "yellow"),
    (Rgb::new(0, 0, 238), "blue"),
    (Rgb::new(205, 0, 205), "magenta"),
    (Rgb::new(0, 205, 205), "cyan"),
    (Rgb::new(229, 229, 229), "white"),
    (Rgb::new(127, 127, 127), "bright black"),
    (Rgb::new(255, 0, 0), "bright red"),
    (Rgb::new(0, 255, 0), "bright green"),
    (Rgb::new(255, 255, 0), "bright yellow"),
    (Rgb::new(92, 92, 255), "bright blue"),
    (Rgb::new(255, 0, 255), "bright magenta"),
    (Rgb::new(0, 255, 255), "bright cyan"),
    (Rgb::new(255, 255, 255), "bright white"),
];

// 6x6x6 色立方用到的三个通道档位
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

// xterm 256 调色板里第 i 号颜色是什么
pub fn ansi256_rgb(i: u8) -> Rgb {
    match i {
        0..=15 => ANSI16[i as usize].0,
        16..=231 => {
            let n = (i - 16) as usize;
            Rgb::new(CUBE[n / 36], CUBE[(n % 36) / 6], CUBE[n % 6])
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Rgb::new(v, v, v)
        }
    }
}

// hsv -> rgb,h 任意角度,s/v 都是 0..1
fn hsv(h: f64, s: f64, v: f64) -> Rgb {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |f: f64| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb::new(to(r), to(g), to(b))
}

// 一张铺满的色卡:rows x cols,空位是 None
#[derive(Clone, Debug)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Option<Rgb>>,
}

impl Grid {
    fn empty(rows: usize, cols: usize) -> Grid {
        Grid {
            rows,
            cols,
            cells: vec![None; rows * cols],
        }
    }

    fn put(&mut self, r: usize, c: usize, color: Rgb) {
        if r < self.rows && c < self.cols {
            self.cells[r * self.cols + c] = Some(color);
        }
    }

    pub fn at(&self, idx: usize) -> Option<Rgb> {
        self.cells.get(idx).copied().flatten()
    }

    // 把线性下标摆成 (行, 列)
    pub fn point(&self, idx: usize) -> (usize, usize) {
        let cols = self.cols.max(1);
        (idx / cols, idx % cols)
    }

    pub fn index(&self, r: usize, c: usize) -> usize {
        r * self.cols + c
    }

    // 光标落在空位上(换调色板、终端变小)时找最近的实心格子
    pub fn nearest_used(&self, idx: usize) -> usize {
        if self.at(idx).is_some() {
            return idx;
        }
        let n = self.cells.len();
        for d in 1..=n {
            if idx + d < n && self.at(idx + d).is_some() {
                return idx + d;
            }
            if idx >= d && self.at(idx - d).is_some() {
                return idx - d;
            }
        }
        idx
    }

    // 朝一个方向走一格;走到空位或出界就继续走,直到碰到实心格子或撞墙
    pub fn step(&self, idx: usize, dr: i32, dc: i32) -> usize {
        let (mut r, mut c) = self.point(idx);
        loop {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr < 0 || nc < 0 || nr >= self.rows as i32 || nc >= self.cols as i32 {
                return idx;
            }
            (r, c) = (nr as usize, nc as usize);
            let i = self.index(r, c);
            if self.at(i).is_some() {
                return i;
            }
        }
    }

    // 同一行里最左/最右的实心格子
    pub fn row_edge(&self, idx: usize, from_right: bool) -> usize {
        let (r, _) = self.point(idx);
        let cols = self.cols;
        for k in 0..cols {
            let c = if from_right { cols - 1 - k } else { k };
            let i = self.index(r, c);
            if self.at(i).is_some() {
                return i;
            }
        }
        idx
    }

    // 同一列里最上/最下的实心格子
    pub fn col_edge(&self, idx: usize, from_bottom: bool) -> usize {
        let (_, c) = self.point(idx);
        let rows = self.rows;
        for k in 0..rows {
            let r = if from_bottom { rows - 1 - k } else { k };
            let i = self.index(r, c);
            if self.at(i).is_some() {
                return i;
            }
        }
        idx
    }
}

// 五种色卡.前两种随终端大小铺满,后面三种是固定尺寸居中摆.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    HueValue,
    HueSat,
    Ansi16,
    Ansi256,
    Gray,
}

pub const ALL: [Kind; 5] = [
    Kind::HueValue,
    Kind::HueSat,
    Kind::Ansi16,
    Kind::Ansi256,
    Kind::Gray,
];

impl Kind {
    // 界面上显示的名字
    pub fn name(self) -> &'static str {
        match self {
            Kind::HueValue => "hue/value",
            Kind::HueSat => "hue/sat",
            Kind::Ansi16 => "ansi16",
            Kind::Ansi256 => "ansi256",
            Kind::Gray => "gray",
        }
    }

    // 命令行里用的短名
    pub fn slug(self) -> &'static str {
        match self {
            Kind::HueValue => "hue",
            Kind::HueSat => "sat",
            Kind::Ansi16 => "ansi16",
            Kind::Ansi256 => "ansi256",
            Kind::Gray => "gray",
        }
    }

    pub fn from_slug(s: &str) -> Option<Kind> {
        let s = s.to_ascii_lowercase();
        ALL.iter()
            .copied()
            .find(|k| k.slug() == s || k.name() == s)
            .or_else(|| {
                s.parse::<usize>()
                    .ok()
                    .filter(|n| (1..=ALL.len()).contains(n))
                    .map(|n| ALL[n - 1])
            })
    }

    pub fn build(self, rows: usize, cols: usize) -> Grid {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let mut g = Grid::empty(rows, cols);
        match self {
            // 横轴色相,纵轴明度:顶行最亮,底行压暗
            Kind::HueValue => {
                let d = (rows - 1).max(1) as f64;
                for r in 0..rows {
                    let v = 1.0 - 0.72 * (r as f64 / d);
                    for c in 0..cols {
                        let h = 360.0 * c as f64 / cols as f64;
                        g.put(r, c, hsv(h, 1.0, v));
                    }
                }
            }
            // 横轴色相,纵轴饱和度:底行接近白
            Kind::HueSat => {
                let d = (rows - 1).max(1) as f64;
                for r in 0..rows {
                    let s = 1.0 - 0.8 * (r as f64 / d);
                    for c in 0..cols {
                        let h = 360.0 * c as f64 / cols as f64;
                        g.put(r, c, hsv(h, s, 1.0));
                    }
                }
            }
            Kind::Ansi16 => {
                place(&mut g, 8, 2, |r, c| ANSI16[r * 8 + c].0);
            }
            Kind::Ansi256 => {
                place(&mut g, 16, 16, |r, c| ansi256_rgb((r * 16 + c) as u8));
            }
            // 32 级灰阶,一行摆开
            Kind::Gray => {
                let n = 32usize.min(cols);
                let r = rows / 2;
                let x0 = (cols - n) / 2;
                for i in 0..n {
                    let v = (i * 255 / (n - 1).max(1)) as u8;
                    g.put(r, x0 + i, Rgb::new(v, v, v));
                }
            }
        }
        g
    }
}

// 把一张 pw x ph 的固定色卡居中放进当前网格
fn place(g: &mut Grid, pw: usize, ph: usize, f: impl Fn(usize, usize) -> Rgb) {
    let r0 = g.rows.saturating_sub(ph) / 2;
    let c0 = g.cols.saturating_sub(pw) / 2;
    for r in 0..ph {
        for c in 0..pw {
            g.put(r0 + r, c0 + c, f(r, c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_upper_case_hash_rrggbb() {
        assert_eq!(Rgb::new(255, 136, 0).hex(), "#FF8800");
        assert_eq!(Rgb::new(0, 0, 0).hex(), "#000000");
    }

    #[test]
    fn ink_flips_on_brightness() {
        assert_eq!(Rgb::new(255, 255, 255).ink(), Rgb::new(0, 0, 0));
        assert_eq!(Rgb::new(0, 0, 0).ink(), Rgb::new(255, 255, 255));
        assert_eq!(Rgb::new(0, 0, 255).ink(), Rgb::new(255, 255, 255));
    }

    #[test]
    fn hsl_matches_known_colors() {
        assert_eq!(Rgb::new(255, 0, 0).hsl(), (0, 100, 50));
        assert_eq!(Rgb::new(0, 255, 0).hsl(), (120, 100, 50));
        assert_eq!(Rgb::new(0, 0, 255).hsl(), (240, 100, 50));
        assert_eq!(Rgb::new(255, 255, 255).hsl(), (0, 0, 100));
        assert_eq!(Rgb::new(128, 128, 128).hsl(), (0, 0, 50));
    }

    #[test]
    fn ansi256_hits_the_standard_entries() {
        assert_eq!(Rgb::new(0, 0, 0).ansi256(), 16);
        assert_eq!(Rgb::new(255, 0, 0).ansi256(), 196);
        assert_eq!(Rgb::new(255, 255, 255).ansi256(), 231);
        assert_eq!(Rgb::new(128, 128, 128).ansi256(), 244);
        // 色立方里的非灰档位映射回自己(灰档位离 24 级灰阶更近,理应被抢走)
        for i in 16..=231u8 {
            let c = ansi256_rgb(i);
            if c.r == c.g && c.g == c.b {
                continue;
            }
            assert_eq!(c.ansi256(), i, "index {i} is not stable");
        }
        for i in 232..=255u8 {
            assert_eq!(ansi256_rgb(i).ansi256(), i, "gray {i} is not stable");
        }
    }

    #[test]
    fn ansi16_picks_the_closest_name() {
        assert_eq!(Rgb::new(255, 0, 0).ansi16(), (9, "bright red"));
        assert_eq!(Rgb::new(0, 0, 0).ansi16(), (0, "black"));
        assert_eq!(Rgb::new(250, 250, 250).ansi16(), (15, "bright white"));
    }

    #[test]
    fn ansi256_grid_is_the_standard_table() {
        let g = Kind::Ansi256.build(16, 16);
        assert_eq!(g.rows, 16);
        assert_eq!(g.cols, 16);
        assert_eq!(g.at(0), Some(Rgb::new(0, 0, 0)));
        assert_eq!(g.at(15), Some(ANSI16[15].0));
        assert_eq!(g.at(231), Some(Rgb::new(255, 255, 255)));
        assert_eq!(g.at(232), Some(Rgb::new(8, 8, 8)));
        assert_eq!(g.at(255), Some(Rgb::new(238, 238, 238)));
    }

    #[test]
    fn fixed_palettes_are_centred_and_padded_with_none() {
        let g = Kind::Ansi16.build(10, 40);
        assert_eq!(g.rows, 10);
        assert_eq!(g.cols, 40);
        assert_eq!(g.at(0), None);
        // 8x2 色卡:行从 (10-2)/2 = 4 开始,列从 (40-8)/2 = 16 开始
        assert_eq!(g.at(4 * 40 + 16), Some(ANSI16[0].0));
        assert_eq!(g.at(5 * 40 + 23), Some(ANSI16[15].0));
        assert_eq!(g.at(4 * 40 + 15), None);
    }

    #[test]
    fn grid_stays_inside_a_small_terminal() {
        let g = Kind::Ansi256.build(4, 10);
        assert_eq!(g.rows, 4);
        assert_eq!(g.cols, 10);
        assert_eq!(g.cells.len(), 40);
        // 放不下就贴着左上角裁掉
        assert_eq!(g.at(0), Some(Rgb::new(0, 0, 0)));
    }

    #[test]
    fn filling_palettes_use_every_cell() {
        for k in [Kind::HueValue, Kind::HueSat] {
            let g = k.build(7, 23);
            assert_eq!(g.cells.len(), 7 * 23);
            assert!(g.cells.iter().all(|c| c.is_some()), "{k:?} left a hole");
        }
        // 第一列就是纯红,颜色随行列变化
        let g = Kind::HueValue.build(8, 36);
        assert_eq!(g.at(0), Some(Rgb::new(255, 0, 0)));
        assert_ne!(g.at(0), g.at(1));
        assert_ne!(g.at(0), g.at(36));
    }

    #[test]
    fn step_skips_padding_and_clamps_at_the_wall() {
        let g = Kind::Ansi16.build(10, 40);
        let first = g.index(4, 16); // 8x2 色卡居中后的左上角
        assert_eq!(g.at(first), Some(ANSI16[0].0));
        assert_eq!(g.step(first, 0, -1), first); // 左边是空白,撞不出去
        assert_eq!(g.step(first, -1, 0), first); // 上面也是空白
        let right = g.step(first, 0, 1);
        assert_eq!(g.at(right), Some(ANSI16[1].0));
        let down = g.step(first, 1, 0);
        assert_eq!(g.at(down), Some(ANSI16[8].0));
    }

    #[test]
    fn row_edge_seeks_the_used_cells_of_the_row() {
        let g = Kind::Ansi16.build(10, 40);
        let mid = g.index(4, 19);
        assert_eq!(g.at(g.row_edge(mid, false)), Some(ANSI16[0].0));
        assert_eq!(g.at(g.row_edge(mid, true)), Some(ANSI16[7].0));
    }

    #[test]
    fn col_edge_seeks_the_used_cells_of_the_column() {
        let g = Kind::Ansi16.build(10, 40);
        let mid = g.index(4, 19);
        // 8x2 色卡占第 4、5 行:列不变,只换行
        assert_eq!(g.at(g.col_edge(mid, false)), Some(ANSI16[3].0));
        assert_eq!(g.at(g.col_edge(mid, true)), Some(ANSI16[11].0));
        assert_eq!(g.point(g.col_edge(mid, true)).1, 19);
        // ansi256 占 16 行,跨过更多行
        let big = Kind::Ansi256.build(25, 50);
        let start = big.index(4, 17);
        assert_eq!(big.at(big.col_edge(start, false)), big.at(start));
        let bottom = big.col_edge(start, true);
        assert_eq!(big.point(bottom), (19, 17));
        // 只有一行的 gray:本列上下都是自己
        let gray = Kind::Gray.build(25, 50);
        let only = gray.index(12, 20);
        assert_eq!(gray.col_edge(only, false), only);
        assert_eq!(gray.col_edge(only, true), only);
    }

    #[test]
    fn nearest_used_snaps_off_padding() {
        let g = Kind::Ansi16.build(10, 40);
        let first = g.index(4, 16);
        assert_eq!(g.nearest_used(0), first);
        assert_eq!(g.nearest_used(first), first);
    }

    #[test]
    fn slugs_and_numbers_name_the_same_palette() {
        assert_eq!(Kind::from_slug("gray"), Some(Kind::Gray));
        assert_eq!(Kind::from_slug("GRAY"), Some(Kind::Gray));
        assert_eq!(Kind::from_slug("5"), Some(Kind::Gray));
        assert_eq!(Kind::from_slug("hue"), Some(Kind::HueValue));
        assert_eq!(Kind::from_slug("nope"), None);
        assert_eq!(ALL[2].slug(), "ansi16");
    }
}
