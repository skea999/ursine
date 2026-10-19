//! Module handlers. v0.1: gating + wiring for `system`, `capabilities`,
//! `config`; every other action returns `NotImplemented` (but still runs
//! `ensure_available` first, so gating is exercised end to end).

use std::path::Path;

use serde_json::json;
use ursine_api::{Action, ApiError, Capability, ModuleId, Response, UrsineConfig};

pub mod packages_nix;

use crate::capabilities::{module_switch, Registry};

/// Everything a handler may touch (no daemon internals).
pub struct ModuleCtx<'a> {
    pub caps: &'a Registry,
    pub config: &'a UrsineConfig,
    pub config_path: &'a Path,
    /// Resolved target user (defaults to the principal).
    pub as_user: &'a str,
}

/// Gating requirements per module.
fn required_caps(module: ModuleId) -> &'static [Capability] {
    match module {
        ModuleId::PackagesNix => &[Capability::Nix],
        ModuleId::Snapshots => &[Capability::Btrbk, Capability::Btrfs],
        ModuleId::Users => &[Capability::Accountsservice],
        ModuleId::Niri => &[Capability::Niri],
        ModuleId::System | ModuleId::Config | ModuleId::Capabilities => &[],
    }
}

/// Route a validated call. `NotImplemented` for everything beyond v0.1 wiring.
pub async fn handle(
    module: ModuleId,
    action: &Action,
    ctx: &ModuleCtx<'_>,
) -> Result<serde_json::Value, ApiError> {
    // Binding rule: capability gate before ANY module action.
    for &cap in required_caps(module) {
        let switch = module_switch(&ctx.config.modules, cap);
        ctx.caps.ensure_available(switch, &[cap])?;
    }

    match (module, action) {
        (ModuleId::System, Action::Status) => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": ursine_api::PROTOCOL_VERSION,
            "auth": if cfg!(feature = "pam") { "pam" } else { "stub" },
        })),
        (ModuleId::Capabilities, Action::List) => Ok(json!(ctx.caps.reports())),
        (ModuleId::Capabilities, Action::Refresh) => {
            ctx.caps.refresh();
            Ok(json!(ctx.caps.reports()))
        }
        (ModuleId::Config, Action::Init) => {
            let caps = ctx.caps.reports();
            crate::config::write_template(ctx.config_path, &caps)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            Ok(json!({ "written": ctx.config_path.display().to_string() }))
        }

        (ModuleId::PackagesNix, action) => packages_nix::handle(action, ctx).await,

        // v1 module surface: wired later milestone by milestone.
        (ModuleId::Snapshots, _) => Err(ApiError::not_implemented("snapshots")),
        (ModuleId::Users, _) => Err(ApiError::not_implemented("users")),
        (ModuleId::Niri, _) => Err(ApiError::not_implemented("niri")),

        _ => Err(ApiError::invalid(format!(
            "action {} is not valid for module {}",
            action_name(action),
            module.as_str()
        ))),
    }
}

/// Stable action name for audit lines.
pub fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Status => "status",
        Action::Init => "init",
        Action::List => "list",
        Action::Refresh => "refresh",
        Action::ListInstalled => "list_installed",
        Action::Search { .. } => "search",
        Action::Install { .. } => "install",
        Action::Remove { .. } => "remove",
        Action::Upgrade { .. } => "upgrade",
        Action::Generations => "generations",
        Action::Rollback { .. } => "rollback",
        Action::WipeHistory { .. } => "wipe_history",
        Action::DiffClosures { .. } => "diff_closures",
        Action::StoreGc => "store_gc",
        Action::PruneGenerations { .. } => "prune_generations",
        Action::ListSnapshots => "list_snapshots",
        Action::Stats => "stats",
        Action::CreateSnapshot { .. } => "create_snapshot",
        Action::RestoreSnapshot { .. } => "restore_snapshot",
        Action::GetBtrbkConfig => "get_btrbk_config",
        Action::InitBtrbkConfig => "init_btrbk_config",
        Action::ListUsers => "list_users",
        Action::CreateUser { .. } => "create_user",
        Action::DeleteUser { .. } => "delete_user",
        Action::SetPassword { .. } => "set_password",
        Action::ListGroups => "list_groups",
        Action::SetGroups { .. } => "set_groups",
        Action::GetNiriConfig => "get_niri_config",
        Action::ValidateNiriConfig { .. } => "validate_niri_config",
        Action::SetNiriSection { .. } => "set_niri_section",
    }
}

/// Response helper (kept here so dispatch stays lean).
pub fn ok(id: Option<u64>, value: serde_json::Value) -> Response {
    Response::ok(id, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::checks::Sysinfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct AllMissing;
    impl Sysinfo for AllMissing {
        fn path_exists(&self, _p: &Path) -> bool {
            false
        }
        fn read_trimmed(&self, _p: &Path) -> Option<String> {
            None
        }
        fn cmd_in_path(&self, _c: &str) -> bool {
            false
        }
    }

    fn ctx_with<S: Sysinfo + 'static>(sys: S) -> (Registry, UrsineConfig, PathBuf) {
        (
            Registry::new(Arc::new(sys)),
            UrsineConfig::default(),
            PathBuf::from("/tmp/none.toml"),
        )
    }

    #[tokio::test]
    async fn system_status_reports_stub_auth_in_dev() {
        let (caps, cfg, path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: &cfg,
            config_path: &path,
            as_user: "u",
        };
        let v = handle(ModuleId::System, &Action::Status, &ctx)
            .await
            .unwrap();
        assert_eq!(
            v["auth"],
            if cfg!(feature = "pam") { "pam" } else { "stub" }
        );
        assert_eq!(v["protocol_version"], ursine_api::PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn capabilities_list_returns_all_six() {
        let (caps, cfg, path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: &cfg,
            config_path: &path,
            as_user: "u",
        };
        let v = handle(ModuleId::Capabilities, &Action::List, &ctx)
            .await
            .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn package_actions_gated_before_notimplemented() {
        let (caps, cfg, path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: &cfg,
            config_path: &path,
            as_user: "u",
        };
        let err = handle(ModuleId::PackagesNix, &Action::ListInstalled, &ctx)
            .await
            .unwrap_err();
        // nix missing on fake machine -> gating error wins over NotImplemented
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
        assert!(err.fix.is_some());
    }

    #[tokio::test]
    async fn disabled_module_is_unavailable_even_if_forced_off() {
        let (caps, mut cfg, path) = ctx_with(AllMissing);
        cfg.modules.packages_nix = ursine_api::TriState::Off;
        let ctx = ModuleCtx {
            caps: &caps,
            config: &cfg,
            config_path: &path,
            as_user: "u",
        };
        let err = handle(
            ModuleId::PackagesNix,
            &Action::Search {
                query: "hello".into(),
            },
            &ctx,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn invalid_module_action_combo_is_rejected() {
        let (caps, cfg, path) = ctx_with(AllMissing);
        let ctx = ModuleCtx {
            caps: &caps,
            config: &cfg,
            config_path: &path,
            as_user: "u",
        };
        let err = handle(
            ModuleId::System,
            &Action::Install {
                package: "x".into(),
            },
            &ctx,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ursine_api::ErrorCode::Invalid);
    }
}
