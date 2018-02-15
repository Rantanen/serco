#![feature(proc_macro)]

extern crate serco;
use serco::prelude::*;

extern crate serco_mpsc;
use serco_mpsc::*;

#[macro_use] extern crate serde_derive;

#[macro_use] extern crate futures;
use futures::prelude::*;

extern crate tokio;
use tokio::executor::current_thread;

// Service contracts

#[service_contract( callback = CallbackContract )]
pub trait CallbackService {
    fn serve( &self ) -> String;
}

#[service_contract]
pub trait CallbackContract {
    fn callback( &self ) -> String;
}

// Service implementations

/// Callback service implementation hosted by the server.
#[service(CallbackService)]
struct MyCallbackService;
impl CallbackService for MyCallbackService {
    fn serve( &self ) -> String {

        let cb = CallbackService::get_callback();
        let s = cb.callback();
        format!( "< {} >", s )
    }
}

/// Callback implementation provided by the client.
#[service(CallbackContract)]
struct MyCallback;
impl CallbackContract for MyCallback{
    fn callback( &self ) -> String {
        String::from( "Callback" )
    }
}

/// Runs the service.
fn run_service() -> Box< Future<Item=(), Error=()> >
{
    // Define a service host for hosting the CallbackServive using a singleton
    // instance of 'MyCallbackService' struct.
    let future = serco::ServiceHost::new( CallbackService::singleton( MyCallbackService ) )
        .endpoint( MpscEndpoint::new( "callback" ) )
        .run()
        .map( |_| println!( "Service shut down." ) )
        .map_err( |e| println!( "Service aborted: {:?}", e ) );

    Box::new( future )
}

/// Executes the client.
fn run_client() -> Box< Future<Item=(), Error=()> >
{

    // Connect to the service.
    let callback : MyCallback = MyCallback;

    let future = MpscClient::new( "callback" ).connect_duplex::<CallbackService, _, _>( callback )
        .map( |conn| {

            println!( "Connection established." );
            println!( "Calling 'serve()' ..." );

            let result = conn.serve();

            println!( "Received: {}", result );

        } )
        .map_err( |e| println!( "Client encountered error: {:?}", e ) );

    Box::new( future )
}

fn main() {

    // Run the service in a thread.
    std::thread::spawn( move || {
        current_thread::run( |_| {
            current_thread::spawn( run_service() )
        } )
    } );

    // Run the client.
    std::thread::sleep( std::time::Duration::from_millis( 10 ) );
    current_thread::run( |_| {
        current_thread::spawn( run_client() )
    } )
}
