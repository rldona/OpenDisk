//! `opendisk`: scan, list findings and clean safely from the terminal.
//!
//! Cleaning is a dry run unless `--execute` is given.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use opendisk_core::clean::{self, Action, CleanOptions, Selection, Status};
use opendisk_core::findings::{self, Finding, MeasureSizer};
use opendisk_core::format::{bytes, parse_bytes};
use opendisk_core::journal::Journal;
use opendisk_core::rules::{RuleSet, Safety};
use opendisk_core::scan::{self, DirNode, ScanOptions};
use opendisk_core::system::RealSystem;
use opendisk_core::{fda, index, paths, snapshots};

#[derive(Parser)]
#[command(
    name = "opendisk",
    version,
    about = "Find what fills your disk and free space safely"
)]
struct Cli {
    /// Home folder the rules apply to (default: $HOME). Point it at a fixture
    /// tree to try OpenDisk without touching your files.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a folder and show where the space goes.
    Scan {
        /// Folder to scan (default: the home folder). Use `/System/Volumes/Data`
        /// for the whole data volume.
        path: Option<PathBuf>,
        /// Levels of folders to print.
        #[arg(long, default_value_t = 2)]
        depth: usize,
        /// Children shown per folder.
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        json: bool,
        /// Don't save the scan in the local index.
        #[arg(long)]
        no_index: bool,
    },
    /// List what the rules found, grouped by category, largest first.
    Findings {
        /// Hide items smaller than this (e.g. 100MB).
        #[arg(long, default_value = "1MB")]
        min_size: String,
        /// Show every matched path, not only the top 5 per rule.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Clean findings. Dry run unless --execute is given.
    Clean {
        /// Only these rule IDs (repeatable). Default: every regenerable rule.
        #[arg(long = "rule")]
        rules: Vec<String>,
        /// Only rules with these safety levels (repeatable).
        #[arg(long, value_enum)]
        safety: Vec<SafetyArg>,
        /// Hide items smaller than this.
        #[arg(long, default_value = "1MB")]
        min_size: String,
        /// Really clean. Without it nothing is changed.
        #[arg(long)]
        execute: bool,
        /// Explicit form of the default; conflicts with --execute.
        #[arg(long, conflicts_with = "execute")]
        dry_run: bool,
        /// Delete regenerable caches directly instead of moving them to the Trash.
        #[arg(long)]
        delete_regenerable: bool,
        /// Allow emptying the Trash (permanent).
        #[arg(long)]
        allow_permanent: bool,
        /// Don't ask for confirmation before executing.
        #[arg(long, short)]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    /// Put back what a cleaning run moved to the Trash.
    Undo { run_id: String },
    /// List the built-in rules.
    Rules,
    /// Show APFS local snapshots.
    Snapshots,
    /// Check Full Disk Access.
    Fda {
        /// Open the Full Disk Access settings pane.
        #[arg(long)]
        open: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum SafetyArg {
    Regenerable,
    Review,
    Personal,
}

impl From<SafetyArg> for Safety {
    fn from(s: SafetyArg) -> Self {
        match s {
            SafetyArg::Regenerable => Safety::Regenerable,
            SafetyArg::Review => Safety::Review,
            SafetyArg::Personal => Safety::Personal,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = match cli.home {
        Some(h) => std::fs::canonicalize(&h).with_context(|| format!("--home {}", h.display()))?,
        None => std::env::var_os("HOME").map(PathBuf::from).context("HOME is not set")?,
    };
    match cli.command {
        Command::Scan {
            path,
            depth,
            top,
            json,
            no_index,
        } => cmd_scan(&home, path.unwrap_or_else(|| home.clone()), depth, top, json, no_index),
        Command::Findings { min_size, all, json } => cmd_findings(&home, &min_size, all, json),
        Command::Clean {
            rules,
            safety,
            min_size,
            execute,
            dry_run: _,
            delete_regenerable,
            allow_permanent,
            yes,
            json,
        } => cmd_clean(
            &home,
            CleanArgs {
                rules,
                safety: safety.into_iter().map(Into::into).collect(),
                min_size,
                execute,
                opts: CleanOptions {
                    delete_regenerable,
                    allow_permanent,
                },
                yes,
                json,
            },
        ),
        Command::Undo { run_id } => cmd_undo(&home, &run_id),
        Command::Rules => cmd_rules(),
        Command::Snapshots => cmd_snapshots(),
        Command::Fda { open } => cmd_fda(&home, open),
    }
}

fn min_bytes(s: &str) -> Result<u64> {
    parse_bytes(s).with_context(|| format!("invalid size {s:?}"))
}

fn cmd_scan(home: &Path, root: PathBuf, depth: usize, top: usize, json: bool, no_index: bool) -> Result<()> {
    let opts = ScanOptions::new(&root);
    let progress = opts.progress.clone();
    let cancel = opts.cancel.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let show = !json && std::io::stderr().is_terminal();
    let ticker = {
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if show {
                    let p = progress.snapshot();
                    eprint!("\rscanning… {} files, {}   ", p.files, bytes(p.bytes));
                    let _ = std::io::stderr().flush();
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };
    let _ = cancel; // Ctrl-C simply terminates the CLI.
    let result = scan::scan(&opts).with_context(|| format!("cannot scan {}", root.display()));
    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();
    if show {
        eprint!("\r\x1b[2K");
    }
    let result = result?;
    if !no_index {
        let db = paths::data_dir(home).join("index.sqlite");
        if let Err(e) = index::Index::open(&db).and_then(|mut i| i.record(&result)) {
            eprintln!("warning: could not save scan index: {e}");
        }
    }
    if json {
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    println!(
        "{}  {}  ({} files, {:.1}s)",
        bytes(result.tree.size),
        result.root.display(),
        result.stats.files,
        result.duration_ms as f64 / 1000.0
    );
    print_tree(&result.tree, 1, depth, top);
    if !result.denied.is_empty() {
        println!(
            "\n{} folders could not be read (e.g. {}). Grant Full Disk Access to your terminal to include them.",
            result.denied.len(),
            result.denied[0].display()
        );
    }
    Ok(())
}

fn print_tree(node: &DirNode, level: usize, depth: usize, top: usize) {
    if level > depth {
        return;
    }
    for child in node.children.iter().take(top) {
        let flag = if child.denied { "  (no access)" } else { "" };
        println!(
            "{:>10}  {}{}{flag}",
            bytes(child.size),
            "  ".repeat(level - 1),
            child.name
        );
        print_tree(child, level + 1, depth, top);
    }
}

fn safety_label(s: Safety) -> &'static str {
    match s {
        Safety::Regenerable => "regenerable",
        Safety::Review => "review",
        Safety::Personal => "personal",
    }
}

fn load_findings(home: &Path, min: u64) -> Vec<Finding> {
    let mut found = findings::find(RuleSet::builtin(), home, &MeasureSizer);
    for f in &mut found {
        f.items.retain(|i| i.bytes >= min);
        f.total_bytes = f.items.iter().map(|i| i.bytes).sum();
    }
    found.retain(|f| !f.items.is_empty());
    found
}

fn cmd_findings(home: &Path, min_size: &str, all: bool, json: bool) -> Result<()> {
    let found = load_findings(home, min_bytes(min_size)?);
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    let mut categories: Vec<(&str, u64)> = Vec::new();
    for f in &found {
        match categories.iter_mut().find(|(c, _)| *c == f.category_label) {
            Some((_, total)) => *total += f.total_bytes,
            None => categories.push((&f.category_label, f.total_bytes)),
        }
    }
    categories.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    let grand: u64 = found.iter().map(|f| f.total_bytes).sum();
    for (cat, total) in &categories {
        println!("\n{cat} — {}", bytes(*total));
        for f in found.iter().filter(|f| f.category_label == *cat) {
            println!(
                "  {:>10}  {} [{}]  ({})",
                bytes(f.total_bytes),
                f.name,
                safety_label(f.safety),
                f.rule_id
            );
            let shown = if all { f.items.len() } else { 5 };
            for i in f.items.iter().take(shown) {
                println!("  {:>10}      {}", bytes(i.bytes), display_home(&i.path, home));
            }
            if f.items.len() > shown {
                println!("              … and {} more", f.items.len() - shown);
            }
        }
    }
    println!("\nTotal found: {} in {} rules", bytes(grand), found.len());
    if fda::status(home).state == fda::FdaState::Denied {
        println!("Note: no Full Disk Access; Mail, Messages, Trash and some containers were not counted.");
    }
    Ok(())
}

fn display_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

struct CleanArgs {
    rules: Vec<String>,
    safety: Vec<Safety>,
    min_size: String,
    execute: bool,
    opts: CleanOptions,
    yes: bool,
    json: bool,
}

fn cmd_clean(home: &Path, args: CleanArgs) -> Result<()> {
    let rules = RuleSet::builtin();
    for id in &args.rules {
        if rules.get(id).is_none() {
            bail!("unknown rule {id:?} (see `opendisk rules`)");
        }
    }
    let found = load_findings(home, min_bytes(&args.min_size)?);
    let selected: Vec<Selection> = found
        .iter()
        .filter(|f| {
            if !args.rules.is_empty() {
                args.rules.contains(&f.rule_id)
            } else if !args.safety.is_empty() {
                args.safety.contains(&f.safety)
            } else {
                f.safety == Safety::Regenerable
            }
        })
        .flat_map(|f| {
            f.items.iter().map(|i| Selection {
                rule_id: Some(f.rule_id.clone()),
                path: i.path.clone(),
                bytes: i.bytes,
            })
        })
        .collect();
    if selected.is_empty() {
        println!("Nothing to clean.");
        return Ok(());
    }
    let sys = RealSystem;
    let plan = clean::plan(&selected, rules, home, &sys, &args.opts);
    if !args.json {
        println!("Plan {}:", plan.id);
        for item in &plan.items {
            let action = match &item.action {
                Action::Trash => "trash".to_string(),
                Action::Delete => "delete".to_string(),
                Action::Command { argv, .. } => format!("run `{}`", argv.join(" ")),
            };
            let status = if item.blockers.is_empty() {
                String::new()
            } else {
                format!("  SKIP: {}", item.blockers.join("; "))
            };
            println!(
                "  {:>10}  {:<26} {}{status}",
                bytes(item.bytes),
                action,
                display_home(&item.path, home)
            );
        }
        println!(
            "\nTo Trash: {}   Delete now: {}   Via tool: {}   Blocked: {}",
            bytes(plan.trash_bytes),
            bytes(plan.delete_bytes),
            bytes(plan.command_bytes),
            bytes(plan.blocked_bytes)
        );
    }
    let dry_run = !args.execute;
    if !dry_run && !args.yes {
        if !std::io::stdin().is_terminal() {
            bail!("refusing to execute without a terminal; pass --yes to confirm");
        }
        print!("Proceed? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }
    let journal = Journal::open(&paths::data_dir(home))?;
    let report = clean::execute(&plan, rules, &sys, Some(&journal), dry_run)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let ok = report
        .outcomes
        .iter()
        .filter(|o| !matches!(o.status, Status::Skipped | Status::Failed))
        .count();
    for o in report
        .outcomes
        .iter()
        .filter(|o| matches!(o.status, Status::Failed | Status::Skipped))
    {
        println!(
            "  {:?}: {} — {}",
            o.status,
            display_home(&o.path, home),
            o.message.as_deref().unwrap_or("")
        );
    }
    if dry_run {
        println!(
            "\nDry run: {ok} items would be cleaned, {} skipped. Nothing was changed. Re-run with --execute to clean.",
            report.skipped
        );
    } else {
        println!(
            "\nDone (run {}): {ok} items cleaned, {} skipped, {} failed.",
            report.run_id, report.skipped, report.failed
        );
        println!("Space really freed (statfs): {}", signed_bytes(report.freed_bytes));
        if report.trashed_bytes > 0 {
            println!(
                "Moved to Trash: {} — freed when you empty the Trash. Undo with `opendisk undo {}`.",
                bytes(report.trashed_bytes),
                report.run_id
            );
        }
    }
    Ok(())
}

fn signed_bytes(n: i64) -> String {
    if n < 0 {
        format!("-{}", bytes(n.unsigned_abs()))
    } else {
        bytes(n as u64)
    }
}

fn cmd_undo(home: &Path, run_id: &str) -> Result<()> {
    let journal = Journal::open(&paths::data_dir(home))?;
    let (restored, failed) = journal.undo(run_id)?;
    for p in &restored {
        println!("restored {}", display_home(p, home));
    }
    for (p, why) in &failed {
        println!("could not restore {}: {why}", display_home(p, home));
    }
    if restored.is_empty() && failed.is_empty() {
        println!("Nothing to undo for run {run_id}.");
    }
    Ok(())
}

fn cmd_rules() -> Result<()> {
    for r in &RuleSet::builtin().rules {
        println!(
            "{:<24} {:<12} {:<12} {}",
            r.id,
            safety_label(r.safety),
            r.category,
            r.name
        );
    }
    Ok(())
}

fn cmd_snapshots() -> Result<()> {
    let report = snapshots::list();
    if let Some(e) = &report.error {
        println!("Could not list snapshots: {e}");
    }
    if report.snapshots.is_empty() {
        println!("No APFS local snapshots on /.");
    }
    for s in &report.snapshots {
        println!("  {}", s.name);
    }
    println!("\n{}", report.explanation);
    println!(
        "\nTo remove Time Machine local snapshots yourself: {}",
        report.thin_command
    );
    Ok(())
}

fn cmd_fda(home: &Path, open: bool) -> Result<()> {
    let status = fda::status(home);
    println!(
        "Full Disk Access: {:?}{}",
        status.state,
        status
            .probed
            .map(|p| format!(" (probed {})", p.display()))
            .unwrap_or_default()
    );
    if open {
        fda::open_settings()?;
    }
    Ok(())
}
