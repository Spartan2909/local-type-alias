use crate::ast::AugmentedGenerics;
use crate::ast::AugmentedImpl;
use crate::ast::AugmentedWhereClause;
use crate::ast::AugmentedWherePredicate;

use syn::AngleBracketedGenericArguments;
use syn::AssocType;
use syn::GenericArgument;
use syn::GenericParam;
use syn::Ident;
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

pub trait Substitute {
    fn substitute(&mut self, aliases: &[(Ident, Type)]);
}

impl Substitute for AugmentedImpl {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        self.generics.substitute(aliases);
    }
}

impl Substitute for AugmentedGenerics {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        for param in &mut self.params {
            if let GenericParam::Type(param) = param {
                if let Some(default) = &mut param.default {
                    default.substitute(aliases);
                }
            }
        }

        if let Some(where_clause) = &mut self.where_clause {
            where_clause.substitute(aliases);
        }
    }
}

impl Substitute for AugmentedWhereClause {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        for predicate in &mut self.predicates {
            predicate.substitute(aliases);
        }
    }
}

impl Substitute for AugmentedWherePredicate {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        if let AugmentedWherePredicate::WherePredicate(WherePredicate::Type(pred)) = self {
            pred.substitute(aliases);
        }
    }
}

impl Substitute for PredicateType {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        self.bounded_ty.substitute(aliases);

        for bound in &mut self.bounds {
            if let TypeParamBound::Trait(bound) = bound {
                for segment in &mut bound.path.segments {
                    segment.arguments.substitute(aliases);
                }
            }
        }
    }
}

impl Substitute for Type {
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        match self {
            Type::Array(TypeArray { elem, .. })
            | Type::Group(TypeGroup { elem, .. })
            | Type::Paren(TypeParen { elem, .. })
            | Type::Ptr(TypePtr { elem, .. })
            | Type::Reference(TypeReference { elem, .. })
            | Type::Slice(TypeSlice { elem, .. }) => elem.substitute(aliases),
            Type::BareFn(TypeBareFn { inputs, output, .. }) => {
                for arg in inputs {
                    arg.ty.substitute(aliases);
                }

                if let ReturnType::Type(_, ty) = output {
                    ty.substitute(aliases);
                }
            }
            Type::Path(path) => {
                if let Some(qself) = &mut path.qself {
                    qself.ty.substitute(aliases);
                } else if path.path.segments.len() == 1 {
                    for (alias, ty) in aliases {
                        if path.path.is_ident(alias) {
                            let mut ty = ty.clone();
                            ty.substitute(aliases);
                            *self = ty;
                            return;
                        }
                    }
                }

                for segment in &mut path.path.segments {
                    segment.arguments.substitute(aliases);
                }
            }
            Type::Tuple(tuple) => tuple.elems.iter_mut().for_each(|ty| ty.substitute(aliases)),
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
    fn substitute(&mut self, aliases: &[(Ident, Type)]) {
        match self {
            PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => {
                for arg in args {
                    match arg {
                        GenericArgument::AssocType(AssocType { ty, .. })
                        | GenericArgument::Type(ty) => ty.substitute(aliases),
                        _ => {}
                    }
                }
            }
            PathArguments::Parenthesized(ParenthesizedGenericArguments {
                inputs, output, ..
            }) => {
                for input in inputs {
                    input.substitute(aliases);
                }

                if let ReturnType::Type(_, ty) = output {
                    ty.substitute(aliases);
                }
            }
            PathArguments::None => {}
        }
    }
}
