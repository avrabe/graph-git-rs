//! Query expression parser
//!
//! Parses query strings into QueryExpr AST nodes.

use super::expr::{QueryExpr, TargetPattern};
use std::str::FromStr;

/// Query parser
pub struct QueryParser;

impl QueryParser {
    /// Parse a query string into a QueryExpr
    pub fn parse(query: &str) -> Result<QueryExpr, String> {
        let query = query.trim();

        // Handle set operations first (lowest precedence)
        if let Some(pos) = find_operator(query, " intersect ") {
            let left = Self::parse(&query[..pos])?;
            let right = Self::parse(&query[pos + 11..])?;
            return Ok(QueryExpr::Intersect(Box::new(left), Box::new(right)));
        }

        if let Some(pos) = find_operator(query, " union ") {
            let left = Self::parse(&query[..pos])?;
            let right = Self::parse(&query[pos + 7..])?;
            return Ok(QueryExpr::Union(Box::new(left), Box::new(right)));
        }

        if let Some(pos) = find_operator(query, " except ") {
            let left = Self::parse(&query[..pos])?;
            let right = Self::parse(&query[pos + 8..])?;
            return Ok(QueryExpr::Except(Box::new(left), Box::new(right)));
        }

        // Handle function calls
        if query.starts_with("deps(") {
            return Self::parse_deps(query);
        }

        if query.starts_with("rdeps(") {
            return Self::parse_rdeps(query);
        }

        if query.starts_with("somepath(") {
            return Self::parse_somepath(query);
        }

        if query.starts_with("allpaths(") {
            return Self::parse_allpaths(query);
        }

        if query.starts_with("kind(") {
            return Self::parse_kind(query);
        }

        if query.starts_with("filter(") {
            return Self::parse_filter(query);
        }

        if query.starts_with("attr(") {
            return Self::parse_attr(query);
        }

        // Task-specific query functions
        if query.starts_with("script(") {
            return Self::parse_script(query);
        }

        if query.starts_with("inputs(") {
            return Self::parse_inputs(query);
        }

        if query.starts_with("outputs(") {
            return Self::parse_outputs(query);
        }

        if query.starts_with("env(") {
            return Self::parse_env(query);
        }

        if query.starts_with("critical-path(") {
            return Self::parse_critical_path(query);
        }

        // Parse as target pattern
        let pattern = TargetPattern::from_str(query)
            .map_err(|e| format!("Invalid target pattern: {}", e))?;
        Ok(QueryExpr::Target(pattern))
    }

    fn parse_deps(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "deps")?;

        if args.is_empty() {
            return Err("deps() requires at least one argument".to_string());
        }

        if args.len() == 1 {
            let expr = Box::new(Self::parse(&args[0])?);
            Ok(QueryExpr::Deps {
                expr,
                max_depth: None,
            })
        } else if args.len() == 2 {
            let expr = Box::new(Self::parse(&args[0])?);
            let max_depth = args[1]
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("Invalid max_depth: {}", args[1]))?;
            Ok(QueryExpr::Deps {
                expr,
                max_depth: Some(max_depth),
            })
        } else {
            Err(format!("deps() takes 1 or 2 arguments, got {}", args.len()))
        }
    }

    fn parse_rdeps(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "rdeps")?;

        if args.len() != 2 {
            return Err(format!("rdeps() takes 2 arguments, got {}", args.len()));
        }

        let universe = Box::new(Self::parse(&args[0])?);
        let target = Box::new(Self::parse(&args[1])?);

        Ok(QueryExpr::ReverseDeps { universe, target })
    }

    fn parse_somepath(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "somepath")?;

        if args.len() != 2 {
            return Err(format!(
                "somepath() takes 2 arguments, got {}",
                args.len()
            ));
        }

        let from = Box::new(Self::parse(&args[0])?);
        let to = Box::new(Self::parse(&args[1])?);

        Ok(QueryExpr::SomePath { from, to })
    }

    fn parse_allpaths(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "allpaths")?;

        if args.len() != 2 {
            return Err(format!(
                "allpaths() takes 2 arguments, got {}",
                args.len()
            ));
        }

        let from = Box::new(Self::parse(&args[0])?);
        let to = Box::new(Self::parse(&args[1])?);

        Ok(QueryExpr::AllPaths { from, to })
    }

    fn parse_kind(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "kind")?;

        if args.len() != 2 {
            return Err(format!("kind() takes 2 arguments, got {}", args.len()));
        }

        let pattern = unquote(&args[0]);
        let expr = Box::new(Self::parse(&args[1])?);

        Ok(QueryExpr::Kind { pattern, expr })
    }

    fn parse_filter(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "filter")?;

        if args.len() != 2 {
            return Err(format!("filter() takes 2 arguments, got {}", args.len()));
        }

        let pattern = unquote(&args[0]);
        let expr = Box::new(Self::parse(&args[1])?);

        Ok(QueryExpr::Filter { pattern, expr })
    }

    fn parse_attr(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "attr")?;

        if args.len() != 3 {
            return Err(format!("attr() takes 3 arguments, got {}", args.len()));
        }

        let name = unquote(&args[0]);
        let value = unquote(&args[1]);
        let expr = Box::new(Self::parse(&args[2])?);

        Ok(QueryExpr::Attr { name, value, expr })
    }

    fn parse_script(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "script")?;

        if args.len() != 1 {
            return Err(format!("script() takes 1 argument, got {}", args.len()));
        }

        let expr = Box::new(Self::parse(&args[0])?);
        Ok(QueryExpr::Script(expr))
    }

    fn parse_inputs(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "inputs")?;

        if args.len() != 1 {
            return Err(format!("inputs() takes 1 argument, got {}", args.len()));
        }

        let expr = Box::new(Self::parse(&args[0])?);
        Ok(QueryExpr::Inputs(expr))
    }

    fn parse_outputs(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "outputs")?;

        if args.len() != 1 {
            return Err(format!("outputs() takes 1 argument, got {}", args.len()));
        }

        let expr = Box::new(Self::parse(&args[0])?);
        Ok(QueryExpr::Outputs(expr))
    }

    fn parse_env(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "env")?;

        if args.len() != 1 {
            return Err(format!("env() takes 1 argument, got {}", args.len()));
        }

        let expr = Box::new(Self::parse(&args[0])?);
        Ok(QueryExpr::Env(expr))
    }

    fn parse_critical_path(query: &str) -> Result<QueryExpr, String> {
        let args = extract_function_args(query, "critical-path")?;

        if args.len() != 1 {
            return Err(format!(
                "critical-path() takes 1 argument, got {}",
                args.len()
            ));
        }

        let expr = Box::new(Self::parse(&args[0])?);
        Ok(QueryExpr::CriticalPath(expr))
    }
}

/// Find an operator at the top level (not inside parentheses or quotes)
fn find_operator(s: &str, op: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_quote = false;
    let mut quote_char = ' ';

    for (i, c) in s.char_indices() {
        match c {
            '\'' | '"' => {
                if !in_quote {
                    in_quote = true;
                    quote_char = c;
                } else if c == quote_char {
                    in_quote = false;
                }
            }
            '(' if !in_quote => depth += 1,
            ')' if !in_quote => depth -= 1,
            _ => {}
        }

        if depth == 0 && !in_quote {
            if s[i..].starts_with(op) {
                return Some(i);
            }
        }
    }

    None
}

/// Extract function arguments from a function call
fn extract_function_args(query: &str, func_name: &str) -> Result<Vec<String>, String> {
    if !query.starts_with(&format!("{}(", func_name)) {
        return Err(format!("Not a {} function call", func_name));
    }

    if !query.ends_with(')') {
        return Err(format!("Missing closing parenthesis in {}", func_name));
    }

    let args_str = &query[func_name.len() + 1..query.len() - 1];

    if args_str.is_empty() {
        return Ok(Vec::new());
    }

    // Split by commas at top level (not inside parentheses or quotes)
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut depth = 0;
    let mut in_quote = false;
    let mut quote_char = ' ';

    for c in args_str.chars() {
        match c {
            '\'' | '"' => {
                if !in_quote {
                    in_quote = true;
                    quote_char = c;
                } else if c == quote_char {
                    in_quote = false;
                }
                current_arg.push(c);
            }
            '(' if !in_quote => {
                depth += 1;
                current_arg.push(c);
            }
            ')' if !in_quote => {
                depth -= 1;
                current_arg.push(c);
            }
            ',' if depth == 0 && !in_quote => {
                args.push(current_arg.trim().to_string());
                current_arg.clear();
            }
            _ => {
                current_arg.push(c);
            }
        }
    }

    if !current_arg.is_empty() {
        args.push(current_arg.trim().to_string());
    }

    Ok(args)
}

/// Remove quotes from a string
fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target() {
        let expr = QueryParser::parse("meta-core:busybox").unwrap();
        assert!(matches!(expr, QueryExpr::Target(_)));
    }

    #[test]
    fn test_parse_deps() {
        let expr = QueryParser::parse("deps(meta-core:busybox)").unwrap();
        match expr {
            QueryExpr::Deps { expr, max_depth } => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
                assert_eq!(max_depth, None);
            }
            _ => panic!("Expected Deps"),
        }

        let expr = QueryParser::parse("deps(meta-core:busybox, 2)").unwrap();
        match expr {
            QueryExpr::Deps { expr, max_depth } => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
                assert_eq!(max_depth, Some(2));
            }
            _ => panic!("Expected Deps"),
        }
    }

    #[test]
    fn test_parse_rdeps() {
        let expr = QueryParser::parse("rdeps(//..., meta-core:glibc)").unwrap();
        match expr {
            QueryExpr::ReverseDeps { universe, target } => {
                assert!(matches!(*universe, QueryExpr::Target(_)));
                assert!(matches!(*target, QueryExpr::Target(_)));
            }
            _ => panic!("Expected ReverseDeps"),
        }
    }

    #[test]
    fn test_parse_kind() {
        let expr = QueryParser::parse("kind('go_binary', //...)").unwrap();
        match expr {
            QueryExpr::Kind { pattern, expr } => {
                assert_eq!(pattern, "go_binary");
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected Kind"),
        }
    }

    #[test]
    fn test_parse_intersect() {
        let expr = QueryParser::parse("deps(meta-core:busybox) intersect kind('class_*', //...)")
            .unwrap();
        assert!(matches!(expr, QueryExpr::Intersect(_, _)));
    }

    #[test]
    fn test_parse_nested() {
        let expr = QueryParser::parse("deps(rdeps(//..., meta-core:glibc))").unwrap();
        match expr {
            QueryExpr::Deps { expr, .. } => {
                assert!(matches!(*expr, QueryExpr::ReverseDeps { .. }));
            }
            _ => panic!("Expected nested Deps -> ReverseDeps"),
        }
    }

    #[test]
    fn test_parse_script() {
        let expr = QueryParser::parse("script(*:busybox:configure)").unwrap();
        match expr {
            QueryExpr::Script(expr) => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected Script"),
        }
    }

    #[test]
    fn test_parse_inputs() {
        let expr = QueryParser::parse("inputs(*:busybox:compile)").unwrap();
        match expr {
            QueryExpr::Inputs(expr) => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected Inputs"),
        }
    }

    #[test]
    fn test_parse_outputs() {
        let expr = QueryParser::parse("outputs(*:busybox:install)").unwrap();
        match expr {
            QueryExpr::Outputs(expr) => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected Outputs"),
        }
    }

    #[test]
    fn test_parse_env() {
        let expr = QueryParser::parse("env(*:busybox:configure)").unwrap();
        match expr {
            QueryExpr::Env(expr) => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected Env"),
        }
    }

    #[test]
    fn test_parse_critical_path() {
        let expr = QueryParser::parse("critical-path(*:busybox:install)").unwrap();
        match expr {
            QueryExpr::CriticalPath(expr) => {
                assert!(matches!(*expr, QueryExpr::Target(_)));
            }
            _ => panic!("Expected CriticalPath"),
        }
    }

    #[test]
    fn test_parse_wildcard_pattern() {
        // Test wildcard recipe
        let expr = QueryParser::parse("*:busybox").unwrap();
        match expr {
            QueryExpr::Target(pattern) => {
                assert!(matches!(pattern, TargetPattern::WildcardRecipe { .. }));
            }
            _ => panic!("Expected Target with WildcardRecipe"),
        }

        // Test wildcard task
        let expr = QueryParser::parse("*:busybox:configure").unwrap();
        match expr {
            QueryExpr::Target(pattern) => {
                assert!(matches!(pattern, TargetPattern::WildcardTask { .. }));
            }
            _ => panic!("Expected Target with WildcardTask"),
        }
    }

    #[test]
    fn test_parse_composed_task_query() {
        // Test composability: script(deps(*:busybox:install, 5))
        let expr = QueryParser::parse("script(deps(*:busybox:install, 5))").unwrap();
        match expr {
            QueryExpr::Script(inner) => match *inner {
                QueryExpr::Deps { expr, max_depth } => {
                    assert!(matches!(*expr, QueryExpr::Target(_)));
                    assert_eq!(max_depth, Some(5));
                }
                _ => panic!("Expected Deps inside Script"),
            },
            _ => panic!("Expected Script"),
        }
    }
}
