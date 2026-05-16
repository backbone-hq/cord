use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DataEnum, DataStruct, DeriveInput, Field, Fields, GenericParam, Type,
    Variant,
};

#[proc_macro_derive(Cord, attributes(cord))]
pub fn derive_cord(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = match &input.data {
        Data::Struct(data) => derive_struct(&input, data),
        Data::Enum(data) => derive_enum(&input, data),
        Data::Union(_) => Err(syn::Error::new_spanned(
            &input,
            "Cord does not support unions",
        )),
    };
    match result {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum CordFieldAttr {
    Width(u32),
    Evolving(u32),
    VarInt,
}

fn parse_cord_field_attrs(field: &Field) -> syn::Result<Option<CordFieldAttr>> {
    for attr in &field.attrs {
        if !attr.path().is_ident("cord") {
            continue;
        }
        let mut result = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("width") {
                let lit: syn::LitInt = meta.value()?.parse()?;
                let val: u32 = lit.base10_parse()?;
                if !matches!(val, 8 | 16 | 32 | 64) {
                    return Err(meta.error("width must be 8, 16, 32, or 64"));
                }
                result = Some(CordFieldAttr::Width(val));
                Ok(())
            } else if meta.path.is_ident("evolving") {
                let lit: syn::LitInt = meta.value()?.parse()?;
                let val: u32 = lit.base10_parse()?;
                if !matches!(val, 8 | 16 | 32) {
                    return Err(meta.error("evolving must be 8, 16, or 32"));
                }
                result = Some(CordFieldAttr::Evolving(val));
                Ok(())
            } else if meta.path.is_ident("varint") {
                result = Some(CordFieldAttr::VarInt);
                Ok(())
            } else {
                Err(meta.error("unknown cord attribute"))
            }
        })?;
        return Ok(result);
    }
    Ok(None)
}

fn parse_cord_variant_index(variant: &Variant) -> syn::Result<Option<u32>> {
    for attr in &variant.attrs {
        if !attr.path().is_ident("cord") {
            continue;
        }
        let mut result = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("index") {
                let lit: syn::LitInt = meta.value()?.parse()?;
                result = Some(lit.base10_parse::<u32>()?);
                Ok(())
            } else {
                Err(meta.error("expected `index = N` on enum variant"))
            }
        })?;
        return Ok(result);
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Wrapper token generation
// ---------------------------------------------------------------------------

/// Returns the ser wrapper path for a field attribute, or None if no wrapping needed.
fn ser_wrapper(attr: &CordFieldAttr) -> Option<TokenStream2> {
    match attr {
        CordFieldAttr::Width(8) => Some(quote!(::cord::__private::Width8Ser)),
        CordFieldAttr::Width(16) => Some(quote!(::cord::__private::Width16Ser)),
        CordFieldAttr::Width(32) => None,
        CordFieldAttr::Width(64) => Some(quote!(::cord::__private::Width64Ser)),
        CordFieldAttr::Evolving(8) => Some(quote!(::cord::__private::Evolving8Ser)),
        CordFieldAttr::Evolving(16) => Some(quote!(::cord::__private::Evolving16Ser)),
        CordFieldAttr::Evolving(32) => None, // Evolving<T> defaults to 32-bit
        CordFieldAttr::VarInt => Some(quote!(::cord::__private::VarIntSer)),
        _ => unreachable!(),
    }
}

/// Extracts the inner type `T` from `Evolving<T>`.
fn extract_evolving_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let seg = type_path.path.segments.last()?;
        if seg.ident == "Evolving" {
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                    return Some(inner);
                }
            }
        }
    }
    None
}

/// Returns the de wrapper type for a field attribute, wrapping the given field type.
/// Returns None if no wrapping needed.
fn de_wrapper(attr: &CordFieldAttr, field_ty: &Type) -> Option<TokenStream2> {
    match attr {
        CordFieldAttr::Width(8) => Some(quote!(::cord::__private::Width8De<#field_ty>)),
        CordFieldAttr::Width(16) => Some(quote!(::cord::__private::Width16De<#field_ty>)),
        CordFieldAttr::Width(32) => None,
        CordFieldAttr::Width(64) => Some(quote!(::cord::__private::Width64De<#field_ty>)),
        CordFieldAttr::Evolving(8) => {
            let inner = extract_evolving_inner(field_ty)
                .expect("evolving attribute requires Evolving<T> field type");
            Some(quote!(::cord::__private::Evolving8De<#inner>))
        }
        CordFieldAttr::Evolving(16) => {
            let inner = extract_evolving_inner(field_ty)
                .expect("evolving attribute requires Evolving<T> field type");
            Some(quote!(::cord::__private::Evolving16De<#inner>))
        }
        CordFieldAttr::Evolving(32) => None,
        CordFieldAttr::VarInt => Some(quote!(::cord::__private::VarIntDe<#field_ty>)),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// CordSchema / CordEncode / CordDecode helpers
// ---------------------------------------------------------------------------

/// Convert a width attribute value (8, 16, 32, 64) to a `Width` enum variant token.
fn width_variant_token(w: u32) -> TokenStream2 {
    match w {
        8 => quote!(::cord::Width::W8),
        16 => quote!(::cord::Width::W16),
        32 => quote!(::cord::Width::W32),
        64 => quote!(::cord::Width::W64),
        _ => unreachable!(),
    }
}

/// Generate the schema expression for a field based on its attribute.
fn schema_expr_for_field(attr: &Option<CordFieldAttr>, field_ty: &Type) -> TokenStream2 {
    match attr {
        None => {
            quote! { <#field_ty as ::cord::CordSchema>::schema() }
        }
        Some(CordFieldAttr::VarInt) => {
            quote! { ::cord::Schema::VarInt(::std::boxed::Box::new(<#field_ty as ::cord::CordSchema>::schema())) }
        }
        Some(CordFieldAttr::Width(w)) => {
            let width_variant = width_variant_token(*w);
            quote! { ::cord::schema::with_width(<#field_ty as ::cord::CordSchema>::schema(), #width_variant) }
        }
        Some(CordFieldAttr::Evolving(w)) => {
            let width_variant = width_variant_token(*w);
            let inner_ty = extract_evolving_inner(field_ty)
                .expect("evolving attribute requires Evolving<T> field type");
            quote! { ::cord::Schema::Evolving(::std::boxed::Box::new(<#inner_ty as ::cord::CordSchema>::schema()), #width_variant) }
        }
    }
}

/// Generate the CordEncode expression for a field based on its attribute.
fn encode_expr_for_field(
    attr: &Option<CordFieldAttr>,
    field_access: &TokenStream2,
    _field_ty: &Type,
) -> TokenStream2 {
    match attr {
        None => {
            quote! { ::cord::CordEncode::encode_cord(&#field_access, __buf)?; }
        }
        Some(CordFieldAttr::VarInt) => {
            quote! { ::cord::__private::encode_varint(__buf, #field_access)?; }
        }
        Some(CordFieldAttr::Width(w)) => {
            let width = width_variant_token(*w);
            quote! { ::cord::__private::encode_with_width(__buf, &#field_access, #width)?; }
        }
        Some(CordFieldAttr::Evolving(w)) => {
            let width = width_variant_token(*w);
            quote! { ::cord::__private::encode_evolving(__buf, &#field_access, #width)?; }
        }
    }
}

/// Generate the CordDecode expression for a field based on its attribute.
fn decode_expr_for_field(
    attr: &Option<CordFieldAttr>,
    field_name: &proc_macro2::Ident,
    field_ty: &Type,
) -> TokenStream2 {
    match attr {
        None => {
            quote! { let #field_name: #field_ty = ::cord::CordDecode::decode_cord(__input)?; }
        }
        Some(CordFieldAttr::VarInt) => {
            quote! { let #field_name: #field_ty = ::cord::__private::decode_varint(__input)?; }
        }
        Some(CordFieldAttr::Width(w)) => {
            let width = width_variant_token(*w);
            quote! { let #field_name: #field_ty = ::cord::__private::decode_with_width(__input, #width)?; }
        }
        Some(CordFieldAttr::Evolving(w)) => {
            let width = width_variant_token(*w);
            let inner_ty = extract_evolving_inner(field_ty)
                .expect("evolving attribute requires Evolving<T> field type");
            quote! { let #field_name: #field_ty = ::cord::__private::decode_evolving::<#inner_ty>(__input, #width)?; }
        }
    }
}

// ---------------------------------------------------------------------------
// Generic bounds helpers
// ---------------------------------------------------------------------------

fn add_bounds(generics: &syn::Generics, bound: TokenStream2) -> TokenStream2 {
    let params: Vec<_> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            GenericParam::Type(t) => {
                let ident = &t.ident;
                Some(quote!(#ident: #bound))
            }
            _ => None,
        })
        .collect();

    if params.is_empty() {
        quote!()
    } else {
        quote!(where #(#params),*)
    }
}

fn generic_params(generics: &syn::Generics, with_de: bool) -> TokenStream2 {
    let params: Vec<_> = generics
        .params
        .iter()
        .map(|p| match p {
            GenericParam::Type(t) => {
                let ident = &t.ident;
                quote!(#ident)
            }
            GenericParam::Lifetime(l) => {
                let lt = &l.lifetime;
                quote!(#lt)
            }
            GenericParam::Const(c) => {
                let ident = &c.ident;
                quote!(#ident)
            }
        })
        .collect();

    match (params.is_empty(), with_de) {
        (true, false) => quote!(),
        (true, true) => quote!(<'de>),
        (false, false) => quote!(<#(#params),*>),
        (false, true) => quote!(<'de, #(#params),*>),
    }
}

// ---------------------------------------------------------------------------
// Struct derive
// ---------------------------------------------------------------------------

fn derive_struct(input: &DeriveInput, data: &DataStruct) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let gp = generic_params(generics, false);
    let gp_de = generic_params(generics, true);
    let ser_bounds = add_bounds(generics, quote!(::serde::Serialize));
    let de_bounds = add_bounds(generics, quote!(::serde::Deserialize<'de>));

    match &data.fields {
        Fields::Named(fields) => {
            derive_named_struct(name, generics, &gp, &gp_de, &ser_bounds, &de_bounds, fields)
        }
        Fields::Unnamed(fields) => {
            derive_tuple_struct(name, generics, &gp, &gp_de, &ser_bounds, &de_bounds, fields)
        }
        Fields::Unit => derive_unit_struct(name, generics, &gp, &gp_de, &ser_bounds, &de_bounds),
    }
}

fn derive_named_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    gp: &TokenStream2,
    gp_de: &TokenStream2,
    ser_bounds: &TokenStream2,
    de_bounds: &TokenStream2,
    fields: &syn::FieldsNamed,
) -> syn::Result<TokenStream2> {
    let field_count = fields.named.len();
    let name_str = name.to_string();

    // Parse attributes for each field
    let mut field_attrs = Vec::new();
    for field in &fields.named {
        field_attrs.push(parse_cord_field_attrs(field)?);
    }

    // --- Serialize ---
    let ser_fields: Vec<TokenStream2> = fields
        .named
        .iter()
        .zip(field_attrs.iter())
        .map(|(field, attr)| {
            let field_name = field.ident.as_ref().unwrap();
            let field_name_str = field_name.to_string();

            match attr {
                Some(a) => match ser_wrapper(a) {
                    Some(wrapper) => {
                        quote! {
                            __state.serialize_field(#field_name_str, &#wrapper::new(&self.#field_name))?;
                        }
                    }
                    None => {
                        quote! {
                            __state.serialize_field(#field_name_str, &self.#field_name)?;
                        }
                    }
                },
                None => {
                    quote! {
                        __state.serialize_field(#field_name_str, &self.#field_name)?;
                    }
                }
            }
        })
        .collect();

    // --- Deserialize ---
    let field_names: Vec<_> = fields
        .named
        .iter()
        .map(|f| f.ident.as_ref().unwrap())
        .collect();
    let field_name_strs: Vec<_> = field_names.iter().map(|n| n.to_string()).collect();
    let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();

    let de_fields: Vec<TokenStream2> = fields
        .named
        .iter()
        .zip(field_attrs.iter())
        .enumerate()
        .map(|(i, (field, attr))| {
            let field_name = field.ident.as_ref().unwrap();
            let field_ty = &field.ty;

            match attr {
                Some(a) => match de_wrapper(a, field_ty) {
                    Some(wrapper_ty) => {
                        quote! {
                            let #field_name = __seq
                                .next_element::<#wrapper_ty>()?
                                .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?
                                .0;
                        }
                    }
                    None => {
                        quote! {
                            let #field_name = __seq
                                .next_element::<#field_ty>()?
                                .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                        }
                    }
                },
                None => {
                    quote! {
                        let #field_name = __seq
                            .next_element::<#field_ty>()?
                            .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                    }
                }
            }
        })
        .collect();

    let _ = (&field_types, &field_name_strs); // suppress unused warnings

    let impl_generics = &generics.params;
    let impl_gen_ser = if impl_generics.is_empty() {
        quote!()
    } else {
        quote!(<#impl_generics>)
    };
    let impl_gen_de = if impl_generics.is_empty() {
        quote!(<'de>)
    } else {
        quote!(<'de, #impl_generics>)
    };

    let expecting_str = format!("struct {}", name);
    let field_strs: Vec<_> = field_names.iter().map(|n| n.to_string()).collect();

    // --- CordSchema ---
    let schema_fields: Vec<TokenStream2> = fields
        .named
        .iter()
        .zip(field_attrs.iter())
        .map(|(field, attr)| {
            let field_name = field.ident.as_ref().unwrap().to_string();
            let field_ty = &field.ty;
            let schema = schema_expr_for_field(attr, field_ty);
            quote! { (#field_name.to_string(), #schema) }
        })
        .collect();

    // --- CordEncode ---
    let encode_fields: Vec<TokenStream2> = fields
        .named
        .iter()
        .zip(field_attrs.iter())
        .map(|(field, attr)| {
            let field_name = field.ident.as_ref().unwrap();
            let field_ty = &field.ty;
            let access = quote! { self.#field_name };
            encode_expr_for_field(attr, &access, field_ty)
        })
        .collect();

    // --- CordDecode ---
    let decode_fields: Vec<TokenStream2> = fields
        .named
        .iter()
        .zip(field_attrs.iter())
        .map(|(field, attr)| {
            let field_name = field.ident.as_ref().unwrap();
            let field_ty = &field.ty;
            decode_expr_for_field(attr, field_name, field_ty)
        })
        .collect();

    let field_names_for_decode: Vec<_> = field_names.clone();

    Ok(quote! {
        impl #impl_gen_ser ::serde::Serialize for #name #gp #ser_bounds {
            fn serialize<__S: ::serde::Serializer>(&self, serializer: __S)
                -> ::core::result::Result<__S::Ok, __S::Error>
            {
                use ::serde::ser::SerializeStruct;
                let mut __state = serializer.serialize_struct(#name_str, #field_count)?;
                #(#ser_fields)*
                __state.end()
            }
        }

        impl #impl_gen_de ::serde::Deserialize<'de> for #name #gp #de_bounds {
            fn deserialize<__D: ::serde::Deserializer<'de>>(__deserializer: __D)
                -> ::core::result::Result<Self, __D::Error>
            {
                struct __Visitor #gp_de #de_bounds {
                    __marker: ::core::marker::PhantomData<&'de #name #gp>,
                }

                impl #impl_gen_de ::serde::de::Visitor<'de> for __Visitor #gp_de #de_bounds {
                    type Value = #name #gp;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        f.write_str(#expecting_str)
                    }

                    fn visit_seq<__A: ::serde::de::SeqAccess<'de>>(
                        self,
                        mut __seq: __A,
                    ) -> ::core::result::Result<#name #gp, __A::Error> {
                        #(#de_fields)*
                        Ok(#name { #(#field_names),* })
                    }
                }

                const FIELDS: &[&str] = &[#(#field_strs),*];
                __deserializer.deserialize_struct(
                    #name_str,
                    FIELDS,
                    __Visitor { __marker: ::core::marker::PhantomData },
                )
            }
        }

        impl #impl_gen_ser ::cord::CordSchema for #name #gp #ser_bounds {
            fn schema() -> ::cord::Schema {
                ::cord::Schema::Struct(::std::vec![#(#schema_fields),*])
            }
        }

        impl #impl_gen_ser ::cord::CordEncode for #name #gp #ser_bounds {
            fn encode_cord(&self, __buf: &mut ::std::vec::Vec<u8>) -> ::cord::CordResult<()> {
                #(#encode_fields)*
                Ok(())
            }
        }

        impl ::cord::CordDecode for #name #gp {
            fn decode_cord(__input: &mut &[u8]) -> ::cord::CordResult<Self> {
                #(#decode_fields)*
                Ok(#name { #(#field_names_for_decode),* })
            }
        }
    })
}

fn derive_tuple_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    gp: &TokenStream2,
    _gp_de: &TokenStream2,
    ser_bounds: &TokenStream2,
    de_bounds: &TokenStream2,
    fields: &syn::FieldsUnnamed,
) -> syn::Result<TokenStream2> {
    let field_count = fields.unnamed.len();
    let name_str = name.to_string();

    let mut field_attrs = Vec::new();
    for field in &fields.unnamed {
        field_attrs.push(parse_cord_field_attrs(field)?);
    }

    // --- Serialize ---
    let ser_fields: Vec<TokenStream2> = fields
        .unnamed
        .iter()
        .zip(field_attrs.iter())
        .enumerate()
        .map(|(i, (_, attr))| {
            let idx = syn::Index::from(i);
            match attr {
                Some(a) => match ser_wrapper(a) {
                    Some(wrapper) => quote! {
                        __state.serialize_element(&#wrapper::new(&self.#idx))?;
                    },
                    None => quote! {
                        __state.serialize_element(&self.#idx)?;
                    },
                },
                None => quote! {
                    __state.serialize_element(&self.#idx)?;
                },
            }
        })
        .collect();

    // --- Deserialize ---
    let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
    let field_idents: Vec<_> = (0..field_count)
        .map(|i| format_ident!("__v{}", i))
        .collect();

    let de_fields: Vec<TokenStream2> = fields
        .unnamed
        .iter()
        .zip(field_attrs.iter())
        .enumerate()
        .map(|(i, (field, attr))| {
            let ident = format_ident!("__v{}", i);
            let field_ty = &field.ty;

            match attr {
                Some(a) => match de_wrapper(a, field_ty) {
                    Some(wrapper_ty) => quote! {
                        let #ident = __seq
                            .next_element::<#wrapper_ty>()?
                            .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?
                            .0;
                    },
                    None => quote! {
                        let #ident = __seq
                            .next_element::<#field_ty>()?
                            .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                    },
                },
                None => quote! {
                    let #ident = __seq
                        .next_element::<#field_ty>()?
                        .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                },
            }
        })
        .collect();

    let _ = &field_types;

    let impl_generics = &generics.params;
    let impl_gen_ser = if impl_generics.is_empty() {
        quote!()
    } else {
        quote!(<#impl_generics>)
    };
    let impl_gen_de = if impl_generics.is_empty() {
        quote!(<'de>)
    } else {
        quote!(<'de, #impl_generics>)
    };

    let expecting_str = format!("tuple struct {}", name);

    // --- CordSchema ---
    let schema_fields_tuple: Vec<TokenStream2> = fields
        .unnamed
        .iter()
        .zip(field_attrs.iter())
        .map(|(field, attr)| {
            let field_ty = &field.ty;
            schema_expr_for_field(attr, field_ty)
        })
        .collect();

    // --- CordEncode ---
    let encode_fields_tuple: Vec<TokenStream2> = fields
        .unnamed
        .iter()
        .zip(field_attrs.iter())
        .enumerate()
        .map(|(i, (field, attr))| {
            let idx = syn::Index::from(i);
            let field_ty = &field.ty;
            let access = quote! { self.#idx };
            encode_expr_for_field(attr, &access, field_ty)
        })
        .collect();

    // --- CordDecode ---
    let decode_fields_tuple: Vec<TokenStream2> = fields
        .unnamed
        .iter()
        .zip(field_attrs.iter())
        .enumerate()
        .map(|(i, (field, attr))| {
            let ident = format_ident!("__v{}", i);
            let field_ty = &field.ty;
            decode_expr_for_field(attr, &ident, field_ty)
        })
        .collect();

    let field_idents_for_decode: Vec<_> = (0..field_count)
        .map(|i| format_ident!("__v{}", i))
        .collect();

    Ok(quote! {
        impl #impl_gen_ser ::serde::Serialize for #name #gp #ser_bounds {
            fn serialize<__S: ::serde::Serializer>(&self, serializer: __S)
                -> ::core::result::Result<__S::Ok, __S::Error>
            {
                use ::serde::ser::SerializeTuple;
                let mut __state = serializer.serialize_tuple(#field_count)?;
                #(#ser_fields)*
                __state.end()
            }
        }

        impl #impl_gen_de ::serde::Deserialize<'de> for #name #gp #de_bounds {
            fn deserialize<__D: ::serde::Deserializer<'de>>(__deserializer: __D)
                -> ::core::result::Result<Self, __D::Error>
            {
                struct __Visitor;

                impl<'de> ::serde::de::Visitor<'de> for __Visitor {
                    type Value = #name;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        f.write_str(#expecting_str)
                    }

                    fn visit_seq<__A: ::serde::de::SeqAccess<'de>>(
                        self,
                        mut __seq: __A,
                    ) -> ::core::result::Result<#name, __A::Error> {
                        #(#de_fields)*
                        Ok(#name(#(#field_idents),*))
                    }
                }

                __deserializer.deserialize_tuple_struct(
                    #name_str,
                    #field_count,
                    __Visitor,
                )
            }
        }

        impl #impl_gen_ser ::cord::CordSchema for #name #gp #ser_bounds {
            fn schema() -> ::cord::Schema {
                ::cord::Schema::Tuple(::std::vec![#(#schema_fields_tuple),*])
            }
        }

        impl #impl_gen_ser ::cord::CordEncode for #name #gp #ser_bounds {
            fn encode_cord(&self, __buf: &mut ::std::vec::Vec<u8>) -> ::cord::CordResult<()> {
                #(#encode_fields_tuple)*
                Ok(())
            }
        }

        impl ::cord::CordDecode for #name #gp {
            fn decode_cord(__input: &mut &[u8]) -> ::cord::CordResult<Self> {
                #(#decode_fields_tuple)*
                Ok(#name(#(#field_idents_for_decode),*))
            }
        }
    })
}

fn derive_unit_struct(
    name: &syn::Ident,
    generics: &syn::Generics,
    gp: &TokenStream2,
    _gp_de: &TokenStream2,
    ser_bounds: &TokenStream2,
    de_bounds: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let name_str = name.to_string();

    let impl_generics = &generics.params;
    let impl_gen_ser = if impl_generics.is_empty() {
        quote!()
    } else {
        quote!(<#impl_generics>)
    };
    let impl_gen_de = if impl_generics.is_empty() {
        quote!(<'de>)
    } else {
        quote!(<'de, #impl_generics>)
    };

    Ok(quote! {
        impl #impl_gen_ser ::serde::Serialize for #name #gp #ser_bounds {
            fn serialize<__S: ::serde::Serializer>(&self, serializer: __S)
                -> ::core::result::Result<__S::Ok, __S::Error>
            {
                serializer.serialize_unit_struct(#name_str)
            }
        }

        impl #impl_gen_de ::serde::Deserialize<'de> for #name #gp #de_bounds {
            fn deserialize<__D: ::serde::Deserializer<'de>>(__deserializer: __D)
                -> ::core::result::Result<Self, __D::Error>
            {
                struct __Visitor;
                impl<'de> ::serde::de::Visitor<'de> for __Visitor {
                    type Value = #name;
                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        f.write_str(#name_str)
                    }
                    fn visit_unit<__E: ::serde::de::Error>(self) -> ::core::result::Result<#name, __E> {
                        Ok(#name)
                    }
                }
                __deserializer.deserialize_unit_struct(#name_str, __Visitor)
            }
        }

        impl #impl_gen_ser ::cord::CordSchema for #name #gp #ser_bounds {
            fn schema() -> ::cord::Schema {
                ::cord::Schema::Unit
            }
        }

        impl #impl_gen_ser ::cord::CordEncode for #name #gp #ser_bounds {
            fn encode_cord(&self, _buf: &mut ::std::vec::Vec<u8>) -> ::cord::CordResult<()> {
                Ok(())
            }
        }

        impl ::cord::CordDecode for #name #gp {
            fn decode_cord(_input: &mut &[u8]) -> ::cord::CordResult<Self> {
                Ok(#name)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Enum derive
// ---------------------------------------------------------------------------

fn derive_enum(input: &DeriveInput, data: &DataEnum) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let gp = generic_params(generics, false);
    let name_str = name.to_string();

    // Parse variant indices
    let mut indices: Vec<(u32, &Variant)> = Vec::new();
    let mut has_any_index = false;
    let mut has_missing_index = false;

    for (i, variant) in data.variants.iter().enumerate() {
        let idx = parse_cord_variant_index(variant)?;
        if idx.is_some() {
            has_any_index = true;
        } else {
            has_missing_index = true;
        }
        indices.push((idx.unwrap_or(i as u32), variant));
    }

    if has_any_index && has_missing_index {
        return Err(syn::Error::new_spanned(
            name,
            "if any variant has #[cord(index = N)], all variants must have it",
        ));
    }

    // Check for duplicate indices
    let mut seen = std::collections::HashSet::new();
    for (idx, variant) in &indices {
        if !seen.insert(idx) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("duplicate variant index {}", idx),
            ));
        }
    }

    let impl_generics = &generics.params;
    let impl_gen_ser = if impl_generics.is_empty() {
        quote!()
    } else {
        quote!(<#impl_generics>)
    };
    let impl_gen_de = if impl_generics.is_empty() {
        quote!(<'de>)
    } else {
        quote!(<'de, #impl_generics>)
    };

    let ser_bounds = add_bounds(generics, quote!(::serde::Serialize));
    let de_bounds = add_bounds(generics, quote!(::serde::Deserialize<'de>));

    // --- Serialize ---
    let ser_arms: Vec<TokenStream2> = indices
        .iter()
        .map(|(idx, variant)| {
            let vname = &variant.ident;
            let vname_str = vname.to_string();

            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        #name::#vname => {
                            serializer.serialize_unit_variant(#name_str, #idx, #vname_str)
                        }
                    }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    quote! {
                        #name::#vname(ref __v0) => {
                            serializer.serialize_newtype_variant(#name_str, #idx, #vname_str, __v0)
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let count = fields.unnamed.len();
                    let field_idents: Vec<_> =
                        (0..count).map(|i| format_ident!("__v{}", i)).collect();
                    let pattern = quote!( #(ref #field_idents),* );
                    let field_sers: Vec<TokenStream2> = field_idents
                        .iter()
                        .map(|id| quote! { __state.serialize_field(#id)?; })
                        .collect();
                    quote! {
                        #name::#vname(#pattern) => {
                            use ::serde::ser::SerializeTupleVariant;
                            let mut __state = serializer.serialize_tuple_variant(
                                #name_str, #idx, #vname_str, #count,
                            )?;
                            #(#field_sers)*
                            __state.end()
                        }
                    }
                }
                Fields::Named(fields) => {
                    let count = fields.named.len();
                    let field_names: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    let pattern = quote!( #(ref #field_names),* );
                    let field_sers: Vec<TokenStream2> = field_names
                        .iter()
                        .map(|n| {
                            let ns = n.to_string();
                            quote! { __state.serialize_field(#ns, #n)?; }
                        })
                        .collect();
                    quote! {
                        #name::#vname { #pattern } => {
                            use ::serde::ser::SerializeStructVariant;
                            let mut __state = serializer.serialize_struct_variant(
                                #name_str, #idx, #vname_str, #count,
                            )?;
                            #(#field_sers)*
                            __state.end()
                        }
                    }
                }
            }
        })
        .collect();

    // --- Deserialize ---
    let variant_names: Vec<_> = data.variants.iter().map(|v| v.ident.to_string()).collect();

    let expecting_str = format!("enum {}", name);

    // Build __Field enum variants and match arms for deserialization
    let field_variant_idents: Vec<_> = indices
        .iter()
        .map(|(_, variant)| format_ident!("__Variant{}", variant.ident))
        .collect();

    let field_from_u32_arms: Vec<TokenStream2> = indices
        .iter()
        .zip(field_variant_idents.iter())
        .map(|((idx, _), fident)| {
            quote! { #idx => ::core::result::Result::Ok(__Field::#fident), }
        })
        .collect();

    let de_arms_field: Vec<TokenStream2> = indices
        .iter()
        .zip(field_variant_idents.iter())
        .map(|((idx, variant), fident)| {
            let vname = &variant.ident;
            // Reuse the body from de_arms but with __Field pattern
            let _ = idx; // idx already baked into __Field
            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        __Field::#fident => {
                            __variant.unit_variant()?;
                            Ok(#name::#vname)
                        }
                    }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    quote! {
                        __Field::#fident => {
                            Ok(#name::#vname(__variant.newtype_variant()?))
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let count = fields.unnamed.len();
                    let field_idents: Vec<_> = (0..count)
                        .map(|i| format_ident!("__v{}", i))
                        .collect();
                    let de_stmts: Vec<TokenStream2> = field_idents
                        .iter()
                        .enumerate()
                        .map(|(i, id)| {
                            quote! {
                                let #id = __seq
                                    .next_element()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                            }
                        })
                        .collect();
                    quote! {
                        __Field::#fident => {
                            struct __TupleVisitor;
                            impl<'de> ::serde::de::Visitor<'de> for __TupleVisitor {
                                type Value = #name;
                                fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                                    f.write_str("tuple variant")
                                }
                                fn visit_seq<__A: ::serde::de::SeqAccess<'de>>(
                                    self,
                                    mut __seq: __A,
                                ) -> ::core::result::Result<#name, __A::Error> {
                                    #(#de_stmts)*
                                    Ok(#name::#vname(#(#field_idents),*))
                                }
                            }
                            __variant.tuple_variant(#count, __TupleVisitor)
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_names: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    let field_strs: Vec<_> =
                        field_names.iter().map(|n| n.to_string()).collect();
                    let de_stmts: Vec<TokenStream2> = field_names
                        .iter()
                        .enumerate()
                        .map(|(i, fname)| {
                            quote! {
                                let #fname = __seq
                                    .next_element()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(#i, &self))?;
                            }
                        })
                        .collect();
                    quote! {
                        __Field::#fident => {
                            struct __StructVisitor;
                            impl<'de> ::serde::de::Visitor<'de> for __StructVisitor {
                                type Value = #name;
                                fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                                    f.write_str("struct variant")
                                }
                                fn visit_seq<__A: ::serde::de::SeqAccess<'de>>(
                                    self,
                                    mut __seq: __A,
                                ) -> ::core::result::Result<#name, __A::Error> {
                                    #(#de_stmts)*
                                    Ok(#name::#vname { #(#field_names),* })
                                }
                            }
                            const FIELD_NAMES: &[&str] = &[#(#field_strs),*];
                            __variant.struct_variant(FIELD_NAMES, __StructVisitor)
                        }
                    }
                }
            }
        })
        .collect();

    // --- CordSchema ---
    let schema_variants: Vec<TokenStream2> = indices
        .iter()
        .map(|(_, variant)| {
            let vname_str = variant.ident.to_string();
            match &variant.fields {
                Fields::Unit => {
                    quote! { (#vname_str.to_string(), ::cord::VariantSchema::Unit) }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    let field_ty = &fields.unnamed.first().unwrap().ty;
                    quote! { (#vname_str.to_string(), ::cord::VariantSchema::Newtype(<#field_ty as ::cord::CordSchema>::schema())) }
                }
                Fields::Unnamed(fields) => {
                    let field_schemas: Vec<TokenStream2> = fields.unnamed.iter().map(|f| {
                        let ty = &f.ty;
                        quote! { <#ty as ::cord::CordSchema>::schema() }
                    }).collect();
                    quote! { (#vname_str.to_string(), ::cord::VariantSchema::Tuple(::std::vec![#(#field_schemas),*])) }
                }
                Fields::Named(fields) => {
                    let field_schemas: Vec<TokenStream2> = fields.named.iter().map(|f| {
                        let fname = f.ident.as_ref().unwrap().to_string();
                        let ty = &f.ty;
                        quote! { (#fname.to_string(), <#ty as ::cord::CordSchema>::schema()) }
                    }).collect();
                    quote! { (#vname_str.to_string(), ::cord::VariantSchema::Struct(::std::vec![#(#field_schemas),*])) }
                }
            }
        })
        .collect();

    // --- CordEncode ---
    let encode_arms: Vec<TokenStream2> = indices
        .iter()
        .map(|(idx, variant)| {
            let vname = &variant.ident;
            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        #name::#vname => {
                            ::cord::__private::encode_variant_index(__buf, #idx, ::cord::Width::W32)?;
                        }
                    }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    quote! {
                        #name::#vname(ref __v0) => {
                            ::cord::__private::encode_variant_index(__buf, #idx, ::cord::Width::W32)?;
                            ::cord::CordEncode::encode_cord(__v0, __buf)?;
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let field_idents: Vec<_> = (0..fields.unnamed.len())
                        .map(|i| format_ident!("__v{}", i))
                        .collect();
                    let pattern = quote!( #(ref #field_idents),* );
                    let encodes: Vec<TokenStream2> = field_idents
                        .iter()
                        .map(|id| quote! { ::cord::CordEncode::encode_cord(#id, __buf)?; })
                        .collect();
                    quote! {
                        #name::#vname(#pattern) => {
                            ::cord::__private::encode_variant_index(__buf, #idx, ::cord::Width::W32)?;
                            #(#encodes)*
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_names: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    let pattern = quote!( #(ref #field_names),* );
                    let encodes: Vec<TokenStream2> = field_names
                        .iter()
                        .map(|n| quote! { ::cord::CordEncode::encode_cord(#n, __buf)?; })
                        .collect();
                    quote! {
                        #name::#vname { #pattern } => {
                            ::cord::__private::encode_variant_index(__buf, #idx, ::cord::Width::W32)?;
                            #(#encodes)*
                        }
                    }
                }
            }
        })
        .collect();

    // --- CordDecode ---
    let decode_arms: Vec<TokenStream2> = indices
        .iter()
        .map(|(idx, variant)| {
            let vname = &variant.ident;
            match &variant.fields {
                Fields::Unit => {
                    quote! { #idx => Ok(#name::#vname), }
                }
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    let ty = &fields.unnamed.first().unwrap().ty;
                    quote! { #idx => Ok(#name::#vname(<#ty as ::cord::CordDecode>::decode_cord(__input)?)), }
                }
                Fields::Unnamed(fields) => {
                    let field_decodes: Vec<TokenStream2> = fields.unnamed.iter().enumerate().map(|(i, f)| {
                        let ident = format_ident!("__v{}", i);
                        let ty = &f.ty;
                        quote! { let #ident = <#ty as ::cord::CordDecode>::decode_cord(__input)?; }
                    }).collect();
                    let field_idents: Vec<_> = (0..fields.unnamed.len())
                        .map(|i| format_ident!("__v{}", i))
                        .collect();
                    quote! {
                        #idx => {
                            #(#field_decodes)*
                            Ok(#name::#vname(#(#field_idents),*))
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_decodes: Vec<TokenStream2> = fields.named.iter().map(|f| {
                        let fname = f.ident.as_ref().unwrap();
                        let ty = &f.ty;
                        quote! { let #fname = <#ty as ::cord::CordDecode>::decode_cord(__input)?; }
                    }).collect();
                    let field_names: Vec<_> = fields.named.iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    quote! {
                        #idx => {
                            #(#field_decodes)*
                            Ok(#name::#vname { #(#field_names),* })
                        }
                    }
                }
            }
        })
        .collect();

    Ok(quote! {
        impl #impl_gen_ser ::serde::Serialize for #name #gp #ser_bounds {
            fn serialize<__S: ::serde::Serializer>(&self, serializer: __S)
                -> ::core::result::Result<__S::Ok, __S::Error>
            {
                match self {
                    #(#ser_arms)*
                }
            }
        }

        impl #impl_gen_de ::serde::Deserialize<'de> for #name #gp #de_bounds {
            fn deserialize<__D: ::serde::Deserializer<'de>>(__deserializer: __D)
                -> ::core::result::Result<Self, __D::Error>
            {
                #[allow(non_camel_case_types)]
                enum __Field {
                    #(#field_variant_idents,)*
                }

                struct __FieldVisitor;

                impl<'de> ::serde::de::Visitor<'de> for __FieldVisitor {
                    type Value = __Field;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        f.write_str(#expecting_str)
                    }

                    fn visit_u32<__E: ::serde::de::Error>(
                        self,
                        __value: u32,
                    ) -> ::core::result::Result<__Field, __E> {
                        match __value {
                            #(#field_from_u32_arms)*
                            _ => ::core::result::Result::Err(::serde::de::Error::invalid_value(
                                ::serde::de::Unexpected::Unsigned(__value as u64),
                                &self,
                            )),
                        }
                    }
                }

                impl<'de> ::serde::Deserialize<'de> for __Field {
                    fn deserialize<__D2: ::serde::Deserializer<'de>>(__deserializer: __D2)
                        -> ::core::result::Result<Self, __D2::Error>
                    {
                        __deserializer.deserialize_identifier(__FieldVisitor)
                    }
                }

                struct __Visitor;

                impl<'de> ::serde::de::Visitor<'de> for __Visitor {
                    type Value = #name;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        f.write_str(#expecting_str)
                    }

                    fn visit_enum<__A: ::serde::de::EnumAccess<'de>>(
                        self,
                        __data: __A,
                    ) -> ::core::result::Result<#name, __A::Error> {
                        use ::serde::de::VariantAccess;
                        let (__field, __variant): (__Field, _) = __data.variant()?;
                        match __field {
                            #(#de_arms_field)*
                        }
                    }
                }

                const VARIANTS: &[&str] = &[#(#variant_names),*];
                __deserializer.deserialize_enum(#name_str, VARIANTS, __Visitor)
            }
        }

        impl #impl_gen_ser ::cord::CordSchema for #name #gp #ser_bounds {
            fn schema() -> ::cord::Schema {
                ::cord::Schema::Enum(
                    ::std::vec![#(#schema_variants),*],
                    ::cord::Width::default(),
                )
            }
        }

        impl #impl_gen_ser ::cord::CordEncode for #name #gp #ser_bounds {
            fn encode_cord(&self, __buf: &mut ::std::vec::Vec<u8>) -> ::cord::CordResult<()> {
                match self {
                    #(#encode_arms)*
                }
                Ok(())
            }
        }

        impl ::cord::CordDecode for #name #gp {
            fn decode_cord(__input: &mut &[u8]) -> ::cord::CordResult<Self> {
                let __variant_index = ::cord::__private::decode_variant_index(__input, ::cord::Width::W32)?;
                match __variant_index {
                    #(#decode_arms)*
                    _ => ::core::result::Result::Err(::cord::CordError::UnknownVariant(__variant_index)),
                }
            }
        }
    })
}
