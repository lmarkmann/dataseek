use std::io::{IsTerminal, Read, Write};

use serde::Serialize;

use crate::output::Out;
use crate::{palette, ui};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "cannot read stdin: no input was piped\n  Try:   pass a file or pipe input: cat FILE | cli-template count"
    )]
    TerminalStdin,

    // A field named `source` is the error source to thiserror without any
    // attribute, so it already reaches the `Cause:` line in main::report.
    // Interpolating it here as well would print it twice.
    #[error(
        "cannot read {path}\n  Try:   check the path, or pipe input instead: cat FILE | cli-template count"
    )]
    Open { path: String, source: std::io::Error },

    // read_to_string reports a UTF-8 failure as InvalidData, which the Open
    // hint answers with two dead ends: the path is right, and piping the same
    // bytes fails identically. Split it out so the hint names the actual
    // constraint.
    #[error(
        "cannot read {path}: not UTF-8 text\n  Try:   count works on text; pass a text file"
    )]
    NotText { path: String },
}

#[derive(Serialize)]
struct Counts {
    lines: usize,
    words: usize,
    bytes: usize,
    source: String,
}

pub fn run(file: Option<&str>, out: &Out) -> anyhow::Result<()> {
    let (text, source) = read_input(file)?;
    out.note(&format!("read {} bytes from {source}", text.len()));

    let counts = Counts {
        lines: text.lines().count(),
        words: text.split_whitespace().count(),
        bytes: text.len(),
        source,
    };

    let mut w = out.stdout();
    if out.json {
        writeln!(w, "{}", serde_json::to_string(&counts)?)?;
    } else if out.plain {
        writeln!(w, "{}\t{}\t{}", counts.lines, counts.words, counts.bytes)?;
    } else {
        let v = palette::success();
        writeln!(
            w,
            "{v}{}{v:#} lines  {v}{}{v:#} words  {v}{}{v:#} bytes",
            counts.lines, counts.words, counts.bytes,
        )?;
    }
    Ok(())
}

/// Classify a failed read. Both call sites go through `read_to_string`, so
/// both can fail either because the bytes are unreachable or because they are
/// not text.
fn read_error(path: &str, source: std::io::Error) -> Error {
    if source.kind() == std::io::ErrorKind::InvalidData {
        Error::NotText { path: path.to_owned() }
    } else {
        Error::Open { path: path.to_owned(), source }
    }
}

fn read_input(file: Option<&str>) -> Result<(String, String), Error> {
    match file {
        None | Some("-") => {
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                return Err(Error::TerminalStdin);
            }

            // stdin is the one read whose length we cannot know up front, so
            // it is the honest place for a spinner: it shows only when a human
            // is waiting on a slow pipe and vanishes the instant input closes.
            let progress = ui::spinner("reading stdin");
            let mut buf = String::new();
            let read = stdin.read_to_string(&mut buf);
            progress.finish_and_clear();
            read.map_err(|source| read_error("<stdin>", source))?;
            Ok((buf, "stdin".into()))
        }
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|source| read_error(path, source))?;
            Ok((text, path.into()))
        }
    }
}
