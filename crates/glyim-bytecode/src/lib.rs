pub mod op;
pub mod value;
pub mod compiler;
pub mod interpreter;

pub use op::BytecodeOp;
pub use value::Value;
pub use compiler::{BytecodeCompiler, BytecodeFn};
pub use interpreter::BytecodeInterpreter;

#[cfg(test)]
mod tests;
