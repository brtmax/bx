//! Usage:
//!   bx cmake --build build
//!   bx --tui cmake --build build
//!   bx --warnings --verbose cmake --build build
//!   cmake --build build 2>&1 | bx

mod classify;
mod palette;
mod render;
mod subprocess;

use anyhow::Result;

use classify::{build_patterns, collect_blocks, Config};
use render::{render_plain, render_tui};
use subprocess::{read_stdin, run_command};

struct Args {
    tui:      bool,
    warnings: bool,
    verbose:  bool,
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
            context:  0,
            cmd:      Vec::new(),
        };

        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--tui"            => args.tui      = true,
                "--warnings"       => args.warnings = true,
                "--verbose" | "-v" => args.verbose  = true,
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
    <build command> 2>&1 | bx [OPTIONS]

OPTIONS:
    --tui           Open interactive TUI navigator after a failed build
    --warnings      Also show warnings (errors only by default)
    --verbose, -v   Stream raw build output live (silent by default)
    --context N     Lines of context per error block (default: 10)
    --help, -h      Show this help

EXAMPLES:
    bx cmake --build --preset debug
    bx --tui cmake --build --preset debug
    bx --verbose --warnings ninja -C build
    cmake --build build 2>&1 | bx

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

fn main() -> Result<()> {
    let args = Args::parse()?;

    let config = Config::load()?;

    let context_limit = if args.context > 0 {
        args.context
    } else {
        config.context.unwrap_or(10)
    };

    let patterns = build_patterns(&config.patterns)?;

    let (raw, success) = if !args.cmd.is_empty() {
        let out = run_command(&args.cmd, args.verbose)?;
        (out.raw, out.success)
    } else {
        (read_stdin()?, false)
    };

    if success {
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
