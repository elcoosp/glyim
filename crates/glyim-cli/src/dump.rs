//! Pretty-printed dump output for AST, HIR, and tokens.

use owo_colors::OwoColorize;
use std::io::Write;

/// Print a token list with colors.
pub fn dump_tokens(source: &str, out: &mut dyn Write) {
    let tokens = glyim_lex::tokenize(source);
    for tok in tokens {
        let _ = writeln!(
            out,
            "{} {}..{} {}",
            "TOK".cyan(),
            tok.start,
            tok.end,
            tok.kind.display_name().yellow()
        );
    }
}

/// Print the AST in a pretty, indented tree.
pub fn dump_ast(source: &str, interner: &glyim_interner::Interner, out: &mut dyn Write) {
    let parse_out = glyim_parse::parse(source);
    if !parse_out.errors.is_empty() {
        for e in &parse_out.errors {
            let _ = writeln!(out, "error: {e}");
        }
    }
    write_ast_items(&parse_out.ast.items, interner, 0, out);
}

fn write_ast_items(
    items: &[glyim_parse::Item],
    interner: &glyim_interner::Interner,
    indent: usize,
    out: &mut dyn Write,
) {
    for item in items {
        match item {
            glyim_parse::Item::Binding { name, value, .. } => {
                let _ = writeln!(
                    out,
                    "{:indent$}binding {} =",
                    "",
                    interner.resolve(*name),
                    indent = indent
                );
                write_expr(value, interner, indent + 2, out);
            }
            glyim_parse::Item::FnDef {
                name, params, body, ..
            } => {
                let params_str = params
                    .iter()
                    .map(|(sym, _, _, _)| interner.resolve(*sym).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "{:indent$}fn {}({})",
                    "",
                    interner.resolve(*name),
                    params_str,
                    indent = indent
                );
                write_expr(body, interner, indent + 2, out);
            }
            glyim_parse::Item::StructDef { name, fields, .. } => {
                let field_str = fields
                    .iter()
                    .map(|(sym, _, _)| interner.resolve(*sym).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    out,
                    "{:indent$}struct {} {{ {} }}",
                    "",
                    interner.resolve(*name),
                    field_str,
                    indent = indent
                );
            }
            glyim_parse::Item::EnumDef { name, variants, .. } => {
                let _ = writeln!(
                    out,
                    "{:indent$}enum {} {{",
                    "",
                    interner.resolve(*name),
                    indent = indent
                );
                for v in variants {
                    let _ = writeln!(
                        out,
                        "{:indent$}  {},",
                        "",
                        interner.resolve(v.name),
                        indent = indent
                    );
                }
                let _ = writeln!(out, "{:indent$}}}", "", indent = indent);
            }
            _ => {
                let _ = writeln!(out, "{:indent$}<item>", "", indent = indent);
            }
        }
    }
}

fn write_expr(
    expr: &glyim_parse::ExprNode,
    interner: &glyim_interner::Interner,
    indent: usize,
    out: &mut dyn Write,
) {
    match &expr.kind {
        glyim_parse::ExprKind::IntLit(n) => {
            let _ = writeln!(out, "{:indent$}{}", "", n, indent = indent);
        }
        glyim_parse::ExprKind::Ident(sym) => {
            let _ = writeln!(
                out,
                "{:indent$}{}",
                "",
                interner.resolve(*sym),
                indent = indent
            );
        }
        glyim_parse::ExprKind::Binary { op, lhs, rhs } => {
            let _ = writeln!(out, "{:indent$}Bin({:?})", "", op, indent = indent);
            write_expr(lhs, interner, indent + 2, out);
            write_expr(rhs, interner, indent + 2, out);
        }
        glyim_parse::ExprKind::Block(items) => {
            let _ = writeln!(out, "{:indent$}Block {{", "", indent = indent);
            for i in items {
                match i {
                    glyim_parse::BlockItem::Expr(e) => write_expr(e, interner, indent + 2, out),
                    glyim_parse::BlockItem::Stmt(_) => {
                        let _ = writeln!(out, "{:indent$}<stmt>", "", indent = indent + 2);
                    }
                }
            }
            let _ = writeln!(out, "{:indent$}}}", "", indent = indent);
        }
        glyim_parse::ExprKind::Lambda { body, .. } => {
            let _ = writeln!(out, "{:indent$}() =>", "", indent = indent);
            write_expr(body, interner, indent + 2, out);
        }
        _ => {
            let _ = writeln!(out, "{:indent$}<expr>", "", indent = indent);
        }
    }
}

/// Print the HIR in a pretty, indented tree.
pub fn dump_hir(source: &str, _interner: &glyim_interner::Interner, out: &mut dyn Write) {
    let parse_out = glyim_parse::parse(source);
    let mut local_interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut local_interner);
    for item in &hir.items {
        match item {
            glyim_hir::item::HirItem::Fn(f) => {
                let name = local_interner.resolve(f.name);
                let _ = writeln!(out, "HIR fn {} ({} params)", name, f.params.len());
                write_hir_expr(&f.body, &local_interner, 2, out);
            }
            glyim_hir::item::HirItem::Struct(s) => {
                let name = local_interner.resolve(s.name);
                let _ = writeln!(out, "HIR struct {}", name);
            }
            glyim_hir::item::HirItem::Enum(e) => {
                let name = local_interner.resolve(e.name);
                let _ = writeln!(out, "HIR enum {}", name);
            }
            _ => {
                let _ = writeln!(out, "HIR <item>");
            }
        }
    }
}

fn write_hir_expr(
    expr: &glyim_hir::HirExpr,
    interner: &glyim_interner::Interner,
    indent: usize,
    out: &mut dyn Write,
) {
    match expr {
        glyim_hir::HirExpr::IntLit { value, .. } => {
            let _ = writeln!(out, "{:indent$}{}", "", value, indent = indent);
        }
        glyim_hir::HirExpr::Ident { name, .. } => {
            let _ = writeln!(
                out,
                "{:indent$}{}",
                "",
                interner.resolve(*name),
                indent = indent
            );
        }
        glyim_hir::HirExpr::Binary { op, lhs, rhs, .. } => {
            let _ = writeln!(out, "{:indent$}Bin({:?})", "", op, indent = indent);
            write_hir_expr(lhs, interner, indent + 2, out);
            write_hir_expr(rhs, interner, indent + 2, out);
        }
        glyim_hir::HirExpr::Block { stmts, .. } => {
            let _ = writeln!(out, "{:indent$}Block {{", "", indent = indent);
            for stmt in stmts {
                match stmt {
                    glyim_hir::HirStmt::Let { name, value, .. } => {
                        let _ = writeln!(
                            out,
                            "{:indent$}let {}",
                            "",
                            interner.resolve(*name),
                            indent = indent + 2
                        );
                        write_hir_expr(value, interner, indent + 4, out);
                    }
                    glyim_hir::HirStmt::Expr(e) => write_hir_expr(e, interner, indent + 2, out),
                    _ => {
                        let _ = writeln!(out, "{:indent$}<stmt>", "", indent = indent + 2);
                    }
                }
            }
            let _ = writeln!(out, "{:indent$}}}", "", indent = indent);
        }
        _ => {
            let _ = writeln!(out, "{:indent$}<expr>", "", indent = indent);
        }
    }
}
