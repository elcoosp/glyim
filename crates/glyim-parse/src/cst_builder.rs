use rowan::api::Language;
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use crate::ParseError;

pub struct CstBuilder {
    builder: rowan::GreenNodeBuilder<'static>,
    errors: Vec<ParseError>,
}

impl CstBuilder {
    pub fn new() -> Self { Self { builder: rowan::GreenNodeBuilder::new(), errors: vec![] } }
    pub fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(glyim_syntax::GlyimLang::kind_to_raw(kind));
    }
    pub fn token(&mut self, kind: SyntaxKind, text: &str) {
        self.builder.token(glyim_syntax::GlyimLang::kind_to_raw(kind), text);
    }
    pub fn finish_node(&mut self) { self.builder.finish_node(); }
    pub fn error(&mut self, err: ParseError) { self.errors.push(err); }
    pub fn error_node(&mut self, tokens: &[(SyntaxKind, &str)], err: ParseError) {
        self.start_node(SyntaxKind::Error);
        for (k,t) in tokens { self.token(*k,t); }
        self.finish_node();
        self.errors.push(err);
    }
    pub fn finish(self) -> (GreenNode, Vec<ParseError>) {
        let green = self.builder.finish();
        (green, self.errors)
    }
}

pub fn green_to_syntax(green: GreenNode) -> SyntaxNode { SyntaxNode::new_root(green) }
