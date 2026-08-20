use std::{fmt, str::FromStr};

use latticeaxiom_core::{IdentifierError, StableId};
use thiserror::Error;

/// Failure to construct a typed gameplay identifier.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GameplayIdError {
    /// The underlying stable identifier is malformed.
    #[error(transparent)]
    InvalidStableId(#[from] IdentifierError),
    /// The stable identifier names a different registration kind.
    #[error("expected stable ID kind `{expected}`, found `{actual}` in `{value}`")]
    WrongKind {
        /// Required registry kind.
        expected: &'static str,
        /// Actual registry kind.
        actual: String,
        /// Full canonical identifier.
        value: String,
    },
    /// A semantic contract identifier omitted its required major suffix.
    #[error("semantic contract `{value}` requires an explicit `@major` suffix")]
    MissingMajor {
        /// Full canonical identifier.
        value: String,
    },
}

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal, $major_required:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(StableId);

        impl $name {
            /// Parses and validates a canonical typed stable identifier.
            ///
            /// # Errors
            ///
            /// Returns [`GameplayIdError`] when the stable ID is malformed,
            /// has the wrong registry kind, or omits a required contract major.
            pub fn parse(value: &str) -> Result<Self, GameplayIdError> {
                let id = StableId::from_str(value)?;
                if id.kind() != $kind {
                    return Err(GameplayIdError::WrongKind {
                        expected: $kind,
                        actual: id.kind().to_owned(),
                        value: id.as_str().to_owned(),
                    });
                }
                if $major_required && id.major().is_none() {
                    return Err(GameplayIdError::MissingMajor {
                        value: id.as_str().to_owned(),
                    });
                }
                Ok(Self(id))
            }

            /// Returns the canonical stable identifier.
            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// Returns the registration namespace.
            #[must_use]
            pub fn namespace(&self) -> &str {
                self.0.namespace()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = GameplayIdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }
    };
}

typed_id!(
    /// Exact item registration identity.
    ItemId,
    "item",
    false
);
typed_id!(
    /// Exact block registration identity.
    BlockId,
    "block",
    false
);
typed_id!(
    /// Versioned item-tag contract identity.
    ItemTagId,
    "item-tag",
    true
);
typed_id!(
    /// Versioned item-role contract identity.
    ItemRoleId,
    "item-role",
    true
);
typed_id!(
    /// Versioned recipe registration identity.
    RecipeId,
    "recipe",
    true
);
typed_id!(
    /// Versioned workstation contract identity.
    WorkstationId,
    "workstation",
    true
);
typed_id!(
    /// Versioned scheduled process identity.
    ProcessId,
    "process",
    true
);
typed_id!(
    /// Versioned tool-class contract identity.
    ToolClassId,
    "tool-class",
    true
);

#[cfg(test)]
mod tests {
    use super::{GameplayIdError, ItemId, ItemTagId};

    #[test]
    fn exact_and_contract_ids_keep_distinct_version_rules() {
        assert!(ItemId::parse("example:item/stone").is_ok());
        assert!(ItemTagId::parse("latticeaxiom:item-tag/stones@1").is_ok());
        assert!(matches!(
            ItemTagId::parse("latticeaxiom:item-tag/stones"),
            Err(GameplayIdError::MissingMajor { .. })
        ));
        assert!(matches!(
            ItemId::parse("example:block/stone"),
            Err(GameplayIdError::WrongKind { .. })
        ));
    }
}
