// swatches:真彩终端的色卡.用 vim 键位在色块网格上挑颜色,yy 把 #RRGGBB 送进剪贴板.
// 本文件负责命令行解析、终端进出、事件循环和键位.

mod copy;
mod palette;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use palette::{Grid, Kind, ALL, SWATCH_W};

const USAGE: &str = "\
swatches - pick a color off a swatch grid and copy its #RRGGBB

usage: swatches [palette]

palettes:
  hue       hue across the row, value down the rows (default)
  sat       hue across the row, saturation down the rows
  ansi16    the classic 16 colors
  ansi256   the xterm 256-color table
  gray      32 steps of gray

options:
  -h, --help       show this help
  -V, --version    show version

keys:
  h / l / j / k    move the cursor (arrows work too)
  0 / $            start / end of the row
  g then g         top of this column, same column
  G                bottom of this column, same column
  y then y         copy the hex code of the current swatch
  tab / shift-tab  next / previous palette    1 .. 5  pick one directly
  ?                toggle the help             q / esc  quit

needs a terminal with 24-bit color support (COLORTERM=truecolor).
";

// 两键序列的待命状态:按了第一个键,等第二个键
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pending {
    Copy,
    ColumnTop,
}

pub struct State {
    pub kind: usize,
    pub grid: Grid,
    pub cursor: usize,
    pub pending: Option<Pending>,
    pub status: String,
    pub help: bool,
}

impl State {
    fn new(kind: usize) -> State {
        // crossterm 会在 NO_COLOR 下把所有颜色序列吞掉,那样色卡只剩空格子,得说清楚
        let status = if crossterm::style::Colored::ansi_color_disabled() {
            "NO_COLOR is set: colors are suppressed, unset it to see the swatches".to_string()
        } else {
            String::new()
        };
        State {
            kind,
            grid: ALL[kind].build(1, 1),
            cursor: 0,
            pending: None,
            status,
            help: false,
        }
    }

    // 终端尺寸变了就重铺色卡;光标落在空位上时自动贴到最近的实心格子
    fn rebuild(&mut self, rows: usize, cols: usize) {
        if rows == 0 || cols == 0 {
            return;
        }
        self.grid = ALL[self.kind].build(rows, cols);
        let last = self.grid.cells.len().saturating_sub(1);
        self.cursor = self.grid.nearest_used(self.cursor.min(last));
    }

    fn set_kind(&mut self, kind: usize) {
        if kind >= ALL.len() || kind == self.kind {
            return;
        }
        self.kind = kind;
        self.status = format!("palette {}", ALL[kind].name());
    }

    fn cycle(&mut self, delta: i32) {
        let n = ALL.len() as i32;
        self.set_kind((self.kind as i32 + delta).rem_euclid(n) as usize);
    }

    fn move_cursor(&mut self, dr: i32, dc: i32) {
        self.cursor = self.grid.step(self.cursor, dr, dc);
    }

    fn copy_selected(&mut self) {
        let Some(color) = self.grid.at(self.cursor) else {
            self.status = "empty swatch".into();
            return;
        };
        let hex = color.hex();
        self.status = match copy::copy(&hex) {
            Ok(how) => format!("copied {hex}  ({how})"),
            Err(e) => format!("copy failed: {e}"),
        };
    }
}

struct Args {
    kind: usize,
}

enum Parsed {
    Run(Args),
    Help,
    Version,
}

fn parse(argv: impl Iterator<Item = String>) -> Result<Parsed, String> {
    let mut args = Args { kind: 0 };
    for arg in argv {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option: {s}")),
            s => match Kind::from_slug(s) {
                Some(k) => args.kind = ALL.iter().position(|x| *x == k).unwrap(),
                None => return Err(format!("unknown palette: {s}")),
            },
        }
    }
    Ok(Parsed::Run(args))
}

fn main() {
    let args = match parse(std::env::args().skip(1)) {
        Ok(Parsed::Run(a)) => a,
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Parsed::Version) => {
            println!("swatches {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Err(e) => {
            eprintln!("swatches: {e}");
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(State::new(args.kind)) {
        eprintln!("swatches: {e}");
        std::process::exit(1);
    }
}

fn run(mut st: State) -> io::Result<()> {
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
    let mut quit = false;
    while !quit {
        let size = terminal.size()?;
        if let Some(reg) = ui::regions(Rect::new(0, 0, size.width, size.height)) {
            st.rebuild(
                reg.body.height as usize,
                (reg.body.width / SWATCH_W as u16) as usize,
            );
        }
        terminal.draw(|f| ui::render(f, &st))?;
        // 画面是静的:一直睡到有键或窗口变化为止
        if event::poll(Duration::from_secs(3600))? {
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => on_key(&mut st, k, &mut quit),
                _ => {}
            }
        }
    }
    ratatui::restore();
    Ok(())
}

fn on_key(st: &mut State, key: KeyEvent, quit: &mut bool) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        *quit = true;
        return;
    }
    // 帮助浮层下,任何键都先收掉它
    if st.help {
        st.help = false;
        if key.code == KeyCode::Char('q') {
            *quit = true;
        }
        return;
    }
    // 两键序列:第一个键只待命,第二个键才干活;别的键取消待命
    if let Some(pending) = st.pending.take() {
        match (pending, key.code) {
            (Pending::Copy, KeyCode::Char('y')) => {
                st.copy_selected();
                return;
            }
            (Pending::ColumnTop, KeyCode::Char('g')) => {
                st.cursor = st.grid.col_edge(st.cursor, false);
                st.status.clear();
                return;
            }
            (_, KeyCode::Esc) => {
                st.status.clear();
                return;
            }
            _ => st.status.clear(),
        }
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => *quit = true,
        KeyCode::Char('h') | KeyCode::Left => st.move_cursor(0, -1),
        KeyCode::Char('l') | KeyCode::Right => st.move_cursor(0, 1),
        KeyCode::Char('k') | KeyCode::Up => st.move_cursor(-1, 0),
        KeyCode::Char('j') | KeyCode::Down => st.move_cursor(1, 0),
        KeyCode::Char('0') | KeyCode::Home => st.cursor = st.grid.row_edge(st.cursor, false),
        KeyCode::Char('$') | KeyCode::End => st.cursor = st.grid.row_edge(st.cursor, true),
        KeyCode::Char('g') => {
            st.pending = Some(Pending::ColumnTop);
            st.status = "g-  press g again for the top of this column".into();
        }
        KeyCode::Char('G') => st.cursor = st.grid.col_edge(st.cursor, true),
        KeyCode::Char('y') => {
            st.pending = Some(Pending::Copy);
            st.status = "y-  press y again to copy".into();
        }
        KeyCode::Char('?') => st.help = true,
        KeyCode::Tab => st.cycle(1),
        KeyCode::BackTab => st.cycle(-1),
        KeyCode::Char(c @ '1'..='5') => st.set_kind(c as usize - '1' as usize),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(v: &[&str]) -> Result<Parsed, String> {
        parse(v.iter().map(|s| s.to_string()))
    }

    fn press(st: &mut State, code: KeyCode) -> bool {
        let mut quit = false;
        on_key(st, KeyEvent::new(code, KeyModifiers::NONE), &mut quit);
        quit
    }

    fn state() -> State {
        let mut st = State::new(0);
        st.rebuild(10, 20);
        st
    }

    #[test]
    fn defaults_to_the_hue_palette() {
        let Ok(Parsed::Run(a)) = args_of(&[]) else {
            panic!("expected a runnable config")
        };
        assert_eq!(ALL[a.kind], Kind::HueValue);
        let Ok(Parsed::Run(a)) = args_of(&["ansi256"]) else {
            panic!("expected a runnable config")
        };
        assert_eq!(ALL[a.kind], Kind::Ansi256);
        let Ok(Parsed::Run(a)) = args_of(&["4"]) else {
            panic!("expected a runnable config")
        };
        assert_eq!(ALL[a.kind], Kind::Ansi256);
    }

    #[test]
    fn help_version_and_bad_input() {
        assert!(matches!(args_of(&["--help"]), Ok(Parsed::Help)));
        assert!(matches!(args_of(&["-V"]), Ok(Parsed::Version)));
        assert!(args_of(&["--nope"]).is_err());
        assert!(args_of(&["chartreuse"]).is_err());
        assert!(args_of(&["gray", "gray"]).is_ok());
    }

    #[test]
    fn hjkl_moves_one_swatch_and_clamps_at_the_edges() {
        let mut st = state();
        let start = st.grid.index(0, 0);
        assert_eq!(st.cursor, start);
        press(&mut st, KeyCode::Char('h'));
        press(&mut st, KeyCode::Char('k'));
        assert_eq!(st.cursor, start, "the cursor left the grid");
        press(&mut st, KeyCode::Char('l'));
        assert_eq!(st.cursor, st.grid.index(0, 1));
        press(&mut st, KeyCode::Char('j'));
        assert_eq!(st.cursor, st.grid.index(1, 1));
        press(&mut st, KeyCode::Left);
        assert_eq!(st.cursor, st.grid.index(1, 0));
        press(&mut st, KeyCode::Up);
        assert_eq!(st.cursor, st.grid.index(0, 0));
    }

    #[test]
    fn row_edge_jumps_stay_in_the_row() {
        let mut st = state();
        press(&mut st, KeyCode::Char('j'));
        press(&mut st, KeyCode::Char('j'));
        press(&mut st, KeyCode::Char('l'));
        press(&mut st, KeyCode::Char('$'));
        assert_eq!(st.cursor, st.grid.index(2, st.grid.cols - 1));
        press(&mut st, KeyCode::Char('0'));
        assert_eq!(st.cursor, st.grid.index(2, 0));
    }

    #[test]
    fn gg_and_capital_g_walk_the_column_the_cursor_is_in() {
        let mut st = state(); // hue/value 10x20,满格
        st.cursor = st.grid.index(3, 7);
        press(&mut st, KeyCode::Char('g'));
        assert_eq!(st.pending, Some(Pending::ColumnTop));
        assert_eq!(st.cursor, st.grid.index(3, 7), "a lone g moved the cursor");
        assert!(st.status.starts_with("g-"), "status was {:?}", st.status);
        press(&mut st, KeyCode::Char('g'));
        assert_eq!(st.pending, None);
        assert_eq!(st.cursor, st.grid.index(0, 7));
        assert_eq!(st.status, "");
        press(&mut st, KeyCode::Char('G'));
        assert_eq!(st.cursor, st.grid.index(9, 7));
        // 列不跟着动:再按 G 就在原地
        press(&mut st, KeyCode::Char('G'));
        assert_eq!(st.cursor, st.grid.index(9, 7));
        press(&mut st, KeyCode::Char('l'));
        press(&mut st, KeyCode::Char('g'));
        press(&mut st, KeyCode::Char('g'));
        assert_eq!(st.cursor, st.grid.index(0, 8), "gg left the column");
    }

    #[test]
    fn gg_and_capital_g_skip_the_padding_of_a_centred_palette() {
        let mut st = State::new(2); // ansi16:8x2 居中
        st.rebuild(10, 20);
        st.cursor = st.grid.index(4, 6); // 8x2 色卡居中后的左上角
        assert!(st.grid.at(st.cursor).is_some());
        press(&mut st, KeyCode::Char('G'));
        assert_eq!(st.cursor, st.grid.index(5, 6));
        press(&mut st, KeyCode::Char('g'));
        press(&mut st, KeyCode::Char('g'));
        assert_eq!(st.cursor, st.grid.index(4, 6));
        // 只有一行的 gray:本列上下都是自己,按了也不动
        let mut gray = State::new(4);
        gray.rebuild(10, 20);
        gray.cursor = gray.grid.index(5, 10);
        assert!(gray.grid.at(gray.cursor).is_some());
        press(&mut gray, KeyCode::Char('G'));
        assert_eq!(gray.cursor, gray.grid.index(5, 10));
    }

    #[test]
    fn yy_copies_and_a_lone_y_just_waits() {
        let mut st = state();
        st.cursor = st.grid.index(0, 3);
        let hex = st.grid.at(st.cursor).unwrap().hex();
        press(&mut st, KeyCode::Char('y'));
        assert_eq!(st.pending, Some(Pending::Copy));
        assert!(st.status.starts_with("y-"), "status was {:?}", st.status);
        press(&mut st, KeyCode::Char('y'));
        assert_eq!(st.pending, None);
        assert!(
            st.status.starts_with(&format!("copied {hex}")),
            "status was {:?}",
            st.status
        );
    }

    #[test]
    fn another_key_cancels_the_pending_y() {
        let mut st = state();
        press(&mut st, KeyCode::Char('y'));
        let before = st.cursor;
        press(&mut st, KeyCode::Char('l'));
        assert_eq!(st.pending, None);
        assert_eq!(st.status, "");
        assert_eq!(
            st.cursor,
            st.grid
                .index(st.grid.point(before).0, st.grid.point(before).1 + 1)
        );
    }

    #[test]
    fn the_other_pending_key_cancels_too() {
        let mut st = state();
        st.cursor = st.grid.index(2, 5);
        press(&mut st, KeyCode::Char('g'));
        // y 顶掉待命的 g,只换待命,不动光标也不复制
        press(&mut st, KeyCode::Char('y'));
        assert_eq!(st.pending, Some(Pending::Copy));
        assert_eq!(st.cursor, st.grid.index(2, 5));
        assert!(st.status.starts_with("y-"), "status was {:?}", st.status);
    }

    #[test]
    fn escape_while_pending_clears_instead_of_quitting() {
        let mut st = state();
        press(&mut st, KeyCode::Char('y'));
        assert!(!press(&mut st, KeyCode::Esc));
        assert_eq!(st.pending, None);
        assert!(press(&mut st, KeyCode::Esc));
    }

    #[test]
    fn tab_and_digits_switch_palettes() {
        let mut st = state();
        press(&mut st, KeyCode::Tab);
        assert_eq!(ALL[st.kind], Kind::HueSat);
        press(&mut st, KeyCode::BackTab);
        assert_eq!(ALL[st.kind], Kind::HueValue);
        press(&mut st, KeyCode::BackTab);
        assert_eq!(ALL[st.kind], Kind::Gray, "wraps around the end");
        press(&mut st, KeyCode::Char('3'));
        assert_eq!(ALL[st.kind], Kind::Ansi16);
        assert_eq!(st.status, "palette ansi16");
    }

    #[test]
    fn switching_palette_keeps_the_cursor_on_a_swatch() {
        let mut st = state();
        // 铺满的色卡换到居中摆放的 ansi16,光标原来的位置是空位
        st.cursor = st.grid.index(9, 19);
        press(&mut st, KeyCode::Char('3'));
        st.rebuild(10, 20);
        assert!(st.grid.at(st.cursor).is_some(), "cursor sat on padding");
    }

    #[test]
    fn help_swallows_one_key_then_gets_out_of_the_way() {
        let mut st = state();
        press(&mut st, KeyCode::Char('?'));
        assert!(st.help);
        assert!(!press(&mut st, KeyCode::Char('j')));
        assert!(!st.help);
        assert_eq!(st.cursor, st.grid.index(0, 0), "the key was not swallowed");
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut st = state();
        assert!(press(&mut st, KeyCode::Char('q')));
        let mut quit = false;
        on_key(
            &mut st,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut quit,
        );
        assert!(quit);
    }

    #[test]
    fn copy_is_a_no_op_without_a_terminal() {
        // cargo test 下 stdout 不是终端,所以这条路径不会碰剪贴板
        assert!(copy::copy("#010203").is_ok());
    }

    #[test]
    fn rebuild_survives_a_degenerate_size() {
        let mut st = State::new(0);
        st.rebuild(0, 0);
        assert!(st.grid.at(st.cursor).is_some());
        st.rebuild(1, 1);
        assert_eq!(st.grid.cells.len(), 1);
    }
}
