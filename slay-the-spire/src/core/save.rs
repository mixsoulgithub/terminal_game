// 存档:一局的状态写成一个文本文件。
// 目录是 ~/.local/share/slay-the-spire(没有 XDG 就退回 ~/.local/share),
// continue 读其中的 run.save;:save 可以另存成具名文件,:run save 再读回来。
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 自动存档(continue 用的那个)的文件名
pub const AUTO: &str = "run.save";

fn base_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 存档目录:~/.local/share/slay-the-spire
pub fn dir() -> PathBuf {
    base_dir().join("slay-the-spire")
}

/// 自动存档路径
pub fn path() -> PathBuf {
    dir().join(AUTO)
}

/// 具名存档路径:名字没带 .save 就补上
pub fn named_path(name: &str) -> PathBuf {
    let name = name.trim();
    let file = if name.ends_with(".save") {
        name.to_string()
    } else {
        format!("{name}.save")
    };
    dir().join(file)
}

pub fn write_at(path: &PathBuf, text: &str) -> io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(path, text)
}

pub fn read_at(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).ok().filter(|s| !s.trim().is_empty())
}

/// "A Note For Yourself" 存卡用的文件名(跟 run.save 分开,跨局留存)
pub const NOTE_FILE: &str = "note.card";

/// 便条存卡文件:最近一局交给下一局的那张牌
pub fn note_path() -> PathBuf {
    dir().join(NOTE_FILE)
}

/// 读便条存卡:(卡 id, 升级次数);文件不在或坏了就返回 None
pub fn read_note_at(path: &PathBuf) -> Option<(String, u8)> {
    let text = read_at(path)?;
    let mut lines = text.lines();
    let id = lines.next()?.trim();
    if id.is_empty() {
        return None;
    }
    let plus = lines
        .next()
        .and_then(|l| l.trim().parse::<u8>().ok())
        .unwrap_or(0);
    Some((id.to_string(), plus))
}

/// 写便条存卡:第一行卡 id,第二行升级次数
pub fn write_note_at(path: &PathBuf, id: &str, plus: u8) -> io::Result<()> {
    write_at(path, &format!("{id}\n{plus}\n"))
}

/// 自动存档:存在 / 清掉 / 读
pub fn exists() -> bool {
    read_at(&path()).is_some()
}

pub fn read() -> Option<String> {
    read_at(&path())
}

pub fn write(text: &str) -> io::Result<()> {
    write_at(&path(), text)
}

pub fn clear() {
    let _ = fs::remove_file(path());
}

pub fn read_named(name: &str) -> Option<String> {
    read_at(&named_path(name))
}

pub fn write_named(name: &str, text: &str) -> io::Result<PathBuf> {
    let p = named_path(name);
    write_at(&p, text)?;
    Ok(p)
}

/// 目录里已有的存档名(不含 .save),供提示用
pub fn list() -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".save").map(|s| s.to_string())
        })
        .collect();
    out.sort();
    out
}

/// ISO 时间戳(UTC):2026-10-07T12:34:56(不引第三方库,自己算日历)
pub fn now_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 天数(1970-01-01 起)转年月日
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
