//! Derives for domain newtypes.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, GenericParam};

/// Generates a fallible `new(value)` constructor for a single-field tuple struct.
///
/// Construction delegates to the inner type's `TryFrom` implementation and returns
/// its error unchanged. The constructor accepts each input type supported by the inner type.
#[proc_macro_derive(TryNew)]
pub fn derive_try_new(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_try_new(&input) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_try_new(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let field = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(fields) => {
                let mut fields = fields.unnamed.iter();
                match (fields.next(), fields.next()) {
                    (Some(field), None) => Some(field),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| syn::Error::new_spanned(input, "TryNew requires a single-field tuple struct"))?;

    let name = &input.ident;
    let inner = &field.ty;
    let mut parameter = format_ident!("__CrumbsTryNewInput");
    while input.generics.params.iter().any(|generic| match generic {
        GenericParam::Type(generic) => generic.ident == parameter,
        GenericParam::Const(generic) => generic.ident == parameter,
        GenericParam::Lifetime(_) => false,
    }) {
        parameter = format_ident!("{}Value", parameter);
    }
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #name #type_generics #where_clause {
            /// Constructs the wrapper through the inner type's `TryFrom` implementation.
            ///
            /// # Errors
            ///
            /// Returns the inner conversion error if the input is invalid.
            pub fn new<#parameter>(value: #parameter)
                -> ::core::result::Result<Self, <#inner as ::core::convert::TryFrom<#parameter>>::Error>
            where
                #inner: ::core::convert::TryFrom<#parameter>,
            {
                <#inner as ::core::convert::TryFrom<#parameter>>::try_from(value).map(Self)
            }
        }
    })
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
