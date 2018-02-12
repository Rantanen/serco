
extern crate serde;
use serde::{Serializer, Serialize, Deserializer};
use serde::de::DeserializeOwned;

#[cfg(test)]
#[macro_use] extern crate serde_json;
#[cfg(test)]
#[macro_use] extern crate serde_derive;

/// A service contract that is callable through dynamic messages.
///
/// Used through #[derive(ServiceContract)] on the service contract traits.
pub trait ServiceContract {

    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        output : S
    ) -> Result<(), ()>
        where
            D: Deserializer<'de>,
            S: Serializer;
}

/// An implementation for the service contract `TContract`.
///
/// Needed so that we can call `ServiceContract` methods on a struct that
/// implements `TContract`, which implements `ServiceContract`.
///
/// The generic parameter really has no other purpose than to allow
/// implementing multiple Services on a single struct:
///
/// ```
/// impl Service<MyService> for AStruct { ... }
/// impl Service<AnotherService> for AStruct { ... }
/// ```
///
/// Used through `#[derive(Service<MyService>)]`.
pub trait Service<TContract: ServiceContract + ?Sized + 'static> {

    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        output : S
    ) -> Result<(), ()>
        where
            D: Deserializer<'de>,
            S: Serializer;
}

/// A service forwarder used by the proxy implementation.
///
/// Essentially this defines the 'invoke' method of Service, but with the
/// Serialize/Deserialize parameters reversed since the client needs to
/// serialize the params, which the service deserializes and so on.
///
/// Defined by the concrete service host.
pub trait Forwarder {

    fn forward<'de, D, S>(
        &self,
        name: &str,
        params : S,
    ) -> Result<D, ()>
        where
            D: DeserializeOwned + 'static,
            S: Serialize + 'static;
}

/// A proxy object that holds a proxy forwarder and can implement
/// the service contracts.
pub struct ServiceProxy<S: ?Sized, F>
{
    pub forwarder: F,
    phantom_data: std::marker::PhantomData<S>,
}

impl<S: ?Sized, F> ServiceProxy<S, F> {
    pub fn new( f: F ) -> ServiceProxy<S, F> {
        ServiceProxy { forwarder: f, phantom_data: std::marker::PhantomData }
    }
}
