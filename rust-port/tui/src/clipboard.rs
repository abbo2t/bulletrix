//! Copy to the system clipboard via OSC 52: the terminal itself sets the
//! clipboard, so this works from WSL2 in Windows Terminal (and over SSH)
//! with no clip.exe/xclip. Same mechanism as Textual's copy_to_clipboard.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::io::{self, Write};

fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

pub fn copy(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(osc52(text).as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_text_as_base64_inside_an_osc52_sequence() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn encodes_utf8_bytes() {
        let mut out = Vec::new();
        copy(&mut out, "café ☕").unwrap();
        assert_eq!(out, b"\x1b]52;c;Y2Fmw6kg4piV\x07");
    }
}
