//! Error types for badbatch.
//!
//! The first five variants are the refusal classes the Python
//! tool raised, one per surface that can refuse, and each carries
//! the fully formed message the command line prints after its
//! error prefix. The last variant is the uncaught family, the
//! failures the Python tool never caught, which the Python
//! interpreter answered with a traceback at exit status 1. The
//! crate answers those with its own one line at the same status,
//! so the variant carries the line to print, formed by the port
//! rather than copied from any traceback.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    /// A recorded run record is malformed.
    #[error("{0}")]
    Record(String),
    /// A stamp document is malformed.
    #[error("{0}")]
    Stamp(String),
    /// A recorded current state is malformed.
    #[error("{0}")]
    State(String),
    /// A recall walk cannot be started as asked.
    #[error("{0}")]
    Walk(String),
    /// A recorded judgment fixture does not match the recorded shape.
    #[error("{0}")]
    Judgment(String),
    /// A failure the Python tool never caught.
    #[error("{0}")]
    Uncaught(String),
}

impl Error {
    /// The message the error carries.
    pub fn message(&self) -> &str {
        match self {
            Error::Record(message)
            | Error::Stamp(message)
            | Error::State(message)
            | Error::Walk(message)
            | Error::Judgment(message)
            | Error::Uncaught(message) => message,
        }
    }

    /// Whether this error belongs to the uncaught family.
    pub fn is_uncaught(&self) -> bool {
        matches!(self, Error::Uncaught(_))
    }
}

/// The result type every fallible library function returns.
pub type Result<T> = std::result::Result<T, Error>;
