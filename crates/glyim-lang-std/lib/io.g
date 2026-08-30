//! I/O primitives for the Glyim standard library.
//!
//! The `std::io` module contains a number of common things you'll need
//! when doing input and output. The core traits are `Read` and `Write`,

/// The `Read` trait allows for reading bytes from a source.
trait Read {
    /// Pull some bytes from this source into the specified buffer.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;

    /// Read the exact number of bytes required to fill `buf`.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.read(&mut buf[filled..]) {
                Result::Ok(0) => {
                    return Result::Err(Error::new(ErrorKind::UnexpectedEof, "failed to fill whole buffer"));
                }
                Result::Ok(n) => {
                    filled += n;
                }
                Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => {
                    continue;
                }
                Result::Err(e) => {
                    return Result::Err(e);
                }
            }
        }
        Result::Ok(())
    }

    /// Read all bytes until EOF in this source, placing them into `buf`.
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize, Error> {
        // FFI: backed by OS read syscalls
        let mut total = 0;
        let mut tmp = [0u8; 4096];
        loop {
            match self.read(&mut tmp) {
                Result::Ok(0) => break,
                Result::Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    total += n;
                }
                Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Result::Err(e) => return Result::Err(e),
            }
        }
        Result::Ok(total)
    }

    /// Read all bytes until EOF in this source, appending them to `buf`.
    fn read_to_string(&mut self, buf: &mut String) -> Result<usize, Error> {
        let mut bytes = Vec::new();
        let n = self.read_to_end(&mut bytes)?;
        // SAFETY: bytes should be valid UTF-8
        buf.push_str(str::from_utf8(&bytes).unwrap_or(""));
        Result::Ok(n)
    }
}

/// The `Write` trait allows for writing bytes to a destination.
trait Write {
    /// Write a buffer into this writer, returning how many bytes were written.
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error>;

    /// Flush this output stream, ensuring that all intermediately buffered contents
    /// reach their destination.
    fn flush(&mut self) -> Result<(), Error>;

    /// Attempts to write an entire buffer into this writer.
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        let mut written = 0;
        while written < buf.len() {
            match self.write(&buf[written..]) {
                Result::Ok(0) => {
                    return Result::Err(Error::new(ErrorKind::WriteZero, "failed to write whole buffer"));
                }
                Result::Ok(n) => written += n,
                Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Result::Err(e) => return Result::Err(e),
            }
        }
        Result::Ok(())
    }

    /// Writes a formatted string into this writer, returning any error encountered.
    fn write_fmt(&mut self, fmt: impl Display) -> Result<(), Error> {
        let s = format!("{}", fmt);
        self.write_all(s.as_bytes())
    }
}

/// The `BufRead` trait adds buffered reading to any `Read` implementation.
trait BufRead: Read {
    /// Return the contents of the internal buffer, filling it with more data
    /// from the inner reader if it is empty.
    fn fill_buf(&mut self) -> Result<&[u8], Error>;

    /// Tell this buffer that `amt` bytes have been consumed from the buffer,
    /// so they should no longer be returned in calls to `read`.
    fn consume(&mut self, amt: usize);

    /// Read all bytes until EOF, appending them to `buf`.
    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize, Error> {
        let mut total = 0;
        loop {
            let (done, used) = {
                let available = self.fill_buf()?;
                if available.is_empty() {
                    break;
                }
                match memchr(byte, available) {
                    Option::Some(i) => {
                        buf.extend_from_slice(&available[..=i]);
                        (true, i + 1)
                    }
                    Option::None => {
                        buf.extend_from_slice(available);
                        (false, available.len())
                    }
                }
            };
            self.consume(used);
            total += used;
            if done {
                break;
            }
        }
        Result::Ok(total)
    }

    /// Read all bytes until a newline (the `0xA` byte) is reached.
    fn read_line(&mut self, buf: &mut String) -> Result<usize, Error> {
        let mut bytes = Vec::new();
        let n = self.read_until(b'\n', &mut bytes)?;
        buf.push_str(str::from_utf8(&bytes).unwrap_or(""));
        Result::Ok(n)
    }
}

/// A handle to the global standard input stream.
struct Stdin {
    // FFI: backed by stdin file descriptor
    _fd: i32,
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        extern "C" {
            fn glyim_stdin_read(fd: i32, buf: *mut u8, len: usize) -> isize;
        }
        // SAFETY: buf is valid for len bytes, fd is stdin
        let n = unsafe { glyim_stdin_read(self._fd, buf.as_mut_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    fn flush(&mut self) -> Result<(), Error> {
        Result::Ok(())
    }
}

/// A handle to the global standard output stream.
struct Stdout {
    _fd: i32,
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        extern "C" {
            fn glyim_stdout_write(fd: i32, buf: *const u8, len: usize) -> isize;
        }
        // SAFETY: buf is valid for len bytes, fd is stdout
        let n = unsafe { glyim_stdout_write(self._fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    fn flush(&mut self) -> Result<(), Error> {
        extern "C" {
            fn glyim_stdout_flush(fd: i32) -> i32;
        }
        let rc = unsafe { glyim_stdout_flush(self._fd) };
        if rc < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(())
        }
    }
}

/// A handle to the global standard error stream.
struct Stderr {
    _fd: i32,
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        extern "C" {
            fn glyim_stderr_write(fd: i32, buf: *const u8, len: usize) -> isize;
        }
        // SAFETY: buf is valid for len bytes, fd is stderr
        let n = unsafe { glyim_stderr_write(self._fd, buf.as_ptr(), buf.len()) };
        if n < 0 {
            Result::Err(Error::last_os_error())
        } else {
            Result::Ok(n as usize)
        }
    }

    fn flush(&mut self) -> Result<(), Error> {
        Result::Ok(())
    }
}

/// Constructs a new handle to the global standard input stream.
fn stdin() -> Stdin {
    Stdin { _fd: 0 }
}

/// Constructs a new handle to the global standard output stream.
fn stdout() -> Stdout {
    Stdout { _fd: 1 }
}

/// Constructs a new handle to the global standard error stream.
fn stderr() -> Stderr {
    Stderr { _fd: 2 }
}

/// The `Error` type for I/O operations.
struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    /// Create a new I/O error from a kind and message.
    fn new(kind: ErrorKind, msg: impl Into<String>) -> Error {
        Error {
            kind,
            message: msg.into(),
        }
    }

    /// Create a new I/O error from the last OS error.
    fn last_os_error() -> Error {
        extern "C" {
            fn glyim_errno() -> i32;
        }
        let errno = unsafe { glyim_errno() };
        Error {
            kind: ErrorKind::from_raw_os_error(errno),
            message: format!("OS error: {}", errno),
        }
    }

    /// Return the kind of this error.
    fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return a reference to the error message.
    fn message(&self) -> &str {
        &self.message
    }
}

/// A specialized `Result` type for I/O operations.
type Result<T> = Result<T, Error>;

/// An enumeration of possible errors that can occur during I/O operations.
enum ErrorKind {
    /// An entity was not found, often a file.
    NotFound,
    /// The operation lacked the necessary privileges to complete.
    PermissionDenied,
    /// The connection was refused by the remote server.
    ConnectionRefused,
    /// The connection was reset by the remote server.
    ConnectionReset,
    /// The connection was aborted (terminated) by the remote server.
    ConnectionAborted,
    /// The network operation failed because it was not connected yet.
    NotConnected,
    /// A socket address could not be bound because the address is already in use.
    AddrInUse,
    /// A nonexistent interface was requested or the requested address was not local.
    AddrNotAvailable,
    /// The system's networking is down.
    NetworkDown,
    /// The operation failed because a pipe was closed.
    BrokenPipe,
    /// An operation could not be completed, because it failed to allocate enough memory.
    OutOfMemory,
    /// The operation was interrupted.
    Interrupted,
    /// Any I/O error not part of this list.
    Other,
    /// An error returned when an operation could not be completed because a call to `write` returned `Ok(0)`.
    WriteZero,
    /// An error returned when an operation could not be completed because an end of file was reached prematurely.
    UnexpectedEof,
    /// An operation could not complete because the I/O resource (socket, pipe,
    /// etc.) is not ready yet and the call would block. The caller should retry
    /// after the reactor signals readiness (see `std::task`).
    WouldBlock,
    /// Any I/O error from the operating system.
    OsError(i32),
}

impl ErrorKind {
    /// Create an `ErrorKind` from a raw OS error number.
    fn from_raw_os_error(code: i32) -> ErrorKind {
        // EWOULDBLOCK / EAGAIN (35 on macOS, 11 on Linux) signal a
        // non-blocking I/O resource that is not yet ready — the async reactor
        // retries the operation once it reports readiness.
        if code == 35 || code == 11 {
            ErrorKind::WouldBlock
        } else {
            ErrorKind::OsError(code)
        }
    }
}

/// A buffer for reading and writing to/from an I/O stream.
struct BufReader<R: Read> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

impl<R: Read> BufReader<R> {
    /// Create a new `BufReader` with a default buffer capacity.
    fn new(inner: R) -> BufReader<R> {
        BufReader::with_capacity(8192, inner)
    }

    /// Create a new `BufReader` with the specified buffer capacity.
    fn with_capacity(capacity: usize, inner: R) -> BufReader<R> {
        BufReader {
            inner,
            buf: Vec::with_capacity(capacity),
            pos: 0,
            cap: 0,
        }
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if self.pos == self.cap {
            self.fill_buf()?;
        }
        let n = min(buf.len(), self.cap - self.pos);
        buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Result::Ok(n)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8], Error> {
        if self.pos == self.cap {
            self.buf.clear();
            let n = self.inner.read(&mut self.buf)?;
            self.cap = n;
            self.pos = 0;
        }
        Result::Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = min(self.pos + amt, self.cap);
    }
}

/// Wraps a writer and buffers its output.
struct BufWriter<W: Write> {
    inner: W,
    buf: Vec<u8>,
}

impl<W: Write> BufWriter<W> {
    /// Create a new `BufWriter` with a default buffer capacity.
    fn new(inner: W) -> BufWriter<W> {
        BufWriter::with_capacity(8192, inner)
    }

    /// Create a new `BufWriter` with the specified buffer capacity.
    fn with_capacity(capacity: usize, inner: W) -> BufWriter<W> {
        BufWriter {
            inner,
            buf: Vec::with_capacity(capacity),
        }
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        if self.buf.len() + buf.len() > self.buf.capacity() {
            self.flush()?;
        }
        if buf.len() >= self.buf.capacity() {
            self.inner.write(buf)
        } else {
            self.buf.extend_from_slice(buf);
            Result::Ok(buf.len())
        }
    }

    fn flush(&mut self) -> Result<(), Error> {
        if !self.buf.is_empty() {
            self.inner.write_all(&self.buf)?;
            self.buf.clear();
        }
        self.inner.flush()
    }
}

/// Copy the entire contents of a reader into a writer.
fn copy<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64, Error> {
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        match reader.read(&mut buf) {
            Result::Ok(0) => break,
            Result::Ok(n) => {
                writer.write_all(&buf[..n])?;
                total += n as u64;
            }
            Result::Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Result::Err(e) => return Result::Err(e),
        }
    }
    Result::Ok(total)
}

/// Print to standard output, with a newline.
macro println! {
    () => {
        let mut out = stdout();
        out.write_all(b"\n").unwrap();
    },
    ($fmt:literal) => {
        let mut out = stdout();
        out.write_all(format!(concat!($fmt, "\n")).as_bytes()).unwrap();
    },
    ($fmt:literal, $($arg:tt)+) => {
        let mut out = stdout();
        out.write_all(format!(concat!($fmt, "\n"), $($arg)+).as_bytes()).unwrap();
    },
}

/// Print to standard output.
macro print! {
    ($fmt:literal) => {
        let mut out = stdout();
        out.write_all(format!($fmt).as_bytes()).unwrap();
    },
    ($fmt:literal, $($arg:tt)+) => {
        let mut out = stdout();
        out.write_all(format!($fmt, $($arg)+).as_bytes()).unwrap();
    },
}

/// Print to standard error, with a newline.
macro eprintln! {
    () => {
        let mut out = stderr();
        out.write_all(b"\n").unwrap();
    },
    ($fmt:literal) => {
        let mut out = stderr();
        out.write_all(format!(concat!($fmt, "\n")).as_bytes()).unwrap();
    },
    ($fmt:literal, $($arg:tt)+) => {
        let mut out = stderr();
        out.write_all(format!(concat!($fmt, "\n"), $($arg)+).as_bytes()).unwrap();
    },
}

/// Print to standard error.
macro eprint! {
    ($fmt:literal) => {
        let mut out = stderr();
        out.write_all(format!($fmt).as_bytes()).unwrap();
    },
    ($fmt:literal, $($arg:tt)+) => {
        let mut out = stderr();
        out.write_all(format!($fmt, $($arg)+).as_bytes()).unwrap();
    },
}

/// Empty struct representing an empty buffer for read/write.
struct Empty;

impl Read for Empty {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Error> {
        Result::Ok(0)
    }
}

/// A reader which is always at EOF.
fn empty() -> Empty {
    Empty
}

/// A writer which consumes all data.
struct Sink;

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        Result::Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Error> {
        Result::Ok(())
    }
}

/// A writer which consumes all data, discarding it.
fn sink() -> Sink {
    Sink
}

/// Repeat a single byte infinitely.
struct Repeat {
    byte: u8,
}

impl Read for Repeat {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        for b in buf.iter_mut() {
            *b = self.byte;
        }
        Result::Ok(buf.len())
    }
}

/// Create a reader that infinitely repeats a single byte.
fn repeat(byte: u8) -> Repeat {
    Repeat { byte }
}
