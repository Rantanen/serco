#![feature(proc_macro)]
#![recursion_limit="256"]

extern crate serco_common;
extern crate proc_macro;

#[macro_use] extern crate quote;

use std::iter::FromIterator;
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn service(
    attr: TokenStream,
    input: TokenStream,
) -> TokenStream
{
    let model = serco_common::ServiceModel
                    ::try_from( attr.into(), input.clone().into() ).unwrap();
    let struct_ident = model.name;
    let mod_ident = model.mod_ident;

    let mut output = vec![];

    for service in &model.services {
        output.push( quote!(
            impl serco::SingletonService< #service > for #struct_ident {
                fn service( self ) -> Box< #service > {
                    Box::new( self )
                }
            }
        ) );
    }

    for service in &model.services {
        output.push( quote!(
            impl serco::InvokeTarget< #service > for #struct_ident {
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
                    ( self as &#service ).invoke( name, params, output )
                }
            }
        ) );
    }

    let output = quote!( #[allow(non_snake_case)] mod #mod_ident {
        extern crate serco;

        extern crate futures;
        #[allow(unused_imports)]
        use self::futures::prelude::*;

        extern crate serde;
        #[allow(unused_imports)]
        use self::serde::{Deserializer, Serializer};

        #( #output )*
    } );

    let output_stream : TokenStream = output.into();
    TokenStream::from_iter(
        input.into_iter().chain( output_stream.into_iter() ) )
}

#[proc_macro_attribute]
pub fn service_contract(
    attr: TokenStream,
    input: TokenStream
) -> TokenStream
{
    let model = serco_common::ServiceContractModel
                    ::try_from( attr.into(), input.clone().into() ).unwrap();

    let ( op_arms, proxy_fns ) : ( Vec<_>, Vec<_> ) = model.operations
                                   .into_iter()
                                   .map( |o| {
            let name = o.name;
            let output = o.output;
            let name_str = name.to_string();

            // Generate argument specific tokens.
            let mut args = vec![];
            let mut arg_defs = vec![];
            let mut params = vec![];
            o.args.into_iter().for_each( |a| {
                        let name = a.name;
                        let ty = a.ty;

                        args.push( quote!( #name ) );
                        arg_defs.push( quote!( #name : #ty ) );
                        params.push( quote!( params.#name ) );
                    } );

            // Turn the sub tokens into references so the quote!()s don't take
            // their ownership.
            let arg_defs = &arg_defs;
            let params = &params;
            (
                quote!( #name_str => {
                    #[derive(Deserialize)]
                    struct Params {
                        #( #arg_defs ),*
                    }

                    #[allow(unused_variables)]
                    let params = Params::deserialize(params).unwrap();
                    let rval = self.#name( #( #params ),* );

                    if let Err(e) = rval.serialize( &mut output ) {
                        return Box::new(
                            Err( serco::ServiceError::from(e) ).into_future()
                        );
                    }

                    Ok( output ).into_future()
                } ),
                quote!( fn #name( &self, #( #arg_defs ),* ) -> #output {

                    #[derive(Serialize)]
                    struct Params {
                        #( #arg_defs ),*
                    }

                    let params = Params { #( #args ),* };
                    let result = self.forwarder.forward( #name_str, params );

                    result.wait().unwrap()
                } )
            )
        } )
        .unzip();

    let service_name = model.name;
    let mod_ident = model.mod_ident;
    let callback = model.callback_interface;
    let output = quote!( #[allow(non_snake_case)] mod #mod_ident {

        use super::*;

        extern crate futures;
        #[allow(unused_imports)]
        use self::futures::prelude::*;

        extern crate serde;
        #[allow(unused_imports)]
        use self::serde::{Serialize, Serializer, Deserialize, Deserializer};

        extern crate serco;
        #[allow(unused_imports)]
        use self::serco::{ServiceContract, ServiceProxy, Forwarder,
                SingletonService, SessionService};

        #[allow(unused_imports)] use std::cell::RefCell;
        #[allow(unused_imports)] use std::sync::Arc;
        task_local!{
            static CALLBACK: RefCell<Option<Arc<#service_name + Send + Sync>>> =
                    RefCell::new(None)
        }

        impl ServiceContract for #service_name
        {
            type CallbackContract = #callback;

            fn set_task_callback<F: Forwarder>(
                callback : Arc<ServiceProxy<Self, F>>
            ) {
                CALLBACK.with( |cell| cell.replace( Some( callback ) ) );
            }

            fn get_task_callback() -> Arc<Self>
            {
                CALLBACK.with( |cell| {
                    cell.borrow().as_ref().map( |o| o.clone() ).unwrap()
                } )
            }
        }

        impl serco::InvokeTarget<#service_name> for #service_name {

            fn invoke<'de, D, S>(
                &self,
                name: &str,
                params: D,
                mut output: S
            ) -> Box<Future<Item=S, Error=serco::ServiceError>>
                where
                    D: Deserializer<'de>,
                    S: 'static,
                    for <'a> &'a mut S: Serializer
            {
                match name {
                    #( #op_arms ),*
                    _ => Box::new( futures::future::err(
                                serco::ServiceError::from( "Bad fn" ) ) ),
                }
            }
        }

        impl #service_name {

            pub fn singleton<T, I>(
                service: T,
            ) -> serco::hosted::Singleton<#service_name, T, I>
                where T: SingletonService<#service_name> + 'static
            {
                serco::hosted::Singleton::new( service )
            }

            pub fn session<T>(
            ) -> serco::hosted::Session<#service_name, T>
                where T: SessionService<#service_name> + 'static
            {
                serco::hosted::Session::new()
            }

            pub fn get_callback() -> Arc<#callback> {
                <Self as ServiceContract>::CallbackContract::get_task_callback()
            }
        }

        impl<F: Forwarder> #service_name for ServiceProxy< #service_name, F > {
            #( #proxy_fns )*
        }
    } );

    let output_stream : TokenStream = output.into();
    TokenStream::from_iter(
        input.into_iter().chain( output_stream.into_iter() ) )
}
