// Copyright (c) 2026 Robert Grosse. All rights reserved.
use std::rc::Rc;

use crate::ast::ParserContext;
use crate::ast::StringId;
use crate::spans::Span;
use crate::spans::Spanned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variance {
    Covariant,
    Contravariant,
    Invariant,
}
impl Variance {
    pub fn flip(&self) -> Self {
        match self {
            Variance::Covariant => Variance::Contravariant,
            Variance::Contravariant => Variance::Covariant,
            Variance::Invariant => Variance::Invariant,
        }
    }

    pub fn to_display(&self) -> &'static str {
        match self {
            Variance::Covariant => "covariant",
            Variance::Contravariant => "contravariant",
            Variance::Invariant => "invariant",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SVKind {
    pub variance: Spanned<Variance>,
    pub kind: SKind,
}
impl SVKind {
    pub fn new(variance: Spanned<Variance>, kind: Option<SKind>) -> Self {
        let kind = kind.unwrap_or((Kind::Star, variance.1));
        Self { variance, kind }
    }

    // Called when generating kinds for spine constructors, where there are
    // no explicit variance annotations. In this case, the best we can do is use the
    // span from the expr instead (this span is also used for the kind for the same reason).
    pub fn new_implicit(var: Variance, kind: Kind, span: Span) -> Self {
        Self::new((var, span), Some((kind, span)))
    }
}

pub type TyConKind = Rc<[SVKind]>;
#[derive(Debug, Clone)]
pub enum Kind {
    Star,
    // Invariant: param list is nonempty
    Arrow(Rc<[SVKind]>),
}
impl Kind {
    pub fn new(params: Vec<SVKind>) -> Self {
        if params.is_empty() {
            Kind::Star
        } else {
            Kind::Arrow(params.into_boxed_slice().into())
        }
    }

    pub fn params(&self) -> &[SVKind] {
        match self {
            Kind::Star => &[],
            Kind::Arrow(kinds) => kinds,
        }
    }
}
pub type SKind = Spanned<Kind>;

// foo.bar.etc. Null coercion indicated by empty path.
#[derive(Debug)]
pub enum PathType {
    Top,
    Bot,
    Hole,
    Placeholder(StringId),
    Single(StringId),
    ModMember(Spanned<StringId>, Spanned<StringId>),
    VarPair(Spanned<StringId>, Spanned<StringId>),
}
impl PathType {
    pub fn new_single(ctx: &mut ParserContext<'_, '_>, name: StringId) -> Self {
        match ctx.strings.resolve(&name) {
            "any" => PathType::Top,
            "never" => PathType::Bot,
            _ => PathType::Single(name),
        }
    }
}

#[derive(Debug)]
pub struct PathTypeWithArgs {
    pub path: Spanned<PathType>,
    pub annot: Option<SKind>,
    pub args: Spanned<Vec<InvTypeDecl>>,
}
impl PathTypeWithArgs {
    pub fn new(path: Spanned<PathType>, annot: Option<SKind>, args: Spanned<Vec<InvTypeDecl>>) -> Self {
        Self { path, annot, args }
    }

    pub fn into_type(self) -> TypeExpr {
        TypeExpr::Named(self)
    }
}

#[derive(Debug)]
pub struct TypeParam {
    pub name: Spanned<StringId>,
    pub kind: SKind,
    pub alias: Spanned<StringId>,
    pub pair_name: Option<StringId>,
}
impl TypeParam {
    pub fn new(name: Spanned<StringId>, kind: Option<SKind>, alias: Option<Spanned<StringId>>) -> Self {
        let kind = kind.unwrap_or((Kind::Star, name.1));
        let alias = alias.unwrap_or(name);
        Self {
            name,
            kind,
            alias,
            pair_name: None,
        }
    }

    pub fn set_pair(&mut self, strings: &mut lasso::Rodeo) {
        self.pair_name = Some(append_dollar(strings, self.name.0));
    }
}

/// Represents an invariant type declaration with read and optional write types.
#[derive(Debug)]
pub enum InvTypeDecl {
    Single(STypeExpr),
    Pair(STypeExpr, STypeExpr),
}
impl InvTypeDecl {
    pub fn new(r: STypeExpr) -> Self {
        Self::Single(r)
    }
    pub fn pair(r: STypeExpr, w: STypeExpr) -> Self {
        Self::Pair(r, w)
    }
}

#[derive(Debug)]
pub enum RecordTypeMember {
    Field(bool, InvTypeDecl),
    Alias(STypeExpr),
    Abstract(TypeParam),
}

pub type KeyPairType = (Spanned<StringId>, RecordTypeMember);

#[derive(Debug)]
pub enum TypeExpr {
    Case(Vec<(Spanned<StringId>, Option<Box<STypeExpr>>)>),
    ConstructorOf(Box<STypeExpr>),
    Func(Vec<TypeParam>, Box<STypeExpr>, Box<STypeExpr>, bool),
    Named(PathTypeWithArgs),
    Record(Vec<KeyPairType>),
    RecursiveDef(StringId, Box<STypeExpr>),
}
pub type STypeExpr = Spanned<TypeExpr>;

fn append_dollar(strings: &mut lasso::Rodeo, name: StringId) -> StringId {
    strings.get_or_intern(format!("{}$", strings.resolve(&name)))
}

#[derive(Debug)]
pub struct NewtypeDefParam {
    pub variance: Spanned<Variance>,
    pub tp: TypeParam,
}
impl NewtypeDefParam {
    pub fn new(strings: &mut lasso::Rodeo, variance: Spanned<Variance>, mut tp: TypeParam) -> Self {
        if let Variance::Invariant = variance.0 {
            tp.set_pair(strings);
        }
        Self { variance, tp }
    }
}

#[derive(Debug)]
pub enum NewtypeRHS {
    Type(STypeExpr),
    Enum(Vec<(Spanned<StringId>, Option<STypeExpr>)>),
}

#[derive(Debug)]
pub struct NewtypeDef {
    pub name_span: Span,
    pub name: StringId,
    pub name2: StringId,
    pub params: Vec<NewtypeDefParam>,
    pub rhs: NewtypeRHS,
}
impl NewtypeDef {
    pub fn new(strings: &mut lasso::Rodeo, name: Spanned<StringId>, params: Vec<NewtypeDefParam>, rhs: NewtypeRHS) -> Self {
        let (name, name_span) = name;
        Self {
            name_span,
            name,
            name2: append_dollar(strings, name),
            params,
            rhs,
        }
    }

    pub fn kind(&self) -> SKind {
        let param_kinds = self
            .params
            .iter()
            .map(|param| SVKind::new(param.variance, Some(param.tp.kind.clone())))
            .collect();
        (Kind::new(param_kinds), self.name_span)
    }
}
