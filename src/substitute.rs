use std::collections::HashMap;
use std::mem;

use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::Span;
use proc_macro2::TokenStream;

use proc_macro2::TokenTree;
use quote::ToTokens;

use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::visit_mut;
use syn::visit_mut::VisitMut;
use syn::Attribute;
use syn::Generics;
use syn::Ident;
use syn::Item;
use syn::ItemConst;
use syn::ItemEnum;
use syn::ItemExternCrate;
use syn::ItemFn;
use syn::ItemForeignMod;
use syn::ItemImpl;
use syn::ItemMacro;
use syn::ItemMod;
use syn::ItemStatic;
use syn::ItemStruct;
use syn::ItemTrait;
use syn::ItemTraitAlias;
use syn::ItemType;
use syn::ItemUnion;
use syn::ItemUse;
use syn::Macro;
use syn::Meta;
use syn::Stmt;
use syn::Token;
use syn::Type;
use syn::TypeParamBound;

pub struct Visitor {
    type_aliases: HashMap<Ident, Type>,
    trait_aliases: HashMap<Ident, Punctuated<TypeParamBound, Token![+]>>,
    in_macros: bool,
    substitution_stack: Vec<Ident>,
    error: Option<syn::Error>,
}

fn attributes_mut(item: &mut Item) -> &mut Vec<Attribute> {
    match item {
        Item::Const(ItemConst { attrs, .. })
        | Item::Enum(ItemEnum { attrs, .. })
        | Item::ExternCrate(ItemExternCrate { attrs, .. })
        | Item::Fn(ItemFn { attrs, .. })
        | Item::ForeignMod(ItemForeignMod { attrs, .. })
        | Item::Impl(ItemImpl { attrs, .. })
        | Item::Macro(ItemMacro { attrs, .. })
        | Item::Mod(ItemMod { attrs, .. })
        | Item::Static(ItemStatic { attrs, .. })
        | Item::Struct(ItemStruct { attrs, .. })
        | Item::Trait(ItemTrait { attrs, .. })
        | Item::TraitAlias(ItemTraitAlias { attrs, .. })
        | Item::Type(ItemType { attrs, .. })
        | Item::Union(ItemUnion { attrs, .. })
        | Item::Use(ItemUse { attrs, .. }) => attrs,
        _ => todo!(),
    }
}

impl Visitor {
    pub fn new(in_macros: bool, item: &mut Item) -> syn::Result<Visitor> {
        let attributes = attributes_mut(item);
        let mut to_remove = Vec::with_capacity(attributes.len());
        let mut type_aliases = HashMap::new();
        let mut trait_aliases = HashMap::new();

        for (index, attribute) in attributes.iter_mut().enumerate() {
            if let Meta::List(meta) = &mut attribute.meta {
                if meta.path.is_ident("alias") {
                    let new_aliases: Punctuated<InlineAlias, Token![,]> =
                        Punctuated::parse_terminated.parse2(mem::take(&mut meta.tokens))?;
                    for alias in new_aliases {
                        match alias {
                            InlineAlias::Type(alias) => {
                                type_aliases.insert(alias.ident, alias.ty);
                            }
                            InlineAlias::Trait(alias) => {
                                trait_aliases.insert(alias.ident, alias.bounds);
                            }
                        }
                    }
                    to_remove.push(index);
                }
            }
        }

        for &index in to_remove.iter().rev() {
            attributes.remove(index);
        }

        Ok(Visitor {
            type_aliases,
            trait_aliases,
            in_macros,
            substitution_stack: Vec::new(),
            error: None,
        })
    }

    fn detect_cycle(&mut self, alias: &Ident) -> syn::Result<()> {
        if self.substitution_stack.contains(alias) {
            Err(syn::Error::new(
                alias.span(),
                "cycle while substituting aliases",
            ))
        } else {
            Ok(())
        }
    }

    fn add_error(&mut self, error: syn::Error) {
        if let Some(exisiting) = &mut self.error {
            exisiting.combine(error);
        } else {
            self.error = Some(error);
        }
    }

    fn try_substitute_token_tree(&mut self, token_tree: &TokenTree) -> Option<TokenStream> {
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

        self.type_aliases
            .get(&ident)
            .map(ToTokens::to_token_stream)
            .or_else(|| {
                self.trait_aliases
                    .get(&ident)
                    .map(ToTokens::to_token_stream)
            })
    }

    fn substitute_token_tree(&mut self, token_tree: TokenTree) -> TokenStream {
        self.try_substitute_token_tree(&token_tree).map_or_else(
            || {
                if let TokenTree::Group(group) = token_tree {
                    let mut tokens = TokenStream::new();
                    for token_tree in group.stream() {
                        tokens.extend(self.substitute_token_tree(token_tree));
                    }
                    let mut new_group = Group::new(group.delimiter(), tokens);
                    new_group.set_span(group.span());
                    TokenTree::Group(new_group).into()
                } else {
                    token_tree.into()
                }
            },
            |stream| stream,
        )
    }
}

impl VisitMut for Visitor {
    fn visit_stmt_mut(&mut self, i: &mut Stmt) {
        if !matches!(i, Stmt::Item(_)) {
            visit_mut::visit_stmt_mut(self, i);
        }
    }

    fn visit_type_mut(&mut self, i: &mut Type) {
        if let Type::Path(path) = i {
            if let Some(qself) = &mut path.qself {
                self.visit_type_mut(&mut qself.ty);
                if qself.as_token.is_some() && qself.position == 1 {
                    if let Some(bounds) = self
                        .trait_aliases
                        .get(&path.path.segments.first().unwrap().ident)
                    {
                        if bounds.len() != 1 {
                            self.add_error(syn::Error::new_spanned(
                                bounds,
                                "cannot use `+` in fully qualified paths",
                            ));
                            return;
                        }

                        let first_segment = path.path.segments.first().unwrap();
                        if first_segment.arguments.is_empty() {
                            self.add_error(syn::Error::new_spanned(
                                &first_segment.arguments,
                                "generic aliases are not supported",
                            ));
                            return;
                        }

                        let mut bounds = bounds.clone();

                        let bound = bounds.first_mut().unwrap();
                        let TypeParamBound::Trait(bound) = bound else {
                            self.add_error(syn::Error::new_spanned(
                                bound,
                                "cannot use non-trait bounds in fully qualified paths",
                            ));
                            return;
                        };

                        bound
                            .path
                            .segments
                            .extend(path.path.segments.iter().skip(1).cloned());

                        path.path.segments = mem::take(&mut bound.path.segments);
                    }
                }
            } else if path.path.segments.len() == 1 {
                let alias = &path.path.segments.last().unwrap().ident;
                if let Some(ty) = self.type_aliases.get(alias) {
                    let mut ty = ty.clone();
                    if let Err(error) = self.detect_cycle(alias) {
                        self.add_error(error);
                        return;
                    }
                    self.substitution_stack.push(alias.clone());
                    self.visit_type_mut(&mut ty);
                    self.substitution_stack.pop();
                    *i = ty;
                    return;
                }
            }

            self.visit_path_mut(&mut path.path);
        } else {
            visit_mut::visit_type_mut(self, i);
        }
    }

    fn visit_macro_mut(&mut self, i: &mut Macro) {
        if self.in_macros {
            let mut new_tokens = TokenStream::new();

            for token_tree in mem::take(&mut i.tokens) {
                new_tokens.extend(self.substitute_token_tree(token_tree));
            }

            i.tokens = new_tokens;
        }
    }

    fn visit_predicate_type_mut(&mut self, i: &mut syn::PredicateType) {
        let iter = i.bounds.iter().enumerate().filter_map(|(i, bound)| {
            if let TypeParamBound::Trait(bound) = bound {
                if bound.path.segments.len() != 1 {
                    return None;
                }
                Some((
                    i,
                    self.trait_aliases
                        .get(&bound.path.segments.first().unwrap().ident)?,
                ))
            } else {
                None
            }
        });

        let mut new_bounds = Punctuated::new();
        let mut to_remove = Vec::new();

        for (index, bounds) in iter {
            to_remove.push(index);
            for pair in bounds.pairs() {
                let (bound, plus) = pair.into_tuple();
                new_bounds.push_value(bound.clone());
                new_bounds.push_punct(
                    plus.copied()
                        .unwrap_or_else(|| Token![+](Span::call_site())),
                );
            }
        }

        for (i, pair) in i.bounds.pairs().enumerate() {
            if !to_remove.contains(&i) {
                let (bound, plus) = pair.into_tuple();
                new_bounds.push_value(bound.clone());
                new_bounds.push_punct(
                    plus.copied()
                        .unwrap_or_else(|| Token![+](Span::call_site())),
                );
            }
        }

        i.bounds = new_bounds;

        visit_mut::visit_predicate_type_mut(self, i);
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

enum InlineAlias {
    Type(InlineTypeAlias),
    Trait(InlineTraitAlias),
}

impl Parse for InlineAlias {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![type]) {
            Ok(InlineAlias::Type(input.parse()?))
        } else if lookahead.peek(Token![trait]) {
            Ok(InlineAlias::Trait(input.parse()?))
        } else {
            Err(lookahead.error())
        }
    }
}

struct InlineTypeAlias {
    _type_token: Token![type],
    ident: Ident,
    _generics: Generics,
    _eq_token: Token![=],
    ty: Type,
}

impl Parse for InlineTypeAlias {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(InlineTypeAlias {
            _type_token: input.parse()?,
            ident: input.parse()?,
            _generics: {
                let mut generics: Generics = input.parse()?;
                generics.where_clause = input.parse()?;
                generics
            },
            _eq_token: input.parse()?,
            ty: input.parse()?,
        })
    }
}

struct InlineTraitAlias {
    _trait_token: Token![trait],
    ident: Ident,
    _generics: Generics,
    _eq_token: Token![=],
    bounds: Punctuated<TypeParamBound, Token![+]>,
}

impl Parse for InlineTraitAlias {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let trait_token = input.parse()?;
        let ident = input.parse()?;
        let mut generics: Generics = input.parse()?;
        let eq_token = input.parse()?;

        let mut bounds = Punctuated::new();
        loop {
            if input.peek(Token![where]) || input.peek(Token![,]) || input.is_empty() {
                break;
            }
            bounds.push_value(input.parse()?);
            if input.peek(Token![where]) || input.peek(Token![,]) || input.is_empty() {
                break;
            }
            bounds.push_punct(input.parse()?);
        }

        generics.where_clause = input.parse()?;

        Ok(InlineTraitAlias {
            _trait_token: trait_token,
            ident,
            _generics: generics,
            _eq_token: eq_token,
            bounds,
        })
    }
}
