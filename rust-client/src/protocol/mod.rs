//! Transport boundary. A demo message is never represented as a successful Zalo send.
#[cfg(feature = "live-login")]
pub mod auth;
pub mod bootstrap;
pub mod client;
#[cfg(feature = "messaging")]
pub mod directory;
#[cfg(feature = "messaging")]
pub(crate) mod hidden;
#[cfg(feature = "network")]
pub mod http;
#[cfg(feature = "messaging")]
pub mod messaging;
pub mod qr;
#[cfg(feature = "live-login")]
pub mod session;
#[cfg(feature = "messaging")]
pub mod signal;
#[cfg(feature = "trust-wasm")]
pub mod trust;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SessionState {
    SignedOut,
    Authenticating,
    Online,
    Offline,
    Expired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Network,
    Timeout,
    AuthenticationRejected,
    MalformedResponse,
}

impl SessionState {
    /// Only a confirmed authentication rejection expires a session.
    pub fn after_failure(self, failure: Failure) -> Self {
        match failure {
            Failure::AuthenticationRejected => Self::Expired,
            Failure::Network | Failure::Timeout | Failure::MalformedResponse => match self {
                Self::Online | Self::Offline => Self::Offline,
                state => state,
            },
        }
    }
}

pub const LIVE_AUTH_AVAILABLE: bool = false;
pub const AUTH_LIMITATION: &str = "Live Zalo login/send is unavailable: protocol verification and independent fixtures are pending.";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_and_malformed_responses_do_not_expire_credentials() {
        for failure in [
            Failure::Network,
            Failure::Timeout,
            Failure::MalformedResponse,
        ] {
            assert_eq!(
                SessionState::Online.after_failure(failure),
                SessionState::Offline
            );
            assert_eq!(
                SessionState::SignedOut.after_failure(failure),
                SessionState::SignedOut
            );
        }
        assert_eq!(
            SessionState::Online.after_failure(Failure::AuthenticationRejected),
            SessionState::Expired
        );
    }
}
