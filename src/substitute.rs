use crate::ast::AugmentedGenerics;
use crate::ast::AugmentedImpl;
use crate::ast::AugmentedWhereClause;
use crate::ast::AugmentedWherePredicate;

use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use quote::ToTokens;
use syn::AngleBracketedGenericArguments;
use syn::AssocType;
use syn::GenericArgument;
use syn::GenericParam;
use syn::Ident;
use syn::Macro;
use syn::ParenthesizedGenericArguments;
use syn::PathArguments;
use syn::PredicateType;
use syn::ReturnType;
use syn::Type;
use syn::TypeArray;
use syn::TypeBareFn;
use syn::TypeGroup;
use syn::TypeMacro;
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

fn into_tree(tokens: TokenStream) -> Result<TokenTree, TokenStream> {
    let mut iter = tokens.clone().into_iter();
    let tree = iter.next().ok_or_else(TokenStream::new)?;
    if iter.next().is_none() {
        Ok(tree)
    } else {
        Err(tokens)
    }
}

fn into_braced_group(token_tree: TokenTree) -> Result<Group, TokenTree> {
    if let TokenTree::Group(group) = token_tree {
        if group.delimiter() == Delimiter::Brace {
            Ok(group)
        } else {
            Err(TokenTree::Group(group))
        }
    } else {
        Err(token_tree)
    }
}

fn is_ident(tokens: TokenStream, ident: &Ident) -> bool {
    let Ok(token_tree) = into_tree(tokens) else {
        return false;
    };

    match token_tree {
        TokenTree::Ident(inner_ident) => &inner_ident == ident,
        _ => false,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn substitute_in_macro(ty: &mut Type, mac: Macro, context: SubstituteContext) {
    let mut new_tokens = TokenStream::new();

    for token_tree in mac.tokens {
        match into_braced_group(token_tree) {
            Ok(group) => match into_tree(group.stream()) {
                Ok(token_tree) => match into_braced_group(token_tree) {
                    Ok(group) => {
                        let mut changed = false;
                        for (alias, ty) in context.aliases {
                            if is_ident(group.stream(), alias) {
                                new_tokens.extend(ty.to_token_stream());
                                changed = true;
                                break;
                            }
                        }
                        if !changed {
                            new_tokens.extend([TokenTree::Group(group)]);
                        }
                    }
                    Err(_) => new_tokens.extend([TokenTree::Group(group)]),
                },
                Err(_) => new_tokens.extend([TokenTree::Group(group)]),
            },
            Err(token_tree) => new_tokens.extend([token_tree]),
        }
    }

    let Macro {
        path,
        bang_token,
        delimiter,
        tokens: _,
    } = mac;
    *ty = Type::Macro(TypeMacro {
        mac: Macro {
            path,
            bang_token,
            delimiter,
            tokens: new_tokens,
        },
    });
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
                let mac = mac.mac.clone();
                substitute_in_macro(self, mac, context);
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
