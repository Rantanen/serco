
extern crate serco;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate erased_serde;

use serde::*;
use serde::de::DeserializeOwned;

use std::thread;
use std::sync::mpsc::{Sender, Receiver, channel};

/// Envelope used by the MPSC endpoints to communicate the calls.
#[derive(Serialize, Deserialize)]
struct Envelope<'a> {
    name : &'a str,
    params: serde_json::Value,
}

/// Service endpoint that hosts the services.
pub struct MpscServiceEndpoint<T: ?Sized> {
    thread: thread::JoinHandle<()>,
    phantom_data: std::marker::PhantomData<T>
}

impl<S: serco::ServiceContract + ?Sized + 'static> MpscServiceEndpoint<S> {

    /// Opens a new endpoint.
    pub fn open<TService>(
        rx: Receiver<String>,
        tx: Sender<String>,
        service: TService
    ) -> MpscServiceEndpoint<S>
        where TService: serco::Service<S> + Send + 'static
    {
        let thread = thread::spawn(move || {

            loop {
                let msg = rx.recv().unwrap();
                let envelope : Envelope = serde_json::from_str(&msg).unwrap();

                let mut output = serde_json::Serializer::new( vec![] );
                service.invoke(
                        envelope.name,
                        envelope.params,
                        &mut output ).unwrap();

                tx.send( unsafe { 
                    String::from_utf8_unchecked( output.into_inner() )
                } );
            }

        });

        MpscServiceEndpoint { thread, phantom_data: std::marker::PhantomData }
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
    pub fn connect( rx: Receiver<String>, tx: Sender<String> ) -> MpscServiceConnection<T>
    {
        let forwarder = MpscServiceForwarder { rx, tx };
        
        MpscServiceConnection {
            proxy: serco::ServiceProxy::new( forwarder ),
            phantom_data: std::marker::PhantomData,
        }
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
    rx: Receiver<String>,
    tx: Sender<String>,
}

impl serco::Forwarder for MpscServiceForwarder
{
    fn forward<'de, D, S>(
        &self,
        name: &str,
        params: S
    ) -> Result<D, ()>
        where
            D: DeserializeOwned + 'static,
            S: Serialize
    {
        let value = serde_json::to_value( params ).unwrap();
        let envelope = Envelope {
            name: name,
            params: value,
        };

        self.tx.send( serde_json::to_string( &envelope ).unwrap() );
        let response = self.rx.recv().unwrap();
        {
            serde_json::from_str( &response ).map_err( |_| () )
        }
    }
}

