// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Content adapters preserve reader position and declared input bounds.

use std::cell::Cell;
use std::io::Cursor;
use std::io::Error;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use qubit_config::Config;
use qubit_mime::CONFIG_MIME_MAX_BUFFER_SIZE;
use qubit_mime::ContentRequirement;
use qubit_mime::MimeConfig;
use qubit_mime::MimeContentBackend;
use qubit_mime::MimeDetectorAdapter;
use qubit_mime::MimeDetectorBackend;
use qubit_mime::MimeDetectorCore;
use qubit_mime::MimeResult;
use qubit_mime::RepositoryMimeDetector;

use crate::support::DirectBackendDetector;

#[derive(Debug)]
struct WholeResourceReaderBackend(DirectBackendDetector);

impl MimeDetectorBackend for WholeResourceReaderBackend {
    fn core(&self) -> &qubit_mime::MimeDetectorCore {
        self.0.core()
    }
    fn max_test_bytes(&self) -> usize {
        self.0.max_test_bytes()
    }
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }
    fn guess_from_filename(&self, filename: &str) -> Vec<String> {
        self.0.guess_from_filename(filename)
    }
    fn guess_from_content(&self, content: &[u8]) -> MimeResult<Vec<String>> {
        self.0.guess_from_content(content)
    }
    fn guess_from_reader(&self, reader: &mut dyn qubit_io::std_io::ReadSeek) -> MimeResult<(Vec<String>, Vec<u8>)> {
        reader.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        assert!(bytes.starts_with(b"%PDF-"));
        Ok((vec!["application/pdf".to_owned()], Vec::new()))
    }
}

#[test]
fn test_adapter_reader_hook_sees_remaining_window_and_restores() {
    let adapter = MimeDetectorAdapter::new(WholeResourceReaderBackend(DirectBackendDetector::new()));
    let mut reader = Cursor::new(b"skip%PDF-1.7 and a long trailer".to_vec());
    reader.set_position(4);
    assert_eq!(adapter.detect_reader(&mut reader).unwrap(), ["application/pdf"]);
    assert_eq!(reader.position(), 4);
}

#[derive(Debug, Clone, Copy)]
enum ReaderProbe {
    Sample,
    SeekValid,
    BeforeBase,
    Overflow,
    EndInvalidPreservesPosition,
    FailAfterSeek,
    ReadThenFail,
}

#[derive(Debug)]
struct ReaderProbeBackend {
    core: MimeDetectorCore,
    probe: ReaderProbe,
    read_calls: Arc<AtomicUsize>,
}

impl ReaderProbeBackend {
    fn with_budget(probe: ReaderProbe, budget: u64, read_calls: Arc<AtomicUsize>) -> Self {
        let mut config = Config::new();
        config
            .set(CONFIG_MIME_MAX_BUFFER_SIZE, budget)
            .expect("set test MIME budget");
        let mime_config = MimeConfig::from_config(&config).expect("parse test MIME config");
        Self {
            core: MimeDetectorCore::new(mime_config),
            probe,
            read_calls,
        }
    }
}

impl MimeDetectorBackend for ReaderProbeBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Complete
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        panic!("reader adapter must not fall back to guess_from_content")
    }
    fn guess_from_reader(&self, reader: &mut dyn qubit_io::std_io::ReadSeek) -> MimeResult<(Vec<String>, Vec<u8>)> {
        match self.probe {
            ReaderProbe::Sample => {
                reader.seek(SeekFrom::Start(0))?;
                let mut sample = [0_u8; 3];
                let count = reader.read(&mut sample)?;
                self.read_calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(&sample[..count], b"PDF");
                Ok((vec!["application/pdf".to_owned()], Vec::new()))
            }
            ReaderProbe::SeekValid => {
                assert_eq!(reader.seek(SeekFrom::Current(2))?, 2);
                assert_eq!(reader.seek(SeekFrom::Current(-1))?, 1);
                assert_eq!(reader.seek(SeekFrom::End(-1))?, 5);
                assert_eq!(reader.seek(SeekFrom::End(2))?, 8);
                let mut byte = [0_u8; 1];
                assert_eq!(reader.read(&mut byte)?, 0, "seek beyond EOF remains valid");
                Ok((vec!["seek-ok".to_owned()], Vec::new()))
            }
            ReaderProbe::BeforeBase => {
                reader.seek(SeekFrom::Start(0))?;
                reader.seek(SeekFrom::Current(-1))?;
                unreachable!("a seek before the view base must fail")
            }
            ReaderProbe::Overflow => {
                reader.seek(SeekFrom::Start(0))?;
                reader.seek(SeekFrom::Current(i64::MAX))?;
                reader.seek(SeekFrom::Current(i64::MAX))?;
                unreachable!("overflowing relative seek must fail")
            }
            ReaderProbe::EndInvalidPreservesPosition => {
                assert_eq!(reader.seek(SeekFrom::Start(2))?, 2);
                assert!(reader.seek(SeekFrom::End(-100)).is_err());
                assert_eq!(reader.stream_position()?, 2);
                Ok((vec!["end-preserved".to_owned()], Vec::new()))
            }
            ReaderProbe::FailAfterSeek => {
                reader.seek(SeekFrom::Start(0))?;
                Err(qubit_mime::MimeError::Io(Error::other("injected hook error")))
            }
            ReaderProbe::ReadThenFail => {
                let mut byte = [0_u8; 1];
                assert_eq!(reader.read(&mut byte)?, 1);
                Err(qubit_mime::MimeError::Io(Error::other("injected hook error")))
            }
        }
    }
}

#[test]
fn test_adapter_complete_sampling_and_seek_window_contract() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter = MimeDetectorAdapter::new(ReaderProbeBackend::with_budget(
        ReaderProbe::Sample,
        3,
        Arc::clone(&calls),
    ));
    assert_eq!(adapter.max_reader_buffer_size(), 3);
    let mut large_resource = vec![b'x'; 1024 * 1024];
    large_resource[..7].copy_from_slice(b"skipPDF");
    let (mut reader, reads) = counting_reader(&large_resource);
    reader.cursor.set_position(4);
    assert_eq!(adapter.detect_reader(&mut reader).unwrap(), ["application/pdf"]);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "random-access Complete hook samples without whole-resource buffering"
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(reader.cursor.position(), 4);

    let valid = MimeDetectorAdapter::new(ReaderProbeBackend::with_budget(
        ReaderProbe::SeekValid,
        3,
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut reader = Cursor::new(b"skipabcdef".to_vec());
    reader.set_position(4);
    assert_eq!(valid.detect_reader(&mut reader).unwrap(), ["seek-ok"]);
    assert_eq!(reader.position(), 4);

    let end_preserved = MimeDetectorAdapter::new(ReaderProbeBackend::with_budget(
        ReaderProbe::EndInvalidPreservesPosition,
        3,
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut reader = Cursor::new(b"skipabcdef".to_vec());
    reader.set_position(4);
    assert_eq!(end_preserved.detect_reader(&mut reader).unwrap(), ["end-preserved"]);
    assert_eq!(reader.position(), 4);
}

#[test]
fn test_adapter_restores_after_view_and_hook_errors() {
    for probe in [
        ReaderProbe::BeforeBase,
        ReaderProbe::Overflow,
        ReaderProbe::FailAfterSeek,
    ] {
        let adapter =
            MimeDetectorAdapter::new(ReaderProbeBackend::with_budget(probe, 3, Arc::new(AtomicUsize::new(0))));
        let mut reader = Cursor::new(b"skipabcdef".to_vec());
        reader.set_position(4);
        assert!(adapter.detect_reader(&mut reader).is_err());
        assert_eq!(reader.position(), 4, "adapter restores reader after {probe:?}");
    }
}

#[derive(Debug)]
struct RestoreFailReader {
    cursor: Cursor<Vec<u8>>,
    read_happened: bool,
}

impl Read for RestoreFailReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.cursor.read(bytes)?;
        self.read_happened |= count > 0;
        Ok(count)
    }
}

impl Seek for RestoreFailReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        if self.read_happened && matches!(from, SeekFrom::Start(4)) {
            return Err(Error::other("injected restore failure"));
        }
        self.cursor.seek(from)
    }
}

#[test]
fn test_adapter_restore_error_takes_priority_over_hook_error() {
    let adapter = MimeDetectorAdapter::new(ReaderProbeBackend::with_budget(
        ReaderProbe::ReadThenFail,
        3,
        Arc::new(AtomicUsize::new(0)),
    ));
    let mut reader = RestoreFailReader {
        cursor: Cursor::new(b"skipabcdef".to_vec()),
        read_happened: false,
    };
    reader.cursor.set_position(4);

    assert!(matches!(
        adapter.detect_reader(&mut reader),
        Err(qubit_mime::MimeError::Io(error))
            if error.to_string().contains("injected restore failure")
    ));
}

#[test]
fn test_adapter_prefix_budget_rejects_before_reading() {
    let mut config = Config::new();
    config.set(CONFIG_MIME_MAX_BUFFER_SIZE, 3_u64).unwrap();
    let core = MimeDetectorCore::new(MimeConfig::from_config(&config).unwrap());
    let backend = OverBudgetPrefixBackend { core };
    let adapter = MimeDetectorAdapter::new(backend);
    let (mut reader, reads) = counting_reader(b"skipabcdef");
    assert!(matches!(
        adapter.detect_reader(&mut reader),
        Err(qubit_mime::MimeError::BufferLimitExceeded { requested: 5, limit: 3 })
    ));
    assert_eq!(reads.get(), 0);
}

#[derive(Debug)]
struct OverBudgetPrefixBackend {
    core: MimeDetectorCore,
}
impl MimeDetectorBackend for OverBudgetPrefixBackend {
    fn core(&self) -> &MimeDetectorCore {
        &self.core
    }
    fn max_test_bytes(&self) -> usize {
        5
    }
    fn guess_from_filename(&self, _filename: &str) -> Vec<String> {
        Vec::new()
    }
    fn guess_from_content(&self, _content: &[u8]) -> MimeResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// The legacy adapter retains backend limits and detects from a nonzero reader
/// position.
#[test]
fn test_detector_adapter_uses_backend_prefix_and_restores_position() {
    let backend = RepositoryMimeDetector::new().expect("built-in repository");
    let expected_limit = backend.max_test_bytes();
    let adapter = MimeDetectorAdapter::new(backend);
    assert_eq!(adapter.backend().max_test_bytes(), expected_limit);
    assert_eq!(
        adapter.content_requirement(),
        ContentRequirement::Prefix(expected_limit)
    );
    let mut reader = Cursor::new(b"skip%PDF-1.7\n".to_vec());
    reader.set_position(4);
    let candidates = adapter.detect_reader(&mut reader).expect("detect PDF prefix");
    assert!(candidates.iter().any(|candidate| candidate == "application/pdf"));
    assert_eq!(reader.position(), 4);
}

#[derive(Debug)]
struct LengthBackend(ContentRequirement);

impl MimeContentBackend for LengthBackend {
    fn content_requirement(&self) -> ContentRequirement {
        self.0
    }

    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        Ok(vec![bytes.len().to_string()])
    }
}

#[derive(Debug)]
struct LimitedLengthBackend(ContentRequirement, usize);

impl MimeContentBackend for LimitedLengthBackend {
    fn content_requirement(&self) -> ContentRequirement {
        self.0
    }
    fn max_reader_buffer_size(&self) -> usize {
        self.1
    }
    fn detect_bytes(&self, bytes: &[u8]) -> MimeResult<Vec<String>> {
        Ok(vec![bytes.len().to_string()])
    }
}

#[derive(Debug)]
struct CountingReader {
    cursor: Cursor<Vec<u8>>,
    reads: Rc<Cell<usize>>,
}

impl Read for CountingReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        self.cursor.read(bytes)
    }
}

impl Seek for CountingReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(from)
    }
}

fn counting_reader(bytes: &[u8]) -> (CountingReader, Rc<Cell<usize>>) {
    let reads = Rc::new(Cell::new(0));
    (
        CountingReader {
            cursor: Cursor::new(bytes.to_vec()),
            reads: Rc::clone(&reads),
        },
        reads,
    )
}

#[test]
fn test_reader_budget_and_complete_boundary_probe() {
    let backend = LimitedLengthBackend(ContentRequirement::Complete, 3);
    let mut exact = Cursor::new(b"abc".to_vec());
    assert_eq!(backend.detect_reader(&mut exact).unwrap(), ["3"]);
    let mut oversized = Cursor::new(b"abcd".to_vec());
    assert!(matches!(
        backend.detect_reader(&mut oversized),
        Err(qubit_mime::MimeError::BufferLimitExceeded { requested: 4, limit: 3 })
    ));
    assert_eq!(oversized.position(), 0);
    let prefix = LimitedLengthBackend(ContentRequirement::Prefix(4), 3);
    let mut input = Cursor::new(b"abcd".to_vec());
    assert!(matches!(
        prefix.detect_reader(&mut input),
        Err(qubit_mime::MimeError::BufferLimitExceeded { requested: 4, limit: 3 })
    ));
    assert_eq!(input.position(), 0);

    let zero = LimitedLengthBackend(ContentRequirement::Complete, 0);
    let mut empty = Cursor::new(Vec::<u8>::new());
    assert_eq!(zero.detect_reader(&mut empty).unwrap(), ["0"]);
    let mut nonempty = Cursor::new(b"x".to_vec());
    assert!(matches!(
        zero.detect_reader(&mut nonempty),
        Err(qubit_mime::MimeError::BufferLimitExceeded { requested: 1, limit: 0 })
    ));
    assert_eq!(nonempty.position(), 0);

    let (mut untouched, reads) = counting_reader(b"must not be read");
    assert!(matches!(
        prefix.detect_reader(&mut untouched),
        Err(qubit_mime::MimeError::BufferLimitExceeded { .. })
    ));
    assert_eq!(reads.get(), 0, "Prefix budget rejection must precede any input read");
}

#[derive(Debug)]
struct DetectFails(Arc<AtomicU64>);

impl MimeContentBackend for DetectFails {
    fn content_requirement(&self) -> ContentRequirement {
        ContentRequirement::Prefix(1)
    }
    fn detect_bytes(&self, _bytes: &[u8]) -> MimeResult<Vec<String>> {
        assert_eq!(
            self.0.load(Ordering::SeqCst),
            0,
            "detect_bytes must run after reader restoration"
        );
        Err(qubit_mime::MimeError::CompleteContentRequired)
    }
}

#[test]
fn test_detect_error_occurs_after_position_is_restored() {
    let position = Arc::new(AtomicU64::new(0));
    let mut reader = PositionReader::new(b"abc", Arc::clone(&position));
    assert!(matches!(
        DetectFails(Arc::clone(&position)).detect_reader(&mut reader),
        Err(qubit_mime::MimeError::CompleteContentRequired)
    ));
    assert_eq!(position.load(Ordering::SeqCst), 0);
}

#[derive(Debug)]
struct PositionReader {
    cursor: Cursor<Vec<u8>>,
    position: Arc<AtomicU64>,
}

impl PositionReader {
    fn new(bytes: &[u8], position: Arc<AtomicU64>) -> Self {
        Self {
            cursor: Cursor::new(bytes.to_vec()),
            position,
        }
    }
}
impl Read for PositionReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let count = self.cursor.read(out)?;
        self.position.store(self.cursor.position(), Ordering::SeqCst);
        Ok(count)
    }
}
impl Seek for PositionReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let position = self.cursor.seek(from)?;
        self.position.store(position, Ordering::SeqCst);
        Ok(position)
    }
}

#[derive(Debug)]
struct FailsAfterByte {
    position: u64,
    fail_restore: bool,
}

impl std::io::Read for FailsAfterByte {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.position == 0 && !out.is_empty() {
            out[0] = b'x';
            self.position = 1;
            Ok(1)
        } else {
            Err(Error::other("injected read failure"))
        }
    }
}
impl Seek for FailsAfterByte {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        match from {
            SeekFrom::Start(0) if !self.fail_restore => {
                self.position = 0;
                Ok(0)
            }
            SeekFrom::Current(0) => Ok(self.position),
            _ => Err(Error::other("injected seek failure")),
        }
    }
}

#[test]
fn test_reader_error_restores_and_restore_error_takes_priority() {
    let backend = LimitedLengthBackend(ContentRequirement::Complete, 4);
    let mut restored = FailsAfterByte {
        position: 0,
        fail_restore: false,
    };
    assert!(
        matches!(backend.detect_reader(&mut restored), Err(qubit_mime::MimeError::Io(error)) if error.to_string().contains("injected read failure"))
    );
    assert_eq!(restored.position, 0);
    let mut restore_fails = FailsAfterByte {
        position: 0,
        fail_restore: true,
    };
    assert!(
        matches!(backend.detect_reader(&mut restore_fails), Err(qubit_mime::MimeError::Io(error)) if error.to_string().contains("injected seek failure"))
    );

    let prefix_backend = LimitedLengthBackend(ContentRequirement::Prefix(1), 4);
    let mut successful_read_restore_fails = FailsAfterByte {
        position: 0,
        fail_restore: true,
    };
    assert!(matches!(
        prefix_backend.detect_reader(&mut successful_read_restore_fails),
        Err(qubit_mime::MimeError::Io(error))
            if error.to_string().contains("injected seek failure")
    ));
}

/// Complete and bounded content contracts consume the intended span and rewind
/// on success.
#[test]
fn test_content_reader_obeys_prefix_and_complete_requirements() {
    for (requirement, expected) in [
        (ContentRequirement::Prefix(2), "2"),
        (ContentRequirement::Prefix(0), "0"),
        (ContentRequirement::Complete, "5"),
    ] {
        let mut reader = Cursor::new(b"skip12345".to_vec());
        reader.set_position(4);
        let backend = LengthBackend(requirement);
        assert_eq!(
            backend.detect_reader(&mut reader).expect("read selected content"),
            vec![expected]
        );
        assert_eq!(reader.position(), 4);
    }
}

#[test]
fn test_adapter_limits_input_and_restores_reader_position() {
    let adapter = MimeDetectorAdapter::new(DirectBackendDetector::new());
    assert_eq!(adapter.backend().max_test_bytes(), 5);
    assert_eq!(adapter.content_requirement(), ContentRequirement::Prefix(5));
    let mut reader = Cursor::new(b"xxhello ignored trailer".to_vec());
    reader.set_position(2);
    assert_eq!(
        adapter.detect_reader(&mut reader).expect("prefix must be classified"),
        ["text/plain"]
    );
    assert_eq!(reader.position(), 2);
    assert!(
        adapter
            .detect_bytes(b"different")
            .expect("nonmatching input is valid")
            .is_empty()
    );
}

#[derive(Debug)]
struct InterruptedReader {
    cursor: Cursor<Vec<u8>>,
    call: usize,
    interrupt_calls: Vec<usize>,
    max_chunk: usize,
}

impl Read for InterruptedReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.call += 1;
        if self.interrupt_calls.contains(&self.call) {
            return Err(Error::from(std::io::ErrorKind::Interrupted));
        }
        let size = buffer.len().min(self.max_chunk);
        self.cursor.read(&mut buffer[..size])
    }
}

impl Seek for InterruptedReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(from)
    }
}

fn interrupted_reader(bytes: &[u8], interrupt_calls: &[usize], max_chunk: usize) -> InterruptedReader {
    InterruptedReader {
        cursor: Cursor::new(bytes.to_vec()),
        call: 0,
        interrupt_calls: interrupt_calls.to_vec(),
        max_chunk,
    }
}

#[test]
fn test_reader_retries_interrupted_reads_and_boundary_probe() {
    let prefix = LimitedLengthBackend(ContentRequirement::Prefix(3), 3);
    let mut first_read = interrupted_reader(b"abc-tail", &[1], 3);
    assert_eq!(prefix.detect_reader(&mut first_read).unwrap(), ["3"]);
    assert_eq!(first_read.cursor.position(), 0);

    let mut middle_read = interrupted_reader(b"abc-tail", &[2], 1);
    assert_eq!(prefix.detect_reader(&mut middle_read).unwrap(), ["3"]);
    assert_eq!(middle_read.cursor.position(), 0);

    let complete = LimitedLengthBackend(ContentRequirement::Complete, 3);
    let mut complete_read = interrupted_reader(b"abc", &[1, 3], 3);
    assert_eq!(complete.detect_reader(&mut complete_read).unwrap(), ["3"]);
    assert_eq!(complete_read.cursor.position(), 0);

    let mut oversized = interrupted_reader(b"abcd", &[2], 3);
    assert!(matches!(
        complete.detect_reader(&mut oversized),
        Err(qubit_mime::MimeError::BufferLimitExceeded { requested: 4, limit: 3 })
    ));
    assert_eq!(oversized.cursor.position(), 0);
}
