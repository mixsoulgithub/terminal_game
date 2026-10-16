// 查询语义（CLI 与 TUI 的 / 过滤器共用）:
//   :name   只搜 emoji，冒号是 gemoji/GitHub 短代码的约定
//   name    三源一起搜（unicode 官方名 / emoji 别名标签描述 / nerd font 图标名）
//
// 查询词和索引里的 search 字段都先过 data::canonical，所以分隔符不影响匹配。
// 档位从高到低：整体前缀 > 词首出现 > 任意位置出现 > 各词首字母按序命中。
// 只认这四种，不做自由子序列 —— 否则 "laugh" 会命中句子里散落的 l…a…u…g…h。
// 同分保持 entries 的既有顺序（按来源、名字排序），结果稳定。

use crate::data::{canonical, Entry, Index, Source};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Query {
    /// 冒号开头；`:` `joy` `:` 这类写法的冒号会被剥掉。
    Emoji(String),
    /// 裸词，三源一起搜。
    Any(String),
}

pub(crate) fn parse(raw: &str) -> Query {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(':') {
        Query::Emoji(canonical(rest.trim_matches(':')))
    } else {
        Query::Any(canonical(trimmed))
    }
}

/// 得分档位，留出位置差值空间。
const SCORE_EXACT: i32 = 1200; // search 整个就等于查询词，压过一切加成
const SCORE_TOKEN: i32 = 950; // 某个词就等于查询词
const SCORE_PREFIX: i32 = 850; // 词还没结束的前缀命中（joystick 之于 joy）
const SCORE_WORD: i32 = 800; // 词首出现
const SCORE_SUBSTR: i32 = 700; // 任意位置出现
const SCORE_ACRONYM: i32 = 400; // 各词首字母按序命中
/// 命中落在 label 本体（短代码 / 官方名）而不是标签、描述里时的加成。
/// `:laugh` 应该先给 :laughing:，而不是把 tag 里带 laugh 的 :smile: 顶上来。
const LABEL_BONUS: i32 = 200;

/// 命中的 entries 下标，按得分降序；needle 为空返回全部候选（fzf 的空查询显示全集）。
pub(crate) fn hits(index: &Index, raw: &str) -> Vec<usize> {
    let (needle, emoji_only) = match parse(raw) {
        Query::Emoji(n) => (n, true),
        Query::Any(n) => (n, false),
    };
    let keep = |entry: &Entry| !emoji_only || entry.source == Source::Emoji;

    if needle.is_empty() {
        return index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| keep(entry))
            .map(|(idx, _)| idx)
            .collect();
    }

    let mut scored: Vec<(i32, usize)> = index
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| keep(entry))
        .filter_map(|(idx, entry)| score_entry(entry, &needle).map(|score| (score, idx)))
        .collect();

    // 同分先给更短的名字（:joy: 在 :joy_cat: 之前），最后退回 entries 顺序
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| {
                index.entries[a.1]
                    .label
                    .len()
                    .cmp(&index.entries[b.1].label.len())
            })
            .then_with(|| a.1.cmp(&b.1))
    });
    scored.into_iter().map(|(_, idx)| idx).collect()
}

fn score_entry(entry: &Entry, needle: &str) -> Option<i32> {
    let hay = &entry.search;
    let label_end = entry.label_key_len as usize;

    if hay == needle {
        return Some(SCORE_EXACT);
    }

    // 整词命中要压过前缀命中：`:joy` 应该先给 :joy:，而不是 :joystick:
    if let Some(index) = token_exact(hay, needle) {
        let score = SCORE_TOKEN - index as i32;
        return Some(score + label_bonus(hay, index, label_end));
    }

    if let Some(pos) = hay.find(needle) {
        // 命中越靠左越好
        let score = if pos == 0 {
            SCORE_PREFIX
        } else if hay.as_bytes()[pos - 1] == b' ' {
            SCORE_WORD - pos as i32
        } else {
            SCORE_SUBSTR - pos as i32
        };
        let end = pos + needle.len();
        let bonus = if end <= label_end { LABEL_BONUS } else { 0 };
        return Some(score + bonus);
    }

    acronym_penalty(hay, needle).map(|gaps| SCORE_ACRONYM - gaps)
}

/// 命中（第 index 个词）是否整体落在 label 本体里，以及它前面还有多少字节。
fn label_bonus(hay: &str, word_index: usize, label_end: usize) -> i32 {
    let word_start = hay
        .split(' ')
        .take(word_index)
        .map(|word| word.len() + 1)
        .sum::<usize>();
    if word_start < label_end {
        LABEL_BONUS
    } else {
        0
    }
}

/// 第几个词等于查询词（hay 是空格分隔的规范形式）。
fn token_exact(hay: &str, needle: &str) -> Option<usize> {
    for (index, word) in hay.split(' ').enumerate() {
        if word == needle {
            return Some(index);
        }
    }
    None
}

/// 各词首字母按序命中，例如 `nsm` -> `nf-fa-smile_o`。逐词推进，只吃词首，
/// 所以长描述里的散字拼不出命中。needle 里的空格忽略，`nf smile` 与 `nfsmile` 同义。
fn acronym_penalty(hay: &str, needle: &str) -> Option<i32> {
    let mut wanted = needle.bytes().filter(|b| *b != b' ');
    let mut current = wanted.next()?;
    let mut gaps = 0i32;

    for word in hay.split(' ') {
        let Some(initial) = word.bytes().next() else {
            continue;
        };
        if initial == current {
            match wanted.next() {
                Some(next) => current = next,
                None => return Some(gaps),
            }
        } else {
            gaps += 1;
        }
    }

    None
}

/// 大小写不敏感的子串查找，返回 (字节位置, 是否落在词首)。
/// hay 可以是任意大小写（逐字节转小写比较），needle 必须已小写。
/// tui.rs 的 `?` 查找也用它，避免对 35k 个名字逐个 to_lowercase。
pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<(usize, bool)> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let hay = hay.as_bytes();
    let needle = needle.as_bytes();
    let first = needle[0];
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i].to_ascii_lowercase() == first
            && hay[i..i + needle.len()]
                .iter()
                .zip(needle)
                .all(|(h, n)| h.to_ascii_lowercase() == *n)
        {
            let word = i == 0 || is_separator(hay[i - 1]);
            return Some((i, word));
        }
        i += 1;
    }
    None
}

fn is_separator(b: u8) -> bool {
    !b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Entry, Source};

    /// search 字段在真实索引里已由 data::canonical 处理，这里直接给规范形式，
    /// label_key_len 按真实布局（label 是 search 的前缀）算。
    fn entry(label: &str, search: &str, source: Source) -> Entry {
        Entry {
            glyph: "x".into(),
            cp: 0x41,
            label: label.into(),
            label_key_len: canonical(label).len() as u32,
            hint: String::new(),
            search: search.into(),
            source,
        }
    }

    fn index() -> Index {
        // 模拟三个来源各一条：emoji 短代码/标签、nerd 图标名、unicode 官方名。
        let entries = vec![
            entry(":joy:", "joy laugh lol face tears", Source::Emoji),
            entry(":smile:", "smile happy grinning", Source::Emoji),
            entry("nf-fa-smile_o", "nf-fa-smile_o", Source::Nerd),
            entry("FACE WITH TEARS OF JOY", "face with tears of joy", Source::Unicode),
            entry("WHITE SMILING FACE", "white smiling face", Source::Unicode),
        ];
        Index::new(entries)
    }

    #[test]
    fn parse_colon_forms() {
        assert_eq!(parse(":joy"), Query::Emoji("joy".into()));
        assert_eq!(parse(":Joy:"), Query::Emoji("joy".into()));
        assert_eq!(parse("joy"), Query::Any("joy".into()));
        assert_eq!(parse("  Joy  "), Query::Any("joy".into()));
    }

    #[test]
    fn emoji_query_stays_in_emoji_source() {
        let index = index();
        let hits = hits(&index, ":joy");
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn any_query_covers_all_sources() {
        let index = index();
        // 一个词同时命中 emoji 短代码、nerd 图标名和 unicode 官方名
        let hits = hits(&index, "smil");
        assert_eq!(hits.len(), 3);
        let sources: Vec<Source> = hits.iter().map(|idx| index.entries[*idx].source).collect();
        assert!(sources.contains(&Source::Emoji));
        assert!(sources.contains(&Source::Nerd));
        assert!(sources.contains(&Source::Unicode));
        // 短代码前缀命中(:smile:)排在词首命中(nf-fa-smile_o)与子串命中之前
        assert_eq!(hits[0], 1);
    }

    #[test]
    fn label_hit_beats_tag_hit() {
        let index = Index::new(
            vec![
                entry(":smile:", "smile happy joy laugh pleased", Source::Emoji),
                entry(":laughing:", "laughing satisfied grinning", Source::Emoji),
            ]);
        // :smile: 只是 tag 里有 laugh，:laughing: 是短代码本身命中
        assert_eq!(hits(&index, ":laugh"), vec![1, 0]);
    }

    #[test]
    fn whole_token_beats_longer_word_prefix() {
        let index = Index::new(
            vec![
                entry(":joystick:", "joystick game arcade", Source::Emoji),
                entry(":joy_cat:", "joy cat tears smiling", Source::Emoji),
                entry(":joy:", "joy laugh lol face tears", Source::Emoji),
            ]);
        // 整词 joy 的两条在前，短的 :joy: 更靠前；joystick 只是前缀，排最后
        assert_eq!(hits(&index, ":joy"), vec![2, 1, 0]);
    }

    #[test]
    fn label_exact_beats_prefix() {
        let index = Index::new(
            vec![
                entry("joyful", "joyful", Source::Emoji),
                entry("joy", "joy", Source::Emoji),
            ]);
        let hits = hits(&index, ":joy");
        assert_eq!(hits, vec![1, 0]);
    }

    #[test]
    fn empty_query_lists_everything() {
        let index = index();
        assert_eq!(hits(&index, "").len(), 5);
        assert_eq!(hits(&index, ":").len(), 2);
    }

    #[test]
    fn acronym_matches_word_initials_only() {
        let index = Index::new(
            vec![
                entry("nf-md-emoticon_happy", "nf md emoticon happy", Source::Nerd),
                entry("nf-fa-smile_o", "nf fa smile o", Source::Nerd),
                entry("LEFTWARDS PUSHING HAND", "leftwards pushing hand", Source::Unicode),
            ]);
        // 词首 nf / md / emoticon 的首字母
        assert_eq!(hits(&index, "nme").len(), 1);
        assert_eq!(hits(&index, "lph").len(), 1);
        // 自由子序列不再命中 "laugh"
        assert!(hits(&index, "laugh").is_empty());
        // 单字母也能命中词首，但不该命中已排除的散字
        assert_eq!(hits(&index, "fso").len(), 1);
    }

    #[test]
    fn find_ci_reports_word_boundary() {
        assert_eq!(find_ci("nf-fa-smile_o", "smile"), Some((6, true)));
        assert_eq!(find_ci("FACE WITH TEARS OF JOY", "tears"), Some((10, true)));
        assert_eq!(find_ci("abc", "bc"), Some((1, false)));
        assert_eq!(find_ci("abc", "z"), None);
    }
}
