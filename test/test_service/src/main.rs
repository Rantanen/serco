
use std::thread;

extern crate futures;
use futures::prelude::*;
use futures::sync::mpsc::channel;

extern crate serco;
extern crate serde;
extern crate serco_mpsc;
#[macro_use] extern crate serde_derive;
use serde::{Deserialize, Serialize, Deserializer, Serializer};

use serco_mpsc::*;

/// The service contract trait.
// #[derive(ServiceContract)]
pub trait MyService {
    fn foo( &self, a : i32 ) -> bool;
}

// #[derive(Service<MyService>)]
struct MyServiceImplementation;
impl MyService for MyServiceImplementation {

    fn foo( &self, a : i32 ) -> bool {
        ( a % 2 ) == 0
    }
}

fn main() {

    // THe MPSC service uses external channels.
    // Normal services will deal with the communcation over network stacks etc.
    let (tx, rx) = channel(1);

    // Connect to the service.
    let client = MpscServiceConnection::<MyService>::connect(tx);

    let t = thread::spawn( move || {
        // Host the service.
        MpscServiceEndpoint::<MyService>::run(
                rx, MyServiceImplementation );
    } );

    // Call the service.
    println!( "{}", client.foo( 2 ) );
    println!( "{}", client.foo( 3 ) );

    // Close the service.
    client.close();
    t.join().expect( "Failed to join thread" );
}

//////////////////////////////////////////////////////////////////////////////
//
// #[derive(...)] implementations below:

// MyService: #[derive(ServiceContract)]:

/// Service contracts #[derive] ServiceContract trait implementation.
///
/// This implementation essentially does the parameter/return value
/// serialization.
impl serco::ServiceContract for MyService {

    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        mut output : S
    ) -> Box<Future<Item=S, Error=serco::ServiceError>>
        where
            D: Deserializer<'de>,
            S: 'static,
            for <'a> &'a mut S: Serializer
    {
        match name {
            "foo" => {
                #[derive(Deserialize)]
                struct Params {
                    a: i32
                }

                let params = Params::deserialize(params).unwrap();
                let rval = self.foo( params.a );

                if let Err(e) = rval.serialize( &mut output ) {
                    return Box::new(
                        Err( serco::ServiceError::from(e) ).into_future()
                    );
                }

                Box::new( Ok( output ).into_future() )
                /*
                Box::new( match result {
                    Ok(_) => Ok( output ),
                    Err(_) => Err( serco::ServiceError::UnknownError ),
                }.into_future() )
                */
            }
            _ => Box::new( futures::future::err( serco::ServiceError::from( "Bad fn" ) ) )
        }
    }
}

/// Implement the service trait for the proxy. The connection will yield
/// a ServiceProxy to us, so the derive needs to impl the service trait
/// on the service proxy.
impl<F: serco::Forwarder> MyService for serco::ServiceProxy<MyService, F> {

    fn foo( &self, a : i32 ) -> bool {

        #[derive(Serialize)]
        struct Params {
            a: i32
        }

        let params = Params { a };
        let result = self.forwarder.forward( "foo", params );
        result.wait().unwrap()
    }
}

// MyServiceImplementation: #[derive(Service<MyService>)]
use serco::ServiceContract;
impl serco::Service<MyService> for MyServiceImplementation {

    fn invoke<'de, D, S>(
        &self,
        name: &str,
        params : D,
        output : S
    ) -> Box<Future<Item=S, Error=serco::ServiceError>>
        where
            D: Deserializer<'de>,
            S: 'static,
            for <'a> &'a mut S: Serializer
    {
        ( self as &MyService ).invoke( name, params, output )
    }
}

