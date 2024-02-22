use crate::substitute::Substitute;
use crate::substitute::SubstituteContext;

use std::iter;

use proc_macro2::Delimiter;
use proc_macro2::TokenStream;

use syn::braced;
use syn::parenthesized;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::token;
use syn::Attribute;
use syn::GenericParam;
use syn::Ident;
use syn::ImplItem;
use syn::Lit;
use syn::Path;
use syn::Token;
use syn::Type;
use syn::TypePath;
use syn::WhereClause;
use syn::WherePredicate;

mod kw {
    syn::custom_keyword!(alias);
}

#[derive(Debug)]
pub struct Options {
    pub in_macros: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for Options {
    fn default() -> Self {
        Options { in_macros: false }
    }
}

impl Parse for Options {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let settings: Punctuated<Setting, Token![,]> = Punctuated::parse_terminated(input)?;
        let mut options = Options::default();
        for setting in settings {
            match setting.name.to_string().as_str() {
                "macros" => {
                    if let Lit::Bool(b) = setting.value {
                        options.in_macros = b.value;
                    } else {
                        return Err(syn::Error::new_spanned(setting.value, "expected a bool"));
                    }
                }
                _ => return Err(syn::Error::new_spanned(setting.name, "unexpected option")),
            }
        }
        Ok(options)
    }
}

#[derive(Debug)]
pub struct Setting {
    name: Ident,
    _eq_token: Token![=],
    value: Lit,
}

impl Parse for Setting {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Setting {
            name: input.parse()?,
            _eq_token: input.parse()?,
            value: input.parse()?,
        })
    }
}

#[derive(Debug)]
pub struct AugmentedImpl {
    pub attrs: Vec<Attribute>,
    pub defaultness: Option<Token![default]>,
    pub unsafety: Option<Token![unsafe]>,
    pub impl_token: Token![impl],
    pub generics: AugmentedGenerics,
    pub trait_: Option<(Option<Token![!]>, Path, Token![for])>,
    pub self_ty: Box<Type>,
    pub brace_token: token::Brace,
    pub items: Vec<ImplItem>,
}

impl AugmentedImpl {
    pub fn substitute(mut self, in_macros: bool) -> syn::ItemImpl {
        let Some(aliases) = self
            .generics
            .where_clause
            .as_ref()
            .map(AugmentedWhereClause::aliases)
        else {
            return self.into_item_impl_lossy();
        };

        let mut substituted_aliases = Vec::with_capacity(aliases.len());
        for (index, (alias, ty)) in aliases.iter().enumerate() {
            let mut ty = ty.clone();
            ty.substitute(SubstituteContext::new(&aliases[0..index], in_macros));
            substituted_aliases.push((alias.clone(), ty));
        }

        Substitute::substitute(
            &mut self,
            SubstituteContext::new(&substituted_aliases, in_macros),
        );

        self.into_item_impl_lossy()
    }

    fn into_item_impl_lossy(self) -> syn::ItemImpl {
        syn::ItemImpl {
            attrs: self.attrs,
            defaultness: self.defaultness,
            unsafety: self.unsafety,
            impl_token: self.impl_token,
            generics: self.generics.into_generics_lossy(),
            trait_: self.trait_,
            self_ty: self.self_ty,
            brace_token: self.brace_token,
            items: self.items,
        }
    }
}

// Lifted almost verbatim from `syn::verbatim::between`.
fn verbatim_between<'a>(begin: ParseStream<'a>, end: ParseStream<'a>) -> TokenStream {
    let end = end.cursor();
    let mut cursor = begin.cursor();

    let mut tokens = TokenStream::new();
    while cursor != end {
        let (tt, next) = cursor.token_tree().unwrap();

        if end < next {
            // A syntax node can cross the boundary of a None-delimited group
            // due to such groups being transparent to the parser in most cases.
            // Any time this occurs the group is known to be semantically
            // irrelevant. https://github.com/dtolnay/syn/issues/1235
            if let Some((inside, _span, after)) = cursor.group(Delimiter::None) {
                assert!(next == after);
                cursor = inside;
                continue;
            }
            panic!("verbatim end must not be inside a delimited group");
        }

        tokens.extend(iter::once(tt));
        cursor = next;
    }
    tokens
}

impl Parse for AugmentedImpl {
    // Lifted almost verbatim from `syn::ItemImpl::parse`.
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attrs = input.call(Attribute::parse_outer)?;
        let defaultness: Option<Token![default]> = input.parse()?;
        let unsafety: Option<Token![unsafe]> = input.parse()?;
        let impl_token: Token![impl] = input.parse()?;

        let has_generics = input.peek(Token![<])
            && (input.peek2(Token![>])
                || input.peek2(Token![#])
                || (input.peek2(Ident) || input.peek2(syn::Lifetime))
                    && (input.peek3(Token![:])
                        || input.peek3(Token![,])
                        || input.peek3(Token![>])
                        || input.peek3(Token![=]))
                || input.peek2(Token![const]));
        let mut generics: AugmentedGenerics = if has_generics {
            input.parse()?
        } else {
            AugmentedGenerics::default()
        };

        let begin = input.fork();
        let polarity = if input.peek(Token![!]) && !input.peek2(token::Brace) {
            Some(input.parse::<Token![!]>()?)
        } else {
            None
        };

        let mut first_ty: Type = input.parse()?;
        let self_ty: Type;
        let trait_;

        let is_impl_for = input.peek(Token![for]);
        if is_impl_for {
            let for_token: Token![for] = input.parse()?;
            let mut first_ty_ref = &first_ty;
            while let Type::Group(ty) = first_ty_ref {
                first_ty_ref = &ty.elem;
            }
            if let Type::Path(TypePath { qself: None, .. }) = first_ty_ref {
                while let Type::Group(ty) = first_ty {
                    first_ty = *ty.elem;
                }
                if let Type::Path(TypePath { qself: None, path }) = first_ty {
                    trait_ = Some((polarity, path, for_token));
                } else {
                    unreachable!();
                }
            } else {
                return Err(syn::Error::new_spanned(first_ty_ref, "expected trait path"));
            }
            self_ty = input.parse()?;
        } else {
            trait_ = None;
            self_ty = if polarity.is_none() {
                first_ty
            } else {
                Type::Verbatim(verbatim_between(&begin, input))
            };
        }

        if input.peek(Token![where]) {
            generics.where_clause = Some(input.parse()?);
        }

        let content;
        let brace_token = braced!(content in input);
        attrs.extend(Attribute::parse_inner(&content)?);

        let mut items = Vec::new();
        while !content.is_empty() {
            items.push(content.parse()?);
        }

        Ok(AugmentedImpl {
            attrs,
            defaultness,
            unsafety,
            impl_token,
            generics,
            trait_,
            self_ty: Box::new(self_ty),
            brace_token,
            items,
        })
    }
}

#[derive(Debug, Default)]
pub struct AugmentedGenerics {
    pub lt_token: Option<Token![<]>,
    pub params: Punctuated<GenericParam, Token![,]>,
    pub gt_token: Option<Token![>]>,
    pub where_clause: Option<AugmentedWhereClause>,
}

impl AugmentedGenerics {
    fn into_generics_lossy(self) -> syn::Generics {
        syn::Generics {
            lt_token: self.lt_token,
            params: self.params,
            gt_token: self.gt_token,
            where_clause: self
                .where_clause
                .map(AugmentedWhereClause::into_where_clause_lossy),
        }
    }
}

impl Parse for AugmentedGenerics {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let syn::Generics {
            lt_token,
            params,
            gt_token,
            where_clause,
        } = input.parse()?;

        Ok(AugmentedGenerics {
            lt_token,
            params,
            gt_token,
            where_clause: where_clause.map(Into::into),
        })
    }
}

#[derive(Debug)]
pub struct AugmentedWhereClause {
    pub where_token: Token![where],
    pub predicates: Punctuated<AugmentedWherePredicate, Token![,]>,
}

impl AugmentedWhereClause {
    fn into_where_clause_lossy(self) -> WhereClause {
        let mut predicates = Punctuated::new();
        for pair in self.predicates.into_pairs() {
            let (value, punct) = pair.into_tuple();
            if let Some(value) = value.into() {
                predicates.push_value(value);
                if let Some(punct) = punct {
                    predicates.push_punct(punct);
                }
            }
        }

        WhereClause {
            where_token: self.where_token,
            predicates,
        }
    }

    fn aliases(&self) -> Vec<(Ident, Type)> {
        self.predicates
            .iter()
            .filter_map(|predicate| {
                if let AugmentedWherePredicate::TypeAlias(alias) = predicate {
                    Some((alias.ident.clone(), alias.ty.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl From<WhereClause> for AugmentedWhereClause {
    fn from(value: WhereClause) -> AugmentedWhereClause {
        let mut predicates = Punctuated::new();
        for pair in value.predicates.into_pairs() {
            let (value, punct) = pair.into_tuple();
            predicates.push_value(value.into());
            if let Some(punct) = punct {
                predicates.push_punct(punct);
            }
        }

        AugmentedWhereClause {
            where_token: value.where_token,
            predicates,
        }
    }
}

impl Parse for AugmentedWhereClause {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(AugmentedWhereClause {
            where_token: input.parse()?,
            predicates: {
                let mut predicates = Punctuated::new();
                loop {
                    if input.is_empty()
                        || input.peek(token::Brace)
                        || input.peek(Token![,])
                        || input.peek(Token![;])
                        || input.peek(Token![:]) && !input.peek(Token![::])
                        || input.peek(Token![=])
                    {
                        break;
                    }
                    let value = input.parse()?;
                    predicates.push_value(value);
                    if !input.peek(Token![,]) {
                        break;
                    }
                    let punct = input.parse()?;
                    predicates.push_punct(punct);
                }
                predicates
            },
        })
    }
}

#[derive(Debug)]
pub enum AugmentedWherePredicate {
    WherePredicate(WherePredicate),
    TypeAlias(InlineTypeAlias),
}

impl From<WherePredicate> for AugmentedWherePredicate {
    fn from(value: WherePredicate) -> AugmentedWherePredicate {
        AugmentedWherePredicate::WherePredicate(value)
    }
}

impl From<AugmentedWherePredicate> for Option<WherePredicate> {
    fn from(value: AugmentedWherePredicate) -> Self {
        if let AugmentedWherePredicate::WherePredicate(predicate) = value {
            Some(predicate)
        } else {
            None
        }
    }
}

impl Parse for AugmentedWherePredicate {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(kw::alias) {
            Ok(AugmentedWherePredicate::TypeAlias(input.parse()?))
        } else {
            Ok(AugmentedWherePredicate::WherePredicate(input.parse()?))
        }
    }
}

#[derive(Debug)]
pub struct InlineTypeAlias {
    pub _alias_token: kw::alias,
    pub _bang: Token![!],
    pub paren: token::Paren,
    pub ident: Ident,
    pub _eq_token: Token![=],
    pub ty: Type,
    pub _colon: Token![:],
}

impl Parse for InlineTypeAlias {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        Ok(InlineTypeAlias {
            _alias_token: input.parse()?,
            _bang: input.parse()?,
            paren: parenthesized!(content in input),
            ident: content.parse()?,
            _eq_token: content.parse()?,
            ty: content.parse()?,
            _colon: input.parse()?,
        })
    }
}
