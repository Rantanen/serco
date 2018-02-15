extern crate futures;
use futures::prelude::*;

extern crate serde;
use serde::{Serializer, Serialize, Deserializer};
use serde::de::DeserializeOwned;
#[macro_use] extern crate serde_derive;

// The crate doesn't really need the macros. However Rust will complain that
// the import does nothing if we don't define #[macro_use]. Once we define
// #[macro_use] to get rid of that warning, Rust will complain that the
// #[macro_use] does nothing. Fortunately THAT warning comes with a named
// warning option so we can allow that explicitly.
//
// Unfortunately clippy disagrees on the macro_use being unused and claims that
// the unused_imports attribute is useless. So now we also need to tell clippy
// to ignore useless attributes in this scenario! \:D/
//
// Intercom encountered the same issue, but I suspect it got "fixed" once it
// started to actually use the proc macros internally. Serco doesn't.
#[cfg_attr(feature = "cargo-clippy", allow(useless_attribute))]
#[allow(unused_imports)]
#[macro_use]
extern crate serco_derive;
pub mod prelude {
    pub use serco_derive::*;
    pub use super::ServiceContract;
}

use std::rc::Rc;
use std::sync::Arc;
use std::marker::PhantomData;
use std::collections::HashMap;
use std::borrow::Cow;

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct ServiceError(pub String);
impl ServiceError {
    pub fn from<T: std::fmt::Debug>( src: T ) -> ServiceError
    {
        ServiceError( format!( "{:?}", src ) )
    }
}

pub struct ServiceHost<
        TService,
        TSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          THostImplementation: HostedService<TService> + 'static,
{
    hosted: THostImplementation,
    session_factory: TSessionFactory,
    endpoints: Vec<Box<ServiceEndpoint<TService, TSessionFactory, THostImplementation>>>,
    sessions: HashMap<String, Rc<THostImplementation::ServiceInstance>>,

    p_service: PhantomData<TService>,
}

impl<TService,
        THostImplementation>
    ServiceHost<
        TService,
        DefaultSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          THostImplementation: HostedService<TService> + 'static,
{
    pub fn new(
        service : THostImplementation
    ) -> ServiceHost<
        TService,
        DefaultSessionFactory,
        THostImplementation,
    >
    {
        ServiceHost {
            hosted: service,

            session_factory: Default::default(),
            endpoints: Default::default(),
            sessions: Default::default(),

            p_service: PhantomData,
        }
    }
}

impl<TService,
        TSessionFactory,
        THostImplementation,
    >
    ServiceHost<
        TService,
        TSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          TSessionFactory: SessionFactory + 'static,
          THostImplementation: HostedService<TService> + 'static,
{
    pub fn session_factory<TNewSessionFactory: SessionFactory + 'static>(
        self,
        session_factory: TNewSessionFactory
    ) -> ServiceHost<
            TService,
            TNewSessionFactory,
            THostImplementation,
        >
    {
        ServiceHost {
            hosted: self.hosted,

            session_factory: session_factory,
            endpoints: Default::default(),
            sessions: Default::default(),

            p_service: PhantomData,
        }
    }

    pub fn endpoint<TEndpoint: ServiceEndpoint<TService, TSessionFactory, THostImplementation> + 'static>(
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

                Ok( ServiceHost {
                    hosted: runtime.hosted,
                    session_factory: runtime.session_factory,
                    sessions: runtime.sessions,
                    endpoints: endpoints,
                    p_service: PhantomData,
                } )
            },
            Err( e ) => Err( ServiceError::from(e) ),
        } );

        Box::new( final_future )
    }
}

pub struct HostRuntime<TService,
        TSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          TSessionFactory: SessionFactory + 'static,
          THostImplementation: HostedService<TService> + 'static,
{
    hosted: THostImplementation,
    session_factory: TSessionFactory,
    sessions: HashMap<String, Rc<THostImplementation::ServiceInstance>>,
}

pub trait SessionInfo {
    fn key(&self) -> Cow<str>;
}

impl SessionInfo for SessionId {
    fn key(&self) -> Cow<str> { Cow::from( self.0.as_ref() ) }
}

impl SessionInfo for usize {
    fn key(&self) -> Cow<str> { Cow::from( format!( "{}", self ) ) }
}

impl<TService,
        TSessionFactory,
        THostImplementation,
    >
    HostRuntime<
        TService,
        TSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          TSessionFactory: SessionFactory + 'static,
          THostImplementation: HostedService<TService, SessionInfo=TSessionFactory::SessionInfo> + 'static,
{
    pub fn get_session<'a, 'b>(
        &'a self,
        id: Option<&'b str>
    ) -> ( Cow<'b, str>, THostImplementation::ServiceInstance )
    {
        match id {
            Some( id ) => {
                let session_info = self.session_factory.get_session( id );
                let session = self.hosted.get_session( session_info );
                ( Cow::from( id ), session )
            }
            None => {
                let ( id, session_info ) = self.session_factory.create_session();
                let session = self.hosted.get_session( session_info );
                ( Cow::from( id ), session )
            }
        }
    }
}

pub trait ServiceEndpoint<TService,
        TSessionFactory,
        THostImplementation,
    >
    where TService: ServiceContract + ?Sized + 'static,
          TSessionFactory: SessionFactory,
          THostImplementation: HostedService<TService> + 'static,
{
    fn run(
        &self,
        host: Rc<HostRuntime<
                TService,
                TSessionFactory,
                THostImplementation,
        >>
    ) -> Box<Future<Item=(), Error=ServiceError>>;
}

pub trait SessionFactory {
    type SessionInfo : SessionInfo;
    fn create_session( &self ) -> ( String, Rc<Self::SessionInfo> );
    fn get_session( &self, key: &str ) -> Rc<Self::SessionInfo>;
}
pub struct DefaultSessionFactory;
impl Default for DefaultSessionFactory {
    fn default() -> Self {
        DefaultSessionFactory
    }
}
impl SessionFactory for DefaultSessionFactory {
    type SessionInfo = SessionId;
    fn create_session( &self ) -> ( String, Rc<Self::SessionInfo> )
    {
        // TODO: Implement.
        (
            String::from( "" ),
            Rc::new( SessionId( "".into() ) )
        )
    }
    fn get_session( &self, _key : &str ) -> Rc<Self::SessionInfo>
    {
        // TODO: Implement.
        Rc::new( SessionId( "".into() ) )
    }
}

/// A trait that specifies service contracts.
///
/// Implemented by the `#[service_contract]` attribute.
pub trait ServiceContract : InvokeTarget<Self> {
    type CallbackContract: ServiceContract<CallbackContract = ()> + ?Sized;

    fn set_task_callback<F: Forwarder>( callback : Arc<ServiceProxy<Self, F>> );
    fn get_task_callback() -> Arc<Self>;
}

impl ServiceContract for () {
    type CallbackContract = ();

    fn set_task_callback<F: Forwarder>( _ : Arc<ServiceProxy<Self, F>> ) {}
    fn get_task_callback() -> Arc<Self> { Arc::new(())}
}

impl InvokeTarget<()> for () {
    fn invoke<'de, D, S>(
        &self,
        _name: &str,
        _params : D,
        _output : S
    ) -> Box<Future<Item=S, Error=ServiceError>>
        where
            D: Deserializer<'de>,
            S: 'static,
            for <'a> &'a mut S: Serializer
    {
        // TODO: Replace with bad method service error.
        panic!( "Nothing should ever be invoked on ()" );
    }
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
pub struct SessionId( pub String );

pub trait SessionService<TContract: ServiceContract + ?Sized + 'static>
        : InvokeTarget<TContract>
{
    type SessionInfo : SessionInfo;
    fn construct( session : Rc<Self::SessionInfo> ) -> Box<TContract>;
}

pub mod hosted {
    use super::*;

    pub struct Singleton<S: ?Sized, T, I> {
        pub singleton: std::rc::Rc<T>,
        p_service: std::marker::PhantomData<S>,
        p_session: std::marker::PhantomData<I>,
    }

    impl<S, T, I> Singleton<S, T, I>
        where S: ServiceContract + ?Sized + 'static,
              T: SingletonService<S> + 'static,
    {
        pub fn new( singleton: T ) -> Self {
            Singleton {
                singleton: std::rc::Rc::new( singleton ),
                p_service: std::marker::PhantomData,
                p_session: std::marker::PhantomData,
            }
        }
    }

    impl<S, T, I> HostedService<S> for Singleton<S, T, I>
        where S: ServiceContract + ?Sized + 'static,
              T: SingletonService<S> + 'static,
    {
        type ServiceInstance = std::rc::Rc<T>;
        type SessionInfo = I;

        fn get_session( &self, _ : Rc<I> ) -> Self::ServiceInstance {
            self.singleton.clone()
        }
    }

    pub struct Session<S: ?Sized, T> {
        phantom_service: std::marker::PhantomData<S>,
        phantom_session: std::marker::PhantomData<T>,
    }

    impl<S, T> Session<S, T>
        where S: ServiceContract + ?Sized + 'static,
              T: SessionService<S> + 'static,
    {
        pub fn new() -> Self {
            Session {
                phantom_service: std::marker::PhantomData,
                phantom_session: std::marker::PhantomData,
            }
        }
    }

    impl<S, T> HostedService<S> for Session<S, T>
        where S: ServiceContract + ?Sized + 'static,
              T: SessionService<S> + 'static,
    {
        type ServiceInstance = Box<S>;
        type SessionInfo = T::SessionInfo;

        fn get_session( &self, session_info : Rc<T::SessionInfo> ) -> Self::ServiceInstance {
            T::construct( session_info )
        }
    }

}

pub trait HostedService<S>
    where S: ServiceContract + ?Sized + 'static
{
    type ServiceInstance : InvokeTarget<S>;
    type SessionInfo;
    fn get_session( &self, session_info : Rc<Self::SessionInfo> ) -> Self::ServiceInstance;
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

impl<A, B> InvokeTarget<A> for Box<B>
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
pub trait Forwarder : 'static {

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

unsafe impl<S: ?Sized, F> Send for ServiceProxy<S, F> {}
unsafe impl<S: ?Sized, F> Sync for ServiceProxy<S, F> {}

impl<S: ?Sized, F: Forwarder> ServiceProxy<S, F> {
    pub fn new( f: F ) -> ServiceProxy<S, F> {
        ServiceProxy { forwarder: f, phantom_data: std::marker::PhantomData }
    }

    pub fn close( self ) { self.forwarder.close() }
}
