//! Token storage behind [`SecretStore`].
//!
//! - [`KeychainStore`](keychain::KeychainStore): production, macOS Keychain
//!   generic passwords with service `cam` and the account name as account.
//! - [`FileStore`]: tests only, one plain file per account. Compiled only with
//!   `cfg(test)` or the `test-store` feature, and selected by
//!   `CAM_TEST_STORE=<dir>`.

use std::path::PathBuf;

use anyhow::Result;

use crate::error::Error;
use crate::token::Token;

/// Keychain service name.
pub const SERVICE: &str = "cam";
pub const ENV_TEST_STORE: &str = "CAM_TEST_STORE";

pub trait SecretStore {
    /// Where tokens live, for messages, e.g. "the macOS Keychain".
    fn location(&self) -> String;
    /// Whether a token is stored for `account`, without reading it.
    fn contains(&self, account: &str) -> Result<bool, Error>;
    /// Creates or replaces the token for `account`.
    fn set(&self, account: &str, token: &Token) -> Result<(), Error>;
    /// Deletes the token for `account`, returning whether there was one.
    fn delete(&self, account: &str) -> Result<bool, Error>;
}

/// The store to use: the test store if `CAM_TEST_STORE` is set, else the
/// platform store.
pub fn open() -> Result<Box<dyn SecretStore>> {
    match std::env::var_os(ENV_TEST_STORE).filter(|dir| !dir.is_empty()) {
        Some(dir) => test_store(dir.into()),
        None => platform_store(),
    }
}

#[cfg(any(test, feature = "test-store"))]
fn test_store(dir: PathBuf) -> Result<Box<dyn SecretStore>> {
    Ok(Box::new(FileStore::new(dir)))
}

/// Without the test store compiled in, `CAM_TEST_STORE` is an error, so a
/// misconfigured test can never fall through to the real Keychain.
#[cfg(not(any(test, feature = "test-store")))]
fn test_store(_dir: PathBuf) -> Result<Box<dyn SecretStore>> {
    anyhow::bail!(
        "{ENV_TEST_STORE} is set, but this build has no test store; refusing to touch the Keychain"
    )
}

#[cfg(target_os = "macos")]
fn platform_store() -> Result<Box<dyn SecretStore>> {
    Ok(Box::new(keychain::KeychainStore))
}

#[cfg(not(target_os = "macos"))]
fn platform_store() -> Result<Box<dyn SecretStore>> {
    anyhow::bail!("no token store for this platform yet; cam supports macOS for now")
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::base::Error as SecError;
    use security_framework::item::{ItemClass, ItemSearchOptions};
    use security_framework::passwords::{delete_generic_password, set_generic_password};

    use super::{SERVICE, SecretStore};
    use crate::error::Error;
    use crate::token::Token;

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;
    const LOCATION: &str = "the macOS Keychain";

    pub struct KeychainStore;

    impl SecretStore for KeychainStore {
        fn location(&self) -> String {
            LOCATION.to_owned()
        }

        fn contains(&self, account: &str) -> Result<bool, Error> {
            // Load attributes only, so the secret itself is never read.
            let found = ItemSearchOptions::new()
                .class(ItemClass::generic_password())
                .service(SERVICE)
                .account(account)
                .load_attributes(true)
                .limit(1)
                .search();
            match found {
                Ok(items) => Ok(!items.is_empty()),
                Err(err) if err.code() == NOT_FOUND => Ok(false),
                Err(err) => Err(failure("look up", account, err)),
            }
        }

        fn set(&self, account: &str, token: &Token) -> Result<(), Error> {
            set_generic_password(SERVICE, account, token.expose().as_bytes())
                .map_err(|err| failure("save", account, err))
        }

        fn delete(&self, account: &str) -> Result<bool, Error> {
            match delete_generic_password(SERVICE, account) {
                Ok(()) => Ok(true),
                Err(err) if err.code() == NOT_FOUND => Ok(false),
                Err(err) => Err(failure("delete", account, err)),
            }
        }
    }

    fn failure(action: &'static str, account: &str, source: SecError) -> Error {
        Error::Store {
            action,
            account: account.to_owned(),
            location: LOCATION.to_owned(),
            source: Box::new(source),
        }
    }
}

#[cfg(any(test, feature = "test-store"))]
pub use file::FileStore;

#[cfg(any(test, feature = "test-store"))]
mod file {
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    use super::SecretStore;
    use crate::account::is_valid_name;
    use crate::error::Error;
    use crate::token::Token;

    /// Test-only store: `<dir>/<account>` holds the token in plain text.
    pub struct FileStore {
        dir: PathBuf,
    }

    impl FileStore {
        pub fn new(dir: PathBuf) -> Self {
            Self { dir }
        }

        fn path(&self, account: &str) -> PathBuf {
            // Callers validate names, so an account can't escape the directory.
            assert!(is_valid_name(account), "invalid account name");
            self.dir.join(account)
        }

        fn failure(&self, action: &'static str, account: &str, source: io::Error) -> Error {
            Error::Store {
                action,
                account: account.to_owned(),
                location: self.location(),
                source: Box::new(source),
            }
        }
    }

    impl SecretStore for FileStore {
        fn location(&self) -> String {
            format!("the test store at {}", self.dir.display())
        }

        fn contains(&self, account: &str) -> Result<bool, Error> {
            self.path(account)
                .try_exists()
                .map_err(|err| self.failure("look up", account, err))
        }

        fn set(&self, account: &str, token: &Token) -> Result<(), Error> {
            fs::create_dir_all(&self.dir)
                .and_then(|()| fs::write(self.path(account), token.expose()))
                .map_err(|err| self.failure("save", account, err))
        }

        fn delete(&self, account: &str) -> Result<bool, Error> {
            match fs::remove_file(self.path(account)) {
                Ok(()) => Ok(true),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(err) => Err(self.failure("delete", account, err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testutil::{Tree, fake_token};

    #[test]
    fn file_store_round_trip() {
        let tree = Tree::new();
        let dir = tree.root.join("store");
        let store = FileStore::new(dir.clone());
        assert!(!store.contains("work").unwrap());

        store.set("work", &fake_token(1)).unwrap();
        assert!(store.contains("work").unwrap());
        assert_eq!(
            fs::read_to_string(dir.join("work")).unwrap(),
            fake_token(1).expose()
        );

        store.set("work", &fake_token(2)).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("work")).unwrap(),
            fake_token(2).expose()
        );

        assert!(store.delete("work").unwrap());
        assert!(!store.contains("work").unwrap());
        assert!(!store.delete("work").unwrap());
    }

    #[test]
    fn file_store_location_names_the_dir() {
        let store = FileStore::new("/tmp/x".into());
        assert_eq!(store.location(), "the test store at /tmp/x");
    }

    #[test]
    #[should_panic(expected = "invalid account name")]
    fn file_store_rejects_path_like_names() {
        let tree = Tree::new();
        let _ = FileStore::new(tree.root.clone()).contains("../escape");
    }
}
