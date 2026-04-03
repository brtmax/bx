use std::path::PathBuf;

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::palette;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Linker,
    Build,
    Warning,
    Note,
}

impl Severity {
    pub fn color(&self) -> ratatui::style::Color {
        match self {
            Severity::Error   => palette::RUST,
            Severity::Linker  => palette::CLAY,
            Severity::Build   => palette::OCHRE,
            Severity::Warning => palette::SAGE,
            Severity::Note    => palette::PINE,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Severity::Error   => "ERR ",
            Severity::Linker  => "LINK",
            Severity::Build   => "BLD ",
            Severity::Warning => "WARN",
            Severity::Note    => "NOTE",
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Severity::Error | Severity::Linker | Severity::Build)
    }
}

pub struct Pattern {
    pub re:       Regex,
    pub severity: Severity,
}

// Order matters: more specific patterns before broader ones.
// fatal error: must come before error: so it isn't swallowed.
fn builtin_patterns() -> Vec<(&'static str, Severity)> {
    vec![
        (r":\s*fatal error:",        Severity::Error),
        (r":\s*error:",              Severity::Error),
        (r"error: ld returned",      Severity::Linker),
        (r"undefined reference to",  Severity::Linker),
        (r"multiple definition of",  Severity::Linker),
        (r"linker command failed",   Severity::Linker),
        (r"cannot find -l",          Severity::Linker),
        (r":\s*warning:",            Severity::Warning),
        (r":\s*note:",               Severity::Note),
        (r"CMake Error",             Severity::Error),
        (r"CMake Warning",           Severity::Warning),
        (r"-- FAILED",               Severity::Error),
        (r"FAILED:",                 Severity::Build),
        (r"ninja: build stopped",    Severity::Build),
        (r"make\[.+\]: \*\*\*",      Severity::Build),
        (r"make: \*\*\*",            Severity::Build),
        (r"too many errors emitted", Severity::Error),
        (r"errors generated",        Severity::Error),
        (r"error generated",         Severity::Error),
    ]
}

/// Build the pattern table, appending any user-defined patterns from config.
/// Built-in patterns are compiled with expect() since they are static and
/// known-valid; user patterns return an error on bad regex.
pub fn build_patterns(extra: &[UserPattern]) -> Result<Vec<Pattern>> {
    let mut patterns: Vec<Pattern> = builtin_patterns()
        .into_iter()
        .map(|(pat, sev)| Pattern {
            re:       Regex::new(pat).expect("built-in pattern is valid"),
            severity: sev,
        })
        .collect();

    for up in extra {
        let re = Regex::new(&up.pattern)
            .with_context(|| format!("invalid regex in config: {:?}", up.pattern))?;
        let severity = match up.severity.as_str() {
            "error"   => Severity::Error,
            "linker"  => Severity::Linker,
            "build"   => Severity::Build,
            "warning" => Severity::Warning,
            "note"    => Severity::Note,
            other => anyhow::bail!(
                "unknown severity {:?} in config (expected error/linker/build/warning/note)", other
            ),
        };
        patterns.push(Pattern { re, severity });
    }

    Ok(patterns)
}

pub fn classify<'a>(line: &str, patterns: &'a [Pattern]) -> Option<&'a Severity> {
    patterns.iter().find(|p| p.re.is_match(line)).map(|p| &p.severity)
}

/// Parsed file:line prefix from a compiler output line (e.g. `src/foo.cpp:42`).
/// Used to detect when context lines belong to a different location than the
/// error that opened the current block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLoc {
    pub file: String,
    pub line: u32,
}

/// Try to parse a `file:line:` or `file:line:col:` prefix.
/// Returns None if the line doesn't look like compiler output.
/// Deliberately lenient — only file and line number are required.
pub fn parse_location(line: &str) -> Option<SourceLoc> {
    if !line.contains(':') {
        return None;
    }

    // file.cpp:42:15: message
    //   parts[0] = file.cpp
    //   parts[1] = 42
    //   parts[2] = 15 (optional)
    let parts: Vec<&str> = line.splitn(4, ':').collect();
    if parts.len() < 2 {
        return None;
    }

    // Reject lines where the first field is a word rather than a path,
    // e.g. "error: something: detail"
    let file = parts[0];
    if file.is_empty() || (!file.contains('/') && !file.contains('.') && file.len() > 20) {
        return None;
    }

    let line_num: u32 = parts[1].trim().parse().ok()?;
    Some(SourceLoc { file: file.to_string(), line: line_num })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextKind {
    Note,
    Context,
}

#[derive(Debug, Clone)]
pub struct ErrorBlock {
    pub trigger:  String,
    pub severity: Severity,
    pub context:  Vec<(ContextKind, String)>,
    pub location: Option<SourceLoc>,
}

impl ErrorBlock {
    pub fn full_text(&self) -> String {
        let mut out = self.trigger.trim_end().to_string();
        for (kind, line) in &self.context {
            let prefix = match kind {
                ContextKind::Note    => "  >> ",
                ContextKind::Context => "     ",
            };
            out.push('\n');
            out.push_str(prefix);
            out.push_str(line.trim_end());
        }
        out
    }

    pub fn detail_lines(&self) -> Vec<ratatui::text::Line<'static>> {
        use ratatui::{style::{Modifier, Style}, text::{Line, Span}};

        let mut lines = vec![Line::from(Span::styled(
            self.trigger.trim_end().to_string(),
            Style::default().fg(self.severity.color()).add_modifier(Modifier::BOLD),
        ))];

        for (kind, line) in &self.context {
            let (prefix, color) = match kind {
                ContextKind::Note    => ("  >> ", palette::PINE),
                ContextKind::Context => ("     ", palette::SLATE),
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, line.trim_end()),
                Style::default().fg(color),
            )));
        }
        lines
    }
}

/// Group raw build output into ErrorBlocks.
///
/// Notes always attach to the previous block, they are annotations of the
/// line above them in compiler output, never a new block.
///
/// Unclassified lines attach as context up to `context_limit`. Context stops
/// early if a line has a parseable location that is in a different file or
/// more than LOC_JUMP_THRESHOLD lines away from the trigger — this prevents
/// context from one error bleeding into the next.
pub fn collect_blocks(raw: &str, context_limit: usize, patterns: &[Pattern]) -> Vec<ErrorBlock> {
    const LOC_JUMP_THRESHOLD: u32 = 5;

    let mut blocks: Vec<ErrorBlock> = Vec::new();
    let mut context_remaining = 0usize;

    for line in raw.lines() {
        let sev = classify(line, patterns);

        if let Some(Severity::Note) = sev {
            if let Some(last) = blocks.last_mut() {
                last.context.push((ContextKind::Note, line.to_string()));
                continue;
            }
        }

        if let Some(s) = sev {
            blocks.push(ErrorBlock {
                trigger:  line.to_string(),
                severity: s.clone(),
                context:  Vec::new(),
                location: parse_location(line),
            });
            context_remaining = context_limit;
            continue;
        }

        if context_remaining == 0 {
            continue;
        }

        // Stop attaching context if this line's location is far from the trigger.
        if let Some(last) = blocks.last() {
            if let (Some(trigger_loc), Some(line_loc)) = (&last.location, parse_location(line)) {
                let different_file = trigger_loc.file != line_loc.file;
                let far_away = trigger_loc.file == line_loc.file
                    && line_loc.line.abs_diff(trigger_loc.line) > LOC_JUMP_THRESHOLD;
                if different_file || far_away {
                    context_remaining = 0;
                    continue;
                }
            }
        }

        if let Some(last) = blocks.last_mut() {
            last.context.push((ContextKind::Context, line.to_string()));
            context_remaining -= 1;
        }
    }

    blocks
}

#[derive(Debug, Deserialize)]
pub struct UserPattern {
    pub pattern:  String,
    pub severity: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub patterns: Vec<UserPattern>,
    /// Default context lines per error. Overridden by --context.
    pub context:  Option<usize>,
}

impl Config {
    /// Load from ~/.config/bx/config.toml.
    /// Missing file is fine; a malformed file is an error.
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file {:?}", path))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config file {:?}", path))
    }
}

fn config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("bx").join("config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("bx").join("config.toml")
    } else {
        PathBuf::from(".bx.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns() -> Vec<Pattern> {
        build_patterns(&[]).expect("built-in patterns are valid")
    }

    #[test]
    fn classifies_gcc_error() {
        let p = patterns();
        assert_eq!(
            classify("src/foo.cpp:42:5: error: use of undeclared identifier 'x'", &p),
            Some(&Severity::Error)
        );
    }

    #[test]
    fn classifies_clang_fatal_error() {
        let p = patterns();
        assert_eq!(
            classify("src/foo.cpp:1:10: fatal error: 'missing.h' file not found", &p),
            Some(&Severity::Error)
        );
    }

    #[test]
    fn classifies_linker_undefined_ref() {
        let p = patterns();
        assert_eq!(
            classify("/usr/bin/ld: undefined reference to `main'", &p),
            Some(&Severity::Linker)
        );
    }

    #[test]
    fn classifies_ninja_failed() {
        let p = patterns();
        assert_eq!(
            classify("FAILED: CMakeFiles/bx.dir/src/main.cpp.o", &p),
            Some(&Severity::Build)
        );
    }

    #[test]
    fn classifies_warning() {
        let p = patterns();
        assert_eq!(
            classify("src/foo.cpp:10:3: warning: unused variable 'x'", &p),
            Some(&Severity::Warning)
        );
    }

    #[test]
    fn classifies_note() {
        let p = patterns();
        assert_eq!(
            classify("src/foo.cpp:8:1: note: declared here", &p),
            Some(&Severity::Note)
        );
    }

    #[test]
    fn noise_returns_none() {
        let p = patterns();
        assert_eq!(classify("[ 42%] Building CXX object ...", &p), None);
        assert_eq!(classify("-- Configuring done", &p), None);
        assert_eq!(classify("", &p), None);
    }

    #[test]
    fn parses_gcc_location() {
        assert_eq!(
            parse_location("src/foo.cpp:42:15: error: bad"),
            Some(SourceLoc { file: "src/foo.cpp".into(), line: 42 })
        );
    }

    #[test]
    fn parses_location_without_column() {
        assert_eq!(
            parse_location("src/foo.cpp:42: error: bad"),
            Some(SourceLoc { file: "src/foo.cpp".into(), line: 42 })
        );
    }

    #[test]
    fn no_location_for_noise() {
        assert_eq!(parse_location("[ 42%] Building"), None);
        assert_eq!(parse_location("ninja: build stopped"), None);
    }

    #[test]
    fn single_error_no_context() {
        let p = patterns();
        let blocks = collect_blocks("src/foo.cpp:1:1: error: bad\n", 10, &p);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].severity, Severity::Error);
        assert!(blocks[0].context.is_empty());
    }

    #[test]
    fn attaches_context_lines() {
        let p = patterns();
        let input = "src/foo.cpp:1:1: error: bad\n   1 | bad_code();\n     | ^~~~~~~~~~\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks[0].context.len(), 2);
        assert_eq!(blocks[0].context[0].0, ContextKind::Context);
    }

    #[test]
    fn note_attaches_to_previous_block() {
        let p = patterns();
        let input = "src/foo.cpp:42:1: error: no matching function\nsrc/foo.cpp:38:1: note: candidate declared here\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].context[0].0, ContextKind::Note);
    }

    #[test]
    fn two_errors_become_two_blocks() {
        let p = patterns();
        let input = "src/foo.cpp:1:1: error: first\nsrc/bar.cpp:2:1: error: second\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn context_window_expires() {
        let p = patterns();
        let input = "src/foo.cpp:1:1: error: bad\nline1\nline2\nline3\n";
        let blocks = collect_blocks(input, 2, &p);
        assert_eq!(blocks[0].context.len(), 2);
    }

    #[test]
    fn location_aware_stops_at_different_file() {
        let p = patterns();
        let input = "src/foo.cpp:1:1: error: bad\nother/bar.cpp:50:1: unclassified line\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks[0].context.len(), 0);
    }

    #[test]
    fn location_aware_stops_far_away_same_file() {
        let p = patterns();
        let input = "src/foo.cpp:1:1: error: bad\nsrc/foo.cpp:100:1: far away\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks[0].context.len(), 0);
    }

    #[test]
    fn location_aware_keeps_nearby_context() {
        let p = patterns();
        let input = "src/foo.cpp:10:1: error: bad\nsrc/foo.cpp:11:1: nearby detail\n";
        let blocks = collect_blocks(input, 10, &p);
        assert_eq!(blocks[0].context.len(), 1);
    }

    #[test]
    fn full_text_includes_context() {
        let block = ErrorBlock {
            trigger:  "foo.cpp:1: error: bad".into(),
            severity: Severity::Error,
            location: None,
            context:  vec![
                (ContextKind::Context, "   1 | bad_code()".into()),
                (ContextKind::Note,    "foo.cpp:5: note: here".into()),
            ],
        };
        let text = block.full_text();
        assert!(text.contains("foo.cpp:1: error: bad"));
        assert!(text.contains("   1 | bad_code()"));
        assert!(text.contains("  >> foo.cpp:5: note: here"));
    }

    #[test]
    fn user_pattern_extends_table() {
        let user = vec![UserPattern { pattern: r"MY_CUSTOM_ERROR".into(), severity: "error".into() }];
        let p = build_patterns(&user).unwrap();
        assert_eq!(classify("MY_CUSTOM_ERROR: went wrong", &p), Some(&Severity::Error));
    }

    #[test]
    fn invalid_user_pattern_returns_error() {
        let user = vec![UserPattern { pattern: r"[invalid".into(), severity: "error".into() }];
        assert!(build_patterns(&user).is_err());
    }
}
