//! Proc macros for the sealed Lattice Axiom registration IR.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use latticeaxiom_core::{PackageName, PackageVersion, SchemaId, StableId};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Expr, ExprLit, Fields, FnArg, GenericArgument, Ident, ItemFn, ItemStruct, Lit,
    LitStr, Meta, MetaList, MetaNameValue, PathArguments, ReturnType, Token, Type,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
    spanned::Spanned,
};

const STAGES_V1: [&str; 10] = [
    "latticeaxiom:system-stage/input/sample@1",
    "latticeaxiom:system-stage/gameplay/fixed-pre@1",
    "latticeaxiom:system-stage/gameplay/fixed@1",
    "latticeaxiom:system-stage/physics/integrate@1",
    "latticeaxiom:system-stage/world/commands-apply@1",
    "latticeaxiom:system-stage/world/revision-commit@1",
    "latticeaxiom:system-stage/derived-work/queue@1",
    "latticeaxiom:system-stage/persistence/capture@1",
    "latticeaxiom:system-stage/async-results/observe@1",
    "latticeaxiom:system-stage/presentation/update@1",
];

/// Declares one stable component and its schema realization mode.
#[proc_macro_attribute]
pub fn component(arguments: TokenStream, item: TokenStream) -> TokenStream {
    emit(expand_component(arguments.into(), item.into()))
}

/// Declares one row-kernel system and its canonical dual/static signature.
#[proc_macro_attribute]
pub fn system(arguments: TokenStream, item: TokenStream) -> TokenStream {
    emit(expand_system(arguments.into(), item.into()))
}

/// Builds sealed registration IR from explicit component and system lists.
#[proc_macro]
pub fn registration_ir(input: TokenStream) -> TokenStream {
    emit(expand_registration_ir(input.into()))
}

fn emit(result: syn::Result<TokenStream2>) -> TokenStream {
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

#[derive(Default)]
struct ComponentArguments {
    id: Option<LitStr>,
    schema: Option<LitStr>,
    mode: Option<ComponentModeArgument>,
}

#[derive(Clone, Copy)]
enum ComponentModeArgument {
    HostTyped,
    GeneratedSharedSchema,
    RuntimeDynamic,
}

fn expand_component(arguments: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let arguments = parse_component_arguments(arguments)?;
    let item: ItemStruct = syn::parse2(item)?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new(
            item.generics.span(),
            "LAX-SDK-COMPONENT-001: generic component declarations are not supported by schema v1",
        ));
    }
    if !has_repr_c(&item.attrs) {
        return Err(syn::Error::new(
            item.ident.span(),
            "LAX-SDK-COMPONENT-002: code-bound components require #[repr(C)]",
        ));
    }
    let Fields::Named(fields) = &item.fields else {
        return Err(syn::Error::new(
            item.fields.span(),
            "LAX-SDK-COMPONENT-003: code-bound components require non-empty named fields",
        ));
    };
    if fields.named.is_empty() {
        return Err(syn::Error::new(
            fields.span(),
            "LAX-SDK-COMPONENT-003: code-bound components require non-empty named fields",
        ));
    }
    for field in &fields.named {
        reject_known_non_pod(&field.ty)?;
    }

    let id = required(arguments.id, item.ident.span(), "id")?;
    validate_stable_id("component id", &id, false)?;
    let mode = arguments.mode.ok_or_else(|| {
        syn::Error::new(
            item.ident.span(),
            "LAX-SDK-COMPONENT-004: missing component mode",
        )
    })?;
    if !matches!(mode, ComponentModeArgument::HostTyped) && arguments.schema.is_none() {
        return Err(syn::Error::new(
            item.ident.span(),
            "LAX-SDK-COMPONENT-005: generated and runtime components require schema = \"...@major\"",
        ));
    }
    if let Some(schema) = &arguments.schema {
        validate_schema_id(schema)?;
    }

    let schema = arguments.schema.map_or_else(
        || quote!(::core::option::Option::None),
        |schema| quote!(::core::option::Option::Some(#schema)),
    );
    let mode = match mode {
        ComponentModeArgument::HostTyped => {
            quote!(::latticeaxiom_sdk::ComponentMode::HostTyped)
        }
        ComponentModeArgument::GeneratedSharedSchema => {
            quote!(::latticeaxiom_sdk::ComponentMode::GeneratedSharedSchema)
        }
        ComponentModeArgument::RuntimeDynamic => {
            quote!(::latticeaxiom_sdk::ComponentMode::RuntimeDynamic)
        }
    };
    let ident = &item.ident;
    let field_types = fields.named.iter().map(|field| &field.ty);
    let rust_api = component_api_descriptor(&item);
    Ok(quote! {
        #item

        const _: fn() = || {
            fn assert_abi_pod<T: ::latticeaxiom_sdk::__private::AbiPodField>() {}
            #(assert_abi_pod::<#field_types>();)*
        };

        impl ::latticeaxiom_sdk::__private::AbiPodField for #ident {}

        impl ::latticeaxiom_sdk::__private::SealedComponent for #ident {}

        impl ::latticeaxiom_sdk::ComponentContract for #ident {
            fn __registration_seed() -> ::latticeaxiom_sdk::__private::ComponentSeed {
                ::latticeaxiom_sdk::__private::ComponentSeed::new(
                    #id,
                    #schema,
                    #mode,
                    #rust_api,
                )
            }
        }
    })
}

fn parse_component_arguments(input: TokenStream2) -> syn::Result<ComponentArguments> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(input)?;
    let mut arguments = ComponentArguments::default();
    for meta in metas {
        let Meta::NameValue(value) = meta else {
            return Err(syn::Error::new(meta.span(), "expected `name = value`"));
        };
        if value.path.is_ident("id") {
            set_once(&mut arguments.id, string_value(&value)?, value.span(), "id")?;
        } else if value.path.is_ident("schema") {
            set_once(
                &mut arguments.schema,
                string_value(&value)?,
                value.span(),
                "schema",
            )?;
        } else if value.path.is_ident("mode") {
            let literal = string_value(&value)?;
            let parsed = match literal.value().as_str() {
                "host-typed" => ComponentModeArgument::HostTyped,
                "generated-shared-schema" => ComponentModeArgument::GeneratedSharedSchema,
                "runtime-dynamic" => ComponentModeArgument::RuntimeDynamic,
                _ => {
                    return Err(syn::Error::new(
                        literal.span(),
                        "LAX-SDK-COMPONENT-006: mode must be host-typed, generated-shared-schema, or runtime-dynamic",
                    ));
                }
            };
            set_once(&mut arguments.mode, parsed, value.span(), "mode")?;
        } else {
            return Err(syn::Error::new(
                value.path.span(),
                "unknown component attribute field",
            ));
        }
    }
    Ok(arguments)
}

#[derive(Default)]
struct SystemArguments {
    id: Option<LitStr>,
    callback: Option<LitStr>,
    stage: Option<LitStr>,
    set: Option<LitStr>,
    policy: Option<SystemPolicyArgument>,
    after: Vec<LitStr>,
    before: Vec<LitStr>,
}

#[derive(Clone, Copy)]
enum SystemPolicyArgument {
    Dual,
    Auto,
    NativeStaticOnly,
}

fn expand_system(arguments: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let arguments = parse_system_arguments(arguments)?;
    let item: ItemFn = syn::parse2(item)?;
    validate_row_kernel_shape(&item)?;
    let id = required(arguments.id, item.sig.ident.span(), "id")?;
    let callback = required(arguments.callback, item.sig.ident.span(), "callback")?;
    let stage = required(arguments.stage, item.sig.ident.span(), "stage")?;
    let policy = arguments.policy.ok_or_else(|| {
        syn::Error::new(
            item.sig.ident.span(),
            "LAX-SDK-SYSTEM-001: missing system policy",
        )
    })?;
    validate_stable_id("system id", &id, false)?;
    validate_stable_id("callback key", &callback, true)?;
    validate_stable_id("system stage", &stage, true)?;
    if !STAGES_V1.contains(&stage.value().as_str()) {
        return Err(syn::Error::new(
            stage.span(),
            "LAX-SDK-SYSTEM-002: stage is not in the system-stage catalog v1",
        ));
    }
    if let Some(set) = &arguments.set {
        validate_stable_id("system set", set, false)?;
    }
    for edge in arguments.after.iter().chain(&arguments.before) {
        validate_stable_id("system ordering edge", edge, false)?;
        if edge.value() == id.value() {
            return Err(syn::Error::new(
                edge.span(),
                "LAX-SDK-SYSTEM-003: a system cannot order against itself",
            ));
        }
    }

    let mut parameter_seeds = Vec::new();
    for input in &item.sig.inputs {
        let FnArg::Typed(parameter) = input else {
            return Err(syn::Error::new(
                input.span(),
                "LAX-SDK-PARAM-001: methods cannot be dual-realization row kernels",
            ));
        };
        parameter_seeds.push(classify_parameter(&parameter.ty, policy)?);
    }

    let set = arguments.set.map_or_else(
        || quote!(::core::option::Option::None),
        |value| quote!(::core::option::Option::Some(#value)),
    );
    let after = arguments.after;
    let before = arguments.before;
    let policy = match policy {
        SystemPolicyArgument::Dual => quote!(::latticeaxiom_sdk::__private::SystemPolicySeed::Dual),
        SystemPolicyArgument::Auto => quote!(::latticeaxiom_sdk::__private::SystemPolicySeed::Auto),
        SystemPolicyArgument::NativeStaticOnly => {
            quote!(::latticeaxiom_sdk::__private::SystemPolicySeed::NativeStaticOnly)
        }
    };
    let ident = &item.sig.ident;
    let seed_ident = format_ident!("__latticeaxiom_system_{}_seed", ident);
    let signature_descriptor = item.sig.to_token_stream().to_string();
    Ok(quote! {
        #item

        #[doc(hidden)]
        fn #seed_ident() -> ::latticeaxiom_sdk::__private::SystemSeed {
            ::latticeaxiom_sdk::__private::SystemSeed::new(
                #id,
                #callback,
                #stage,
                #set,
                ::std::vec![#(#after),*],
                ::std::vec![#(#before),*],
                #policy,
                ::std::vec![#(#parameter_seeds),*],
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                #signature_descriptor,
            )
        }
    })
}

fn parse_system_arguments(input: TokenStream2) -> syn::Result<SystemArguments> {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(input)?;
    let mut arguments = SystemArguments::default();
    for meta in metas {
        match meta {
            Meta::NameValue(value) if value.path.is_ident("id") => {
                set_once(&mut arguments.id, string_value(&value)?, value.span(), "id")?;
            }
            Meta::NameValue(value) if value.path.is_ident("callback") => {
                set_once(
                    &mut arguments.callback,
                    string_value(&value)?,
                    value.span(),
                    "callback",
                )?;
            }
            Meta::NameValue(value) if value.path.is_ident("stage") => {
                set_once(
                    &mut arguments.stage,
                    string_value(&value)?,
                    value.span(),
                    "stage",
                )?;
            }
            Meta::NameValue(value) if value.path.is_ident("set") => {
                set_once(
                    &mut arguments.set,
                    string_value(&value)?,
                    value.span(),
                    "set",
                )?;
            }
            Meta::NameValue(value) if value.path.is_ident("policy") => {
                let literal = string_value(&value)?;
                let parsed = match literal.value().as_str() {
                    "dual" => SystemPolicyArgument::Dual,
                    "auto" => SystemPolicyArgument::Auto,
                    "native-static-only" => SystemPolicyArgument::NativeStaticOnly,
                    _ => {
                        return Err(syn::Error::new(
                            literal.span(),
                            "LAX-SDK-SYSTEM-004: policy must be dual, auto, or native-static-only",
                        ));
                    }
                };
                set_once(&mut arguments.policy, parsed, value.span(), "policy")?;
            }
            Meta::List(list) if list.path.is_ident("after") => {
                arguments.after = parse_literal_list(&list)?;
            }
            Meta::List(list) if list.path.is_ident("before") => {
                arguments.before = parse_literal_list(&list)?;
            }
            _ => {
                return Err(syn::Error::new(
                    meta.span(),
                    "unknown system attribute field",
                ));
            }
        }
    }
    reject_duplicate_literals(&arguments.after, "after")?;
    reject_duplicate_literals(&arguments.before, "before")?;
    Ok(arguments)
}

fn validate_row_kernel_shape(item: &ItemFn) -> syn::Result<()> {
    let signature = &item.sig;
    if signature.asyncness.is_some()
        || signature.constness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
        || !signature.generics.params.is_empty()
    {
        return Err(syn::Error::new(
            signature.span(),
            "LAX-SDK-SYSTEM-005: row kernels must be safe, synchronous, non-generic Rust functions",
        ));
    }
    if !matches!(signature.output, ReturnType::Default) {
        return Err(syn::Error::new(
            signature.output.span(),
            "LAX-SDK-SYSTEM-006: row kernels return commands through sinks and must return ()",
        ));
    }
    Ok(())
}

fn classify_parameter(ty: &Type, policy: SystemPolicyArgument) -> syn::Result<TokenStream2> {
    let Type::Path(path) = ty else {
        return unsupported_parameter(ty, policy);
    };
    let Some(segment) = path.path.segments.last() else {
        return unsupported_parameter(ty, policy);
    };
    match segment.ident.to_string().as_str() {
        "Read" => component_parameter(segment, "Required", "Read"),
        "OptionalRead" => component_parameter(segment, "Optional", "Read"),
        "Write" => component_parameter(segment, "Required", "Write"),
        "OptionalWrite" => component_parameter(segment, "Optional", "Write"),
        "With" => filter_parameter(segment, true),
        "Without" => filter_parameter(segment, false),
        "RowEntity" if no_type_arguments(segment) => {
            Ok(quote!(::latticeaxiom_sdk::__private::row_entity_parameter()))
        }
        "FixedTick" if no_type_arguments(segment) => {
            Ok(quote!(::latticeaxiom_sdk::__private::fixed_tick_parameter()))
        }
        "CommandSink" if has_only_lifetime_arguments(segment) => Ok(quote!(
            ::latticeaxiom_sdk::__private::command_sink_parameter()
        )),
        _ => unsupported_parameter(ty, policy),
    }
}

fn component_parameter(
    segment: &syn::PathSegment,
    requirement: &str,
    access: &str,
) -> syn::Result<TokenStream2> {
    let component = single_type_argument(segment)?;
    let requirement = Ident::new(requirement, Span::call_site());
    let access = Ident::new(access, Span::call_site());
    Ok(quote! {
        ::latticeaxiom_sdk::__private::component_parameter::<#component>(
            ::latticeaxiom_sdk::ComponentRequirement::#requirement,
            ::latticeaxiom_sdk::ComponentAccessKind::#access,
        )
    })
}

fn filter_parameter(segment: &syn::PathSegment, with: bool) -> syn::Result<TokenStream2> {
    let component = single_type_argument(segment)?;
    Ok(quote! {
        ::latticeaxiom_sdk::__private::filter_parameter::<#component>(#with)
    })
}

fn unsupported_parameter(ty: &Type, policy: SystemPolicyArgument) -> syn::Result<TokenStream2> {
    if matches!(policy, SystemPolicyArgument::Dual) {
        return Err(syn::Error::new(
            ty.span(),
            "LAX-SDK-PARAM-002: unsupported dual-realization SystemParam; use the SDK row vocabulary or choose auto/native-static-only",
        ));
    }
    Ok(quote! {
        ::latticeaxiom_sdk::__private::static_only_parameter(
            ::latticeaxiom_sdk::NativeStaticOnlyReason::UnsupportedSystemParameter,
        )
    })
}

fn single_type_argument(segment: &syn::PathSegment) -> syn::Result<&Type> {
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new(
            segment.span(),
            "LAX-SDK-PARAM-003: component row parameters require one component type",
        ));
    };
    let types = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect::<Vec<_>>();
    if types.len() != 1 {
        return Err(syn::Error::new(
            arguments.span(),
            "LAX-SDK-PARAM-003: component row parameters require one component type",
        ));
    }
    types.first().copied().ok_or_else(|| {
        syn::Error::new(
            arguments.span(),
            "LAX-SDK-PARAM-003: component row parameters require one component type",
        )
    })
}

fn no_type_arguments(segment: &syn::PathSegment) -> bool {
    matches!(segment.arguments, PathArguments::None)
}

fn has_only_lifetime_arguments(segment: &syn::PathSegment) -> bool {
    match &segment.arguments {
        PathArguments::None => true,
        PathArguments::AngleBracketed(arguments) => arguments
            .args
            .iter()
            .all(|argument| matches!(argument, GenericArgument::Lifetime(_))),
        PathArguments::Parenthesized(_) => false,
    }
}

struct RegistrationIrInput {
    package: LitStr,
    version: LitStr,
    components: Vec<Ident>,
    systems: Vec<Ident>,
}

impl Parse for RegistrationIrInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut package = None;
        let mut version = None;
        let mut components = None;
        let mut systems = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "package" {
                set_once(&mut package, input.parse()?, key.span(), "package")?;
            } else if key == "version" {
                set_once(&mut version, input.parse()?, key.span(), "version")?;
            } else if key == "components" {
                set_once(
                    &mut components,
                    parse_ident_array(input)?,
                    key.span(),
                    "components",
                )?;
            } else if key == "systems" {
                set_once(
                    &mut systems,
                    parse_ident_array(input)?,
                    key.span(),
                    "systems",
                )?;
            } else {
                return Err(syn::Error::new(key.span(), "unknown registration_ir field"));
            }
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            package: required(package, Span::call_site(), "package")?,
            version: required(version, Span::call_site(), "version")?,
            components: required(components, Span::call_site(), "components")?,
            systems: required(systems, Span::call_site(), "systems")?,
        })
    }
}

fn expand_registration_ir(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: RegistrationIrInput = syn::parse2(input)?;
    input
        .package
        .value()
        .parse::<PackageName>()
        .map_err(|error| syn::Error::new(input.package.span(), error.to_string()))?;
    input
        .version
        .value()
        .parse::<PackageVersion>()
        .map_err(|error| syn::Error::new(input.version.span(), error.to_string()))?;
    reject_duplicate_idents(&input.components, "component")?;
    reject_duplicate_idents(&input.systems, "system")?;
    let package = input.package;
    let version = input.version;
    let components = input.components;
    let system_seeds = input.systems.iter().map(|system| {
        let seed = format_ident!("__latticeaxiom_system_{}_seed", system);
        quote!(#seed())
    });
    Ok(quote! {
        ::latticeaxiom_sdk::__private::build_registration_ir(
            #package,
            #version,
            ::std::vec![
                #(<#components as ::latticeaxiom_sdk::ComponentContract>::__registration_seed()),*
            ],
            ::std::vec![#(#system_seeds),*],
        )
    })
}

fn parse_ident_array(input: ParseStream<'_>) -> syn::Result<Vec<Ident>> {
    let content;
    syn::bracketed!(content in input);
    Ok(Punctuated::<Ident, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

fn parse_literal_list(list: &MetaList) -> syn::Result<Vec<LitStr>> {
    Ok(Punctuated::<LitStr, Token![,]>::parse_terminated
        .parse2(list.tokens.clone())?
        .into_iter()
        .collect())
}

fn string_value(value: &MetaNameValue) -> syn::Result<LitStr> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = &value.value
    else {
        return Err(syn::Error::new(
            value.value.span(),
            "attribute value must be a string literal",
        ));
    };
    Ok(value.clone())
}

fn required<T>(value: Option<T>, span: Span, name: &str) -> syn::Result<T> {
    value.ok_or_else(|| syn::Error::new(span, format!("missing `{name}`")))
}

fn set_once<T>(slot: &mut Option<T>, value: T, span: Span, name: &str) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        Err(syn::Error::new(span, format!("duplicate `{name}`")))
    } else {
        Ok(())
    }
}

fn validate_stable_id(field: &str, literal: &LitStr, require_major: bool) -> syn::Result<()> {
    let value = literal.value();
    value
        .parse::<StableId>()
        .map_err(|error| syn::Error::new(literal.span(), format!("invalid {field}: {error}")))?;
    if require_major && !has_positive_major(&value) {
        return Err(syn::Error::new(
            literal.span(),
            format!("{field} must include a positive @major suffix"),
        ));
    }
    Ok(())
}

fn validate_schema_id(literal: &LitStr) -> syn::Result<()> {
    literal
        .value()
        .parse::<SchemaId>()
        .map(|_| ())
        .map_err(|error| syn::Error::new(literal.span(), error.to_string()))
}

fn has_positive_major(value: &str) -> bool {
    value
        .rsplit_once('@')
        .and_then(|(_, major)| major.parse::<u64>().ok())
        .is_some_and(|major| major > 0)
}

fn has_repr_c(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("repr")
            && attribute.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .to_string()
                    .split(',')
                    .any(|part| part.trim() == "C")
            })
    })
}

fn reject_known_non_pod(ty: &Type) -> syn::Result<()> {
    match ty {
        Type::Reference(_) | Type::Ptr(_) | Type::BareFn(_) | Type::TraitObject(_) => {
            Err(syn::Error::new(
                ty.span(),
                "LAX-SDK-COMPONENT-007: references, pointers, functions, and trait objects are not ABI-POD fields",
            ))
        }
        Type::Path(path) => {
            let rejected = path.path.segments.last().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "bool" | "char" | "usize" | "isize" | "String" | "Vec" | "Box" | "Rc" | "Arc"
                )
            });
            if rejected {
                Err(syn::Error::new(
                    ty.span(),
                    "LAX-SDK-COMPONENT-008: field type is not in the fixed-width ABI-POD vocabulary",
                ))
            } else {
                Ok(())
            }
        }
        Type::Array(array) => {
            reject_known_non_pod(&array.elem)?;
            if matches!(&array.len, Expr::Lit(ExprLit { lit: Lit::Int(value), .. }) if value.base10_digits() == "0")
            {
                return Err(syn::Error::new(
                    array.len.span(),
                    "LAX-SDK-COMPONENT-009: zero-length arrays are not accepted in schema v1",
                ));
            }
            Ok(())
        }
        _ => Err(syn::Error::new(
            ty.span(),
            "LAX-SDK-COMPONENT-010: unsupported ABI-POD field syntax",
        )),
    }
}

fn component_api_descriptor(item: &ItemStruct) -> String {
    let ident = &item.ident;
    let fields = item.fields.iter().map(|field| {
        let name = &field.ident;
        let ty = &field.ty;
        quote!(#name: #ty)
    });
    quote!(struct #ident { #(#fields),* }).to_string()
}

fn reject_duplicate_literals(values: &[LitStr], field: &str) -> syn::Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.value()) {
            return Err(syn::Error::new(
                value.span(),
                format!("duplicate `{field}` edge"),
            ));
        }
    }
    Ok(())
}

fn reject_duplicate_idents(values: &[Ident], field: &str) -> syn::Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.to_string()) {
            return Err(syn::Error::new(
                value.span(),
                format!("duplicate `{field}` entry"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_parameter_whitelist_accepts_every_foundation_shape() {
        for source in [
            "Read<'_, Position>",
            "OptionalRead<'_, Position>",
            "Write<'_, Position>",
            "OptionalWrite<'_, Position>",
            "With<Position>",
            "Without<Position>",
            "RowEntity",
            "FixedTick",
            "CommandSink<'_>",
        ] {
            let parsed = syn::parse_str::<Type>(source);
            assert!(
                parsed
                    .as_ref()
                    .is_ok_and(|ty| classify_parameter(ty, SystemPolicyArgument::Dual).is_ok()),
                "whitelisted parameter failed: {source}"
            );
        }
    }

    #[test]
    fn unsupported_dynamic_parameters_are_dual_errors_and_auto_static_only() {
        for source in [
            "World",
            "Query<'_, Position>",
            "Local<'_, u32>",
            "NonSend<'_, u32>",
            "ParamSet<'_, ()>",
            "Entity",
            "Handle<Image>",
            "AssetServer",
            "CustomSystemParam",
        ] {
            let parsed = syn::parse_str::<Type>(source);
            assert!(parsed.as_ref().is_ok_and(|ty| {
                classify_parameter(ty, SystemPolicyArgument::Dual).is_err()
                    && classify_parameter(ty, SystemPolicyArgument::Auto).is_ok()
            }));
        }
    }

    #[test]
    fn stage_catalog_is_exact() {
        assert_eq!(STAGES_V1.len(), 10);
        assert!(STAGES_V1.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn schema_macro_rejects_missing_major() {
        let literal = LitStr::new("example:schema/position", Span::call_site());
        assert!(validate_schema_id(&literal).is_err());
    }
}
