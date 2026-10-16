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
    fs::write(d.path().join(".despec.toml"),format!("schema_version = 1\nrequired_version = \"{}\"\n[outputs]\ncss = \"out.css\"\nscss = \"out.scss\"\ndart = \"out.dart\"\n[sources]\ninclude = [\"source.scss\"]\n{}",env!("CARGO_PKG_VERSION"),include_str!("fixtures/tokens.toml"))).unwrap();
    fs::write(
        d.path().join("source.scss"),
        "/* #fff */\n// #123456\na {color: rgba($token, 0.3);}\n",
    )
    .unwrap();
    d
}
#[test]
fn generated_bodies_match() {
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
fn dart_typography_letter_spacing_conversion_and_fallback() {
    use despec::catalog::render;
    use serde_json::json;
    for (size, unit, spacing, weight, expected) in [
        (
            24.0,
            "px",
            json!({"value": -0.015, "unit": "em"}),
            600,
            Some("-0.36"),
        ),
        (
            12.5,
            "px",
            json!({"value": 0.02, "unit": "em"}),
            600,
            Some("0.25"),
        ),
        (
            24.0,
            "px",
            json!({"value": -0.0, "unit": "em"}),
            600,
            Some("0"),
        ),
        (
            24.0,
            "px",
            json!({"value": 0.72, "unit": "px"}),
            600,
            Some("0.72"),
        ),
        (24.0, "px", json!(0), 600, Some("0")),
        (24.0, "px", json!(1.5), 600, Some("1.5")),
        (
            1.5,
            "rem",
            json!({"value": -0.015, "unit": "em"}),
            600,
            None,
        ),
        (24.0, "px", json!({"value": 0.02, "unit": "rem"}), 600, None),
        (24.0, "px", json!({"value": 0.02, "unit": "em"}), 550, None),
        (1e308, "px", json!({"value": 2, "unit": "em"}), 600, None),
    ] {
        let tokens = serde_json::from_value(json!({"title": {
            "type": "typography",
            "value": {"fontSize": {"value": size, "unit": unit},
                "fontWeight": weight, "lineHeight": 1.25, "letterSpacing": spacing}
        }}))
        .unwrap();
        let dart = render(&tokens, &Default::default(), "dart", "generated").unwrap();
        if let Some(expected) = expected {
            assert!(
                dart.contains("const TextStyle title = TextStyle("),
                "{dart}"
            );
            assert!(
                dart.contains(&format!("  letterSpacing: {expected},")),
                "{dart}"
            );
        } else {
            assert!(
                dart.contains("const Map<String, Object> title = <String, Object>{"),
                "{dart}"
            );
            let scss = render(&tokens, &Default::default(), "scss", "generated").unwrap();
            let original = scss
                .lines()
                .find_map(|line| line.strip_prefix("$title-letter-spacing: "))
                .unwrap()
                .trim_end_matches(';');
            assert!(
                dart.contains(&format!("  'letterSpacing': '{original}',")),
                "{dart}"
            );
        }
    }
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
        base.replace(env!("CARGO_PKG_VERSION"), "9.0.0"),
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
        assert!(validate(&parse(input), &Default::default()).is_err());
    }
    let ts = parse(
        r##"{"a":{"type":"typography","value":{"fontSize":{"value":12,"unit":"px"},"fontWeight":101,"lineHeight":1.2}},"b":{"type":"fontFamily","value":"$font"}}"##,
    );
    let result = render(&ts, &Default::default(), "dart", "generated").unwrap();
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

#[test]
fn one_target_may_have_several_destinations() {
    let d = fixture();
    let p = d.path().join(".despec.toml");
    let text = fs::read_to_string(&p)
        .unwrap()
        .replace(
            "css = \"out.css\"",
            "css = [\"out.css\", \"admin/out.css\"]",
        )
        .replace(
            "include = [\"source.scss\"]",
            "include = [\"source.scss\", \"admin/*.css\"]",
        );
    fs::write(&p, text).unwrap();
    fs::create_dir(d.path().join("admin")).unwrap();
    // A handwritten file keeps the admin include pattern from selecting only generated output.
    fs::write(
        d.path().join("admin/base.css"),
        "a { color: var(--ds-accent-500); }\n",
    )
    .unwrap();
    assert!(run(d.path(), &["generate"]).status.success());
    assert_eq!(
        fs::read_to_string(d.path().join("out.css")).unwrap(),
        fs::read_to_string(d.path().join("admin/out.css")).unwrap()
    );
    // The second destination is a generated file too: never scanned, and stale on its own.
    assert!(run(d.path(), &["check"]).status.success());
    fs::write(d.path().join("admin/out.css"), "a { color: #abcdef; }").unwrap();
    let result = run(d.path(), &["check", "--json"]);
    assert!(!result.status.success());
    let v: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(v["diagnostics"][0]["rule_id"], "generated.stale");
    assert_eq!(v["diagnostics"][0]["path"], "admin/out.css");
    // An empty list and a duplicate destination are configuration errors.
    for bad in ["css = []", "css = [\"out.css\", \"out.css\"]"] {
        let text = fs::read_to_string(&p)
            .unwrap()
            .replace("css = [\"out.css\", \"admin/out.css\"]", bad);
        fs::write(&p, text).unwrap();
        let result = run(d.path(), &["check", "--json"]);
        assert!(!result.status.success(), "{bad}");
        let v: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(v["diagnostics"][0]["rule_id"], "config.invalid", "{bad}");
        let text = fs::read_to_string(&p)
            .unwrap()
            .replace(bad, "css = [\"out.css\", \"admin/out.css\"]");
        fs::write(&p, text).unwrap();
    }
}

fn modes_fixture(extra: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join(".despec.toml"),
        format!(
            r##"schema_version = 1
required_version = "{}"
[outputs]
css = "out.css"
scss = "out.scss"
dart = "out.dart"
[modes.dark]
css = ".dark"
[modes.high-contrast]
css = "@media (prefers-contrast: more)"
[tokens]
"bg.canvas" = {{ type = "color", value = "#ffffff", modes = {{ dark = "#0b0d10", high-contrast = "#000000" }} }}
"shadow.card" = {{ type = "shadow", value = {{ offsetX = {{ value = 0, unit = "px" }}, offsetY = {{ value = 1, unit = "px" }}, blur = {{ value = 2, unit = "px" }}, color = "rgba(0, 0, 0, 0.1)" }}, modes = {{ dark = "none" }} }}
"space.md" = {{ type = "dimension", value = {{ value = 16, unit = "px" }} }}
{extra}"##,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    d
}
#[test]
fn mode_values_render_per_target() {
    let d = modes_fixture("");
    let result = run(d.path(), &["generate"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let body = |ext: &str| {
        fs::read_to_string(d.path().join(format!("out.{ext}")))
            .unwrap()
            .split_once('\n')
            .unwrap()
            .1
            .to_owned()
    };
    assert_eq!(
        body("css"),
        "
:root {
  --ds-bg-canvas: #ffffff;
  --ds-shadow-card: 0 1px 2px rgba(0, 0, 0, 0.1);
  --ds-space-md: 16px;
}

.dark {
  --ds-bg-canvas: #0b0d10;
  --ds-shadow-card: none;
}

@media (prefers-contrast: more) {
  :root {
    --ds-bg-canvas: #000000;
  }
}
"
    );
    assert_eq!(
        body("scss"),
        "
$bg-canvas: #ffffff;
$bg-canvas-dark: #0b0d10;
$bg-canvas-high-contrast: #000000;
$shadow-card: 0 1px 2px rgba(0, 0, 0, 0.1);
$shadow-card-dark: none;
$space-md: 16px;
"
    );
    let dart = body("dart");
    for line in [
        "const Color bgCanvas = Color(0xFFFFFFFF);",
        "const Color bgCanvasDark = Color(0xFF0B0D10);",
        "const Color bgCanvasHighContrast = Color(0xFF000000);",
        "const String shadowCardDark = 'none';",
    ] {
        assert!(dart.contains(line), "{line}\n{dart}");
    }
    assert!(
        run(d.path(), &["check", "--generated-only"])
            .status
            .success()
    );
}
#[test]
fn mode_errors_fail_before_writing() {
    for (extra, rule) in [
        (
            r##""fg" = { type = "color", value = "#111111", modes = { sepia = "#222222" } }"##,
            "token.invalid",
        ),
        (
            r##""gap" = { type = "dimension", value = { value = 1, unit = "px" }, modes = { dark = { value = 2, unit = "px" } } }"##,
            "token.invalid",
        ),
        (
            r##""fg" = { type = "color", value = "#111111", modes = { dark = "#fff" } }"##,
            "token.invalid",
        ),
        (
            r##""fg" = { type = "color", value = "#111111", modes = {} }"##,
            "token.invalid",
        ),
        (
            r##""bg.canvas-dark" = { type = "color", value = "#111111" }"##,
            "token.invalid",
        ),
        ("[modes.unused]\ncss = \".unused\"", "token.invalid"),
        ("[modes.Bad]\ncss = \".bad\"", "config.invalid"),
        (
            "[modes.bad]\ncss = \".bad { color: red }\"",
            "config.invalid",
        ),
        ("[modes.bad]", "config.invalid"),
        ("[modes.bad]\ncss = \".bad /*\"", "config.invalid"),
        ("[modes.bad]\ncss = '[data-theme=\"bad]'", "config.invalid"),
        ("[modes.bad]\ncss = \"@import url(x)\"", "config.invalid"),
        ("[modes.bad]\ncss = \"@media \"", "config.invalid"),
        ("[modes.bad]\ncss = \".a)(\"", "config.invalid"),
        (
            r##""fx" = { type = "shadow", value = "none", modes = { dark = { offsetX = 1 } } }"##,
            "token.invalid",
        ),
        (
            r##""fg" = { type = "color", value = "#111111", targets = { scss = "ink" }, modes = { dark = "#222222" } }
"ink.dark" = { type = "color", value = "#333333" }"##,
            "token.invalid",
        ),
        (
            "[modes.a-1]\ncss = \".a-1\"\n[modes.a1]\ncss = \".a1\"\n[tokens.fg]\ntype = \"color\"\nvalue = \"#111111\"\nmodes = { a-1 = \"#222222\", a1 = \"#333333\" }",
            "token.invalid",
        ),
    ] {
        let d = modes_fixture(extra);
        assert!(!run(d.path(), &["generate"]).status.success(), "{extra}");
        let output = run(d.path(), &["check", "--json", "--generated-only"]);
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["diagnostics"][0]["rule_id"], rule, "{extra}\n{v}");
        assert!(!d.path().join("out.css").exists(), "{extra}");
    }
}
#[test]
fn modes_without_css_output_need_no_selector() {
    let d = modes_fixture("");
    let p = d.path().join(".despec.toml");
    let text = fs::read_to_string(&p)
        .unwrap()
        .replace("css = \"out.css\"\n", "")
        .replace("css = \".dark\"\n", "");
    fs::write(&p, text).unwrap();
    let result = run(d.path(), &["generate"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn mode_names_follow_target_overrides_and_scss_default() {
    let d = modes_fixture(
        r##""fg" = { type = "color", value = "#111111", scssDefault = true, targets = { css = "text", scss = "ink", dart = "inkColor" }, modes = { dark = "#eeeeee" } }
[modes.contrast]
css = '[data-contrast="more"]'"##,
    );
    let p = d.path().join(".despec.toml");
    let text = fs::read_to_string(&p).unwrap().replace(
        "modes = { dark = \"#eeeeee\" }",
        "modes = { dark = \"#eeeeee\", contrast = \"#000000\" }",
    );
    fs::write(&p, text).unwrap();
    let result = run(d.path(), &["generate"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |ext: &str| fs::read_to_string(d.path().join(format!("out.{ext}"))).unwrap();
    let css = read("css");
    assert!(
        css.contains("\n.dark {\n  --ds-bg-canvas: #0b0d10;\n  --ds-text: #eeeeee;\n"),
        "{css}"
    );
    assert!(
        css.contains("\n[data-contrast=\"more\"] {\n  --ds-text: #000000;\n}\n"),
        "{css}"
    );
    let scss = read("scss");
    assert!(scss.contains("$ink: #111111 !default;\n$ink-contrast: #000000 !default;\n$ink-dark: #eeeeee !default;\n"), "{scss}");
    assert!(read("dart").contains("const Color inkColorDark = Color(0xFFEEEEEE);"));
    let text = fs::read_to_string(&p)
        .unwrap()
        .replace("\"#eeeeee\"", "\"#dddddd\"");
    fs::write(&p, text).unwrap();
    let output = run(d.path(), &["check", "--json", "--generated-only"]);
    assert!(!output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["rule_id"], "generated.stale");
}
#[test]
fn rgb_channels_render_beside_css_colors_and_modes() {
    let d = modes_fixture("");
    let p = d.path().join(".despec.toml");
    let text = fs::read_to_string(&p).unwrap().replace(
        r##""bg.canvas" = { type = "color", value = "#ffffff","##,
        r##""bg.canvas" = { type = "color", value = "#ffffff", rgbChannels = true,"##,
    );
    fs::write(&p, text).unwrap();
    let result = run(d.path(), &["generate"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |ext: &str| fs::read_to_string(d.path().join(format!("out.{ext}"))).unwrap();
    let css = read("css");
    for block in [
        ":root {\n  --ds-bg-canvas: #ffffff;\n  --ds-bg-canvas-rgb: 255 255 255;\n",
        ".dark {\n  --ds-bg-canvas: #0b0d10;\n  --ds-bg-canvas-rgb: 11 13 16;\n",
        "  :root {\n    --ds-bg-canvas: #000000;\n    --ds-bg-canvas-rgb: 0 0 0;\n  }\n",
    ] {
        assert!(css.contains(block), "{block}\n{css}");
    }
    assert!(!read("scss").contains("rgb:"));
    assert!(!read("dart").contains("Rgb"));
}
#[test]
fn rgb_channel_errors() {
    for extra in [
        r##""fg" = { type = "color", value = "rgba(0, 0, 0, 0.5)", rgbChannels = true }"##,
        r##""fg" = { type = "color", value = "#111111", rgbChannels = true, modes = { dark = "rgba(0, 0, 0, 0.5)" } }"##,
        r##""gap" = { type = "dimension", value = { value = 1, unit = "px" }, rgbChannels = true }"##,
        r##""fg" = { type = "color", value = "#aéabc", rgbChannels = true }"##,
        r##""fg" = { type = "color", value = "#111111", rgbChannels = "yes" }"##,
        r##""fg" = { type = "color", value = "#111111", targets = { css = "ink" }, rgbChannels = true }
"ink.rgb" = { type = "color", value = "#222222" }"##,
    ] {
        let d = modes_fixture(extra);
        assert!(!run(d.path(), &["generate"]).status.success(), "{extra}");
        let output = run(d.path(), &["check", "--json", "--generated-only"]);
        let v: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            v["diagnostics"][0]["rule_id"], "token.invalid",
            "{extra}\n{v}"
        );
    }
}
