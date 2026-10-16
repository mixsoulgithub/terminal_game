//! graph-tui:终端里的通用图渲染引擎.
//!
//! 本文件只负责命令行、终端进出与主循环;图数据在 model.rs,解析在 dot.rs,
//! 布局在 layout.rs,画法在 canvas.rs + render.rs,交互在 app.rs.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, TerminalOptions, Viewport};

use graph_tui::app::{Action, App, Options};
use graph_tui::lang::Lang;
use graph_tui::layout::Layout;
use graph_tui::model::Graph;

const USAGE: &str = "\
graph-tui - a universal graph renderer for the terminal
(box-drawing edges, one true colour per group, orthogonal routing, interactive pan/zoom)

usage: graph-tui [options] [file.dot]

With no file, the graph is read from stdin.

options:
  --file <path>       read a graph .dot file (same as giving it as an argument; '-' = stdin)
  --layout <name>     geo | relax | force | layered. Default: geo when every node has a
                      position, otherwise layered for acyclic graphs and force otherwise
  --dump              render one frame to stdout (true-colour ANSI) and exit; use with --size
  --size <WxH>        canvas size for --dump (default 200x50)
  --labels            start with every node name drawn (same as pressing b)
  --at <name>         start with the cursor on that node
  --find <text>       run a search at startup (name / alias / group number)
  --lang <zh|en>      language of the app's own text (default: zh). The help and errors
                      on this page stay English either way
  -v, --version       print the version and exit
  -h, --help          show this help and exit

input (.dot subset):
  digraph deps {                          graph or digraph (digraph: edges get an arrowhead)
    node [shape=box];                     default attributes for nodes / edges
    \"a\" [pos=\"0,0\", label=\"A\", terms=\"alpha|ay\"];   pos is optional, x right / y up
    \"a\" -> \"b\" [color=\"#4c8ef8\", group=\"deps\"];      group decides colour and walking
  }

layouts:
  geo      use the positions from the input as they are
  relax    spring-relax those positions so spacing evens out and runs snap to - | / \\\\
  force    force-directed, for graphs with no positions at all
  layered  layered (Sugiyama), for directed dependency graphs and flowcharts

keys:
  h j k l / arrows   move to the nearest node on screen in that direction
  H K / J L          step one node along the group (its own order, not geography)
  + - / 0            zoom in / out / reset
  space z            centre the view on the cursor node
  f                  fit the focused group to the screen
  b                  toggle node labels
  /                  search names or aliases; a bare number focuses that group
  n / N              next / previous search hit
  V                  focus the groups through the cursor node (again to clear)
  r                  cycle the layout
  Ctrl-d / Ctrl-u    jump half a screen down / up
  Ctrl-o / Ctrl-i    back / forward through the nodes you jumped to
  ?                  key reference (any key closes it)
  :                  command line: layout <name> | side <x> | cycle <x> | lang <zh|en>
  mouse              click = jump, drag = pan, wheel = zoom, Ctrl+wheel = pan
  q / esc            quit
";

struct Args {
    file: Option<String>,
    layout: Option<Layout>,
    dump: bool,
    size: (u16, u16),
    labels: bool,
    at: Option<String>,
    find: Option<String>,
    lang: Lang,
    help: bool,
    version: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            file: None,
            layout: None,
            dump: false,
            size: (200, 50),
            labels: false,
            at: None,
            find: None,
            lang: Lang::default(),
            help: false,
            version: false,
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        let mut next = |what: &str| -> Result<String, String> {
            i += 1;
            argv.get(i)
                .cloned()
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match arg {
            "-h" | "--help" => a.help = true,
            "-v" | "--version" => a.version = true,
            "--dump" => a.dump = true,
            "--labels" => a.labels = true,
            "--file" => a.file = Some(next("--file")?),
            "--at" => a.at = Some(next("--at")?),
            "--find" => a.find = Some(next("--find")?),
            "--layout" => {
                let v = next("--layout")?;
                a.layout = Some(Layout::parse(&v).ok_or_else(|| format!("unknown layout {v}"))?);
            }
            "--lang" => {
                let v = next("--lang")?;
                a.lang = Lang::parse(&v).ok_or_else(|| format!("unknown language {v}"))?;
            }
            "--size" => {
                let v = next("--size")?;
                a.size = parse_size(&v)?;
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}")),
            s => {
                if a.file.is_some() {
                    return Err(format!("only one input file is allowed (already got one, then {s})"));
                }
                a.file = Some(s.to_string());
            }
        }
        i += 1;
    }
    Ok(a)
}

fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("--size wants WxH, got {s}"))?;
    let w: u16 = w.trim().parse().map_err(|_| format!("bad width in {s}"))?;
    let h: u16 = h.trim().parse().map_err(|_| format!("bad height in {s}"))?;
    if w < 8 || h < 4 {
        return Err("canvas too small (at least 8x4)".to_string());
    }
    Ok((w, h))
}

/// 读图:文件、stdin("-") 或者没有参数时也从 stdin 读
fn source_text(a: &Args) -> Result<(String, Option<String>), String> {
    match a.file.as_deref() {
        Some(p) if p != "-" => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?;
            Ok((text, Some(p.to_string())))
        }
        _ => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            if text.trim().is_empty() {
                return Err("no input: pass a .dot file, or pipe the graph into stdin".to_string());
            }
            Ok((text, None))
        }
    }
}

/// 打印一帧(真彩 ANSI)
fn dump(buf: &ratatui::buffer::Buffer) {
    let mut out = String::new();
    for y in 0..buf.area.height {
        // 全角字的第二格不用再输出(宽字本身已经占了两列)
        let mut skip = 0usize;
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if skip > 0 {
                skip -= 1;
                continue;
            }
            skip = unicode_width::UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
            let (r, g, b) = match cell.fg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (212, 218, 232),
            };
            let (br, bg, bb) = match cell.bg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (9, 10, 17),
            };
            let mut sgr = format!("\x1b[38;2;{r};{g};{b};48;2;{br};{bg};{bb}");
            if cell.modifier.contains(Modifier::BOLD) {
                sgr.push_str(";1");
            }
            if cell.modifier.contains(Modifier::DIM) {
                sgr.push_str(";2");
            }
            sgr.push('m');
            out.push_str(&sgr);
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    print!("{out}");
    let _ = io::stdout().flush();
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let a = parse_args(&argv)?;
    if a.help {
        print!("{USAGE}");
        return Ok(());
    }
    if a.version {
        println!("graph-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let (text, source) = source_text(&a)?;
    let graph = Graph::from_dot(&text)?;
    let opts = Options {
        layout: a.layout,
        labels: a.labels,
        at: a.at.clone(),
        find: a.find.clone(),
        lang: a.lang,
    };
    let mut app = App::new(graph, opts)?;
    app.source = source;

    if a.dump {
        let backend = TestBackend::new(a.size.0, a.size.1);
        let mut term = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, a.size.0, a.size.1)),
            },
        )
        .map_err(|e| format!("cannot start the terminal: {e}"))?;
        term.draw(|f| {
            let area = f.area();
            app.draw(f.buffer_mut(), area);
        })
        .map_err(|e| format!("cannot draw: {e}"))?;
        dump(term.backend().buffer());
        return Ok(());
    }

    let mut out = io::stdout();
    enable_raw_mode().map_err(|e| format!("cannot enter raw mode: {e}"))?;
    execute!(out, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| format!("cannot enter the alternate screen: {e}"))?;
    let res = event_loop(&mut app, out);
    let mut out = io::stdout();
    let _ = disable_raw_mode();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture);
    res
}

fn event_loop(app: &mut App, out: io::Stdout) -> Result<(), String> {
    let mut term =
        Terminal::new(CrosstermBackend::new(out)).map_err(|e| format!("cannot start the terminal: {e}"))?;
    loop {
        term.draw(|f| {
            let area = f.area();
            app.draw(f.buffer_mut(), area);
        })
        .map_err(|e| format!("cannot draw: {e}"))?;
        app.tick();
        // 没有动画在跑就少醒几次
        let wait = if app.busy() { 16 } else { 250 };
        if event::poll(Duration::from_millis(wait)).map_err(|e| format!("cannot read events: {e}"))? {
            let ev = event::read().map_err(|e| format!("cannot read events: {e}"))?;
            if matches!(ev, Event::Resize(..)) {
                continue;
            }
            if app.handle(ev) == Action::Quit {
                return Ok(());
            }
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("graph-tui: {e}");
            eprintln!("try graph-tui --help");
            ExitCode::FAILURE
        }
    }
}
