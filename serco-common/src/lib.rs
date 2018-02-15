
extern crate proc_macro2;
extern crate syn;
#[macro_use] extern crate quote;

use std::collections::HashSet;

// Use proc_macro2 token stream, because proc_macro token stream won't work
// during unit tests.
use proc_macro2::TokenStream;
use syn::*;

#[derive(Debug, PartialEq)]
pub enum ServiceContractError {
    BadItem,
    BadArgument,
}

#[derive(Debug, PartialEq)]
pub struct ServiceContractModel {
    pub name: Ident,
    pub callback_interface: Type,
    pub mod_ident: Ident,
    pub operations : Vec<Operation>
}

#[derive(Debug, PartialEq)]
pub struct Operation {
    pub name : Ident,
    pub args : Vec<OperationArgument>,
    pub output : Type,
}

#[derive(Debug, PartialEq)]
pub struct OperationArgument {
    pub name : Ident,
    pub ty : Type,
}

#[derive(Debug, PartialEq)]
pub enum ServiceError {
    BadItem,
    BadAttribute,
}

#[derive(Debug, PartialEq)]
pub struct ServiceModel {
    pub name: Ident,
    pub mod_ident: Ident,
    pub services: HashSet<Ident>,
    pub has_session: bool,
}

impl ServiceContractModel {

    pub fn try_from(
        attribute : TokenStream,
        tokens : TokenStream
    ) -> Result<ServiceContractModel, ServiceContractError>
    {
        let args : ServiceContractAttributeArgs = syn::parse2( attribute )
                .map_err( |_| ServiceContractError::BadItem )?;
        let input : ItemTrait = syn::parse2( tokens )
                .map_err( |_| ServiceContractError::BadItem )?;
        let mod_ident = Ident::from( format!( "{}_impl_mod", input.ident ) );

        Ok( ServiceContractModel {
            name: input.ident,
            mod_ident: mod_ident,
            callback_interface: args.callback_interface,
            operations: input.items.into_iter().filter_map( |i|
                    match i {
                        TraitItem::Method( tim ) => Some( tim ),
                        _ => None
                    } ).map( Operation::try_from )
                    .collect::<Result<Vec<_>, _>>()?,
        } )
    }
}

impl Operation {

    pub fn try_from(
        method : TraitItemMethod
    ) -> Result<Operation, ServiceContractError>
    {
        let mut arg_iter = method.sig.decl.inputs.into_iter();
        let _self_arg = arg_iter.next();
        Ok( Operation {
            name: method.sig.ident,
            args: arg_iter
                    .map( |i| OperationArgument::try_from( i ) )
                    .collect::<Result<Vec<_>, _>>()?,
            output: method.sig.decl.output.to_type()
        } )
    }
}

impl OperationArgument {

    pub fn try_from(
        arg : FnArg
    ) -> Result<OperationArgument, ServiceContractError>
    {
        let arg = match arg {
            FnArg::Captured(arg) => arg,
            _ => return Err( ServiceContractError::BadArgument ),
        };
        let ident = match arg.pat {
            Pat::Ident( pi ) => pi.ident,
            _ => return Err( ServiceContractError::BadArgument ),
        };
        Ok( OperationArgument {
            name: ident,
            ty: arg.ty,
        } )
    }
}

impl ServiceModel {

    pub fn try_from(
        attribute : TokenStream,
        tokens : TokenStream,
    ) -> Result<ServiceModel, ServiceError>
    {
        let input : ItemStruct = syn::parse2( tokens )
                .map_err( |_| ServiceError::BadItem )?;
        let args : ServiceAttributeArgs = syn::parse2( attribute )
                .map_err( |_| ServiceError::BadAttribute )?;
        let mod_ident = Ident::from( format!( "{}_impl_mod", input.ident ) );

        let has_session = input.fields.iter()
                .find( |&f|
                    if let syn::Type::Path( ref path_ty ) = f.ty {
                        path_ty.path.segments.last()
                            .expect( "Paths are not empty" )
                            .value()
                            .ident == "SessionInfo"
                    } else {
                        false
                    } )
                .is_some();

        Ok( ServiceModel {
            name: input.ident,
            mod_ident: mod_ident,
            services: args.services,
            has_session,
        } )
    }
}

struct ServiceContractAttributeArgs {
    callback_interface: Type,
}

impl synom::Synom for ServiceContractAttributeArgs {
    named!(parse -> Self, 
        map!(
            option!( map!(
                parens!( punctuated::Punctuated
                             ::<ServiceContractAttributeArg, Token![,]>
                             ::parse_terminated_nonempty ),
                |(_, args)| ServiceContractAttributeArgs::from( args )
            ) ),
            |opt| opt.unwrap_or_else( || Default::default() )
        )
    );
}

enum ServiceContractAttributeArg {
    Callback( Type )
}

impl Default for ServiceContractAttributeArgs {
    fn default() -> Self {
        ServiceContractAttributeArgs {
            callback_interface: parse_quote!( () )
        }
    }
}

impl<I> From<I> for ServiceContractAttributeArgs
    where I: IntoIterator<Item=ServiceContractAttributeArg>
{
    fn from( src: I ) -> Self {
        let mut result : Self = Default::default();
        
        use ServiceContractAttributeArg::*;
        for arg in src {
            match arg {
                Callback( t ) => result.callback_interface = t,
            }
        }

        result
    }
}

impl synom::Synom for ServiceContractAttributeArg {
    named!(parse -> Self, do_parse!(
            name: syn!(Ident) >>
            punct!(=) >>
            arg: switch!( value!( name.as_ref() ),
                "callback" => map!(
                    syn!(Type),
                    ServiceContractAttributeArg::Callback
                )
                |
                _ => reject!()
            ) >>
            ( arg )
    ) );
}

struct ServiceAttributeArgs {
    services: HashSet<Ident>,
}

impl synom::Synom for ServiceAttributeArgs {
    named!(parse -> Self, map!(
        parens!(punctuated::Punctuated::<Ident, Token![,]>
                    ::parse_terminated_nonempty),
        |(_parens, vars)| ServiceAttributeArgs {
            services: vars.into_iter().collect(),
        }
    ));
}

trait GetType {
    fn to_type( self ) -> Type;
}

impl GetType for ReturnType {
    fn to_type( self ) -> Type {
        match self {
            ReturnType::Default => parse_quote!( () ),
            ReturnType::Type( _, t ) => *t,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::iter::FromIterator;

    #[test]
    pub fn singleton_service() {
        let model = ServiceModel::try_from(
            quote!( ( SomeService ) ).into(),
            quote!( struct Singleton; ).into(),
        ).unwrap();

        assert_eq!( model, ServiceModel {
            name: Ident::from( "Singleton" ),
            mod_ident: Ident::from( "Singleton_impl_mod" ),
            has_session: false,
            services: HashSet::from_iter( vec![
                Ident::from( "SomeService" )
            ].into_iter() ),
        } );
    }

    #[test]
    pub fn non_singleton_service() {
        let model = ServiceModel::try_from(
            quote!( ( SessionService ) ).into(),
            quote!( struct Session{ sess: SessionInfo } ).into(),
        ).unwrap();

        assert_eq!( model, ServiceModel {
            name: Ident::from( "Session" ),
            mod_ident: Ident::from( "Session_impl_mod" ),
            has_session: true,
            services: HashSet::from_iter( vec![
                Ident::from( "SessionService" )
            ].into_iter() ),
        } );
    }

    #[test]
    pub fn multiple_services() {
        let model = ServiceModel::try_from(
            quote!( ( A, B, C, D ) ).into(),
            quote!( struct Services; ).into(),
        ).unwrap();

        assert_eq!( model, ServiceModel {
            name: Ident::from( "Services" ),
            mod_ident: Ident::from( "Services_impl_mod" ),
            has_session: false,
            services: HashSet::from_iter( vec![
                Ident::from( "A" ),
                Ident::from( "B" ),
                Ident::from( "C" ),
                Ident::from( "D" ),
            ].into_iter() ),
        } );
    }

    #[test]
    pub fn service_contract() {
        let model = ServiceContractModel::try_from(
            quote!().into(),
            quote!( trait SomeContract {
                fn op_1( &self, a: u32, b: bool ) -> String;
                fn op_2( &self, something: String );
            } ).into()
        ).unwrap();

        assert_eq!( model, ServiceContractModel {
            name: Ident::from( "SomeContract" ),
            mod_ident: Ident::from( "SomeContract_impl_mod" ),
            callback_interface: parse_quote!( () ),
            operations: vec![
                Operation {
                    name: Ident::from( "op_1" ),
                    output: parse_quote!( String ),
                    args: vec![
                        OperationArgument {
                            name: Ident::from( "a" ),
                            ty: parse_quote!( u32 ),
                        },
                        OperationArgument {
                            name: Ident::from( "b" ),
                            ty: parse_quote!( bool ),
                        },
                    ],
                },
                Operation {
                    name: Ident::from( "op_2" ),
                    output: parse_quote!( () ),
                    args: vec![
                        OperationArgument {
                            name: Ident::from( "something" ),
                            ty: parse_quote!( String ),
                        },
                    ],
                },
            ],
        } );
    }

    #[test]
    pub fn callback_interface() {
        let model = ServiceContractModel::try_from(
            quote!(
                ( callback = CallbackItf )
            ).into(),
            quote!( trait SomeContract {} ).into()
        ).unwrap();

        assert_eq!( model, ServiceContractModel {
            name: Ident::from( "SomeContract" ),
            mod_ident: Ident::from( "SomeContract_impl_mod" ),
            callback_interface: parse_quote!( CallbackItf ),
            operations: vec![]
        } );
    }
}
