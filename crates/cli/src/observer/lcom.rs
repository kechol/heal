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
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::complexity::{parse, ParsedFile};
use crate::observer::lang::Language;
use crate::observer::walk::{walk_supported_files_under, ExcludeMatcher};
use crate::observer::{impl_workspace_builder, ObservationMeta, Observer};
use crate::observers::ObserverReports;

impl_workspace_builder!(LcomObserver);

#[derive(Debug, Clone, Default)]
pub struct LcomObserver {
    pub enabled: bool,
    pub excluded: Vec<String>,
    pub min_cluster_count: u32,
    /// Optional workspace sub-path; see `ComplexityObserver::workspace`.
    pub workspace: Option<PathBuf>,
}

impl LcomObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.lcom.enabled,
            excluded: cfg.exclude_lines(),
            min_cluster_count: cfg.metrics.lcom.min_cluster_count,
            workspace: None,
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
        let matcher = ExcludeMatcher::compile(root, &self.excluded)
            .expect("exclude patterns validated at config load");
        for path in walk_supported_files_under(root, &matcher, self.workspace.as_deref()) {
            let Some(lang) = Language::from_path(&path) else {
                continue;
            };
            if !lang.supports_lcom() {
                continue;
            }
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

    // field-name → distinct method indices that touch it. The
    // `BTreeSet` dedupes within a method (a method touching `this.foo`
    // ten times still produces one entry) so the union-find pass
    // doesn't waste work re-unioning the same pair.
    let mut field_to_methods: HashMap<String, BTreeSet<usize>> = HashMap::new();
    let mut method_calls: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); methods.len()];
    let method_name_to_index: HashMap<&str, usize> = methods
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.as_str(), i))
        .collect();

    for (i, method) in methods.iter().enumerate() {
        let refs = collect_self_refs(method.body, parsed.source.as_bytes(), parsed.lang);
        for field in refs.fields {
            field_to_methods.entry(field).or_default().insert(i);
        }
        for callee in refs.method_calls {
            if let Some(&j) = method_name_to_index.get(callee.as_str()) {
                if j != i {
                    method_calls[i].insert(j);
                }
            }
        }
    }

    let mut uf = UnionFind::new(methods.len());
    for members in field_to_methods.values() {
        let mut iter = members.iter();
        if let Some(&first) = iter.next() {
            for &m in iter {
                uf.union(first, m);
            }
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
    let Some(body) = class_node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if !is_method_kind(child, lang) {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Ok(name) = name_node.utf8_text(source) else {
            continue;
        };
        let Some(body_node) = child.child_by_field_name("body") else {
            continue;
        };
        methods.push(MethodEntry {
            name: name.to_owned(),
            body: body_node,
        });
    }
    methods
}

// `node` is unused when only no-LCOM languages (Go/Scala) are enabled.
#[cfg_attr(
    not(any(
        feature = "lang-ts",
        feature = "lang-js",
        feature = "lang-py",
        feature = "lang-rust"
    )),
    allow(unused_variables)
)]
fn is_method_kind(node: Node<'_>, lang: Language) -> bool {
    match lang {
        #[cfg(feature = "lang-ts")]
        Language::TypeScript | Language::Tsx => {
            matches!(node.kind(), "method_definition" | "method_signature")
        }
        #[cfg(feature = "lang-js")]
        Language::JavaScript | Language::Jsx => node.kind() == "method_definition",
        #[cfg(feature = "lang-py")]
        Language::Python => node.kind() == "function_definition",
        // Go has no class scope; methods attach to types via receivers
        // and live at module scope. Receiver-grouped LCOM lands in
        // v0.3+. `Language::supports_lcom` short-circuits before this
        // is reached, but the variant must still be matched.
        #[cfg(feature = "lang-go")]
        Language::Go => false,
        // Scala spans class / trait / object / case-class / given
        // constructs and uses bare-name field access more than
        // `this.field`. A class-aware LCOM that handles this richness
        // needs the LSP backend (v0.5+); skipped via `supports_lcom`.
        #[cfg(feature = "lang-scala")]
        Language::Scala => false,
        #[cfg(feature = "lang-rust")]
        Language::Rust => node.kind() == "function_item",
    }
}

fn class_name_for(class_node: Node<'_>, source: &[u8], lang: Language) -> String {
    // Rust trait impls have both `trait` and `type` fields; we deliberately
    // pick `type` so `impl Foo for Bar` and `impl Bar` collapse to `Bar`.
    let lookup = match lang {
        #[cfg(feature = "lang-ts")]
        Language::TypeScript | Language::Tsx => "name",
        #[cfg(feature = "lang-js")]
        Language::JavaScript | Language::Jsx => "name",
        #[cfg(feature = "lang-py")]
        Language::Python => "name",
        // Go's LCOM is a no-op (see `is_method_kind`); the field name
        // here doesn't matter.
        #[cfg(feature = "lang-go")]
        Language::Go => "name",
        #[cfg(feature = "lang-scala")]
        Language::Scala => "name",
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

/// Per-language tree-sitter shape of "receiver-bound member access".
/// One row replaces an entire `visit_*` function — adding a third
/// language is a table entry, not new code.
#[derive(Clone, Copy)]
struct SelfRefShape {
    /// Outer node kind that names the access (TS: `member_expression`,
    /// Rust: `field_expression`, Python: `attribute`).
    access_kind: &'static str,
    /// Field name on the access for the receiver.
    receiver_field: &'static str,
    /// Predicate identifying the receiver. TS / JS / Rust filter by
    /// node kind alone (`this` / `self` are dedicated keyword nodes).
    /// Python uses an `identifier` whose source text is `"self"`, so
    /// the predicate is text-based.
    is_receiver: fn(Node<'_>, &[u8]) -> bool,
    /// Field name on the access for the property.
    property_field: &'static str,
    /// Kind of the parent call expression. Most grammars use
    /// `call_expression`; Python's `call` node is the outlier.
    call_kind: &'static str,
}

#[cfg(any(feature = "lang-ts", feature = "lang-js"))]
fn ts_js_is_receiver(node: Node<'_>, _: &[u8]) -> bool {
    node.kind() == "this"
}
#[cfg(feature = "lang-rust")]
fn rust_is_receiver(node: Node<'_>, _: &[u8]) -> bool {
    node.kind() == "self"
}
#[cfg(feature = "lang-py")]
fn py_is_receiver(node: Node<'_>, source: &[u8]) -> bool {
    node.kind() == "identifier" && node.utf8_text(source).is_ok_and(|t| t == "self")
}

#[cfg(any(feature = "lang-ts", feature = "lang-js"))]
const SELF_REF_TS_JS: SelfRefShape = SelfRefShape {
    access_kind: "member_expression",
    receiver_field: "object",
    is_receiver: ts_js_is_receiver,
    property_field: "property",
    call_kind: "call_expression",
};
#[cfg(feature = "lang-rust")]
const SELF_REF_RUST: SelfRefShape = SelfRefShape {
    access_kind: "field_expression",
    receiver_field: "value",
    is_receiver: rust_is_receiver,
    property_field: "field",
    call_kind: "call_expression",
};
#[cfg(feature = "lang-py")]
const SELF_REF_PY: SelfRefShape = SelfRefShape {
    access_kind: "attribute",
    receiver_field: "object",
    is_receiver: py_is_receiver,
    property_field: "attribute",
    call_kind: "call",
};

// Returns `None` for languages whose LCOM is a no-op (Go, Scala);
// when neither feature is enabled the function trivially returns
// `Some`, but we keep the Option so the no-op case stays expressible.
#[cfg_attr(
    not(any(feature = "lang-go", feature = "lang-scala")),
    allow(clippy::unnecessary_wraps)
)]
fn self_ref_shape(lang: Language) -> Option<SelfRefShape> {
    match lang {
        #[cfg(feature = "lang-ts")]
        Language::TypeScript | Language::Tsx => Some(SELF_REF_TS_JS),
        #[cfg(feature = "lang-js")]
        Language::JavaScript | Language::Jsx => Some(SELF_REF_TS_JS),
        #[cfg(feature = "lang-py")]
        Language::Python => Some(SELF_REF_PY),
        // Go has no class scope and Scala's class story needs the LSP
        // backend; both languages opt out of LCOM via
        // `Language::supports_lcom`. `is_method_kind` also returns
        // false, so this branch is only reached if a future caller
        // forgets the supports_lcom gate.
        #[cfg(feature = "lang-go")]
        Language::Go => None,
        #[cfg(feature = "lang-scala")]
        Language::Scala => None,
        #[cfg(feature = "lang-rust")]
        Language::Rust => Some(SELF_REF_RUST),
    }
}

fn collect_self_refs(body: Node<'_>, source: &[u8], lang: Language) -> SelfRefs {
    let mut refs = SelfRefs::default();
    let Some(shape) = self_ref_shape(lang) else {
        return refs;
    };
    let mut cursor = body.walk();
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        visit_self_ref(node, shape, source, &mut refs);
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    refs
}

fn visit_self_ref(node: Node<'_>, shape: SelfRefShape, source: &[u8], refs: &mut SelfRefs) {
    if node.kind() != shape.access_kind {
        return;
    }
    let Some(receiver) = node.child_by_field_name(shape.receiver_field) else {
        return;
    };
    if !(shape.is_receiver)(receiver, source) {
        return;
    }
    let Some(prop) = node.child_by_field_name(shape.property_field) else {
        return;
    };
    let Ok(name) = prop.utf8_text(source) else {
        return;
    };
    let name = name.to_owned();
    if is_call_target(node, shape) {
        refs.method_calls.push(name);
    } else {
        refs.fields.push(name);
    }
}

/// `node` is the receiver expression of a member/field access.
/// Returns true when the parent is a call node whose `function` field
/// is exactly `node`. The parent kind is grammar-specific
/// (`call_expression` for TS / JS / Rust, `call` for Python).
fn is_call_target(node: Node<'_>, shape: SelfRefShape) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != shape.call_kind {
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

pub struct LcomFeature;

impl Feature for LcomFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "lcom",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.metrics.lcom.enabled
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(lc) = reports.lcom.as_ref() else {
            return Vec::new();
        };
        let workspaces = cfg.project.workspaces.as_slice();
        let kept: Vec<_> = lc
            .classes
            .iter()
            .filter(|c| c.cluster_count >= lc.min_cluster_count.max(1))
            .collect();
        let mut out = Vec::with_capacity(kept.len());
        for (class, finding) in kept.iter().zip(lc.into_findings()) {
            let cal_lcom = cal.metrics_for_file(&class.file, workspaces).lcom.as_ref();
            let severity =
                cal_lcom.map_or(Severity::Ok, |c| c.classify(f64::from(class.cluster_count)));
            out.push(decorate(finding, severity, hotspot));
        }
        out
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

    #[cfg(feature = "lang-js")]
    fn run_js(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::JavaScript).unwrap();
        classes_in(&parsed, Path::new("test.js"))
    }

    #[cfg(feature = "lang-py")]
    fn run_py(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::Python).unwrap();
        classes_in(&parsed, Path::new("test.py"))
    }

    #[cfg(feature = "lang-go")]
    fn run_go(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::Go).unwrap();
        classes_in(&parsed, Path::new("test.go"))
    }

    #[cfg(feature = "lang-scala")]
    fn run_scala(source: &str) -> Vec<ClassLcom> {
        let parsed = parse(source.to_owned(), Language::Scala).unwrap();
        classes_in(&parsed, Path::new("test.scala"))
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

    #[cfg(feature = "lang-js")]
    #[test]
    fn js_cohesive_class_has_one_cluster() {
        let src = r"
            class Counter {
                constructor() { this.value = 0; }
                inc() { this.value += 1; }
                dec() { this.value -= 1; }
                get() { return this.value; }
            }
        ";
        let classes = run_js(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_name, "Counter");
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-js")]
    #[test]
    fn js_split_class_has_multiple_clusters() {
        let src = r"
            class Mixed {
                inc() { this.count = (this.count || 0) + 1; }
                value() { return this.count; }
                push(msg) { (this.log = this.log || []).push(msg); }
                tail() { return this.log && this.log[0]; }
            }
        ";
        let classes = run_js(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 2);
    }

    #[cfg(feature = "lang-js")]
    #[test]
    fn js_method_call_merges_clusters() {
        // `tail()` bridges {push, tail} (log) ∪ {inc, value} (count).
        let src = r"
            class Bridged {
                inc() { this.count = (this.count || 0) + 1; }
                value() { return this.count; }
                push(msg) { (this.log = this.log || []).push(msg); }
                tail() {
                    this.value();
                    return this.log && this.log[0];
                }
            }
        ";
        let classes = run_js(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-py")]
    #[test]
    fn py_cohesive_class_has_one_cluster() {
        let src = r"
class Counter:
    def __init__(self):
        self.value = 0

    def inc(self):
        self.value += 1

    def get(self):
        return self.value
";
        let classes = run_py(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].class_name, "Counter");
        assert_eq!(classes[0].cluster_count, 1);
    }

    #[cfg(feature = "lang-py")]
    #[test]
    fn py_split_class_has_multiple_clusters() {
        let src = r"
class Mixed:
    def inc(self):
        self.count = (self.count or 0) + 1

    def value(self):
        return self.count

    def push(self, msg):
        self.log.append(msg)

    def tail(self):
        return self.log[-1]
";
        let classes = run_py(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 2);
    }

    #[cfg(feature = "lang-go")]
    #[test]
    fn go_lcom_is_a_noop_in_v0_2() {
        // Go's class-aware LCOM is deferred — `is_method_kind` returns
        // false for every node, so even a file with multiple methods
        // produces no Findings.
        let src = r"
package x

type Counter struct { value int }

func (c *Counter) Inc() { c.value += 1 }
func (c *Counter) Get() int { return c.value }
";
        assert!(run_go(src).is_empty());
    }

    #[cfg(feature = "lang-scala")]
    #[test]
    fn scala_lcom_is_a_noop_in_v0_2() {
        // Same shape as Go — Scala's class story is too rich for the
        // tree-sitter-approx backend; deferred to v0.3+ LSP.
        let src = r"
class Counter {
  private var value = 0
  def inc(): Unit = value += 1
  def get: Int = value
}
";
        assert!(run_scala(src).is_empty());
    }

    #[cfg(feature = "lang-py")]
    #[test]
    fn py_method_call_merges_clusters() {
        // `tail` touches `log` AND calls `value()` — the bridge.
        let src = r"
class Bridged:
    def inc(self):
        self.count = (self.count or 0) + 1

    def value(self):
        return self.count

    def push(self, msg):
        self.log.append(msg)

    def tail(self):
        self.value()
        return self.log[-1]
";
        let classes = run_py(src);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].cluster_count, 1);
    }
}
