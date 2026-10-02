// 该文件实现视口状态:行号表(过滤)、折行重排、滚动、搜索定位、高亮区间的单元格映射.
// 这里不做任何绘制,全部是可单测的纯逻辑.
use crate::doc::{char_width, Doc, Match, Row};

pub struct Options {
    pub wrap: bool,
    pub word_wrap: bool,
    pub numbers: bool,
    pub rainbow: bool,
    pub follow: bool,
}

pub struct Pager {
    pub doc: Doc,
    pub top: usize,
    pub wrap: bool,
    pub word_wrap: bool,
    pub numbers: bool,
    pub rainbow: bool,
    pub follow: bool,
    pub help: bool,
    // 行号列宽度;0 表示关闭
    pub gutter: usize,
    // 正文起始列(= 行号列 + 1 格间隔)
    pub text_x: usize,
    // 折行用的正文宽度
    pub text_w: usize,
    // 正文可用行数(终端高度减去底栏)
    pub view_h: usize,
    pub matches: Vec<Match>,
    pub cur: Option<usize>,
    pub status: String,
    // 参与显示的行号;有过滤时是子集,否则是全部
    idx: Vec<usize>,
    idx_rev: u64,
    filter: Option<String>,
    rows: Vec<Row>,
    // 跟随模式启动时,第一次拿到布局就先跳到文件末尾(像 tail -f)
    pending_bottom: bool,
}

impl Pager {
    pub fn new(doc: Doc, o: Options) -> Pager {
        Pager {
            doc,
            top: 0,
            wrap: o.wrap,
            word_wrap: o.word_wrap,
            numbers: o.numbers,
            rainbow: o.rainbow,
            follow: o.follow,
            help: false,
            gutter: 0,
            text_x: 0,
            text_w: 0,
            view_h: 0,
            matches: Vec::new(),
            cur: None,
            status: String::new(),
            idx: Vec::new(),
            idx_rev: u64::MAX,
            filter: None,
            rows: Vec::new(),
            pending_bottom: o.follow,
        }
    }

    // 当前应显示的若干可视行
    pub fn view(&self) -> &[Row] {
        let start = self.top.min(self.rows.len());
        let end = (start + self.view_h).min(self.rows.len());
        &self.rows[start..end]
    }

    pub fn max_top(&self) -> usize {
        self.rows.len().saturating_sub(self.view_h)
    }

    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    // 打开跟随;下一次拿到布局时先跳到文件末尾(像 tail -f)
    pub fn start_following(&mut self) {
        self.follow = true;
        self.pending_bottom = true;
    }

    // 每帧渲染前调用:列宽变化时重排,并夹住滚动位置
    pub fn ensure_layout(&mut self, w: u16, h: u16) {
        let digits = self.doc.len().to_string().len().max(2);
        let gutter = if self.numbers { digits } else { 0 };
        let text_x = if gutter > 0 { gutter + 1 } else { 0 };
        let text_w = (w as usize).saturating_sub(text_x).max(1);
        self.view_h = (h as usize).saturating_sub(1);
        if self.rows.is_empty() {
            self.text_w = text_w;
            self.reflow();
        } else if text_w != self.text_w {
            self.text_w = text_w;
            self.reflow_keeping_view();
        }
        self.gutter = gutter;
        self.text_x = text_x;
        self.clamp_top();
        // 一次性的:跟随模式启动时先落到文件末尾
        if self.pending_bottom {
            self.pending_bottom = false;
            self.to_bottom();
        }
    }

    fn reflow(&mut self) {
        if self.idx_rev != self.doc.rev() {
            self.rebuild_idx();
        }
        self.rows = if self.wrap {
            self.doc.wrap_lines(&self.idx, self.text_w, self.word_wrap)
        } else {
            self.doc.nowrap_lines(&self.idx)
        };
        self.clamp_top();
    }

    fn rebuild_idx(&mut self) {
        let idx = match self.filter.clone() {
            Some(f) => self.doc.filter(&f),
            None => (0..self.doc.len()).collect(),
        };
        self.idx = idx;
        self.idx_rev = self.doc.rev();
    }

    // 重排后让视口停在同一处内容上,而不是跳回文件开头
    fn reflow_keeping_view(&mut self) {
        let anchor = self.rows.get(self.top).map(|r| (r.line, r.b0));
        self.reflow();
        if let Some((line, b0)) = anchor {
            if let Some(i) = self.row_of(line, b0) {
                self.top = i;
            }
        }
        self.clamp_top();
    }

    fn clamp_top(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    pub fn scroll(&mut self, n: i64) {
        self.top = (self.top as i64 + n).clamp(0, self.max_top() as i64) as usize;
    }

    pub fn page(&mut self, dir: i64) {
        self.scroll(dir * self.view_h.max(1) as i64);
    }

    pub fn half_page(&mut self, dir: i64) {
        self.scroll(dir * (self.view_h / 2).max(1) as i64);
    }

    pub fn to_top(&mut self) {
        self.top = 0;
    }

    pub fn to_bottom(&mut self) {
        self.top = self.max_top();
    }

    // 文件追加内容后调用:重建行号表;原本贴着底部就继续贴底(像 tail -f)
    pub fn content_changed(&mut self) {
        let stick = self.rows.is_empty() || self.top >= self.max_top();
        self.reflow();
        if stick {
            self.to_bottom();
        }
    }

    pub fn percent(&self) -> u16 {
        let total = self.rows.len();
        if total == 0 {
            return 100;
        }
        let bottom = (self.top + self.view_h).min(total);
        (bottom * 100 / total) as u16
    }

    // 定位包含 (line, 字节偏移 b0) 的可视行
    pub fn row_of(&self, line: usize, b0: usize) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let i = self.rows.partition_point(|r| (r.line, r.b0) <= (line, b0));
        Some(i.saturating_sub(1))
    }

    fn reveal(&mut self, row: usize) {
        if row < self.top {
            self.top = row;
        } else if self.view_h > 0 && row >= self.top + self.view_h {
            self.top = row.saturating_sub(self.view_h / 3);
        }
        self.clamp_top();
    }

    fn reveal_match(&mut self, i: usize) {
        let m = self.matches[i];
        if let Some(row) = self.row_of(m.line, m.b0) {
            self.reveal(row);
        }
    }

    // 跳到一个匹配上;forward 从视口顶部往下找,backward 往上找,越界则绕回
    pub fn search(&mut self, needle: &str, backward: bool) {
        self.matches = self.doc.find(needle);
        self.cur = None;
        if self.matches.is_empty() {
            self.status = if needle.is_empty() {
                String::new()
            } else {
                format!("no match: {needle}")
            };
            return;
        }
        let idx = if backward {
            self.matches
                .iter()
                .rposition(|m| self.row_of(m.line, m.b0).is_some_and(|r| r < self.top))
                .unwrap_or(self.matches.len() - 1)
        } else {
            self.matches
                .iter()
                .position(|m| self.row_of(m.line, m.b0).is_some_and(|r| r >= self.top))
                .unwrap_or(0)
        };
        self.set_cur(idx);
    }

    pub fn next_match(&mut self, dir: i64) {
        if self.matches.is_empty() {
            self.status = "no match".into();
            return;
        }
        let len = self.matches.len() as i64;
        let idx = ((self.cur.unwrap_or(0) as i64 + dir).rem_euclid(len)) as usize;
        self.set_cur(idx);
    }

    fn set_cur(&mut self, idx: usize) {
        self.cur = Some(idx);
        self.status = format!("{} of {}", idx + 1, self.matches.len());
        self.reveal_match(idx);
    }

    pub fn clear_search(&mut self) {
        self.matches.clear();
        self.cur = None;
        self.status.clear();
    }

    // 只留下命中 needle 的行;needle 为空则清除过滤
    pub fn set_filter(&mut self, needle: &str) {
        self.filter = match needle.is_empty() {
            true => None,
            false => Some(needle.to_string()),
        };
        let anchor = self.rows.get(self.top).map(|r| (r.line, r.b0));
        self.rebuild_idx();
        self.reflow();
        self.top = match anchor.and_then(|(l, b)| self.row_of(l, b)) {
            Some(i) => i,
            None => 0,
        };
        self.clamp_top();
        self.status = match &self.filter {
            Some(f) => format!("{} of {} lines match {f}", self.idx.len(), self.doc.len()),
            None => "filter cleared".into(),
        };
    }

    pub fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.reflow_keeping_view();
    }

    pub fn toggle_word_wrap(&mut self) {
        self.word_wrap = !self.word_wrap;
        self.reflow_keeping_view();
    }

    pub fn toggle_numbers(&mut self) {
        self.numbers = !self.numbers;
    }

    // 某可视行上,匹配高亮所占的单元格列区间(相对正文起点),第三项表示是否为当前匹配
    pub fn match_ranges(&self, row: &Row) -> Vec<(u16, u16, bool)> {
        let mut out: Vec<(u16, u16, bool)> = Vec::new();
        if self.matches.is_empty() {
            return out;
        }
        let lo = self.matches.partition_point(|m| m.line < row.line);
        let hi = self.matches.partition_point(|m| m.line <= row.line);
        if lo == hi {
            return out;
        }
        let line = self.doc.line(row.line);
        let slice = &line[row.b0..row.b1];
        let mut mi = lo;
        let mut col = 0u16;
        for (i, ch) in slice.char_indices() {
            let b = row.b0 + i;
            while mi < hi && b >= self.matches[mi].b1 {
                mi += 1;
            }
            let inside = mi < hi && b >= self.matches[mi].b0 && b < self.matches[mi].b1;
            let w = char_width(ch).max(1) as u16;
            if inside {
                let is_cur = self.cur == Some(mi);
                match out.last_mut() {
                    Some((_, e, c)) if *c == is_cur && *e == col => *e = col + w,
                    _ => out.push((col, col + w, is_cur)),
                }
            }
            col += w;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options {
            wrap: true,
            word_wrap: false,
            numbers: false,
            rainbow: false,
            follow: false,
        }
    }

    fn pager(text: &str) -> Pager {
        let mut p = Pager::new(Doc::new("t".into(), text), opts());
        p.ensure_layout(20, 11); // 底栏占 1 行 -> view_h = 10
        p
    }

    fn lines(n: usize) -> String {
        (0..n)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn scroll_clamps_at_both_ends() {
        let mut p = pager(&lines(30));
        assert_eq!(p.view_h, 10);
        assert_eq!(p.max_top(), 20);
        p.scroll(-5);
        assert_eq!(p.top, 0);
        p.scroll(100);
        assert_eq!(p.top, 20);
        p.scroll(-3);
        assert_eq!(p.top, 17);
    }

    #[test]
    fn page_and_half_page_move_by_viewport() {
        let mut p = pager(&lines(30));
        p.page(1);
        assert_eq!(p.top, 10);
        p.half_page(-1);
        assert_eq!(p.top, 5);
        p.to_bottom();
        assert_eq!(p.top, 20);
        p.to_top();
        assert_eq!(p.top, 0);
    }

    #[test]
    fn percent_tracks_bottom_of_viewport() {
        let mut p = pager(&lines(30));
        assert_eq!(p.percent(), 33);
        p.to_bottom();
        assert_eq!(p.percent(), 100);
    }

    #[test]
    fn search_finds_first_match_at_or_below_the_viewport() {
        // 30 行,view_h = 10;把 banana 放在第 3、14、25 行
        let text = lines(30)
            .lines()
            .enumerate()
            .map(|(i, l)| {
                if i == 3 || i == 14 || i == 25 {
                    "banana".to_string()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut p = pager(&text);
        p.scroll(10); // 视口 10..20
        p.search("banana", false);
        assert_eq!(p.matches.len(), 3);
        // 从视口顶部往下找:命中第 14 行
        assert_eq!(p.cur, Some(1));
        assert!(p.top <= 14 && 14 < p.top + p.view_h);
        p.next_match(1);
        assert_eq!(p.cur, Some(2));
        // 绕回第 3 行,视口随之上滚
        p.next_match(1);
        assert_eq!(p.cur, Some(0));
        assert!(p.top <= 3);
        p.next_match(-1);
        assert_eq!(p.cur, Some(2));
    }

    #[test]
    fn search_does_not_scroll_when_everything_fits() {
        let mut p = pager("apple\nbanana\ncherry\nbanana\n");
        p.search("banana", false);
        assert_eq!(p.matches.len(), 2);
        assert_eq!(p.cur, Some(0));
        assert_eq!(p.top, 0);
        assert_eq!(p.status, "1 of 2");
        p.next_match(1);
        assert_eq!(p.cur, Some(1));
        p.next_match(1);
        assert_eq!(p.cur, Some(0));
    }

    #[test]
    fn search_reveals_a_match_far_below() {
        let mut p = pager(&lines(100));
        p.search("line 80", false);
        assert_eq!(p.cur, Some(0));
        assert!(p.top <= 80 && 80 < p.top + p.view_h);
    }

    #[test]
    fn search_without_hit_reports_it() {
        let mut p = pager("abc\n");
        p.search("zzz", false);
        assert!(p.matches.is_empty());
        assert_eq!(p.cur, None);
        assert!(p.status.starts_with("no match"));
    }

    #[test]
    fn reflow_keeps_the_view_anchored() {
        let text = (0..30)
            .map(|i| format!("{i:02} 0123456789012345678901234567890123456789"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut p = pager(&text);
        p.scroll(10);
        let (line, b0) = {
            let r = p.rows[p.top];
            (r.line, r.b0)
        };
        p.ensure_layout(10, 12);
        let now = p.rows[p.top];
        assert_eq!((now.line, now.b0), (line, b0));
    }

    #[test]
    fn toggling_numbers_does_not_lose_position() {
        let mut p = pager(&lines(40));
        p.to_bottom();
        p.toggle_numbers();
        p.ensure_layout(20, 12);
        assert_eq!(p.top, p.max_top());
        assert_eq!(p.gutter, 2);
        assert_eq!(p.text_x, 3);
        assert_eq!(p.text_w, 17);
    }

    #[test]
    fn wrap_toggles_between_wrapped_and_one_row_per_line() {
        let text = (0..30)
            .map(|i| format!("{i:02} {}", "x".repeat(40)))
            .collect::<Vec<_>>()
            .join("\n");
        let mut p = pager(&text); // 正文宽 20
        let wrapped = p.rows.len();
        assert!(wrapped > 30);
        p.toggle_wrap();
        assert!(!p.wrap);
        assert_eq!(p.rows.len(), 30);
        p.toggle_wrap();
        assert!(p.wrap);
        assert_eq!(p.rows.len(), wrapped);
    }

    #[test]
    fn word_wrap_breaks_at_spaces() {
        let text = "aaaa bbbb cccc dddd eeee ffff\n";
        let mut p = pager(&text); // 正文宽 20
        // 按字符切:第 20 格正好落在空格上,下一段从 eeee 开始
        assert_eq!(p.doc.text(p.rows[0]), "aaaa bbbb cccc dddd ");
        assert_eq!(p.doc.text(p.rows[1]), "eeee ffff");
        p.toggle_word_wrap();
        assert!(p.word_wrap);
        // 按词切:整词留住,尾空格被吃掉
        assert_eq!(p.doc.text(p.rows[0]), "aaaa bbbb cccc dddd");
        assert_eq!(p.doc.text(p.rows[1]), "eeee ffff");
        // 切换折行方式不该丢掉位置
        let anchor = p.rows[p.top].line;
        p.toggle_word_wrap();
        assert_eq!(p.rows[p.top].line, anchor);
    }

    #[test]
    fn filter_hides_the_other_lines() {
        let text = (0..60)
            .map(|i| {
                if i % 3 == 0 {
                    format!("keep {i}")
                } else {
                    format!("drop {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut p = pager(&text);
        p.set_filter("keep");
        assert_eq!(p.idx.len(), 20);
        assert_eq!(p.doc.len(), 60);
        assert_eq!(p.rows.len(), 20);
        assert!(p.status.contains("20 of 60"));
        assert_eq!(p.filter(), Some("keep"));
        // 视口只在这 20 行里滚动
        p.to_bottom();
        assert_eq!(p.top, 10);
        assert_eq!(p.doc.text(p.rows[p.top]), "keep 30");
        p.set_filter("");
        assert_eq!(p.idx.len(), 60);
        assert_eq!(p.filter(), None);
        assert_eq!(p.status, "filter cleared");
    }

    #[test]
    fn filter_keeps_the_anchor_when_it_survives() {
        // 100 行,偶数行命中;视口只有 10 行,所以锚点必须落在还能放得下的范围内
        let text = (0..100)
            .map(|i| {
                if i % 2 == 0 {
                    format!("keep {i}")
                } else {
                    format!("drop {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut p = pager(&text);
        p.scroll(40);
        let anchor = p.rows[p.top].line;
        assert_eq!(anchor, 40);
        p.set_filter("keep");
        assert_eq!(p.idx.len(), 50);
        assert_eq!(p.rows[p.top].line, anchor);
        assert_eq!(p.top, 20);
    }

    #[test]
    fn filter_falls_back_to_the_top_when_nothing_matches() {
        let text = lines(30);
        let mut p = pager(&text);
        p.to_bottom();
        p.set_filter("zzz");
        assert_eq!(p.idx.len(), 0);
        assert_eq!(p.top, 0);
        assert!(p.view().is_empty());
        assert!(p.status.contains("0 of 30"));
    }

    #[test]
    fn follow_starts_at_the_bottom_on_the_first_layout() {
        let mut p = Pager::new(
            Doc::new("t".into(), &format!("{}\n", lines(30))),
            Options {
                follow: true,
                ..opts()
            },
        );
        // 首帧就落到末尾,而不是停在文件开头
        p.ensure_layout(20, 12);
        assert_eq!(p.top, p.max_top());
        assert_eq!(p.view().last().unwrap().line, p.doc.len() - 1);
        // 然后继续贴底,新内容一进来就能看到
        p.doc.append("line 30\nline 31\n");
        p.content_changed();
        assert_eq!(p.top, p.max_top());
        assert_eq!(p.view().last().unwrap().line, 31);
        // 用户往上翻之后不再被拽回去
        p.scroll(-5);
        let top = p.top;
        p.doc.append("line 32\n");
        p.content_changed();
        assert_eq!(p.top, top);
    }

    #[test]
    fn appending_sticks_to_the_bottom_when_already_there() {
        let mut p = pager(&format!("{}\n", lines(30)));
        assert_eq!(p.doc.len(), 30);
        p.to_bottom();
        assert_eq!(p.top, 20);
        p.doc.append("line 30\nline 31\n");
        p.content_changed();
        assert_eq!(p.max_top(), 22);
        assert_eq!(p.top, 22);
    }

    #[test]
    fn appending_does_not_yank_a_scrolled_up_view() {
        let mut p = pager(&format!("{}\n", lines(30)));
        p.scroll(5);
        p.doc.append("line 30\nline 31\n");
        p.content_changed();
        assert_eq!(p.top, 5);
    }

    #[test]
    fn appended_lines_respect_the_filter() {
        let mut p = pager("line 0\nline 1\nline 2\n");
        p.set_filter("line 1");
        assert_eq!(p.idx.len(), 1);
        p.doc.append("line 12\nzzz\n");
        p.content_changed();
        assert_eq!(p.idx.len(), 2);
        assert_eq!(p.doc.len(), 5);
    }

    #[test]
    fn appended_line_completes_a_half_line_in_place() {
        let mut p = pager("a\npar");
        assert_eq!(p.idx.len(), 2);
        p.doc.append("tial\n");
        p.content_changed();
        assert_eq!(p.idx.len(), 2);
        assert_eq!(p.doc.text(p.rows[1]), "partial");
    }

    #[test]
    fn match_ranges_map_columns_of_a_wrapped_row() {
        let mut p = pager("霓虹 abcdefgh 霓虹\n");
        p.ensure_layout(8, 12);
        // 折成 "霓虹 abc" | "defgh 霓" | "虹"
        assert_eq!(p.rows.len(), 3);
        assert_eq!(p.doc.text(p.rows[1]), "defgh 霓");
        p.search("霓虹", false);
        assert_eq!(p.matches.len(), 2);
        assert_eq!(p.match_ranges(&p.rows[0]), vec![(0, 4, true)]);
        assert_eq!(p.match_ranges(&p.rows[1]), vec![(6, 8, false)]);
        assert_eq!(p.match_ranges(&p.rows[2]), vec![(0, 2, false)]);
    }

    #[test]
    fn match_ranges_empty_without_search() {
        let p = pager("abc\n");
        assert!(p.match_ranges(&p.rows[0]).is_empty());
    }

    #[test]
    fn empty_document_is_safe() {
        let mut p = pager("");
        assert_eq!(p.rows.len(), 0);
        assert_eq!(p.percent(), 100);
        p.scroll(5);
        assert_eq!(p.top, 0);
        assert!(p.view().is_empty());
    }
}
