//! Windows specifics: Recycle Bin, volumes, processes and known folders.

use std::collections::HashSet;
use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows::Win32::UI::Shell::{
    FOLDERID_Profile, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, SHGetKnownFolderPath,
    FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, KF_FLAG_DEFAULT,
};

use crate::system::recycle::{self, parse_info};
use crate::system::{TrashCapacity, TrashOutcome};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide_buf(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ------------------------------------------------------------------ volumes

fn volume_path(path: &Path) -> Option<String> {
    let name = wide(&path.to_string_lossy());
    let mut buf = [0u16; MAX_PATH as usize];
    // SAFETY: `name` is NUL-terminated and `buf` is writable.
    unsafe { GetVolumePathNameW(PCWSTR::from_raw(name.as_ptr()), &mut buf).ok()? };
    let out = from_wide_buf(&buf);
    (!out.is_empty()).then_some(out)
}

fn volume_guid(volume: &str) -> Option<String> {
    let name = wide(volume);
    let mut buf = [0u16; MAX_PATH as usize];
    // SAFETY: `name` is NUL-terminated and `buf` is writable.
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR::from_raw(name.as_ptr()), &mut buf).ok()? };
    let out = from_wide_buf(&buf);
    (!out.is_empty()).then_some(out)
}

pub fn available_bytes(path: &Path) -> io::Result<u64> {
    let name = wide(&path.to_string_lossy());
    let mut free_to_caller = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    // SAFETY: the path is NUL-terminated and the out pointers are writable.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR::from_raw(name.as_ptr()),
            Some(&mut free_to_caller),
            Some(&mut total),
            Some(&mut total_free),
        )
        .map_err(io::Error::from)?;
    }
    Ok(free_to_caller)
}

pub fn total_bytes(path: &Path) -> io::Result<u64> {
    let name = wide(&path.to_string_lossy());
    let mut free_to_caller = 0u64;
    let mut total = 0u64;
    let mut total_free = 0u64;
    // SAFETY: as in `available_bytes`.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR::from_raw(name.as_ptr()),
            Some(&mut free_to_caller),
            Some(&mut total),
            Some(&mut total_free),
        )
        .map_err(io::Error::from)?;
    }
    Ok(total)
}

/// Per-item Recycle Bin limit for the volume holding `path`.
pub fn trash_capacity(path: &Path) -> TrashCapacity {
    let Some(volume) = volume_path(path) else {
        return TrashCapacity::Unknown("cannot find the drive".into());
    };
    let drive = wide(&volume);
    // SAFETY: `drive` is NUL-terminated.
    let kind = unsafe { GetDriveTypeW(PCWSTR::from_raw(drive.as_ptr())) };
    if kind != DRIVE_FIXED {
        return TrashCapacity::Disabled;
    }
    let Some(guid) = volume_guid(&volume) else {
        return TrashCapacity::Unknown("cannot identify the drive".into());
    };
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume\{guid}");
    let max_mb = reg_dword(&key, "MaxCapacity");
    let nuke = reg_dword(&key, "NukeOnDelete");
    if nuke == Some(1) || max_mb.is_some() {
        return recycle::capacity_from_registry(max_mb, nuke, 0);
    }
    match total_bytes(path) {
        Ok(total) if total > 0 => recycle::capacity_from_registry(None, None, total),
        Ok(_) => TrashCapacity::Unknown("the drive reports no size".into()),
        Err(e) => TrashCapacity::Unknown(e.to_string()),
    }
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    let key = wide(key);
    let value = wide(value);
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and `data`/`size` are writable.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(key.as_ptr()),
            PCWSTR::from_raw(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

// ------------------------------------------------------------- Recycle Bin

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: every successful CoInitializeEx is paired with one call.
        unsafe { CoUninitialize() };
    }
}

/// COM objects must live in an STA, and the caller may already be in an MTA
/// (async runtimes, rayon), so every recycle gets its own short-lived thread.
pub fn trash_impl(path: &Path) -> io::Result<TrashOutcome> {
    let path = path.to_path_buf();
    std::thread::spawn(move || trash_on_thread(&path))
        .join()
        .map_err(|_| io::Error::other("the Recycle Bin call panicked"))?
}

fn trash_on_thread(path: &Path) -> io::Result<TrashOutcome> {
    // SAFETY: `CoInitializeEx` has no preconditions; a failed call means COM
    // is already initialised in an incompatible mode, which we report.
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    if hr.is_err() {
        return Err(io::Error::other("cannot start COM for the Recycle Bin"));
    }
    let _com = ComGuard;

    let text = HSTRING::from(path.to_string_lossy().as_ref());
    // SAFETY: the shell objects are created, used and dropped on this thread.
    unsafe {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        op.SetOperationFlags(
            FOF_ALLOWUNDO
                | FOFX_RECYCLEONDELETE
                | FOF_NOCONFIRMATION
                | FOF_WANTNUKEWARNING
                | FOF_SILENT
                | FOF_NOERRORUI
                | FOFX_EARLYFAILURE,
        )?;
        let item: IShellItem = SHCreateItemFromParsingName(&text, None)?;
        op.DeleteItem(Some(&item), None)?;
        op.PerformOperations()?;
        if op.GetAnyOperationsAborted()?.as_bool() {
            return Err(io::Error::other("the operation was cancelled"));
        }
    }
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(io::Error::other("the item is still there"));
    }
    Ok(find_recycled(path))
}

fn current_sid() -> Option<String> {
    let mut token = HANDLE::default();
    // SAFETY: the token handle is closed below and the buffers are sized.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let _guard = Token(token);
        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), size, &mut size).ok()?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const c_void));
        sid
    }
}

struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn recycle_dir(path: &Path) -> Option<PathBuf> {
    let volume = volume_path(path)?;
    let sid = current_sid()?;
    Some(Path::new(&volume).join("$Recycle.Bin").join(sid))
}

/// Finds the `$R` that holds `original` after the shell moved it. If the bin
/// cannot be read the item is still considered recycled (we just do not know
/// its name). If it can be read and no record matches, Windows deleted it.
fn find_recycled(original: &Path) -> TrashOutcome {
    let Some(dir) = recycle_dir(original) else {
        return TrashOutcome::Moved(None);
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return TrashOutcome::Moved(None);
    };
    let text = original.to_string_lossy();
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("$I") {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Some(info) = parse_info(&bytes) else { continue };
        if !recycle::same_original(&info.original, &text) {
            continue;
        }
        if best.as_ref().is_none_or(|(at, _)| info.deleted_filetime >= *at) {
            let r_name = format!("$R{}", &name[2..]);
            best = Some((info.deleted_filetime, dir.join(r_name)));
        }
    }
    match best {
        Some((_, path)) => TrashOutcome::Moved(Some(path)),
        None => TrashOutcome::Erased,
    }
}

/// Puts an item back, checking that the `$I` record still describes it.
pub fn restore_trashed(from: &Path, to: &Path) -> io::Result<()> {
    let name = from
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the Trash entry has no name"))?;
    let Some(info_name) = recycle::sibling_info_name(name) else {
        return std::fs::rename(from, to);
    };
    let info = from.with_file_name(info_name);
    if let Ok(bytes) = std::fs::read(&info) {
        if let Some(parsed) = parse_info(&bytes) {
            if !recycle::same_original(&parsed.original, &to.to_string_lossy()) {
                return Err(io::Error::other("the Recycle Bin record belongs to another item"));
            }
        }
    }
    std::fs::rename(from, to)?;
    let _ = std::fs::remove_file(info);
    Ok(())
}

pub fn forget_trash_info(trashed: &Path) {
    let Some(name) = trashed.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    if let Some(info) = recycle::sibling_info_name(name) {
        let _ = std::fs::remove_file(trashed.with_file_name(info));
    }
}

// --------------------------------------------------------------- processes

/// Running process names, lowercase and without `.exe`.
pub fn running_processes() -> HashSet<String> {
    let mut out = HashSet::new();
    // SAFETY: the snapshot is closed below and `entry` is initialised.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let _guard = Snapshot(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_err() {
            return out;
        }
        loop {
            let end = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_lowercase();
            let name = name.strip_suffix(".exe").unwrap_or(&name).to_string();
            if !name.is_empty() {
                out.insert(name);
            }
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }
    }
    out
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateToolhelp32Snapshot.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// ----------------------------------------------------------- known folders

pub fn profile_dir() -> Option<PathBuf> {
    // SAFETY: the returned string is freed and checked before use.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Profile, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const c_void));
        text.map(PathBuf::from)
    }
}
