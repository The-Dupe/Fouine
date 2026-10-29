//! Matrice de compatibilite fonctionnelle (voir docs/PLAN.md) : un test par ligne.

use fouine_core::export::{export, Format};
use fouine_core::query::natural_cmp;
use fouine_core::scan::scan_tree;
use fouine_core::store::Store;
use fouine_core::{search, Index, Options, Sort};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Dossier temporaire supprime a la fin (cree avec les droits par defaut du dossier parent).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!(
                "fouine-test-{}-{}-{nanos}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&p).unwrap();
        let canon = fs::canonicalize(&p).unwrap();
        TempDir(PathBuf::from(
            canon.to_string_lossy().trim_start_matches(r"\\?\"),
        ))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> (TempDir, Index) {
    let tmp = TempDir::new();
    let r = tmp.path();
    fs::create_dir_all(r.join("Projets/Alpha")).unwrap();
    fs::create_dir_all(r.join("Photos")).unwrap();
    for (rel, size) in [
        ("Projets/Alpha/rapport.docx", 10usize),
        ("Projets/Alpha/Rapport-final.PDF", 3000),
        ("Projets/notes.txt", 5),
        ("Photos/plage.jpg", 2_000_000),
        ("Photos/ete.JPG", 10),
        ("Photos/img2.png", 1),
        ("Photos/img10.png", 1),
    ] {
        fs::write(r.join(rel), vec![b'x'; size]).unwrap();
    }
    let mut ix = Index::new();
    scan_tree(&mut ix, r, &[]);
    (tmp, ix)
}

fn q(ix: &Index, text: &str) -> Vec<String> {
    let mut v: Vec<String> = search(ix, text, Options::default(), 1000, Sort::Name)
        .unwrap()
        .iter()
        .map(|&i| ix.name(i).to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn sous_chaine_insensible_a_la_casse() {
    let (_t, ix) = fixture();
    assert_eq!(q(&ix, "rapport"), ["Rapport-final.PDF", "rapport.docx"]);
}

#[test]
fn et_ou_non() {
    let (_t, ix) = fixture();
    assert_eq!(q(&ix, "rapport final"), ["Rapport-final.PDF"]);
    assert_eq!(q(&ix, "plage | notes"), ["notes.txt", "plage.jpg"]);
    assert_eq!(q(&ix, "plage OR notes"), ["notes.txt", "plage.jpg"]);
    assert_eq!(q(&ix, "rapport !final"), ["rapport.docx"]);
    assert_eq!(q(&ix, "rapport NOT final"), ["rapport.docx"]);
    assert_eq!(q(&ix, "rapport AND final"), ["Rapport-final.PDF"]);
}

#[test]
fn extension_et_type() {
    let (_t, ix) = fixture();
    assert_eq!(q(&ix, "ext:jpg"), ["ete.JPG", "plage.jpg"]);
    assert_eq!(
        q(&ix, "ext:.pdf;docx"),
        ["Rapport-final.PDF", "rapport.docx"]
    );
    assert_eq!(q(&ix, "folder: pho"), ["Photos"]);
    assert!(q(&ix, "file: photos").is_empty());
}

#[test]
fn taille() {
    let (_t, ix) = fixture();
    assert_eq!(q(&ix, "size:>1mb"), ["plage.jpg"]);
    assert_eq!(q(&ix, "ext:pdf size:1kb..10kb"), ["Rapport-final.PDF"]);
    assert_eq!(q(&ix, "ext:txt size:5"), ["notes.txt"]);
    assert_eq!(q(&ix, "ext:txt size:>5"), Vec::<String>::new());
    assert_eq!(q(&ix, "ext:txt size:>=5"), ["notes.txt"]);
    assert!(search(&ix, "size:12zz", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn dates() {
    let mut ix = Index::new();
    let now = chrono::Local::now().timestamp();
    ix.add(
        "recent.txt",
        fouine_core::index::NO_PARENT,
        false,
        Some(1),
        Some(now),
    );
    ix.add(
        "ancien.txt",
        fouine_core::index::NO_PARENT,
        false,
        Some(1),
        Some(1_000_000_000),
    );
    assert_eq!(q(&ix, "dm:today"), ["recent.txt"]);
    assert_eq!(q(&ix, "dm:2001"), ["ancien.txt"]);
    assert_eq!(q(&ix, "dm:<2010"), ["ancien.txt"]);
    assert_eq!(
        q(&ix, "dm:2001..2002 | dm:today"),
        ["ancien.txt", "recent.txt"]
    );
    assert!(search(&ix, "dm:pasunedate", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn casse_regex_jokers_mot_entier() {
    let (_t, ix) = fixture();
    assert_eq!(q(&ix, "case:Rapport"), ["Rapport-final.PDF"]);
    assert_eq!(q(&ix, "regex:^r.*docx$"), ["rapport.docx"]);
    assert_eq!(q(&ix, "*.jpg"), ["ete.JPG", "plage.jpg"]);
    assert_eq!(q(&ix, "img?.png"), ["img2.png"]);
    assert_eq!(q(&ix, "ww:final"), ["Rapport-final.PDF"]);
    assert!(q(&ix, "ww:fin").is_empty());
    assert!(search(&ix, "regex:([", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn chemin() {
    let (_t, ix) = fixture();
    assert_eq!(
        q(&ix, "alpha\\ rapport"),
        ["Rapport-final.PDF", "rapport.docx"]
    );
    assert_eq!(q(&ix, "path:photos plage"), ["plage.jpg"]);
    assert_eq!(q(&ix, "\"rapport-final\""), ["Rapport-final.PDF"]);
    assert!(q(&ix, "\"final rapport\"").is_empty());
}

#[test]
fn tri_naturel_et_par_taille() {
    let (_t, ix) = fixture();
    let ids = search(&ix, "img", Options::default(), 10, Sort::Name).unwrap();
    assert_eq!(
        ids.iter().map(|&i| ix.name(i)).collect::<Vec<_>>(),
        ["img2.png", "img10.png"]
    );
    let by_size = search(&ix, "ext:jpg", Options::default(), 10, Sort::Size).unwrap();
    assert_eq!(ix.name(by_size[0]), "plage.jpg");
    assert_eq!(natural_cmp("a2", "a10"), std::cmp::Ordering::Less);
    assert_eq!(natural_cmp("a02", "a2"), std::cmp::Ordering::Equal);
}

#[test]
fn limite_et_requete_vide() {
    let (_t, ix) = fixture();
    assert_eq!(
        search(&ix, "", Options::default(), 10, Sort::Name)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        search(&ix, "e", Options::default(), 3, Sort::Name)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        search(&ix, "e", Options::default(), 2, Sort::None)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn incremental_ajout_et_suppression() {
    let (t, mut ix) = fixture();
    let p = t.path().join("nouveau.txt");
    fs::write(&p, "x").unwrap();
    ix.add_path(&p).unwrap();
    assert_eq!(q(&ix, "nouveau"), ["nouveau.txt"]);
    let id = ix.lookup(&p.to_string_lossy()).unwrap();
    assert_eq!(ix.remove_tree(id), 1);
    assert!(q(&ix, "nouveau").is_empty());
}

#[test]
fn incremental_dossier_et_compactage() {
    let (t, mut ix) = fixture();
    let d = t.path().join("dossier");
    fs::create_dir_all(d.join("sous")).unwrap();
    fs::write(d.join("sous/profond.txt"), "x").unwrap();
    ix.add_path(&d).unwrap();
    assert_eq!(q(&ix, "profond"), ["profond.txt"]);
    let id = ix.lookup(&d.to_string_lossy()).unwrap();
    assert_eq!(ix.remove_tree(id), 3);
    assert!(q(&ix, "profond").is_empty() && q(&ix, "sous").is_empty());
    let (before, alive) = (ix.capacity_ids(), ix.len());
    ix.compact();
    assert!(ix.capacity_ids() < before);
    assert_eq!(ix.capacity_ids(), alive);
    assert_eq!(q(&ix, "plage"), ["plage.jpg"]);
    let first = search(&ix, "plage", Options::default(), 1, Sort::Name).unwrap()[0];
    assert!(Path::new(&ix.path(first)).exists());
}

#[test]
fn persistance() {
    let (t, mut ix) = fixture();
    let id = ix
        .lookup(&t.path().join("Photos").join("ete.JPG").to_string_lossy())
        .unwrap();
    ix.remove_tree(id);
    let db = t.path().join("index.db");
    ix.save(&db).unwrap();
    let back = Index::load(&db).unwrap();
    assert_eq!(back.len(), ix.len());
    assert_eq!(q(&back, "ext:jpg"), ["plage.jpg"]);
}

#[test]
fn historique_et_favoris() {
    let tmp = TempDir::new();
    let p = tmp.path().join("s.json");
    let mut s = Store::open(&p);
    for h in ["a", "b", "a", "  ", "c"] {
        s.record(h).unwrap();
    }
    assert_eq!(s.history(), ["c", "a", "b"]);
    s.set_bookmark("pdfs", "ext:pdf").unwrap();
    s.set_bookmark("pdfs", "ext:pdf size:>1mb").unwrap();
    let mut s2 = Store::open(&p);
    assert_eq!(s2.bookmarks().get("pdfs").unwrap(), "ext:pdf size:>1mb");
    assert!(s2.delete_bookmark("pdfs").unwrap());
    assert!(!s2.delete_bookmark("pdfs").unwrap());
    assert!(s2.set_bookmark("", "x").is_err());
    for i in 0..150 {
        s2.record(&format!("q{i}")).unwrap();
    }
    assert_eq!(s2.history().len(), 100);
    fs::write(&p, "{pas du json").unwrap();
    assert!(Store::open(&p).history().is_empty());
}

#[test]
fn export_formats() {
    let mut ix = Index::new();
    let r = ix.add("C:\\", fouine_core::index::NO_PARENT, true, Some(0), None);
    let a = ix.add("a;\"b\".txt", r, false, Some(12), Some(1_700_000_000));
    let csv = export(&ix, &[a], Format::Csv).unwrap();
    assert!(csv.starts_with("nom;chemin;type;taille;modifie\r\n"));
    assert!(csv.contains("\"a;\"\"b\"\".txt\";"));
    let json: serde_json::Value =
        serde_json::from_str(&export(&ix, &[a], Format::Json).unwrap()).unwrap();
    assert_eq!(json[0]["taille"], 12);
    assert!(export(&ix, &[a], Format::Txt)
        .unwrap()
        .ends_with("a;\"b\".txt\n"));
    assert_eq!(Format::from_extension("xml"), None);
}

#[cfg(windows)]
#[test]
fn ntfs_sans_droits_ou_avec() {
    // Sans administrateur : erreur propre (pas de panique). Avec : le volume doit contenir des entrees.
    let vols = fouine_core::ntfs::ntfs_volumes();
    assert!(!vols.is_empty(), "au moins un volume NTFS attendu");
    let mut ix = Index::new();
    match fouine_core::ntfs::scan_ntfs(&mut ix, vols[0]) {
        Ok(n) => assert!(n > 1000 && ix.len() == n),
        Err(e) => assert!(
            matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::Other
                    | std::io::ErrorKind::NotFound
            ),
            "{e}"
        ),
    }
}

/// Attend (jusqu'a 5 s) qu'une condition devienne vraie.
fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..50 {
        if cond() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    cond()
}

#[test]
fn surveillance_reelle_creation_renommage_suppression() {
    use fouine_core::watch::Watcher;
    use std::sync::{Arc, RwLock};
    let (t, ix) = fixture();
    let shared = Arc::new(RwLock::new(ix));
    let w = Watcher::start(
        Arc::clone(&shared),
        &[t.path().to_path_buf()],
        Arc::new(Default::default()),
        || {},
    );
    assert!(w.failed.is_empty(), "{:?}", w.failed);
    let found = |name: &str| {
        !search(
            &shared.read().unwrap(),
            name,
            Options::default(),
            10,
            Sort::None,
        )
        .unwrap()
        .is_empty()
    };

    let a = t.path().join("Photos").join("live-un.txt");
    fs::write(&a, "x").unwrap();
    assert!(wait_for(|| found("live-un")), "creation non vue");

    let b = t.path().join("Photos").join("live-deux.txt");
    fs::rename(&a, &b).unwrap();
    assert!(
        wait_for(|| found("live-deux") && !found("live-un")),
        "renommage non vu"
    );

    fs::remove_file(&b).unwrap();
    assert!(wait_for(|| !found("live-deux")), "suppression non vue");

    let d = t.path().join("nouveau-dossier");
    fs::create_dir_all(d.join("sous")).unwrap();
    fs::write(d.join("sous").join("profond-live.txt"), "x").unwrap();
    assert!(wait_for(|| found("profond-live")), "dossier cree non vu");
    fs::remove_dir_all(&d).unwrap();
    assert!(
        wait_for(|| !found("profond-live") && !found("nouveau-dossier")),
        "dossier supprime non vu"
    );
}

/// Test reel du volume NTFS : a lancer en administrateur avec `scripts/test-ntfs.ps1`.
#[cfg(windows)]
#[test]
#[ignore = "exige les droits administrateur"]
fn ntfs_volume_reel_administrateur() {
    let vol = fouine_core::ntfs::ntfs_volumes()[0];
    let mut ix = Index::new();
    let t0 = std::time::Instant::now();
    let n = fouine_core::ntfs::scan_ntfs(&mut ix, vol).expect("droits administrateur requis");
    println!("{n} entrees sur {vol}: en {:?}", t0.elapsed());
    assert!(n > 10_000 && ix.len() == n);
    // Un fichier connu du systeme doit etre trouvable, avec un chemin qui existe sur le disque.
    let ids = search(&ix, "ntoskrnl.exe", Options::default(), 50, Sort::Path).unwrap();
    assert!(!ids.is_empty(), "ntoskrnl.exe introuvable");
    assert!(
        ids.iter()
            .any(|&i| std::path::Path::new(&ix.path(i)).exists()),
        "aucun chemin reconstruit n'existe"
    );
    // Echantillon : les chemins reconstruits existent (hors fichiers temporaires disparus entre-temps).
    let sample = search(&ix, "ext:dll", Options::default(), 200, Sort::None).unwrap();
    let ok = sample
        .iter()
        .filter(|&&i| std::path::Path::new(&ix.path(i)).exists())
        .count();
    assert!(
        ok * 100 / sample.len() >= 90,
        "{ok}/{} chemins valides",
        sample.len()
    );
}

#[test]
fn debut_fin_nom_exact_profondeur() {
    let (_t, ix) = fixture();
    assert_eq!(
        q(&ix, "startwith:rapport"),
        ["Rapport-final.PDF", "rapport.docx"]
    );
    assert_eq!(q(&ix, "startwith:rapport ext:docx"), ["rapport.docx"]);
    assert_eq!(q(&ix, "endwith:.jpg"), ["ete.JPG", "plage.jpg"]);
    assert_eq!(q(&ix, "exact:notes.txt"), ["notes.txt"]);
    assert_eq!(q(&ix, "wfn:NOTES.TXT"), ["notes.txt"]);
    assert!(q(&ix, "exact:notes").is_empty());
    // profondeur : les fichiers de Photos sont un niveau plus haut que ceux de Projets/Alpha
    assert_eq!(q(&ix, "depth:>0 plage"), ["plage.jpg"]);
    let deep = q(&ix, "depth:>=3 file:");
    assert!(deep.contains(&"rapport.docx".to_string()) && !deep.contains(&"plage.jpg".to_string()));
    assert!(search(&ix, "depth:abc", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn categories_de_fichiers() {
    let (_t, ix) = fixture();
    assert_eq!(
        q(&ix, "type:image"),
        ["ete.JPG", "img10.png", "img2.png", "plage.jpg"]
    );
    assert_eq!(
        q(&ix, "type:documents rapport"),
        ["Rapport-final.PDF", "rapport.docx"]
    );
    assert_eq!(q(&ix, "type:photos !img"), ["ete.JPG", "plage.jpg"]);
    assert!(q(&ix, "type:video").is_empty());
    assert!(search(&ix, "type:inconnu", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn dates_creation_et_acces() {
    let (tmp, _) = fixture();
    let f = fs::File::options()
        .write(true)
        .open(tmp.path().join("Projets/notes.txt"))
        .unwrap();
    // Date de modification et d'acces reculees ; la creation reste celle d'aujourd'hui.
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    f.set_times(
        std::fs::FileTimes::new()
            .set_modified(old)
            .set_accessed(old),
    )
    .unwrap();
    drop(f);
    let mut ix = Index::new();
    scan_tree(&mut ix, tmp.path(), &[]);
    assert_eq!(q(&ix, "dm:2001"), ["notes.txt"]);
    assert_eq!(q(&ix, "da:2001"), ["notes.txt"]);
    assert_eq!(q(&ix, "dateaccessed:<2010"), ["notes.txt"]);
    assert!(!q(&ix, "dc:today").is_empty());
    assert!(q(&ix, "dc:today").contains(&"notes.txt".to_string()));
    assert!(q(&ix, "dc:<2010").is_empty());
    assert!(q(&ix, "datecreated:2001").is_empty());
    assert!(search(&ix, "dc:pasunedate", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn contenu() {
    let tmp = TempDir::new();
    let r = tmp.path();
    fs::write(r.join("a.txt"), "Le Budget Annuel est prêt").unwrap();
    fs::write(r.join("b.txt"), "rien ici").unwrap();
    fs::write(r.join("c.log"), "budget en retard").unwrap();
    // Au-dela de la limite lue : le motif est ignoré.
    let mut gros = vec![b'x'; fouine_core::query::CONTENT_MAX_BYTES as usize];
    gros.extend_from_slice(b"budget");
    fs::write(r.join("gros.txt"), gros).unwrap();
    fs::create_dir(r.join("budget")).unwrap();
    let mut ix = Index::new();
    scan_tree(&mut ix, r, &[]);
    assert_eq!(q(&ix, "content:budget"), ["a.txt", "c.log"]);
    assert_eq!(q(&ix, "ext:txt content:budget"), ["a.txt"]);
    assert_eq!(q(&ix, "content:\"budget annuel\""), ["a.txt"]);
    assert_eq!(q(&ix, "content:prêt"), ["a.txt"]);
    assert_eq!(q(&ix, "!content:budget ext:txt"), ["b.txt", "gros.txt"]);
    let sensible = Options {
        case: true,
        ..Options::default()
    };
    let v = search(&ix, "content:Budget", sensible, 100, Sort::Name).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(ix.name(v[0]), "a.txt");
    assert!(search(&ix, "content:", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn parents_infolder_childcount() {
    let (tmp, ix) = fixture();
    let n = q(&ix, "depth:2");
    assert!(!n.is_empty());
    assert_eq!(q(&ix, "parents:2"), n);
    assert_eq!(q(&ix, "parents:>=2 | parents:<2"), q(&ix, "*"));

    let projets = tmp.path().join("Projets");
    let sous = q(&ix, &format!("infolder:\"{}\"", projets.display()));
    assert_eq!(
        sous,
        ["Alpha", "Rapport-final.PDF", "notes.txt", "rapport.docx"]
    );
    // Le dossier lui-meme n'en fait pas partie ; `parent:` ne prend que les enfants directs.
    assert_eq!(
        q(&ix, &format!("parent:\"{}\"", projets.display())),
        ["Alpha", "notes.txt"]
    );
    // Un prefixe de nom n'est pas un dossier parent.
    let faux = tmp.path().join("Proj");
    assert!(q(&ix, &format!("infolder:\"{}\"", faux.display())).is_empty());

    // Alpha (2 fichiers), Projets (Alpha + notes.txt) et la racine (Projets + Photos) ont 2 entrees.
    let mut deux = vec![
        "Alpha".to_string(),
        "Projets".to_string(),
        tmp.path().display().to_string(),
    ];
    deux.sort();
    assert_eq!(q(&ix, "childcount:2"), deux);
    assert_eq!(q(&ix, "childcount:4"), ["Photos"]);
    assert_eq!(q(&ix, "childcount:>=2 file:"), Vec::<String>::new());
    assert!(q(&ix, "childcount:0").is_empty());
    assert!(search(&ix, "childcount:abc", Options::default(), 10, Sort::Name).is_err());
}

#[test]
fn doublons_par_nom() {
    let tmp = TempDir::new();
    let r = tmp.path();
    for d in ["a", "b", "c"] {
        fs::create_dir(r.join(d)).unwrap();
    }
    fs::write(r.join("a/x.txt"), "1").unwrap();
    fs::write(r.join("b/X.TXT"), "2").unwrap();
    fs::write(r.join("c/y.txt"), "3").unwrap();
    let mut ix = Index::new();
    scan_tree(&mut ix, r, &[]);
    assert_eq!(q(&ix, "dupe:"), ["X.TXT", "x.txt"]);
    assert_eq!(q(&ix, "!dupe: ext:txt"), ["y.txt"]);
    assert_eq!(q(&ix, "dupe: ext:txt c"), Vec::<String>::new());
}

#[test]
fn demon_surveille_et_enregistre() {
    use fouine_core::daemon::{run, DaemonConfig};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let tmp = TempDir::new();
    let r = tmp.path();
    fs::create_dir(r.join("dossier")).unwrap();
    fs::write(r.join("dossier/avant.txt"), "1").unwrap();
    fs::write(r.join("dossier/a_supprimer.txt"), "1").unwrap();
    let db = r.join("index.db");
    // Racine surveillee : un sous-dossier distinct du fichier de base, pour ne pas s'observer soi-meme.
    let cfg = DaemonConfig {
        roots: vec![r.join("dossier").display().to_string()],
        exclusions: Default::default(),
        db: db.clone(),
        save_every: std::time::Duration::from_millis(300),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = Arc::clone(&stop);
    let th = std::thread::spawn(move || run(&cfg, &s2, &|_| {}));
    let noms = |db: &std::path::Path| -> Vec<String> {
        let Ok(ix) = Index::load(db) else {
            return vec![];
        };
        let mut v: Vec<String> = search(&ix, "*.txt", Options::default(), 100, Sort::Name)
            .unwrap()
            .iter()
            .map(|&i| ix.name(i).to_string())
            .collect();
        v.sort();
        v
    };
    assert!(wait_for(|| noms(&db) == ["a_supprimer.txt", "avant.txt"]));
    std::thread::sleep(std::time::Duration::from_millis(800)); // laisse la surveillance s'armer
    fs::write(r.join("dossier/apres.txt"), "2").unwrap();
    fs::remove_file(r.join("dossier/a_supprimer.txt")).unwrap();
    let ok = wait_for(|| noms(&db) == ["apres.txt", "avant.txt"]);
    stop.store(true, Ordering::Relaxed);
    th.join().unwrap();
    assert!(ok, "ajout et suppression non enregistres : {:?}", noms(&db));
}

#[test]
fn persistance_ancien_format_gzip() {
    // Un fichier ecrit par une version precedente (gzip) doit rester lisible.
    let (tmp, ix) = fixture();
    let db = tmp.path().join("index-lz4.db");
    ix.save(&db).unwrap();
    let octets = fs::read(&db).unwrap();
    assert_eq!(
        &octets[..4],
        &[0x04, 0x22, 0x4D, 0x18],
        "format LZ4 attendu"
    );
    let relu = Index::load(&db).unwrap();
    assert_eq!(relu.len(), ix.len());
    // Re-ecrit le meme contenu en gzip (ancien format) a partir du flux decompresse.
    let mut brut = Vec::new();
    std::io::Read::read_to_end(
        &mut lz4_flex::frame::FrameDecoder::new(&octets[..]),
        &mut brut,
    )
    .unwrap();
    let gz = tmp.path().join("index-gzip.db");
    {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(
            fs::File::create(&gz).unwrap(),
            flate2::Compression::fast(),
        );
        enc.write_all(&brut).unwrap();
        enc.finish().unwrap();
    }
    let ancien = Index::load(&gz).unwrap();
    assert_eq!(ancien.len(), ix.len());
    assert_eq!(q(&ancien, "rapport"), q(&ix, "rapport"));
}

#[test]
fn parcours_parallele_gros_arbre() {
    // Plus de 4 096 entrees : le parcours est decoupe en plusieurs blocs, dont les parents doivent rester justes.
    let tmp = TempDir::new();
    let r = tmp.path();
    let (mut total, mut profond) = (1usize, String::new()); // 1 = la racine
    for a in 0..6 {
        for b in 0..6 {
            let d = r.join(format!("a{a}")).join(format!("b{b}"));
            fs::create_dir_all(&d).unwrap();
            total += if b == 0 { 2 } else { 1 }; // a{a} (une fois) et b{b}
            for f in 0..200 {
                fs::write(d.join(format!("fichier-{a}-{b}-{f}.txt")), "x").unwrap();
                total += 1;
            }
            if (a, b) == (4, 5) {
                profond = format!("fichier-{a}-{b}-77.txt");
            }
        }
    }
    let mut ix = Index::new();
    let n = scan_tree(&mut ix, r, &[]);
    assert_eq!(n, total, "nombre d'entrees indexees");
    assert_eq!(ix.len(), total);
    let trouves = search(&ix, &profond, Options::default(), 10, Sort::Name).unwrap();
    assert_eq!(trouves.len(), 1);
    let chemin = ix.path(trouves[0]);
    assert!(
        chemin.ends_with(&format!("a4{0}b5{0}{profond}", std::path::MAIN_SEPARATOR)),
        "chemin : {chemin}"
    );
    // Tous les fichiers sont presents (l'aide `q` plafonne a 1 000 resultats : on compte avec une limite haute)
    let fichiers = search(&ix, "ext:txt", Options::default(), 100_000, Sort::None).unwrap();
    assert_eq!(fichiers.len(), 6 * 6 * 200);
}

#[cfg(windows)]
#[test]
#[ignore = "exige les droits administrateur ; lit tout un volume NTFS (quelques secondes)"]
fn ntfs_lecture_directe_equivaut_systeme() {
    use fouine_core::ntfs::{ntfs_volumes, scan_ntfs_raw, scan_ntfs_usn};
    let Some(&lettre) = ntfs_volumes().first() else {
        return;
    };
    let (mut a, mut b) = (Index::new(), Index::new());
    let (Ok(na), Ok(nb)) = (scan_ntfs_raw(&mut a, lettre), scan_ntfs_usn(&mut b, lettre)) else {
        return; // pas de droits : l'autre test NTFS couvre ce cas
    };
    let chemins = |ix: &Index| -> std::collections::HashSet<String> {
        (0..ix.capacity_ids() as u32)
            .filter(|&i| ix.is_alive(i))
            .map(|i| ix.path(i).to_lowercase())
            .collect()
    };
    let (pa, pb) = (chemins(&a), chemins(&b));
    let seulement_systeme = pb.difference(&pa).count();
    // La lecture directe voit tous les liens physiques (plus de chemins) ; elle ne doit presque rien perdre.
    assert!(na >= nb, "directe {na} < systeme {nb}");
    assert!(
        seulement_systeme * 1000 <= pb.len(),
        "{seulement_systeme} chemins vus seulement par le systeme sur {}",
        pb.len()
    );
}

fn tous(ix: &Index) -> Vec<String> {
    let mut v: Vec<String> = search(ix, "*", Options::default(), 100_000, Sort::None)
        .unwrap_or_default()
        .iter()
        .map(|&i| ix.path(i))
        .collect();
    v.sort();
    v
}

#[test]
fn exclusions_parcours_dossiers_motifs_caches() {
    use fouine_core::exclusions::Exclusions;
    use fouine_core::scan::scan_tree_with;
    let tmp = TempDir::new();
    let r = tmp.path();
    fs::create_dir_all(r.join("garde")).unwrap();
    fs::create_dir_all(r.join("exclu/sous")).unwrap();
    for f in [
        "garde/a.txt",
        "garde/b.tmp",
        "garde/c.log",
        "garde/cache.txt",
        "exclu/d.txt",
        "exclu/sous/e.txt",
    ] {
        fs::write(r.join(f), "x").unwrap();
    }
    #[cfg(windows)]
    {
        // attribut « cache » (outil systeme attrib)
        let ok = std::process::Command::new("attrib")
            .arg("+H")
            .arg(r.join("garde/cache.txt"))
            .status()
            .unwrap();
        assert!(ok.success());
    }
    let noms = |excl: &Exclusions| -> Vec<String> {
        let mut ix = Index::new();
        scan_tree_with(&mut ix, r, excl);
        let mut v: Vec<String> = (0..ix.capacity_ids() as u32)
            .filter(|&i| ix.is_alive(i) && !ix.is_dir(i))
            .map(|i| ix.name(i).to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        noms(&Exclusions::default()),
        ["a.txt", "b.tmp", "c.log", "cache.txt", "d.txt", "e.txt"]
    );
    // dossier exclu : tout son contenu disparait ; motifs : *.tmp et *.log
    let dossier = r.join("exclu").to_string_lossy().into_owned();
    assert_eq!(
        noms(&Exclusions::new(
            &[dossier],
            &["*.tmp; *.log".into()],
            false,
            false
        )),
        ["a.txt", "cache.txt"]
    );
    // fichiers caches (Windows : attribut ; ailleurs : nom commencant par un point)
    #[cfg(windows)]
    assert_eq!(
        noms(&Exclusions::new(&[], &[], true, false)),
        ["a.txt", "b.tmp", "c.log", "d.txt", "e.txt"]
    );
    // un motif ne s'applique pas aux dossiers
    let mut ix = Index::new();
    fs::create_dir(r.join("garde/dossier.tmp")).unwrap();
    scan_tree_with(
        &mut ix,
        r,
        &Exclusions::new(&[], &["*.tmp".into()], false, false),
    );
    assert!(tous(&ix).iter().any(|p| p.ends_with("dossier.tmp")));
}

#[cfg(windows)]
#[test]
#[ignore = "exige les droits administrateur ; lit tout un volume NTFS (quelques secondes)"]
fn ntfs_exclusions() {
    use fouine_core::exclusions::{Exclusions, ATTR_HIDDEN, ATTR_SYSTEM};
    use fouine_core::ntfs::{add_volume, ntfs_volumes, read_volume};
    let Some(&lettre) = ntfs_volumes().first() else {
        return;
    };
    let Ok(data) = read_volume(lettre) else {
        return;
    };
    let (mut tout, mut filtre) = (Index::new(), Index::new());
    add_volume(&mut tout, &data, &Exclusions::default());
    let dossier = format!("{lettre}:\\Windows");
    let excl = Exclusions::new(
        std::slice::from_ref(&dossier),
        &["*.dll".into()],
        true,
        true,
    );
    add_volume(&mut filtre, &data, &excl);
    assert!(
        filtre.len() < tout.len(),
        "les exclusions doivent retirer des entrees"
    );
    let chemins = tous_gros(&filtre);
    let prefixe = dossier.to_lowercase();
    assert!(
        chemins
            .iter()
            .all(|p| !p.to_lowercase().starts_with(&format!("{prefixe}\\"))),
        "contenu d'un dossier exclu present"
    );
    assert!(
        chemins.iter().all(|p| !p.to_lowercase().ends_with(".dll")),
        "motif *.dll non applique"
    );
    // echantillon : aucune entree retenue n'est cachee ou systeme sur le disque
    let n = filtre.capacity_ids();
    let pas = (n / 3000).max(1);
    let (mut verifies, mut fautes) = (0, 0);
    for i in (0..n as u32).step_by(pas) {
        if !filtre.is_alive(i) {
            continue;
        }
        let Ok(m) = std::fs::symlink_metadata(format!(r"\\?\{}", filtre.path(i))) else {
            continue;
        };
        use std::os::windows::fs::MetadataExt;
        verifies += 1;
        if m.file_attributes() & (ATTR_HIDDEN | ATTR_SYSTEM) != 0 {
            fautes += 1;
        }
    }
    assert!(verifies > 1000, "echantillon trop petit : {verifies}");
    assert!(
        fautes * 200 <= verifies,
        "{fautes} entrees cachees ou systeme sur {verifies}"
    );
}

#[cfg(windows)]
fn tous_gros(ix: &Index) -> Vec<String> {
    (0..ix.capacity_ids() as u32)
        .filter(|&i| ix.is_alive(i))
        .map(|i| ix.path(i))
        .collect()
}
