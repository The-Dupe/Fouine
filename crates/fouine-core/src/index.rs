//! Index en colonnes : noms contigus (original + minuscules), parents, indicateurs, tailles, dates.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use memchr::memmem;
use serde::{Deserialize, Serialize};

pub type Id = u32;
pub const NO_PARENT: Id = u32::MAX;
const UNKNOWN_SIZE: u64 = u64::MAX;
const UNKNOWN_TIME: i64 = i64::MIN;

pub const FLAG_DIR: u8 = 1;
const FLAG_DEAD: u8 = 2;

#[derive(Default)]
pub struct Index {
    names: String,
    name_start: Vec<u32>,
    lower: String, // chaque nom en minuscules suivi de '\n'
    lower_start: Vec<u32>,
    parent: Vec<Id>,
    flags: Vec<u8>,
    size: Vec<AtomicU64>,
    mtime: Vec<AtomicI64>,
    dead: usize,
    child: Option<HashMap<(Id, u64), Vec<Id>>>,
}

fn hash_name(lower: &str) -> u64 {
    // FNV-1a : stable et suffisant, les collisions sont departagees par comparaison des noms
    let mut h = 0xcbf29ce484222325u64;
    for b in lower.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre d'entrees vivantes.
    pub fn len(&self) -> usize {
        self.name_start.len() - self.dead
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Nombre total de cases, mortes comprises (bornes des identifiants).
    pub fn capacity_ids(&self) -> usize {
        self.name_start.len()
    }

    /// Reserve la place pour `entries` entrees et `name_bytes` octets de noms : evite les recopies des grands
    /// tableaux pendant une indexation massive.
    pub fn reserve(&mut self, entries: usize, name_bytes: usize) {
        self.names.reserve(name_bytes);
        self.lower.reserve(name_bytes + entries);
        self.name_start.reserve(entries);
        self.lower_start.reserve(entries);
        self.parent.reserve(entries);
        self.flags.reserve(entries);
        self.size.reserve(entries);
        self.mtime.reserve(entries);
    }

    pub fn add(
        &mut self,
        name: &str,
        parent: Id,
        is_dir: bool,
        size: Option<u64>,
        mtime: Option<i64>,
    ) -> Id {
        let id = self.name_start.len() as Id;
        self.name_start.push(self.names.len() as u32);
        self.names.push_str(name);
        let low_start = self.lower.len();
        self.lower_start.push(low_start as u32);
        // minuscules ecrites directement dans le tampon (pas d'allocation par entree) ; un saut de ligne dans
        // un nom devient une espace, car '\n' sert de separateur.
        for c in name.chars() {
            if c == '\n' {
                self.lower.push(' ');
            } else if c.is_ascii() {
                self.lower.push(c.to_ascii_lowercase());
            } else {
                self.lower.extend(c.to_lowercase());
            }
        }
        let low_end = self.lower.len();
        self.lower.push('\n');
        self.parent.push(parent);
        self.flags.push(if is_dir { FLAG_DIR } else { 0 });
        self.size.push(AtomicU64::new(size.unwrap_or(UNKNOWN_SIZE)));
        self.mtime
            .push(AtomicI64::new(mtime.unwrap_or(UNKNOWN_TIME)));
        if let Some(map) = self.child.as_mut() {
            let h = hash_name(&self.lower[low_start..low_end]);
            map.entry((parent, h)).or_default().push(id);
        }
        id
    }

    pub fn name(&self, id: Id) -> &str {
        let i = id as usize;
        let end = self
            .name_start
            .get(i + 1)
            .map_or(self.names.len(), |&e| e as usize);
        &self.names[self.name_start[i] as usize..end]
    }

    pub fn lower_name(&self, id: Id) -> &str {
        let i = id as usize;
        let end = self
            .lower_start
            .get(i + 1)
            .map_or(self.lower.len(), |&e| e as usize)
            - 1;
        &self.lower[self.lower_start[i] as usize..end]
    }

    pub fn parent(&self, id: Id) -> Id {
        self.parent[id as usize]
    }

    pub fn is_dir(&self, id: Id) -> bool {
        self.flags[id as usize] & FLAG_DIR != 0
    }

    pub fn is_alive(&self, id: Id) -> bool {
        self.flags[id as usize] & FLAG_DEAD == 0
    }

    /// Chemin complet, avec `\` ou `/` selon la plateforme du chemin racine.
    pub fn path(&self, id: Id) -> String {
        let mut parts = Vec::new();
        let mut cur = id;
        while cur != NO_PARENT && parts.len() < 1024 {
            parts.push(self.name(cur));
            cur = self.parent[cur as usize];
        }
        parts.reverse();
        let mut out = String::new();
        for p in parts {
            if !out.is_empty() && !out.ends_with(['\\', '/']) {
                out.push(std::path::MAIN_SEPARATOR);
            }
            out.push_str(p);
        }
        out
    }

    /// Taille et date de modification ; lues sur le disque a la premiere demande puis gardees.
    pub fn stat(&self, id: Id) -> (u64, i64) {
        let i = id as usize;
        let mut size = self.size[i].load(Ordering::Relaxed);
        let mut mtime = self.mtime[i].load(Ordering::Relaxed);
        if size == UNKNOWN_SIZE || mtime == UNKNOWN_TIME {
            let meta = std::fs::metadata(self.path(id)).ok();
            size = match &meta {
                Some(m) if !m.is_dir() => m.len(),
                _ => 0,
            };
            mtime = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            self.size[i].store(size, Ordering::Relaxed);
            self.mtime[i].store(mtime, Ordering::Relaxed);
        }
        (size, mtime)
    }

    /// Dates de creation et d'acces (secondes Unix), lues sur le disque a chaque appel et non stockees ;
    /// 0 si le fichier est illisible ou si le systeme ne fournit pas la date.
    pub fn times(&self, id: Id) -> (i64, i64) {
        let secs = |t: std::io::Result<std::time::SystemTime>| {
            t.ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64)
        };
        match std::fs::metadata(self.path(id)) {
            Ok(m) => (secs(m.created()), secs(m.accessed())),
            Err(_) => (0, 0),
        }
    }

    /// Oublie la taille et la date : elles seront relues a la demande.
    pub fn invalidate(&self, id: Id) {
        self.size[id as usize].store(UNKNOWN_SIZE, Ordering::Relaxed);
        self.mtime[id as usize].store(UNKNOWN_TIME, Ordering::Relaxed);
    }

    /// Identifiants vivants dont le nom en minuscules contient `needle` (deja en minuscules).
    pub fn candidates<'a>(&'a self, needle: &'a str) -> Box<dyn Iterator<Item = Id> + 'a> {
        if needle.is_empty() || needle.contains('\n') {
            return Box::new((0..self.name_start.len() as Id).filter(move |&i| self.is_alive(i)));
        }
        let finder = memmem::Finder::new(needle.as_bytes()).into_owned();
        let hay = self.lower.as_bytes();
        let mut from = 0usize;
        Box::new(std::iter::from_fn(move || loop {
            let pos = from + finder.find(&hay[from.min(hay.len())..])?;
            let id = self.lower_start.partition_point(|&s| (s as usize) <= pos) - 1;
            from = self
                .lower_start
                .get(id + 1)
                .map_or(hay.len(), |&s| s as usize);
            if self.flags[id] & FLAG_DEAD == 0 {
                return Some(id as Id);
            }
        }))
    }

    // --- mises a jour incrementales ---

    fn children(&mut self) -> &mut HashMap<(Id, u64), Vec<Id>> {
        if self.child.is_none() {
            let mut map: HashMap<(Id, u64), Vec<Id>> = HashMap::new();
            for id in 0..self.name_start.len() as Id {
                if self.is_alive(id) {
                    map.entry((self.parent[id as usize], hash_name(self.lower_name(id))))
                        .or_default()
                        .push(id);
                }
            }
            self.child = Some(map);
        }
        self.child.as_mut().unwrap()
    }

    fn child_of(&mut self, parent: Id, lower: &str) -> Option<Id> {
        let key = (parent, hash_name(lower));
        let ids = self.children().get(&key)?.clone();
        ids.into_iter()
            .find(|&i| self.is_alive(i) && self.lower_name(i) == lower)
    }

    /// Identifiant d'un chemin complet deja indexe.
    pub fn lookup(&mut self, path: &str) -> Option<Id> {
        let norm = path
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase();
        let (mut cur, mut best) = (None, 0usize);
        for id in 0..self.name_start.len() as Id {
            if self.parent[id as usize] != NO_PARENT || !self.is_alive(id) {
                continue;
            }
            let base = self.lower_name(id).replace('/', "\\");
            let base = base.trim_end_matches('\\');
            if (norm == base || norm.starts_with(&format!("{base}\\")))
                && (cur.is_none() || base.len() > best)
            {
                cur = Some(id);
                best = base.len();
            }
        }
        let mut cur = cur?;
        for part in norm[best..].split('\\').filter(|p| !p.is_empty()) {
            cur = self.child_of(cur, part)?;
        }
        Some(cur)
    }

    /// Indexe un chemin existant (le dossier parent doit etre connu), dossier compris avec son contenu.
    pub fn add_path(&mut self, path: &std::path::Path) -> Option<Id> {
        let parent_path = path.parent()?.to_string_lossy().into_owned();
        let parent = self.lookup(&parent_path)?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        if let Some(old) = self.child_of(parent, &name.to_lowercase()) {
            self.invalidate(old);
            return Some(old);
        }
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let id = self.add(
            &name,
            parent,
            meta.is_dir(),
            Some(if meta.is_dir() { 0 } else { meta.len() }),
            mtime,
        );
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(path) {
                for e in rd.flatten() {
                    self.add_path(&e.path());
                }
            }
        }
        Some(id)
    }

    /// Retire une entree et ses descendants ; retourne le nombre d'entrees retirees.
    pub fn remove_tree(&mut self, id: Id) -> usize {
        let mut gone = vec![false; self.name_start.len()];
        gone[id as usize] = true;
        let mut changed = true;
        while changed {
            changed = false;
            for j in 0..self.name_start.len() {
                let p = self.parent[j];
                if !gone[j] && self.flags[j] & FLAG_DEAD == 0 && p != NO_PARENT && gone[p as usize]
                {
                    gone[j] = true;
                    changed = true;
                }
            }
        }
        let mut n = 0;
        for (j, g) in gone.iter().enumerate() {
            if *g && self.flags[j] & FLAG_DEAD == 0 {
                self.flags[j] |= FLAG_DEAD;
                self.dead += 1;
                n += 1;
            }
        }
        self.child = None;
        n
    }

    /// Elimine les entrees mortes et renumerote.
    pub fn compact(&mut self) {
        if self.dead == 0 {
            return;
        }
        let n = self.name_start.len();
        let mut remap = vec![NO_PARENT; n];
        let mut next = 0;
        for (i, slot) in remap.iter_mut().enumerate() {
            if self.flags[i] & FLAG_DEAD == 0 {
                *slot = next;
                next += 1;
            }
        }
        let mut out = Index::new();
        for i in 0..n as Id {
            if !self.is_alive(i) {
                continue;
            }
            let p = self.parent[i as usize];
            let np = if p == NO_PARENT {
                NO_PARENT
            } else {
                remap[p as usize]
            };
            let (s, m) = (
                self.size[i as usize].load(Ordering::Relaxed),
                self.mtime[i as usize].load(Ordering::Relaxed),
            );
            out.add(
                self.name(i),
                np,
                self.is_dir(i),
                (s != UNKNOWN_SIZE).then_some(s),
                (m != UNKNOWN_TIME).then_some(m),
            );
        }
        *self = out;
    }

    // --- persistance ---

    pub fn save(&self, path: &std::path::Path) -> Result<(), crate::Error> {
        use std::io::Write;
        let snap = Snapshot {
            names: self.names.clone(),
            name_start: self.name_start.clone(),
            parent: self.parent.clone(),
            flags: self.flags.clone(),
            size: self
                .size
                .iter()
                .map(|a| a.load(Ordering::Relaxed))
                .collect(),
            mtime: self
                .mtime
                .iter()
                .map(|a| a.load(Ordering::Relaxed))
                .collect(),
        };
        let tmp = path.with_extension("tmp");
        let file = std::fs::File::create(&tmp)?;
        // LZ4 : bien plus rapide que gzip pour un gros index (la lecture des anciens fichiers gzip reste prise en charge).
        let mut enc = lz4_flex::frame::FrameEncoder::new(std::io::BufWriter::new(file));
        bincode::serialize_into(&mut enc, &snap)?;
        enc.finish().map_err(std::io::Error::other)?.flush()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &std::path::Path) -> Result<Self, crate::Error> {
        use std::io::BufRead;
        let file = std::fs::File::open(path)?;
        let mut rd = std::io::BufReader::new(file);
        // Format d'apres les premiers octets : gzip (ancien) ou LZ4 (actuel).
        let snap: Snapshot = if rd.fill_buf()?.starts_with(&[0x1f, 0x8b]) {
            bincode::deserialize_from(flate2::read::GzDecoder::new(rd))?
        } else {
            bincode::deserialize_from(lz4_flex::frame::FrameDecoder::new(rd))?
        };
        let mut ix = Index::new();
        let n = snap.name_start.len();
        for i in 0..n {
            let end = snap
                .name_start
                .get(i + 1)
                .map_or(snap.names.len(), |&e| e as usize);
            let name = &snap.names[snap.name_start[i] as usize..end];
            let id = ix.add(
                name,
                snap.parent[i],
                snap.flags[i] & FLAG_DIR != 0,
                (snap.size[i] != UNKNOWN_SIZE).then_some(snap.size[i]),
                (snap.mtime[i] != UNKNOWN_TIME).then_some(snap.mtime[i]),
            );
            if snap.flags[i] & FLAG_DEAD != 0 {
                ix.flags[id as usize] |= FLAG_DEAD;
                ix.dead += 1;
            }
        }
        Ok(ix)
    }
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    names: String,
    name_start: Vec<u32>,
    parent: Vec<Id>,
    flags: Vec<u8>,
    size: Vec<u64>,
    mtime: Vec<i64>,
}
