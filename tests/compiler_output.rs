// tests/compiler_output.rs
//
// Integration tests using real compiler output strings.
// These verify that bx correctly classifies and groups lines from actual
// GCC, Clang, Cargo/rustc, and Zig build output.

use bx::classify::{build_patterns, classify, collect_blocks, ContextKind, Severity};

fn patterns() -> Vec<bx::classify::Pattern> {
    build_patterns(&[]).expect("built-in patterns are valid")
}

// GCC output

#[test]
fn gcc_type_error_with_caret() {
    let p = patterns();
    let input = r#"src/main.cpp: In function 'int main()':
src/main.cpp:7:20: error: cannot convert 'std::string' {aka 'std::basic_string<char>'} to 'int'
    7 |     int result = add(name, 5);
      |                  ~~~^~~~~~~~
      |                     |
      |                     std::string {aka std::basic_string<char>}
src/main.cpp:3:15: note: initializing argument 1 of 'int add(int, int)'
    3 | int add(int a, int b) {
      |         ~~~~^
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].severity, Severity::Error);
    // caret lines and the note should all be captured
    let full = blocks[0].full_text();
    assert!(full.contains("cannot convert"));
    assert!(full.contains("std::basic_string"));
    // note attaches to the same block
    assert!(blocks[0].context.iter().any(|(k, _)| *k == ContextKind::Note));
}

#[test]
fn gcc_undeclared_identifier() {
    let p = patterns();
    let input = r#"src/main.cpp:10:22: error: 'unknown_var' was not declared in this scope
   10 |     std::cout << result << unknown_var << "\n";
      |                           ^~~~~~~~~~~
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("unknown_var"));
}

#[test]
fn gcc_linker_undefined_reference() {
    let p = patterns();
    let input = r#"/usr/bin/ld: CMakeFiles/demo.dir/src/main.cpp.o: in function `main':
main.cpp:(.text+0x8c): undefined reference to `divide(double, double)'
collect2: error: ld returned 1 exit status
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert!(blocks.iter().any(|b| b.severity == Severity::Linker));
    assert!(blocks.iter().any(|b| b.full_text().contains("divide")));
}

#[test]
fn gcc_multiple_errors_become_separate_blocks() {
    let p = patterns();
    let input = r#"src/main.cpp:7:20: error: cannot convert 'std::string' to 'int'
    7 |     int result = add(name, 5);
      |                      ^~~~
src/main.cpp:10:22: error: 'unknown_var' was not declared in this scope
   10 |     std::cout << result << unknown_var << "\n";
      |                           ^~~~~~~~~~~
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 2);
    assert!(blocks[0].full_text().contains("cannot convert"));
    assert!(blocks[1].full_text().contains("unknown_var"));
}

#[test]
fn gcc_warning_not_included_by_default() {
    let p = patterns();
    let input = r#"src/main.cpp:5:15: warning: unused variable 'x' [-Wunused-variable]
    5 |     int x = 0;
      |             ^
src/main.cpp:10:5: error: use of undeclared identifier 'foo'
   10 |     foo();
      |     ^~~
"#;
    let blocks = collect_blocks(input, 10, &p);
    let errors: Vec<_> = blocks.iter().filter(|b| b.severity.is_error()).collect();
    let warnings: Vec<_> = blocks.iter().filter(|b| b.severity == Severity::Warning).collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(warnings.len(), 1);
}

// Clang output

#[test]
fn clang_error_with_template_note() {
    let p = patterns();
    let input = r#"src/main.cpp:29:6: error: call to deleted function 'add'
   29 |     add("", "");
      |     ^~~
src/main.cpp:24:6: note: candidate template ignored: substitution failure [with A = const char *, B = const char *]
   24 | auto add(A a, B b) -> decltype(a + b) {
      |      ^
1 error generated.
"#;
    let blocks = collect_blocks(input, 10, &p);
    // The note should attach to the error block, not create a new one
    assert_eq!(blocks.iter().filter(|b| b.severity == Severity::Error).count(), 1);
    let err = blocks.iter().find(|b| b.severity == Severity::Error).unwrap();
    assert!(err.context.iter().any(|(k, _)| *k == ContextKind::Note));
}

#[test]
fn clang_fatal_error_missing_header() {
    let p = patterns();
    let input = r#"src/main.cpp:1:10: fatal error: 'missing_header.h' file not found
    1 | #include "missing_header.h"
      |          ^~~~~~~~~~~~~~~~~~
1 error generated.
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("fatal error"));
}

#[test]
fn clang_ninja_failed_line_is_build_severity() {
    let p = patterns();
    let line = "FAILED: CMakeFiles/demo.dir/src/main.cpp.o";
    assert_eq!(classify(line, &p), Some(&Severity::Build));
}

#[test]
fn clang_ninja_stopped_line_is_build_severity() {
    let p = patterns();
    let line = "ninja: build stopped: subcommand failed.";
    assert_eq!(classify(line, &p), Some(&Severity::Build));
}

#[test]
fn clang_progress_lines_are_noise() {
    let p = patterns();
    assert_eq!(classify("[ 12%] Building CXX object CMakeFiles/demo.dir/src/main.cpp.o", &p), None);
    assert_eq!(classify("[ 75%] Linking CXX executable demo", &p), None);
    assert_eq!(classify("-- Configuring done", &p), None);
    assert_eq!(classify("-- Build files have been written to: /build", &p), None);
}

// Cargo / rustc output

#[test]
fn cargo_error_with_code_and_location() {
    let p = patterns();
    let input = r#"error[E0308]: mismatched types
 --> src/main.rs:11:25
  |
11 |     let result: &str = add(1, 2);
  |                 ----   ^^^^^^^^^ expected `&str`, found `i32`
  |                 |
  |                 expected due to this
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("E0308"));
    assert!(blocks[0].full_text().contains("mismatched types"));
}

#[test]
fn cargo_use_of_moved_value() {
    let p = patterns();
    let input = r#"error[E0382]: use of moved value: `s`
 --> src/main.rs:15:12
  |
14 |     let s2 = s;
  |              - value moved here
15 |     greet(&s);
  |            ^^ value used here after move
  |
  = note: move occurs because `s` has type `String`, which does not implement the `Copy` trait
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("E0382"));
    // the note line should be captured
    assert!(blocks[0].full_text().contains("Copy"));
}

#[test]
fn cargo_undeclared_variable() {
    let p = patterns();
    let input = r#"error[E0425]: cannot find value `missing_var` in this scope
 --> src/main.rs:18:20
  |
18 |     println!("{}", missing_var);
  |                    ^^^^^^^^^^^ not found in this scope
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("missing_var"));
}

#[test]
fn cargo_could_not_compile_is_build_not_error() {
    let p = patterns();
    let line = "error: could not compile `demo` (bin \"demo\") due to 3 previous errors";
    assert_eq!(classify(line, &p), Some(&Severity::Build));
}

#[test]
fn cargo_aborting_is_build() {
    let p = patterns();
    let line = "aborting due to 3 previous errors; 1 warning emitted";
    assert_eq!(classify(line, &p), Some(&Severity::Build));
}

#[test]
fn cargo_warning_with_location() {
    let p = patterns();
    let input = r#"warning[unused_variables]: unused variable: `result`
 --> src/main.rs:11:9
  |
11 |     let result: &str = add(1, 2);
  |         ^^^^^^ help: if this is intentional, prefix it with an underscore: `_result`
  |
  = note: `#[warn(unused_variables)]` on by default
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].severity, Severity::Warning);
}

#[test]
fn cargo_help_line_is_note() {
    let p = patterns();
    let line = "help: consider using `_x` to suppress the warning";
    assert_eq!(classify(line, &p), Some(&Severity::Note));
}

#[test]
fn cargo_multiple_errors_separate_blocks() {
    let p = patterns();
    let input = r#"error[E0308]: mismatched types
 --> src/main.rs:11:25
  |
11 |     let result: &str = add(1, 2);
  |                         ^^^^^^^^^ expected `&str`, found `i32`

error[E0382]: use of moved value: `s`
 --> src/main.rs:15:12
  |
15 |     greet(&s);
  |            ^^ value used here after move

error[E0425]: cannot find value `missing_var` in this scope
 --> src/main.rs:18:20
  |
18 |     println!("{}", missing_var);
  |                    ^^^^^^^^^^^ not found in this scope
"#;
    let blocks = collect_blocks(input, 10, &p);
    let errors: Vec<_> = blocks.iter().filter(|b| b.severity == Severity::Error).collect();
    assert_eq!(errors.len(), 3);
    assert!(errors[0].full_text().contains("E0308"));
    assert!(errors[1].full_text().contains("E0382"));
    assert!(errors[2].full_text().contains("E0425"));
}

// Zig output

#[test]
fn zig_type_mismatch() {
    let p = patterns();
    let input = r#"main.zig:13:22: error: expected type 'u32', found 'comptime_int'
    const result = add(-1, 5);
                       ^
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("expected type"));
}

#[test]
fn zig_undeclared_identifier() {
    let p = patterns();
    let input = r#"main.zig:19:11: error: use of undeclared identifier 'missing_name'
    greet(missing_name);
          ^~~~~~~~~~~~
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks[0].severity, Severity::Error);
    assert!(blocks[0].full_text().contains("missing_name"));
}

#[test]
fn zig_referenced_by_chain_attaches_to_error() {
    let p = patterns();
    let input = r#"main.zig:13:22: error: expected type 'u32', found 'comptime_int'
    const result = add(-1, 5);
                       ^
referenced by:
    main: main.zig:12:5
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    // referenced by should be a note attached to the error
    assert!(blocks[0].context.iter().any(|(k, line)| {
        *k == ContextKind::Note && line.contains("referenced by")
    }));
}

#[test]
fn zig_build_summary_failure() {
    let p = patterns();
    let line = "Build Summary: 0/3 steps succeeded; 1 failed";
    assert_eq!(classify(line, &p), Some(&Severity::Build));
}

#[test]
fn zig_note_line() {
    let p = patterns();
    let line = "main.zig:3:12: note: parameter type declared here";
    assert_eq!(classify(line, &p), Some(&Severity::Note));
}

#[test]
fn zig_multiple_errors_separate_blocks() {
    let p = patterns();
    let input = r#"main.zig:13:22: error: expected type 'u32', found 'comptime_int'
    const result = add(-1, 5);
                       ^
main.zig:19:11: error: use of undeclared identifier 'missing_name'
    greet(missing_name);
          ^~~~~~~~~~~~
"#;
    let blocks = collect_blocks(input, 10, &p);
    let errors: Vec<_> = blocks.iter().filter(|b| b.severity == Severity::Error).collect();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].full_text().contains("expected type"));
    assert!(errors[1].full_text().contains("missing_name"));
}

#[test]
fn zig_noise_lines_are_ignored() {
    let p = patterns();
    assert_eq!(classify("pub fn main() void {", &p), None);
    assert_eq!(classify("    const std = @import(\"std\");", &p), None);
    assert_eq!(classify("}", &p), None);
    assert_eq!(classify("", &p), None);
}

// Context grouping correctness

#[test]
fn context_does_not_bleed_between_adjacent_errors() {
    let p = patterns();
    // Two errors in the same file at distant lines — context from the first
    // should not contaminate the second block.
    let input = r#"src/foo.cpp:10:5: error: first error
   10 | bad_code();
      | ^
src/foo.cpp:50:5: error: second error
   50 | more_bad();
      | ^
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 2);
    let first_text = blocks[0].full_text();
    let second_text = blocks[1].full_text();
    assert!(first_text.contains("first error"));
    assert!(!first_text.contains("second error"));
    assert!(second_text.contains("second error"));
    assert!(!second_text.contains("first error"));
}

#[test]
fn notes_always_attach_regardless_of_location() {
    let p = patterns();
    // Note refers to a different line than the error, but should still attach.
    let input = r#"src/main.cpp:42:5: error: no matching function for call to 'foo'
   42 |     foo(1.0);
      |     ^~~
src/main.cpp:10:6: note: candidate function not viable: no known conversion from 'double' to 'int'
   10 | void foo(int x) {}
      |      ^
"#;
    let blocks = collect_blocks(input, 10, &p);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].context.iter().any(|(k, _)| *k == ContextKind::Note));
}

#[test]
fn all_context_stored_up_to_next_error() {
    let p = patterns();
    // 20 context lines between two errors — all should be stored even though
    // context_limit is 5. The limit is for display only.
    let mut input = "src/foo.cpp:1:1: error: first\n".to_string();
    for i in 0..20 {
        input.push_str(&format!("   context line {}\n", i));
    }
    input.push_str("src/foo.cpp:30:1: error: second\n");

    let blocks = collect_blocks(&input, 5, &p);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].context_limit, 5);
    // All 20 lines stored even though display limit is 5
    assert_eq!(
        blocks[0].context.iter().filter(|(k, _)| *k == ContextKind::Context).count(),
        20
    );
}
