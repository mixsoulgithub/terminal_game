// 输入状态机:所有键位都在这里,UI 只读状态.
// 操作风格向 vim 靠:地图用 h/l 往前后看路、j/k 选岔路,enter 确认,esc 取消,: 开命令行.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::run::{Run, Screen};
use crate::ui::overlay::Overlay;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Command,
}

pub struct App {
    pub run: Run,
    pub mode: Mode,
    pub cmd: String,
    pub overlay: Option<Overlay>,
    pub overlay_scroll: u16,
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
    /// 等待选定目标的药水槽
    pub potion_pending: Option<usize>,
    pub msg: String,
    pub warn: bool,
    pub quit: bool,
}

impl App {
    pub fn new(seed: u64) -> App {
        App {
            run: Run::new(seed),
            mode: Mode::Normal,
            cmd: String::new(),
            overlay: None,
            overlay_scroll: 0,
            hand_sel: 0,
            target_sel: 0,
            map_sel: 0,
            map_scroll: 0,
            term_size: (100, 30),
            rest_index: 0,
            potion_pending: None,
            msg: "h/l look along the road, j/k pick a fork, enter to go".to_string(),
            warn: false,
            quit: false,
        }
    }

    pub fn restart(&mut self, seed: u64) {
        self.run = Run::new(seed);
        self.mode = Mode::Normal;
        self.cmd.clear();
        self.overlay = None;
        self.overlay_scroll = 0;
        self.hand_sel = 0;
        self.target_sel = 0;
        self.map_sel = 0;
        self.map_scroll = 0;
        self.rest_index = 0;
        self.potion_pending = None;
        self.info(format!("new run, seed {seed}"));
    }

    fn info(&mut self, text: impl Into<String>) {
        self.msg = text.into();
        self.warn = false;
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.msg = text.into();
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
        if self.overlay.is_some() {
            self.overlay_key(key);
            return;
        }
        if self.potion_pending.is_some() {
            self.potion_target_key(key);
            return;
        }
        if key.code == KeyCode::Char(':') {
            self.mode = Mode::Command;
            self.cmd.clear();
            return;
        }
        if key.code == KeyCode::Char('?') {
            self.open_overlay(Overlay::Help);
            return;
        }
        // 全局叠加层开关
        match key.code {
            KeyCode::Char('d') if self.has_piles() => {
                self.open_overlay(Overlay::Deck);
                return;
            }
            KeyCode::Char('D') if self.has_piles() => {
                self.open_overlay(Overlay::Discard);
                return;
            }
            KeyCode::Char('X') if self.has_piles() => {
                self.open_overlay(Overlay::Exhaust);
                return;
            }
            KeyCode::Char('z') if self.has_piles() => {
                self.open_overlay(Overlay::Relics);
                return;
            }
            KeyCode::Char('p') if self.has_piles() => {
                self.open_overlay(Overlay::Potions);
                return;
            }
            _ => {}
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
        }
    }

    fn has_piles(&self) -> bool {
        matches!(self.run.screen, Screen::Map | Screen::Combat | Screen::Pick)
    }

    fn open_overlay(&mut self, ov: Overlay) {
        self.overlay = Some(ov);
        self.overlay_scroll = 0;
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.overlay = None;
                self.potion_pending = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.overlay_scroll = self.overlay_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.overlay_scroll = self.overlay_scroll.saturating_sub(1);
            }
            KeyCode::Char('g') => self.overlay_scroll = 0,
            KeyCode::Char('G') => self.overlay_scroll = u16::MAX / 2,
            KeyCode::Char('t') if self.overlay == Some(Overlay::Potions) => {
                self.msg = "press 1-3 to toss that potion".to_string();
                self.warn = false;
            }
            KeyCode::Char(c) if self.overlay == Some(Overlay::Potions) => {
                if let Some(slot) = digit_slot(c) {
                    self.overlay = None;
                    self.drink(slot);
                }
            }
            _ => {}
        }
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

    fn map_key(&mut self, key: KeyEvent) {
        let reach_len = self.run.reachable().len();
        let total = self.run.map.total_floors();
        let visible = self.visible_floors();
        let max_start = total.saturating_sub(visible);
        match key.code {
            // 往前后看路
            KeyCode::Char('l') | KeyCode::Right => {
                self.map_scroll = (self.map_scroll + 1).min(max_start);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.map_scroll = self.map_scroll.saturating_sub(1);
            }
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

    fn combat_key(&mut self, key: KeyEvent) {
        let hand_len = self.run.combat().map(|c| c.hand.len()).unwrap_or(0);
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + hand_len - 1) % hand_len;
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + 1) % hand_len;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_target(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_target(-1),
            KeyCode::Char('e') | KeyCode::Char(' ') => {
                if let Some(c) = self.run.combat_mut() {
                    c.end_turn();
                }
                self.run.sync_combat();
                self.info("enemies act...");
                self.clamp();
            }
            KeyCode::Enter => self.play(),
            KeyCode::Char(c) => {
                if let Some(slot) = hand_slot(c) {
                    if slot < hand_len {
                        self.hand_sel = slot;
                        self.play();
                    } else {
                        self.warn(format!("no card in slot {}", slot + 1));
                    }
                }
            }
            _ => {}
        }
    }

    fn play(&mut self) {
        let target = Some(self.target_sel);
        let r = match self.run.combat_mut() {
            Some(c) => c
                .play_card(self.hand_sel, target)
                .map(|_| {
                    let label = c
                        .discard
                        .last()
                        .map(|x| x.label())
                        .or_else(|| c.exhaust.last().map(|x| x.label()))
                        .unwrap_or_else(|| "card".to_string());
                    format!("played {label}")
                })
                .map_err(|e| e.to_string()),
            None => return,
        };
        self.ok(r);
        self.run.sync_combat();
        self.clamp();
    }

    // ---- 奖励 ----

    fn reward_key(&mut self, key: KeyEvent) {
        let n = self.run.reward_slots().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    if let Some(r) = self.run.reward.as_mut() {
                        r.index = (r.index + 1).min(n - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(r) = self.run.reward.as_mut() {
                    r.index = r.index.saturating_sub(1);
                }
            }
            KeyCode::Char('g') => {
                if let Some(r) = self.run.reward.as_mut() {
                    r.index = 0;
                }
            }
            KeyCode::Char('G') => {
                if let Some(r) = self.run.reward.as_mut() {
                    r.index = n.saturating_sub(1);
                }
            }
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
                        if let Some(r) = self.run.reward.as_mut() {
                            r.index = slot;
                        }
                        self.take_reward();
                    }
                }
            }
            _ => {}
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
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    if let Some(s) = self.run.shop.as_mut() {
                        s.index = (s.index + 1).min(n - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(s) = self.run.shop.as_mut() {
                    s.index = s.index.saturating_sub(1);
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
                    self.run.event_index_set((i + 1).min(n - 1));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.run.event.as_ref().map(|s| s.index).unwrap_or(0);
                self.run.event_index_set(i.saturating_sub(1));
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
                        p.index = (p.index + 1).min(n - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = self.run.picker.as_mut() {
                    p.index = p.index.saturating_sub(1);
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
            KeyCode::Char('r') => {
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
            }
            KeyCode::Backspace => {
                self.cmd.pop();
            }
            KeyCode::Enter => {
                let cmd = std::mem::take(&mut self.cmd);
                self.mode = Mode::Normal;
                self.exec_command(&cmd);
            }
            KeyCode::Char(c) => {
                if self.cmd.len() < 40 {
                    self.cmd.push(c);
                }
            }
            _ => {}
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
            "q" | "qa" | "quit" | "wq" | "x" => self.quit = true,
            "help" => self.open_overlay(Overlay::Help),
            "deck" | "d" => self.open_overlay(Overlay::Deck),
            "discard" => self.open_overlay(Overlay::Discard),
            "exhaust" => self.open_overlay(Overlay::Exhaust),
            "relics" | "z" => self.open_overlay(Overlay::Relics),
            "potions" | "p" => self.open_overlay(Overlay::Potions),
            "seed" => self.info(format!("seed {}", self.run.seed)),
            "map" => {
                let reach = self.run.reachable();
                self.info(format!(
                    "floor {}/{}  node {:?}  options {}",
                    self.run.floor() + 1,
                    self.run.map.total_floors(),
                    self.run.cur_node(),
                    reach.len()
                ));
            }
            "new" | "restart" => {
                let seed = if rest.is_empty() {
                    self.run.seed.wrapping_add(0x2545_F491)
                } else {
                    rest.parse::<u64>().unwrap_or(self.run.seed)
                };
                self.restart(seed);
            }
            "quaff" => {
                if let Ok(slot) = rest.parse::<usize>() {
                    if slot >= 1 && slot <= self.run.player.potions.len() {
                        self.drink(slot - 1);
                    } else {
                        self.warn("no such potion slot");
                    }
                } else {
                    self.warn("usage: quaff <1-3>");
                }
            }
            "toss" => {
                if let Ok(slot) = rest.parse::<usize>() {
                    if slot >= 1 && slot <= self.run.player.potions.len() {
                        let r = self.run.toss_potion(slot - 1);
                        self.ok(r);
                    } else {
                        self.warn("no such potion slot");
                    }
                } else {
                    self.warn("usage: toss <1-3>");
                }
            }
            "" => {}
            other => self.warn(format!("unknown command: {other}")),
        }
    }

    // ---- 状态维护 ----

    /// 每次操作后把光标限制在合法范围内
    pub fn clamp(&mut self) {
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
            Screen::Map => vec![
                ("h/l", "look along the road"),
                ("j/k", "pick a fork"),
                ("enter", "go"),
                ("z", "relics"),
                ("p", "potions"),
                ("?", "help"),
                (":", "cmd"),
            ],
            Screen::Combat => vec![
                ("1-9/h l", "card"),
                ("j/k", "target"),
                ("enter", "play"),
                ("e", "end turn"),
                ("d/D/X", "piles"),
                ("p", "potions"),
            ],
            Screen::Reward => vec![
                ("j/k", "pick"),
                ("enter", "take"),
                ("c", "skip cards"),
                ("esc", "leave"),
            ],
            Screen::Shop => vec![("j/k", "pick"), ("enter", "buy"), ("esc", "leave")],
            Screen::Rest => vec![("j/k", "pick"), ("enter", "confirm")],
            Screen::Event => vec![("j/k", "pick"), ("enter", "choose")],
            Screen::Treasure => vec![("enter", "take the relic")],
            Screen::Pick => vec![("j/k", "pick"), ("enter", "confirm"), ("esc", "cancel")],
            Screen::Victory | Screen::Death => {
                vec![("r", "same seed"), ("n", "new seed"), ("q", "quit")]
            }
        }
    }

    pub fn help_rows(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("h l", "map: look back / forward along the road"),
            ("j k", "map: pick a fork    combat: card / target"),
            ("enter", "confirm / play the selected card"),
            ("esc", "cancel / close"),
            ("1-9 0", "combat: select and play the nth card"),
            ("e space", "combat: end your turn"),
            ("d D X", "deck / discard / exhaust pile"),
            ("z", "relics"),
            ("p", "potions (then 1-3 to drink, t then 1-3 to toss)"),
            ("g G", "first / last item in a list"),
            ("c", "reward: skip the card choices"),
            ("?", "this help"),
            (":", "command line"),
            (":q", "quit"),
            (":help", "this help"),
            (":deck :relics :potions", "open those lists"),
            (":seed", "show the run seed"),
            (":quaff N :toss N", "use or discard potion N"),
            (":new [seed]", "start a new run"),
            ("r n", "after the run ends: restart with the same / a new seed"),
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

#[cfg(test)]
mod tests {
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
        app.handle_key(key('e'));
        let c = app.run.combat().unwrap();
        assert_eq!(c.turn, turn + 1);
        assert_eq!(c.phase, crate::core::combat::Phase::PlayerTurn);
    }

    #[test]
    fn overlays_open_and_close() {
        let mut app = App::new(6);
        app.handle_key(key('d'));
        assert_eq!(app.overlay, Some(Overlay::Deck));
        app.handle_key(key('j'));
        assert_eq!(app.overlay_scroll, 1);
        app.handle_key(esc());
        assert!(app.overlay.is_none());
        app.handle_key(key('?'));
        assert_eq!(app.overlay, Some(Overlay::Help));
        app.handle_key(key('q'));
        assert!(app.overlay.is_none());
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
        for c in "new 77".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter());
        assert_eq!(app.run.screen, Screen::Map);
        assert_eq!(app.run.seed, 77);
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
        assert_eq!(app.overlay, Some(Overlay::Potions));
        app.handle_key(key('1'));
        assert!(app.overlay.is_none());
        assert_eq!(app.potion_pending, Some(0));
        app.handle_key(esc());
        assert!(app.potion_pending.is_none());
        assert!(app.run.player.potions[0].is_some(), "取消不该消耗药水");
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
        assert_eq!(app.run.screen, Screen::Reward);
        app.handle_key(key('j'));
        app.handle_key(esc());
        assert_eq!(app.run.screen, Screen::Map);
        assert!(app.run.reward.is_none());
    }

    #[test]
    fn quitting_from_end_screen() {
        let mut app = App::new(13);
        app.run.screen = Screen::Death;
        app.handle_key(key('q'));
        assert!(app.quit);
        app.quit = false;
        app.handle_key(key('r'));
        assert_eq!(app.run.screen, Screen::Map);
    }
}
