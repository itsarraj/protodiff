use std::fs;
use std::path::PathBuf;

use clap::Parser;

use protodiff::diff::{diff, Severity};
use protodiff::parse::parse_proto;

#[derive(Parser)]
#[command(
    name = "protodiff",
    about = "Diffs two .proto files for wire-breaking changes — a removed field, a changed field number or type, a removed RPC"
)]
struct Cli {
    /// The old (baseline) .proto file.
    old: PathBuf,
    /// The new (candidate) .proto file.
    new: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let old_text = fs::read_to_string(&cli.old)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", cli.old.display()))?;
    let new_text = fs::read_to_string(&cli.new)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", cli.new.display()))?;

    let old = parse_proto(&old_text);
    let new = parse_proto(&new_text);

    let findings = diff(&old, &new);
    if findings.is_empty() {
        println!("no breaking changes found");
        return Ok(());
    }

    for f in &findings {
        println!("{}: {}", f.severity, f.summary);
    }

    let breaking = findings.iter().any(|f| f.severity == Severity::Breaking);
    if breaking {
        std::process::exit(1);
    }
    Ok(())
}
