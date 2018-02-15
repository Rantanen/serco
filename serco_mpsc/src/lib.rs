
#[macro_use] extern crate lazy_static;

extern crate futures;
use futures::prelude::*;
use futures::sync::oneshot;

extern crate tokio_core;
use tokio_core::reactor::{Core};

extern crate serco;
use serco::{InvokeTarget, ServiceContract};
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate erased_serde;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use serde::*;
use serde::de::DeserializeOwned;

use futures::sync::mpsc::{Sender, channel};

/// Envelope used by the MPSC endpoints to communicate the calls.
#[derive(Debug, Serialize, Deserialize)]
struct RequestEnvelope<'a> {
    name: &'a str,
    params: serde_json::Value,
}

/// Envelope used by the MPSC endpoints to communicate the calls.
#[derive(Debug, Serialize, Deserialize)]
struct ResponseEnvelope {
    result: Result<serde_json::Value, serco::ServiceError>,
}


pub struct MpscEndpoint {
    endpoint: String,
}

impl MpscEndpoint {
    pub fn new<T: Into<String>>( endpoint: T ) -> Self {
        Self { endpoint: endpoint.into() }
    }
}

impl<TService,
        THostImplementation,
        TSessionFactory>
    serco::ServiceEndpoint<TService,
        TSessionFactory,
        THostImplementation,
    >
    for MpscEndpoint
    where TService: serco::ServiceContract + ?Sized + 'static,
          TSessionFactory: serco::SessionFactory + 'static,
          THostImplementation: serco::HostedService<TService, SessionInfo=TSessionFactory::SessionInfo> + 'static,
{
    fn run(
        &self,
        host: Rc<serco::HostRuntime<
                TService,
                TSessionFactory,
                THostImplementation,
        >>
    ) -> Box<Future<Item=(), Error=serco::ServiceError>>
    {
        let (endpoint_tx, endpoint_rx) = channel(1);
        set_endpoint( self.endpoint.clone(), endpoint_tx );

        let result = endpoint_rx.for_each( move |(client_tx, callback_tx)| {

            let ( session_id, session ) = host.get_session( None );
            let session = Rc::new( session );

            let (tx, rx) = channel(1);
            client_tx.send(( session_id.to_string(), tx )).unwrap();

            let forwarder = Arc::new( serco::ServiceProxy::new(
                    MpscForwarder {
                        _id: session_id.to_string(),
                        tx: callback_tx,
                    } ) );

            rx.for_each( move |(msg, response_tx)| {

                let forwarder = forwarder.clone();
                let session = session.clone();
                
                let output = serde_json::Serializer::new( vec![] );
                TService::CallbackContract::set_task_callback( forwarder );
                let envelope : RequestEnvelope =
                        serde_json::from_str(&msg).unwrap();
                session.invoke(
                        envelope.name,
                        envelope.params,
                        output )
                .then( |result| {
                    Ok( match result {
                        Ok( ok ) => {
                            ResponseEnvelope {
                                result: Ok( serde_json::from_slice( &ok.into_inner() ).unwrap() )
                            }
                        }
                        Err( e ) => ResponseEnvelope {
                            result: Err( serco::ServiceError::from(e) )
                        },
                    }  )
                } )
                .map( |response| {
                       let json = serde_json::to_string( &response ).unwrap();
                       response_tx.send( json )
                } )
                .map( |_| () )
            } )

        } );

        // TODO: Report issue on bad diagnostics on missing map_err here.
        Box::new( result.map_err( |e| serco::ServiceError::from(e) ) )
    }
}



















use std::sync::Mutex;
lazy_static! {
    static ref ENDPOINTS : Mutex<HashMap<String, Endpoint>>
            = Mutex::new( HashMap::new() );
}
type Endpoint = Sender<(  // Host listen callback.
    oneshot::Sender<(     // Client on-connect callback
        String,           // Session ID
        Sender<(          // Client request pipe
            String,
            oneshot::Sender<String>
        )>
    )>,
    Sender<( String, oneshot::Sender<String>)>  // Server callback pipe
)>;

pub fn get_endpoint( name : &str ) -> Option<Endpoint>
{
    let guard = ENDPOINTS.lock().unwrap();
    guard.get( name ).map( |tx| tx.clone() )
}

pub fn set_endpoint<T: Into<String>>( name : T, endpoint : Endpoint )
{
    let mut guard = ENDPOINTS.lock().unwrap();
    guard.insert( name.into(), endpoint );
}

pub struct MpscClient {
    endpoint : String,
}

impl MpscClient {
    pub fn new<T: Into<String>>( endpoint: T ) -> MpscClient {
        MpscClient {
            endpoint: endpoint.into(),
        }
    }

    pub fn connect<S>(
        &self,
    ) -> Box<Future<Item=MpscServiceConnection<S>, Error=String>>
        where S: serco::ServiceContract<CallbackContract = ()> + ?Sized + 'static,
    {
        MpscServiceConnection::<S>::connect(
                &self.endpoint, () )
    }

    pub fn connect_duplex<S, C, T>(
        &self,
        callback: T,
    ) -> Box<Future<Item=MpscServiceConnection<S>, Error=String>>
        where S: serco::ServiceContract<CallbackContract = C> + ?Sized + 'static,
            C: serco::ServiceContract<CallbackContract = ()> + ?Sized + 'static,
            T: serco::InvokeTarget<C> + Send + 'static,
    {
        MpscServiceConnection::<S>::connect(
                &self.endpoint, callback )
    }

}

/// Service connection used by the client implementation.
///
/// (Since the real functionality is in the forwarder, this should probably
/// move to the framework at some point)
pub struct MpscServiceConnection<T : ?Sized> {
    proxy: serco::ServiceProxy<T, MpscForwarder>,
    phantom_data: std::marker::PhantomData<T>,
    _callback_handle: std::thread::JoinHandle<()>,
}

impl<T: serco::ServiceContract + ?Sized + 'static> MpscServiceConnection<T> {

    /// Connects to an MPSC endpoint.
    pub fn connect<C>(
        host_endpoint: &str,
        callback: C,
    ) -> Box<Future<Item=MpscServiceConnection<T>, Error=String>>
        where C: serco::InvokeTarget<T::CallbackContract> + Send + 'static
    {
        let endpoint = get_endpoint( host_endpoint ).unwrap();
        let ( tx, rx ) = oneshot::channel();
        let ( callback_tx, callback_rx ) =
                channel::<(String, oneshot::Sender<String>)>(1);

        let join_handle = std::thread::spawn( move || {
            let mut core = Core::new().expect( "Failed to spawn callback core" );
            core.run( callback_rx.for_each( move |(msg, response_tx)| {

                let output = serde_json::Serializer::new( vec![] );
                let envelope : RequestEnvelope =
                        serde_json::from_str(&msg).unwrap();
                callback.invoke(
                        envelope.name,
                        envelope.params,
                        output )
                .then( |result| {
                    Ok( match result {
                        Ok( ok ) => {
                            ResponseEnvelope {
                                result: Ok( serde_json::from_slice( &ok.into_inner() ).unwrap() )
                            }
                        }
                        Err( e ) => ResponseEnvelope {
                            result: Err( serco::ServiceError::from(e) )
                        },
                    }  )
                } )
                .map( |response| {
                       let json = serde_json::to_string( &response ).unwrap();
                       response_tx.send( json )
                } )
                .map( |_| () )

            } ) ).unwrap();
        } );

        Box::new( endpoint.send(( tx, callback_tx ))
            .then( |_| rx )
            .map( |( id, connection_tx )| {

                let forwarder = MpscForwarder {
                    _id: id,
                    tx: connection_tx,
                };
                
                MpscServiceConnection {
                    proxy: serco::ServiceProxy::new( forwarder ),
                    phantom_data: std::marker::PhantomData,
                    _callback_handle: join_handle,
                }
            } )
            .map_err( |e| format!( "{:?}", e ) ) )
    }

    pub fn close( self ) {
        self.proxy.close();
    }
}

/// Dereference the connection into the service proxy.
/// The proxy implements the actual service trait.
impl<T: serco::ServiceContract + ?Sized> std::ops::Deref for MpscServiceConnection<T>
{
    type Target = serco::ServiceProxy<T, MpscForwarder>;

    fn deref( &self ) -> &Self::Target {
        &self.proxy
    }
}

/// Proxy forwarder that can take the method calls from the client and turns
/// them into messages that can be passed to the service host.
pub struct MpscForwarder {
    _id: String,
    tx: Sender<( String, oneshot::Sender<String> )>,
}

impl serco::Forwarder for MpscForwarder
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
        let value = serde_json::to_value( params ).unwrap();
        let envelope = RequestEnvelope {
            name: name,
            params: value,
        };
        let msg = serde_json::to_string( &envelope ).unwrap();

        Box::new( futures::future::lazy( move || {
            let (tx_once, rx_once) = oneshot::channel();
            tx.send( ( msg, tx_once ) )
                .map_err( |e| serco::ServiceError::from(e) )
                .then( |_| rx_once )
        } )
        .then( |result| {
            match result {
                Ok( envelope_str ) => {
                    let envelope : ResponseEnvelope = serde_json::from_str( &envelope_str ).unwrap();
                    envelope.result.map( |v| D::deserialize( v ).unwrap() )
                },
                Err(e) => Err( serco::ServiceError::from( e ) )
            }
        } ) )
    }

    fn close( mut self ) {
        self.tx.close().expect( "Failed to close tx" );
    }
}
