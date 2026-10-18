//! Lexical detection of hard-coded dimensions: CSS declarations, Tailwind arbitrary
//! values, Flutter constructor arguments, and forbidden class names. Like the color
//! check, this reads comment-stripped text and does not parse any language.
use crate::config::RuleConfig;
use globset::GlobSet;
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::LazyLock,
};

/// Dimension rule IDs accepted under `[rules.<id>]`; `color` is accepted separately.
pub const DIMENSION_RULES: [&str; 7] = [
    "spacing",
    "gap",
    "border-width",
    "outline",
    "shadow",
    "radius",
    "font-size",
];

/// One hard-coded value; `offset` is a byte offset into the scanned text.
pub struct Finding {
    pub rule: &'static str,
    pub offset: usize,
    pub literal: String,
}

/// CSS properties per rule. Shorthands and longhands are both listed; border-radius,
/// border-color, border-style, and border-spacing are not border widths.
const CSS_PROPERTIES: [(&str, &str); 7] = [
    (
        "spacing",
        r"(?:margin|padding)(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?",
    ),
    ("gap", r"(?:grid-)?(?:row-|column-)?gap"),
    (
        "border-width",
        r"border(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?(?:-width)?",
    ),
    ("outline", r"outline(?:-width|-offset)?"),
    ("shadow", r"(?:box|text)-shadow"),
    (
        "radius",
        r"border-(?:(?:top|bottom)-(?:left|right)-|(?:start|end)-(?:start|end)-)?radius",
    ),
    ("font-size", r"font-size"),
];

/// Tailwind utility prefixes that take an arbitrary `-[...]` value, per rule.
const TAILWIND_PREFIXES: [(&str, &str); 7] = [
    ("spacing", r"space-[xy]|m[xytrblse]?|p[xytrblse]?"),
    ("gap", r"gap(?:-[xy])?"),
    ("border-width", r"border(?:-[xytrblse])?"),
    ("outline", r"outline-offset|outline|ring-offset|ring"),
    ("shadow", r"drop-shadow|shadow"),
    (
        "radius",
        r"rounded(?:-(?:tl|tr|br|bl|ss|se|es|ee|ts|te|bs|be|[trblse]))?",
    ),
    ("font-size", r"text"),
];

/// Declarations in markup and scripts: the value ends at `;`, `}`, a line break, or a
/// quote, because it usually sits inside an attribute or a string.
const MARKUP_VALUE: &str = r#"[^;}\n"'`]*"#;
static CSS_DECLARATIONS: LazyLock<Vec<(&'static str, Regex)>> =
    LazyLock::new(|| declarations(MARKUP_VALUE));
/// Declarations in stylesheets: the value may wrap over lines, as formatters write
/// long shadows and shorthands, and ends at `;`, `}`, or a `{` opening a nested rule.
const STYLESHEET_VALUE: &str = r"[^;{}]*";
static STYLESHEET_DECLARATIONS: LazyLock<Vec<(&'static str, Regex)>> =
    LazyLock::new(|| declarations(STYLESHEET_VALUE));

fn declarations(value: &str) -> Vec<(&'static str, Regex)> {
    CSS_PROPERTIES
        .iter()
        .map(|(rule, props)| (*rule, declaration(props, value)))
        .collect()
}

/// A declaration of one of `props` whose value, matching `value`, is group 1.
fn declaration(props: &str, value: &str) -> Regex {
    Regex::new(&format!(r#"(?im)(?:^|[\s;{{"'`])(?:{props})\s*:({value})"#)).unwrap()
}
static TAILWIND: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    TAILWIND_PREFIXES
        .iter()
        .map(|(rule, prefixes)| {
            let pattern = format!(r#"(?m)(?:^|[\s"'`{{:!])-?((?:{prefixes})-\[([^\]\s"'`]+)\])"#);
            (*rule, Regex::new(&pattern).unwrap())
        })
        .collect()
});
static UNIT_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)-?(?:\d+(?:\.\d+)?|\.\d+)(?:cqmin|cqmax|vmin|vmax|svh|lvh|dvh|svw|lvw|dvw|cqw|cqh|cqi|cqb|rem|rlh|cap|px|em|ex|ch|ic|lh|vh|vw|vi|vb|pt|pc|cm|mm|in|q|%)",
    )
    .unwrap()
});
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-?(?:\d+(?:\.\d+)?|\.\d+)").unwrap());
static COLOR_FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color|color-mix)\(").unwrap()
});
static TYPE_HINT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([a-z-]+):").unwrap());
/// Flutter calls whose arguments are all dimensions of the rule.
static DART_CALLS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        (
            "spacing",
            r"\bEdgeInsets(?:Directional)?\s*\.\s*(?:all|only|symmetric|fromLTRB|fromSTEB)\s*\(",
        ),
        (
            "radius",
            r"\b(?:BorderRadius\s*\.\s*circular|Radius\s*\.\s*(?:circular|elliptical))\s*\(",
        ),
    ]
    .into_iter()
    .map(|(rule, p)| (rule, Regex::new(p).unwrap()))
    .collect()
});
/// Flutter calls where only the named arguments are dimensions of the rule.
static DART_NAMED: LazyLock<Vec<(&'static str, Regex, Regex)>> = LazyLock::new(|| {
    [
        ("gap", r"\bSizedBox\s*\(", r"\b(?:width|height)\s*:"),
        (
            "border-width",
            r"\b(?:BorderSide\s*\(|Border\s*\.\s*all\s*\()",
            r"\bwidth\s*:",
        ),
        (
            "shadow",
            r"\bBoxShadow\s*\(",
            r"\b(?:blurRadius|spreadRadius)\s*:",
        ),
    ]
    .into_iter()
    .map(|(rule, call, arg)| (rule, Regex::new(call).unwrap(), Regex::new(arg).unwrap()))
    .collect()
});
/// Flutter named arguments that are dimensions wherever they appear.
static DART_ANYWHERE: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        (
            "gap",
            r"\b(?:spacing|runSpacing|mainAxisSpacing|crossAxisSpacing)\s*:",
        ),
        ("font-size", r"\bfontSize\s*:"),
    ]
    .into_iter()
    .map(|(rule, p)| (rule, Regex::new(p).unwrap()))
    .collect()
});
static DART_BOX_SHADOW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bBoxShadow\s*\(").unwrap());
static DART_OFFSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bOffset\s*\(").unwrap());

/// The `allow`, `classes`, and `keywords` in effect for a rule, from its table or
/// from one of its overrides.
struct Values {
    allow: BTreeSet<String>,
    classes: Option<GlobSet>,
    keywords: bool,
}

impl Values {
    fn new(allow: Option<&[String]>, classes: &[String], keywords: bool) -> Result<Self, String> {
        Ok(Self {
            allow: allow
                .into_iter()
                .flatten()
                .map(|v| v.to_lowercase())
                .collect(),
            classes: if classes.is_empty() {
                None
            } else {
                Some(crate::scan::globs(classes)?)
            },
            keywords,
        })
    }
}

struct Active {
    rule: &'static str,
    values: Values,
    overrides: Vec<(GlobSet, Values)>,
}

/// Compiled `[rules.*]` settings, built once per scan.
pub struct Rules {
    active: Vec<Active>,
}

/// The rules as they apply to one file.
pub struct Selection<'a> {
    active: Vec<(&'static str, &'a Values)>,
    /// (rule, index) of each override chosen for the file.
    pub overrides: Vec<(&'static str, usize)>,
}

impl Rules {
    /// Expects a configuration that passed `Config::validate`.
    pub fn new(config: &BTreeMap<String, RuleConfig>) -> Result<Self, String> {
        let mut active = vec![];
        for (id, rule) in config {
            let rule_id = std::iter::once("color")
                .chain(DIMENSION_RULES)
                .find(|r| r == id)
                .ok_or_else(|| format!("unknown rule {id}"))?;
            let mut overrides = vec![];
            for o in &rule.overrides {
                // A key the override leaves out keeps the table's value.
                let values = Values::new(
                    o.allow.as_deref().or(rule.allow.as_deref()),
                    o.classes.as_deref().unwrap_or(&rule.classes),
                    o.keywords.unwrap_or(rule.keywords),
                )?;
                overrides.push((crate::scan::globs(&o.paths)?, values));
            }
            active.push(Active {
                rule: rule_id,
                values: Values::new(rule.allow.as_deref(), &rule.classes, rule.keywords)?,
                overrides,
            });
        }
        Ok(Self { active })
    }

    /// Resolves each rule for `path`, relative to the configuration directory: the
    /// first override whose `paths` match it, otherwise the rule's table.
    pub fn select(&self, path: &std::path::Path) -> Selection<'_> {
        let mut selection = Selection {
            active: vec![],
            overrides: vec![],
        };
        for a in &self.active {
            let values = match a
                .overrides
                .iter()
                .position(|(paths, _)| paths.is_match(path))
            {
                Some(i) => {
                    selection.overrides.push((a.rule, i));
                    &a.overrides[i].1
                }
                None => &a.values,
            };
            selection.active.push((a.rule, values));
        }
        selection
    }
}

impl Selection<'_> {
    /// Findings in `text`, in no particular order; the caller sorts by offset.
    pub fn find(&self, text: &str, extension: Option<&str>) -> Vec<Finding> {
        let dart = extension == Some("dart");
        let stylesheet = matches!(extension, Some("css" | "scss" | "sass" | "less"));
        let mut out = vec![];
        let mut seen = BTreeSet::new();
        let words = if dart || self.active.iter().all(|(_, v)| v.classes.is_none()) {
            vec![]
        } else if stylesheet {
            apply_classes(text, extension == Some("sass")).0
        } else {
            // Words inside an `@apply` come from it alone, so its `;`, `!important`,
            // and the `@apply` keyword never reach the plain split.
            let (mut words, spans) = apply_classes(text, false);
            words.extend(
                class_names(text)
                    .into_iter()
                    .filter(|(o, _)| !spans.iter().any(|s| s.contains(o))),
            );
            words
        };
        let mut push = |rule: &'static str, offset: usize, literal: &str| {
            if seen.insert((rule, offset)) {
                out.push(Finding {
                    rule,
                    offset,
                    literal: literal.to_owned(),
                });
            }
        };
        // Indented Sass ends declarations at line breaks.
        let wraps = stylesheet && extension != Some("sass");
        for &(rule, a) in &self.active {
            let allowed = |literal: &str| a.allow.contains(&literal.to_lowercase());
            if a.keywords && !dart {
                for (offset, literal) in keyword_colors(text, stylesheet, wraps) {
                    push(rule, offset, &literal);
                }
            }
            if rule != "color" {
                if dart {
                    for (offset, literal) in dart_values(text, rule) {
                        if !is_zero(&literal) && !allowed(&literal) {
                            push(rule, offset, &literal);
                        }
                    }
                } else {
                    for (offset, literal) in css_values(text, rule, wraps) {
                        if !allowed(&literal) {
                            push(rule, offset, &literal);
                        }
                    }
                    for (offset, literal, values) in tailwind_values(text, rule) {
                        if !allowed(&literal) && !values.iter().all(|v| allowed(v)) {
                            push(rule, offset, &literal);
                        }
                    }
                }
            }
            if let Some(classes) = &a.classes {
                for &(offset, class) in &words {
                    if classes.is_match(class) && !allowed(class) {
                        push(rule, offset, class);
                    }
                }
            }
        }
        out
    }
}

/// Nonzero numbers with a unit in `value`, as (offset within value, literal). A number
/// directly after an identifier character, `.`, `$`, or `#` belongs to a name such as
/// `--space-12px` or `$gap2`; a `-` after such a character is part of the name too,
/// otherwise it is the sign of a negative value.
fn unit_numbers(value: &str) -> Vec<(usize, String)> {
    UNIT_NUMBER
        .find_iter(value)
        .filter(|m| standalone(value.as_bytes(), m, b"_-.") && !is_zero(m.as_str()))
        .map(|m| (m.start(), m.as_str().to_owned()))
        .collect()
}

/// Whether a number match stands alone rather than inside a name: nothing before it
/// is an identifier character, `-`, `.`, `$`, or `#`, and nothing after it is
/// alphanumeric or one of `after`.
fn standalone(b: &[u8], m: &regex::Match, after: &[u8]) -> bool {
    let before = m.start().checked_sub(1).map(|i| b[i]);
    let next = b.get(m.end()).copied();
    !before.is_some_and(|c| c.is_ascii_alphanumeric() || b"_-.$#".contains(&c))
        && !next.is_some_and(|c| c.is_ascii_alphanumeric() || after.contains(&c))
}

/// Whether the numeric part of a literal is zero, so `0`, `0px`, and `-0.0rem` pass.
fn is_zero(literal: &str) -> bool {
    let end = literal
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(literal.len());
    literal[..end].parse::<f64>().is_ok_and(|n| n == 0.0)
}

fn css_values(text: &str, rule: &str, stylesheet: bool) -> Vec<(usize, String)> {
    let declarations = if stylesheet {
        &STYLESHEET_DECLARATIONS
    } else {
        &CSS_DECLARATIONS
    };
    let re = &declarations.iter().find(|(r, _)| *r == rule).unwrap().1;
    let mut out = vec![];
    for caps in re.captures_iter(text) {
        let value = caps.get(1).unwrap();
        for (offset, literal) in unit_numbers(value.as_str()) {
            out.push((value.start() + offset, literal));
        }
    }
    out
}

/// Arbitrary values such as `md:p-[13px]`, as (offset, class without variants or
/// sign, nonzero unit values inside). Classes without such a value are skipped.
fn tailwind_values(text: &str, rule: &str) -> Vec<(usize, String, Vec<String>)> {
    let re = &TAILWIND.iter().find(|(r, _)| *r == rule).unwrap().1;
    let mut out = vec![];
    for caps in re.captures_iter(text) {
        let class = caps.get(1).unwrap();
        let inner = caps.get(2).unwrap().as_str();
        if rule == "font-size"
            && (inner.starts_with('#')
                || COLOR_FUNCTION.is_match(inner)
                || TYPE_HINT.captures(inner).is_some_and(|c| &c[1] != "length"))
        {
            continue;
        }
        // Tailwind writes spaces inside arbitrary values as underscores.
        let values: Vec<String> = unit_numbers(&inner.replace('_', " "))
            .into_iter()
            .map(|(_, v)| v)
            .collect();
        if !values.is_empty() {
            out.push((class.start(), class.as_str().to_owned(), values));
        }
    }
    out
}

/// The 148 named colors of CSS Color Module Level 4, lowercase. `transparent` and
/// `currentColor` are not among them.
#[rustfmt::skip]
const NAMED_COLORS: [&str; 148] = [
    "aliceblue", "antiquewhite", "aqua", "aquamarine", "azure", "beige", "bisque", "black",
    "blanchedalmond", "blue", "blueviolet", "brown", "burlywood", "cadetblue", "chartreuse",
    "chocolate", "coral", "cornflowerblue", "cornsilk", "crimson", "cyan", "darkblue",
    "darkcyan", "darkgoldenrod", "darkgray", "darkgreen", "darkgrey", "darkkhaki",
    "darkmagenta", "darkolivegreen", "darkorange", "darkorchid", "darkred", "darksalmon",
    "darkseagreen", "darkslateblue", "darkslategray", "darkslategrey", "darkturquoise",
    "darkviolet", "deeppink", "deepskyblue", "dimgray", "dimgrey", "dodgerblue",
    "firebrick", "floralwhite", "forestgreen", "fuchsia", "gainsboro", "ghostwhite", "gold",
    "goldenrod", "gray", "green", "greenyellow", "grey", "honeydew", "hotpink", "indianred",
    "indigo", "ivory", "khaki", "lavender", "lavenderblush", "lawngreen", "lemonchiffon",
    "lightblue", "lightcoral", "lightcyan", "lightgoldenrodyellow", "lightgray",
    "lightgreen", "lightgrey", "lightpink", "lightsalmon", "lightseagreen", "lightskyblue",
    "lightslategray", "lightslategrey", "lightsteelblue", "lightyellow", "lime",
    "limegreen", "linen", "magenta", "maroon", "mediumaquamarine", "mediumblue",
    "mediumorchid", "mediumpurple", "mediumseagreen", "mediumslateblue",
    "mediumspringgreen", "mediumturquoise", "mediumvioletred", "midnightblue", "mintcream",
    "mistyrose", "moccasin", "navajowhite", "navy", "oldlace", "olive", "olivedrab",
    "orange", "orangered", "orchid", "palegoldenrod", "palegreen", "paleturquoise",
    "palevioletred", "papayawhip", "peachpuff", "peru", "pink", "plum", "powderblue",
    "purple", "rebeccapurple", "red", "rosybrown", "royalblue", "saddlebrown", "salmon",
    "sandybrown", "seagreen", "seashell", "sienna", "silver", "skyblue", "slateblue",
    "slategray", "slategrey", "snow", "springgreen", "steelblue", "tan", "teal", "thistle",
    "tomato", "turquoise", "violet", "wheat", "white", "whitesmoke", "yellow",
    "yellowgreen",
];

fn named_color(word: &str) -> bool {
    NAMED_COLORS.contains(&word.to_ascii_lowercase().as_str())
}

/// CSS properties whose value may hold a color: every `*-color` longhand, vendor
/// prefixed or not, and the shorthands and properties that take a color among others.
const COLOR_PROPERTIES: &str = concat!(
    r"(?:-[a-z]+-)?(?:[a-z]+-)*color|background|border(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?)?",
    r"|outline|(?:box|text)-shadow|fill|stroke|column-rule|text-decoration|text-emphasis",
);
static COLOR_DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| declaration(COLOR_PROPERTIES, MARKUP_VALUE));
static STYLESHEET_COLOR_DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| declaration(COLOR_PROPERTIES, STYLESHEET_VALUE));
/// Presentation attributes that take a color, such as SVG `fill="black"`. Group 2 or 3
/// is the quoted value.
static COLOR_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:^|\s)(?:fill|stroke|color|stop-color)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
        .unwrap()
});
/// Tailwind color utilities that take an arbitrary `-[...]` value.
static TAILWIND_COLOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?m)(?:^|[\s"'`{:!])((?:bg|text|border(?:-[xytrblse])?|outline|ring-offset|ring|"#,
        r#"divide|decoration|placeholder|caret|accent|fill|stroke|shadow|from|via|to)"#,
        r#"-\[([^\]\s"'`]+)\])"#,
    ))
    .unwrap()
});
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_-]+").unwrap());

/// Named colors in CSS declarations, color attributes, and Tailwind arbitrary values,
/// as (offset, literal). `stylesheet` selects the file kind and `wraps` the
/// declaration end, as for `css_values`.
fn keyword_colors(text: &str, stylesheet: bool, wraps: bool) -> Vec<(usize, String)> {
    let declaration = if wraps {
        &STYLESHEET_COLOR_DECLARATION
    } else {
        &COLOR_DECLARATION
    };
    let mut out = vec![];
    for caps in declaration.captures_iter(text) {
        let value = caps.get(1).unwrap();
        let b = value.as_str().as_bytes();
        for m in WORD.find_iter(value.as_str()) {
            // A word after `$`, `.`, `#`, `@`, `%`, or `/` is a variable, a member, a
            // selector, or a path, and one before `.` or `(` is a file name or a call.
            let before = m.start().checked_sub(1).map(|i| b[i]);
            let next = b.get(m.end()).copied();
            if !before.is_some_and(|c| b"$.#@%/\\".contains(&c))
                && !next.is_some_and(|c| b".(".contains(&c))
                && named_color(m.as_str())
            {
                out.push((value.start() + m.start(), m.as_str().to_owned()));
            }
        }
    }
    if !stylesheet {
        for caps in COLOR_ATTRIBUTE.captures_iter(text) {
            let value = caps.get(1).or_else(|| caps.get(2)).unwrap();
            let word = value.as_str().trim();
            if named_color(word) {
                let offset = value.start() + value.as_str().find(word).unwrap();
                out.push((offset, word.to_owned()));
            }
        }
    }
    for caps in TAILWIND_COLOR.captures_iter(text) {
        let inner = caps.get(2).unwrap().as_str();
        if named_color(inner.strip_prefix("color:").unwrap_or(inner)) {
            let class = caps.get(1).unwrap();
            out.push((class.start(), class.as_str().to_owned()));
        }
    }
    out
}

static APPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)(?:^|[\s;{])@apply\s").unwrap());

/// Class names in the arguments of each `@apply`, which end at `;`, `}`, or, when
/// `line_ends` is set, a line break outside brackets, with the byte range of each
/// `@apply` and its arguments. `!important` is not a class.
fn apply_classes(text: &str, line_ends: bool) -> (Vec<(usize, &str)>, Vec<Range<usize>>) {
    let b = text.as_bytes();
    let mut out = vec![];
    let mut spans = vec![];
    for m in APPLY.find_iter(text) {
        let from = m.end();
        let mut to = from;
        let mut depth = 0usize;
        while to < b.len() {
            match b[to] {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b';' | b'}' if depth == 0 => break,
                b'\n' if line_ends && depth == 0 => break,
                _ => (),
            }
            to += 1;
        }
        spans.push(m.start()..to + 1);
        for (offset, class) in class_names(&text[from..to]) {
            let offset = from + offset;
            if !(class == "important" && text[..offset].ends_with('!')) {
                out.push((offset, class));
            }
        }
    }
    (out, spans)
}

/// Class-like words split at whitespace, and at quotes, parentheses, braces, and
/// commas outside brackets, with variants, `!`, and a leading `-` removed. A word
/// ending in `:` is an object key or a CSS property, not a class.
fn class_names(text: &str) -> Vec<(usize, &str)> {
    let mut out = vec![];
    let mut start = 0;
    let mut depth = 0usize;
    let b = text.as_bytes();
    for i in 0..=b.len() {
        let c = b.get(i).copied();
        match c {
            Some(b'[') => depth += 1,
            Some(b']') => depth = depth.saturating_sub(1),
            _ => (),
        }
        let split = match c {
            None => true,
            Some(c) => c.is_ascii_whitespace() || (depth == 0 && b"\"'`(){},".contains(&c)),
        };
        if !split {
            continue;
        }
        let word = &text[start..i];
        let begin = start;
        start = i + 1;
        depth = 0;
        if word.ends_with(':') {
            continue;
        }
        let mut variant_end = 0;
        let mut d = 0usize;
        for (j, c) in word.bytes().enumerate() {
            match c {
                b'[' => d += 1,
                b']' => d = d.saturating_sub(1),
                b':' if d == 0 => variant_end = j + 1,
                _ => (),
            }
        }
        let mut offset = begin + variant_end;
        let mut class = &word[variant_end..];
        for prefix in ["!", "-"] {
            if let Some(rest) = class.strip_prefix(prefix) {
                class = rest;
                offset += prefix.len();
            }
        }
        let class = class.strip_suffix('!').unwrap_or(class);
        if !class.is_empty() {
            out.push((offset, class));
        }
    }
    out
}

/// Byte index of the bracket closing the one at `open`, skipping string literals.
fn closing(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            q @ (b'"' | b'\'') => i = skip_string(b, i, q),
            _ => (),
        }
        i += 1;
    }
    None
}

fn skip_string(b: &[u8], open: usize, quote: u8) -> usize {
    let mut i = open + 1;
    while i < b.len() && b[i] != quote {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i
}

/// End of the argument expression starting at `from`: the next top-level `,` or `;`,
/// or the bracket closing the enclosing call.
fn argument_end(b: &[u8], from: usize, limit: usize) -> usize {
    let mut i = from;
    while i < limit {
        match b[i] {
            b'(' | b'[' | b'{' => match closing(b, i) {
                Some(end) => i = end,
                None => return limit,
            },
            b')' | b']' | b'}' | b',' | b';' => return i,
            q @ (b'"' | b'\'') => i = skip_string(b, i, q),
            _ => (),
        }
        i += 1;
    }
    limit
}

/// `text[from..to]` with nested brackets and strings blanked out, so that only the
/// top-level tokens of an argument list remain at their original offsets.
fn top_level(text: &str, from: usize, to: usize) -> String {
    let b = text.as_bytes();
    let mut out = b[from..to].to_vec();
    let mut i = from;
    while i < to {
        let end = match b[i] {
            b'(' | b'[' | b'{' => closing(b, i).unwrap_or(to - 1).min(to - 1),
            q @ (b'"' | b'\'') => skip_string(b, i, q).min(to - 1),
            _ => {
                i += 1;
                continue;
            }
        };
        out[i - from..=end - from].fill(b' ');
        i = end + 1;
    }
    // Only whole ASCII-delimited ranges were blanked, so this stays valid UTF-8.
    String::from_utf8(out).unwrap()
}

/// Unitless number literals among the top-level tokens of `text[from..to]`.
fn dart_numbers(text: &str, from: usize, to: usize) -> Vec<(usize, String)> {
    let masked = top_level(text, from, to);
    NUMBER
        .find_iter(&masked)
        .filter(|m| standalone(masked.as_bytes(), m, b"_"))
        .map(|m| (from + m.start(), m.as_str().to_owned()))
        .collect()
}

/// Number literals in the value of each top-level argument named by `arg` inside the
/// argument list `text[from..to]`.
fn dart_named(text: &str, from: usize, to: usize, arg: &Regex) -> Vec<(usize, String)> {
    let masked = top_level(text, from, to);
    let b = text.as_bytes();
    arg.find_iter(&masked)
        .flat_map(|m| {
            let start = from + m.end();
            dart_numbers(text, start, argument_end(b, start, to))
        })
        .collect()
}

fn dart_values(text: &str, rule: &str) -> Vec<(usize, String)> {
    let b = text.as_bytes();
    let mut out = vec![];
    // The argument list of a call whose match ends just after its `(`.
    let args = |m: regex::Match| closing(b, m.end() - 1).map(|close| (m.end(), close));
    for (r, re) in DART_CALLS.iter() {
        if *r == rule {
            for (from, to) in re.find_iter(text).filter_map(args) {
                out.extend(dart_numbers(text, from, to));
            }
        }
    }
    for (r, call, arg) in DART_NAMED.iter() {
        if *r == rule {
            for (from, to) in call.find_iter(text).filter_map(args) {
                out.extend(dart_named(text, from, to, arg));
            }
        }
    }
    for (r, re) in DART_ANYWHERE.iter() {
        if *r == rule {
            for m in re.find_iter(text) {
                out.extend(dart_numbers(
                    text,
                    m.end(),
                    argument_end(b, m.end(), b.len()),
                ));
            }
        }
    }
    if rule == "shadow" {
        for (from, to) in DART_BOX_SHADOW.find_iter(text).filter_map(args) {
            for m in DART_OFFSET.find_iter(&text[from..to]) {
                let open = from + m.end() - 1;
                if let Some(close) = closing(b, open) {
                    out.extend(dart_numbers(text, open + 1, close));
                }
            }
        }
    }
    out
}
