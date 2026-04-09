//! Spawn the build command and capture its output.
//!
//! stdout and stderr are drained concurrently on two threads to avoid the
//! deadlock that happens when you read them sequentially: a subprocess can
//! block writing to one pipe while the other pipe's buffer is full.
//! Each thread sends lines down an mpsc channel; the main thread receives
//! from both and decides whether to print them live (--verbose) or stay silent.

use std::{
    io::{self, BufRead, BufReader, IsTerminal},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

use anyhow::{Context, Result};

pub struct CommandOutput {
    /// Merged stdout + stderr, in arrival order.
    pub raw:     String,
    pub success: bool,
}

#[derive(Clone, Copy)]
enum Pipe { Stdout, Stderr }

/// Run `cmd` as a subprocess. Non-zero exit is not an error — it comes back
/// as `success: false` so the caller can decide what to do.
/// Returns true if a line looks like a build progress indicator rather than
/// compiler output. Matches ninja/cmake progress lines like `[ 42%] Building...`
/// and `-- Installing:` style cmake install lines.
fn is_progress_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('[') && t.contains('%') && t.contains(']')
        || t.starts_with("-- ")
}

pub fn run_command(cmd: &[String], verbose: bool, progress: bool) -> Result<CommandOutput> {
    anyhow::ensure!(!cmd.is_empty(), "no command provided");

    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {:?}", cmd[0]))?;

    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    let (tx, rx) = mpsc::channel::<(Pipe, String)>();
    let tx2 = tx.clone();

    // Each thread owns its pipe and one sender. When the pipe closes (process
    // exited), the thread exits and the sender drops. Once both senders have
    // dropped, rx starts returning Err and the receive loop below ends.
    let t_out = thread::spawn(move || {
        for line in BufReader::new(stdout_pipe).lines().map_while(Result::ok) {
            if verbose || (progress && is_progress_line(&line)) {
                println!("{}", line);
            }
            let _ = tx.send((Pipe::Stdout, line));
        }
    });

    let t_err = thread::spawn(move || {
        for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
            if verbose || (progress && is_progress_line(&line)) {
                eprintln!("{}", line);
            }
            let _ = tx2.send((Pipe::Stderr, line));
        }
    });

    let mut raw = String::new();
    for (_pipe, line) in &rx {
        raw.push_str(&line);
        raw.push('\n');
    }

    // wait() must come after draining rx — otherwise we can deadlock if the
    // subprocess is blocked writing to a full pipe buffer.
    let status = child.wait().context("failed to wait for build command")?;

    t_out.join().ok();
    t_err.join().ok();

    Ok(CommandOutput { raw, success: status.success() })
}

/// Read all of stdin (pipe mode: `cmake --build build 2>&1 | bx`).
pub fn read_stdin() -> Result<String> {
    let stdin = io::stdin();
    anyhow::ensure!(
        !stdin.is_terminal(),
        "no input — pass a build command or pipe output:\n  bx cmake --build build\n  cmake --build build 2>&1 | bx"
    );
    stdin.lock().lines()
        .map(|l| l.map(|s| s + "\n"))
        .collect::<io::Result<_>>()
        .context("failed to read stdin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_line_ninja_percentage() {
        assert!(is_progress_line("[ 42%] Building CXX object foo.cpp.o"));
        assert!(is_progress_line("[100%] Linking CXX executable bx"));
        assert!(is_progress_line("[  3%] Building C object bar.c.o"));
    }

    #[test]
    fn progress_line_cmake_dash() {
        assert!(is_progress_line("-- Configuring done"));
        assert!(is_progress_line("-- Build files have been written to: /build"));
        assert!(is_progress_line("-- Installing: /usr/local/bin/bx"));
    }

    #[test]
    fn progress_line_rejects_errors() {
        assert!(!is_progress_line("src/foo.cpp:10:5: error: bad"));
        assert!(!is_progress_line("FAILED: CMakeFiles/bx.dir/src/main.cpp.o"));
        assert!(!is_progress_line("ninja: build stopped: subcommand failed."));
        assert!(!is_progress_line(""));
    }

    #[test]
    fn run_command_captures_stdout_and_stderr() {
        let cmd: Vec<String> = vec![
            "sh".into(),
            "-c".into(),
            "echo hello && echo world >&2".into(),
        ];
        let out = run_command(&cmd, false, false).unwrap();
        assert!(out.raw.contains("hello"));
        assert!(out.raw.contains("world"));
        assert!(out.success);
    }

    #[test]
    fn run_command_reports_failure() {
        let cmd: Vec<String> = vec!["sh".into(), "-c".into(), "exit 1".into()];
        let out = run_command(&cmd, false, false).unwrap();
        assert!(!out.success);
    }

    #[test]
    fn run_command_unknown_binary_errors() {
        let cmd: Vec<String> = vec!["__bx_nonexistent_binary__".into()];
        assert!(run_command(&cmd, false, false).is_err());
    }
}
