mod lexer;
mod token;
pub use lexer::{Lexer, tokenize};
pub use token::Token;

#[cfg(test)]
mod tests;
