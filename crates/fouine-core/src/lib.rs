//! fouine-core : index de fichiers, langage de requetes, favoris, historique et export.
//! Aucune dependance a une interface ; le module `scan` et la persistance sont reserves au natif.

#[cfg(not(target_arch = "wasm32"))]
pub mod daemon;
pub mod exclusions;
pub mod export;
pub mod index;
#[cfg(windows)]
pub mod ntfs;
pub mod query;
#[cfg(not(target_arch = "wasm32"))]
pub mod scan;
pub mod store;
pub mod volumes;
#[cfg(not(target_arch = "wasm32"))]
pub mod watch;

pub use index::{Id, Index};
pub use query::{search, Options, Query, Sort};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("requete invalide : {0}")]
    Query(String),
    #[error("entree/sortie : {0}")]
    Io(#[from] std::io::Error),
    #[error("format de donnees : {0}")]
    Format(#[from] Box<bincode::ErrorKind>),
    #[error("format de donnees : {0}")]
    Json(#[from] serde_json::Error),
}
