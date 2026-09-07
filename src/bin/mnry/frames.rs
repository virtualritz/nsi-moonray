//! Frame-number placeholders, the way `rdl` spells them.
//!
//! `foo.@.nsi` with `--frames 1-3` is three files; `foo.@4.nsi` pads to
//! four digits. The sequence syntax itself is `frame-sequence`'s, the
//! same crate `rdl` uses, so `10-20@2` and `10-20@b` mean there what
//! they mean here.

use anyhow::{Result, anyhow};

/// Expand each argument into the files it names.
///
/// An argument with no `@` is itself, once, whether or not frames were
/// given -- a placeholder is what asks for the sequence.
pub fn expand(files: &[String], frames: Option<&str>) -> Result<Vec<String>> {
    let sequence = match frames {
        Some(text) => parse(text)?,
        None => Vec::new(),
    };

    let mut out = Vec::new();
    for file in files {
        match placeholder(file) {
            None => out.push(file.clone()),
            Some((at, padding)) => {
                if sequence.is_empty() {
                    return Err(anyhow!(
                        "{file:?} has a frame placeholder but no --frames \
                         to fill it with"
                    ));
                }
                let width = if padding == 0 { 1 } else { padding };
                let token = &file[at..at + if padding == 0 { 1 } else { 2 }];
                for frame in &sequence {
                    out.push(file.replace(
                        token,
                        &format!("{frame:0width$}", width = width),
                    ));
                }
            }
        }
    }

    Ok(out)
}

/// The frames a sequence expression names.
pub fn parse(text: &str) -> Result<Vec<isize>> {
    frame_sequence::parse_frame_sequence(text)
        .map_err(|error| anyhow!("in frame sequence {text:?}: {error}"))
}

/// Where the `@` is, and how many digits follow it.
///
/// `@4` pads to four; a bare `@` does not pad. Only one digit of
/// padding is read, which is `rdl`'s behaviour and enough for every
/// frame numbering anyone uses.
fn placeholder(file: &str) -> Option<(usize, usize)> {
    let at = file.find('@')?;
    let padding = file
        .get(at + 1..at + 2)
        .and_then(|digit| digit.parse::<usize>().ok())
        .unwrap_or(0);

    Some((at, padding))
}
