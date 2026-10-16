//! 光栅化:把画布坐标画成"每格往哪几个方向连",再按方向位合成制表符.
//! 这一层只认识字符格,不认识 ratatui(上屏由 render 做).

use std::collections::HashSet;

/// 格子上的方向位:一格会和上下左右四个方向连
pub(crate) const LEFT: u8 = 1;
pub(crate) const RIGHT: u8 = 2;
pub(crate) const UP: u8 = 4;
pub(crate) const DOWN: u8 = 8;

// 方向位的组合(模式匹配里要当作一个整体,不能现算)
const LR: u8 = LEFT | RIGHT;
const UD: u8 = UP | DOWN;
const LRD: u8 = LEFT | RIGHT | DOWN;
const LRU: u8 = LEFT | RIGHT | UP;
const UDR: u8 = UP | DOWN | RIGHT;
const UDL: u8 = UP | DOWN | LEFT;
const LRUD: u8 = LEFT | RIGHT | UP | DOWN;
const DR: u8 = DOWN | RIGHT;
const DL: u8 = DOWN | LEFT;
const UR: u8 = UP | RIGHT;
const UL: u8 = UP | LEFT;

/// 方向位 -> 制表符(横竖 + 圆角 + 三通四通).
/// 单个方向不成笔画(路径至少有一步),落到兜底那档给个 '│'
pub(crate) fn box_char(mask: u8) -> char {
    match mask {
        0 => ' ',
        LRUD => '┼',
        LRD => '┬',
        LRU => '┴',
        UDR => '├',
        UDL => '┤',
        LR => '─',
        UD => '│',
        DR => '╭',
        DL => '╮',
        UR => '╰',
        UL => '╯',
        _ => '│',
    }
}

/// a 和 c 相对 b 在同一侧(都偏北/南/东/西)时,边在 b 这里应该是"竖着进"或"横着进";
/// 一进一出(正常穿过)就交给主方向自己定.返回 true 表示这一段要求横着进节点
pub(crate) fn arrive_hor(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> Option<bool> {
    let (da, dc) = (a.1 - b.1, c.1 - b.1);
    if da != 0 && dc != 0 && da.signum() == dc.signum() {
        return Some(false); // 两头都在上边或都在下边:竖着进
    }
    let (xa, xc) = (a.0 - b.0, c.0 - b.0);
    if xa != 0 && xc != 0 && xa.signum() == xc.signum() {
        return Some(true); // 两头都在左边或都在右边:横着进
    }
    None
}

/// 画布:边只记"这一格往哪几个方向连",最后按方向位合成制表符,
/// 这样拐弯处自然就是 ╭╮╯╰,交叉处自然就是 ┼,不用逐段猜字符
pub(crate) struct Canvas {
    pub(crate) x0: i32,
    pub(crate) y0: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) mask: Vec<u8>,
    pub(crate) fg: Vec<(u8, u8, u8)>,
    /// 这一格的颜色是第几个分组占的(-1 = 还没人占)
    pub(crate) owner: Vec<i16>,
    /// 占住这一格的那个分组在这一格的方向位(别的分组从这儿过就"断一格")
    pub(crate) owner_mask: Vec<u8>,
    /// 这一格的颜色是不是"光标那条分组"的
    pub(crate) mine: Vec<bool>,
    /// 节点:0 没有,1 普通节点,2 枢纽
    pub(crate) node: Vec<u8>,
    /// 这一格上有哪几个分组(位掩码,最多 64 个够用)
    pub(crate) groups_here: Vec<u64>,
    /// 作为节点,这一个节点属于哪几个分组(只是想从这儿穿过的分组不算)
    pub(crate) node_groups: Vec<u64>,
    /// 所有节点的格子:画边时先避开它们,别从节点身上穿过去
    pub(crate) node_cells: HashSet<(i32, i32)>,
    /// 有向边画在箭头那一格的字形(None = 这一格没有箭头)
    pub(crate) arrow: Vec<Option<char>>,
    /// 聚焦的分组(位掩码;0 = 不聚焦).没聚焦时所有分组都正常画
    pub(crate) focus: u64,
    /// 光标所在节点的那几个分组:颜色冲突时它们说了算
    pub(crate) priority: Vec<bool>,
}

impl Canvas {
    pub(crate) fn new(
        area: (i32, i32, i32, i32),
        priority: Vec<bool>,
        focus: u64,
    ) -> Canvas {
        let (x0, y0, w, h) = area;
        let n = (w.max(0) * h.max(0)) as usize;
        Canvas {
            x0,
            y0,
            w: w.max(0),
            h: h.max(0),
            mask: vec![0; n],
            fg: vec![(0, 0, 0); n],
            owner: vec![-1; n],
            owner_mask: vec![0; n],
            mine: vec![false; n],
            node: vec![0; n],
            groups_here: vec![0; n],
            node_groups: vec![0; n],
            node_cells: HashSet::new(),
            arrow: vec![None; n],
            focus,
            priority,
        }
    }

    /// 一格只能有一种颜色,也只能有一种形状.
    /// 颜色谁说了算:光标所在节点的那些分组 > 先画的分组 —— 于是两组挤在同一格时,
    /// 颜色会跟着光标走,从当前节点就能顺着颜色找到自己那一组.
    /// 形状只按"先画的说了算",**不跟光标走**:不然挪一下光标,谁在这里断一格就翻到
    /// 另一组上,整段边的字形都跟着变,看着就是边在变形
    fn claim(&mut self, i: usize, group: usize, bit: u8, fg: (u8, u8, u8)) {
        let cur = self.owner[i];
        if cur == group as i16 {
            self.owner_mask[i] |= bit;
            if self.priority[group] {
                self.fg[i] = fg; // 这一格本来就是自己占的,颜色归自己
            }
            return;
        }
        if cur < 0 {
            self.owner[i] = group as i16;
            self.owner_mask[i] = bit;
            self.fg[i] = fg;
            self.mine[i] = self.priority[group];
        } else if self.priority[group] && !self.priority[cur as usize] {
            self.fg[i] = fg; // 只让颜色,形状留给先画的那一组
        }
    }

    pub(crate) fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return None;
        }
        Some(((y - self.y0) * self.w + (x - self.x0)) as usize)
    }

    /// 这一格有没有东西(节点名标注只往空位上写)
    pub(crate) fn at(&self, x: i32, y: i32) -> u8 {
        match self.idx(x, y) {
            Some(i) => {
                let mut k = self.node[i];
                if self.mask[i] != 0 {
                    k = k.max(1);
                }
                k
            }
            None => u8::MAX,
        }
    }

    /// 在 (x, y) 记上"这里和某个方向连着"
    fn link(&mut self, x: i32, y: i32, bit: u8, group: usize, fg: (u8, u8, u8)) {
        let Some(i) = self.idx(x, y) else { return };
        self.mask[i] |= bit;
        self.groups_here[i] |= 1 << group;
        self.claim(i, group, bit, fg);
    }

    /// 一段直线(同一行或同一列)
    fn run(&mut self, a: (i32, i32), b: (i32, i32), group: usize, fg: (u8, u8, u8)) {
        if a.1 == b.1 {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            for x in x0..x1 {
                self.link(x, a.1, RIGHT, group, fg);
                self.link(x + 1, a.1, LEFT, group, fg);
            }
        } else if a.0 == b.0 {
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            for y in y0..y1 {
                self.link(a.0, y, DOWN, group, fg);
                self.link(a.0, y + 1, UP, group, fg);
            }
        }
    }

    /// 一段直线上的格子(含首尾)
    fn span(&self, a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        if a.1 == b.1 {
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                out.push((x, a.1));
            }
        } else if a.0 == b.0 {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                out.push((a.0, y));
            }
        }
        out
    }

    /// 从 from 出发、只走这个分组已经画过的格子,能不能走到 to
    fn joined(&self, group: usize, from: (i32, i32), to: (i32, i32)) -> bool {
        let bit = 1u64 << group;
        let has = |p: (i32, i32)| {
            self.idx(p.0, p.1)
                .map(|i| self.groups_here[i] & bit != 0)
                .unwrap_or(false)
        };
        if !has(from) || !has(to) {
            return false;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![from];
        seen.insert(from);
        while let Some(p) = stack.pop() {
            if p == to {
                return true;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let q = (p.0 + dx, p.1 + dy);
                if has(q) && seen.insert(q) {
                    stack.push(q);
                }
            }
        }
        false
    }

    /// 这条折线会踩到多少**别的分组**已经画过的格子(复用自己的走廊不算代价,
    /// 否则"下去再上来"会因为想省格子而绕到旁边去,围出一个方框)
    fn cost(&self, a: (i32, i32), corner: (i32, i32), b: (i32, i32), group: usize) -> i32 {
        let bit = 1u64 << group;
        let mut n = 0;
        // 这一笔里有没有"这个分组还没占过"的格子
        let fresh = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .any(|p| {
                self.idx(p.0, p.1)
                    .map(|i| self.groups_here[i] & bit == 0)
                    .unwrap_or(false)
            });
        // 两端在这个分组已经画过的地方是通的,而这一笔又要踩新格子:
        // 那就等于把自己绕成一个环(图上一个封闭的圈),尽量别这么拐
        if fresh && (self.joined(group, a, corner) || self.joined(group, corner, b)) {
            n += 1000;
        }
        // 再把"封口"也算进去:这一笔落下之后,如果某一格的四条边都成了这个分组的笔画,
        // 那一格就被自己围住了(视觉上就是个方框),这种拐法也要尽量躲开
        let path: HashSet<(i32, i32)> = self
            .span(a, corner)
            .into_iter()
            .chain(self.span(corner, b))
            .collect();
        let mine = |p: (i32, i32)| {
            path.contains(&p)
                || self
                    .idx(p.0, p.1)
                    .map(|i| self.groups_here[i] & bit != 0)
                    .unwrap_or(false)
        };
        let mut encloses = false;
        'outer: for &(x, y) in &path {
            for (dx, dy) in [(-1i32, -1i32), (-1, 0), (0, -1), (0, 0)] {
                let c = (x + dx, y + dy);
                if mine(c) && mine((c.0 + 1, c.1)) && mine((c.0, c.1 + 1)) && mine((c.0 + 1, c.1 + 1))
                {
                    encloses = true;
                    break 'outer;
                }
            }
        }
        if encloses {
            n += 1000;
        }
        for p in self.span(a, corner).into_iter().chain(self.span(corner, b)) {
            if p == a || p == b {
                continue; // 两个端点本来就是节点
            }
            if let Some(i) = self.idx(p.0, p.1) {
                if self.groups_here[i] != 0 && self.groups_here[i] & bit == 0 {
                    n += 1;
                }
            }
            // 从节点身上穿过去很难看:边会画进那个 ○ 里,还容易和别的段围出方框
            if self.node_cells.contains(&p) {
                n += 100;
            }
        }
        n
    }

    /// 一条边:同排或同列直接连,否则拐一个直角.
    ///
    /// 拐法有两种(先横后竖 / 先竖后横).`arrive_hor` 是这一端要求的"进节点方向"
    /// (由前后两个节点的位置定,见 arrive_hor):要求竖着进,就先横着走再竖着进,
    /// 于是同一个分组"下去再上来"会走同一列,不会绕着节点围出一个方框.
    /// 实在会多踩别人很多格子,才让位给"踩得少"的那种.
    ///
    /// `heads` 是有向边的箭头画在哪几端:(画在 a 端, 画在 b 端).
    /// 同一个分组里 a -> b 与 b -> a 是两条独立的边,合到这一段上就是两端各一个箭头
    pub(crate) fn edge(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        group: usize,
        fg: (u8, u8, u8),
        arrive_hor: Option<bool>,
        heads: (bool, bool),
    ) {
        let (at_a, at_b) = heads;
        if a == b {
            return;
        }
        if a.1 == b.1 || a.0 == b.0 {
            self.run(a, b, group, fg);
            // 直线没有拐点:从另一头看过来,箭头就落在终点前面那一格
            if at_b {
                self.head_arrow(a, b, group);
            }
            if at_a {
                self.head_arrow(b, a, group);
            }
            return;
        }
        let hor_first = (b.0, a.1);
        let ver_first = (a.0, b.1);
        let (c1, c2) = (
            self.cost(a, hor_first, b, group),
            self.cost(a, ver_first, b, group),
        );
        // 要求竖着进节点(进节点那一段是竖的)就先横后竖
        let prefer_hor = arrive_hor
            .map(|h| !h)
            .unwrap_or((b.0 - a.0).abs() >= (b.1 - a.1).abs());
        let corner = if prefer_hor {
            if c1 <= c2 + 1 {
                hor_first
            } else {
                ver_first
            }
        } else if c2 <= c1 + 1 {
            ver_first
        } else {
            hor_first
        };
        self.run(a, corner, group, fg);
        self.run(corner, b, group, fg);
        // 拐点也是"最后一段"的起点,两端的箭头各自照自己那一端的走向画
        if at_b {
            self.head_arrow(corner, b, group);
        }
        if at_a {
            self.head_arrow(corner, a, group);
        }
    }

    /// 有向边的箭头:画在 to 紧挨着的那一格(从 from 往 to 走的方向).
    /// 一格最多一个箭头;撞上节点字形、或者这一格已经归别的分组,就跳过
    fn head_arrow(&mut self, from: (i32, i32), to: (i32, i32), group: usize) {
        let (dx, dy) = ((to.0 - from.0).signum(), (to.1 - from.1).signum());
        let (cell, ch) = if dx != 0 {
            ((to.0 - dx, to.1), if dx > 0 { '>' } else { '<' })
        } else if dy != 0 {
            ((to.0, to.1 - dy), if dy > 0 { 'v' } else { '^' })
        } else {
            return;
        };
        let Some(i) = self.idx(cell.0, cell.1) else { return };
        if self.node[i] != 0 {
            return; // 节点字形优先
        }
        if self.arrow[i].is_some() {
            return;
        }
        // 这一格得是这个分组自己占的:和别的分组挤在一起时方向说不清
        if self.owner[i] != group as i16 || self.groups_here[i] != 1u64 << group {
            return;
        }
        self.arrow[i] = Some(ch);
    }

    /// group_mask:这一个节点经过的所有分组;group:用哪个颜色(fg 也是它的).
    /// 孤立节点不属于任何分组,给 None:格子照画,颜色用中性色
    pub(crate) fn node_at(
        &mut self,
        x: i32,
        y: i32,
        hub: bool,
        group_mask: u64,
        group: Option<usize>,
        fg: (u8, u8, u8),
    ) {
        let Some(i) = self.idx(x, y) else { return };
        self.node[i] = if hub { 2 } else { 1 };
        self.groups_here[i] |= group_mask;
        self.node_groups[i] = group_mask;
        match group {
            Some(g) => {
                self.owner[i] = g as i16;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = self.priority[g];
            }
            None => {
                self.owner[i] = -1;
                self.owner_mask[i] = 0;
                self.fg[i] = fg;
                self.mine[i] = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 方向位合成制表符() {
        assert_eq!(box_char(LEFT | RIGHT), '─');
        assert_eq!(box_char(UP | DOWN), '│');
        assert_eq!(box_char(DOWN | RIGHT), '╭');
        assert_eq!(box_char(DOWN | LEFT), '╮');
        assert_eq!(box_char(UP | RIGHT), '╰');
        assert_eq!(box_char(UP | LEFT), '╯');
        assert_eq!(box_char(UP | DOWN | RIGHT), '├');
        assert_eq!(box_char(UP | DOWN | LEFT), '┤');
        assert_eq!(box_char(LEFT | RIGHT | DOWN), '┬');
        assert_eq!(box_char(LEFT | RIGHT | UP), '┴');
        assert_eq!(box_char(LEFT | RIGHT | UP | DOWN), '┼');
        assert_eq!(box_char(0), ' ');
    }

    /// 直角拐弯:两笔的方向位一合,自然就是四个圆角之一
    #[test]
    fn 拐角两笔合成圆角() {
        let corner_of = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| {
            let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
            cv.edge(a, b, 0, (255, 0, 0), None, (false, false));
            box_char(cv.mask[cv.idx(c.0, c.1).expect("在画布内")])
        };
        // 先横后竖:拐点在右上角是 ╮、在左上角是 ╭
        assert_eq!(corner_of((2, 2), (7, 7), (7, 2)), '╮');
        assert_eq!(corner_of((12, 2), (7, 7), (7, 2)), '╭');
        // 先竖后横:拐点在左下角是 ╰、在右下角是 ╯
        assert_eq!(corner_of((2, 2), (7, 12), (2, 12)), '╰');
        assert_eq!(corner_of((12, 2), (7, 12), (12, 12)), '╯');
    }

    /// 三通四通:横竖两笔在**同一个分组**的同一格上接起来就是 ┼ / ┬
    #[test]
    fn 三通四通按方向位合成() {
        let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, false));
        cv.edge((5, 2), (5, 9), 0, (255, 0, 0), None, (false, false));
        let cross = cv.idx(5, 5).expect("在画布内");
        assert_eq!(cv.mask[cross], LEFT | RIGHT | UP | DOWN);
        assert_eq!(box_char(cv.mask[cross]), '┼');
        // 只从上面接过来:三通里的 ┬
        let mut cv = Canvas::new((0, 0, 40, 40), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, false));
        cv.edge((5, 5), (5, 9), 0, (255, 0, 0), None, (false, false));
        let tee = cv.idx(5, 5).expect("在画布内");
        assert_eq!(cv.mask[tee], LEFT | RIGHT | DOWN);
        assert_eq!(box_char(cv.mask[tee]), '┬');
    }

    /// "下去再上来"的两条边必须共用同一列:
    /// a 与 c 都在 b 的北边,于是进 b 与出 b 都走竖的,两条边落在同一列,
    /// 不会围出一个方框
    #[test]
    fn 下去又上来要走同一列() {
        let mut cv = Canvas::new((0, 0, 60, 30), vec![false; 2], 0);
        let (a, b, c) = ((10, 5), (13, 9), (12, 5));
        let arrive = arrive_hor(a, b, c);
        assert_eq!(arrive, Some(false), "两头都在北边,该竖着进节点");
        cv.edge(a, b, 0, (255, 0, 0), arrive, (false, false));
        cv.edge(b, c, 0, (255, 0, 0), None, (false, false));
        // b 所在那一列:从 a 的行到 b 的行,每一格都该有笔画
        for y in a.1..=b.1 {
            let i = cv.idx(b.0, y).expect("在画布内");
            assert_ne!(cv.mask[i], 0, "({}, {y}) 该有笔画", b.0);
        }
        // 同一个纵向区间里,别的列不该再出现笔画(那就是绕出来的方框)
        for x in a.0..b.0 {
            for y in a.1 + 1..b.1 {
                let i = cv.idx(x, y).expect("在画布内");
                assert_eq!(cv.mask[i], 0, "({x}, {y}) 不该有笔画:边绕出去了");
            }
        }
    }

    /// 有向边的箭头:画在终点紧挨着的那一格,方向按最后那一段的走向
    #[test]
    fn 有向边箭头画在终点那一格() {
        // 往东
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((2, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'));
        assert_eq!(cv.arrow[cv.idx(9, 5).unwrap()], None, "箭头不占终点本身");
        assert_eq!(cv.arrow[cv.idx(2, 5).unwrap()], None, "只在终点那一头画");
        // 往西:箭头跟着走,画在另一侧
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((9, 5), (2, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(3, 5).unwrap()], Some('<'));
        // 往南(屏幕行号变大)
        let mut cv = Canvas::new((0, 0, 20, 20), vec![false; 1], 0);
        cv.edge((5, 2), (5, 12), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 11).unwrap()], Some('v'));
        // 往北
        let mut cv = Canvas::new((0, 0, 20, 20), vec![false; 1], 0);
        cv.edge((5, 12), (5, 2), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 3).unwrap()], Some('^'));
    }

    /// 拐弯的边:箭头按**最后那一段**的走向,不是两个端点的连线方向
    #[test]
    fn 拐弯的有向边按最后一段画箭头() {
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        // 横向比纵向长,先横后竖:最后一段是竖的,往下走到 (20,10)
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (false, true));
        assert_eq!(box_char(cv.mask[cv.idx(20, 2).unwrap()]), '╮', "先横后竖");
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "竖着进节点");
        // 箭头画在 a 那一头时也一样:它指着箭头所在的那个节点,所以是反着的
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, false));
        assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "指回 (2,2)");
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], None, "另一头不画");
    }

    /// 箭头不盖节点字形
    #[test]
    fn 箭头不盖节点字形() {
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.node_at(5, 5, false, 1, Some(0), (0, 255, 0));
        cv.node_cells.insert((5, 5));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(5, 5).unwrap()], None, "节点那一格不画箭头");
        assert_eq!(cv.arrow[cv.idx(4, 5).unwrap()], None, "箭头不会往前挪一格");
    }

    /// 一格已经归别的分组(两组挤在同一格)时不画箭头:方向说不清
    #[test]
    fn 箭头不进别人占的格子() {
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 2], 0);
        cv.edge((0, 5), (9, 5), 1, (0, 0, 255), None, (false, false));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], None);
        assert_eq!(cv.groups_here[cv.idx(8, 5).unwrap()], 0b11, "两组都在这格上");
        // 一格最多一个箭头:同一笔画再走一遍也不重复画
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 2], 0);
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (false, true));
        let n = cv.arrow.iter().filter(|a| a.is_some()).count();
        assert_eq!(n, 1, "两笔画同一格时只留一个箭头");
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'), "先到的那一个说了算");
    }

    /// 同一个分组里 a -> b 与 b -> a 是两条边,合到一段上就是两端各一个箭头
    #[test]
    fn 双向边两端都有箭头() {
        // 直线:去程那一头朝东,回程那一头朝西
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        cv.edge((0, 5), (9, 5), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow[cv.idx(8, 5).unwrap()], Some('>'), "箭头对着 (9,5)");
        assert_eq!(cv.arrow[cv.idx(1, 5).unwrap()], Some('<'), "箭头对着 (0,5)");
        // 拐弯的也一样:两端各自照自己那一端的走向画
        let mut cv = Canvas::new((0, 0, 40, 20), vec![false; 1], 0);
        cv.edge((2, 2), (20, 10), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow[cv.idx(20, 9).unwrap()], Some('v'), "b 端竖着进来");
        assert_eq!(cv.arrow[cv.idx(3, 2).unwrap()], Some('<'), "a 端横着回去");
        // 两个节点挨着(中间没有空格)时箭头没地方放,节点字形优先,谁都不画
        let mut cv = Canvas::new((0, 0, 20, 10), vec![false; 1], 0);
        for x in [4, 5] {
            cv.node_at(x, 5, false, 1, Some(0), (0, 255, 0));
            cv.node_cells.insert((x, 5));
        }
        cv.edge((4, 5), (5, 5), 0, (255, 0, 0), None, (true, true));
        assert_eq!(cv.arrow.iter().filter(|a| a.is_some()).count(), 0);
    }
}
