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
required_version = "0.1.0"

[outputs]
css = "generated/tokens.css"
scss = "generated/_tokens.scss"
dart = "generated/tokens.dart"

[sources]
include = ["src/**/*.css", "src/**/*.scss", "lib/**/*.dart"]
exclude = ["src/vendor/**"]

[tokens."accent.primary"]
type = "color"
value = "#123456"

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
fail before output writing. At least one output and token are required.

Tokens support `color` (#RRGGBB or rgba), `dimension` (numeric value and unit),
`number`, `keyword`, `fontFamily`, `shadow` (none or pixel offsetX/offsetY/blur
and color), and `typography` (fontSize and lineHeight; optional fontWeight and
letterSpacing). Quote token names containing dots. Metadata such as description
is accepted. `targets = { css = "name", scss = "name", dart = "camelName" }`
overrides names; `scssDefault = true` emits Sass `!default`. CSS names get a
`--ds-` prefix. Dart emits TextStyle for pixel typography with supported weights
100, 200, …, 900; other typography becomes a constant map. Absent optional fields
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
