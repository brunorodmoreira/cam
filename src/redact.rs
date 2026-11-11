//! Redaction of anything that looks like a Claude token.

const PREFIX: &str = "sk-ant-";
const REDACTED: &str = "sk-ant-[REDACTED]";

/// Replaces every match of `sk-ant-[A-Za-z0-9_-]+` with `sk-ant-[REDACTED]`.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(PREFIX) {
        out.push_str(&rest[..start]);
        let body = &rest[start + PREFIX.len()..];
        let len = body
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(body.len());
        out.push_str(if len == 0 { PREFIX } else { REDACTED });
        rest = &body[len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token_body() {
        assert_eq!(
            redact("invalid account name \"sk-ant-oat01-NOT_A-real0\" in x"),
            "invalid account name \"sk-ant-[REDACTED]\" in x"
        );
    }

    #[test]
    fn redacts_every_occurrence() {
        assert_eq!(
            redact("sk-ant-aaa and sk-ant-bbb\nsk-ant-ccc"),
            "sk-ant-[REDACTED] and sk-ant-[REDACTED]\nsk-ant-[REDACTED]"
        );
    }

    #[test]
    fn leaves_other_text_alone() {
        assert_eq!(redact("work (from CAM_ACCOUNT)"), "work (from CAM_ACCOUNT)");
        assert_eq!(redact("bare sk-ant- prefix"), "bare sk-ant- prefix");
        assert_eq!(redact(""), "");
    }
}
