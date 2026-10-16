// 数据层: emoji 短代码 / nerd font 图标名 / Unicode 官方名的索引。
// 三份原始数据首次运行时用 curl 下载到 ~/.cache/uni-browser（尊重 XDG_CACHE_HOME），
// 之后完全离线；删掉缓存目录即可强制重新下载。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// 每个文件给一串候选 URL，按顺序尝试：本机网络直连 raw.githubusercontent.com 会被
// 卡死（0 字节超时），jsdelivr CDN 可用且快，所以 CDN 在前、官方源兜底。
// unicode.org 本身能连但只有 ~20KB/s，UnicodeData.txt 有 2.2MB，放最后。
/// github/gemoji: emoji 短代码、别名与标签（`:laugh:` 这一套的源头）。
const EMOJI_URLS: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/github/gemoji@master/db/emoji.json",
    "https://raw.githubusercontent.com/github/gemoji/master/db/emoji.json",
];
/// nerd-fonts 官方图标名表（键是去掉 nf- 前缀的名字，值是字形和码点）。
const NERD_URLS: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/ryanoasis/nerd-fonts@master/glyphnames.json",
    "https://cdn.jsdelivr.net/gh/ryanoasis/nerd-fonts@master/bin/scripts/lib/glyphnames.json",
    "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/glyphnames.json",
];
const UCD_URLS: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/unicode-org/icu@main/icu4c/source/data/unidata/UnicodeData.txt",
    "https://www.unicode.org/Public/UCD/latest/ucd/UnicodeData.txt",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Source {
    Emoji,
    Nerd,
    Unicode,
}

impl Source {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Source::Emoji => "emoji",
            Source::Nerd => "nerd",
            Source::Unicode => "unicode",
        }
    }
}

/// 匹配用的规范形式: 小写、把 `- _ : 空格` 等分隔符折叠成单个空格、去掉首尾空格。
/// 索引里的 search 字段和查询词都过这一层，所以 `fa-smile`、`fa smile`、`FA_SMILE`
/// 三者等价，而子串匹配不会因为分隔符不同而漏掉。
/// 字形墙收录的 Unicode 区段：终端画图与做游戏会用到的那几块。
/// 只收 UnicodeData 里真实存在的条目，未分配的空洞不会出现在墙上。
const WALL_RANGES: &[(u32, u32)] = &[
    (0x2190, 0x21FF),   // 箭头
    (0x2200, 0x22FF),   // 数学运算符
    (0x2300, 0x23FF),   // 杂项技术符号（电源、时钟、括号）
    (0x2460, 0x24FF),   // 带圈字母数字
    (0x2500, 0x257F),   // 线框
    (0x2580, 0x259F),   // 方块元素
    (0x25A0, 0x25FF),   // 几何图形
    (0x2600, 0x26FF),   // 杂项符号（骰子、棋、天气）
    (0x2700, 0x27BF),   // dingbats
    (0x27F0, 0x27FF),   // 补充箭头 A
    (0x2800, 0x28FF),   // braille（点阵画像素）
    (0x2900, 0x297F),   // 补充箭头 B
    (0x2B00, 0x2BFF),   // 杂项符号与箭头
    (0x1F000, 0x1F0FF), // 麻将、多米诺、扑克
    (0x1FB00, 0x1FBFF), // 传统计算符号（方块几分之几，画像素用）
];

pub(crate) fn canonical(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            pending_space = true;
        }
    }

    out
}

pub(crate) struct Entry {
    /// 实际字符；emoji 可能是多码点序列（ZWJ、肤色、区域旗）。
    pub(crate) glyph: String,
    /// 首个码点：列表和状态行里显示的十六进制。
    pub(crate) cp: u32,
    /// 主名字（emoji 用主别名 `:joy:`，nerd 用图标名，unicode 用官方名）。
    pub(crate) label: String,
    /// 次要信息（emoji 用描述，unicode 用类别，范围用区间）。
    pub(crate) hint: String,
    /// canonical() 过的匹配文本，查询时直接子串查找，不必每次按键重算。
    /// 布局是 `canonical(label) + 别名 + 标签 + 描述`，所以 label 的规范形式一定是它的前缀。
    pub(crate) search: String,
    /// search 里属于 label 本体的字节数；命中落在这段之内才算命中短代码/名字本身。
    pub(crate) label_key_len: u32,
    pub(crate) source: Source,
}

pub(crate) struct Index {
    /// 顺序即展示顺序：emoji（按 CLDR 分类）、nerd 图标（按名字）、unicode（按码点）。
    /// 搜索同分时也沿用这个顺序，结果因此稳定。
    pub(crate) entries: Vec<Entry>,
}

impl Index {
    pub(crate) fn new(entries: Vec<Entry>) -> Self {
        Self { entries }
    }

    /// 字形墙：emoji + nerd font 图标 + WALL_RANGES 里的字符，顺序沿用 entries
    /// （emoji/nerd 按名字，unicode 按码点，方便按线框家族连着看）。
    pub(crate) fn wall(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| match entry.source {
                Source::Emoji | Source::Nerd => true,
                Source::Unicode => WALL_RANGES
                    .iter()
                    .any(|(lo, hi)| (*lo..=*hi).contains(&entry.cp)),
            })
            .map(|(idx, _)| idx)
            .collect()
    }

}

pub(crate) fn load(progress: bool) -> Result<Index, String> {
    let dir = cache_dir()?;
    let emoji_text = fetch(&dir, "emoji.json", EMOJI_URLS, progress)?;
    let nerd_text = fetch(&dir, "glyphnames.json", NERD_URLS, progress)?;
    let ucd_text = fetch(&dir, "UnicodeData.txt", UCD_URLS, progress)?;

    let mut emoji = Vec::new();
    parse_emoji(&emoji_text, &mut emoji)?;

    let mut nerd = Vec::new();
    parse_nerd(&nerd_text, &mut nerd)?;

    let mut unicode = Vec::new();
    parse_unicode_data(&ucd_text, &mut unicode)?;

    let mut entries = Vec::with_capacity(emoji.len() + nerd.len() + unicode.len());
    entries.append(&mut emoji);
    entries.append(&mut nerd);
    entries.append(&mut unicode);

    Ok(Index::new(entries))
}

// --- 缓存与下载

fn cache_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or("neither XDG_CACHE_HOME nor HOME is set, no place for the cache")?;

    let dir = base.join("uni-browser");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create cache dir {}: {e}", dir.display()))?;
    Ok(dir)
}

/// 缓存命中直接读，否则逐个候选 URL 尝试，下载到 <name>.part 再改名，
/// 避免半截文件被当成有效缓存。全部失败时把每个 URL 的结果报出来。
fn fetch(dir: &Path, name: &str, urls: &[&str], progress: bool) -> Result<String, String> {
    let path = dir.join(name);

    match std::fs::read_to_string(&path) {
        Ok(text) if !text.is_empty() => return Ok(text),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {}
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    }

    let tmp = path.with_extension("part");
    let mut failures = Vec::new();

    for url in urls {
        if progress {
            eprintln!("uni-browser: downloading {url}");
        }

        let status = std::process::Command::new("curl")
            .args([
                "-fsSL",
                "--retry",
                "2",
                "--connect-timeout",
                "10",
                "--max-time",
                "300",
                "-o",
            ])
            .arg(&tmp)
            .arg(url)
            .status();

        match status {
            Ok(status) if status.success() => {
                std::fs::rename(&tmp, &path).map_err(|e| format!("rename {}: {e}", tmp.display()))?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                if text.is_empty() {
                    return Err(format!("{} is empty, delete it and retry", path.display()));
                }
                return Ok(text);
            }
            Ok(status) => failures.push(format!("{url}: {status}")),
            Err(e) => failures.push(format!("{url}: curl could not be started ({e})")),
        }
    }

    let _ = std::fs::remove_file(&tmp);
    Err(format!(
        "download of {name} failed; place a copy at {} from any of these sources and retry:\n  {}",
        path.display(),
        failures.join("\n  ")
    ))
}

// --- 解析

#[derive(serde::Deserialize)]
struct GemojiRow {
    emoji: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn parse_emoji(text: &str, out: &mut Vec<Entry>) -> Result<(), String> {
    let rows: Vec<GemojiRow> =
        serde_json::from_str(text).map_err(|e| format!("parse gemoji emoji.json: {e}"))?;

    // 按 gemoji 的分类分组：分类顺序取它在文件里首次出现的位置（即 CLDR 的顺序，
    // Smileys 在前），组内保持文件顺序。按短代码或分类名字母序排会变成字母汤。
    let mut grouped: Vec<(usize, Entry)> = Vec::with_capacity(rows.len());
    let mut category_order: HashMap<String, usize> = HashMap::new();

    for (row_index, row) in rows.into_iter().enumerate() {
        let Some(cp) = row.emoji.chars().next().map(|c| c as u32) else {
            continue;
        };
        // 没有别名时退回描述，两个都没有说明这条数据不可用。
        let label = match row.aliases.first() {
            Some(alias) => format!(":{alias}:"),
            None if !row.description.is_empty() => row.description.clone(),
            None => continue,
        };

        // label 也进 search，保证 label 的规范形式是 search 的前缀
        let mut raw_search = format!("{label} ");
        for word in row.aliases.iter().chain(row.tags.iter()).chain([&row.description]) {
            raw_search.push_str(word);
            raw_search.push(' ');
        }

        let order = *category_order
            .entry(row.category)
            .or_insert(row_index);

        grouped.push((
            order,
            Entry {
                glyph: row.emoji,
                cp,
                label_key_len: canonical(&label).len() as u32,
                label,
                hint: row.description,
                search: canonical(&raw_search),
                source: Source::Emoji,
            },
        ));
    }

    grouped.sort_by_key(|(order, _)| *order);
    out.extend(grouped.into_iter().map(|(_, entry)| entry));

    Ok(())
}

#[derive(serde::Deserialize)]
struct NerdGlyph {
    #[serde(default, rename = "char")]
    ch: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

/// glyphnames.json 顶层除图标外还有 METADATA 之类的对象；BTreeMap 顺带给出稳定顺序。
/// 键是去掉前缀的短名（`fa-smile_o`），官方文档里的名字是 `nf-fa-smile_o`，这里补回来。
fn parse_nerd(text: &str, out: &mut Vec<Entry>) -> Result<(), String> {
    let table: BTreeMap<String, NerdGlyph> = serde_json::from_str(text)
        .map_err(|e| format!("parse nerd-fonts glyphnames.json: {e}"))?;

    for (name, glyph) in table {
        let Some((char, cp)) = nerd_char(&glyph) else {
            continue;
        };
        let label = if name.starts_with("nf-") {
            name
        } else {
            format!("nf-{name}")
        };
        let search = canonical(&label);

        out.push(Entry {
            glyph: char,
            cp,
            label_key_len: search.len() as u32,
            label,
            hint: String::new(),
            search,
            source: Source::Nerd,
        });
    }

    Ok(())
}

/// char 字段是真正的字符就用它，否则退回 code 十六进制字段。
fn nerd_char(glyph: &NerdGlyph) -> Option<(String, u32)> {
    if let Some(ch) = glyph.ch.as_deref() {
        let mut chars = ch.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            // 图标本体是私用区或符号区，ASCII 的 char 字段说明这个字段不是图标
            if !c.is_ascii() {
                return Some((ch.to_string(), c as u32));
            }
        }
    }

    let code = glyph.code.as_deref()?.trim().trim_start_matches("0x");
    let cp = u32::from_str_radix(code, 16).ok()?;
    char::from_u32(cp).map(|c| (c.to_string(), cp))
}

/// UnicodeData.txt: 逐行 `<码点>;<名字>;<类别>;...`。
/// 名字形如 `<..., First>` / `<..., Last>` 的成对行表示码点区间（CJK 等），
/// 合成一条可搜索的区间条目，而不是把上万个码点摊成条目。
fn parse_unicode_data(text: &str, out: &mut Vec<Entry>) -> Result<(), String> {
    let mut pending: Option<(u32, String)> = None;

    for line in text.lines() {
        let mut fields = line.split(';');
        let (Some(cp_hex), Some(name)) = (fields.next(), fields.next()) else {
            continue;
        };
        let category = fields.next().unwrap_or("");
        let Ok(cp) = u32::from_str_radix(cp_hex, 16) else {
            continue;
        };
        if cp >= 0x110000 {
            continue;
        }

        if let Some(inner) = name.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            if let Some(range) = inner.strip_suffix(", First") {
                pending = Some((cp, range.to_string()));
                continue;
            }
            if inner.ends_with(", Last") {
                if let Some((start, range)) = pending.take() {
                    // 代理对区间没有可渲染的字形
                    if !(0xD800..=0xDFFF).contains(&start) {
                        let search = canonical(&range);
                        out.push(Entry {
                            glyph: glyph_of(start),
                            cp: start,
                            label_key_len: search.len() as u32,
                            label: range,
                            hint: format!("U+{start:04X}..U+{cp:04X}"),
                            search,
                            source: Source::Unicode,
                        });
                    }
                }
                continue;
            }
            // 其余是 `<control>` 这类伪名：保留为码点条目，但不进名字索引
            let search = canonical(name);
            out.push(Entry {
                glyph: glyph_of(cp),
                cp,
                label_key_len: search.len() as u32,
                label: name.to_string(),
                hint: category.to_string(),
                search,
                source: Source::Unicode,
            });
            continue;
        }

        let search = canonical(name);
        out.push(Entry {
            glyph: glyph_of(cp),
            cp,
            label: name.to_string(),
            label_key_len: search.len() as u32,
            hint: category.to_string(),
            search,
            source: Source::Unicode,
        });
    }

    Ok(())
}

/// 字形补到 3 列，后面的列才对得齐（CLI 与 TUI 列表共用）。
pub(crate) fn glyph_cell(glyph: &str) -> String {
    let width = UnicodeWidthStr::width(glyph);
    let mut cell = String::with_capacity(glyph.len() + 3);
    cell.push_str(glyph);
    for _ in width..3 {
        cell.push(' ');
    }
    cell
}

/// 条目要显示的字形。**绝不能把终端的控制字符原样写出去**：
/// C0 控制符用控制图形 U+2400 段表示，C1 控制符会被终端当控制序列执行、
/// bidi 覆盖符会反转该行后面的字符、unicode-width 认为不可打印的字符，
/// 这三类统一换成占位点。零宽组合符保持原样（列表里靠名字辨认）。
fn glyph_of(cp: u32) -> String {
    if cp <= 0x1F {
        return char::from_u32(0x2400 + cp).unwrap().to_string();
    }
    if cp == 0x7F {
        return '\u{2421}'.to_string();
    }
    if (0x0080..=0x009F).contains(&cp)
        || (0x202A..=0x202E).contains(&cp)
        || (0x2066..=0x2069).contains(&cp)
    {
        return PLACEHOLDER.into();
    }
    match char::from_u32(cp) {
        Some(ch) if ch.width().is_some() => ch.to_string(),
        _ => PLACEHOLDER.into(),
    }
}

/// 没有字形可言的条目显示这个点。
const PLACEHOLDER: &str = "\u{00B7}";

#[cfg(test)]
mod tests {
    use super::*;

    const UCD_SAMPLE: &str = "\
0000;<control>;Cc;0;BN;;;;;N;NULL;;;;
0041;LATIN CAPITAL LETTER A;Lu;0;L;;;;;N;;;;0061;
0085;<control>;Cc;0;BN;;;;;N;NEXT LINE (NEL);;;;
3400;<CJK Ideograph Extension A, First>;Lo;0;L;;;;;N;;;;;
4DBF;<CJK Ideograph Extension A, Last>;Lo;0;L;;;;;N;;;;;
4E00;<CJK Ideograph, First>;Lo;0;L;;;;;N;;;;;
9FFF;<CJK Ideograph, Last>;Lo;0;L;;;;;N;;;;;
D800;<Non Private Use High Surrogate, First>;Cs;0;L;;;;;N;;;;;
DB7F;<Non Private Use High Surrogate, Last>;Cs;0;L;;;;;N;;;;;
1F602;FACE WITH TEARS OF JOY;So;0;ON;;;;;N;;;;;
";

    fn parse_sample() -> Vec<Entry> {
        let mut entries = Vec::new();
        parse_unicode_data(UCD_SAMPLE, &mut entries).unwrap();
        entries
    }

    #[test]
    fn unicode_data_keeps_names_and_control_pictures() {
        let entries = parse_sample();
        let letter = entries.iter().find(|e| e.cp == 0x41).unwrap();
        assert_eq!(letter.label, "LATIN CAPITAL LETTER A");
        assert_eq!(letter.hint, "Lu");

        // 控制符没有字形，用控制图形代替；C1 控制符会被终端当控制序列，必须换占位点
        assert!(entries.iter().any(|e| e.label == "<control>"));
        assert_eq!(entries.iter().find(|e| e.cp == 0x00).unwrap().glyph, "\u{2400}");
        assert_eq!(entries.iter().find(|e| e.cp == 0x85).unwrap().glyph, "\u{00B7}");
    }

    #[test]
    fn unicode_data_collapses_ranges_and_skips_surrogates() {
        let entries = parse_sample();
        assert_eq!(entries.iter().filter(|e| e.cp == 0x3400).count(), 1);
        let range = entries.iter().find(|e| e.cp == 0x3400).unwrap();
        assert_eq!(range.label, "CJK Ideograph Extension A");
        assert_eq!(range.hint, "U+3400..U+4DBF");

        assert!(!entries.iter().any(|e| (0xD800..=0xDFFF).contains(&e.cp)));
    }

    #[test]
    fn emoji_rows_use_primary_alias_as_label() {
        let json = format!(
            r#"[
            {{"emoji":"{}","description":"face with tears of joy",
             "aliases":["joy"],"tags":["face","laugh","tear"]}},
            {{"emoji":"A","description":"letter a","aliases":[],"tags":[]}},
            {{"emoji":"B","description":"","aliases":[],"tags":[]}}
        ]"#,
            '\u{1F602}'
        );
        let mut out = Vec::new();
        parse_emoji(&json, &mut out).unwrap();
        // 无别名但有描述的退回描述；两样都空的丢弃
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].label, ":joy:");
        assert_eq!(out[0].cp, 0x1F602);
        assert_eq!(out[1].label, "letter a");
        // 别名和标签都要能搜到
        assert!(out[0].search.contains("laugh"));
        assert!(out[0].search.contains("tears of joy"));
    }

    #[test]
    fn nerd_glyphs_accept_char_or_code_field() {
        let with_char = NerdGlyph {
            ch: Some("\u{e0b0}".into()),
            code: Some("e0b0".into()),
        };
        assert_eq!(nerd_char(&with_char), Some(("\u{e0b0}".into(), 0xE0B0)));

        let code_only = NerdGlyph {
            ch: Some("0".into()),
            code: Some("f0e0".into()),
        };
        assert_eq!(nerd_char(&code_only), Some(("\u{f0e0}".into(), 0xF0E0)));

        let metadata = NerdGlyph {
            ch: None,
            code: None,
        };
        assert_eq!(nerd_char(&metadata), None);
    }

    #[test]
    fn nerd_table_skips_metadata_entry() {
        let json = r#"{
            "METADATA": {"version": "3.0"},
            "fa-smile_o": {"char": "\uf118", "code": "f118"}
        }"#;
        let mut out = Vec::new();
        parse_nerd(json, &mut out).unwrap();
        assert_eq!(out.len(), 1);
        // 名字表里的键没有 nf- 前缀，展示时补回来
        assert_eq!(out[0].label, "nf-fa-smile_o");
        assert_eq!(out[0].cp, 0xF118);
        // search 走 canonical，分隔符折成空格，所以 "fa-smile" 与 "fa smile" 等价
        assert_eq!(out[0].search, "nf fa smile o");
    }

    #[test]
    fn wall_keeps_curated_sources_and_art_ranges() {
        let make = |glyph: &str, cp: u32, label: &str, source: Source| Entry {
            glyph: glyph.into(),
            cp,
            label: label.into(),
            label_key_len: label.len() as u32,
            hint: String::new(),
            search: canonical(label),
            source,
        };
        // 顺序与真实索引一致：emoji、nerd、unicode
        let index = Index::new(vec![
            make("\u{1F602}", 0x1F602, ":joy:", Source::Emoji),
            make("\u{f118}", 0xF118, "nf-fa-smile_o", Source::Nerd),
            make("a", 0x41, "LATIN CAPITAL LETTER A", Source::Unicode),
            make("-", 0x2500, "BOX DRAWINGS LIGHT HORIZONTAL", Source::Unicode),
            make("\u{2801}", 0x2801, "BRAILLE PATTERN DOTS-1", Source::Unicode),
            make("\u{4E2D}", 0x4E2D, "CJK IDEOGRAPH", Source::Unicode),
        ]);
        // ASCII 与 CJK 不进墙；线框、braille 跟在 emoji/nerd 后面
        assert_eq!(index.wall(), vec![0, 1, 3, 4]);
    }

    #[test]
    fn canonical_folds_separators_and_case() {
        assert_eq!(canonical("nf-fa-SMILE_o"), "nf fa smile o");
        assert_eq!(canonical("  :joy:  "), "joy");
        assert_eq!(canonical("FACE WITH TEARS OF JOY"), "face with tears of joy");
        assert_eq!(canonical(""), "");
    }

    #[test]
    fn glyph_cell_pads_to_three_columns() {
        assert_eq!(glyph_cell("A"), "A  ");
        assert_eq!(glyph_cell("\u{4E2D}"), "\u{4E2D} ");
        // 比 3 列宽的字形原样保留，不会补出负数的空格
        assert_eq!(glyph_cell("abcd"), "abcd");
    }
}
