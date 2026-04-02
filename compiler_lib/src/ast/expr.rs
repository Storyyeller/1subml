// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast::*;
use crate::short_str::ShortStr;
use crate::spans::Span;
use crate::spans::Spanned;
use crate::type_errors::HoleSrc;

#[derive(Debug, Clone, Copy)]
pub enum Literal {
    Bool,
    Float,
    Int,
    Str,
}

#[derive(Debug, Clone, Copy)]
pub enum Op {
    Add,
    Sub,
    Mult,
    Div,
    Rem,

    Lt,
    Lte,
    Gt,
    Gte,

    Eq,
    Neq,

    BoolAnd,
    BoolOr,
}

pub type OpType = (Option<Literal>, Literal);
pub const BOOL_OP: OpType = (Some(Literal::Bool), Literal::Bool);
pub const INT_OP: OpType = (Some(Literal::Int), Literal::Int);
pub const FLOAT_OP: OpType = (Some(Literal::Float), Literal::Float);
pub const STR_OP: OpType = (Some(Literal::Str), Literal::Str);
pub const INT_CMP: OpType = (Some(Literal::Int), Literal::Bool);
pub const FLOAT_CMP: OpType = (Some(Literal::Float), Literal::Bool);
pub const ANY_CMP: OpType = (None, Literal::Bool);

// Struct types for each Expr variant
#[derive(Debug)]
pub struct BinOpExpr {
    pub lhs: Box<SExpr>,
    pub rhs: Box<SExpr>,
    pub op_type: OpType,
    pub op: Op,
    pub str: ShortStr,
}

#[derive(Debug)]
pub struct BlockExpr {
    pub statements: Vec<Statement>,
    pub expr: Option<Box<SExpr>>,
}

#[derive(Debug)]
pub struct CallExpr {
    pub func: Box<SExpr>,
    pub arg: Box<SExpr>,
    pub eval_arg_first: bool,
}

#[derive(Debug)]
pub struct CaseExpr {
    pub tag: Spanned<StringId>,
    pub expr: Box<SExpr>,
}

pub type TypeSubstitutions = Vec<(Spanned<StringId>, STypeExpr)>;
#[derive(Debug)]
pub struct CoerceTarget {
    pub ty: STypeExpr,
    pub substitutions: TypeSubstitutions,
}
impl CoerceTarget {
    pub fn hole_src_for_implicit_substitution(&self) -> CoerceSubHoleSrc {
        if let Some((_, ty)) = self.substitutions.last() {
            CoerceSubHoleSrc::Append(ty.1)
        } else {
            CoerceSubHoleSrc::Empty(self.ty.1)
        }
    }
}

#[derive(Debug)]
pub struct CoerceExpr {
    pub expr: Box<SExpr>,
    pub input_type: Option<STypeExpr>,
    pub target: CoerceTarget,
}

#[derive(Debug, Clone, Copy)]
pub enum CoerceSubHoleSrc {
    Append(Span),
    Empty(Span),
}
impl CoerceSubHoleSrc {
    pub fn get(&self, name: StringId) -> HoleSrc {
        match self {
            CoerceSubHoleSrc::Append(span) => HoleSrc::AddSubstitutionToEnd(*span, name),
            CoerceSubHoleSrc::Empty(span) => HoleSrc::AddSubstitutions(*span, name),
        }
    }
}

#[derive(Debug)]
pub struct FieldAccessExpr {
    pub expr: Box<SExpr>,
    pub field: Spanned<StringId>,
}

#[derive(Debug)]
pub struct FieldSetExpr {
    pub expr: Box<SExpr>,
    pub field: Spanned<StringId>,
    pub value: Box<SExpr>,
}

#[derive(Debug)]
pub struct FuncDefExpr {
    pub type_params: Option<Vec<TypeParam>>,
    pub coercions: ImplicitCoercions,
    pub param: Spanned<LetPattern>,
    pub return_type: Option<STypeExpr>,
    pub body: Box<SExpr>,
}

#[derive(Debug)]
pub struct IfExpr {
    pub cond: Spanned<Box<SExpr>>,
    pub then_expr: Box<SExpr>,
    pub else_expr: Option<Box<SExpr>>,
}

#[derive(Debug)]
pub struct LiteralExpr {
    pub lit_type: Literal,
    pub value: Spanned<String>,
}

#[derive(Debug)]
pub struct LoopExpr {
    pub body: Box<SExpr>,
}

#[derive(Debug)]
pub struct MatchCase {
    pub pattern: Spanned<LetPattern>,
    pub guard: Option<Box<SExpr>>,
}
#[derive(Debug)]
pub struct MatchArm {
    pub cases: Vec<MatchCase>,
    pub expr: Box<SExpr>,
}

#[derive(Debug)]
pub struct MatchExpr {
    pub expr: Spanned<Box<SExpr>>,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug)]
pub enum RecordExprMember {
    Field(bool, Box<SExpr>, Option<STypeExpr>),
    Alias(STypeExpr),
}
pub type KeyPair = (Spanned<StringId>, RecordExprMember);

#[derive(Debug)]
pub struct RecordExpr {
    pub fields: Vec<KeyPair>,
}

#[derive(Debug)]
pub struct TypedExpr {
    pub expr: Box<SExpr>,
    pub type_expr: STypeExpr,
}

#[derive(Debug)]
pub struct VariableExpr {
    pub name: StringId,
}

#[derive(Debug)]
pub enum Expr {
    BinOp(BinOpExpr),
    Block(BlockExpr),
    Call(CallExpr),
    Case(CaseExpr),
    Coerce(CoerceExpr),
    FieldAccess(FieldAccessExpr),
    FieldSet(FieldSetExpr),
    FuncDef(FuncDefExpr),
    Identity(Option<STypeExpr>),
    If(IfExpr),
    Literal(LiteralExpr),
    Loop(LoopExpr),
    Match(MatchExpr),
    Record(RecordExpr),
    Typed(TypedExpr),
    Variable(VariableExpr),
    Unsafe(String),
}
pub type SExpr = Spanned<Expr>;

// Constructor functions for Expr variants
pub fn binop(lhs: Box<SExpr>, rhs: Box<SExpr>, op_type: OpType, op: Op, str: ShortStr) -> Expr {
    Expr::BinOp(BinOpExpr {
        lhs,
        rhs,
        op_type,
        op,
        str,
    })
}

pub fn block(statements: Vec<Statement>, expr: Option<SExpr>) -> Expr {
    let expr = expr.map(Box::new);
    Expr::Block(BlockExpr { statements, expr })
}

pub fn call(func: Box<SExpr>, arg: Box<SExpr>, eval_arg_first: bool) -> Expr {
    Expr::Call(CallExpr {
        func,
        arg,
        eval_arg_first,
    })
}

pub fn case(tag: Spanned<StringId>, expr: Box<SExpr>) -> Expr {
    Expr::Case(CaseExpr { tag, expr })
}

pub fn field_access(expr: Box<SExpr>, field: Spanned<StringId>) -> Expr {
    Expr::FieldAccess(FieldAccessExpr { expr, field })
}

pub fn field_set(expr: Box<SExpr>, field: Spanned<StringId>, value: Box<SExpr>) -> Expr {
    Expr::FieldSet(FieldSetExpr { expr, field, value })
}

pub fn func_def(
    type_params: Option<Vec<TypeParam>>,
    coercions: ImplicitCoercions,
    param: Spanned<LetPattern>,
    return_type: Option<STypeExpr>,
    body: Box<SExpr>,
) -> Expr {
    Expr::FuncDef(FuncDefExpr {
        type_params,
        coercions,
        param,
        return_type,
        body,
    })
}

pub fn if_expr(cond: Spanned<Box<SExpr>>, then_expr: Box<SExpr>, else_expr: Option<Box<SExpr>>) -> Expr {
    Expr::If(IfExpr {
        cond,
        then_expr,
        else_expr,
    })
}

pub fn literal(lit_type: Literal, value: Spanned<String>) -> Expr {
    Expr::Literal(LiteralExpr { lit_type, value })
}

pub fn loop_expr(body: Box<SExpr>) -> Expr {
    Expr::Loop(LoopExpr { body })
}

pub fn match_expr(expr: Spanned<Box<SExpr>>, arms: Vec<MatchArm>) -> Expr {
    Expr::Match(MatchExpr { expr, arms })
}

pub fn record(fields: Vec<KeyPair>) -> Expr {
    Expr::Record(RecordExpr { fields })
}

pub fn paren_tuple_typed_or_coerce(
    strings: &mut lasso::Rodeo,
    mut exprs: Spanned<Vec<SExpr>>,
    ascription: Option<STypeExpr>,
    coerce: Option<(STypeExpr, Option<TypeSubstitutions>)>,
) -> Expr {
    // First handle the possible tuple part on the left:
    let expr = if exprs.0.len() == 1 {
        exprs.0.pop().unwrap()
    } else {
        // Tuple
        let fields = enumerate_tuple_fields(exprs.0, strings, |name, val| {
            (name, expr::RecordExprMember::Field(false, Box::new((val, name.1)), None))
        });
        (record(fields), exprs.1)
    };

    // Now check for type ascription or coercion
    match (ascription, coerce) {
        (input_type, Some((target_type, subs))) => Expr::Coerce(CoerceExpr {
            expr: Box::new(expr),
            input_type,
            target: CoerceTarget {
                ty: target_type,
                substitutions: subs.unwrap_or_default(),
            },
        }),
        (Some(input_type), None) => Expr::Typed(TypedExpr {
            expr: Box::new(expr),
            type_expr: input_type,
        }),
        // No type ascription or coercion, just parenthesized expression
        _ => expr.0,
    }
}

pub fn variable(name: StringId) -> Expr {
    Expr::Variable(VariableExpr { name })
}
