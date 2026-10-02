// 渲染入口与画字符的小工具.
// 所有框线只用 ASCII,颜色承担全部视觉效果(和 neon 保持一致).
pub mod battle;
pub mod hud;
pub mod mapview;
pub mod menu;
pub mod overlay;
pub mod theme;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::Frame;

use crate::app::{App, Mode};

/// 在缓冲里写一行 ASCII 文本,越界自动截断
pub fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    if y >= buf.area.height || x >= buf.area.width {
        return;
    }
    let max = (buf.area.width - x) as usize;
    let s = truncate(text, max);
    buf.set_string(x, y, &s, style);
}

/// 按显示宽度截断(全角字符算两格)
pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

/// 截断后右补空格
pub fn fit(s: &str, width: usize) -> String {
    let t = truncate(s, width);
    let w = display_width(&t);
    if w >= width {
        return t;
    }
    let mut out = t;
    out.push_str(&" ".repeat(width - w));
    out
}

pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// 在缓冲里写一行,右侧补空格到 width(用于选中行的底色铺满)
pub fn put_padded(buf: &mut Buffer, x: u16, y: u16, text: &str, width: usize, style: Style) {
    put(buf, x, y, &fit(text, width), style);
}

/// ASCII 方框
pub fn draw_box(buf: &mut Buffer, area: Rect, title: &str, style: Style, title_style: Style) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let w = area.width as usize;
    let top = format!("+{}+", "-".repeat(w.saturating_sub(2)));
    put(buf, area.x, area.y, &top, style);
    let bottom = format!("+{}+", "-".repeat(w.saturating_sub(2)));
    put(buf, area.x, area.y + area.height - 1, &bottom, style);
    for y in area.y + 1..area.y + area.height - 1 {
        put(buf, area.x, y, "|", style);
        put(buf, area.x + area.width - 1, y, "|", style);
    }
    if !title.is_empty() && w > 6 {
        let t = format!(" {} ", truncate(title, w - 6));
        put(buf, area.x + 2, area.y, &t, title_style);
    }
}

/// 水平分隔线
pub fn hline(buf: &mut Buffer, x: u16, y: u16, width: u16, ch: char, style: Style) {
    if width == 0 {
        return;
    }
    let line: String = std::iter::repeat(ch).take(width as usize).collect();
    put(buf, x, y, &line, style);
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let buf = f.buffer_mut();
    buf.set_style(area, Style::default().bg(theme::BG).fg(theme::FG));
    if area.width < 24 || area.height < 6 {
        put(buf, area.x, area.y, "terminal too small", theme::fg(theme::WARN));
        return;
    }
    let hud_area = Rect::new(area.x, area.y, area.width, 1);
    let status_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    // 顶部隔开一条线,信息更清楚
    hline(buf, body.x, body.y, body.width, '-', theme::fg(theme::BORDER));
    let body = Rect::new(body.x, body.y + 1, body.width, body.height.saturating_sub(1));

    hud::render(buf, hud_area, app);
    match app.run.screen {
        crate::core::run::Screen::Map => mapview::render(buf, body, app),
        crate::core::run::Screen::Combat => battle::render(buf, body, app),
        _ => menu::render(buf, body, app),
    };
    status(buf, status_area, app);
    if let Some(ov) = app.overlay {
        overlay::render(buf, area, app, ov);
    }
}

/// 底栏:左边是模式与消息,右边是当前界面的按键提示
fn status(buf: &mut Buffer, area: Rect, app: &App) {
    let width = area.width as usize;
    let mode = match app.mode {
        Mode::Normal => format!("-- {} --", app.run.screen.name()),
        Mode::Command => ":".to_string(),
    };
    let left = if app.mode == Mode::Command {
        format!(":{}{}", app.cmd, "_")
    } else {
        format!("{mode} {}", app.msg)
    };
    let hints = app.key_hints();
    let hint_text: String = hints
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join("   ");
    let hint_w = display_width(&hint_text);
    let left_w = width.saturating_sub(hint_w + 2);
    put_padded(buf, area.x, area.y, &left, left_w, theme::selected());
    if hint_w + 1 < width {
        put(
            buf,
            area.x + 1 + left_w as u16,
            area.y,
            &hint_text,
            theme::fg(theme::DIM),
        );
    }
}


#[cfg(test)]
mod tests {
    //! 渲染层回归测试:每个界面都要能在几种终端尺寸下画出来,关键内容要真的出现在屏幕上.
    use super::*;
    use crate::app::{App, Mode};
    use crate::ui::overlay::Overlay;
    use crate::core::enemies;
    use crate::core::run::Screen;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn screen_text(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| render(f, app)).expect("draw");
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn app_in_combat(seed: u64, encounter: &'static str) -> App {
        let mut app = App::new(seed);
        let enc = enemies::encounter_def(encounter).expect("遭遇存在");
        app.run.debug_start_combat(enc);
        app.clamp();
        app
    }

    #[test]
    fn map_screen_shows_legend_and_boss() {
        let app = App::new(5);
        let text = screen_text(&app, 110, 40);
        assert!(text.contains("M monster"), "地图缺少图例:\n{text}");
        assert!(text.contains('B'), "缺少 Boss 节点");
        assert!(text.contains("$99"), "顶栏没画出来");
        assert!(text.contains("80/80"), "血量数字没画出来");
    }

    #[test]
    fn combat_screen_shows_every_enemy_and_hand() {
        let app = app_in_combat(7, "three_sentries");
        let text = screen_text(&app, 120, 36);
        for e in app.run.combat().unwrap().enemies.iter() {
            assert!(text.contains(&e.name), "敌人 {} 没画出来:\n{text}", e.name);
        }
        assert!(text.contains("3/3"), "没有能量显示");
        assert!(!text.contains("##"), "不该再出现血条");
        let name = app.run.combat().unwrap().hand[0].label();
        assert!(text.contains(&name), "手牌 {name} 没画出来:\n{text}");
    }

    #[test]
    fn overlays_render_their_content() {
        let mut app = App::new(9);
        app.overlay = Some(Overlay::Deck);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains("cards in deck"), "牌组界面没内容:\n{text}");
        app.overlay = Some(Overlay::Help);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains(":q"), "帮助界面没列出 :q");
        app.overlay = Some(Overlay::Relics);
        let text = screen_text(&app, 110, 36);
        assert!(text.contains("Burning Blood"), "遗物界面没列出起始遗物");
    }

    #[test]
    fn command_line_is_visible_while_typing() {
        let mut app = App::new(3);
        app.mode = Mode::Command;
        app.cmd = "seed".to_string();
        let text = screen_text(&app, 100, 30);
        assert!(text.contains(":seed"), "命令行没显示出来:\n{text}");
    }

    #[test]
    fn every_screen_survives_tiny_and_huge_terminals() {
        let mut app = App::new(11);
        let screens = [
            Screen::Map,
            Screen::Combat,
            Screen::Reward,
            Screen::Rest,
            Screen::Shop,
            Screen::Event,
            Screen::Treasure,
            Screen::Pick,
            Screen::Victory,
            Screen::Death,
        ];
        for size in [(24u16, 6u16), (40, 12), (80, 24), (200, 60)] {
            for s in screens.iter() {
                app.run.screen = *s;
                let _ = screen_text(&app, size.0, size.1);
            }
        }
        // 真数据下的战斗界面也要能画
        let combat = app_in_combat(13, "three_sentries");
        for size in [(24u16, 6u16), (60, 18), (140, 50)] {
            let _ = screen_text(&combat, size.0, size.1);
        }
        // 叠加层在各种尺寸下也要能画
        for ov in [
            Overlay::Deck,
            Overlay::Relics,
            Overlay::Potions,
            Overlay::Help,
            Overlay::Discard,
            Overlay::Exhaust,
        ] {
            app.overlay = Some(ov);
            for size in [(24u16, 6u16), (60, 20), (160, 50)] {
                let _ = screen_text(&app, size.0, size.1);
            }
        }
        app.overlay = None;
        // 极端窄屏只画一行提示,不能 panic
        let _ = screen_text(&app, 10, 4);
        let _ = screen_text(&app, 1, 1);
    }
}
