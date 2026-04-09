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

/// Find the saved command file. Prefers .git/bx so it is never accidentally
/// committed; falls back to .bx-command in the current directory.
fn saved_command_path() -> PathBuf {
    let git_dir = PathBuf::from(".git");
    if git_dir.is_dir() {
        git_dir.join("bx")
    } else {
        PathBuf::from(".bx-command")
    }
}

fn save_command(cmd: &[String]) -> Result<()> {
    let path = saved_command_path();
    std::fs::write(&path, cmd.join("\n"))
        .with_context(|| format!("failed to save command to {:?}", path))?;
    println!("bx: saved command to {:?}", path);
    println!("bx: run `bx` (with any flags) to use it");
    Ok(())
}

fn load_command() -> Result<Option<Vec<String>>> {
    let path = saved_command_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read saved command from {:?}", path))?;
    let cmd: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    if cmd.is_empty() {
        return Ok(None);
    }
    Ok(Some(cmd))
}

// Args

struct Args {
    tui:      bool,
    warnings: bool,
    verbose:  bool,
    progress: bool,
    save:     bool,
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
        return save_command(&args.cmd);
    }

    // Resolve the build command: explicit > saved > stdin
    let cmd: Vec<String> = if !args.cmd.is_empty() {
        args.cmd.clone()
    } else if let Some(saved) = load_command()? {
        eprintln!("bx: using saved command: {}", saved.join(" "));
        saved
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

    if args.tui {
        let shown: Vec<_> = blocks.into_iter().filter(|b| {
            b.severity.is_error() || (args.warnings && !b.severity.is_error())
        }).collect();
        render_tui(shown)?;
    } else {
        render_plain(&blocks, args.warnings);
    }

    Ok(())
}
