# bx

Build error extractor with Vim keybindings for C++ / CMake / Ninja. Wraps your build command, filters the noise, and shows you only what went wrong. Always thought it's tedious to filter through the output if you compile in the terminal alot. I wanted the ability to jump around a structured error output using vim keybindings. Neovim does this for me to some extend but I wanted it to be editor-agnostic as well. Should work from any shell, provide proper interacive navigation, and error blocks can be yanked/copied directly for further research. 

<img src="bx.gif" width="400"/>

Partially a learning project for Rust, so be lenient :-)

## Install

```bash
cargo install --path .
```

## Usage

```
bx [--tui] [--warnings] [--verbose] [--context N] <build command>
```

For example: 
```
bx cmake --build --preset debug
bx --tui cmake --build --preset debug
```


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

```toml
context = 15  # context lines per error (default: 10)

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
