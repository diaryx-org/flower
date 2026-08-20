//! The **page** projection: one level of the document at a time.
//!
//! [`tree`](crate::tree) answers "what does the whole document look like?" — every
//! visible node, indented by depth. That is the right shape for a small config and
//! the wrong one for a deep config, where the useful levels drift right until the
//! keys no longer fit and every screen is mostly ancestors you already know about.
//!
//! A page answers a narrower question: "what is *in* this container?" It lists one
//! container's children and nothing below them, so depth costs a navigation step
//! instead of a column of indentation, and a document nested twelve deep renders
//! exactly as wide as one nested twice. It is the model behind a settings menu —
//! a stable list of categories, and a page you push into and pop back out of.
//!
//! ## Inline vs. drill
//!
//! Listing one level mechanically would be a poor settings menu: a two-key group
//! would cost a whole page to show two lines, and you would spend the interaction
//! budget on containers rather than values. Real settings menus don't do that;
//! they inline the small groups and reserve a page for the substantial ones.
//!
//! So a container is **inlined** into its parent's page — a titled group, its
//! members listed underneath — when it is small and made entirely of scalars
//! ([`inlines`]); otherwise it becomes a **drill** row that opens a page of its
//! own. The test is deliberately structural rather than schema-driven: flower has
//! to be useful on a document nobody has described. A [`Schema`](crate::Schema)
//! can supersede it later — declared group titles, ordering, an "advanced"
//! section — and feed the same renderer, because the shape it produces is the
//! same.
//!
//! Inlining is one level deep by design. A group inlined into a page never itself
//! contains a group (it is all scalars, by [`inlines`]), so a page is at most two
//! ranks: its own children, and the members of the groups among them. That bound
//! is what keeps a page readable without a second indentation scheme.
//!
//! Inlining is a *presentation* default, never a cage: a group header keeps its
//! own path, so it stays selectable, deletable, and openable as a page like any
//! other container.

use std::collections::{HashMap, HashSet};

use fig::Value;
pub use fig_schema::Seg;

use crate::tree::{VKind, key_to_string, preview, value_at};

/// The largest container still inlined into its parent's page.
///
/// Six is the point where a group stops reading as a handful of related fields
/// and starts reading as a list — and where inlining two of them in a row would
/// fill a short terminal with somebody else's fields. It is a presentation
/// constant, not a correctness one: raising it inlines more, lowering it drills
/// more, and nothing else changes.
pub const INLINE_MAX: usize = 6;

/// What a [`PageItem`] does when you activate it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A leaf: editable in place.
    Scalar,
    /// A container substantial enough to earn its own page. `count` is how many
    /// children it holds — what a "12 fields ›" affordance shows.
    Drill { count: usize },
    /// The title of a container inlined into *this* page. The items that follow it
    /// at [`PageItem::inset`] 1 are its members.
    ///
    /// Selectable, and openable as a page in its own right: the inline rendering
    /// is a default, not a restriction.
    GroupHeader { count: usize },
}

/// One line of a page.
#[derive(Clone, Debug)]
pub struct PageItem {
    /// The fig path to this node from the document root — the same currency
    /// [`tree`](crate::tree) deals in, so an edit op takes it unchanged.
    pub path: Vec<Seg>,
    /// The mapping key, or `[i]` for a sequence item.
    pub label: String,
    pub vkind: VKind,
    /// A one-line rendering of the value (the scalar text, or `{n}` / `[n]`).
    pub preview: String,
    pub kind: ItemKind,
    /// 0 for a direct child of the page's focus; 1 for a member of a group inlined
    /// into it. Never more — see the module docs.
    pub inset: usize,
    /// A readable stand-in for a sequence item's index — the value of whichever
    /// of its fields best names it ([`title_keys`]). `None` for a mapping entry,
    /// whose key already names it, and for an item nothing distinguishes.
    ///
    /// It never replaces [`label`](Self::label): the index is what the path is
    /// addressed by and what a reorder moves, so a frontend shows both.
    pub title: Option<String>,
    /// A container's entire contents in flow form (`{branches: [master]}`), when
    /// they are short enough to be worth showing instead of counting.
    ///
    /// `1 field ›` is strictly less than the document says: the field is right
    /// there and it fits. A count is what you fall back to when the contents
    /// don't ([`SUMMARY_BUDGET`]), not the default way to describe a small
    /// container. `None` for a scalar, whose value is already its own row.
    pub summary: Option<String>,
}

impl PageItem {
    /// Whether activating this item opens a page (rather than editing a value).
    ///
    /// A group header does **not**, though it names a container: its members are
    /// already on this page, so the page it would open shows exactly what you can
    /// already see — the same two rows twice, once on each side of a split. A
    /// container is worth a page when the page tells you something; this one
    /// cannot. Its members are reached by moving onto them, and every op that
    /// takes the group itself takes a path, which the header still carries.
    pub fn is_drill(&self) -> bool {
        matches!(self.kind, ItemKind::Drill { .. })
    }

    /// Whether this item names a container at all — a drill row, or the header of
    /// a group inlined into this page.
    pub fn is_container(&self) -> bool {
        matches!(
            self.kind,
            ItemKind::Drill { .. } | ItemKind::GroupHeader { .. }
        )
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self.kind, ItemKind::Scalar)
    }

    /// Whether this item's *label* can be changed — true for a mapping entry,
    /// false for a sequence item.
    ///
    /// A sequence item's label is its index: it is the position, not a name, so
    /// there is nothing to rename and the only thing that moves it is a reorder.
    /// The inference is one line, which is exactly why it belongs here — every
    /// frontend that redid it would be one edit away from disagreeing with the
    /// op that actually refuses.
    pub fn can_rename(&self) -> bool {
        matches!(self.path.last(), Some(Seg::Key(_)))
    }
}

/// One container's children, ready to render.
#[derive(Clone, Debug, Default)]
pub struct Page {
    /// The container being listed. Empty is the document root.
    pub focus: Vec<Seg>,
    pub items: Vec<PageItem>,
    /// What this page's own container is called, when it is a sequence item and
    /// its index is not worth reading — the same title its row carried on the
    /// page you opened it from, so the breadcrumb agrees with what you clicked.
    pub title: Option<String>,
}

impl Page {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Where `path` sits in this page, if it is on it.
    pub fn position_of(&self, path: &[Seg]) -> Option<usize> {
        self.items.iter().position(|i| i.path == path)
    }

    /// Whether any item on this page opens a page of its own.
    ///
    /// A page with none is a leaf of the navigation, and — at the root — a
    /// document with no depth to navigate at all, which is how a frontend knows
    /// to spend the whole width on one pane instead of drawing an empty second
    /// one. See [`Model::pages_would_degenerate`](crate::Model::pages_would_degenerate).
    pub fn has_drills(&self) -> bool {
        self.items.iter().any(PageItem::is_drill)
    }

    /// The page's title as a breadcrumb — `server › limits`, or `root_label` for
    /// the document root.
    pub fn breadcrumb(&self, root_label: &str) -> String {
        if self.focus.is_empty() {
            return root_label.to_string();
        }
        let mut parts: Vec<String> = self.focus.iter().map(seg_label).collect();
        if let (Some(title), Some(last)) = (&self.title, parts.last_mut()) {
            *last = title.clone();
        }
        parts.join(" › ")
    }
}

/// How a path segment reads in a breadcrumb or a label.
pub fn seg_label(seg: &Seg) -> String {
    match seg {
        Seg::Key(k) => k.clone(),
        Seg::Index(i) => format!("[{i}]"),
    }
}

/// Whether `v` is a container at all — the test for whether a path can be focused.
pub fn is_container(v: &Value) -> bool {
    matches!(v, Value::Map(_) | Value::Seq(_))
}

/// How many children `v` holds (0 for a scalar).
fn child_count(v: &Value) -> usize {
    match v {
        Value::Map(entries) => entries.len(),
        Value::Seq(items) => items.len(),
        _ => 0,
    }
}

/// Whether `v` is inlined into its parent's page rather than given one of its own:
/// a non-empty container of at most [`INLINE_MAX`] children, none of which is
/// itself a container.
///
/// An empty container is excluded deliberately. It has nothing to inline, and a
/// titled group with no members under it reads as a rendering bug; as a drill row
/// it stays visible, countable, and somewhere to add the first key.
pub fn inlines(v: &Value) -> bool {
    let children: Box<dyn Iterator<Item = &Value>> = match v {
        Value::Map(entries) => Box::new(entries.iter().map(|(_, c)| c)),
        Value::Seq(items) => Box::new(items.iter()),
        _ => return false,
    };
    let n = child_count(v);
    n > 0 && n <= INLINE_MAX && !children.into_iter().any(is_container)
}

/// Keys that conventionally name the thing they sit in, best first.
///
/// A small list on purpose. It is a tie-breaker over the structural evidence
/// below, not the mechanism: config files that call it something else are the
/// common case, and a list long enough to cover them would start guessing wrong.
const NAME_KEYS: [&str; 5] = ["name", "title", "id", "label", "key"];

/// Rank the keys of a sequence's items by how well each one *names* an item,
/// best first.
///
/// A sequence of mappings is the one place a config has no names to show: the
/// items are addressed by index, and `[0]`, `[1]`, `[2]` tell you nothing about
/// which step, service, or rule you are looking at. The information is there —
/// it is just in a field rather than in a key — so this works out which field.
///
/// Three signals, in one score:
///
/// - **coverage** — how many items have this key at all, with a scalar value.
/// - **distinctness** — how many of those values differ. A key that reads the
///   same on every item cannot tell them apart, however faithfully it is filled
///   in, so this is weighted hardest.
/// - **convention** — whether it is one of [`NAME_KEYS`].
///
/// A *ranking* rather than a single answer, because items in the same sequence
/// need not have the same keys: a GitHub Actions step is named by `uses` or by
/// `run` depending on which kind of step it is, and each item takes the best
/// key it actually has ([`title_of`]).
pub fn title_keys(items: &[Value]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut stats: HashMap<String, (usize, HashSet<String>)> = HashMap::new();
    let mut mappings = 0usize;

    for item in items {
        let Value::Map(entries) = item else { continue };
        mappings += 1;
        for (k, v) in entries {
            if is_container(v) {
                continue;
            }
            let key = key_to_string(k);
            let seen = stats.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (0, HashSet::new())
            });
            seen.0 += 1;
            seen.1.insert(preview(v));
        }
    }
    if mappings == 0 {
        return Vec::new();
    }

    let mut ranked: Vec<(f64, usize, usize, &String)> = order
        .iter()
        .enumerate()
        .map(|(doc_order, key)| {
            let (present, values) = &stats[key];
            let coverage = *present as f64 / mappings as f64;
            let distinctness = values.len() as f64 / *present as f64;
            let convention = NAME_KEYS.iter().position(|n| n.eq_ignore_ascii_case(key));
            let score =
                coverage + 1.5 * distinctness + if convention.is_some() { 2.0 } else { 0.0 };
            (score, convention.unwrap_or(NAME_KEYS.len()), doc_order, key)
        })
        .collect();
    // Best score first; ties settled by convention, then by the order the
    // document itself puts the keys in — both stable, so a page does not
    // reshuffle its titles when an unrelated field is edited.
    ranked.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    ranked.into_iter().map(|(_, _, _, k)| k.clone()).collect()
}

/// The title `item` takes from a ranking: the value of the best-ranked key it
/// actually has. `None` for a non-mapping, or one with none of the keys.
pub fn title_of(ranking: &[String], item: &Value) -> Option<String> {
    let Value::Map(entries) = item else {
        return None;
    };
    ranking.iter().find_map(|want| {
        entries
            .iter()
            .find_map(|(k, v)| (!is_container(v) && key_to_string(k) == *want).then(|| preview(v)))
    })
}

/// How long a container's flow-form summary may get before a count is the more
/// useful thing to show.
///
/// Generous, because the renderer applies the real limit — whatever room the row
/// actually has — and falls back to the count on its own. This only stops the
/// projection building a 4KB string for a container nobody could render anyway.
pub const SUMMARY_BUDGET: usize = 72;

/// A container's whole contents on one line, in flow form, or `None` if they run
/// past `budget`.
///
/// Flow form because that is how the formats themselves write a small container
/// — `{branches: [master]}` is valid YAML, JSON, and (near enough) TOML — so it
/// reads as the document rather than as a rendering of it.
pub fn flow(v: &Value, budget: usize) -> Option<String> {
    let rendered = match v {
        Value::Map(entries) => {
            let parts = entries
                .iter()
                .map(|(k, val)| Some(format!("{}: {}", key_to_string(k), flow(val, budget)?)))
                .collect::<Option<Vec<_>>>()?;
            format!("{{{}}}", parts.join(", "))
        }
        Value::Seq(items) => {
            let parts = items
                .iter()
                .map(|i| flow(i, budget))
                .collect::<Option<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        scalar => preview(scalar),
    };
    (rendered.chars().count() <= budget).then_some(rendered)
}

/// Build the page listing the container at `focus`.
///
/// `hidden_top_level` is the same root-scoped hiding [`tree::build_rows`] honors
/// (an embedder's managed keys): applied only when `focus` is the root, so a
/// nested key that happens to share a hidden name is untouched.
///
/// A `focus` that doesn't resolve, or that names a scalar, yields an empty page.
/// The projection stays total so a frontend never has to guard it; the model
/// keeps `focus` on a real container anyway
/// ([`Model::reanchor_focus`](crate::Model)).
pub fn build_page(root: &Value, focus: &[Seg], hidden_top_level: &HashSet<String>) -> Page {
    let mut page = Page {
        focus: focus.to_vec(),
        items: Vec::new(),
        title: page_title(root, focus),
    };
    let Some(node) = value_at(root, focus) else {
        return page;
    };
    let at_root = focus.is_empty();

    // A sequence's items render alike, whatever their individual sizes.
    //
    // Applying the inline test per item would expand whichever entries happen to
    // be small and collapse the rest — a list where some rows are three lines and
    // others are one, which reads as a rendering fault rather than as a list. It
    // also destroys the one comparison a list is for: entry against entry. So a
    // sequence inlines every mapping item or none, and "none" is the answer as
    // soon as one item is too big or too nested to inline.
    //
    // A mapping's children are under no such rule: they have distinct names, so
    // a mix of inlined groups and drill rows reads as what it is.
    let (uniform, ranking) = match node {
        Value::Seq(items) => (
            Some(items.iter().all(|i| !is_container(i) || inlines(i))),
            title_keys(items),
        ),
        _ => (None, Vec::new()),
    };

    for (label, path, child) in children_of(node, focus) {
        if at_root && hidden_top_level.contains(&label) {
            continue;
        }
        let title = title_of(&ranking, child);
        if !is_container(child) {
            page.items
                .push(item(label, path, child, ItemKind::Scalar, 0, title));
        } else if uniform.unwrap_or(true) && inlines(child) {
            let count = child_count(child);
            page.items.push(item(
                label,
                path.clone(),
                child,
                ItemKind::GroupHeader { count },
                0,
                title,
            ));
            for (sub_label, sub_path, sub) in children_of(child, &path) {
                page.items
                    .push(item(sub_label, sub_path, sub, ItemKind::Scalar, 1, None));
            }
        } else {
            let count = child_count(child);
            page.items.push(item(
                label,
                path,
                child,
                ItemKind::Drill { count },
                0,
                title,
            ));
        }
    }
    page
}

/// The title of the container `focus` names, when it is a sequence item — the
/// same one its row carried on the page it was opened from.
fn page_title(root: &Value, focus: &[Seg]) -> Option<String> {
    let Some(Seg::Index(i)) = focus.last() else {
        return None;
    };
    let Value::Seq(items) = value_at(root, &focus[..focus.len() - 1])? else {
        return None;
    };
    title_of(&title_keys(items), items.get(*i)?)
}

fn item(
    label: String,
    path: Vec<Seg>,
    v: &Value,
    kind: ItemKind,
    inset: usize,
    title: Option<String>,
) -> PageItem {
    PageItem {
        path,
        label,
        vkind: VKind::of(v),
        preview: preview(v),
        kind,
        inset,
        title,
        summary: is_container(v).then(|| flow(v, SUMMARY_BUDGET)).flatten(),
    }
}

/// The (label, path, value) of each child of a container, in document order.
/// Empty for a scalar.
fn children_of<'v>(node: &'v Value, base: &[Seg]) -> Vec<(String, Vec<Seg>, &'v Value)> {
    let extend = |seg: Seg| {
        let mut p = base.to_vec();
        p.push(seg);
        p
    };
    match node {
        Value::Map(entries) => entries
            .iter()
            .map(|(k, v)| {
                let key = key_to_string(k);
                (key.clone(), extend(Seg::Key(key)), v)
            })
            .collect(),
        Value::Seq(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), extend(Seg::Index(i)), v))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fig::Format;

    const SAMPLE: &str = "\
title = \"flower\"
version = 1
enabled = true

[server]
host = \"localhost\"
port = 8080
tags = [\"alpha\", \"beta\"]

[server.limits]
max_connections = 100
timeout = 30.5
";

    fn value_of(src: &str, fmt: Format) -> Value {
        fig::Document::parse(src.as_bytes(), fmt)
            .expect("parse")
            .to_value()
            .expect("to_value")
    }

    fn sample() -> Value {
        value_of(SAMPLE, Format::Toml)
    }

    fn page_of(root: &Value, focus: &[Seg]) -> Page {
        build_page(root, focus, &HashSet::new())
    }

    fn key(k: &str) -> Seg {
        Seg::Key(k.to_string())
    }

    /// `label`, `inset`, and what activating it does — the whole shape of a page
    /// in one comparable form.
    fn shape(page: &Page) -> Vec<(String, usize, &'static str)> {
        page.items
            .iter()
            .map(|i| {
                let kind = match i.kind {
                    ItemKind::Scalar => "scalar",
                    ItemKind::Drill { .. } => "drill",
                    ItemKind::GroupHeader { .. } => "group",
                };
                (i.label.clone(), i.inset, kind)
            })
            .collect()
    }

    #[test]
    fn the_root_page_lists_one_level_and_drills_the_rest() {
        let root = sample();
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![
                ("title".into(), 0, "scalar"),
                ("version".into(), 0, "scalar"),
                ("enabled".into(), 0, "scalar"),
                // Mixed children (two containers among four) — a page of its own.
                ("server".into(), 0, "drill"),
            ]
        );
    }

    #[test]
    fn small_all_scalar_containers_inline_into_the_page() {
        let root = sample();
        // `tags` (2 strings) and `limits` (2 numbers) are both small and entirely
        // scalar, so `server` renders as one page rather than three.
        assert_eq!(
            shape(&page_of(&root, &[key("server")])),
            vec![
                ("host".into(), 0, "scalar"),
                ("port".into(), 0, "scalar"),
                ("tags".into(), 0, "group"),
                ("[0]".into(), 1, "scalar"),
                ("[1]".into(), 1, "scalar"),
                ("limits".into(), 0, "group"),
                ("max_connections".into(), 1, "scalar"),
                ("timeout".into(), 1, "scalar"),
            ]
        );
    }

    #[test]
    fn an_inlined_member_keeps_its_own_path() {
        let root = sample();
        let page = page_of(&root, &[key("server")]);
        let timeout = page
            .items
            .iter()
            .find(|i| i.label == "timeout")
            .expect("timeout on server's page");
        // The path is the document's, not the page's — an edit op takes it as-is
        // even though the row is two ranks below the page's focus.
        assert_eq!(
            timeout.path,
            vec![key("server"), key("limits"), key("timeout")]
        );
    }

    #[test]
    fn a_container_too_big_to_inline_drills() {
        let mut src = String::from("[big]\n");
        for i in 0..=INLINE_MAX {
            src.push_str(&format!("k{i} = {i}\n"));
        }
        let root = value_of(&src, Format::Toml);
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![("big".into(), 0, "drill")]
        );

        // One fewer child and the same container inlines.
        let trimmed = src
            .rsplit_once('\n')
            .unwrap()
            .0
            .rsplit_once('\n')
            .unwrap()
            .0;
        let root = value_of(&format!("{trimmed}\n"), Format::Toml);
        assert_eq!(page_of(&root, &[]).items[0].inset, 0);
        assert!(matches!(
            page_of(&root, &[]).items[0].kind,
            ItemKind::GroupHeader { .. }
        ));
    }

    #[test]
    fn a_container_holding_a_container_drills_however_small() {
        let root = value_of("{\"a\": {\"b\": {\"c\": 1}}}", Format::Json);
        // `a` has one child — but that child is a container, so inlining it would
        // put a group inside a group and reintroduce unbounded depth.
        assert_eq!(shape(&page_of(&root, &[])), vec![("a".into(), 0, "drill")]);
        assert_eq!(
            shape(&page_of(&root, &[key("a")])),
            vec![("b".into(), 0, "group"), ("c".into(), 1, "scalar")]
        );
    }

    #[test]
    fn an_empty_container_drills_rather_than_inlining_as_a_headless_group() {
        let root = value_of("{\"empty\": {}, \"none\": []}", Format::Json);
        assert_eq!(
            shape(&page_of(&root, &[])),
            vec![("empty".into(), 0, "drill"), ("none".into(), 0, "drill")]
        );
        assert!(page_of(&root, &[key("empty")]).is_empty());
    }

    #[test]
    fn hiding_is_scoped_to_the_root_page() {
        let root = value_of(
            "{\"id\": 1, \"inner\": {\"id\": 2, \"keep\": 3}}",
            Format::Json,
        );
        let hidden = HashSet::from(["id".to_string()]);
        let rooted = build_page(&root, &[], &hidden);
        assert_eq!(
            shape(&rooted),
            vec![
                ("inner".into(), 0, "group"),
                ("id".into(), 1, "scalar"),
                ("keep".into(), 1, "scalar")
            ]
        );
        // The nested `id` shares the name and is untouched — the group inlined
        // into the root page still carries it.
        let inner = build_page(&root, &[key("inner")], &hidden);
        assert_eq!(
            shape(&inner),
            vec![("id".into(), 0, "scalar"), ("keep".into(), 0, "scalar")]
        );
    }

    #[test]
    fn a_page_that_cannot_be_listed_is_empty_rather_than_a_panic() {
        let root = sample();
        assert!(page_of(&root, &[key("nope")]).is_empty());
        assert!(page_of(&root, &[key("title")]).is_empty());
    }

    #[test]
    fn breadcrumbs_name_the_lineage() {
        let root = sample();
        assert_eq!(page_of(&root, &[]).breadcrumb("‹document›"), "‹document›");
        assert_eq!(
            page_of(&root, &[key("server"), key("limits")]).breadcrumb("‹document›"),
            "server › limits"
        );
        assert_eq!(
            page_of(&root, &[key("server"), key("tags")]).breadcrumb("x"),
            "server › tags"
        );
    }

    #[test]
    fn a_flat_document_has_nothing_to_drill_into() {
        let flat = value_of("{\"a\": 1, \"b\": 2}", Format::Json);
        assert!(!page_of(&flat, &[]).has_drills());
        assert!(page_of(&sample(), &[]).has_drills());
    }

    // ── titles for sequence items ─────────────────────────────────────────

    /// A workflow's steps: the case with no single naming key. Different kinds of
    /// step are named by different fields, and one field (`if`) reads the same on
    /// the items that have it.
    const STEPS: &str = r#"{"steps": [
        {"uses": "actions/checkout@v7"},
        {"uses": "dtolnay/rust-toolchain@stable", "if": "always"},
        {"uses": "Swatinem/rust-cache@v2", "if": "always", "with": {"key": "a"}},
        {"run": "cargo xtask ci", "shell": "bash"}
    ]}"#;

    fn titles(page: &Page) -> Vec<Option<String>> {
        page.items.iter().map(|i| i.title.clone()).collect()
    }

    #[test]
    fn a_sequence_item_is_titled_by_the_field_that_distinguishes_it() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps")]);
        assert_eq!(
            titles(&page),
            vec![
                Some("actions/checkout@v7".into()),
                Some("dtolnay/rust-toolchain@stable".into()),
                Some("Swatinem/rust-cache@v2".into()),
                // No `uses` at all — falls to the next-best key it does have.
                Some("cargo xtask ci".into()),
            ]
        );
    }

    #[test]
    fn a_key_that_reads_the_same_on_every_item_loses_to_one_that_does_not() {
        let root = value_of(STEPS, Format::Json);
        let Value::Map(entries) = &root else {
            unreachable!()
        };
        let Value::Seq(items) = &entries[0].1 else {
            unreachable!()
        };
        let ranking = title_keys(items);
        // `if` is on two items and says "always" on both, so it names neither.
        let uses = ranking.iter().position(|k| k == "uses").expect("uses");
        let cond = ranking.iter().position(|k| k == "if").expect("if");
        assert!(uses < cond, "{ranking:?}");
        // `with` is a container: never a title.
        assert!(!ranking.iter().any(|k| k == "with"), "{ranking:?}");
    }

    #[test]
    fn a_conventional_name_key_outranks_a_merely_distinct_one() {
        let root = value_of(
            r#"{"env": [
                {"name": "HOME", "value": "/root"},
                {"name": "PATH", "value": "/bin"}
            ]}"#,
            Format::Json,
        );
        let page = page_of(&root, &[key("env")]);
        // Both items are small and all-scalar, so they inline — and the title
        // lands on the group header, which is the row standing in for the item.
        // `value` is exactly as distinct and as well covered as `name`; `name`
        // wins because it is what a config author means by a name.
        assert_eq!(
            page.items
                .iter()
                .filter(|i| i.inset == 0)
                .map(|i| i.title.clone())
                .collect::<Vec<_>>(),
            vec![Some("HOME".into()), Some("PATH".into())]
        );
    }

    #[test]
    fn a_mapping_entry_is_never_titled() {
        let root = sample();
        assert!(page_of(&root, &[]).items.iter().all(|i| i.title.is_none()));
        // Nor is a sequence of scalars: the value is already the whole row.
        let tags = page_of(&root, &[key("server"), key("tags")]);
        assert!(tags.items.iter().all(|i| i.title.is_none()));
    }

    #[test]
    fn a_sequence_renders_its_items_uniformly() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps")]);
        // The third step nests a `with` mapping, so it cannot inline — and none of
        // the others do either, however small. A list reads as a list.
        assert!(
            page.items
                .iter()
                .all(|i| matches!(i.kind, ItemKind::Drill { .. })),
            "{:?}",
            shape(&page)
        );

        // Take the nesting away and every item inlines, again as a group.
        let flat = value_of(
            r#"{"steps": [{"run": "a"}, {"run": "b", "shell": "sh"}]}"#,
            Format::Json,
        );
        let page = page_of(&flat, &[key("steps")]);
        assert_eq!(
            shape(&page),
            vec![
                ("[0]".into(), 0, "group"),
                ("run".into(), 1, "scalar"),
                ("[1]".into(), 0, "group"),
                ("run".into(), 1, "scalar"),
                ("shell".into(), 1, "scalar"),
            ]
        );
    }

    #[test]
    fn a_titled_item_carries_its_title_into_its_own_breadcrumb() {
        let root = value_of(STEPS, Format::Json);
        let page = page_of(&root, &[key("steps"), Seg::Index(3)]);
        assert_eq!(page.title.as_deref(), Some("cargo xtask ci"));
        assert_eq!(page.breadcrumb("‹document›"), "steps › cargo xtask ci");
        // Its own children are mapping entries, so none of them is titled.
        assert!(page.items.iter().all(|i| i.title.is_none()));
    }

    #[test]
    fn a_multi_line_value_is_cut_to_its_first_line() {
        let root = value_of(
            "{\"steps\": [{\"run\": \"set -e\\ncargo test\\n\"}]}",
            Format::Json,
        );
        let page = page_of(&root, &[key("steps")]);
        // A YAML block scalar would otherwise draw a row several lines tall and
        // throw every row below it out of alignment.
        assert_eq!(page.items[0].title.as_deref(), Some("set -e …"));
        assert!(!page.items[0].preview.contains('\n'));
    }

    // ── flow summaries ────────────────────────────────────────────────────

    #[test]
    fn a_container_that_fits_on_the_row_shows_its_contents_not_a_count() {
        let root = value_of(
            r#"{"on": {"push": {"branches": ["master"]}, "pull_request": null}}"#,
            Format::Json,
        );
        let page = page_of(&root, &[key("on")]);
        let push = &page.items[0];
        // It has one field, and the field is right there: counting it to `1 field`
        // would say strictly less than the document does in the same room.
        assert!(matches!(push.kind, ItemKind::Drill { count: 1 }));
        assert_eq!(push.summary.as_deref(), Some("{branches: [master]}"));
    }

    #[test]
    fn a_container_too_long_to_summarise_falls_back_to_being_counted() {
        let long = "x".repeat(SUMMARY_BUDGET);
        let root = value_of(
            &format!(r#"{{"outer": {{"a": {{"b": "{long}"}}}}}}"#),
            Format::Json,
        );
        let page = page_of(&root, &[key("outer")]);
        assert!(page.items[0].summary.is_none());
        // The budget is a length limit, not a depth one — shorten the value and
        // the same shape summarises fine.
        let root = value_of(r#"{"outer": {"a": {"b": "x"}}}"#, Format::Json);
        assert_eq!(
            page_of(&root, &[key("outer")]).items[0].summary.as_deref(),
            Some("{b: x}")
        );
    }

    #[test]
    fn a_scalar_is_never_summarised() {
        let root = sample();
        let page = page_of(&root, &[]);
        assert!(
            page.items
                .iter()
                .filter(|i| matches!(i.kind, ItemKind::Scalar))
                .all(|i| i.summary.is_none())
        );
    }

    #[test]
    fn a_group_header_is_not_a_drill() {
        let root = sample();
        let page = page_of(&root, &[key("server")]);
        let limits = page
            .items
            .iter()
            .find(|i| i.label == "limits")
            .expect("limits");
        assert!(matches!(limits.kind, ItemKind::GroupHeader { .. }));
        // It names a container, but opening it would show what is already here.
        assert!(limits.is_container());
        assert!(!limits.is_drill());
        assert!(!page.has_drills());
    }

    #[test]
    fn only_a_mapping_entry_can_be_renamed() {
        let root = sample();
        let page = page_of(&root, &[key("server"), key("tags")]);
        // A sequence item's label is its index — a position, not a name.
        assert!(page.items.iter().all(|i| !i.can_rename()));
        assert!(page_of(&root, &[]).items.iter().all(|i| i.can_rename()));
    }
}
