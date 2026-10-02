// 该文件实现文档模型:载入与追加文本、展开 tab、转义控制字符、折行、搜索与过滤.
// 折行、搜索、过滤都建立在同一套"显示行"上,坐标用字节偏移,切片永远落在字符边界上.
use unicode_width::UnicodeWidthChar;

// 一个可视行:指向显示行 line 上的字节区间 [b0, b1)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub b0: usize,
    pub b1: usize,
}

// 一处匹配,坐标系同 Row
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub b0: usize,
    pub b1: usize,
}

// 制表位宽度
const TAB: usize = 8;

pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

// 展开 tab、把不可打印字符写成 ^X,以免打乱终端
fn expand(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0usize;
    for ch in line.chars() {
        match ch {
            '\t' => {
                let stop = TAB - col % TAB;
                for _ in 0..stop {
                    out.push(' ');
                }
                col += stop;
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push('^');
                out.push(if c as u32 == 0x7f {
                    '?'
                } else {
                    char::from(b'@' + c as u8)
                });
                col += 2;
            }
            c => {
                out.push(c);
                col += char_width(c);
            }
        }
    }
    out
}

// 大小写折叠;多字符折叠(如 'İ')只取首字符,对分页器足够
fn fold(c: char, ci: bool) -> char {
    if !ci {
        return c;
    }
    c.to_lowercase().next().unwrap_or(c)
}

// needle 全小写时忽略大小写(少了多看多)
fn pattern(needle: &str) -> Option<(Vec<char>, bool)> {
    if needle.is_empty() {
        return None;
    }
    let ci = !needle.chars().any(|c| c.is_uppercase());
    Some((needle.chars().map(|c| fold(c, ci)).collect(), ci))
}

// 在一行里找出所有匹配的字节区间,按出现顺序
fn find_in(line: &str, pat: &[char], ci: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let folded: Vec<char> = chars.iter().map(|(_, c)| fold(*c, ci)).collect();
    let mut k = 0usize;
    while k + pat.len() <= chars.len() {
        if folded[k..k + pat.len()] == pat[..] {
            let b0 = chars[k].0;
            let b1 = if k + pat.len() < chars.len() {
                chars[k + pat.len()].0
            } else {
                line.len()
            };
            out.push((b0, b1));
            k += pat.len();
        } else {
            k += 1;
        }
    }
    out
}

pub struct Doc {
    pub name: String,
    // 已经收到换行符的完整行(已展开)
    lines: Vec<String>,
    // 还没等到换行符的尾巴,原始文本;追加时会被拼回去,所以 follow 场景不会撕裂
    pending: String,
    // 尾巴展开后的样子,渲染用
    pending_shown: String,
    rev: u64,
}

impl Doc {
    pub fn new(name: String, text: &str) -> Doc {
        let mut d = Doc {
            name,
            lines: Vec::new(),
            pending: String::new(),
            pending_shown: String::new(),
            rev: 0,
        };
        d.append(text);
        d
    }

    // 追加文本;标签"版本号"每次变化,视口据此决定要不要重建行号表
    pub fn append(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.rev += 1;
        let raw = match self.pending.is_empty() {
            true => chunk.to_string(),
            false => format!("{}{chunk}", self.pending),
        };
        match raw.rfind('\n') {
            Some(i) => {
                for l in raw[..i].split('\n') {
                    let l = l.strip_suffix('\r').unwrap_or(l);
                    self.lines.push(expand(l));
                }
                self.set_pending(&raw[i + 1..]);
            }
            None => self.set_pending(&raw),
        }
    }

    fn set_pending(&mut self, tail: &str) {
        let shown = tail.strip_suffix('\r').unwrap_or(tail);
        self.pending_shown = expand(shown);
        self.pending = tail.to_string();
    }

    // 整份换掉(文件被截断或轮转时)
    pub fn replace(&mut self, text: &str) {
        self.lines.clear();
        self.pending.clear();
        self.pending_shown.clear();
        self.append(text);
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    // 显示行数;未完成的尾巴也算一行
    pub fn len(&self) -> usize {
        self.lines.len() + if self.pending.is_empty() { 0 } else { 1 }
    }

    pub fn line(&self, i: usize) -> &str {
        if i < self.lines.len() {
            &self.lines[i]
        } else {
            &self.pending_shown
        }
    }

    pub fn text(&self, r: Row) -> &str {
        &self.line(r.line)[r.b0..r.b1]
    }

    // 不折行:每条显示行一个可视行
    pub fn nowrap_lines(&self, idx: &[usize]) -> Vec<Row> {
        idx.iter()
            .map(|&i| Row {
                line: i,
                b0: 0,
                b1: self.line(i).len(),
            })
            .collect()
    }

    // 按显示宽度折行;宽字符(中文、全角)占 2 格,不会被切成两半.
    // word 为真时优先在最后一个空白处断行,只有超长的单词才会被硬切.
    pub fn wrap_lines(&self, idx: &[usize], width: usize, word: bool) -> Vec<Row> {
        let width = width.max(1);
        let mut rows = Vec::new();
        for &i in idx {
            let line = self.line(i);
            if line.is_empty() {
                rows.push(Row {
                    line: i,
                    b0: 0,
                    b1: 0,
                });
                continue;
            }
            let (mut b0, mut col) = (0usize, 0usize);
            // 本段里最后一个可断的空白位置
            let mut brk: Option<usize> = None;
            for (bo, ch) in line.char_indices() {
                let w = char_width(ch);
                if col > 0 && col + w > width {
                    let (cut, next) = match (word, brk) {
                        (true, Some(s)) if s > b0 => (s, s + 1),
                        _ => (bo, bo),
                    };
                    rows.push(Row {
                        line: i,
                        b0,
                        b1: cut,
                    });
                    b0 = next;
                    col = line[next..bo].chars().map(char_width).sum();
                    brk = None;
                }
                if ch == ' ' {
                    brk = Some(bo);
                }
                col = (col + w).min(width);
            }
            rows.push(Row {
                line: i,
                b0,
                b1: line.len(),
            });
        }
        rows
    }

    pub fn find(&self, needle: &str) -> Vec<Match> {
        let mut out = Vec::new();
        let Some((pat, ci)) = pattern(needle) else {
            return out;
        };
        for i in 0..self.len() {
            for (b0, b1) in find_in(self.line(i), &pat, ci) {
                out.push(Match { line: i, b0, b1 });
            }
        }
        out
    }

    // 过滤:返回命中的显示行号;needle 为空则返回全部
    pub fn filter(&self, needle: &str) -> Vec<usize> {
        let Some((pat, ci)) = pattern(needle) else {
            return (0..self.len()).collect();
        };
        (0..self.len())
            .filter(|&i| !find_in(self.line(i), &pat, ci).is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> Doc {
        Doc::new("t".into(), s)
    }

    fn all(d: &Doc) -> Vec<usize> {
        (0..d.len()).collect()
    }

    fn rows_of(d: &Doc, w: usize) -> Vec<Row> {
        d.wrap_lines(&all(d), w, false)
    }

    #[test]
    fn counts_the_unterminated_tail_as_a_line() {
        assert_eq!(doc("a\nb\n").len(), 2);
        assert_eq!(doc("a\nb").len(), 2);
        assert_eq!(doc("").len(), 0);
        assert_eq!(doc("\n").len(), 1);
    }

    #[test]
    fn expands_tabs_to_tabstops() {
        assert_eq!(doc("a\tb").line(0), "a       b");
        assert_eq!(doc("12345678\tb").line(0), "12345678        b");
    }

    #[test]
    fn escapes_control_chars() {
        assert_eq!(doc("a\x01b\x7f").line(0), "a^Ab^?");
    }

    #[test]
    fn strip_cr() {
        assert_eq!(doc("a\r\nb").line(0), "a");
        // 尾巴里的 \r 也不该漏出来
        assert_eq!(doc("a\r").line(0), "a");
    }

    #[test]
    fn append_joins_a_half_line() {
        let mut d = doc("a\nhal");
        assert_eq!(d.len(), 2);
        d.append("f\nb\n");
        assert_eq!(d.len(), 3);
        assert_eq!(d.line(1), "half");
        assert_eq!(d.line(2), "b");
    }

    #[test]
    fn append_without_newline_keeps_growing_one_line() {
        let mut d = doc("a\n");
        d.append("x");
        d.append("y");
        assert_eq!(d.len(), 2);
        assert_eq!(d.line(1), "xy");
        d.append("\nz");
        assert_eq!(d.len(), 3);
        assert_eq!(d.line(1), "xy");
        assert_eq!(d.line(2), "z");
    }

    #[test]
    fn append_bumps_rev() {
        let mut d = doc("a\n");
        let r = d.rev();
        d.append("b\n");
        assert!(d.rev() > r);
        d.append("");
        assert_eq!(d.rev(), r + 1);
    }

    #[test]
    fn replace_starts_over() {
        let mut d = doc("a\nb\nc\n");
        d.replace("z\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d.line(0), "z");
    }

    #[test]
    fn wrap_keeps_wide_chars_whole() {
        let d = doc("中文中文");
        let rows = rows_of(&d, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(d.text(rows[0]), "中文");
        assert_eq!(d.text(rows[1]), "中文");
        let rows = rows_of(&d, 3);
        assert_eq!(d.text(rows[0]), "中");
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn wide_char_exactly_filling_the_row_stays_on_it() {
        let d = doc("ab中文");
        let rows = rows_of(&d, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(d.text(rows[0]), "ab中");
        assert_eq!(d.text(rows[1]), "文");
        let d = doc("abc中文");
        let rows = rows_of(&d, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(d.text(rows[0]), "abc");
        assert_eq!(d.text(rows[1]), "中文");
    }

    #[test]
    fn wrap_breaks_on_width_limit() {
        let d = doc("abcdef");
        let rows = rows_of(&d, 4);
        assert_eq!(d.text(rows[0]), "abcd");
        assert_eq!(d.text(rows[1]), "ef");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn wrap_keeps_blank_lines() {
        let d = doc("a\n\nb");
        let rows = rows_of(&d, 10);
        assert_eq!(rows.len(), 3);
        assert_eq!(d.text(rows[1]), "");
        assert_eq!(rows[1].line, 1);
    }

    #[test]
    fn word_wrap_breaks_at_spaces() {
        let d = doc("aa bb cc dd");
        let rows = d.wrap_lines(&all(&d), 5, true);
        assert_eq!(rows.len(), 3);
        assert_eq!(d.text(rows[0]), "aa");
        assert_eq!(d.text(rows[1]), "bb");
        assert_eq!(d.text(rows[2]), "cc dd");
        // 同一个宽度下,按字符切会切在词中间
        let hard = rows_of(&d, 5);
        assert_eq!(d.text(hard[0]), "aa bb");
    }

    #[test]
    fn word_wrap_falls_back_to_a_hard_break() {
        // 没有空白可断的超长单词
        let d = doc("aaaaaaaaaa");
        let rows = d.wrap_lines(&all(&d), 4, true);
        assert_eq!(rows.len(), 3);
        assert_eq!(d.text(rows[0]), "aaaa");
        assert_eq!(d.text(rows[2]), "aa");
    }

    #[test]
    fn word_wrap_never_emits_an_empty_row() {
        // 整行都是空格时,不能为了断行吐出一个空行
        let d = doc("        x");
        let rows = d.wrap_lines(&all(&d), 3, true);
        assert!(rows.iter().all(|r| r.b1 > r.b0));
        assert!(d.text(*rows.last().unwrap()).ends_with('x'));
    }

    #[test]
    fn find_is_smart_case() {
        let d = doc("Hello world\nhello WORLD");
        assert_eq!(d.find("hello").len(), 2);
        assert_eq!(d.find("Hello").len(), 1);
        assert_eq!(d.find("WORLD").len(), 1);
        let m = d.find("world");
        assert_eq!(m.len(), 2);
        assert_eq!(
            d.text(Row {
                line: m[0].line,
                b0: m[0].b0,
                b1: m[0].b1
            }),
            "world"
        );
    }

    #[test]
    fn find_reports_byte_ranges_that_slice_cleanly() {
        let d = doc("霓虹灯 neon");
        let m = d.find("neon");
        assert_eq!(m.len(), 1);
        assert_eq!(
            d.text(Row {
                line: 0,
                b0: m[0].b0,
                b1: m[0].b1
            }),
            "neon"
        );
    }

    #[test]
    fn find_empty_needle_matches_nothing() {
        assert!(doc("abc").find("").is_empty());
        assert!(doc("abc").find("zzz").is_empty());
    }

    #[test]
    fn find_sees_the_unterminated_tail() {
        let d = doc("a\nneedle");
        let m = d.find("needle");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line, 1);
    }

    #[test]
    fn filter_returns_line_numbers() {
        let d = doc("alpha\nbeta\nalpha again\n");
        assert_eq!(d.filter("alpha"), vec![0, 2]);
        assert_eq!(d.filter("ALPHA"), Vec::<usize>::new());
        assert_eq!(d.filter(""), vec![0, 1, 2]);
    }

    #[test]
    fn filter_sees_the_unterminated_tail() {
        let d = doc("alpha\nbeta");
        assert_eq!(d.filter("beta"), vec![1]);
    }
}
