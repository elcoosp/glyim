use crate::parser::tokens::Tokens;
use glyim_syntax::SyntaxKind;

pub(crate) fn recover(tokens: &mut Tokens) {
    loop {
        match tokens.peek() {
            None => break,
            Some(tok)
                if matches!(
                    tok.kind,
                    SyntaxKind::KwFn
                        | SyntaxKind::KwLet
                        | SyntaxKind::KwStruct
                        | SyntaxKind::KwEnum
                        | SyntaxKind::KwImpl
                        | SyntaxKind::Eof
                ) =>
            {
                break
            }
            Some(tok) if tok.kind == SyntaxKind::RBrace => {
                tokens.bump();
                break;
            }
            _ => {
                tokens.bump();
            }
        }
    }
}
