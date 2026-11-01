//! Closed client identity shared by authentication, persistence and request builders.
use serde::{Deserialize, Serialize};

pub const PC_LINUX_USER_AGENT: &str = "ZaloPC-linux-24v681";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum ClientType {
    /// Existing Linux desktop launcher selects type 24 for both renderer pages.
    PcLinux24,
    /// Earlier native records omitted the type and used 30. Metadata only:
    /// no verified migration or user-agent/origin contract exists for their reuse.
    Legacy30,
}
impl ClientType {
    pub const fn legacy30() -> Self {
        Self::Legacy30
    }
    pub const fn api_type(self) -> &'static str {
        match self {
            Self::PcLinux24 => "24",
            Self::Legacy30 => "30",
        }
    }
    pub const fn user_agent(self) -> Option<&'static str> {
        match self {
            Self::PcLinux24 => Some(PC_LINUX_USER_AGENT),
            Self::Legacy30 => None,
        }
    }
    pub const fn supports_native_session(self) -> bool {
        matches!(self, Self::PcLinux24)
    }
}
impl From<ClientType> for u8 {
    fn from(value: ClientType) -> Self {
        match value {
            ClientType::PcLinux24 => 24,
            ClientType::Legacy30 => 30,
        }
    }
}
impl TryFrom<u8> for ClientType {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            24 => Ok(Self::PcLinux24),
            30 => Ok(Self::Legacy30),
            _ => Err("Unsupported client type"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_closed_and_legacy_cannot_invent_a_runtime_user_agent() {
        assert_eq!(serde_json::to_string(&ClientType::PcLinux24).unwrap(), "24");
        assert_eq!(
            serde_json::from_str::<ClientType>("30").unwrap(),
            ClientType::Legacy30
        );
        for value in ["0", "23", "25", "255", "256", "\"24\"", "null"] {
            assert!(serde_json::from_str::<ClientType>(value).is_err());
        }
        assert_eq!(ClientType::Legacy30.user_agent(), None);
        assert!(!ClientType::Legacy30.supports_native_session());
    }
}
