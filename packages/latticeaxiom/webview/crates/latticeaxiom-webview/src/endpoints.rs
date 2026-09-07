use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, btree_map::Entry};

/// Package-owned synchronous operation over the product's chosen context.
///
/// A Bevy product can use `World` as its context without making this library
/// depend on Bevy. The function must validate parameters and enforce its domain
/// rules. Asynchronous work should enqueue a bounded product-owned operation.
pub type EndpointHandler<C> = fn(&mut C, &Value) -> Result<Value, String>;

/// Client-visible description of one explicitly installed package operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EndpointMetadata {
    /// Scoped source package identity, for example `@example/metrics`.
    pub owner: String,
    /// Fully qualified method, for example `@example/metrics:read`.
    pub method: String,
    /// Short human-readable name for diagnostics or an author-provided panel.
    pub title: String,
    /// Description of the operation's behavior and domain requirements.
    pub description: String,
}

/// Rejected installation of a package-owned method.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndpointRegistrationError {
    /// The owner is not a lowercase scoped package identity.
    #[error("invalid endpoint package owner: {0}")]
    InvalidOwner(String),
    /// The method's exact namespace does not belong to its declared owner.
    #[error("endpoint method {method} does not belong to package {owner}")]
    OwnerMismatch {
        /// Declared package identity.
        owner: String,
        /// Attempted fully qualified method.
        method: String,
    },
    /// The local operation name is empty or contains unsupported characters.
    #[error("invalid endpoint method: {0}")]
    InvalidMethod(String),
    /// Another operation already owns this exact method.
    #[error("endpoint method is already installed: {0}")]
    DuplicateMethod(String),
}

struct Endpoint<C> {
    metadata: EndpointMetadata,
    handler: EndpointHandler<C>,
}

/// Explicit installation and stable lookup of author-owned operations.
///
/// Constructing this registry, importing a package, or projecting metadata never
/// activates domain systems or invokes an operation. Products explicitly choose
/// registrations. Namespaces prevent accidental collisions; they do not sandbox
/// trusted native source packages or replace product admission checks.
///
/// A handler lookup returns a copied function pointer so a product can release
/// its registry resource borrow before passing its world to the handler.
pub struct EndpointRegistry<C> {
    endpoints: BTreeMap<String, Endpoint<C>>,
}

impl<C> Default for EndpointRegistry<C> {
    fn default() -> Self {
        Self {
            endpoints: BTreeMap::new(),
        }
    }
}

impl<C> std::fmt::Debug for EndpointRegistry<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EndpointRegistry")
            .field("methods", &self.endpoints.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl<C> EndpointRegistry<C> {
    /// Install an operation without invoking it or mutating the domain context.
    ///
    /// # Errors
    /// Rejects malformed owners/methods, owner mismatches, and duplicate methods.
    /// A rejected registration preserves the existing operation.
    pub fn register(
        &mut self,
        metadata: EndpointMetadata,
        handler: EndpointHandler<C>,
    ) -> Result<(), EndpointRegistrationError> {
        validate(&metadata)?;
        match self.endpoints.entry(metadata.method.clone()) {
            Entry::Occupied(_) => Err(EndpointRegistrationError::DuplicateMethod(metadata.method)),
            Entry::Vacant(entry) => {
                entry.insert(Endpoint { metadata, handler });
                Ok(())
            }
        }
    }

    /// Look up a copied handler, ending the registry borrow before invocation.
    /// Unknown methods return `None` and never invoke a fallback implicitly.
    #[must_use]
    pub fn lookup(&self, method: &str) -> Option<EndpointHandler<C>> {
        self.endpoints.get(method).map(|endpoint| endpoint.handler)
    }

    /// Project installed endpoint metadata in stable fully qualified method order.
    /// This iterator does not invoke handlers or expose the domain context.
    #[must_use]
    pub fn metadata(&self) -> impl ExactSizeIterator<Item = &EndpointMetadata> {
        self.endpoints.values().map(|endpoint| &endpoint.metadata)
    }
}

fn validate(metadata: &EndpointMetadata) -> Result<(), EndpointRegistrationError> {
    let owner_valid = metadata
        .owner
        .strip_prefix('@')
        .and_then(|owner| owner.split_once('/'))
        .is_some_and(|(scope, package)| owner_component(scope) && owner_component(package));
    if !owner_valid {
        return Err(EndpointRegistrationError::InvalidOwner(
            metadata.owner.clone(),
        ));
    }
    let prefix = format!("{}:", metadata.owner);
    let Some(local) = metadata.method.strip_prefix(&prefix) else {
        return Err(EndpointRegistrationError::OwnerMismatch {
            owner: metadata.owner.clone(),
            method: metadata.method.clone(),
        });
    };
    if local.is_empty()
        || !local.as_bytes()[0].is_ascii_alphanumeric()
        || !local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(EndpointRegistrationError::InvalidMethod(
            metadata.method.clone(),
        ));
    }
    Ok(())
}

fn owner_component(value: &str) -> bool {
    !value.is_empty()
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(method: &str) -> EndpointMetadata {
        EndpointMetadata {
            owner: "@example/metrics".into(),
            method: method.into(),
            title: "Metrics".into(),
            description: "Read a package-owned metric".into(),
        }
    }

    #[test]
    fn rejects_duplicate_without_replacing_the_existing_handler()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = EndpointRegistry::<u64>::default();
        registry.register(metadata("@example/metrics:read"), |context, _| {
            Ok(Value::from(*context))
        })?;
        assert!(matches!(
            registry.register(metadata("@example/metrics:read"), |_, _| Ok(Value::Null)),
            Err(EndpointRegistrationError::DuplicateMethod(_))
        ));
        let handler = registry
            .lookup("@example/metrics:read")
            .ok_or("missing endpoint")?;
        drop(registry);
        assert_eq!(handler(&mut 7, &Value::Null)?, Value::from(7));
        Ok(())
    }

    #[test]
    fn validates_exact_package_namespace_and_unknown_methods() {
        let mut registry = EndpointRegistry::<()>::default();
        for method in ["@example/other:read", "@example/metrics-extra:read", "read"] {
            assert!(matches!(
                registry.register(metadata(method), |(), _| Ok(Value::Null)),
                Err(EndpointRegistrationError::OwnerMismatch { .. })
            ));
        }
        for method in [
            "@example/metrics:",
            "@example/metrics:read:other",
            "@example/metrics:../read",
        ] {
            assert!(matches!(
                registry.register(metadata(method), |(), _| Ok(Value::Null)),
                Err(EndpointRegistrationError::InvalidMethod(_))
            ));
        }
        let mut invalid_owner = metadata("@example/metrics:read");
        invalid_owner.owner = "example/metrics".into();
        assert!(matches!(
            registry.register(invalid_owner, |(), _| Ok(Value::Null)),
            Err(EndpointRegistrationError::InvalidOwner(_))
        ));
        assert!(registry.lookup("@example/metrics:read").is_none());
        assert_eq!(registry.metadata().len(), 0);
    }

    #[test]
    fn metadata_is_stable_and_does_not_invoke_operations() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut registry = EndpointRegistry::<()>::default();
        registry.register(metadata("@example/metrics:z"), |(), _| {
            Err("not invoked".into())
        })?;
        registry.register(metadata("@example/metrics:a"), |(), _| {
            Err("not invoked".into())
        })?;
        assert_eq!(
            registry
                .metadata()
                .map(|row| row.method.as_str())
                .collect::<Vec<_>>(),
            vec!["@example/metrics:a", "@example/metrics:z"]
        );
        assert!(serde_json::to_value(registry.metadata().collect::<Vec<_>>())?.is_array());
        Ok(())
    }
}
