//! 搜索:节点名(中文也行)与别名(拼音、缩写)都认,纯数字当成分组号.
//!
//! 分数越小越准.顺序:名字完全相等 < 名字前缀 < 名字包含 < 别名完全相等 <
//! 别名前缀 < 首字母缩写 < 别名包含.同分按节点顺序,结果稳定.

use crate::model::Graph;

/// 匹配结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// 输入是数字:要高亮的分组
    Groups(Vec<usize>),
    /// 输入是文字:匹配到的节点(按匹配质量排好)
    Nodes(Vec<usize>),
    /// 空的
    Empty,
}

/// 别名查询的归一化:只留字母数字,小写,ü 与 v 都当 u
pub fn latin(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c == 'ü' || c == 'ǖ' || c == 'ǘ' || c == 'ǚ' || c == 'ǜ' || c == 'v' {
                Some('u')
            } else if c.is_alphanumeric() {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else {
                None
            }
        })
        .collect()
}

/// 拼音的首字母缩写:"wu lin guang chang" -> "wlgc"
pub fn initials(pinyin: &str) -> String {
    pinyin
        .split_whitespace()
        .filter_map(|s| s.chars().next())
        .collect()
}

/// 一个节点的匹配得分:越小越好,None = 不匹配
fn score(g: &Graph, i: usize, text: &str, lower: &str, latin_q: &str) -> Option<u8> {
    let label = &g.nodes[i].label;
    if label == text {
        return Some(0);
    }
    let label_lower = label.to_lowercase();
    if !lower.is_empty() {
        if label_lower.starts_with(lower) {
            return Some(1);
        }
        if label_lower.contains(lower) {
            return Some(2);
        }
    }
    if latin_q.is_empty() {
        return None;
    }
    let mut best: Option<u8> = None;
    for t in &g.nodes[i].terms {
        let l = latin(t);
        if l.is_empty() {
            continue;
        }
        let s = if l == latin_q {
            3
        } else if l.starts_with(latin_q) {
            4
        } else if !l.contains(' ') && !latin_q.contains(' ') && initials(t) == latin_q {
            5
        } else if l.contains(latin_q) {
            6
        } else {
            continue;
        };
        best = Some(best.map_or(s, |b: u8| b.min(s)));
    }
    best
}

/// 匹配节点,按"越准越前"排
pub fn nodes(g: &Graph, text: &str) -> Vec<usize> {
    let lower = text.to_lowercase();
    let latin_q = latin(text);
    let mut hits: Vec<(u8, usize)> = (0..g.nodes.len())
        .filter_map(|i| score(g, i, text, &lower, &latin_q).map(|s| (s, i)))
        .collect();
    hits.sort_unstable();
    hits.into_iter().map(|(_, i)| i).collect()
}

/// 数字 -> 分组(分组名以这串数字打头,或者就是这串数字)
pub fn groups(g: &Graph, digits: &str) -> Vec<usize> {
    (0..g.groups.len())
        .filter(|&i| {
            let label = g.groups[i].label.trim();
            let lead: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
            lead == digits || label == digits
        })
        .collect()
}

/// 解析输入:全是数字当分组号,其余当节点名/别名
pub fn parse(g: &Graph, text: &str) -> Found {
    let t = text.trim();
    if t.is_empty() {
        return Found::Empty;
    }
    if t.chars().all(|c| c.is_ascii_digit()) && !g.groups.is_empty() {
        let groups = groups(g, t);
        if !groups.is_empty() {
            return Found::Groups(groups);
        }
    }
    let hits = nodes(g, t);
    if hits.is_empty() {
        Found::Empty
    } else {
        Found::Nodes(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> Graph {
        let mut b = Graph::builder();
        let a = b.node("武林广场", None);
        b.terms(a, &["wu lin guang chang"]);
        let c = b.node("吴山广场", None);
        b.terms(c, &["wu shan guang chang"]);
        let d = b.node("打铁关", None);
        b.terms(d, &["da tie guan"]);
        let g1 = b.group("1号线", Some((0xff, 0, 0)));
        let g2 = b.group("2号线", Some((0, 0xff, 0)));
        b.edge(a, d, g1, false);
        b.edge(a, c, g2, false);
        b.build().unwrap()
    }

    #[test]
    fn 中文全等排最前() {
        let m = map();
        assert_eq!(nodes(&m, "武林广场")[0], 0);
    }

    #[test]
    fn 拼音与首字母都能搜到() {
        let m = map();
        assert_eq!(nodes(&m, "datieguan")[0], 2);
        assert_eq!(nodes(&m, "dtg")[0], 2);
        assert!(nodes(&m, "wushan").contains(&1));
    }

    #[test]
    fn 数字当分组() {
        let m = map();
        assert_eq!(parse(&m, "1"), Found::Groups(vec![0]));
        assert_eq!(parse(&m, "2"), Found::Groups(vec![1]));
    }

    #[test]
    fn 搜不到就是空() {
        let m = map();
        assert_eq!(parse(&m, "zzzz"), Found::Empty);
        assert_eq!(parse(&m, "  "), Found::Empty);
    }
}
