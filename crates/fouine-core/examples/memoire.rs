//! Mesure la memoire de l'index : `cargo run --release -p fouine-core --example memoire [nombre d'entrees]`.
//! Un allocateur compteur donne les octets reellement alloues (pas une estimation du systeme).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use fouine_core::index::NO_PARENT;
use fouine_core::{search, Index, Options, Sort};

struct Compteur;
static ALLOUE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Compteur {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOUE.fetch_add(l.size(), Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        ALLOUE.fetch_sub(l.size(), Ordering::Relaxed);
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOUE.fetch_add(n, Ordering::Relaxed);
        ALLOUE.fetch_sub(l.size(), Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: Compteur = Compteur;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(500_000);
    let mots = [
        "rapport", "facture", "photo", "projet", "notes", "budget", "client", "contrat",
    ];
    let avant = ALLOUE.load(Ordering::Relaxed);
    let mut ix = Index::new();
    let racine = ix.add("C:\\", NO_PARENT, true, Some(0), None);
    let mut dossiers = vec![racine];
    let mut graine = 42u64;
    let mut alea = |m: usize| {
        graine = graine.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((graine >> 33) as usize) % m
    };
    for _ in 0..n / 20 {
        let p = dossiers[alea(dossiers.len())];
        dossiers.push(ix.add(
            &format!("{}{}", mots[alea(8)], alea(1000)),
            p,
            true,
            Some(0),
            None,
        ));
    }
    while ix.len() < n {
        let p = dossiers[alea(dossiers.len())];
        ix.add(
            &format!("{}-{}-{}.pdf", mots[alea(8)], mots[alea(8)], alea(10_000)),
            p,
            false,
            Some(1234),
            Some(1_700_000_000),
        );
    }
    let _ = search(&ix, "rapport", Options::default(), 10, Sort::None);
    let octets = ALLOUE.load(Ordering::Relaxed) - avant;
    println!(
        "{} entrees : {:.1} Mo alloues, {:.0} octets par entree",
        ix.len(),
        octets as f64 / 1_048_576.0,
        octets as f64 / ix.len() as f64
    );
}
