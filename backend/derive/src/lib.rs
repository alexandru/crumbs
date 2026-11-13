//! Derives for domain newtypes.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, GenericParam};

/// Generates a fallible `new(value)` constructor for a single-field tuple struct.
///
/// Construction delegates to the inner type's `TryFrom` implementation and returns
/// its error unchanged. The constructor accepts each input type supported by the inner type.
#[proc_macro_derive(AliasTryNew)]
pub fn derive_alias_try_new(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_alias_try_new(&input) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Generates a borrowed `value(&self)` accessor for a single-field tuple struct.
#[proc_macro_derive(AliasValue)]
pub fn derive_alias_value(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_alias_value(&input) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Forwards formatting to the inner value of a single-field tuple struct.
#[proc_macro_derive(Display)]
pub fn derive_display(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_display(&input) {
        Ok(output) => output.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_alias_try_new(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let field = newtype_field(input, "AliasTryNew")?;

    let name = &input.ident;
    let inner = &field.ty;
    let mut parameter = format_ident!("__CrumbsAliasTryNewInput");
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

fn expand_alias_value(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let field = newtype_field(input, "AliasValue")?;
    let name = &input.ident;
    let inner = &field.ty;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #name #type_generics #where_clause {
            /// Borrows the wrapped value.
            pub const fn value(&self) -> &#inner {
                &self.0
            }
        }
    })
}

fn expand_display(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let field = newtype_field(input, "Display")?;
    let name = &input.ident;
    let inner = &field.ty;
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote!(#inner: ::core::fmt::Display));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::core::fmt::Display for #name #type_generics #where_clause {
            fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, formatter)
            }
        }
    })
}

fn newtype_field<'a>(input: &'a DeriveInput, derive: &str) -> syn::Result<&'a syn::Field> {
    match &input.data {
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
    .ok_or_else(|| {
        syn::Error::new_spanned(
            input,
            format!("{derive} requires a single-field tuple struct"),
        )
    })
}

#[cfg(test)]
#[path = "../unit-tests/lib.rs"]
mod tests;
