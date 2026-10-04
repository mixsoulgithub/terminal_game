// 输入状态机:所有键位都在这里,UI 只读状态.
// 操作风格向 vim 靠:地图用 h/l 往前后看路、j/k 选岔路,enter 确认,esc 取消,: 开命令行.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::core::run::{RewardSlot, Run, Screen};
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
    /// 等待选定目标的药水槽
    pub potion_pending: Option<usize>,
    /// 药水列表里按过 t,下一个数字是"丢掉"而不是"喝掉"
    pub toss_pending: bool,
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
            overlay_sel: 0,
            hand_sel: 0,
            target_sel: 0,
            map_sel: 0,
            map_scroll: 0,
            term_size: (100, 30),
            rest_index: 0,
            potion_pending: None,
            toss_pending: false,
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
        self.overlay_sel = 0;
        self.hand_sel = 0;
        self.target_sel = 0;
        self.map_sel = 0;
        self.map_scroll = 0;
        self.rest_index = 0;
        self.potion_pending = None;
        self.toss_pending = false;
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
        // 叠加层:再按同一个键就关掉,按另一个叠加层键就直接切过去
        if let Some(ov) = self.overlay {
            if let Some(other) = overlay_key_of(key.code) {
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
        // 全局叠加层开关:任何阶段(含结算界面)都能看牌组/地图/遗物/药水
        if let Some(ov) = overlay_key_of(key.code) {
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
        }
    }

    fn open_overlay(&mut self, ov: Overlay) {
        self.overlay = Some(ov);
        self.overlay_sel = 0;
        // 历史记录先看最新的一条,其他列表从头看
        self.overlay_scroll = if ov == Overlay::History { u16::MAX / 2 } else { 0 };
        if ov == Overlay::Deck {
            // 牌组窗口的光标要停在第一张牌上,别停在分组标题
            self.deck_cursor_end(false);
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) {
        let on_map = self.overlay == Some(Overlay::Map);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                self.overlay = None;
                self.potion_pending = None;
                self.toss_pending = false;
            }
            KeyCode::Char('l') | KeyCode::Right if on_map => self.scroll_map(1),
            KeyCode::Char('h') | KeyCode::Left if on_map => self.scroll_map(-1),
            KeyCode::Char('g') if on_map => self.jump_map(false),
            KeyCode::Char('G') if on_map => self.jump_map(true),
            KeyCode::Char('j') | KeyCode::Down => {
                if self.overlay == Some(Overlay::Deck) {
                    self.move_deck_cursor(1);
                } else {
                    self.overlay_scroll = self.overlay_scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.overlay == Some(Overlay::Deck) {
                    self.move_deck_cursor(-1);
                } else {
                    self.overlay_scroll = self.overlay_scroll.saturating_sub(1);
                }
            }
            KeyCode::Char('g') if self.overlay == Some(Overlay::Deck) => self.deck_cursor_end(false),
            KeyCode::Char('G') if self.overlay == Some(Overlay::Deck) => self.deck_cursor_end(true),
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

    /// 牌组窗口的光标移动:只在可选的行之间走
    fn move_deck_cursor(&mut self, delta: i32) {
        let rows = crate::ui::overlay::deck_rows(self);
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
        let np = (pos as i32 + delta).clamp(0, picks.len() as i32 - 1) as usize;
        self.overlay_sel = picks[np];
    }

    fn deck_cursor_end(&mut self, last: bool) {
        let rows = crate::ui::overlay::deck_rows(self);
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

    fn combat_key(&mut self, key: KeyEvent) {
        let hand_len = self.run.combat().map(|c| c.hand.len()).unwrap_or(0);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + 1) % hand_len;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if hand_len > 0 {
                    self.hand_sel = (self.hand_sel + hand_len - 1) % hand_len;
                }
            }
            KeyCode::Char('h') | KeyCode::Left => self.move_target(-1),
            KeyCode::Char('l') | KeyCode::Right => self.move_target(1),
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
            "deck" | "d" | "cards" => self.open_overlay(Overlay::Deck),
            "map" | "m" => self.open_overlay(Overlay::Map),
            "relics" | "r" => self.open_overlay(Overlay::Relics),
            "potions" | "p" => self.open_overlay(Overlay::Potions),
            "history" | "log" | "H" => self.open_overlay(Overlay::History),
            "seed" => self.info(format!("seed {}", self.run.seed)),
            "win" => {
                if self.run.combat().is_some() {
                    self.run.debug_win_battle();
                    self.clamp();
                } else {
                    self.warn("not in a battle");
                }
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
                ("h/l", "look"),
                ("j/k", "fork"),
                ("enter", "go"),
                ("m d r p", "lists"),
                ("H", "history"),
                ("?", "help"),
                (":", "cmd"),
            ],
            Screen::Combat => vec![("?", "help"), (":", "cmd")],
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
                ("d/m/p", "cards/map/potions"),
                ("q", "quit"),
            ],
        }
    }

    pub fn help_rows(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("h l", "map: look back / forward    combat: pick target    reward: pick card"),
            ("j k", "map: pick a fork    combat: pick card    reward: pick gold/cards/relic/potion"),
            ("enter", "confirm / play the selected card"),
            ("esc", "cancel / close"),
            ("1-9 0", "combat: select and play the nth card"),
            ("e space", "combat: end your turn"),
            ("d", "cards: deck; in combat all four piles"),
            ("m", "map, look along the road with h/l"),
            ("r", "relics"),
            ("p", "potions (then 1-3 to drink, t then 1-3 to toss)"),
            ("H", "history: everything that happened in this run"),
            ("g G", "first / last item in a list"),
            ("c", "reward: skip the card choices"),
            ("?", "this help"),
            (":", "command line"),
            (":q", "quit"),
            (":help", "this help"),
            (":deck :relics :potions", "open those lists"),
            (":seed", "show the run seed"),
            (":win", "win the current battle (skip to the reward)"),
            (":quaff N :toss N", "use or discard potion N"),
            (":new [seed]", "start a new run"),
            ("R n", "after the run ends: restart with the same / a new seed"),
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
fn overlay_key_of(code: KeyCode) -> Option<Overlay> {
    match code {
        KeyCode::Char('d') => Some(Overlay::Deck),
        KeyCode::Char('m') => Some(Overlay::Map),
        KeyCode::Char('r') => Some(Overlay::Relics),
        KeyCode::Char('p') => Some(Overlay::Potions),
        KeyCode::Char('H') => Some(Overlay::History),
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
        app.handle_key(key('j'));
        assert_eq!(app.hand_sel, 1);
        let before = app.target_sel;
        app.handle_key(key('l'));
        if enemies > 1 {
            assert_ne!(app.target_sel, before, "多敌时 l 应换目标");
        } else {
            assert_eq!(app.target_sel, before, "只有一个敌人时目标不变");
        }
        assert_eq!(app.hand_sel, 1, "移动目标不该动手牌光标");
        // 反复移动不会越界
        for _ in 0..enemies + 2 {
            app.handle_key(key('l'));
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
    fn overlays_open_in_any_phase_and_toggle_closed() {
        // 地图阶段
        let mut app = App::new(6);
        for (k, ov) in [
            ('d', Overlay::Deck),
            ('m', Overlay::Map),
            ('r', Overlay::Relics),
            ('p', Overlay::Potions),
        ] {
            app.handle_key(key(k));
            assert_eq!(app.overlay, Some(ov), "{k} 应该打开 {ov:?}");
            // 再按一次同一个键就关掉
            app.handle_key(key(k));
            assert!(app.overlay.is_none(), "{k} 再按一次应该关掉");
        }
        app.handle_key(key('d'));
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
            app.handle_key(key('d'));
            assert_eq!(app.overlay, Some(Overlay::Deck), "{screen:?} 里 d 应该能看牌组");
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
    fn potion_list_toss_flow() {
        let mut app = App::new(9);
        let def = crate::core::potions::POTIONS.first().unwrap();
        app.run.player.potions[0] = Some(def);
        app.handle_key(key('p'));
        assert_eq!(app.overlay, Some(Overlay::Potions));
        // t 之后按数字是丢掉
        app.handle_key(key('t'));
        assert!(app.toss_pending);
        app.handle_key(key('1'));
        assert!(app.run.player.potions[0].is_none(), "t + 1 应该丢掉药水");
        assert!(app.overlay.is_none());
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
