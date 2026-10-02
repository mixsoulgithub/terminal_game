// neon:一个 less 式的分页器,配色会流动.
// 本文件负责命令行解析、终端进入/退出、事件循环,以及跟随文件增长.
mod doc;
mod neon;
mod pager;
mod ui;

use std::fs;
use std::io::{self, IsTerminal, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;

use doc::Doc;
use neon::Flow;
use pager::{Options, Pager};
use ui::{Prompt, PromptKind};

// 跟随模式查文件大小的间隔
const FOLLOW_POLL: Duration = Duration::from_millis(200);

const USAGE: &str = "\
neon - a less-like pager with a flowing neon palette

usage: neon [options] [file]
       command | neon [options]

options:
  -n, --numbers    show line numbers
  -S, --no-wrap    do not wrap long lines
      --word-wrap  wrap at word boundaries
  -f, --follow     follow the file as it grows (like tail -f)
      --no-anim    disable the color flow (static neon)
      --rainbow    tint every glyph with the flowing gradient
      --fps <n>    animation frames per second (default 30)
      --flow <x>   color flow speed multiplier (default 1.0)
  -h, --help       show this help
  -V, --version    show version

keys:
  j / k       line            space / b    page          d / u   half page
  g / G       top/bottom      / ?          search        n / N   next/prev match
  &           filter lines    esc          clear search (quits if none)  q  quit
  m           wrap            w            word wrap     F       follow the file
  l           numbers         a / r        animation      h       help
  + / -       flow speed      mouse wheel  scroll

needs a terminal with 24-bit color support (COLORTERM=truecolor).
";

struct Args {
    file: Option<String>,
    numbers: bool,
    wrap: bool,
    word_wrap: bool,
    follow: bool,
    anim: bool,
    rainbow: bool,
    fps: f64,
    flow: f32,
}

enum Parsed {
    Run(Args),
    Help,
    Version,
}

fn parse(argv: impl Iterator<Item = String>) -> Result<Parsed, String> {
    let mut a = Args {
        file: None,
        numbers: false,
        wrap: true,
        word_wrap: false,
        follow: false,
        anim: true,
        rainbow: false,
        fps: 30.0,
        flow: 1.0,
    };
    let mut it = argv.peekable();
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match key.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-n" | "--numbers" => a.numbers = true,
            "-S" | "--no-wrap" => a.wrap = false,
            "--word-wrap" => a.word_wrap = true,
            "-f" | "--follow" => a.follow = true,
            "--no-anim" => a.anim = false,
            "--rainbow" => a.rainbow = true,
            "--fps" => {
                let v = value("--fps")?;
                a.fps = v.parse().map_err(|_| format!("bad --fps: {v}"))?;
            }
            "--flow" => {
                let v = value("--flow")?;
                a.flow = v.parse().map_err(|_| format!("bad --flow: {v}"))?;
            }
            "--" => {
                if let Some(f) = it.next() {
                    a.file = Some(f);
                }
                break;
            }
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option: {s}")),
            s => {
                if a.file.is_some() {
                    return Err("only one file can be opened".into());
                }
                a.file = Some(s.to_string());
            }
        }
    }
    Ok(Parsed::Run(a))
}

fn read_stdin() -> io::Result<String> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn load(args: &Args) -> io::Result<(String, String)> {
    match args.file.as_deref() {
        Some("-") => Ok(("(stdin)".into(), read_stdin()?)),
        Some(path) => {
            let bytes = fs::read(path)?;
            Ok((path.to_string(), String::from_utf8_lossy(&bytes).into_owned()))
        }
        None => {
            if io::stdin().is_terminal() {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "no input: give a file, or pipe something in",
                ))
            } else {
                Ok(("(stdin)".into(), read_stdin()?))
            }
        }
    }
}

fn main() {
    let args = match parse(std::env::args().skip(1)) {
        Ok(Parsed::Run(a)) => a,
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Parsed::Version) => {
            println!("neon {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(e) => {
            eprintln!("neon: {e}");
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    let (name, text) = match load(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("neon: {e}");
            std::process::exit(1);
        }
    };
    let doc = Doc::new(name, &text);
    let mut pager = Pager::new(
        doc,
        Options {
            wrap: args.wrap,
            word_wrap: args.word_wrap,
            numbers: args.numbers,
            rainbow: args.rainbow,
            follow: false,
        },
    );
    // 跟随需要真实文件;从管道来的内容没法跟随
    let tail = match (args.follow, args.file.as_deref()) {
        (true, Some(p)) if p != "-" => {
            pager.start_following();
            Some(Tail::new(Path::new(p)))
        }
        (true, _) => {
            pager.status = "follow needs a file".into();
            None
        }
        _ => None,
    };
    let mut flow = Flow::new(args.flow);
    flow.enabled = args.anim;
    if let Err(e) = run(pager, flow, args.fps, tail) {
        eprintln!("neon: {e}");
        std::process::exit(1);
    }
}

// 跟随文件增长:只读新增的那一段;跨读到一半的多字节字符留在 pending 里等下一次
struct Tail {
    path: PathBuf,
    pos: u64,
    pending: Vec<u8>,
}

impl Tail {
    fn new(path: &Path) -> Tail {
        Tail {
            path: path.to_path_buf(),
            pos: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            pending: Vec::new(),
        }
    }

    // 返回 true 表示 doc 有新内容写进去了
    fn poll(&mut self, doc: &mut Doc) -> bool {
        let Ok(meta) = fs::metadata(&self.path) else {
            return false;
        };
        let len = meta.len();
        if len < self.pos {
            // 被截断或轮转:整份重读
            let Ok(bytes) = fs::read(&self.path) else {
                return false;
            };
            self.pos = len;
            self.pending.clear();
            doc.replace(&String::from_utf8_lossy(&bytes));
            return true;
        }
        if len == self.pos {
            return false;
        }
        let Ok(mut f) = fs::File::open(&self.path) else {
            return false;
        };
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return false;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return false;
        }
        self.pos += buf.len() as u64;
        self.pending.extend_from_slice(&buf);
        let text = take_utf8(&mut self.pending);
        if text.is_empty() {
            return false;
        }
        doc.append(&text);
        true
    }
}

// 取出 pending 里完整的 UTF-8 前缀,残缺的尾巴留到下一次;真正的坏字节换成 U+FFFD
fn take_utf8(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                if let Ok(s) = std::str::from_utf8(&pending[..good]) {
                    out.push_str(s);
                }
                match e.error_len() {
                    None => {
                        pending.drain(..good);
                        break;
                    }
                    Some(bad) => {
                        out.push('\u{fffd}');
                        pending.drain(..good + bad);
                    }
                }
            }
        }
    }
    out
}

fn run(mut pager: Pager, mut flow: Flow, fps: f64, mut tail: Option<Tail>) -> io::Result<()> {
    // 终端不可用(例如 stdout 被重定向)时给一句人话,不要 panic
    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            ratatui::restore();
            return Err(io::Error::new(
                e.kind(),
                format!("cannot run here: {e} (needs a terminal on stdout and a tty for keys)"),
            ));
        }
    };
    execute!(io::stdout(), EnableMouseCapture)?;
    let period = Duration::from_secs_f64(1.0 / fps.clamp(1.0, 240.0));
    let mut last = Instant::now();
    let mut next_poll = Instant::now();
    let mut mode: Option<Prompt> = None;
    let mut quit = false;
    let mut dirty = true;

    while !quit {
        // 跟随:按固定间隔查一次文件大小
        if pager.follow && Instant::now() >= next_poll {
            next_poll = Instant::now() + FOLLOW_POLL;
            if let Some(t) = tail.as_mut() {
                if t.poll(&mut pager.doc) {
                    pager.content_changed();
                    dirty = true;
                }
            }
        }
        let size = terminal.size()?;
        pager.ensure_layout(size.width, size.height);
        if dirty || flow.enabled {
            terminal.draw(|f| ui::render(f, &pager, &flow, mode.as_ref()))?;
            dirty = false;
        }
        // 动画时按帧率唤醒;跟随文件时至少这个频率看一眼;否则一直等到有事件
        let mut timeout = Duration::from_secs(3600);
        if flow.enabled {
            timeout = timeout.min(period);
        }
        if pager.follow {
            timeout = timeout.min(FOLLOW_POLL);
        }
        if event::poll(timeout)? {
            dirty = true;
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    on_key(&mut pager, &mut flow, k, &mut mode, &mut quit, tail.is_some());
                }
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollDown => pager.scroll(3),
                    MouseEventKind::ScrollUp => pager.scroll(-3),
                    _ => {}
                },
                Event::Resize(..) => {}
                _ => {}
            }
        }
        let now = Instant::now();
        flow.tick((now - last).as_secs_f32());
        last = now;
    }

    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}

fn on_key(
    pager: &mut Pager,
    flow: &mut Flow,
    key: KeyEvent,
    mode: &mut Option<Prompt>,
    quit: &mut bool,
    has_tail: bool,
) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        *quit = true;
        return;
    }
    if let Some(prompt) = mode.as_mut() {
        match key.code {
            KeyCode::Char(c) => prompt.text.push(c),
            KeyCode::Backspace => {
                prompt.text.pop();
            }
            KeyCode::Enter => {
                let (text, kind) = (prompt.text.clone(), prompt.kind);
                *mode = None;
                match kind {
                    PromptKind::SearchFwd => pager.search(&text, false),
                    PromptKind::SearchBack => pager.search(&text, true),
                    PromptKind::Filter => pager.set_filter(&text),
                }
            }
            KeyCode::Esc => *mode = None,
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Char('q') => *quit = true,
        // 有搜索痕迹时先清除,再按才退出
        KeyCode::Esc => {
            if pager.matches.is_empty() {
                *quit = true;
            } else {
                pager.clear_search();
            }
        }
        KeyCode::Char('j') | KeyCode::Down => pager.scroll(1),
        KeyCode::Char('k') | KeyCode::Up => pager.scroll(-1),
        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => pager.page(1),
        KeyCode::Char('b') | KeyCode::PageUp => pager.page(-1),
        KeyCode::Char('d') => pager.half_page(1),
        KeyCode::Char('u') => pager.half_page(-1),
        KeyCode::Char('g') | KeyCode::Home => pager.to_top(),
        KeyCode::Char('G') | KeyCode::End => pager.to_bottom(),
        KeyCode::Char('/') => {
            *mode = Some(Prompt {
                text: String::new(),
                kind: PromptKind::SearchFwd,
            })
        }
        KeyCode::Char('?') => {
            *mode = Some(Prompt {
                text: String::new(),
                kind: PromptKind::SearchBack,
            })
        }
        KeyCode::Char('&') => {
            *mode = Some(Prompt {
                text: String::new(),
                kind: PromptKind::Filter,
            })
        }
        KeyCode::Char('n') => pager.next_match(1),
        KeyCode::Char('N') => pager.next_match(-1),
        KeyCode::Char('h') => pager.help = !pager.help,
        KeyCode::Char('m') => pager.toggle_wrap(),
        KeyCode::Char('w') => pager.toggle_word_wrap(),
        KeyCode::Char('l') => pager.toggle_numbers(),
        KeyCode::Char('a') => {
            flow.enabled = !flow.enabled;
        }
        KeyCode::Char('r') => pager.rainbow = !pager.rainbow,
        KeyCode::Char('F') => {
            if !has_tail {
                pager.status = "follow needs a file".into();
            } else {
                if pager.follow {
                    pager.follow = false;
                } else {
                    pager.start_following();
                }
                pager.status = match pager.follow {
                    true => "following".into(),
                    false => "follow off".into(),
                };
            }
        }
        KeyCode::Char('+') | KeyCode::Char('=') => flow.speed_up(1.5),
        KeyCode::Char('-') | KeyCode::Char('_') => flow.speed_up(1.0 / 1.5),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(v: &[&str]) -> Result<Parsed, String> {
        parse(v.iter().map(|s| s.to_string()))
    }

    fn new_pager(text: &str) -> Pager {
        let mut p = Pager::new(
            Doc::new("t".into(), text),
            Options {
                wrap: true,
                word_wrap: false,
                numbers: false,
                rainbow: false,
                follow: false,
            },
        );
        p.ensure_layout(20, 12);
        p
    }

    fn press(p: &mut Pager, f: &mut Flow, m: &mut Option<Prompt>, q: &mut bool, code: KeyCode) {
        on_key(p, f, KeyEvent::new(code, KeyModifiers::NONE), m, q, true);
    }

    fn banana_file() -> String {
        (0..30)
            .map(|i| {
                if i == 5 || i == 25 {
                    "banana".to_string()
                } else {
                    format!("line {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn defaults_are_a_wrapping_animated_pager() {
        let Ok(Parsed::Run(a)) = args_of(&["notes.txt"]) else {
            panic!("expected a runnable config");
        };
        assert_eq!(a.file.as_deref(), Some("notes.txt"));
        assert!(a.wrap && a.anim && !a.numbers && !a.rainbow);
        assert!(!a.follow && !a.word_wrap);
        assert_eq!(a.fps, 30.0);
        assert_eq!(a.flow, 1.0);
    }

    #[test]
    fn flags_and_values() {
        let Ok(Parsed::Run(a)) =
            args_of(&["-n", "-S", "--rainbow", "--word-wrap", "-f", "--fps", "60", "--flow=2.5", "f"])
        else {
            panic!("expected a runnable config");
        };
        assert!(a.numbers && !a.wrap && a.rainbow && a.word_wrap && a.follow);
        assert_eq!(a.fps, 60.0);
        assert_eq!(a.flow, 2.5);
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert!(matches!(args_of(&["--help"]), Ok(Parsed::Help)));
        assert!(matches!(args_of(&["-V"]), Ok(Parsed::Version)));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(args_of(&["--nope"]).is_err());
        assert!(args_of(&["--fps"]).is_err());
        assert!(args_of(&["--fps", "fast"]).is_err());
        assert!(args_of(&["a", "b"]).is_err());
    }

    #[test]
    fn double_dash_takes_the_rest_as_a_file() {
        let Ok(Parsed::Run(a)) = args_of(&["--", "--weird-name"]) else {
            panic!("expected a runnable config");
        };
        assert_eq!(a.file.as_deref(), Some("--weird-name"));
    }

    #[test]
    fn dash_means_stdin() {
        let Ok(Parsed::Run(a)) = args_of(&["-"]) else {
            panic!("expected a runnable config");
        };
        assert_eq!(a.file.as_deref(), Some("-"));
    }

    #[test]
    fn slash_opens_a_prompt_and_enter_searches() {
        let mut p = new_pager("apple\nbanana\ncherry\nbanana\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('/'));
        assert!(mode.is_some());
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('b'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('a'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Backspace);
        assert_eq!(mode.as_ref().unwrap().text, "b");
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert!(mode.is_none());
        assert_eq!(p.matches.len(), 2);
    }

    #[test]
    fn prompt_mode_swallows_command_keys() {
        let mut p = new_pager("apple\nbanana\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('/'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('j'));
        assert_eq!(mode.as_ref().unwrap().text, "j");
        assert_eq!(p.top, 0);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('q'));
        assert!(!quit);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Esc);
        assert!(mode.is_none());
    }

    #[test]
    fn question_mark_searches_backward_from_the_viewport() {
        let mut p = new_pager(&banana_file());
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        p.scroll(15); // 视口 15..25
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('/'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('B'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert!(p.matches.is_empty()); // 大写字母 -> 区分大小写
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('?'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('b'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert_eq!(p.matches.len(), 2);
        assert_eq!(p.cur, Some(0)); // 视口上方最近的一个
    }

    #[test]
    fn ampersand_filters_lines() {
        let mut p = new_pager("keep one\ndrop\nalso keep\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('&'));
        assert_eq!(mode.as_ref().unwrap().kind, PromptKind::Filter);
        for c in "keep".chars() {
            press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char(c));
        }
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert_eq!(p.view().len(), 2);
        // 空过滤清除
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('&'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert_eq!(p.view().len(), 3);
        assert_eq!(p.filter(), None);
    }

    #[test]
    fn follow_key_needs_a_file() {
        let mut p = new_pager("a\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        on_key(&mut p, &mut f, KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE), &mut mode, &mut quit, false);
        assert!(!p.follow);
        assert_eq!(p.status, "follow needs a file");
        on_key(&mut p, &mut f, KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE), &mut mode, &mut quit, true);
        assert!(p.follow);
        assert_eq!(p.status, "following");
        on_key(&mut p, &mut f, KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE), &mut mode, &mut quit, true);
        assert!(!p.follow);
        assert_eq!(p.status, "follow off");
    }

    #[test]
    fn esc_clears_the_search_before_quitting() {
        let mut p = new_pager("apple\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('/'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('a'));
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Enter);
        assert_eq!(p.matches.len(), 1);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Esc);
        assert!(!quit);
        assert!(p.matches.is_empty());
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Esc);
        assert!(quit);
    }

    #[test]
    fn toggle_keys_flip_their_own_switch() {
        let mut p = new_pager(&banana_file());
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('m'));
        assert!(!p.wrap);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('w'));
        assert!(p.word_wrap);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('l'));
        assert!(p.numbers);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('r'));
        assert!(p.rainbow);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('a'));
        assert!(!f.enabled);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('h'));
        assert!(p.help);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('G'));
        assert_eq!(p.top, p.max_top());
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('g'));
        assert_eq!(p.top, 0);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('j'));
        assert_eq!(p.top, 1);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('q'));
        assert!(quit);
    }

    #[test]
    fn ctrl_c_quits_even_from_the_prompt() {
        let mut p = new_pager("a\n");
        let mut f = Flow::new(1.0);
        let (mut mode, mut quit) = (None, false);
        press(&mut p, &mut f, &mut mode, &mut quit, KeyCode::Char('/'));
        on_key(
            &mut p,
            &mut f,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut mode,
            &mut quit,
            true,
        );
        assert!(quit);
    }

    #[test]
    fn take_utf8_waits_for_a_split_multibyte_char() {
        let mut pending = Vec::new();
        let text = "霓虹";
        let bytes = text.as_bytes();
        pending.extend_from_slice(&bytes[..2]); // 第一个字只到一半
        assert_eq!(take_utf8(&mut pending), "");
        assert_eq!(pending.len(), 2);
        pending.extend_from_slice(&bytes[2..]);
        assert_eq!(take_utf8(&mut pending), "霓虹");
        assert!(pending.is_empty());
    }

    #[test]
    fn take_utf8_replaces_invalid_bytes() {
        let mut pending = b"ok\xff\xfeend".to_vec();
        assert_eq!(take_utf8(&mut pending), "ok\u{fffd}\u{fffd}end");
        assert!(pending.is_empty());
    }

    fn temp_file(tag: &str, body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("neon_{tag}_{}.txt", std::process::id()));
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn tail_only_reads_what_is_new() {
        let path = temp_file("tail", "one\ntwo\n");
        let mut doc = Doc::new("t".into(), "");
        doc.replace(&fs::read_to_string(&path).unwrap());
        let mut t = Tail::new(&path);
        assert_eq!(t.pos, 8);
        assert!(!t.poll(&mut doc)); // 没有新内容
        assert_eq!(doc.len(), 2);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, b"three\n").unwrap();
        f.sync_all().unwrap();
        drop(f);
        assert!(t.poll(&mut doc));
        assert_eq!(doc.len(), 3);
        assert_eq!(doc.line(2), "three");
        assert!(!t.poll(&mut doc));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tail_reloads_after_a_truncate() {
        let path = temp_file("trunc", "aaa\nbbb\nccc\n");
        let mut doc = Doc::new("t".into(), &fs::read_to_string(&path).unwrap());
        let mut t = Tail::new(&path);
        fs::write(&path, "z\n").unwrap();
        assert!(t.poll(&mut doc));
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.line(0), "z");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tail_keeps_a_half_line_until_it_completes() {
        let path = temp_file("half", "a\n");
        let mut doc = Doc::new("t".into(), &fs::read_to_string(&path).unwrap());
        let mut t = Tail::new(&path);
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, b"par").unwrap();
        f.sync_all().unwrap();
        assert!(t.poll(&mut doc));
        assert_eq!(doc.len(), 2);
        assert_eq!(doc.line(1), "par");
        std::io::Write::write_all(&mut f, b"tial\n").unwrap();
        f.sync_all().unwrap();
        drop(f);
        assert!(t.poll(&mut doc));
        assert_eq!(doc.len(), 2);
        assert_eq!(doc.line(1), "partial");
        let _ = fs::remove_file(&path);
    }
}
