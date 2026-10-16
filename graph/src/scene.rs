//! 画布坐标:同一张图可以有几套坐标(输入自带的真实坐标,或者布局算出来的),
//! 渲染与交互都只看这一层,不看它是怎么来的.

/// 布局的标称画布(格).留一点余量:200x50 的窗口下整幅图是"放大"铺满的,
/// 而不是被压着塞进去 —— 压着塞会四舍五入把两个节点并到相邻格
pub const NOMINAL: (f64, f64) = (184.0, 45.0);

/// 画布规格:按节点数开方放大,让"每个节点分到多少格子"和小图基准差不多.
/// 节点多了画布不放大,后处理在密集区就找不到空位,节点会叠在一起
pub fn nominal_for(n: usize) -> (f64, f64) {
    let k = (n as f64 / 262.0).sqrt().max(1.0);
    (NOMINAL.0 * k, NOMINAL.1 * k)
}

/// 一套"画布坐标"
#[derive(Clone, Debug)]
pub struct Scene {
    pub pos: Vec<(f64, f64)>,
    pub bbox: (f64, f64, f64, f64),
    /// 一行的物理高度相当于几列:真实地理按字符格 2:1 压扁,布局算出来的坐标是 1:1
    pub aspect: f64,
    /// y 是向上的(输入坐标)还是向下的(布局算出来的已经是屏幕方向)
    pub flip_y: bool,
}

impl Scene {
    pub fn from_pos(pos: Vec<(f64, f64)>, aspect: f64, flip_y: bool) -> Scene {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &pos {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        if pos.is_empty() {
            (x0, y0, x1, y1) = (0.0, 0.0, 0.0, 0.0);
        }
        Scene {
            pos,
            bbox: (x0, y0, x1, y1),
            aspect,
            flip_y,
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (
            (self.bbox.0 + self.bbox.2) / 2.0,
            (self.bbox.1 + self.bbox.3) / 2.0,
        )
    }

    pub fn size(&self) -> (f64, f64) {
        (
            (self.bbox.2 - self.bbox.0).max(1.0),
            (self.bbox.3 - self.bbox.1).max(1.0),
        )
    }
}
