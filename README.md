# despec

A standalone Rust CLI for shared design tokens and explicit source policies.
The installed binary needs no Node, npm, Dart, or Python runtime.

## Usage

Build with Rust 1.92 or newer: `cargo build --release --locked`.
Install locally with `cargo install --path . --locked`.
Run `despec init`, edit `.despec.toml`, then `despec generate` and `despec check`.
`init` refuses to overwrite an existing file. `--config PATH` selects another
TOML configuration; all paths resolve relative to its directory.

```toml
schema_version = 1
required_version = "0.6.0"

[outputs]
css = "generated/tokens.css"            # one destination
scss = ["web/_tokens.scss", "admin/_tokens.scss"] # or several, one per project
dart = "generated/tokens.dart"

[sources]
include = ["src/**/*.css", "src/**/*.scss", "lib/**/*.dart"]
exclude = ["src/vendor/**"]
baseline = "despec-baseline.json" # optional: existing hard-coded values, see below

[modes.dark]
css = ".dark" # or '[data-theme="dark"]', or "@media (prefers-color-scheme: dark)"

[tokens."accent.primary"]
type = "color"
value = "#123456"
modes = { dark = "#7ab8ff" }

[tokens."type.body.md"]
type = "typography"
value = { fontSize = { value = 16, unit = "px" }, lineHeight = 1.6 }

[rules.spacing] # optional: also report hard-coded spacing
allow = ["1px"]

[rules.radius]
classes = ["rounded", "rounded-*"] # forbid these class names too

[[exceptions]]
rule = "source.color-literal"
path = "src/brand.css"
literal = "#abcdef"
reason = "Third-party brand specification"
count = 1
# line = 12 # optional, one-based
```

The configuration is the single source of truth: no external JSON/YAML catalog.
Schema version and exact CLI version are required. Unknown configuration keys,
duplicate TOML keys, invalid tokens, output collisions, and unsafe output paths
fail before output writing. At least one output and token are required. A target
lists one path or several; every listed file is generated identically, checked for
staleness by itself, and excluded from source scanning.

Modes give color and shadow tokens alternative values, such as a dark theme.
Declare each mode under `[modes.<name>]` with a kebab-case name, then set its value
beside the default with `modes = { <name> = <value> }`. A mode value has the same
form as the token's `value`.
CSS reuses the default property names in a block after `:root`, under the mode's
`css` selector, so tokens without a mode value keep their default there. A `css`
starting with `@media`, `@supports`, or `@container` instead wraps `:root`. Mode
blocks follow `:root` in mode-name order, so the selector needs at least the
specificity of `:root` (a class or attribute selector; not `html` or `*`), and when
several modes apply at once the last in name order wins. `css` is required only
when a CSS output is configured; braces, semicolons, comments, and unbalanced
quotes or brackets are rejected. Sass and Dart have no cascade, so
mode values become separate declarations named with the mode as a suffix:
`$accent-primary-dark` and `accentPrimaryDark`. Only tokens with a mode value get
one, so Sass and Dart consumers fall back to the default name themselves. Referring to an undeclared mode,
declaring a mode no token uses, and mode values on other token types are errors;
suffixed names take part in output name collision checks.

`rgbChannels = true` on a color token also emits its channels for CSS, so Tailwind
v3 style opacity works in every browser: `--ds-accent-primary-rgb: 18 52 86;` beside
`--ds-accent-primary`, used as `rgb(var(--ds-accent-primary-rgb) / <alpha-value>)`.
Mode blocks override the channels too. Every value of the token, mode values
included, must be #RRGGBB, because an rgba alpha would be lost. Sass and Dart
outputs are unchanged; `-rgb` names take part in CSS collision checks.

Tokens support `color` (#RRGGBB or rgba), `dimension` (numeric value and unit),
`number`, `keyword`, `fontFamily`, `shadow` (none or pixel offsetX/offsetY/blur
and color), and `typography` (fontSize and lineHeight; optional fontWeight and
letterSpacing). Quote token names containing dots. Metadata such as description
is accepted. `targets = { css = "name", scss = "name", dart = "camelName" }`
overrides names; `scssDefault = true` emits Sass `!default`. CSS names get a
`--ds-` prefix. Dart emits TextStyle for pixel typography with supported weights
100, 200, …, 900. Letter spacing in `em` is multiplied by the token’s pixel
font size for Dart; CSS/SCSS retain `em`. Numeric and `px` letter spacing are
unchanged. Non-pixel font sizes, unsupported letter-spacing units or weights,
and non-finite conversion results fall back to a constant map. Absent optional fields
remain absent. Dollar signs in Dart strings are escaped. Font families cannot
contain statement delimiters or line breaks.

`generate` validates and renders all targets before staging files beside their
destinations, then replaces each destination. Invalid input leaves outputs
unchanged. Replacement is atomic per file, not a transaction across all files;
an I/O failure during replacement can leave a partially updated set.
`check` never writes. It checks generated contents and source rules.
`check --generated-only` explicitly skips source policies for generation-only
projects or compatibility testing; normal check fails on absent/empty scan scope or any include pattern selecting zero files.

`check --json` emits one object with `schema_version: 1`, `ok`, `scanned_files`,
and `diagnostics` containing `rule_id`, `path`, `message`, and optional `line`.
`source.*-literal` diagnostics also carry `literal`, the matched source text.
Exit status is 0 for success, 1 for findings/configuration/I/O failures, and 2
for invalid CLI syntax. Rule IDs are `config.invalid`, `token.invalid`,
`generated.stale`, `source.color-literal`, `source.<rule>-literal` for each
dimension rule below, `source.no-files`,
`source.scan-error`, `exception.unused`, `baseline.increase`, `baseline.stale`,
`baseline.invalid`, `cli.invalid`, and `io.error`.

Source policies are deliberately lexical, not an AST analyzer. They detect
hex colors, numeric rgb/rgba/hsl/hsla functions, and Dart Color(0xAARRGGBB).
C-style, line, and HTML comments are ignored; quoted strings are scanned.
Standalone CSS ID selectors such as `#abcdef {` are ignored; complex selectors
can still produce false positives. Numeric color expressions
split over lines, language interpolation, aliases, named colors, and computed
colors are not fully analyzed. Keep existing language-specific AST guards.
Configure a meaningful explicit scope; generated files are skipped. Each include is walked from its fixed directory prefix to avoid traversing unrelated trees. Symlinks
are not followed. `.git`, `node_modules`, and `target` trees are excluded.
Exceptions require a reason and exact path/literal; counts must match exactly
(default 1). Unused or surplus exceptions fail checks. Do not turn every finding
into an exception to make a project pass; record existing ones in a baseline
instead.

### Baseline

A project adopting despec may already contain hundreds of hard-coded values. A
baseline records that existing debt so it can only shrink: `[sources] baseline`
names a JSON file (a relative path inside the configuration directory, not an
output or the configuration) that is never scanned. It records how many
`source.*-literal` findings remain per rule, path, and literal after exceptions
are applied, so an exception always wins over the baseline. A new hard-coded
value that is genuinely needed goes into `[[exceptions]]` with a reason, never
into the baseline.

```json
{
  "schema_version": 1,
  "entries": [
    {"rule": "source.radius-literal", "path": "src/card.css", "literal": "4px", "count": 1},
    {"rule": "source.spacing-literal", "path": "src/card.css", "literal": "12px", "count": 3}
  ]
}
```

`despec baseline` writes one entry per line, with the keys in the order `rule`,
`path`, `literal`, `count`, so a large baseline stays small and pull requests
that lower different entries rarely conflict. An empty baseline is
`"entries": []`. Entries are sorted by rule, path, and literal, each count is at
least 1, and the file ends with a newline. The file is read as ordinary JSON, so
the multi-line layout written before 0.6.0 (one key per line) is still accepted;
`despec baseline` converts it only when it rewrites the file because a count
dropped, and leaves an unchanged file in either layout as it is. Line numbers are not recorded, so moving code within a
file does not invalidate the file. Paths are recorded, so renaming a file with
recorded entries reports them as new; review that only the rename changed, then
delete the baseline and run `despec baseline` again.

`despec baseline` creates the file from the current findings when it is absent.
When it exists, the file is rewritten only if no count grows and no new
(rule, path, literal) appears: lowered counts are written and entries that reach
zero are removed; an unchanged result leaves the file untouched. Any growth
writes nothing and reports every grown entry as `baseline.increase` (exit 1).
It also writes nothing while other source diagnostics remain, such as
`source.no-files` or `exception.unused`, because an incomplete scan would
otherwise erase entries. Without `[sources] baseline` it fails with
`config.invalid`.

`despec check` compares the findings with the baseline. Findings of an entry at
or below its count are not reported. When an entry exceeds its count, all of its
findings are reported with `(found N, baseline M)` appended. An entry whose count
dropped, including to zero, fails as `baseline.stale` at the baseline path until
`despec baseline` lowers it. A missing or malformed file, or another
`schema_version`, is `baseline.invalid`. When the scan reports `source.no-files`,
the comparison is skipped. `check --generated-only` ignores the baseline.

### Dimension rules

Colors are always checked. Seven dimension rules run only when their table is
present: `[rules.spacing]`, `[rules.gap]`, `[rules.border-width]`,
`[rules.outline]`, `[rules.shadow]`, `[rules.radius]`, and `[rules.font-size]`.
Their findings use rule IDs such as `source.spacing-literal` and work with
`[[exceptions]]` exactly like colors; an exception matches only its own rule, and
an exception for a dimension rule requires that rule's table. A
table accepts only `allow`, values to accept compared as written but
case-insensitively (`"16"` does not cover `"16.0"`), and `classes`, class-name
globs to forbid. `[rules.color]` accepts only `classes`. Zero is always accepted,
with or without a unit. Unknown rule IDs and keys are configuration errors.

Detected forms, after comments are removed:

| Rule | CSS property (CSS, Sass, Vue, HTML, JSX/TSX) | Tailwind `-[...]` prefix | Flutter (`.dart`) |
|---|---|---|---|
| spacing | `margin`, `padding` and their `-top`/`-right`/`-bottom`/`-left`/`-inline`/`-block` (`-start`/`-end`) longhands | `m mx my mt mr mb ml ms me p px py pt pr pb pl ps pe space-x space-y` | `EdgeInsets` and `EdgeInsetsDirectional` `.all/.only/.symmetric/.fromLTRB/.fromSTEB` |
| gap | `gap`, `row-gap`, `column-gap`, `grid-gap` variants | `gap gap-x gap-y` | `SizedBox` `width:`/`height:`; `spacing:`, `runSpacing:`, `mainAxisSpacing:`, `crossAxisSpacing:` |
| border-width | `border`, its side and logical longhands, and their `-width` | `border` and `border-x/y/t/r/b/l/s/e` | `BorderSide` and `Border.all` `width:` |
| outline | `outline`, `outline-width`, `outline-offset` | `outline outline-offset ring ring-offset` | |
| shadow | offsets, blur, and spread in `box-shadow`, `text-shadow` | `shadow drop-shadow` | `BoxShadow` `blurRadius:`, `spreadRadius:`, and its `Offset(...)` |
| radius | `border-radius` and each corner | `rounded` and every side and corner | `BorderRadius.circular`, `Radius.circular`, `Radius.elliptical` |
| font-size | `font-size` | `text` with a length | `fontSize:` |

In a CSS declaration every nonzero number with a unit (`px`, `rem`, `em`, `%`,
viewport, container, and absolute units) is one finding, including later values of a
shorthand, `var()` fallbacks, `calc()` terms, and negative values; `literal` is the
number and unit, such as `12px`, with its sign (`allow = ["8px"]` does not cover
`-8px`). The property must start a line or follow
whitespace, `;`, `{`, or a quote (including a backtick). In CSS, SCSS, and Less
files the value ends at `;`, `}`, or a `{` opening a nested rule, so values wrapped
over several lines are checked and each number reports its own line. Elsewhere,
such as in indented `.sass`, style attributes, and embedded CSS, the value ends at `;`, `}`, a line
break, or a quote. A number directly after a letter, digit, `_`, `-`, `.`, `$`, or `#` is part of
a name and ignored. Tailwind classes may carry variants (`md:`), `!`, and a negative
`-`, which are removed from `literal` (`md:-mt-[4px]` reports `mt-[4px]`). A class
counts only when its value holds a nonzero unit number; it is accepted when `allow`
lists the class or every such number. `text-[...]` holding a color (`#fff`,
`rgb(...)`) or a non-`length` type hint is not a font size. In Dart, unitless
number literals at the top level of the listed arguments are reported, so names
such as `ds.space4` and nested calls such as `gap(4)` are not. `classes` applies to
files other than CSS, Sass, Less, and Dart: words split at whitespace, quotes,
parentheses, braces, and commas, with a trailing `:` (as in `{ rounded: on }`),
variants, `!`, and `-` removed, are matched
against the globs, and `literal` is the class name.

Not detected: camelCase style objects in JavaScript (`marginTop: 8`, `padding:
"8px"`), `line-height` and `letter-spacing`, custom property and Sass variable
definitions (`--card-padding: 12px`, `$gap: 12px`), media queries and other
properties such as `width`, and values assembled at run time or through
interpolation. Unrelated text can still match lexically, such as a word in prose that
equals a forbidden class name.

## Development and releases

Run `cargo fmt --check`, `cargo test --locked`, and
`cargo clippy --locked --all-targets -- -D warnings`.
Tests cover legacy generated bodies, malformed input, nonwriting checks,
source scope, and exception behavior. Legacy reference outputs are test data;
only their first-line generation notice changes.

CI tests Linux and macOS. The release workflow builds macOS arm64/x86_64 and
Linux x86_64 binaries and checksum files as workflow artifacts on a version tag
or manual dispatch. Publishing a release remains a separate manual action.
The project had no license declaration; this initial implementation adopts MIT.

Before a crates.io release, inspect `cargo package --list --locked` and run
`cargo publish --dry-run --locked`. In a worktree, first run `portool sync` and
`portool check`, then run these Cargo commands through `portool exec --`.
The package explicitly includes sources, tests, manifest, lockfile, README,
and license; generated local settings are excluded. A successful dry-run
validates packaging and compilation, not crate-name availability or publishing
permissions. Actual publication requires a separate authorized action.
