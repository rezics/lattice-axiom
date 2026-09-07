//! A package library and an independent consumer compose ordinary Rust exports.
//! Merely linking/importing `metrics_package` performs no registration or work.

use latticeaxiom_webview::EndpointRegistry;
use serde_json::Value;

mod metrics_package {
    use latticeaxiom_webview::{EndpointMetadata, EndpointRegistrationError, EndpointRegistry};
    use serde_json::Value;

    #[derive(Debug, Default)]
    pub struct Metrics {
        pub reads: u64,
    }

    pub fn install(
        registry: &mut EndpointRegistry<Metrics>,
    ) -> Result<(), EndpointRegistrationError> {
        registry.register(
            EndpointMetadata {
                owner: "@example/metrics".into(),
                method: "@example/metrics:read".into(),
                title: "Metrics".into(),
                description: "Read this package's metric counter".into(),
            },
            |context, _| {
                context.reads += 1;
                Ok(Value::from(context.reads))
            },
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut context = metrics_package::Metrics::default();
    let mut registry = EndpointRegistry::default();
    assert_eq!(registry.metadata().len(), 0);
    assert_eq!(context.reads, 0);

    // This explicit product installation is separate from importing the library.
    metrics_package::install(&mut registry)?;
    assert_eq!(context.reads, 0);
    let handler = registry
        .lookup("@example/metrics:read")
        .ok_or("missing endpoint")?;
    // A Bevy product can now release its registry resource borrow and pass World.
    let value = handler(&mut context, &Value::Null)?;
    assert_eq!(value, Value::from(1));
    Ok(())
}
