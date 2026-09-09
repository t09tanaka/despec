use serde_json::Value;
use std::{fs, process::Command};
fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_despec"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join(".despec.toml"),format!("schema_version = 1\nrequired_version = \"0.1.0\"\n[outputs]\ncss = \"out.css\"\nscss = \"out.scss\"\ndart = \"out.dart\"\n[sources]\ninclude = [\"source.scss\"]\n{}",include_str!("fixtures/tokens.toml"))).unwrap();
    fs::write(
        d.path().join("source.scss"),
        "/* #fff */\n// #123456\na {color: rgba($token, 0.3);}\n",
    )
    .unwrap();
    d
}
#[test]
fn legacy_bodies_match() {
    let d = fixture();
    let result = run(d.path(), &["generate"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (ext, expected) in [
        ("css", include_str!("fixtures/expected.css")),
        ("scss", include_str!("fixtures/expected.scss")),
        ("dart", include_str!("fixtures/expected.dart")),
    ] {
        let actual = fs::read_to_string(d.path().join(format!("out.{ext}"))).unwrap();
        assert_eq!(
            actual.split_once('\n').unwrap().1,
            expected.split_once('\n').unwrap().1,
            "{ext}"
        );
    }
    assert!(run(d.path(), &["check"]).status.success());
}
#[test]
fn stale_and_invalid_are_read_only() {
    let d = fixture();
    assert!(run(d.path(), &["generate"]).status.success());
    fs::write(d.path().join("out.css"), "sentinel").unwrap();
    let result = run(d.path(), &["check", "--json"]);
    assert!(!result.status.success());
    let v: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["rule_id"], "generated.stale");
    assert_eq!(
        fs::read_to_string(d.path().join("out.css")).unwrap(),
        "sentinel"
    );
    let p = d.path().join(".despec.toml");
    let text = fs::read_to_string(&p)
        .unwrap()
        .replace("#1a7f94", "invalid");
    fs::write(p, text).unwrap();
    assert!(!run(d.path(), &["generate"]).status.success());
    assert_eq!(
        fs::read_to_string(d.path().join("out.css")).unwrap(),
        "sentinel"
    );
}
#[test]
fn source_exceptions_and_empty_scan() {
    let d = fixture();
    assert!(run(d.path(), &["generate"]).status.success());
    fs::write(d.path().join("source.scss"), "a {color: #abcdef;}\n").unwrap();
    assert!(!run(d.path(), &["check"]).status.success());
    let p = d.path().join(".despec.toml");
    let mut text = fs::read_to_string(&p).unwrap();
    text.push_str("\n[[exceptions]]\nrule = \"source.color-literal\"\npath = \"source.scss\"\nliteral = \"#abcdef\"\nreason = \"third-party brand\"\nline = 1\n");
    fs::write(&p, text).unwrap();
    assert!(run(d.path(), &["check"]).status.success());
    fs::write(d.path().join("source.scss"), "a {}\n").unwrap();
    assert!(
        String::from_utf8_lossy(&run(d.path(), &["check"]).stderr).contains("exception.unused")
    );
    fs::remove_file(d.path().join("source.scss")).unwrap();
    assert!(String::from_utf8_lossy(&run(d.path(), &["check"]).stderr).contains("source.no-files"));
}
#[test]
fn config_errors_are_json_and_init_never_overwrites() {
    let d = tempfile::tempdir().unwrap();
    assert!(run(d.path(), &["init"]).status.success());
    assert!(!run(d.path(), &["init"]).status.success());
    let p = d.path().join(".despec.toml");
    let base = fs::read_to_string(&p).unwrap();
    for text in [
        base.replace("0.1.0", "9.0.0"),
        base.replace("schema_version = 1", "schema_version = 2"),
        format!("unknown = true\n{base}"),
        format!("schema_version = 1\n{base}"),
        base.replace("generated/tokens.css", ".despec.toml"),
    ] {
        fs::write(&p, text).unwrap();
        let output = run(d.path(), &["check", "--json"]);
        assert!(!output.status.success());
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["diagnostics"][0]["rule_id"], "config.invalid");
    }
}
#[test]
fn validation_collisions_and_dart_safety() {
    use despec::catalog::{render, validate};
    let parse = |s: &str| serde_json::from_str(s).unwrap();
    for input in [
        r##"{"class":{"type":"color","value":"#123456"}}"##,
        r##"{"a.b":{"type":"number","value":1},"a-b":{"type":"number","value":2}}"##,
        r##"{"a":{"type":"shadow","value":{"offsetX":2}}}"##,
        r##"{"a":{"type":"dimension","value":{"value":2,"unit":"px;"}}}"##,
    ] {
        assert!(validate(&parse(input)).is_err());
    }
    let ts = parse(
        r##"{"a":{"type":"typography","value":{"fontSize":{"value":12,"unit":"px"},"fontWeight":101,"lineHeight":1.2}},"b":{"type":"fontFamily","value":"$font"}}"##,
    );
    let result = render(&ts, "dart", "generated").unwrap();
    assert!(result.contains("Map<String, Object>"));
    assert!(result.contains("\\$font"));
}

#[test]
fn exception_growth_and_missing_scope_fail() {
    let d = fixture();
    assert!(run(d.path(), &["generate"]).status.success());
    let p = d.path().join(".despec.toml");
    let mut text = fs::read_to_string(&p).unwrap();
    text.push_str("\n[[exceptions]]\nrule = \"source.color-literal\"\npath = \"source.scss\"\nliteral = \"#abcdef\"\nreason = \"brand\"\n");
    fs::write(&p, &text).unwrap();
    fs::write(
        d.path().join("source.scss"),
        "a {color: #abcdef; background: #abcdef;}",
    )
    .unwrap();
    assert!(!run(d.path(), &["check"]).status.success());
    text = text.replace(
        "include = [\"source.scss\"]",
        "include = [\"source.scss\", \"missing.scss\"]",
    );
    fs::write(&p, text).unwrap();
    assert!(String::from_utf8_lossy(&run(d.path(), &["check"]).stderr).contains("source.no-files"));
    let output = run(d.path(), &["check", "--json", "--unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["diagnostics"][0]["rule_id"],
        "cli.invalid"
    );
}

#[test]
fn vue_html_comments_preserve_strings_and_line_numbers() {
    let d = fixture();
    let config = d.path().join(".despec.toml");
    let text = fs::read_to_string(&config)
        .unwrap()
        .replace("source.scss", "source.vue");
    fs::write(config, text).unwrap();
    fs::write(
        d.path().join("source.vue"),
        concat!(
            "<template>\n",
            "<!-- issue/#1026:\n",
            "  ignore #abcdef and 'quotes'\n",
            "-->\n",
            "<div style=\"color: #123456\" />\n",
            "</template>\n",
            "<script>\n",
            "const quoted = \"<!-- #234567 -->\";\n",
            "const template = `<!-- #345678 -->`;\n",
            "</script>\n",
            "<style>\n",
            "#abcdef { color: var(--ds-x) }\n",
            "a { color: #456789; }\n",
            "</style>\n",
        ),
    )
    .unwrap();
    assert!(run(d.path(), &["generate"]).status.success());
    let output = run(d.path(), &["check", "--json"]);
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = json["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 4, "{json}");
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d["line"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![5, 8, 9, 13]
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d["rule_id"] == "source.color-literal")
    );
}

#[test]
fn standalone_css_id_selector_is_not_a_color() {
    let d = fixture();
    let config = d.path().join(".despec.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("source.scss", "source.css"),
    )
    .unwrap();
    fs::write(
        d.path().join("source.css"),
        "#abcdef { color: var(--ds-x) }\na {color: #abcdef;}\n",
    )
    .unwrap();
    assert!(run(d.path(), &["generate"]).status.success());
    let output = run(d.path(), &["check", "--json"]);
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(json["diagnostics"][0]["line"], 2);
}
