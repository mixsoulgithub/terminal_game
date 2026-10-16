//! .dot 子集解析(通用图,不再是地铁专用).
//!
//! 认这几种写法,其余一律忽略:
//!   strict? graph|digraph 名字 { ... }      图的类型(digraph -> 边默认有箭头)
//!   node [k=v, ...];  /  edge [k=v, ...];  默认属性(只对后面出现的元素生效)
//!   "甲" [pos="x,y", label="显示名", terms="a|b"];      节点
//!   "甲" -- "乙" [color="#rrggbb", group="主线"];        无向边
//!   "甲" -> "乙" [color="#rrggbb", group="主线"];        有向边
//!   "甲" <- "乙" [..];                                    有向边,方向反过来
//!   "甲" "乙" [..];                                       没写箭头,方向由图类型定
//! 值都是 utf-8 字符串,不处理转义.子图(subgraph)当透明容器:里面的节点与边照收.
//! 边端点没声明过就隐式建一个节点(graphviz 的语义),所以 `"a" -> "b";` 一句就够;
//! 节点顺序永远是首次出现的顺序,名字大小写敏感,一个名字只对应一个节点.
//!
//! 分两趟:先把节点表建出来(边端点也算一次出现),再连边;边用名字写,
//! 靠这张表换成 Builder 的索引.

use std::collections::HashMap;

use crate::model::Graph;

/// 属性表:保持出现顺序,同名键以最后一次为准
#[derive(Clone, Debug, Default)]
struct Attrs {
    kv: Vec<(String, String)>,
}

impl Attrs {
    fn get(&self, key: &str) -> Option<&str> {
        self.kv.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.kv.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.kv.push((key.to_string(), value.to_string())),
        }
    }

    /// 只补自己缺的键:节点/边自己的属性优先于默认属性
    fn fill_from(&mut self, other: &Attrs) {
        for (k, v) in &other.kv {
            if self.get(k).is_none() {
                self.kv.push((k.clone(), v.clone()));
            }
        }
    }
}

/// 解析阶段的节点:label 缺省就拿名字当显示名
struct RawNode {
    name: String,
    label: Option<String>,
    pos: Option<(f64, f64)>,
    /// 搜索别名(不含 name)
    terms: Vec<String>,
}

/// 解析阶段的边:两端是节点下标
struct RawEdge {
    a: usize,
    b: usize,
    directed: bool,
    color: Option<(u8, u8, u8)>,
    group: String,
}

pub fn parse(text: &str) -> Result<Graph, String> {
    let stmts = statements(text);
    let mut digraph = false;
    let mut node_def = Attrs::default();
    let mut edge_def = Attrs::default();
    let mut nodes: Vec<RawNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<RawEdge> = Vec::new();

    for (n, stmt) in stmts.iter().enumerate() {
        let tok = tokens(stmt);
        if tok.is_empty() {
            continue;
        }
        // 第一条语句里的 strict / graph / digraph 决定整张图的类型
        if n == 0 && matches!(tok[0].as_str(), "strict" | "graph" | "digraph") {
            digraph = tok.iter().any(|t| t == "digraph");
            continue;
        }
        match tok[0].as_str() {
            // 子图是透明容器:只吃掉这一句,里面的节点与边由后面的语句照常收,
            // 所以不会凭空多出一个叫 subgraph 的节点
            "subgraph" | "strict" => continue,
            // 默认属性:后面出现的节点/边缺哪个键就从这里补
            "node" | "edge" => {
                let attrs = parse_attrs(&tok, 1);
                if tok[0] == "node" {
                    node_def = attrs;
                } else {
                    edge_def = attrs;
                }
                continue;
            }
            // graph [..] 是图级属性,图模型里没有对应字段,收下就丢
            _ if tok[0] == "graph" => continue,
            // 零碎的标点不是节点
            _ if matches!(tok[0].as_str(), "[" | "]" | "=" | "--" | "->" | "<-") => continue,
            // 图级属性赋值语句(rankdir=LR 之类)也不是节点
            _ if tok.get(1).map(String::as_str) == Some("=") => continue,
            _ => {}
        }
        // 边:中间那个词是箭头写法
        if let Some((a, b, directed, at)) = edge_parts(&tok, digraph) {
            let mut attrs = parse_attrs(&tok, at);
            attrs.fill_from(&edge_def);
            // group 与 line 都算分组名,两个都有就用 group
            let group = attrs
                .get("group")
                .filter(|s| !s.is_empty())
                .or_else(|| attrs.get("line").filter(|s| !s.is_empty()))
                .unwrap_or("");
            // 端点没声明过就按 `"名字";` 隐式建一个,顺序仍是首次出现的顺序
            let a = touch(&mut nodes, &mut index, &a, &node_def);
            let b = touch(&mut nodes, &mut index, &b, &node_def);
            edges.push(RawEdge {
                a,
                b,
                directed,
                color: color_of(&attrs),
                group: group.to_string(),
            });
            continue;
        }
        // 节点:名字 + 可选属性
        let mut attrs = parse_attrs(&tok, 1);
        attrs.fill_from(&node_def);
        merge_node(&mut nodes, &mut index, raw_of(&tok[0], &attrs));
    }

    // 节点按下标顺序进图,再连边
    let mut b = Graph::builder();
    for node in &nodes {
        let label = node.label.clone().unwrap_or_else(|| node.name.clone());
        let i = b.node(label, node.pos);
        let mut terms: Vec<&str> = Vec::new();
        // 显示名换成 label 之后,原来的节点名仍然能搜到
        if let Some(l) = &node.label {
            if l != &node.name {
                terms.push(&node.name);
            }
        }
        for t in &node.terms {
            if !terms.contains(&t.as_str()) {
                terms.push(t);
            }
        }
        if !terms.is_empty() {
            b.terms(i, &terms);
        }
    }
    for e in &edges {
        let g = b.group(&e.group, e.color);
        b.edge(e.a, e.b, g, e.directed);
    }
    b.build()
}

/// 记下这个名字,返回节点下标:没声明过就按 `"名字";` 隐式建一个,
/// 所以隐式节点和显式声明一样吃当时的 node 默认属性
fn touch(
    nodes: &mut Vec<RawNode>,
    index: &mut HashMap<String, usize>,
    name: &str,
    def: &Attrs,
) -> usize {
    let mut attrs = Attrs::default();
    attrs.fill_from(def);
    merge_node(nodes, index, raw_of(name, &attrs))
}

/// 同名节点只留一个:后一份定义只补前面缺的键
fn merge_node(
    nodes: &mut Vec<RawNode>,
    index: &mut HashMap<String, usize>,
    fresh: RawNode,
) -> usize {
    match index.get(&fresh.name) {
        Some(&i) => {
            let old = &mut nodes[i];
            if old.pos.is_none() {
                old.pos = fresh.pos;
            }
            if old.label.is_none() {
                old.label = fresh.label;
            }
            for t in fresh.terms {
                if !old.terms.contains(&t) {
                    old.terms.push(t);
                }
            }
            i
        }
        None => {
            index.insert(fresh.name.clone(), nodes.len());
            nodes.push(fresh);
            nodes.len() - 1
        }
    }
}

/// 从属性表里取出名字以外的东西
fn raw_of(name: &str, attrs: &Attrs) -> RawNode {
    let mut terms = Vec::new();
    for key in ["terms", "pinyin", "pinyin2"] {
        split_terms(attrs.get(key), &mut terms);
    }
    RawNode {
        name: name.to_string(),
        label: attrs
            .get("label")
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        pos: attrs.get("pos").and_then(parse_pos),
        terms,
    }
}

/// 边语句拆成 (起点, 终点, 有向, 属性表在分词里的下标);不是边就返回 None
fn edge_parts(tok: &[String], digraph: bool) -> Option<(String, String, bool, usize)> {
    let (a, b, directed, at) = match tok.get(1).map(String::as_str) {
        // "--" 永远无向,"->" 永远有向,"<-" 是从右边指向左边
        Some("--") => (tok[0].clone(), tok.get(2).cloned().unwrap_or_default(), false, 3),
        Some("->") => (tok[0].clone(), tok.get(2).cloned().unwrap_or_default(), true, 3),
        Some("<-") => (tok.get(2).cloned().unwrap_or_default(), tok[0].clone(), true, 3),
        // 只有两个名字,没写箭头:方向由图类型定
        Some(t) if t != "[" && t != "=" => (tok[0].clone(), tok[1].clone(), digraph, 2),
        _ => return None,
    };
    // 端点缺了(比如 `"甲" --;`)当这句没写全,忽略而不是造一个没名字的节点
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a, b, directed, at))
}

/// 把一段文本切成"语句":去掉注释,按引号外的 ';' 切,花括号只当分隔符
fn statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for line in text.lines() {
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            // 行内注释:引号外的 "//" 之后到行尾都不要
            if !quoted && c == '/' && chars.peek() == Some(&'/') {
                break;
            }
            match c {
                '"' => {
                    quoted = !quoted;
                    cur.push(c);
                }
                ';' if !quoted => out.push(std::mem::take(&mut cur)),
                '{' | '}' if !quoted => {
                    if !cur.trim().is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            }
        }
        // 行与行之间总有空白,不然两行会粘成一个词
        cur.push(' ');
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// 切词:引号字符串去掉壳,'=' '[' ']' ',' 各自成词,
/// '--' '->' '<-' 各自成词(三者不能混为一谈,方向不一样)
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut quoted = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                quoted = false;
                out.push(std::mem::take(&mut buf));
            } else {
                buf.push(c);
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '=' | '[' | ']' | ',' => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
                if c != ',' {
                    out.push(c.to_string());
                }
            }
            '-' | '<' => {
                let arrow = match (c, chars.peek().copied()) {
                    ('-', Some('-')) => Some("--"),
                    ('-', Some('>')) => Some("->"),
                    ('<', Some('-')) => Some("<-"),
                    _ => None,
                };
                match arrow {
                    Some(op) => {
                        if !buf.is_empty() {
                            out.push(std::mem::take(&mut buf));
                        }
                        chars.next();
                        out.push(op.to_string());
                    }
                    None => buf.push(c),
                }
            }
            c if c.is_whitespace() => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            c => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// 从 '[' 开始解析属性表;没有属性表就是空表
fn parse_attrs(tok: &[String], mut i: usize) -> Attrs {
    let mut attrs = Attrs::default();
    if tok.get(i).map(String::as_str) != Some("[") {
        return attrs;
    }
    i += 1;
    while i < tok.len() {
        match tok[i].as_str() {
            "]" => break,
            key => {
                let mut value = "";
                if tok.get(i + 1).map(String::as_str) == Some("=") {
                    value = tok.get(i + 2).map(String::as_str).unwrap_or("");
                    i += 3;
                } else {
                    i += 1;
                }
                if !key.is_empty() {
                    attrs.set(key, value);
                }
            }
        }
    }
    attrs
}

/// "x,y" -> 坐标;没写或者写坏了都当没有坐标
fn parse_pos(s: &str) -> Option<(f64, f64)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
}

/// 别名属性按 '|' 拆开:前后空白与空项丢掉,重复的只留一份
fn split_terms(value: Option<&str>, out: &mut Vec<String>) {
    if let Some(v) = value {
        for part in v.split('|') {
            let part = part.trim();
            if !part.is_empty() && !out.iter().any(|t| t == part) {
                out.push(part.to_string());
            }
        }
    }
}

/// color="#rrggbb" -> 真彩色;'#' 可省,写得不完整就当没写
fn color_of(attrs: &Attrs) -> Option<(u8, u8, u8)> {
    let hex = attrs.get("color")?.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((b(0)?, b(2)?, b(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PALETTE;

    #[test]
    fn 解析节点与边() {
        let g = parse(
            "// 注释\n\
             graph g {\n\
             node [shape=point];\n\
             edge [color=\"#112233\", line=\"2号线\"];\n\
             \"甲\" [pos=\"10,20\"];\n\
             \"乙\" [pos=\"30.5,-40\"];\n\
             \"甲\" -- \"乙\" [color=\"#df4661\", line=\"1号线\"];\n\
             \"甲\" -- \"乙\";\n\
             }\n",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].label, "甲");
        assert_eq!(g.nodes[0].pos, Some((10.0, 20.0)));
        assert_eq!(g.nodes[1].pos, Some((30.5, -40.0)));
        assert_eq!(g.edges.len(), 2);
        // 边自己的属性盖过 edge 默认属性
        assert_eq!(g.groups[g.edges[0].group].label, "1号线");
        assert_eq!(g.group_color(g.edges[0].group), (0xdf, 0x46, 0x61));
        assert_eq!(g.groups[g.edges[1].group].label, "2号线");
        assert_eq!(g.group_color(g.edges[1].group), (0x11, 0x22, 0x33));
    }

    #[test]
    fn 没有坐标的节点是_none() {
        let g = parse("graph g {\n \"甲\";\n}").unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn 边没写颜色就发调色板颜色() {
        let g = parse("graph g { \"甲\"; \"乙\"; \"甲\" -- \"乙\"; }").unwrap();
        assert_eq!(g.groups[g.edges[0].group].color, PALETTE[0]);
    }

    #[test]
    fn group_与_line_都算分组名且_group_优先() {
        let g = parse(
            "graph g {\n\
             \"甲\"; \"乙\"; \"丙\";\n\
             \"甲\" -- \"乙\" [group=\"主线\", line=\"1号线\"];\n\
             \"乙\" -- \"丙\" [line=\"支线\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.groups[g.edges[0].group].label, "主线");
        assert_eq!(g.groups[g.edges[1].group].label, "支线");
    }

    #[test]
    fn digraph_的箭头边有向_graph_里的也有向() {
        let g = parse("digraph deps { \"a\"; \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert!(g.edges[0].directed);
        let g = parse("graph g { \"a\"; \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert!(g.edges[0].directed);
    }

    #[test]
    fn 双减号永远无向_即使_digraph() {
        let g = parse("digraph g { \"a\"; \"b\"; \"a\" -- \"b\"; }").unwrap();
        assert!(!g.edges[0].directed);
    }

    #[test]
    fn 没写箭头的边按图类型定方向() {
        let g = parse("digraph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert!(g.edges[0].directed);
        let g = parse("graph g { \"a\"; \"b\"; \"a\" \"b\"; }").unwrap();
        assert!(!g.edges[0].directed);
    }

    #[test]
    fn 左箭头是从右边指向左边() {
        let g = parse("graph g { \"甲\"; \"乙\"; \"甲\" <- \"乙\"; }").unwrap();
        let e = g.edges[0];
        assert_eq!(g.nodes[e.a].label, "乙");
        assert_eq!(g.nodes[e.b].label, "甲");
        assert!(e.directed);
    }

    #[test]
    fn subgraph_当透明容器不造节点() {
        let g = parse(
            "graph g {\n\
             subgraph cluster_a {\n\
               \"甲\"; \"乙\";\n\
               \"甲\" -- \"乙\";\n\
             }\n\
             \"丙\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert!(g.nodes.iter().all(|n| n.label != "subgraph"));
        assert!(g.nodes.iter().all(|n| n.label != "cluster_a"));
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 默认属性被自己的属性盖过() {
        let g = parse(
            "graph g {\n\
             node [pos=\"1,1\", label=\"默认\"];\n\
             edge [group=\"默认组\"];\n\
             \"甲\" [label=\"甲显示\"];\n\
             \"乙\" [pos=\"2,2\"];\n\
             \"甲\" -- \"乙\" [group=\"自己的组\"];\n\
             \"乙\" -- \"甲\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes[0].label, "甲显示");
        assert_eq!(g.nodes[0].pos, Some((1.0, 1.0)));
        assert_eq!(g.nodes[1].pos, Some((2.0, 2.0)));
        assert_eq!(g.groups[g.edges[0].group].label, "自己的组");
        assert_eq!(g.groups[g.edges[1].group].label, "默认组");
    }

    #[test]
    fn label_当显示名且节点名进别名() {
        let g = parse(
            "graph g {\n\
             \"station\" [label=\"站点\", terms=\"a|b\", pinyin=\"zhan dian\", pinyin2=\"cham|\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes[0].label, "站点");
        let terms = &g.nodes[0].terms;
        for t in ["station", "a", "b", "zhan dian", "cham"] {
            assert!(terms.iter().any(|s| s == t), "缺别名 {t}: {terms:?}");
        }
    }

    #[test]
    fn 同名节点合并() {
        let g = parse(
            "graph g {\n\
             \"甲\";\n\
             \"甲\" [pos=\"3,4\", terms=\"jia\"];\n\
             \"乙\";\n\
             \"甲\" -- \"乙\";\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].label, "甲");
        assert_eq!(g.nodes[0].pos, Some((3.0, 4.0)));
        assert!(g.nodes[0].terms.iter().any(|t| t == "jia"));
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 边端点没声明过就隐式建节点并按首次出现排序() {
        let g = parse("digraph g { \"a\" -> \"b\"; \"b\" -> \"c\"; }").unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[0].label, "a");
        assert_eq!(g.nodes[1].label, "b");
        assert_eq!(g.nodes[2].label, "c");
        assert_eq!(g.edges.len(), 2);
        // 隐式建的节点没有坐标
        assert!(!g.has_pos());
        assert_eq!(g.nodes[0].pos, None);
    }

    #[test]
    fn 声明写在边后面也生效() {
        let g = parse(
            "graph g {\n\
             \"a\" -> \"b\";\n\
             \"a\" [pos=\"1,2\", terms=\"ay\"];\n\
             }",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.nodes[0].pos, Some((1.0, 2.0)));
        assert!(g.nodes[0].terms.iter().any(|t| t == "ay"));
    }

    #[test]
    fn 名字大小写敏感() {
        let g = parse("graph g { \"A\"; \"a\"; \"A\" -- \"a\"; }").unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn 空图报错() {
        assert!(parse("graph g { }").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn strict_digraph_也认() {
        let g = parse("strict digraph g { \"a\"; \"b\"; \"a\" -> \"b\"; \"a\" -> \"b\"; }").unwrap();
        assert!(g.edges[0].directed);
        assert_eq!(g.edges.len(), 1);
    }
}
