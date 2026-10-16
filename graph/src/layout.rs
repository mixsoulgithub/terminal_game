//! 布局:把图变成画布坐标(单位是字符格,x 向右、y 向下).
//!
//! 四套算法,对输入的要求不一样:
//!   geo      直接用输入自带的坐标(要求每个节点都有 pos)
//!   relax    从输入坐标出发做弹簧放松,把节点距拉匀、走向贴到四种笔画上(要求有 pos)
//!   force    力导布局,没有坐标也能用
//!   layered  分层布局(Sugiyama 那一套),适合有向的依赖图/流程图
//!
//! geo 直接按输入坐标画(等比铺开);relax 落在 scene::nominal_for 给的标称画布上
//! (和终端画面一样宽扁,节点多了画布跟着放大);force 用一张按节点数开的紧凑方画布
//! (见 FORCE_SPAN),layered 按层号铺开.力导与分层都不跟标称画布对齐 ——
//! 它们要的是紧凑的正确形状,铺满屏幕是渲染那边的自适应缩放干的活.
//!
//! 规模与复杂度(节点 n、边 m、规模系数 k = sqrt(n / 262)):
//!   relax   每轮 O(n + m + repel 的网格邻域),轮数 = min(600 k, 迭代预算),见 Cfg::iters
//!   force   同上一套轮数;每轮 O(n + m + 网格邻域里的点对)
//!   layered DFS 破环 + 拓扑分层 + 几轮重心,都是 O(n + m),收尾同样走 polish
//!   polish  取整 O(n * 环数);每轮 separate / spread_all / octilinear 走候选格的
//!           (2r+1)^2 邻域,迭代里没人动就提前退出
//! 同心环数、搜索半径、迭代轮数与后处理轮数全都有上限(见 Cfg 与下面几个常量),
//! 而且每个节点收尾时只在原位附近活动,所以节点翻十几倍也只是慢一点,不会卡死.
//!
//! 确定性:哈希表只用"按 key 查",从不按哈希顺序遍历;凡是要排序的地方都用节点索引兜底,
//! 没有坐标时的起点用黄金角撒点而不是随机数.同一张图跑几次结果完全一样.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::lang::Lang;
use crate::model::Graph;
use crate::scene::{nominal_for, Scene};

/// 相邻节点的目标间距(格)
const STEP: f64 = 2.6;
/// 任意两节点的最小间距(格)
const MIN_GAP: f64 = 2.6;
/// 放松迭代轮数基准
const ITERS: usize = 600;
/// 迭代总量的封顶:轮数 x 节点数不超过这个量级,节点多了就少迭代几轮.
/// 一轮是一遍网格 + 邻域里的点对,总量封在这里,几千个节点也就几秒
const ITER_BUDGET: usize = 240_000;
/// 力导的工作画布:方画布,边长 = max(2 格, FORCE_SPAN * sqrt(节点数)).
/// 按节点数开方是"每个节点分到差不多格子"的意思;乘得比 STEP 大一点,
/// 给弹簧留出余地(画布太紧,节点会被挤到边框上,收尾又得一个个搬开)
const FORCE_SPAN: f64 = STEP * 1.6;
/// 力导的期望间距上限(格):小图不能按画布摊到几十格远,那样中间全是空白
const FORCE_K_MAX: f64 = 4.0;
/// 力导的重力:往画布中心轻轻拽一把.
/// 只靠斥力和弹簧,节点会被推到画布边框上挤成一圈(边框上到处是挨着的节点,
/// 收尾就得把它们一个个搬开,整幅图会胀大好几倍);有了重力密度就摊得均匀
const GRAVITY: f64 = 0.06;
/// 后处理里"最多磨几轮"的轮数上限:每轮都要扫一遍边,节点多了宁可少磨几轮
const ROUND_CAP: usize = 64;
/// 掰直(octilinear)的轮数上限:这一步最贵(每条边要在 8 个方向上试格子),
/// 节点再多也不加轮数 —— 磨到后面基本在原地打转,不如把时间留给别的
const OCTI_CAP: usize = 24;
/// 再大的图也至少迭代这么多轮
const MIN_ITERS: usize = 40;
/// 拉直力:让一组上连着三节点尽量共线,画出来才有长长的直走廊
const K_STRAIGHT: f64 = 0.06;
/// 锚的力度:越小越不理会地理上的疏密,越往节点距均匀那边靠
const ANCHOR_W: f64 = 0.030;
/// 分层布局的层间距(格):>= 2 才保证上下两层的节点不贴在一起
const LAYER_GAP: f64 = 3.0;
/// 分层布局同一层里相邻两节点至少隔几格
const LANE_GAP: f64 = 3.0;
/// 重心扫描轮数
const BARY_ROUNDS: usize = 4;
/// 黄金角(弧度):没有坐标时用它撒点,不用随机数
const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

/// 规模参数:节点一多,画布、搜索半径、迭代轮数都要跟着放大 ——
/// 不然后处理在密集区找不到空位,节点会叠在一起(262 个节点的图是基准,k = 1)
struct Cfg {
    k: f64,
}

impl Cfg {
    fn of(n: usize) -> Cfg {
        Cfg {
            k: (n as f64 / 262.0).sqrt().max(1.0),
        }
    }
    /// 同心环往外找空位的上限(基准 64,k 大了也只能到 96 环,再往外找不如让节点挤一点)
    fn rings(&self) -> i32 {
        ((64.0 * self.k) as i32).min(96)
    }
    /// best_cell 的搜索半径(基准 3,也就是 7x7;封顶 5)
    fn radius(&self) -> i32 {
        ((3.0 * self.k).round() as i32).clamp(2, 5)
    }
    /// clean_cell 允许挪动的格数(基准 2;封顶 4)
    fn nudge(&self) -> i32 {
        ((2.0 * self.k).round() as i32).clamp(1, 4)
    }
    /// 后处理里"最多磨几轮"的轮数:基准 x k,封顶 ROUND_CAP.
    /// 基准值是杭州那种规模(262 个节点)调出来的,小图完全按原样走
    fn rounds(&self, base: usize) -> usize {
        ((base as f64 * self.k).round() as usize).min(ROUND_CAP)
    }
    /// 主迭代轮数:基准 x k,再按 ITER_BUDGET 封顶,但不少于 MIN_ITERS
    fn iters(&self, n: usize, base: usize) -> usize {
        let want = (base as f64 * self.k).round() as usize;
        want.min((ITER_BUDGET / n.max(1)).max(MIN_ITERS))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Geo,
    Relax,
    Force,
    Layered,
}

impl Layout {
    /// 四套算法,按状态栏/R 键循环的顺序
    pub fn all() -> [Layout; 4] {
        [Layout::Geo, Layout::Relax, Layout::Force, Layout::Layered]
    }

    /// 命令行 / ":" 里认的名字(大小写与前后空格都无所谓)
    pub fn parse(s: &str) -> Option<Layout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "geo" | "real" | "geographic" => Some(Layout::Geo),
            "relax" | "spring" | "relaxed" => Some(Layout::Relax),
            "force" | "fr" => Some(Layout::Force),
            "layered" | "dag" | "sugiyama" => Some(Layout::Layered),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Layout::Geo => "geo",
            Layout::Relax => "relax",
            Layout::Force => "force",
            Layout::Layered => "layered",
        }
    }

    /// 状态栏里的中文/英文名
    pub fn label(&self, lang: Lang) -> &'static str {
        match self {
            Layout::Geo => lang.pick("真实坐标", "geographic"),
            Layout::Relax => lang.pick("松弛", "relaxed"),
            Layout::Force => lang.pick("力导", "force"),
            Layout::Layered => lang.pick("分层", "layered"),
        }
    }

    /// r 键循环到下一套
    pub fn next(&self) -> Layout {
        match self {
            Layout::Geo => Layout::Relax,
            Layout::Relax => Layout::Force,
            Layout::Force => Layout::Layered,
            Layout::Layered => Layout::Geo,
        }
    }

    /// 这套算法要求输入自带坐标吗
    pub fn needs_pos(&self) -> bool {
        matches!(self, Layout::Geo | Layout::Relax)
    }

    /// 没指定布局时自动挑一套:有坐标就按坐标画,没坐标就按图有没有环在
    /// 分层与力导之间选
    pub fn pick(g: &Graph) -> Layout {
        if g.has_pos() {
            Layout::Geo
        } else if has_cycle(g) {
            Layout::Force
        } else {
            Layout::Layered
        }
    }
}

/// 跑一套布局,得到画布坐标
pub fn scene(g: &Graph, l: Layout) -> Result<Scene, String> {
    match l {
        Layout::Geo => {
            need_pos(g, l)?;
            Ok(Scene::from_pos(geo(g), 2.0, true))
        }
        Layout::Relax => {
            need_pos(g, l)?;
            Ok(Scene::from_pos(relax(g), 1.0, false))
        }
        Layout::Force => Ok(Scene::from_pos(force(g), 1.0, false)),
        Layout::Layered => Ok(Scene::from_pos(layered(g), 1.0, false)),
    }
}

/// geo / relax 要求每个节点都有坐标:缺了就报错,不猜
fn need_pos(g: &Graph, l: Layout) -> Result<(), String> {
    if g.has_pos() {
        Ok(())
    } else {
        Err(format!("layout {} needs a position on every node", l.name()))
    }
}

/// 全部边,不分分组:后处理要保证的是"任意一条边两端都隔开"
fn edge_pairs(g: &Graph) -> Vec<(usize, usize)> {
    g.edges.iter().map(|e| (e.a, e.b)).collect()
}

/// 真实坐标:等比铺进标称画布,画布 y 保持向上(交给 Scene::flip_y 处理).
/// 等比是关键 —— 各向异性拉伸会把地图拽变形,而真实坐标的意义就是相对方位;
/// 字符格一行有 2 列高,所以纵向可用容量要乘 aspect
fn geo(g: &Graph) -> Vec<(f64, f64)> {
    let aspect = 2.0;
    let (w, h) = nominal_for(g.nodes.len());
    let (x0, y0, x1, y1) = g.bbox.unwrap_or((0.0, 0.0, 1.0, 1.0));
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    let cols = (w - 6.0).max(1.0);
    let rows = (h - 4.0).max(1.0) * aspect;
    // 一个等比因子 + 居中:宽图铺满宽度、高图铺满高度,剩下的方向留白
    let s = (cols / gw).min(rows / gh);
    let ox = (w - gw * s) / 2.0;
    let oy = ((h - 4.0) * aspect - gh * s) / 2.0 + 2.0;
    g.nodes
        .iter()
        .map(|nd| {
            let (x, y) = nd.pos.unwrap_or((0.0, 0.0));
            ((x - x0) * s + ox, (y - y0) * s + oy)
        })
        .collect()
}

/// 从自带坐标出发:等比铺进标称画布,y 翻成屏幕方向(画布 y 向下)
fn from_pos(g: &Graph, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (x0, y0, x1, y1) = g.bbox.unwrap_or((0.0, 0.0, 1.0, 1.0));
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    g.nodes
        .iter()
        .map(|nd| {
            let (x, y) = nd.pos.unwrap_or((0.0, 0.0));
            (
                (x - x0) / gw * (w - 6.0) + 3.0,
                (1.0 - (y - y0) / gh) * (h - 4.0) + 2.0,
            )
        })
        .collect()
}

/// 力导的起点(有坐标时):等比铺进画布,y 翻成屏幕方向.
/// 各向同性很要紧 —— 硬把一张方图拉成画布的长宽比,行距会只剩几格,
/// 收尾阶段到处是贴着的点对,那就有的忙了
fn fit_pos(g: &Graph, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (x0, y0, x1, y1) = g.bbox.unwrap_or((0.0, 0.0, 1.0, 1.0));
    let gw = (x1 - x0).max(1.0);
    let gh = (y1 - y0).max(1.0);
    let band = h - 4.0;
    let s = ((w - 6.0) / gw).min(band / gh);
    let ox = (w - gw * s) / 2.0;
    let oy = (band - gh * s) / 2.0;
    g.nodes
        .iter()
        .map(|nd| {
            let (x, y) = nd.pos.unwrap_or((0.0, 0.0));
            ((x - x0) * s + ox, 2.0 + (band - ((y - y0) * s + oy)))
        })
        .collect()
}

fn bbox(p: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for q in p {
        x0 = x0.min(q.0);
        y0 = y0.min(q.1);
        x1 = x1.max(q.0);
        y1 = y1.max(q.1);
    }
    (x0, y0, x1, y1)
}

/// 边的目标方向(格坐标下的单位向量):水平、竖直或 45 度
fn snap_dir(dx: f64, dy: f64) -> (f64, f64) {
    let (ax, ay) = (dx.abs(), dy.abs());
    if ay < ax * 0.4 {
        (dx.signum(), 0.0)
    } else if ax < ay * 0.4 {
        (0.0, dy.signum())
    } else {
        // 45 度:格坐标里就是这个斜率,画出来正好一个直角拐弯
        let k = std::f64::consts::FRAC_1_SQRT_2;
        (dx.signum() * k, dy.signum() * k)
    }
}

/// 斥力:两节点贴太近就推开(用网格找候选,免得 O(n^2))
fn repel(p: &[(f64, f64)], f: &mut [(f64, f64)], k: f64) {
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / MIN_GAP).floor() as i32, (q.1 / MIN_GAP).floor() as i32);
        grid.entry(cell).or_default().push(i);
    }
    for (i, q) in p.iter().enumerate() {
        let cell = ((q.0 / MIN_GAP).floor() as i32, (q.1 / MIN_GAP).floor() as i32);
        for di in -1..=1 {
            for dj in -1..=1 {
                for &j in grid
                    .get(&(cell.0 + di, cell.1 + dj))
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    if j <= i {
                        continue;
                    }
                    let (dx, dy) = (p[j].0 - q.0, p[j].1 - q.1);
                    let len = (dx * dx + dy * dy).sqrt();
                    if len >= MIN_GAP {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 { (0.0, 1.0) } else { (dx / len, dy / len) };
                    let push = (MIN_GAP - len) * k;
                    f[i].0 -= ux * push;
                    f[i].1 -= uy * push;
                    f[j].0 += ux * push;
                    f[j].1 += uy * push;
                }
            }
        }
    }
}

/// 取整去重叠:一格一节点,占了就按同心环往外找最近的空格
fn snap_to_grid(p: &[(f64, f64)], cfg: &Cfg) -> Vec<(f64, f64)> {
    let mut taken: HashSet<(i32, i32)> = HashSet::with_capacity(p.len());
    let mut out = vec![(0.0, 0.0); p.len()];
    let mut order: Vec<usize> = (0..p.len()).collect();
    order.sort_by(|&a, &b| {
        p[a].1
            .total_cmp(&p[b].1)
            .then(p[a].0.total_cmp(&p[b].0))
            .then(a.cmp(&b))
    });
    for i in order {
        let (x0, y0) = (p[i].0.round() as i32, p[i].1.round() as i32);
        let mut spot = (x0, y0);
        'grow: for r in 0..cfg.rings() {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    if !taken.contains(&(x0 + dx, y0 + dy)) {
                        spot = (x0 + dx, y0 + dy);
                        break 'grow;
                    }
                }
            }
        }
        taken.insert(spot);
        out[i] = (spot.0 as f64, spot.1 as f64);
    }
    out
}

/// 八个方向:横竖与两条对角线
const DIRS8: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// 候选格有没有跑出这个节点的活动范围(收尾只许局部微调,见 polish 上的说明)
fn within(leash: (&[(i32, i32)], i32), who: usize, c: (i32, i32)) -> bool {
    let (origin, drift) = leash;
    let o = origin[who];
    (c.0 - o.0).abs().max((c.1 - o.1).abs()) <= drift
}

/// 把边的走向整成横、竖或 45 度:让端点挪到同一个方向上最近的空格.
/// 这样画出来才是干净的一笔,而不是一格一格爬的阶梯.
fn octilinear(p: &mut [(f64, f64)], edges: &[(usize, usize)], cfg: &Cfg, leash: (&[(i32, i32)], i32)) {
    for _ in 0..cfg.rounds(24).min(OCTI_CAP) {
        let mut taken: HashSet<(i32, i32)> =
            p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (ax, ay) = (p[a].0 as i32, p[a].1 as i32);
            let (bx, by) = (p[b].0 as i32, p[b].1 as i32);
            let (dx, dy) = (bx - ax, by - ay);
            if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
                continue;
            }
            // 先试挪 b;b 周围满了就把 a 挪到以 b 为中心的同方向格子上
            let spot = clean_cell((ax, ay), (bx, by), &taken, cfg.nudge(), leash, b)
                .map(|c| (b, c))
                .or_else(|| {
                    clean_cell((bx, by), (ax, ay), &taken, cfg.nudge(), leash, a).map(|c| (a, c))
                });
            let Some((who, c)) = spot else { continue };
            let old = (p[who].0 as i32, p[who].1 as i32);
            taken.remove(&old);
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// 以 base 为圆心、在 8 个方向上找一个空格,使它到 want 最近(用来把边掰直).
/// 超出 who 活动范围的格子先筛掉(见 best_cell 上的说明)
fn clean_cell(
    base: (i32, i32),
    want: (i32, i32),
    taken: &HashSet<(i32, i32)>,
    nudge: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let l = (want.0 - base.0).abs().max((want.1 - base.1).abs());
    let mut best: Option<(i64, (i32, i32))> = None;
    for len in [l - 1, l, l + 1, l + 2] {
        if len < 1 {
            continue;
        }
        for (ux, uy) in DIRS8 {
            let c = (base.0 + ux * len, base.1 + uy * len);
            // 只许就近挪几格:不然节点会被越推越远,整幅图炸开
            if (c.0 - want.0).abs().max((c.1 - want.1).abs()) > nudge {
                continue;
            }
            if !within(leash, who, c) {
                continue;
            }
            if !room_at(c, taken, want) {
                continue;
            }
            let d = ((c.0 - want.0) as i64).pow(2) + ((c.1 - want.1) as i64).pow(2);
            if best.map(|(s, _)| d < s).unwrap_or(true) {
                best = Some((d, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// 任意两节点都不能挨在一起(不只是边上的相邻节点):
/// 挨着画出来就是两个点糊在一块,密集的地方最容易这样
fn spread_all(p: &mut [(f64, f64)], cfg: &Cfg, leash: (&[(i32, i32)], i32)) {
    for _ in 0..cfg.rounds(40) {
        // 一格可能摞了不止一个节点(前面的收尾偶尔会撞上),所以这里用多重映射
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, q) in p.iter().enumerate() {
            grid.entry((q.0 as i32, q.1 as i32)).or_default().push(i);
        }
        let mut taken: HashSet<(i32, i32)> = grid.keys().copied().collect();
        let mut moved = false;
        for i in 0..p.len() {
            let (x, y) = (p[i].0 as i32, p[i].1 as i32);
            let mut close: Option<usize> = None;
            // 同一格里的另一个节点也算"贴在一起"
            if let Some(cell) = grid.get(&(x, y)) {
                if let Some(&j) = cell.iter().find(|&&j| j > i) {
                    close = Some(j);
                }
            }
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    if let Some(cell) = grid.get(&(x + dx, y + dy)) {
                        if let Some(&j) = cell.iter().find(|&&j| j > i) {
                            close = Some(j);
                        }
                    }
                }
            }
            let Some(j) = close else { continue };
            // 谁挪都行:先试 i,再试 j;每种先找"周围八格也空着"的位子,
            // 实在没有才退一步只要求这一格空
            let r = cfg.radius();
            let spot = best_cell(p[j], p[i], &taken, true, r, leash, i)
                .map(|c| (i, c))
                .or_else(|| best_cell(p[i], p[j], &taken, true, r, leash, j).map(|c| (j, c)))
                .or_else(|| best_cell(p[j], p[i], &taken, false, r, leash, i).map(|c| (i, c)))
                .or_else(|| best_cell(p[i], p[j], &taken, false, r, leash, j).map(|c| (j, c)));
            if let Some((who, c)) = spot {
                let old = (p[who].0 as i32, p[who].1 as i32);
                if let Some(cell) = grid.get_mut(&old) {
                    cell.retain(|&k| k != who);
                }
                grid.entry(c).or_default().push(who);
                // taken 也要跟着改:不然它还以为旧格占着、新格空着,
                // 后面就会把别的节点搬进刚占上的这一格(格子少的时候必撞)
                taken.remove(&old);
                taken.insert(c);
                p[who] = (c.0 as f64, c.1 as f64);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// 边上的相邻节点至少要隔一格:不然中间那截线会被两个节点字符吃掉,线看起来是断的
fn separate(p: &mut [(f64, f64)], edges: &[(usize, usize)], cfg: &Cfg, leash: (&[(i32, i32)], i32)) {
    for _ in 0..cfg.rounds(60) {
        let mut taken: HashSet<(i32, i32)> =
            p.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
        let mut moved = false;
        for &(a, b) in edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            if dx.abs().max(dy.abs()) >= 2.0 {
                continue;
            }
            // 先挪 b:在附近找一个离 a 更远的空格,挪不动就反过来挪 a.
            // 先找"周围也空着"的位子,实在没有才退而求其次只要求这一格空
            let r = cfg.radius();
            let spot = best_cell(p[a], p[b], &taken, true, r, leash, b)
                .map(|c| (b, c))
                .or_else(|| best_cell(p[b], p[a], &taken, true, r, leash, a).map(|c| (a, c)))
                .or_else(|| best_cell(p[a], p[b], &taken, false, r, leash, b).map(|c| (b, c)))
                .or_else(|| best_cell(p[b], p[a], &taken, false, r, leash, a).map(|c| (a, c)));
            let Some((who, c)) = spot else { continue };
            taken.remove(&(p[who].0 as i32, p[who].1 as i32));
            taken.insert(c);
            p[who] = (c.0 as f64, c.1 as f64);
            moved = true;
        }
        if !moved {
            break;
        }
    }
}

/// 候选格能不能站:自己空着,周围 8 格也不能有别的节点.
/// 只有"要挪走的那一个节点"自己现在占的那一格可以不算 —— 那个位子马上就空出来了.
/// (别的节点占的格子一律算占着,不然会把两个节点摞到同一格上)
fn room_at(c: (i32, i32), taken: &HashSet<(i32, i32)>, own: (i32, i32)) -> bool {
    let blocked = |q: (i32, i32)| taken.contains(&q) && q != own;
    if blocked(c) {
        return false;
    }
    !DIRS8.iter().any(|(ux, uy)| blocked((c.0 + ux, c.1 + uy)))
}

/// 在 keep 周围 (2r+1)^2 里挑一个空格:离 from 越远越好,同时别离 keep 太远.
/// 超出这个节点活动范围的格子先筛掉:那种格子再多也用不上,
/// 而"能不能站"的检查要点 9 次哈希,先比一下格坐标便宜得多
fn best_cell(
    from: (f64, f64),
    keep: (f64, f64),
    taken: &HashSet<(i32, i32)>,
    strict: bool,
    radius: i32,
    leash: (&[(i32, i32)], i32),
    who: usize,
) -> Option<(i32, i32)> {
    let (kx, ky) = (keep.0 as i32, keep.1 as i32);
    let own = (kx, ky);
    let mut best: Option<(f64, (i32, i32))> = None;
    // 基准是 7x7:挤成一坨的地方(枢纽扎堆)近处常常一个空位都没有,节点多了还要更大
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let c = (kx + dx, ky + dy);
            if !within(leash, who, c) {
                continue;
            }
            if strict && !room_at(c, taken, own) {
                continue;
            }
            if !strict && taken.contains(&c) {
                continue;
            }
            let d_from = ((c.0 as f64 - from.0).powi(2) + (c.1 as f64 - from.1).powi(2)).sqrt();
            let score = d_from * 3.0 - (dx * dx + dy * dy) as f64;
            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// 后处理流水线:先一格一节点地对到整数格上,再让"分开(separate) / 拉开(spread_all) /
/// 拉直(octilinear)"三件事来回磨几轮 —— 它们互相会打断对方,挑位子时都尽量选
/// "周围也空着"的格子,磨到互相让步;最后兜一次底,把边上相邻的节点再拉开一遍.
/// 力导与分层也走这一套.节点都落在整数格上,而且两两不同格.
///
/// 每个节点只许在"取整后的原位"周围 drift(3 倍搜索半径)格里活动.
/// 没有这个限制,密集处会连锁反应 —— 这一轮把节点挪开、下一轮它又跟别的邻居贴上了,
/// 几十轮下来节点一路漂出去,整幅图胀成画布的几倍大(渲染的自适应缩放再把节点压成一坨).
/// 限住之后,收尾最多把包围盒撑大 2 * drift 格;代价是极密的地方可能留一对挨着的节点.
fn polish(p: Vec<(f64, f64)>, edges: &[(usize, usize)], cfg: &Cfg) -> Vec<(f64, f64)> {
    let mut out = snap_to_grid(&p, cfg);
    let origin: Vec<(i32, i32)> = out.iter().map(|q| (q.0 as i32, q.1 as i32)).collect();
    let leash = (origin.as_slice(), cfg.radius() * 3);
    for _ in 0..6 {
        separate(&mut out, edges, cfg, leash);
        spread_all(&mut out, cfg, leash);
        octilinear(&mut out, edges, cfg, leash);
    }
    separate(&mut out, edges, cfg, leash);
    out
}

/// 每组里"夹在中间"的节点:(前一节点, 中间节点, 后一节点).
/// 按分组算度,不是按整图度数 —— 枢纽也是某组路径上的中间节点,同样要拉直
fn bends(g: &Graph) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for grp in 0..g.groups.len() {
        let mut nb: HashMap<usize, Vec<usize>> = HashMap::new();
        for e in g.edges.iter().filter(|e| e.group == grp) {
            nb.entry(e.a).or_default().push(e.b);
            nb.entry(e.b).or_default().push(e.a);
        }
        // 按节点索引升序走一遍,结果与哈希顺序无关
        let mut nodes: Vec<usize> = nb.keys().copied().collect();
        nodes.sort_unstable();
        for i in nodes {
            let v = &nb[&i];
            // 这一组里正好两个邻居才算中间节点:端点、分叉口的方向本来就不定
            if v.len() == 2 {
                out.push((v[0], i, v[1]));
            }
        }
    }
    out
}

/// 松弛:从输入坐标出发做弹簧放松,再走 polish.
///
/// 长度弹簧让每条边上相邻的节点保持 STEP,方向吸附把边的走向掰到横竖 45 度,
/// 三节点拉直力抹平一站一站的小折角,另外有一个很弱的锚把整幅图拽在原位附近 ——
/// 结果是相对方位跟输入坐标一致,但节点距疏密均匀.
fn relax(g: &Graph) -> Vec<(f64, f64)> {
    let n = g.nodes.len();
    let cfg = Cfg::of(n);
    let (w, h) = nominal_for(n);
    let mut p = from_pos(g, w, h);
    let edges = edge_pairs(g);
    let bends = bends(g);
    // 锚点跟着当前布局一起缩放:只保留"相对方位",不让输入里稀疏的地方把整幅图撑大
    let anchor_raw = p.clone();
    let raw = bbox(&anchor_raw);
    let raw_c = ((raw.0 + raw.2) / 2.0, (raw.1 + raw.3) / 2.0);
    let raw_size = ((raw.2 - raw.0).max(1.0), (raw.3 - raw.1).max(1.0));

    let mut anchor = anchor_raw.clone();
    let iters = cfg.iters(n, ITERS);
    for it in 0..iters {
        if it % 20 == 0 {
            let b = bbox(&p);
            let c = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
            let s = 0.5 * ((b.2 - b.0) / raw_size.0 + (b.3 - b.1) / raw_size.1);
            for i in 0..n {
                anchor[i] = (
                    c.0 + (anchor_raw[i].0 - raw_c.0) * s,
                    c.1 + (anchor_raw[i].1 - raw_c.1) * s,
                );
            }
        }
        let mut f = vec![(0.0f64, 0.0f64); n];
        for &(a, b) in &edges {
            let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let (ux, uy) = (dx / len, dy / len);
            // 长度弹簧:相邻节点保持在 STEP 附近
            let fl = (len - STEP) * 0.30;
            f[a].0 += ux * fl;
            f[a].1 += uy * fl;
            f[b].0 -= ux * fl;
            f[b].1 -= uy * fl;
            // 方向吸附:把边往横 / 竖 / 45 度上掰
            let (tx, ty) = snap_dir(dx, dy);
            let (px, py) = (-ty, tx);
            let c = (dx * px + dy * py) * 0.30;
            f[a].0 += px * c;
            f[a].1 += py * c;
            f[b].0 -= px * c;
            f[b].1 -= py * c;
        }
        for &(a, b, c) in &bends {
            // 中间节点往两边连线的中点靠,一站一站的小折角就会被抹平
            f[b].0 += ((p[a].0 + p[c].0) / 2.0 - p[b].0) * K_STRAIGHT;
            f[b].1 += ((p[a].1 + p[c].1) / 2.0 - p[b].1) * K_STRAIGHT;
        }
        repel(&p, &mut f, 0.7);
        for i in 0..n {
            // 锚:整幅图别跑偏
            f[i].0 += (anchor[i].0 - p[i].0) * ANCHOR_W;
            f[i].1 += (anchor[i].1 - p[i].1) * ANCHOR_W;
            p[i].0 += f[i].0.clamp(-1.2, 1.2);
            p[i].1 += f[i].1.clamp(-1.2, 1.2);
        }
    }
    polish(p, &edges, &cfg)
}

/// 没有坐标时的起点:按黄金角(太阳花)撒点 —— 面积均匀、不用随机数,
/// 同一张图每次跑出来都一样;撒成画布形状的椭圆,免得方图挤在正中间
fn spiral(n: usize, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (rx, ry) = ((w - 6.0) / 2.0, (h - 4.0) / 2.0);
    (0..n)
        .map(|i| {
            let r = ((i as f64 + 0.5) / n as f64).sqrt();
            let a = i as f64 * GOLDEN_ANGLE;
            (cx + rx * r * a.cos(), cy + ry * r * a.sin())
        })
        .collect()
}

/// 力导:没有坐标也能画.
///
/// Fruchterman-Reingold —— 斥力让节点散开、边上的弹簧把相邻节点拉到理想间距 k,
/// 每个节点再被一个很弱的重力往画布中心拽一把(只靠前两个力,整幅图会被推到
/// 画布边框上挤成一圈,收尾阶段就得把那些节点一个个搬开,图会胀大好几倍),
/// 每轮位移按线性退火的温度封顶(先生猛后细微,免得来回弹),并夹在画布框里不许飞出去.
/// 有坐标就从坐标出发(铺到画布、y 翻成屏幕方向),没有坐标就按黄金角撒一圈点.
/// 斥力只算一格网格邻域内的节点(格边长就是斥力截断半径),所以一轮是 O(n + m).
///
/// 工作画布按图自己定(见 FORCE_SPAN),不走 scene::nominal_for:标称画布是按
/// "屏幕有多大"开的,节点一少就会把节点摊到几十格远、中间大片空白.
/// 力导要的是紧凑的正确形状,放大交给渲染的自适应缩放
fn force(g: &Graph) -> Vec<(f64, f64)> {
    let n = g.nodes.len();
    let cfg = Cfg::of(n);
    let span = (FORCE_SPAN * (n as f64).sqrt()).max(STEP * 2.0);
    let (w, h) = (span, span);
    let mut p = if g.has_pos() {
        fit_pos(g, w, h)
    } else {
        spiral(n, w, h)
    };
    let edges = edge_pairs(g);
    // 期望间距:紧凑画布均摊下来每个节点分到的边长,夹在 STEP 到 FORCE_K_MAX 之间
    let k = (w * h / n as f64).sqrt().clamp(STEP, FORCE_K_MAX);
    // 超过这个距离就不算斥力了,同时也是网格的格边长(3x3 邻域正好盖住它)
    let cutoff = k * 2.5;
    let iters = cfg.iters(n, ITERS);
    let t0 = (w / 10.0).min(k * 2.0);
    let center = (w / 2.0, h / 2.0);
    let mut disp = vec![(0.0f64, 0.0f64); n];
    for it in 0..iters {
        // 线性退火:最后一轮几乎不动,只做微调
        let t = t0 * (1.0 - it as f64 / iters as f64);
        fr_forces(&p, &mut disp, &edges, k, cutoff, center, GRAVITY);
        for i in 0..n {
            let (dx, dy) = disp[i];
            let l = (dx * dx + dy * dy).sqrt();
            if l <= 1e-9 {
                continue;
            }
            let s = l.min(t) / l;
            p[i].0 = (p[i].0 + dx * s).clamp(1.0, w - 1.0);
            p[i].1 = (p[i].1 + dy * s).clamp(1.0, h - 1.0);
        }
    }
    // 力导的疏密是相对的,绝对尺度没有意义:把结果等比铺满标称画布.
    // 力导的平衡点是"边上相邻节点差不多 k 格",但整幅图占多大是自由的;
    // 不铺满的话节点会缩在中间或贴上边框,均摊下来每个节点的格子数偏少,
    // 收尾在密集处就找不到空位.缩放不改变形状,只让均摊格子数和画布对齐
    let b = bbox(&p);
    let s = ((w - 6.0) / (b.2 - b.0).max(1e-6)).min((h - 4.0) / (b.3 - b.1).max(1e-6));
    let (bcx, bcy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    for q in p.iter_mut() {
        *q = (w / 2.0 + (q.0 - bcx) * s, h / 2.0 + (q.1 - bcy) * s);
    }
    polish(p, &edges, &cfg)
}

/// 点落在哪个网格格子里(格边长 cell)
fn cell_of(p: (f64, f64), cell: f64) -> (i32, i32) {
    ((p.0 / cell).floor() as i32, (p.1 / cell).floor() as i32)
}

/// 一轮力:斥力(只算 cutoff 内的,网格找候选)+ 边上的弹簧力 + 往画布中心的重力,
/// 都累加到 disp.每一对只算一次(i < j 时算,两边同时受力),
/// 所以一轮是 O(n + m + 邻域里的对数)
fn fr_forces(
    p: &[(f64, f64)],
    disp: &mut [(f64, f64)],
    edges: &[(usize, usize)],
    k: f64,
    cutoff: f64,
    center: (f64, f64),
    gravity: f64,
) {
    for d in disp.iter_mut() {
        *d = (0.0, 0.0);
    }
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, q) in p.iter().enumerate() {
        grid.entry(cell_of(*q, cutoff)).or_default().push(i);
    }
    for i in 0..p.len() {
        let (cx, cy) = cell_of(p[i], cutoff);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(bucket) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &j in bucket {
                    if j <= i {
                        continue;
                    }
                    let (ex, ey) = (p[j].0 - p[i].0, p[j].1 - p[i].1);
                    let len = (ex * ex + ey * ey).sqrt();
                    if len >= cutoff {
                        continue;
                    }
                    let (ux, uy) = if len < 1e-6 { (0.0, 1.0) } else { (ex / len, ey / len) };
                    // 斥力 k^2 / d:越近推得越狠
                    let f = k * k / len.max(0.01);
                    disp[i].0 -= ux * f;
                    disp[i].1 -= uy * f;
                    disp[j].0 += ux * f;
                    disp[j].1 += uy * f;
                }
            }
        }
    }
    for &(a, b) in edges {
        let (dx, dy) = (p[b].0 - p[a].0, p[b].1 - p[a].1);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / len, dy / len);
        // 弹簧力 d^2 / k:离得越远拉得越狠
        let f = len * len / k;
        disp[a].0 += ux * f;
        disp[a].1 += uy * f;
        disp[b].0 -= ux * f;
        disp[b].1 -= uy * f;
    }
    for i in 0..p.len() {
        disp[i].0 += (center.0 - p[i].0) * gravity;
        disp[i].1 += (center.1 - p[i].1) * gravity;
    }
}

/// 分层:先把边定向,再破环、定层号
fn ranks(g: &Graph) -> (Vec<usize>, Vec<(usize, usize)>) {
    let n = g.nodes.len();
    let dirs: Vec<(usize, usize)> = g
        .edges
        .iter()
        .filter(|e| e.directed)
        .map(|e| (e.a, e.b))
        .collect();
    if dirs.is_empty() {
        // 一条有向边都没有:按无向处理,连通块的 BFS 深度就是层号
        let r = ranks_bfs(g);
        // 顺手把每条边都朝下摆(层号小的当上端),后面扫重心要用
        let arcs = g
            .edges
            .iter()
            .map(|e| if r[e.a] <= r[e.b] { (e.a, e.b) } else { (e.b, e.a) })
            .collect();
        (r, arcs)
    } else {
        let (kept, _) = break_cycles(n, &dirs);
        (longest_path(n, &kept), kept)
    }
}

/// 有向图里找环:DFS 三色标记,指向"当前路径上"(灰色)节点的边就是回边,去掉它环就断了.
/// 返回(去掉回边后剩下的边, 有没有去掉过东西)
fn break_cycles(n: usize, arcs: &[(usize, usize)]) -> (Vec<(usize, usize)>, bool) {
    let mut out_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &(a, _)) in arcs.iter().enumerate() {
        out_of[a].push(i);
    }
    let mut color = vec![0u8; n]; // 0 没碰过 1 在当前路径上 2 走完了
    let mut cursor = vec![0usize; n];
    let mut kept = vec![true; arcs.len()];
    let mut dropped = false;
    for s in 0..n {
        if color[s] != 0 {
            continue;
        }
        color[s] = 1;
        let mut stack = vec![s];
        loop {
            let v = match stack.last() {
                Some(&v) => v,
                None => break,
            };
            if cursor[v] < out_of[v].len() {
                let ei = out_of[v][cursor[v]];
                cursor[v] += 1;
                let w = arcs[ei].1;
                match color[w] {
                    0 => {
                        color[w] = 1;
                        stack.push(w);
                    }
                    1 => {
                        kept[ei] = false;
                        dropped = true;
                    }
                    _ => {}
                }
            } else {
                color[v] = 2;
                stack.pop();
            }
        }
    }
    let kept_arcs: Vec<(usize, usize)> = arcs
        .iter()
        .zip(kept.iter())
        .filter(|(_, &k)| k)
        .map(|(&e, _)| e)
        .collect();
    (kept_arcs, dropped)
}

/// 最长路径当层号:入度为零的先入队,松弛出边.队列按索引升序起步,拓扑序是确定的
fn longest_path(n: usize, arcs: &[(usize, usize)]) -> Vec<usize> {
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for &(a, b) in arcs {
        succ[a].push(b);
        indeg[b] += 1;
    }
    let mut rank = vec![0usize; n];
    let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    while let Some(u) = q.pop_front() {
        for k in 0..succ[u].len() {
            let v = succ[u][k];
            rank[v] = rank[v].max(rank[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    rank
}

/// 无向图的层号:每个连通块从索引最小的节点开始 BFS,深度就是层号
fn ranks_bfs(g: &Graph) -> Vec<usize> {
    let n = g.nodes.len();
    let mut rank = vec![usize::MAX; n];
    let mut q: VecDeque<usize> = VecDeque::new();
    for s in 0..n {
        if rank[s] != usize::MAX {
            continue;
        }
        rank[s] = 0;
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &(v, _) in &g.adj[u] {
                if rank[v] == usize::MAX {
                    rank[v] = rank[u] + 1;
                    q.push_back(v);
                }
            }
        }
    }
    rank
}

/// 层内排序:每个节点往相邻层里邻居的平均位置靠,来回扫几轮把交叉的边理顺.
/// 排序键用 (重心, 节点索引),一样的时候按索引定,结果稳定
fn barycenter(layers: &mut [Vec<usize>], arcs: &[(usize, usize)], rank: &[usize]) {
    let n = rank.len();
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in arcs {
        if rank[a] < rank[b] {
            pred[b].push(a);
            succ[a].push(b);
        }
    }
    let depth = layers.len();
    for round in 0..BARY_ROUNDS {
        // 一轮下行(按上一层排)、一轮上行(按下一层排)交替
        let down = round % 2 == 0;
        let order: Vec<usize> = if down {
            (1..depth).collect()
        } else {
            (0..depth.saturating_sub(1)).rev().collect()
        };
        for r in order {
            let adjacent = if down { r - 1 } else { r + 1 };
            let nb = if down { &pred } else { &succ };
            // 相邻层里"节点 -> 它排第几"
            let mut slot = vec![0.0f64; n];
            for (j, &v) in layers[adjacent].iter().enumerate() {
                slot[v] = j as f64;
            }
            let mut key: Vec<(f64, usize)> = Vec::with_capacity(layers[r].len());
            for (j, &v) in layers[r].iter().enumerate() {
                let mut sum = 0.0;
                let mut cnt = 0usize;
                for &u in &nb[v] {
                    if rank[u] == adjacent {
                        sum += slot[u];
                        cnt += 1;
                    }
                }
                // 相邻层里一个邻居都没有:留在原地
                let bary = if cnt == 0 { j as f64 } else { sum / cnt as f64 };
                key.push((bary, v));
            }
            key.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            layers[r] = key.into_iter().map(|(_, v)| v).collect();
        }
    }
}

/// polish 之后把节点收回到各自的层带里:层号决定行(y = 层号 * 层间距),
/// 同一层按当前 x 从左到右排,相邻两个至少隔 LANE_GAP.
/// 于是任意两点不同格、同层不挤,有向边(层号大的在下)的上下次序一定保得住
fn relayer(p: &mut [(f64, f64)], rank: &[usize]) {
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..p.len() {
        layers[rank[i]].push(i);
    }
    for (r, layer) in layers.iter_mut().enumerate() {
        layer.sort_by(|&a, &b| p[a].0.total_cmp(&p[b].0).then(a.cmp(&b)));
        let y = r as f64 * LAYER_GAP;
        let mut prev = f64::NEG_INFINITY;
        for &i in layer.iter() {
            let want = p[i].0.round();
            let x = if prev.is_finite() {
                want.max(prev + LANE_GAP)
            } else {
                want
            };
            p[i] = (x, y);
            prev = x;
        }
    }
}

/// 分层布局(Sugiyama-lite,够小够明确):
///   1. 有向边当约束,DFS 找回边先去掉 —— 有环的图也画得出来
///   2. 按最长路径定层号(层号小的在上);一条有向边都没有时按无向处理,
///      连通块的 BFS 深度当层号
///   3. 层内按重心扫几轮定顺序,排序用节点索引兜底,结果稳定
///   4. y = 层号 * 层间距,x = 层内顺序 * 层间距,先摆成这样
///   5. 走同一套 polish 把边理顺,再按层收回层带(见 relayer),
///      保证有向边一定自上而下
fn layered(g: &Graph) -> Vec<(f64, f64)> {
    let n = g.nodes.len();
    let cfg = Cfg::of(n);
    let (rank, arcs) = ranks(g);
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    let mut p = vec![(0.0, 0.0); n];
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for i in 0..n {
        layers[rank[i]].push(i);
    }
    for (r, layer) in layers.iter().enumerate() {
        for (j, &i) in layer.iter().enumerate() {
            p[i] = (j as f64 * LANE_GAP, r as f64 * LAYER_GAP);
        }
    }
    barycenter(&mut layers, &arcs, &rank);
    let edges = edge_pairs(g);
    let mut out = polish(p, &edges, &cfg);
    relayer(&mut out, &rank);
    out
}

/// 图里有没有环:图里有有向边就按有向图算(回边就是环),不然按无向图算.
/// pick 用它决定没坐标的图走分层(无环)还是力导(有环)
fn has_cycle(g: &Graph) -> bool {
    let n = g.nodes.len();
    let dirs: Vec<(usize, usize)> = g
        .edges
        .iter()
        .filter(|e| e.directed)
        .map(|e| (e.a, e.b))
        .collect();
    if !dirs.is_empty() {
        return break_cycles(n, &dirs).1;
    }
    // 无向:DFS 走,碰到还在当前路径上的节点就是环(来路那一条边不算)
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &g.edges {
        adj[e.a].push(e.b);
        adj[e.b].push(e.a);
    }
    let mut color = vec![0u8; n];
    let mut parent = vec![usize::MAX; n];
    let mut cursor = vec![0usize; n];
    for s in 0..n {
        if color[s] != 0 {
            continue;
        }
        color[s] = 1;
        let mut stack = vec![s];
        loop {
            let v = match stack.last() {
                Some(&v) => v,
                None => break,
            };
            if cursor[v] < adj[v].len() {
                let w = adj[v][cursor[v]];
                cursor[v] += 1;
                if w == parent[v] {
                    continue;
                }
                match color[w] {
                    0 => {
                        color[w] = 1;
                        parent[w] = v;
                        stack.push(w);
                    }
                    1 => return true,
                    _ => {}
                }
            } else {
                color[v] = 2;
                stack.pop();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四个节点的一条折线,坐标都有
    fn line_graph() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((10.0, 0.0)));
        let d = b.node("c", Some((20.0, 3.0)));
        let e = b.node("d", Some((30.0, 3.0)));
        let g = b.group("主线", None);
        b.edge(a, c, g, false);
        b.edge(c, d, g, false);
        b.edge(d, e, g, false);
        b.build().unwrap()
    }

    /// 有向无环图,没有坐标
    fn dag() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let d = b.node("c", None);
        let e = b.node("d", None);
        let g = b.group("阶段", None);
        b.edge(a, c, g, true);
        b.edge(a, d, g, true);
        b.edge(c, e, g, true);
        b.edge(d, e, g, true);
        b.build().unwrap()
    }

    /// 有向环 a -> b -> c -> a
    fn directed_cycle() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let d = b.node("c", None);
        let g = b.group("环", None);
        b.edge(a, c, g, true);
        b.edge(c, d, g, true);
        b.edge(d, a, g, true);
        b.build().unwrap()
    }

    /// 无向的方块(一个四节点的环)
    fn undirected_box() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let d = b.node("c", None);
        let e = b.node("d", None);
        let g = b.group("圈", None);
        b.edge(a, c, g, false);
        b.edge(c, d, g, false);
        b.edge(d, e, g, false);
        b.edge(e, a, g, false);
        b.build().unwrap()
    }

    fn 格距(sc: &Scene, i: usize, j: usize) -> f64 {
        (sc.pos[i].0 - sc.pos[j].0)
            .abs()
            .max((sc.pos[i].1 - sc.pos[j].1).abs())
    }

    #[test]
    fn geo_缺坐标就报错() {
        let g = dag();
        assert!(scene(&g, Layout::Geo).is_err());
        assert!(scene(&g, Layout::Relax).is_err());
        assert!(scene(&line_graph(), Layout::Geo).is_ok());
        // 力导与分层不要求坐标
        assert!(scene(&g, Layout::Force).is_ok());
        assert!(scene(&g, Layout::Layered).is_ok());
    }

    #[test]
    fn relax_拉开节点与相邻节点() {
        let g = line_graph();
        let sc = scene(&g, Layout::Relax).unwrap();
        for i in 0..sc.pos.len() {
            assert!(sc.pos[i].0.fract() == 0.0 && sc.pos[i].1.fract() == 0.0);
            for j in i + 1..sc.pos.len() {
                assert!(格距(&sc, i, j) >= 1.0, "节点 {i} 和 {j} 摞在同一格");
            }
        }
        for e in &g.edges {
            assert!(
                格距(&sc, e.a, e.b) >= 2.0,
                "边 {} - {} 两端的节点挨得太近",
                e.a,
                e.b
            );
        }
    }

    #[test]
    fn force_两次跑结果一样() {
        for g in [dag(), undirected_box(), line_graph()] {
            let a = scene(&g, Layout::Force).unwrap();
            let b = scene(&g, Layout::Force).unwrap();
            assert_eq!(a.pos, b.pos, "力导结果不稳定");
        }
    }

    #[test]
    fn layered_有向边自上而下() {
        let g = dag();
        let sc = scene(&g, Layout::Layered).unwrap();
        for e in &g.edges {
            assert!(
                sc.pos[e.b].1 > sc.pos[e.a].1,
                "有向边 {} -> {} 的下游没排在下边",
                e.a,
                e.b
            );
        }
    }

    #[test]
    fn 有环图也能画出来() {
        for l in [Layout::Force, Layout::Layered] {
            let sc = scene(&directed_cycle(), l).unwrap();
            assert_eq!(sc.pos.len(), 3);
            assert!(sc.pos.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
        }
        assert!(scene(&undirected_box(), Layout::Layered).is_ok());
    }

    /// 同一对节点两个方向(有向图里的二元环):两套布局都得给出一上一下两个位子
    #[test]
    fn 来回的边也能画() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let grp = b.group("来回", None);
        b.edge(a, c, grp, true);
        b.edge(c, a, grp, true);
        let g = b.build().unwrap();
        for l in [Layout::Force, Layout::Layered] {
            let sc = scene(&g, l).unwrap();
            assert_eq!(sc.pos.len(), 2);
            assert!(sc.pos[0] != sc.pos[1], "{} 把两个节点摞在一起了", l.name());
        }
    }

    #[test]
    fn pick_按坐标与环挑布局() {
        // 有坐标:按坐标画
        assert_eq!(Layout::pick(&line_graph()), Layout::Geo);
        // 没坐标 + 有向无环:分层
        assert_eq!(Layout::pick(&dag()), Layout::Layered);
        // 没坐标 + 有向有环:力导
        assert_eq!(Layout::pick(&directed_cycle()), Layout::Force);
        // 没坐标 + 无向有环:力导
        assert_eq!(Layout::pick(&undirected_box()), Layout::Force);
    }

    #[test]
    fn 包围盒不塌成一点() {
        for l in Layout::all() {
            let sc = scene(&line_graph(), l).unwrap();
            let (w, h) = (sc.bbox.2 - sc.bbox.0, sc.bbox.3 - sc.bbox.1);
            assert!(w > 0.0 || h > 0.0, "{} 的输出全挤在一个点上", l.name());
            assert!(sc.pos.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
        }
    }

    #[test]
    fn 名字别名与循环() {
        for l in Layout::all() {
            assert_eq!(Layout::parse(l.name()), Some(l));
            assert_eq!(l.next().next().next().next(), l);
            assert!(!l.label(Lang::Zh).is_empty() && !l.label(Lang::En).is_empty());
        }
        assert_eq!(Layout::parse(" real "), Some(Layout::Geo));
        assert_eq!(Layout::parse("SPRING"), Some(Layout::Relax));
        assert_eq!(Layout::parse("fr"), Some(Layout::Force));
        assert_eq!(Layout::parse("dag"), Some(Layout::Layered));
        assert_eq!(Layout::parse("sugiyama"), Some(Layout::Layered));
        assert_eq!(Layout::parse("nope"), None);
        assert!(Layout::Geo.needs_pos() && Layout::Relax.needs_pos());
        assert!(!Layout::Force.needs_pos() && !Layout::Layered.needs_pos());
    }

    #[test]
    fn geo_按等比铺进画布() {
        let g = line_graph();
        let sc = scene(&g, Layout::Geo).unwrap();
        // 输入是一张扁图(30 x 3),等比铺开后横向占满、纵向只占一点
        let (w, _) = nominal_for(g.nodes.len());
        assert!(sc.bbox.0 >= 2.0 && sc.bbox.2 <= w - 2.0);
        // y 向上:输入里最高的两个点在画布上也在上面
        assert!(sc.pos[2].1 > sc.pos[0].1 && sc.pos[3].1 > sc.pos[1].1);
    }
}
