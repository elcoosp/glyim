use glyim_db::Database;
use glyim_pipeline::compile_file_to_mir;
fn try_compile(name: &str, src: &str) {
    let path = std::env::temp_dir().join(format!("glyim_m_{}.g", name));
    std::fs::write(&path, src).unwrap();
    let mut db = Database::new(glyim_db::CrateConfig {
        name: "t".into(), target_triple: "x86_64-apple-darwin".into(), opt_level: 0,
    });
    match compile_file_to_mir(&mut db, &path) {
        Ok(_) => eprintln!("[{}] OK", name),
        Err(ds) => { eprintln!("[{}] {} diags:", name, ds.len()); for d in &ds { eprintln!("  - {}", d.message); } }
    }
}

// Cluster A: generic-method-on-generic-struct with T return
#[test] fn a_generic_return() {
    try_compile("a1", r#"
struct W<T> { v: T }
impl<T> W<T> {
    fn get(&self) -> T { self.v }
}
fn f() -> i32 {
    let w = W { v: 0 };
    w.get()
}
"#);
}

#[test] fn a_generic_new() {
    try_compile("a2", r#"
struct W<T> { v: T }
impl<T> W<T> {
    fn new(v: T) -> W<T> { W { v } }
    fn get(&self) -> T { self.v }
}
fn f() -> i32 {
    let w = W::new(0);
    w.get()
}
"#);
}

#[test] fn a_generic_field() {
    try_compile("a3", r#"
struct W<T> { v: T }
fn f() -> i32 {
    let w = W { v: 0 };
    w.v
}
"#);
}

// Cluster C: dereference
#[test] fn c_deref_rawptr() {
    try_compile("c1", r#"
fn f(p: *const i32) -> i32 {
    unsafe { *p }
}
"#);
}

#[test] fn c_deref_ref() {
    try_compile("c2", r#"
fn f(x: &i32) -> i32 { *x }
"#);
}

#[test] fn c_deref_index() {
    try_compile("c3", r#"
fn f(buf: &mut [u8], i: usize) -> u8 {
    let mut v = 0;
    v = buf[i];
    v
}
"#);
}

// Cluster G: T::from_str
#[test] fn g_generic_assoc_fn() {
    try_compile("g1", r#"
trait FromStr { fn from_str(s: &str) -> Self; }
fn parse<T: FromStr>(s: &str) -> T { T::from_str(s) }
"#);
}

// Cluster J: field access on generic self
#[test] fn j_self_field() {
    try_compile("j1", r#"
struct S<T> { fd: T }
impl<T> S<T> {
    fn get(&self) -> T { self.fd }
}
"#);
}
