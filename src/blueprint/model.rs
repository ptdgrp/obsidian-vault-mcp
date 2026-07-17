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

impl TodoStatus {
    pub fn from_marker(marker: char) -> Option<Self> {
        match marker {
            ' ' => Some(Self::Pending),
            '/' => Some(Self::InProgress),
            'x' | 'X' => Some(Self::Completed),
            '?' => Some(Self::Blocked),
            '-' => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CheckItem {
    pub text: String,
    pub completed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub status: TodoStatus,
    pub created_by: Option<String>,
    pub owner: Option<String>,
    pub completed_by: Option<String>,
    pub depends_on: Vec<String>,
    pub completion_criteria: Vec<CheckItem>,
    pub handoff: Vec<String>,
    pub result_summary: Option<String>,
    pub references: Vec<String>,
    pub block_reason: Option<String>,
    pub cancel_reason: Option<String>,
    pub children: Vec<Todo>,
}
