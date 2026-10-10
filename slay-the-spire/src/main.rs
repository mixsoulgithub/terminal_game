// spire:终端爬塔(类杀戮尖塔的构筑 roguelike).
// 本文件只负责命令行解析、终端进出与事件循环,游戏逻辑在 core/,界面在 ui/.
mod app;
mod core;
mod rng;
mod ui;

use std::env;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use crossterm::event::{self, Event};

use app::App;

const USAGE: &str = "\
spire - a slay-the-spire-like deckbuilding roguelike for the terminal

usage: spire [options]

options:
  --seed <n|STR>  fixed seed, so a run can be reproduced
                  a decimal number is used as-is; anything else is read as a
                  base-35 seed string (same form `:seed` prints)
  --ascension <n> ascension level 0-20 for a new game (default 0 = off)
  --dump <what>   print data and exit
                  (cards|enemies|monsters|relics|potions|events|events-json|gated)
  --replay <seed> headless scripted run: walk act 1 to the boss, one JSON
                  line per step on stdout (no terminal needed)
  --script <file> path script for --replay (neow/reward/card/event/rest/shop)
  --sandbox <seed> <scenario.json>
                  headless single-combat sandbox: build the given board, run the
                  given actions, one JSON line per step on stdout (diff ruler)
  --sandbox-batch <seed> <list>
                  run many sandbox scenarios in one process; each line of <list>
                  is `name<TAB>scenario.json`, output is `#name` + its JSONL
  -h, --help      show this help
  -V, --version   show version

keys (vim style, keyboard only):
  map        h/l look back/forward along the road, j/k pick a fork
             enter go, g/G jump to the ends of the road, : cmd
  combat     1-9/0 or h/l select a card, j/k pick a target, enter play
             e or space end turn
  overlays   d cards (in combat: hand/draw/discard/exhaust), m map,
             r relics, p potions - press the same key again or esc to close
  lists      j/k move, g/G top/bottom, enter confirm, esc cancel
  always     ? help, :q quit, ctrl-c quit

  the whole game is playable with h j k l, enter, esc, digits and :commands.
";

struct Args {
    seed: Option<u64>,
    /// 新游戏的飞升等级(0..=20,默认 0)
    ascension: u32,
    dump: Option<String>,
    /// 无头脚本化运行:走完第一章,每步一行 JSON
    replay: Option<u64>,
    /// --replay 用的路径脚本文件
    script: Option<String>,
    /// 沙盒:(种子, scenario.json 路径)
    sandbox: Option<(u64, String)>,
    /// 沙盒批量:(种子, 清单路径)
    sandbox_batch: Option<(u64, String)>,
    help: bool,
    version: bool,
}

fn parse(argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        seed: None,
        ascension: 0,
        dump: None,
        replay: None,
        script: None,
        sandbox: None,
        sandbox_batch: None,
        help: false,
        version: false,
    };
    let mut it = argv.peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--seed" => {
                let v = it.next().ok_or("--seed needs a value")?;
                args.seed = Some(
                    crate::rng::seed_from_arg(&v)
                        .ok_or_else(|| format!("bad seed: {v}"))?,
                );
            }
            "--ascension" => {
                let v = it.next().ok_or("--ascension needs a value")?;
                let n: i64 = v.parse().map_err(|_| format!("bad ascension: {v}"))?;
                args.ascension = crate::core::ascension::clamp(n);
            }
            "--dump" => {
                let v = it.next().ok_or("--dump needs a value")?;
                args.dump = Some(v);
            }
            "--replay" => {
                let v = it.next().ok_or("--replay needs a seed")?;
                args.replay =
                    Some(crate::rng::seed_from_arg(&v).ok_or_else(|| format!("bad seed: {v}"))?);
            }
            "--script" => {
                let v = it.next().ok_or("--script needs a path")?;
                args.script = Some(v);
            }
            "--sandbox-batch" => {
                let s = it.next().ok_or("--sandbox-batch needs a seed")?;
                let seed = crate::rng::seed_from_arg(&s).ok_or_else(|| format!("bad seed: {s}"))?;
                let path = it.next().ok_or("--sandbox-batch needs a list path")?;
                args.sandbox_batch = Some((seed, path));
            }
            "--sandbox" => {
                let s = it.next().ok_or("--sandbox needs a seed")?;
                let seed = crate::rng::seed_from_arg(&s).ok_or_else(|| format!("bad seed: {s}"))?;
                let path = it.next().ok_or("--sandbox needs a scenario path")?;
                args.sandbox = Some((seed, path));
            }
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}


fn main() -> ExitCode {
    let args = match parse(env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("spire: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("spire {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some(what) = args.dump.as_deref() {
        return match dump(what) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("spire: {e}");
                ExitCode::from(2)
            }
        };
    }
    if let Some((seed, path)) = args.sandbox_batch {
        let list = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("spire: cannot read scenario list {path}: {e}");
                return ExitCode::from(2);
            }
        };
        print!("{}", core::replay::sandbox::run_batch(seed, &list));
        return ExitCode::SUCCESS;
    }
    if let Some((seed, path)) = args.sandbox {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("spire: cannot read scenario {path}: {e}");
                return ExitCode::from(2);
            }
        };
        return match sandbox(seed, &text) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("spire: {e}");
                ExitCode::from(1)
            }
        };
    }
    if let Some(seed) = args.replay {
        let script = match args.script.as_deref() {
            None => None,
            Some(path) => match std::fs::read_to_string(path) {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!("spire: cannot read script {path}: {e}");
                    return ExitCode::from(2);
                }
            },
        };
        return match replay(seed, script.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("spire: {e}");
                ExitCode::from(1)
            }
        };
    }
    if !io::stdout().is_terminal() {
        eprintln!("spire: needs a terminal (stdout is not a tty)");
        return ExitCode::from(1);
    }
    // --seed 缺省时:优先用用户设过的"待用种子"(一直留到被覆盖),没有才随机
    let seed = match args.seed {
        Some(s) => s,
        None => match crate::core::save::read_seed() {
            Some(s) => {
                eprintln!(
                    "spire: using saved seed {} ({s})",
                    crate::rng::seed_to_string(s)
                );
                s
            }
            None => crate::rng::random_seed(),
        },
    };
    match run(seed, args.ascension) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("spire: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(seed: u64, ascension: u32) -> io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let mut app = App::start_asc(seed, ascension);
    app.clamp();
    app.maybe_save();
    let result = (|| -> io::Result<()> {
        loop {
            // 地图要用终端宽度算一屏放几层,所以每轮都把尺寸交给 App
            let size = terminal.size()?;
            app.term_size = (size.width, size.height);
            terminal.draw(|f| ui::render(f, &app))?;
            if app.quit {
                break;
            }
            // 抖动动画期间用带超时的轮询,超时就重画一帧
            let has_event = if app.ticking() {
                event::poll(std::time::Duration::from_millis(60))?
            } else {
                true
            };
            if has_event {
                match event::read()? {
                    Event::Key(k) => {
                        app.handle_key(k);
                        app.clamp();
                        app.maybe_save();
                    }
                    Event::Resize(..) => {}
                    _ => {}
                }
            } else {
                app.tick();
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

/// 无头脚本化运行:同 seed + 同路径脚本,每步一行 JSON 到 stdout.
fn replay(seed: u64, script: Option<&str>) -> Result<(), String> {
    let policy = core::replay::policy_from_script(script)?;
    let text = core::replay::run_jsonl(seed, &policy)?;
    print!("{text}");
    Ok(())
}

/// 沙盒:构造给定局面、跑给定动作,逐步一行 JSON(与参考侧同 schema)
fn sandbox(seed: u64, scenario: &str) -> Result<(), String> {
    let text = core::replay::sandbox::run(seed, scenario)?;
    print!("{text}");
    Ok(())
}

/// 打印静态数据,便于查表与自检
fn dump(what: &str) -> Result<(), String> {
    match what {
        "cards" => {
            for c in core::cards::CARDS {
                let cost = match c.cost {
                    core::card::Cost::Fixed(n) => n.to_string(),
                    core::card::Cost::X => "X".to_string(),
                    core::card::Cost::Unplayable => "-".to_string(),
                };
                println!(
                    "{:<20} {:<8} {:<8} cost {:<2} {}",
                    c.id,
                    c.kind.name(),
                    c.rarity.name(),
                    cost,
                    c.text
                );
            }
            Ok(())
        }
        "enemies" => {
            for e in core::enemies::ENEMIES {
                println!(
                    "{:<20} {:<8} hp {}-{}  moves {}",
                    e.id,
                    e.kind.name(),
                    e.hp.0,
                    e.hp.1,
                    e.moves.len()
                );
            }
            Ok(())
        }
        "monsters" => {
            print!("{}", core::enemies::dump_json());
            println!();
            Ok(())
        }
        "relics" => {
            for r in core::relics::RELICS {
                println!("{:<20} {:<8} {:<8} {}", r.id, r.tier.name(), r.rarity().name(), r.desc);
            }
            Ok(())
        }
        "potions" => {
            for p in core::potions::POTIONS {
                println!("{:<20} {:<8} {}", p.id, p.rarity.name(), p.desc);
            }
            Ok(())
        }
        // 刻意不实现的机制(别的职业专属/原作里本作没有的系统):
        // 沙盒扫描器据此把它们排除在"必须一致"之外
        "gated" => {
            for r in core::relics::RELICS {
                if r.fx == core::relics::RelicFx::ZERO && !r.note.is_empty() {
                    println!("relic {} {}", r.id, r.note);
                }
            }
            for p in core::potions::POTIONS {
                let missing = match p.fx {
                    core::potions::PotionFx::Nothing => true,
                    core::potions::PotionFx::AddCardToHand { id, .. } => {
                        core::cards::card_def(id).is_none()
                    }
                    _ => false,
                };
                if missing {
                    println!("potion {} {}", p.id, p.desc);
                }
            }
            Ok(())
        }
        "events" => {
            for e in core::events::EVENTS {
                println!("{:<20} {} choices", e.id, e.choices.len());
            }
            Ok(())
        }
        // 事件选项/条件/效果的完整 JSON(含多屏后半段),给 tools/audit_events_impl.ts
        "events-json" => {
            print!("{}", core::events::dump_json());
            Ok(())
        }
        other => Err(format!("unknown dump target: {other}")),
    }
}
