use ratatui::style::Color;

pub const RUST:  Color = Color::Rgb(0xb8, 0x6b, 0x65); // errors
pub const CLAY:  Color = Color::Rgb(0xc9, 0x88, 0x80); // linker errors
pub const OCHRE: Color = Color::Rgb(0xcc, 0x9e, 0x54); // build/ninja failures
pub const SAGE:  Color = Color::Rgb(0x86, 0x9c, 0x7a); // warnings
pub const PINE:  Color = Color::Rgb(0x56, 0x70, 0x6b); // notes
pub const SLATE: Color = Color::Rgb(0xa8, 0xbe, 0xf0); // context / active border
pub const MUTED: Color = Color::Rgb(0x9a, 0x8f, 0x82); // status bar, dim text
pub const DIM:   Color = Color::Rgb(0x4a, 0x42, 0x38); // inactive borders
