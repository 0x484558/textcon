use std::cell::Cell;
use std::io::{self, Read, Write};
use std::path::Path;
use std::rc::Rc;

use textcon::{Engine, EngineOptions, RenderMode};

struct GeneratedReader {
    remaining: u64,
    reads: Rc<Cell<usize>>,
    byte: u8,
}

impl Read for GeneratedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        let count = usize::try_from(self.remaining.min(buffer.len() as u64)).unwrap();
        buffer[..count].fill(self.byte);
        self.remaining -= count as u64;
        Ok(count)
    }
}

struct FailImmediately;

impl Write for FailImmediately {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failure_stops_source_consumption() {
    let reads = Rc::new(Cell::new(0));
    let mut input = GeneratedReader {
        remaining: 1024 * 1024 * 1024,
        reads: Rc::clone(&reads),
        byte: b'x',
    };
    let engine = Engine::new(EngineOptions {
        render: RenderMode::Raw,
        ..EngineOptions::default()
    })
    .unwrap();
    let error = engine
        .render_reader(Path::new("stream"), &mut input, &mut FailImmediately)
        .unwrap_err();
    assert!(error.is_output_broken_pipe());
    assert_eq!(reads.get(), 1, "reader continued after output failed");
}

#[test]
fn large_single_line_streams_without_collecting_output() {
    let reads = Rc::new(Cell::new(0));
    let mut input = GeneratedReader {
        remaining: 64 * 1024 * 1024,
        reads: Rc::clone(&reads),
        byte: b'x',
    };
    let engine = Engine::new(EngineOptions::default()).unwrap();
    engine
        .render_reader(Path::new("large.md"), &mut input, &mut io::sink())
        .unwrap();
    assert!(reads.get() > 100);
}

#[test]
fn included_placeholder_text_is_not_reparsed() {
    let temporary = tempfile::TempDir::new().unwrap();
    std::fs::write(temporary.path().join("inner"), b"{{ @missing }}").unwrap();
    let engine = Engine::new(EngineOptions {
        base_dir: temporary.path().to_path_buf(),
        ..EngineOptions::default()
    })
    .unwrap();
    let mut output = Vec::new();
    engine
        .expand_template(&mut &b"{{ @inner }}"[..], &mut output)
        .unwrap();
    assert_eq!(output, b"{{ @missing }}");
}
