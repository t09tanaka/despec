use serde_json::Value;
use std::{fs, process::Command};

fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_despec"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

/// A project scanning one source file, with `extra` appended to the configuration.
fn project(file: &str, source: &str, extra: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join(".despec.toml"),
        format!(
            "schema_version = 1\nrequired_version = \"{}\"\n[outputs]\ncss = \"out.css\"\n[sources]\ninclude = [\"{file}\"]\n[tokens.ink]\ntype = \"color\"\nvalue = \"#123456\"\n{extra}",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    fs::write(d.path().join(file), source).unwrap();
    run(d.path(), &["generate"]);
    d
}

fn check(d: &tempfile::TempDir) -> Value {
    let output = run(d.path(), &["check", "--json"]);
    serde_json::from_slice(&output.stdout).unwrap()
}

/// (rule_id, line, literal) for every diagnostic.
fn findings(file: &str, source: &str, extra: &str) -> Vec<(String, u64, String)> {
    let v = check(&project(file, source, extra));
    v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["rule_id"].as_str().unwrap().to_owned(),
                d["line"].as_u64().unwrap_or(0),
                d["literal"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn f(rule: &str, line: u64, literal: &str) -> (String, u64, String) {
    (format!("source.{rule}-literal"), line, literal.to_owned())
}

const ALL_RULES: &str = "[rules.spacing]\n[rules.gap]\n[rules.border-width]\n[rules.outline]\n[rules.shadow]\n[rules.radius]\n[rules.font-size]\n";

#[test]
fn rules_are_off_unless_configured() {
    let css = "a { padding: 12px; border: 2px solid #ccc; }\n";
    assert_eq!(findings("a.css", css, ""), vec![f("color", 1, "#ccc")]);
    assert_eq!(
        findings("a.css", css, "[rules.spacing]\n"),
        vec![f("spacing", 1, "12px"), f("color", 1, "#ccc")]
    );
}

#[test]
fn css_declarations_per_rule() {
    let css = concat!(
        "a {\n",
        "  margin: 4px 8px;\n",
        "  padding-inline-start: 1.5rem;\n",
        "  margin-top: var(--ds-space, 6px);\n",
        "  margin-left: calc(100% - 10px);\n",
        "  margin-bottom: -3px;\n",
        "  row-gap: 5px; column-gap: 7px; gap: 9px; grid-gap: 11px;\n",
        "  border-top-width: 1px; outline: 3px solid; outline-offset: 2px;\n",
        "  box-shadow: 0 1px 2px var(--ds-ink); text-shadow: 0 0 4px;\n",
        "  border-radius: 6px; border-top-left-radius: 50%;\n",
        "  font-size: 13px;\n",
        "}\n",
    );
    assert_eq!(
        findings("a.scss", css, ALL_RULES),
        vec![
            f("spacing", 2, "4px"),
            f("spacing", 2, "8px"),
            f("spacing", 3, "1.5rem"),
            f("spacing", 4, "6px"),
            f("spacing", 5, "100%"),
            f("spacing", 5, "10px"),
            f("spacing", 6, "-3px"),
            f("gap", 7, "5px"),
            f("gap", 7, "7px"),
            f("gap", 7, "9px"),
            f("gap", 7, "11px"),
            f("border-width", 8, "1px"),
            f("outline", 8, "3px"),
            f("outline", 8, "2px"),
            f("shadow", 9, "1px"),
            f("shadow", 9, "2px"),
            f("shadow", 9, "4px"),
            f("radius", 10, "6px"),
            f("radius", 10, "50%"),
            f("font-size", 11, "13px"),
        ]
    );
}

#[test]
fn css_lookalikes_are_not_dimensions() {
    let css = concat!(
        "/* padding: 12px */\n",
        "// margin: 13px\n",
        "@media (min-width: 768px) { a { color: var(--ds-ink); } }\n",
        ":root { --card-padding: 12px; }\n",
        "a { padding: var(--space-12); margin: $space-2; gap: $gap2px; }\n",
        "a { scroll-margin: 8px; line-height: 20px; letter-spacing: 1px; width: 10px; }\n",
        "a { padding: 0; margin: 0px 0 -0rem; border-radius: 4px; font-size: 1REM; }\n",
        "a { border-color: var(--x); border-style: solid; border-spacing: 2px; }\n",
        "a { background: url(img-12px.png); }\n",
        "$gap: 12px;\n",
    );
    let rules = "[rules.spacing]\n[rules.gap]\n[rules.border-width]\n[rules.radius]\nallow = [\"4px\"]\n[rules.font-size]\nallow = [\"1rem\"]\n";
    assert_eq!(findings("a.scss", css, rules), vec![]);
}

#[test]
fn stylesheet_values_wrap_over_lines() {
    let css = concat!(
        "a {\n",
        "  box-shadow:\n",
        "    0 1px 2px rgba(0,0,0,.1),\n",
        "    0 4px 8px rgba(0,0,0,.2);\n",
        "  margin:\n",
        "    8px\n",
        "    16px;\n",
        "}\n",
        "a { margin: 4px }\n",
        "a { padding: 3px\n  b { color: var(--x) } }\n",
    );
    let expected = vec![
        f("shadow", 3, "1px"),
        f("shadow", 3, "2px"),
        f("color", 3, "rgba(0,0,0,.1)"),
        f("shadow", 4, "4px"),
        f("shadow", 4, "8px"),
        f("color", 4, "rgba(0,0,0,.2)"),
        f("spacing", 6, "8px"),
        f("spacing", 7, "16px"),
        f("spacing", 9, "4px"),
        f("spacing", 10, "3px"),
    ];
    for file in ["a.css", "a.scss", "a.less"] {
        assert_eq!(findings(file, css, ALL_RULES), expected, "{file}");
    }
    // Indented Sass has no semicolons, so its values still end at the line break.
    assert_eq!(
        findings(
            "a.sass",
            "a\n  margin: 4px\n  width: 10px 20px\n",
            ALL_RULES
        ),
        vec![f("spacing", 2, "4px")]
    );
    // Markup still ends a value at the line break.
    assert_eq!(
        findings(
            "a.vue",
            "<style>\na {\n  margin:\n    8px;\n}\n</style>\n",
            ALL_RULES
        ),
        vec![]
    );
}

#[test]
fn css_in_js_templates() {
    assert_eq!(
        findings(
            "a.ts",
            "const a = css`padding: 8px;`;\nconst b = `margin: 4px`;\n",
            ALL_RULES
        ),
        vec![f("spacing", 1, "8px"), f("spacing", 2, "4px")]
    );
}

#[test]
fn color_and_dimension_on_one_line() {
    assert_eq!(
        findings("a.css", "a { border: 2px solid #ccc; }\n", ALL_RULES),
        vec![f("border-width", 1, "2px"), f("color", 1, "#ccc")]
    );
}

#[test]
fn markup_and_tailwind_arbitrary_values() {
    let tsx = concat!(
        "<div style=\"padding: 3px\" className=\"md:p-[13px] hover:!-mt-[4px] gap-x-[6px]\" />\n",
        "<div className={'border-[2px] ring-offset-[1px] shadow-[0_1px_2px_#000]'} />\n",
        "<div className=\"rounded-tl-[8px] text-[15px] p-[calc(100%_-_8px)]\" />\n",
        "<div className=\"text-[#fff] text-[length:var(--x)] text-[rgb(0_0_0/50%)] text-[color:var(--y)]\" />\n",
        "<div className=\"p-[0px] w-[13px] border-[#ccc] top-p-[3px] p-4 rounded-[4px]\" />\n",
        "<div style={{ marginTop: 8, padding: '0 auto' }} />\n",
    );
    let rules = "[rules.spacing]\n[rules.gap]\n[rules.border-width]\n[rules.outline]\n[rules.shadow]\n[rules.radius]\nallow = [\"4px\"]\n[rules.font-size]\n";
    let mut expected = vec![
        f("spacing", 1, "3px"),
        f("spacing", 1, "p-[13px]"),
        f("spacing", 1, "mt-[4px]"),
        f("gap", 1, "gap-x-[6px]"),
        f("border-width", 2, "border-[2px]"),
        f("outline", 2, "ring-offset-[1px]"),
        f("shadow", 2, "shadow-[0_1px_2px_#000]"),
        f("color", 2, "#000"),
        f("radius", 3, "rounded-tl-[8px]"),
        f("font-size", 3, "text-[15px]"),
        f("spacing", 3, "p-[calc(100%_-_8px)]"),
        f("color", 4, "#fff"),
        f("color", 5, "#ccc"),
    ];
    let mut actual = findings("a.tsx", tsx, rules);
    // Color findings precede dimension findings at the same offset; compare as sets
    // of (line, rule, literal) because the shadow class and its color share a line.
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
}

#[test]
fn dart_arguments_per_rule() {
    let dart = concat!(
        "final a = EdgeInsets.symmetric(\n",
        "  horizontal: 16,\n",
        "  vertical: ds.space4,\n",
        ");\n",
        "final b = EdgeInsetsDirectional.fromSTEB(8, 0, spacing4, 2.5);\n",
        "final c = SizedBox(width: 12, height: ds.gap, child: Text('24'));\n",
        "final d = Wrap(spacing: 6, runSpacing: 0, children: []);\n",
        "final e = BorderSide(color: ink, width: 1.5);\n",
        "final g = BorderRadius.circular(10);\n",
        "final h = BoxShadow(blurRadius: 4, spreadRadius: -1, offset: Offset(0, 3));\n",
        "final i = TextStyle(fontSize: 14);\n",
        "final j = Transform.translate(offset: Offset(0, 4));\n",
        "final k = SizedBox(width: math.max(1, 2));\n",
        "final l = EdgeInsets.all(gap(5));\n",
        "// final m = EdgeInsets.all(7);\n",
        "final n = Radius.elliptical(3, 9);\n",
        "final o = BorderRadius.circular(4);\n",
    );
    let rules = "[rules.spacing]\n[rules.gap]\n[rules.border-width]\n[rules.outline]\n[rules.shadow]\n[rules.radius]\nallow = [\"4\"]\n[rules.font-size]\n";
    let mut actual = findings("a.dart", dart, rules);
    actual.sort_by_key(|(_, line, _)| *line);
    assert_eq!(
        actual,
        vec![
            f("spacing", 2, "16"),
            f("spacing", 5, "8"),
            f("spacing", 5, "2.5"),
            f("gap", 6, "12"),
            f("gap", 7, "6"),
            f("border-width", 8, "1.5"),
            f("radius", 9, "10"),
            f("shadow", 10, "4"),
            f("shadow", 10, "-1"),
            f("shadow", 10, "3"),
            f("font-size", 11, "14"),
            f("radius", 16, "3"),
            f("radius", 16, "9"),
        ]
    );
}

#[test]
fn forbidden_classes() {
    let html = "<a class=\"rounded md:rounded-lg !rounded-t px-4 [&:hover]:p-2\">rounded</a>\n";
    let rules = "[rules.radius]\nclasses = [\"rounded\", \"rounded-*\"]\n[rules.spacing]\nclasses = [\"p-*\", \"px-*\"]\nallow = [\"px-4\"]\n[rules.color]\nclasses = [\"bg-red-*\"]\n";
    assert_eq!(
        findings("a.html", html, rules),
        vec![
            f("radius", 1, "rounded"),
            f("radius", 1, "rounded-lg"),
            f("radius", 1, "rounded-t"),
            f("spacing", 1, "p-2"),
        ]
    );
    assert_eq!(
        findings("a.vue", "<b class=\"bg-red-500 rounded-[8px]\" />\n", rules),
        vec![f("color", 1, "bg-red-500"), f("radius", 1, "rounded-[8px]")]
    );
    // Quoted object keys count, and an unmatched bracket does not swallow later words.
    assert_eq!(
        findings(
            "a.tsx",
            "const open = \"[\";\n<b className={cn({ 'rounded': on })} />\n",
            rules
        ),
        vec![f("radius", 2, "rounded")]
    );
    // Outside `@apply`, stylesheets are never matched against class names.
    assert_eq!(
        findings("a.css", ".rounded { padding: var(--x); }\n", rules),
        vec![]
    );
}

#[test]
fn dimension_exceptions_match_rule_and_literal() {
    let source = "a { padding: 12px; margin: 12px; }\n";
    let exception = |rule: &str, literal: &str, count: usize| {
        format!(
            "[rules.spacing]\n[[exceptions]]\nrule = \"{rule}\"\npath = \"a.css\"\nliteral = \"{literal}\"\nreason = \"vendor layout\"\ncount = {count}\nline = 1\n"
        )
    };
    let d = project(
        "a.css",
        source,
        &exception("source.spacing-literal", "12px", 2),
    );
    assert_eq!(check(&d)["ok"], true);
    // A color exception never suppresses a dimension with the same literal.
    let d = project(
        "a.css",
        source,
        &exception("source.color-literal", "12px", 2),
    );
    let v = check(&d);
    let ids: Vec<_> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["rule_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "source.spacing-literal",
            "source.spacing-literal",
            "exception.unused"
        ]
    );
    assert_eq!(v["diagnostics"][0]["literal"], "12px");
    assert_eq!(
        v["diagnostics"][0]["message"],
        "use a spacing token instead of 12px"
    );
    assert!(v["diagnostics"][2].get("literal").is_none());
    assert!(
        v["diagnostics"][2]["message"]
            .as_str()
            .unwrap()
            .starts_with("exception count mismatch for 12px:"),
        "{v}"
    );
    let d = project(
        "a.css",
        "a {}\n",
        &exception("source.spacing-literal", "12px", 1),
    );
    let v = check(&d);
    assert_eq!(v["diagnostics"][0]["rule_id"], "exception.unused");
    assert!(
        v["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("exception count mismatch for source.spacing-literal 12px:"),
        "{v}"
    );
}

#[test]
fn rule_configuration_errors() {
    for extra in [
        "[rules.color]\nallow = [\"#fff\"]\n",
        "[rules.bogus]\n",
        "[rules.spacing]\nfoo = 1\n",
        "[rules.spacing]\nallow = [\"\"]\n",
        "[rules.radius]\nclasses = [\"rounded-[\"]\n",
        "[[exceptions]]\nrule = \"source.gap-literal\"\npath = \"a.css\"\nliteral = \"1px\"\nreason = \"x\"\n",
        "[[exceptions]]\nrule = \"source.line-height-literal\"\npath = \"a.css\"\nliteral = \"1px\"\nreason = \"x\"\n",
    ] {
        let d = project("a.css", "a {}\n", extra);
        let v = check(&d);
        assert_eq!(v["diagnostics"][0]["rule_id"], "config.invalid", "{extra}");
    }
}

#[test]
fn apply_classes_in_stylesheets() {
    let rules = "[rules.radius]\nclasses = [\"rounded-*\"]\n[rules.spacing]\nclasses = [\"p-*\"]\n";
    let css = concat!(
        ".card {\n",
        "  @apply p-4 md:rounded-lg !important;\n",
        "  padding: var(--x);\n",
        "  margin: 0;\n",
        "}\n",
        ".b { @apply\n  rounded-md\n  hover:p-2 }\n",
    );
    let expected = vec![
        f("spacing", 2, "p-4"),
        f("radius", 2, "rounded-lg"),
        f("radius", 7, "rounded-md"),
        f("spacing", 8, "p-2"),
    ];
    assert_eq!(findings("a.css", css, rules), expected);
    assert_eq!(findings("a.scss", css, rules), expected);
    assert_eq!(findings("a.less", css, rules), expected);
    // Indented Sass ends `@apply` at the line break.
    assert_eq!(
        findings(
            "a.sass",
            ".card\n  @apply p-4 rounded-lg\n  rounded-sm: x\n",
            rules
        ),
        vec![f("spacing", 2, "p-4"), f("radius", 2, "rounded-lg")]
    );
    // Declarations after `@apply` are checked as before, and an arbitrary value found
    // by both checks is reported once.
    assert_eq!(
        findings(
            "a.css",
            "a { @apply p-[12px] rounded-lg; padding: 8px; }\n",
            rules
        ),
        vec![
            f("spacing", 1, "p-[12px]"),
            f("radius", 1, "rounded-lg"),
            f("spacing", 1, "8px"),
        ]
    );
    // Vue `<style>`: `@apply` counts with its trailing `;` removed, and in
    // `border: 1px` only the width is reported, not `border` as a class.
    let vue = concat!(
        "<template><b class=\"border\" /></template>\n",
        "<style>\n",
        ".a { @apply p-4 rounded-lg; border: 1px solid; }\n",
        ".b {\n  @apply rounded-md\n    p-2;\n}\n",
        "</style>\n",
    );
    let border = "[rules.border-width]\nclasses = [\"border\", \"border-*\"]\n";
    // An allowed class in `@apply` stays allowed, and `!important` and a bracketed
    // `;` neither count nor end the arguments.
    assert_eq!(
        findings(
            "a.vue",
            "<style>\n.a { @apply p-4 bg-[url('a;b')] rounded-lg; }\n.b { @apply p-2 !important }\n</style>\n",
            "[rules.radius]\nclasses = [\"rounded-*\"]\nallow = [\"rounded-lg\"]\n[rules.spacing]\nclasses = [\"p-*\", \"bg-*\", \"important*\", \"@apply\"]\nallow = [\"bg-[url('a;b')]\"]\n"
        ),
        vec![f("spacing", 2, "p-4"), f("spacing", 3, "p-2")]
    );
    assert_eq!(
        findings("a.vue", vue, &format!("{rules}{border}")),
        vec![
            f("border-width", 1, "border"),
            f("spacing", 3, "p-4"),
            f("radius", 3, "rounded-lg"),
            f("border-width", 3, "1px"),
            f("radius", 5, "rounded-md"),
            f("spacing", 6, "p-2"),
        ]
    );
}

#[test]
fn words_ending_in_a_colon_are_not_classes() {
    let rules = "[rules.border-width]\nclasses = [\"border\", \"border-*\"]\n[rules.outline]\nclasses = [\"ring\"]\n";
    let tsx = concat!(
        "const look = { border: '1px solid', ring: true };\n",
        "type Box = { border: number };\n",
        "const flags = { 'border-2': on };\n",
        "<b className=\"border ring\" />\n",
    );
    assert_eq!(
        findings("a.tsx", tsx, rules),
        vec![
            f("border-width", 3, "border-2"),
            f("border-width", 4, "border"),
            f("outline", 4, "ring"),
        ]
    );
}
