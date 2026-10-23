//! users — accountsservice D-Bus plus root `usermod` for groups.

mod accounts;
mod groups;

use serde_json::json;
use ursine_api::{Action, ApiError};

use super::ModuleCtx;

pub async fn handle(action: &Action, ctx: &ModuleCtx<'_>) -> Result<serde_json::Value, ApiError> {
    match action {
        Action::ListUsers => Ok(json!({ "users": accounts::list().await? })),
        Action::CreateUser {
            name,
            fullname,
            admin,
        } => {
            groups::validate_username(name)?;
            groups::validate_fullname(fullname)?;
            if *admin {
                groups::ensure_known_group("wheel")?;
            }
            // Locked password; the GUI sets it explicitly after creation.
            accounts::create(name, fullname, *admin, "!").await?;
            if *admin {
                groups::add_group(name, "wheel").await?;
            }
            Ok(json!({"created": name}))
        }
        Action::DeleteUser { uid, remove_files } => {
            reject_system_uid(*uid)?;
            accounts::delete(*uid, *remove_files).await?;
            Ok(json!({"deleted": uid}))
        }
        Action::SetPassword { uid, new_password } => {
            reject_system_uid(*uid)?;
            groups::validate_password(new_password)?;
            let password = new_password.clone();
            let hashed = tokio::task::spawn_blocking(move || {
                pwhash::sha512_crypt::hash_with(
                    pwhash::HashSetup {
                        salt: None,
                        rounds: Some(500_000),
                    },
                    password,
                )
            })
            .await
            .map_err(|_| ApiError::internal("password hashing task failed"))?
            .map_err(|_| ApiError::internal("password hashing failed"))?;
            accounts::set_password(*uid, &hashed).await?;
            Ok(json!({"password_set": uid}))
        }
        Action::ListGroups => Ok(json!({"groups": groups::list_groups()?})),
        Action::SetGroups {
            uid,
            groups: new_groups,
        } => {
            reject_system_uid(*uid)?;
            let user = accounts::list()
                .await?
                .into_iter()
                .find(|user| user.uid == *uid)
                .ok_or_else(|| ApiError::new(ursine_api::ErrorCode::NotFound, "user not found"))?;
            if user.name == ctx.as_user
                && groups::memberships(&user.name)?
                    .iter()
                    .any(|group| group == "wheel")
                && !new_groups.iter().any(|group| group == "wheel")
            {
                return Err(ApiError::new(
                    ursine_api::ErrorCode::Forbidden,
                    "cannot remove your own wheel membership",
                ));
            }
            groups::set_groups(&user.name, new_groups).await?;
            Ok(json!({"groups_set": uid}))
        }
        _ => Err(ApiError::invalid("action is not valid for module users")),
    }
}

fn reject_system_uid(uid: u32) -> Result<(), ApiError> {
    if !(1000..60000).contains(&uid) {
        return Err(ApiError::new(
            ursine_api::ErrorCode::Forbidden,
            "system and nobody-style users cannot be modified",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_uids_are_rejected() {
        assert!(reject_system_uid(0).is_err());
        assert!(reject_system_uid(999).is_err());
        assert!(reject_system_uid(1000).is_ok());
    }
}
