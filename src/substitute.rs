use crate::ast::AugmentedGenerics;
use crate::ast::AugmentedImpl;
use crate::ast::AugmentedWhereClause;
use crate::ast::AugmentedWherePredicate;

use std::mem;

use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use quote::ToTokens;
use syn::AngleBracketedGenericArguments;
use syn::AssocType;
use syn::FnArg;
use syn::GenericArgument;
use syn::GenericParam;
use syn::Ident;
use syn::ImplItem;
use syn::Macro;
use syn::ParenthesizedGenericArguments;
use syn::PathArguments;
use syn::PredicateType;
use syn::ReturnType;
use syn::Type;
use syn::TypeArray;
use syn::TypeBareFn;
use syn::TypeGroup;
use syn::TypeParamBound;
use syn::TypeParen;
use syn::TypePtr;
use syn::TypeReference;
use syn::TypeSlice;
use syn::WherePredicate;

#[derive(Debug, Clone, Copy)]
pub struct SubstituteContext<'a> {
    pub aliases: &'a [(Ident, Type)],
    pub in_macros: bool,
}

impl<'a> SubstituteContext<'a> {
    pub const fn new(aliases: &'a [(Ident, Type)], in_macros: bool) -> SubstituteContext<'a> {
        SubstituteContext { aliases, in_macros }
    }
}

pub trait Substitute {
    fn substitute(&mut self, context: SubstituteContext);
}

impl Substitute for AugmentedImpl {
    fn substitute(&mut self, context: SubstituteContext) {
        self.generics.substitute(context);

        for item in &mut self.items {
            item.substitute(context);
        }
    }
}

impl Substitute for ImplItem {
    fn substitute(&mut self, context: SubstituteContext) {
        match self {
            ImplItem::Const(item) => item.ty.substitute(context),
            ImplItem::Fn(item) => {
                for input in &mut item.sig.inputs {
                    if let FnArg::Typed(input) = input {
                        input.ty.substitute(context);
                    }
                }

                if let ReturnType::Type(_, ty) = &mut item.sig.output {
                    ty.substitute(context);
                }
            }
            ImplItem::Type(item) => item.ty.substitute(context),
            ImplItem::Macro(item) => {
                if context.in_macros {
                    item.mac.substitute(context);
                }
            }
            ImplItem::Verbatim(_) => {}
            _ => panic!("unknown item {self:#?}"),
        }
    }
}

impl Substitute for AugmentedGenerics {
    fn substitute(&mut self, context: SubstituteContext) {
        for param in &mut self.params {
            if let GenericParam::Type(param) = param {
                if let Some(default) = &mut param.default {
                    default.substitute(context);
                }
            }
        }

        if let Some(where_clause) = &mut self.where_clause {
            where_clause.substitute(context);
        }
    }
}

impl Substitute for AugmentedWhereClause {
    fn substitute(&mut self, context: SubstituteContext) {
        for predicate in &mut self.predicates {
            predicate.substitute(context);
        }
    }
}

impl Substitute for AugmentedWherePredicate {
    fn substitute(&mut self, context: SubstituteContext) {
        if let AugmentedWherePredicate::WherePredicate(WherePredicate::Type(pred)) = self {
            pred.substitute(context);
        }
    }
}

impl Substitute for PredicateType {
    fn substitute(&mut self, context: SubstituteContext) {
        self.bounded_ty.substitute(context);

        for bound in &mut self.bounds {
            if let TypeParamBound::Trait(bound) = bound {
                for segment in &mut bound.path.segments {
                    segment.arguments.substitute(context);
                }
            }
        }
    }
}

fn into_token_tree(tokens: TokenStream) -> Result<TokenTree, TokenStream> {
    let mut iter = tokens.clone().into_iter();
    let token_tree = iter.next().ok_or_else(TokenStream::new)?;
    if iter.next().is_none() {
        Ok(token_tree)
    } else {
        Err(tokens)
    }
}

fn try_substitute_token_tree(
    token_tree: &TokenTree,
    context: SubstituteContext,
) -> Option<TokenStream> {
    let TokenTree::Group(group) = token_tree else {
        return None;
    };

    if group.delimiter() != Delimiter::Brace {
        return None;
    }

    let Ok(TokenTree::Group(group)) = into_token_tree(group.stream()) else {
        return None;
    };

    if group.delimiter() != Delimiter::Brace {
        return None;
    }

    let Ok(TokenTree::Ident(ident)) = into_token_tree(group.stream()) else {
        return None;
    };

    for (alias, ty) in context.aliases {
        if alias == &ident {
            return Some(ty.to_token_stream());
        }
    }

    None
}

fn substitute_token_tree(token_tree: TokenTree, context: SubstituteContext) -> TokenStream {
    if let Some(stream) = try_substitute_token_tree(&token_tree, context) {
        stream
    } else if let TokenTree::Group(group) = token_tree {
        let mut tokens = TokenStream::new();
        for token_tree in group.stream() {
            tokens.extend(substitute_token_tree(token_tree, context));
        }
        let mut new_group = Group::new(group.delimiter(), tokens);
        new_group.set_span(group.span());
        TokenTree::Group(new_group).into()
    } else {
        token_tree.into()
    }
}

impl Substitute for Macro {
    fn substitute(&mut self, context: SubstituteContext) {
        let mut new_tokens = TokenStream::new();

        for token_tree in mem::take(&mut self.tokens) {
            new_tokens.extend(substitute_token_tree(token_tree, context));
        }

        self.tokens = new_tokens;
    }
}

impl Substitute for Type {
    fn substitute(&mut self, context: SubstituteContext) {
        match self {
            Type::Array(TypeArray { elem, .. })
            | Type::Group(TypeGroup { elem, .. })
            | Type::Paren(TypeParen { elem, .. })
            | Type::Ptr(TypePtr { elem, .. })
            | Type::Reference(TypeReference { elem, .. })
            | Type::Slice(TypeSlice { elem, .. }) => elem.substitute(context),
            Type::BareFn(TypeBareFn { inputs, output, .. }) => {
                for arg in inputs {
                    arg.ty.substitute(context);
                }

                if let ReturnType::Type(_, ty) = output {
                    ty.substitute(context);
                }
            }
            Type::Macro(mac) if context.in_macros => {
                mac.mac.substitute(context);
            }
            Type::Path(path) => {
                if let Some(qself) = &mut path.qself {
                    qself.ty.substitute(context);
                } else if path.path.segments.len() == 1 {
                    for (alias, ty) in context.aliases {
                        if path.path.is_ident(alias) {
                            let mut ty = ty.clone();
                            ty.substitute(context);
                            *self = ty;
                            return;
                        }
                    }
                }

                for segment in &mut path.path.segments {
                    segment.arguments.substitute(context);
                }
            }
            Type::Tuple(tuple) => tuple.elems.iter_mut().for_each(|ty| ty.substitute(context)),
            Type::ImplTrait(_)
            | Type::Infer(_)
            | Type::Macro(_)
            | Type::Never(_)
            | Type::TraitObject(_)
            | Type::Verbatim(_) => {}
            _ => panic!("unknown type format {self:#?}"),
        }
    }
}

impl Substitute for PathArguments {
    fn substitute(&mut self, context: SubstituteContext) {
        match self {
            PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => {
                for arg in args {
                    match arg {
                        GenericArgument::AssocType(AssocType { ty, .. })
                        | GenericArgument::Type(ty) => ty.substitute(context),
                        _ => {}
                    }
                }
            }
            PathArguments::Parenthesized(ParenthesizedGenericArguments {
                inputs, output, ..
            }) => {
                for input in inputs {
                    input.substitute(context);
                }

                if let ReturnType::Type(_, ty) = output {
                    ty.substitute(context);
                }
            }
            PathArguments::None => {}
        }
    }
}
