//! What crosses the boundary when something is refused.

/// A refusal, as a foreign caller meets it.
///
/// Two kinds, kept apart because their remedies differ. A `Protocol` refusal
/// carries a §12 code every implementation reproduces, and §1 says that code
/// is what a wallet turns into a sentence for its user. A `Host` refusal is
/// local to this device — a store that would not write, a relay that could not
/// be reached — and carries no code because no other implementation could
/// produce the same one.
#[derive(Debug, uniffi::Error)]
pub enum SplitzError {
    Protocol { code: String, detail: String },
    Host { detail: String, transient: bool },
}

// `detail` rather than `message`: uniffi gives a Kotlin error variant a
// property per field, and `message` collides with `Throwable.message`, which
// the generated file then does not compile.

impl std::fmt::Display for SplitzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitzError::Protocol { code, detail } => write!(f, "{code}: {detail}"),
            SplitzError::Host { detail, .. } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for SplitzError {}

impl From<splitz_core::SplitError> for SplitzError {
    fn from(e: splitz_core::SplitError) -> Self {
        SplitzError::Protocol {
            code: e.code.to_owned(),
            detail: e.message,
        }
    }
}

impl From<splitz_host::HostError> for SplitzError {
    fn from(e: splitz_host::HostError) -> Self {
        // A refusal with a §12 code crosses as one, whichever function raised
        // it, so a caller branches on the code and never on prose.
        match e {
            splitz_host::HostError::Protocol(e) => return e.into(),
            splitz_host::HostError::ForeignKey(bill) => {
                return SplitzError::Protocol {
                    code: splitz_core::code::INVITE_KEY_MISMATCH.to_owned(),
                    detail: format!("the key held for {bill} is not the one it was made with"),
                }
            }
            _ => {}
        }
        let transient = matches!(
            e,
            splitz_host::HostError::Relay {
                transient: true,
                ..
            } | splitz_host::HostError::Swap {
                transient: true,
                ..
            }
        );
        SplitzError::Host {
            detail: e.to_string(),
            transient,
        }
    }
}

pub type Result<T> = std::result::Result<T, SplitzError>;
