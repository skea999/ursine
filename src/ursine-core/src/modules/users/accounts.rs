//! System accounts without D-Bus — the Webmin pattern: read
//! `/etc/passwd` directly, create/delete through the system's own user
//! CLI (shadow `useradd`/`userdel`, `BusyBox` `adduser`/`deluser`), and
//! write passwords straight into `/etc/shadow`.
//!
//! Every command runs as root with an absolute trusted path and argv
//! arguments only.

use std::path::Path;

use ursine_api::{ApiError, UserSummary};

const USERADD: [&str; 3] = ["/usr/sbin/useradd", "/sbin/useradd", "/usr/bin/useradd"];
const CHPASSWD: [&str; 3] = ["/usr/sbin/chpasswd", "/sbin/chpasswd", "/usr/bin/chpasswd"];
const ADDUSER: [&str; 3] = ["/usr/sbin/adduser", "/sbin/adduser", "/usr/bin/adduser"];
const USERDEL: [&str; 3] = ["/usr/sbin/userdel", "/sbin/userdel", "/usr/bin/userdel"];
const DELUSER: [&str; 3] = ["/usr/sbin/deluser", "/sbin/deluser", "/usr/bin/deluser"];

pub(super) type User = UserSummary;

fn find(candidates: &[&'static str]) -> Option<&'static str> {
    candidates
        .iter()
        .copied()
        .find(|path| Path::new(path).is_file())
}

fn no_tool(what: &str) -> ApiError {
    ApiError::unavailable(what, "apk add shadow  # provides useradd/userdel")
}

/// Resolve a login name for `uid` from `/etc/passwd` (system uids are
/// rejected by the module before this is called).
fn user_name_for_uid(uid: u32) -> Result<String, ApiError> {
    let raw = read_passwd()?;
    raw.lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let uid_field = fields.next()?;
            Some((name, uid_field))
        })
        .find_map(|(name, uid_field)| {
            (uid_field.parse::<u32>().ok()? == uid).then(|| name.to_string())
        })
        .ok_or_else(|| ApiError::new(ursine_api::ErrorCode::NotFound, "user not found"))
}

fn read_passwd() -> Result<String, ApiError> {
    std::fs::read_to_string("/etc/passwd")
        .map_err(|e| ApiError::internal(format!("read /etc/passwd: {e}")))
}

/// Normal login users (uid 1000..60000) straight from `/etc/passwd`.
pub(super) fn list() -> Result<Vec<User>, ApiError> {
    let raw = read_passwd()?;
    let mut users: Vec<User> = raw
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let uid: u32 = fields.next()?.parse().ok()?;
            let _gid = fields.next()?;
            let fullname = fields.next()?.to_string();
            let home = fields.next()?.to_string();
            let shell = fields.next()?.to_string();
            (1000..60000).contains(&uid).then(|| User {
                uid,
                name: name.to_string(),
                fullname,
                shell,
                home,
                groups: Vec::new(),
            })
        })
        .collect();
    // One /etc/group read for the whole listing (not per user).
    let group_raw = std::fs::read_to_string("/etc/group")
        .map_err(|e| ApiError::internal(format!("read /etc/group: {e}")))?;
    for user in &mut users {
        user.groups = super::groups::memberships_from(&group_raw, &user.name);
    }
    users.sort_by_key(|user| user.uid);
    Ok(users)
}

/// Create a locked account (password is set later through
/// [`set_password`]). Shadow's `useradd` first, `BusyBox` `adduser` as the
/// fallback; both create the home directory and the primary group with
/// the same name as the user.
pub(super) async fn create(name: &str, fullname: &str) -> Result<(), ApiError> {
    let home = format!("/home/{name}");
    if let Some(program) = find(&USERADD) {
        run(
            program,
            &[
                "-m", "-d", &home, "-c", fullname, "-s", "/bin/sh", "-U", name,
            ],
        )
        .await?;
    } else if let Some(program) = find(&ADDUSER) {
        // `BusyBox` adduser only (Debian's Perl adduser has a different
        // syntax, but Debian always ships shadow's useradd).
        run(
            program,
            &["-D", "-g", fullname, "-s", "/bin/sh", "-h", &home, name],
        )
        .await?;
    } else {
        return Err(no_tool("no user creation tool found"));
    }
    // Explicit lock: never trust a tool's default shadow field (an empty
    // field would mean a passwordless account until the GUI sets one).
    lock_password(name).await
}

/// Delete an account; `remove_files` also drops the home directory
/// (`userdel -r` / `BusyBox` `deluser -r`).
pub(super) async fn delete(uid: u32, remove_files: bool) -> Result<(), ApiError> {
    let name = user_name_for_uid(uid)?;
    if let Some(program) = find(&USERDEL) {
        let mut args: Vec<&str> = Vec::new();
        if remove_files {
            args.push("-r");
        }
        args.push(&name);
        run(program, &args).await
    } else if let Some(program) = find(&DELUSER) {
        let mut args: Vec<&str> = Vec::new();
        if remove_files {
            args.push("-r");
        }
        args.push(&name);
        run(program, &args).await
    } else {
        Err(no_tool("no user deletion tool found"))
    }
}

/// Replace an account's shadow password field through `chpasswd -e`:
/// the CLI takes the lock files itself, works identically on shadow and
/// `BusyBox` (no `usermod -p` plaintext trap), and the value never
/// leaves our process as plaintext (the hash was computed in-process).
async fn replace_password_field(name: &str, field: &str) -> Result<(), ApiError> {
    let program = find(&CHPASSWD).ok_or_else(|| no_tool("chpasswd not found"))?;
    // No trailing newline: the quoted-stdin runner appends one.
    let input = format!("{name}:{field}");
    crate::modules::packages_nix::exec::run_root_program_quoted_stdin(
        program,
        &[],
        std::time::Duration::from_secs(60),
        &input,
    )
    .await
    .map(|_| ())
}

/// Write a pre-computed hash (`$6$…`) into the account's password field.
pub(super) async fn set_password(uid: u32, hashed: &str) -> Result<(), ApiError> {
    let name = user_name_for_uid(uid)?;
    replace_password_field(&name, hashed).await
}

/// Set the account's shadow password field to `!` (locked).
async fn lock_password(name: &str) -> Result<(), ApiError> {
    replace_password_field(name, "!").await
}

/// Quoted-value argv runner: full names carry spaces (`John Doe`), which
/// the package-atom guard would reject.
async fn run(program: &str, args: &[&str]) -> Result<(), ApiError> {
    crate::modules::packages_nix::exec::run_root_program_value(
        program,
        args,
        std::time::Duration::from_secs(60),
    )
    .await
    .map(|_| ())
}
