//! Integration coverage for the complexity module: function extraction,
//! CCN, and Cognitive Complexity over hand-crafted TypeScript / TSX snippets
//! whose scores are derived inline so test failures are easy to interpret.

use heal_observer::complexity::{analyze, extract_functions, parse, FunctionMetric};
use heal_observer::lang::Language;

fn analyze_ts(source: &str) -> Vec<FunctionMetric> {
    let parsed = parse(source.to_string(), Language::TypeScript).expect("parse ok");
    analyze(&parsed)
}

fn metric<'a>(metrics: &'a [FunctionMetric], name: &str) -> &'a FunctionMetric {
    metrics
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("no metric named {name}; have {metrics:?}"))
}

#[test]
fn extracts_all_function_shaped_scopes() {
    // 5 scope kinds the registry must handle:
    //   function_declaration, method_definition (x2), arrow_function bound to
    //   const, anonymous function_expression callback.
    let source = r"
function topLevel() { return 1; }

class Box {
    open() { return true; }
    close() { return false; }
}

const arrow = (n: number) => n * 2;

[1, 2, 3].forEach(function (n) { console.log(n); });
";

    let parsed = parse(source.to_string(), Language::TypeScript).expect("parse ok");
    let scopes = extract_functions(&parsed);

    let names: Vec<&str> = scopes.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"topLevel"),
        "missing function_declaration: {names:?}"
    );
    assert!(
        names.contains(&"open"),
        "missing first method_definition: {names:?}"
    );
    assert!(
        names.contains(&"close"),
        "missing second method_definition: {names:?}"
    );
    assert!(
        names.contains(&"arrow"),
        "missing arrow_function via variable_declarator: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("<anonymous@")),
        "missing anonymous function_expression: {names:?}"
    );
    assert_eq!(scopes.len(), 5, "expected exactly 5 scopes, got {scopes:?}");
}

#[test]
fn ccn_baseline_for_empty_function_is_one() {
    let metrics = analyze_ts("function noop() {}");
    let m = metric(&metrics, "noop");
    assert_eq!(m.ccn, 1);
}

#[test]
fn ccn_sums_decision_points() {
    // Hand count for `mixed`:
    //   1 (baseline)
    // + 1 (if)
    // + 1 (for)
    // + 1 (&&)
    // + 1 (ternary)
    // = 5
    let source = r#"
function mixed(xs: number[], flag: boolean) {
    if (flag && xs.length > 0) {        // if + &&
        for (const x of xs) {            // for
            console.log(x > 0 ? "pos" : "neg"); // ternary
        }
    }
}
"#;
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "mixed");
    assert_eq!(m.ccn, 5, "got {m:?}");
}

#[test]
fn cognitive_baseline_for_straight_line_is_zero() {
    let source = "function add(a: number, b: number): number { return a + b; }";
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "add");
    assert_eq!(m.cognitive, 0);
}

#[test]
fn cognitive_nests_with_depth() {
    // From Sonar's PDF: three nested ifs score 1 + 2 + 3 = 6.
    let source = r"
function deep(a: boolean, b: boolean, c: boolean) {
    if (a) {                  // +1 (depth 0)
        if (b) {              // +2 (depth 1)
            if (c) {          // +3 (depth 2)
                return 1;
            }
        }
    }
    return 0;
}
";
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "deep");
    assert_eq!(m.cognitive, 6, "got {m:?}");
}

#[test]
fn cognitive_else_if_chain_does_not_double_nest() {
    // if … else if … else if … else
    //   +1 (if, depth 0)
    //   +1 (else-if, no nesting bonus)
    //   +1 (else-if, no nesting bonus)
    //   +1 (else)
    //   = 4
    let source = r#"
function classify(n: number): string {
    if (n < 0) {
        return "neg";
    } else if (n === 0) {
        return "zero";
    } else if (n < 10) {
        return "small";
    } else {
        return "large";
    }
}
"#;
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "classify");
    assert_eq!(m.cognitive, 4, "got {m:?}");
}

#[test]
fn cognitive_logical_chain_counts_operator_switches() {
    // `a && b || c && d`:
    //   +1 (chain present)
    // + 2 (op switches: && → ||, || → &&)
    // = 3
    let source = "function chain(a: boolean, b: boolean, c: boolean, d: boolean) { return a && b || c && d; }";
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "chain");
    assert_eq!(m.cognitive, 3, "got {m:?}");

    // `a && b && c`: one chain, no switches = +1.
    let source = "function same(a: boolean, b: boolean, c: boolean) { return a && b && c; }";
    let metrics = analyze_ts(source);
    let m = metric(&metrics, "same");
    assert_eq!(m.cognitive, 1, "got {m:?}");
}

#[test]
fn nested_function_isolation() {
    // outer's body has 1 if → CCN 2, Cognitive 1.
    // inner's body has 2 ifs (one nested) → CCN 3, Cognitive 1+2 = 3.
    // outer must NOT include inner's decision points.
    let source = r"
function outer(a: boolean, b: boolean, c: boolean) {
    function inner(x: boolean, y: boolean) {
        if (x) {
            if (y) {
                return 1;
            }
        }
        return 0;
    }
    if (a) {
        return inner(b, c);
    }
    return -1;
}
";
    let metrics = analyze_ts(source);
    let outer = metric(&metrics, "outer");
    let inner = metric(&metrics, "inner");

    assert_eq!(
        outer.ccn, 2,
        "outer CCN should exclude inner's decisions: {outer:?}"
    );
    assert_eq!(
        outer.cognitive, 1,
        "outer Cognitive should exclude inner: {outer:?}"
    );
    assert_eq!(inner.ccn, 3, "inner CCN: {inner:?}");
    assert_eq!(inner.cognitive, 3, "inner Cognitive (1 + 2): {inner:?}");
}

#[test]
fn tsx_grammar_parses_components() {
    let source = r"
type Props = { items: string[] };

function List({ items }: Props) {
    if (items.length === 0) {
        return <p>empty</p>;
    }
    return (
        <ul>
            {items.map((it) => (
                <li key={it}>{it}</li>
            ))}
        </ul>
    );
}
";
    let parsed = parse(source.to_string(), Language::Tsx).expect("tsx parse ok");
    let metrics = analyze(&parsed);

    let list = metric(&metrics, "List");
    assert_eq!(list.ccn, 2, "if adds 1 to baseline 1: {list:?}");
    assert_eq!(list.cognitive, 1, "single if at depth 0: {list:?}");

    // The arrow callback inside .map((it) => ...) is also a scope — it has
    // no decision points, so CCN 1 / Cognitive 0.
    let anon = metrics
        .iter()
        .find(|m| m.name.starts_with("<anonymous@"))
        .expect("anonymous arrow inside .map should be its own scope");
    assert_eq!(anon.ccn, 1);
    assert_eq!(anon.cognitive, 0);
}
