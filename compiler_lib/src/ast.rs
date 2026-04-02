// Copyright (c) 2026 Robert Grosse. All rights reserved.
pub mod expr;
pub mod types;

pub use expr::Expr;
pub use expr::SExpr;
pub use types::*;

use crate::ast::expr::*;
use crate::spans::*;
use crate::tuples::*;

pub type StringId = lasso::Spur;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImportId(pub StringId);

pub struct ParserContext<'a, 'input> {
    pub span_maker: SpanMaker<'input>,
    pub strings: &'a mut lasso::Rodeo,
    pub imports: Vec<Spanned<ImportId>>,
}
impl ParserContext<'_, '_> {
    pub fn import_id(&mut self, components: Spanned<Vec<&str>>) -> Spanned<ImportId> {
        let s = components.0.join(".");
        let id = self.strings.get_or_intern(s);
        let id = ImportId(id);
        let res = (id, components.1);
        self.imports.push(res);
        res
    }
}

type LetDefinition = (LetPattern, Box<SExpr>);
pub type LetRecDefinition = (Spanned<StringId>, Option<STypeExpr>, SExpr);

type OptHint = Option<Spanned<PathTypeWithArgs>>;

#[derive(Debug)]
pub enum RecordPatternMember {
    Field(Box<LetPattern>),
}
impl RecordPatternMember {
    pub fn field(pat: LetPattern) -> Self {
        Self::Field(Box::new(pat))
    }
}

pub type RecordPatternFields = Vec<(Spanned<StringId>, RecordPatternMember)>;
#[derive(Debug)]
pub struct VarPattern(pub (Option<StringId>, Span), pub Option<STypeExpr>);

#[derive(Debug)]
pub enum LetPattern {
    Case(OptHint, Spanned<StringId>, Option<Box<LetPattern>>),
    Record(OptHint, Spanned<RecordPatternFields>, Option<VarPattern>),
    Var(VarPattern),
}

fn enumerate_tuple_fields<T, R>(
    vals: impl IntoIterator<Item = (T, Span)>,
    strings: &mut lasso::Rodeo,
    mut make_field: impl FnMut(Spanned<StringId>, T) -> R,
) -> Vec<R> {
    vals.into_iter()
        .enumerate()
        .map(|(i, (val, span))| {
            let name = tuple_name(strings, i as u32);
            make_field((name, span), val)
        })
        .collect()
}

pub fn make_tuple_pattern_fields(
    first: Spanned<LetPattern>,
    mut vals: Vec<Spanned<LetPattern>>,
    strings: &mut lasso::Rodeo,
) -> RecordPatternFields {
    vals.insert(0, first);
    enumerate_tuple_fields(vals, strings, |name, val| (name, RecordPatternMember::field(val)))
}

pub fn make_tuple_type(mut vals: Vec<STypeExpr>, strings: &mut lasso::Rodeo) -> TypeExpr {
    if vals.len() <= 1 {
        return vals.pop().unwrap().0;
    }

    let fields = enumerate_tuple_fields(vals, strings, |name, val| {
        let stype = (val, name.1);
        (name, RecordTypeMember::Field(false, InvTypeDecl::new(stype)))
    });
    TypeExpr::Record(fields)
}

#[derive(Debug)]
pub struct NameAlias {
    pub name: Spanned<StringId>,
    pub alias: Spanned<StringId>,
}
impl NameAlias {
    pub fn new(name: Spanned<StringId>, alias: Option<Spanned<StringId>>) -> Self {
        let alias = alias.unwrap_or(name);
        Self { name, alias }
    }
}

#[derive(Debug)]
pub enum ImportStyle {
    Full(Spanned<StringId>),
    Fields(Vec<NameAlias>),
}

pub fn full_import(strings: &mut lasso::Rodeo, lhs: Spanned<ImportId>, alias: Option<Spanned<StringId>>) -> Statement {
    // If no alias is provided, we use the last component of the module path as the name.
    let alias = alias.unwrap_or_else(|| {
        let s = strings.resolve(&lhs.0.0);
        let s = s.rsplit('.').next().unwrap_or(s).to_owned();
        let id = strings.get_or_intern(s);
        (id, lhs.1)
    });

    Statement::Import(lhs, ImportStyle::Full(alias))
}
pub fn field_import(lhs: Spanned<ImportId>, fields: Vec<NameAlias>) -> Statement {
    Statement::Import(lhs, ImportStyle::Fields(fields))
}

#[derive(Debug)]
pub struct ImplicitCoercions(pub Vec<(Spanned<StringId>, Option<Spanned<StringId>>, Option<Spanned<StringId>>)>);

#[derive(Debug)]
pub enum Statement {
    Empty,
    Expr(SExpr),
    Import(Spanned<ImportId>, ImportStyle),
    LetDef(LetDefinition),
    LetRecDef(Vec<LetRecDefinition>),
    ModuleDef(Spanned<StringId>, Option<STypeExpr>, ImplicitCoercions, SExpr),
    NewtypeDef(NewtypeDef),
    NewtypeRecDef(Vec<NewtypeDef>),
    Println(Vec<SExpr>),
    TypeAlias(Spanned<StringId>, STypeExpr),
}

#[derive(Debug)]
pub struct Exports {
    // Invariant: Target type must be a TypeExpr::Record.
    pub target: CoerceTarget,
    pub coercions: ImplicitCoercions,
}
impl Exports {
    pub fn field_keys(&self) -> Result<impl Iterator<Item = Spanned<StringId>>, ICE> {
        if let TypeExpr::Record(fields) = &self.target.ty.0 {
            Ok(fields
                .iter()
                // Include only fields, not type or alias names.
                .filter(|(_, member)| matches!(member, RecordTypeMember::Field(..)))
                .map(|(name, _)| *name))
        } else {
            Err(ice())
        }
    }
}

#[derive(Debug)]
pub struct File {
    pub statements: Spanned<Vec<Statement>>,
    pub exports: Option<Exports>,
}
