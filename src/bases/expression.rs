//! A parsed expression is validated before rows are filtered: unknown functions
//! cannot silently turn into null/false, even when the input set is empty.
use super::value::Value;
use anyhow::{Context, bail};

#[derive(Clone, Debug)]
pub enum Expr {
    Literal(Value),
    Ident(String),
    List(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
}
#[derive(Clone, Debug)]
enum Token {
    Literal(Value),
    Ident(String),
    Symbol(String),
    End,
}
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

pub const GLOBALS: &[(&str, usize, usize)] = &[
    ("escapeHTML", 1, 1),
    ("date", 1, 1),
    ("duration", 1, 1),
    ("file", 1, 1),
    ("html", 1, 1),
    ("if", 2, 3),
    ("image", 1, 1),
    ("icon", 1, 1),
    ("link", 1, 2),
    ("list", 1, 1),
    ("max", 1, usize::MAX),
    ("min", 1, usize::MAX),
    ("now", 0, 0),
    ("number", 1, 1),
    ("today", 0, 0),
    ("random", 0, 0),
];
pub const METHODS: &[(&str, usize, usize)] = &[
    ("isTruthy", 0, 0),
    ("isType", 1, 1),
    ("toString", 0, 0),
    ("isEmpty", 0, 0),
    ("date", 0, 0),
    ("format", 1, 1),
    ("time", 0, 0),
    ("relative", 0, 0),
    ("contains", 1, 1),
    ("containsAll", 0, usize::MAX),
    ("containsAny", 0, usize::MAX),
    ("endsWith", 1, 1),
    ("lower", 0, 0),
    ("replace", 2, 2),
    ("repeat", 1, 1),
    ("reverse", 0, 0),
    ("slice", 1, 2),
    ("split", 1, 2),
    ("startsWith", 1, 1),
    ("title", 0, 0),
    ("trim", 0, 0),
    ("abs", 0, 0),
    ("ceil", 0, 0),
    ("floor", 0, 0),
    ("round", 0, 1),
    ("toFixed", 1, 1),
    ("filter", 1, 1),
    ("flat", 0, 0),
    ("join", 1, 1),
    ("map", 1, 1),
    ("reduce", 2, 2),
    ("sort", 0, 0),
    ("unique", 0, 0),
    ("asFile", 0, 0),
    ("linksTo", 1, 1),
    ("asLink", 0, 1),
    ("hasLink", 1, 1),
    ("hasProperty", 1, 1),
    ("hasTag", 1, usize::MAX),
    ("inFolder", 1, 1),
    ("keys", 0, 0),
    ("values", 0, 0),
    ("matches", 1, 1),
    // Official custom-summary examples also use these aggregate helpers.
    ("mean", 0, 0),
    ("sum", 0, 0),
    ("min", 0, 0),
    ("max", 0, 0),
    ("median", 0, 0),
    ("stddev", 0, 0),
];
pub fn check_arity(
    name: &str,
    count: usize,
    functions: &[(&str, usize, usize)],
) -> anyhow::Result<()> {
    let Some((_, min, max)) = functions.iter().find(|(n, _, _)| *n == name) else {
        bail!("unsupported function: {name}");
    };
    if count < *min || count > *max {
        bail!("{name} received {count} arguments; expected {min}..={max}");
    }
    Ok(())
}
pub fn parse(source: &str) -> anyhow::Result<Expr> {
    let tokens = tokenize(source).with_context(|| format!("expression {source:?}"))?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    let expr = parser
        .expr(0)
        .with_context(|| format!("expression {source:?}"))?;
    if !matches!(parser.peek(), Token::End) {
        bail!(
            "unexpected token {:?} in expression {source:?}",
            parser.peek()
        );
    }
    validate(&expr).with_context(|| format!("expression {source:?}"))?;
    Ok(expr)
}
pub fn validate(expr: &Expr) -> anyhow::Result<()> {
    match expr {
        Expr::Literal(_) | Expr::Ident(_) => {}
        Expr::Unary(_, a) => validate(a)?,
        Expr::Binary(_, a, b) | Expr::Member(a, b) => {
            validate(a)?;
            validate(b)?;
        }
        Expr::List(items) => {
            for item in items {
                validate(item)?;
            }
        }
        Expr::Object(items) => {
            for (_, item) in items {
                validate(item)?;
            }
        }
        Expr::Call(callee, args) => {
            match callee.as_ref() {
                Expr::Ident(name) => check_arity(name, args.len(), GLOBALS)?,
                Expr::Member(_, name) => {
                    if let Expr::Literal(Value::String(name)) = name.as_ref() {
                        check_arity(name, args.len(), METHODS)?;
                    } else {
                        bail!("dynamic function names are unsupported");
                    }
                }
                _ => bail!("expression is not callable"),
            }
            validate(callee)?;
            for arg in args {
                validate(arg)?;
            }
        }
    }
    Ok(())
}
impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn next(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.pos += 1;
        token
    }
    fn take(&mut self, s: &str) -> bool {
        if matches!(self.peek(),Token::Symbol(v) if v==s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> anyhow::Result<()> {
        if self.take(s) {
            Ok(())
        } else {
            bail!("expected {s:?}, got {:?}", self.peek())
        }
    }
    fn expr(&mut self, min: u8) -> anyhow::Result<Expr> {
        self.depth += 1;
        if self.depth > 128 {
            bail!("expression nesting exceeds 128");
        }
        let mut left = match self.next() {
            Token::Literal(v) => Expr::Literal(v),
            Token::Ident(v) => Expr::Ident(v),
            Token::Symbol(s) if ["!", "-", "+"].contains(&s.as_str()) => {
                Expr::Unary(s, Box::new(self.expr(7)?))
            }
            Token::Symbol(s) if s == "(" => {
                let v = self.expr(0)?;
                self.expect(")")?;
                v
            }
            Token::Symbol(s) if s == "[" => {
                let mut items = Vec::new();
                if !self.take("]") {
                    loop {
                        items.push(self.expr(0)?);
                        if self.take("]") {
                            break;
                        }
                        self.expect(",")?;
                        if self.take("]") {
                            break;
                        }
                    }
                }
                Expr::List(items)
            }
            Token::Symbol(s) if s == "{" => {
                let mut items = Vec::new();
                if !self.take("}") {
                    loop {
                        let key = match self.next() {
                            Token::Ident(k) | Token::Literal(Value::String(k)) => k,
                            other => bail!("invalid object key {other:?}"),
                        };
                        self.expect(":")?;
                        items.push((key, self.expr(0)?));
                        if self.take("}") {
                            break;
                        }
                        self.expect(",")?;
                        if self.take("}") {
                            break;
                        }
                    }
                }
                Expr::Object(items)
            }
            other => bail!("expected an expression, got {other:?}"),
        };
        loop {
            if ast_depth(&left) > 128 {
                bail!("expression AST depth exceeds 128");
            }
            if self.take(".") {
                let Token::Ident(name) = self.next() else {
                    bail!("expected property name after dot");
                };
                left = Expr::Member(Box::new(left), Box::new(Expr::Literal(Value::String(name))));
                continue;
            }
            if self.take("[") {
                let key = self.expr(0)?;
                self.expect("]")?;
                left = Expr::Member(Box::new(left), Box::new(key));
                continue;
            }
            if self.take("(") {
                let mut args = Vec::new();
                if !self.take(")") {
                    loop {
                        args.push(self.expr(0)?);
                        if self.take(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                left = Expr::Call(Box::new(left), args);
                continue;
            }
            let Token::Symbol(op) = self.peek() else {
                break;
            };
            let precedence = match op.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | ">" | "<=" | ">=" => 4,
                "+" | "-" => 5,
                "*" | "/" | "%" => 6,
                _ => break,
            };
            if precedence < min {
                break;
            }
            let Token::Symbol(op) = self.next() else {
                unreachable!()
            };
            let right = self.expr(precedence + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        if ast_depth(&left) > 128 {
            bail!("expression AST depth exceeds 128");
        }
        self.depth -= 1;
        Ok(left)
    }
}
fn ast_depth(expr: &Expr) -> usize {
    1 + match expr {
        Expr::Literal(_) | Expr::Ident(_) => 0,
        Expr::Unary(_, expr) => ast_depth(expr),
        Expr::Binary(_, a, b) | Expr::Member(a, b) => ast_depth(a).max(ast_depth(b)),
        Expr::Call(callee, args) => {
            ast_depth(callee).max(args.iter().map(ast_depth).max().unwrap_or(0))
        }
        Expr::List(items) => items.iter().map(ast_depth).max().unwrap_or(0),
        Expr::Object(items) => items
            .iter()
            .map(|(_, item)| ast_depth(item))
            .max()
            .unwrap_or(0),
    }
}
fn tokenize(source: &str) -> anyhow::Result<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut tokens = Vec::new();
    let mut start = true;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let token = if ch == '\'' || ch == '"' {
            let quote = ch;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let c = chars[i];
                i += 1;
                if c == quote {
                    closed = true;
                    break;
                }
                if c != '\\' {
                    s.push(c);
                    continue;
                }
                let Some(c) = chars.get(i).copied() else {
                    bail!("unfinished escape");
                };
                i += 1;
                match c {
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'b' => s.push('\u{8}'),
                    'f' => s.push('\u{c}'),
                    'v' => s.push('\u{b}'),
                    '0' => s.push('\0'),
                    '\n' => {}
                    'u' | 'x' => {
                        let digits = if c == 'u' { 4 } else { 2 };
                        if i + digits > chars.len() {
                            bail!("unfinished Unicode escape");
                        }
                        let code = u32::from_str_radix(
                            &chars[i..i + digits].iter().collect::<String>(),
                            16,
                        )?;
                        i += digits;
                        let code = if (0xd800..=0xdbff).contains(&code)
                            && chars.get(i..i + 2) == Some(&['\\', 'u'])
                        {
                            i += 2;
                            if i + 4 > chars.len() {
                                bail!("unfinished surrogate pair");
                            }
                            let low = u32::from_str_radix(
                                &chars[i..i + 4].iter().collect::<String>(),
                                16,
                            )?;
                            i += 4;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                bail!("invalid surrogate pair");
                            }
                            0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00)
                        } else {
                            code
                        };
                        s.push(char::from_u32(code).context("invalid Unicode scalar in string")?);
                    }
                    other => s.push(other),
                }
            }
            if !closed {
                bail!("unterminated string");
            }
            Token::Literal(Value::String(s))
        } else if ch.is_ascii_digit()
            || (ch == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let begin = i;
            i += 1;
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
                i += 1;
                while chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            if matches!(chars.get(i), Some('e' | 'E')) {
                i += 1;
                if matches!(chars.get(i), Some('+' | '-')) {
                    i += 1;
                }
                while chars.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            let n = chars[begin..i].iter().collect::<String>().parse::<f64>()?;
            Token::Literal(Value::Number(n))
        } else if ch == '/' && start {
            i += 1;
            let mut pattern = String::new();
            let mut class = false;
            let mut closed = false;
            while i < chars.len() {
                let c = chars[i];
                i += 1;
                if c == '\\' {
                    pattern.push(c);
                    let Some(c) = chars.get(i) else {
                        bail!("unfinished regex escape");
                    };
                    pattern.push(*c);
                    i += 1;
                    continue;
                }
                if c == '[' {
                    class = true;
                }
                if c == ']' {
                    class = false;
                }
                if c == '/' && !class {
                    closed = true;
                    break;
                }
                pattern.push(c);
            }
            if !closed {
                bail!("unterminated regular expression");
            }
            let begin = i;
            while chars.get(i).is_some_and(char::is_ascii_alphabetic) {
                i += 1;
            }
            let flags: String = chars[begin..i].iter().collect();
            let mut seen = std::collections::HashSet::new();
            for flag in flags.chars() {
                if !"gimsuvy".contains(flag) || !seen.insert(flag) {
                    bail!("unsupported or duplicate regex flag: {flag}");
                }
            }
            if flags.contains('u') && flags.contains('v') {
                bail!("regex flags u and v cannot be combined");
            }
            regress::Regex::with_flags(&pattern, flags.as_str())
                .map_err(|e| anyhow::anyhow!("invalid regular expression: {e}"))?;
            Token::Literal(Value::Regex { pattern, flags })
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let begin = i;
            i += 1;
            while chars
                .get(i)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            {
                i += 1;
            }
            let ident: String = chars[begin..i].iter().collect();
            match ident.as_str() {
                "true" => Token::Literal(Value::Bool(true)),
                "false" => Token::Literal(Value::Bool(false)),
                "null" => Token::Literal(Value::Null),
                _ => Token::Ident(ident),
            }
        } else {
            i += 1;
            let mut s = ch.to_string();
            if i < chars.len()
                && ["==", "!=", "<=", ">=", "&&", "||"]
                    .contains(&format!("{ch}{}", chars[i]).as_str())
            {
                s.push(chars[i]);
                i += 1;
            }
            if ![
                "+", "-", "*", "/", "%", "!", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "(",
                ")", "[", "]", "{", "}", ".", ",", ":",
            ]
            .contains(&s.as_str())
            {
                bail!("unsupported token {s:?} at character {i}");
            }
            Token::Symbol(s)
        };
        start = match &token {
            Token::Literal(_) | Token::Ident(_) => false,
            Token::Symbol(s) => ![")", "]", "}"].contains(&s.as_str()),
            Token::End => true,
        };
        tokens.push(token);
        if tokens.len() > 100_000 {
            bail!("expression exceeds 100000 tokens");
        }
    }
    tokens.push(Token::End);
    Ok(tokens)
}
