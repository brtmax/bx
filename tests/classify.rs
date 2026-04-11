// Unit tests for the core classification logic: classify(), parse_location(),
// collect_blocks(), and the config/pattern system.

use bx::classify::{
    build_patterns, classify, collect_blocks, ContextKind, ErrorBlock, Pattern,
    Severity, SourceLoc, UserPattern,
};

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
fn context_stores_all_lines_until_next_error() {
    let p = patterns();
    let input = "src/foo.cpp:1:1: error: bad\nline1\nline2\nline3\n";
    let blocks = collect_blocks(input, 2, &p);
    assert_eq!(blocks[0].context.len(), 3);
    assert_eq!(blocks[0].context_limit, 2);
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
        trigger:       "foo.cpp:1: error: bad".into(),
        severity:      Severity::Error,
        location:      None,
        context_limit: 10,
        context:       vec![
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

#[test]
fn classifies_rustc_error_with_code() {
    let p = patterns();
    assert_eq!(
        classify("error[E0382]: use of moved value: `x`", &p),
        Some(&Severity::Error)
    );
}

#[test]
fn classifies_rustc_error_without_code() {
    let p = patterns();
    assert_eq!(
        classify("error: could not compile `mike` (bin \"mike\") due to 3 previous errors", &p),
        Some(&Severity::Build)
    );
}

#[test]
fn classifies_rustc_warning() {
    let p = patterns();
    assert_eq!(
        classify("warning[unused_variables]: unused variable: `x`", &p),
        Some(&Severity::Warning)
    );
}

#[test]
fn classifies_rustc_note() {
    let p = patterns();
    assert_eq!(
        classify("note: `#[warn(unused_variables)]` on by default", &p),
        Some(&Severity::Note)
    );
}

#[test]
fn classifies_rustc_help() {
    let p = patterns();
    assert_eq!(
        classify("help: consider using `_x` to suppress the warning", &p),
        Some(&Severity::Note)
    );
}

#[test]
fn classifies_cargo_aborting() {
    let p = patterns();
    assert_eq!(
        classify("aborting due to 3 previous errors", &p),
        Some(&Severity::Build)
    );
}

#[test]
fn classifies_zig_error() {
    let p = patterns();
    assert_eq!(
        classify("src/main.zig:42:10: error: expected type 'u32', found 'i32'", &p),
        Some(&Severity::Error)
    );
}

#[test]
fn classifies_zig_note() {
    let p = patterns();
    assert_eq!(
        classify("src/main.zig:10:5: note: parameter type declared here", &p),
        Some(&Severity::Note)
    );
}

#[test]
fn classifies_zig_referenced_by() {
    let p = patterns();
    assert_eq!(
        classify("referenced by:", &p),
        Some(&Severity::Note)
    );
}

#[test]
fn classifies_zig_build_summary_failure() {
    let p = patterns();
    assert_eq!(
        classify("Build Summary: 0/3 steps succeeded; 1 failed", &p),
        Some(&Severity::Build)
    );
}

#[test]
fn zig_noise_is_none() {
    let p = patterns();
    assert_eq!(classify("pub fn main() void {", &p), None);
    assert_eq!(classify("const std = @import(\"std\");", &p), None);
}

// Need to re-export parse_location for tests since it is not in the public prelude
use bx::classify::parse_location;
