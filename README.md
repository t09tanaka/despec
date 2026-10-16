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
required_version = "0.4.0"

[outputs]
css = "generated/tokens.css"            # one destination
scss = ["web/_tokens.scss", "admin/_tokens.scss"] # or several, one per project
dart = "generated/tokens.dart"

[sources]
include = ["src/**/*.css", "src/**/*.scss", "lib/**/*.dart"]
exclude = ["src/vendor/**"]

[modes.dark]
css = ".dark" # or '[data-theme="dark"]', or "@media (prefers-color-scheme: dark)"

[tokens."accent.primary"]
type = "color"
value = "#123456"
modes = { dark = "#7ab8ff" }

[tokens."type.body.md"]
type = "typography"
value = { fontSize = { value = 16, unit = "px" }, lineHeight = 1.6 }

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
Exit status is 0 for success, 1 for findings/configuration/I/O failures, and 2
for invalid CLI syntax. Rule IDs are `config.invalid`, `token.invalid`,
`generated.stale`, `source.color-literal`, `source.no-files`,
`source.scan-error`, `exception.unused`, `cli.invalid`, and `io.error`.

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
(default 1). Unused or surplus exceptions fail checks. Do not automatically
baseline every finding to make a project pass.

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
