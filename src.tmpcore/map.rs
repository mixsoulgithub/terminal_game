// 地图:第一到第三章都是 7 列 15 层,6 条从下往上爬的路径 + 顶上单独一个
// Boss 节点;第四章是第 3 列一条四层的竖线(休息点 → 商店 → 精英 → 心脏).
//
// 生成方式照抄参考实现(refs/slay-the-cli/src/engine/run/mapGen.ts,也就是原作
// Map.cpp 的移植):先铺 6 条随机路径,再按每行的房间预算贴房间类型.
// 两处"原作的怪毛病"也照抄,因为它们就是原作的行为:
//   - 第 13 行只算"未分配"却不算进房间预算的 total(于是休息点比例少算一行),
//   - getCommonAncestor 里那个 `x1 < y` 的比较(反编译原样).
// 掷点全部来自 registry 的 mapRng:各章按 act 重种(第一章 seed + 1,
// 第二章 seed + 200,第三章 seed + 600),而"标不标燃烧精英"由 set_burning 决定.
use crate::rng::Rng;

/// 普通层数(0..15);第 15 层是 Boss 前的休息点
pub const FLOORS: usize = 15;
/// 地图列数
pub const COLS: usize = 7;
/// 路径条数
pub const PATHS: usize = 6;
/// 最右边那一列的下标
const ROW_END_NODE: i32 = COLS as i32 - 1;
/// Boss 挂在第几列(参考实现里写死的 3)
const BOSS_COL: usize = 3;

const SHOP_ROOM_CHANCE: f64 = 0.05;
const REST_ROOM_CHANCE: f64 = 0.12;
const TREASURE_ROOM_CHANCE: f64 = 0.0;
const EVENT_ROOM_CHANCE: f64 = 0.22;
const ELITE_ROOM_CHANCE: f64 = 0.08;
/// 飞升 1+:精英多约 60%(参考实现 ELITE_ROOM_CHANCE_A1 = A0 * 1.6)
const ELITE_ROOM_CHANCE_A1: f64 = 0.128;

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
            // 普通怪与精英都是 E,精英靠红底区分
            NodeKind::Monster => 'e',
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

    fn from_room(room: Room) -> NodeKind {
        match room {
            Room::Monster => NodeKind::Monster,
            Room::Elite => NodeKind::Elite,
            Room::Event => NodeKind::Event,
            Room::Rest => NodeKind::Rest,
            Room::Shop => NodeKind::Shop,
            Room::Treasure => NodeKind::Treasure,
        }
    }
}

#[derive(Debug)]
pub struct Node {
    pub floor: usize,
    pub col: usize,
    pub kind: NodeKind,
    /// 这个精英是不是"燃烧精英":打通给绿钥匙,开局带一个增益
    pub burning: bool,
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
    /// 燃烧精英的增益编号(0..=3);没有燃烧精英时是 -1
    pub burning_buff: i32,
}

impl ActMap {
    pub fn node(&self, i: usize) -> &Node {
        &self.nodes[i]
    }

    /// 燃烧精英的节点下标(没有就是 None)
    pub fn burning_node(&self) -> Option<usize> {
        self.nodes.iter().position(|n| n.burning)
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

    /// 从某个节点出发往前能走到的所有节点(含它自己).
    /// 地图界面用它来画"选了这间房之后的整片未来",所以含 Boss 那一层.
    pub fn forward_reachable(&self, from: usize) -> Vec<bool> {
        let mut seen = vec![false; self.nodes.len()];
        if from >= self.nodes.len() {
            return seen;
        }
        let mut stack = vec![from];
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            for nxt in &self.nodes[i].next {
                stack.push(*nxt);
            }
        }
        seen
    }

    /// 生成地图.掷点从 rng(registry 的 mapRng)来.
    /// `set_burning` 决定要不要标一个燃烧精英:第一章一定标,二、三章只有
    /// 绿钥匙还没到手时才标(参考实现 generateActMapFor 的 setBurning).
    /// `asc` 决定精英出现频率(飞升 1+ 按参考实现乘 1.6).
    pub fn generate(rng: &mut Rng, set_burning: bool, asc: u32) -> ActMap {
        let mut g = Grid::new();
        create_paths(&mut g, rng);
        filter_redundant_edges_from_first_row(&mut g);
        assign_rooms(&mut g, rng, asc);

        // 燃烧精英:从所有精英里随机挑一个(参考实现的 assignBurningElite),
        // 再掷一个增益编号(0..=3).标的时候两掷都消耗,不标就一掷不掷.
        let elites = g.elites();
        let mut burning = None;
        let mut buff = -1;
        if set_burning && !elites.is_empty() {
            let idx = rng.random(elites.len() as u32 - 1) as usize;
            burning = Some(elites[idx]);
            buff = rng.random_range(0, 3);
        }

        g.into_act_map(burning, buff)
    }

    /// 第四章的固定地图:第 3 列一条竖线,休息点 → 商店 → 精英 → 心脏
    /// (参考实现 act4Map).这一章只有 4 层,靠 boss 节点的楼层算总层数.
    pub fn act4() -> ActMap {
        let kinds = [
            NodeKind::Rest,
            NodeKind::Shop,
            NodeKind::Elite,
            NodeKind::Boss,
        ];
        let mut nodes: Vec<Node> = Vec::new();
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); FLOORS + 1];
        for (floor, kind) in kinds.iter().enumerate() {
            let i = nodes.len();
            nodes.push(Node {
                floor,
                col: BOSS_COL,
                kind: *kind,
                burning: false,
                prev: if floor == 0 { Vec::new() } else { vec![i - 1] },
                next: if floor + 1 < kinds.len() {
                    vec![i + 1]
                } else {
                    Vec::new()
                },
            });
            rows[floor].push(i);
        }
        let boss = kinds.len() - 1;
        ActMap {
            nodes,
            rows,
            boss,
            burning_buff: -1,
        }
    }

    /// 按参考实现的 mapToString 排版,给金标准测试逐格对比用
    #[cfg(test)]
    pub fn to_rows_string(&self) -> String {
        let present: Vec<Vec<bool>> = (0..FLOORS)
            .map(|y| {
                (0..COLS)
                    .map(|x| self.rows[y].iter().any(|i| self.nodes[*i].col == x))
                    .collect()
            })
            .collect();
        let mut lines: Vec<String> = Vec::new();
        for y in 0..FLOORS {
            let mut line = String::new();
            for x in 0..COLS {
                if present[y][x] {
                    let node = self
                        .rows[y]
                        .iter()
                        .map(|i| &self.nodes[*i])
                        .find(|n| n.col == x)
                        .expect("present 的格子有节点");
                    // 参考实现的排版把普通怪写成 M(本作界面上用 e,这里只为对比)
                    line.push(if node.kind == NodeKind::Monster {
                        'M'
                    } else {
                        node.kind.sigil()
                    });
                    let mut edges: Vec<usize> = node
                        .next
                        .iter()
                        .map(|i| self.nodes[*i].col)
                        .collect();
                    edges.sort_unstable();
                    let e = edges
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    line.push_str(&format!("{e:<6}"));
                } else {
                    line.push(' ');
                    line.push_str(&format!("{:<6}", ""));
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}

// ---- 参考实现里的那张网格 ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Room {
    Monster,
    Elite,
    Event,
    Rest,
    Shop,
    Treasure,
}

#[derive(Clone)]
struct GNode {
    room: Option<Room>,
    /// 通向下一行哪些列(升序去重)
    edges: Vec<usize>,
    /// 上一行哪些列通向它(可能重复)
    parents: Vec<usize>,
}

struct Grid {
    /// [行][列]
    n: Vec<Vec<GNode>>,
}

impl Grid {
    fn new() -> Grid {
        Grid {
            n: (0..FLOORS)
                .map(|_| {
                    (0..COLS)
                        .map(|_| GNode {
                            room: None,
                            edges: Vec::new(),
                            parents: Vec::new(),
                        })
                        .collect()
                })
                .collect(),
        }
    }

    fn add_edge(&mut self, y: usize, x: usize, edge: usize) {
        let e = &mut self.n[y][x].edges;
        if let Err(i) = e.binary_search(&edge) {
            e.insert(i, edge);
        }
    }

    fn elites(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (y, row) in self.n.iter().enumerate() {
            for (x, node) in row.iter().enumerate() {
                if node.room == Some(Room::Elite) {
                    out.push((x, y));
                }
            }
        }
        out
    }

    /// 把网格变成引擎用的 ActMap:存在的节点拼成下标,边接成 prev/next,
    /// 顶上再挂一个 Boss 节点(第 14 行的路径终点都通向它).
    /// burning 是燃烧精英所在格 (列, 行),buff 是它的增益编号.
    fn into_act_map(self, burning: Option<(usize, usize)>, buff: i32) -> ActMap {
        let present = |y: usize, x: usize| -> bool {
            if y == FLOORS - 1 {
                self.n[FLOORS - 2].iter().any(|n| n.edges.contains(&x))
            } else {
                !self.n[y][x].edges.is_empty()
            }
        };
        // 行优先编号,同一行里下标顺序就是列顺序
        let mut index: Vec<Vec<Option<usize>>> = vec![vec![None; COLS]; FLOORS];
        let mut nodes: Vec<Node> = Vec::new();
        for y in 0..FLOORS {
            for x in 0..COLS {
                if !present(y, x) {
                    continue;
                }
                let kind = self.n[y][x]
                    .room
                    .map(NodeKind::from_room)
                    .unwrap_or(NodeKind::Monster);
                index[y][x] = Some(nodes.len());
                nodes.push(Node {
                    floor: y,
                    col: x,
                    kind,
                    burning: burning == Some((x, y)),
                    prev: Vec::new(),
                    next: Vec::new(),
                });
            }
        }
        let boss = nodes.len();
        nodes.push(Node {
            floor: FLOORS,
            col: BOSS_COL,
            kind: NodeKind::Boss,
            burning: false,
            prev: Vec::new(),
            next: Vec::new(),
        });
        for y in 0..FLOORS {
            for x in 0..COLS {
                let Some(from) = index[y][x] else { continue };
                for edge in self.n[y][x].edges.clone() {
                    let to = if y == FLOORS - 1 {
                        Some(boss)
                    } else {
                        index[y + 1][edge]
                    };
                    let Some(to) = to else { continue };
                    if !nodes[from].next.contains(&to) {
                        nodes[from].next.push(to);
                    }
                    if !nodes[to].prev.contains(&from) {
                        nodes[to].prev.push(from);
                    }
                }
            }
        }
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); FLOORS + 1];
        for (i, n) in nodes.iter().enumerate() {
            rows[n.floor].push(i);
        }
        for r in rows.iter_mut() {
            r.sort_by_key(|i| nodes[*i].col);
        }
        ActMap {
            nodes,
            rows,
            boss,
            burning_buff: buff,
        }
    }
}

/// 参考实现的 randRange:random(max - min) + min
fn rand_range(rng: &mut Rng, min: i32, max: i32) -> i32 {
    rng.random((max - min) as u32) as i32 + min
}

fn max_edge(n: &GNode) -> usize {
    *n.edges.last().expect("有边才有最大值")
}

fn min_edge(n: &GNode) -> usize {
    n.edges[0]
}

/// 反编译原样保留了 `x1 < y` 这个比较(不是 x1 < x2),`y` 就是当前行号
fn get_common_ancestor(g: &Grid, x1: usize, x2: usize, y: usize) -> Option<usize> {
    let (l, r) = if (x1 as i32) < y as i32 { (x1, x2) } else { (x2, x1) };
    if g.n[y][l].parents.is_empty() || g.n[y][r].parents.is_empty() {
        return None;
    }
    let left_x = *g.n[y][l].parents.iter().max().unwrap();
    let right_min = *g.n[y][r].parents.iter().min().unwrap();
    if left_x == right_min {
        Some(left_x)
    } else {
        None
    }
}

fn choose_path_parent_loop_randomizer(
    g: &Grid,
    rng: &mut Rng,
    cur_x: i32,
    cur_y: usize,
    new_x: i32,
) -> i32 {
    let mut new_x = new_x;
    let parents = g.n[cur_y + 1][new_x as usize].parents.clone();
    for parent_x in parents {
        if cur_x == parent_x as i32 {
            continue;
        }
        if get_common_ancestor(g, parent_x, cur_x as usize, cur_y).is_none() {
            continue;
        }
        if new_x > cur_x {
            new_x = cur_x + rand_range(rng, -1, 0);
            if new_x < 0 {
                new_x = cur_x;
            }
        } else if new_x == cur_x {
            new_x = cur_x + rand_range(rng, -1, 1);
            if new_x > ROW_END_NODE {
                new_x = cur_x - 1;
            } else if new_x < 0 {
                new_x = cur_x + 1;
            }
        } else {
            new_x = cur_x + rand_range(rng, 0, 1);
            if new_x > ROW_END_NODE {
                new_x = cur_x;
            }
        }
    }
    new_x
}

fn choose_path_adjust_new_x(g: &Grid, cur_x: usize, cur_y: usize, new_edge_x: i32) -> i32 {
    let mut new_edge_x = new_edge_x;
    if cur_x != 0 && !g.n[cur_y][cur_x - 1].edges.is_empty() {
        let e = max_edge(&g.n[cur_y][cur_x - 1]) as i32;
        if e > new_edge_x {
            new_edge_x = e;
        }
    }
    if cur_x < COLS - 1 && !g.n[cur_y][cur_x + 1].edges.is_empty() {
        let e = min_edge(&g.n[cur_y][cur_x + 1]) as i32;
        if e < new_edge_x {
            new_edge_x = e;
        }
    }
    new_edge_x
}

fn choose_new_path(g: &Grid, rng: &mut Rng, cur_x: usize, cur_y: usize) -> usize {
    let (min, max) = if cur_x == 0 {
        (0, 1)
    } else if cur_x == COLS - 1 {
        (-1, 0)
    } else {
        (-1, 1)
    };
    let new_edge_x = cur_x as i32 + rand_range(rng, min, max);
    let new_edge_x = choose_path_parent_loop_randomizer(g, rng, cur_x as i32, cur_y, new_edge_x);
    choose_path_adjust_new_x(g, cur_x, cur_y, new_edge_x).max(0) as usize
}

fn create_paths_iteration(g: &mut Grid, rng: &mut Rng, start_x: usize) {
    let mut cur_x = start_x;
    for cur_y in 0..FLOORS - 1 {
        let new_x = choose_new_path(g, rng, cur_x, cur_y);
        g.add_edge(cur_y, cur_x, new_x);
        g.n[cur_y + 1][new_x].parents.push(cur_x);
        cur_x = new_x;
    }
    // 每条路径的终点都连到最上层的 Boss 列
    g.add_edge(FLOORS - 1, cur_x, BOSS_COL);
}

fn create_paths(g: &mut Grid, rng: &mut Rng) {
    let first_start_x = rand_range(rng, 0, COLS as i32 - 1) as usize;
    create_paths_iteration(g, rng, first_start_x);
    for i in 1..PATHS {
        let mut start_x = rand_range(rng, 0, COLS as i32 - 1) as usize;
        while start_x == first_start_x && i == 1 {
            start_x = rand_range(rng, 0, COLS as i32 - 1) as usize;
        }
        create_paths_iteration(g, rng, start_x);
    }
}

fn filter_redundant_edges_from_first_row(g: &mut Grid) {
    let mut visited = [false; COLS];
    for src_x in 0..COLS {
        let mut i = g.n[0][src_x].edges.len();
        while i > 0 {
            i -= 1;
            let dest_x = g.n[0][src_x].edges[i];
            if visited[dest_x] {
                g.n[1][dest_x].parents.retain(|p| *p != src_x);
                g.n[0][src_x].edges.remove(i);
            } else {
                visited[dest_x] = true;
            }
        }
    }
}

// ---- 贴房间类型 ----

struct RoomCounts {
    total: i64,
    unassigned: i64,
}

fn get_room_counts_and_assign_fixed(g: &mut Grid) -> RoomCounts {
    let mut counts = RoomCounts {
        total: 0,
        unassigned: 0,
    };
    for row in 0..FLOORS {
        for x in 0..COLS {
            if g.n[row][x].edges.is_empty() {
                continue;
            }
            let fixed = if row == 0 {
                Some(Room::Monster)
            } else if row == 8 {
                Some(Room::Treasure)
            } else if row == FLOORS - 1 {
                Some(Room::Rest)
            } else {
                None
            };
            match fixed {
                Some(r) => {
                    g.n[row][x].room = Some(r);
                    counts.total += 1;
                }
                None => {
                    counts.unassigned += 1;
                    // 第 13 行的怪毛病:算未分配,但不进 total
                    if row != FLOORS - 2 {
                        counts.total += 1;
                    }
                }
            }
        }
    }
    counts
}

fn fill_room_array(counts: &RoomCounts, elite_chance: f64) -> Vec<Room> {
    let mut arr = vec![Room::Monster; counts.unassigned as usize];
    let shop_count = (counts.total as f64 * SHOP_ROOM_CHANCE).round() as usize;
    let rest_count = (counts.total as f64 * REST_ROOM_CHANCE).round() as usize;
    let treasure_count = (counts.total as f64 * TREASURE_ROOM_CHANCE).round() as usize;
    let elite_count = (counts.total as f64 * elite_chance).round() as usize;
    let event_count = (counts.total as f64 * EVENT_ROOM_CHANCE).round() as usize;

    let mut i = 0usize;
    let put = |arr: &mut Vec<Room>, room: Room, n: usize, i: &mut usize| {
        for _ in 0..n {
            if *i >= arr.len() {
                break;
            }
            arr[*i] = room;
            *i += 1;
        }
    };
    put(&mut arr, Room::Shop, shop_count, &mut i);
    put(&mut arr, Room::Rest, rest_count, &mut i);
    put(&mut arr, Room::Treasure, treasure_count, &mut i);
    put(&mut arr, Room::Elite, elite_count, &mut i);
    put(&mut arr, Room::Event, event_count, &mut i);
    arr
}

/// 同行/上下行之间那几张"谁挨着谁"的表:列号集合用升序去重的 Vec 表示
struct RoomAssignData {
    offset: usize,
    row_rooms: [Option<Room>; COLS],
    prev_row_rooms: [Option<Room>; COLS],
    sibling_cols: [Vec<usize>; COLS],
    next_sibling_cols: [Vec<usize>; COLS],
    parent_cols: [Vec<usize>; COLS],
    next_parent_cols: [Vec<usize>; COLS],
    rooms: Vec<Room>,
}

fn empty_cols() -> [Vec<usize>; COLS] {
    std::array::from_fn(|_| Vec::new())
}

fn add_to(set: &mut Vec<usize>, v: usize) {
    if let Err(i) = set.binary_search(&v) {
        set.insert(i, v);
    }
}

impl RoomAssignData {
    fn new(rooms: Vec<Room>) -> Self {
        RoomAssignData {
            offset: 0,
            row_rooms: [None; COLS],
            prev_row_rooms: [None; COLS],
            sibling_cols: empty_cols(),
            next_sibling_cols: empty_cols(),
            parent_cols: empty_cols(),
            next_parent_cols: empty_cols(),
            rooms,
        }
    }

    fn set_data(&mut self, x: usize, node: &GNode) {
        if node.edges.len() == 1 {
            add_to(&mut self.next_parent_cols[node.edges[0]], x);
            return;
        }
        let mut sibling_mask: Vec<usize> = Vec::new();
        for edge in &node.edges {
            add_to(&mut sibling_mask, *edge);
            for s in sibling_mask.clone() {
                add_to(&mut self.next_sibling_cols[*edge], s);
            }
            add_to(&mut self.next_parent_cols[*edge], x);
        }
    }

    fn set_cur_data_only(&mut self, x: usize, node: &GNode) {
        self.row_rooms[x] = node.room;
    }

    fn remove_element(&mut self, idx: usize) {
        let mut i = idx;
        while i > self.offset {
            self.rooms[i] = self.rooms[i - 1];
            i -= 1;
        }
        self.offset += 1;
    }

    fn next_row(&mut self) {
        self.prev_row_rooms = self.row_rooms;
        self.row_rooms = [None; COLS];
        self.sibling_cols = std::mem::replace(&mut self.next_sibling_cols, empty_cols());
        self.parent_cols = std::mem::replace(&mut self.next_parent_cols, empty_cols());
    }

    fn sibling_match(&self, x: usize, room: Room) -> bool {
        self.sibling_cols[x]
            .iter()
            .any(|s| self.row_rooms[*s] == Some(room))
    }

    fn parent_match(&self, x: usize, room: Room) -> bool {
        self.parent_cols[x]
            .iter()
            .any(|p| self.prev_row_rooms[*p] == Some(room))
    }
}

fn assign_room_to_node(g: &mut Grid, y: usize, x: usize, data: &mut RoomAssignData) {
    let mut tried: Vec<Room> = Vec::new();
    let mut i = data.offset;
    while i < data.rooms.len() {
        let room = data.rooms[i];
        if tried.contains(&room) {
            i += 1;
            continue;
        }
        tried.push(room);

        let skip = (room == Room::Elite && y <= 4) || (room == Room::Rest && (y <= 4 || y >= 13));
        if skip {
            i += 1;
            continue;
        }

        if room == Room::Event || room == Room::Monster {
            if data.sibling_match(x, room) {
                i += 1;
                continue;
            }
            g.n[y][x].room = Some(room);
            data.row_rooms[x] = Some(room);
            data.remove_element(i);
            return;
        }

        if !data.parent_match(x, room) && !data.sibling_match(x, room) {
            g.n[y][x].room = Some(room);
            data.row_rooms[x] = Some(room);
            data.remove_element(i);
            return;
        }
        i += 1;
    }
    g.n[y][x].room = Some(Room::Monster); // 兜底:不消耗队列
}

fn assign_rooms(g: &mut Grid, rng: &mut Rng, asc: u32) {
    let counts = get_room_counts_and_assign_fixed(g);
    let elite_chance = if asc > 0 {
        ELITE_ROOM_CHANCE_A1
    } else {
        ELITE_ROOM_CHANCE
    };
    let mut rooms = fill_room_array(&counts, elite_chance);

    // 用不计数器的原始掷点原地洗牌,和参考实现一样
    let mut i = counts.unassigned as usize;
    while i > 1 {
        let j = rng.next_int_raw(i as u32) as usize;
        rooms.swap(i - 1, j);
        i -= 1;
    }

    let mut data = RoomAssignData::new(rooms);
    for row in 0..FLOORS - 1 {
        for x in 0..COLS {
            if g.n[row][x].edges.is_empty() {
                continue;
            }
            let node = g.n[row][x].clone();
            if row == 0 || row == 8 {
                data.set_data(x, &node);
            } else if row == 7 || row == FLOORS - 2 {
                assign_room_to_node(g, row, x, &mut data);
                data.set_cur_data_only(x, &g.n[row][x]);
            } else {
                assign_room_to_node(g, row, x, &mut data);
                data.set_data(x, &g.n[row][x]);
            }
        }
        data.next_row();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(seed: u64) -> ActMap {
        ActMap::generate(&mut Rng::new(seed), true, 0)
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

    /// 参考实现的落座规矩:精英不在前 5 行,休息点/商店不在前 5 行也不在第 13 行,
    /// 商店/精英/休息点还要求父节点不同型
    #[test]
    fn room_placement_rules_hold() {
        for seed in 0..40 {
            let m = map(seed);
            for n in &m.nodes {
                match n.kind {
                    NodeKind::Elite => assert!(
                        n.floor >= 5,
                        "seed {seed}: 精英不该出现在第 {} 层",
                        n.floor + 1
                    ),
                    NodeKind::Rest => {
                        // 最后一层是定死的休息点,其余休息点只在第 6..13 层
                        assert!(
                            n.floor == FLOORS - 1 || (5..=12).contains(&n.floor),
                            "seed {seed}: 休息点落在第 {} 层",
                            n.floor + 1
                        );
                        for p in &n.prev {
                            assert_ne!(
                                m.node(*p).kind,
                                n.kind,
                                "seed {seed}: 第 {} 层休息点和上层同型",
                                n.floor + 1
                            );
                        }
                    }
                    // 商店只看"上层不同型、同行不挨着",没有层数限制
                    NodeKind::Shop => {
                        for p in &n.prev {
                            assert_ne!(
                                m.node(*p).kind,
                                n.kind,
                                "seed {seed}: 第 {} 层商店和上层同型",
                                n.floor + 1
                            );
                        }
                    }
                    NodeKind::Boss => assert_eq!(n.floor, FLOORS),
                    _ => {}
                }
                // 同一个父节点的两个子节点不许同型(第 7、13 行往下是固定层,不在此列)
                if n.floor != 7 && n.floor != FLOORS - 2 {
                    for (i, a) in n.next.iter().enumerate() {
                        for b in n.next.iter().skip(i + 1) {
                            let (ka, kb) = (m.node(*a).kind, m.node(*b).kind);
                            // 怪物不占房间配额,两个同类怪是可以的(兜底那一步)
                            assert!(
                                ka == NodeKind::Monster || ka != kb,
                                "seed {seed}: 第 {} 层同一个岔口分出两个同类房间",
                                n.floor + 2
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn forward_reachable_covers_the_rest_of_the_road() {
        for seed in 0..10 {
            let m = map(seed);
            for &start in m.row(0) {
                let fwd = m.forward_reachable(start);
                assert!(fwd[start], "起点自己要算进去");
                assert!(fwd[m.boss], "往前一定包含 Boss");
                for (i, ok) in fwd.iter().enumerate() {
                    if *ok || i == m.boss {
                        continue;
                    }
                    for p in &m.nodes[i].prev {
                        assert!(!fwd[*p], "前向闭包漏了 {i}");
                    }
                }
            }
            let row0 = m.row(0);
            if row0.len() > 1 {
                let a = m.forward_reachable(row0[0]);
                let b = m.forward_reachable(row0[row0.len() - 1]);
                assert_ne!(a, b, "不同岔路的未来应该不同");
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
        // 同层节点按列排序,且两节点往下一层的出口列不会交叉
        for seed in 0..20 {
            let m = map(seed);
            for f in 0..FLOORS {
                let row = m.row(f);
                for w in row.windows(2) {
                    let (a, b) = (m.node(w[0]), m.node(w[1]));
                    assert!(a.col < b.col, "同层节点应按列排序");
                    for an in &a.next {
                        for bn in &b.next {
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
