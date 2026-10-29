//! Volumes (disques) de la machine : lettre, type, systeme de fichiers, taille. Sert a la fenetre
//! « Volumes et dossiers » et a l'inclusion automatique des nouveaux volumes (comme Everything).
//! Windows seulement pour l'instant ; ailleurs la liste est vide.

#[derive(Debug, Clone)]
pub struct VolumeInfo {
    /// Racine, par exemple `C:\`.
    pub root: String,
    pub letter: char,
    pub label: String,
    /// `NTFS`, `FAT32`, `exFAT`, `ReFS`...
    pub filesystem: String,
    pub fixed: bool,
    pub removable: bool,
    pub size: u64,
}

impl VolumeInfo {
    /// La table des fichiers de ce volume est-elle lisible directement (lecture rapide) ?
    pub fn fast_read(&self) -> bool {
        self.fixed && self.filesystem == "NTFS"
    }
}

#[cfg(windows)]
pub fn list() -> Vec<VolumeInfo> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let mut out = Vec::new();
    // SAFETY: appels FFI avec des tampons proprietaires de longueur indiquee ; pointeurs nuls autorises par l'API.
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
            let kind = GetDriveTypeW(wide.as_ptr());
            if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
                continue; // lecteurs reseau, optiques, disques en memoire : non proposes
            }
            let (mut label, mut fs) = ([0u16; 64], [0u16; 32]);
            if GetVolumeInformationW(
                wide.as_ptr(),
                label.as_mut_ptr(),
                64,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                32,
            ) == 0
            {
                continue; // pas de media ou volume illisible
            }
            let text = |b: &[u16]| {
                String::from_utf16_lossy(b)
                    .trim_end_matches('\0')
                    .to_string()
            };
            let mut total = 0u64;
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                &mut total,
                std::ptr::null_mut(),
            );
            out.push(VolumeInfo {
                root,
                letter,
                label: text(&label),
                filesystem: text(&fs),
                fixed: kind == DRIVE_FIXED,
                removable: kind == DRIVE_REMOVABLE,
                size: total,
            });
        }
    }
    out
}

#[cfg(not(windows))]
pub fn list() -> Vec<VolumeInfo> {
    Vec::new()
}
