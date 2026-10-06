use super::token_tree::TokenTree;
use glyim_syntax::SyntaxKind;
use smol_str::SmolStr;
use std::collections::HashMap;

/// Substitute metavariables (`$x`) in `template` using `bindings`.
///
/// Returns the expanded token stream, or `Err(name)` if a metavariable used in
/// the template is not bound. §19.3: an unbound metavariable is a hard error
/// (it used to be silently dropped), because a `$(...)*` repetition tied to a
/// metavar that never matched produces an expansion that cannot type-check.
///
/// HIR-2: bindings are *depth-aware* — each metavariable maps to one
/// `Vec<TokenTree>` **per matched iteration** of the repetition that binds it,
/// not to one flat token list. The outer length is therefore the repetition
/// count; before this fix it counted captured *tokens*, so a multi-token
/// fragment such as `1 + 2` split into three bogus iterations.
pub(crate) fn substitute(
    template: &[TokenTree],
    bindings: &HashMap<SmolStr, Vec<Vec<TokenTree>>>,
) -> Result<Vec<TokenTree>, SmolStr> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < template.len() {
        let tree = &template[i];
        match tree {
            TokenTree::Token(SyntaxKind::Dollar, _) => {
                i += 1;
                if i >= template.len() {
                    result.push(TokenTree::Token(SyntaxKind::Dollar, SmolStr::from("$")));
                    break;
                }
                match &template[i] {
                    TokenTree::Token(SyntaxKind::Ident, name) => {
                        let text = name.as_str();
                        if text == "crate" {
                            result.push(TokenTree::DollarCrate);
                        } else if let Some(iterations) = bindings.get(name) {
                            // Outside a repetition each metavar has exactly one
                            // iteration; splice its tokens verbatim.
                            for iteration in iterations {
                                result.extend(iteration.iter().cloned());
                            }
                        } else {
                            // §19.3: an unbound metavariable must be a hard error.
                            return Err(name.clone());
                        }
                        i += 1;
                    }
                    TokenTree::Group(SyntaxKind::LParen, inner, SyntaxKind::RParen) => {
                        i += 1;
                        let separator = if i < template.len()
                            && !matches!(
                                &template[i],
                                TokenTree::Token(
                                    SyntaxKind::Star | SyntaxKind::Plus | SyntaxKind::Question,
                                    _
                                )
                            ) {
                            let sep = template[i].clone();
                            i += 1;
                            Some(sep)
                        } else {
                            None
                        };
                        if i >= template.len() {
                            break;
                        }
                        let rep_kind = match &template[i] {
                            TokenTree::Token(SyntaxKind::Star, _) => RepKind::ZeroOrMore,
                            TokenTree::Token(SyntaxKind::Plus, _) => RepKind::OneOrMore,
                            TokenTree::Token(SyntaxKind::Question, _) => RepKind::ZeroOrOne,
                            _ => break,
                        };
                        i += 1;

                        // Find all metavariable names in the inner pattern.
                        let var_names = find_all_metavars(inner);
                        // T082-PATCHED [MAC-3]: for `*`/`+` repetitions, any
                        // metavariable inside the body that was never matched
                        // by the pattern is a hard error (previously the
                        // `filter_map` below silently dropped it, expanding
                        // the whole repetition to nothing). For `?`
                        // repetitions the metavariable may legitimately be
                        // unbound — an *optional* tail like `$(, $x:expr)?`
                        // doesn't bind `$x` when the caller omits it, and the
                        // correct expansion is zero iterations.
                        if !matches!(rep_kind, RepKind::ZeroOrOne) {
                            for name in &var_names {
                                if !bindings.contains_key(name) {
                                    return Err(SmolStr::from(format!(
                                        "unbound metavariable `${}` inside a repetition",
                                        name
                                    )));
                                }
                            }
                        }
                        // HIR-2: each metavar maps to one entry *per matched
                        // iteration*, so the outer length IS the repetition
                        // count.
                        let repetitions: usize = var_names
                            .iter()
                            .filter_map(|name| bindings.get(name).map(|v| v.len()))
                            .max()
                            .unwrap_or(0);

                        match rep_kind {
                            RepKind::ZeroOrMore | RepKind::OneOrMore => {
                                for rep_idx in 0..repetitions {
                                    if rep_idx > 0
                                        && let Some(ref sep) = separator
                                    {
                                        result.push(sep.clone());
                                    }
                                    let rep_bindings =
                                        extract_repetition_bindings(bindings, &var_names, rep_idx);
                                    let subbed = substitute(inner, &rep_bindings)?;
                                    result.extend(subbed);
                                }
                            }
                            RepKind::ZeroOrOne => {
                                if repetitions > 0 {
                                    let rep_bindings =
                                        extract_repetition_bindings(bindings, &var_names, 0);
                                    let subbed = substitute(inner, &rep_bindings)?;
                                    result.extend(subbed);
                                }
                            }
                        }
                    }
                    _ => {
                        result.push(TokenTree::Token(SyntaxKind::Dollar, SmolStr::from("$")));
                        result.push(template[i].clone());
                        i += 1;
                    }
                }
            }
            TokenTree::Group(open, inner, close) => {
                let subbed_inner = substitute(inner, bindings)?;
                result.push(TokenTree::Group(*open, subbed_inner, *close));
                i += 1;
            }
            other => {
                result.push(other.clone());
                i += 1;
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug)]
enum RepKind {
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
}

fn find_all_metavars(trees: &[TokenTree]) -> Vec<SmolStr> {
    // HIR-3: recurse into delimiter groups so metavars nested in a
    // repetition body (`$( ... $x ... )*`) or any group (`$(foo($x)),*`) are
    // found. The previous flat scan only saw `$name` at the top level, so a
    // nested `$( $( $x ),* ),*` collected no names and expanded to nothing.
    fn walk(trees: &[TokenTree], names: &mut Vec<SmolStr>) {
        let mut i = 0;
        while i < trees.len() {
            match &trees[i] {
                TokenTree::Token(SyntaxKind::Dollar, _) if i + 1 < trees.len() => {
                    match &trees[i + 1] {
                        TokenTree::Token(SyntaxKind::Ident, name) => {
                            names.push(name.clone());
                            i += 2;
                            continue;
                        }
                        TokenTree::Group(_, inner, _) => {
                            walk(inner, names);
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
                TokenTree::Group(_, inner, _) => walk(inner, names),
                _ => {}
            }
            i += 1;
        }
    }
    let mut names = Vec::new();
    walk(trees, &mut names);
    names
}

/// Re-wrap iteration `index` of each captured metavar into a depth-0 binding
/// map so the recursive `substitute` call for one repetition body splices
/// exactly that iteration's tokens.
fn extract_repetition_bindings(
    bindings: &HashMap<SmolStr, Vec<Vec<TokenTree>>>,
    var_names: &[SmolStr],
    index: usize,
) -> HashMap<SmolStr, Vec<Vec<TokenTree>>> {
    let mut result = HashMap::new();
    for name in var_names {
        if let Some(iterations) = bindings.get(name)
            && index < iterations.len()
        {
            // One outer entry = one "iteration" of the body being expanded.
            result.insert(name.clone(), vec![iterations[index].clone()]);
        }
    }
    result
}

#[cfg(test)]
mod hir3_tests {
    use super::*;

    fn tok(k: SyntaxKind, t: &str) -> TokenTree {
        TokenTree::Token(k, SmolStr::from(t))
    }
    fn group(inner: Vec<TokenTree>) -> TokenTree {
        TokenTree::Group(SyntaxKind::LParen, inner, SyntaxKind::RParen)
    }

    /// HIR-3: a metavar nested inside a delimiter group must be found. The old
    /// flat scan only saw `$name` at the top level, so `$( $( $x ),* ),*`
    /// collected no names and expanded to nothing.
    #[test]
    fn find_all_metavars_recurses_into_groups() {
        // Template shape: `$( wrap($x) ),*` — the `$x` is inside `wrap(...)`.
        let wrap = vec![
            tok(SyntaxKind::Ident, "wrap"),
            group(vec![
                tok(SyntaxKind::Dollar, "$"),
                tok(SyntaxKind::Ident, "x"),
            ]),
        ];
        let names = find_all_metavars(&wrap);
        assert_eq!(
            names,
            vec![SmolStr::from("x")],
            "metavar inside a group must be found; got {names:?}"
        );
    }

    /// HIR-3: a doubly-nested repetition `$( $( $x ),* ),*` finds `$x`.
    #[test]
    fn find_all_metavars_finds_deeply_nested() {
        let inner_rep = vec![
            tok(SyntaxKind::Dollar, "$"),
            group(vec![
                tok(SyntaxKind::Dollar, "$"),
                tok(SyntaxKind::Ident, "x"),
            ]),
            tok(SyntaxKind::Star, "*"),
        ];
        let names = find_all_metavars(&inner_rep);
        assert_eq!(names, vec![SmolStr::from("x")]);
    }

    /// HIR-2: a repetition over a *multi-token* fragment splices the whole
    /// fragment once per iteration. Before the depth-aware binding fix the
    /// outer length counted captured tokens, so this produced six statements.
    #[test]
    fn repetition_splices_multi_token_fragments_per_iteration() {
        // Template: `$( let _ = $e; )*`
        let inner = vec![
            tok(SyntaxKind::KwLet, "let"),
            tok(SyntaxKind::Ident, "_"),
            tok(SyntaxKind::Eq, "="),
            tok(SyntaxKind::Dollar, "$"),
            tok(SyntaxKind::Ident, "e"),
            tok(SyntaxKind::Semicolon, ";"),
        ];
        let template = vec![
            tok(SyntaxKind::Dollar, "$"),
            group(inner),
            tok(SyntaxKind::Star, "*"),
        ];

        // Two iterations, each a multi-token `$e`.
        let mut bindings: HashMap<SmolStr, Vec<Vec<TokenTree>>> = HashMap::new();
        bindings.insert(
            SmolStr::from("e"),
            vec![
                vec![
                    tok(SyntaxKind::Ident, "aa"),
                    tok(SyntaxKind::Plus, "+"),
                    tok(SyntaxKind::Ident, "bb"),
                ],
                vec![
                    tok(SyntaxKind::Ident, "cc"),
                    tok(SyntaxKind::Star, "*"),
                    tok(SyntaxKind::Ident, "dd"),
                ],
            ],
        );

        let out = substitute(&template, &bindings).expect("substitution succeeds");
        let lets = out
            .iter()
            .filter(|t| matches!(t, TokenTree::Token(SyntaxKind::KwLet, _)))
            .count();
        assert_eq!(lets, 2, "one `let` per iteration, got {}", lets);
        // And the whole fragment is spliced intact.
        assert!(
            out.iter()
                .any(|t| matches!(t, TokenTree::Token(SyntaxKind::Plus, _)))
        );
        assert!(
            out.iter()
                .any(|t| matches!(t, TokenTree::Token(SyntaxKind::Star, _)))
        );
    }
}
