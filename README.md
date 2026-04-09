# bx

Build error extractor with Vim keybindings for C++ / CMake / Ninja / Rust / Zig. Extendable. Wraps your build command, filters the noise, and shows you only what went wrong. Always thought it's tedious to filter through the output if you compile in the terminal alot. I wanted the ability to jump around a structured error output using vim keybindings. Neovim does this for me to some extend but I wanted it to be editor-agnostic as well. Should work from any shell, provide proper interacive navigation, and error blocks can be yanked/copied directly for further research. 

<img src="bx.gif" width="400"/>

Partially a learning project for Rust, so be lenient :-) More or less a reference implementation / PoC. Eventually I want to rewrite it in Zig. 

## Install

```bash
cargo install --path .
```

## Usage
```bash
bx --save <build command>   save command for this project
bx [OPTIONS]                run saved command
bx [OPTIONS] <build command>

OPTIONS:
  --tui        open TUI navigator on failure (default when using saved command)
  --warnings   also show warnings
  --verbose    stream all build output live
  --progress   show only progress lines live ([ 42%] Building...)
  --context N  context lines shown per error in TUI (default: 10)
```

For example: 
```
bx cmake --build --preset debug
bx --tui cmake --build --preset debug
```

## Presetting the Build Command
### once per project
```bash
bx --save cmake --build --preset debug
```

### every build after that
```bash
bx
bx --tui
bx --progress
bx --tui --warnings
```
The command is stored in `.git/bx`. If it's not executed within a git repo, it falls back to `.bx-command`.

## Vim-ish keybindings

| Key | Action |
|---|---|
| `j` / `k` | Move between errors |
| `gg` / `G` | Jump to first / last |
| `Enter` | Focus detail pane |
| `Esc` / `q` | Back / quit |
| `hjkl` | Scroll detail pane |
| `y` | Copy error to clipboard |

## Config

`~/.config/bx/config.toml`

The context lines are how many lines it shows per error in the detail pane. bx always collects everything up to the next error, so the "context" here is only a display cap. 
```toml
context = 15  

[[patterns]]
pattern  = "MY_TOOL: error"
severity = "error"
```

## Next Up / Roadmap
- Extend pattern table for other languages
- Improve 'gg' handling for timeout case with expiry
- Compiler detection
- Use clap for arg parsing, shell completions
- More vim keybindings, line numbers, $n$ {h,j,k,l}
- Progress bar for normal build outpu

There should also be some way to retrieve the output from the last compilation without having to re-run it. Either just pipe to terminal after or bring it up with some special flag. Have to experiment what feels most natural/intuitive.

## License

MIT
