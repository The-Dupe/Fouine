//! Windows seulement : integre l'icone et les informations de version (VERSIONINFO) dans `fouine.exe`,
//! pour que l'Explorateur, la barre des taches et les proprietes du fichier affichent le produit.
//! Ailleurs, ne fait rien. Un compilateur de ressources absent est un avertissement, sauf si
//! `FOUINE_REQUIRE_WINRES` est defini (builds de release).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/fouine.ico");
    println!("cargo:rerun-if-env-changed=FOUINE_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/fouine.ico")
            .set("ProductName", "Fouine")
            .set(
                "FileDescription",
                "Fouine : recherche instantanee de fichiers",
            )
            .set(
                "LegalCopyright",
                "Copyright (c) 2026 Fouine contributors. MIT OR Apache-2.0.",
            )
            .set("OriginalFilename", "fouine.exe")
            .set("InternalName", "fouine");
        if let Err(e) = res.compile() {
            if std::env::var_os("FOUINE_REQUIRE_WINRES").is_some() {
                panic!("integration des ressources Windows impossible : {e}");
            }
            println!(
                "cargo:warning=fouine.exe compile sans icone ni informations de version : {e}"
            );
        }
    }
}
