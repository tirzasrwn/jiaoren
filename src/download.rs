//! One curl download helper with percentage progress, shared by the font,
//! Chromium and mermaid.js downloads.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// `curl`'s meter line percentage, 0..=100.
pub(crate) fn parse_percent(line: &str) -> Option<u8> {
    let pct = line.split_whitespace().next()?;
    let value: u8 = pct.parse().ok()?;
    (value <= 100).then_some(value)
}

/// Download `url` to `dest` with `curl`, calling `progress` on each new whole
/// percentage. The caller owns cleanup of a failed partial file.
pub(crate) fn curl(
    url: &str,
    dest: &Path,
    max_time: u64,
    progress: &mut dyn FnMut(u8),
) -> Result<(), String> {
    let mut child = Command::new("curl")
        .arg("-fL")
        .arg("--proto")
        .arg("=https")
        .arg("--proto-redir")
        .arg("=https")
        .arg("--connect-timeout")
        .arg("5")
        .arg("--max-time")
        .arg(max_time.to_string())
        .arg("--retry")
        .arg("2")
        .arg("--retry-delay")
        .arg("1")
        .arg("--retry-connrefused")
        .arg("-o")
        .arg(dest)
        .arg(url)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn curl: {e}"))?;
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = child.kill();
            return Err("curl stderr unavailable".to_string());
        }
    };
    let mut chunk = [0u8; 4096];
    let mut pending = String::new();
    let mut last = 0u8;
    loop {
        let n = match stderr.read(&mut chunk) {
            Ok(n) => n,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("read curl progress: {e}"));
            }
        };
        if n == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&chunk[..n]));
        while let Some(at) = pending.find(['\r', '\n']) {
            let line = pending[..at].to_string();
            pending.drain(..=at);
            if let Some(pct) = parse_percent(&line) {
                if pct != last {
                    last = pct;
                    progress(pct);
                }
            }
        }
    }
    if let Some(pct) = parse_percent(&pending) {
        if pct != last {
            progress(pct);
        }
    }
    let status = child.wait().map_err(|e| format!("wait curl: {e}"))?;
    if !status.success() {
        return Err(format!("curl exited {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn parses_curl_progress_lines() {
        assert_eq!(parse_percent(" 12  249k   12  249k"), Some(12));
        assert_eq!(parse_percent("100  2.6M  100  2.6M    0     0"), Some(100));
        assert_eq!(parse_percent("  0     0    0     0"), Some(0));
        assert_eq!(parse_percent("  % Total    % Received"), None);
        assert_eq!(parse_percent("101  x"), None);
        assert_eq!(parse_percent(""), None);
    }
}
