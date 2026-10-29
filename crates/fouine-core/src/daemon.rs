//! Indexation et surveillance continues sans interface : sert au mode service et a l'arriere-plan du systray.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::exclusions::Exclusions;
use crate::scan::scan_tree_with;
use crate::watch::Watcher;
use crate::Index;

/// Lettre du volume NTFS si `root` est un volume entier (sans exclusions) lisible par la table des fichiers.
#[cfg(windows)]
fn ntfs_letter(root: &str) -> Option<char> {
    let r = root.trim_end_matches(['\\', '/']);
    if r.len() == 2 && r.ends_with(':') {
        let letter = r.chars().next()?.to_ascii_uppercase();
        if crate::ntfs::ntfs_volumes().contains(&letter) {
            return Some(letter);
        }
    }
    None
}

/// Indexe une racine : table des fichiers NTFS si c'est un volume entier et si les droits le permettent
/// (sans exclusions), sinon parcours classique. `log` recoit les replis et erreurs.
pub fn index_root(
    ix: &mut Index,
    root: &str,
    exclude: &[String],
    log: &(dyn Fn(&str) + Sync),
) -> usize {
    index_roots(
        ix,
        &[root.to_string()],
        &Exclusions::from_folders(exclude),
        log,
    )
}

/// Indexe plusieurs racines. Les volumes NTFS entiers sont lus **en parallele** (une table des fichiers par
/// disque), puis ajoutes a l'index dans l'ordre des racines ; les autres racines sont parcourues
/// (parcours lui-meme parallele). Une lecture NTFS qui echoue se rabat sur le parcours des dossiers.
pub fn index_roots(
    ix: &mut Index,
    roots: &[String],
    exclusions: &Exclusions,
    log: &(dyn Fn(&str) + Sync),
) -> usize {
    #[cfg(windows)]
    let mut volumes: Vec<Option<crate::ntfs::VolumeData>> = roots.iter().map(|_| None).collect();
    #[cfg(windows)]
    std::thread::scope(|sc| {
        let handles: Vec<_> = roots
            .iter()
            .enumerate()
            .filter_map(|(i, r)| ntfs_letter(r).map(|l| (i, l)))
            .map(|(i, l)| (i, l, sc.spawn(move || crate::ntfs::read_volume(l))))
            .collect();
        for (i, letter, h) in handles {
            match h.join() {
                Ok(Ok(data)) => volumes[i] = Some(data),
                Ok(Err(e)) => log(&format!(
                    "MFT impossible sur {letter}: ({e}), parcours classique"
                )),
                Err(_) => log(&format!("lecture du volume {letter} interrompue")),
            }
        }
    });
    let mut total = 0;
    for (i, root) in roots.iter().enumerate() {
        #[cfg(windows)]
        if let Some(data) = volumes[i].take() {
            total += crate::ntfs::add_volume(ix, &data, exclusions);
            continue;
        }
        let _ = i;
        total += scan_tree_with(ix, Path::new(root), exclusions);
    }
    let _ = log;
    total
}

/// Enregistre l'index sur disque au plus toutes les `every`, seulement s'il a change (`dirty`),
/// dans un fil propre : fonctionne meme quand aucune fenetre n'est affichee. Un dernier
/// enregistrement a lieu a l'arret (`Drop`).
pub struct Autosaver {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Autosaver {
    pub fn start(
        index: Arc<RwLock<Index>>,
        db: PathBuf,
        every: Duration,
        dirty: Arc<AtomicBool>,
    ) -> Autosaver {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let mut last = Instant::now();
            let save = || {
                if dirty.swap(false, Ordering::Relaxed) {
                    if let Ok(ix) = index.read() {
                        let _ = ix.save(&db);
                    }
                }
            };
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
                if last.elapsed() >= every {
                    save();
                    last = Instant::now();
                }
            }
            save();
        });
        Autosaver {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Autosaver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub struct DaemonConfig {
    pub roots: Vec<String>,
    pub exclusions: Exclusions,
    pub db: PathBuf,
    pub save_every: Duration,
}

/// Charge l'index enregistre (ou l'indexe s'il est absent), le surveille et l'enregistre regulierement
/// jusqu'a ce que `stop` passe a vrai. Un dernier enregistrement a lieu a la fin.
pub fn run(cfg: &DaemonConfig, stop: &AtomicBool, log: &(dyn Fn(&str) + Sync)) {
    let mut ix = Index::load(&cfg.db).unwrap_or_default();
    if ix.is_empty() {
        log("indexation demarree");
        let t0 = Instant::now();
        let total = index_roots(&mut ix, &cfg.roots, &cfg.exclusions, log);
        log(&format!(
            "{total} elements indexes en {:.1} s",
            t0.elapsed().as_secs_f32()
        ));
        let _ = ix.save(&cfg.db);
    }
    let index = Arc::new(RwLock::new(ix));
    let dirty = Arc::new(AtomicBool::new(false));
    let roots: Vec<PathBuf> = cfg.roots.iter().map(PathBuf::from).collect();
    let flag = Arc::clone(&dirty);
    let watcher = Watcher::start(
        Arc::clone(&index),
        &roots,
        Arc::new(cfg.exclusions.clone()),
        move || {
            flag.store(true, Ordering::Relaxed);
        },
    );
    for (root, err) in &watcher.failed {
        log(&format!(
            "surveillance impossible sur {}: {err}",
            root.display()
        ));
    }
    log(&format!(
        "surveillance de {} dossier(s)",
        roots.len() - watcher.failed.len()
    ));
    let saver = Autosaver::start(
        Arc::clone(&index),
        cfg.db.clone(),
        cfg.save_every,
        Arc::clone(&dirty),
    );
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(watcher);
    drop(saver);
    log("arret");
}
