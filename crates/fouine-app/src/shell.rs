//! Integration au bureau : instance unique, signal « afficher la fenetre », icone de la zone de notification.
//! Windows seulement ; ailleurs, les fonctions ne font rien (pas de zone de notification geree).

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

/// Vrai quand l'utilisateur a choisi Quitter (menu de l'icone) : la fermeture de la fenetre n'est plus interceptee.
pub static QUIT: AtomicBool = AtomicBool::new(false);

/// Affiche la fenetre principale et lui donne le focus (utilisable depuis n'importe quel fil).
pub fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// Demande la fermeture reelle de l'application.
pub fn quit(ctx: &egui::Context) {
    QUIT.store(true, Ordering::Relaxed);
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    pub use tray_icon::TrayIcon;
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    };

    const MUTEX_NAME: &str = "Fouine.Instance";
    const EVENT_NAME: &str = "Fouine.Afficher";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Garde l'instance unique tant qu'il existe.
    pub struct InstanceGuard(HANDLE);

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Prend le verrou d'instance unique. `None` si une autre instance existe deja de cette session
    /// (elle est alors invitee a afficher sa fenetre). `attente_s` : delai de reessai (relance en administrateur).
    pub fn acquire_instance(wait_s: u64) -> Option<InstanceGuard> {
        let name = wide(MUTEX_NAME);
        for _ in 0..=wait_s * 10 {
            let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            if !h.is_null() && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
                return Some(InstanceGuard(h));
            }
            if !h.is_null() {
                unsafe { CloseHandle(h) };
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        signal_show();
        None
    }

    /// Demande a l'instance deja lancee d'afficher sa fenetre.
    pub fn signal_show() {
        let name = wide(EVENT_NAME);
        let h = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !h.is_null() {
            unsafe {
                SetEvent(h);
                CloseHandle(h);
            }
        }
    }

    /// Fil qui attend le signal « afficher » d'une autre instance.
    pub fn listen_show(ctx: egui::Context) {
        let name = wide(EVENT_NAME);
        let h = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if h.is_null() {
            return;
        }
        let h = h as usize;
        std::thread::spawn(move || loop {
            if unsafe { WaitForSingleObject(h as HANDLE, 500) } == 0 {
                show_window(&ctx);
            }
        });
    }

    fn icon() -> Icon {
        // Meme dessin que l'icone de l'exe (assets/fouine.svg), en RGBA 64x64 brut.
        Icon::from_rgba(include_bytes!("../assets/fouine-64.rgba").to_vec(), 64, 64)
            .expect("icone valide")
    }

    /// Cree l'icone de la zone de notification (a appeler depuis le fil de l'interface).
    /// Clic gauche : afficher ; menu : Ouvrir Fouine / Quitter.
    pub fn create_tray(ctx: &egui::Context) -> Option<TrayIcon> {
        let menu = Menu::new();
        let open = MenuItem::new("Ouvrir Fouine", true, None);
        let quit_item = MenuItem::new("Quitter", true, None);
        menu.append_items(&[&open, &quit_item]).ok()?;
        let (open_id, quit_id) = (open.id().clone(), quit_item.id().clone());
        let c = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = e
            {
                show_window(&c);
            }
        }));
        let c = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == open_id {
                show_window(&c);
            } else if e.id == quit_id {
                quit(&c);
            }
        }));
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Fouine : indexation et surveillance des fichiers")
            .with_icon(icon())
            .build()
            .ok()
    }
}

#[cfg(not(windows))]
pub struct InstanceGuard;
#[cfg(not(windows))]
pub fn acquire_instance(_wait_s: u64) -> Option<InstanceGuard> {
    Some(InstanceGuard)
}
#[cfg(not(windows))]
pub fn listen_show(_ctx: egui::Context) {}
#[cfg(not(windows))]
pub struct TrayIcon;
#[cfg(not(windows))]
pub fn create_tray(_ctx: &egui::Context) -> Option<TrayIcon> {
    None
}
