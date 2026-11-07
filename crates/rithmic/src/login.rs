//! Where a login comes from.
//!
//! Two places, and only two. A person's own login is an id, a user ID and a
//! system the app stores as settings, with the password in the desktop keyring,
//! read at the moment of signing in and kept nowhere else. A developer's is
//! a file outside the repo, named by [`DEV_LOGIN`]: the user ID on its first
//! line and the password on its second, which is how `~/.rithmic` is laid
//! out. That one never goes near the keyring, so working on the app never
//! writes anybody's login into it.

use std::path::Path;

use crate::{keyring, Credentials, Error};

/// The environment variable that names a development login file.
pub const DEV_LOGIN: &str = "OMACHARTS_RITHMIC_LOGIN";

/// A login, as far as it is known before signing in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Login {
    /// Everything in hand, from a development file.
    Given(Credentials),
    /// The password is in the keyring, under the login's id (or, saved
    /// before logins had ids, under this user and system), and is read when
    /// the session signs in.
    Keyring { login: String, user: String, system: String, url: String },
}

impl Login {
    /// The login in the file `path`: user ID on the first line, password on
    /// the second. Anything after that is ignored, and nothing read from it
    /// is ever put in an error.
    pub fn from_file(path: &Path, system: &str, url: &str) -> Result<Login, Error> {
        let text = std::fs::read_to_string(path).map_err(|_| {
            Error::NoLogin(format!("{DEV_LOGIN} names {}, which could not be read", path.display()))
        })?;
        let mut lines = text.lines().map(str::trim);
        match (lines.next(), lines.next()) {
            (Some(user), Some(password)) if !user.is_empty() && !password.is_empty() => {
                Ok(Login::Given(Credentials {
                    user: user.to_string(),
                    password: password.to_string(),
                    system: system.to_string(),
                    url: url.to_string(),
                }))
            }
            _ => Err(Error::NoLogin(format!(
                "{} should hold a user ID on its first line and a password on its second",
                path.display()
            ))),
        }
    }

    /// The development login, if [`DEV_LOGIN`] names a file.
    pub fn from_env(system: &str, url: &str) -> Option<Result<Login, Error>> {
        let path = std::env::var_os(DEV_LOGIN).filter(|path| !path.is_empty())?;
        Some(Login::from_file(Path::new(&path), system, url))
    }

    pub fn system(&self) -> &str {
        match self {
            Login::Given(credentials) => &credentials.system,
            Login::Keyring { system, .. } => system,
        }
    }

    /// Everything needed to sign in, reading the keyring if it has to.
    pub(crate) async fn credentials(&self) -> Result<Credentials, Error> {
        match self {
            Login::Given(credentials) => Ok(credentials.clone()),
            Login::Keyring { login, user, system, url } => {
                let password = keyring::password(login, user, system).await?.ok_or_else(|| {
                    Error::NoLogin(format!("the keyring holds no Rithmic password for {system}"))
                })?;
                Ok(Credentials {
                    user: user.clone(),
                    password,
                    system: system.clone(),
                    url: url.clone(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rithmic-login-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("login");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_file_is_read_as_a_user_then_a_password() {
        let path = scratch("good", "trader\nPLANTED-PASSWORD\nignored\n");
        let login = Login::from_file(&path, crate::TEST_SYSTEM, crate::TEST_URL).unwrap();
        let Login::Given(credentials) = &login else { panic!("not given") };
        assert_eq!(credentials.user, "trader");
        assert_eq!(credentials.password, "PLANTED-PASSWORD");
        assert_eq!(login.system(), crate::TEST_SYSTEM);
        assert!(!format!("{login:?}").contains("PLANTED-PASSWORD"));
    }

    /// A file with the password and nothing else must not have its one line
    /// read back to anybody as a user ID, or worse, in an error.
    #[test]
    fn a_file_that_is_not_a_login_says_so_without_quoting_it() {
        let path = scratch("short", "PLANTED-PASSWORD\n");
        let error = Login::from_file(&path, crate::TEST_SYSTEM, crate::TEST_URL).unwrap_err();
        assert!(matches!(error, Error::NoLogin(_)));
        assert!(!error.to_string().contains("PLANTED-PASSWORD"), "{error}");
        assert!(!format!("{error:?}").contains("PLANTED-PASSWORD"), "{error:?}");

        let missing = Login::from_file(Path::new("/nonexistent/rithmic"), "x", "y").unwrap_err();
        assert!(missing.to_string().contains("could not be read"), "{missing}");
    }
}
