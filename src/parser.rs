#![allow(clippy::missing_const_for_fn, clippy::redundant_pub_crate)]

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use crate::error::{Result, TextconError};

const INPUT_BUFFER_SIZE: usize = 64 * 1024;
const LITERAL_BUFFER_SIZE: usize = 64 * 1024;
pub(crate) const MAX_REFERENCE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReferenceProcessor {
    Inherit,
    Markdown,
    Raw,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ParsedReference {
    pub(crate) path: PathBuf,
    pub(crate) processor: ReferenceProcessor,
    pub(crate) offset: u64,
}

#[derive(Debug)]
struct Candidate {
    start: u64,
    bytes: Vec<u8>,
    reference_like: bool,
}

pub(crate) fn expand<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    input_name: &str,
    mut on_reference: F,
) -> Result<()>
where
    R: Read,
    W: Write,
    F: FnMut(ParsedReference, &mut W) -> Result<()>,
{
    let mut scanner = Scanner::new(writer, &mut on_reference);
    let mut buffer = vec![0_u8; INPUT_BUFFER_SIZE].into_boxed_slice();
    let mut offset = 0_u64;
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
        for &byte in &buffer[..count] {
            scanner.feed(byte, offset)?;
            offset = offset.saturating_add(1);
        }
    }
    scanner.finish()
}

struct Scanner<'a, W, F> {
    writer: &'a mut W,
    on_reference: &'a mut F,
    literal: Vec<u8>,
    first_brace: Option<u64>,
    candidate: Option<Candidate>,
    replay: VecDeque<(u8, u64)>,
}

impl<'a, W, F> Scanner<'a, W, F>
where
    W: Write,
    F: FnMut(ParsedReference, &mut W) -> Result<()>,
{
    fn new(writer: &'a mut W, on_reference: &'a mut F) -> Self {
        Self {
            writer,
            on_reference,
            literal: Vec::with_capacity(LITERAL_BUFFER_SIZE),
            first_brace: None,
            candidate: None,
            replay: VecDeque::new(),
        }
    }

    fn feed(&mut self, byte: u8, offset: u64) -> Result<()> {
        self.replay.push_back((byte, offset));
        while let Some((next, next_offset)) = self.replay.pop_front() {
            if self.candidate.is_some() {
                self.feed_candidate(next, next_offset)?;
            } else {
                self.feed_literal(next, next_offset)?;
            }
        }
        Ok(())
    }

    fn feed_literal(&mut self, byte: u8, offset: u64) -> Result<()> {
        if let Some(first_offset) = self.first_brace.take() {
            if byte == b'{' {
                let slash_count = self
                    .literal
                    .iter()
                    .rev()
                    .take_while(|&&value| value == b'\\')
                    .count();
                self.literal.truncate(self.literal.len() - slash_count);
                self.literal
                    .extend(std::iter::repeat_n(b'\\', slash_count / 2));
                if slash_count % 2 == 1 {
                    self.push_literal(b'{')?;
                    self.push_literal(b'{')?;
                } else {
                    self.flush_literal()?;
                    self.candidate = Some(Candidate {
                        start: first_offset,
                        bytes: vec![b'{', b'{'],
                        reference_like: false,
                    });
                }
            } else {
                self.push_literal(b'{')?;
                self.feed_literal(byte, offset)?;
            }
            return Ok(());
        }

        if byte == b'{' {
            self.first_brace = Some(offset);
        } else {
            self.push_literal(byte)?;
        }
        Ok(())
    }

    fn feed_candidate(&mut self, byte: u8, offset: u64) -> Result<()> {
        let candidate = self.candidate.as_mut().expect("candidate exists");
        candidate.bytes.push(byte);
        if candidate.bytes.len() > MAX_REFERENCE_BYTES {
            if candidate.reference_like {
                return Err(TextconError::TemplateSyntax {
                    offset: candidate.start,
                    message: format!("reference exceeds the {MAX_REFERENCE_BYTES}-byte limit"),
                });
            }
            self.release_unrelated_candidate()?;
            return Ok(());
        }

        if !candidate.reference_like {
            let prefix = &candidate.bytes[2..];
            if let Some(&last) = prefix.last() {
                if last == b'@' && prefix[..prefix.len() - 1].iter().all(|b| is_ws(*b)) {
                    candidate.reference_like = true;
                } else if !is_ws(last) {
                    self.release_unrelated_candidate()?;
                }
            }
            return Ok(());
        }

        if byte == b'}' && candidate.bytes.len() >= 4 {
            let length = candidate.bytes.len();
            if candidate.bytes[length - 2] == b'}' && !is_escaped(&candidate.bytes, length - 2) {
                let completed = self.candidate.take().expect("candidate exists");
                let parsed = parse_reference(&completed)?;
                self.flush_literal()?;
                (self.on_reference)(parsed, self.writer)?;
            }
        }
        let _ = offset;
        Ok(())
    }

    fn release_unrelated_candidate(&mut self) -> Result<()> {
        let candidate = self.candidate.take().expect("candidate exists");
        let start = candidate.start;
        self.push_literal(b'{')?;
        for (index, &byte) in candidate.bytes[1..].iter().enumerate().rev() {
            self.replay.push_front((byte, start + 1 + index as u64));
        }
        Ok(())
    }

    fn push_literal(&mut self, byte: u8) -> Result<()> {
        self.literal.push(byte);
        if self.literal.len() >= LITERAL_BUFFER_SIZE {
            self.flush_literal()?;
        }
        Ok(())
    }

    fn flush_literal(&mut self) -> Result<()> {
        if !self.literal.is_empty() {
            self.writer
                .write_all(&self.literal)
                .map_err(TextconError::output)?;
            self.literal.clear();
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        if let Some(offset) = self.first_brace.take() {
            let _ = offset;
            self.push_literal(b'{')?;
        }
        if let Some(candidate) = self.candidate.take() {
            if candidate.reference_like {
                return Err(TextconError::TemplateSyntax {
                    offset: candidate.start,
                    message: "unterminated reference".to_owned(),
                });
            }
            for byte in candidate.bytes {
                self.push_literal(byte)?;
            }
        }
        self.flush_literal()
    }
}

fn parse_reference(candidate: &Candidate) -> Result<ParsedReference> {
    let inner = &candidate.bytes[2..candidate.bytes.len() - 2];
    let mut start = 0;
    while start < inner.len() && is_ws(inner[start]) {
        start += 1;
    }
    debug_assert_eq!(inner.get(start), Some(&b'@'));
    start += 1;

    let mut pipe = None;
    let mut index = start;
    while index < inner.len() {
        if inner[index] == b'|' && !is_escaped(inner, index) {
            let has_separator_space = index > start && is_ws(inner[index - 1]);
            if has_separator_space {
                if pipe.is_some() {
                    return syntax(candidate, "multiple reference processors");
                }
                pipe = Some(index);
            }
        } else if inner[index] == b'}' && !is_escaped(inner, index) {
            return syntax(candidate, "unescaped '}' in reference path");
        }
        index += 1;
    }

    let (raw_path, processor) = if let Some(pipe_index) = pipe {
        let path = trim_ascii(&inner[start..pipe_index]);
        let processor_bytes = trim_ascii(&inner[pipe_index + 1..]);
        let processor = match processor_bytes {
            b"raw" => ReferenceProcessor::Raw,
            b"markdown" => ReferenceProcessor::Markdown,
            b"" => return syntax(candidate, "missing reference processor"),
            _ => {
                return syntax(
                    candidate,
                    &format!(
                        "unknown reference processor '{}'",
                        String::from_utf8_lossy(processor_bytes)
                    ),
                );
            }
        };
        (path, processor)
    } else {
        (trim_ascii(&inner[start..]), ReferenceProcessor::Inherit)
    };

    if raw_path.is_empty() {
        return syntax(
            candidate,
            "reference path is empty; use '.' for the base directory",
        );
    }
    let path_bytes = unescape_path(raw_path);
    if path_bytes.contains(&0) {
        return syntax(candidate, "reference path contains NUL");
    }
    let path_string = String::from_utf8(path_bytes).map_err(|_| TextconError::TemplateSyntax {
        offset: candidate.start,
        message: "reference path is not valid UTF-8".to_owned(),
    })?;

    Ok(ParsedReference {
        path: PathBuf::from(path_string),
        processor,
        offset: candidate.start,
    })
}

fn syntax<T>(candidate: &Candidate, message: &str) -> Result<T> {
    Err(TextconError::TemplateSyntax {
        offset: candidate.start,
        message: message.to_owned(),
    })
}

fn unescape_path(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'\\'
            && input
                .get(index + 1)
                .is_some_and(|next| matches!(next, b'|' | b'}'))
        {
            output.push(input[index + 1]);
            index += 2;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    output
}

fn is_escaped(bytes: &[u8], index: usize) -> bool {
    let slash_count = bytes[..index]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count();
    slash_count % 2 == 1
}

const fn is_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

fn trim_ascii(input: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < input.len() && is_ws(input[start]) {
        start += 1;
    }
    trim_ascii_end(&input[start..])
}

fn trim_ascii_end(input: &[u8]) -> &[u8] {
    let mut end = input.len();
    while end > 0 && is_ws(input[end - 1]) {
        end -= 1;
    }
    &input[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read};

    struct Chunked<R> {
        inner: R,
        maximum: usize,
    }

    impl<R: Read> Read for Chunked<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(self.maximum);
            self.inner.read(&mut buffer[..limit])
        }
    }

    fn run(input: &[u8]) -> Result<(Vec<u8>, Vec<ParsedReference>)> {
        run_chunked(input, input.len().max(1))
    }

    fn run_chunked(input: &[u8], maximum: usize) -> Result<(Vec<u8>, Vec<ParsedReference>)> {
        let mut output = Vec::new();
        let mut refs = Vec::new();
        expand(
            &mut Chunked {
                inner: Cursor::new(input),
                maximum,
            },
            &mut output,
            "test",
            |reference, _| {
                refs.push(reference);
                Ok(())
            },
        )?;
        Ok((output, refs))
    }

    #[test]
    fn parses_processors_and_literal_pipes() {
        let (_, refs) =
            run(b"{{ @a|b }} {{ @dir | markdown }} {{ @x | raw }} {{ @  spaced  }}").unwrap();
        assert_eq!(refs[0].path, PathBuf::from("a|b"));
        assert_eq!(refs[0].processor, ReferenceProcessor::Inherit);
        assert_eq!(refs[1].processor, ReferenceProcessor::Markdown);
        assert_eq!(refs[2].processor, ReferenceProcessor::Raw);
        assert_eq!(refs[3].path, PathBuf::from("spaced"));
    }

    #[test]
    fn escape_and_overlap_are_preserved() {
        let (output, refs) = run(br"\{{ @literal }} {{{ @real }}}").unwrap();
        assert_eq!(output, b"{{ @literal }} {}");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, PathBuf::from("real"));
    }

    #[test]
    fn malformed_reference_is_an_error() {
        let error = run(b"prefix {{ @file | html }}").unwrap_err();
        assert!(matches!(
            error,
            TextconError::TemplateSyntax { offset: 7, .. }
        ));
    }

    #[test]
    fn unrelated_braces_pass_through() {
        let (output, refs) = run(b"{{ value }} and {{").unwrap();
        assert_eq!(output, b"{{ value }} and {{");
        assert!(refs.is_empty());
    }

    #[test]
    fn every_chunk_size_has_identical_semantics() {
        let fixtures: &[&[u8]] = &[
            br"prefix {{ @a\|b | raw }} suffix",
            br"\{{ literal }} and {{{ @real }}}",
            b"{{ value }} {{ @dir | markdown }}\r\n",
            b"arbitrary \xff bytes {{ @file }}",
            b"{{ @  spaced  | raw }}",
        ];
        for fixture in fixtures {
            let expected = run(fixture);
            for chunk in 1..=fixture.len() + 1 {
                let actual = run_chunked(fixture, chunk);
                assert_eq!(
                    format!("{actual:?}"),
                    format!("{expected:?}"),
                    "fixture {fixture:?}, chunk {chunk}"
                );
            }
        }
    }

    #[test]
    fn escape_parity_is_deterministic() {
        let (output, refs) = run(br"\{{ @a }} \\{{ @b }} \\\{{ @c }}").unwrap();
        assert_eq!(output, br"{{ @a }} \ \{{ @c }}");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, PathBuf::from("b"));
    }

    #[test]
    fn reference_limit_is_enforced_at_next_byte() {
        let accepted_padding = MAX_REFERENCE_BYTES - b"{{ @ }}".len();
        let mut accepted = b"{{ @".to_vec();
        accepted.extend(std::iter::repeat_n(b'a', accepted_padding));
        accepted.extend_from_slice(b" }}");
        assert_eq!(accepted.len(), MAX_REFERENCE_BYTES);
        assert!(run(&accepted).is_ok());

        let mut rejected = accepted;
        rejected.insert(rejected.len() - 2, b'b');
        assert!(matches!(
            run(&rejected),
            Err(TextconError::TemplateSyntax { .. })
        ));
    }
}
