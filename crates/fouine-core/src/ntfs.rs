//! Indexation rapide d'un volume NTFS : on lit la table des fichiers au lieu de parcourir les dossiers.
//! Demande les droits administrateur pour ouvrir `\\.\X:`.
//!
//! Deux voies :
//! - `raw` (par defaut) : lecture directe de la table des fichiers, en parallele ; fournit aussi taille et date ;
//! - enumeration du systeme (`FSCTL_ENUM_USN_DATA`, API Windows publique) : plus lente (le noyau l'execute sur un
//!   seul fil), utilisee en secours si la lecture directe echoue. Elle ne fournit pas taille ni date : elles sont
//!   alors lues a la demande (voir `Index::stat`).

mod raw;

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_HANDLE_EOF, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetVolumeInformationW, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_GET_NTFS_VOLUME_DATA, FSCTL_QUERY_USN_JOURNAL,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::exclusions::Exclusions;
use crate::index::{Id, Index, NO_PARENT};

const DRIVE_FIXED: u32 = 3;
const ATTR_DIRECTORY: u32 = 0x10;
const FRN_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const ROOT_FRN: usize = 5;
/// Nombre de plages de numeros d'enregistrement pour l'enumeration du systeme.
const PARTS: u64 = 64;
const ABSENT: u32 = u32::MAX;
/// Taille ou date non fournie par la source (lue a la demande plus tard).
const NO_SIZE: u64 = u64::MAX;
const NO_MTIME: i64 = i64::MIN;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Lettres des volumes fixes formates en NTFS.
pub fn ntfs_volumes() -> Vec<char> {
    ('A'..='Z')
        .filter(|l| {
            let root = wide(&format!("{l}:\\"));
            let mut fs = [0u16; 32];
            // SAFETY: tampons valides pour les longueurs indiquees, pointeurs nuls autorises par l'API.
            unsafe {
                GetDriveTypeW(root.as_ptr()) == DRIVE_FIXED
                    && GetVolumeInformationW(
                        root.as_ptr(),
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        fs.as_mut_ptr(),
                        32,
                    ) != 0
                    && String::from_utf16_lossy(&fs).trim_end_matches('\0') == "NTFS"
            }
        })
        .collect()
}

/// Un enregistrement lu : numero, parent, nom (dans l'arene de son segment), type, taille et date.
#[derive(Clone, Copy)]
struct Rec {
    frn: u32,
    parent: u32,
    off: u32,
    len: u32,
    dir: bool,
    size: u64,
    mtime: i64,
    /// Attributs Windows (cache, systeme...).
    attrs: u32,
}

/// Resultat de la lecture d'un segment.
#[derive(Default)]
struct Part {
    recs: Vec<Rec>,
    /// Noms supplementaires des fichiers a liens physiques multiples (meme numero que l'entree principale).
    links: Vec<Rec>,
    /// Lecture directe seulement : noms trouves dans des enregistrements d'extension (frn = fichier de base).
    ext: Vec<Rec>,
    /// Lecture directe seulement : fichiers de base sans nom residant (type, taille, date).
    orphans: Vec<Rec>,
    arena: String,
}

/// SAFETY : `path` doit etre un chemin de volume termine par un zero.
unsafe fn open_volume(path: &[u16]) -> io::Result<HANDLE> {
    let h = CreateFileW(
        path.as_ptr(),
        GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        std::ptr::null(),
        OPEN_EXISTING,
        0,
        std::ptr::null_mut(),
    );
    if h == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(h)
    }
}

/// Retourne (prochain USN du journal, nombre d'enregistrements de la table des fichiers).
/// SAFETY : `h` doit etre une poignee de volume valide ouverte en lecture.
unsafe fn volume_info(h: HANDLE) -> io::Result<(i64, u64)> {
    let mut journal = [0u8; 80];
    let mut got = 0u32;
    if DeviceIoControl(
        h,
        FSCTL_QUERY_USN_JOURNAL,
        std::ptr::null(),
        0,
        journal.as_mut_ptr().cast(),
        80,
        &mut got,
        std::ptr::null_mut(),
    ) == 0
    {
        return Err(io::Error::last_os_error());
    }
    let next_usn = i64::from_le_bytes(journal[16..24].try_into().unwrap());
    // NTFS_VOLUME_DATA_BUFFER : octets par enregistrement a 48, taille valide de la table a 56.
    let mut vol = [0u8; 512];
    let max_rec = if DeviceIoControl(
        h,
        FSCTL_GET_NTFS_VOLUME_DATA,
        std::ptr::null(),
        0,
        vol.as_mut_ptr().cast(),
        512,
        &mut got,
        std::ptr::null_mut(),
    ) != 0
    {
        let per = u32::from_le_bytes(vol[48..52].try_into().unwrap()) as u64;
        let valid = i64::from_le_bytes(vol[56..64].try_into().unwrap()).max(0) as u64;
        valid.checked_div(per).unwrap_or(0)
    } else {
        0
    };
    Ok((next_usn, if max_rec == 0 { 1 << 24 } else { max_rec }))
}

/// Enumere (API du systeme) les enregistrements dont le numero est dans `[lo, hi[`.
/// SAFETY : `h` doit etre une poignee de volume valide ouverte en lecture.
unsafe fn enumerate_range(h: HANDLE, next_usn: i64, lo: u64, hi: u64) -> io::Result<Part> {
    let mut part = Part::default();
    let mut buf = vec![0u8; 1 << 20];
    let mut units: Vec<u16> = Vec::with_capacity(256);
    let mut start = lo;
    let mut got = 0u32;
    'lecture: loop {
        let mut input = [0u8; 24]; // MFT_ENUM_DATA_V0 : debut, USN bas, USN haut
        input[..8].copy_from_slice(&start.to_le_bytes());
        input[16..24].copy_from_slice(&next_usn.to_le_bytes());
        if DeviceIoControl(
            h,
            FSCTL_ENUM_USN_DATA,
            input.as_ptr().cast(),
            24,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut got,
            std::ptr::null_mut(),
        ) == 0
        {
            if GetLastError() == ERROR_HANDLE_EOF {
                break;
            }
            return Err(io::Error::last_os_error());
        }
        let raw = &buf[..got as usize];
        if raw.len() < 8 {
            break;
        }
        start = u64::from_le_bytes(raw[..8].try_into().unwrap());
        let mut pos = 8;
        while pos + 60 <= raw.len() {
            let len = u32::from_le_bytes(raw[pos..pos + 4].try_into().unwrap()) as usize;
            if len < 60 || pos + len > raw.len() {
                break;
            }
            let frn = u64::from_le_bytes(raw[pos + 8..pos + 16].try_into().unwrap()) & FRN_MASK;
            if frn >= hi {
                break 'lecture; // l'enumeration est croissante : le reste appartient a une autre plage
            }
            let parent = u64::from_le_bytes(raw[pos + 16..pos + 24].try_into().unwrap()) & FRN_MASK;
            let attrs = u32::from_le_bytes(raw[pos + 52..pos + 56].try_into().unwrap());
            let nlen = u16::from_le_bytes(raw[pos + 56..pos + 58].try_into().unwrap()) as usize;
            let noff = u16::from_le_bytes(raw[pos + 58..pos + 60].try_into().unwrap()) as usize;
            if pos + noff + nlen <= raw.len() && frn <= u32::MAX as u64 && parent <= u32::MAX as u64
            {
                units.clear();
                units.extend(
                    raw[pos + noff..pos + noff + nlen]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c)),
                );
                let off = part.arena.len() as u32;
                part.arena.extend(
                    char::decode_utf16(units.iter().copied())
                        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)),
                );
                part.recs.push(Rec {
                    attrs,
                    frn: frn as u32,
                    parent: parent as u32,
                    off,
                    len: part.arena.len() as u32 - off,
                    dir: attrs & ATTR_DIRECTORY != 0,
                    size: NO_SIZE,
                    mtime: NO_MTIME,
                });
            }
            pos += len;
        }
    }
    Ok(part)
}

/// Nombre de fils : `FOUINE_THREADS`, sinon `par_defaut`.
fn thread_count(par_defaut: usize) -> usize {
    std::env::var("FOUINE_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(par_defaut, |n| n.clamp(1, 32))
}

fn cpus() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Enumeration par l'API du systeme : le noyau ne la parallelise pas (mesure : 1 fil ou 8 fils, meme duree),
/// un seul fil suffit donc par defaut.
fn enumerate_usn(path: &[u16]) -> io::Result<Vec<Part>> {
    // SAFETY: appels FFI avec des tampons proprietaires ; chaque poignee est fermee a la sortie de son fil.
    let (next_usn, max_rec) = unsafe {
        let h = open_volume(path)?;
        let r = volume_info(h);
        CloseHandle(h);
        r?
    };
    let span = max_rec.div_ceil(PARTS).max(1);
    let next = AtomicUsize::new(0);
    let parts: Mutex<Vec<Option<Part>>> = Mutex::new((0..PARTS).map(|_| None).collect());
    let failure: Mutex<Option<io::Error>> = Mutex::new(None);
    std::thread::scope(|sc| {
        for _ in 0..thread_count(1) {
            sc.spawn(|| {
                // SAFETY: poignee ouverte et fermee dans ce fil.
                let h = match unsafe { open_volume(path) } {
                    Ok(h) => h,
                    Err(e) => {
                        *failure.lock().unwrap() = Some(e);
                        return;
                    }
                };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed) as u64;
                    if i >= PARTS || failure.lock().unwrap().is_some() {
                        break;
                    }
                    let lo = i * span;
                    let hi = if i == PARTS - 1 {
                        u64::MAX
                    } else {
                        (i + 1) * span
                    };
                    // SAFETY: `h` est une poignee de volume valide.
                    match unsafe { enumerate_range(h, next_usn, lo, hi) } {
                        Ok(p) => parts.lock().unwrap()[i as usize] = Some(p),
                        Err(e) => {
                            *failure.lock().unwrap() = Some(e);
                            break;
                        }
                    }
                }
                // SAFETY: poignee ouverte plus haut dans ce fil.
                unsafe { CloseHandle(h) };
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    Ok(parts
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect())
}

/// Identifiant d'une entree exclue : ses descendants le sont aussi.
const EXCL: Id = Id::MAX - 1;

/// Une entree doit-elle etre exclue ? (`par` : identifiant reel du parent)
fn skip(ix: &Index, par: Id, name: &str, dir: bool, attrs: u32, excl: &Exclusions) -> bool {
    if excl.is_empty() {
        return false;
    }
    if excl.attrs_excluded(attrs) || (!dir && excl.name_excluded(name)) {
        return true;
    }
    if dir && excl.has_folders() && excl.folder_name_candidate(name) {
        let mut p = ix.path(par);
        if !p.ends_with('\\') {
            p.push('\\');
        }
        p.push_str(name);
        return excl.folder_excluded(&p);
    }
    false
}

/// Construit l'index a partir des enregistrements lus (communs aux deux voies) ; retourne le nombre d'entrees.
/// Les exclusions s'appliquent ici : une entree exclue (et tout ce qu'elle contient) n'est pas ajoutee.
fn build(ix: &mut Index, letter: char, parts: &[Part], excl: &Exclusions) -> usize {
    // Tableaux indexes par numero d'enregistrement (pas de table de hachage).
    let total: usize = parts.iter().map(|p| p.recs.len() + p.links.len()).sum();
    let max_frn = parts
        .iter()
        .flat_map(|p| p.recs.iter().map(|r| r.frn))
        .max()
        .unwrap_or(0) as usize;
    let mut parent = vec![ABSENT; max_frn + 1];
    let mut name_part = vec![u32::MAX; max_frn + 1];
    let mut name_off = vec![0u32; max_frn + 1];
    let mut name_len = vec![0u32; max_frn + 1];
    let mut rec_of: Vec<(bool, u64, i64, u32)> = vec![(false, NO_SIZE, NO_MTIME, 0); max_frn + 1];
    for (pi, p) in parts.iter().enumerate() {
        for r in &p.recs {
            let f = r.frn as usize;
            parent[f] = r.parent;
            name_part[f] = pi as u32;
            name_off[f] = r.off;
            name_len[f] = r.len;
            rec_of[f] = (r.dir, r.size, r.mtime, r.attrs);
        }
    }
    let arena_bytes: usize = parts.iter().map(|p| p.arena.len()).sum();
    ix.reserve(total + 1, arena_bytes + 16);

    let root = ix.add(&format!("{letter}:\\"), NO_PARENT, true, Some(0), None);
    let mut ids = vec![Id::MAX; max_frn + 1];
    if ROOT_FRN <= max_frn {
        ids[ROOT_FRN] = root;
    }
    // Les parents ne precedent pas toujours leurs enfants : resolution iterative avec pile explicite.
    let mut added = 1;
    let mut chain: Vec<usize> = Vec::new();
    for start in 0..=max_frn {
        if name_part[start] == u32::MAX || ids[start] != Id::MAX {
            continue;
        }
        chain.clear();
        let mut cur = start;
        while ids[cur] == Id::MAX {
            if name_part[cur] == u32::MAX || chain.len() >= 4096 {
                break; // parent inconnu ou boucle : rattache a la racine
            }
            chain.push(cur);
            let p = parent[cur];
            if p == ABSENT || p as usize > max_frn {
                break;
            }
            cur = p as usize;
        }
        let mut par = if ids[cur] != Id::MAX { ids[cur] } else { root };
        for &f in chain.iter().rev() {
            let a = &parts[name_part[f] as usize].arena;
            let name = &a[name_off[f] as usize..(name_off[f] + name_len[f]) as usize];
            let (dir, size, mtime, attrs) = rec_of[f];
            if par == EXCL || skip(ix, par, name, dir, attrs, excl) {
                ids[f] = EXCL;
                par = EXCL;
                continue;
            }
            let size = if dir {
                Some(0)
            } else {
                (size != NO_SIZE).then_some(size)
            };
            let mtime = (mtime != NO_MTIME).then_some(mtime);
            let id = ix.add(name, par, dir, size, mtime);
            ids[f] = id;
            par = id;
            added += 1;
        }
    }
    // Liens physiques supplementaires : un fichier apparait sous chacun de ses noms.
    for (pi, p) in parts.iter().enumerate() {
        for l in &p.links {
            let par = match ids.get(l.parent as usize) {
                Some(&id) if id == EXCL => continue,
                Some(&id) if id != Id::MAX => id,
                _ => root,
            };
            let a = &parts[pi].arena;
            let name = &a[l.off as usize..(l.off + l.len) as usize];
            if skip(ix, par, name, false, l.attrs, excl) {
                continue;
            }
            let size = (l.size != NO_SIZE).then_some(l.size);
            let mtime = (l.mtime != NO_MTIME).then_some(l.mtime);
            ix.add(name, par, false, size, mtime);
            added += 1;
        }
    }
    added
}

fn profil(nom: &str, t0: std::time::Instant, t_lecture: std::time::Duration, n: usize) {
    if std::env::var_os("FOUINE_PROFIL").is_some() {
        eprintln!(
            "profil NTFS ({nom}) : lecture {:.2} s, construction {:.2} s ({n} entrees)",
            t_lecture.as_secs_f64(),
            (t0.elapsed() - t_lecture).as_secs_f64(),
        );
    }
}

/// Contenu lu d'un volume, pret a etre ajoute a un index (`add_volume`). Permet de lire plusieurs volumes en
/// parallele (un disque chacun) puis de les ajouter l'un apres l'autre.
pub struct VolumeData {
    letter: char,
    parts: Vec<Part>,
}

/// Lit un volume NTFS (lecture directe de la table des fichiers, repli sur l'enumeration du systeme).
/// N'ecrit dans aucun index : un echec ne laisse rien derriere lui.
pub fn read_volume(letter: char) -> io::Result<VolumeData> {
    let path = wide(&format!("\\\\.\\{letter}:"));
    let threads = thread_count(cpus().clamp(2, 8));
    let parts = match raw::enumerate_raw(&path, threads) {
        Ok(p) => p,
        Err(e) => {
            if std::env::var_os("FOUINE_PROFIL").is_some() {
                eprintln!("lecture directe impossible ({e}) : enumeration du systeme");
            }
            enumerate_usn(&path)?
        }
    };
    Ok(VolumeData { letter, parts })
}

/// Ajoute a l'index un volume lu par `read_volume` ; retourne le nombre d'entrees ajoutees.
pub fn add_volume(ix: &mut Index, data: &VolumeData, excl: &Exclusions) -> usize {
    build(ix, data.letter, &data.parts, excl)
}

/// Lecture directe de la table des fichiers (rapide ; fournit aussi taille et date).
pub fn scan_ntfs_raw(ix: &mut Index, letter: char) -> io::Result<usize> {
    let t0 = std::time::Instant::now();
    let path = wide(&format!("\\\\.\\{letter}:"));
    let parts = raw::enumerate_raw(&path, thread_count(cpus().clamp(2, 8)))?;
    let t_lecture = t0.elapsed();
    let n = build(ix, letter, &parts, &Exclusions::default());
    profil("directe", t0, t_lecture, n);
    Ok(n)
}

/// Enumeration par l'API du systeme (secours).
pub fn scan_ntfs_usn(ix: &mut Index, letter: char) -> io::Result<usize> {
    let t0 = std::time::Instant::now();
    let path = wide(&format!("\\\\.\\{letter}:"));
    let parts = enumerate_usn(&path)?;
    let t_lecture = t0.elapsed();
    let n = build(ix, letter, &parts, &Exclusions::default());
    profil("systeme", t0, t_lecture, n);
    Ok(n)
}

/// Enumere tout le volume `letter` et l'ajoute a `ix` ; retourne le nombre d'entrees ajoutees.
/// Lecture directe de la table des fichiers, avec repli sur l'enumeration du systeme si elle echoue.
pub fn scan_ntfs(ix: &mut Index, letter: char) -> io::Result<usize> {
    match scan_ntfs_raw(ix, letter) {
        Ok(n) => Ok(n),
        Err(e) => {
            if std::env::var_os("FOUINE_PROFIL").is_some() {
                eprintln!("lecture directe impossible ({e}) : enumeration du systeme");
            }
            scan_ntfs_usn(ix, letter)
        }
    }
}
