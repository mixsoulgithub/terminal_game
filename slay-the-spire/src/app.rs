// 输入状态机:所有键位都在这里,UI 只读状态.
// 操作风格向 vim 靠:地图用 h/l 往前后看路、j/k 选岔路,enter 确认,esc 取消,: 开命令行.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::compendium::{self, Library};
use crate::core::corpus::CharacterInfo;
use crate::core::roster;
use crate::core::run::{RewardSlot, Run, Screen};
use crate::core::save;
use crate::ui::overlay::Overlay;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Command,
}

/// 战斗里的一次抖动动画:谁、往哪边、还剩几帧、起步延迟几帧、还要重抖几次
pub struct BattleShake {
    pub who: crate::core::combat::ShakeWho,
    pub dir: i32,
    pub frames: u8,
    /// 起步前先等几帧(挨打的比出手的晚一帧)
    pub delay: u8,
    /// 连击/多段:抖完一轮再来几轮(3x7 就抖 3 次,X 费就抖 X 次)
    pub repeats: u8,
    /// true = 这次是自己出手(要顶到最大幅度停几帧),false = 挨打
    pub attacking: bool,
    /// 这次抖动的最大幅度(挨打按伤害分档,出手固定 2)
    pub amp: i32,
    /// 这次抖动的总帧数:幅度越大帧数越多,看着才平滑
    pub total: u8,
}

/// 命令名(第一层补全用),按字典序不排序也行,补全时会排
const COMMANDS: &[&str] = &[
    "card", "help", "q", "quit", "relic", "restart", "room", "run", "save", "seed", "win",
];
/// :room / :relic / :card 的参数
const ROOM_ARGS: &[&str] = &["battle", "boss", "elite", "enemy", "event", "shop"];
const RELIC_ARGS: &[&str] = &["add", "remove"];
const CARD_ARGS: &[&str] = &["add", "pile", "remove", "upgrade"];
const RUN_ARGS: &[&str] = &["save", "seed"];
const RESTART_ARGS: &[&str] = &["fight", "run", "turn"];

pub struct App {
    pub run: Run,
    pub mode: Mode,
    pub cmd: String,
    pub overlay: Option<Overlay>,
    pub overlay_scroll: u16,
    /// 上一次渲染算出的最大滚动量:到底之后再按 j 不会继续累加
    pub overlay_max: std::cell::Cell<u16>,
    /// 上一次 Tab 补出来的候选表与位置;连续按 Tab 就在里面循环
    comp_list: Vec<String>,
    comp_idx: usize,
    comp_pending: bool,
    /// 看牌组窗口里的光标(行下标,跳过分组标题)
    pub overlay_sel: usize,
    /// 手牌光标
    pub hand_sel: usize,
    /// 敌人光标
    pub target_sel: usize,
    /// 可达节点里的选择序号(岔路)
    pub map_sel: usize,
    /// 地图视野的起始层:l 往前看,h 往后看
    pub map_scroll: usize,
    /// 最近一次已知的终端尺寸,地图要用它算一屏放几层
    pub term_size: (u16, u16),
    pub rest_index: usize,
    /// 顶栏里正在挑的药水槽(Some 时 p 之后的上下选择模式)
    pub potion_sel: Option<usize>,
    /// 等待选定目标的药水槽
    pub potion_pending: Option<usize>,
    /// 药水列表里按过 t,下一个数字是"丢掉"而不是"喝掉"
    pub toss_pending: bool,
    pub msg: String,
    pub warn: bool,
    pub quit: bool,
    /// 开始界面光标
    pub title_sel: usize,
    /// 角色选择光标
    pub char_sel: usize,
    /// 图鉴子菜单光标
    pub comp_sel: usize,
    /// 当前看的图鉴种类
    pub library: Library,
    /// 消息还能显示几帧(0 = 不管了,交给下一次操作)
    pub msg_ttl: u8,
    /// 手牌选择模式里已经选中的手牌下标
    pub choice_sel: Vec<usize>,
    /// 战斗里的抖动(掉血/出手),自带帧数与重复次数
    pub battle_shakes: Vec<BattleShake>,
    /// 浩劫链的播报:(层级, 牌名, 还剩几帧)
    pub play_banners: Vec<(u8, String, u8)>,
    /// 战斗开始时的快照(:restart fight)
    pub fight_snap: Option<crate::core::combat::Combat>,
    /// 本回合开始时的快照(:restart turn)
    turn_snap: Option<crate::core::combat::Combat>,
    /// 快照对应的 战斗序号 / 回合数
    snap_fight_seq: u64,
    snap_turn: u32,

    /// 商店里买不成时抖一下动画:还剩几帧
    pub shake: u8,
    /// 抖的是哪一行
    pub shake_row: Option<usize>,
    /// 图鉴当前标签页
    pub lib_tab: usize,
    /// 图鉴光标(当前页里的条目下标)
    pub lib_sel: usize,
}

impl App {
    pub fn new(seed: u64) -> App {
        App {
            run: Run::new(seed),
            mode: Mode::Normal,
            cmd: String::new(),
            comp_list: Vec::new(),
            comp_idx: 0,
            comp_pending: false,
            overlay: None,
            overlay_scroll: 0,
            overlay_max: std::cell::Cell::new(0),
            overlay_sel: 0,
            hand_sel: 0,
            target_sel: 0,
            map_sel: 0,
            map_scroll: 0,
            term_size: (100, 30),
            rest_index: 0,
            potion_sel: None,
            potion_pending: None,
            toss_pending: false,
            msg: String::new(),
            warn: false,
            quit: false,
            title_sel: 0,
            char_sel: 0,
            comp_sel: 0,
            msg_ttl: 0,
            choice_sel: Vec::new(),
            battle_shakes: Vec::new(),
            play_banners: Vec::new(),
            fight_snap: None,
            turn_snap: None,
            snap_fight_seq: 0,
            snap_turn: 0,
            shake: 0,
            shake_row: None,
            library: Library::Cards,
            lib_tab: 0,
            lib_sel: 0,
        }
    }

    /// 真正的入口:从开始界面进(开始界面/角色选择/图鉴)
    pub fn start(seed: u64) -> App {
        let mut app = App::new(seed);
        app.run.screen = Screen::Title;
        // 底栏一直显示按键提示,这里不再塞一遍
        app.msg = String::new();
        app
    }

    /// 开始界面的键位:不碰 run 的其它状态
    fn start_key(&mut self, key: KeyEvent) {
        match self.run.screen {
            Screen::Title => self.title_key(key),
            Screen::CharSelect => self.char_key(key),
            Screen::Compendium => self.comp_key(key),
            Screen::Library => self.lib_key(key),
            _ => {}
        }
    }

    /// 开始界面上的条目:(名字, 能不能选, 说明)
    pub fn title_entries(&self) -> Vec<(&'static str, bool, &'static str)> {
        vec![
            ("continue", save::exists(), "resume the saved run"),
            ("new game", true, "pick a character"),
            ("compendium", true, "cards, relics and potions"),
            ("quit", true, "leave the spire"),
        ]
    }

    fn title_key(&mut self, key: KeyEvent) {
        let n = self.title_entries().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.title_sel = (self.title_sel + 1) % n,
            KeyCode::Char('k') | KeyCode::Up => self.title_sel = (self.title_sel + n - 1) % n,
            KeyCode::Char('g') => self.title_sel = 0,
            KeyCode::Char('G') => self.title_sel = n - 1,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Enter | KeyCode::Char(' ') => {
                let (label, ok, _) = self.title_entries()[self.title_sel];
                if !ok {
                    self.warn("no saved run");
                    return;
                }
                match label {
                    "continue" => self.load_save(),
                    "new game" => {
                        self.run.screen = Screen::CharSelect;
                        self.char_sel = 0;
                        self.msg = String::new();
                        self.warn = false;
                    }
                    "compendium" => {
                        self.run.screen = Screen::Compendium;
                        self.comp_sel = 0;
                        self.msg = String::new();
                    }
                    _ => self.quit = true,
                }
            }
            KeyCode::Esc => self.quit = true,
            _ => {}
        }
    }

    fn char_key(&mut self, key: KeyEvent) {
        let n = roster::all().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.char_sel = (self.char_sel + 1) % n,
            KeyCode::Char('k') | KeyCode::Up => self.char_sel = (self.char_sel + n - 1) % n,
            KeyCode::Char('g') => self.char_sel = 0,
            KeyCode::Char('G') => self.char_sel = n - 1,
            KeyCode::Esc | KeyCode::Char('q') => {
                self.run.screen = Screen::Title;
                self.msg = String::new();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let ch: &'static CharacterInfo = roster::by_index(self.char_sel);
                match roster::blocked_reason(ch) {
                    Some(reason) => self.warn(format!("{} not playable yet: {}", ch.name, reason)),
                    None => self.start_run(ch),
                }
            }
            _ => {}
        }
    }

    /// 开一局新游戏:建 run,然后进 Neow 的祝福
    pub fn start_run(&mut self, ch: &'static CharacterInfo) {
        let seed = self.run.seed;
        match Run::new_for(seed, ch) {
            Ok(mut run) => {
                run.open_neow();
                self.run = run;
                self.hand_sel = 0;
                self.target_sel = 0;
                self.map_sel = 0;
                self.map_scroll = 0;
                self.rest_index = 0;
                self.potion_sel = None;
                self.potion_pending = None;
                save::clear();
                self.info(format!("{}: seed {seed}", ch.name));
            }
            Err(e) => self.warn(e),
        }
    }

    fn comp_key(&mut self, key: KeyEvent) {
        let n = Library::ALL.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.comp_sel = (self.comp_sel + 1) % n,
            KeyCode::Char('k') | KeyCode::Up => self.comp_sel = (self.comp_sel + n - 1) % n,
            KeyCode::Esc | KeyCode::Char('q') => {
                self.run.screen = Screen::Title;
                self.msg = String::new();
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.library = Library::ALL[self.comp_sel % n];
                self.lib_tab = 0;
                self.lib_sel = 0;
                self.run.screen = Screen::Library;
                self.msg = String::new();
            }
            _ => {}
        }
    }

    /// 图鉴:h/l/Tab 换标签页(循环),j/k 在页内选(循环),1/2/3 换册子
    fn lib_key(&mut self, key: KeyEvent) {
        let tabs = compendium::groups(self.library).len().max(1);
        let n = compendium::items(self.library, self.lib_tab).len();
        let step = |cur: usize, d: i32| -> usize {
            if n == 0 {
                return 0;
            }
            (cur as i32 + d).rem_euclid(n as i32) as usize
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.lib_sel = step(self.lib_sel, 1),
            KeyCode::Char('k') | KeyCode::Up => self.lib_sel = step(self.lib_sel, -1),
            KeyCode::Char('g') => self.lib_sel = 0,
            KeyCode::Char('G') => self.lib_sel = n.saturating_sub(1),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
                self.lib_tab = (self.lib_tab + 1) % tabs;
                self.lib_sel = 0;
                self.info(self.tab_title());
            }
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
                self.lib_tab = (self.lib_tab + tabs - 1) % tabs;
                self.lib_sel = 0;
                self.info(self.tab_title());
            }
            KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
                let i = match key.code {
                    KeyCode::Char('1') => 0,
                    KeyCode::Char('2') => 1,
                    _ => 2,
                };
                self.library = Library::ALL[i];
                self.lib_tab = 0;
                self.lib_sel = 0;
                self.info(self.library.title());
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.run.screen = Screen::Compendium;
                self.msg = String::new();
            }
            _ => {}
        }
        // 换页之后光标要落在本页条目里
        let n = compendium::items(self.library, self.lib_tab).len();
        self.lib_sel = if n == 0 { 0 } else { self.lib_sel.min(n - 1) };
    }

    /// 当前标签页的名字,状态栏显示用
    pub fn tab_title(&self) -> String {
        let gs = compendium::groups(self.library);
        let g = gs
            .get(self.lib_tab % gs.len().max(1))
            .map(|g| g.name)
            .unwrap_or("");
        format!("{} / {}", self.library.title(), g)
    }

    /// 把一局装进来,顺手把各种光标复位
    fn adopt_run(&mut self, run: Run) {
        self.run = run;
        self.hand_sel = 0;
        self.target_sel = 0;
        self.map_sel = 0;
        self.map_scroll = 0;
        self.rest_index = 0;
        self.potion_sel = None;
        self.potion_pending = None;
        self.overlay = None;
        self.choice_sel.clear();
        self.battle_shakes.clear();
    }

    fn load_save(&mut self) {
        let Some(text) = save::read() else {
            self.warn("no saved run");
            return;
        };
        match Run::from_save(&text) {
            Ok(run) => {
                self.adopt_run(run);
                self.info("continued run");
            }
            Err(e) => self.warn(e),
        }
    }

    /// 在地图界面(每层开头)自动存档;结算时清掉存档。
    /// 只有真正的事件循环会调它,单测不该碰磁盘。
    pub fn maybe_save(&mut self) {
        match self.run.screen {
            Screen::Map => {
                let _ = save::write(&self.run.save_text());
            }
            Screen::Victory | Screen::Death => save::clear(),
            _ => {}
        }
    }

    pub fn restart(&mut self, seed: u64) {
        self.run = Run::new(seed);
        self.mode = Mode::Normal;
        self.cmd.clear();
        self.overlay = None;
        self.overlay_scroll = 0;
        self.overlay_max.set(0);
        self.overlay_sel = 0;
        self.hand_sel = 0;
        self.target_sel = 0;
        self.map_sel = 0;
        self.map_scroll = 0;
        self.rest_index = 0;
        self.potion_sel = None;
        self.potion_pending = None;
        self.toss_pending = false;
        self.info(format!("new run, seed {seed}"));
    }

    /// 命令行的补全候选:只算当前这一层。
    /// 还没敲空格 -> 补命令名;敲了空格 -> 补这条命令的参数。
    pub fn completions(&self) -> Vec<String> {
        let text = self.cmd.as_str();
        match text.split_once(' ') {
            None => matching(COMMANDS, text),
            Some((head, rest)) => {
                let cur = rest.rsplit(' ').next().unwrap_or("");
                // 命令名只敲了一半(ro)也算,唯一匹配才认
                // :run save <名字> 的候选是存档目录里的名字
                if resolve_command(head).as_deref() == Some("run")
                    && rest.split_whitespace().next() == Some("save")
                    && rest.contains(' ')
                {
                    let low = cur.to_lowercase();
                    let mut names: Vec<String> = crate::core::save::list()
                        .into_iter()
                        .filter(|n| n.to_lowercase().starts_with(&low))
                        .collect();
                    names.sort();
                    return names;
                }
                let args: &[&str] = match resolve_command(head).as_deref() {
                    Some("room") => ROOM_ARGS,
                    Some("relic") => RELIC_ARGS,
                    Some("card") => CARD_ARGS,
                    Some("run") => RUN_ARGS,
                    Some("restart") => RESTART_ARGS,
                    _ => &[],
                };
                matching(args, cur)
            }
        }
    }

    /// 抖动动画总共几帧(每帧间隔见 main.rs 的轮询时间)
    pub const SHAKE_FRAMES: u8 = 6;
    /// 挨打的比出手的晚几帧起步
    pub const HURT_DELAY: u8 = 2;
    /// 出手方收回来用几帧:1 帧(松手就直接弹回原位,不拖)
    pub const ATTACK_RETURN: u8 = 1;
    /// 挨打的伤害分档:达到 5 / 20 / 50 各涨一档幅度
    pub const HURT_STEPS: [i32; 3] = [5, 20, 50];

    /// 挨打该抖多大:伤害越高幅度越大(不够 5 点就是最小的一档)
    pub fn hurt_amp(damage: i32) -> i32 {
        1 + Self::HURT_STEPS.iter().filter(|t| damage >= **t).count() as i32
    }

    /// 幅度越大越要多给几帧,不然大抖会显得一跳一跳
    fn hurt_total(amp: i32) -> u8 {
        Self::SHAKE_FRAMES + (amp.max(2) - 2) as u8 * 2
    }
    /// 消息保留多少帧(60ms 一帧,50 帧约 3 秒)
    pub const MSG_FRAMES: u8 = 50;
    /// 对面还剩几帧就松手:比对面抖完稍微早一点开始收
    pub const RELEASE_EARLY: u8 = 2;

    /// 浩劫链播报的基础时长(60ms 一帧,40 帧约 2.4 秒)
    pub const BANNER_FRAMES: u8 = 40;

    /// 链越深停得越久:第 1 层是基础时长,每深一层多四分之一个基础时长
    pub fn banner_frames(depth: u8) -> u8 {
        Self::BANNER_FRAMES.saturating_add(
            depth
                .saturating_sub(1)
                .saturating_mul(Self::BANNER_FRAMES / 4),
        )
    }

    /// 还在抖(或有消息要倒计时):事件循环要用超时轮询,好一帧帧重画
    pub fn ticking(&self) -> bool {
        self.shake > 0
            || self.msg_ttl > 0
            || !self.battle_shakes.is_empty()
            || !self.play_banners.is_empty()
            || self.run.holding_victory()
    }

    /// 走一帧;返回是否还要继续。
    /// 出手方会一直顶在最大幅度上,直到对面把自己的抖动演完才收回来。
    pub fn tick(&mut self) -> bool {
        use crate::core::combat::ShakeWho;
        self.run.tick_win_hold();
        self.shake = self.shake.saturating_sub(1);
        if self.msg_ttl > 0 {
            self.msg_ttl -= 1;
            if self.msg_ttl == 0 {
                self.msg.clear();
            }
        }
        for b in self.play_banners.iter_mut() {
            b.2 = b.2.saturating_sub(1);
        }
        self.play_banners.retain(|b| b.2 > 0);
        // 两侧挨打的抖动还剩几帧(出手方要盯着对面这个数)
        let hurt_left = |hero: bool| -> u8 {
            self.battle_shakes
                .iter()
                .filter(|b| !b.attacking && matches!(b.who, ShakeWho::Hero) == hero)
                .map(|b| b.frames)
                .max()
                .unwrap_or(0)
        };
        let hero_hurt = hurt_left(true);
        let enemy_hurt = hurt_left(false);
        for b in self.battle_shakes.iter_mut() {
            if b.delay > 0 {
                b.delay -= 1;
                continue;
            }
            if b.attacking {
                let left = match b.who {
                    ShakeWho::Hero => enemy_hurt,
                    ShakeWho::Enemy(_) => hero_hurt,
                };
                // 对面还剩得多就继续顶住;快演完了(留 RELEASE_EARLY 帧)就松手
                let waiting = left > Self::RELEASE_EARLY;
                if waiting {
                    // 顶住:帧数不走,对面演完再开始收
                    b.frames = b.total;
                    continue;
                }
                if b.frames >= b.total {
                    // 对面演完了:进入收回阶段
                    b.frames = Self::ATTACK_RETURN;
                    continue;
                }
            }
            b.frames = b.frames.saturating_sub(1);
            if b.frames == 0 && b.repeats > 0 {
                // 多段/连击:再来一轮
                b.repeats -= 1;
                b.frames = b.total;
            }
        }
        self.battle_shakes.retain(|b| b.frames > 0);
        self.ticking()
    }

    /// 战斗里某个目标现在往哪边挪几格(攻击方朝对面冲,挨打的往反方向退)
    pub fn battle_shake_offset(&self, who: crate::core::combat::ShakeWho) -> i32 {
        let Some(b) = self.battle_shakes.iter().find(|b| b.who == who) else {
            return 0;
        };
        if b.delay > 0 {
            return 0;
        }
        // 出手:顶在最大幅度上(帧数由 tick 控制,对面演完才收回来)
        // 挨打:从自己的最大幅度平滑递减到 0
        let step = (b.total - b.frames) as f32;
        let amp = if b.attacking {
            // 松手即弹回:顶住时是最大幅度,一进入收回阶段就是 0
            if b.frames > Self::ATTACK_RETURN {
                b.amp
            } else {
                0
            }
        } else {
            let frac = (step / b.total.max(1) as f32).clamp(0.0, 1.0);
            (b.amp as f32 * (1.0 - frac)).round() as i32
        };
        b.dir * amp
    }

    /// 把浩劫链收进来做播报:一层一张,同时消失在计时结束
    fn collect_havoc_chain(&mut self) {
        let Some(c) = self.run.combat_mut() else {
            self.play_banners.clear();
            return;
        };
        let chain = std::mem::take(&mut c.havoc_chain);
        if chain.is_empty() {
            return;
        }
        self.play_banners.clear();
        for (depth, label) in chain {
            self.play_banners
                .push((depth, label, Self::banner_frames(depth)));
        }
    }

    /// 把战斗引擎攒下的抖动事件收进来。
    /// 同一目标已有动画就累加"还要抖几次"(3x7 抖 3 次,X 费抖 X 次);
    /// 挨打比出手晚一帧起步,所以先看到攻击方冲出去,再看到对面挨退。
    fn collect_battle_shakes(&mut self) {
        use crate::core::combat::ShakeKind;
        let Some(c) = self.run.combat_mut() else {
            return;
        };
        for s in std::mem::take(&mut c.shakes) {
            let attacking = s.kind == ShakeKind::Attack;
            let delay = if attacking { 0 } else { Self::HURT_DELAY };
            // 挨打按伤害分档:伤害越大抖得越狠,帧数也跟着加,保持平滑
            let amp = if attacking { 2 } else { Self::hurt_amp(s.amount) };
            let total = if attacking {
                Self::SHAKE_FRAMES
            } else {
                Self::hurt_total(amp)
            };
            match self.battle_shakes.iter_mut().find(|b| b.who == s.who) {
                Some(b) => {
                    b.dir = s.dir;
                    b.repeats = (b.repeats + 1).min(8);
                }
                None => self.battle_shakes.push(BattleShake {
                    who: s.who,
                    dir: s.dir,
                    frames: total,
                    delay,
                    repeats: 0,
                    attacking,
                    amp,
                    total,
                }),
            }
        }
    }

    /// 抖动相位:不针对某一行,消息提示也用这个
    pub fn shake_nudge(&self) -> i32 {
        if self.shake == 0 || (Self::SHAKE_FRAMES - self.shake) % 2 != 0 {
            0
        } else {
            1
        }
    }

    /// 这一行现在往右挪几格:慢慢点两下(每次 1 格),然后回位。
    /// 只往右挪,右边超出去的部分由渲染那层截掉。
    pub fn shake_offset(&self, row: usize) -> i32 {
        if self.shake == 0 || self.shake_row != Some(row) {
            return 0;
        }
        if (Self::SHAKE_FRAMES - self.shake) % 2 == 0 {
            1
        } else {
            0
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.msg = text.into();
        self.msg_ttl = Self::MSG_FRAMES;
        self.warn = false;
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.msg = text.into();
        self.msg_ttl = Self::MSG_FRAMES;
        self.warn = true;
    }

    fn ok_unit(&mut self, r: Result<(), String>) {
        match r {
            Ok(()) => self.info("done"),
            Err(e) => self.warn(e),
        }
    }

    fn ok(&mut self, r: Result<String, String>) {
        match r {
            Ok(m) => self.info(m),
            Err(e) => self.warn(e),
        }
    }

    // ---- 键位分派 ----

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::Command => {
                self.command_key(key);
                return;
            }
            Mode::Normal => {}
        }
        // 命令行与帮助是全局键:开始界面、叠加层开着的时候一样能用
        if key.code == KeyCode::Char(':') {
            self.mode = Mode::Command;
            self.cmd.clear();
            self.comp_pending = false;
            self.comp_list.clear();
            return;
        }
        if key.code == KeyCode::Char('?') {
            self.open_overlay(Overlay::Help);
            return;
        }
        // 叠加层:再按同一个键就关掉,按另一个叠加层键就直接切过去
        if let Some(ov) = self.overlay {
            if let Some(other) = overlay_key_of(self, key.code) {
                self.potion_pending = None;
                self.toss_pending = false;
                if other == ov {
                    self.overlay = None;
                } else {
                    self.open_overlay(other);
                }
                return;
            }
            self.overlay_key(key);
            return;
        }
        // 开始界面/角色选择/图鉴:剩下的键才是菜单键
        if matches!(
            self.run.screen,
            Screen::Title | Screen::CharSelect | Screen::Compendium | Screen::Library
        ) {
            self.start_key(key);
            return;
        }
        // 正在顶栏挑药水:键都交给它
        if self.potion_sel.is_some() {
            self.potion_sel_key(key);
            return;
        }
        if self.potion_pending.is_some() {
            self.potion_target_key(key);
            return;
        }
        // p:不开窗口,直接在顶栏药水区挑第一瓶
        if key.code == KeyCode::Char('p') {
            self.potion_sel = Some(0);
            self.toss_pending = false;
            self.info("h/l choose a potion, enter drink, t then 1-3 toss");
            return;
        }
        // 全局叠加层开关:任何阶段(含结算界面)都能看牌组/地图/遗物/药水
        if let Some(ov) = overlay_key_of(self, key.code) {
            self.open_overlay(ov);
            return;
        }
        match self.run.screen {
            Screen::Map => self.map_key(key),
            Screen::Combat => self.combat_key(key),
            Screen::Reward => self.reward_key(key),
            Screen::Shop => self.shop_key(key),
            Screen::Rest => self.rest_key(key),
            Screen::Event => self.event_key(key),
            Screen::Treasure => self.treasure_key(key),
            Screen::Pick => self.pick_key(key),
            Screen::Victory | Screen::Death => self.over_key(key),
            // 开始界面那几个在上面就返回了
            Screen::Title | Screen::CharSelect | Screen::Compendium | Screen::Library => {}
        }
    }

    fn open_overlay(&mut self, ov: Overlay) {
        self.overlay = Some(ov);
        self.overlay_sel = 0;
        // 历史记录先看最新的一条,其他列表从头看
        self.overlay_scroll = if ov == Overlay::History { u16::MAX / 2 } else { 0 };
        if matches!(
            ov,
            Overlay::Deck | Overlay::Draw | Overlay::Discard | Overlay::Exhaust | Overlay::Offered
        ) {
            // 光标要停在第一张牌上,别停在小标题上
            self.deck_cursor_end(false);
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        let on_map = self.overlay == Some(Overlay::Map);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                let picking = matches!(
                    self.overlay,
                    Some(Overlay::Draw)
                        | Some(Overlay::Discard)
                        | Some(Overlay::Exhaust)
                        | Some(Overlay::Offered)
                );
                self.overlay = None;
                self.potion_pending = None;
                self.toss_pending = false;
                // 正在选牌:esc 等于取消这次出牌
                if picking && self.run.combat().map(|c| c.choice.is_some()).unwrap_or(false) {
                    if let Some(c) = self.run.combat_mut() {
                        c.cancel_choice();
                    }
                    self.choice_sel.clear();
                    self.info("cancelled");
                    self.clamp();
                }
            }
            KeyCode::Char('l') | KeyCode::Right if on_map => self.scroll_map(1),
            KeyCode::Char('h') | KeyCode::Left if on_map => self.scroll_map(-1),
            KeyCode::Char('g') if on_map => self.jump_map(false),
            KeyCode::Char('G') if on_map => self.jump_map(true),
            // 选牌窗口里空格和回车都算"选中光标下那张"(跟手牌模式的空格一致)
            KeyCode::Enter | KeyCode::Char(' ')
                if self.choice_overlay().is_some() && self.overlay == self.choice_overlay() =>
            {
                // 窗口行号 = 候选序号,换回牌堆里的真实下标
                let idx = self
                    .run
                    .combat()
                    .and_then(|c| c.choice_candidates().get(self.overlay_sel).map(|(i, _)| *i));
                let r = match idx {
                    Some(idx) => self
                        .run
                        .combat_mut()
                        .map(|c| c.choose(idx))
                        .unwrap_or_else(|| Err("not in a battle".to_string())),
                    None => {
                        // 候选都选完了,这一下当"收工"
                        if let Some(c) = self.run.combat_mut() {
                            c.finish_choice();
                        }
                        Ok(())
                    }
                };
                self.ok_unit(r);
                if self.run.combat().map(|c| c.choice.is_none()).unwrap_or(false) {
                    self.overlay = None;
                }
                self.clamp();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if matches!(
                    self.overlay,
                    Some(Overlay::Deck)
                        | Some(Overlay::Draw)
                        | Some(Overlay::Discard)
                        | Some(Overlay::Exhaust)
                ) {
                    self.move_deck_cursor(1);
                } else {
                    // 先夹回有效范围(历史记录打开时是从 u16::MAX/2 起步的),
                    // 再 +1;到底之后继续按 j 不会攒着,免得按 k 要先"还回去"
                    let cur = self.overlay_scroll.min(self.overlay_max.get());
                    self.overlay_scroll = cur.saturating_add(1).min(self.overlay_max.get());
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if matches!(
                    self.overlay,
                    Some(Overlay::Deck)
                        | Some(Overlay::Draw)
                        | Some(Overlay::Discard)
                        | Some(Overlay::Exhaust)
                ) {
                    self.move_deck_cursor(-1);
                } else {
                    let cur = self.overlay_scroll.min(self.overlay_max.get());
                    self.overlay_scroll = cur.saturating_sub(1);
                }
            }
            KeyCode::Char('g')
                if matches!(
                    self.overlay,
                    Some(Overlay::Deck)
                        | Some(Overlay::Draw)
                        | Some(Overlay::Discard)
                        | Some(Overlay::Exhaust)
                ) =>
            {
                self.deck_cursor_end(false)
            }
            KeyCode::Char('G')
                if matches!(
                    self.overlay,
                    Some(Overlay::Deck)
                        | Some(Overlay::Draw)
                        | Some(Overlay::Discard)
                        | Some(Overlay::Exhaust)
                ) =>
            {
                self.deck_cursor_end(true)
            }
            KeyCode::Char('g') => self.overlay_scroll = 0,
            KeyCode::Char('G') => self.overlay_scroll = u16::MAX / 2,
            KeyCode::Char('t') if self.overlay == Some(Overlay::Potions) => {
                self.toss_pending = true;
                self.info("press 1-3 to toss that potion");
            }
            KeyCode::Char(c) if self.overlay == Some(Overlay::Potions) => {
                if let Some(slot) = digit_slot(c) {
                    let toss = std::mem::take(&mut self.toss_pending);
                    self.overlay = None;
                    if toss {
                        let r = self.run.toss_potion(slot);
                        self.ok(r);
                    } else {
                        self.drink(slot);
                    }
                }
            }
            _ => {}
        }
    }

    /// 顶栏药水选择模式:h/l(或 j/k)换瓶,enter 喝,数字直接喝,t+数字丢
    fn potion_sel_key(&mut self, key: KeyEvent) {
        let n = self.run.player.potions.len();
        let cur = self.potion_sel.unwrap_or(0).min(n.saturating_sub(1));
        if n == 0 {
            self.potion_sel = None;
            return;
        }
        match key.code {
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char('j') | KeyCode::Down => {
                self.potion_sel = Some((cur + 1) % n);
            }
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('k') | KeyCode::Up => {
                self.potion_sel = Some((cur + n - 1) % n);
            }
            KeyCode::Enter => {
                self.potion_sel = None;
                if self.run.player.potions[cur].is_some() {
                    self.drink(cur);
                } else {
                    self.warn("that slot is empty");
                }
            }
            // esc 或再按一次 p 都退回"不看药水说明"的状态
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('p') => self.potion_sel = None,
            KeyCode::Char('t') => {
                self.toss_pending = true;
                self.info("press 1-3 to toss that potion");
            }
            KeyCode::Char(c) => {
                if let Some(slot) = digit_slot(c) {
                    if slot < n {
                        let toss = std::mem::take(&mut self.toss_pending);
                        self.potion_sel = None;
                        if toss {
                            let r = self.run.toss_potion(slot);
                            self.ok(r);
                        } else {
                            self.drink(slot);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// 牌组窗口的光标移动:只在可选的行之间走
    /// 当前叠加层的卡片行(牌组 / 三个牌堆)
    fn overlay_rows(&self) -> Vec<crate::ui::CardRow> {
        match self.overlay {
            Some(ov @ (Overlay::Draw | Overlay::Discard | Overlay::Exhaust | Overlay::Offered)) => {
                crate::ui::overlay::deck_rows(self, ov)
            }
            _ => crate::ui::overlay::deck_rows(self, Overlay::Deck),
        }
    }

    fn move_deck_cursor(&mut self, delta: i32) {
        let rows = self.overlay_rows();
        let picks: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, crate::ui::CardRow::Card { selectable: true, .. }))
            .map(|(i, _)| i)
            .collect();
        if picks.is_empty() {
            return;
        }
        let pos = picks
            .iter()
            .position(|i| *i == self.overlay_sel)
            .unwrap_or(0);
        let n = picks.len() as i32;
        let np = (pos as i32 + delta).rem_euclid(n) as usize;
        self.overlay_sel = picks[np];
    }

    fn deck_cursor_end(&mut self, last: bool) {
        let rows = self.overlay_rows();
        let picks: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, crate::ui::CardRow::Card { selectable: true, .. }))
            .map(|(i, _)| i)
            .collect();
        self.overlay_sel = if last {
            picks.last().copied().unwrap_or(0)
        } else {
            picks.first().copied().unwrap_or(0)
        };
    }

    fn drink(&mut self, slot: usize) {
        let needs_target = self
            .run
            .player
            .potions
            .get(slot)
            .and_then(|p| p.as_ref())
            .map(|p| p.target.needs_enemy())
            .unwrap_or(false);
        if self.run.screen == Screen::Combat && needs_target {
            let alive = self.total_alive();
            if alive == 0 {
                self.warn("no living target");
                return;
            }
            self.potion_pending = Some(slot);
            self.info("select a target with j/k, enter to confirm, esc to cancel");
            return;
        }
        let r = self.run.quaff_potion(slot, None);
        match r {
            Ok(m) => {
                self.info(m);
                self.clamp();
            }
            Err(e) => self.warn(e),
        }
    }

    fn potion_target_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.potion_pending = None;
                self.info("cancelled");
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_target(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_target(-1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let Some(slot) = self.potion_pending.take() else {
                    return;
                };
                let target = Some(self.target_sel);
                match self.run.quaff_potion(slot, target) {
                    Ok(m) => {
                        self.info(m);
                        self.clamp();
                    }
                    Err(e) => self.warn(e),
                }
            }
            _ => {}
        }
    }

    fn total_alive(&self) -> usize {
        self.run
            .combat()
            .map(|c| c.alive_enemies().len())
            .unwrap_or(0)
    }

    fn move_target(&mut self, delta: i32) {
        let Some(c) = self.run.combat() else {
            return;
        };
        let n = c.enemies.len();
        if n == 0 {
            return;
        }
        let mut idx = self.target_sel % n;
        for _ in 0..n {
            idx = ((idx as i32 + delta).rem_euclid(n as i32)) as usize;
            if c.enemies[idx].alive() {
                self.target_sel = idx;
                return;
            }
        }
    }

    // ---- 地图 ----

    /// 一屏能放几层(和 ui 用同一套算法)
    fn visible_floors(&self) -> usize {
        crate::ui::mapview::visible_floors(self.term_size.0 as usize, self.run.map.total_floors())
    }

    /// 把视野挪到选中节点附近,保证光标一定看得见
    fn follow_selection(&mut self) {
        let reach = self.run.reachable();
        let focus = match reach.get(self.map_sel.min(reach.len().saturating_sub(1))) {
            Some(i) => self.run.map.node(*i).floor,
            None => self.run.floor(),
        };
        let visible = self.visible_floors();
        let max_start = self.run.map.total_floors().saturating_sub(visible);
        self.map_scroll = focus.saturating_sub(visible / 2).min(max_start);
    }

    /// 沿路往后/往前看一步
    fn scroll_map(&mut self, delta: i32) {
        let max_start = self
            .run
            .map
            .total_floors()
            .saturating_sub(self.visible_floors());
        if delta >= 0 {
            self.map_scroll = (self.map_scroll + delta as usize).min(max_start);
        } else {
            self.map_scroll = self.map_scroll.saturating_sub((-delta) as usize);
        }
    }

    /// 视野跳到路的起点或尽头
    fn jump_map(&mut self, to_end: bool) {
        let max_start = self
            .run
            .map
            .total_floors()
            .saturating_sub(self.visible_floors());
        self.map_scroll = if to_end { max_start } else { 0 };
    }

    fn map_key(&mut self, key: KeyEvent) {
        let reach_len = self.run.reachable().len();
        let max_start = self
            .run
            .map
            .total_floors()
            .saturating_sub(self.visible_floors());
        match key.code {
            // 往前后看路
            KeyCode::Char('l') | KeyCode::Right => self.scroll_map(1),
            KeyCode::Char('h') | KeyCode::Left => self.scroll_map(-1),
            // 选岔路
            KeyCode::Char('j') | KeyCode::Down => {
                if reach_len > 0 {
                    self.map_sel = (self.map_sel + 1) % reach_len;
                    self.follow_selection();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if reach_len > 0 {
                    self.map_sel = (self.map_sel + reach_len - 1) % reach_len;
                    self.follow_selection();
                }
            }
            KeyCode::Char('g') => self.map_scroll = 0,
            KeyCode::Char('G') => self.map_scroll = max_start,
            KeyCode::Char('m') => {}
            KeyCode::Enter | KeyCode::Char(' ') => {
                let reach = self.run.reachable();
                let Some(node) = reach.get(self.map_sel.min(reach.len().saturating_sub(1))) else {
                    self.warn("nowhere to go");
                    return;
                };
                let node = *node;
                match self.run.enter_node(node) {
                    Ok(()) => {
                        let kind = self.run.map.node(node).kind;
                        self.map_sel = 0;
                        self.hand_sel = 0;
                        self.rest_index = 0;
                        self.follow_selection();
                        self.info(format!(
                            "floor {}: {}",
                            self.run.map.node(node).floor + 1,
                            kind.name()
                        ));
                        self.clamp();
                    }
                    Err(e) => self.warn(e),
                }
            }
            _ => {}
        }
    }

    // ---- 战斗 ----

    /// 有选牌待定时,战斗里的键先走这里
    fn choice_key(&mut self, key: KeyEvent) -> bool {
        use crate::core::combat::ChoiceSource;
        let Some(ch) = self.run.combat().map(|c| c.choice.as_ref()).flatten() else {
            return false;
        };
        let source = ch.source;
        match key.code {
            KeyCode::Esc => {
                if let Some(c) = self.run.combat_mut() {
                    c.cancel_choice();
                }
                self.choice_sel.clear();
                self.overlay = None;
                self.info("cancelled");
                self.clamp();
            }
            // 手牌:空格切换选中,回车确认
            KeyCode::Char(' ') if source == ChoiceSource::Hand => self.toggle_choice(),
            KeyCode::Enter if source == ChoiceSource::Hand => {
                self.confirm_choice();
            }
            // 弃牌堆/消耗堆的选牌:窗口没开就先开出来(空格也能把它叫出来),
            // 开着的窗口由 overlay_key 用空格/回车选中
            KeyCode::Char(' ') | KeyCode::Enter
                if source != ChoiceSource::Hand && self.overlay.is_none() =>
            {
                self.open_choice_window();
                self.clamp();
            }
            _ => return false,
        }
        true
    }

    /// 选牌模式:切换光标那张牌的选中状态(空格、数字键都走这里)
    fn toggle_choice(&mut self) {
        let cur = self.hand_sel;
        let choosable = self
            .run
            .combat()
            .map(|c| c.choice_candidates().iter().any(|(i, _)| *i == cur))
            .unwrap_or(false);
        if !choosable {
            // 这张不在可选范围内:提示一下并抖一抖
            self.warn("this card cannot be chosen");
            self.shake = Self::SHAKE_FRAMES;
            self.shake_row = None;
            self.clamp();
            return;
        }
        if let Some(pos) = self.choice_sel.iter().position(|i| *i == cur) {
            self.choice_sel.remove(pos);
        } else {
            // 选够张数就不让再选,得先取消一张
            let need = self
                .run
                .combat()
                .and_then(|c| c.choice.as_ref())
                .map(|c| c.need)
                .unwrap_or(1);
            if need > 0 && self.choice_sel.len() >= need {
                self.warn(format!("already picked {need}, unselect one first"));
            } else {
                self.choice_sel.push(cur);
                // 选满就立刻生效,不用再按回车;"不限张数"的得自己按回车
                if need > 0 && self.choice_sel.len() >= need {
                    self.confirm_choice();
                }
            }
        }
    }

    /// 有待选择时,能量行中间那句提示
    pub fn select_hint(&self) -> Option<String> {
        let ch = self.run.combat()?.choice.as_ref()?;
        Some(if ch.need == 0 {
            "select any number, enter when done".to_string()
        } else {
            format!("select up to {} card(s)", ch.need)
        })
    }

    /// 确认选择:把选中的几张交出去(从后往前,免得前面的选择挪动后面的下标)
    fn confirm_choice(&mut self) {
        let mut picks = self.choice_sel.clone();
        // 一张都没选:直接收工(多选类的牌允许少选)
        let any = !picks.is_empty();
        picks.sort_unstable();
        picks.reverse();
        let mut err: Option<String> = None;
        if let Some(c) = self.run.combat_mut() {
            for idx in picks {
                if let Err(e) = c.choose(idx) {
                    err = Some(e);
                    break;
                }
            }
            // 该消耗/该弃掉的牌在这里收尾
            c.finish_choice();
        }
        match err {
            Some(e) => self.warn(e),
            None => {
                self.choice_sel.clear();
                if any {
                    self.info("done");
                }
            }
        }
        self.clamp();
    }

    fn combat_key(&mut self, key: KeyEvent) {
        // 已经赢了:停 2 秒看结算,这期间不接受任何战斗操作
        if self.run.holding_victory() {
            return;
        }
        // 有待选择的牌:先让选择模式处理(esc 取消、空格选、回车确认)
        if self.choice_key(key) {
            return;
        }
        let hand_len = self.run.combat().map(|c| c.hand.len()).unwrap_or(0);
        match key.code {
            // h/l 循环换手牌,j/k 循环换敌人目标
            KeyCode::Char('l') | KeyCode::Right => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + 1) % hand_len;
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + hand_len - 1) % hand_len;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_target(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_target(-1),
            // 空格/回车 = 打出选中的牌
            KeyCode::Char(' ') | KeyCode::Enter => self.play(),
            // e = 结束回合
            KeyCode::Char('e') => {
                if let Some(c) = self.run.combat_mut() {
                    c.end_turn();
                }
                self.run.sync_combat();
                self.info("enemies act...");
                self.clamp();
            }
            // 1-9 0 = 选中第 1..10 张;同一张再按一次(或空格)= 打出
            KeyCode::Char(c) => {
                let Some(slot) = hand_slot(c) else { return };
                if slot >= hand_len {
                    self.warn(format!("no card in slot {}", slot + 1));
                } else if self.run.combat().is_some_and(|x| x.choice.is_some()) {
                    // 正在选牌:数字键把光标挪过去并切换选中
                    self.hand_sel = slot;
                    self.toggle_choice();
                    self.clamp();
                } else if slot == self.hand_sel {
                    self.play();
                } else {
                    self.hand_sel = slot;
                    self.clamp();
                }
            }
            _ => {}
        }
    }

    /// 当前待选择对应的窗口(手牌选择直接用底下的手牌栏,不开窗口)
    fn choice_overlay(&self) -> Option<Overlay> {
        use crate::core::combat::ChoiceSource;
        let ch = self.run.combat().and_then(|c| c.choice.as_ref())?;
        match ch.source {
            ChoiceSource::Discard => Some(Overlay::Discard),
            ChoiceSource::Exhaust => Some(Overlay::Exhaust),
            ChoiceSource::Draw => Some(Overlay::Draw),
            ChoiceSource::Offered => Some(Overlay::Offered),
            ChoiceSource::Hand => None,
        }
    }

    /// 有待选择且来源不是手牌时,自动弹出对应的牌堆窗口
    fn open_choice_window(&mut self) {
        self.overlay = self.choice_overlay();
        if self.overlay.is_some() {
            self.overlay_sel = 0;
            self.choice_sel.clear();
        }
    }

    fn play(&mut self) {
        let target = Some(self.target_sel);
        // 报"打出了哪张"要用打出去的那张本身:牌可能被消耗、或在结算里换了位置
        let played = self
            .run
            .combat()
            .and_then(|c| c.hand.get(self.hand_sel))
            .map(|x| x.label());
        let r = match self.run.combat_mut() {
            Some(c) => c
                .play_card(self.hand_sel, target)
                .map(|_| format!("played {}", played.unwrap_or_else(|| "card".to_string())))
                .map_err(|e| e.to_string()),
            None => return,
        };
        self.ok(r);
        self.run.sync_combat();
        // 开了选牌就不要再挂着"played xxx",让信息行让给选择提示
        if self.run.combat().map(|c| c.choice.is_some()).unwrap_or(false) {
            self.msg.clear();
            self.msg_ttl = 0;
        }
        // 需要从弃牌堆/消耗堆选牌就自动弹窗
        self.open_choice_window();
        self.clamp();
    }

    // ---- 奖励 ----

    fn reward_key(&mut self, key: KeyEvent) {
        let slots = self.run.reward_slots();
        let n = slots.len();
        // 奖励分成几组:金币、卡牌段、遗物、药水;j/k 在组间走,卡牌组内用 h/l 循环
        let card_slots: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, RewardSlot::Card(_)))
            .map(|(i, _)| i)
            .collect();
        let group_of = |i: usize| -> u8 {
            match slots.get(i) {
                Some(RewardSlot::Gold) => 0,
                Some(RewardSlot::Card(_)) => 1,
                Some(RewardSlot::Relic) => 2,
                _ => 3,
            }
        };
        let mut groups: Vec<u8> = Vec::new();
        for i in 0..n {
            let g = group_of(i);
            if groups.last() != Some(&g) {
                groups.push(g);
            }
        }
        let cur = self.run.reward.as_ref().map(|r| r.index).unwrap_or(0).min(n.saturating_sub(1));
        let cur_g = group_of(cur);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(pos) = groups.iter().position(|g| *g == cur_g) {
                    if let Some(&ng) = groups.get(pos + 1) {
                        if let Some(idx) = (0..n).find(|i| group_of(*i) == ng) {
                            self.set_reward_index(idx);
                        }
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(pos) = groups.iter().position(|g| *g == cur_g) {
                    if pos > 0 {
                        let pg = groups[pos - 1];
                        if let Some(idx) = (0..n).rev().find(|i| group_of(*i) == pg) {
                            self.set_reward_index(idx);
                        }
                    }
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if cur_g == 1 && card_slots.len() > 1 {
                    let pos = card_slots.iter().position(|i| *i == cur).unwrap_or(0);
                    self.set_reward_index(card_slots[(pos + card_slots.len() - 1) % card_slots.len()]);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if cur_g == 1 && card_slots.len() > 1 {
                    let pos = card_slots.iter().position(|i| *i == cur).unwrap_or(0);
                    self.set_reward_index(card_slots[(pos + 1) % card_slots.len()]);
                }
            }
            KeyCode::Char('g') => self.set_reward_index(0),
            KeyCode::Char('G') => self.set_reward_index(n.saturating_sub(1)),
            KeyCode::Char('c') => {
                let m = self.run.reward_skip_cards();
                self.info(m);
                self.run.reward_clamp();
            }
            KeyCode::Enter => self.take_reward(),
            KeyCode::Esc | KeyCode::Char('q') => {
                let next = self.run.leave_reward();
                self.info(format!("moving on ({})", next.name()));
                self.clamp();
            }
            KeyCode::Char(c) => {
                if let Some(slot) = hand_slot(c) {
                    if slot < n {
                        self.set_reward_index(slot);
                        self.take_reward();
                    }
                }
            }
            _ => {}
        }
    }

    fn set_reward_index(&mut self, i: usize) {
        if let Some(r) = self.run.reward.as_mut() {
            r.index = i;
        }
    }

    fn take_reward(&mut self) {
        let r = self.run.reward_take();
        self.ok(r);
        self.run.reward_clamp();
        if self.run.reward_slots().is_empty() {
            let next = self.run.leave_reward();
            self.info(format!("reward collected ({})", next.name()));
        }
    }

    // ---- 商店 ----

    fn shop_key(&mut self, key: KeyEvent) {
        let n = self.run.shop.as_ref().map(|s| s.items.len()).unwrap_or(0);
        match key.code {
            // j/k 循环选:到底再按一下回到开头
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    if let Some(s) = self.run.shop.as_mut() {
                        s.index = (s.index + 1) % n;
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    if let Some(s) = self.run.shop.as_mut() {
                        s.index = (s.index + n - 1) % n;
                    }
                }
            }
            KeyCode::Char('g') => {
                if let Some(s) = self.run.shop.as_mut() {
                    s.index = 0;
                }
            }
            KeyCode::Char('G') => {
                if let Some(s) = self.run.shop.as_mut() {
                    s.index = n.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                let r = self.run.buy_selected();
                // 灰行(卖光了 / 钱不够):抖一下提示买不了
                if let Err(e) = &r {
                    if e == "sold out" || e.starts_with("needs ") {
                        self.shake = App::SHAKE_FRAMES;
                        self.shake_row = self.run.shop.as_ref().map(|s| s.index);
                    }
                }
                self.ok(r);
                self.run.shop_clamp();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.run.leave_shop();
                self.info("back to the map");
                self.clamp();
            }
            _ => {}
        }
    }

    // ---- 营火 ----

    fn rest_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Down | KeyCode::Right => {
                self.rest_index = (self.rest_index + 1) % 2;
            }
            KeyCode::Char('k') | KeyCode::Char('h') | KeyCode::Up | KeyCode::Left => {
                self.rest_index = (self.rest_index + 1) % 2;
            }
            KeyCode::Char('1') => self.rest_index = 0,
            KeyCode::Char('2') => self.rest_index = 1,
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.rest_index == 0 {
                    self.run.rest_heal();
                    self.info("you rest by the fire");
                } else {
                    self.run.rest_smith();
                    self.info("choose a card to upgrade");
                }
                self.clamp();
            }
            _ => {}
        }
    }

    // ---- 事件 ----

    fn event_key(&mut self, key: KeyEvent) {
        let (n, done) = match self.run.event.as_ref() {
            Some(st) => (st.def.choices.len(), st.result.is_some()),
            None => return,
        };
        if done {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q')) {
                self.run.leave_event();
                self.info("back to the map");
                self.clamp();
            }
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    let i = self.run.event.as_ref().map(|s| s.index).unwrap_or(0);
                    // 循环选:到底再按 j 回到第一项
                    self.run.event_index_set((i + 1) % n);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    let i = self.run.event.as_ref().map(|s| s.index).unwrap_or(0);
                    self.run.event_index_set((i + n - 1) % n);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let i = self.run.event.as_ref().map(|s| s.index).unwrap_or(0);
                let r = self.run.choose_event(i);
                self.ok_unit(r);
                self.clamp();
            }
            KeyCode::Char(c) => {
                if let Some(slot) = hand_slot(c) {
                    if slot < n {
                        self.run.event_index_set(slot);
                        let r = self.run.choose_event(slot);
                        self.ok_unit(r);
                        self.clamp();
                    }
                }
            }
            _ => {}
        }
    }

    // ---- 宝箱 ----

    fn treasure_key(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('t') | KeyCode::Esc | KeyCode::Char('q')
        ) {
            self.run.take_treasure();
            self.info("back to the map");
            self.clamp();
        }
    }

    // ---- 选牌 ----

    fn pick_key(&mut self, key: KeyEvent) {
        let n = self.run.picker_candidates().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    if let Some(p) = self.run.picker.as_mut() {
                        p.index = (p.index + 1) % n;
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    if let Some(p) = self.run.picker.as_mut() {
                        p.index = (p.index + n - 1) % n;
                    }
                }
            }
            KeyCode::Char('g') => {
                if let Some(p) = self.run.picker.as_mut() {
                    p.index = 0;
                }
            }
            KeyCode::Char('G') => {
                if let Some(p) = self.run.picker.as_mut() {
                    p.index = n.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                let r = self.run.picker_confirm();
                self.ok(r);
                self.clamp();
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.run.picker_cancel();
                self.info("cancelled");
                self.clamp();
            }
            _ => {}
        }
    }

    // ---- 结算 ----

    fn over_key(&mut self, key: KeyEvent) {
        match key.code {
            // 大写 R 重开,小写 r 留给"看遗物"
            KeyCode::Char('R') => {
                let seed = self.run.seed;
                self.restart(seed);
            }
            KeyCode::Char('n') => {
                let seed = self.run.seed.wrapping_add(0x9E37_79B9);
                self.restart(seed);
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    // ---- 命令行 ----

    fn command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.cmd.clear();
                self.comp_pending = false;
            }
            KeyCode::Tab => self.complete_command(),
            KeyCode::Backspace => {
                self.cmd.pop();
                self.comp_pending = false;
            }
            KeyCode::Enter => {
                let cmd = std::mem::take(&mut self.cmd);
                self.mode = Mode::Normal;
                self.exec_command(&cmd);
            }
            KeyCode::Char(c) => {
                // 命令行上限放宽一点,调试命令经常很长
                if self.cmd.len() < 200 {
                    self.cmd.push(c);
                    self.comp_pending = false;
                }
            }
            _ => {}
        }
    }

    /// Tab 补全:唯一候选直接补上;多个候选先补到公共前缀,再按 Tab 就在候选里循环。
    /// 补完是一个完整命令名的话再跟一个空格,接着敲参数。
    fn complete_command(&mut self) {
        // 连续按:光标那段还是上一轮补出来的,就换下一个候选(循环)
        if self.comp_pending && !self.comp_list.is_empty() {
            self.comp_idx = (self.comp_idx + 1) % self.comp_list.len();
            self.set_token(self.comp_list[self.comp_idx].clone());
            return;
        }
        let hints = self.completions();
        if hints.is_empty() {
            self.comp_pending = false;
            return;
        }
        if hints.len() == 1 {
            self.comp_pending = false;
            self.comp_list.clear();
            self.set_token(hints[0].clone());
            // 整个命令已经敲完:跟一个空格好接着打参数
            if self.completions().is_empty() {
                self.cmd.push(' ');
            }
            return;
        }
        // 多个候选:先看公共前缀能不能多给几个字
        let cur = self.current_token().to_string();
        let mut common = hints[0].clone();
        for h in &hints[1..] {
            while !h.starts_with(&common) {
                common.pop();
                if common.is_empty() {
                    break;
                }
            }
        }
        self.comp_list = hints;
        if !common.is_empty() && common.len() > cur.len() {
            // 停在公共前缀上;下一次 Tab 接着往后循环
            self.comp_idx = self
                .comp_list
                .iter()
                .position(|c| *c == common)
                .unwrap_or(self.comp_list.len() - 1);
            self.set_token(common);
        } else {
            self.comp_idx = 0;
            self.set_token(self.comp_list[0].clone());
        }
        self.comp_pending = true;
    }

    /// 命令行里正在敲的那一段(最后一个空格之后的部分)
    fn current_token(&self) -> &str {
        self.cmd.rsplit(' ').next().unwrap_or("")
    }

    /// 把正在敲的那一段换成 `text`
    fn set_token(&mut self, text: String) {
        match self.cmd.rfind(' ') {
            Some(i) => self.cmd = format!("{} {}", &self.cmd[..i], text),
            None => self.cmd = text,
        }
    }

    fn exec_command(&mut self, raw: &str) {
        let cmd = raw.trim();
        let cmd = cmd.strip_suffix('!').unwrap_or(cmd);
        let (head, rest) = match cmd.split_once(' ') {
            Some((h, r)) => (h, r.trim()),
            None => (cmd, ""),
        };
        match head {
            "q" | "quit" => self.quit = true,
            "help" => self.open_overlay(Overlay::Help),
            "seed" => self.info(format!("seed {}", self.run.seed)),
            // :relic add <名字|all> / :relic remove <名字>
            "relic" => {
                let (sub, args) = split_sub(rest);
                let r = match sub {
                    "add" if !args.is_empty() => self.run.debug_add_relics(args),
                    "remove" if !args.is_empty() => self.run.debug_remove_relic(args),
                    _ => Err("usage: relic add <name|all> / relic remove <name>".to_string()),
                };
                self.ok(r);
                self.clamp();
            }
            // :card add <名字> / :card remove [名字|all|hand|hand all] / :card upgrade <名字> [次数]
            "card" => {
                let (sub, args) = split_sub(rest);
                let r = match sub {
                    "add" if !args.is_empty() => self.run.debug_add_cards(args),
                    "remove" => match args.trim().to_lowercase().as_str() {
                        "" => {
                            self.run.debug_open_remove_picker();
                            Ok("pick a card to remove".to_string())
                        }
                        "hand" => self
                            .run
                            .debug_begin_hand_remove()
                            .map(|_| "pick a card from your hand".to_string()),
                        other => self.run.debug_remove_card(other),
                    },
                    // :card pile hand|draw|discard|exhaust <名字, 名字>
                    "pile" => {
                        let (pile, names) = split_sub(args);
                        if names.is_empty() {
                            Err("usage: card pile <hand|draw|discard|exhaust> <name, ...>"
                                .to_string())
                        } else {
                            self.run.debug_pile_cards(pile, names)
                        }
                    }
                    "upgrade" => match args.rsplit_once(' ') {
                        Some((name, n)) if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
                            self.run
                                .debug_upgrade_card(name, n.parse::<usize>().unwrap_or(1))
                        }
                        _ if !args.is_empty() => self.run.debug_upgrade_card(args, 1),
                        _ => Err("usage: card upgrade <name> [times]".to_string()),
                    },
                    _ => Err(
                        "usage: card add <name,...> / card pile <pile> <name,...> / card remove [name|all|hand|hand all] / card upgrade <name> [times]"
                            .to_string(),
                    ),
                };
                self.ok(r);
                self.clamp();
            }
            // :room shop / battle / event(调试用:直接进房间,不动地图)
            "room" => {
                let what = if rest.is_empty() { "battle" } else { rest };
                let r = self.run.debug_room(what);
                self.ok(r);
                self.clamp();
            }
            "win" => {
                if self.run.combat().is_some() {
                    self.run.debug_win_battle();
                    self.clamp();
                } else {
                    self.warn("not in a battle");
                }
            }
            // :restart run|fight|turn(不给就是 turn)
            "restart" => {
                let (sub, _) = split_sub(rest);
                let r: Result<String, String> = match sub {
                    "" | "turn" => match (self.turn_snap.clone(), self.run.combat_mut()) {
                        (Some(snap), Some(c)) => {
                            *c = snap;
                            Ok("restarted the turn".to_string())
                        }
                        _ => Err("no battle to restart".to_string()),
                    },
                    "fight" => match (self.fight_snap.clone(), self.run.combat_mut()) {
                        (Some(snap), Some(c)) => {
                            *c = snap;
                            Ok("restarted the fight".to_string())
                        }
                        _ => Err("no battle to restart".to_string()),
                    },
                    "run" => {
                        let seed = self.run.seed;
                        self.restart(seed);
                        Ok(format!("restarted the run (seed {seed})"))
                    }
                    other => Err(format!("unknown restart target: {other}")),
                };
                self.ok(r);
                self.clamp();
            }
            // :run 用当前种子重来;:run seed [n] 换种子(不给就随机);:run save <名字> 读存档
            "run" => {
                let (sub, args) = split_sub(rest);
                let r: Result<String, String> = match sub {
                    "" => {
                        let seed = self.run.seed;
                        self.restart(seed);
                        Ok(format!("rerun seed {seed}"))
                    }
                    "seed" => {
                        let seed = if args.is_empty() {
                            crate::rng::random_seed()
                        } else {
                            match args.parse::<u64>() {
                                Ok(n) => n,
                                Err(_) => {
                                    self.warn(format!("bad seed: {args}"));
                                    return;
                                }
                            }
                        };
                        self.restart(seed);
                        Ok(format!("new run, seed {seed}"))
                    }
                    "save" => {
                        if args.is_empty() {
                            let names = crate::core::save::list();
                            Err(format!(
                                "usage: run save <name>   available: {}",
                                if names.is_empty() {
                                    "(none)".to_string()
                                } else {
                                    names.join(", ")
                                }
                            ))
                        } else {
                            match crate::core::save::read_named(args) {
                                Some(text) => match Run::from_save(&text) {
                                    Ok(run) => {
                                        self.adopt_run(run);
                                        Ok(format!("loaded save {args}"))
                                    }
                                    Err(e) => Err(e),
                                },
                                None => Err(format!(
                                    "no save named {args} in {}",
                                    crate::core::save::dir().display()
                                )),
                            }
                        }
                    }
                    _ => Err("usage: run | run seed [seed] | run save <name>".to_string()),
                };
                self.ok(r);
                self.clamp();
            }
            // :save [名字] 另存一份(不给名字就用 角色-层数-ISO时间)
            "save" => {
                let name = if rest.trim().is_empty() {
                    let stamp = crate::core::save::now_stamp().replace(':', "-");
                    format!("{}-{}-{}", self.run.character, self.run.floor_reached, stamp)
                } else {
                    rest.trim().to_string()
                };
                let r = crate::core::save::write_named(&name, &self.run.save_text())
                    .map(|p| format!("saved {}", p.display()))
                    .map_err(|e| e.to_string());
                self.ok(r);
            }
            "" => {}
            other => self.warn(format!("unknown command: {other}")),
        }
    }

    // ---- 状态维护 ----

    /// 战斗开始 / 新回合开始时拍快照,供 :restart fight|turn 回退
    fn sync_snapshots(&mut self) {
        let seq = self.run.fight_seq;
        let Some(c) = self.run.combat() else {
            self.fight_snap = None;
            self.turn_snap = None;
            return;
        };
        let (turn, snap) = (c.turn, c.clone());
        if self.fight_snap.is_none() || self.snap_fight_seq != seq {
            self.snap_fight_seq = seq;
            self.fight_snap = Some(snap.clone());
            self.turn_snap = Some(snap);
            self.snap_turn = turn;
        } else if self.snap_turn != turn {
            self.snap_turn = turn;
            self.turn_snap = Some(snap);
        }
    }

    /// 每次操作后把光标限制在合法范围内
    pub fn clamp(&mut self) {
        self.sync_snapshots();
        self.collect_battle_shakes();
        self.collect_havoc_chain();
        if let Some(c) = self.run.combat() {
            if c.hand.is_empty() {
                self.hand_sel = 0;
            } else if self.hand_sel >= c.hand.len() {
                self.hand_sel = c.hand.len() - 1;
            }
            if c.enemies.is_empty() {
                self.target_sel = 0;
            } else if self.target_sel >= c.enemies.len() || !c.enemies[self.target_sel].alive() {
                if let Some(alive) = c.first_alive() {
                    self.target_sel = alive;
                } else {
                    self.target_sel = self.target_sel.min(c.enemies.len() - 1);
                }
            }
        } else {
            self.hand_sel = 0;
            self.target_sel = 0;
        }
        let reach = self.run.reachable().len();
        if reach == 0 {
            self.map_sel = 0;
        } else if self.map_sel >= reach {
            self.map_sel = reach - 1;
        }
    }

    pub fn key_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.overlay.is_some() {
            return vec![("j/k", "scroll"), ("g/G", "top/bottom"), ("esc", "close")];
        }
        if self.potion_pending.is_some() {
            return vec![("j/k", "target"), ("enter", "use"), ("esc", "cancel")];
        }
        match self.run.screen {
            Screen::Title => vec![("j/k", "pick"), ("enter", "confirm"), ("q", "quit")],
            Screen::CharSelect => vec![("j/k", "pick"), ("enter", "start"), ("esc", "back")],
            Screen::Compendium => vec![("j/k", "pick"), ("enter", "open"), ("esc", "back")],
            Screen::Library => vec![
                ("j/k", "pick"),
                ("h/l tab", "change tab"),
                ("1-3", "cards/relics/potions"),
                ("g/G", "top/bottom"),
                ("esc", "back"),
            ],
            Screen::Map => vec![
                ("h/l", "look"),
                ("j/k", "fork"),
                ("enter", "go"),
                ("m tab r", "lists"),
                ("p", "potion"),
                ("H", "history"),
                ("?", "help"),
                (":", "cmd"),
            ],
            Screen::Combat => vec![
                ("1-9 0", "select card, again to play"),
                ("space/enter", "play"),
                ("h l j k", "card / target"),
                ("e", "end turn"),
                ("tab", "deck"),
                ("U D E", "undrawn/discarded/exhausted"),
                ("?", "help"),
                (":", "cmd"),
            ],
            Screen::Reward => vec![
                ("j/k", "gold/cards/relic/potion"),
                ("h/l", "card"),
                ("enter", "take"),
                ("c", "skip cards"),
                ("esc", "leave"),
            ],
            Screen::Shop => vec![("j/k", "pick"), ("enter", "buy"), ("esc", "leave")],
            Screen::Rest => vec![("j/k", "pick"), ("enter", "confirm")],
            Screen::Event => vec![("j/k", "pick"), ("enter", "choose")],
            Screen::Treasure => vec![("enter", "take the relic")],
            Screen::Pick => vec![("j/k", "pick"), ("enter", "confirm"), ("esc", "cancel")],
            Screen::Victory | Screen::Death => vec![
                ("R", "same seed"),
                ("n", "new seed"),
                ("tab/m/p", "cards/map/potions"),
                ("q", "quit"),
            ],
        }
    }

    pub fn help_rows(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("h l", "map: look along the road    combat: pick card    reward: pick card"),
            ("j k", "map: pick a fork    combat: pick target    reward: pick gold/cards/relic/potion"),
            ("h l j k", "same as the left/right and up/down arrow keys"),
            ("space enter", "confirm / play the selected card"),
            ("esc", "cancel / close"),
            ("1-9 0", "combat: select the 1..10th card; press it again (or space) to play"),
            ("space", "combat: select when picking a card"),
            ("esc", "combat: cancel the pending card choice"),
            ("e", "combat: end your turn"),
            ("tab", "the whole deck"),
            ("U D E", "combat: undrawn / discarded / exhausted piles"),
            ("m", "map, look along the road with h/l"),
            ("r", "relics"),
            ("p", "potions: h/l choose, enter drink, 1-3 drink, t then 1-3 toss"),
            ("H", "history: everything that happened in this run"),
            ("g G", "first / last item in a list"),
            ("c", "reward: skip the card choices"),
            ("?", "this help"),
            (":", "command line"),
            (":q", "quit"),
            (":help", "this help"),
            (":seed", "show the run seed"),
            (":room shop|battle|event", "jump straight into that room (debug)"),
            (":relic add|all|remove <name>", "add or drop relics by name (debug)"),
            (
                ":card add|remove|upgrade <name> [n]",
                "change your deck / hand by name (debug)",
            ),
            (":win", "win the current battle (skip to the reward)"),
            (":restart [run|fight|turn]", "roll back to the run/fight/turn start (turn)"),
            (":run", "run the current seed from the beginning"),
            (":run seed [n]", "start a new run (random seed if omitted)"),
            (":run save <name>", "load a save from the save directory"),
            (":save [name]", "save the current run (auto-named if omitted)"),

            ("ctrl-c", "quit at any time"),
        ]
    }
}

/// 手牌/列表里的 1-9 与 0(第十个)
fn hand_slot(c: char) -> Option<usize> {
    match c {
        '1'..='9' => Some(c as usize - '1' as usize),
        '0' => Some(9),
        _ => None,
    }
}

/// 药水槽只有 1-3
fn digit_slot(c: char) -> Option<usize> {
    match c {
        '1'..='3' => Some(c as usize - '1' as usize),
        _ => None,
    }
}

/// 叠加层的全局开关
/// 把敲了一半的命令名补成完整命令名(唯一匹配才认)
fn resolve_command(head: &str) -> Option<String> {
    if COMMANDS.contains(&head) {
        return Some(head.to_string());
    }
    let mut hits: Vec<&str> = COMMANDS
        .iter()
        .copied()
        .filter(|c| c.starts_with(head))
        .collect();
    hits.sort_unstable();
    hits.dedup();
    if hits.len() == 1 {
        Some(hits[0].to_string())
    } else {
        None
    }
}

/// 把 "add Blood for Blood" 拆成 ("add", "Blood for Blood")
fn split_sub(rest: &str) -> (&str, &str) {
    match rest.split_once(' ') {
        Some((a, b)) => (a, b.trim()),
        None => (rest, ""),
    }
}

/// 前缀匹配:字典序、去重、不把完全相同的那条算进去(敲全了就不再提示)
fn matching(pool: &[&str], prefix: &str) -> Vec<String> {
    let mut v: Vec<&str> = pool
        .iter()
        .copied()
        .filter(|c| c.starts_with(prefix) && *c != prefix)
        .collect();
    v.sort_unstable();
    v.dedup();
    v.into_iter().map(|s| s.to_string()).collect()
}

/// 叠加层开关。战斗里 d/e 让给"弃牌堆/消耗堆",所以牌组改用 D,
/// 另外 u 看待抽、d 看弃牌、e 看消耗;平时还是 d 看牌组那一套。
fn overlay_key_of(app: &App, code: KeyCode) -> Option<Overlay> {
    // 整副牌组用 tab 看;U/D/E 是战斗里未抽/弃牌/消耗三个堆
    let in_combat = app.run.combat().is_some() && app.run.screen == Screen::Combat;
    match code {
        KeyCode::Tab => Some(Overlay::Deck),
        KeyCode::Char('U') if in_combat => Some(Overlay::Draw),
        KeyCode::Char('D') if in_combat => Some(Overlay::Discard),
        KeyCode::Char('E') if in_combat => Some(Overlay::Exhaust),
        KeyCode::Char('m') => Some(Overlay::Map),
        KeyCode::Char('r') => Some(Overlay::Relics),
        KeyCode::Char('p') => Some(Overlay::Potions),
        KeyCode::Char('H') => Some(Overlay::History),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    /// 链越深播报停得越久(第 1 层基础,每层 +50%)
    #[test]
    fn deeper_banners_stay_longer() {
        let f = App::banner_frames;
        assert_eq!(f(1), App::BANNER_FRAMES);
        assert_eq!(f(2), App::BANNER_FRAMES + App::BANNER_FRAMES / 4);
        assert_eq!(f(3), App::BANNER_FRAMES + App::BANNER_FRAMES / 2);
        // 极深也不会溢出(u8 饱和)
        assert!(f(255) >= f(10) && f(10) > f(3));
    }

    use super::*;
    use crossterm::event::KeyEvent;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    #[test]
    fn picker_keys_cycle_cards_around() {
        let mut app = App::new(1);
        app.run.rest_smith();
        let n = app.run.picker_candidates().len();
        assert!(n >= 2, "升级候选至少两张才能看出循环");
        assert_eq!(app.run.picker.as_ref().unwrap().index, 0);
        app.handle_key(key('k'));
        assert_eq!(app.run.picker.as_ref().unwrap().index, n - 1);
        app.handle_key(key('j'));
        assert_eq!(app.run.picker.as_ref().unwrap().index, 0);
        for _ in 0..n {
            app.handle_key(key('j'));
        }
        assert_eq!(app.run.picker.as_ref().unwrap().index, 0);
    }

    #[test]
    fn map_keys_look_along_the_road_and_pick_forks() {
        let mut app = App::new(1);
        // 窄终端才看得出"看路":一屏放不下整条路
        app.term_size = (40, 30);
        let visible = app.visible_floors();
        let total = app.run.map.total_floors();
        assert_eq!(visible, 10);
        assert!(total > visible);

        // j/k 选岔路,并带动视野
        let n = app.run.reachable().len();
        assert!(n > 1, "第一层应该有多个起点");
        app.handle_key(key('j'));
        assert_eq!(app.map_sel, 1);
        app.handle_key(key('k'));
        assert_eq!(app.map_sel, 0);
        for _ in 0..n * 2 {
            app.handle_key(key('j'));
        }
        assert!(app.map_sel < n);

        // h/l 看路
        let here = app.map_scroll;
        app.handle_key(key('l'));
        assert_eq!(app.map_scroll, here + 1);
        app.handle_key(key('h'));
        assert_eq!(app.map_scroll, here);
        app.handle_key(key('G'));
        assert_eq!(app.map_scroll, total - visible, "G 应看到路尽头");
        app.handle_key(key('l'));
        assert_eq!(app.map_scroll, total - visible, "l 不该越过路尽头");
        app.handle_key(key('g'));
        assert_eq!(app.map_scroll, 0);
        app.handle_key(key('h'));
        assert_eq!(app.map_scroll, 0, "h 不该越过路起点");
    }

    #[test]
    fn enter_on_map_starts_the_combat() {
        let mut app = App::new(2);
        // 第一层只有普通怪
        app.handle_key(enter());
        assert_eq!(app.run.screen, Screen::Combat);
        assert!(app.run.combat().is_some());
        assert_eq!(app.run.combat().unwrap().hand.len(), 5);
    }

    fn arrow(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// 多个同前缀候选时,Tab 补到公共前缀之后在候选里循环
    #[test]
    fn tab_cycles_through_candidates() {
        let mut app = App::new(1);
        app.mode = Mode::Command;
        app.cmd = "room ".to_string();
        app.handle_key(arrow(KeyCode::Tab));
        assert_eq!(app.cmd, "room battle", "第一下 Tab 落到第一个候选");
        app.handle_key(arrow(KeyCode::Tab));
        assert_eq!(app.cmd, "room boss", "再按 Tab 换下一个");
        app.handle_key(arrow(KeyCode::Tab));
        assert_eq!(app.cmd, "room elite");
        // 打字会打断循环
        app.handle_key(key('x'));
        app.handle_key(arrow(KeyCode::Tab));
        assert!(
            app.cmd == "room battlex" || app.cmd == "room elitex",
            "打错字之后循环重来: {}",
            app.cmd
        );
    }

    /// 数字键先选中、再按一次才打出;方向键和 h/l、j/k 完全等价;tab/U/D/E 看牌堆
    #[test]
    fn digits_pick_then_play_and_arrows_match_hjkl() {
        let mut app = App::new(5);
        app.handle_key(enter());
        let before = app.run.combat().unwrap().hand.len();
        let sel0 = app.hand_sel;
        app.handle_key(key('2'));
        assert_eq!(app.hand_sel, 1, "数字键是选中第 2 张");
        assert_eq!(app.run.combat().unwrap().hand.len(), before, "按一次只是选中");
        app.handle_key(key('2'));
        assert_eq!(
            app.run.combat().unwrap().hand.len(),
            before - 1,
            "同一张再按一次才打出"
        );
        let _ = sel0;

        // 方向键 = h/l(left/right) 和 j/k(up/down)
        let n = app.run.combat().unwrap().hand.len();
        let sel = app.hand_sel;
        app.handle_key(arrow(KeyCode::Right));
        assert_eq!(app.hand_sel, (sel + 1) % n, "Right 和 l 等价");
        app.handle_key(arrow(KeyCode::Left));
        assert_eq!(app.hand_sel, sel, "Left 和 h 等价");
        app.handle_key(key('j'));
        let by_j = app.target_sel;
        app.handle_key(arrow(KeyCode::Down));
        assert_eq!(app.target_sel, by_j, "Down 和 j 落到同一个目标");
        app.handle_key(arrow(KeyCode::Up));
        assert_eq!(app.target_sel, by_j, "Up 和 k 落到同一个目标");

        // tab 看牌组,U/D/E 看未抽/弃牌/消耗
        app.handle_key(arrow(KeyCode::Tab));
        assert_eq!(app.overlay, Some(Overlay::Deck));
        app.handle_key(esc());
        for (c, ov) in [
            ('E', Overlay::Exhaust),
            ('U', Overlay::Draw),
            ('D', Overlay::Discard),
        ] {
            app.handle_key(key(c));
            assert_eq!(app.overlay, Some(ov), "{c} 应打开对应牌堆");
            app.handle_key(esc());
        }
        // e = 结束回合
        let turn = app.run.combat().unwrap().turn;
        app.handle_key(key('e'));
        assert!(app.run.combat().map(|c| c.turn > turn).unwrap_or(true), "e 结束回合");
    }

    #[test]
    fn hand_and_target_move_independently() {
        let mut app = App::new(3);
        app.handle_key(enter());
        let hand_len = app.run.combat().unwrap().hand.len();
        let enemies = app.run.combat().unwrap().enemies.len();
        assert!(hand_len >= 2, "起手应该有至少两张牌");
        app.handle_key(key('l'));
        assert_eq!(app.hand_sel, 1);
        let before = app.target_sel;
        app.handle_key(key('j'));
        if enemies > 1 {
            assert_ne!(app.target_sel, before, "多敌时 j 应换目标");
        } else {
            assert_eq!(app.target_sel, before, "只有一个敌人时目标不变");
        }
        assert_eq!(app.hand_sel, 1, "移动目标不该动手牌光标");
        // 反复移动不会越界
        for _ in 0..enemies + 2 {
            app.handle_key(key('j'));
        }
        assert!(app.target_sel < enemies);
    }

    #[test]
    fn digits_play_cards_and_cost_energy() {
        let mut app = App::new(4);
        app.handle_key(enter());
        let energy_before = app.run.combat().unwrap().energy;
        let hp_before = app.run.combat().unwrap().enemies[0].hp;
        app.handle_key(key('1'));
        let c = app.run.combat().unwrap();
        assert!(c.energy < energy_before);
        assert!(c.enemies[0].hp <= hp_before);
        assert_eq!(c.hand.len(), 4);
    }

    #[test]
    fn end_turn_returns_to_player() {
        let mut app = App::new(5);
        app.handle_key(enter());
        let turn = app.run.combat().unwrap().turn;
        app.handle_key(key('e')); // e = 结束回合
        let c = app.run.combat().unwrap();
        assert_eq!(c.turn, turn + 1);
        assert_eq!(c.phase, crate::core::combat::Phase::PlayerTurn);
    }

    #[test]
    fn overlays_open_in_any_phase_and_toggle_closed() {
        // 地图阶段
        let mut app = App::new(6);
        for (k, ov) in [
            (KeyCode::Tab, Overlay::Deck),
            (KeyCode::Char('m'), Overlay::Map),
            (KeyCode::Char('r'), Overlay::Relics),
        ] {
            app.handle_key(arrow(k));
            assert_eq!(app.overlay, Some(ov), "{k:?} 应该打开 {ov:?}");
            // 再按一次同一个键就关掉
            app.handle_key(arrow(k));
            assert!(app.overlay.is_none(), "{k:?} 再按一次应该关掉");
        }
        // p 不开窗口,而是在顶栏药水区挑第一瓶
        app.handle_key(key('p'));
        assert_eq!(app.potion_sel, Some(0));
        assert!(app.overlay.is_none());
        app.handle_key(esc());
        assert!(app.potion_sel.is_none());
        app.handle_key(arrow(KeyCode::Tab));
        app.handle_key(key('j'));
        // 牌组窗口的 j/k 是挪光标(跳到下一张牌),不是滚动
        assert_eq!(app.overlay_sel, 1);
        app.handle_key(esc());
        assert!(app.overlay.is_none());
        app.handle_key(key('?'));
        assert_eq!(app.overlay, Some(Overlay::Help));
        app.handle_key(key('q'));
        assert!(app.overlay.is_none());
    }

    #[test]
    fn overlays_work_in_every_screen() {
        let mut app = App::new(6);
        for screen in [
            Screen::Map,
            Screen::Combat,
            Screen::Reward,
            Screen::Rest,
            Screen::Shop,
            Screen::Event,
            Screen::Treasure,
            Screen::Pick,
        ] {
            app.run.screen = screen;
            app.overlay = None;
            app.handle_key(arrow(KeyCode::Tab));
            assert_eq!(
                app.overlay,
                Some(Overlay::Deck),
                "{screen:?} 里 tab 应该能看牌组"
            );
            app.handle_key(key('m'));
            assert_eq!(app.overlay, Some(Overlay::Map), "{screen:?} 里 m 应该能看地图");
            app.overlay = None;
        }
    }

    #[test]
    fn map_overlay_scrolls_along_the_road() {
        let mut app = App::new(6);
        app.term_size = (40, 24);
        app.handle_key(key('m'));
        assert_eq!(app.overlay, Some(Overlay::Map));
        let before = app.map_scroll;
        app.handle_key(key('l'));
        assert_eq!(app.map_scroll, before + 1, "地图叠加层里 l 应该往前看");
        app.handle_key(key('h'));
        assert_eq!(app.map_scroll, before);
        // 地图叠加层里 enter 不该把人送进节点
        app.handle_key(enter());
        assert_eq!(app.run.screen, Screen::Map);
        assert!(app.run.pos.is_none(), "叠加层里 enter 不能真的走");
    }

    /// :room 三条命令能直接进对应房间,而且不动地图和位置
    #[test]
    fn room_commands_jump_into_rooms_without_touching_the_map() {
        let mut app = App::new(7);
        let pos = app.run.pos;
        let path = app.run.path.clone();
        for (cmd, want) in [
            ("room shop", Screen::Shop),
            ("room event", Screen::Event),
            ("room battle", Screen::Combat),
        ] {
            app.handle_key(key(':'));
            for c in cmd.chars() {
                app.handle_key(key(c));
            }
            app.handle_key(enter());
            assert_eq!(app.run.screen, want, "{cmd} 没进对房间");
            assert_eq!(app.run.pos, pos, "{cmd} 改了位置");
            assert_eq!(app.run.path, path, "{cmd} 改了路径");
        }
    }

    /// 命令行补全:只给这一层的可能、字典序、敲全了不提示
    /// 战斗抖动:出手自己往右冲、敌人挨打往右退;敌人打过来则相反
    #[test]
    fn battle_shakes_follow_attack_and_damage() {
        use crate::core::combat::ShakeWho;
        let mut app = App::new(7);
        app.handle_key(enter()); // 进第一场战斗
        assert_eq!(app.run.screen, Screen::Combat);
        let idx = app
            .run
            .combat()
            .unwrap()
            .hand
            .iter()
            .position(|c| c.kind() == crate::core::card::CardType::Attack);
        let Some(idx) = idx else { return };
        app.hand_sel = idx;
        app.handle_key(key(' ')); // 空格 = 打出这张攻击牌
        assert!(
            app.battle_shake_offset(ShakeWho::Hero) > 0,
            "出手时角色应该往右冲"
        );
        assert!(
            app.battle_shakes
                .iter()
                .any(|b| matches!(b.who, ShakeWho::Enemy(0)) && b.dir > 0),
            "挨打的敌人应该往右退"
        );
        // 出手方顶在最大幅度上:对面还没抖完就一直顶住
        assert_eq!(
            app.battle_shake_offset(ShakeWho::Hero),
            2,
            "出手应该顶到最大幅度"
        );
        let mut held = 0;
        while app.battle_shake_offset(ShakeWho::Hero) == 2 && held < 40 {
            app.tick();
            held += 1;
        }
        assert!(
            held >= App::SHAKE_FRAMES - App::RELEASE_EARLY,
            "顶住的时间不该这么短({held} 帧)"
        );
        assert!(
            app.battle_shakes
                .iter()
                .any(|b| !b.attacking && matches!(b.who, ShakeWho::Enemy(_))),
            "松手时对面应该还没抖完(要略早于对面结束)"
        );
        // 对面演完后出手方收回来并结束
        for _ in 0..(App::SHAKE_FRAMES + App::ATTACK_RETURN + 2) {
            app.tick();
        }
        assert!(
            !app.battle_shakes
                .iter()
                .any(|b| b.attacking && matches!(b.who, ShakeWho::Hero)),
            "对面演完并收回之后,出手动画该结束了"
        );
        // 轮到敌人出手:敌人先往左冲,玩家晚一帧才往左退
        app.battle_shakes.clear();
        app.handle_key(key('e')); // e = 结束回合
        assert!(
            app.battle_shakes
                .iter()
                .any(|b| matches!(b.who, ShakeWho::Enemy(_)) && b.dir < 0 && b.delay == 0),
            "敌人出手应该先往左冲"
        );
        assert_eq!(
            app.battle_shake_offset(ShakeWho::Hero),
            0,
            "挨打的要晚几帧才动"
        );
        for _ in 0..App::HURT_DELAY {
            app.tick();
        }
        assert!(
            app.battle_shake_offset(ShakeWho::Hero) < 0,
            "延迟走完之后角色应该往左退"
        );

        // 多段攻击(3x7 这种):同一个目标连挨 3 下,要抖 3 轮
        app.battle_shakes.clear();
        {
            let c = app.run.combat_mut().unwrap();
            c.shakes.clear();
            for _ in 0..3 {
                c.shakes.push(crate::core::combat::Shake {
                    who: ShakeWho::Enemy(0),
                    dir: 1,
                    kind: crate::core::combat::ShakeKind::Hurt,
                    amount: 7,
                });
            }
        }
        app.clamp();
        let b = app
            .battle_shakes
            .iter()
            .find(|b| matches!(b.who, ShakeWho::Enemy(0)))
            .expect("应该有敌人的抖动");
        assert_eq!(b.repeats, 2, "3 段攻击 = 先抖 1 轮 + 再抖 2 轮");

        // 伤害越大抖得越狠,帧数也跟着加(保持平滑)
        assert_eq!(App::hurt_amp(4), 1, "不到 5 点是最小一档");
        assert_eq!(App::hurt_amp(5), 2);
        assert_eq!(App::hurt_amp(19), 2);
        assert_eq!(App::hurt_amp(20), 3);
        assert_eq!(App::hurt_amp(50), 4);
        assert!(App::hurt_total(4) > App::hurt_total(2), "幅度大要多给帧数");
        assert_eq!(b.amp, 2, "7 点伤害是第 2 档");
        assert_eq!(b.total, App::hurt_total(2));
    }

    #[test]
    fn command_line_completions_are_one_layer_sorted() {
        let mut app = App::new(7);
        app.handle_key(key(':'));
        for c in "ro".chars() {
            app.handle_key(key(c));
        }
        assert_eq!(app.completions(), vec!["room"], "只提示这一层的可能");
        app.handle_key(key(' '));
        assert_eq!(
            app.completions(),
            vec!["battle", "boss", "elite", "enemy", "event", "shop"],
            "参数按字典序"
        );
        app.handle_key(key('s'));
        assert_eq!(app.completions(), vec!["shop"]);
        app.handle_key(key('h'));
        app.handle_key(key('o'));
        app.handle_key(key('p'));
        assert!(app.completions().is_empty(), "敲全了就不该再提示");
    }

    #[test]
    fn command_mode_quits() {
        let mut app = App::new(7);
        app.handle_key(key(':'));
        assert_eq!(app.mode, Mode::Command);
        app.handle_key(key('q'));
        app.handle_key(enter());
        assert!(app.quit);
    }

    #[test]
    fn command_mode_escape_cancels() {
        let mut app = App::new(8);
        app.handle_key(key(':'));
        app.handle_key(key('x'));
        app.handle_key(esc());
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.cmd.is_empty());
        assert!(!app.quit);
    }

    #[test]
    fn unknown_command_warns() {
        let mut app = App::new(9);
        app.handle_key(key(':'));
        for c in "frobnicate".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter());
        assert!(app.warn);
        assert!(app.msg.contains("unknown command"));
    }

    #[test]
    fn seed_command_reports_seed() {
        let mut app = App::new(4242);
        app.handle_key(key(':'));
        for c in "seed".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter());
        assert!(app.msg.contains("4242"));
    }

    #[test]
    fn new_run_resets_state() {
        let mut app = App::new(10);
        app.handle_key(enter());
        assert_eq!(app.run.screen, Screen::Combat);
        app.handle_key(key(':'));
        for c in "run seed 77".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter());
        assert_eq!(app.run.screen, Screen::Map);
        assert_eq!(app.run.seed, 77);
        // 不给种子就是随机种子
        app.handle_key(key(':'));
        for c in "run seed".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter());
        assert_ne!(app.run.seed, 77, "不给种子应该换一个随机种子");
    }

    #[test]
    fn potion_targeting_flow() {
        let mut app = App::new(11);
        app.handle_key(enter());
        // 直接塞一瓶需要目标的药水
        let def = crate::core::potions::POTIONS
            .iter()
            .find(|p| p.target.needs_enemy());
        let Some(def) = def else { return };
        app.run.player.potions[0] = Some(def);
        app.handle_key(key('p'));
        assert_eq!(app.potion_sel, Some(0));
        app.handle_key(key('1'));
        assert!(app.potion_sel.is_none());
        assert_eq!(app.potion_pending, Some(0));
        app.handle_key(esc());
        assert!(app.potion_pending.is_none());
        assert!(app.run.player.potions[0].is_some(), "取消不该消耗药水");
    }

    #[test]
    fn potion_list_toss_flow() {
        let mut app = App::new(9);
        let def = crate::core::potions::POTIONS.first().unwrap();
        app.run.player.potions[0] = Some(def);
        app.handle_key(key('p'));
        assert_eq!(app.potion_sel, Some(0));
        // t 之后按数字是丢掉
        app.handle_key(key('t'));
        assert!(app.toss_pending);
        app.handle_key(key('1'));
        assert!(app.run.player.potions[0].is_none(), "t + 1 应该丢掉药水");
        assert!(app.potion_sel.is_none());
        // 不带 t 时按数字是喝掉;地图上只能喝能在地图上用的那瓶
        let map_potion = crate::core::potions::POTIONS
            .iter()
            .find(|p| p.out_of_combat)
            .unwrap();
        app.run.player.potions[1] = Some(map_potion);
        app.handle_key(key('p'));
        app.handle_key(key('2'));
        assert!(app.run.player.potions[1].is_none(), "直接按数字应该喝掉药水");
        // 战斗专用药水在地图上按了也喝不掉,应该留在格子里
        app.run.player.potions[2] = Some(def);
        app.handle_key(key('p'));
        app.handle_key(key('3'));
        assert!(app.run.player.potions[2].is_some(), "战斗专用药水不该在地图上被喝掉");
    }

    #[test]
    fn reward_escape_leaves_and_clamps() {
        let mut app = App::new(12);
        app.handle_key(enter());
        {
            let c = app.run.combat_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = crate::core::combat::Phase::Won;
        }
        app.run.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            app.run.tick_win_hold();
        }
        assert_eq!(app.run.screen, Screen::Reward);
        app.handle_key(key('j'));
        app.handle_key(esc());
        assert_eq!(app.run.screen, Screen::Map);
        assert!(app.run.reward.is_none());
    }

    #[test]
    fn reward_keys_hop_groups_and_cycle_cards() {
        let mut app = App::new(12);
        app.handle_key(enter());
        {
            let c = app.run.combat_mut().unwrap();
            for e in c.enemies.iter_mut() {
                e.hp = 0;
            }
            c.phase = crate::core::combat::Phase::Won;
        }
        app.run.sync_combat();
        for _ in 0..=Run::VICTORY_HOLD {
            app.run.tick_win_hold();
        }
        // 造一份"金币 + 三张牌 + 遗物"的奖励
        {
            let r = app.run.reward.as_mut().unwrap();
            r.gold = 10;
            r.gold_taken = false;
            r.cards = vec![
                crate::core::cards::card("strike"),
                crate::core::cards::card("defend"),
                crate::core::cards::card("bash"),
            ];
            r.card_taken = false;
            r.relic = crate::core::relics::relic_def("vajra");
            r.relic_taken = false;
            r.index = 0;
        }
        // 槽位:0 金币,1..3 卡牌,4 遗物
        assert_eq!(app.run.reward_slots().len(), 5);
        let idx = |app: &App| app.run.reward.as_ref().unwrap().index;
        // j 从金币跳到卡牌组的第一张,而不是逐槽走
        app.handle_key(key('j'));
        assert_eq!(idx(&app), 1);
        // h/l 在卡牌组内循环
        app.handle_key(key('l'));
        assert_eq!(idx(&app), 2);
        app.handle_key(key('h'));
        assert_eq!(idx(&app), 1);
        app.handle_key(key('h'));
        assert_eq!(idx(&app), 3, "卡牌组内应循环到最后一张");
        // j 跳到下一组(遗物)
        app.handle_key(key('j'));
        assert_eq!(idx(&app), 4);
        // k 回到卡牌组的最后一张,再 k 回到金币
        app.handle_key(key('k'));
        assert_eq!(idx(&app), 3);
        app.handle_key(key('k'));
        assert_eq!(idx(&app), 0);
    }

    #[test]
    fn quitting_from_end_screen() {
        let mut app = App::new(13);
        app.run.screen = Screen::Death;
        app.handle_key(key('q'));
        assert!(app.quit);
        app.quit = false;
        // 结算界面:大写 R 重开,小写 r 是看遗物
        app.handle_key(key('r'));
        assert_eq!(app.overlay, Some(Overlay::Relics));
        app.handle_key(esc());
        app.handle_key(key('R'));
        assert_eq!(app.run.screen, Screen::Map);
    }
}
