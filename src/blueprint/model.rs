use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}

impl TodoStatus {
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Pending => " ",
            Self::InProgress => "/",
            Self::Completed => "x",
            Self::Blocked => "?",
            Self::Cancelled => "-",
        }
    }
}
