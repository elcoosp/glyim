//! SCRATCH repros for ISSUE-0003 triage. Prints diagnostics, never fails.
use glyim_span::FileId;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use glyim_test::mock::MockCodegen;
use std::sync::Arc;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, FileId::from_raw(1), &[])
}

fn show(tag: &str, src: &str) {
    let out = compile(src);
        eprintln!("=== {tag}: {} diagnostic(s)", out.diagnostics.len());
    for d in &out.diagnostics {
        eprintln!("  - [{}] {} @ {:?}", d.code, d.message, d.span);
    }
}

#[test]
fn zz_issue3_repros() {
    // A: &str.into() -> Error via From/Into
    show(
        "A into-str-to-Error",
        r#"
        struct Error { msg: String }
        impl From<&str> for Error { fn from(s: &str) -> Error { Error { msg: s.to_string() } } }
        fn f() -> Result<i32, Error> { Result::Err("boom".into()) }
        "#,
    );
    // B: method call on generic type param R: Read
    show(
        "B generic-param method",
        r#"
        trait Read { fn read(&mut self, buf: &mut [u8]) -> i32; }
        struct S { x: i32 }
        impl Read for S { fn read(&mut self, buf: &mut [u8]) -> i32 { 1 } }
        fn copy<R: Read>(reader: &mut R) -> i32 {
            let mut buf = [0u8; 8];
            reader.read(&mut buf)
        }
        "#,
    );
    // C: &String -> &str deref coercion on return
    show(
        "C deref-coerce String->str",
        r#"
        struct DirEntry { path: String }
        impl DirEntry {
            fn path(&self) -> &str { &self.path }
        }
        "#,
    );
    // D: array == comparison
    show(
        "D array-eq",
        r#"
        struct A { segments: [u16; 8] }
        impl A {
            fn is_unspecified(&self) -> bool { self.segments == [0, 0, 0, 0, 0, 0, 0, 0] }
        }
        "#,
    );
    // E: to_string on &str
    show(
        "E to_string",
        r#"
        fn f(path: &str) -> String { path.to_string() }
        "#,
    );
    // F: str len/as_ptr via &str
    show(
        "F str-methods",
        r#"
        extern "C" { fn ext(p: *const u8, n: usize) -> i32; }
        fn f(path: &str) -> i32 { unsafe { ext(path.as_ptr(), path.len()) } }
        "#,
    );
    // G: generic impl block with bound
    show(
        "G generic-impl",
        r#"
        trait Read { fn read(&mut self) -> i32; }
        struct BufReader<R> { inner: R }
        impl<R: Read> BufReader<R> {
            fn fill(&mut self) -> i32 { self.inner.read() }
        }
        "#,
    );
    // H: File::open ? + file.metadata()
    show(
        "H open-question-metadata",
        r#"
        struct Metadata { len: u64 }
        struct File { fd: i32 }
        impl File {
            fn open(path: &str) -> Result<File, Error> { Result::Ok(File { fd: 1 }) }
            fn metadata(&self) -> Result<Metadata, Error> { Result::Ok(Metadata { len: 1 }) }
        }
        struct Error;
        fn metadata(path: &str) -> Result<Metadata, Error> {
            let file = File::open(path)?;
            file.metadata()
        }
        "#,
    );
    // H2: ? alone, no let
    show(
        "H2 question-only",
        r#"
        struct Error;
        fn maybe() -> Result<i32, Error> { Result::Ok(1) }
        fn f() -> Result<i32, Error> {
            let x = maybe()?;
            Result::Ok(x)
        }
        "#,
    );
    // H3: let without ?, then method call
    show(
        "H3 no-question",
        r#"
        struct Metadata { len: u64 }
        struct File { fd: i32 }
        impl File {
            fn open(path: &str) -> Result<File, Error> { Result::Ok(File { fd: 1 }) }
            fn metadata(&self) -> Result<Metadata, Error> { Result::Ok(Metadata { len: 1 }) }
        }
        struct Error;
        fn metadata(path: &str) -> Result<Metadata, Error> {
            let file = File::open(path);
            match file {
                Result::Ok(f) => f.metadata(),
                Result::Err(e) => Result::Err(e),
            }
        }
        "#,
    );
    // I: slice range index &buf[a..b]
    show(
        "I slice-range",
        r#"
        fn f(buf: &[u8], pos: usize, cap: usize) -> &[u8] { &buf[pos..cap] }
        "#,
    );
    // I2: copy_from_slice
    show(
        "I2 copy-from-slice",
        r#"
        fn f(dst: &mut [u8], src: &[u8]) { dst.copy_from_slice(src); }
        "#,
    );
    // K: match with ref binding + guard
    show(
        "K match-ref-guard",
        r#"
        enum Kind { A, B }
        struct Error { k: Kind }
        impl Error { fn kind(&self) -> Kind { Kind::A } }
        fn f(r: Result<i32, Error>) -> i32 {
            match r {
                Result::Ok(n) => n,
                Result::Err(ref e) if true => 0,
                Result::Err(e) => 1,
            }
        }
        "#,
    );
    // R1: format! with impl Display param
    show(
        "R1 format-impl-display",
        r#"
        trait Display { fn fmt(&self) -> String; }
        trait Write { fn write_all(&mut self, buf: &[u8]) -> i32; fn write_fmt(&mut self, fmt: impl Display) -> i32; }
        struct W;
        impl Write for W {
            fn write_all(&mut self, buf: &[u8]) -> i32 { 1 }
            fn write_fmt(&mut self, fmt: impl Display) -> i32 {
                let s = format!("{}", fmt);
                self.write_all(s.as_bytes())
            }
        }
        "#,
    );
    // R2: map_err with wildcard closure
    show(
        "R2 map-err-wildcard",
        r#"
        struct Error;
        impl Error { fn new(s: String) -> Error { Error } }
        fn f(r: Result<i32, i32>) -> Result<i32, Error> {
            r.map_err(|_| Error::new("bad".to_string()))
        }
        "#,
    );
    // R3: DirEntry shape
    show(
        "R3 direntry",
        r#"
        struct Metadata { len: u64 }
        struct DirEntry { path: String }
        fn free_metadata(path: &str) -> Result<Metadata, Error> { Result::Ok(Metadata { len: 1 }) }
        struct Error;
        impl DirEntry {
            fn path(&self) -> &str { &self.path }
            fn metadata(&self) -> Result<Metadata, Error> { free_metadata(&self.path) }
        }
        "#,
    );
    // R5: array eq with u16 elements
    show(
        "R5 array-eq-u16",
        r#"
        struct A { segments: [u16; 8] }
        impl A {
            fn is_unspecified(&self) -> bool { self.segments == [0u16, 0u16, 0u16, 0u16, 0u16, 0u16, 0u16, 0u16] }
        }
        "#,
    );
    // R6: BufReader::read shape (min + range index + copy_from_slice)
    show(
        "R6 bufread-read",
        r#"
        fn min(a: usize, b: usize) -> usize { a }
        struct BufReader { buf: Vec<u8>, pos: usize, cap: usize }
        impl BufReader {
            fn read(&mut self, buf: &mut [u8]) -> usize {
                let n = min(buf.len(), self.cap - self.pos);
                buf[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
                self.pos += n;
                n
            }
        }
        "#,
    );
    // S1: Vec field + self read, no ranges
    show(
        "S1 vec-self",
        r#"
        struct BufReader { buf: Vec<u8>, pos: usize, cap: usize }
        impl BufReader {
            fn f(&mut self) -> usize { self.cap - self.pos }
        }
        "#,
    );
    // S2: plus-assign on field
    show(
        "S2 plus-assign",
        r#"
        struct B { pos: usize }
        impl B {
            fn f(&mut self, n: usize) { self.pos += n; }
        }
        "#,
    );
    // S3: range index into Vec field
    show(
        "S3 range-index-vec",
        r#"
        struct B { buf: Vec<u8> }
        impl B {
            fn f(&self, a: usize, b: usize) -> &[u8] { &self.buf[a..b] }
        }
        "#,
    );
    // S4: copy_from_slice with range args, free fns
    show(
        "S4 copy-ranges",
        r#"
        fn f(dst: &mut [u8], src: &[u8], n: usize) { dst[..n].copy_from_slice(&src[..n]); }
        "#,
    );
    // T1: Vec::with_capacity + as_mut_ptr/capacity/set_len + from_utf8
    show(
        "T1 vec-ffi",
        r#"
        extern "C" { fn ext(out: *mut u8, cap: usize) -> isize; }
        fn f() -> String {
            let mut buf = Vec::with_capacity(4096);
            let n = unsafe { ext(buf.as_mut_ptr(), buf.capacity()) };
            unsafe { buf.set_len(n as usize); }
            String::from_utf8(buf).unwrap_or_default()
        }
        "#,
    );
    // T2: turbofish struct literal + generic box fns
    show(
        "T2 turbofish-struct",
        r#"
        struct Slot<T> { result: Option<T> }
        fn f<T>() -> Slot<T> { Slot::<T> { result: Option::None } }
        "#,
    );
    // T3: str::from_utf8 + map_err + ?
    show(
        "T3 from-utf8",
        r#"
        struct Error;
        impl Error { fn new(s: String) -> Error { Error } }
        fn f(bytes: &[u8]) -> Result<String, Error> {
            let s = str::from_utf8(bytes).map_err(|_| Error::new("bad".to_string()))?;
            Result::Ok(s.to_string())
        }
        "#,
    );
    // T4: .expect on Option and Result
    show(
        "T4 expect",
        r#"
        fn f(o: Option<i32>, r: Result<i32, i32>) -> i32 {
            let a = o.expect("none");
            let b = r.expect("err");
            a + b
        }
        "#,
    );
    // T5: Option take
    show(
        "T5 take",
        r#"
        struct P { f: Option<i32> }
        fn f(mut p: P) -> Option<i32> { p.f.take() }
        "#,
    );
    // U1: field access through Box
    show(
        "U1 box-field",
        r#"
        struct Payload { slot: i32, f: i32 }
        fn f(payload: Box<Payload>) -> i32 { payload.slot }
        "#,
    );
    // U2: deref Box
    show(
        "U2 box-deref",
        r#"
        struct Slot { result: Option<i32> }
        fn f(b: Box<Slot>) -> Slot { *b }
        "#,
    );
    // U3: field assign through Box + match on field
    show(
        "U3 box-field-assign-match",
        r#"
        struct Slot { result: Option<i32> }
        fn f(mut slot: Box<Slot>, v: i32) -> i32 {
            slot.result = Option::Some(v);
            match slot.result {
                Option::Some(x) => x,
                Option::None => 0,
            }
        }
        "#,
    );
    // U4: field through &-reference (DirEntry::path shape needs &self auto-deref too)
    show(
        "U4 ref-field",
        r#"
        struct DirEntry { path: String }
        impl DirEntry {
            fn path(&self) -> &String { &self.path }
        }
        "#,
    );
}

#[test]
fn zz_nested_async_diag() {
    show(
        "nested-async",
        r#"
        enum Poll<T> { Ready(T), Pending }
        trait Future {
            type Output;
            fn poll(&mut self) -> Poll<Self::Output>;
        }
        fn block_on<F: Future>(mut f: F) -> F::Output {
            loop {
                match f.poll() {
                    Poll::Ready(v) => return v,
                    Poll::Pending => { }
                }
            }
        }
        async fn dep(x: i32) -> i32 { x }
        async fn nested(a: i32) -> i32 { let x = dep(a).await; x + 1 }
        fn main() -> i32 {
            let f = nested(5);
            block_on(f)
        }
    "#,
    );
        assert!(true);
}

