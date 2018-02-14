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

    let mut output = vec![];

    if !model.has_session {
        for service in &model.services {
            output.push( quote!(
                impl serco::SingletonService< #service > for #struct_ident {
                    fn service( self ) -> Box< #service > {
                        Box::new( self )
                    }
                }
            ) );
        }
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

    let output = quote!( mod service_impl {
        extern crate serco;
        extern crate futures;
        use self::futures::prelude::*;
        extern crate serde;
        use self::serde::{Deserializer, Serializer};

        #( #output )*
    } );

    let output_stream : TokenStream = output.into();
    TokenStream::from_iter(
        input.into_iter().chain( output_stream.into_iter() ) )
}

#[proc_macro_attribute]
pub fn service_contract(
    _attr: TokenStream,
    input: TokenStream
) -> TokenStream
{
    let model = serco_common::ServiceContractModel
                    ::try_from(input.clone().into()).unwrap();

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

                    Box::new( Ok( output ).into_future() )
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
    let output = quote!( mod serco_impl {

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

        impl ServiceContract for #service_name {}

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

            pub fn singleton<T>(
                service: T,
                endpoint: &'static str,
            ) -> serco::hosted::Singleton<#service_name, T>
                where T: SingletonService<#service_name> + Send + 'static
            {
                serco::hosted::Singleton::new( service, endpoint )
            }

            pub fn host<T>(
                endpoint: &'static str,
            ) -> serco::hosted::Session<#service_name, T>
                where T: SessionService<#service_name> + Send + 'static
            {
                serco::hosted::Session::new( endpoint )
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
