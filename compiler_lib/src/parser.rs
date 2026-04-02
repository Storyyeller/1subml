// Copyright (c) 2026 Robert Grosse. All rights reserved.
use crate::ast;
use crate::spans::*;
use lalrpop_util::ParseError;

use crate::grammar::ScriptParser;

fn friendly_token(raw: &str) -> &str {
    match raw {
        r###"r#"\"[^\\\\\"\\n\\r]*(?:\\\\[tn'\"\\\\][^\\\\\"\\n\\r]*)*\""#"### => "string literal",
        r###"r#"-?(?:0|[1-9][0-9]*)[eE]-?[0-9]+"#"### => "number",
        r###"r#"-?(?:0|[1-9][0-9]*)\\.[0-9]*(?:[eE]-?[0-9]+)?"#"### => "number",
        r###"r#"-?(?:[0-9]+)"#"### => "number",
        r###"r#"[A-Za-z_$][\\w$]*"#"### => "identifier",
        r###"r#"(?:&&)|(?:\\|\\|)"#"### => r#""&&", "||""#,
        r###"r#"[<>]=?\\.?|[!=]="#"### => "comparison operator",
        r###"r#"[\\*/%]\\.?"#"### => "operator",
        r###"r#"[\\+\\-]\\.?|\\^"#"### => r#""+", "-", "^", operator"#,
        other => other,
    }
}

fn format_expected(expected: &[String]) -> String {
    let mut names: Vec<&str> = expected.iter().flat_map(|s| friendly_token(s).split(", ")).collect();
    names.sort_unstable();
    names.dedup();
    names.join(", ")
}

fn convert_parse_error<T: std::fmt::Display>(
    mut sm: SpanMaker,
    e: ParseError<usize, T, (&'static str, Span)>,
) -> SpannedError {
    match e {
        ParseError::InvalidToken { location } => {
            SpannedError::new1("SyntaxError: Invalid token.", sm.span(location, location))
        }
        ParseError::UnrecognizedEof { location, expected } => {
            let mut e = SpannedError::new1("SyntaxError: Unexpected end of input.", sm.span(location, location));
            e.push_str(format!("Note: Expected one of: {}", format_expected(&expected)));
            e.push("Parse error occurred here:", sm.span(location, location));
            e
        }
        ParseError::UnrecognizedToken { token, expected } => {
            let mut e = SpannedError::new1(
                format!("SyntaxError: Unexpected token '{}'.", token.1),
                sm.span(token.0, token.2),
            );
            e.push_str(format!("Note: Expected one of: {}", format_expected(&expected)));
            e.push("Parse error occurred here:", sm.span(token.0, token.2));
            e
        }
        ParseError::ExtraToken { token } => {
            SpannedError::new1("SyntaxError: Unexpected extra token.", sm.span(token.0, token.2))
        }
        ParseError::User { error: (msg, span) } => SpannedError::new1(msg, span),
    }
}

pub struct Parser {
    parser: ScriptParser,
    pub spans: SpanManager,
}
impl Parser {
    pub fn new() -> Self {
        Self {
            parser: ScriptParser::new(),
            spans: SpanManager::default(),
        }
    }

    pub fn parse(
        &mut self,
        strings: &mut lasso::Rodeo,
        input: &str,
    ) -> Result<(ast::File, Vec<Spanned<ast::ImportId>>), SpannedError> {
        let span_maker = self.spans.add_source(input.to_owned());
        let mut ctx = ast::ParserContext {
            span_maker,
            strings,
            imports: Vec::new(),
        };

        let ast = self
            .parser
            .parse(&mut ctx, input)
            .map_err(|e| convert_parse_error(ctx.span_maker, e))?;
        Ok((ast, ctx.imports))
    }
}
