//! Lecture directe de la table des fichiers NTFS ($MFT) : on lit les enregistrements bruts du volume, en
//! parallele, et on en extrait nom, parent, type, taille et date de modification. Bien plus rapide que
//! `FSCTL_ENUM_USN_DATA`, que le noyau execute sur un seul fil.
//!
//! Format (documentation publique de NTFS) : chaque enregistrement fait `BytesPerFileRecordSegment` octets,
//! commence par « FILE », et ses secteurs de 512 octets se terminent par un marqueur a remplacer
//! (« update sequence array »). Les attributs suivent : $STANDARD_INFORMATION (0x10, dates), $FILE_NAME (0x30,
//! parent et nom), $DATA (0x80, taille). Un fichier qui a trop d'attributs pour un seul enregistrement (surtout
//! les fichiers a nombreux liens physiques) en utilise d'autres, dits « d'extension », qui indiquent leur
//! fichier de base : comme toute la table est lue en sequence, on les recupere au passage et on les rattache
//! ensuite en memoire, sans lecture supplementaire.
//! Chaque nom d'un fichier a liens physiques multiples est indexe (hors noms courts DOS).
//!
//! Les droits d'administrateur sont requis pour lire le volume. Tout echec (format inattendu, extents
//! incomplets) renvoie une erreur : l'appelant se rabat alors sur l'enumeration du systeme.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, SetFilePointerEx, FILE_BEGIN};
use windows_sys::Win32::System::Ioctl::FSCTL_GET_NTFS_VOLUME_DATA;
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::{open_volume, Part, Rec, FRN_MASK, NO_MTIME, NO_SIZE};

/// Taille d'une lecture : 8 Mo, multiple de toute taille de cluster usuelle.
const PIECE: u64 = 8 * 1024 * 1024;
const EPOCH_DELTA: i64 = 11_644_473_600;

fn u16at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}
fn u32at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Verifie la signature et retablit les octets de fin de secteur ; faux si l'enregistrement est invalide.
fn apply_fixups(rec: &mut [u8]) -> bool {
    if rec.len() < 48 || &rec[..4] != b"FILE" {
        return false;
    }
    let (usa_off, usa_cnt) = (u16at(rec, 4), u16at(rec, 6));
    if usa_cnt < 2 || usa_off + usa_cnt * 2 > rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_cnt {
        let end = i * 512;
        if end > rec.len() || rec[end - 2..end] != usn {
            return false;
        }
        rec[end - 2] = rec[usa_off + 2 * i];
        rec[end - 1] = rec[usa_off + 2 * i + 1];
    }
    true
}

/// Un nom de fichier : parent et plage du nom UTF-16 dans l'enregistrement (octets).
#[derive(Clone, Copy)]
struct Name {
    parent: u32,
    start: usize,
    end: usize,
}

/// Parcourt les attributs d'un enregistrement (deja corrige) ; `f(type, debut, longueur, non_residente, longueur_du_nom)`.
fn for_each_attr(rec: &[u8], mut f: impl FnMut(u32, usize, usize, bool, u8)) {
    let mut attr = u16at(rec, 0x14);
    while attr + 16 <= rec.len() {
        let t = u32at(rec, attr);
        if t == 0xFFFF_FFFF {
            break;
        }
        let len = u32at(rec, attr + 4) as usize;
        if len < 16 || attr + len > rec.len() {
            break;
        }
        f(t, attr, len, rec[attr + 8] != 0, rec[attr + 9]);
        attr += len;
    }
}

/// Noms ($FILE_NAME) residents d'un enregistrement, hors noms courts DOS (espace de noms 2).
fn collect_names(rec: &[u8], out: &mut Vec<Name>) {
    for_each_attr(rec, |t, attr, _len, nonres, _nl| {
        if t != 0x30 || nonres {
            return;
        }
        let c = attr + u16at(rec, attr + 0x14);
        if c + 66 > rec.len() {
            return;
        }
        let (nl, ns) = (rec[c + 64] as usize, rec[c + 65]);
        let parent = u64at(rec, c) & FRN_MASK;
        if ns != 2 && c + 66 + nl * 2 <= rec.len() && parent <= u32::MAX as u64 {
            out.push(Name {
                parent: parent as u32,
                start: c + 66,
                end: c + 66 + nl * 2,
            });
        }
    });
}

/// Nature d'un enregistrement lu.
enum Parsed {
    /// Fichier ou dossier (enregistrement de base) avec ses noms residents (peut etre vide : noms en extension).
    Base {
        dir: bool,
        names: Vec<Name>,
        size: u64,
        mtime: i64,
        attrs: u32,
    },
    /// Enregistrement d'extension : noms supplementaires du fichier de base `base`.
    Ext { base: u32, names: Vec<Name> },
}

/// Analyse un enregistrement deja corrige. `None` : enregistrement inutilise.
fn parse_record(rec: &[u8]) -> Option<Parsed> {
    let flags = u16at(rec, 0x16);
    if flags & 1 == 0 {
        return None;
    }
    let base = u64at(rec, 0x20) & FRN_MASK;
    let mut names = Vec::new();
    collect_names(rec, &mut names);
    if base != 0 {
        return if base <= u32::MAX as u64 && !names.is_empty() {
            Some(Parsed::Ext {
                base: base as u32,
                names,
            })
        } else {
            None
        };
    }
    let (mut size, mut mtime, mut attrs) = (NO_SIZE, NO_MTIME, 0u32);
    for_each_attr(rec, |t, attr, len, nonres, name_len| match t {
        0x10 if !nonres => {
            let c = attr + u16at(rec, attr + 0x14);
            if c + 16 <= rec.len() {
                mtime = u64at(rec, c + 8) as i64 / 10_000_000 - EPOCH_DELTA;
            }
            if c + 36 <= rec.len() {
                attrs = u32at(rec, c + 32);
            }
        }
        0x80 if name_len == 0 => {
            size = if !nonres {
                u32at(rec, attr + 0x10) as u64
            } else if len >= 0x38 {
                u64at(rec, attr + 0x30)
            } else {
                NO_SIZE
            };
        }
        _ => {}
    });
    Some(Parsed::Base {
        dir: flags & 2 != 0,
        names,
        size,
        mtime,
        attrs,
    })
}

/// Segments de la table des fichiers sur le disque : (octet de depart, longueur, numero du premier enregistrement).
type Piece = (u64, u64, u64);

/// Lit `len` octets a `off` du volume.
/// SAFETY : `h` doit etre une poignee de volume valide ; `off` et `len` alignes sur le secteur.
unsafe fn read_at(h: HANDLE, off: u64, buf: &mut [u8], len: usize) -> io::Result<()> {
    if SetFilePointerEx(h, off as i64, std::ptr::null_mut(), FILE_BEGIN) == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut got = 0u32;
    if ReadFile(
        h,
        buf.as_mut_ptr().cast(),
        len as u32,
        &mut got,
        std::ptr::null_mut(),
    ) == 0
    {
        return Err(io::Error::last_os_error());
    }
    if got as usize != len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "lecture du volume incomplete",
        ));
    }
    Ok(())
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Extents (numero de cluster logique, nombre de clusters) d'un attribut non residant ; `p` est le debut de la
/// liste de couples, `end` la fin de l'attribut. Les extents creux ne sont pas pris en charge.
fn decode_runs(rec: &[u8], mut p: usize, end: usize) -> io::Result<Vec<(i64, u64)>> {
    let (mut lcn, mut runs) = (0i64, Vec::new());
    while p < end && rec[p] != 0 {
        let (ls, os) = ((rec[p] & 0xF) as usize, (rec[p] >> 4) as usize);
        if ls == 0 || os == 0 || p + 1 + ls + os > end {
            return Err(bad("extent non pris en charge"));
        }
        let mut length = 0u64;
        for i in 0..ls {
            length |= (rec[p + 1 + i] as u64) << (8 * i);
        }
        let mut delta = 0i64;
        for i in 0..os {
            delta |= (rec[p + 1 + ls + i] as i64) << (8 * i);
        }
        if rec[p + ls + os] & 0x80 != 0 {
            delta -= 1i64 << (8 * os); // extension du signe
        }
        lcn += delta;
        p += 1 + ls + os;
        runs.push((lcn, length));
    }
    Ok(runs)
}

/// Segments de la table des fichiers, lus dans la liste d'extents de l'enregistrement 0 ($MFT).
fn mft_pieces(rec0: &[u8], bpc: u64, bpr: u64) -> io::Result<Vec<Piece>> {
    let mut found = None;
    for_each_attr(rec0, |t, attr, len, nonres, name_len| {
        // $DATA sans nom, non residente
        if t == 0x80 && name_len == 0 && nonres && found.is_none() {
            found = Some((attr, len));
        }
    });
    let (attr, len) =
        found.ok_or_else(|| bad("attribut $DATA de la table des fichiers introuvable"))?;
    let valid = u64at(rec0, attr + 0x38) / bpr * bpr; // taille initialisee, en enregistrements entiers
    let runs = decode_runs(rec0, attr + u16at(rec0, attr + 0x20), attr + len)?;
    let (mut stream, mut pieces) = (0u64, Vec::new());
    for (lcn, length) in runs {
        if stream >= valid {
            break;
        }
        let mut run_off = lcn as u64 * bpc;
        let mut run_len = (length * bpc).min(valid - stream);
        while run_len > 0 {
            let n = run_len.min(PIECE);
            if !n.is_multiple_of(bpr) || !run_off.is_multiple_of(512) {
                return Err(bad("alignement de la table des fichiers inattendu"));
            }
            pieces.push((run_off, n, stream / bpr));
            run_off += n;
            run_len -= n;
            stream += n;
        }
    }
    if pieces.is_empty() {
        return Err(bad("table des fichiers vide"));
    }
    if stream < valid {
        // La liste d'extents continue dans des enregistrements d'extension : non lue ici (repli sur le systeme).
        return Err(bad("table des fichiers fragmentee : extents incomplets"));
    }
    Ok(pieces)
}

/// Ajoute un nom a l'arene d'une plage.
fn push_name(part: &mut Part, units: &mut Vec<u16>, rec: &[u8], n: Name) -> (u32, u32) {
    units.clear();
    units.extend(
        rec[n.start..n.end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c)),
    );
    let off = part.arena.len() as u32;
    part.arena.extend(
        char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)),
    );
    (off, part.arena.len() as u32 - off)
}

/// Lit toute la table des fichiers du volume : resultats par segment, dans l'ordre.
pub(super) fn enumerate_raw(path: &[u16], threads: usize) -> io::Result<Vec<Part>> {
    // SAFETY: appels FFI avec des tampons proprietaires ; chaque poignee est fermee a la sortie de son fil.
    let (pieces, bpr) = unsafe {
        let h = open_volume(path)?;
        let res = (|| -> io::Result<(Vec<Piece>, u64)> {
            let mut vol = [0u8; 512];
            let mut got = 0u32;
            if DeviceIoControl(
                h,
                FSCTL_GET_NTFS_VOLUME_DATA,
                std::ptr::null(),
                0,
                vol.as_mut_ptr().cast(),
                512,
                &mut got,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let bps = u32at(&vol, 40) as u64;
            let bpc = u32at(&vol, 44) as u64;
            let bpr = u32at(&vol, 48) as u64;
            let mft_lcn = u64at(&vol, 64);
            if bps == 0 || bpc == 0 || bpr < 1024 || !bpr.is_multiple_of(bps) {
                return Err(bad("geometrie NTFS inattendue"));
            }
            let mut buf = vec![0u8; bpr as usize];
            read_at(h, mft_lcn * bpc, &mut buf, bpr as usize)?;
            if !apply_fixups(&mut buf) {
                return Err(bad("enregistrement $MFT invalide"));
            }
            Ok((mft_pieces(&buf, bpc, bpr)?, bpr))
        })();
        CloseHandle(h);
        res?
    };

    // Lecture parallele de tous les segments.
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Part>>> = Mutex::new((0..pieces.len()).map(|_| None).collect());
    let failure: Mutex<Option<io::Error>> = Mutex::new(None);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| {
                // SAFETY: poignee ouverte et fermee dans ce fil.
                let h = match unsafe { open_volume(path) } {
                    Ok(h) => h,
                    Err(e) => {
                        *failure.lock().unwrap() = Some(e);
                        return;
                    }
                };
                let mut buf = vec![0u8; PIECE as usize];
                let mut units: Vec<u16> = Vec::with_capacity(256);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= pieces.len() || failure.lock().unwrap().is_some() {
                        break;
                    }
                    let (off, len, first) = pieces[i];
                    // SAFETY: `h` est une poignee de volume valide ; `off` et `len` sont alignes.
                    if let Err(e) = unsafe { read_at(h, off, &mut buf, len as usize) } {
                        *failure.lock().unwrap() = Some(e);
                        break;
                    }
                    let mut part = Part::default();
                    for j in 0..(len / bpr) as usize {
                        let rec = &mut buf[j * bpr as usize..(j + 1) * bpr as usize];
                        let frn = first + j as u64;
                        if frn > u32::MAX as u64 || !apply_fixups(rec) {
                            continue;
                        }
                        match parse_record(rec) {
                            None => {}
                            Some(Parsed::Base {
                                dir,
                                names,
                                size,
                                mtime,
                                attrs,
                            }) => {
                                if names.is_empty() {
                                    // noms dans des enregistrements d'extension : on garde seulement le reste
                                    part.orphans.push(Rec {
                                        attrs,
                                        frn: frn as u32,
                                        parent: 0,
                                        off: 0,
                                        len: 0,
                                        dir,
                                        size,
                                        mtime,
                                    });
                                }
                                for (k, &n) in names.iter().enumerate() {
                                    let (off, len) = push_name(&mut part, &mut units, rec, n);
                                    let r = Rec {
                                        attrs,
                                        frn: frn as u32,
                                        parent: n.parent,
                                        off,
                                        len,
                                        dir,
                                        size,
                                        mtime,
                                    };
                                    if k == 0 {
                                        part.recs.push(r);
                                    } else {
                                        part.links.push(r);
                                    }
                                }
                            }
                            Some(Parsed::Ext { base, names }) => {
                                for &n in &names {
                                    let (off, len) = push_name(&mut part, &mut units, rec, n);
                                    part.ext.push(Rec {
                                        attrs: 0,
                                        frn: base,
                                        parent: n.parent,
                                        off,
                                        len,
                                        dir: false,
                                        size: NO_SIZE,
                                        mtime: NO_MTIME,
                                    });
                                }
                            }
                        }
                    }
                    slots.lock().unwrap()[i] = Some(part);
                }
                // SAFETY: poignee ouverte plus haut dans ce fil.
                unsafe { CloseHandle(h) };
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    let mut parts: Vec<Part> = slots
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect();

    // Rattachement des noms d'extension a leur fichier de base : si la base n'a aucun nom, le premier devient
    // l'entree principale (avec type, taille et date de la base) ; tous les autres sont des liens physiques.
    let mut orphans: HashMap<u32, (bool, u64, i64, u32)> = HashMap::new();
    for p in &mut parts {
        for o in p.orphans.drain(..) {
            orphans.insert(o.frn, (o.dir, o.size, o.mtime, o.attrs));
        }
    }
    let mut n_ext = 0usize;
    for p in &mut parts {
        for mut e in std::mem::take(&mut p.ext) {
            n_ext += 1;
            if let Some((dir, size, mtime, attrs)) = orphans.remove(&e.frn) {
                e.attrs = attrs;
                e.dir = dir;
                e.size = size;
                e.mtime = mtime;
                p.recs.push(e);
            } else {
                p.links.push(e);
            }
        }
    }
    if std::env::var_os("FOUINE_PROFIL").is_some() {
        eprintln!("profil NTFS (directe) : {n_ext} noms rattaches depuis des enregistrements d'extension, {} fichiers restent sans nom", orphans.len());
    }
    Ok(parts)
}
