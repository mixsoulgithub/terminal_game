// copy.rs:把颜色码送进系统剪贴板.
// 两条路一起走:一条 OSC 52 逃逸序列(tmux 会顺手把它写进自己的 paste buffer),
// 一条在 $TMUX 下直接调 tmux load-buffer.不是终端就别动剪贴板(比如被重定向或跑测试).

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

// OSC 52:让终端自己把内容放进选择区,经 base64 编码
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

pub fn base64(data: &[u8]) -> String {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(B64[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// 返回用到了哪条路,拿来写状态栏
pub fn copy(text: &str) -> Result<String, String> {
    if !std::io::stdout().is_terminal() {
        return Ok("no tty".into());
    }
    // OSC 52 写 stderr,免得和 ratatui 写在 stdout 的帧交错在一起
    let mut err = std::io::stderr();
    let _ = err.write_all(osc52(text).as_bytes());
    let _ = err.flush();
    let mut used = String::from("osc52");
    if std::env::var_os("TMUX").is_some() {
        match tmux_load(text) {
            Ok(()) => used.push_str("+tmux"),
            Err(e) => return Ok(format!("osc52 (tmux failed: {e})")),
        }
    }
    Ok(used)
}

fn tmux_load(text: &str) -> Result<(), String> {
    let mut child = Command::new("tmux")
        .args(["load-buffer", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(child.stdin.take());
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn osc52_wraps_the_base64_payload() {
        assert_eq!(osc52("#FF8800"), "\x1b]52;c;I0ZGODgwMA==\x07");
        assert_eq!(base64(b"#FF8800"), "I0ZGODgwMA==");
    }
}
