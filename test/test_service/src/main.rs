#![feature(proc_macro)]

use std::thread;

#[macro_use] extern crate futures;
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

#[service_contract( callback = CallbackContract )]
pub trait CallbackService {
    fn serve( &self ) -> String;
}

#[service_contract]
pub trait CallbackContract {
    fn callback( &self ) -> String;
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

#[service(MyService)]
struct SessionImplementation {
    session: std::rc::Rc<ComplexSession>
}
impl MyService for SessionImplementation {

    fn name( &self ) -> String {
        self.session.value.get().to_string()
    }

    fn foo( &self, a : i32 ) -> bool {
        self.session.value.set( a );
        true
    }
}

use serco::ServiceContract;

#[service(CallbackService)]
struct MyCallbackService;
impl CallbackService for MyCallbackService {
    fn serve( &self ) -> String {

        let cb = CallbackService::get_callback();
        let s = cb.callback();
        format!( "< {} >", s )
    }
}

#[service(CallbackContract)]
struct MyCallback;
impl CallbackContract for MyCallback{
    fn callback( &self ) -> String {
        String::from( "Callback" )
    }
}

impl serco::SessionService<MyService> for SessionImplementation {

    type SessionInfo = ComplexSession;
    fn construct( session: std::rc::Rc<ComplexSession> ) -> Box<MyService> {
        Box::new( SessionImplementation { session: session } )
    }
}

struct MySessionFactory;
impl serco::SessionFactory for MySessionFactory {
    type SessionInfo = usize;
    fn create_session( &self ) -> ( String, std::rc::Rc<Self::SessionInfo> ) {
        ( format!( "{}", 0 ), std::rc::Rc::new( 0 ) )
    }
    fn get_session( &self, key: &str ) -> std::rc::Rc<Self::SessionInfo> {
        std::rc::Rc::new( 0 )
    }
}

struct ComplexSession {
    key: String,
    value: std::cell::Cell<i32>,
}
impl serco::SessionInfo for ComplexSession {
    fn key(&self) -> std::borrow::Cow<str> { std::borrow::Cow::from( self.key.as_ref() ) }
}

struct ComplexSessionFactory;
impl serco::SessionFactory for ComplexSessionFactory {
    type SessionInfo = ComplexSession;
    fn create_session( &self ) -> ( String, std::rc::Rc<Self::SessionInfo> ) {
        (
            format!( "{}", 0 ),
            std::rc::Rc::new( ComplexSession {
                key: format!( "{}", 0 ),
                value: 0.into(),
            } )
        )
    }
    fn get_session( &self, key: &str ) -> std::rc::Rc<Self::SessionInfo> {
        std::rc::Rc::new( ComplexSession { key: key.to_string(), value: 0.into() } )
    }
}

fn main() {

    thread::spawn( move || {

        let host = serco::ServiceHost2::new( MyService::session::<SessionImplementation>( "unique" ) )
                            .session_factory( ComplexSessionFactory )
                            .endpoint( MpscEndpoint::new( "test" ) )
                            .run();

        // let host = MpscServiceHost::new( "foo" )
        //         .host( MyService::singleton( MyServiceImplementation( "u" ), "unique" ) )
        //         .host( MyService::singleton( MyServiceImplementation( "single" ), "singleton" ) )
        //         .run();

        let mut core = Core::new().expect( "Failed to create core" );
        core.run( host ).unwrap();
    } );

    thread::spawn( move || {

        let host = serco::ServiceHost2::new( CallbackService::singleton( MyCallbackService, "unique" ) )
                            .endpoint( MpscEndpoint::new( "callback" ) )
                            .run();

        let mut core = Core::new().expect( "Failed to create core" );
        core.run( host ).unwrap();
    } );

    // Allow the service to start.
    thread::sleep( std::time::Duration::from_millis( 10 ) );
    let mut core = Core::new().expect( "Failed to create core" );

    // Connect to the service.
    let handle = core.handle();
    core.run( MpscClient::new( "test", handle ).connect::<MyService>().map( |conn| {

        // Call the service.
        println!( "{}", conn.name() );
        println!( "{}", conn.foo( 3 ) );

    } ) ).unwrap();

    // Connect to the service.
    let handle = core.handle();
    core.run( MpscClient::new( "test", handle ).connect::<MyService>().map( |conn| {

        // Call the service.
        println!( "{}", conn.name() );
        println!( "{}", conn.foo( 4 ) );
        println!( "{}", conn.name() );

    } ) ).unwrap();

    // Connect to the service.
    let callback : MyCallback = MyCallback;
    let handle = core.handle();
    core.run( MpscClient::new( "callback", handle ).connect_duplex::<CallbackService, _, _>( callback ).map( |conn| {

        // Call the service.
        println!( "{}", conn.serve() );

    } ) ).unwrap();
}
