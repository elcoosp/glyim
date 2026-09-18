use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use glyim_test::mock::MockCodegen;
use std::sync::Arc;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, glyim_span::FileId::from_raw(1), &[])
}

fn main() {
    let src = r#"
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
    "#;
    let output = compile(src);
    for diag in &output.diagnostics {
        println!("{:?}", diag);
    }
}