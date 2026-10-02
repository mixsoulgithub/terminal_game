// spire:终端爬塔(类杀戮尖塔的构筑 roguelike).
// 本文件只负责命令行解析、终端进出与事件循环,游戏逻辑在 core/,界面在 ui/.
mod app;
mod core;
mod rng;
mod ui;

use std::env;
use std::io::{self, IsTerminal};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event};

use app::App;

const USAGE: &str = "\
spire - a slay-the-spire-like deckbuilding roguelike for the terminal

usage: spire [options]

options:
  --seed <n>      fixed seed, so a run can be reproduced
  --dump <what>   print data and exit (cards|enemies|relics|potions|events)
  -h, --help      show this help
  -V, --version   show version

keys (vim style, keyboard only):
  map        h/j/k/l pick a node, enter to go, g/G first/last, : cmd
  combat     1-9/0 or h/l pick a card, j/k pick a target, enter play
             e or space end turn, d/D/X piles, z relics, p potions
  lists      j/k move, g/G top/bottom, enter confirm, esc cancel
  always     ? help, :q quit, ctrl-c quit

  the whole game is playable with h j k l, enter, esc, digits and :commands.
";

struct Args {
    seed: Option<u64>,
    dump: Option<String>,
    help: bool,
    version: bool,
}

fn parse(argv: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        seed: None,
        dump: None,
        help: false,
        version: false,
    };
    let mut it = argv.peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--seed" => {
                let v = it.next().ok_or("--seed needs a number")?;
                args.seed = Some(v.parse::<u64>().map_err(|_| format!("bad seed: {v}"))?);
            }
            "--dump" => {
                let v = it.next().ok_or("--dump needs a value")?;
                args.dump = Some(v);
            }
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn random_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u64) << 32
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
    if !io::stdout().is_terminal() {
        eprintln!("spire: needs a terminal (stdout is not a tty)");
        return ExitCode::from(1);
    }
    let seed = args.seed.unwrap_or_else(random_seed);
    match run(seed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("spire: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(seed: u64) -> io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let mut app = App::new(seed);
    app.clamp();
    let result = (|| -> io::Result<()> {
        loop {
            terminal.draw(|f| ui::render(f, &app))?;
            if app.quit {
                break;
            }
            match event::read()? {
                Event::Key(k) => {
                    app.handle_key(k);
                    app.clamp();
                }
                Event::Resize(..) => {}
                _ => {}
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
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
        "relics" => {
            for r in core::relics::RELICS {
                println!("{:<20} {:<8} {}", r.id, r.rarity.name(), r.desc);
            }
            Ok(())
        }
        "potions" => {
            for p in core::potions::POTIONS {
                println!("{:<20} {:<8} {}", p.id, p.rarity.name(), p.desc);
            }
            Ok(())
        }
        "events" => {
            for e in core::events::EVENTS {
                println!("{:<20} {} choices", e.id, e.choices.len());
            }
            Ok(())
        }
        other => Err(format!("unknown dump target: {other}")),
    }
}
