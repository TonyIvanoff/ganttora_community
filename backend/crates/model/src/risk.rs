//! # Risks: things that could go wrong with a task
//!
//! ## What this file is for
//! Defines [`Risk`], one thing that *could* go wrong, and what is done about
//! it. Example: "The wood may arrive late", high priority, mitigated by
//! "Order two weeks earlier", consequence "The wall is built a week later".
//!
//! ## Where it fits
//! Part of the `model` crate (data only). A [`Task`](crate::Task) holds its
//! risks in `Task::risks`. The `document` crate adds, edits and removes them
//! with commands and refuses incomplete ones; `gtra` stores them in the
//! table `risks`. The engine never looks at them.
//!
//! ## Main ideas
//! - **A risk has not happened, so it never moves dates.** When it happens,
//!   the user records a [`Blocker`](crate::Blocker), which does.
//! - **Two responses:** *approved* (the risk is known and accepted, nothing
//!   needs to be done) or *mitigate* (a note says how).
//! - **The consequence is never forgotten:** it is either written down, or
//!   the user ticks "no consequence".
//!
//! ## Documentation

use crate::ids::RiskId;
use serde::{Deserialize, Serialize};

/// How important a risk is.
///
/// # Beginner note
/// `PartialOrd, Ord` let us compare priorities. The order is the order of
/// the lines below, so `Low < Medium < High`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskPriority {
    /// Little harm, or very unlikely.
    Low,
    /// The default.
    #[default]
    Medium,
    /// Needs attention first.
    High,
}

/// What is done about a risk.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskResponse {
    /// Known and accepted: no mitigation is needed.
    Approved,
    /// Something is done; [`Risk::mitigation`] says what.
    #[default]
    Mitigate,
}

/// One thing that could go wrong with a task.
///
/// # JSON
/// ```text
/// { "id": "…", "description": "Wood may be late", "priority": "high",
///   "response": "mitigate", "mitigation": "Order two weeks earlier",
///   "noConsequence": false, "consequence": "The wall is a week later" }
/// ```
///
/// # Example
/// ```
/// use model::{Risk, RiskId, RiskResponse};
/// let mut risk = Risk::new(RiskId::new(), "Wood may be late");
/// assert!(!risk.is_complete()); // no mitigation, no consequence
/// risk.response = RiskResponse::Approved;
/// risk.no_consequence = true;
/// assert!(risk.is_complete());
/// ```
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    /// Unique ID of the risk.
    pub id: RiskId,
    /// What could go wrong.
    pub description: String,
    /// How important it is.
    #[serde(default)]
    pub priority: RiskPriority,
    /// What is done about it.
    #[serde(default)]
    pub response: RiskResponse,
    /// How it is mitigated; empty for an approved risk.
    #[serde(default)]
    pub mitigation: String,
    /// The user ticked "no consequence".
    #[serde(default)]
    pub no_consequence: bool,
    /// What happens if it goes wrong; empty when there is no consequence.
    #[serde(default)]
    pub consequence: String,
}

impl Risk {
    /// Creates a risk with default values (medium, mitigate, texts empty).
    ///
    /// # Parameters
    /// - `id`: the risk's ID (the caller makes it, so tests can fix it).
    /// - `description`: what could go wrong.
    ///
    /// # Returns
    /// The new risk. It is not complete yet (see [`Risk::is_complete`]).
    pub fn new(id: RiskId, description: impl Into<String>) -> Self {
        Risk {
            id,
            description: description.into(),
            priority: RiskPriority::default(),
            response: RiskResponse::default(),
            mitigation: String::new(),
            no_consequence: false,
            consequence: String::new(),
        }
    }

    /// `true` when everything required is filled in: a description, a
    /// mitigation note (unless approved) and a consequence (unless ticked
    /// "no consequence"). Text of spaces only counts as empty.
    pub fn is_complete(&self) -> bool {
        let written = |text: &str| !text.trim().is_empty();
        written(&self.description)
            && (self.response == RiskResponse::Approved || written(&self.mitigation))
            && (self.no_consequence || written(&self.consequence))
    }
}
