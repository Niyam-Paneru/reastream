//! `reastream` — project a REA result document without holding it.
//!
//! The tool is deliberately small: it reads one document, applies one rule set,
//! and writes the records. What it does not do is buffer, which is the whole
//! point — a document too large to parse is still small enough to read.

use std::io;

use reastream::{application_rules, Dossier};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match Options::parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("reastream: {message}");
            eprintln!();
            eprint!("{}", usage());
            std::process::exit(2);
        }
    };

    if options.help {
        eprint!("{}", usage());
        return;
    }

    let rules = application_rules();
    let dossier = match options.source() {
        Some(path) => {
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("reastream: {}: {error}", path.display());
                    std::process::exit(2);
                }
            };
            match Dossier::with_rules(file, &rules) {
                Ok(dossier) => dossier,
                Err(error) => {
                    eprintln!("reastream: {}: {error}", path.display());
                    std::process::exit(2);
                }
            }
        }
        None => {
            let stdin = io::stdin();
            match Dossier::with_rules(stdin.lock(), &rules) {
                Ok(dossier) => dossier,
                Err(error) => {
                    eprintln!("reastream: stdin: {error}");
                    std::process::exit(2);
                }
            }
        }
    };

    match options.mode {
        Mode::Count => print_summary(&dossier),
        Mode::Table => print_table(&dossier),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    /// Records, one field pair per line.
    #[default]
    Table,
    /// What was read and what was kept.
    Count,
}

#[derive(Debug, Clone, Default)]
struct Options {
    path: Option<String>,
    mode: Mode,
    help: bool,
}

impl Options {
    fn source(&self) -> Option<PathBuf> {
        self.path.as_ref().map(PathBuf::from)
    }
}

use std::path::PathBuf;

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options {
            mode: Mode::Table,
            ..Options::default()
        };
        for arg in args {
            match arg.as_str() {
                "--help" | "-h" => options.help = true,
                "--count" => options.mode = Mode::Count,
                other if other.starts_with('-') => {
                    return Err(format!("unknown option: {other}"));
                }
                other => {
                    if options.path.is_some() {
                        return Err(format!("unexpected argument: {other}"));
                    }
                    options.path = Some(other.to_string());
                }
            }
        }
        Ok(options)
    }
}

fn usage() -> String {
    format!(
        "usage: reastream <result.json> [--count]\n\
         \n\
         \x20 <result.json>   A REA result document. Omit to read standard input.\n\
         \x20 --count         Report what was read and what was kept.\n\
         \n\
         \x20 The built-in rules, and the measurement that justifies them, are in\n\
         \x20 docs/measurements/README.md.\n\
         \n"
    )
}

fn print_summary(dossier: &Dossier) {
    let summary = dossier.summary();
    println!("records        {}", summary.records);
    println!("encoding       {}", encoding_name(summary.encoding));
    println!("bytes read     {}", summary.bytes_in);
    println!("bytes decoded  {}", summary.bytes_decoded);
    println!("bytes kept     {}", dossier.kept_bytes());
    println!("kept ratio     {:.6}%", summary.kept_ratio * 100.0);
    println!("complete       {}", summary.complete);
}

fn encoding_name(encoding: Option<reastream::Encoding>) -> &'static str {
    match encoding {
        Some(reastream::Encoding::Utf8) => "utf8",
        Some(reastream::Encoding::Utf8WithBom) => "utf8 (bom)",
        Some(reastream::Encoding::Utf16Le) => "utf16le",
        Some(reastream::Encoding::Utf16Be) => "utf16be",
        None => "unknown",
    }
}

fn print_table(dossier: &Dossier) {
    let summary = dossier.summary();
    eprintln!(
        "# encoding={} complete={} records={} bytes_kept={}",
        encoding_name(summary.encoding),
        summary.complete,
        summary.records,
        dossier.kept_bytes()
    );
    for record in &dossier.records {
        let fields: Vec<String> = record
            .fields
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        println!("{}", fields.join(" "));
    }
}
