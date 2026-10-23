//! `/etc/group` reader and dynamic group-membership updates.
//!
//! Platforms differ: shadow provides `usermod`, Alpine/BusyBox provides
//! `addgroup` + `delgroup`, and minimal systems may only have `gpasswd`.
//! Detection is runtime-only; no build-time distro assumption.

use std::path::Path;

use ursine_api::ApiError;

const USERMOD_CANDIDATES: [&str; 3] = ["/usr/sbin/usermod", "/sbin/usermod", "/usr/bin/usermod"];
const ADDGROUP_CANDIDATES: [&str; 3] =
    ["/usr/sbin/addgroup", "/sbin/addgroup", "/usr/bin/addgroup"];
const DELGROUP_CANDIDATES: [&str; 3] =
    ["/usr/sbin/delgroup", "/sbin/delgroup", "/usr/bin/delgroup"];
const GPASSWD_CANDIDATES: [&str; 2] = ["/usr/bin/gpasswd", "/bin/gpasswd"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupTool {
    Usermod(&'static str),
    AddGroup {
        add: &'static str,
        del: &'static str,
    },
    Gpasswd(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupCommand {
    program: &'static str,
    args: Vec<String>,
}

impl GroupTool {
    fn detect() -> Result<Self, ApiError> {
        Self::detect_with(|path| Path::new(path).is_file())
    }

    fn detect_with(exists: impl Fn(&str) -> bool) -> Result<Self, ApiError> {
        if let Some(program) = USERMOD_CANDIDATES.iter().find(|path| exists(path)) {
            return Ok(Self::Usermod(program));
        }
        if let (Some(add), Some(del)) = (
            ADDGROUP_CANDIDATES.iter().find(|path| exists(path)),
            DELGROUP_CANDIDATES.iter().find(|path| exists(path)),
        ) {
            return Ok(Self::AddGroup { add, del });
        }
        if let Some(program) = GPASSWD_CANDIDATES.iter().find(|path| exists(path)) {
            return Ok(Self::Gpasswd(program));
        }
        Err(ApiError::unavailable(
            "no group-management tool found",
            "install shadow (usermod) or busybox (addgroup/delgroup)",
        ))
    }

    fn replace_commands(
        &self,
        user: &str,
        desired: &[String],
        current: &[String],
    ) -> Vec<GroupCommand> {
        match self {
            Self::Usermod(program) => vec![GroupCommand {
                program,
                args: vec!["-G".into(), desired.join(","), user.into()],
            }],
            Self::AddGroup { add, del } => {
                let mut commands = Vec::new();
                for group in current.iter().filter(|group| !desired.contains(group)) {
                    commands.push(GroupCommand {
                        program: del,
                        args: vec![user.into(), group.clone()],
                    });
                }
                for group in desired.iter().filter(|group| !current.contains(group)) {
                    commands.push(GroupCommand {
                        program: add,
                        args: vec![user.into(), group.clone()],
                    });
                }
                commands
            }
            Self::Gpasswd(program) => {
                let mut commands = Vec::new();
                for group in current.iter().filter(|group| !desired.contains(group)) {
                    commands.push(GroupCommand {
                        program,
                        args: vec!["-d".into(), user.into(), group.clone()],
                    });
                }
                for group in desired.iter().filter(|group| !current.contains(group)) {
                    commands.push(GroupCommand {
                        program,
                        args: vec!["-a".into(), user.into(), group.clone()],
                    });
                }
                commands
            }
        }
    }

    fn add_command(&self, user: &str, group: &str) -> GroupCommand {
        match self {
            Self::Usermod(program) => GroupCommand {
                program,
                args: vec!["-aG".into(), group.into(), user.into()],
            },
            Self::AddGroup { add, .. } => GroupCommand {
                program: add,
                args: vec![user.into(), group.into()],
            },
            Self::Gpasswd(program) => GroupCommand {
                program,
                args: vec!["-a".into(), user.into(), group.into()],
            },
        }
    }
}

async fn run_commands(commands: &[GroupCommand]) -> Result<(), ApiError> {
    for command in commands {
        let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
        crate::modules::packages_nix::exec::run_root_program(
            command.program,
            &args,
            std::time::Duration::from_secs(60),
        )
        .await?;
    }
    Ok(())
}

pub(super) fn validate_username(name: &str) -> Result<(), ApiError> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "user name must match [a-z_][a-z0-9_-]{0,31}",
        ))
    }
}

pub(super) fn validate_fullname(name: &str) -> Result<(), ApiError> {
    if name.len() > 128 || name.chars().any(char::is_control) {
        return Err(ApiError::invalid("invalid full name"));
    }
    Ok(())
}

pub(super) fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.is_empty() || password.len() > 1024 {
        return Err(ApiError::invalid("invalid password"));
    }
    Ok(())
}

pub(super) fn parse_group_names(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| line.split(':').next())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn memberships(user: &str) -> Result<Vec<String>, ApiError> {
    let raw = std::fs::read_to_string("/etc/group")
        .map_err(|e| ApiError::internal(format!("read /etc/group: {e}")))?;
    Ok(raw
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let group = fields.next()?;
            fields.next()?;
            fields.next()?;
            let members = fields.next().unwrap_or_default();
            members
                .split(',')
                .any(|member| member == user)
                .then(|| group.to_string())
        })
        .collect())
}

pub(super) fn list_groups() -> Result<Vec<String>, ApiError> {
    let raw = std::fs::read_to_string("/etc/group")
        .map_err(|e| ApiError::internal(format!("read /etc/group: {e}")))?;
    Ok(parse_group_names(&raw))
}

pub(super) async fn set_groups(user: &str, groups: &[String]) -> Result<(), ApiError> {
    validate_username(user)?;
    let known = list_groups()?;
    let mut unique = Vec::new();
    for group in groups {
        if group.is_empty()
            || group.len() > 32
            || group.starts_with('-')
            || !group
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            || !known.iter().any(|known| known == group)
        {
            return Err(ApiError::invalid(format!(
                "unknown or invalid group: {group}"
            )));
        }
        if !unique.contains(group) {
            unique.push(group.clone());
        }
    }
    let current = memberships(user)?;
    let tool = GroupTool::detect()?;
    let commands = tool.replace_commands(user, &unique, &current);
    run_commands(&commands).await
}

pub(super) fn ensure_known_group(group: &str) -> Result<(), ApiError> {
    if group.is_empty()
        || group.len() > 32
        || group.starts_with('-')
        || !group
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err(ApiError::invalid("invalid group name"));
    }
    if !list_groups()?.iter().any(|known| known == group) {
        return Err(ApiError::invalid(format!("unknown group: {group}")));
    }
    Ok(())
}

pub(super) async fn add_group(user: &str, group: &str) -> Result<(), ApiError> {
    validate_username(user)?;
    ensure_known_group(group)?;
    let tool = GroupTool::detect()?;
    run_commands(&[tool.add_command(user, group)]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_group_names() {
        assert_eq!(
            parse_group_names("root:x:0:\nwheel:x:10:a,b\n"),
            vec!["root", "wheel"]
        );
    }

    #[test]
    fn validates_usernames() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("_svc-1").is_ok());
        assert!(validate_username("Alice").is_err());
        assert!(validate_username("-root").is_err());
        assert!(validate_username("a/b").is_err());
    }

    #[test]
    fn prefers_usermod_then_busybox_then_gpasswd() {
        let usermod = GroupTool::detect_with(|path| path == "/usr/sbin/usermod").unwrap();
        assert!(matches!(usermod, GroupTool::Usermod("/usr/sbin/usermod")));

        let busybox = GroupTool::detect_with(|path| {
            matches!(path, "/usr/sbin/addgroup" | "/usr/sbin/delgroup")
        })
        .unwrap();
        assert!(matches!(
            busybox,
            GroupTool::AddGroup {
                add: "/usr/sbin/addgroup",
                del: "/usr/sbin/delgroup"
            }
        ));

        let gpasswd = GroupTool::detect_with(|path| path == "/usr/bin/gpasswd").unwrap();
        assert!(matches!(gpasswd, GroupTool::Gpasswd("/usr/bin/gpasswd")));
        assert!(GroupTool::detect_with(|_| false).is_err());
    }

    #[test]
    fn usermod_replacement_is_one_exact_command() {
        let tool = GroupTool::detect_with(|path| path == "/usr/sbin/usermod").unwrap();
        let commands = tool.replace_commands(
            "alice",
            &["wheel".into(), "network".into()],
            &["wheel".into()],
        );
        assert_eq!(
            commands,
            vec![GroupCommand {
                program: "/usr/sbin/usermod",
                args: vec!["-G".into(), "wheel,network".into(), "alice".into()],
            }]
        );
        assert_eq!(
            tool.replace_commands("alice", &[], &["wheel".into()]),
            vec![GroupCommand {
                program: "/usr/sbin/usermod",
                args: vec!["-G".into(), String::new(), "alice".into()],
            }]
        );
    }

    #[test]
    fn alpine_tools_diff_memberships() {
        let tool = GroupTool::detect_with(|path| {
            matches!(path, "/usr/sbin/addgroup" | "/usr/sbin/delgroup")
        })
        .unwrap();
        let commands = tool.replace_commands(
            "alice",
            &["network".into()],
            &["wheel".into(), "network".into()],
        );
        assert_eq!(
            commands,
            vec![GroupCommand {
                program: "/usr/sbin/delgroup",
                args: vec!["alice".into(), "wheel".into()],
            }]
        );
    }
}
