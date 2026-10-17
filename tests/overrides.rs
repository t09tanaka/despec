use serde_json::Value;
use std::{fs, path::Path, process::Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_despec"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

/// A project scanning `apps/**` with the same `source` written to every app, so each
/// app's findings show which settings applied to it.
fn project(apps: &[&str], file: &str, source: &str, extra: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join(".despec.toml"),
        format!(
            "schema_version = 1\nrequired_version = \"{}\"\n[outputs]\ncss = \"out.css\"\n[sources]\ninclude = [\"apps/**\"]\n[tokens.ink]\ntype = \"color\"\nvalue = \"#123456\"\n{extra}",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    for app in apps {
        let dir = d.path().join("apps").join(app);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(file), source).unwrap();
    }
    // Generation fails for an invalid configuration, which `check` then reports.
    run(d.path(), &["generate"]);
    d
}

fn check(d: &tempfile::TempDir) -> Value {
    let output = run(d.path(), &["check", "--json"]);
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.success(), v["ok"] == true);
    v
}

/// (rule_id, path, literal) for every diagnostic.
fn findings(v: &Value) -> Vec<(String, String, String)> {
    v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["rule_id"].as_str().unwrap().to_owned(),
                d["path"].as_str().unwrap().to_owned(),
                d["literal"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn f(rule: &str, app: &str, file: &str, literal: &str) -> (String, String, String) {
    (
        format!("source.{rule}-literal"),
        format!("apps/{app}/{file}"),
        literal.to_owned(),
    )
}

const MARKUP: &str = "<p class=\"rounded-lg rounded-xl\"></p>\n";

#[test]
fn classes_are_replaced_for_matched_files_only() {
    let extra = concat!(
        "[rules.radius]\nclasses = [\"rounded\", \"rounded-lg\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = [\"rounded-xl\"]\n",
    );
    let d = project(&["admin", "web"], "a.html", MARKUP, extra);
    // admin drops rounded-lg and adds rounded-xl; web keeps the table's classes.
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("radius", "admin", "a.html", "rounded-xl"),
            f("radius", "web", "a.html", "rounded-lg"),
        ]
    );
    // An empty list turns class matching off for the matched files.
    let extra = concat!(
        "[rules.radius]\nclasses = [\"rounded-*\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = []\n",
    );
    let d = project(&["admin", "web"], "a.html", MARKUP, extra);
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("radius", "web", "a.html", "rounded-lg"),
            f("radius", "web", "a.html", "rounded-xl"),
        ]
    );
}

#[test]
fn allow_is_replaced_not_merged() {
    let extra = concat!(
        "[rules.spacing]\nallow = [\"8px\"]\n",
        "[[rules.spacing.overrides]]\npaths = [\"apps/admin/**\"]\nallow = [\"12px\"]\n",
    );
    let d = project(
        &["admin", "web"],
        "a.css",
        "a { padding: 8px; margin: 12px; }\n",
        extra,
    );
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("spacing", "admin", "a.css", "8px"),
            f("spacing", "web", "a.css", "12px"),
        ]
    );
}

#[test]
fn keywords_are_replaced() {
    let css = "a { color: black; }\n";
    let on_except_admin = concat!(
        "[rules.color]\nkeywords = true\n",
        "[[rules.color.overrides]]\npaths = [\"apps/admin/**\"]\nkeywords = false\n",
    );
    let d = project(&["admin", "web"], "a.css", css, on_except_admin);
    assert_eq!(
        findings(&check(&d)),
        vec![f("color", "web", "a.css", "black")]
    );
    let only_admin = concat!(
        "[rules.color]\n",
        "[[rules.color.overrides]]\npaths = [\"apps/admin/**\"]\nkeywords = true\n",
    );
    let d = project(&["admin", "web"], "a.css", css, only_admin);
    assert_eq!(
        findings(&check(&d)),
        vec![f("color", "admin", "a.css", "black")]
    );
}

#[test]
fn keys_left_out_keep_the_table_values() {
    let extra = concat!(
        "[rules.radius]\nallow = [\"4px\"]\nclasses = [\"rounded-lg\", \"rounded-xl\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nallow = [\"rounded-lg\"]\n",
    );
    let source = "<p class=\"rounded-lg rounded-xl rounded-[4px]\"></p>\n";
    let d = project(&["admin", "web"], "a.html", source, extra);
    // admin accepts rounded-lg instead of 4px and keeps the table's classes; web
    // uses the table as is.
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("radius", "admin", "a.html", "rounded-xl"),
            f("radius", "admin", "a.html", "rounded-[4px]"),
            f("radius", "web", "a.html", "rounded-lg"),
            f("radius", "web", "a.html", "rounded-xl"),
        ]
    );
    let classes_only = concat!(
        "[rules.spacing]\nallow = [\"8px\"]\n",
        "[[rules.spacing.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = [\"p-2\"]\n",
    );
    let d = project(
        &["admin"],
        "a.html",
        "<p class=\"p-2 p-[8px]\" style=\"margin: 8px\"></p>\n",
        classes_only,
    );
    assert_eq!(
        findings(&check(&d)),
        vec![f("spacing", "admin", "a.html", "p-2")]
    );
}

#[test]
fn only_the_first_matching_override_applies() {
    let extra = concat!(
        "[rules.radius]\nclasses = [\"rounded-lg\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = []\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nclasses = [\"rounded-xl\"]\n",
    );
    let d = project(&["admin", "web"], "a.html", MARKUP, extra);
    assert_eq!(
        findings(&check(&d)),
        vec![f("radius", "web", "a.html", "rounded-xl")]
    );
}

#[test]
fn an_override_that_applies_to_no_file_is_reported() {
    // One matches nothing; the other matches only files the first override takes.
    let extra = concat!(
        "[rules.radius]\nclasses = [\"rounded-lg\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = []\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/*.html\"]\nclasses = [\"rounded-xl\"]\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/docs/**\"]\nclasses = []\n",
    );
    let d = project(&["admin", "web"], "a.html", MARKUP, extra);
    let v = check(&d);
    assert_eq!(
        findings(&v),
        vec![
            f("radius", "web", "a.html", "rounded-lg"),
            ("override.unused".into(), ".despec.toml".into(), "".into()),
            ("override.unused".into(), ".despec.toml".into(), "".into()),
        ]
    );
    assert_eq!(
        v["diagnostics"][1]["message"],
        "rules.radius.overrides[1] applies to no scanned file; paths [\"apps/admin/*.html\"] match none, or only files an earlier override takes"
    );
    assert!(
        v["diagnostics"][2]["message"]
            .as_str()
            .unwrap()
            .starts_with("rules.radius.overrides[2] applies to no scanned file"),
        "{v}"
    );
}

#[test]
fn override_configuration_errors() {
    for extra in [
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = []\n",
        "[rules.radius]\n[[rules.radius.overrides]]\nclasses = []\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"apps/[\"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"\"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"../apps/**\"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"/apps/**\"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nexclude = []\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nclasses = [\"rounded-[\"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nallow = [\" \"]\n",
        "[rules.radius]\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nkeywords = true\n",
        "[rules.color]\n[[rules.color.overrides]]\npaths = [\"apps/**\"]\nallow = [\"#fff\"]\n",
        // Without its table the rule would turn on for every file.
        "[[rules.radius.overrides]]\npaths = [\"apps/**\"]\nclasses = [\"rounded\"]\n",
        "[rules]\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\n",
        "[rules]\nradius.overrides = [{ paths = [\"apps/**\"] }]\n",
    ] {
        let d = project(&["web"], "a.css", "a {}\n", extra);
        let v = check(&d);
        assert_eq!(v["diagnostics"][0]["rule_id"], "config.invalid", "{extra}");
    }
    let d = project(
        &["web"],
        "a.css",
        "a {}\n",
        "[[rules.radius.overrides]]\npaths = [\"apps/**\"]\n",
    );
    assert_eq!(
        check(&d)["diagnostics"][0]["message"],
        "rules.radius.overrides requires the rule's own table: a [rules.radius] header, an inline radius = { ... } under [rules], or another rules.radius key"
    );
    // A table written after its overrides, as dotted keys, or inline still counts.
    for extra in [
        "[[rules.radius.overrides]]\npaths = [\"apps/**\"]\n[rules.radius]\n",
        "[rules]\nradius.classes = []\n[[rules.radius.overrides]]\npaths = [\"apps/**\"]\n",
        "[rules]\nradius = { overrides = [{ paths = [\"apps/**\"] }] }\n",
    ] {
        let d = project(&["web"], "a.css", "a {}\n", extra);
        assert_eq!(check(&d)["ok"], true, "{extra}");
    }
}

#[test]
fn overrides_with_exceptions_and_baseline() {
    let extra = concat!(
        "[rules.spacing]\nallow = [\"8px\"]\n",
        "[[rules.spacing.overrides]]\npaths = [\"apps/admin/**\"]\nallow = [\"12px\"]\n",
        "[[exceptions]]\nrule = \"source.spacing-literal\"\npath = \"apps/admin/a.css\"\nliteral = \"8px\"\nreason = \"legacy print layout\"\n",
    );
    let d = project(
        &["admin", "web"],
        "a.css",
        "a { padding: 8px; margin: 12px; }\n",
        extra,
    );
    // The exception covers the 8px the override no longer allows.
    assert_eq!(
        findings(&check(&d)),
        vec![f("spacing", "web", "a.css", "12px")]
    );
    let config = d.path().join(".despec.toml");
    let text = fs::read_to_string(&config).unwrap().replace(
        "[sources]\n",
        "[sources]\nbaseline = \"despec-baseline.json\"\n",
    );
    fs::write(&config, &text).unwrap();
    assert!(run(d.path(), &["baseline"]).status.success());
    let recorded = fs::read_to_string(d.path().join("despec-baseline.json")).unwrap();
    assert!(
        recorded.contains("\"path\": \"apps/web/a.css\""),
        "{recorded}"
    );
    assert!(!recorded.contains("apps/admin"), "{recorded}");
    assert_eq!(check(&d)["ok"], true);
    // An unused override blocks writing the baseline like other source diagnostics.
    fs::write(
        &config,
        format!("{text}[[rules.spacing.overrides]]\npaths = [\"apps/docs/**\"]\n"),
    )
    .unwrap();
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("override.unused"), "{stderr}");
    assert!(stderr.contains("baseline not written"), "{stderr}");
}

#[test]
fn overrides_apply_to_every_detection_path() {
    let admin =
        "[[rules.radius.overrides]]\npaths = [\"apps/admin/**\"]\nclasses = [\"rounded-xl\"]\n";
    let radius = format!("[rules.radius]\nclasses = [\"rounded-lg\"]\n{admin}");
    // `classes` in `@apply`, in a stylesheet and in a `<style>` block.
    for (file, source) in [
        ("a.css", ".a { @apply rounded-lg rounded-xl; }\n"),
        (
            "a.html",
            "<style>.a { @apply rounded-lg rounded-xl }</style>\n",
        ),
    ] {
        let d = project(&["admin", "web"], file, source, &radius);
        assert_eq!(
            findings(&check(&d)),
            vec![
                f("radius", "admin", file, "rounded-xl"),
                f("radius", "web", file, "rounded-lg"),
            ],
            "{file}"
        );
    }
    // `allow` for a Tailwind arbitrary value and for a Dart argument.
    for (rule, file, source, allow, literal) in [
        (
            "spacing",
            "a.html",
            "<p class=\"p-[12px]\"></p>\n",
            "12px",
            "p-[12px]",
        ),
        (
            "gap",
            "a.dart",
            "const box = SizedBox(width: 8);\n",
            "8",
            "8",
        ),
    ] {
        let extra = format!(
            "[rules.{rule}]\n[[rules.{rule}.overrides]]\npaths = [\"apps/admin/**\"]\nallow = [\"{allow}\"]\n"
        );
        let d = project(&["admin", "web"], file, source, &extra);
        assert_eq!(
            findings(&check(&d)),
            vec![f(rule, "web", file, literal)],
            "{file}"
        );
    }
    // `keywords` for a markup attribute and a Tailwind arbitrary value.
    let d = project(
        &["admin", "web"],
        "a.html",
        "<rect fill=\"white\"></rect><p class=\"bg-[white]\"></p>\n",
        "[rules.color]\n[[rules.color.overrides]]\npaths = [\"apps/admin/**\"]\nkeywords = true\n",
    );
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("color", "admin", "a.html", "white"),
            f("color", "admin", "a.html", "bg-[white]"),
        ]
    );
    // The overrides of an inline rule table apply too; dotted keys that only
    // name overrides leave the table implied and are rejected.
    let inline = "[rules]\nradius = { classes = [\"rounded-lg\"], overrides = [{ paths = [\"apps/admin/**\"], classes = [\"rounded-xl\"] }] }\n";
    let d = project(&["admin", "web"], "a.html", MARKUP, inline);
    assert_eq!(
        findings(&check(&d)),
        vec![
            f("radius", "admin", "a.html", "rounded-xl"),
            f("radius", "web", "a.html", "rounded-lg"),
        ]
    );
    let dotted = "[rules]\nradius.overrides = [{ paths = [\"apps/admin/**\"], classes = [\"rounded-xl\"] }]\n";
    let d = project(&["admin", "web"], "a.html", MARKUP, dotted);
    assert_eq!(check(&d)["diagnostics"][0]["rule_id"], "config.invalid");
}
