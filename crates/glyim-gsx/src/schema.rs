//! Schemas and dialects (design doc §5).
//!
//! §2 principle 2: *the compiler is host-agnostic; hosts are libraries.* The
//! passes in this crate only know **Schemas** — machine-readable descriptions
//! of one element: props + types, events, content model, a11y role,
//! invalidation class per prop, serialization form (§5.1). They never contain
//! the string `"div"`; the `web` dialect is data like any other (§5.2).
//!
//! Dialects (§5.3): a named set of schemas. The portable `core` dialect
//! (`Stack, Text, Image, Button, Toggle, Input, Scroll, List`) maps onto
//! every host; host-specific elements live in their own namespace
//! (`<web:canvas>`).

use std::collections::BTreeMap;

/// Invalidation classes a prop can affect (§5.2 `affects(...)`, G5).
///
/// "Schema-driven invalidation classes: each prop declares
/// `affects(None|Paint|Layout|Structure|A11y)`; a patch emits *one*
/// `host.invalidate(max class)` at its end" (G5). Ordering is the max-hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invalidation {
    /// The prop does not affect rendering at all.
    None,
    /// Repaint only.
    Paint,
    /// Geometry changed.
    Layout,
    /// Tree shape changed (add/remove nodes).
    Structure,
    /// Accessibility tree changed.
    A11y,
}

/// How a hole's value must be escaped — chosen at compile time from the hole
/// *position* and schema (G6: type-driven, context-aware escaping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeCtx {
    /// Plain text content (HTML entity escaping for web).
    Text,
    /// Attribute value.
    Attr,
    /// URL attribute (`href`, `src`) — percent-encoding + attr escaping.
    Url,
    /// Style attribute / style table.
    Style,
    /// Script context — never escaped automatically; requires TrustedHtml.
    Script,
}

/// Value kinds props can have — the checking-relevant part of "prop + type"
/// (§5.2 table: "prop type | typeck (checking), escaper selection (G6)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropTy {
    /// Any string-ish value.
    Str,
    /// Boolean (bare attrs and toggles).
    Bool,
    /// Signed integer.
    Int,
    /// Floating point.
    Float,
    /// A length (`style_width`).
    Length,
    /// A class list (`class`).
    ClassList,
    /// An event payload — declared via `#[event]` (§5.2).
    Event,
    /// Opaque / any.
    Any,
}

impl PropTy {
    /// Whether this type needs escaping at all (G6: "integers, bools and
    /// enums-with-identifier-variants need *no* escaping").
    pub fn needs_escaping(&self) -> bool {
        matches!(self, PropTy::Str | PropTy::ClassList | PropTy::Any)
    }
}

/// Serialization form of an element (§5.2 table: consumed by the SSR
/// emitter, G7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Serialization {
    /// Void element: `<br>` style, never has children (`<br/>` in SSR).
    pub void: bool,
    /// Boolean attrs render as `disabled` (present) / omitted (absent).
    pub boolean_attrs: bool,
    /// Self-closing allowed in source (`<input/>`).
    pub self_closing_ok: bool,
}

/// One element's schema (§5.2).
#[derive(Debug, Clone)]
pub struct ElementSchema {
    /// The tag this schema describes (`button`, `web:canvas`, core `Toggle`).
    pub tag: String,
    /// Props by name, with type + invalidation class + escape context.
    pub props: BTreeMap<String, PropSchema>,
    /// Events by name (`click` of `on:click`), §5.2 `#[event]`.
    pub events: BTreeMap<String, PropTy>,
    /// Allowed child kinds.
    pub content: ContentModel,
    /// A11y role, when the element has one (G14).
    pub role: Option<String>,
    /// Serialization form.
    pub serialization: Serialization,
    /// Whether the element is dialect-namespaced (`web:canvas` → "web").
    pub namespace: Option<String>,
}

/// One prop's schema: fields consumed by passes (§5.2 table).
#[derive(Debug, Clone)]
pub struct PropSchema {
    /// Prop type.
    pub ty: PropTy,
    /// `affects(...)` — invalidation class (G5).
    pub affects: Invalidation,
    /// Escape context for dynamic values (G6).
    pub escape: EscapeCtx,
    /// `#[a11y(required_if(no_text_child))]` — required when the element has
    /// no text child (G14).
    pub a11y_required_if_no_text_child: bool,
}

/// Allowed children (§5.2 `content = Phrasing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentModel {
    /// Anything goes (flow content).
    Any,
    /// Text and phrasing elements only (`h1`, `p`).
    Phrasing,
    /// Only list items (`ul`, `ol`).
    ListItems,
    /// No children at all (void elements).
    Empty,
}

impl ElementSchema {
    /// Look up a prop; `class:warn`-style sub-attrs check the parent name.
    pub fn prop(&self, name: &str) -> Option<&PropSchema> {
        self.props.get(name)
    }

    /// Look up an event (`click` of `on:click`).
    pub fn event(&self, name: &str) -> Option<&PropTy> {
        self.events.get(name)
    }

    /// Check a child against the content model; returns `None` when valid.
    pub fn child_violation(&self, child: &ChildShape) -> Option<String> {
        match self.content {
            ContentModel::Any => None,
            ContentModel::Empty => Some(format!(
                "`{}` is a void element and cannot have children",
                self.tag
            )),
            ContentModel::Phrasing => match child {
                ChildShape::Text | ChildShape::Element("h1") | ChildShape::Element("span")
                | ChildShape::Element("p") | ChildShape::Element("output")
                | ChildShape::Element("input") | ChildShape::Element("button") => None,
                ChildShape::Element(other) => {
                    Some(format!("`{other}` is not phrasing content inside `{}`", self.tag))
                }
            },
            ContentModel::ListItems => match child {
                ChildShape::Element("li") => None,
                ChildShape::Text => None,
                ChildShape::Element(other) => {
                    Some(format!("`{other}` is not a list item inside `{}`", self.tag))
                }
            },
        }
    }
}

/// The shape of one child, for content-model checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildShape {
    /// Text child.
    Text,
    /// An element with a tag name (intrinsic or core element).
    Element(&'static str),
}

/// A dialect: a named set of schemas (§5.1).
#[derive(Debug, Clone, Default)]
pub struct Dialect {
    /// Dialect name (`web`, `core`, `tui`, …).
    pub name: String,
    /// Schemas by tag.
    pub elements: BTreeMap<String, ElementSchema>,
}

impl Dialect {
    /// An empty dialect.
    pub fn new(name: &str) -> Dialect {
        Dialect {
            name: name.to_string(),
            elements: BTreeMap::new(),
        }
    }

    /// Register one schema.
    pub fn register(&mut self, schema: ElementSchema) {
        self.elements.insert(schema.tag.clone(), schema);
    }

    /// Look up an element, honoring the namespace (`web:canvas`).
    pub fn element(&self, namespace: Option<&str>, tag: &str) -> Option<&ElementSchema> {
        if let Some(ns) = namespace {
            self.elements.get(&format!("{ns}:{tag}"))
        } else {
            self.elements.get(tag)
        }
    }
}

/// The registry of active dialects: `core` is always present; host dialects
/// are libraries plugged in per program (§5.3).
#[derive(Debug, Clone, Default)]
pub struct SchemaRegistry {
    /// Dialects by name.
    pub dialects: BTreeMap<String, Dialect>,
}

impl SchemaRegistry {
    /// A registry with the portable `core` dialect and the `web` dialect.
    pub fn standard() -> SchemaRegistry {
        let mut r = SchemaRegistry::default();
        r.dialects.insert("core".to_string(), core_dialect());
        r.dialects.insert("web".to_string(), web_dialect());
        r
    }

    /// Register a dialect.
    pub fn register(&mut self, d: Dialect) {
        self.dialects.insert(d.name.clone(), d);
    }

    /// Resolve an element across dialects: dialect-qualified tags look in
    /// their own dialect first, then `core`, then the default.
    pub fn resolve(
        &self,
        default_dialect: &str,
        namespace: Option<&str>,
        tag: &str,
    ) -> Option<&ElementSchema> {
        if let Some(ns) = namespace {
            if let Some(d) = self.dialects.get(ns)
                && let Some(s) = d.element(Some(ns), tag) {
                    return Some(s);
                }
            return None;
        }
        self.dialects
            .get(default_dialect)
            .and_then(|d| d.element(None, tag))
            .or_else(|| self.dialects.get("core").and_then(|d| d.element(None, tag)))
    }
}

/// Helper: one prop.
fn prop(ty: PropTy, affects: Invalidation, escape: EscapeCtx) -> PropSchema {
    PropSchema {
        ty,
        affects,
        escape,
        a11y_required_if_no_text_child: false,
    }
}

/// §5.3: the portable `core` dialect — a library written in `core` renders
/// anywhere.
pub fn core_dialect() -> Dialect {
    let mut d = Dialect::new("core");
    for tag in ["Stack", "Text", "Image", "Button", "Toggle", "Input", "Scroll", "List"] {
        let mut s = ElementSchema {
            tag: tag.to_string(),
            props: BTreeMap::new(),
            events: BTreeMap::new(),
            content: ContentModel::Any,
            role: None,
            serialization: Serialization {
                void: false,
                boolean_attrs: true,
                self_closing_ok: true,
            },
            namespace: None,
        };
        s.props.insert("label".to_string(), prop(PropTy::Str, Invalidation::Paint, EscapeCtx::Text));
        match tag {
            "Button" => {
                s.events.insert("click".to_string(), PropTy::Event);
                s.role = Some("Button".to_string());
            }
            "Toggle" => {
                s.props.insert("checked".to_string(), prop(PropTy::Bool, Invalidation::Structure, EscapeCtx::Text));
                s.events.insert("change".to_string(), PropTy::Event);
            }
            "Input" => {
                s.props.insert("value".to_string(), prop(PropTy::Str, Invalidation::Paint, EscapeCtx::Text));
                s.events.insert("input".to_string(), PropTy::Event);
            }
            "Stack" => {
                s.props.insert("axis".to_string(), prop(PropTy::Str, Invalidation::Layout, EscapeCtx::Attr));
                s.props.insert("gap".to_string(), prop(PropTy::Int, Invalidation::Layout, EscapeCtx::Attr));
            }
            _ => {}
        }
        d.register(s);
    }
    d
}

/// §5.2: the `web` dialect, seeded with the exact `Button` schema from the
/// design doc (prop → affects class, event, a11y rule) plus the intrinsics
/// the §3.1 Counter example needs.
///
/// Big dialects (full HTML5) should be *generated from data* (§5.2); this
/// hand-written subset is the seed that generation replaces.
pub fn web_dialect() -> Dialect {
    let mut d = Dialect::new("web");

    // §5.2's Button, verbatim in spirit:
    //   class  #[affects(Paint)]
    //   disabled #[affects(Structure, A11y)]
    //   style_width #[affects(Layout)]
    //   #[event] click
    //   #[a11y(required_if(no_text_child))] aria_label
    let mut button = ElementSchema {
        tag: "button".to_string(),
        props: BTreeMap::new(),
        events: BTreeMap::new(),
        content: ContentModel::Phrasing,
        role: Some("Button".to_string()),
        serialization: Serialization {
            void: false,
            boolean_attrs: true,
            self_closing_ok: false,
        },
        namespace: None,
    };
    button.props.insert(
        "class".to_string(),
        prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr),
    );
    button.props.insert(
        "disabled".to_string(),
        prop(PropTy::Bool, Invalidation::Structure, EscapeCtx::Text),
    );
    button.props.insert(
        "style_width".to_string(),
        prop(PropTy::Length, Invalidation::Layout, EscapeCtx::Style),
    );
    button.events.insert("click".to_string(), PropTy::Event);
    let mut aria = prop(PropTy::Str, Invalidation::A11y, EscapeCtx::Attr);
    aria.a11y_required_if_no_text_child = true;
    button.props.insert("aria_label".to_string(), aria);
    d.register(button);

    // The §3.1 Counter example's other intrinsics.
    let section = ElementSchema {
        tag: "section".to_string(),
        props: BTreeMap::from([
            ("class".to_string(), prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr)),
            ("aria_label".to_string(), prop(PropTy::Str, Invalidation::A11y, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::Any,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(section);

    let h1 = ElementSchema {
        tag: "h1".to_string(),
        props: BTreeMap::from([
            ("class".to_string(), prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::Phrasing,
        role: Some("Heading".to_string()),
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(h1);

    let output = ElementSchema {
        tag: "output".to_string(),
        props: BTreeMap::from([
            ("class".to_string(), prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::Phrasing,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(output);

    let input = ElementSchema {
        tag: "input".to_string(),
        props: BTreeMap::from([
            ("value".to_string(), prop(PropTy::Str, Invalidation::Paint, EscapeCtx::Text)),
            ("placeholder".to_string(), prop(PropTy::Str, Invalidation::Paint, EscapeCtx::Attr)),
            ("disabled".to_string(), prop(PropTy::Bool, Invalidation::Structure, EscapeCtx::Text)),
            ("type".to_string(), prop(PropTy::Str, Invalidation::Structure, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::from([("input".to_string(), PropTy::Event)]),
        content: ContentModel::Empty,
        role: None,
        serialization: Serialization { void: true, boolean_attrs: true, self_closing_ok: true },
        namespace: None,
    };
    d.register(input);

    let ul = ElementSchema {
        tag: "ul".to_string(),
        props: BTreeMap::from([
            ("class".to_string(), prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::ListItems,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(ul);

    let li = ElementSchema {
        tag: "li".to_string(),
        props: BTreeMap::new(),
        events: BTreeMap::new(),
        content: ContentModel::Phrasing,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(li);

    let p = ElementSchema {
        tag: "p".to_string(),
        props: BTreeMap::from([
            ("class".to_string(), prop(PropTy::ClassList, Invalidation::Paint, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::Phrasing,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: true, self_closing_ok: false },
        namespace: None,
    };
    d.register(p);

    // The dialect-namespace escape hatch (§3.2): `<web:canvas/>`.
    let canvas = ElementSchema {
        tag: "web:canvas".to_string(),
        props: BTreeMap::from([
            ("width".to_string(), prop(PropTy::Int, Invalidation::Layout, EscapeCtx::Attr)),
            ("height".to_string(), prop(PropTy::Int, Invalidation::Layout, EscapeCtx::Attr)),
        ]),
        events: BTreeMap::new(),
        content: ContentModel::Empty,
        role: None,
        serialization: Serialization { void: false, boolean_attrs: false, self_closing_ok: true },
        namespace: Some("web".to_string()),
    };
    d.register(canvas);

    d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.2: the Button schema's fields are consumed by exactly the passes
    /// the doc's table says: affects → invalidation, event → handler typing,
    /// a11y → lint, serialization → SSR.
    #[test]
    fn button_schema_matches_design_doc() {
        let r = SchemaRegistry::standard();
        let b = r.resolve("web", None, "button").expect("button schema");
        // class affects Paint; disabled affects Structure; style_width Layout.
        assert_eq!(b.prop("class").unwrap().affects, Invalidation::Paint);
        assert_eq!(b.prop("disabled").unwrap().affects, Invalidation::Structure);
        assert_eq!(b.prop("style_width").unwrap().affects, Invalidation::Layout);
        // #[event] click
        assert!(b.event("click").is_some());
        assert!(b.event("hover").is_none());
        // #[a11y(required_if(no_text_child))] aria_label
        assert!(b.prop("aria_label").unwrap().a11y_required_if_no_text_child);
        // role = Button
        assert_eq!(b.role.as_deref(), Some("Button"));
    }

    /// §5.1: "The compiler passes only know Schemas. They never contain the
    /// string 'div'" — resolution goes through the registry, so an unknown
    /// tag is a *data lookup miss*, not a hardcoded name.
    #[test]
    fn unknown_tag_is_a_data_miss() {
        let r = SchemaRegistry::standard();
        assert!(r.resolve("web", None, "fancypants").is_none());
        assert!(r.resolve("web", None, "button").is_some());
        assert!(r.resolve("web", None, "Stack").is_some()); // core fallback (§5.3)
    }

    /// §3.2: dialect-namespaced tags resolve through their own dialect only.
    #[test]
    fn dialect_namespaced_resolution() {
        let r = SchemaRegistry::standard();
        let c = r.resolve("web", Some("web"), "canvas").expect("canvas");
        assert_eq!(c.namespace.as_deref(), Some("web"));
        // `native:window` has no schema anywhere yet.
        assert!(r.resolve("web", Some("native"), "window").is_none());
    }

    /// G5: the max-hierarchy orders invalidation classes for the single
    /// `host.invalidate(max)` call at the end of a patch.
    #[test]
    fn invalidation_max_hierarchy() {
        let ordered = [
            Invalidation::None,
            Invalidation::Paint,
            Invalidation::Layout,
            Invalidation::Structure,
            Invalidation::A11y,
        ];
        for w in ordered.windows(2) {
            assert!(w[0] < w[1], "{:?} must sort before {:?}", w[0], w[1]);
        }
        assert_eq!(
            [Invalidation::Paint, Invalidation::A11y]
                .into_iter()
                .max()
                .unwrap(),
            Invalidation::A11y
        );
    }

    /// G6: integers and bools need no escaping; strings do.
    #[test]
    fn escaping_by_type() {
        assert!(!PropTy::Int.needs_escaping());
        assert!(!PropTy::Bool.needs_escaping());
        assert!(PropTy::Str.needs_escaping());
        assert!(PropTy::ClassList.needs_escaping());
    }

    /// Content models: `ul` only takes `li`; `input` is void.
    #[test]
    fn content_models() {
        let r = SchemaRegistry::standard();
        let ul = r.resolve("web", None, "ul").unwrap();
        assert!(ul.child_violation(&ChildShape::Element("li")).is_none());
        assert!(ul
            .child_violation(&ChildShape::Element("div"))
            .is_some());
        let input = r.resolve("web", None, "input").unwrap();
        assert_eq!(input.content, ContentModel::Empty);
        assert!(input.serialization.void);
        // §5.2: boolean attrs are part of the serialization form.
        assert!(input.serialization.boolean_attrs);
    }

    /// §5.3: core dialect ports — `Stack`/`Toggle` are reachable from the web
    /// default without a namespace (a `core` library renders anywhere).
    #[test]
    fn core_dialect_portability() {
        let r = SchemaRegistry::standard();
        let stack = r.resolve("web", None, "Stack").expect("core Stack");
        assert_eq!(stack.prop("gap").unwrap().affects, Invalidation::Layout);
        let toggle = r.resolve("web", None, "Toggle").expect("core Toggle");
        assert!(toggle.event("change").is_some());
    }

    /// §5.2: "Big dialects should be generated from data" — the registry is
    /// data-driven; adding a dialect is registering schemas, not code changes
    /// in the passes.
    #[test]
    fn dialects_are_data() {
        let mut r = SchemaRegistry::standard();
        let mut tui = Dialect::new("tui");
        tui.register(ElementSchema {
            tag: "box".to_string(),
            props: BTreeMap::new(),
            events: BTreeMap::new(),
            content: ContentModel::Any,
            role: None,
            serialization: Serialization { void: false, boolean_attrs: false, self_closing_ok: true },
            namespace: None,
        });
        r.register(tui);
        assert!(r.resolve("tui", None, "box").is_some());
        // And the same passes work over it unchanged.
        assert!(r.resolve("tui", None, "box").unwrap().child_violation(&ChildShape::Text).is_none());
    }
}
