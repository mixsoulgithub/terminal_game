//! 端到端:examples 里的图在每一套布局下都画得出来,而且在足够大的画布上
//! 每个节点各占一格(这是布局与渲染衔接处最容易坏的地方:布局给的坐标尺度
//! 如果比画布小太多,投影取整会把两个节点并到同一格上).

use std::collections::HashSet;

use graph_tui::app::View;
use graph_tui::layout::{scene, Layout};
use graph_tui::model::Graph;
use graph_tui::render;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const W: u16 = 200;
const H: u16 = 50;

fn node_glyphs(buf: &Buffer, map: Rect) -> HashSet<(u16, u16)> {
    let mut out = HashSet::new();
    for y in map.y..map.y + map.height {
        for x in map.x..map.x + map.width {
            let sym = buf[(x, y)].symbol();
            if matches!(sym, "○" | "◎" | "●" | "◉") {
                out.insert((x, y));
            }
        }
    }
    out
}

fn rendered(path: &str, l: Layout) -> (Graph, Buffer, Rect) {
    let text = std::fs::read_to_string(path).unwrap();
    let g = Graph::from_dot(&text).unwrap();
    let sc = scene(&g, l).unwrap();
    let area = Rect::new(0, 0, W, H);
    let mut buf = Buffer::empty(area);
    let p = render::render(&mut buf, area, &g, &sc, &View::default());
    (g, buf, p.map)
}

#[test]
fn 每套布局每个节点各占一格() {
    let cases = [
        ("examples/metro.dot", Layout::Geo),
        ("examples/metro.dot", Layout::Relax),
        ("examples/metro.dot", Layout::Force),
        ("examples/deps.dot", Layout::Force),
        ("examples/deps.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Force),
    ];
    for (path, l) in cases {
        let (g, buf, map) = rendered(path, l);
        let cells = node_glyphs(&buf, map);
        assert_eq!(
            cells.len(),
            g.nodes.len(),
            "{path} 走 {} 布局时节点挤到 {} 格里(共 {} 个节点)",
            l.name(),
            cells.len(),
            g.nodes.len()
        );
    }
}

#[test]
fn 一帧里每一条边都画出了笔画() {
    for (path, l) in [
        ("examples/metro.dot", Layout::Geo),
        ("examples/deps.dot", Layout::Layered),
        ("examples/pipeline.dot", Layout::Layered),
    ] {
        let (_, buf, map) = rendered(path, l);
        let mut strokes = 0;
        for y in map.y..map.y + map.height {
            for x in map.x..map.x + map.width {
                if matches!(buf[(x, y)].symbol(), "─" | "│" | "╭" | "╮" | "╯" | "╰" | "├" | "┤" | "┬" | "┴" | "┼") {
                    strokes += 1;
                }
            }
        }
        assert!(strokes > 0, "{path} 走 {} 布局时一条边都没画出来", l.name());
    }
}

/// examples/limits 里全是故意为难引擎的图:高扇出、完全图、400 点网格、共线、
/// 坐标重合、离群点、平行边、孤立点、多个连通块、超长与中文标签、高扇入箭头。
/// 它们画出来好不好看是另一回事,但**不能炸**:四套布局 × 三种画布都得有输出。
#[test]
fn 极端输入不炸() {
    let mut files: Vec<String> = std::fs::read_dir("examples/limits")
        .unwrap()
        .map(|e| e.unwrap().path().to_string_lossy().to_string())
        .collect();
    files.sort();
    assert!(files.len() >= 16, "压力用例应该有十几张:{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let g = match Graph::from_dot(&text) {
            Ok(g) => g,
            // 自环这类引擎明确不收的输入,报错就算处理过了
            Err(_) => continue,
        };
        for l in Layout::all() {
            if l.needs_pos() && !g.has_pos() {
                continue;
            }
            let sc = scene(&g, l).unwrap();
            for (w, h) in [(200u16, 50u16), (40, 12), (8, 4)] {
                let area = Rect::new(0, 0, w, h);
                let mut buf = Buffer::empty(area);
                let p = render::render(&mut buf, area, &g, &sc, &View::default());
                // 画面再小也得认得出这是一张图,而不是一片空白或越界
                assert!(p.map.width <= w && p.map.height <= h, "{path} 地图区越界了");
                if (w, h) == (200, 50) {
                    let cells = node_glyphs(&buf, p.map);
                    assert!(!cells.is_empty(), "{path} 走 {} 布局时一个节点都没画", l.name());
                }
            }
        }
    }
}

/// examples/real 里的图都是真实数据转的(模块依赖、Cargo.lock、数据库外键、
/// systemd 单元、TCP 状态机):它们该和手写的小图一样,在够大的画布上每个节点各占一格。
#[test]
fn 真实数据的图也画得下() {
    let mut files: Vec<String> = std::fs::read_dir("examples/real")
        .unwrap()
        .map(|e| e.unwrap().path().to_string_lossy().to_string())
        .collect();
    files.sort();
    assert!(files.len() >= 5, "真实用例应该有五张:{files:?}");
    for path in files {
        for l in Layout::all() {
            if l.needs_pos() && !Graph::from_dot(&std::fs::read_to_string(&path).unwrap()).unwrap().has_pos() {
                continue;
            }
            let (g, buf, map) = rendered(&path, l);
            let cells = node_glyphs(&buf, map);
            assert_eq!(
                cells.len(),
                g.nodes.len(),
                "{path} 走 {} 布局时节点挤到 {} 格里(共 {} 个)",
                l.name(),
                cells.len(),
                g.nodes.len()
            );
        }
    }
}
