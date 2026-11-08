use glyim_syntax::SyntaxKind;
use smol_str::SmolStr;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenTree {
    Token(SyntaxKind, SmolStr),
    Group(SyntaxKind, Vec<TokenTree>, SyntaxKind),
    DollarCrate,
}

impl TokenTree {
    pub(crate) fn kind(&self) -> Option<SyntaxKind> {
        match self {
            TokenTree::Token(k, _) => Some(*k),
            TokenTree::Group(..) => None,
            TokenTree::DollarCrate => Some(SyntaxKind::Ident),
        }
    }

    pub(crate) fn text(&self) -> SmolStr {
        match self {
            TokenTree::Token(_, t) => t.clone(),
            TokenTree::Group(..) => SmolStr::from(""),
            TokenTree::DollarCrate => SmolStr::from("$crate"),
        }
    }
}

/// Parse a syntax node subtree into a flat vector of `TokenTrees`.
pub(crate) fn collect_token_trees(node: &glyim_syntax::SyntaxNode) -> Vec<TokenTree> {
    let mut trees = Vec::new();
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                let kind = n.kind();
                match kind {
                    SyntaxKind::TokenTree => {
                        let mut tokens = n.children_with_tokens();
                        let first = tokens.next();
                        let last = tokens.last();
                        if let (
                            Some(rowan::NodeOrToken::Token(open)),
                            Some(rowan::NodeOrToken::Token(close)),
                        ) = (first, last)
                            && is_open_delim(open.kind())
                            && is_close_delim(close.kind())
                        {
                            let inner = collect_token_trees(&n);
                            let inner_len = inner.len().saturating_sub(2);
                            let inner: Vec<_> = inner.into_iter().skip(1).take(inner_len).collect();
                            trees.push(TokenTree::Group(open.kind(), inner, close.kind()));
                            continue;
                        }
                        trees.extend(collect_token_trees(&n));
                    }
                    SyntaxKind::MacroCall => {
                        let ts = flatten_token_tree(&n);
                        trees.extend(ts);
                    }
                    _ => {
                        trees.extend(collect_token_trees(&n));
                    }
                }
            }
            rowan::NodeOrToken::Token(t) => {
                let kind = t.kind();
                if kind == SyntaxKind::Whitespace
                    || kind == SyntaxKind::LineComment
                    || kind == SyntaxKind::BlockComment
                    || kind == SyntaxKind::DocComment
                {
                    continue;
                }
                trees.push(TokenTree::Token(kind, SmolStr::from(t.text())));
            }
        }
    }
    trees
}

/// Flatten a `TokenTree` node into a flat token list.
pub(crate) fn flatten_token_tree(node: &glyim_syntax::SyntaxNode) -> Vec<TokenTree> {
    collect_token_trees(node)
}

fn is_open_delim(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LParen | SyntaxKind::LBrace | SyntaxKind::LBracket
    )
}

fn is_close_delim(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::RParen | SyntaxKind::RBrace | SyntaxKind::RBracket
    )
}
