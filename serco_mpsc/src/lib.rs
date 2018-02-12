
extern crate futures;
use futures::prelude::*;
use futures::sync::oneshot;

extern crate tokio_core;
use tokio_core::reactor::Core;

extern crate serco;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate erased_serde;

use serde::*;
use serde::de::DeserializeOwned;

use futures::sync::mpsc::{Sender, Receiver};

/// Envelope used by the MPSC endpoints to communicate the calls.
#[derive(Serialize, Deserialize)]
struct Envelope<'a> {
    name : &'a str,
    params: serde_json::Value,
}

/// Service endpoint that hosts the services.
pub struct MpscServiceEndpoint<T: ?Sized> {
    phantom_data: std::marker::PhantomData<T>
}

impl<S: serco::ServiceContract + ?Sized + 'static> MpscServiceEndpoint<S> {

    /// Opens a new endpoint.
    pub fn run<TService>(
        rx: Receiver<( String, oneshot::Sender<String> )>,
        service: TService,
    )
        where TService: serco::Service<S> + Send + 'static
    {
        let service = std::rc::Rc::new( service );
        let mut core = Core::new().expect( "Failed to create core" );

        // Turn each message into a future.
        core.run( rx.for_each( |( msg, tx )| {
            let service = service.clone();

            let f = futures::future::lazy( move || {

                let envelope : Envelope = serde_json::from_str(&msg).unwrap();
                let output = serde_json::Serializer::new( vec![] );
                service.invoke(
                        envelope.name,
                        envelope.params,
                        output )
                } )
                .then( move |result| {
                    match result {
                        Ok(output) => unsafe {
                            tx.send( String::from_utf8_unchecked( output.into_inner() ) )
                                .map_err( |e| serco::ServiceError::from(e) )
                        },
                        Err(e) => Err( serco::ServiceError::from(e) )
                    }
                } )
                .map_err( |e| {
                    panic!( "{:?}", e.0 );
                } )
                .map_err( |_| () );

            Box::new( f )
        } ) ).unwrap();
    }
}

/// Service connection used by the client implementation.
///
/// (Since the real functionality is in the forwarder, this should probably
/// move to the framework at some point)
pub struct MpscServiceConnection<T : ?Sized> {
    proxy: serco::ServiceProxy<T, MpscServiceForwarder>,
    phantom_data: std::marker::PhantomData<T>,
}

impl<T: serco::ServiceContract + ?Sized> MpscServiceConnection<T> {

    /// Connects to an MPSC endpoint.
    pub fn connect( tx: Sender<( String, oneshot::Sender<String> )> ) -> MpscServiceConnection<T>
    {
        let forwarder = MpscServiceForwarder { tx };
        
        MpscServiceConnection {
            proxy: serco::ServiceProxy::new( forwarder ),
            phantom_data: std::marker::PhantomData,
        }
    }

    pub fn close( self ) {
        self.proxy.close();
    }
}

/// Dereference the connection into the service proxy.
/// The proxy implements the actual service trait.
impl<T: serco::ServiceContract + ?Sized> std::ops::Deref for MpscServiceConnection<T>
{
    type Target = serco::ServiceProxy<T, MpscServiceForwarder>;

    fn deref( &self ) -> &Self::Target {
        &self.proxy
    }
}

/// Proxy forwarder that can take the method calls from the client and turns
/// them into messages that can be passed to the service host.
pub struct MpscServiceForwarder {
    tx: Sender<( String, oneshot::Sender<String> )>,
}

impl serco::Forwarder for MpscServiceForwarder
{
    fn forward<D, S>(
        &self,
        name: &'static str,
        params: S
    ) -> Box<Future<Item=D, Error=serco::ServiceError>>
        where
            D: DeserializeOwned + 'static,
            S: Serialize + 'static,
    {
        let tx = self.tx.clone();
        Box::new( futures::future::lazy( move || {
            let value = serde_json::to_value( params ).unwrap();
            let envelope = Envelope {
                name: name,
                params: value,
            };
            let msg = serde_json::to_string( &envelope ).unwrap();

            let (tx_once, rx_once) = oneshot::channel();
            tx.send( ( msg, tx_once ) )
                .map_err( |e| serco::ServiceError::from(e) )
                .then( |_| rx_once )
        } )
        .then( |response| {
            match response {
                Ok(data)
                    => serde_json::from_str( &data )
                            .map_err( |e| serco::ServiceError::from( e ) ),
                Err(e) => Err( serco::ServiceError::from( e ) )
            }
        } ) )
    }

    fn close( mut self ) {
        self.tx.close().expect( "Failed to close tx" );
    }
}

