//! The password, in the desktop keyring.
//!
//! Through the Secret Service, which gnome-keyring provides on Omarchy, and
//! never anywhere else: the settings database is backed up and can be copied
//! to another machine, and a password must go with neither.
//!
//! One item per user and system, found by these attributes, which are also
//! what `secret-tool` would be given to find it by hand:
//!
//! ```text
//! application=omacharts service=rithmic system=<system> user=<user ID>
//! ```
//!
//! The blocking functions here run a small runtime of their own for the one
//! call, so they can be called from anywhere but inside a tokio runtime.

use crate::Error;

const LABEL: &str = "Omacharts: Rithmic login";

fn attributes<'a>(user: &'a str, system: &'a str) -> [(&'static str, &'a str); 4] {
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

/// The password saved for `user` on `system`, if there is one.
pub(crate) async fn password(user: &str, system: &str) -> Result<Option<String>, Error> {
    let keyring = open().await?;
    let items = keyring.search_items(&attributes(user, system)).await.map_err(unreadable)?;
    let Some(item) = items.first() else { return Ok(None) };
    item.unlock().await.map_err(unreadable)?;
    let secret = item.secret().await.map_err(unreadable)?;
    match std::str::from_utf8(&secret) {
        Ok(text) if !text.is_empty() => Ok(Some(text.to_string())),
        _ => Ok(None),
    }
}

fn blocking<T>(work: impl std::future::Future<Output = Result<T, Error>>) -> Result<T, Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::NoLogin(format!("the keyring could not be used: {error}")))?
        .block_on(work)
}

/// Save `password` for `user` on `system`, replacing whatever was there.
pub fn save(user: &str, system: &str, password: &str) -> Result<(), Error> {
    blocking(async {
        let keyring = open().await?;
        keyring
            .create_item(LABEL, &attributes(user, system), oo7::Secret::text(password), true)
            .await
            .map_err(unreadable)
    })
}

/// Is a password saved for `user` on `system`?
pub fn holds(user: &str, system: &str) -> Result<bool, Error> {
    blocking(async { Ok(password(user, system).await?.is_some()) })
}

/// Forget the password saved for `user` on `system`. True if there was one.
pub fn forget(user: &str, system: &str) -> Result<bool, Error> {
    blocking(async {
        let keyring = open().await?;
        let found = !keyring.search_items(&attributes(user, system)).await.map_err(unreadable)?.is_empty();
        if found {
            keyring.delete(&attributes(user, system)).await.map_err(unreadable)?;
        }
        Ok(found)
    })
}
