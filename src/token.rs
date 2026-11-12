//! Tokens from `claude setup-token`. A [`Token`] never prints its value: it
//! has no `Display`, and its `Debug` is redacted.

use std::fmt;
use std::io::{self, IsTerminal, Read};

use crate::error::Error;

const PREFIX: &str = "sk-ant-";
const API_KEY_PREFIX: &str = "sk-ant-api";

pub struct Token(String);

impl Token {
    /// Trims `input` and checks it looks like an OAuth token: `sk-ant-`
    /// followed by `[A-Za-z0-9_-]+`, and not an API key. Errors never
    /// include the input.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let input = input.trim();
        if input.is_empty() {
            return Err(Error::TokenEmpty);
        }
        if input.starts_with(API_KEY_PREFIX) {
            return Err(Error::TokenIsApiKey);
        }
        let well_formed = input.strip_prefix(PREFIX).is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        });
        if !well_formed {
            return Err(Error::TokenMalformed);
        }
        Ok(Self(input.to_owned()))
    }

    /// A token read back from a store. `None` if the bytes aren't a valid
    /// token, e.g. a Keychain item edited by hand. The bytes are dropped,
    /// never echoed.
    pub fn from_stored(bytes: Vec<u8>) -> Option<Self> {
        Self::parse(&String::from_utf8(bytes).ok()?).ok()
    }

    /// The raw value. Only for the secret store and the environment of the
    /// `claude` child.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// Reads the token for `account`: a hidden prompt when stdin is a terminal,
/// otherwise all of stdin.
pub fn read(account: &str) -> Result<Token, Error> {
    let stdin = io::stdin();
    let input = if stdin.is_terminal() {
        eprintln!(
            "Paste the token that `claude setup-token` printed for \"{account}\" (input is hidden)."
        );
        rpassword::prompt_password("Token: ").map_err(Error::ReadToken)?
    } else {
        let mut input = String::new();
        stdin
            .lock()
            .read_to_string(&mut input)
            .map_err(Error::ReadToken)?;
        input
    };
    Token::parse(&input)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake values shaped like tokens. Not real credentials.
    const FAKE: &str = "sk-ant-oat01-FAKE_unit-test-0001";

    #[test]
    fn accepts_and_trims() {
        assert_eq!(Token::parse(FAKE).unwrap().expose(), FAKE);
        assert_eq!(Token::parse(&format!("  {FAKE}\n")).unwrap().expose(), FAKE);
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(Token::parse(""), Err(Error::TokenEmpty)));
        assert!(matches!(Token::parse(" \n\t"), Err(Error::TokenEmpty)));
    }

    #[test]
    fn rejects_api_keys() {
        assert!(matches!(
            Token::parse("sk-ant-api03-FAKE"),
            Err(Error::TokenIsApiKey)
        ));
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "hello",
            "sk-ant-",
            "sk-ant-oat01-FAKE with-space",
            "sk-ant-oat01-FAKE\nsecond-line",
            "sk-ant-oat01-FAKE.dot",
            "Bearer sk-ant-oat01-FAKE",
            "sk-live-FAKE",
        ] {
            assert!(
                matches!(Token::parse(bad), Err(Error::TokenMalformed)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn errors_never_echo_the_input() {
        for bad in ["sk-ant-oat01-FAKE with-space", "sk-ant-api03-FAKE", "hello"] {
            let message = Token::parse(bad).unwrap_err().to_string();
            assert!(
                !message.contains("FAKE") && !message.contains("hello"),
                "{message}"
            );
        }
    }

    #[test]
    fn from_stored_round_trips_and_rejects_junk() {
        let token = Token::from_stored(FAKE.as_bytes().to_vec()).unwrap();
        assert_eq!(token.expose(), FAKE);
        assert!(Token::from_stored(Vec::new()).is_none());
        assert!(Token::from_stored(b"not a token".to_vec()).is_none());
        assert!(Token::from_stored(vec![0xff, 0xfe]).is_none());
    }

    #[test]
    fn debug_is_redacted() {
        let debug = format!("{:?}", Token::parse(FAKE).unwrap());
        assert_eq!(debug, "Token(<redacted>)");
    }
}
