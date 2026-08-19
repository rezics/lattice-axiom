use std::{cmp::Ordering, fmt, num::NonZeroU64, str::FromStr};

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;
use uuid::Uuid;

/// An error produced while validating or parsing a stable identifier.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum IdentifierError {
    /// A logical package name was not a root name or a scoped package name.
    #[error("invalid package name `{value}`: {reason}")]
    InvalidPackageName {
        /// The rejected textual value.
        value: String,
        /// A description of the violated grammar rule.
        reason: &'static str,
    },
    /// A stable registration identifier violated its grammar.
    #[error("invalid stable ID `{value}`: {reason}")]
    InvalidStableId {
        /// The rejected textual value.
        value: String,
        /// A description of the violated grammar rule.
        reason: &'static str,
    },
    /// A capability identifier did not identify a versioned capability.
    #[error("invalid capability ID `{value}`: {reason}")]
    InvalidCapabilityId {
        /// The rejected textual value.
        value: String,
        /// A description of the violated capability rule.
        reason: &'static str,
    },
    /// A schema identifier did not use the `schema` registration kind.
    #[error("invalid schema ID `{value}`: {reason}")]
    InvalidSchemaId {
        /// The rejected textual value.
        value: String,
        /// A description of the violated schema rule.
        reason: &'static str,
    },
    /// A source identifier did not use the `source` registration kind.
    #[error("invalid source ID `{value}`: {reason}")]
    InvalidSourceId {
        /// The rejected textual value.
        value: String,
        /// A description of the violated source rule.
        reason: &'static str,
    },
    /// A package version was not a strict semantic version.
    #[error("invalid package version `{value}`: {reason}")]
    InvalidPackageVersion {
        /// The rejected textual value.
        value: String,
        /// The parser diagnostic.
        reason: String,
    },
    /// A package version requirement was syntactically invalid.
    #[error("invalid package version requirement `{value}`: {reason}")]
    InvalidPackageVersionReq {
        /// The rejected textual value.
        value: String,
        /// The parser diagnostic.
        reason: String,
    },
    /// A world identifier was not a UUID.
    #[error("invalid world ID `{value}`: {reason}")]
    InvalidWorldId {
        /// The rejected textual value.
        value: String,
        /// The parser diagnostic.
        reason: String,
    },
}

/// The logical identity of one versioned package.
///
/// Accepted values are an unscoped root name such as `terrenia` or a scoped
/// name such as `@terrenia/blocks`. Package names are deliberately independent
/// from registration namespaces and source paths.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageName(String);

impl PackageName {
    /// Returns the canonical textual package name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this is a scoped `@scope/name` package.
    #[must_use]
    pub fn is_scoped(&self) -> bool {
        self.0.starts_with('@')
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PackageName {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_package_name(value)?;
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for PackageName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PackageName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A stable registration identity used across runtime and persistence boundaries.
///
/// The grammar is `<namespace>:<kind>/<path>` with an optional terminal
/// `@<positive-major>`. Namespace, kind, and path segments use lowercase ASCII
/// letters, digits, `.`, `_`, or `-`; every segment begins and ends with an
/// ASCII letter or digit. A path may contain multiple slash-separated segments.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StableId {
    value: String,
    namespace_end: usize,
    kind_end: usize,
    major_marker: Option<usize>,
    major: Option<NonZeroU64>,
}

impl StableId {
    /// Returns the full canonical stable identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the registration namespace.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.value[..self.namespace_end]
    }

    /// Returns the registration kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.value[self.namespace_end + 1..self.kind_end]
    }

    /// Returns the slash-separated registration path without its major suffix.
    #[must_use]
    pub fn path(&self) -> &str {
        let path_end = self.major_marker.unwrap_or(self.value.len());
        &self.value[self.kind_end + 1..path_end]
    }

    /// Returns the contract major suffix, when the identifier has one.
    #[must_use]
    pub fn major(&self) -> Option<NonZeroU64> {
        self.major
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for StableId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_stable_id(value)
    }
}

impl Serialize for StableId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A stable identifier constrained to the versioned `capability` kind.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CapabilityId {
    stable_id: StableId,
    major: NonZeroU64,
}

impl CapabilityId {
    /// Returns the underlying stable identifier.
    #[must_use]
    pub fn as_stable_id(&self) -> &StableId {
        &self.stable_id
    }

    /// Returns the full canonical capability identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.stable_id.as_str()
    }

    /// Returns the required positive capability contract major.
    #[must_use]
    pub fn major(&self) -> NonZeroU64 {
        self.major
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CapabilityId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        StableId::from_str(value)?.try_into()
    }
}

impl TryFrom<StableId> for CapabilityId {
    type Error = IdentifierError;

    fn try_from(value: StableId) -> Result<Self, Self::Error> {
        if value.kind() != "capability" {
            return Err(IdentifierError::InvalidCapabilityId {
                value: value.to_string(),
                reason: "registration kind must be `capability`",
            });
        }
        let Some(major) = value.major() else {
            return Err(IdentifierError::InvalidCapabilityId {
                value: value.to_string(),
                reason: "a capability ID requires a positive major suffix",
            });
        };
        Ok(Self {
            stable_id: value,
            major,
        })
    }
}

impl Serialize for CapabilityId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CapabilityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A stable identifier constrained to the `schema` registration kind.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaId(StableId);

impl SchemaId {
    /// Returns the underlying stable identifier.
    #[must_use]
    pub fn as_stable_id(&self) -> &StableId {
        &self.0
    }

    /// Returns the full canonical schema identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for SchemaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SchemaId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        StableId::from_str(value)?.try_into()
    }
}

impl TryFrom<StableId> for SchemaId {
    type Error = IdentifierError;

    fn try_from(value: StableId) -> Result<Self, Self::Error> {
        if value.kind() != "schema" {
            return Err(IdentifierError::InvalidSchemaId {
                value: value.to_string(),
                reason: "registration kind must be `schema`",
            });
        }
        Ok(Self(value))
    }
}

impl Serialize for SchemaId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SchemaId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A stable identifier constrained to the unversioned `source` kind.
///
/// Source IDs identify entries in the controlled package-source universe and
/// remain stable when a local source directory is relocated.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(StableId);

impl SourceId {
    /// Returns the underlying stable identifier.
    #[must_use]
    pub fn as_stable_id(&self) -> &StableId {
        &self.0
    }

    /// Returns the full canonical source identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SourceId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        StableId::from_str(value)?.try_into()
    }
}

impl TryFrom<StableId> for SourceId {
    type Error = IdentifierError;

    fn try_from(value: StableId) -> Result<Self, Self::Error> {
        if value.kind() != "source" {
            return Err(IdentifierError::InvalidSourceId {
                value: value.to_string(),
                reason: "registration kind must be `source`",
            });
        }
        if value.major().is_some() {
            return Err(IdentifierError::InvalidSourceId {
                value: value.to_string(),
                reason: "a source ID cannot have a contract major suffix",
            });
        }
        Ok(Self(value))
    }
}

impl Serialize for SourceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SourceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A validated strict `SemVer` 2.0 package version.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PackageVersion(Version);

impl PackageVersion {
    /// Compares semantic-version precedence while ignoring build metadata.
    #[must_use]
    pub fn precedence_cmp(&self, other: &Self) -> Ordering {
        version_precedence_cmp(&self.0, &other.0)
    }

    /// Provides a deterministic total order for exact version identities.
    ///
    /// This first compares `SemVer` precedence and then uses build metadata only
    /// to order otherwise equal exact identities. Resolver compatibility and
    /// highest-version selection must use [`Self::precedence_cmp`] instead.
    #[must_use]
    pub fn exact_cmp(&self, other: &Self) -> Ordering {
        self.precedence_cmp(other)
            .then_with(|| self.0.build.cmp(&other.0.build))
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for PackageVersion {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Version::parse(value)
            .map(Self)
            .map_err(|error| IdentifierError::InvalidPackageVersion {
                value: value.to_owned(),
                reason: error.to_string(),
            })
    }
}

impl Serialize for PackageVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// One normalized operation in a package version requirement intersection.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VersionComparatorOperator {
    /// The candidate must have equal `SemVer` precedence.
    Equal,
    /// The candidate must have lower `SemVer` precedence.
    Less,
    /// The candidate must have lower or equal `SemVer` precedence.
    LessOrEqual,
    /// The candidate must have greater `SemVer` precedence.
    Greater,
    /// The candidate must have greater or equal `SemVer` precedence.
    GreaterOrEqual,
}

impl fmt::Display for VersionComparatorOperator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Equal => "=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        })
    }
}

/// A normalized comparison against one complete strict package version.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VersionComparator {
    operator: VersionComparatorOperator,
    version: PackageVersion,
}

impl VersionComparator {
    /// Returns the comparison operation.
    #[must_use]
    pub const fn operator(&self) -> VersionComparatorOperator {
        self.operator
    }

    /// Returns the comparison version.
    #[must_use]
    pub const fn version(&self) -> &PackageVersion {
        &self.version
    }
}

impl fmt::Display for VersionComparator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}{}", self.operator, self.version)
    }
}

/// A normalized intersection of strict package-version comparators.
///
/// The accepted source grammar is deliberately narrower than Cargo's: exact
/// `=1.2.3`, `<`/`<=`/`>`/`>=` intersections, `~1.2.3`, or `^1.2.3`.
/// Bare versions, wildcards, unions, hyphen ranges, and shorthand versions are
/// rejected. Tilde and caret inputs are expanded to normalized comparator
/// pairs. In particular, `^0.2.3` normalizes to `>=0.2.3 <1.0.0`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PackageVersionReq(Vec<VersionComparator>);

impl PackageVersionReq {
    /// Returns the normalized comparator intersection.
    #[must_use]
    pub fn comparators(&self) -> &[VersionComparator] {
        &self.0
    }

    /// Tests a package version against this normalized range.
    ///
    /// Build metadata is ignored for precedence comparisons. A pre-release
    /// candidate participates only when at least one comparator explicitly
    /// contains a pre-release with the same major, minor, and patch version.
    #[must_use]
    pub fn matches(&self, candidate: &PackageVersion) -> bool {
        if !candidate.0.pre.is_empty()
            && !self.0.iter().any(|comparator| {
                !comparator.version.0.pre.is_empty()
                    && same_version_core(&comparator.version.0, &candidate.0)
            })
        {
            return false;
        }

        self.0
            .iter()
            .all(|comparator| comparator.matches(candidate))
    }
}

impl fmt::Display for PackageVersionReq {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, comparator) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" ")?;
            }
            comparator.fmt(formatter)?;
        }
        Ok(())
    }
}

impl FromStr for PackageVersionReq {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_package_version_req(value)
    }
}

impl Serialize for PackageVersionReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackageVersionReq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A persistent UUID identifying one world independently of its display name.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorldId(Uuid);

impl WorldId {
    /// Creates a new random UUID version 4 world identifier.
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a world identifier from an already validated UUID.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(formatter)
    }
}

impl FromStr for WorldId {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|error| IdentifierError::InvalidWorldId {
                value: value.to_owned(),
                reason: error.to_string(),
            })
    }
}

impl Serialize for WorldId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WorldId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

fn validate_package_name(value: &str) -> Result<(), IdentifierError> {
    let invalid = |reason| IdentifierError::InvalidPackageName {
        value: value.to_owned(),
        reason,
    };

    if let Some(scoped) = value.strip_prefix('@') {
        let Some((scope, name)) = scoped.split_once('/') else {
            return Err(invalid("a scoped name must use `@scope/name`"));
        };
        if name.contains('/') {
            return Err(invalid("a scoped name contains exactly one slash"));
        }
        if !is_identifier_segment(scope) || !is_identifier_segment(name) {
            return Err(invalid(
                "scope and name must be canonical lowercase segments",
            ));
        }
        return Ok(());
    }

    if value.contains(['/', '@']) {
        return Err(invalid("an unscoped name cannot contain `/` or `@`"));
    }
    if !is_identifier_segment(value) {
        return Err(invalid("a root name must be a canonical lowercase segment"));
    }
    Ok(())
}

fn parse_stable_id(value: &str) -> Result<StableId, IdentifierError> {
    let invalid = |reason| IdentifierError::InvalidStableId {
        value: value.to_owned(),
        reason,
    };

    let (base, major_marker, major) = if let Some((base, major_text)) = value.rsplit_once('@') {
        if base.contains('@') || major_text.is_empty() {
            return Err(invalid("the optional major suffix must be terminal"));
        }
        if major_text.len() > 1 && major_text.starts_with('0') {
            return Err(invalid("the major suffix cannot contain leading zeroes"));
        }
        let major = major_text
            .parse::<NonZeroU64>()
            .map_err(|_| invalid("the major suffix must be a positive integer"))?;
        (base, Some(base.len()), Some(major))
    } else {
        (value, None, None)
    };

    let Some((namespace, remainder)) = base.split_once(':') else {
        return Err(invalid("a stable ID requires one namespace separator `:`"));
    };
    if remainder.contains(':') {
        return Err(invalid("a stable ID contains exactly one `:`"));
    }
    let Some((kind, path)) = remainder.split_once('/') else {
        return Err(invalid(
            "a stable ID requires a kind and path separated by `/`",
        ));
    };
    if !is_identifier_segment(namespace) {
        return Err(invalid(
            "the namespace must be a canonical lowercase segment",
        ));
    }
    if !is_identifier_segment(kind) {
        return Err(invalid("the kind must be a canonical lowercase segment"));
    }
    if path.is_empty() || !path.split('/').all(is_identifier_segment) {
        return Err(invalid(
            "every path component must be a canonical lowercase segment",
        ));
    }

    let namespace_end = namespace.len();
    let kind_end = namespace_end + 1 + kind.len();
    Ok(StableId {
        value: value.to_owned(),
        namespace_end,
        kind_end,
        major_marker,
        major,
    })
}

fn is_identifier_segment(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }

    let mut last = first;
    for character in characters {
        if !character.is_ascii_lowercase()
            && !character.is_ascii_digit()
            && !matches!(character, '-' | '_' | '.')
        {
            return false;
        }
        last = character;
    }
    last.is_ascii_lowercase() || last.is_ascii_digit()
}

impl VersionComparator {
    fn new(operator: VersionComparatorOperator, version: PackageVersion) -> Self {
        Self { operator, version }
    }

    fn matches(&self, candidate: &PackageVersion) -> bool {
        let ordering = version_precedence_cmp(&candidate.0, &self.version.0);
        match self.operator {
            VersionComparatorOperator::Equal => ordering.is_eq(),
            VersionComparatorOperator::Less => ordering.is_lt(),
            VersionComparatorOperator::LessOrEqual => !ordering.is_gt(),
            VersionComparatorOperator::Greater => ordering.is_gt(),
            VersionComparatorOperator::GreaterOrEqual => !ordering.is_lt(),
        }
    }
}

fn parse_package_version_req(value: &str) -> Result<PackageVersionReq, IdentifierError> {
    let invalid = |reason: &'static str| IdentifierError::InvalidPackageVersionReq {
        value: value.to_owned(),
        reason: reason.to_owned(),
    };

    if value.is_empty() || value.trim() != value {
        return Err(invalid(
            "a version range must be non-empty without surrounding whitespace",
        ));
    }
    if value.contains("||") || value.contains('*') {
        return Err(invalid("unions and wildcard ranges are not supported"));
    }
    if value.contains(',') {
        return Err(invalid(
            "comparator intersections use ASCII whitespace, not commas",
        ));
    }

    if let Some(version_text) = value.strip_prefix('~') {
        if version_text.contains(char::is_whitespace) {
            return Err(invalid("tilde ranges contain exactly one complete version"));
        }
        let lower = parse_req_version(value, version_text)?;
        let upper_major = lower.0.major;
        let upper_minor = lower
            .0
            .minor
            .checked_add(1)
            .ok_or_else(|| invalid("tilde upper bound overflows the minor version"))?;
        return Ok(PackageVersionReq(normalize_comparators(vec![
            VersionComparator::new(VersionComparatorOperator::GreaterOrEqual, lower),
            VersionComparator::new(
                VersionComparatorOperator::Less,
                PackageVersion(Version::new(upper_major, upper_minor, 0)),
            ),
        ])));
    }

    if let Some(version_text) = value.strip_prefix('^') {
        if version_text.contains(char::is_whitespace) {
            return Err(invalid("caret ranges contain exactly one complete version"));
        }
        let lower = parse_req_version(value, version_text)?;
        let upper_major = lower
            .0
            .major
            .checked_add(1)
            .ok_or_else(|| invalid("caret upper bound overflows the major version"))?;
        return Ok(PackageVersionReq(normalize_comparators(vec![
            VersionComparator::new(VersionComparatorOperator::GreaterOrEqual, lower),
            VersionComparator::new(
                VersionComparatorOperator::Less,
                PackageVersion(Version::new(upper_major, 0, 0)),
            ),
        ])));
    }

    let tokens = value.split_ascii_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(invalid("a version range requires at least one comparator"));
    }
    if tokens.len() == 1 && tokens[0].starts_with('=') {
        let version = parse_comparator_version(value, tokens[0], "=")?;
        return Ok(PackageVersionReq(vec![VersionComparator::new(
            VersionComparatorOperator::Equal,
            version,
        )]));
    }

    let mut comparators = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (operator, prefix) = if token.starts_with(">=") {
            (VersionComparatorOperator::GreaterOrEqual, ">=")
        } else if token.starts_with("<=") {
            (VersionComparatorOperator::LessOrEqual, "<=")
        } else if token.starts_with('>') {
            (VersionComparatorOperator::Greater, ">")
        } else if token.starts_with('<') {
            (VersionComparatorOperator::Less, "<")
        } else {
            return Err(invalid(
                "only explicit comparator intersections, exact, tilde, or caret ranges are supported",
            ));
        };
        let version = parse_comparator_version(value, token, prefix)?;
        comparators.push(VersionComparator::new(operator, version));
    }
    Ok(PackageVersionReq(normalize_comparators(comparators)))
}

fn parse_comparator_version(
    range: &str,
    token: &str,
    prefix: &str,
) -> Result<PackageVersion, IdentifierError> {
    let version_text = &token[prefix.len()..];
    parse_req_version(range, version_text)
}

fn parse_req_version(range: &str, version: &str) -> Result<PackageVersion, IdentifierError> {
    PackageVersion::from_str(version).map_err(|error| IdentifierError::InvalidPackageVersionReq {
        value: range.to_owned(),
        reason: error.to_string(),
    })
}

fn normalize_comparators(mut comparators: Vec<VersionComparator>) -> Vec<VersionComparator> {
    comparators.sort_by(|left, right| {
        comparator_rank(left.operator)
            .cmp(&comparator_rank(right.operator))
            .then_with(|| left.version.exact_cmp(&right.version))
    });
    comparators.dedup();
    comparators
}

const fn comparator_rank(operator: VersionComparatorOperator) -> u8 {
    match operator {
        VersionComparatorOperator::Equal => 0,
        VersionComparatorOperator::GreaterOrEqual => 1,
        VersionComparatorOperator::Greater => 2,
        VersionComparatorOperator::Less => 3,
        VersionComparatorOperator::LessOrEqual => 4,
    }
}

fn same_version_core(left: &Version, right: &Version) -> bool {
    left.major == right.major && left.minor == right.minor && left.patch == right.patch
}

fn version_precedence_cmp(left: &Version, right: &Version) -> Ordering {
    left.major
        .cmp(&right.major)
        .then_with(|| left.minor.cmp(&right.minor))
        .then_with(|| left.patch.cmp(&right.patch))
        .then_with(|| left.pre.cmp(&right.pre))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_name_accepts_root_and_scoped_forms() {
        for value in ["terrenia", "example-2", "@terrenia/blocks", "@a/b.c"] {
            assert!(
                PackageName::from_str(value).is_ok(),
                "expected `{value}` to be valid"
            );
        }
    }

    #[test]
    fn package_name_rejects_noncanonical_forms() {
        for value in [
            "",
            "@terrenia",
            "terrenia/blocks",
            "@/blocks",
            "@scope/",
            "@scope/name/extra",
            "Uppercase",
            "-leading",
            "trailing-",
        ] {
            assert!(
                PackageName::from_str(value).is_err(),
                "expected `{value}` to be invalid"
            );
        }
    }

    #[test]
    fn stable_id_exposes_validated_parts() {
        let parsed = StableId::from_str("latticeaxiom:block-tag/storage-blocks/copper@2");
        assert!(parsed.is_ok());
        let id = parsed.unwrap_or_else(|error| panic!("valid stable ID was rejected: {error}"));
        assert_eq!(id.namespace(), "latticeaxiom");
        assert_eq!(id.kind(), "block-tag");
        assert_eq!(id.path(), "storage-blocks/copper");
        assert_eq!(id.major(), NonZeroU64::new(2));
    }

    #[test]
    fn stable_id_rejects_invalid_forms() {
        for value in [
            "terrenia",
            "terrenia:block",
            "terrenia:/stone",
            ":block/stone",
            "terrenia:block/",
            "terrenia:block/a//b",
            "Terrenia:block/stone",
            "terrenia:block/stone@0",
            "terrenia:block/stone@01",
            "terrenia:block/stone@major",
            "terrenia:block/stone@1@2",
        ] {
            assert!(
                StableId::from_str(value).is_err(),
                "expected `{value}` to be invalid"
            );
        }
    }

    #[test]
    fn source_id_requires_an_unversioned_source_kind() {
        let source = SourceId::from_str("latticeaxiom:source/terrenia");
        assert!(source.is_ok());
        assert!(SourceId::from_str("latticeaxiom:block/stone").is_err());
        assert!(SourceId::from_str("latticeaxiom:source/terrenia@1").is_err());

        let source = source.unwrap_or_else(|error| panic!("valid source ID was rejected: {error}"));
        assert_eq!(json_round_trip(&source), source);
    }

    #[test]
    fn specialized_ids_enforce_kind_and_capability_major() {
        assert!(CapabilityId::from_str("latticeaxiom:capability/settings@1").is_ok());
        assert!(CapabilityId::from_str("latticeaxiom:capability/settings").is_err());
        assert!(CapabilityId::from_str("latticeaxiom:schema/settings@1").is_err());
        assert!(SchemaId::from_str("latticeaxiom:schema/world-header@1").is_ok());
        assert!(SchemaId::from_str("latticeaxiom:capability/settings@1").is_err());
    }

    #[test]
    fn serde_revalidates_identifier_newtypes() {
        assert!(serde_json::from_str::<PackageName>(r#""@scope/name""#).is_ok());
        assert!(serde_json::from_str::<PackageName>(r#""@scope""#).is_err());
        assert!(serde_json::from_str::<StableId>(r#""example:block/stone@1""#).is_ok());
        assert!(serde_json::from_str::<StableId>(r#""example:block/stone@0""#).is_err());
        assert!(serde_json::from_str::<CapabilityId>(r#""example:capability/blocks""#).is_err());
        assert!(serde_json::from_str::<SchemaId>(r#""example:block/stone""#).is_err());
    }

    #[test]
    fn package_versions_are_strict_and_round_trip() {
        let version = PackageVersion::from_str("1.2.3-alpha.1+build.7")
            .unwrap_or_else(|error| panic!("valid package version was rejected: {error}"));
        assert!(PackageVersion::from_str("1.2").is_err());
        assert!(PackageVersion::from_str("v1.2.3").is_err());
        assert!(PackageVersion::from_str("01.2.3").is_err());

        let requirement = PackageVersionReq::from_str(">=1.2.3 <2.0.0");
        assert!(requirement.is_ok());
        assert!(PackageVersionReq::from_str("definitely-not-semver").is_err());

        let encoded = serde_json::to_string(&version).unwrap_or_default();
        let decoded = serde_json::from_str::<PackageVersion>(&encoded);
        assert_eq!(decoded.ok(), Some(version));
    }

    #[test]
    fn package_version_separates_precedence_from_exact_identity() {
        let first = version("1.0.0+build.1");
        let second = version("1.0.0+build.2");

        assert_ne!(first, second);
        assert_eq!(first.precedence_cmp(&second), Ordering::Equal);
        assert_ne!(first.exact_cmp(&second), Ordering::Equal);
    }

    #[test]
    fn version_ranges_use_lattice_caret_and_tilde_semantics() {
        let caret = PackageVersionReq::from_str("^0.2.3")
            .unwrap_or_else(|error| panic!("valid caret range was rejected: {error}"));
        assert_eq!(caret.to_string(), ">=0.2.3 <1.0.0");
        assert!(caret.matches(&version("0.2.3")));
        assert!(caret.matches(&version("0.9.9")));
        assert!(!caret.matches(&version("1.0.0")));
        let encoded = serde_json::to_string(&caret).unwrap_or_default();
        assert_eq!(
            serde_json::from_str::<PackageVersionReq>(&encoded).ok(),
            Some(caret)
        );

        let tilde = PackageVersionReq::from_str("~0.2.3")
            .unwrap_or_else(|error| panic!("valid tilde range was rejected: {error}"));
        assert_eq!(tilde.to_string(), ">=0.2.3 <0.3.0");
        assert!(tilde.matches(&version("0.2.9")));
        assert!(!tilde.matches(&version("0.3.0")));
    }

    #[test]
    fn version_ranges_reject_cargo_shorthand_and_unsupported_grammar() {
        for value in [
            "1.2.3",
            "1.2",
            "*",
            "1.x",
            ">=1.0.0 || <2.0.0",
            "1.2.3 - 2.0.0",
            ">=1.0.0, <2.0.0",
            "^1.2",
            "~1",
        ] {
            assert!(
                PackageVersionReq::from_str(value).is_err(),
                "expected `{value}` to be rejected"
            );
        }
    }

    #[test]
    fn version_ranges_gate_pre_release_candidates_by_matching_core() {
        let release_only = PackageVersionReq::from_str(">=1.0.0 <2.0.0")
            .unwrap_or_else(|error| panic!("valid bounded range was rejected: {error}"));
        assert!(!release_only.matches(&version("1.1.0-alpha.1")));

        let explicit_pre = PackageVersionReq::from_str(">=1.1.0-alpha.1 <2.0.0")
            .unwrap_or_else(|error| panic!("valid prerelease range was rejected: {error}"));
        assert!(explicit_pre.matches(&version("1.1.0-beta.1")));
        assert!(!explicit_pre.matches(&version("1.2.0-alpha.1")));
        assert!(explicit_pre.matches(&version("1.2.0")));
    }

    #[test]
    fn world_id_displays_and_round_trips_as_a_uuid() {
        const TEXT: &str = "018f5f3c-7c45-7e89-b321-0123456789ab";
        let parsed = WorldId::from_str(TEXT);
        assert!(parsed.is_ok());
        let world_id = parsed.unwrap_or_else(|error| panic!("valid UUID was rejected: {error}"));
        assert_eq!(world_id.to_string(), TEXT);

        let encoded = serde_json::to_string(&world_id).unwrap_or_default();
        let decoded = serde_json::from_str::<WorldId>(&encoded);
        assert_eq!(decoded.ok(), Some(world_id));
        assert!(serde_json::from_str::<WorldId>(r#""not-a-uuid""#).is_err());
    }

    #[test]
    fn string_identifiers_round_trip_through_json() {
        let package = PackageName::from_str("@terrenia/worldgen")
            .unwrap_or_else(|error| panic!("valid package name was rejected: {error}"));
        let stable = StableId::from_str("terrenia:dimension/terrenia")
            .unwrap_or_else(|error| panic!("valid stable ID was rejected: {error}"));
        let capability = CapabilityId::from_str("latticeaxiom:capability/worldgen@1")
            .unwrap_or_else(|error| panic!("valid capability ID was rejected: {error}"));
        let schema = SchemaId::from_str("latticeaxiom:schema/world-header@1")
            .unwrap_or_else(|error| panic!("valid schema ID was rejected: {error}"));
        let source = SourceId::from_str("latticeaxiom:source/terrenia")
            .unwrap_or_else(|error| panic!("valid source ID was rejected: {error}"));

        assert_eq!(json_round_trip(&package), package);
        assert_eq!(json_round_trip(&stable), stable);
        assert_eq!(json_round_trip(&capability), capability);
        assert_eq!(json_round_trip(&schema), schema);
        assert_eq!(json_round_trip(&source), source);
    }

    fn json_round_trip<T>(value: &T) -> T
    where
        T: Clone + for<'de> Deserialize<'de> + Serialize,
    {
        let encoded = serde_json::to_string(value).unwrap_or_default();
        serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("round trip failed: {error}"))
    }

    fn version(value: &str) -> PackageVersion {
        PackageVersion::from_str(value)
            .unwrap_or_else(|error| panic!("test package version was invalid: {error}"))
    }
}
