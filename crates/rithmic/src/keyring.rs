//! The password, in the desktop keyring.
//!
//! Through the Secret Service, which gnome-keyring provides on Omarchy, and
//! never anywhere else: the settings database is backed up and can be copied
//! to another machine, and a password must go with neither.
//!
//! One item per login, found by the login's id, which is the app's own name
//! for it and stays the same when its user ID or system is retyped. The user
//! and system are on the item too, so that `secret-tool` can find it by hand:
//!
//! ```text
//! application=omacharts service=rithmic login=<id> system=<system> user=<user ID>
//! ```
//!
//! An item saved before there could be several logins has no `login`, only
//! the user and system. It is still found, by those, for the login they
//! belong to, until that login saves a password of its own.
//!
//! The blocking functions here run a small runtime of their own for the one
//! call, so they can be called from anywhere but inside a tokio runtime.

use crate::Error;

const LABEL: &str = "Omacharts: Rithmic login";

fn of_login(login: &str) -> [(&'static str, &str); 3] {
    [("application", "omacharts"), ("service", "rithmic"), ("login", login)]
}

fn of_user<'a>(user: &'a str, system: &'a str) -> [(&'static str, &'a str); 4] {
    [("application", "omacharts"), ("service", "rithmic"), ("system", system), ("user", user)]
}

fn unreadable(error: oo7::Error) -> Error {
    Error::NoLogin(format!("the keyring could not be used: {error}"))
}

async fn open() -> Result<oo7::Keyring, Error> {
    let keyring = oo7::Keyring::new().await.map_err(unreadable)?;
    keyring.unlock().await.map_err(unreadable)?;
    Ok(keyring)
}

/// The items saved before logins had ids, for `user` on `system`.
async fn unnamed(keyring: &oo7::Keyring, user: &str, system: &str) -> Result<Vec<oo7::Item>, Error> {
    let mut found = Vec::new();
    for item in keyring.search_items(&of_user(user, system)).await.map_err(unreadable)? {
        let attributes = item.attributes().await.map_err(unreadable)?;
        if !attributes.contains_key("login") {
            found.push(item);
        }
    }
    Ok(found)
}

async fn secret_of(item: &oo7::Item) -> Result<Option<String>, Error> {
    item.unlock().await.map_err(unreadable)?;
    let secret = item.secret().await.map_err(unreadable)?;
    match std::str::from_utf8(&secret) {
        Ok(text) if !text.is_empty() => Ok(Some(text.to_string())),
        _ => Ok(None),
    }
}

/// The password saved for the login `login`, which is `user` on `system`,
/// if there is one.
pub(crate) async fn password(login: &str, user: &str, system: &str) -> Result<Option<String>, Error> {
    let keyring = open().await?;
    let items = keyring.search_items(&of_login(login)).await.map_err(unreadable)?;
    if let Some(item) = items.first() {
        return secret_of(item).await;
    }
    match unnamed(&keyring, user, system).await?.first() {
        Some(item) => secret_of(item).await,
        None => Ok(None),
    }
}

fn blocking<T>(work: impl std::future::Future<Output = Result<T, Error>>) -> Result<T, Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::NoLogin(format!("the keyring could not be used: {error}")))?
        .block_on(work)
}

/// Save `password` for the login `login`, `user` on `system`, replacing
/// whatever was there.
pub fn save(login: &str, user: &str, system: &str, password: &str) -> Result<(), Error> {
    blocking(async {
        let keyring = open().await?;
        let attributes = [
            ("application", "omacharts"),
            ("service", "rithmic"),
            ("login", login),
            ("system", system),
            ("user", user),
        ];
        keyring
            .create_item(LABEL, &attributes, oo7::Secret::text(password), true)
            .await
            .map_err(unreadable)
    })
}

/// Is a password saved for the login `login`, `user` on `system`?
pub fn holds(login: &str, user: &str, system: &str) -> Result<bool, Error> {
    blocking(async { Ok(password(login, user, system).await?.is_some()) })
}

/// Forget the password saved for the login `login`, and one saved for
/// `user` on `system` before logins had ids. True if there was one.
pub fn forget(login: &str, user: &str, system: &str) -> Result<bool, Error> {
    blocking(async {
        let keyring = open().await?;
        let mut found = !keyring.search_items(&of_login(login)).await.map_err(unreadable)?.is_empty();
        if found {
            keyring.delete(&of_login(login)).await.map_err(unreadable)?;
        }
        for item in unnamed(&keyring, user, system).await? {
            item.delete().await.map_err(unreadable)?;
            found = true;
        }
        Ok(found)
    })
}
