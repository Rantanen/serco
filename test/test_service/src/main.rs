#![feature(proc_macro)]

use std::thread;

extern crate futures;
use futures::prelude::*;
extern crate tokio_core;
use tokio_core::reactor::Core;

extern crate serco;
use serco::{ServiceHost};

extern crate serco_derive;
use serco_derive::*;

extern crate serco_mpsc;
use serco_mpsc::*;

#[macro_use] extern crate serde_derive;

/// The service contract trait.
#[service_contract]
pub trait MyService {
    fn name( &self ) -> String;
    fn foo( &self, a : i32 ) -> bool;
}

#[service(MyService)]
struct MyServiceImplementation( &'static str );
impl MyService for MyServiceImplementation {

    fn name( &self ) -> String {
        self.0.to_string()
    }

    fn foo( &self, a : i32 ) -> bool {
        ( a % 2 ) == 0
    }
}

fn main() {

    thread::spawn( move || {

        let host = MpscServiceHost::new( "foo" )
                .host( MyService::singleton( MyServiceImplementation( "u" ), "unique" ) )
                .host( MyService::singleton( MyServiceImplementation( "single" ), "singleton" ) )
                .run();

        let mut core = Core::new().expect( "Failed to create core" );
        core.run( host ).unwrap();
    } );

    // Allow the service to start.
    thread::sleep( std::time::Duration::from_millis( 10 ) );

    // Connect to the service.
    MpscClient::new( "foo" ).connect::<MyService>( "singleton" ).map( |conn| {

        // Call the service.
        println!( "{}", conn.name() );
        println!( "{}", conn.foo( 3 ) );
    } ).wait().unwrap();

    // Connect to the service.
    MpscClient::new( "foo" ).connect::<MyService>( "unique" ).map( |conn| {

        // Call the service.
        println!( "{}", conn.name() );
        println!( "{}", conn.foo( 4 ) );
    } ).wait().unwrap();
}
