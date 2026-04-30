//! LCOM (Lack of Cohesion of Methods) approximation.
//!
//! Counts the number of disjoint method clusters per class — Tornhill's
//! "internal split" companion to Change Coupling. Two methods belong
//! to the same cluster if they touch a shared field or one calls the
//! other; LCOM is the resulting connected-component count. A class
//! with `cluster_count == 1` is cohesive; `>= 2` means the class is
//! mechanically separable into smaller pieces.
//!
//! ## Approximation
//!
//! HEAL's v0.2 backend is `tree-sitter-approx`: pure syntactic walk,
//! no type resolution. That means:
//! - `this.foo` / `self.foo` references count, but inherited fields
//!   from a base class don't (we never see the base).
//! - Dynamic property access (`this[name]`, computed keys) is invisible.
//! - Methods that share state via a *helper function* outside the class
//!   look unrelated. Static methods on the same class look unrelated to
//!   instance methods.
//!
//! These limitations bias toward over-reporting (false positives — a
//! class that's actually cohesive looks split). The renderer is meant
//! to surface candidates for review, not to make automatic decisions.
//! `backend = "lsp"` (v0.5+) replaces this with a typed implementation.
//!
//! ## Scope
//!
//! - **TypeScript**: `class_declaration` (and TSX `class_declaration`).
//!   Methods are `method_definition` nodes inside the class body.
//!   Field references: `member_expression` whose object is `this`.
//! - **Rust**: `impl_item` (inherent impl + trait impl, both treated
//!   the same). Methods are `function_item` inside the impl body.
//!   Field references: `field_expression` whose value is `self`.
//!
//! Module-scope LCOM (Rust file-level free functions, TS named-export
//! groups) is deferred — the grouping rules are project-specific and
//! adding them now would produce more noise than signal.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, QueryCursor, StreamingIterator};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::observer::complexity::{parse, ParsedFile};
use crate::observer::lang::Language;
use crate::observer::walk::walk_supported_files;
use crate::observer::{ObservationMeta, Observer};

/// Default `min_cluster_count` floor — anything `>= 2` (separable into
/// at least two clusters) is worth surfacing. The Calibration percentile
/// breaks layer Severity on top.
pub const DEFAULT_MIN_CLUSTER_COUNT: u32 = 2;

#[derive(Debug, Clone, Default)]
pub struct LcomObserver {
    pub enabled: bool,
    pub excluded: Vec<String>,
    pub min_cluster_count: u32,
}

impl LcomObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.lcom.enabled,
            excluded: cfg.observer_excluded_paths(),
            min_cluster_count: cfg.metrics.lcom.min_cluster_count,
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> LcomReport {
        let mut report = LcomReport {
            min_cluster_count: self.min_cluster_count,
            ..LcomReport::default()
        };
        if !self.enabled {
            return report;
        }
        for path in walk_supported_files(root, &self.excluded) {
            let Some(lang) = Language::from_path(&path) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = parse(source, lang) else {
                continue;
            };
            for class in classes_in(&parsed, &path) {
                report.classes.push(class);
            }
        }
        report.classes.sort_by(|a, b| {
            b.cluster_count
                .cmp(&a.cluster_count)
                .then_with(|| a.file.cmp(&b.file))
        });

        let totals = LcomTotals {
            classes_scanned: report.classes.len(),
            classes_with_lcom: report
                .classes
                .iter()
                .filter(|c| c.cluster_count >= self.min_cluster_count)
                .count(),
            max_cluster_count: report
                .classes
                .iter()
                .map(|c| c.cluster_count)
                .max()
                .unwrap_or(0),
        };
        report.totals = totals;
        report
    }
}

impl Observer for LcomObserver {
    type Output = LcomReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "lcom",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LcomReport {
    pub classes: Vec<ClassLcom>,
    pub totals: LcomTotals,
    pub min_cluster_count: u32,
}

impl LcomReport {
    /// Top-N classes by `cluster_count` desc. The underlying Vec is
    /// already sorted at scan time; this just truncates a clone.
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<ClassLcom> {
        self.classes.iter().take(n).cloned().collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LcomTotals {
    pub classes_scanned: usize,
    pub classes_with_lcom: usize,
    pub max_cluster_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClassLcom {
    pub file: PathBuf,
    pub language: String,
    pub class_name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub method_count: u32,
    pub cluster_count: u32,
    /// Each cluster is a list of method names that share state /
    /// call-graph reachability. Sorted within each cluster + across
    /// clusters for deterministic output.
    pub clusters: Vec<MethodCluster>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodCluster {
    pub methods: Vec<String>,
}

impl IntoFindings for LcomReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.classes
            .iter()
            .filter(|c| c.cluster_count >= self.min_cluster_count.max(1))
            .map(|c| {
                let summary = format!(
                    "LCOM={} clusters across {} methods in {} ({})",
                    c.cluster_count, c.method_count, c.class_name, c.language,
                );
                let location = Location {
                    file: c.file.clone(),
                    line: Some(c.start_line),
                    symbol: Some(c.class_name.clone()),
                };
                let seed = format!("lcom:{}:{}", c.cluster_count, c.method_count);
                Finding::new("lcom", location, summary, &seed)
            })
            .collect()
    }
}

/// Walk every class scope in the parsed file, computing per-class LCOM.
fn classes_in(parsed: &ParsedFile, file: &Path) -> Vec<ClassLcom> {
    let q = parsed.lang.lcom_query();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q.query, parsed.tree.root_node(), parsed.source.as_bytes());

    let mut out: Vec<ClassLcom> = Vec::new();
    while let Some(m) = matches.next() {
        for cap in m
            .captures
            .iter()
            .filter(|c| c.index == q.captures.class_scope)
        {
            if let Some(class) = analyze_class(cap.node, parsed, file) {
                out.push(class);
            }
        }
    }
    out
}

fn analyze_class(class_node: Node<'_>, parsed: &ParsedFile, file: &Path) -> Option<ClassLcom> {
    let methods = collect_methods(class_node, parsed.lang, parsed.source.as_bytes());
    if methods.is_empty() {
        return None;
    }
    let class_name = class_name_for(class_node, parsed.source.as_bytes(), parsed.lang);

    // method_index → fields it touches + sibling-method calls
    let mut field_to_methods: HashMap<String, Vec<usize>> = HashMap::new();
    let mut method_calls: Vec<Vec<usize>> = vec![Vec::new(); methods.len()];
    let method_name_to_index: HashMap<&str, usize> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.as_str(), i))
        .collect();

    for (i, method) in methods.iter().enumerate() {
        let refs = collect_self_refs(method.body, parsed.source.as_bytes(), parsed.lang);
        for field in refs.fields {
            field_to_methods.entry(field).or_default().push(i);
        }
        for callee in refs.method_calls {
            if let Some(&j) = method_name_to_index.get(callee.as_str()) {
                if j != i {
                    method_calls[i].push(j);
                }
            }
        }
    }

    let mut uf = UnionFind::new(methods.len());
    for members in field_to_methods.values() {
        for w in members.windows(2) {
            uf.union(w[0], w[1]);
        }
    }
    for (i, callees) in method_calls.iter().enumerate() {
        for &j in callees {
            uf.union(i, j);
        }
    }

    let mut clusters: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for (i, m) in methods.iter().enumerate() {
        clusters
            .entry(uf.find(i))
            .or_default()
            .insert(m.name.clone());
    }
    let mut clusters_vec: Vec<MethodCluster> = clusters
        .into_values()
        .map(|set| MethodCluster {
            methods: set.into_iter().collect(),
        })
        .collect();
    clusters_vec.sort_by(|a, b| {
        b.methods
            .len()
            .cmp(&a.methods.len())
            .then_with(|| a.methods.cmp(&b.methods))
    });

    Some(ClassLcom {
        file: file.to_path_buf(),
        language: parsed.lang.name().to_owned(),
        class_name,
        start_line: u32::try_from(class_node.start_position().row + 1).unwrap_or(u32::MAX),
        end_line: u32::try_from(class_node.end_position().row + 1).unwrap_or(u32::MAX),
        method_count: u32::try_from(methods.len()).unwrap_or(u32::MAX),
        cluster_count: u32::try_from(clusters_vec.len()).unwrap_or(u32::MAX),
        clusters: clusters_vec,
    })
}

struct MethodEntry<'a> {
    name: String,
    body: Node<'a>,
}

/// Gather every method definition inside a class body, in source order.
/// Anonymous / static-init / constructor blocks without a name are
/// skipped (they participate in cohesion but are awkward to label).
fn collect_methods<'a>(
    class_node: Node<'a>,
    lang: Language,
    source: &[u8],
) -> Vec<MethodEntry<'a>> {
    let body = class_body(class_node, lang);
    let Some(body) = body else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    for child in iter_children(body) {
        if !is_method_kind(child, lang) {
            continue;
        }
        let Some(name) = method_name(child, source) else {
            continue;
        };
        let Some(body_node) = method_body(child) else {
            continue;
        };
        methods.push(MethodEntry {
            name,
            body: body_node,
        });
    }
    methods
}

fn class_body(class_node: Node<'_>, lang: Language) -> Option<Node<'_>> {
    let _ = lang; // body field name is "body" for both grammars
    class_node.child_by_field_name("body")
}

fn iter_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    let kids: Vec<_> = node.named_children(&mut cursor).collect();
    kids.into_iter()
}

fn is_method_kind(node: Node<'_>, lang: Language) -> bool {
    match lang {
        #[cfg(feature = "lang-ts")]
        Language::TypeScript | Language::Tsx => {
            matches!(node.kind(), "method_definition" | "method_signature")
        }
        #[cfg(feature = "lang-rust")]
        Language::Rust => node.kind() == "function_item",
    }
}

fn method_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let name_node = node.child_by_field_name("name")?;
    let text = name_node.utf8_text(source).ok()?;
    Some(text.to_owned())
}

fn method_body(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body")
}

fn class_name_for(class_node: Node<'_>, source: &[u8], lang: Language) -> String {
    // TS: class_declaration has a `name` field. Rust impl_item has a
    // `type` field (the type the impl applies to). Trait impls also
    // have a `trait` field; the displayed name uses just the type so
    // `impl Foo for Bar` and `impl Bar` both render as `Bar`.
    let lookup = match lang {
        #[cfg(feature = "lang-ts")]
        Language::TypeScript | Language::Tsx => "name",
        #[cfg(feature = "lang-rust")]
        Language::Rust => "type",
    };
    if let Some(n) = class_node.child_by_field_name(lookup) {
        if let Ok(text) = n.utf8_text(source) {
            return text.to_owned();
        }
    }
    format!("<class@{}>", class_node.start_position().row + 1)
}

#[derive(Default)]
struct SelfRefs {
    fields: Vec<String>,
    method_calls: Vec<String>,
}

/// Walk a method body looking for `this.foo` / `self.foo` and `this.bar()`
/// / `self.bar()`. The split into "field" vs "method call" is based on
/// whether the parent is a `call_expression` whose function is the
/// member-access node — pure syntactic, no type info.
fn collect_self_refs(body: Node<'_>, source: &[u8], lang: Language) -> SelfRefs {
    let mut refs = SelfRefs::default();
    let mut cursor = body.walk();
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        match lang {
            #[cfg(feature = "lang-ts")]
            Language::TypeScript | Language::Tsx => visit_ts(node, source, &mut refs),
            #[cfg(feature = "lang-rust")]
            Language::Rust => visit_rust(node, source, &mut refs),
        }
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    refs
}

#[cfg(feature = "lang-ts")]
fn visit_ts(node: Node<'_>, source: &[u8], refs: &mut SelfRefs) {
    if node.kind() != "member_expression" {
        return;
    }
    let Some(object) = node.child_by_field_name("object") else {
        return;
    };
    if object.kind() != "this" {
        return;
    }
    let Some(prop) = node.child_by_field_name("property") else {
        return;
    };
    let Ok(name) = prop.utf8_text(source) else {
        return;
    };
    let name = name.to_owned();
    if is_call_target(node) {
        refs.method_calls.push(name);
    } else {
        refs.fields.push(name);
    }
}

#[cfg(feature = "lang-rust")]
fn visit_rust(node: Node<'_>, source: &[u8], refs: &mut SelfRefs) {
    if node.kind() != "field_expression" {
        return;
    }
    let Some(value) = node.child_by_field_name("value") else {
        return;
    };
    if value.kind() != "self" {
        return;
    }
    let Some(field) = node.child_by_field_name("field") else {
        return;
    };
    let Ok(name) = field.utf8_text(source) else {
        return;
    };
    let name = name.to_owned();
    if is_call_target(node) {
        refs.method_calls.push(name);
    } else {
        refs.fields.push(name);
    }
}

/// `node` is the receiver expression of a member/field access.
/// Returns true when the parent is a call expression whose `function`
/// (TS) or callee (Rust) is exactly `node`. This is the syntactic
/// signal "this is a method call" vs "this is a field read".
fn is_call_target(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "call_expression" {
        return false;
    }
    let function = parent.child_by_field_name("function");
    matches!(function, Some(f) if f == node)
}

/// Disjoint-set union with path compression. Methods start in their
/// own singleton; each shared-field or call edge merges two sets. The
/// final cluster count is the number of unique roots.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "lang-ts")]
    fn run_ts(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::TypeScript).unwrap();
        classes_in(&parsed, Path::new("test.ts"))
    }

    #[cfg(feature = "lang-rust")]
    fn run_rust(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::Rust).unwrap();
        classes_in(&parsed, Path::new("test.rs"))
    }

    #[cfg(feature = "lang-ts")]
    #[test]
    fn ts_cohesive_class_has_one_cluster() {
        let src = r"
            class Counter {
                private value = 0;
                inc() { this.value += 1; }
                dec() { this.value -= 1; }
                get() { return this.value; }
            }
        ";
        let classes = run_ts(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_name, "Counter");
        assert_eq!(classes[0].method_count, 3);
        assert_eq!(classes[0].cluster_count, 1, "all methods touch `value`");
    }

    #[cfg(feature = "lang-ts")]
    #[test]
    fn ts_split_class_has_multiple_clusters() {
        // Two unrelated responsibility groups: counter (a/b touch `count`)
        // and logger (c/d touch `log`). They don't share fields and don't
        // call each other, so LCOM = 2.
        let src = r"
            class Mixed {
                private count = 0;
                private log: string[] = [];
                inc() { this.count += 1; }
                value() { return this.count; }
                push(msg: string) { this.log.push(msg); }
                tail() { return this.log[this.log.length - 1]; }
            }
        ";
        let classes = run_ts(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 2);
    }

    #[cfg(feature = "lang-ts")]
    #[test]
    fn ts_method_call_merges_clusters() {
        // `tail()` reaches into both responsibility groups — touches
        // `log` directly *and* calls `value()` — so the union-find
        // merges {inc, value} with {push, tail} into one cluster.
        let src = r"
            class Bridged {
                private count = 0;
                private log: string[] = [];
                inc() { this.count += 1; }
                value() { return this.count; }
                push(msg: string) { this.log.push(msg); }
                tail() {
                    this.value();
                    return this.log[0];
                }
            }
        ";
        let classes = run_ts(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_cohesive_impl_has_one_cluster() {
        let src = r"
            struct Counter { value: i32 }
            impl Counter {
                fn inc(&mut self) { self.value += 1; }
                fn dec(&mut self) { self.value -= 1; }
                fn get(&self) -> i32 { self.value }
            }
        ";
        let classes = run_rust(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_name, "Counter");
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_split_impl_has_multiple_clusters() {
        let src = r"
            struct Mixed { count: i32, log: Vec<String> }
            impl Mixed {
                fn inc(&mut self) { self.count += 1; }
                fn value(&self) -> i32 { self.count }
                fn push(&mut self, m: String) { self.log.push(m); }
                fn tail(&self) -> Option<&String> { self.log.last() }
            }
        ";
        let classes = run_rust(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 2);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_method_call_merges_clusters() {
        // `tail` touches `log` *and* calls `value()` — the call-graph
        // edge plus the shared field stitch the two clusters into one.
        let src = r"
            struct Bridged { count: i32, log: Vec<String> }
            impl Bridged {
                fn inc(&mut self) { self.count += 1; }
                fn value(&self) -> i32 { self.count }
                fn push(&mut self, m: String) { self.log.push(m); }
                fn tail(&self) -> i32 {
                    let _ = self.log.last();
                    self.value()
                }
            }
        ";
        let classes = run_rust(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-ts")]
    #[test]
    fn empty_class_is_skipped() {
        let src = "class Empty {}";
        let classes = run_ts(src);
        assert!(classes.is_empty());
    }
}
