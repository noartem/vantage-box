//! Tailing sing-box's own stdout/stderr log file into the UI.
//!
//! Both run modes redirect sing-box output to `sing-box-data/sing-box.log` and
//! read it exactly once — for the immediate-exit alert. The journal gets its
//! entries from the Clash API, which has no replay: the first seconds of
//! startup (FATAL, ruleset errors before the API is reachable) never reached
//! the UI. This module polls the file and pushes every complete line as a
//! `process://log` event; the dashboard "Process" panel shows the raw feed.
//!
//! Polling, not `notify`: the file is truncated and rewritten on every sing-box
//! start and lines are append-only between resets — there is nothing to
//! debounce, and a poll loop survives all of that with no watcher lifecycle.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::Emitter;

pub const EVENT_CORE_LOG: &str = "process://log";

/// How much of a fresh or rewritten file to ship at once: a trace-level log
/// from a long previous session must not be replayed wholesale.
const TAIL_CAP: usize = 256 * 1024;
/// Poll interval. Appends between polls are picked up wholesale.
const POLL_MS: u64 = 500;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreLogChunk {
    /// `true` — the file was truncated (sing-box restarted) or this is the
    /// first read: the panel drops what it showed and starts over.
    pub reset: bool,
    pub lines: Vec<String>,
}

/// Watches the log file forever (app lifetime) and emits chunks to the UI.
pub fn start(app: tauri::AppHandle) {
    let log_path = match crate::process::log_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("corelog not started: {e}");
            return;
        }
    };
    std::thread::Builder::new()
        .name("corelog".into())
        .spawn(move || {
            // `usize::MAX` — nothing was read yet: the first poll goes through
            // the capped-tail path, same as a truncation.
            let mut offset = usize::MAX;
            loop {
                if let Outcome::Chunk(chunk) = poll_file(&log_path, &mut offset) {
                    let _ = app.emit(EVENT_CORE_LOG, chunk);
                }
                std::thread::sleep(Duration::from_millis(POLL_MS));
            }
        })
        .ok();
}

enum Outcome {
    Quiet,
    Chunk(CoreLogChunk),
}

/// One poll: reads what appeared since `offset` and updates it in place.
///
/// - File missing → one `reset` chunk (the file was cleaned up), then quiet.
/// - File shrank or nothing was read yet → re-read a capped tail, `reset: true`.
/// - Otherwise append everything up to the last complete line, `reset: false`.
fn poll_file(path: &Path, offset: &mut usize) -> Outcome {
    let Ok(mut file) = File::open(path) else {
        // The file disappears between runs. One reset, then quiet until it is
        // back; after it returns, the content counts as a fresh append.
        if *offset != 0 {
            *offset = 0;
            return Outcome::Chunk(CoreLogChunk {
                reset: true,
                lines: vec![],
            });
        }
        return Outcome::Quiet;
    };

    let len = file.metadata().map(|m| m.len() as usize).unwrap_or(0);

    let (start, reset) = if len < *offset {
        // Truncated — sing-box restarted. Re-read a bounded tail.
        (tail_start(&mut file, len), true)
    } else if *offset == 0 {
        // The file returned after disappearing: a fresh append.
        (0, false)
    } else {
        (*offset, false)
    };

    if start >= len {
        *offset = start.min(len);
        return if reset {
            Outcome::Chunk(CoreLogChunk {
                reset: true,
                lines: vec![],
            })
        } else {
            Outcome::Quiet
        };
    }

    file.seek(SeekFrom::Start(start as u64)).ok();
    let mut buf = vec![0u8; len - start];
    if file.read_exact(&mut buf).is_err() {
        return Outcome::Quiet;
    }
    let (lines, consumed) = split_lines(&buf);
    *offset = start + consumed;

    if reset || !lines.is_empty() {
        Outcome::Chunk(CoreLogChunk { reset, lines })
    } else {
        // A partial final line only — hold it for the next poll.
        Outcome::Quiet
    }
}

/// Start offset for a capped read: the last `TAIL_CAP` bytes, advanced past
/// the first newline so the panel never opens mid-line.
fn tail_start(file: &mut File, len: usize) -> usize {
    if len <= TAIL_CAP {
        return 0;
    }
    let cut = len - TAIL_CAP;
    if file.seek(SeekFrom::Start(cut as u64)).is_err() {
        return 0;
    }
    let mut probe = [0u8; 4096];
    let mut pos = cut;
    loop {
        match file.read(&mut probe) {
            Ok(0) => return len,
            Ok(n) => {
                for (i, &b) in probe[..n].iter().enumerate() {
                    if b == b'\n' {
                        return pos + i + 1;
                    }
                }
                pos += n;
            }
            Err(_) => return cut,
        }
    }
}

/// Splits complete lines out of the freshly read bytes; a partial trailing
/// line is left for the next poll. `\r` is trimmed (the file is written on
/// Windows) and terminal color codes are dropped — sing-box colorizes its
/// output even into a redirected file, and ESC sequences are noise for a text
/// panel. Returns the lines and how many bytes they consumed.
fn split_lines(buf: &[u8]) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, &b) in buf.iter().enumerate() {
        if b == b'\n' {
            let mut end = i;
            if end > start && buf[end - 1] == b'\r' {
                end -= 1;
            }
            let text = String::from_utf8_lossy(&buf[start..end]);
            lines.push(strip_ansi(&text));
            start = i + 1;
        }
    }
    (lines, start)
}

/// Removes CSI escape sequences (`ESC [ … letter`), the form sing-box uses for
/// colors. A bare ESC is dropped too — nothing sane follows it in a log line.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{poll_file, split_lines, Outcome};
    use std::path::Path;
    use std::path::PathBuf;

    fn lines_of(outcome: Outcome) -> (bool, Vec<String>) {
        match outcome {
            Outcome::Quiet => panic!("expected a chunk, got quiet"),
            Outcome::Chunk(c) => (c.reset, c.lines),
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    /// A unique scratch file per test, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!(
                "vantage-corelog-{tag}-{}-{}.log",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            Scratch(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn appended_lines_arrive_as_one_chunk() {
        let s = Scratch::new("append");
        write(&s.0, "line one\r\nline two\r\n");
        let mut offset = usize::MAX;
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(reset);
        assert_eq!(lines, vec!["line one", "line two"]);

        append(&s.0, "line three\r\n");
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(!reset);
        assert_eq!(lines, vec!["line three"]);
    }

    #[test]
    fn partial_trailing_line_is_held_back() {
        let s = Scratch::new("partial");
        write(&s.0, "whole line\r\nhalf li");
        let mut offset = usize::MAX;
        let (_, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert_eq!(lines, vec!["whole line"]);

        append(&s.0, "ne\r\nnext\r\n");
        let (_, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert_eq!(lines, vec!["half line", "next"]);
    }

    #[test]
    fn truncation_resets_and_rereads() {
        let s = Scratch::new("trunc");
        write(&s.0, "old run\r\nold run 2\r\n");
        let mut offset = usize::MAX;
        let (_, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert_eq!(lines.len(), 2);

        write(&s.0, "new run\r\n");
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(reset);
        assert_eq!(lines, vec!["new run"]);
    }

    #[test]
    fn missing_file_resets_once_then_quiet() {
        let s = Scratch::new("missing");
        let mut offset = usize::MAX;
        // Never created: the first poll clears the (empty) panel once, then
        // goes quiet — no repeated resets for a file that is not there.
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(reset);
        assert!(lines.is_empty());
        assert!(matches!(poll_file(&s.0, &mut offset), Outcome::Quiet));

        // After something was shown, a vanished file resets exactly once.
        write(&s.0, "gone soon\r\n");
        let _ = poll_file(&s.0, &mut offset);
        std::fs::remove_file(&s.0).unwrap();
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(reset);
        assert!(lines.is_empty());
        assert!(matches!(poll_file(&s.0, &mut offset), Outcome::Quiet));
    }

    #[test]
    fn initial_read_is_capped_and_line_aligned() {
        let s = Scratch::new("cap");
        let long_line = "x".repeat(120) + "\r\n";
        let mut text = String::new();
        for i in 0..3000 {
            text += &format!("line {i:05} {long_line}");
        }
        write(&s.0, &text);
        let mut offset = usize::MAX;
        let (reset, lines) = lines_of(poll_file(&s.0, &mut offset));
        assert!(reset);
        // 256 KiB cap: only the tail of the file, whole lines only.
        assert!(lines.len() < 3000);
        // The cut lands at the end of line 1029, so that whole line is first.
        assert!(lines[0].starts_with("line 01029 "));
        assert_eq!(offset, text.len());
    }

    #[test]
    fn split_lines_reports_consumed_bytes() {
        let (lines, consumed) = split_lines(b"a\r\nb\r\nc");
        assert_eq!(lines, vec!["a", "b"]);
        // "a\r\nb\r\n" — six bytes; "c" waits for its newline.
        assert_eq!(consumed, 6);
        let (lines, consumed) = split_lines(b"no newline yet");
        assert!(lines.is_empty());
        assert_eq!(consumed, 0);
    }

    #[test]
    fn color_codes_are_stripped_from_lines() {
        let (lines, _) = split_lines(b"\x1b[36mINFO\x1b[0m[10] started\r\n");
        assert_eq!(lines, vec!["INFO[10] started"]);

        let (lines, _) = split_lines(b"\x1b[31mFATAL\x1b[0m decode config: bad\r\n");
        assert_eq!(lines, vec!["FATAL decode config: bad"]);
    }
}
