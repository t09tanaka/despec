use crate::config::Mode;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn matches(pattern: &str, s: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(s)
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn num(v: &Value) -> String {
    let n = v.as_f64().unwrap();
    if n == 0.0 { "0".into() } else { n.to_string() }
}
fn dim(v: &Value) -> String {
    format!("{}{}", num(&v["value"]), s(&v["unit"]))
}
fn dimension(v: &Value) -> bool {
    v.is_object()
        && v["value"].as_f64().is_some_and(f64::is_finite)
        && matches(r"^[a-zA-Z%]+$", s(&v["unit"]))
}
fn number(v: &Value) -> bool {
    v.as_f64().is_some_and(f64::is_finite)
}
fn color(v: &Value) -> Result<(String, String), String> {
    let value = s(v);
    if matches(r"^#[0-9a-fA-F]{6}$", value) {
        return Ok((
            value.to_lowercase(),
            format!("Color(0xFF{})", value[1..].to_uppercase()),
        ));
    }
    let re = Regex::new(
        r"^rgba\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(0(?:\.\d+)?|1(?:\.0+)?)\s*\)$",
    )
    .unwrap();
    let c = re
        .captures(value)
        .ok_or("color must be #RRGGBB or rgba(r, g, b, a)")?;
    let channels: Vec<u16> = (1..4).map(|i| c[i].parse().unwrap()).collect();
    if channels.iter().any(|v| *v > 255) {
        return Err("rgba channels must be between 0 and 255".into());
    }
    let a = (c[4].parse::<f64>().unwrap() * 255.0).round() as u8;
    Ok((
        format!(
            "rgba({}, {}, {}, {})",
            channels[0], channels[1], channels[2], &c[4]
        ),
        format!(
            "Color(0x{:02X}{:02X}{:02X}{:02X})",
            a, channels[0], channels[1], channels[2]
        ),
    ))
}
/// `R G B` for a #RRGGBB color, for `rgb(var(--ds-x-rgb) / <alpha>)` in CSS.
fn rgb_channels(v: &Value) -> Option<String> {
    let hex = s(v).strip_prefix('#').filter(|h| h.len() == 6)?;
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(format!("{} {} {}", c(0)?, c(2)?, c(4)?))
}
/// Extra CSS custom properties a record emits besides its value.
fn css_extras(p: &str, t: &Value) -> Vec<(String, String)> {
    if t["rgbChannels"] == true {
        vec![(
            format!("{}-rgb", name(p, t, "css")),
            rgb_channels(&t["value"]).unwrap(),
        )]
    } else {
        vec![]
    }
}
fn camel(p: &str) -> String {
    let mut parts = p.split(['.', '_', '-']);
    let mut out = parts.next().unwrap_or("").to_owned();
    for part in parts {
        let mut cs = part.chars();
        if let Some(c) = cs.next() {
            out.extend(c.to_uppercase());
            out.extend(cs)
        }
    }
    out
}
fn name(p: &str, t: &Value, target: &str) -> String {
    t["targets"][target]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if target == "dart" {
                camel(p)
            } else {
                p.replace(['.', '_'], "-")
            }
        })
}
fn pascal(p: &str) -> String {
    let c = camel(p);
    let mut cs = c.chars();
    cs.next()
        .map(|f| f.to_uppercase().chain(cs).collect())
        .unwrap_or_default()
}
fn mode_values(t: &Value) -> impl Iterator<Item = (&String, &Value)> {
    t.get("modes")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}
/// The record a mode renders. CSS reuses the default name and overrides it through the
/// cascade; Sass and Dart have no cascade, so their names gain the mode as a suffix.
fn mode_token(p: &str, t: &Value, mode: &str, v: &Value) -> Value {
    let mut m = t.clone();
    m["value"] = v.clone();
    m["targets"] = json!({
        "css": name(p, t, "css"),
        "scss": format!("{}-{mode}", name(p, t, "scss")),
        "dart": format!("{}{}", name(p, t, "dart"), pascal(mode)),
    });
    m
}
const RESERVED: &str = "abstract as assert async await base bool break case catch class const continue covariant default deferred do double dynamic else enum export extends extension external factory false final finally for get hide if implements import in int interface is late library mixin new never null num of on operator part required rethrow return sealed set show static super switch sync this throw true try type typedef var void when with yield";
fn properties(v: &Value) -> Vec<&'static str> {
    ["fontSize", "fontWeight", "lineHeight", "letterSpacing"]
        .into_iter()
        .filter(|p| v.get(*p).is_some())
        .collect()
}
fn values(p: &str, t: &Value, target: &str) -> Vec<(String, String)> {
    let n = name(p, t, target);
    let v = &t["value"];
    let val = match s(&t["type"]) {
        "color" => color(v).unwrap().0,
        "dimension" => dim(v),
        "number" => num(v),
        "keyword" | "fontFamily" => s(v).into(),
        "shadow" => {
            if v == "none" {
                "none".into()
            } else {
                let offset = |k: &str| {
                    if v[k]["value"].as_f64() == Some(0.0) {
                        "0".into()
                    } else {
                        dim(&v[k])
                    }
                };
                format!(
                    "{} {} {} {}",
                    offset("offsetX"),
                    offset("offsetY"),
                    dim(&v["blur"]),
                    color(&v["color"]).unwrap().0
                )
            }
        }
        "typography" => {
            return properties(v)
                .iter()
                .map(|p| {
                    let suffix = match (*p, target) {
                        ("fontSize", "css") => "size",
                        ("fontWeight", "css") => "weight",
                        ("lineHeight", "css") => "lh",
                        ("letterSpacing", "css") => "ls",
                        ("fontSize", _) => "font-size",
                        ("fontWeight", _) => "font-weight",
                        ("lineHeight", _) => "line-height",
                        _ => "letter-spacing",
                    };
                    (
                        format!("{n}-{suffix}"),
                        if v[p].is_object() {
                            dim(&v[p])
                        } else {
                            num(&v[p])
                        },
                    )
                })
                .collect();
        }
        _ => unreachable!(),
    };
    vec![(n, val)]
}
fn valid_value(ty: &str, v: &Value) -> Result<bool, String> {
    Ok(match ty {
        "color" => {
            color(v)?;
            true
        }
        "dimension" => dimension(v),
        "number" => number(v),
        "keyword" => matches(r"^[a-zA-Z][a-zA-Z0-9-]*$", s(v)),
        "fontFamily" => !s(v).trim().is_empty() && !s(v).contains(['\n', '\r', ';', '{', '}']),
        "shadow" => {
            v == "none"
                || (v.is_object()
                    && ["offsetX", "offsetY", "blur"]
                        .iter()
                        .all(|k| dimension(&v[k]) && v[k]["unit"] == "px")
                    && color(&v["color"]).is_ok())
        }
        "typography" => {
            v.is_object()
                && dimension(&v["fontSize"])
                && number(&v["lineHeight"])
                && v.get("fontWeight").is_none_or(number)
                && v.get("letterSpacing")
                    .is_none_or(|v| number(v) || dimension(v))
        }
        _ => false,
    })
}
pub fn validate(
    tokens: &BTreeMap<String, Value>,
    modes: &BTreeMap<String, Mode>,
) -> Result<(), String> {
    for (p, t) in tokens {
        let valid = (|| -> Result<(), String> {
            if !matches(r"^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$", p) {
                return Err("unsafe token path".into());
            }
            if !t.is_object() || t.get("value").is_none() {
                return Err("record must be an object with value".into());
            }
            if t.get("scssDefault").is_some_and(|v| !v.is_boolean()) {
                return Err("scssDefault must be boolean".into());
            }
            if let Some(r) = t.get("rgbChannels") {
                if !r.is_boolean() {
                    return Err("rgbChannels must be boolean".into());
                }
                if r == true
                    && (s(&t["type"]) != "color"
                        || std::iter::once(&t["value"])
                            .chain(mode_values(t).map(|(_, v)| v))
                            .any(|v| rgb_channels(v).is_none()))
                {
                    return Err(
                        "rgbChannels requires a color token whose values are all #RRGGBB".into(),
                    );
                }
            }
            if let Some(targets) = t.get("targets") {
                let targets = targets.as_object().ok_or("targets must be an object")?;
                for (target, n) in targets {
                    if !["css", "scss", "dart"].contains(&target.as_str())
                        || !matches(
                            if target == "dart" {
                                r"^[a-z][a-zA-Z0-9]*$"
                            } else {
                                r"^[a-z][a-z0-9-]*$"
                            },
                            s(n),
                        )
                    {
                        return Err("unsafe output target name".into());
                    }
                }
            }
            if RESERVED.split_whitespace().any(|r| r == name(p, t, "dart")) {
                return Err("reserved Dart identifier".into());
            }
            let ty = s(&t["type"]);
            if !valid_value(ty, &t["value"])? {
                return Err(format!("invalid {ty} value"));
            }
            if let Some(ms) = t.get("modes") {
                let ms = ms.as_object().ok_or("modes must be an object")?;
                if ms.is_empty() {
                    return Err("modes must not be empty".into());
                }
                if !["color", "shadow"].contains(&ty) {
                    return Err("only color and shadow tokens can vary by mode".into());
                }
                for (m, v) in ms {
                    if !modes.contains_key(m) {
                        return Err(format!("mode {m} is not declared"));
                    }
                    if !valid_value(ty, v)? {
                        return Err(format!("invalid {ty} value for mode {m}"));
                    }
                }
            }
            Ok(())
        })();
        valid.map_err(|e| format!("Invalid token {p}: {e}"))?;
    }
    for m in modes.keys() {
        if !tokens.values().any(|t| t["modes"].get(m).is_some()) {
            return Err(format!(
                "mode {m} is declared but no token sets a value for it"
            ));
        }
    }
    let mut seen = BTreeMap::new();
    for (p, t) in tokens {
        for target in ["css", "scss", "dart"] {
            let mut records = vec![t.clone()];
            if target != "css" {
                records.extend(mode_values(t).map(|(m, v)| mode_token(p, t, m, v)));
            }
            let names: Vec<String> = records
                .iter()
                .flat_map(|t| {
                    if target == "dart" {
                        vec![name(p, t, target)]
                    } else {
                        let mut names: Vec<String> =
                            values(p, t, target).into_iter().map(|(n, _)| n).collect();
                        if target == "css" {
                            names.extend(css_extras(p, t).into_iter().map(|(n, _)| n));
                        }
                        names
                    }
                })
                .collect();
            for n in names {
                if let Some(old) = seen.insert(format!("{target}:{n}"), p) {
                    return Err(format!(
                        "Invalid token {p}: {target} output name collision with {old}: {n}"
                    ));
                }
            }
        }
    }
    Ok(())
}
fn dart(p: &str, t: &Value) -> String {
    let n = name(p, t, "dart");
    let v = &t["value"];
    match s(&t["type"]) {
        "color" => format!("const Color {n} = {};", color(v).unwrap().1),
        "dimension" => {
            if v["unit"] == "px" {
                format!("const double {n} = {};", num(&v["value"]))
            } else {
                format!("const String {n} = '{}';", dim(v))
            }
        }
        "number" => format!(
            "const {} {n} = {};",
            if v.as_f64().unwrap().fract() == 0.0 {
                "int"
            } else {
                "double"
            },
            num(v)
        ),
        "keyword" | "fontFamily" => {
            let val = serde_json::to_string(v).unwrap().replace('$', "\\$");
            let declaration = format!("const String {n} = {val};");
            if declaration.encode_utf16().count() > 80 {
                format!("const String {n} =\n    {val};")
            } else {
                declaration
            }
        }
        "shadow" => {
            if v == "none" {
                format!("const String {n} = 'none';")
            } else {
                format!(
                    "const List<BoxShadow> {n} = <BoxShadow>[\n  BoxShadow(color: {}, blurRadius: {}, offset: Offset({}, {})),\n];",
                    color(&v["color"]).unwrap().1,
                    num(&v["blur"]["value"]),
                    num(&v["offsetX"]["value"]),
                    num(&v["offsetY"]["value"])
                )
            }
        }
        "typography" => {
            let letter_spacing = v.get("letterSpacing").and_then(|x| {
                let value = if !x.is_object() {
                    x.as_f64()
                } else if x["unit"] == "px" {
                    x["value"].as_f64()
                } else if x["unit"] == "em" {
                    // em is relative to this typography token's font size.
                    Some(x["value"].as_f64().unwrap() * v["fontSize"]["value"].as_f64().unwrap())
                } else {
                    None
                };
                value.filter(|n| n.is_finite())
            });
            let supported = v["fontSize"]["unit"] == "px"
                && (v.get("letterSpacing").is_none() || letter_spacing.is_some())
                && v.get("fontWeight").is_none_or(|x| {
                    let w = x.as_f64().unwrap();
                    (100.0..=900.0).contains(&w) && w % 100.0 == 0.0
                });
            let fields: Vec<String> = properties(v)
                .into_iter()
                .map(|p| {
                    let x = &v[p];
                    if supported {
                        let key = if p == "lineHeight" { "height" } else { p };
                        let value = if p == "fontWeight" {
                            format!("FontWeight.w{}", num(x))
                        } else if p == "letterSpacing" {
                            num(&Value::from(letter_spacing.unwrap()))
                        } else if x.is_object() {
                            num(&x["value"])
                        } else {
                            num(x)
                        };
                        format!("  {key}: {value},")
                    } else {
                        format!(
                            "  '{p}': {},",
                            if x.is_object() {
                                format!("'{}'", dim(x))
                            } else {
                                num(x)
                            }
                        )
                    }
                })
                .collect();
            if supported {
                format!(
                    "const TextStyle {n} = TextStyle(\n{}\n);",
                    fields.join("\n")
                )
            } else {
                format!(
                    "const Map<String, Object> {n} = <String, Object>{{\n{}\n}};",
                    fields.join("\n")
                )
            }
        }
        _ => unreachable!(),
    }
}
pub fn render(
    tokens: &BTreeMap<String, Value>,
    modes: &BTreeMap<String, Mode>,
    target: &str,
    warning: &str,
) -> Result<String, String> {
    validate(tokens, modes)?;
    // ASCII token alphabet; punctuation ordered as in the legacy locale collation.
    let mut entries: Vec<_> = tokens.iter().collect();
    entries.sort_by_key(|(p, _)| {
        p.replace('_', "\u{1}")
            .replace('-', "\u{2}")
            .replace('.', "\u{3}")
    });
    let declare = |p: &str, t: &Value| -> Vec<String> {
        if target == "dart" {
            vec![dart(p, t)]
        } else {
            let mut vs = values(p, t, target);
            if target == "css" {
                vs.extend(css_extras(p, t));
            }
            vs.into_iter()
                .map(|(n, v)| {
                    if target == "css" {
                        format!("  --ds-{n}: {v};")
                    } else {
                        format!(
                            "${n}: {v}{};",
                            if t["scssDefault"] == true {
                                " !default"
                            } else {
                                ""
                            }
                        )
                    }
                })
                .collect()
        }
    };
    let declarations: Vec<String> = entries
        .iter()
        .flat_map(|(p, t)| {
            let mut out = declare(p, t);
            if target != "css" {
                for (m, v) in mode_values(t) {
                    out.extend(declare(p, &mode_token(p, t, m, v)));
                }
            }
            out
        })
        .collect();
    let body = declarations.join("\n");
    Ok(match target {
        "css" => {
            let mut out = format!("/* {warning} */\n\n:root {{\n{body}\n}}\n");
            for (m, mode) in modes {
                let css = mode
                    .css
                    .as_deref()
                    .ok_or_else(|| format!("mode {m} requires css for the css output"))?;
                let lines: Vec<String> = entries
                    .iter()
                    .filter_map(|(p, t)| {
                        t["modes"]
                            .get(m)
                            .map(|v| declare(p, &mode_token(p, t, m, v)))
                    })
                    .flatten()
                    .collect();
                if css.starts_with('@') {
                    let lines: Vec<String> = lines.iter().map(|l| format!("  {l}")).collect();
                    out += &format!("\n{css} {{\n  :root {{\n{}\n  }}\n}}\n", lines.join("\n"));
                } else {
                    out += &format!("\n{css} {{\n{}\n}}\n", lines.join("\n"));
                }
            }
            out
        }
        "scss" => format!("// {warning}\n\n{body}\n"),
        "dart" => format!(
            "// {warning}\n// ignore_for_file: prefer_single_quotes, lines_longer_than_80_chars\n\nimport 'package:flutter/material.dart';\n\n{body}\n"
        ),
        _ => return Err("unknown target".into()),
    })
}
