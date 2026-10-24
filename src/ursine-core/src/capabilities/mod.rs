//! Capability detection: the machine's reality, checked live, never persisted.

pub mod checks;

use std::sync::Arc;

use ursine_api::{Capability, CapabilityReport, ModulesConfig, TriState};

use checks::{check_capability, Sysinfo, SystemSysinfo};

/// Registry of detection results. Cheap checks re-run per request; the heavy
/// `Btrfs` probe is cached until `refresh()`.
pub struct Registry {
    sysinfo: Arc<dyn Sysinfo>,
    btrfs_cached: Arc<std::sync::Mutex<Option<bool>>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry").finish_non_exhaustive()
    }
}

impl Registry {
    pub fn new(sysinfo: Arc<dyn Sysinfo>) -> Self {
        Self {
            sysinfo,
            btrfs_cached: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn system() -> Self {
        Self::new(Arc::new(SystemSysinfo))
    }

    /// Single capability check (cheap; safe to call per request).
    pub fn check(&self, cap: Capability) -> bool {
        check_capability(&*self.sysinfo, cap)
    }

    /// Full report list for `capabilities.list`.
    pub fn reports(&self) -> Vec<CapabilityReport> {
        Capability::ALL.iter().map(|&c| self.report(c)).collect()
    }

    pub fn report(&self, cap: Capability) -> CapabilityReport {
        if self.check(cap) {
            CapabilityReport::available(cap)
        } else {
            let (reason, fix) = checks::hints(cap);
            CapabilityReport::unavailable(cap, reason, fix)
        }
    }

    /// Refreshes cached probes (`capabilities.refresh`).
    pub fn refresh(&self) {
        *self
            .btrfs_cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Resolve the tri-state switch for a module and enforce availability.
    /// Called before EVERY module action (binding rule).
    pub fn ensure_available(
        &self,
        switch: TriState,
        required: &[Capability],
    ) -> Result<(), ursine_api::ApiError> {
        if switch == TriState::Off {
            return Err(ursine_api::ApiError::new(
                ursine_api::ErrorCode::Unavailable,
                "module disabled by configuration",
            ));
        }
        for &cap in required {
            if !self.check(cap) {
                let (reason, fix) = checks::hints(cap);
                if switch == TriState::On {
                    tracing::warn!(capability = cap.as_str(), "forced on but missing");
                }
                return Err(ursine_api::ApiError::unavailable(reason, fix));
            }
        }
        Ok(())
    }
}

/// Switch for a capability's owning module.
pub fn module_switch(modules: &ModulesConfig, cap: Capability) -> TriState {
    match cap {
        Capability::Btrbk | Capability::Btrfs => modules.snapshots,
        Capability::Nix => modules.packages_nix,
        Capability::Apk => modules.packages_apk,
        Capability::Flatpak => modules.packages_flatpak,
        Capability::Cups => modules.printers,
        Capability::Accountsservice => modules.users,
        Capability::Niri => modules.niri,
        Capability::Dinit => TriState::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct Fake {
        paths: Vec<String>,
        cmds: Vec<String>,
        proc1: Option<String>,
    }

    impl Sysinfo for Fake {
        fn path_exists(&self, p: &Path) -> bool {
            self.paths.iter().any(|x| Path::new(x) == p)
        }
        fn read_trimmed(&self, p: &Path) -> Option<String> {
            if p == Path::new("/proc/1/comm") {
                self.proc1.clone()
            } else {
                None
            }
        }
        fn cmd_in_path(&self, cmd: &str) -> bool {
            self.cmds.iter().any(|c| c == cmd)
        }
    }

    fn registry(f: Fake) -> Registry {
        Registry::new(Arc::new(f))
    }

    #[test]
    fn detects_btrbk_and_nix() {
        let r = registry(Fake {
            paths: vec!["/nix/store".into(), "/run/dbus/system_bus_socket".into()],
            cmds: vec!["btrbk".into(), "nix".into(), "niri".into()],
            proc1: Some("dinit".into()),
        });
        assert!(r.check(Capability::Btrbk));
        assert!(r.check(Capability::Nix));
        assert!(r.check(Capability::Niri));
        assert!(r.check(Capability::Dinit));
        assert!(r.check(Capability::Accountsservice));
    }

    #[test]
    fn missing_backends_report_reason_and_fix() {
        let r = registry(Fake {
            paths: vec![],
            cmds: vec![],
            proc1: Some("init".into()),
        });
        let rep = r.report(Capability::Btrbk);
        assert!(!rep.available);
        assert!(rep.reason.as_deref().unwrap().contains("btrbk"));
        assert!(rep.fix.as_deref().unwrap().contains("apk add"));
        // nix alone (store without binary) is not enough
        let r2 = registry(Fake {
            paths: vec!["/nix/store".into()],
            cmds: vec![],
            proc1: None,
        });
        assert!(!r2.check(Capability::Nix));
    }

    #[test]
    fn ensure_available_matrix() {
        let present = registry(Fake {
            paths: vec![],
            cmds: vec!["btrbk".into()],
            proc1: None,
        });
        // auto + present -> ok
        assert!(present
            .ensure_available(TriState::Auto, &[Capability::Btrbk])
            .is_ok());
        // off -> always unavailable
        let err = present
            .ensure_available(TriState::Off, &[Capability::Btrbk])
            .unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        // on + missing -> unavailable (warn logged)
        let missing = registry(Fake {
            paths: vec![],
            cmds: vec![],
            proc1: None,
        });
        let err = missing
            .ensure_available(TriState::On, &[Capability::Btrbk])
            .unwrap_err();
        assert!(err.fix.is_some());
    }

    #[test]
    fn switch_for_maps_modules() {
        let m = ModulesConfig {
            snapshots: TriState::Off,
            ..Default::default()
        };
        assert_eq!(module_switch(&m, Capability::Btrbk), TriState::Off);
        assert_eq!(module_switch(&m, Capability::Btrfs), TriState::Off);
        assert!(module_switch(&m, Capability::Nix).is_auto());
    }
}
