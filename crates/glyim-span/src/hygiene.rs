use crate::{ExpnId, HygieneKey, Span, SyntaxContext, Transparency};
use glyim_core::interner::Name;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Mark.
pub struct Mark {
    /// Struct.
    pub expn_id: ExpnId,
    /// Struct.
    pub transparency: Transparency,
}

#[derive(Clone, Debug)]
/// ExpnData.
pub struct ExpnData {
    /// Struct.
    pub expn_id: ExpnId,
    /// Struct.
    pub parent: ExpnId,
    /// Struct.
    pub kind: ExpnKind,
    /// Struct.
    pub call_site: Span,
    /// Struct.
    pub def_site: Span,
    /// Struct.
    pub transparency: Transparency,
}

#[derive(Clone, Debug)]
/// ExpnKind.
pub enum ExpnKind {
    /// Variant.
    MacroRules {
        /// name field.
        name: Name,
    },
    /// Variant.
    ProcMacro {
        /// name field.
        name: Name,
    },
    /// Variant.
    Builtin {
        /// name field.
        name: Name,
    },
    /// Variant.
    Root,
}

#[derive(Clone, Debug)]
struct SyntaxContextData {
    outer_expn: ExpnId,
    outer_transparency: Transparency,
    parent: SyntaxContext,
}

#[derive(Clone)]
/// HygieneCtx.
pub struct HygieneCtx {
    expansions: Vec<ExpnData>,
    next_expn_id: u32,
    syntax_contexts: Vec<SyntaxContextData>,
    next_syntax_context: u32,
    key: HygieneKey,
}

impl HygieneCtx {
    /// new.
    pub fn new() -> Self {
        Self {
            expansions: vec![ExpnData {
                expn_id: ExpnId::ROOT,
                parent: ExpnId::ROOT,
                kind: ExpnKind::Root,
                call_site: Span::DUMMY,
                def_site: Span::DUMMY,
                transparency: Transparency::Opaque,
            }],
            next_expn_id: 1,
            syntax_contexts: Vec::new(),
            next_syntax_context: 1,
            key: HygieneKey::new(),
        }
    }

    /// push_expansion.
    pub fn push_expansion(&mut self, mut data: ExpnData) -> ExpnId {
        let raw_id = self.next_expn_id;
        self.next_expn_id += 1;
        let id = ExpnId::from_hygiene_key(self.key, raw_id);
        data.expn_id = id;
        self.expansions.push(data);
        id
    }

    /// apply_mark.
    pub fn apply_mark(&mut self, span: Span, mark: Mark) -> Span {
        let new_ctx = SyntaxContext::from_hygiene_key(self.key, self.next_syntax_context);
        self.next_syntax_context += 1;
        self.syntax_contexts.push(SyntaxContextData {
            outer_expn: mark.expn_id,
            outer_transparency: mark.transparency,
            parent: span.ctx,
        });
        Span::new(span.file, span.lo, span.hi, new_ctx)
    }

    /// remove_mark.
    pub fn remove_mark(&self, span: Span) -> (Span, Option<Mark>) {
        if span.ctx.is_root() {
            return (span, None);
        }
        let idx = span.ctx.to_raw() as usize - 1;
        if let Some(ctx_data) = self.syntax_contexts.get(idx) {
            let mark = Mark {
                expn_id: ctx_data.outer_expn,
                transparency: ctx_data.outer_transparency,
            };
            (
                Span::new(span.file, span.lo, span.hi, ctx_data.parent),
                Some(mark),
            )
        } else {
            (span, None)
        }
    }

    /// expn_data.
    pub fn expn_data(&self, id: ExpnId) -> Option<&ExpnData> {
        self.expansions.get(id.to_raw() as usize)
    }

    /// Plan §2.1: expose the syntax-context id carried by a span so the
    /// resolver (`glyim-def-map`) can consult hygiene when two identifiers
    /// share text but differ in syntax context (a macro-introduced `let tmp`
    /// must not capture a use-site `tmp`). `SyntaxContext` *is* the id.
    pub fn syntax_context(&self, span: Span) -> SyntaxContext {
        span.ctx
    }

    /// adjust.
    pub fn adjust(&mut self, span: Span, scope_ctx: SyntaxContext) -> Span {
        let mut current = span;
        while current.ctx != scope_ctx && !current.ctx.is_root() {
            let (next, _) = self.remove_mark(current);
            current = next;
        }
        current
    }
}

impl Default for HygieneCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxContext {
    pub(crate) fn from_hygiene_key(_key: HygieneKey, raw: u32) -> Self {
        let _ = _key;
        SyntaxContext(raw)
    }
}

impl ExpnId {
    pub(crate) fn from_hygiene_key(_key: HygieneKey, raw: u32) -> Self {
        let _ = _key;
        ExpnId(raw)
    }
}
