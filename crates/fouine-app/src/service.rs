//! Service Windows « Fouine » : indexe et surveille en permanence, sans session ouverte ni fenetre.
//! Le service lance `fouine.exe --service` ; il garde l'index a jour dans `%ProgramData%\Fouine`,
//! que l'interface relit. Installer / desinstaller exige les droits d'administrateur.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::settings::{self, Settings};

pub const NAME: &str = "Fouine";

/// Etat du service vu de l'interface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Absent,
    Stopped,
    Running,
}

fn manager(access: ServiceManagerAccess) -> windows_service::Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access)
}

/// Lit l'etat du service (ne demande aucun droit particulier).
pub fn state() -> State {
    let Ok(m) = manager(ServiceManagerAccess::CONNECT) else {
        return State::Absent;
    };
    match m
        .open_service(NAME, ServiceAccess::QUERY_STATUS)
        .and_then(|s| s.query_status())
    {
        Ok(st) if st.current_state == ServiceState::Stopped => State::Stopped,
        Ok(_) => State::Running,
        Err(_) => State::Absent,
    }
}

/// Installe le service (demarrage automatique) avec les dossiers et exclusions actuels, puis le demarre.
pub fn install(current: &Settings) -> Result<(), String> {
    let dir = settings::service_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier des donnees : {e}"))?;
    current.save_in(&dir);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let m = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE).map_err(
        |e| {
            format!(
                "acces au gestionnaire de services refuse (droits d'administrateur requis) : {e}"
            )
        },
    )?;
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "Fouine : indexation des fichiers".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec!["--service".into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let svc = m
        .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
        .map_err(|e| format!("creation du service : {e}"))?;
    let _ = svc
        .set_description("Garde a jour l'index de Fouine (fichiers ajoutes, modifies, supprimes).");
    svc.start::<&str>(&[])
        .map_err(|e| format!("demarrage du service : {e}"))
}

/// Arrete puis supprime le service. Les donnees de `%ProgramData%\Fouine` sont conservees.
pub fn uninstall() -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT)
        .map_err(|e| format!("gestionnaire de services : {e}"))?;
    let svc = m
        .open_service(
            NAME,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("service introuvable ou droits insuffisants : {e}"))?;
    if svc
        .query_status()
        .is_ok_and(|s| s.current_state != ServiceState::Stopped)
    {
        let _ = svc.stop();
        for _ in 0..50 {
            if svc
                .query_status()
                .is_ok_and(|s| s.current_state == ServiceState::Stopped)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    svc.delete().map_err(|e| format!("suppression : {e}"))
}

define_windows_service!(ffi_service_main, service_main);

/// Point d'entree de `fouine.exe --service` : rend la main au gestionnaire de services.
pub fn run_dispatcher() -> Result<(), windows_service::Error> {
    service_dispatcher::start(NAME, ffi_service_main)
}

fn service_main(_args: Vec<OsString>) {
    let _ = run_service();
}

fn run_service() -> windows_service::Result<()> {
    settings::set_home(settings::service_dir());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let handler = service_control_handler::register(NAME, move |c| match c {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            flag.store(true, Ordering::Relaxed);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let status = |state, accept, wait: u32| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(u64::from(wait)),
        process_id: None,
    };
    handler.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    ))?;
    run_daemon(&stop);
    handler.set_service_status(status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        0,
    ))
}

/// Boucle d'indexation et de surveillance sur les reglages du dossier de donnees courant.
pub fn run_daemon(stop: &AtomicBool) {
    let s = Settings::load();
    let cfg = fouine_core::daemon::DaemonConfig {
        roots: s.effective_roots(),
        exclusions: s.exclusions(),
        db: s.db_path(),
        save_every: Duration::from_secs(30),
    };
    fouine_core::daemon::run(&cfg, stop, &|m| crate::log(m));
}
