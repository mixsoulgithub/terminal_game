// uni-browser: 查询 emoji、nerd font 图标与 Unicode 码点。
// 无参数进 TUI，带查询词直接在终端打印结果。

mod data;
mod search;
mod tui;

use std::process::ExitCode;

const USAGE: &str = "\
uni-browser - query emoji, nerd font icons and unicode codepoints

usage:
  uni-browser              TUI: glyph wall (emoji, nerd font icons, art ranges)
  uni-browser <query>      print matches in the terminal
  uni-browser -h|--help    this help

query:
  :laugh                   emoji only: shortcode, alias, tag, description
  laugh                    all sources: unicode names, emoji, nerd font icons
  nf-fa-smile              nerd font icon names

what the wall shows (search always covers everything):
  emoji, nerd font icons, box drawing / blocks / braille, arrows, math,
  geometric shapes, misc symbols, dingbats, mahjong/dominoes/cards,
  legacy computing blocks

data:
  downloaded once with curl into $XDG_CACHE_HOME/uni-browser
  (default ~/.cache/uni-browser); delete that directory to refresh
  sources: github/gemoji, nerd-fonts glyphnames.json, UnicodeData.txt

keys in the TUI:
  h j k l    move by glyph / row    /    search every codepoint as a list
  g / G      first / last glyph     ?    find: jump to the next match
  PgUp/PgDn  half a screen (Ctrl+U/D too)
  :          command line (:help, :q)
  q          quit
";

/// CLI 模式最多打印多少条，其余只报数量。
const CLI_LIMIT: usize = 50;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        None => run_tui(),
        Some(_) => run_cli(&args.join(" ")),
    }
}

fn run_tui() -> ExitCode {
    let index = match data::load(true) {
        Ok(index) => index,
        Err(err) => return fail(&err),
    };

    match tui::run(index) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => fail(&format!("TUI failed: {err}")),
    }
}

fn run_cli(query: &str) -> ExitCode {
    let index = match data::load(true) {
        Ok(index) => index,
        Err(err) => return fail(&err),
    };

    let hits = search::hits(&index, query);
    if hits.is_empty() {
        eprintln!("uni-browser: no match for {query}");
        return ExitCode::from(1);
    }

    for idx in hits.iter().take(CLI_LIMIT) {
        let entry = &index.entries[*idx];
        let mut line = format!(
            "{}  U+{:04X}  {}  [{}]",
            data::glyph_cell(&entry.glyph),
            entry.cp,
            entry.label,
            entry.source.tag()
        );
        if !entry.hint.is_empty() {
            line.push_str("  ");
            line.push_str(&entry.hint);
        }
        println!("{line}");
    }

    if hits.len() > CLI_LIMIT {
        eprintln!(
            "uni-browser: {} more matches (showing {CLI_LIMIT})",
            hits.len() - CLI_LIMIT
        );
    }

    ExitCode::SUCCESS
}

fn fail(message: &str) -> ExitCode {
    eprintln!("uni-browser: {message}");
    ExitCode::FAILURE
}
