//! accountsservice D-Bus access (root daemon scope).

use ursine_api::{ApiError, UserSummary};
use zbus::Proxy;

const SERVICE: &str = "org.freedesktop.Accounts";
const ROOT: &str = "/org/freedesktop/Accounts";
const IFACE: &str = "org.freedesktop.Accounts";
const USER_IFACE: &str = "org.freedesktop.Accounts.User";

pub(super) type User = UserSummary;

async fn connection() -> Result<zbus::Connection, ApiError> {
    zbus::Connection::system().await.map_err(|e| {
        ApiError::unavailable("accountsservice unavailable", format!("start dbus: {e}"))
    })
}

async fn root_proxy(conn: &zbus::Connection) -> Result<Proxy<'static>, ApiError> {
    Proxy::new(conn, SERVICE, ROOT, IFACE)
        .await
        .map_err(|e| ApiError::internal(format!("accounts proxy: {e}")))
}

async fn user_proxy(conn: &zbus::Connection, path: &str) -> Result<Proxy<'static>, ApiError> {
    Proxy::new(conn, SERVICE, path.to_string(), USER_IFACE)
        .await
        .map_err(|e| ApiError::internal(format!("user proxy: {e}")))
}

pub(super) async fn list() -> Result<Vec<User>, ApiError> {
    let conn = connection().await?;
    let proxy = root_proxy(&conn).await?;
    let paths: Vec<zbus::zvariant::OwnedObjectPath> = proxy
        .call("ListCachedUsers", &())
        .await
        .map_err(|e| ApiError::internal(format!("list users: {e}")))?;

    let mut users = Vec::new();
    for path in paths {
        let user = user_proxy(&conn, path.as_str()).await?;
        let uid: u64 = user
            .get_property("Uid")
            .await
            .map_err(|e| ApiError::internal(format!("user uid: {e}")))?;
        if !(1000..60000).contains(&uid) {
            continue;
        }
        let name: String = property(&user, "UserName").await?;
        users.push(User {
            uid: u32::try_from(uid).unwrap_or(u32::MAX),
            groups: super::groups::memberships(&name)?,
            name,
            fullname: property(&user, "RealName").await?,
            shell: property(&user, "Shell").await?,
            home: property(&user, "HomeDirectory").await?,
        });
    }
    users.sort_by_key(|user| user.uid);
    Ok(users)
}

async fn property(proxy: &Proxy<'_>, name: &str) -> Result<String, ApiError> {
    proxy
        .get_property(name)
        .await
        .map_err(|e| ApiError::internal(format!("user property {name}: {e}")))
}

async fn path_for_uid(conn: &zbus::Connection, uid: u32) -> Result<String, ApiError> {
    let proxy = root_proxy(conn).await?;
    let path: zbus::zvariant::OwnedObjectPath = proxy
        .call("FindUserById", &(u64::from(uid),))
        .await
        .map_err(|_| ApiError::new(ursine_api::ErrorCode::NotFound, "user not found"))?;
    Ok(path.as_str().to_string())
}

pub(super) async fn create(
    name: &str,
    fullname: &str,
    admin: bool,
    hashed: &str,
) -> Result<(), ApiError> {
    let conn = connection().await?;
    let proxy = root_proxy(&conn).await?;
    let account_type: i32 = i32::from(admin);
    let path: zbus::zvariant::OwnedObjectPath = proxy
        .call("CreateUser", &(name, fullname, account_type))
        .await
        .map_err(|e| ApiError::internal(format!("create user: {e}")))?;
    let user = user_proxy(&conn, path.as_str()).await?;
    user.call::<_, _, ()>("SetPassword", &(hashed,))
        .await
        .map_err(|e| ApiError::internal(format!("lock new user password: {e}")))?;
    Ok(())
}

pub(super) async fn delete(uid: u32, remove_files: bool) -> Result<(), ApiError> {
    let conn = connection().await?;
    let proxy = root_proxy(&conn).await?;
    proxy
        .call::<_, _, ()>("DeleteUser", &(u64::from(uid), remove_files))
        .await
        .map_err(|e| ApiError::internal(format!("delete user: {e}")))?;
    Ok(())
}

pub(super) async fn set_password(uid: u32, hashed: &str) -> Result<(), ApiError> {
    let conn = connection().await?;
    let path = path_for_uid(&conn, uid).await?;
    let proxy = user_proxy(&conn, &path).await?;
    proxy
        .call::<_, _, ()>("SetPassword", &(hashed,))
        .await
        .map_err(|e| ApiError::internal(format!("set password: {e}")))?;
    Ok(())
}
