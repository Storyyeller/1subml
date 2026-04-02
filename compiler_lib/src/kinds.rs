// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast;
use crate::spans::Span;
use crate::spans::SpannedError as SyntaxError;
use std::cell::Cell;
use std::iter::zip;
use std::rc::Rc;

type Result<T> = std::result::Result<T, SyntaxError>;

pub fn kinds_are_equal(a: &ast::Kind, b: &ast::Kind) -> bool {
    match (a, b) {
        (ast::Kind::Star, ast::Kind::Star) => true,
        (ast::Kind::Arrow(lkinds), ast::Kind::Arrow(rkinds)) => {
            if lkinds.len() != rkinds.len() {
                return false;
            }
            for (l, r) in zip(lkinds.iter(), rkinds.iter()) {
                if l.variance.0 != r.variance.0 || !kinds_are_equal(&l.kind.0, &r.kind.0) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn check_kinds(actual: &ast::SKind, expected: &ast::SKind) -> Result<()> {
    let aspan = actual.1;
    let espan = expected.1;

    let expected = expected.0.params();
    let actual = actual.0.params();

    if expected.len() != actual.len() {
        return Err(SyntaxError::new2(
            format!("KindError: Type constructor has {} parameters here:", actual.len()),
            aspan,
            format!("Note: But it is expected to have {} parameters here:", expected.len()),
            espan,
        ));
    }

    for (l, r) in zip(actual.iter(), expected.iter()) {
        if l.variance.0 != r.variance.0 {
            return Err(SyntaxError::new2(
                format!(
                    "KindError: Type constructor variance mismatch. Expected to be {} here:",
                    l.variance.0.to_display()
                ),
                l.variance.1,
                format!("Note: But it is {} here:", r.variance.0.to_display()),
                r.variance.1,
            ));
        }
        check_kinds(&l.kind, &r.kind)?;
    }

    Ok(())
}

fn print_kind_sub(k: &ast::Kind, out: &mut String) {
    match k {
        ast::Kind::Star => {}
        ast::Kind::Arrow(params) => {
            out.push_str("[");
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str("; ");
                }
                out.push_str(match param.variance.0 {
                    ast::Variance::Covariant => "+",
                    ast::Variance::Contravariant => "-",
                    ast::Variance::Invariant => "^",
                });
                print_kind_sub(&param.kind.0, out);
            }
            out.push_str("]");
        }
    }
}

pub fn print_kind(k: &ast::Kind) -> String {
    match k {
        ast::Kind::Star => "[]".to_string(),
        _ => {
            let mut out = String::new();
            print_kind_sub(k, &mut out);
            out
        }
    }
}

#[derive(Clone)]
pub enum KindVar {
    Known(ast::SKind),
    Var(Rc<(Span, Cell<Option<ast::SKind>>)>),
}
impl KindVar {
    pub fn new_var(span: Span) -> Self {
        KindVar::Var(Rc::new((span, Cell::new(None))))
    }

    pub fn flatten(&mut self) {
        if let KindVar::Var(rc) = self
            && let Some(k) = rc.1.take()
        {
            let temp = k.clone();
            rc.1.replace(Some(k));
            *self = KindVar::Known(temp);
        }
    }

    pub fn force(mut self) -> Result<ast::SKind> {
        self.flatten();
        match self {
            KindVar::Known(k) => Ok(k),
            KindVar::Var(rc) => {
                let mut e = SyntaxError::new();
                e.push_str("KindError: An explicit kind annotation is required here.");
                e.push_insert("", rc.0, " as [...] ");
                Err(e)
            }
        }
    }

    pub fn check(&mut self, expected: &ast::SKind) -> Result<()> {
        self.flatten();
        match self {
            KindVar::Known(actual) => check_kinds(actual, expected),
            KindVar::Var(rc) => {
                rc.1.replace(Some(expected.clone()));
                Ok(())
            }
        }
    }
}
impl std::fmt::Debug for KindVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KindVar::Known(k) => write!(f, "Known({:?})", print_kind(&k.0)),
            KindVar::Var(_) => write!(f, "Var(...)"),
        }
    }
}
