use serde_json::{Value, json};
use std::{fs, path::Path, process::Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_despec"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn config(baseline: &str, extra: &str) -> String {
    format!(
        "schema_version = 1\nrequired_version = \"{}\"\n[outputs]\ncss = \"out.css\"\n[sources]\ninclude = [\"src/**\"]\n{baseline}\n[rules.spacing]\n[rules.radius]\n[tokens.a]\ntype = \"color\"\nvalue = \"#123456\"\n{extra}",
        env!("CARGO_PKG_VERSION")
    )
}
/// A project whose baseline lives inside the scanned tree, so it must be skipped.
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join(".despec.toml"),
        config("baseline = \"src/baseline.json\"", ""),
    )
    .unwrap();
    fs::create_dir(d.path().join("src")).unwrap();
    fs::write(
        d.path().join("src/b.css"),
        "a { margin: 12px; padding: 12px; color: #abcdef; }\n",
    )
    .unwrap();
    fs::write(
        d.path().join("src/a.css"),
        "a { border-radius: 4px; margin: 12px; }\n",
    )
    .unwrap();
    assert!(run(d.path(), &["generate"]).status.success());
    d
}
fn check(d: &Path) -> (bool, Value) {
    let output = run(d, &["check", "--json"]);
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.success(), v["ok"] == true);
    (output.status.success(), v)
}
fn rule_ids(v: &Value) -> Vec<String> {
    v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["rule_id"].as_str().unwrap().to_owned())
        .collect()
}
fn baseline(d: &Path) -> String {
    fs::read_to_string(d.join("src/baseline.json")).unwrap()
}
fn entries(d: &Path) -> Value {
    serde_json::from_str::<Value>(&baseline(d)).unwrap()["entries"].clone()
}

#[test]
fn initial_baseline_is_sorted_and_passes_check() {
    let d = fixture();
    let (ok, v) = check(d.path());
    assert!(!ok);
    // Without a recorded baseline every finding is still reported.
    assert_eq!(rule_ids(&v).len(), 6, "{v}");
    assert_eq!(rule_ids(&v)[5], "baseline.invalid");
    assert!(
        v["diagnostics"][5]["message"]
            .as_str()
            .unwrap()
            .contains("run despec baseline")
    );
    assert!(run(d.path(), &["baseline"]).status.success());
    let text = baseline(d.path());
    assert!(text.ends_with("}\n"));
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"schema_version": 1, "entries": [
            {"rule": "source.color-literal", "path": "src/b.css", "literal": "#abcdef", "count": 1},
            {"rule": "source.radius-literal", "path": "src/a.css", "literal": "4px", "count": 1},
            {"rule": "source.spacing-literal", "path": "src/a.css", "literal": "12px", "count": 1},
            {"rule": "source.spacing-literal", "path": "src/b.css", "literal": "12px", "count": 2},
        ]})
    );
    assert!(text.find("\"rule\"").unwrap() < text.find("\"count\"").unwrap());
    let (ok, v) = check(d.path());
    assert!(ok, "{v}");
    // Running again changes nothing, and moving code to other lines still passes.
    assert!(run(d.path(), &["baseline"]).status.success());
    assert_eq!(baseline(d.path()), text);
    fs::write(
        d.path().join("src/b.css"),
        "\n\nb {\n  padding: 12px;\n}\na { color: #abcdef; margin: 12px; }\n",
    )
    .unwrap();
    assert!(check(d.path()).0);
}

#[test]
fn hand_edited_order_is_rewritten_stably() {
    let d = fixture();
    let shuffled = json!({"schema_version": 1, "entries": [
        {"rule": "source.spacing-literal", "path": "src/b.css", "literal": "12px", "count": 2},
        {"rule": "source.color-literal", "path": "src/b.css", "literal": "#abcdef", "count": 1},
        {"rule": "source.spacing-literal", "path": "src/a.css", "literal": "12px", "count": 1},
        {"rule": "source.radius-literal", "path": "src/a.css", "literal": "4px", "count": 1},
    ]});
    fs::write(d.path().join("src/baseline.json"), shuffled.to_string()).unwrap();
    assert!(check(d.path()).0);
    assert!(run(d.path(), &["baseline"]).status.success());
    let sorted = baseline(d.path());
    fs::remove_file(d.path().join("src/baseline.json")).unwrap();
    assert!(run(d.path(), &["baseline"]).status.success());
    assert_eq!(baseline(d.path()), sorted);
}

#[test]
fn growth_fails_check_and_is_never_recorded() {
    let d = fixture();
    assert!(run(d.path(), &["baseline"]).status.success());
    let before = baseline(d.path());
    fs::write(
        d.path().join("src/a.css"),
        "a { border-radius: 4px; margin: 12px; padding: 12px; }\n",
    )
    .unwrap();
    let (ok, v) = check(d.path());
    assert!(!ok);
    let ds = v["diagnostics"].as_array().unwrap();
    assert_eq!(ds.len(), 2, "{v}");
    for (d, line) in ds.iter().zip([1, 1]) {
        assert_eq!(d["rule_id"], "source.spacing-literal");
        assert_eq!(d["path"], "src/a.css");
        assert_eq!(d["line"], line);
        assert!(
            d["message"]
                .as_str()
                .unwrap()
                .ends_with("(found 2, baseline 1)")
        );
    }
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "baseline.increase: src/a.css: source.spacing-literal 12px: found 2, baseline 1"
        ),
        "{stderr}"
    );
    assert_eq!(baseline(d.path()), before);

    // A key the baseline does not list is an increase from zero.
    fs::write(
        d.path().join("src/a.css"),
        "a { border-radius: 4px; margin: 12px; gap: 8px; padding: 1rem; }\n",
    )
    .unwrap();
    let (ok, v) = check(d.path());
    assert!(!ok);
    assert_eq!(v["diagnostics"][0]["literal"], "1rem", "{v}");
    assert!(
        v["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("found 1, baseline 0")
    );
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("1rem: found 1, baseline 0"));
    assert_eq!(baseline(d.path()), before);
}

#[test]
fn shrinking_is_stale_until_recorded() {
    let d = fixture();
    assert!(run(d.path(), &["baseline"]).status.success());
    fs::write(
        d.path().join("src/b.css"),
        "a { padding: 12px; color: var(--ds-a); }\n",
    )
    .unwrap();
    let (ok, v) = check(d.path());
    assert!(!ok);
    assert_eq!(rule_ids(&v), ["baseline.stale", "baseline.stale"], "{v}");
    let messages: Vec<_> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            assert_eq!(d["path"], "src/baseline.json");
            d["message"].as_str().unwrap()
        })
        .collect();
    assert_eq!(
        messages,
        [
            "source.color-literal src/b.css #abcdef: found 0, baseline 1; run despec baseline to lower it",
            "source.spacing-literal src/b.css 12px: found 1, baseline 2; run despec baseline to lower it",
        ]
    );
    assert!(run(d.path(), &["baseline"]).status.success());
    let entries = entries(d.path());
    assert_eq!(entries.as_array().unwrap().len(), 3);
    assert_eq!(entries[0]["rule"], "source.radius-literal");
    assert_eq!(entries[2]["count"], 1);
    assert!(check(d.path()).0);
}

#[test]
fn exceptions_take_precedence_over_the_baseline() {
    let d = fixture();
    let exception = "[[exceptions]]\nrule = \"source.spacing-literal\"\npath = \"src/b.css\"\nliteral = \"12px\"\nreason = \"vendor layout\"\nline = 1\ncount = 2\n";
    // The exception covers both values on line 1, so only line 2 is debt.
    fs::write(
        d.path().join("src/b.css"),
        "a { margin: 12px; padding: 12px; color: #abcdef; }\nb { margin: 12px; }\n",
    )
    .unwrap();
    fs::write(
        d.path().join(".despec.toml"),
        config("baseline = \"src/baseline.json\"", exception),
    )
    .unwrap();
    assert!(run(d.path(), &["baseline"]).status.success());
    let entries = entries(d.path());
    let b12 = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == "src/b.css" && e["literal"] == "12px")
        .unwrap();
    assert_eq!(b12["count"], 1);
    assert!(check(d.path()).0);
    // A surplus exception is reported, and the baseline is not rewritten around it.
    fs::write(
        d.path().join("src/b.css"),
        "a { padding: 1px; color: #abcdef; }\n",
    )
    .unwrap();
    let before = baseline(d.path());
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exception.unused"));
    assert_eq!(baseline(d.path()), before);
}

#[test]
fn invalid_baselines_are_reported_and_kept() {
    let d = fixture();
    for text in [
        "not json".to_owned(),
        json!({"schema_version": 2, "entries": []}).to_string(),
        json!({"schema_version": 1, "entries": [{"rule": "source.spacing-literal", "path": "src/a.css", "literal": "12px", "count": 0}]}).to_string(),
        json!({"schema_version": 1, "entries": [{"rule": "generated.stale", "path": "src/a.css", "literal": "12px", "count": 1}]}).to_string(),
        json!({"schema_version": 1, "entries": [{"rule": "source.spacing-literal", "path": "../a.css", "literal": "12px", "count": 1}]}).to_string(),
        json!({"schema_version": 1, "entries": [], "extra": true}).to_string(),
        json!({"schema_version": 1, "entries": [
            {"rule": "source.spacing-literal", "path": "src/a.css", "literal": "12px", "count": 1},
            {"rule": "source.spacing-literal", "path": "src/a.css", "literal": "12px", "count": 1},
        ]})
        .to_string(),
    ] {
        fs::write(d.path().join("src/baseline.json"), &text).unwrap();
        let (ok, v) = check(d.path());
        assert!(!ok);
        assert!(rule_ids(&v).contains(&"baseline.invalid".to_owned()), "{text}: {v}");
        let output = run(d.path(), &["baseline"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("baseline.invalid: src/baseline.json: "));
        assert_eq!(baseline(d.path()), text);
    }
}

#[test]
fn configuration_and_scope_errors() {
    let d = fixture();
    let p = d.path().join(".despec.toml");
    fs::write(&p, config("", "")).unwrap();
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("config.invalid: .despec.toml: despec baseline requires [sources] baseline")
    );
    for bad in [
        "../b.json",
        "/tmp/b.json",
        "out.css",
        ".despec.toml",
        "./.despec.toml",
    ] {
        fs::write(&p, config(&format!("baseline = \"{bad}\""), "")).unwrap();
        let (ok, v) = check(d.path());
        assert!(!ok);
        assert_eq!(rule_ids(&v), ["config.invalid"], "{bad}");
    }
    // An incomplete scan neither rewrites the baseline nor reports every entry as stale.
    fs::write(&p, config("baseline = \"src/baseline.json\"", "")).unwrap();
    assert!(run(d.path(), &["baseline"]).status.success());
    let before = baseline(d.path());
    fs::write(
        &p,
        config("baseline = \"src/baseline.json\"", "")
            .replace("\"src/**\"", "\"src/**\", \"missing/**\""),
    )
    .unwrap();
    let (ok, v) = check(d.path());
    assert!(!ok);
    assert_eq!(rule_ids(&v), ["source.no-files"], "{v}");
    let output = run(d.path(), &["baseline"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source.no-files"));
    assert_eq!(baseline(d.path()), before);
    // --generated-only ignores the baseline entirely.
    fs::remove_file(d.path().join("src/baseline.json")).unwrap();
    assert!(
        run(d.path(), &["check", "--generated-only"])
            .status
            .success()
    );
}
