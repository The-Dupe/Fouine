//! Actions propres au systeme : ouvrir, afficher dans le gestionnaire de fichiers, mise en forme.

use std::process::Command;

pub fn open(path: &str) {
    #[cfg(windows)]
    let _ = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(path).spawn();
}

pub fn reveal(path: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("explorer")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").args(["-R", path]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = Command::new("xdg-open").arg(dir).spawn();
    }
}

pub fn human_size(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["o", "Ko", "Mo", "Go", "To"] {
        if v < 1024.0 || unit == "To" {
            return if unit == "o" {
                format!("{n} o")
            } else {
                format!("{v:.1} {unit}")
            };
        }
        v /= 1024.0;
    }
    unreachable!()
}

/// Vrai si le processus s'execute avec les droits d'administrateur (jeton eleve).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && e.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relance Fouine avec les droits d'administrateur (invite UAC). Vrai si la demande est partie ;
/// l'appelant quitte alors, la nouvelle instance attendant la fin de celle-ci (`--relance`).
#[cfg(windows)]
pub fn relaunch_elevated() -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let (verb, file, args) = (
        wide("runas"),
        wide(&exe.to_string_lossy()),
        wide("--relance"),
    );
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            1,
        )
    };
    r as usize > 32
}

#[cfg(not(windows))]
pub fn relaunch_elevated() -> bool {
    false
}

/// Ouvre une adresse (https:// ou mailto:) avec l'application par defaut du systeme.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
        let (verb, target) = (wide("open"), wide(url));
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            );
        }
    }
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

#[cfg(windows)]
fn wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ouvre le dossier qui contient `path` (sans selectionner le fichier).
pub fn open_folder(path: &str) {
    if let Some(dir) = std::path::Path::new(path).parent() {
        open(&dir.to_string_lossy());
    }
}

/// Place des fichiers dans le presse-papiers comme l'Explorateur (copier ou couper) : ils peuvent ensuite
/// etre colles dans l'Explorateur ou dans Fouine.
#[cfg(windows)]
pub fn clipboard_files(paths: &[String], cut: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
    };
    const CF_HDROP: u32 = 15;
    // DROPFILES (20 octets : decalage, point, fNC, fWide) puis les chemins en UTF-16, un NUL entre chaque, deux a la fin.
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&20u32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 8]);
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes());
    for p in paths {
        for u in p.encode_utf16() {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf.extend_from_slice(&[0, 0]);
    }
    buf.extend_from_slice(&[0, 0]);
    let effect_bytes = (if cut { 2u32 } else { 1u32 }).to_le_bytes();
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("presse-papiers indisponible".into());
        }
        EmptyClipboard();
        let set = |format: u32, data: &[u8]| -> bool {
            let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data.len());
            if h.is_null() {
                return false;
            }
            let p = GlobalLock(h);
            if p.is_null() {
                GlobalFree(h);
                return false;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), p.cast::<u8>(), data.len());
            GlobalUnlock(h);
            if SetClipboardData(format, h).is_null() {
                GlobalFree(h);
                return false;
            }
            true
        };
        let effect = RegisterClipboardFormatW(wide_nul("Preferred DropEffect").as_ptr());
        let ok = set(CF_HDROP, &buf) && set(effect, &effect_bytes);
        CloseClipboard();
        if ok {
            Ok(())
        } else {
            Err("ecriture dans le presse-papiers impossible".into())
        }
    }
}

#[cfg(not(windows))]
pub fn clipboard_files(_paths: &[String], _cut: bool) -> Result<(), String> {
    Err("copie de fichiers non prise en charge sur ce systeme".into())
}

/// Vrai si le presse-papiers contient des fichiers (copies ou coupes).
#[cfg(windows)]
pub fn clipboard_has_files() -> bool {
    use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
    unsafe { IsClipboardFormatAvailable(15) != 0 }
}

#[cfg(not(windows))]
pub fn clipboard_has_files() -> bool {
    false
}

/// Colle dans `dir` les fichiers du presse-papiers (copie ou deplacement selon copier / couper).
/// Windows affiche ses propres fenetres de progression et de conflit. Retourne le nombre d'elements.
#[cfg(windows)]
pub fn paste_files_into(dir: &str) -> Result<usize, String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::UI::Shell::{
        DragQueryFileW, SHFileOperationW, FO_COPY, FO_MOVE, SHFILEOPSTRUCTW,
    };
    const CF_HDROP: u32 = 15;
    let mut paths: Vec<Vec<u16>> = Vec::new();
    let mut cut = false;
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP) == 0 {
            return Err("aucun fichier dans le presse-papiers".into());
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("presse-papiers indisponible".into());
        }
        let h = GetClipboardData(CF_HDROP);
        if !h.is_null() {
            let n = DragQueryFileW(h, u32::MAX, std::ptr::null_mut(), 0);
            for i in 0..n {
                let len = DragQueryFileW(h, i, std::ptr::null_mut(), 0) as usize;
                let mut b = vec![0u16; len + 1];
                DragQueryFileW(h, i, b.as_mut_ptr(), (len + 1) as u32);
                b.truncate(len);
                paths.push(b);
            }
        }
        let effect = RegisterClipboardFormatW(wide_nul("Preferred DropEffect").as_ptr());
        let he = GetClipboardData(effect);
        if !he.is_null() {
            let p = GlobalLock(he);
            if !p.is_null() {
                cut = (*(p as *const u32) & 2) != 0;
                GlobalUnlock(he);
            }
        }
        CloseClipboard();
    }
    if paths.is_empty() {
        return Err("aucun fichier dans le presse-papiers".into());
    }
    let mut from: Vec<u16> = Vec::new();
    for p in &paths {
        from.extend_from_slice(p);
        from.push(0);
    }
    from.push(0);
    let mut to: Vec<u16> = dir.encode_utf16().collect();
    to.extend_from_slice(&[0, 0]);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = std::mem::zeroed();
        op.wFunc = if cut { FO_MOVE } else { FO_COPY };
        op.pFrom = from.as_ptr();
        op.pTo = to.as_ptr();
        let r = SHFileOperationW(&mut op);
        if r != 0 || op.fAnyOperationsAborted != 0 {
            return Err("operation annulee ou echouee".into());
        }
        if cut && OpenClipboard(std::ptr::null_mut()) != 0 {
            EmptyClipboard(); // comme l'Explorateur : un deplacement ne se colle qu'une fois
            CloseClipboard();
        }
    }
    Ok(paths.len())
}

#[cfg(not(windows))]
pub fn paste_files_into(_dir: &str) -> Result<usize, String> {
    Err("collage de fichiers non pris en charge sur ce systeme".into())
}

/// Envoie un fichier ou un dossier a la corbeille (Windows demande confirmation).
#[cfg(windows)]
pub fn recycle(paths: &[String]) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FO_DELETE, SHFILEOPSTRUCTW,
    };
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.encode_utf16());
        from.push(0);
    }
    from.push(0);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = std::mem::zeroed();
        op.wFunc = FO_DELETE;
        op.pFrom = from.as_ptr();
        op.fFlags = FOF_ALLOWUNDO as _;
        let r = SHFileOperationW(&mut op);
        if r != 0 || op.fAnyOperationsAborted != 0 {
            return Err("suppression annulee ou echouee".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn recycle(_paths: &[String]) -> Result<(), String> {
    Err("suppression non prise en charge sur ce systeme".into())
}

/// Vrai une seule fois a l'appui d'une touche (code touche Windows `vk`), tant qu'elle reste enfoncee.
/// Sert pour Ctrl+C / Ctrl+X / Ctrl+V : quand le presse-papiers ne contient que des fichiers, la couche
/// fenetre n'emet aucun evenement clavier pour Ctrl+V.
#[cfg(windows)]
pub fn key_edge(vk: i32) -> bool {
    use std::sync::Mutex;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    static DOWN: Mutex<Vec<i32>> = Mutex::new(Vec::new());
    let now = unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0;
    let Ok(mut down) = DOWN.lock() else {
        return false;
    };
    let was = down.contains(&vk);
    if now && !was {
        down.push(vk);
        true
    } else {
        if !now {
            down.retain(|&k| k != vk);
        }
        false
    }
}

#[cfg(not(windows))]
pub fn key_edge(_vk: i32) -> bool {
    false
}
