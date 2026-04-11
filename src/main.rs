//! bx — build error extractor
//!
//! Usage:
//!   bx cmake --build build
//!   bx --save cmake --build --preset debug   (saves command for this project)
//!   bx                                        (runs saved command)
//!   bx --tui
//!   cmake --build build 2>&1 | bx

mod classify;
mod palette;
mod render;
mod subprocess;

use std::path::PathBuf;

use anyhow::{Context, Result};

use classify::{build_patterns, collect_blocks, Config};
use render::{render_plain, render_tui};
use subprocess::{read_stdin, run_command};

// Saved command, stored in .git/bx or .bx-command in the project root

/// WIP, but for now: 
/// Walk up from the current directory to find the git root.
///
/// If `local` is false (default): only stops at a real `.git` directory,
/// which means submodule `.git` files are skipped and we land at the parent
/// repo root. This is the right default for most workflows.
///
/// If `local` is true (--local flag): stops at the first `.git` entry
/// regardless of whether it is a file or directory, so submodule roots are
/// treated as independent roots.
fn find_git_root(local: bool) -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let git = dir.join(".git");
        if local && git.exists() {
            return Some(dir);
        } 
        if !dir.pop() {
            return None;
        }
    }
}

/// Find the saved command file. Walks up to the git root and stores it in
/// .git/bx, never committed. Falls back to .bx-command in cwd if not in
/// a git repo.
fn saved_command_path(local: bool) -> PathBuf {
    if let Some(root) = find_git_root(local) {
        root.join(".git").join("bx")
    } else {
        PathBuf::from(".bx-command")
    }
}

struct SavedCommand {
    cmd: Vec<String>,
    /// The working directory to run the command from.
    dir: PathBuf,
}

fn save_command(cmd: &[String], local: bool) -> Result<()> {
    let path = saved_command_path(local);
    let cwd  = std::env::current_dir().context("failed to get current directory")?;

    // File format: first line is the working directory, remaining lines are
    // the command arguments one per line.
    let mut content = cwd.to_string_lossy().to_string();
    content.push('\n');
    content.push_str(&cmd.join("\n"));

    std::fs::write(&path, content)
        .with_context(|| format!("failed to save command to {:?}", path))?;

    println!("bx: saved");
    println!("    dir: {}", cwd.display());
    println!("    cmd: {}", cmd.join(" "));
    Ok(())
}

fn load_command(local: bool) -> Result<Option<SavedCommand>> {
    let path = saved_command_path(local);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read saved command from {:?}", path))?;
    let mut lines = raw.lines().peekable();

    // Handle old format gracefully
    let first = match lines.next() {
        Some(l) if !l.is_empty() => l,
        _ => return Ok(None),
    };

    let (dir, cmd_first) = if first.starts_with('/') || first.starts_with('~') {
        // New format: first line is the directory
        (PathBuf::from(first), None)
    } else {
        // Old format: first line is the first command argument, use cwd
        eprintln!("bx: save file is in old format — re-run `bx --save <command>` to update it");
        (std::env::current_dir().unwrap_or_default(), Some(first.to_string()))
    };

    let mut cmd: Vec<String> = lines.map(|s| s.to_string()).collect();
    if let Some(first_arg) = cmd_first {
        cmd.insert(0, first_arg);
    }
    if cmd.is_empty() {
        return Ok(None);
    }
    Ok(Some(SavedCommand { cmd, dir }))
}

// Args

struct Args {
    tui:      bool,
    warnings: bool,
    verbose:  bool,
    progress: bool,
    save:     bool,
    local:    bool,
    context:  usize,
    cmd:      Vec<String>,
}

impl Args {
    fn parse() -> Result<Self> {
        let raw: Vec<String> = std::env::args().skip(1).collect();
        let mut args = Args {
            tui:      false,
            warnings: false,
            verbose:  false,
            progress: false,
            save:     false,
            local:    false,
            context:  0,
            cmd:      Vec::new(),
        };

        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--tui"             => args.tui      = true,
                "--warnings"        => args.warnings = true,
                "--verbose" | "-v"  => args.verbose  = true,
                "--progress" | "-p" => args.progress = true,
                "--save"            => args.save     = true,
                "--local"           => args.local    = true,
                "--context" => {
                    i += 1;
                    args.context = raw.get(i)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(10);
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => args.cmd.push(other.to_string()),
            }
            i += 1;
        }

        Ok(args)
    }
}

fn print_help() {
    println!(
        r#"bx — build error extractor

USAGE:
    bx [OPTIONS] <build command...>
    bx [OPTIONS]                      (uses saved command)
    <build command> 2>&1 | bx

OPTIONS:
    --tui           Open interactive TUI navigator after a failed build
    --warnings      Also show warnings (errors only by default)
    --verbose, -v   Stream all build output live
    --progress, -p  Show only build progress lines live ([ 42%] Building...)
    --save          Save the given command for this project
    --local         Treat the nearest .git (file or dir) as root — useful in submodules
    --context N     Lines of context per error block (default: 10)
    --help, -h      Show this help

EXAMPLES:
    bx --save cmake --build --preset debug   # save once
    bx                                        # run saved command
    bx --tui                                  # run saved command, open TUI on failure
    bx cmake --build --preset debug           # run without saving
    cmake --build build 2>&1 | bx             # pipe mode

SAVED COMMAND:
    Stored in .git/bx (or .bx-command if not in a git repo).
    .git/bx is ignored by git automatically — never committed.

TUI KEYBINDINGS:
    j / k           Move between errors
    gg / G          Jump to first / last error
    Enter           Focus detail pane
    Esc / q         Back to list / quit
    h j k l         Scroll detail pane
    y               Yank (copy) current error block to clipboard

CONFIG FILE:
    ~/.config/bx/config.toml

    [[patterns]]
    pattern  = "MY_COMPILER: error"
    severity = "error"    # error | linker | build | warning | note
"#
    );
}

// main

fn main() -> Result<()> {
    let args = Args::parse()?;

    // --save: persist the command and exit
    if args.save {
        anyhow::ensure!(!args.cmd.is_empty(), "--save requires a build command");
        return save_command(&args.cmd, args.local);
    }

    // Resolve the build command: explicit > saved > stdin
    let mut using_saved = false;
    let cmd: Vec<String> = if !args.cmd.is_empty() {
        args.cmd.clone()
    } else if let Some(saved) = load_command(args.local)? {
        eprintln!("bx: {} $ {}", saved.dir.display(), saved.cmd.join(" "));
        std::env::set_current_dir(&saved.dir)
            .with_context(|| format!("failed to cd to saved directory {:?}", saved.dir))?;
        using_saved = true;
        saved.cmd
    } else {
        Vec::new() // will fall through to stdin check below
    };

    let config = Config::load()?;

    let context_limit = if args.context > 0 {
        args.context
    } else {
        config.context.unwrap_or(10)
    };

    let patterns = build_patterns(&config.patterns)?;

    let (raw, success) = if !cmd.is_empty() {
        let out = run_command(&cmd, args.verbose, args.progress)?;
        (out.raw, out.success)
    } else {
        (read_stdin()?, false)
    };

    if success {
        if !args.verbose && !args.progress {
            print!("{}", raw);
        }
        return Ok(());
    }

    let blocks = collect_blocks(&raw, context_limit, &patterns);

    // Default to TUI when running a saved command with no explicit mode flag.
    // If the user passed --tui explicitly, or is using a saved command, open TUI.
    let use_tui = args.tui || using_saved;
    if use_tui {
        let shown: Vec<_> = blocks.into_iter().filter(|b| {
            b.severity.is_error() || (args.warnings && !b.severity.is_error())
        }).collect();
        render_tui(shown)?;
    } else {
        render_plain(&blocks, args.warnings);
    }

    Ok(())
}
