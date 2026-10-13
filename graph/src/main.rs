// metro:终端里的杭州地铁线路图.
// 本文件只负责命令行、终端进出与按键;图数据在 docs/hangzhou-subway.dot(可 --file 换),
// 解析在 dot.rs,模型在 map.rs,绘制在 ui.rs.
mod dot;
mod layout;
mod map;
mod search;
mod ui;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, TerminalOptions, Viewport};

use map::{Map, Scene};
use search::Found;
use ui::{Mode, View};

/// 搜索态下重新算一遍命中:数字=高亮整条线,文字=站名/拼音
fn refresh(map: &Map, view: &mut View) {
    let text = view.query.clone().unwrap_or_default();
    let found = search::parse(map, &text);
    view.hit_at = 0;
    view.hits = match found {
        // 数字:聚焦这几条线(它们的所有站都算命中,光标不动)
        Found::Lines(lines) => {
            view.focus = lines.clone();
            (0..map.names.len())
                .filter(|i| map.station_lines[*i].iter().any(|l| lines.contains(l)))
                .collect()
        }
        // 文字:聚焦命中那些站涉及的线,光标跳到第一个
        Found::Stations(hits) => {
            view.focus = hits
                .iter()
                .flat_map(|i| map.station_lines[*i].iter().copied())
                .fold(Vec::new(), |mut v, l| {
                    if !v.contains(&l) {
                        v.push(l);
                    }
                    v
                });
            if let Some(&first) = hits.first() {
                view.cursor = first;
            }
            hits
        }
        Found::Empty => {
            view.focus.clear();
            Vec::new()
        }
    };
}


/// 内嵌的线路图:docs/hangzhou-subway.dot
const EMBEDDED: &str = include_str!("../docs/hangzhou-subway.dot");

const USAGE: &str = "\
metro - 终端里的杭州地铁线路图(ASCII,一条线路一种颜色)

usage: metro [options]

options:
  --file <path>  读取线路图 .dot(默认内嵌 docs/hangzhou-subway.dot)
  --dump         渲染一帧到 stdout(真彩 ANSI)后退出,可配合 --size
  --size <WxH>   --dump 的画布大小(默认 200x50)
  --labels       一上来就把所有站名标出来(等于按 b)
  --real         按真实经纬度画(默认是间距均匀的示意图,按 r 切换)
  --at <站名>    开局把光标停在那一站
  --find <词>    开局就搜一遍(站名/拼音/首字母,或线路号)
  -h, --help     显示帮助

keys:
  h j k l / 方向键  朝那个方向挑屏幕上最近的一站(眼睛看到哪个近就哪个,
                    换终端尺寸/切视图都可能不一样);聚焦某条线时这条线上的站优先
  H J K L          强制走图上相邻的那一站(这条线的下一站,按真实经纬度判方向);
                    线路拐弯画出锥形也认;那个方向没有相邻站就不动
  + -              放大 / 缩小
  0                复位成整幅图
  空格             把光标那一站挪到画面正中
                   (聚焦某条线时,方向键优先走那条线)
  b                开关站名标注
  /                搜站名或拼音(首字母也行);输入纯数字则聚焦那条线
  n                跳到下一个命中的站
  r                在示意图与真实地理之间切换
  q / esc          退出
";

struct Args {
    file: Option<String>,
    dump: bool,
    size: Option<(u16, u16)>,
    labels: bool,
    /// 用真实经纬度画(默认是间距均匀的示意图)
    real: bool,
    /// 开局把光标停在哪一站
    at: Option<String>,
    /// 开局就搜一遍(等于按 / 输入再回车)
    find: Option<String>,
    help: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        file: None,
        dump: false,
        size: None,
        labels: false,
        real: false,
        at: None,
        find: None,
        help: false,
    };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--file" => {
                i += 1;
                args.file = Some(argv.get(i).ok_or("--file 缺少路径")?.clone());
            }
            "--dump" => args.dump = true,
            "--labels" => args.labels = true,
            "--real" => args.real = true,
            "--at" => {
                i += 1;
                args.at = Some(argv.get(i).ok_or("--at 缺少站名")?.clone());
            }
            "--find" => {
                i += 1;
                args.find = Some(argv.get(i).ok_or("--find 缺少内容")?.clone());
            }
            "--size" => {
                i += 1;
                let raw = argv.get(i).ok_or("--size 缺少尺寸")?;
                let (w, h) = raw.split_once(['x', 'X']).ok_or("--size 形如 200x50")?;
                args.size = Some((
                    w.parse().map_err(|_| "宽度不是数字")?,
                    h.parse().map_err(|_| "高度不是数字")?,
                ));
            }
            "-h" | "--help" => args.help = true,
            other => return Err(format!("未知参数 {other}")),
        }
        i += 1;
    }
    Ok(args)
}

/// vim 方向键:返回 (列方向, 行方向),h 往西、l 往东、k 往北、j 往南
/// 方向键的映射:(方向, 是否"只走图上相邻的一站").
/// 小写 h j k l 和方向键只是朝那个方向按真实经纬度挑最近的一站;
/// 大写 H J K L 则强制走图上和当前站相邻的那一站(这条线的下一站),没有就不动.
fn dir_of(code: KeyCode) -> Option<((i32, i32), bool)> {
    match code {
        KeyCode::Char('h') | KeyCode::Left => Some(((-1, 0), false)),
        KeyCode::Char('l') | KeyCode::Right => Some(((1, 0), false)),
        KeyCode::Char('k') | KeyCode::Up => Some(((0, -1), false)),
        KeyCode::Char('j') | KeyCode::Down => Some(((0, 1), false)),
        KeyCode::Char('H') => Some(((-1, 0), true)),
        KeyCode::Char('L') => Some(((1, 0), true)),
        KeyCode::Char('K') => Some(((0, -1), true)),
        KeyCode::Char('J') => Some(((0, 1), true)),
        _ => None,
    }
}

fn dump(buf: &ratatui::buffer::Buffer) {
    let mut out = String::new();
    for y in 0..buf.area.height {
        // 全角字的第二格不用再输出(宽字本身已经占了两列)
        let mut skip_cells = 0usize;
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if skip_cells > 0 {
                skip_cells -= 1;
                continue;
            }
            skip_cells = unicode_width::UnicodeWidthStr::width(cell.symbol()).saturating_sub(1);
            let (r, g, b) = match cell.fg {
                Color::Rgb(r, g, b) => (r, g, b),
                Color::Reset => (212, 218, 232),
                _ => (212, 218, 232),
            };
            let (br, bg_, bb) = match cell.bg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (9, 10, 17),
            };
            let mut sgr = String::from("\x1b[");
            sgr.push_str(&format!("38;2;{r};{g};{b};48;2;{br};{bg_};{bb}"));
            if cell.modifier.contains(Modifier::BOLD) {
                sgr.push_str(";1");
            }
            if cell.modifier.contains(Modifier::DIM) {
                sgr.push_str(";2");
            }
            sgr.push_str("m");
            out.push_str(&sgr);
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    print!("{out}");
    let _ = io::stdout().flush();
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("metro: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let text = match &args.file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("metro: 读不到 {path}: {e}");
                return ExitCode::from(1);
            }
        },
        None => EMBEDDED.to_string(),
    };
    let map = match Map::from_dot(&text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("metro: 线路图有问题: {e}");
            return ExitCode::from(1);
        }
    };
    let (w, h) = args
        .size
        .or_else(|| crossterm::terminal::size().ok())
        .unwrap_or((200, 50));
    let real = Scene::real(&map);
    let schematic = Scene::schematic(&map);
    let mut view = View {
        labels: args.labels,
        mode: if args.real { Mode::Real } else { Mode::Schematic },
        ..View::default()
    };
    // 默认光标停在离画面中心最近的一站(按当前那套坐标算,别串了)
    let scene = scene_of(&view, &real, &schematic);
    let center = scene.center();
    let dist = |i: usize| {
        let p = scene.pos[i];
        (p.0 - center.0).powi(2) + (p.1 - center.1).powi(2)
    };
    view.cursor = match &args.at {
        Some(name) => match map.names.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                eprintln!("metro: 没有叫 {name} 的站");
                return ExitCode::from(1);
            }
        },
        None => (0..scene.pos.len())
            .min_by(|&a, &b| dist(a).partial_cmp(&dist(b)).unwrap())
            .unwrap_or(0),
    };
    if let Some(text) = &args.find {
        view.query = Some(text.clone());
        refresh(&map, &mut view);
        view.query = None;
    }
    if args.dump {
        let mut term = Terminal::new(ratatui::backend::TestBackend::new(w, h)).expect("测试终端");
        term.draw(|f| {
            let area = f.area();
            ui::render(f.buffer_mut(), area, &map, scene_of(&view, &real, &schematic), &view);
        })
        .expect("绘制");
        dump(term.backend().buffer());
        return ExitCode::SUCCESS;
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("metro: 需要真正的终端;只想要一帧可以用 --dump");
        return ExitCode::from(1);
    }
    match run(&mut view, &map, &real, &schematic) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("metro: {e}");
            ExitCode::from(1)
        }
    }
}

use std::io::IsTerminal;

/// 平滑移动:把视图中心朝目标挪一段(每帧走掉剩余距离的 22%,指数逼近),
/// 距目标不足半格就直接吸附过去.返回 true 表示还在动,主循环要接着按帧重画
fn animate(view: &mut View, scene: &Scene, cell: f64) -> bool {
    let Some(t) = view.target else {
        return false;
    };
    let cur = view.center.unwrap_or_else(|| scene.center());
    const K: f64 = 0.22;
    let next = (cur.0 + (t.0 - cur.0) * K, cur.1 + (t.1 - cur.1) * K);
    if (t.0 - next.0).abs() < cell * 0.5 && (t.1 - next.1).abs() < cell * 0.5 {
        view.center = Some(t);
        view.target = None;
        false
    } else {
        view.center = Some(next);
        true
    }
}

/// 让视图平滑地挪到某一点(画布坐标):已经在动的就从当前显示的位置接着走
fn pan_to(view: &mut View, scene: &Scene, p: (f64, f64)) {
    if view.center.is_none() {
        view.center = Some(scene.center());
    }
    view.target = Some(p);
}

/// 当前模式用的是哪套坐标
fn scene_of<'a>(view: &View, real: &'a Scene, schematic: &'a Scene) -> &'a Scene {
    match view.mode {
        Mode::Real => real,
        Mode::Schematic => schematic,
    }
}

fn run(view: &mut View, map: &Map, real: &Scene, schematic: &Scene) -> io::Result<()> {
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let mut term = Terminal::with_options(
        CrosstermBackend::new(stdout),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )?;
    let result = event_loop(&mut term, view, map, real, schematic);
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(term.backend_mut(), crossterm::terminal::LeaveAlternateScreen)?;
    result
}

fn event_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    view: &mut View,
    map: &Map,
    real: &Scene,
    schematic: &Scene,
) -> io::Result<()> {
    let mut search_from = view.cursor;
    loop {
        // 移动动画:还在动就按 ~60fps 重画,停了就慢慢等按键
        let moving = {
            let sc = scene_of(view, real, schematic);
            let lay = ui::layout(map, sc, term.size()?.into(), view);
            animate(view, sc, lay.proj.scale)
        };
        term.draw(|f| {
            let area = f.area();
            ui::render(f.buffer_mut(), area, map, scene_of(view, real, schematic), view);
        })?;
        let wait = if moving {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(200)
        };
        if !event::poll(wait)? {
            continue;
        }
        let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        else {
            continue;
        };
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
            return Ok(());
        }
        let _ = &search_from;
        // 搜索态:所有按键都归输入行
        if view.query.is_some() {
            match code {
                KeyCode::Esc => {
                    view.query = None;
                    view.hits.clear();
                    view.focus.clear();
                    view.cursor = search_from;
                }
                KeyCode::Enter => {
                    view.query = None; // 命中留着,可以按 n 一个个跳
                }
                KeyCode::Backspace => {
                    if let Some(q) = view.query.as_mut() {
                        q.pop();
                    }
                    refresh(map, view);
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(q) = view.query.as_mut() {
                        q.push(c);
                    }
                    refresh(map, view);
                }
                _ => {}
            }
            continue;
        }
        let mut moved = None;
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            _ if dir_of(code).is_some() => moved = dir_of(code),
            KeyCode::Char('+') | KeyCode::Char('=') => view.zoom = (view.zoom * 1.4).min(64.0),
            KeyCode::Char('-') | KeyCode::Char('_') => view.zoom = (view.zoom / 1.4).max(1.0),
            // 空格:把光标那一站挪到画面正中(看局部的时候好使)
            KeyCode::Char(' ') => {
                let sc = scene_of(view, real, schematic);
                let p = sc.pos[view.cursor.min(sc.pos.len() - 1)];
                pan_to(view, sc, p);
            }
            KeyCode::Char('0') => {
                view.zoom = 1.0;
                view.center = None;
                view.target = None;
            }
            KeyCode::Char('b') => view.labels = !view.labels,
            KeyCode::Char('/') => {
                search_from = view.cursor;
                view.query = Some(String::new());
                view.hits.clear();
                view.focus.clear();
            }
            // n:跳到下一个命中的站(搜完一串站就靠它挨个看)
            KeyCode::Char('n') if !view.hits.is_empty() => {
                view.hit_at = (view.hit_at + 1) % view.hits.len();
                view.cursor = view.hits[view.hit_at];
            }
            KeyCode::Char('r') => {
                view.mode = match view.mode {
                    Mode::Real => Mode::Schematic,
                    Mode::Schematic => Mode::Real,
                };
                view.zoom = 1.0;
                view.center = None;
                view.target = None;
            }
            _ => {}
        }
        if let Some((dir, force)) = moved {
            let area = term.size()?;
            let sc = scene_of(view, real, schematic);
            let lay = ui::layout(map, sc, area.into(), view);
            view.cursor = ui::step(map, sc, &lay, view.cursor, dir, &view.focus, force);
            // 光标跑出视野就把视图挪过去
            let c = lay.proj.cell(sc.pos[view.cursor]);
            let inside = c.0 >= lay.area.x as i32
                && c.0 < (lay.area.x + lay.area.width) as i32
                && c.1 >= lay.area.y as i32
                && c.1 < (lay.area.y + lay.area.height) as i32;
            if !inside {
                let old = view.center.unwrap_or_else(|| sc.center());
                let p = sc.pos[view.cursor];
                let want = (old.0 + (p.0 - old.0) * 0.85, old.1 + (p.1 - old.1) * 0.85);
                pan_to(view, sc, want);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 平滑移动:一帧挪一段(指数逼近),不是一步到位;方向不变,最后吸附到目标
    #[test]
    fn 平滑移动是逐步逼近的() {
        let m = Map::from_dot(include_str!("../docs/hangzhou-subway.dot")).unwrap();
        let sc = Scene::schematic(&m);
        let mut view = View::default();
        let goal = (1000.0, 500.0);
        pan_to(&mut view, &sc, goal);
        assert!(view.center.is_some(), "开始动之前得先有起点");
        let mut dists = Vec::new();
        let mut frames = 0;
        while animate(&mut view, &sc, 1.0) {
            let cur = view.center.unwrap();
            dists.push((goal.0 - cur.0).hypot(goal.1 - cur.1));
            frames += 1;
            assert!(frames < 200, "动画没个头");
        }
        println!("动了 {frames} 帧,距离序列前几项 {:?}", &dists[..5.min(dists.len())]);
        assert!(frames > 5, "该是好幾帧的动画,不是一步到位(实际 {frames} 帧)");
        assert!(dists.windows(2).all(|w| w[1] < w[0]), "离目标该越来越近");
        assert_eq!(view.center, Some(goal), "最后要吸附到目标");
        assert!(view.target.is_none(), "到位后目标要清掉");
        // 停在原地之后 animate 该说"不动了"
        assert!(!animate(&mut view, &sc, 1.0));
    }

    fn hjkl_方向对得上() {
        // (列方向, 行方向):h 西、l 东、k 北、j 南;方向键一样
        assert_eq!(dir_of(KeyCode::Char('h')), Some(((-1, 0), false)));
        assert_eq!(dir_of(KeyCode::Char('l')), Some(((1, 0), false)));
        assert_eq!(dir_of(KeyCode::Char('k')), Some(((0, -1), false)));
        assert_eq!(dir_of(KeyCode::Char('j')), Some(((0, 1), false)));
        assert_eq!(dir_of(KeyCode::Left), Some(((-1, 0), false)));
        assert_eq!(dir_of(KeyCode::Right), Some(((1, 0), false)));
        assert_eq!(dir_of(KeyCode::Up), Some(((0, -1), false)));
        assert_eq!(dir_of(KeyCode::Down), Some(((0, 1), false)));
        // 大写:强制走图上相邻的那一站
        assert_eq!(dir_of(KeyCode::Char('H')), Some(((-1, 0), true)));
        assert_eq!(dir_of(KeyCode::Char('L')), Some(((1, 0), true)));
        assert_eq!(dir_of(KeyCode::Char('K')), Some(((0, -1), true)));
        assert_eq!(dir_of(KeyCode::Char('J')), Some(((0, 1), true)));
        assert_eq!(dir_of(KeyCode::Char('x')), None);
    }
}
