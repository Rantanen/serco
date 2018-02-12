
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
    let ( server_tx, client_rx ) = std::sync::mpsc::channel();
    let ( client_tx, server_rx ) = std::sync::mpsc::channel();

    // Host the service.
    let endpoint = MpscServiceEndpoint::<MyService>::open(
            server_rx, server_tx, MyServiceImplementation );

    // Connect to the service.
    let client = MpscServiceConnection::<MyService>::connect(
            client_rx, client_tx );

    // Call the service.
    println!( "{}", client.foo( 2 ) );
    println!( "{}", client.foo( 3 ) );
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
        name : &str,
        params : D,
        output : S
    ) -> Result<(), ()>
        where
            D: Deserializer<'de>,
            S: Serializer,
    {
        match name {
            "foo" => {
                #[derive(Deserialize)]
                struct Params {
                    a: i32
                }

                let params = Params::deserialize(params).unwrap();
                let result = self.foo( params.a );

                result.serialize( output )
                        .map( |_| () )
                        .map_err( |_| () )
            }
            _ => Err(())
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
        result.unwrap()
    }
}

// MyServiceImplementation: #[derive(Service<MyService>)]
use serco::ServiceContract;
impl serco::Service<MyService> for MyServiceImplementation {

    fn invoke<'de, D, S>(
        &self,
        name : &str,
        params : D,
        output : S
    ) -> Result<(), ()>
        where
            D: Deserializer<'de>,
            S: Serializer,
    {
        ( self as &MyService ).invoke( name, params, output )
    }
}

