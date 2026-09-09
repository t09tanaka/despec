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
}
const WARNING: &str = "This file is generated from .despec.toml. Run despec generate.";
const INITIAL: &str = r##"schema_version = 1
required_version = "0.1.0"

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
        file.write_all(INITIAL.as_bytes())
            .map_err(|e| fail(e.to_string()))?;
        return Ok((0, vec![]));
    }
    let input = fs::read_to_string(&cli.config).map_err(|e| fail(e.to_string()))?;
    let c: Config = toml::from_str(&input).map_err(|e| fail(e.to_string()))?;
    c.validate().map_err(fail)?;
    despec::catalog::validate(&c.tokens).map_err(|e| ("token.invalid".into(), e))?;
    let root = cli
        .config
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = fs::canonicalize(root).map_err(|e| fail(e.to_string()))?;
    let config_path = fs::canonicalize(&cli.config).map_err(|e| fail(e.to_string()))?;
    let mut files = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for (target, p) in &c.outputs {
        let path = root.join(p);
        let mut ancestor = path.as_path();
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| fail("invalid output path".into()))?;
        }
        let resolved = fs::canonicalize(ancestor).map_err(|e| fail(e.to_string()))?;
        if !resolved.starts_with(&root) {
            return Err(fail(
                "output symlink escapes configuration directory".into(),
            ));
        }
        let resolved = resolved.join(path.strip_prefix(ancestor).unwrap());
        if resolved == config_path || !seen.insert(resolved) {
            return Err(fail(
                "output aliases another output or the configuration".into(),
            ));
        }
        files.push((
            p,
            path,
            despec::catalog::render(&c.tokens, target, WARNING)
                .map_err(|e| ("token.invalid".into(), e))?,
        ));
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
            ds.extend(scan)
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
