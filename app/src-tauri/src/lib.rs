//! Tauri shell around `opendisk-core`. The UI can only call the commands
//! registered here (and allowed in `capabilities/default.json`); there are no
//! filesystem or shell plugins.

use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use opendisk_core::clean::{self, CleanOptions, Plan, Report, Selection};
use opendisk_core::fda::{self, FdaStatus};
use opendisk_core::findings::{self, Finding, ScanSizer};
use opendisk_core::journal::Journal;
use opendisk_core::rules::RuleSet;
use opendisk_core::scan::{self, DirNode, ScanOptions, ScanResult};
use opendisk_core::snapshots::{self, SnapshotReport};
use opendisk_core::system::{self, RealSystem, TRASH_NAME};
use opendisk_core::{index, paths};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    home: PathBuf,
    scan: Mutex<Option<Arc<ScanResult>>>,
    findings: Mutex<Vec<Finding>>,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    plans: Mutex<HashMap<String, Plan>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct AppInfo {
    home: String,
    version: &'static str,
    data_volume: String,
    os: &'static str,
    /// `/` or `\\`, so the UI can join and split paths.
    path_sep: &'static str,
    /// "Trash" or "Recycle Bin".
    trash_name: &'static str,
    /// "Finder", "Files" or "File Explorer".
    file_manager: &'static str,
}

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        home: state.home.to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION"),
        data_volume: paths::data_volume(&state.home).to_string_lossy().into_owned(),
        os: std::env::consts::OS,
        path_sep: std::path::MAIN_SEPARATOR_STR,
        trash_name: TRASH_NAME,
        file_manager: if cfg!(target_os = "macos") {
            "Finder"
        } else if cfg!(windows) {
            "File Explorer"
        } else {
            "Files"
        },
    }
}

#[derive(Serialize)]
struct DiskInfo {
    total: u64,
    available: u64,
}

#[tauri::command]
fn disk_info(state: State<'_, AppState>) -> CmdResult<DiskInfo> {
    Ok(DiskInfo {
        total: system::total_bytes(&state.home).map_err(err)?,
        available: system::available_bytes(&state.home).map_err(err)?,
    })
}

#[tauri::command]
fn fda_status(state: State<'_, AppState>) -> FdaStatus {
    fda::status(&state.home)
}

#[tauri::command]
fn open_fda_settings() -> CmdResult<()> {
    fda::open_settings().map_err(err)
}

#[derive(Clone, Serialize)]
struct ScanProgress {
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    elapsed_ms: u64,
    phase: &'static str,
}

#[derive(Clone, Serialize)]
struct ScanDone {
    root: String,
    total_bytes: u64,
    files: u64,
    duration_ms: u64,
    cancelled: bool,
    denied: Vec<String>,
    findings_bytes: u64,
    error: Option<String>,
}

/// Starts a scan in the background. Progress arrives as `scan-progress`
/// events, the end as one `scan-done` event.
#[tauri::command]
fn start_scan(app: AppHandle, state: State<'_, AppState>, target: String) -> CmdResult<()> {
    let root = match target.as_str() {
        "home" => state.home.clone(),
        "volume" => paths::data_volume(&state.home),
        _ => return Err("unknown scan target".into()),
    };
    let mut cancel_slot = state.cancel.lock().unwrap();
    if cancel_slot.is_some() {
        return Err("a scan is already running".into());
    }
    let opts = ScanOptions::new(&root);
    *cancel_slot = Some(opts.cancel.clone());
    drop(cancel_slot);
    let home = state.home.clone();

    std::thread::spawn(move || {
        let started = Instant::now();
        let done = Arc::new(AtomicBool::new(false));
        let phase = Arc::new(Mutex::new("scanning"));
        let ticker = {
            let (app, done, phase, progress) = (app.clone(), done.clone(), phase.clone(), opts.progress.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    let p = progress.snapshot();
                    let _ = app.emit(
                        "scan-progress",
                        ScanProgress {
                            files: p.files,
                            dirs: p.dirs,
                            bytes: p.bytes,
                            errors: p.errors,
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            phase: *phase.lock().unwrap(),
                        },
                    );
                    std::thread::sleep(Duration::from_millis(150));
                }
            })
        };

        let state = app.state::<AppState>();
        let outcome = scan::scan(&opts);
        let mut done_event = ScanDone {
            root: root.to_string_lossy().into_owned(),
            total_bytes: 0,
            files: 0,
            duration_ms: 0,
            cancelled: false,
            denied: Vec::new(),
            findings_bytes: 0,
            error: None,
        };
        match outcome {
            Ok(result) => {
                *phase.lock().unwrap() = "findings";
                let found = if result.cancelled {
                    Vec::new()
                } else {
                    findings::find(RuleSet::builtin(), &home, &ScanSizer(&result))
                };
                if !result.cancelled {
                    let db = paths::data_dir(&home).join("index.sqlite");
                    let _ = index::Index::open(&db).and_then(|mut i| i.record(&result));
                }
                done_event.total_bytes = result.tree.size;
                done_event.files = result.tree.files;
                done_event.duration_ms = result.duration_ms;
                done_event.cancelled = result.cancelled;
                done_event.denied = result
                    .denied
                    .iter()
                    .take(20)
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                done_event.findings_bytes = found.iter().map(|f| f.total_bytes).sum();
                *state.findings.lock().unwrap() = found;
                *state.scan.lock().unwrap() = Some(Arc::new(result));
            }
            Err(e) => done_event.error = Some(e.to_string()),
        }
        done.store(true, Ordering::Relaxed);
        let _ = ticker.join();
        *state.cancel.lock().unwrap() = None;
        let _ = app.emit("scan-done", done_event);
    });
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    if let Some(c) = state.cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
struct TreeNode {
    name: String,
    path: String,
    size: u64,
    files: u64,
    denied: bool,
    /// Folded small children ("other").
    other: bool,
    children: Vec<TreeNode>,
}

/// Biggest children kept per folder; the rest are folded into one "other".
const MAX_CHILDREN: usize = 40;

fn to_dto(node: &DirNode, path: &Path, depth: u32) -> TreeNode {
    let mut children = Vec::new();
    if depth > 0 {
        for c in node.children.iter().take(MAX_CHILDREN) {
            children.push(to_dto(c, &path.join(&c.name), depth - 1));
        }
        let rest: Vec<&DirNode> = node.children.iter().skip(MAX_CHILDREN).collect();
        let files_size = node.own_files_size;
        let rest_size: u64 = rest.iter().map(|c| c.size).sum::<u64>() + files_size;
        if rest_size > 0 {
            children.push(TreeNode {
                name: if rest.is_empty() {
                    "Files".into()
                } else {
                    format!("{} smaller items", rest.len())
                },
                path: path.to_string_lossy().into_owned(),
                size: rest_size,
                files: 0,
                denied: false,
                other: true,
                children: Vec::new(),
            });
        }
    }
    TreeNode {
        name: node.name.clone(),
        path: path.to_string_lossy().into_owned(),
        size: node.size,
        files: node.files,
        denied: node.denied,
        other: false,
        children,
    }
}

/// A slice of the last scan's tree, `depth` levels below `path`.
#[tauri::command]
fn get_tree(state: State<'_, AppState>, path: Option<String>, depth: Option<u32>) -> CmdResult<TreeNode> {
    let scan = state.scan.lock().unwrap().clone().ok_or("no scan yet")?;
    let path = path.map_or_else(|| scan.root.clone(), PathBuf::from);
    let node = scan.find(&path).ok_or("folder not in the scan")?;
    let mut dto = to_dto(node, &path, depth.unwrap_or(2).min(4));
    if path == scan.root {
        dto.name = path.to_string_lossy().into_owned();
    }
    Ok(dto)
}

#[tauri::command]
fn get_findings(state: State<'_, AppState>) -> Vec<Finding> {
    state.findings.lock().unwrap().clone()
}

#[tauri::command]
async fn create_plan(state: State<'_, AppState>, selection: Vec<Selection>, options: CleanOptions) -> CmdResult<Plan> {
    let home = state.home.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clean::plan(&selection, RuleSet::builtin(), &home, &RealSystem, &options)
    })
    .await
    .map_err(err)?;
    state.plans.lock().unwrap().insert(plan.id.clone(), plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn execute_plan(state: State<'_, AppState>, plan_id: String, dry_run: bool) -> CmdResult<Report> {
    let plan = state
        .plans
        .lock()
        .unwrap()
        .get(&plan_id)
        .cloned()
        .ok_or("unknown plan; preview again")?;
    let home = state.home.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        let journal = Journal::open(&paths::data_dir(&home)).map_err(err)?;
        clean::execute(&plan, RuleSet::builtin(), &RealSystem, Some(&journal), dry_run).map_err(err)
    })
    .await
    .map_err(err)??;
    if !dry_run {
        // A plan is single-use: anything it touched has changed.
        state.plans.lock().unwrap().remove(&plan_id);
        let cleaned: Vec<&Path> = report
            .outcomes
            .iter()
            .filter(|o| {
                o.trashed_to.is_some() || o.status == clean::Status::Deleted || o.status == clean::Status::Erased
            })
            .map(|o| o.path.as_path())
            .collect();
        let mut found = state.findings.lock().unwrap();
        for f in found.iter_mut() {
            f.items.retain(|i| !cleaned.contains(&i.path.as_path()));
            f.total_bytes = f.items.iter().map(|i| i.bytes).sum();
        }
        found.retain(|f| !f.items.is_empty());
    }
    Ok(report)
}

#[derive(Serialize)]
struct UndoResult {
    restored: Vec<String>,
    failed: Vec<(String, String)>,
}

#[tauri::command]
fn undo_run(state: State<'_, AppState>, run_id: String) -> CmdResult<UndoResult> {
    let journal = Journal::open(&paths::data_dir(&state.home)).map_err(err)?;
    let (restored, failed) = journal.undo(&run_id).map_err(err)?;
    Ok(UndoResult {
        restored: restored.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        failed: failed
            .into_iter()
            .map(|(p, why)| (p.to_string_lossy().into_owned(), why))
            .collect(),
    })
}

#[tauri::command]
async fn list_snapshots() -> CmdResult<SnapshotReport> {
    tauri::async_runtime::spawn_blocking(snapshots::list).await.map_err(err)
}

/// Shows a path in Finder, Files or File Explorer. Only paths inside the home
/// folder.
#[tauri::command]
fn reveal_path(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !paths::is_normalized_absolute(&p) || !p.starts_with(&state.home) {
        return Err("can only reveal paths inside the home folder".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&p)
            .status()
            .map_err(err)?;
    }
    #[cfg(windows)]
    {
        if path.contains('"') {
            return Err("the path contains a quote".into());
        }
        // explorer returns 1 even when it works, so the status is ignored.
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", p.display()))
            .spawn()
            .map_err(err)?;
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        // xdg-open can't select an item, so open the folder that holds it.
        let folder = if p.is_dir() {
            p.as_path()
        } else {
            p.parent().unwrap_or(&p)
        };
        std::process::Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(err)?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let home = paths::home_dir().expect("cannot find the home folder");
    tauri::Builder::default()
        .manage(AppState {
            home,
            scan: Mutex::default(),
            findings: Mutex::default(),
            cancel: Mutex::default(),
            plans: Mutex::default(),
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            disk_info,
            fda_status,
            open_fda_settings,
            start_scan,
            cancel_scan,
            get_tree,
            get_findings,
            create_plan,
            execute_plan,
            undo_run,
            list_snapshots,
            reveal_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenDisk");
}
