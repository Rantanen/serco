extern crate futures;
use futures::prelude::*;

extern crate serde;
use serde::{Serializer, Serialize, Deserializer};
use serde::de::DeserializeOwned;
#[macro_use] extern crate serde_derive;

use std::rc::Rc;
use std::marker::PhantomData;
use std::collections::HashMap;

#[cfg(test)]
#[macro_use] extern crate serde_json;
#[cfg(test)]
#[macro_use] extern crate serde_derive;

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct ServiceError(pub String);
impl ServiceError {
    pub fn from<T: std::fmt::Debug>( src: T ) -> ServiceError
    {
        ServiceError( format!( "{:?}", src ) )
    }
}

pub struct ServiceHost2<
        TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
{
    hosted: THostImplementation,
    session_factory: TSessionFactory,
    endpoints: Vec<Box<ServiceEndpoint<TService, TImplementation, THostImplementation, TSessionFactory>>>,
    sessions: HashMap<SessionInfo, Rc<THostImplementation::ServiceInstance>>,

    p_service: PhantomData<TService>,
    p_implementation: PhantomData<TImplementation>,
}

impl<TService,
        TImplementation,
        THostImplementation>
    ServiceHost2<
        TService,
        TImplementation,
        THostImplementation,
        DefaultSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
{
    pub fn new(
        service : THostImplementation
    ) -> ServiceHost2<
        TService,
        TImplementation,
        THostImplementation,
        DefaultSessionFactory>
    {
        ServiceHost2 {
            hosted: service,

            session_factory: Default::default(),
            endpoints: Default::default(),
            sessions: Default::default(),

            p_service: PhantomData,
            p_implementation: PhantomData,
        }
    }
}

impl<TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    ServiceHost2<
        TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
          TSessionFactory: SessionFactory + 'static,
{
    pub fn endpoint<TEndpoint: ServiceEndpoint<TService, TImplementation, THostImplementation, TSessionFactory> + 'static>(
        mut self,
        endpoint: TEndpoint
    ) -> Self
    {
        self.endpoints.push( Box::new( endpoint ) );
        self
    }

    pub fn run( self ) -> Box<Future<Item=Self, Error=ServiceError>>
    {
        let runtime = Rc::new( HostRuntime {
            hosted: self.hosted,
            session_factory: self.session_factory,
            sessions: self.sessions,
        } );

        let runtime_clone = runtime.clone();
        let run_futures = futures::future::join_all( self.endpoints
            .into_iter()
            .map( move |endpoint| {
                endpoint.run( runtime_clone.clone() ).map( |_| endpoint )
            } ) );

        let final_future = run_futures.then( |run_results| match run_results {
            Ok(endpoints) => {
                let runtime = Rc::try_unwrap( runtime )
                            .map_err( |_| "Leaking RCs" )
                            .unwrap();

                Ok( ServiceHost2 {
                    hosted: runtime.hosted,
                    session_factory: runtime.session_factory,
                    sessions: runtime.sessions,
                    endpoints: endpoints,
                    p_service: PhantomData,
                    p_implementation: PhantomData,
                } )
            },
            Err( e ) => Err( ServiceError::from(e) ),
        } );

        Box::new( final_future )
    }
}

pub struct HostRuntime<TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
          TSessionFactory: SessionFactory + 'static,
{
    hosted: THostImplementation,
    session_factory: TSessionFactory,
    sessions: HashMap<SessionInfo, Rc<THostImplementation::ServiceInstance>>,
}

impl<TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    HostRuntime<
        TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
          TSessionFactory: SessionFactory + 'static,
{
    pub fn get_session( &self, id: Option<SessionInfo> ) -> Rc<THostImplementation::ServiceInstance>
    {
        Rc::new( self.hosted.get_session( SessionInfo(String::from("")) ) )
    }
}

pub trait ServiceEndpoint<TService,
        TImplementation,
        THostImplementation,
        TSessionFactory>
    where TService: ServiceContract + ?Sized + 'static,
          TImplementation: InvokeTarget<TService> + 'static,
          THostImplementation: HostedService<TService, TImplementation> + 'static,
          TSessionFactory: SessionFactory,
{
    fn run(
        &self,
        host: Rc<HostRuntime<
                TService,
                TImplementation,
                THostImplementation,
                TSessionFactory>>
    ) -> Box<Future<Item=(), Error=ServiceError>>;
}

pub trait SessionFactory {}
pub struct DefaultSessionFactory;
impl Default for DefaultSessionFactory {
    fn default() -> Self {
        DefaultSessionFactory
    }
}
impl SessionFactory for DefaultSessionFactory {}

/// A trait that specifies service contracts.
///
/// Implemented by the `#[service_contract]` attribute.
pub trait ServiceContract : InvokeTarget<Self> {}

pub trait ServiceHost {
    fn host<S, T, H>( self, service: H ) -> Self
        where S: ServiceContract + ?Sized + 'static,
              T: InvokeTarget<S> + 'static,
              H: HostedService<S, T> + 'static;

    fn run( self ) -> Box<Future<Item=Self, Error=ServiceError>>;
}

pub trait SingletonEndpoint<S>
    where S: ServiceContract + ?Sized + 'static
{
    fn singleton_host<T: SingletonService<S> + Send + 'static>( service: T ) -> Self;
}

pub trait SingletonService<TContract: ServiceContract + ?Sized + 'static>
        : InvokeTarget<TContract>
{
    fn service( self ) -> Box<TContract>;
}

#[derive(PartialEq, Eq, Hash)]
pub struct SessionInfo( pub String );

pub trait SessionService<TContract: ServiceContract + ?Sized + 'static>
        : InvokeTarget<TContract>
{
    fn construct( session : SessionInfo ) -> Box<TContract>;
}

pub mod hosted {
    use super::*;

    pub struct Singleton<S: ?Sized, T> {
        endpoint: &'static str,
        pub singleton: std::rc::Rc<T>,
        phantom_data: std::marker::PhantomData<S>,
    }

    impl<S, T> Singleton<S, T>
        where S: ServiceContract + ?Sized + 'static,
              T: SingletonService<S> + 'static,
    {
        pub fn new( singleton: T, endpoint: &'static str ) -> Self {
            Singleton {
                endpoint,
                singleton: std::rc::Rc::new( singleton ),
                phantom_data: std::marker::PhantomData
            }
        }
    }

    impl<S, T> HostedService<S, T> for Singleton<S, T>
        where S: ServiceContract + ?Sized + 'static,
              T: SingletonService<S> + 'static,
    {
        type ServiceInstance = std::rc::Rc<T>;
        fn endpoint(&self) -> &'static str { self.endpoint }
        fn get_session( &self, session_info : SessionInfo ) -> Self::ServiceInstance {
            self.singleton.clone()
        }
    }

    pub struct Session<S: ?Sized, T> {
        endpoint: &'static str,
        phantom_service: std::marker::PhantomData<S>,
        phantom_session: std::marker::PhantomData<T>,
    }

    impl<S, T> Session<S, T>
        where S: ServiceContract + ?Sized + 'static,
              T: SessionService<S> + 'static,
    {
        pub fn new( endpoint: &'static str ) -> Self {
            Session {
                endpoint,
                phantom_service: std::marker::PhantomData,
                phantom_session: std::marker::PhantomData,
            }
        }
    }
}

pub trait HostedService<S, T>
    where S: ServiceContract + ?Sized + 'static,
          T: InvokeTarget<S> + 'static,
{
    type ServiceInstance : InvokeTarget<S>;
    fn endpoint(&self) -> &'static str;
    fn get_session( &self, session_info : SessionInfo ) -> Self::ServiceInstance;
}

pub trait InvokeTarget<Service>
    where Service: ServiceContract + ?Sized
{
    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        output : S
    ) -> Box<Future<Item=S, Error=ServiceError>>
        where
            D: Deserializer<'de>,
            S: 'static,
            for <'a> &'a mut S: Serializer;
}

impl<A, B> InvokeTarget<A> for std::rc::Rc<B>
    where A: ServiceContract + ?Sized,
          B: InvokeTarget<A> + ?Sized,
{
    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        output : S
    ) -> Box<Future<Item=S, Error=ServiceError>>
        where
            D: Deserializer<'de>,
            S: 'static,
            for <'a> &'a mut S: Serializer
    {
        ( self as &B ).invoke( name, params, output )
    }
}

/// A service forwarder used by the proxy implementation.
///
/// Essentially this defines the 'invoke' method of Service, but with the
/// Serialize/Deserialize parameters reversed since the client needs to
/// serialize the params, which the service deserializes and so on.
///
/// Defined by the concrete service host.
pub trait Forwarder {

    fn forward<D, S>(
        &self,
        name: &'static str,
        params : S,
    ) -> Box<Future<Item=D, Error=ServiceError>>
        where
            D: DeserializeOwned + 'static,
            S: Serialize + 'static;

    fn close( self );
}

/// A proxy object that holds a proxy forwarder and can implement
/// the service contracts.
pub struct ServiceProxy<S: ?Sized, F>
{
    pub forwarder: F,
    phantom_data: std::marker::PhantomData<S>,
}

impl<S: ?Sized, F: Forwarder> ServiceProxy<S, F> {
    pub fn new( f: F ) -> ServiceProxy<S, F> {
        ServiceProxy { forwarder: f, phantom_data: std::marker::PhantomData }
    }

    pub fn close( self ) { self.forwarder.close() }
}
