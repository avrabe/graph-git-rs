# Rowan-Based Resilient BitBake Parser Architecture

## Why Rowan?

Rowan is the Concrete Syntax Tree (CST) library used by rust-analyzer. Unlike traditional Abstract Syntax Trees (AST), Rowan provides:

1. **Lossless Parsing**: Preserves all information including whitespace, comments, and invalid syntax
2. **Error Resilience**: Continues parsing even with syntax errors
3. **Incremental Reparsing**: Efficient updates for changed files
4. **Green/Red Tree Pattern**: Memory-efficient immutable trees with cheap position tracking
5. **Battle-Tested**: Powers rust-analyzer's excellent IDE experience

This makes it perfect for analyzing partial BitBake files, incomplete layers, and missing includes.

## Architecture Overview

```
Input (.bb file)
      ↓
   Lexer (logos) → Tokens
      ↓
   Parser (rowan) → CST (Concrete Syntax Tree)
      ↓
   Extractor → Structured Data
      ↓
   Resolver → Resolve includes/variables
      ↓
   Graph Builder → Neo4j
```

## Key Design Principles

1. **Never Fail**: Always produce some result, even for malformed input
2. **Collect Errors**: Track errors but continue parsing
3. **Graceful Degradation**: Extract what's available, skip what's not
4. **Minimal Assumptions**: Don't assume files exist or variables are defined
5. **Layer Awareness**: Handle missing layers and files gracefully

## Token Types

Using `logos` for fast lexing with error recovery:

```rust
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r\n\f]+")]  // Skip whitespace
pub enum TokenKind {
    // Operators
    #[token("=")]
    Eq,
    #[token(":=")]
    ColonEq,
    #[token("+=")]
    PlusEq,
    #[token("=+")]
    EqPlus,
    #[token(".=")]
    DotEq,
    #[token("=.")]
    EqDot,
    #[token("?=")]
    QuestionEq,
    #[token("??=")]
    QuestionQuestionEq,

    // Override syntax
    #[token(":append")]
    ColonAppend,
    #[token(":prepend")]
    ColonPrepend,
    #[token(":remove")]
    ColonRemove,

    // Keywords
    #[token("inherit")]
    Inherit,
    #[token("include")]
    Include,
    #[token("require")]
    Require,
    #[token("export")]
    Export,
    #[token("def")]
    Def,
    #[token("python")]
    Python,

    // Identifiers and literals
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_\-]*")]
    Ident,

    #[regex(r#""([^"\\]|\\.)*""#)]
    #[regex(r#"'([^'\\]|\\.)*'"#)]
    String,

    // Variable expansion
    #[regex(r"\$\{[^}]+\}")]
    VarExpansion,

    // Comments
    #[regex(r"#[^\n]*")]
    Comment,

    // Delimiters
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,

    // Special
    #[token(";")]
    Semicolon,
    #[token(":")]
    Colon,
    #[token("\\")]
    Backslash,

    // Newline (significant in BitBake)
    #[regex(r"\n")]
    Newline,

    // Error recovery
    #[error]
    Error,

    // End of file
    Eof,

    // Synthetic nodes (not from lexer)
    Whitespace,
    LineComment,
}
```

## Syntax Tree Structure

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens (same as TokenKind)
    EQ,
    COLON_EQ,
    PLUS_EQ,
    // ... all token types

    // Composite nodes
    ROOT,                    // File root
    VARIABLE_ASSIGNMENT,     // VAR = "value"
    VARIABLE_APPEND,         // VAR += "value"
    VARIABLE_PREPEND,        // VAR =+ "value"
    OVERRIDE_ASSIGNMENT,     // VAR:append = "value"
    VARIABLE_FLAG,           // VAR[flag] = "value"

    INHERIT_STATEMENT,       // inherit cmake
    INCLUDE_STATEMENT,       // include foo.inc
    REQUIRE_STATEMENT,       // require bar.inc
    EXPORT_STATEMENT,        // export VAR

    SHELL_FUNCTION,          // do_compile() { ... }
    PYTHON_FUNCTION,         // python do_compile() { ... }
    ANONYMOUS_PYTHON,        // python __anonymous() { ... }

    FUNCTION_CALL,           // oe_runmake
    STRING_LITERAL,
    IDENTIFIER,
    OVERRIDE_LIST,           // :append:machine
    PARAMETER_LIST,          // (params)

    ERROR,                   // Error recovery node
}
```

## Parser Implementation

```rust
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    builder: GreenNodeBuilder,
    errors: Vec<ParseError>,
}

impl Parser {
    pub fn parse(input: &str) -> Parse {
        let mut lexer = TokenKind::lexer(input);
        let mut tokens = Vec::new();

        // Tokenize with error recovery
        while let Some(token) = lexer.next() {
            tokens.push(Token {
                kind: token.unwrap_or(TokenKind::Error),
                text: lexer.slice().to_string(),
                span: lexer.span(),
            });
        }

        let mut parser = Parser {
            tokens,
            pos: 0,
            builder: GreenNodeBuilder::new(),
            errors: Vec::new(),
        };

        parser.parse_root();

        Parse {
            green_node: parser.builder.finish(),
            errors: parser.errors,
        }
    }

    fn parse_root(&mut self) {
        self.builder.start_node(ROOT.into());

        while !self.at_eof() {
            self.skip_trivia();

            if self.at_eof() {
                break;
            }

            // Try to parse a statement
            if !self.parse_statement() {
                // Error recovery: skip to next line
                self.errors.push(ParseError {
                    message: format!("Unexpected token: {:?}", self.current()),
                    span: self.current_span(),
                });
                self.advance_with_error();
            }
        }

        self.builder.finish_node();
    }

    fn parse_statement(&mut self) -> bool {
        match self.current() {
            TokenKind::Inherit => self.parse_inherit(),
            TokenKind::Include => self.parse_include(),
            TokenKind::Require => self.parse_require(),
            TokenKind::Export => self.parse_export(),
            TokenKind::Def | TokenKind::Python => self.parse_function(),
            TokenKind::Ident => {
                // Look ahead for assignment operators
                if self.is_assignment_ahead() {
                    self.parse_assignment()
                } else if self.peek() == Some(TokenKind::LParen) {
                    self.parse_shell_function()
                } else {
                    false
                }
            }
            TokenKind::Comment => {
                self.bump();  // Just consume comment
                true
            }
            TokenKind::Newline => {
                self.bump();  // Consume empty line
                true
            }
            _ => false,
        }
    }

    fn parse_assignment(&mut self) -> bool {
        self.builder.start_node(VARIABLE_ASSIGNMENT.into());

        // Parse variable name with optional overrides
        if !self.at(TokenKind::Ident) {
            self.builder.finish_node();
            return false;
        }

        self.bump();  // variable name

        // Handle override syntax: VAR:append:machine
        while self.at(TokenKind::Colon) {
            self.bump();  // :
            if self.at(TokenKind::Ident) ||
               self.at(TokenKind::ColonAppend) ||
               self.at(TokenKind::ColonPrepend) ||
               self.at(TokenKind::ColonRemove) {
                self.bump();  // override
            }
        }

        // Handle flag syntax: VAR[flag]
        if self.at(TokenKind::LBracket) {
            self.bump();  // [
            if self.at(TokenKind::Ident) {
                self.bump();  // flag name
            }
            self.expect(TokenKind::RBracket);  // ]
        }

        // Parse operator
        let op_kind = match self.current() {
            TokenKind::Eq => VARIABLE_ASSIGNMENT,
            TokenKind::ColonEq => VARIABLE_ASSIGNMENT,
            TokenKind::PlusEq => VARIABLE_APPEND,
            TokenKind::EqPlus => VARIABLE_PREPEND,
            TokenKind::DotEq => VARIABLE_APPEND,
            TokenKind::EqDot => VARIABLE_PREPEND,
            TokenKind::QuestionEq => VARIABLE_ASSIGNMENT,
            TokenKind::QuestionQuestionEq => VARIABLE_ASSIGNMENT,
            _ => {
                self.error("Expected assignment operator");
                self.builder.finish_node();
                return false;
            }
        };

        self.bump();  // operator

        // Parse value (can be multiline with \)
        self.parse_value();

        self.builder.finish_node();
        true
    }

    fn parse_value(&mut self) {
        loop {
            self.skip_whitespace();

            match self.current() {
                TokenKind::String => {
                    self.bump();
                }
                TokenKind::VarExpansion => {
                    self.bump();
                }
                TokenKind::Ident => {
                    self.bump();
                }
                TokenKind::Backslash => {
                    self.bump();  // Line continuation
                    if self.at(TokenKind::Newline) {
                        self.bump();
                    }
                    continue;  // Keep parsing
                }
                _ => break,
            }

            // Check for continuation or end
            if self.at(TokenKind::Backslash) {
                continue;
            } else {
                break;
            }
        }
    }

    fn advance_with_error(&mut self) {
        self.builder.start_node(ERROR.into());
        self.bump();

        // Skip to next newline or statement start
        while !self.at_eof() &&
              !self.at(TokenKind::Newline) &&
              !self.at(TokenKind::Inherit) &&
              !self.at(TokenKind::Include) &&
              !self.at(TokenKind::Require) {
            self.bump();
        }

        self.builder.finish_node();
    }
}
```

## Data Extraction from CST

```rust
pub struct BitbakeExtractor {
    variable_resolver: VariableResolver,
}

impl BitbakeExtractor {
    pub fn extract(cst: &SyntaxNode, file_path: &Path) -> Result<BitbakeRecipe, Vec<ExtractionError>> {
        let mut extractor = Self {
            variable_resolver: VariableResolver::new(),
        };

        let mut recipe = BitbakeRecipe {
            file_path: file_path.to_path_buf(),
            ..Default::default()
        };

        let mut errors = Vec::new();

        // Pass 1: Extract all variables for resolution context
        for node in cst.descendants() {
            if node.kind() == VARIABLE_ASSIGNMENT {
                if let Some((name, value, op)) = extractor.extract_variable(&node) {
                    extractor.variable_resolver.set(name.clone(), value.clone());
                    recipe.variables.insert(name, VariableAssignment {
                        value,
                        operation: op,
                        ..Default::default()
                    });
                }
            }
        }

        // Pass 2: Extract SRC_URI entries
        for (name, var) in &recipe.variables {
            if name == "SRC_URI" || name.starts_with("SRC_URI:") || name.starts_with("SRC_URI[") {
                match UriParser::parse(&var.value, &extractor.variable_resolver) {
                    Ok(sources) => recipe.sources.extend(sources),
                    Err(e) => errors.push(e.into()),
                }
            }
        }

        // Pass 3: Extract SRCREV
        if let Some(srcrev) = recipe.variables.get("SRCREV") {
            // Associate with git sources
            for source in &mut recipe.sources {
                if matches!(source.scheme, UriScheme::Git | UriScheme::GitSubmodule) {
                    if source.srcrev.is_none() {
                        source.srcrev = Some(srcrev.value.clone());
                    }
                }
            }
        }

        // Pass 4: Extract includes
        for node in cst.descendants() {
            match node.kind() {
                INCLUDE_STATEMENT | REQUIRE_STATEMENT => {
                    if let Some(path) = extractor.extract_include_path(&node) {
                        let resolved = extractor.variable_resolver.resolve(&path);
                        recipe.includes.push(IncludeDirective {
                            path: resolved,
                            required: node.kind() == REQUIRE_STATEMENT,
                        });
                    }
                }
                INHERIT_STATEMENT => {
                    for class_name in extractor.extract_inherit_classes(&node) {
                        recipe.inherits.push(ClassInherit {
                            class_name,
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }

        if errors.is_empty() {
            Ok(recipe)
        } else {
            Err(errors)
        }
    }
}
```

## Resilient Include Resolution

```rust
pub struct IncludeResolver {
    search_paths: Vec<PathBuf>,
    cache: HashMap<PathBuf, Option<BitbakeRecipe>>,
}

impl IncludeResolver {
    pub fn resolve_with_includes(
        &mut self,
        recipe: &mut BitbakeRecipe,
        base_path: &Path,
    ) -> Vec<ResolutionWarning> {
        let mut warnings = Vec::new();
        let mut processed = HashSet::new();
        processed.insert(recipe.file_path.clone());

        self.resolve_recursive(recipe, base_path, &mut processed, &mut warnings);
        warnings
    }

    fn resolve_recursive(
        &mut self,
        recipe: &mut BitbakeRecipe,
        base_path: &Path,
        processed: &mut HashSet<PathBuf>,
        warnings: &mut Vec<ResolutionWarning>,
    ) {
        let includes: Vec<_> = recipe.includes.iter().cloned().collect();

        for include_directive in includes {
            let resolved_path = self.find_include_file(
                &include_directive.path,
                &recipe.file_path,
                base_path,
            );

            match resolved_path {
                Some(path) if processed.contains(&path) => {
                    // Circular include - skip
                    warnings.push(ResolutionWarning::CircularInclude {
                        file: path,
                        included_from: recipe.file_path.clone(),
                    });
                }
                Some(path) => {
                    processed.insert(path.clone());

                    match self.parse_file_cached(&path) {
                        Ok(mut included) => {
                            // Merge data
                            recipe.sources.extend(included.sources.clone());
                            recipe.variables.extend(included.variables.clone());
                            recipe.build_depends.extend(included.build_depends.clone());
                            recipe.inherits.extend(included.inherits.clone());

                            // Recursively resolve includes
                            self.resolve_recursive(&mut included, base_path, processed, warnings);
                        }
                        Err(e) => {
                            warnings.push(ResolutionWarning::IncludeParseError {
                                file: path,
                                error: e.to_string(),
                            });
                        }
                    }
                }
                None => {
                    // File not found
                    if include_directive.required {
                        warnings.push(ResolutionWarning::RequiredIncludeNotFound {
                            path: include_directive.path.clone(),
                            searched_in: self.search_paths.clone(),
                        });
                    } else {
                        // Non-fatal for include (vs require)
                        warnings.push(ResolutionWarning::IncludeNotFound {
                            path: include_directive.path.clone(),
                        });
                    }
                }
            }
        }
    }

    fn find_include_file(&self, pattern: &str, current_file: &Path, base_path: &Path) -> Option<PathBuf> {
        let search_locations = vec![
            // 1. Relative to current file
            current_file.parent()?.join(pattern),

            // 2. Relative to layer base
            base_path.join(pattern),

            // 3. In common include directories
            base_path.join("conf").join(pattern),
            base_path.join("classes").join(pattern),
            base_path.join("recipes-core/include").join(pattern),
        ];

        // Add configured search paths
        for search_path in &self.search_paths {
            search_locations.push(search_path.join(pattern));
        }

        search_locations.into_iter().find(|p| p.exists())
    }

    fn parse_file_cached(&mut self, path: &Path) -> Result<BitbakeRecipe, BitbakeError> {
        if let Some(cached) = self.cache.get(path) {
            return cached.clone().ok_or(BitbakeError::CachedParseError);
        }

        let result = self.parse_file(path);
        self.cache.insert(path.to_path_buf(), result.as_ref().ok().cloned());
        result
    }

    fn parse_file(&self, path: &Path) -> Result<BitbakeRecipe, BitbakeError> {
        let content = std::fs::read_to_string(path)?;
        let parse = Parser::parse(&content);

        // Even with parse errors, try to extract what we can
        let recipe = BitbakeExtractor::extract(&parse.syntax(), path)
            .unwrap_or_else(|errors| {
                // Return partial recipe with errors noted
                BitbakeRecipe {
                    file_path: path.to_path_buf(),
                    parse_errors: errors,
                    ..Default::default()
                }
            });

        Ok(recipe)
    }
}
```

## Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incomplete_assignment() {
        // Missing value - should recover
        let input = r#"
FOO =
BAR = "value"
        "#;

        let parse = Parser::parse(input);
        assert!(!parse.errors.is_empty());

        let recipe = BitbakeExtractor::extract(&parse.syntax(), Path::new("test.bb"));
        // Should still extract BAR
        assert!(recipe.is_ok());
    }

    #[test]
    fn test_missing_include() {
        let input = r#"
include ${BPN}-crates.inc
SRC_URI = "git://example.com/repo.git"
        "#;

        let mut recipe = parse_recipe(input, Path::new("test.bb"));
        let mut resolver = IncludeResolver::new();

        let warnings = resolver.resolve_with_includes(&mut recipe, Path::new("."));

        // Should warn about missing include but continue
        assert!(!warnings.is_empty());
        // Should still have SRC_URI from main file
        assert_eq!(recipe.sources.len(), 1);
    }

    #[test]
    fn test_meta_fmu_recipe() {
        // Test with actual meta-fmu recipe
        let content = std::fs::read_to_string("/tmp/meta-fmu/recipes-application/fmu/fmu-rs_0.2.0.bb").unwrap();

        let parse = Parser::parse(&content);
        let recipe = BitbakeExtractor::extract(&parse.syntax(), Path::new("fmu-rs_0.2.0.bb")).unwrap();

        // Should find git SRC_URI
        assert!(recipe.sources.iter().any(|s| s.scheme == UriScheme::Git));

        // Should find includes
        assert!(recipe.includes.iter().any(|i| i.path.contains("crates.inc")));
        assert!(recipe.includes.iter().any(|i| i.path.contains("srcrev.inc")));
    }
}
```

## Migration Path

1. **Phase 1**: Implement lexer and basic parser
2. **Phase 2**: Add CST extraction to existing data model
3. **Phase 3**: Implement include resolution with error recovery
4. **Phase 4**: Test with meta-fmu and Poky
5. **Phase 5**: Replace old implementation, keeping backward compat API

## Performance Characteristics

- **Lexing**: O(n) with input size
- **Parsing**: O(n) worst case, typically faster
- **CST Storage**: O(n) memory with sharing
- **Include Resolution**: O(n * m) where n=files, m=includes per file
- **Caching**: Amortized O(1) for repeated parses

## Benefits

1. **Handles Malformed Input**: Never crashes, always produces result
2. **Preserves Information**: Can reconstruct original file exactly
3. **Rich Error Information**: Precise error locations and messages
4. **IDE-Ready**: Same tech as rust-analyzer
5. **Incremental**: Can reparse only changed portions
6. **Memory Efficient**: Green trees shared across versions

## Next Steps

1. Implement TokenKind with logos
2. Implement Parser with rowan
3. Implement BitbakeExtractor
4. Test with meta-fmu examples
5. Add include resolution
6. Test with Poky
7. Integrate with graph database
