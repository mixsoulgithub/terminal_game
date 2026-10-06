// 存档:一局的状态写成一个文本文件,开始界面的 continue 读它.
// 只在地图界面(每层开头)存,存的是"这一层刚开始"的状态.
use std::fs;
use std::io;
use std::path::PathBuf;

/// 存档路径:$XDG_DATA_HOME/spire/run.save,没有就 ~/.local/share/spire/run.save,
/// 再没有就放在当前目录
pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("spire").join("run.save")
}

pub fn write(text: &str) -> io::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&p, text)
}

pub fn read() -> Option<String> {
    fs::read_to_string(path()).ok().filter(|s| !s.trim().is_empty())
}

pub fn exists() -> bool {
    read().is_some()
}

pub fn clear() {
    let _ = fs::remove_file(path());
}
