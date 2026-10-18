use clap::{Parser, Subcommand};
use despec::{Diagnostic, config::Config};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, global = true, default_value = ".despec.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init,
    Generate,
    Check {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        generated_only: bool,
    },
    /// Record the remaining hard-coded values, or lower the recorded counts.
    Baseline,
}
const WARNING: &str = "This file is generated from .despec.toml. Run despec generate.";
const INITIAL: &str = r##"schema_version = 1
required_version = "@VERSION@"

[outputs]
css = "generated/tokens.css"
scss = "generated/_tokens.scss"
dart = "generated/tokens.dart"

[sources]
include = ["src/**/*.css", "src/**/*.scss", "lib/**/*.dart"]
exclude = []

[tokens."accent.primary"]
type = "color"
value = "#123456"
"##;
fn run(cli: &Cli) -> Result<(usize, Vec<Diagnostic>), (String, String)> {
    let fail = |e: String| ("config.invalid".into(), e);
    if matches!(cli.command, Command::Init) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&cli.config)
            .map_err(|e| fail(e.to_string()))?;
        file.write_all(
            INITIAL
                .replace("@VERSION@", env!("CARGO_PKG_VERSION"))
                .as_bytes(),
        )
        .map_err(|e| fail(e.to_string()))?;
        return Ok((0, vec![]));
    }
    let input = fs::read_to_string(&cli.config).map_err(|e| fail(e.to_string()))?;
    let c: Config = toml::from_str(&input).map_err(|e| fail(e.to_string()))?;
    c.validate().map_err(fail)?;
    despec::config::check_rule_tables(&input).map_err(fail)?;
    despec::catalog::validate(&c.tokens, &c.modes).map_err(|e| ("token.invalid".into(), e))?;
    let root = cli
        .config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = fs::canonicalize(root).map_err(|e| fail(e.to_string()))?;
    let config_path = fs::canonicalize(&cli.config).map_err(|e| fail(e.to_string()))?;
    let mut files = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for (target, p) in c.output_paths() {
        let path = root.join(p);
        let resolved = resolve(&root, &path).map_err(|e| fail(format!("output {e}")))?;
        if resolved == config_path || !seen.insert(resolved) {
            return Err(fail(
                "output aliases another output or the configuration".into(),
            ));
        }
        files.push((
            p,
            path,
            despec::catalog::render(&c.tokens, &c.modes, target, WARNING)
                .map_err(|e| ("token.invalid".into(), e))?,
        ));
    }
    // Only commands that read or write the baseline resolve it.
    let uses_baseline = matches!(
        cli.command,
        Command::Baseline
            | Command::Check {
                generated_only: false,
                ..
            }
    );
    let baseline = match c.baseline().filter(|_| uses_baseline) {
        Some(p) => {
            let path = root.join(p);
            let resolved = resolve(&root, &path).map_err(|e| fail(format!("baseline {e}")))?;
            if resolved == config_path || seen.contains(&resolved) {
                return Err(fail(
                    "sources.baseline aliases an output or the configuration".into(),
                ));
            }
            Some((p.to_string_lossy().into_owned(), path))
        }
        None => None,
    };
    if matches!(cli.command, Command::Baseline) {
        let Some((name, path)) = baseline else {
            return Err(fail("despec baseline requires [sources] baseline".into()));
        };
        return write_baseline(&root, &c, &name, &path);
    }
    if let Command::Check { generated_only, .. } = cli.command {
        let mut ds = vec![];
        for (p, path, expected) in files {
            match fs::read_to_string(path) {
                Ok(actual) if actual == expected => (),
                Ok(_) => ds.push(Diagnostic::new(
                    "generated.stale",
                    &p.to_string_lossy(),
                    "generated output differs; run despec generate",
                )),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => ds.push(Diagnostic::new(
                    "generated.stale",
                    &p.to_string_lossy(),
                    "generated output is missing; run despec generate",
                )),
                Err(e) => return Err(("io.error".into(), e.to_string())),
            }
        }
        let mut count = 0;
        if !generated_only {
            let (n, scan) =
                despec::scan::scan(&root, &c).map_err(|e| ("source.scan-error".into(), e))?;
            count = n;
            match &baseline {
                None => ds.extend(scan),
                Some((name, path)) => match read_baseline(name, path)? {
                    Err(d) => {
                        ds.extend(scan);
                        ds.push(d)
                    }
                    // An incomplete scan would make every entry look stale.
                    Ok(_) if scan.iter().any(|d| d.rule_id == "source.no-files") => ds.extend(scan),
                    Ok(recorded) => ds.extend(despec::baseline::apply(scan, &recorded, name)),
                },
            }
        }
        return Ok((count, ds));
    }
    let mut staged = vec![];
    for (_, path, text) in files {
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).map_err(|e| ("io.error".into(), e.to_string()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| ("io.error".into(), e.to_string()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| ("io.error".into(), e.to_string()))?;
        staged.push((file, path));
    }
    for (file, path) in staged {
        file.persist(path)
            .map_err(|e| ("io.error".into(), e.to_string()))?;
    }
    Ok((0, vec![]))
}
/// Resolves `path` through its nearest existing ancestor, which must stay inside
/// `root` so that a symlink cannot redirect a write elsewhere.
fn resolve(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let mut ancestor = path;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("path is invalid")?;
    }
    let resolved = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
    if !resolved.starts_with(root) {
        return Err("symlink escapes configuration directory".into());
    }
    Ok(resolved.join(path.strip_prefix(ancestor).unwrap()))
}
type Failure = (String, String);
/// Reads the baseline file; `Ok(Err(_))` is a `baseline.invalid` diagnostic.
fn read_baseline(
    name: &str,
    path: &Path,
) -> Result<Result<despec::baseline::Counts, Diagnostic>, Failure> {
    let invalid = |m: &str| Ok(Err(Diagnostic::new("baseline.invalid", name, m)));
    match fs::read_to_string(path) {
        Ok(text) => match despec::baseline::parse(&text) {
            Ok(counts) => Ok(Ok(counts)),
            Err(e) => invalid(&e),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            invalid("baseline file is missing; run despec baseline")
        }
        Err(e) => Err(("io.error".into(), e.to_string())),
    }
}
/// Creates the baseline from the current findings, or lowers it. Nothing is written
/// while any count would grow or another source diagnostic remains.
fn write_baseline(
    root: &Path,
    c: &Config,
    name: &str,
    path: &Path,
) -> Result<(usize, Vec<Diagnostic>), Failure> {
    use despec::baseline;
    let (n, scan) = despec::scan::scan(root, c).map_err(|e| ("source.scan-error".into(), e))?;
    if scan.iter().any(|d| !baseline::is_literal(d)) {
        let mut ds: Vec<_> = scan
            .into_iter()
            .filter(|d| !baseline::is_literal(d))
            .collect();
        ds.push(Diagnostic::new(
            "baseline.invalid",
            name,
            "baseline not written; fix the other source diagnostics first",
        ));
        return Ok((n, ds));
    }
    let found = baseline::count(&scan);
    let previous = match fs::read_to_string(path) {
        Ok(text) => {
            let recorded = match baseline::parse(&text) {
                Ok(recorded) => recorded,
                Err(e) => return Ok((n, vec![Diagnostic::new("baseline.invalid", name, &e)])),
            };
            let increases = baseline::increases(&found, &recorded);
            if !increases.is_empty() {
                let ds = increases
                    .into_iter()
                    .map(|((rule, source, literal), found, recorded)| {
                        let mut d = Diagnostic::new(
                            "baseline.increase",
                            source,
                            &format!(
                                "{rule} {literal}: found {found}, baseline {recorded}; baseline not written. Remove the new literal or add an [[exceptions]] entry with a reason"
                            ),
                        );
                        d.literal = Some(literal.clone());
                        d
                    })
                    .collect();
                return Ok((n, ds));
            }
            Some(text)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(("io.error".into(), e.to_string())),
    };
    let text = baseline::render(&found);
    // An unchanged file is left alone, including a CRLF checkout and a file still
    // in the layout written before 0.6.0.
    if previous.is_some_and(|p| {
        let p = p.replace("\r\n", "\n");
        p == text || p == baseline::render_legacy(&found)
    }) {
        return Ok((n, vec![]));
    }
    let io = |e: std::io::Error| ("io.error".to_string(), e.to_string());
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent).map_err(io)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)?;
    file.persist(path).map_err(|e| io(e.error))?;
    Ok((n, vec![]))
}
fn main() {
    let cli = Cli::try_parse().unwrap_or_else(|e| {
        let args: Vec<_> = std::env::args().collect();
        if e.use_stderr() && args.iter().any(|a|a=="check") && args.iter().any(|a|a=="--json") {
            println!("{}", serde_json::json!({"schema_version":1,"ok":false,"scanned_files":0,"diagnostics":[Diagnostic::new("cli.invalid","",&e.to_string())]}));
            std::process::exit(2);
        }
        e.exit()
    });
    let json = matches!(cli.command, Command::Check { json: true, .. });
    let (scanned_files, diagnostics) = match run(&cli) {
        Ok(result) => result,
        Err((id, message)) => (
            0,
            vec![Diagnostic::new(
                &id,
                &cli.config.to_string_lossy(),
                &message,
            )],
        ),
    };
    let ok = diagnostics.is_empty();
    if json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"ok":ok,"scanned_files":scanned_files,"diagnostics":diagnostics})
        )
    } else {
        for d in diagnostics {
            eprintln!(
                "{}: {}{}: {}",
                d.rule_id,
                d.path,
                d.line.map(|l| format!(":{l}")).unwrap_or_default(),
                d.message
            )
        }
    }
    if !ok {
        std::process::exit(1)
    }
}
