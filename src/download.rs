//! One curl download helper with percentage progress, shared by the font,
//! Chromium and mermaid.js downloads. Also pins SHA-256 verification for every
//! downloaded artifact.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// `curl`'s meter line percentage, 0..=100.
pub(crate) fn parse_percent(line: &str) -> Option<u8> {
    let pct = line.split_whitespace().next()?;
    let value: u8 = pct.parse().ok()?;
    (value <= 100).then_some(value)
}

/// A SHA-256 digest, lowercase hex. Test-only: the download path hashes files.
#[cfg(test)]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.hex()
}

/// SHA-256 of a file, read in chunks so the 132 MB Chromium zip is not loaded
/// into memory just to verify it.
pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.hex())
}

/// Verify a downloaded temp file against a pinned SHA-256. A mismatch deletes
/// the file and returns a message that names the artifact and both digests.
pub(crate) fn verify_sha256(path: &Path, expected: &str, label: &str) -> Result<(), String> {
    let actual = match sha256_file(path) {
        Ok(hash) => hash,
        Err(e) => {
            let _ = std::fs::remove_file(path);
            return Err(format!("{label}: {e}"));
        }
    };
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err(format!(
        "{label}: sha256 mismatch (expected {expected}, got {actual}); download deleted"
    ))
}

/// Reject a completed download that exceeded its byte cap. The caller's temp
/// file is removed so a later read can never see oversized content.
fn reject_oversized(path: &Path, max_bytes: u64) -> Result<(), String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .len();
    if len <= max_bytes {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err(format!(
        "download is {len} bytes (limit {max_bytes} bytes); deleted"
    ))
}

/// Build the `curl` invocation so the limits are unit-testable without a
/// network request.
fn curl_command(url: &str, dest: &Path, max_time: u64, max_bytes: u64) -> Command {
    let mut cmd = Command::new("curl");
    cmd.arg("-fL")
        .arg("--proto")
        .arg("=https")
        .arg("--proto-redir")
        .arg("=https")
        .arg("--connect-timeout")
        .arg("5")
        .arg("--max-time")
        .arg(max_time.to_string())
        .arg("--max-filesize")
        .arg(max_bytes.to_string())
        // Abort a transfer that stalls instead of waiting for --max-time.
        .arg("--speed-limit")
        .arg("1024")
        .arg("--speed-time")
        .arg("30")
        .arg("--retry")
        .arg("2")
        .arg("--retry-delay")
        .arg("1")
        .arg("--retry-connrefused")
        .arg("-o")
        .arg(dest)
        .arg(url);
    cmd
}

/// Download `url` to `dest` with `curl`, calling `progress` on each new whole
/// percentage. `max_bytes` caps the result; `curl` aborts early when the server
/// declares a larger body, and the temp file is checked again before success.
/// The caller owns cleanup of a failed partial file.
pub(crate) fn curl(
    url: &str,
    dest: &Path,
    max_time: u64,
    max_bytes: u64,
    progress: &mut impl FnMut(u8),
) -> Result<(), String> {
    let mut child = curl_command(url, dest, max_time, max_bytes)
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
        let cap = if status.code() == Some(63) {
            format!(" (exceeds {max_bytes} bytes)")
        } else {
            String::new()
        };
        return Err(format!("curl exited {status}{cap}"));
    }
    reject_oversized(dest, max_bytes)
}

/// Minimal SHA-256 (FIPS 180-4), so pinned digests can be checked without a
/// new dependency. Only the one-shot byte/file helpers above use it.
struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    len: u64,
}

const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            buffer: [0; 64],
            buffered: 0,
            len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
    }

    fn hex(mut self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = self.len.wrapping_mul(8);
        self.buffer[self.buffered] = 0x80;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..].copy_from_slice(&bits.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);
        let mut out = String::with_capacity(64);
        for word in self.state {
            for byte in word.to_be_bytes() {
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0x0f)] as char);
            }
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];
        for (i, k) in SHA256_K.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{curl_command, parse_percent, reject_oversized, sha256_hex, verify_sha256};
    use std::path::Path;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn verify_accepts_the_pinned_hash() {
        let path = std::env::temp_dir().join(format!("jiaoren-hash-ok-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert!(verify_sha256(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "test artifact"
        )
        .is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn verify_rejects_a_mismatch_and_deletes_the_temp_file() {
        let path = std::env::temp_dir().join(format!("jiaoren-hash-bad-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let err = verify_sha256(&path, &"0".repeat(64), "test artifact").unwrap_err();
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert!(!path.exists(), "bad download must be deleted");
    }

    #[test]
    fn curl_command_sets_the_size_and_stall_caps() {
        let cmd = curl_command("https://example.com/a", Path::new("/tmp/a"), 60, 1234);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for pair in [
            ["--max-filesize", "1234"],
            ["--speed-limit", "1024"],
            ["--speed-time", "30"],
            ["--max-time", "60"],
        ] {
            assert!(
                args.windows(2)
                    .any(|w| w[0].as_str() == pair[0] && w[1].as_str() == pair[1]),
                "missing {pair:?} in {args:?}"
            );
        }
    }

    #[test]
    fn oversized_temp_file_is_rejected_and_removed() {
        let path = std::env::temp_dir().join(format!("jiaoren-size-bad-{}", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();
        let err = reject_oversized(&path, 9).unwrap_err();
        assert!(err.contains("limit 9 bytes"), "{err}");
        assert!(!path.exists(), "oversized download must be deleted");
    }

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
