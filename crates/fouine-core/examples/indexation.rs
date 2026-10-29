//! Mesure la vitesse d'indexation.
//!   cargo run --release -p fouine-core --example indexation -- <dossier>      parcours de dossiers
//!   cargo run --release -p fouine-core --example indexation -- ntfs:C         NTFS (lecture directe, secours systeme)
//!   cargo run --release -p fouine-core --example indexation -- ntfsraw:C      NTFS, lecture directe seulement
//!   cargo run --release -p fouine-core --example indexation -- ntfsusn:C      NTFS, enumeration du systeme seulement
//!   cargo run --release -p fouine-core --example indexation -- compare:C      compare les deux voies NTFS (chemins)
//! Option : une 2e valeur repete la mesure (par defaut 3 fois, on garde la meilleure).
use std::path::Path;
use std::time::Instant;

use fouine_core::scan::scan_tree;
use fouine_core::Index;

#[cfg(windows)]
fn ntfs(ix: &mut Index, mode: &str, lettre: char) -> usize {
    match mode {
        "ntfsraw" => fouine_core::ntfs::scan_ntfs_raw(ix, lettre),
        "ntfsusn" => fouine_core::ntfs::scan_ntfs_usn(ix, lettre),
        _ => fouine_core::ntfs::scan_ntfs(ix, lettre),
    }
    .expect("lecture NTFS (administrateur requis)")
}

#[cfg(not(windows))]
fn ntfs(_ix: &mut Index, _mode: &str, _lettre: char) -> usize {
    panic!("NTFS : Windows seulement")
}

fn chemins(ix: &Index) -> Vec<String> {
    let mut v: Vec<String> = (0..ix.capacity_ids() as u32)
        .filter(|&i| ix.is_alive(i))
        .map(|i| ix.path(i).to_lowercase())
        .collect();
    v.sort_unstable();
    v
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cible = args.first().cloned().unwrap_or_else(|| ".".into());
    let fois: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(3);

    if let Some(l) = cible.strip_prefix("compare:") {
        let lettre = l.chars().next().unwrap_or('C');
        let (mut a, mut b) = (Index::new(), Index::new());
        let na = ntfs(&mut a, "ntfsraw", lettre);
        let nb = ntfs(&mut b, "ntfsusn", lettre);
        let (pa, pb) = (chemins(&a), chemins(&b));
        let set_b: std::collections::HashSet<&String> = pb.iter().collect();
        let set_a: std::collections::HashSet<&String> = pa.iter().collect();
        let seulement_a: Vec<&&String> = set_a.iter().filter(|p| !set_b.contains(**p)).collect();
        let seulement_b: Vec<&&String> = set_b.iter().filter(|p| !set_a.contains(**p)).collect();
        println!("directe : {na} entrees ; systeme : {nb} entrees");
        println!(
            "chemins seulement dans la lecture directe : {}",
            seulement_a.len()
        );
        for p in seulement_a.iter().take(15) {
            println!("   + {p}");
        }
        // Pour quelques chemins vus seulement par le systeme : le meme nom existe-t-il ailleurs dans la lecture directe ?
        let mut par_nom: std::collections::HashMap<&str, Vec<&String>> =
            std::collections::HashMap::new();
        for p in &pa {
            par_nom
                .entry(p.rsplit('\\').next().unwrap_or(p))
                .or_default()
                .push(p);
        }
        for p in seulement_b.iter().take(6) {
            let base = p.rsplit('\\').next().unwrap_or(p);
            println!("   ? {p}");
            println!(
                "       meme nom cote lecture directe : {:?}",
                par_nom
                    .get(base)
                    .map(|v| v.iter().take(3).collect::<Vec<_>>())
            );
        }
        println!(
            "chemins seulement dans l'enumeration du systeme : {}",
            seulement_b.len()
        );
        for p in seulement_b.iter().take(15) {
            println!("   - {p}");
        }
        // Verification sur echantillon : les chemins de la lecture directe existent-ils, et ses tailles / dates sont-elles justes ?
        let pas = (seulement_a.len() / 3000).max(1);
        let (mut vus, mut existent) = (0, 0);
        for p in seulement_a.iter().step_by(pas) {
            vus += 1;
            if std::fs::symlink_metadata(format!(r"\\?\{p}")).is_ok() {
                existent += 1;
            }
        }
        println!("echantillon de {vus} chemins vus seulement par la lecture directe : {existent} existent sur le disque");
        let n = a.capacity_ids();
        let pas = (n / 4000).max(1);
        let (mut fichiers, mut taille_ok, mut date_ok) = (0, 0, 0);
        for i in (0..n as u32).step_by(pas) {
            if !a.is_alive(i) || a.is_dir(i) {
                continue;
            }
            let Ok(m) = std::fs::metadata(format!(r"\\?\{}", a.path(i))) else {
                continue;
            };
            fichiers += 1;
            let (taille, date) = a.stat(i);
            if taille == m.len() {
                taille_ok += 1;
            }
            let reel = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            if (date - reel).abs() <= 2 {
                date_ok += 1;
            }
        }
        println!(
            "echantillon de {fichiers} fichiers : taille juste {taille_ok}, date juste {date_ok}"
        );
        return;
    }

    // volumes:C:\;D:\ : indexe plusieurs racines comme l'application (volumes NTFS lus en parallele), puis
    // mesure une premiere recherche triee par nom : c'est le « pret a chercher » comparable a Everything.
    if let Some(liste) = cible.strip_prefix("volumes:") {
        let racines: Vec<String> = liste.split(';').map(String::from).collect();
        let mut meilleur = (f64::MAX, 0.0);
        let mut total = 0;
        for _ in 0..fois {
            let t = Instant::now();
            let mut ix = Index::new();
            total =
                fouine_core::daemon::index_roots(&mut ix, &racines, &Default::default(), &|m| {
                    eprintln!("{m}")
                });
            let t_index = t.elapsed().as_secs_f64();
            let t2 = Instant::now();
            let n = fouine_core::search(
                &ix,
                "dll",
                fouine_core::Options::default(),
                5000,
                fouine_core::Sort::Name,
            )
            .map(|v| v.len())
            .unwrap_or(0);
            let t_rech = t2.elapsed().as_secs_f64();
            println!("  {total} entrees : index complet en {t_index:.2} s ; 1re recherche « dll » triee par nom ({n} resultats) en {:.0} ms ; pret a chercher apres {:.2} s", t_rech * 1000.0, t_index + t_rech);
            if t_index + t_rech < meilleur.0 {
                meilleur = (t_index + t_rech, t_index);
            }
        }
        println!(
            "{cible} : {total} entrees, meilleur temps pret a chercher {:.2} s (index {:.2} s)",
            meilleur.0, meilleur.1
        );
        // Redemarrage : enregistrement puis rechargement de l'index complet (le cas courant au demarrage)
        let mut ix = Index::new();
        fouine_core::daemon::index_roots(&mut ix, &racines, &Default::default(), &|_| {});
        let fichier = std::env::temp_dir().join("fouine-bench-index.db");
        let t = Instant::now();
        ix.save(&fichier).expect("enregistrement");
        let t_save = t.elapsed().as_secs_f64();
        let taille = std::fs::metadata(&fichier).map(|m| m.len()).unwrap_or(0);
        let t = Instant::now();
        let relu = Index::load(&fichier).expect("rechargement");
        let t_load = t.elapsed().as_secs_f64();
        println!(
            "  index enregistre en {t_save:.2} s ({:.0} Mo), recharge en {t_load:.2} s ({} entrees)",
            taille as f64 / 1e6,
            relu.len()
        );
        let _ = std::fs::remove_file(&fichier);
        return;
    }

    let mut meilleur = f64::MAX;
    let mut total = 0;
    for _ in 0..fois {
        let t = Instant::now();
        let mut ix = Index::new();
        let n =
            if let Some((mode, l)) = cible.split_once(':').filter(|(m, _)| m.starts_with("ntfs")) {
                ntfs(&mut ix, mode, l.chars().next().unwrap_or('C'))
            } else {
                scan_tree(&mut ix, Path::new(&cible), &[])
            };
        let s = t.elapsed().as_secs_f64();
        total = n;
        println!("  {n} entrees en {s:.2} s ({:.0} entrees/s)", n as f64 / s);
        meilleur = meilleur.min(s);
    }
    println!(
        "{cible} : {total} entrees, meilleur temps {meilleur:.2} s, {:.0} entrees/s",
        total as f64 / meilleur
    );
}
