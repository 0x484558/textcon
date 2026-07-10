#![allow(clippy::missing_const_for_fn, clippy::redundant_pub_crate)]

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::error::{Result, TextconError};

const COPY_BUFFER_SIZE: usize = 64 * 1024;

pub(crate) fn copy_raw<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    input_name: &str,
) -> Result<u64> {
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE].into_boxed_slice();
    let mut written = 0_u64;
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => return Ok(written),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(TextconError::Input {
                    name: input_name.to_owned(),
                    source,
                });
            }
        };
        writer
            .write_all(&buffer[..count])
            .map_err(TextconError::output)?;
        written = written.saturating_add(count as u64);
    }
}

pub(crate) fn write_markdown_record<R: Read, W: Write>(
    logical_path: &Path,
    reader: &mut R,
    adaptive: bool,
    writer: &mut W,
) -> Result<()> {
    let label = encode_path(logical_path.as_os_str());
    writer
        .write_all(format!("# `{label}`\n\n").as_bytes())
        .map_err(TextconError::output)?;

    let mut tail = TailWriter::new(writer);
    if adaptive {
        transform_markdown(reader, &mut tail, &label)?;
    } else {
        copy_raw(reader, &mut tail, &label)?;
    }

    if tail.bytes_written() != 0 {
        let endings = tail.trailing_line_endings();
        let missing = if endings == 1 && tail.ends_with_lone_cr() {
            2
        } else {
            2_usize.saturating_sub(endings)
        };
        tail.write_all(&b"\n\n"[..missing])
            .map_err(TextconError::output)?;
    }
    Ok(())
}

pub(crate) fn write_body<R: Read, W: Write>(
    logical_path: &Path,
    reader: &mut R,
    adaptive: bool,
    writer: &mut W,
) -> Result<()> {
    let label = encode_path(logical_path.as_os_str());
    if adaptive {
        transform_markdown(reader, writer, &label)
    } else {
        copy_raw(reader, writer, &label).map(|_| ())
    }
}

pub(crate) fn is_markdown_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let bytes = extension.as_encoded_bytes();
        bytes.eq_ignore_ascii_case(b"md") || bytes.eq_ignore_ascii_case(b"markdown")
    })
}

pub(crate) fn encode_path(path: &OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        encode_unix_bytes(path.as_bytes())
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        let mut encoded = String::new();
        for unit in path.encode_wide() {
            if unit <= 0x7f && is_safe_ascii(unit as u8, true) {
                encoded.push(char::from(unit as u8));
            } else {
                use std::fmt::Write as _;
                write!(encoded, "%u{unit:04X}").expect("writing to String cannot fail");
            }
        }
        return encoded;
    }

    #[cfg(not(any(unix, windows)))]
    encode_unix_bytes(path.as_encoded_bytes())
}

fn encode_unix_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::new();
    for &byte in bytes {
        if is_safe_ascii(byte, false) {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

const fn is_safe_ascii(byte: u8, windows: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'.' | b'_' | b'/' | b'-')
        || (windows && byte == b'\\')
}

fn transform_markdown<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    input_name: &str,
) -> Result<()> {
    let mut transformer = MarkdownTransformer::default();
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE].into_boxed_slice();
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(TextconError::Input {
                    name: input_name.to_owned(),
                    source,
                });
            }
        };
        transformer
            .transform(&buffer[..count], writer)
            .map_err(TextconError::output)?;
    }
    transformer.finish(writer).map_err(TextconError::output)
}

#[derive(Clone, Copy, Debug)]
struct Fence {
    marker: u8,
    length: usize,
}

#[derive(Debug)]
enum LineState {
    Start(Vec<u8>),
    Hashes {
        prefix: Vec<u8>,
        count: usize,
    },
    FenceRun {
        marker: u8,
        count: usize,
    },
    FenceRest {
        marker: u8,
        count: usize,
        valid: bool,
    },
    Pass,
}

impl Default for LineState {
    fn default() -> Self {
        Self::Start(Vec::with_capacity(3))
    }
}

#[derive(Debug, Default)]
struct MarkdownTransformer {
    fence: Option<Fence>,
    line: LineState,
    after_cr: bool,
}

impl MarkdownTransformer {
    fn transform<W: Write>(&mut self, input: &[u8], writer: &mut W) -> io::Result<()> {
        let mut index = 0;
        while index < input.len() {
            if matches!(self.line, LineState::Pass) && !self.after_cr {
                let remainder = &input[index..];
                let run = remainder
                    .iter()
                    .position(|byte| matches!(byte, b'\r' | b'\n'))
                    .unwrap_or(remainder.len());
                if run != 0 {
                    writer.write_all(&remainder[..run])?;
                    index += run;
                    continue;
                }
            }
            if let LineState::FenceRest { marker, valid, .. } = &mut self.line
                && !self.after_cr
            {
                let remainder = &input[index..];
                let run = remainder
                    .iter()
                    .position(|byte| matches!(byte, b'\r' | b'\n'))
                    .unwrap_or(remainder.len());
                if run != 0 {
                    let bytes = &remainder[..run];
                    if self.fence.is_some() {
                        *valid &= bytes.iter().all(|byte| matches!(byte, b' ' | b'\t'));
                    } else if *marker == b'`' && bytes.contains(&b'`') {
                        *valid = false;
                    }
                    writer.write_all(bytes)?;
                    index += run;
                    continue;
                }
            }

            let byte = input[index];
            index += 1;
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    writer.write_all(&[byte])?;
                    continue;
                }
            }

            if byte == b'\r' || byte == b'\n' {
                self.finish_line(writer)?;
                writer.write_all(&[byte])?;
                self.after_cr = byte == b'\r';
                continue;
            }
            self.consume_byte(byte, writer)?;
        }
        Ok(())
    }

    fn consume_byte<W: Write>(&mut self, byte: u8, writer: &mut W) -> io::Result<()> {
        match &mut self.line {
            LineState::Start(prefix) => {
                if byte == b' ' && prefix.len() < 4 {
                    prefix.push(byte);
                    if prefix.len() == 4 {
                        writer.write_all(prefix)?;
                        self.line = LineState::Pass;
                    }
                } else if prefix.len() <= 3 && self.fence.is_none() && byte == b'#' {
                    let mut hashes = std::mem::take(prefix);
                    hashes.push(byte);
                    self.line = LineState::Hashes {
                        prefix: hashes,
                        count: 1,
                    };
                } else if prefix.len() <= 3
                    && self
                        .fence
                        .map_or(matches!(byte, b'`' | b'~'), |fence| byte == fence.marker)
                {
                    writer.write_all(prefix)?;
                    writer.write_all(&[byte])?;
                    self.line = LineState::FenceRun {
                        marker: byte,
                        count: 1,
                    };
                } else {
                    writer.write_all(prefix)?;
                    writer.write_all(&[byte])?;
                    self.line = LineState::Pass;
                }
            }
            LineState::Hashes { prefix, count } => {
                if byte == b'#' && *count < 7 {
                    prefix.push(byte);
                    *count += 1;
                    if *count == 7 {
                        writer.write_all(prefix)?;
                        self.line = LineState::Pass;
                    }
                } else {
                    let shift = *count <= 5 && matches!(byte, b' ' | b'\t');
                    Self::write_hash_prefix(prefix, shift, writer)?;
                    writer.write_all(&[byte])?;
                    self.line = LineState::Pass;
                }
            }
            LineState::FenceRun { marker, count } => {
                writer.write_all(&[byte])?;
                if byte == *marker {
                    *count = count.saturating_add(1);
                } else {
                    let valid = if self.fence.is_some() {
                        matches!(byte, b' ' | b'\t')
                    } else {
                        *marker != b'`' || byte != b'`'
                    };
                    self.line = LineState::FenceRest {
                        marker: *marker,
                        count: *count,
                        valid,
                    };
                }
            }
            LineState::FenceRest {
                marker,
                count,
                valid,
            } => {
                writer.write_all(&[byte])?;
                if self.fence.is_some() {
                    *valid &= matches!(byte, b' ' | b'\t');
                } else if *marker == b'`' && byte == b'`' {
                    *valid = false;
                }
                let _ = count;
            }
            LineState::Pass => writer.write_all(&[byte])?,
        }
        Ok(())
    }

    fn finish_line<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        let line = std::mem::take(&mut self.line);
        match line {
            LineState::Start(prefix) => writer.write_all(&prefix)?,
            LineState::Hashes { prefix, count } => {
                Self::write_hash_prefix(&prefix, count <= 5, writer)?;
            }
            LineState::FenceRun { marker, count } => self.finish_fence(marker, count, true),
            LineState::FenceRest {
                marker,
                count,
                valid,
            } => self.finish_fence(marker, count, valid),
            LineState::Pass => {}
        }
        Ok(())
    }

    fn finish_fence(&mut self, marker: u8, count: usize, valid: bool) {
        if !valid {
            return;
        }
        if let Some(open) = self.fence {
            if marker == open.marker && count >= open.length {
                self.fence = None;
            }
        } else if count >= 3 {
            self.fence = Some(Fence {
                marker,
                length: count,
            });
        }
    }

    fn write_hash_prefix<W: Write>(prefix: &[u8], shift: bool, writer: &mut W) -> io::Result<()> {
        let hash_start = prefix.iter().position(|&byte| byte == b'#').unwrap_or(0);
        writer.write_all(&prefix[..hash_start])?;
        if shift {
            writer.write_all(b"#")?;
        }
        writer.write_all(&prefix[hash_start..])
    }

    fn finish<W: Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.finish_line(writer)
    }
}

struct TailWriter<'a, W> {
    inner: &'a mut W,
    tail: Vec<u8>,
    written: u64,
}

impl<'a, W> TailWriter<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        Self {
            inner,
            tail: Vec::with_capacity(4),
            written: 0,
        }
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn trailing_line_endings(&self) -> usize {
        let mut index = self.tail.len();
        let mut count = 0;
        while index > 0 && count < 2 {
            match self.tail[index - 1] {
                b'\n' => {
                    index -= 1;
                    if index > 0 && self.tail[index - 1] == b'\r' {
                        index -= 1;
                    }
                    count += 1;
                }
                b'\r' => {
                    index -= 1;
                    count += 1;
                }
                _ => break,
            }
        }
        count
    }

    fn ends_with_lone_cr(&self) -> bool {
        self.tail.last() == Some(&b'\r')
    }
}

impl<W: Write> Write for TailWriter<'_, W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.inner.write_all(buffer)?;
        self.written = self.written.saturating_add(buffer.len() as u64);
        if buffer.len() >= 4 {
            self.tail.clear();
            self.tail.extend_from_slice(&buffer[buffer.len() - 4..]);
        } else {
            self.tail.extend_from_slice(buffer);
            if self.tail.len() > 4 {
                let excess = self.tail.len() - 4;
                self.tail.drain(..excess);
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn shifts_atx_headings_outside_fences() {
        let input = b"# one\n## two\n##### five\n###### six\n```md\n# code\n```\nSetext\n===\n";
        let mut output = Vec::new();
        transform_markdown(&mut Cursor::new(input), &mut output, "test.md").unwrap();
        assert_eq!(
            output,
            b"## one\n### two\n###### five\n###### six\n```md\n# code\n```\nSetext\n===\n"
        );
    }

    #[test]
    fn markdown_record_has_h1_and_boundary() {
        let mut output = Vec::new();
        write_markdown_record(
            Path::new("src/main.rs"),
            &mut Cursor::new(b"fn main() {}"),
            false,
            &mut output,
        )
        .unwrap();
        assert_eq!(output, b"# `src/main.rs`\n\nfn main() {}\n\n");
    }

    #[test]
    fn preserves_non_headings_crlf_and_long_fences() {
        let input = b"#not\r\n   # yes\r\n    # code\r\n~~~~\r\n## fenced\r\n~~~~~\r\n## after\r\n";
        let mut output = Vec::new();
        transform_markdown(&mut Cursor::new(input), &mut output, "test.md").unwrap();
        assert_eq!(
            output,
            b"#not\r\n   ## yes\r\n    # code\r\n~~~~\r\n## fenced\r\n~~~~~\r\n### after\r\n"
        );
    }

    #[test]
    fn record_uses_minimum_missing_line_endings() {
        for (body, suffix) in [
            (&b"body"[..], &b"body\n\n"[..]),
            (&b"body\n"[..], &b"body\n\n"[..]),
            (&b"body\r\n"[..], &b"body\r\n\n"[..]),
            (&b"body\r"[..], &b"body\r\n\n"[..]),
            (&b"body\n\n"[..], &b"body\n\n"[..]),
        ] {
            let mut output = Vec::new();
            write_markdown_record(
                Path::new("file"),
                &mut Cursor::new(body),
                false,
                &mut output,
            )
            .unwrap();
            assert!(output.ends_with(suffix), "{output:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn path_encoding_is_lossless_and_safe() {
        use std::os::unix::ffi::OsStrExt as _;
        let path = OsStr::from_bytes(b"a%`\n\xff");
        assert_eq!(encode_path(path), "a%25%60%0A%FF");
    }
}
