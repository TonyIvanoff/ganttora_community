//! # Identifiers: one UUID type per kind of entity
//!
//! ## What this file is for
//! Defines the ID types [`TaskId`], [`DependencyId`], [`CalendarId`],
//! [`ProjectId`] and [`BlockerId`]. Every task, link, calendar, project and
//! blocker carries one of them,
//! and other data refers to it by that ID (a dependency says "from task
//! `TaskId(…)` to task `TaskId(…)`").
//!
//! ## Where it fits
//! Part of the `model` crate. Used everywhere: the engine looks tasks up by
//! `TaskId`, the `.gtra` file stores the IDs as text, the UI receives them in
//! JSON.
//!
//! ## Main ideas
//! - A **UUID** (Universally Unique Identifier) is a random 128-bit number,
//!   usually written like `67e55044-10b1-426f-9247-bb680e5fe0c8`. The chance
//!   that two random UUIDs are equal is so small that we can ignore it. So two
//!   people can create tasks on two computers and the IDs never clash.
//! - We never use row numbers as IDs: inserting a row would
//!   change every number below it and break every link.
//! - Each kind of entity gets its **own** ID type, even though all of them
//!   hold a UUID inside. This is called the **newtype pattern**. It lets the
//!   compiler catch mistakes such as passing a `CalendarId` where a `TaskId`
//!   is expected.
//!
//! ## Documentation

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Writes one ID type. All ID types are identical apart from their name
/// and their documentation, so instead of copying the same code again and
/// again we let a **macro** write it for us.
///
/// # Beginner note: macros
/// A macro is "code that writes code". `macro_rules!` defines a pattern; every
/// time we call `define_id!(...)` below, the compiler pastes the code inside
/// the macro, with `$name` replaced by the name we passed in. You have already
/// used macros without noticing: `println!` and `vec!` are macros (the `!`
/// gives them away).
macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        ///
        /// # Beginner note: derives
        /// The `#[derive(...)]` line asks the compiler to write common
        /// abilities for us: `Clone`/`Copy` (the value can be duplicated
        /// cheaply), `PartialEq`/`Eq` (two IDs can be compared with `==`),
        /// `PartialOrd`/`Ord` (IDs can be sorted, needed to use them as keys
        /// in a `BTreeMap`), `Hash`, `Debug` (printable with `{:?}`) and
        /// `Serialize`/`Deserialize` (convertible to and from JSON).
        /// `#[serde(transparent)]` makes the JSON form just the UUID text,
        /// for example `"67e55044-10b1-426f-9247-bb680e5fe0c8"`.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new, random ID (a version 4 UUID).
            ///
            /// # Parameters
            /// None.
            ///
            /// # Returns
            /// A fresh ID that is, in practice, different from every other ID
            /// ever created.
            ///
            /// # Errors / edge cases
            /// None. Note that the result is random, so code that must be
            /// deterministic (the scheduling engine) never calls this.
            ///
            /// # Example
            /// ```
            #[doc = concat!("use model::", stringify!($name), ";")]
            #[doc = concat!("let a = ", stringify!($name), "::new();")]
            #[doc = concat!("let b = ", stringify!($name), "::new();")]
            /// assert_ne!(a, b); // two new IDs are never equal
            /// ```
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID, for example one read from a `.gtra`
            /// file or written by hand in an example.
            ///
            /// # Parameters
            /// - `uuid`: the UUID to wrap.
            ///
            /// # Returns
            /// The ID holding exactly that UUID.
            ///
            /// # Example
            /// ```
            #[doc = concat!("use model::", stringify!($name), ";")]
            /// use uuid::Uuid;
            #[doc = concat!("let id = ", stringify!($name), "::from_uuid(Uuid::nil());")]
            /// assert_eq!(id.as_uuid(), Uuid::nil());
            /// ```
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the UUID stored inside this ID.
            ///
            /// # Returns
            /// A copy of the inner UUID.
            ///
            /// # Example
            /// ```
            #[doc = concat!("use model::", stringify!($name), ";")]
            /// use uuid::Uuid;
            #[doc = concat!("assert!(", stringify!($name), "::from_uuid(Uuid::nil()).as_uuid().is_nil());")]
            /// ```
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        /// `Default` gives a new random ID, the same as `new()`. Clippy (the
        /// Rust style checker) asks for this whenever a type has a `new()`
        /// without parameters.
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        /// `Display` decides how the ID looks when printed with `{}`: the
        /// usual hyphenated UUID text, for example
        /// `67e55044-10b1-426f-9247-bb680e5fe0c8`.
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // Delegate to the UUID's own Display.
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(
    /// The unique ID of a [`Task`](crate::Task).
    TaskId
);

define_id!(
    /// The unique ID of a [`Dependency`](crate::Dependency) (a link between
    /// two tasks).
    DependencyId
);

define_id!(
    /// The unique ID of a [`Calendar`](crate::Calendar).
    CalendarId
);

define_id!(
    /// The unique ID of a [`Project`](crate::Project).
    ProjectId
);

define_id!(
    /// The unique ID of a [`Blocker`](crate::Blocker), an unplanned delay
    /// recorded on a task.
    BlockerId
);

define_id!(
    /// The unique ID of a [`Risk`](crate::Risk), something that could go
    /// wrong with a task.
    RiskId
);
