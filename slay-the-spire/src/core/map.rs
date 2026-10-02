// 第一章地图:7 列 15 层,6 条从下往上爬的路径.
// 生成方式沿用爬塔那套:先铺 6 条随机路径,再按层规则贴房间类型.
use crate::rng::Rng;

/// 普通层数(0..15);第 15 层是 Boss 前的休息点
pub const FLOORS: usize = 15;
/// 地图列数
pub const COLS: usize = 7;
/// 路径条数
pub const PATHS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Monster,
    Elite,
    Event,
    Rest,
    Shop,
    Treasure,
    Boss,
}

impl NodeKind {
    pub fn sigil(self) -> char {
        match self {
            NodeKind::Monster => 'M',
            NodeKind::Elite => 'E',
            NodeKind::Event => '?',
            NodeKind::Rest => 'R',
            NodeKind::Shop => '$',
            NodeKind::Treasure => 'T',
            NodeKind::Boss => 'B',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Monster => "Monster",
            NodeKind::Elite => "Elite",
            NodeKind::Event => "Event",
            NodeKind::Rest => "Rest",
            NodeKind::Shop => "Shop",
            NodeKind::Treasure => "Treasure",
            NodeKind::Boss => "Boss",
        }
    }

}

#[derive(Debug)]
pub struct Node {
    pub floor: usize,
    pub col: usize,
    pub kind: NodeKind,
    /// 上一层可通向本节点的节点下标
    pub prev: Vec<usize>,
    /// 本节点通向的下一层节点下标
    pub next: Vec<usize>,
}

#[derive(Debug)]
pub struct ActMap {
    pub nodes: Vec<Node>,
    /// 每层的节点下标
    pub rows: Vec<Vec<usize>>,
    /// Boss 节点下标
    pub boss: usize,
}

impl ActMap {
    pub fn node(&self, i: usize) -> &Node {
        &self.nodes[i]
    }

    pub fn row(&self, floor: usize) -> &[usize] {
        self.rows.get(floor).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 总共多少层(含 Boss 层)
    pub fn total_floors(&self) -> usize {
        self.nodes[self.boss].floor + 1
    }

    /// 站在 cur 上时可选的下一节点;cur 为 None 表示还没出发
    pub fn reachable_from(&self, cur: Option<usize>) -> Vec<usize> {
        match cur {
            None => self.rows[0].clone(),
            Some(i) => {
                let mut v = self.nodes[i].next.clone();
                v.sort_unstable();
                v
            }
        }
    }

    /// 生成地图
    ///
    /// 6 条路径按"列号非递减"的顺序同时往上走:第 i 条永远不跑到第 i+1 条的右边,
    /// 于是路径只会合并或分叉,不会交叉。两条路径落在同一列时共用同一个节点。
    pub fn generate(rng: &mut Rng) -> ActMap {
        let mut nodes: Vec<Node> = Vec::new();
        // grid[floor][col] = 该位置已建立的节点下标
        let mut grid: Vec<Vec<Option<usize>>> = vec![vec![None; COLS]; FLOORS];

        let ensure = |nodes: &mut Vec<Node>, grid: &mut Vec<Vec<Option<usize>>>, f: usize, c: usize| -> usize {
            if let Some(i) = grid[f][c] {
                return i;
            }
            let i = nodes.len();
            nodes.push(Node {
                floor: f,
                col: c,
                kind: NodeKind::Monster,
                prev: Vec::new(),
                next: Vec::new(),
            });
            grid[f][c] = Some(i);
            i
        };

        // 起点列:随机取 6 个再排序,允许重复(重复即共用起点)
        let mut cols: Vec<usize> = (0..PATHS).map(|_| rng.below(COLS as u32) as usize).collect();
        cols.sort_unstable();
        let mut prev_nodes: Vec<Option<usize>> = vec![None; PATHS];
        for f in 0..FLOORS {
            for i in 0..PATHS {
                let idx = ensure(&mut nodes, &mut grid, f, cols[i]);
                if let Some(p) = prev_nodes[i] {
                    if !nodes[p].next.contains(&idx) {
                        nodes[p].next.push(idx);
                    }
                    if !nodes[idx].prev.contains(&p) {
                        nodes[idx].prev.push(p);
                    }
                }
                prev_nodes[i] = Some(idx);
            }
            if f + 1 == FLOORS {
                break;
            }
            // 下一层的列:直行优先,左右各一半;并且不许越过左边那条已经选好的列
            let mut lower = 0usize;
            let mut next_cols = vec![0usize; PATHS];
            for i in 0..PATHS {
                let c = cols[i];
                let mut cands: Vec<(usize, u32)> = vec![(c, 2)];
                if c > 0 {
                    cands.push((c - 1, 1));
                }
                if c + 1 < COLS {
                    cands.push((c + 1, 1));
                }
                cands.retain(|(x, _)| *x >= lower);
                let chosen = if cands.is_empty() {
                    lower.min(COLS - 1)
                } else {
                    let ws: Vec<u32> = cands.iter().map(|(_, w)| *w).collect();
                    cands[rng.weighted_idx(&ws).unwrap_or(0)].0
                };
                next_cols[i] = chosen.min(COLS - 1);
                lower = next_cols[i];
            }
            cols = next_cols;
        }

        // Boss 节点:所有第 14 层节点都连上去
        let boss = nodes.len();
        nodes.push(Node {
            floor: FLOORS,
            col: COLS / 2,
            kind: NodeKind::Boss,
            prev: Vec::new(),
            next: Vec::new(),
        });
        for i in 0..nodes.len() - 1 {
            if nodes[i].floor == FLOORS - 1 {
                nodes[i].next.push(boss);
                nodes[boss].prev.push(i);
            }
        }

        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); FLOORS + 1];
        for (i, n) in nodes.iter().enumerate() {
            rows[n.floor].push(i);
        }
        for r in rows.iter_mut() {
            // 同层按列排序,渲染时从左到右稳定
            r.sort_by_key(|i| nodes[*i].col);
        }

        let mut map = ActMap { nodes, rows, boss };
        map.assign_kinds(rng);
        map
    }

    /// 贴房间类型.顺序很关键:必须按层从下往上,父节点的类型才是已知的
    fn assign_kinds(&mut self, rng: &mut Rng) {
        // 固定层
        for i in 0..self.nodes.len() {
            let f = self.nodes[i].floor;
            self.nodes[i].kind = match f {
                0 => NodeKind::Monster,
                8 => NodeKind::Treasure,
                _ if f == FLOORS - 1 => NodeKind::Rest,
                _ if f == FLOORS => NodeKind::Boss,
                _ => NodeKind::Monster,
            };
        }
        let mut pending: Vec<usize> = (0..self.nodes.len())
            .filter(|i| {
                let f = self.nodes[*i].floor;
                f != 0 && f != 8 && f != FLOORS - 1 && f != FLOORS
            })
            .collect();
        pending.sort_by_key(|i| self.nodes[*i].floor);
        for i in pending {
            let floor = self.nodes[i].floor;
            let parents: Vec<NodeKind> = self
                .nodes[i]
                .prev
                .iter()
                .map(|p| self.nodes[*p].kind)
                .collect();
            let mut cands: Vec<(NodeKind, u32)> = vec![
                (NodeKind::Monster, 45),
                (NodeKind::Event, 22),
                (NodeKind::Rest, 12),
                (NodeKind::Shop, 5),
            ];
            if floor >= 5 {
                cands.push((NodeKind::Elite, 16));
            }
            cands.retain(|(k, _)| {
                // 同一条路径上不连续出现同一类型(怪物例外)
                if *k != NodeKind::Monster && parents.contains(k) {
                    return false;
                }
                // 休息点与商店不出现在 Boss 前两层
                if matches!(k, NodeKind::Rest | NodeKind::Shop) && floor >= FLOORS - 2 {
                    return false;
                }
                true
            });
            if cands.is_empty() {
                continue;
            }
            let ws: Vec<u32> = cands.iter().map(|(_, w)| *w).collect();
            if let Some(idx) = rng.weighted_idx(&ws) {
                self.nodes[i].kind = cands[idx].0;
            }
        }
        // 保底:至少一个精英、一个商店、一个事件
        self.ensure_kind_present(rng, NodeKind::Elite, 5);
        self.ensure_kind_present(rng, NodeKind::Shop, 1);
        self.ensure_kind_present(rng, NodeKind::Event, 1);
    }

    fn ensure_kind_present(&mut self, rng: &mut Rng, kind: NodeKind, min_floor: usize) {
        if self.nodes.iter().any(|n| n.kind == kind) {
            return;
        }
        let cands: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                n.kind == NodeKind::Monster
                    && n.floor >= min_floor
                    && n.floor < FLOORS - 1
                    && n.floor != 8
            })
            .map(|(i, _)| i)
            .collect();
        if cands.is_empty() {
            return;
        }
        let pick = cands[rng.below(cands.len() as u32) as usize];
        self.nodes[pick].kind = kind;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(seed: u64) -> ActMap {
        ActMap::generate(&mut Rng::new(seed))
    }

    #[test]
    fn fixed_floors_have_fixed_kinds() {
        for seed in 0..20 {
            let m = map(seed);
            for i in m.row(0) {
                assert_eq!(m.node(*i).kind, NodeKind::Monster, "第一层只能是怪");
            }
            for i in m.row(8) {
                assert_eq!(m.node(*i).kind, NodeKind::Treasure, "第九层是宝箱");
            }
            for i in m.row(FLOORS - 1) {
                assert_eq!(m.node(*i).kind, NodeKind::Rest, "最后一层是休息点");
            }
            assert_eq!(m.node(m.boss).kind, NodeKind::Boss);
        }
    }

    #[test]
    fn edges_are_symmetric_and_mostly_connected() {
        for seed in 0..20 {
            let m = map(seed);
            for (i, n) in m.nodes.iter().enumerate() {
                for nxt in &n.next {
                    assert!(
                        m.nodes[*nxt].prev.contains(&i),
                        "next/prev 不对称: {i} -> {nxt}"
                    );
                    assert_eq!(m.nodes[*nxt].floor, n.floor + 1, "边只能连相邻层");
                }
                for p in &n.prev {
                    assert!(m.nodes[*p].next.contains(&i), "prev/next 不对称");
                    assert_eq!(m.nodes[*p].floor + 1, n.floor);
                }
            }
        }
    }

    #[test]
    fn every_non_start_node_has_a_parent() {
        for seed in 0..20 {
            let m = map(seed);
            for (i, n) in m.nodes.iter().enumerate() {
                if n.floor == 0 {
                    assert!(n.prev.is_empty(), "第一层不该有父节点");
                } else {
                    assert!(!n.prev.is_empty(), "节点 {i} 没有父节点,无法到达");
                }
            }
        }
    }

    #[test]
    fn every_node_reaches_the_boss() {
        for seed in 0..10 {
            let m = map(seed);
            // 从第一层任意节点都能走到 Boss
            for &start in m.row(0) {
                let mut stack = vec![start];
                let mut seen = vec![false; m.nodes.len()];
                let mut reached_boss = false;
                while let Some(i) = stack.pop() {
                    if seen[i] {
                        continue;
                    }
                    seen[i] = true;
                    if i == m.boss {
                        reached_boss = true;
                    }
                    for nxt in &m.nodes[i].next {
                        stack.push(*nxt);
                    }
                }
                assert!(reached_boss, "从 {start} 走不到 Boss");
            }
        }
    }

    #[test]
    fn each_node_has_at_most_three_children() {
        for seed in 0..20 {
            let m = map(seed);
            for n in &m.nodes {
                assert!(n.next.len() <= 3, "单个节点最多三个分叉");
            }
        }
    }

    #[test]
    fn elites_shops_and_events_are_present() {
        for seed in 0..30 {
            let m = map(seed);
            let has = |k: NodeKind| m.nodes.iter().any(|n| n.kind == k);
            assert!(has(NodeKind::Elite), "seed {seed} 地图没有精英");
            assert!(has(NodeKind::Shop), "seed {seed} 地图没有商店");
            assert!(has(NodeKind::Event), "seed {seed} 地图没有事件");
        }
    }

    #[test]
    fn progression_is_always_possible() {
        // 站在任意节点,下一层至少有一个可选目标(除了 Boss)
        for seed in 0..10 {
            let m = map(seed);
            for (i, n) in m.nodes.iter().enumerate() {
                if n.kind == NodeKind::Boss {
                    assert!(n.next.is_empty());
                } else {
                    assert!(!n.next.is_empty(), "节点 {i} 没有出口");
                    let r = m.reachable_from(Some(i));
                    assert!(!r.is_empty());
                }
            }
            assert_eq!(m.reachable_from(None), m.rows[0]);
        }
    }

    #[test]
    fn same_seed_same_map() {
        let a = map(123);
        let b = map(123);
        assert_eq!(a.nodes.len(), b.nodes.len());
        for (x, y) in a.nodes.iter().zip(b.nodes.iter()) {
            assert_eq!(x.floor, y.floor);
            assert_eq!(x.col, y.col);
            assert_eq!(x.kind, y.kind);
            assert_eq!(x.next, y.next);
        }
    }

    #[test]
    fn paths_do_not_cross() {
        // 路径不交叉:同一层两节点的列序,在下一层保持同样的相对关系
        for seed in 0..20 {
            let m = map(seed);
            for f in 0..FLOORS {
                let row = m.row(f);
                for w in row.windows(2) {
                    let (a, b) = (m.node(w[0]), m.node(w[1]));
                    assert!(a.col < b.col, "同层节点应按列排序");
                    for (_, an) in a.next.iter().enumerate() {
                        for bn in b.next.iter() {
                            if m.node(*an).floor != f + 1 || m.node(*bn).floor != f + 1 {
                                continue;
                            }
                            assert!(
                                m.node(*an).col <= m.node(*bn).col,
                                "路径出现交叉: {a:?} / {b:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
