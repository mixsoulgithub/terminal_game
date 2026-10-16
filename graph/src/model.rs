//! 图模型:节点、边、边分组.与画法无关,布局与渲染都只看这里.
//!
//! 用 Builder 造图(索引由 push 返回),build() 之后索引才是稳定的,
//! 所以边一律用节点索引、而不是名字来指人.

use std::collections::HashMap;

/// 一组边:同一条地铁线 / 同一类依赖 / 同一条泳道.
/// 决定颜色,也决定"沿线走上一站下一站"的顺序.
#[derive(Clone, Debug)]
pub struct Group {
    pub label: String,
    pub color: (u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub group: usize,
    /// 有向边:在 b 那一端画个箭头
    pub directed: bool,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub label: String,
    /// 坐标:x 向右、y 向上.单位随意(只有相对关系有意义).
    /// None = 这一站没有坐标,布局得自己算
    pub pos: Option<(f64, f64)>,
    /// 搜索别名(拼音、缩写、英文名),按顺序试
    pub terms: Vec<String>,
}

impl Node {
    pub fn new(label: impl Into<String>, pos: Option<(f64, f64)>) -> Node {
        Node {
            label: label.into(),
            pos,
            terms: Vec::new(),
        }
    }
}

/// 没指定颜色时按这个顺序发
pub const PALETTE: [(u8, u8, u8); 12] = [
    (0x4c, 0x8e, 0xf8),
    (0xef, 0x4b, 0x4b),
    (0x22, 0xb4, 0x6a),
    (0xf5, 0x9e, 0x0b),
    (0xa8, 0x55, 0xf7),
    (0x14, 0xb8, 0xa6),
    (0xf4, 0x72, 0xb6),
    (0xa3, 0xe6, 0x35),
    (0xfb, 0x92, 0x3c),
    (0x60, 0xa5, 0xfa),
    (0x8d, 0x6e, 0x63),
    (0x78, 0x90, 0x9c),
];

/// 图的构造器:先把节点和分组建出来,拿到索引,再连边
pub struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    groups: Vec<Group>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        Builder {
            nodes: Vec::new(),
            edges: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// 加一个节点,返回它的索引
    pub fn node(&mut self, label: impl Into<String>, pos: Option<(f64, f64)>) -> usize {
        self.nodes.push(Node::new(label, pos));
        self.nodes.len() - 1
    }

    /// 补搜索别名(pinyin / 缩写 / 英文名)
    pub fn terms(&mut self, i: usize, terms: &[&str]) {
        self.nodes[i].terms = terms.iter().map(|s| s.to_string()).collect();
    }

    /// 找一个同名分组,没有就建一个.同名分组颜色以先建的为准
    pub fn group(&mut self, label: impl Into<String>, color: Option<(u8, u8, u8)>) -> usize {
        let label = label.into();
        if let Some(i) = self.groups.iter().position(|g| g.label == label) {
            return i;
        }
        let color = color.unwrap_or(PALETTE[self.groups.len() % PALETTE.len()]);
        self.groups.push(Group { label, color });
        self.groups.len() - 1
    }

    pub fn edge(&mut self, a: usize, b: usize, group: usize, directed: bool) {
        self.edges.push(Edge {
            a,
            b,
            group,
            directed,
        });
    }

    /// 收尾:校验、去掉重复边、算派生结构(邻接表、节点的分组、包围盒、每个分组的节点序列)
    pub fn build(mut self) -> Result<Graph, String> {
        if self.nodes.is_empty() {
            return Err("the graph has no nodes".to_string());
        }
        let n = self.nodes.len();
        for e in &self.edges {
            if e.a >= n || e.b >= n {
                return Err(format!("edge refers to a missing node {} -- {}", e.a, e.b));
            }
            if e.a == e.b {
                return Err(format!("self loops are not supported: node {}", e.a));
            }
            if e.group >= self.groups.len() {
                return Err(format!("edge refers to a missing group {}", e.group));
            }
        }
        if self.groups.is_empty() {
            self.groups.push(Group {
                label: String::new(),
                color: PALETTE[0],
            });
        }
        // 同一条边(同分组)只留一条.有向边按有序端点去重:a->b 与 b->a 是两条边
        // (有向图里那是个二元环),不能当成重复
        let mut seen: HashMap<(usize, usize, usize), ()> = HashMap::new();
        self.edges.retain(|e| {
            let (a, b) = if e.directed || e.a < e.b {
                (e.a, e.b)
            } else {
                (e.b, e.a)
            };
            seen.insert((a, b, e.group), ()).is_none()
        });

        let mut node_groups = vec![Vec::new(); n];
        let mut adj = vec![Vec::new(); n];
        for e in &self.edges {
            for (me, other) in [(e.a, e.b), (e.b, e.a)] {
                if !node_groups[me].contains(&e.group) {
                    node_groups[me].push(e.group);
                }
                adj[me].push((other, e.group));
            }
        }
        for v in node_groups.iter_mut() {
            v.sort_unstable();
        }

        let mut bbox = None;
        if self.nodes.iter().all(|nd| nd.pos.is_some()) {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in self.nodes.iter().filter_map(|nd| nd.pos) {
                x0 = x0.min(p.0);
                y0 = y0.min(p.1);
                x1 = x1.max(p.0);
                y1 = y1.max(p.1);
            }
            bbox = Some((x0, y0, x1, y1));
        }

        let mut graph = Graph {
            nodes: self.nodes,
            edges: self.edges,
            groups: self.groups,
            node_groups,
            adj,
            bbox,
            series: Vec::new(),
        };
        graph.series = graph.build_series();
        Ok(graph)
    }
}

/// 一张图.节点用索引指,边挂在分组上
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    /// 每个节点经过的分组(升序去重)
    pub node_groups: Vec<Vec<usize>>,
    /// 每个节点的邻居:(邻居, 分组)
    pub adj: Vec<Vec<(usize, usize)>>,
    /// 全部节点都有坐标时的包围盒 (x0, y0, x1, y1)
    pub bbox: Option<(f64, f64, f64, f64)>,
    /// 每个分组走一遍的节点序列(可能因为分叉而分成几条)
    series: Vec<(usize, Vec<usize>)>,
}

impl Graph {
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// 从 .dot 文本建图(见 dot.rs)
    pub fn from_dot(text: &str) -> Result<Graph, String> {
        crate::dot::parse(text)
    }

    /// 枢纽:经过两条及以上分组的节点(地铁的换乘站)
    pub fn hub(&self, i: usize) -> bool {
        self.node_groups[i].len() > 1
    }

    pub fn degree(&self, i: usize) -> usize {
        self.adj[i].len()
    }

    /// 每个节点都有坐标吗
    pub fn has_pos(&self) -> bool {
        self.bbox.is_some()
    }

    pub fn pos(&self, i: usize) -> (f64, f64) {
        self.nodes[i].pos.unwrap_or((0.0, 0.0))
    }

    pub fn group_color(&self, g: usize) -> (u8, u8, u8) {
        self.groups[g].color
    }

    /// 每个分组走一遍的节点序列:(分组号, 节点序列).
    /// 建图时算一次存下来:渲染每帧都要用(照着顺序走,拐弯处方向才接得上),
    /// 几百个节点的图每帧重算不划算
    pub fn series(&self) -> &[(usize, Vec<usize>)] {
        &self.series
    }

    /// 按分组把边串成路径.有分叉就分成几条;
    /// 有环的连通块没有度数 != 2 的起点,所以最后兜底从任意点起走(闭合成环)
    fn build_series(&self) -> Vec<(usize, Vec<usize>)> {
        let mut out = Vec::new();
        for g in 0..self.groups.len() {
            let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
            for e in self.edges.iter().filter(|e| e.group == g) {
                adj.entry(e.a).or_default().push(e.b);
                adj.entry(e.b).or_default().push(e.a);
            }
            let key = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
            let mut visited: std::collections::HashSet<(usize, usize)> =
                std::collections::HashSet::new();
            // 度 != 2 的节点是天然的路径端点,先当起点
            let mut starts: Vec<usize> = adj
                .iter()
                .filter(|(_, v)| v.len() != 2)
                .map(|(k, _)| *k)
                .collect();
            starts.sort_unstable();
            for &start in starts.iter().chain(adj.keys()) {
                for &nb in &adj[&start] {
                    if visited.contains(&key(start, nb)) {
                        continue;
                    }
                    let mut path = vec![start, nb];
                    visited.insert(key(start, nb));
                    let (mut prev, mut cur) = (start, nb);
                    while let Some(&next) = adj[&cur]
                        .iter()
                        .find(|&&n| n != prev && !visited.contains(&key(cur, n)))
                    {
                        visited.insert(key(cur, next));
                        path.push(next);
                        prev = cur;
                        cur = next;
                    }
                    if path.len() > 1 {
                        out.push((g, path));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("a", Some((0.0, 0.0)));
        let c = b.node("b", Some((1.0, 0.0)));
        let d = b.node("c", Some((2.0, 0.0)));
        let g = b.group("主线", Some((0xff, 0, 0)));
        b.edge(a, c, g, false);
        b.edge(c, d, g, false);
        b.build().unwrap()
    }

    #[test]
    fn 建图算出邻接与分组() {
        let g = sample();
        assert_eq!(g.adj[1].len(), 2);
        assert!(!g.hub(1));
        assert_eq!(g.series().len(), 1);
        assert_eq!(g.series()[0].1, vec![0, 1, 2]);
        assert_eq!(g.bbox, Some((0.0, 0.0, 2.0, 0.0)));
    }

    #[test]
    fn 缺坐标时没有包围盒() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", Some((1.0, 0.0)));
        let g = b.group("g", None);
        b.edge(a, c, g, false);
        let g = b.build().unwrap();
        assert!(!g.has_pos());
    }

    #[test]
    fn 自己连自己被拒() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let g = b.group("g", None);
        b.edge(a, a, g, false);
        assert!(b.build().is_err());
    }

    #[test]
    fn 重复边只留一条() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g = b.group("g", None);
        b.edge(a, c, g, false);
        b.edge(c, a, g, false);
        let g = b.build().unwrap();
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 反向的有向边是两条边() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g = b.group("g", None);
        b.edge(a, c, g, true);
        b.edge(c, a, g, true);
        let g = b.build().unwrap();
        assert_eq!(g.edges.len(), 2);
        // adj 是"每条边的两端各记一次",所以两条平行边在这一对节点上各记两次
        assert_eq!(g.adj[0].len(), 2);
        assert_eq!(g.adj[1].len(), 2);
    }

    #[test]
    fn 没给颜色的分组自动排在调色板上() {
        let mut b = Graph::builder();
        let a = b.node("a", None);
        let c = b.node("b", None);
        let g1 = b.group("g1", None);
        let g2 = b.group("g2", None);
        b.edge(a, c, g1, false);
        b.edge(a, c, g2, false);
        let g = b.build().unwrap();
        assert_eq!(g.groups[0].color, PALETTE[0]);
        assert_eq!(g.groups[1].color, PALETTE[1]);
        assert!(g.hub(0) && g.hub(1));
    }
}
