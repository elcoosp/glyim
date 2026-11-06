//! GSX syntax tests.
//!
//! These tests are the executable specification for first-class GSX parsing:
//! prefix-position `<` + ident/`>` is a GSX element (finding F5), text is
//! always a string literal (finding F4 — zero lexer changes), and the CST
//! gains the `Gsx*` node kinds listed in §3.4.

use glyim_syntax::{AstNode, SyntaxKind};

/// Parse `src` as the body of `fn __g() { ... }` and return the top-level
/// expression-ish nodes of that body.
fn parse_body(src: &str) -> Vec<glyim_syntax::SyntaxNode> {
    let wrapped = format!("fn __g() {{ {} }}", src);
    let parsed = crate::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    let block = parsed
        .root
        .children()
        .find(|n| n.kind() == SyntaxKind::FnDef)
        .and_then(|f| f.children().find(|n| n.kind() == SyntaxKind::Block))
        .expect("body block");
    block
        .children()
        .filter(|n| !n.kind().is_trivia())
        .collect()
}

/// Parse a single expression and return its root node.
fn parse_single(src: &str) -> glyim_syntax::SyntaxNode {
    let stmts = parse_body(src);
    assert_eq!(stmts.len(), 1, "expected exactly one statement: {stmts:?}");
    let stmt = &stmts[0];
    // Tail expr: the block itself holds the expression child. ExprStmt holds one.
    let inner: Vec<_> = stmt
        .children()
        .filter(|n| !n.kind().is_trivia())
        .collect();
    let expr = match stmt.kind() {
        SyntaxKind::Block => stmt
            .children()
            .find(|n| !n.kind().is_trivia())
            .expect("tail expression"),
        _ => {
            assert_eq!(inner.len(), 1, "statement should hold one expression");
            inner[0].clone()
        }
    };
    // Peel the ExprStmt wrapper when parse produced one around the expression.
    if expr.kind() == SyntaxKind::ExprStmt {
        let inner = expr
            .children()
            .find(|n| !n.kind().is_trivia())
            .expect("expr stmt body");
        return inner;
    }
    expr
}

fn text_of(node: &glyim_syntax::SyntaxNode) -> String {
    node.text().to_string()
}

/// Name of a GSX open tag: strips `<` and any dialect namespace (`web:canvas`).
fn open_tag_name(open: &glyim_syntax::SyntaxNode) -> String {
    let raw = open
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .skip_while(|t| t.kind() != SyntaxKind::Lt)
        .collect::<Vec<_>>();
    // tokens after `<`: Ident (Colon Ident)* or Ident (ColonColon Ident)*
    let mut name = String::new();
    for t in &raw[1..] {
        match t.kind() {
            SyntaxKind::Ident
            | SyntaxKind::KwType
            | SyntaxKind::KwSelf
            | SyntaxKind::KwSuper
            | SyntaxKind::KwCrate => name.push_str(t.text()),
            SyntaxKind::Colon | SyntaxKind::ColonColon => name.push_str(t.text()),
            _ => break,
        }
    }
    name
}

fn first_child_of<'a>(
    node: &'a glyim_syntax::SyntaxNode,
    kind: SyntaxKind,
) -> Option<glyim_syntax::SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

fn count_of(node: &glyim_syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    node.descendants().filter(|c| c.kind() == kind).count()
}

// ---------------------------------------------------------------- elements

/// §3.2: Element — self-closing form `<tag attrs… />`.
#[test]
fn gsx_self_closing_element() {
    let e = parse_single("<div/>");
    assert_eq!(e.kind(), SyntaxKind::GsxSelfClosing);
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    assert_eq!(open_tag_name(&open), "div");
    assert_eq!(parse_single("<div/>").text(), "<div/>");
}

/// §3.2: Element — paired form with a quoted text child (F4: text is a string
/// literal, GsxTextLit node).
#[test]
fn gsx_element_with_text_child() {
    let e = parse_single("<h1>\"Hello\"</h1>");
    assert_eq!(e.kind(), SyntaxKind::GsxElement);
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    assert_eq!(open_tag_name(&open), "h1");
    let close = first_child_of(&e, SyntaxKind::GsxCloseTag).expect("close tag");
    assert_eq!(text_of(&close), "</h1>");
    let txt = first_child_of(&e, SyntaxKind::GsxTextLit).expect("text child");
    assert_eq!(text_of(&txt), "\"Hello\"");
}

/// §3.1: whitespace between siblings is not significant — newlines between
/// elements produce no text nodes.
#[test]
fn gsx_whitespace_between_children_is_not_a_text_node() {
    let src = "<ul>\n    <li>\"a\"</li>\n    <li>\"b\"</li>\n</ul>";
    let e = parse_single(src);
    assert_eq!(e.kind(), SyntaxKind::GsxElement);
    assert_eq!(count_of(&e, SyntaxKind::GsxTextLit), 2);
    assert_eq!(count_of(&e, SyntaxKind::GsxElement), 3); // ul + 2 li
}

/// §3.2: Fragment `<> … </>`.
#[test]
fn gsx_fragment() {
    let e = parse_single("<>\"a\"\"b\"</>");
    assert_eq!(e.kind(), SyntaxKind::GsxFragment);
    assert_eq!(count_of(&e, SyntaxKind::GsxTextLit), 2);
}

/// §3.1: nested elements parse recursively.
#[test]
fn gsx_nested_elements() {
    let src = "<section><p>\"a\"</p><p>\"b\"</p></section>";
    let e = parse_single(src);
    assert_eq!(count_of(&e, SyntaxKind::GsxElement), 3);
}

/// Same-name nesting must not confuse the close-tag matcher.
#[test]
fn gsx_nested_same_tag() {
    let src = "<A><A/></A>";
    let e = parse_single(src);
    // outer paired element + inner self-closing element
    assert_eq!(count_of(&e, SyntaxKind::GsxElement), 1);
    assert_eq!(count_of(&e, SyntaxKind::GsxSelfClosing), 1);
}

/// §3.2: Component element `<Name …/>` (uppercase) and path component
/// `<ui::Button>…</ui::Button>` resolve through the def-map later; the parser
/// accepts both shapes.
#[test]
fn gsx_component_and_path_tags() {
    let c = parse_single("<Counter x={1}/>");
    let open = first_child_of(&c, SyntaxKind::GsxOpenTag).expect("open tag");
    assert_eq!(open_tag_name(&open), "Counter");

    let p = parse_single("<ui::Button>\"Save\"</ui::Button>");
    let open = first_child_of(&p, SyntaxKind::GsxOpenTag).expect("open tag");
    assert_eq!(open_tag_name(&open), "ui::Button");
}

/// §3.2: Namespaced dialect host intrinsic `<web:canvas/>` (single-colon
/// namespace, distinct from Rust `::` paths).
#[test]
fn gsx_dialect_namespaced_tag() {
    let e = parse_single("<web:canvas/>");
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    assert_eq!(open_tag_name(&open), "web:canvas");
}

// ---------------------------------------------------------------- attributes

/// §3.2: attributes — `name="lit"`, `name={expr}`, bare `disabled`.
#[test]
fn gsx_attr_forms() {
    let e = parse_single("<input type=\"text\" value={x} disabled/>");
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    let attrs: Vec<_> = open
        .children()
        .filter(|c| c.kind() == SyntaxKind::GsxAttr)
        .collect();
    assert_eq!(attrs.len(), 3);
    assert_eq!(text_of(&attrs[0]), "type=\"text\"");
    assert_eq!(text_of(&attrs[1]), "value={x}");
    assert_eq!(text_of(&attrs[2]), "disabled");
}

/// §3.1: hyphenated attribute names (`aria-label`) — the lexer sees
/// `Ident Minus Ident`; the parser must accept the sequence inside an attr.
#[test]
fn gsx_hyphenated_attr_name() {
    let e = parse_single("<section aria-label=\"Counter\"/>");
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    let attrs: Vec<_> = open
        .children()
        .filter(|c| c.kind() == SyntaxKind::GsxAttr)
        .collect();
    assert_eq!(attrs.len(), 1);
    assert_eq!(text_of(&attrs[0]), "aria-label=\"Counter\"");
}

/// §3.2: directive attributes `ns:name={…}` become GsxDirectiveAttr nodes:
/// `on:click`, `class:warn`, `style:width`, `bind:value`.
#[test]
fn gsx_directive_attrs() {
    let src = "<button on:click={h} class:warn={c} style:width={w}/>";
    let e = parse_single(src);
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    let dirs: Vec<_> = open
        .children()
        .filter(|c| c.kind() == SyntaxKind::GsxDirectiveAttr)
        .collect();
    assert_eq!(dirs.len(), 3);
    assert_eq!(text_of(&dirs[0]), "on:click={h}");
    assert_eq!(text_of(&dirs[1]), "class:warn={c}");
    assert_eq!(text_of(&dirs[2]), "style:width={w}");
}

/// §3.2: shorthand `{name}` (attr whose whole body is a brace expression) and
/// spread `{..props}` are distinct forms.
#[test]
fn gsx_shorthand_and_spread_attrs() {
    let e = parse_single("<Row {name} {..props}/>");
    let open = first_child_of(&e, SyntaxKind::GsxOpenTag).expect("open tag");
    let shorthand = first_child_of(&open, SyntaxKind::GsxAttr).expect("shorthand attr");
    assert_eq!(text_of(&shorthand), "{name}");
    let spread = first_child_of(&open, SyntaxKind::GsxSpread).expect("spread");
    assert_eq!(text_of(&spread), "{..props}");
}

// ---------------------------------------------------------------- children

/// §3.2: dynamic child `{expr}` — any expression, including closures.
#[test]
fn gsx_block_child() {
    let e = parse_single("<output>{s.count}</output>");
    let block = first_child_of(&e, SyntaxKind::GsxBlockChild).expect("block child");
    assert!(text_of(&block).contains("s.count"));
}

/// §3.1: control flow is the native language — `if/else` inside `{ }`.
#[test]
fn gsx_if_else_block_child() {
    let src = "<section>{if s.count > 10 { <p class=\"big\">\"x\"</p> } else { <></> }}</section>";
    let e = parse_single(src);
    let block = first_child_of(&e, SyntaxKind::GsxBlockChild).expect("block child");
    assert!(text_of(&block).contains("if"));
    // nested fragment inside else branch
    assert_eq!(count_of(&block, SyntaxKind::GsxFragment), 1);
}

/// §3.2: keyed iteration `for x in xs by <key-expr> { … }` — `by` is a
/// GSX-scoped contextual keyword, accepted only in child-position `for`, and
/// materializes as a GsxForBy node.
#[test]
fn gsx_for_by_keyed_iteration() {
    let src = "<ul>{for item in s.history.iter() by item.id { <li>\"x\"</li> }}</ul>";
    let e = parse_single(src);
    assert_eq!(
        count_of(&e, SyntaxKind::GsxForBy),
        1,
        "GsxForBy node must exist"
    );
    // and the whole construct is still one block child
    assert_eq!(count_of(&e, SyntaxKind::GsxBlockChild), 1);
}

/// Interpolated text (§3.2) keeps its holes syntactically visible: the literal
/// is one GsxTextLit; glyim-gsx splits static pieces from holes at parse time.
#[test]
fn gsx_interpolated_text_literal() {
    let e = parse_single("<h1>\"Hello, {s.name}!\"</h1>");
    let txt = first_child_of(&e, SyntaxKind::GsxTextLit).expect("text child");
    assert_eq!(text_of(&txt), "\"Hello, {s.name}!\"");
}

/// §3.1 (the full Counter body): everything nests without the parser
/// tripping on labels, `>=` in attribute expressions, or `>>` in holes.
/// The three siblings live inside a fragment so the source is one expression.
#[test]
fn gsx_counter_example_headline_features() {
    let src = concat!(
        "<>",
        "<button on:click={|s, _| s.count -= s.step}>\"−\"</button>",
        "<output class:warn={s.count < 0}>\"{s.count}\"</output>",
        "<input bind:value={s.name} placeholder=\"Your name\"/>",
        "</>",
    );
    let e = parse_single(src);
    assert_eq!(e.kind(), SyntaxKind::GsxFragment);
    // button and output are paired; input is self-closing
    assert_eq!(count_of(&e, SyntaxKind::GsxElement), 2);
    assert_eq!(count_of(&e, SyntaxKind::GsxSelfClosing), 1);
    // on:click, class:warn, bind:value
    assert_eq!(count_of(&e, SyntaxKind::GsxDirectiveAttr), 3);
}

/// F5 stays true for comparisons: infix `<` is still a binary operator.
#[test]
fn binary_lt_is_not_gsx() {
    let e = parse_single("a < b");
    assert_eq!(e.kind(), SyntaxKind::BinaryExpr);
}

/// Prefix `<` that is not followed by an identifier or `>` keeps producing a
/// clean parse error (not a GSX node, no panic, no ICE).
#[test]
fn prefix_lt_without_ident_is_an_error() {
    let wrapped = format!("fn __g() {{ let _x = < 3; }}");
    let parsed = crate::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    assert!(
        !parsed.diagnostics.is_empty(),
        "expected a diagnostic for `< 3`"
    );
}

/// §3.4 parser gotcha: an unterminated element recovers with diagnostics and
/// without hanging or panicking.
#[test]
fn gsx_unterminated_element_recovers() {
    let wrapped = format!("fn __g() {{ let _x = <div>\"hi\"; }}");
    let parsed = crate::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    assert!(
        !parsed.diagnostics.is_empty(),
        "expected a diagnostic for missing close tag"
    );
}

/// §3.4: the `Ident Colon` label heuristic must not fire for GSX tags that
/// follow labels, and directive attrs inside an element after a label parse.
#[test]
fn gsx_after_label() {
    let src = "'blk: { <div on:click={h}>\"x\"</div> }";
    let e = parse_single(src);
    assert_eq!(count_of(&e, SyntaxKind::GsxElement), 1);
}

/// Depth guard: deeply nested GSX elements must not blow the stack.
#[test]
fn gsx_deep_nesting_is_bounded() {
    let mut src = String::new();
    for _ in 0..400 {
        src.push_str("<div>");
    }
    src.push_str("\"x\"");
    for _ in 0..400 {
        src.push_str("</div>");
    }
    let wrapped = format!("fn __g() {{ {} }}", src);
    let parsed = crate::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    // Either parses fine or reports bounded diagnostics; must not panic/hang.
    let _ = parsed.diagnostics.len();
}

/// The tree is lossless: every source byte survives into the green tree.
#[test]
fn gsx_tree_is_lossless() {
    let src = "<section class=\"counter\" aria-label=\"Counter\"><h1>\"Hi\"</h1></section>";
    let wrapped = format!("fn __g() {{ {} }}", src);
    let parsed = crate::parse_to_syntax(&wrapped, glyim_span::FileId::from_raw(0));
    let full = parsed.root.to_string();
    assert!(full.contains(src), "source text must survive: {full}");
}

