//! `panchat` — read an AI chat export and write it somewhere useful.
//!
//! The CLI exists so the library's claim is checkable in one command without
//! writing any code: `panchat chatgpt-export/ --format json | jq` either works
//! or it does not.
//!
//! The input is an unpacked export — a directory, or a single
//! `conversations.json`. Zip archives are not read; the crate says so rather
//! than reporting the archive as unrecognisable.

use clap::{Parser, ValueEnum};
use panchat::export::{self, Branches};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "panchat",
    about = "Read AI chat exports from any vendor into one representation.",
    version
)]
struct Args {
    /// Export file or directory (ChatGPT conversations.json, a Claude export
    /// folder, …). Vendor is detected automatically.
    input: PathBuf,

    /// Output file. Writes to stdout when omitted.
    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(short, long, value_enum, default_value_t = Format::Json)]
    format: Format,

    /// Include every branch, not just the one the vendor marked current.
    #[arg(long)]
    all_branches: bool,

    /// Print what was detected and what would be lost, then exit.
    #[arg(long)]
    inspect: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    /// The full representation, pretty-printed.
    Json,
    /// One conversation per line.
    Jsonl,
    /// One `{role, content}` turn per line.
    Turns,
    /// Markdown with YAML frontmatter, one document per conversation.
    Markdown,
}

fn main() {
    // Debug-formatted errors (`NotRecognized("…")`) are for us; a person
    // holding an export that did not import needs the sentence inside them.
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let files = panchat::read_path(&args.input)?;

    if args.inspect {
        match panchat::detect(&files) {
            Some(d) => {
                println!("platform:   {}", d.platform);
                println!("variant:    {}", d.variant);
                println!("shape:      v{}", d.variant_version);
                println!("confidence: {:.2}", d.confidence);
                for n in &d.notes {
                    println!("note:       {n}");
                }
            }
            None => {
                println!("platform:   unrecognised");
                return Ok(());
            }
        }
    }

    let doc = panchat::normalize(&files)?;

    // Warnings go to stderr so `panchat … | jq` stays clean while the user
    // still sees what their export left behind.
    for w in &doc.warnings {
        let scope = w.conversation_id.as_deref().unwrap_or("export");
        let times = if w.count > 1 {
            format!(" (x{})", w.count)
        } else {
            String::new()
        };
        eprintln!(
            "{:?}: {}{} [{}]",
            w.severity,
            w.code.describe(),
            times,
            scope
        );
    }

    if args.inspect {
        println!("conversations: {}", doc.conversations.len());
        println!("artifacts:     {}", doc.artifacts.len());
        println!("warnings:      {}", doc.warnings.len());
        return Ok(());
    }

    let branches = if args.all_branches {
        Branches::All
    } else {
        Branches::ActiveOnly
    };

    let rendered = match args.format {
        Format::Json => export::to_json(&doc)?,
        Format::Jsonl => export::to_jsonl(&doc)?,
        Format::Turns => export::to_turns_jsonl(&doc, branches)?,
        Format::Markdown => doc
            .conversations
            .iter()
            .map(|c| export::to_markdown(c, branches))
            .collect::<Vec<_>>()
            .join("\n---\n\n"),
    };

    match args.output {
        Some(path) => std::fs::write(path, rendered)?,
        None => print!("{rendered}"),
    }
    Ok(())
}
