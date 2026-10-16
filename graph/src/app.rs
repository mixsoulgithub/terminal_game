//! 交互状态与事件.这一层不碰画法(那是 render),只维护"现在在看哪儿、光标在哪、搜了什么".
//!
//! 事件处理与动画都在这里,main.rs 只负责命令行与终端进出.

use std::time::Instant;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::lang::Lang;
use crate::layout::{self, Layout};
use crate::model::Graph;
use crate::render;
use crate::scene::Scene;
use crate::search::{self, Found};

/// 光标呼吸一个来回的时长(秒):约 75 下/分,和心跳差不多
const BEAT: f64 = 0.8;

/// 光标跑出视野后,视图往它那边靠过去的比例(不是一步到位,看起来才是"跟过去")
const FOLLOW: f64 = 0.85;

/// 平移时长的下限与上限(秒):远的只多给一点,不能越远越拖
const PAN_MIN: f64 = 0.2;
const PAN_MAX: f64 = 0.42;

/// 缩放的范围:键盘与滚轮共用
const ZOOM_MIN: f64 = 0.2;
const ZOOM_MAX: f64 = 64.0;

/// 一段平移动画:按**时长**缓动(和地图软件一个路子),不是"每帧走固定格数"
#[derive(Clone, Copy)]
pub struct Pan {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub start: Instant,
    /// 走多少秒(远的多给一点,但有封顶)
    pub dur: f64,
}

/// "沿线走一站"有多个候选时弹的选择浮层
pub struct Choice {
    /// 候选项:(节点, 分组)
    pub items: Vec<(usize, usize)>,
    /// 当前选中的第几个
    pub at: usize,
}

/// 视图状态:render 只读它,app 改它
pub struct View {
    /// 现在用哪套坐标
    pub layout: Layout,
    /// 正在输入的搜索词(None = 不在搜索)
    pub query: Option<String>,
    /// 搜索命中/高亮的节点(会一直留着,方便按 n 一个个跳)
    pub hits: Vec<usize>,
    /// 聚焦的分组(只留这几组的颜色,别的压暗)
    pub focus: Vec<usize>,
    /// 当前跳到第几个
    pub hit_at: usize,
    /// 放大倍数(1 = 铺满整个区域)
    pub zoom: f64,
    /// 视图中心(画布坐标),None = 用整幅图的中心
    pub center: Option<(f64, f64)>,
    /// 平移目标(见 app::tick)
    pub target: Option<(f64, f64)>,
    /// 正在进行的平移
    pub pan: Option<Pan>,
    pub cursor: usize,
    /// 是否画出所有节点名
    pub labels: bool,
    /// 光标节点"呼吸"的相位:0 = 本站原色,1 = 纯红(主循环按时间算出来)
    pub pulse: f64,
    /// 跳过的节点(搜到的、鼠标点的、半屏跳的),Ctrl-o / Ctrl-i 前后翻
    pub history: Vec<usize>,
    pub hist_at: usize,
    /// 是否显示键位速查
    pub help: bool,
    /// 绕行容忍强度:同组那一档允许比全局最优远几倍(见 main.rs 里的 :cycle 说明).
    /// None = 按现状(聚焦时 2.0,没聚焦时 1.0),用 :cycle 显式设过就按设的来
    pub cycle: Option<f64>,
    /// 侧偏权重:打分里"横向偏斜"乘几倍(默认 2.0,可用 :side 调).调小就更认远近
    pub side: f64,
    /// ":" 命令行的输入缓冲
    pub cmd: Option<String>,
    /// 底部提示(命令回显)
    pub note: Option<String>,
    /// 界面语言:只影响应用自己的提示文字(状态栏、速查、":" 回显);
    /// 命令行那套(-h 帮助与报错)固定英文,不跟着它走
    pub lang: Lang,
    /// 沿线走一站时的候选浮层
    pub choice: Option<Choice>,
    /// 呼吸相位从哪个时刻开始算(tick 用)
    pub started: Instant,
}

impl Default for View {
    fn default() -> Self {
        View {
            layout: Layout::Geo,
            query: None,
            hits: Vec::new(),
            focus: Vec::new(),
            hit_at: 0,
            zoom: 1.0,
            center: None,
            target: None,
            pan: None,
            cursor: 0,
            labels: false,
            pulse: 0.0,
            history: Vec::new(),
            hist_at: 0,
            help: false,
            cycle: None,
            side: 2.0,
            cmd: None,
            note: None,
            lang: Lang::default(),
            choice: None,
            started: Instant::now(),
        }
    }
}

/// 打开一张图时的初始设置
#[derive(Clone, Debug)]
pub struct Options {
    pub layout: Option<Layout>,
    pub labels: bool,
    /// 开局把光标停在这个节点上(名字,可模糊)
    pub at: Option<String>,
    /// 开局就搜一遍(等于按 / 输入再回车)
    pub find: Option<String>,
    pub lang: Lang,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            layout: None,
            labels: false,
            at: None,
            find: None,
            lang: Lang::default(),
        }
    }
}

/// 主循环该做什么
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Continue,
    Quit,
}

pub struct App {
    pub graph: Graph,
    pub scene: Scene,
    pub view: View,
    /// 数据来源(状态栏显示),从 stdin 来就是 None
    pub source: Option<String>,
    /// 上一帧画面区域(鼠标坐标要按它换算;draw 时刷新)
    pub area: Rect,
    /// 数字前缀(5l = 连挪 5 个节点)
    pending: Option<u32>,
    /// 左键拖动:起点格、按下时的视图中心、当时的投影
    drag: Option<((u16, u16), (f64, f64), render::Proj)>,
    /// 进搜索态之前光标在哪(Esc 取消时退回去)
    search_from: usize,
}

impl App {
    pub fn new(graph: Graph, opts: Options) -> Result<App, String> {
        let layout = opts.layout.unwrap_or_else(|| Layout::pick(&graph));
        let scene = layout::scene(&graph, layout)?;
        let view = View {
            layout,
            labels: opts.labels,
            lang: opts.lang,
            ..View::default()
        };
        let mut app = App {
            graph,
            scene,
            view,
            source: None,
            area: Rect::default(),
            pending: None,
            drag: None,
            search_from: 0,
        };
        // 开局光标停在离画面中心最近的节点上:整幅图铺满时这一眼就能看到
        let c = app.scene.center();
        app.view.cursor = (0..app.scene.pos.len())
            .min_by(|&a, &b| {
                dist2(app.scene.pos[a], c)
                    .partial_cmp(&dist2(app.scene.pos[b], c))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        // --at:按名字/别名找,找不着就把话说在提示行上,不拦着开局
        if let Some(name) = &opts.at {
            match search::nodes(&app.graph, name).first() {
                Some(&i) => app.view.cursor = i,
                None => {
                    let note = app.view.lang.pick_owned(
                        format!("没有叫 {name} 的节点"),
                        format!("no node named {name}"),
                    );
                    app.view.note = Some(note);
                }
            }
        }
        // --find:等于按 / 输入再回车,命中留着可以按 n 跳
        if let Some(text) = &opts.find {
            if !text.trim().is_empty() {
                app.view.query = Some(text.clone());
                refresh(&app.graph, &mut app.view);
                app.view.query = None;
                if app.view.hits.is_empty() {
                    let note = app.view.lang.pick_owned(
                        format!("没搜到 {text}"),
                        format!("nothing matched {text}"),
                    );
                    app.view.note = Some(note);
                }
            }
        }
        app.search_from = app.view.cursor;
        note_here(&mut app.view);
        Ok(app)
    }

    /// 换一套坐标(布局),光标与搜索都按节点索引保留
    pub fn rebind(&mut self, layout: Layout) -> Result<(), String> {
        let scene = layout::scene(&self.graph, layout)?;
        self.scene = scene;
        self.view.layout = layout;
        // 平移目标说的是旧坐标上的点,换了坐标就没意义;光标序号与缩放留在原地
        self.view.pan = None;
        self.view.target = None;
        Ok(())
    }

    /// 有没有动画在跑(没有就可以少画几帧)
    pub fn busy(&self) -> bool {
        self.view.target.is_some() || self.view.pan.is_some()
    }

    /// 按时间推进动画(平移缓动、光标呼吸)
    pub fn tick(&mut self) {
        self.view.pulse = pulse_at(self.view.started.elapsed().as_secs_f64() / BEAT);
        let cell = render::panels(&self.graph, &self.scene, self.area, &self.view)
            .proj
            .scale;
        animate(&mut self.view, &self.scene, cell);
    }

    /// 画一帧(顺手把画面区域记下来,鼠标换算要用)
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) {
        self.area = area;
        render::render(buf, area, &self.graph, &self.scene, &self.view);
        if self.view.help {
            render::render_help(buf, area, render::help_keys(self.view.lang), self.view.lang);
        }
        if self.view.choice.is_some() {
            render::render_choice(buf, area, &self.graph, &self.view);
        }
    }

    /// 一个事件
    pub fn handle(&mut self, ev: Event) -> Action {
        if let Event::Mouse(me) = &ev {
            self.handle_mouse(*me);
            return Action::Continue;
        }
        let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = ev
        else {
            return Action::Continue;
        };
        if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
            return Action::Quit;
        }
        // 速查浮层:任意键关掉
        if self.view.help {
            self.view.help = false;
            return Action::Continue;
        }
        // 选"沿线走一站"走哪条分组:数字直选、j k 挪、回车走、esc 取消
        if self.view.choice.is_some() {
            self.handle_choice(code);
            return Action::Continue;
        }
        // ":" 命令行:所有按键都归输入行
        if self.view.cmd.is_some() {
            self.handle_cmd(code, modifiers);
            return Action::Continue;
        }
        // 搜索态:所有按键都归输入行
        if self.view.query.is_some() {
            self.handle_search(code, modifiers);
            return Action::Continue;
        }
        let mut moved = None;
        let mut topo = None;
        let mut steps: u32 = 1;
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
            KeyCode::Char('?') => self.view.help = true,
            // z / 空格:把光标那个节点挪到画面正中(看局部的时候好使)
            KeyCode::Char('z') | KeyCode::Char(' ') => self.center_cursor(),
            // f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
            KeyCode::Char('f') => self.fit_focus(),
            // 半屏跳:远距离旅行用
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                let next = self.leap((0, 1));
                self.jump_to(next);
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                let next = self.leap((0, -1));
                self.jump_to(next);
            }
            // 跳转历史:Tab 就是 Ctrl-i
            KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
                if jump_history(&mut self.view, true) {
                    self.follow_cursor();
                }
            }
            KeyCode::Tab | KeyCode::Char('i') if modifiers.contains(KeyModifiers::CONTROL) => {
                if jump_history(&mut self.view, false) {
                    self.follow_cursor();
                }
            }
            _ if dir_of(code).is_some() => {
                moved = dir_of(code);
                steps = self.pending.take().unwrap_or(1);
            }
            _ if topo_of(code).is_some() => topo = topo_of(code),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.view.zoom = (self.view.zoom * 1.4).min(ZOOM_MAX)
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.view.zoom = (self.view.zoom / 1.4).max(1.0)
            }
            KeyCode::Char('0') => {
                self.view.zoom = 1.0;
                self.view.center = None;
                self.view.target = None;
            }
            KeyCode::Char('b') => self.view.labels = !self.view.labels,
            // V:聚焦光标所在的分组(枢纽会同时聚焦好几条),再按一次取消
            KeyCode::Char('V') => focus_groups_of(&self.graph, &mut self.view),
            KeyCode::Char('/') => {
                self.search_from = self.view.cursor;
                self.view.query = Some(String::new());
                self.view.hits.clear();
                self.view.focus.clear();
            }
            // n / N:下一个 / 上一个命中的节点
            KeyCode::Char('n') if !self.view.hits.is_empty() => {
                self.view.hit_at = (self.view.hit_at + 1) % self.view.hits.len();
                let next = self.view.hits[self.view.hit_at];
                self.jump_to(next);
            }
            KeyCode::Char('N') if !self.view.hits.is_empty() => {
                self.view.hit_at =
                    (self.view.hit_at + self.view.hits.len() - 1) % self.view.hits.len();
                let next = self.view.hits[self.view.hit_at];
                self.jump_to(next);
            }
            // r:四套布局轮着换(光标按节点序号保留)
            KeyCode::Char('r') => self.cycle_layout(),
            KeyCode::Char(':') => {
                self.view.cmd = Some(String::new());
                self.view.note = None;
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                // 数字当计数前缀(5l);不是数字就把计数清掉
                if !count_digit(&mut self.pending, c) {
                    self.pending = None;
                }
            }
            _ => self.pending = None,
        }
        if let Some(dir) = moved {
            self.move_cursor(dir, steps);
        }
        if let Some(next) = topo {
            self.step_along(next);
        }
        Action::Continue
    }

    /// ":" 命令行:layout <名>、side <倍数>、cycle <倍数|auto>、lang <zh|en>
    fn cmd_apply(&mut self, line: &str) {
        let mut it = line.split_whitespace();
        let (cmd, arg) = (it.next().unwrap_or(""), it.next());
        match (cmd, arg) {
            ("layout", Some(a)) => match Layout::parse(a) {
                Some(l) => match self.rebind(l) {
                    Ok(()) => self.note_layout(l),
                    Err(e) => self.view.note = Some(e),
                },
                None => {
                    let note = self.view.lang.pick_owned(
                        format!("layout 只认 geo / relax / force / layered,不认 {a}"),
                        format!("layout takes geo / relax / force / layered, not {a}"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("layout", None) => {
                let n = self.view.layout.name();
                let note = self.view
                    .lang
                    .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
                self.view.note = Some(note);
            }
            ("cycle", Some("auto")) => {
                self.view.cycle = None;
                let note = self.view.lang.pick_owned(
                    "cycle = auto(没聚焦 1.0,聚焦 2.0)".to_string(),
                    "cycle = auto (1.0 unfocused, 2.0 focused)".to_string(),
                );
                self.view.note = Some(note);
            }
            ("cycle", Some(a)) => match a.parse::<f64>() {
                Ok(v) => {
                    let v = v.clamp(0.0, 50.0);
                    self.view.cycle = Some(v);
                    let note = self.view.lang.pick_owned(
                        format!("cycle = {v:.2}(同分组那一档最多允许远这么多倍)"),
                        format!("cycle = {v:.2} (same-group tier may be this much farther)"),
                    );
                    self.view.note = Some(note);
                }
                Err(_) => {
                    let note = self.view.lang.pick_owned(
                        format!("cycle:{a} 不是数字(或 auto)"),
                        format!("cycle: {a} is not a number (or auto)"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("cycle", None) => {
                let text = match self.view.cycle {
                    Some(v) => format!("cycle = {v:.2}"),
                    None => self
                        .view
                        .lang
                        .pick(
                            "cycle = auto(没聚焦 1.0,聚焦 2.0)",
                            "cycle = auto (1.0 unfocused, 2.0 focused)",
                        )
                        .to_string(),
                };
                self.view.note = Some(text);
            }
            ("side", Some(a)) => match a.parse::<f64>() {
                Ok(v) => {
                    let v = v.clamp(0.0, 20.0);
                    self.view.side = v;
                    let note = self.view.lang.pick_owned(
                        format!("side = {v:.2}(横向偏斜算几倍距离,调小更认远近)"),
                        format!("side = {v:.2} (weight of sideways offset; lower = plain nearest)"),
                    );
                    self.view.note = Some(note);
                }
                Err(_) => {
                    let note = self.view.lang.pick_owned(
                        format!("side:{a} 不是数字"),
                        format!("side: {a} is not a number"),
                    );
                    self.view.note = Some(note);
                }
            },
            ("side", None) => {
                let v = self.view.side;
                self.view.note = Some(format!("side = {v:.2}"));
            }
            ("lang", arg) => {
                let want = match arg {
                    Some(a) => Lang::parse(a),
                    None => Some(self.view.lang.other()),
                };
                match want {
                    Some(l) => {
                        self.view.lang = l;
                        let code = l.code();
                        let note = l.pick_owned(
                            format!("语言 = {code}(zh 中文,en English)"),
                            format!("language = {code} (zh / en)"),
                        );
                        self.view.note = Some(note);
                    }
                    None => {
                        let note = self
                            .view
                            .lang
                            .pick("lang 只认 zh 或 en", "lang takes zh or en")
                            .to_string();
                        self.view.note = Some(note);
                    }
                }
            }
            ("", None) => self.view.note = None,
            _ => {
                let note = self.view.lang.pick_owned(
                    format!(
                        "未知命令:{line}(有 layout <名>、side <倍数>、cycle <倍数>、lang <zh|en>)"
                    ),
                    format!(
                        "unknown command: {line} (try layout <name>, side <x>, cycle <x>, lang <zh|en>)"
                    ),
                );
                self.view.note = Some(note);
            }
        }
    }

    /// 换布局后把名字说在提示行上
    fn note_layout(&mut self, l: Layout) {
        let n = l.name();
        let note = self
            .view
            .lang
            .pick_owned(format!("布局 = {n}"), format!("layout = {n}"));
        self.view.note = Some(note);
    }

    /// r:轮到下一套布局
    fn cycle_layout(&mut self) {
        let l = self.view.layout.next();
        match self.rebind(l) {
            Ok(()) => self.note_layout(l),
            Err(e) => self.view.note = Some(e),
        }
    }

    /// 选候选浮层里的项
    fn handle_choice(&mut self, code: KeyCode) {
        let Some(c) = self.view.choice.as_ref() else {
            return;
        };
        let (n, at) = (c.items.len(), c.at);
        if n == 0 {
            self.view.choice = None;
            return;
        }
        let mut go: Option<usize> = None;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.view.choice = None;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(c) = self.view.choice.as_mut() {
                    c.at = (at + 1) % n;
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(c) = self.view.choice.as_mut() {
                    c.at = (at + n - 1) % n;
                }
                return;
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Char(' ') => go = Some(at),
            // 数字 1..n 直选,别的键不理
            KeyCode::Char(c) => {
                if let Some(d) = c.to_digit(10) {
                    if d >= 1 && (d as usize) <= n {
                        go = Some(d as usize - 1);
                    }
                }
            }
            _ => {}
        }
        if let Some(node) = go.and_then(|i| self.view.choice.as_ref().map(|c| c.items[i].0)) {
            self.view.choice = None;
            self.jump_to(node);
        }
    }

    /// ":" 输入行
    fn handle_cmd(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Esc => self.view.cmd = None,
            KeyCode::Enter => {
                let line = self.view.cmd.take().unwrap_or_default();
                self.cmd_apply(&line);
            }
            KeyCode::Backspace => {
                if let Some(b) = self.view.cmd.as_mut() {
                    b.pop();
                }
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(b) = self.view.cmd.as_mut() {
                    b.push(c);
                }
            }
            _ => {}
        }
    }

    /// 搜索输入行:每敲一下都重算命中
    fn handle_search(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            // esc:连人带命中一起退回去
            KeyCode::Esc => {
                self.view.query = None;
                self.view.hits.clear();
                self.view.focus.clear();
                self.view.cursor = self.search_from;
            }
            // 回车:命中留着,之后可以按 n 一个个跳
            KeyCode::Enter => self.view.query = None,
            KeyCode::Backspace => {
                if let Some(q) = self.view.query.as_mut() {
                    q.pop();
                }
                refresh(&self.graph, &mut self.view);
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(q) = self.view.query.as_mut() {
                    q.push(c);
                }
                refresh(&self.graph, &mut self.view);
            }
            _ => {}
        }
    }

    /// 鼠标:滚轮缩放(按住 Ctrl 改成平移)、左键点中节点跳过去、按住拖动平移
    fn handle_mouse(&mut self, me: MouseEvent) {
        match me.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = matches!(me.kind, MouseEventKind::ScrollUp);
                let ctrl = me.modifiers.contains(KeyModifiers::CONTROL);
                let shift = me.modifiers.contains(KeyModifiers::SHIFT);
                if !ctrl && !shift {
                    let k = if up { 1.15 } else { 1.0 / 1.15 };
                    self.view.zoom = (self.view.zoom * k).clamp(ZOOM_MIN, 8.0);
                    self.view.note = Some(format!("zoom x{:.2}", self.view.zoom));
                } else {
                    let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
                    let from = self.view.center.unwrap_or_else(|| self.scene.center());
                    let step = if up { -3 } else { 3 };
                    let (dc, dr) = if shift { (step, 0) } else { (0, step) };
                    self.view.center = Some(render::pan_center(&panels.proj, from, dc, dr));
                    self.view.target = None;
                    let note = if shift {
                        self.view
                            .lang
                            .pick_owned(format!("平移 {step} 列"), format!("pan {step} columns"))
                    } else {
                        self.view
                            .lang
                            .pick_owned(format!("平移 {step} 行"), format!("pan {step} rows"))
                    };
                    self.view.note = Some(note);
                }
            }
            // 按下:记住起点与当时的视图中心,准备拖动
            MouseEventKind::Down(MouseButton::Left) => {
                let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
                let center = self.view.center.unwrap_or_else(|| self.scene.center());
                self.drag = Some(((me.column, me.row), center, panels.proj));
            }
            // 拖动:图跟着手走(往右拖,中心往左挪)
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(((c0, r0), center0, proj)) = self.drag {
                    let dc = me.column as i32 - c0 as i32;
                    let dr = me.row as i32 - r0 as i32;
                    if dc != 0 || dr != 0 {
                        self.view.center = Some(render::pan_center(&proj, center0, -dc, -dr));
                        self.view.target = None;
                    }
                }
            }
            // 松开:几乎没动才算"点了一下",那就跳过去;拖过了就不算点击
            MouseEventKind::Up(MouseButton::Left) => {
                let moved = self
                    .drag
                    .as_ref()
                    .map(|((c0, r0), _, _)| {
                        (me.column as i32 - *c0 as i32).abs()
                            + (me.row as i32 - *r0 as i32).abs()
                            > 1
                    })
                    .unwrap_or(false);
                self.drag = None;
                if !moved {
                    let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
                    if let Some(i) =
                        render::node_at(&self.scene, &panels, self.area, me.column, me.row)
                    {
                        self.jump_to(i);
                    }
                }
            }
            _ => {}
        }
    }

    /// 跳到某个节点:挪光标、记进历史,再保证它还在画面里
    fn jump_to(&mut self, i: usize) {
        if i == self.view.cursor || i >= self.graph.nodes.len() {
            return;
        }
        self.view.cursor = i;
        // 历史记的是"到过的节点":先挪光标再记,Ctrl-o 才会退到上一个到过的节点
        note_here(&mut self.view);
        self.follow_cursor();
    }

    /// z / 空格:把光标那个节点挪到画面正中
    fn center_cursor(&mut self) {
        let cursor = self.view.cursor.min(self.scene.pos.len().saturating_sub(1));
        let p = self.scene.pos[cursor];
        pan_to(&mut self.view, &self.scene, p);
    }

    /// f:把聚焦的分组(没聚焦就是光标所在的分组)铺满画面
    fn fit_focus(&mut self) {
        let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
        let groups = if self.view.focus.is_empty() {
            self.graph.node_groups[self.view.cursor.min(self.graph.nodes.len() - 1)].clone()
        } else {
            self.view.focus.clone()
        };
        let pts: Vec<(f64, f64)> = (0..self.scene.pos.len())
            .filter(|&i| self.graph.node_groups[i].iter().any(|l| groups.contains(l)))
            .map(|i| self.scene.pos[i])
            .collect();
        if let Some((z, c)) = fit_to(&self.scene, panels.map, pts.into_iter()) {
            self.view.zoom = z;
            self.view.center = Some(c);
            self.view.target = None;
        }
    }

    /// 光标跑出视野就把视图平滑挪过去(不动到光标头上,留下"看得见"的余量)
    fn follow_cursor(&mut self) {
        // 还没画过一帧就没有视口,无从判断光标在不在画面里
        if self.area.width == 0 || self.area.height == 0 {
            return;
        }
        let cursor = self.view.cursor.min(self.scene.pos.len().saturating_sub(1));
        let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
        let c = panels.proj.cell(self.scene.pos[cursor]);
        let inside = c.0 >= panels.map.x as i32
            && c.0 < (panels.map.x + panels.map.width) as i32
            && c.1 >= panels.map.y as i32
            && c.1 < (panels.map.y + panels.map.height) as i32;
        if inside {
            return;
        }
        let old = self.view.center.unwrap_or_else(|| self.scene.center());
        let p = self.scene.pos[cursor];
        let want = (
            old.0 + (p.0 - old.0) * FOLLOW,
            old.1 + (p.1 - old.1) * FOLLOW,
        );
        pan_to(&mut self.view, &self.scene, want);
    }

    /// 半屏跳:从光标往 dir 方向挪半屏,落到离那儿最近的节点上(夹在画面里)
    fn leap(&self, dir: (i32, i32)) -> usize {
        let cursor = self.view.cursor.min(self.scene.pos.len().saturating_sub(1));
        let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
        let c = panels.proj.cell(self.scene.pos[cursor]);
        let dx = dir.0 * (panels.map.width as i32 / 2).max(1);
        let dy = dir.1 * (panels.map.height as i32 / 2).max(1);
        let (ax, ay) = (panels.map.x as i32, panels.map.y as i32);
        let x_max = ax + panels.map.width as i32 - 1;
        let y_max = ay + panels.map.height as i32 - 1;
        let x = (c.0 + dx).clamp(ax, x_max.max(ax));
        let y = (c.1 + dy).clamp(ay, y_max.max(ay));
        render::node_at(&self.scene, &panels, self.area, x as u16, y as u16).unwrap_or(cursor)
    }

    /// 方向键移动:走一步挑屏幕上最近的那个节点,连着按计数前缀就走几步
    fn move_cursor(&mut self, dir: (i32, i32), steps: u32) {
        let panels = render::panels(&self.graph, &self.scene, self.area, &self.view);
        let mut why = String::new();
        for _ in 0..steps {
            let cyc = self.view.cycle.unwrap_or(if self.view.focus.is_empty() {
                1.0
            } else {
                2.0
            });
            let (next, cands) = step_cfg(
                &self.graph,
                &self.scene,
                &panels.proj,
                self.view.cursor,
                dir,
                &self.view.focus,
                cyc,
                self.view.side,
            );
            if next == self.view.cursor {
                break;
            }
            if why.is_empty() {
                why = explain_dir(
                    &self.graph,
                    dir,
                    next,
                    &cands,
                    cyc,
                    self.view.side,
                    self.view.lang,
                );
            }
            self.view.cursor = next;
        }
        if !why.is_empty() {
            self.view.note = Some(why);
        }
        self.follow_cursor();
    }

    /// H K / J L:沿分组走一站(枢纽上好几条分组都能走就弹候选)
    fn step_along(&mut self, next: bool) {
        let cands = line_step(&self.graph, self.view.cursor, next, &self.view.focus);
        match cands.len() {
            0 => {}
            1 => {
                let key = if next { "J/L" } else { "H/K" };
                let node = cands[0].0;
                self.jump_to(node);
                let label = self.graph.nodes[node].label.clone();
                let note = self.view.lang.pick_owned(
                    format!(
                        "{key} 沿分组{}:{label}",
                        if next { "下一站" } else { "上一站" }
                    ),
                    format!(
                        "{key} along the group to the {} node: {label}",
                        if next { "next" } else { "previous" }
                    ),
                );
                self.view.note = Some(note);
            }
            // 好几条分组都能走:弹个列表让用户挑
            _ => self.view.choice = Some(Choice { items: cands, at: 0 }),
        }
    }
}

/// 两点距离的平方(只用来比远近,不用开方)
fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// 闪的强度曲线:一个正弦,phase 是 0..1 的循环位置.
/// 0 -> 1 -> 0,两头平滑停住(正弦的斜率在端点本来就是 0),所以闪起来不生硬
fn pulse_at(phase: f64) -> f64 {
    0.5 - 0.5 * (phase * std::f64::consts::TAU).cos()
}

/// vim 方向键:返回 (列方向, 行方向),h 往西、l 往东、k 往北、j 往南
/// 小写/方向键:朝那个方向在屏幕上挑最近的一个节点
fn dir_of(code: KeyCode) -> Option<(i32, i32)> {
    match code {
        KeyCode::Char('h') | KeyCode::Left => Some((-1, 0)),
        KeyCode::Char('l') | KeyCode::Right => Some((1, 0)),
        KeyCode::Char('k') | KeyCode::Up => Some((0, -1)),
        KeyCode::Char('j') | KeyCode::Down => Some((0, 1)),
        _ => None,
    }
}

/// 大写:沿分组走一站,只认分组自己的节点顺序(和真实方位、屏幕方向都无关).
/// H K 是上一个,J L 是下一个
fn topo_of(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Char('H') | KeyCode::Char('K') => Some(false),
        KeyCode::Char('J') | KeyCode::Char('L') => Some(true),
        _ => None,
    }
}

/// 数字前缀:5l = 连挪 5 个节点.返回 true 表示这个键被当成计数吃了
fn count_digit(pending: &mut Option<u32>, c: char) -> bool {
    let Some(d) = c.to_digit(10) else {
        return false;
    };
    if d == 0 && pending.is_none() {
        return false; // 没前缀时 0 还是"复位"
    }
    *pending = Some((pending.unwrap_or(0) * 10 + d).min(999));
    true
}

/// 把"现在这里"记进跳转历史(Ctrl-o / Ctrl-i 前后翻)
fn note_here(view: &mut View) {
    if view.history.last() == Some(&view.cursor) {
        view.hist_at = view.history.len() - 1;
        return;
    }
    view.history.truncate(view.hist_at + 1); // 从历史中间跳走,后面那段丢掉
    view.history.push(view.cursor);
    if view.history.len() > 64 {
        view.history.remove(0);
    }
    view.hist_at = view.history.len() - 1;
}

/// 回到历史里的上一个 / 下一个跳过的节点.返回 false 表示到头了
fn jump_history(view: &mut View, back: bool) -> bool {
    if view.history.is_empty() {
        return false;
    }
    if back {
        if view.hist_at == 0 {
            return false;
        }
        view.hist_at -= 1;
    } else {
        if view.hist_at + 1 >= view.history.len() {
            return false;
        }
        view.hist_at += 1;
    }
    view.cursor = view.history[view.hist_at];
    true
}

/// 搜索态下重新算一遍命中:数字=聚焦这几条分组,文字=节点名/别名
fn refresh(g: &Graph, view: &mut View) {
    let text = view.query.clone().unwrap_or_default();
    view.hit_at = 0;
    match search::parse(g, &text) {
        // 数字:聚焦这几条分组(它们经过的节点都算命中,光标不动)
        Found::Groups(groups) => {
            view.hits = (0..g.nodes.len())
                .filter(|i| g.node_groups[*i].iter().any(|l| groups.contains(l)))
                .collect();
            view.focus = groups;
        }
        // 文字:聚焦命中那些节点经过的分组,光标跳到第一个
        Found::Nodes(hits) => {
            view.focus = hits
                .iter()
                .flat_map(|i| g.node_groups[*i].iter().copied())
                .fold(Vec::new(), |mut v, l| {
                    if !v.contains(&l) {
                        v.push(l);
                    }
                    v
                });
            if let Some(&first) = hits.first() {
                view.cursor = first;
            }
            view.hits = hits;
        }
        Found::Empty => {
            view.focus.clear();
            view.hits.clear();
        }
    }
}

/// 把光标所在的分组聚焦起来(枢纽会有好几条),再按一次取消
fn focus_groups_of(g: &Graph, view: &mut View) {
    if view.cursor >= g.nodes.len() {
        return;
    }
    let groups = g.node_groups[view.cursor].clone();
    // 已经在聚焦里了(比如刚才就是按 V 聚焦的),那这一下就是取消
    if !groups.is_empty() && groups.iter().all(|l| view.focus.contains(l)) {
        view.focus.clear();
        view.hits.clear();
        view.hit_at = 0;
        return;
    }
    view.focus = groups;
    let focus = view.focus.clone();
    view.hits = (0..g.nodes.len())
        .filter(|&i| g.node_groups[i].iter().any(|l| focus.contains(l)))
        .collect();
    view.hit_at = 0;
}

/// 沿线走一站:返回候选 (节点, 分组).
///
/// `next` 为真取下一站、为假取上一站;顺序是**分组自己的节点顺序**(路径里的先后),
/// 和真实地理位置、屏幕方向都无关.聚焦了某几条分组时只算这几条;
/// 同一个节点在多条分组/多条支路上都出现时只留一个候选,并记住是哪条分组带过去的
fn line_step(g: &Graph, cursor: usize, next: bool, focus: &[usize]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (group, path) in g.series() {
        if !focus.is_empty() && !focus.contains(group) {
            continue;
        }
        let Some(i) = path.iter().position(|&s| s == cursor) else {
            continue;
        };
        let j = if next {
            i + 1
        } else if i == 0 {
            continue;
        } else {
            i - 1
        };
        if j >= path.len() {
            continue;
        }
        let st = path[j];
        if st == cursor || out.iter().any(|&(s, _)| s == st) {
            continue;
        }
        out.push((st, *group));
    }
    out
}

/// 方向键选节点的完整过程:返回 (选中, 按分数排好的候选 [(节点, 分数, 是否同组)]).
///
/// 打分 `距离 + side_w x 横向偏斜`.候选分两档:
///   0 档 = "同一条分组"的节点(聚焦了某几条就按它们算;没聚焦就按**光标节点自己的分组**)
///   1 档 = 其它
/// 0 档里最好的那个只要不超过全局最优的 cycle 倍就走它 ——
/// cycle 调小就更认"屏幕上最近",调大就更认"同一条分组";side_w 调小则纯看远近.
fn step_cfg(
    g: &Graph,
    sc: &Scene,
    proj: &render::Proj,
    cursor: usize,
    dir: (i32, i32),
    focus: &[usize],
    cycle: f64,
    side_w: f64,
) -> (usize, Vec<(usize, f64, bool)>) {
    if cursor >= sc.pos.len() || cursor >= g.nodes.len() {
        return (cursor, Vec::new());
    }
    let cur = proj.cell(sc.pos[cursor]);
    let (dc, dr) = (dir.0 as f64, dir.1 as f64);
    let mut cands: Vec<(u8, f64, usize, bool)> = Vec::new();
    for (i, p) in sc.pos.iter().enumerate() {
        if i == cursor || i >= g.nodes.len() {
            continue;
        }
        let c = proj.cell(*p);
        let (dx, dy) = ((c.0 - cur.0) as f64, (c.1 - cur.1) as f64);
        let along = dx * dc + dy * dr;
        if along <= 0.0 {
            continue; // 不在这个方向上
        }
        let side = (dx * dr - dy * dc).abs();
        if side > along {
            continue; // 偏出去超过 45 度,不算"这个方向"
        }
        // "同一条分组"这一档:聚焦了几条就按它算;没聚焦就按光标节点自己的分组算
        let mine: &[usize] = if focus.is_empty() {
            &g.node_groups[cursor]
        } else {
            focus
        };
        let focused = g.node_groups[i].iter().any(|l| mine.contains(l));
        let tier = if focused { 0u8 } else { 1u8 };
        // 分数:距离 + side_w 倍的横向偏斜.偏斜权重要压得住"斜着挨着"的节点 ——
        // 不然按 k 会挑右上/左上挨着的那个,而不是正上方那个
        let score = (dx * dx + dy * dy).sqrt() + side * side_w;
        cands.push((tier, score, i, focused));
    }
    cands.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let ranked: Vec<(usize, f64, bool)> = cands.iter().map(|&(_, s, i, f)| (i, s, f)).collect();
    let all_min = cands
        .iter()
        .map(|&(_, s, _, _)| s)
        .fold(f64::INFINITY, f64::min);
    // 0 档最好的那个只要不超过全局最优的 cycle 倍,就走它(cycle 设 0 就是不额外优先)
    if let Some(&(0, score, i, _)) = cands.first() {
        if score <= cycle * all_min {
            return (i, ranked);
        }
    }
    let best = cands
        .iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|&(_, _, i, _)| i)
        .unwrap_or(cursor);
    (best, ranked)
}

/// 方向键走了谁、为什么:把分数最高的几个候选拼成一行提示(调 :side / :cycle 时看这个)
fn explain_dir(
    g: &Graph,
    dir: (i32, i32),
    picked: usize,
    cands: &[(usize, f64, bool)],
    cycle: f64,
    side: f64,
    lang: Lang,
) -> String {
    let key = match dir {
        (0, -1) => "k",
        (0, 1) => "j",
        (-1, 0) => "h",
        _ => "l",
    };
    let mut s = lang.pick_owned(
        format!(
            "{key}(cycle {cycle:.2} side {side:.2}) 选中 {};",
            g.nodes[picked].label
        ),
        format!(
            "{key}(cycle {cycle:.2} side {side:.2}) picked {};",
            g.nodes[picked].label
        ),
    );
    for (n, score, focused) in cands.iter().take(3) {
        s += &lang.pick_owned(
            format!(
                " {} {score:.2}{}",
                g.nodes[*n].label,
                if *focused { " 同组" } else { " 异组" }
            ),
            format!(
                " {} {score:.2}{}",
                g.nodes[*n].label,
                if *focused { " same group" } else { " other group" }
            ),
        );
    }
    s
}

/// 平移:按**时长**缓动 —— 记下起点、终点、开始时刻,每帧按 `已过时间 / 时长` 求进度,
/// 再乘一条缓动曲线(地图软件的标准做法).时长基本不随距离变,远的只多给一点并封顶.
/// 曲线用 ease-in(进度取平方):起步慢,看得清是往哪边挪;越接近终点越快,到了就干脆停住
fn animate(view: &mut View, scene: &Scene, cell: f64) -> bool {
    let Some(to) = view.target else {
        view.pan = None;
        return false;
    };
    let cur = view.center.unwrap_or_else(|| scene.center());
    let (cw, ch) = (cell.max(1e-6), (cell * scene.aspect).max(1e-6));
    let pan = match view.pan {
        Some(p) if p.to == to => p, // 还是同一段,接着走
        _ => {
            // 新的一段:按时长决定走多久
            let cells = ((to.0 - cur.0) / cw).abs().max(((to.1 - cur.1) / ch).abs());
            let pan = Pan {
                from: cur,
                to,
                start: Instant::now(),
                dur: (0.16 + cells * 0.002).clamp(PAN_MIN, PAN_MAX),
            };
            view.pan = Some(pan);
            pan
        }
    };
    let t = (pan.start.elapsed().as_secs_f64() / pan.dur).clamp(0.0, 1.0);
    let e = t * t;
    view.center = Some((
        pan.from.0 + (pan.to.0 - pan.from.0) * e,
        pan.from.1 + (pan.to.1 - pan.from.1) * e,
    ));
    if t >= 1.0 {
        view.center = Some(pan.to);
        view.pan = None;
        view.target = None;
        return false;
    }
    true
}

/// 让视图平滑地挪到某一点(画布坐标):已经在动的就从当前显示的位置接着走
fn pan_to(view: &mut View, scene: &Scene, p: (f64, f64)) {
    if view.center.is_none() {
        view.center = Some(scene.center());
    }
    view.target = Some(p);
}

/// 把这几个节点的包围盒铺满地图区:返回 (缩放, 中心).
/// 和 render 里铺满整幅图那套缩放共用一份公式,所以结果直接塞回 view 就行
fn fit_to(
    sc: &Scene,
    map: Rect,
    pts: impl Iterator<Item = (f64, f64)>,
) -> Option<(f64, (f64, f64))> {
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for p in pts {
        b = Some(match b {
            None => (p.0, p.1, p.0, p.1),
            Some((x0, y0, x1, y1)) => (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1)),
        });
    }
    let (x0, y0, x1, y1) = b?;
    let (bw, bh) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let cols = (map.width as f64 - 4.0).max(8.0);
    let rows = (map.height as f64 - 2.0).max(4.0) * sc.aspect;
    let want = (bw / cols).max(bh / rows);
    let full = render::fit_scale(sc, map.width, map.height);
    Some(((full / want).clamp(ZOOM_MIN, ZOOM_MAX), ((x0 + x1) / 2.0, (y0 + y1) / 2.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 两条分组在节点 1 交叉:分组 0 走 0-1-2,分组 1 走 3-1-4
    fn 样例图() -> Graph {
        let mut b = Graph::builder();
        let n0 = b.node("甲", Some((0.0, 0.0)));
        let n1 = b.node("乙", Some((2.0, 0.0)));
        let n2 = b.node("丙", Some((4.0, 0.0)));
        let n3 = b.node("丁", Some((2.0, 3.0)));
        let n4 = b.node("戊", Some((2.0, -3.0)));
        let g0 = b.group("1号线", Some((0xff, 0, 0)));
        let g1 = b.group("2号线", Some((0, 0xff, 0)));
        b.edge(n0, n1, g0, false);
        b.edge(n1, n2, g0, false);
        b.edge(n3, n1, g1, false);
        b.edge(n1, n4, g1, false);
        b.build().unwrap()
    }

    /// 一条直线上排开的 21 个节点,用来验证"光标跑出视野"
    fn 长图() -> Graph {
        let mut b = Graph::builder();
        let g = b.group("主线", Some((0xff, 0, 0)));
        let mut prev = b.node("n0", Some((0.0, 0.0)));
        for i in 1..=20 {
            let n = b.node(format!("n{i}"), Some((i as f64 * 2.0, 0.0)));
            b.edge(prev, n, g, false);
            prev = n;
        }
        b.build().unwrap()
    }

    /// 不走布局引擎,直接照节点坐标搭一个 App:这几条测试只看交互
    fn 搭(g: Graph) -> App {
        let pos: Vec<(f64, f64)> = (0..g.nodes.len()).map(|i| g.pos(i)).collect();
        let scene = Scene::from_pos(pos, 1.0, false);
        let view = View {
            layout: Layout::Geo,
            ..View::default()
        };
        App {
            graph: g,
            scene,
            view,
            source: None,
            area: Rect::default(),
            pending: None,
            drag: None,
            search_from: 0,
        }
    }

    fn 键(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn 在画面里(p: &render::Panels, c: (i32, i32)) -> bool {
        c.0 >= p.map.x as i32
            && c.0 < (p.map.x + p.map.width) as i32
            && c.1 >= p.map.y as i32
            && c.1 < (p.map.y + p.map.height) as i32
    }

    #[test]
    fn 数字前缀() {
        let mut p: Option<u32> = None;
        assert!(count_digit(&mut p, '5'));
        assert_eq!(p, Some(5));
        assert!(count_digit(&mut p, '0'));
        assert_eq!(p, Some(50), "10l 里的 0 该接着前面");
        assert!(count_digit(&mut p, '0'));
        assert_eq!(p, Some(500));
        let mut q: Option<u32> = None;
        assert!(!count_digit(&mut q, '0'), "没前缀时 0 不算计数(是复位)");
        assert_eq!(q, None);
        assert!(!count_digit(&mut q, 'x'), "不是数字就不算");
        let mut r: Option<u32> = Some(999);
        assert!(count_digit(&mut r, '9'));
        assert_eq!(r, Some(999), "计数封顶");
    }

    #[test]
    fn 跳转历史从中间跳走会截断尾部() {
        let mut view = View::default();
        view.cursor = 10;
        note_here(&mut view);
        view.cursor = 20;
        note_here(&mut view);
        view.cursor = 30;
        note_here(&mut view);
        assert_eq!(view.history, vec![10, 20, 30]);
        assert!(jump_history(&mut view, true));
        assert_eq!(view.cursor, 20);
        // 从历史中间(20)跳到 99:后面那段(30)该丢掉
        view.cursor = 99;
        note_here(&mut view);
        assert_eq!(view.history, vec![10, 20, 99]);
        assert_eq!(view.hist_at, 2);
        assert!(!jump_history(&mut view, false), "到尾巴了就不动");
        // 光标已经在历史最后一条上:再记一次只把指针放回末尾,不重复入栈
        view.hist_at = 0;
        note_here(&mut view);
        assert_eq!(view.history, vec![10, 20, 99]);
        assert_eq!(view.hist_at, 2);
    }

    #[test]
    fn 搜索命中刷新() {
        let g = 样例图();
        let mut view = View::default();
        view.query = Some("1".into());
        refresh(&g, &mut view);
        assert_eq!(view.focus, vec![0]);
        assert_eq!(view.hits, vec![0, 1, 2]);
        assert_eq!(view.hit_at, 0);
        // 文字命中:光标跳到第一个,聚焦它经过的分组
        view.query = Some("丙".into());
        refresh(&g, &mut view);
        assert_eq!(view.cursor, 2);
        assert_eq!(view.hits, vec![2]);
        assert_eq!(view.focus, vec![0]);
        // 搜不到:命中与聚焦都清掉
        view.query = Some("zzzz".into());
        refresh(&g, &mut view);
        assert!(view.hits.is_empty());
        assert!(view.focus.is_empty());
    }

    #[test]
    fn 沿线走一站有多个候选时让用户挑() {
        let mut app = 搭(样例图());
        app.view.cursor = 1; // 乙:两条分组都能往下走
        app.handle(键(KeyCode::Char('J')));
        let c = app.view.choice.as_ref().expect("应该弹出候选");
        assert_eq!(c.items, vec![(2, 0), (4, 1)]);
        assert_eq!(c.at, 0);
        // j 挪一格,回车确定
        app.handle(键(KeyCode::Char('j')));
        assert_eq!(app.view.choice.as_ref().unwrap().at, 1);
        app.handle(键(KeyCode::Enter));
        assert!(app.view.choice.is_none());
        assert_eq!(app.view.cursor, 4);
        // esc 取消:光标不动
        app.view.cursor = 1;
        app.handle(键(KeyCode::Char('J')));
        app.handle(键(KeyCode::Esc));
        assert!(app.view.choice.is_none());
        assert_eq!(app.view.cursor, 1);
        // 数字直选第二项
        app.handle(键(KeyCode::Char('J')));
        app.handle(键(KeyCode::Char('2')));
        assert!(app.view.choice.is_none());
        assert_eq!(app.view.cursor, 4);
    }

    #[test]
    fn 沿线走一站只有一个候选就直接走() {
        let mut app = 搭(样例图());
        app.view.cursor = 2; // 丙:只有 1 号线能退回去
        app.handle(键(KeyCode::Char('H')));
        assert!(app.view.choice.is_none());
        assert_eq!(app.view.cursor, 1);
        assert!(app.view.note.is_some());
        // 聚焦一条分组之后,枢纽上也不再有别的候选
        app.view.focus = vec![1];
        app.handle(键(KeyCode::Char('J')));
        assert!(app.view.choice.is_none());
        assert_eq!(app.view.cursor, 4);
    }

    #[test]
    fn 冒号命令解析() {
        let mut app = 搭(样例图());
        app.cmd_apply("side 3.5");
        assert_eq!(app.view.side, 3.5);
        app.cmd_apply("cycle 4");
        assert_eq!(app.view.cycle, Some(4.0));
        app.cmd_apply("cycle auto");
        assert_eq!(app.view.cycle, None);
        app.cmd_apply("layout geo");
        assert_eq!(app.view.layout, Layout::Geo);
        app.cmd_apply("layout");
        assert!(app.view.note.as_deref().unwrap().contains("geo"));
        app.cmd_apply("lang en");
        assert_eq!(app.view.lang, Lang::En);
        // 非法输入只回显,不动状态
        app.cmd_apply("side 大");
        assert_eq!(app.view.side, 3.5);
        app.cmd_apply("cycle 大");
        assert_eq!(app.view.cycle, None);
        app.cmd_apply("layout 没有这套");
        assert_eq!(app.view.layout, Layout::Geo);
        app.cmd_apply("lang ??");
        assert_eq!(app.view.lang, Lang::En);
        app.cmd_apply("瞎写的命令");
        assert!(app.view.note.is_some());
        // 空行清掉回显
        app.cmd_apply("");
        assert!(app.view.note.is_none());
    }

    #[test]
    fn r_循环四套布局() {
        let mut app = 搭(样例图());
        app.view.zoom = 3.0;
        let cursor = app.view.cursor;
        for want in [Layout::Relax, Layout::Force, Layout::Layered, Layout::Geo] {
            app.handle(键(KeyCode::Char('r')));
            assert_eq!(app.view.layout, want);
            assert_eq!(app.scene.pos.len(), app.graph.nodes.len());
            assert_eq!(app.view.cursor, cursor, "换坐标不动光标");
            assert_eq!(app.view.zoom, 3.0, "换坐标不动缩放");
        }
    }

    #[test]
    fn 光标跑出视野会把视图拉过去() {
        let mut app = 搭(长图());
        app.area = Rect::new(0, 0, 80, 24);
        app.view.zoom = 5.0; // 放大到只看得到中间一小段
        let far = app.graph.nodes.len() - 1;
        let before = render::panels(&app.graph, &app.scene, app.area, &app.view);
        assert!(
            !在画面里(&before, before.proj.cell(app.scene.pos[far])),
            "放大之后最右那个节点该在画面外"
        );
        app.view.hits = vec![far];
        app.view.hit_at = 0;
        app.handle(键(KeyCode::Char('n')));
        assert_eq!(app.view.cursor, far);
        assert!(app.busy(), "该开始往那边平移");
        app.tick();
        if let Some(p) = app.view.pan.as_mut() {
            p.start = Instant::now() - Duration::from_secs(5); // 假装时间到了
        }
        app.tick();
        assert!(!app.busy());
        let after = render::panels(&app.graph, &app.scene, app.area, &app.view);
        assert!(
            在画面里(&after, after.proj.cell(app.scene.pos[far])),
            "平移结束后光标该回到画面里"
        );
    }

    #[test]
    fn 开局按节点名定位光标() {
        let app = App::new(
            样例图(),
            Options {
                at: Some("乙".into()),
                ..Default::default()
            },
        )
        .expect("开局");
        assert_eq!(app.view.cursor, 1);
        // 找不到:说一声,别拦着开局
        let app = App::new(
            样例图(),
            Options {
                at: Some("没有这个节点".into()),
                ..Default::default()
            },
        )
        .expect("开局");
        assert!(app.view.note.is_some());
    }

    #[test]
    fn 开局搜索命中会聚焦分组() {
        let app = App::new(
            样例图(),
            Options {
                find: Some("2".into()),
                ..Default::default()
            },
        )
        .expect("开局");
        assert_eq!(app.view.focus, vec![1]);
        assert_eq!(app.view.hits, vec![1, 3, 4]);
        assert!(app.view.query.is_none());
    }

    #[test]
    fn 开局搜不到会把说明写进提示行() {
        let app = App::new(
            样例图(),
            Options {
                find: Some("zzzz".into()),
                ..Default::default()
            },
        )
        .expect("开局");
        assert!(app.view.hits.is_empty());
        assert!(app.view.note.is_some());
    }
}
