pub mod compiler;
pub mod interpreter;
pub mod op;
pub mod value;

pub use compiler::{BytecodeCompiler, BytecodeFn};
pub use interpreter::BytecodeInterpreter;
pub use op::BytecodeOp;
pub use value::Value;

#[cfg(test)]
mod tests;
