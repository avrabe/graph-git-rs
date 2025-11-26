//! Query expression parser using proper tokenization
//!
//! Parses query strings into QueryExpr AST nodes with good error messages.

use super::expr::{QueryExpr, TargetPattern};
use super::lexer::{QueryLexer, Token, TokenKind, ParseError};

/// Query parser with proper tokenization
pub struct QueryParser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    source: &'a str,
}

impl<'a> QueryParser<'a> {
    /// Parse a query string into a QueryExpr
    pub fn parse(query: &str) -> Result<QueryExpr, ParseError> {
        let tokens = QueryLexer::tokenize(query);
        let mut parser = QueryParser {
            tokens,
            pos: 0,
            source: query,
        };

        let expr = parser.parse_expr()?;

        // Ensure we consumed all tokens
        if parser.pos < parser.tokens.len() {
            let tok = &parser.tokens[parser.pos];
            return Err(ParseError::new(
                format!("unexpected token '{}'", tok.text),
                tok.span.clone(),
                query,
            ));
        }

        Ok(expr)
    }

    /// Parse an expression (handles set operations at lowest precedence)
    fn parse_expr(&mut self) -> Result<QueryExpr, ParseError> {
        let mut left = self.parse_primary()?;

        // Handle set operations (left-associative)
        while let Some(tok) = self.peek() {
            match tok.kind {
                TokenKind::Intersect => {
                    self.advance();
                    let right = self.parse_primary()?;
                    left = QueryExpr::Intersect(Box::new(left), Box::new(right));
                }
                TokenKind::Union => {
                    self.advance();
                    let right = self.parse_primary()?;
                    left = QueryExpr::Union(Box::new(left), Box::new(right));
                }
                TokenKind::Except => {
                    self.advance();
                    let right = self.parse_primary()?;
                    left = QueryExpr::Except(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// Parse a primary expression (function call or target pattern)
    fn parse_primary(&mut self) -> Result<QueryExpr, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            ParseError::new(
                "unexpected end of query",
                self.source.len()..self.source.len(),
                self.source,
            )
        })?;

        // Check for function calls
        if tok.is_function() {
            return self.parse_function_call();
        }

        // Parse as target pattern
        self.parse_target_pattern()
    }

    /// Parse a function call
    fn parse_function_call(&mut self) -> Result<QueryExpr, ParseError> {
        let func_tok = self.advance().unwrap();
        let func_name = func_tok.text.clone();
        let func_span = func_tok.span.clone();

        // Expect opening paren
        self.expect(TokenKind::LParen)?;

        // Parse arguments
        let args = self.parse_args()?;

        // Expect closing paren
        self.expect(TokenKind::RParen)?;

        // Build the appropriate QueryExpr
        match func_name.as_str() {
            "deps" => self.build_deps(args, func_span),
            "rdeps" => self.build_rdeps(args, func_span),
            "somepath" => self.build_somepath(args, func_span),
            "allpaths" => self.build_allpaths(args, func_span),
            "kind" => self.build_kind(args, func_span),
            "filter" => self.build_filter(args, func_span),
            "attr" => self.build_attr(args, func_span),
            "script" => self.build_unary(args, func_span, "script", QueryExpr::Script),
            "inputs" => self.build_unary(args, func_span, "inputs", QueryExpr::Inputs),
            "outputs" => self.build_unary(args, func_span, "outputs", QueryExpr::Outputs),
            "env" => self.build_unary(args, func_span, "env", QueryExpr::Env),
            "critical-path" => self.build_unary(args, func_span, "critical-path", QueryExpr::CriticalPath),
            _ => Err(ParseError::new(
                format!("unknown function '{func_name}'"),
                func_span,
                self.source,
            )),
        }
    }

    /// Parse comma-separated arguments
    fn parse_args(&mut self) -> Result<Vec<QueryArg>, ParseError> {
        let mut args = Vec::new();

        // Check for empty args
        if self.check(TokenKind::RParen) {
            return Ok(args);
        }

        // Parse first arg
        args.push(self.parse_arg()?);

        // Parse remaining args
        while self.check(TokenKind::Comma) {
            self.advance(); // consume comma
            args.push(self.parse_arg()?);
        }

        Ok(args)
    }

    /// Parse a single argument (can be expr, string, or number)
    fn parse_arg(&mut self) -> Result<QueryArg, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            ParseError::new(
                "expected argument",
                self.source.len()..self.source.len(),
                self.source,
            )
        })?;

        match tok.kind {
            TokenKind::Number => {
                let tok = self.advance().unwrap();
                Ok(QueryArg::Number(tok.number_value().unwrap()))
            }
            TokenKind::DoubleQuotedString | TokenKind::SingleQuotedString => {
                let tok = self.advance().unwrap();
                Ok(QueryArg::String(tok.string_value()))
            }
            _ => {
                // Try to parse as expression
                let expr = self.parse_expr()?;
                Ok(QueryArg::Expr(expr))
            }
        }
    }

    /// Parse a target pattern
    fn parse_target_pattern(&mut self) -> Result<QueryExpr, ParseError> {
        // Check for //... (all targets)
        if self.check(TokenKind::AllTargets) {
            self.advance();
            return Ok(QueryExpr::Target(TargetPattern::All));
        }

        // Check for * (also all targets)
        if self.check(TokenKind::Star) && self.peek_next().map(|t| t.kind != TokenKind::Colon).unwrap_or(true) {
            self.advance();
            return Ok(QueryExpr::Target(TargetPattern::All));
        }

        // Parse pattern parts: [layer:]recipe[:task]
        let first = self.parse_pattern_part()?;

        if !self.check(TokenKind::Colon) {
            // Just a recipe name (wildcard layer)
            return Ok(QueryExpr::Target(TargetPattern::WildcardRecipe {
                recipe: first,
            }));
        }

        self.advance(); // consume first colon

        // Check for ... (layer:...)
        if self.check(TokenKind::Ellipsis) {
            self.advance();
            return Ok(QueryExpr::Target(TargetPattern::LayerAll(first)));
        }

        let second = self.parse_pattern_part()?;

        if !self.check(TokenKind::Colon) {
            // layer:recipe or *:recipe
            if first == "*" {
                return Ok(QueryExpr::Target(TargetPattern::WildcardRecipe {
                    recipe: second,
                }));
            }
            return Ok(QueryExpr::Target(TargetPattern::Recipe {
                layer: first,
                recipe: second,
            }));
        }

        self.advance(); // consume second colon

        let third = self.parse_pattern_part()?;

        // layer:recipe:task or *:recipe:task
        if first == "*" {
            if third == "*" {
                return Ok(QueryExpr::Target(TargetPattern::WildcardRecipeAllTasks {
                    recipe: second,
                }));
            }
            return Ok(QueryExpr::Target(TargetPattern::WildcardTask {
                recipe: second,
                task: third,
            }));
        }

        if third == "*" {
            return Ok(QueryExpr::Target(TargetPattern::RecipeAllTasks {
                layer: first,
                recipe: second,
            }));
        }

        Ok(QueryExpr::Target(TargetPattern::Task {
            layer: first,
            recipe: second,
            task: third,
        }))
    }

    /// Parse a pattern part (identifier or *)
    fn parse_pattern_part(&mut self) -> Result<String, ParseError> {
        let tok = self.peek().ok_or_else(|| {
            ParseError::new(
                "expected pattern part",
                self.source.len()..self.source.len(),
                self.source,
            )
        })?;

        match tok.kind {
            TokenKind::Ident => {
                let tok = self.advance().unwrap();
                Ok(tok.text)
            }
            TokenKind::Star => {
                self.advance();
                Ok("*".to_string())
            }
            _ => Err(ParseError::new(
                format!("expected identifier or '*', found '{}'", tok.text),
                tok.span.clone(),
                self.source,
            )),
        }
    }

    // Builder helpers for each function type

    fn build_deps(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        match args.len() {
            1 => {
                let expr = args.into_iter().next().unwrap().into_expr().ok_or_else(|| {
                    ParseError::new("deps() argument must be a target expression", span.clone(), self.source)
                })?;
                Ok(QueryExpr::Deps {
                    expr: Box::new(expr),
                    max_depth: None,
                })
            }
            2 => {
                let mut iter = args.into_iter();
                let expr = iter.next().unwrap().into_expr().ok_or_else(|| {
                    ParseError::new("deps() first argument must be a target expression", span.clone(), self.source)
                })?;
                let depth = iter.next().unwrap().into_number().ok_or_else(|| {
                    ParseError::new("deps() second argument must be a number", span.clone(), self.source)
                })?;
                Ok(QueryExpr::Deps {
                    expr: Box::new(expr),
                    max_depth: Some(depth),
                })
            }
            n => Err(ParseError::new(
                format!("deps() takes 1 or 2 arguments, got {n}"),
                span,
                self.source,
            )),
        }
    }

    fn build_rdeps(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 2 {
            return Err(ParseError::new(
                format!("rdeps() takes 2 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let universe = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("rdeps() first argument must be a target expression", span.clone(), self.source)
        })?;
        let target = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("rdeps() second argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::ReverseDeps {
            universe: Box::new(universe),
            target: Box::new(target),
        })
    }

    fn build_somepath(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 2 {
            return Err(ParseError::new(
                format!("somepath() takes 2 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let from = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("somepath() first argument must be a target expression", span.clone(), self.source)
        })?;
        let to = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("somepath() second argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::SomePath {
            from: Box::new(from),
            to: Box::new(to),
        })
    }

    fn build_allpaths(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 2 {
            return Err(ParseError::new(
                format!("allpaths() takes 2 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let from = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("allpaths() first argument must be a target expression", span.clone(), self.source)
        })?;
        let to = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("allpaths() second argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::AllPaths {
            from: Box::new(from),
            to: Box::new(to),
        })
    }

    fn build_kind(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 2 {
            return Err(ParseError::new(
                format!("kind() takes 2 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let pattern = iter.next().unwrap().into_string().ok_or_else(|| {
            ParseError::new("kind() first argument must be a string pattern", span.clone(), self.source)
        })?;
        let expr = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("kind() second argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::Kind {
            pattern,
            expr: Box::new(expr),
        })
    }

    fn build_filter(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 2 {
            return Err(ParseError::new(
                format!("filter() takes 2 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let pattern = iter.next().unwrap().into_string().ok_or_else(|| {
            ParseError::new("filter() first argument must be a string pattern", span.clone(), self.source)
        })?;
        let expr = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("filter() second argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::Filter {
            pattern,
            expr: Box::new(expr),
        })
    }

    fn build_attr(&self, args: Vec<QueryArg>, span: std::ops::Range<usize>) -> Result<QueryExpr, ParseError> {
        if args.len() != 3 {
            return Err(ParseError::new(
                format!("attr() takes 3 arguments, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let mut iter = args.into_iter();
        let name = iter.next().unwrap().into_string().ok_or_else(|| {
            ParseError::new("attr() first argument must be a string", span.clone(), self.source)
        })?;
        let value = iter.next().unwrap().into_string().ok_or_else(|| {
            ParseError::new("attr() second argument must be a string", span.clone(), self.source)
        })?;
        let expr = iter.next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new("attr() third argument must be a target expression", span.clone(), self.source)
        })?;
        Ok(QueryExpr::Attr {
            name,
            value,
            expr: Box::new(expr),
        })
    }

    fn build_unary<F>(
        &self,
        args: Vec<QueryArg>,
        span: std::ops::Range<usize>,
        func_name: &str,
        constructor: F,
    ) -> Result<QueryExpr, ParseError>
    where
        F: FnOnce(Box<QueryExpr>) -> QueryExpr,
    {
        if args.len() != 1 {
            return Err(ParseError::new(
                format!("{func_name}() takes 1 argument, got {}", args.len()),
                span,
                self.source,
            ));
        }
        let expr = args.into_iter().next().unwrap().into_expr().ok_or_else(|| {
            ParseError::new(format!("{func_name}() argument must be a target expression"), span.clone(), self.source)
        })?;
        Ok(constructor(Box::new(expr)))
    }

    // Token manipulation helpers

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_next(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1)
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek().map(|t| t.kind == kind).unwrap_or(false)
    }

    fn advance(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        if let Some(tok) = self.peek() {
            if tok.kind == kind {
                return Ok(self.advance().unwrap());
            }
            return Err(ParseError::new(
                format!("expected '{}', found '{}'", kind, tok.text),
                tok.span.clone(),
                self.source,
            ));
        }
        Err(ParseError::new(
            format!("expected '{}', found end of query", kind),
            self.source.len()..self.source.len(),
            self.source,
        ))
    }
}

/// Argument types during parsing
enum QueryArg {
    Expr(QueryExpr),
    String(String),
    Number(usize),
}

impl QueryArg {
    fn into_expr(self) -> Option<QueryExpr> {
        match self {
            QueryArg::Expr(e) => Some(e),
            _ => None,
        }
    }

    fn into_string(self) -> Option<String> {
        match self {
            QueryArg::String(s) => Some(s),
            QueryArg::Expr(QueryExpr::Target(TargetPattern::WildcardRecipe { recipe })) => Some(recipe),
            _ => None,
        }
    }

    fn into_number(self) -> Option<usize> {
        match self {
            QueryArg::Number(n) => Some(n),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_target() {
        let expr = QueryParser::parse("busybox").unwrap();
        assert!(matches!(expr, QueryExpr::Target(TargetPattern::WildcardRecipe { .. })));
    }

    #[test]
    fn test_parse_target_pattern() {
        let expr = QueryParser::parse("meta-core:busybox").unwrap();
        assert!(matches!(expr, QueryExpr::Target(TargetPattern::Recipe { .. })));
    }

    #[test]
    fn test_parse_deps() {
        let expr = QueryParser::parse("deps(busybox)").unwrap();
        match expr {
            QueryExpr::Deps { max_depth, .. } => assert_eq!(max_depth, None),
            _ => panic!("Expected Deps"),
        }

        let expr = QueryParser::parse("deps(busybox, 2)").unwrap();
        match expr {
            QueryExpr::Deps { max_depth, .. } => assert_eq!(max_depth, Some(2)),
            _ => panic!("Expected Deps"),
        }
    }

    #[test]
    fn test_parse_rdeps() {
        let expr = QueryParser::parse("rdeps(//..., glibc)").unwrap();
        assert!(matches!(expr, QueryExpr::ReverseDeps { .. }));
    }

    #[test]
    fn test_parse_kind() {
        let expr = QueryParser::parse(r#"kind("*-native", //...)"#).unwrap();
        match expr {
            QueryExpr::Kind { pattern, .. } => assert_eq!(pattern, "*-native"),
            _ => panic!("Expected Kind"),
        }
    }

    #[test]
    fn test_parse_intersect() {
        let expr = QueryParser::parse("deps(busybox) intersect kind(\"lib*\", //...)").unwrap();
        assert!(matches!(expr, QueryExpr::Intersect(_, _)));
    }

    #[test]
    fn test_parse_nested() {
        let expr = QueryParser::parse("deps(rdeps(//..., glibc))").unwrap();
        match expr {
            QueryExpr::Deps { expr, .. } => {
                assert!(matches!(*expr, QueryExpr::ReverseDeps { .. }));
            }
            _ => panic!("Expected nested"),
        }
    }

    #[test]
    fn test_parse_wildcard() {
        let expr = QueryParser::parse("*:busybox:do_compile").unwrap();
        assert!(matches!(
            expr,
            QueryExpr::Target(TargetPattern::WildcardTask { .. })
        ));
    }

    #[test]
    fn test_parse_all_targets() {
        let expr = QueryParser::parse("//...").unwrap();
        assert!(matches!(expr, QueryExpr::Target(TargetPattern::All)));

        let expr = QueryParser::parse("*").unwrap();
        assert!(matches!(expr, QueryExpr::Target(TargetPattern::All)));
    }

    #[test]
    fn test_error_message() {
        let err = QueryParser::parse("deps(").unwrap_err();
        assert!(err.message.contains("expected"));
    }

    #[test]
    fn test_script_query() {
        let expr = QueryParser::parse("script(*:busybox:configure)").unwrap();
        assert!(matches!(expr, QueryExpr::Script(_)));
    }
}
