//! The small expression language of `entity_query.where` and
//! `sim_step.until`: a deterministic subset of CEL.
//!
//! ```text
//! Transform.translation.y < 0 && has(Camera)
//! distance(Transform.translation, [0, 0, 0]) < 5 || id == "player"
//! matches(id, "crate_*") && !has(Model)
//! ```
//!
//! - Paths start with a component name (`Transform.translation`) or `id`
//!   (the entity's scene id). Array fields take `.x .y .z .w` or `[0]`.
//! - Literals: numbers, `"strings"`, `true`, `false`, `null`, `[lists]`.
//! - Operators: `+ - * /`, `== != < <= > >=`, `&& || !`, parentheses.
//! - Functions: `has(Component)`, `abs(x)`, `len(x)`, `length(v)`,
//!   `distance(a, b)`, `min(a, b)`, `max(a, b)`, `matches(text, "glob*")`.
//!
//! An entity without the component gives `null`, and any comparison with
//! `null` (or between mismatched types) is false, so a query never fails
//! halfway through a scene. Mistakes are found when the expression is
//! compiled: syntax errors and unknown component or field names come back
//! with a column and a hint.

use serde_json::Value;
use wish_core::suggest::unknown_name_hint;
use wish_core::{Diagnostic, Result};
use wish_ecs::Registry;

/// The longest expression accepted, in characters.
pub const MAX_EXPRESSION_LEN: usize = 2000;
const MAX_DEPTH: usize = 64;

/// A compiled expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    node: Node,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Literal(Val),
    List(Vec<Node>),
    /// The scene id.
    Id,
    /// A component and the steps into it.
    Path(String, Vec<Step>),
    Has(String),
    /// How many entities of the world have the component.
    Count(String),
    /// Items picked out of a computed list.
    Select(Box<Node>, Vec<Step>),
    Call(Func, Vec<Node>),
    Not(Box<Node>),
    Neg(Box<Node>),
    Binary(Op, Box<Node>, Box<Node>),
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Field(String),
    Index(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Func {
    Abs,
    Len,
    Length,
    Distance,
    Min,
    Max,
    Matches,
}

const FUNCTIONS: &[&str] = &[
    "has", "count", "abs", "len", "length", "distance", "min", "max", "matches",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

/// A value during evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// Missing or `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A number.
    Num(f64),
    /// A string.
    Str(String),
    /// A list (vectors are lists of numbers).
    List(Vec<Val>),
}

impl Val {
    fn from_json(value: &Value) -> Self {
        match value {
            Value::Null | Value::Object(_) => Self::Null,
            Value::Bool(flag) => Self::Bool(*flag),
            Value::Number(number) => number.as_f64().map_or(Self::Null, Self::Num),
            Value::String(text) => Self::Str(text.clone()),
            Value::Array(items) => Self::List(items.iter().map(Self::from_json).collect()),
        }
    }

    /// Whether the value counts as true: only `true` does.
    pub fn is_true(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    fn numbers(&self) -> Option<Vec<f64>> {
        match self {
            Self::List(items) => items
                .iter()
                .map(|item| match item {
                    Self::Num(number) => Some(*number),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }
}

/// What an expression is evaluated against: one entity.
pub trait Subject {
    /// The entity's scene id.
    fn id(&self) -> &str;
    /// The component's JSON value, if the entity has it.
    fn component(&self, name: &str) -> Option<Value>;
    /// How many entities of the whole world have the component (`count(X)`);
    /// `None` where the subject cannot see the world.
    fn count(&self, _component: &str) -> Option<usize> {
        None
    }
}

// --------------------------------------------------------------------------
// Lexer
// --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Str(String),
    Ident(String),
    Sym(&'static str),
    End,
}

struct Lexed {
    token: Token,
    /// 1-based column, in characters.
    column: u32,
}

fn syntax(column: u32, message: String, hint: &str) -> Diagnostic {
    Diagnostic::error("expr_syntax", message, hint).at_line(1, Some(column))
}

const SYMBOLS: &[&str] = &[
    "&&", "||", "==", "!=", "<=", ">=", "<", ">", "!", "+", "-", "*", "/", "(", ")", "[", "]", ",",
    ".",
];

fn lex(text: &str) -> Result<Vec<Lexed>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        let column = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if c.is_whitespace() {
            index += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_digit()
                    || chars[index] == '.'
                    || chars[index] == 'e'
                    || chars[index] == 'E'
                    || ((chars[index] == '-' || chars[index] == '+')
                        && matches!(chars[index - 1], 'e' | 'E')))
            {
                index += 1;
            }
            let literal: String = chars[start..index].iter().collect();
            let number: f64 = literal.parse().map_err(|_| {
                syntax(
                    column,
                    format!("'{literal}' is not a number."),
                    "Write numbers like 3, -0.5 or 1e3.",
                )
            })?;
            out.push(Lexed {
                token: Token::Num(number),
                column,
            });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            out.push(Lexed {
                token: Token::Ident(chars[start..index].iter().collect()),
                column,
            });
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            index += 1;
            let mut value = String::new();
            loop {
                match chars.get(index) {
                    None => {
                        return Err(syntax(
                            column,
                            "A string is not closed.".to_owned(),
                            "End the string with the same quote it starts with.",
                        ));
                    }
                    Some(&ch) if ch == quote => {
                        index += 1;
                        break;
                    }
                    Some('\\') => {
                        if let Some(&next) = chars.get(index + 1) {
                            value.push(next);
                        }
                        index += 2;
                    }
                    Some(&ch) => {
                        value.push(ch);
                        index += 1;
                    }
                }
            }
            out.push(Lexed {
                token: Token::Str(value),
                column,
            });
            continue;
        }
        let rest: String = chars[index..chars.len().min(index + 2)].iter().collect();
        let Some(symbol) = SYMBOLS.iter().find(|symbol| rest.starts_with(**symbol)) else {
            let hint = match c {
                '=' => "Compare with == (two equals signs).",
                '&' => "Write 'and' as &&.",
                '|' => "Write 'or' as ||.",
                _ => {
                    "Use paths like Transform.translation.y, numbers, \"strings\", operators (+ - * / == != < <= > >= && || !) and functions such as has(Camera)."
                }
            };
            return Err(syntax(column, format!("Unexpected character '{c}'."), hint));
        };
        // A lone '=' or '&' is caught above; a single '=' never matches.
        index += symbol.len();
        out.push(Lexed {
            token: Token::Sym(symbol),
            column,
        });
    }
    out.push(Lexed {
        token: Token::End,
        column: u32::try_from(chars.len() + 1).unwrap_or(u32::MAX),
    });
    Ok(out)
}

// --------------------------------------------------------------------------
// Parser
// --------------------------------------------------------------------------

struct Parser<'a> {
    tokens: Vec<Lexed>,
    at: usize,
    registry: &'a Registry,
    depth: usize,
}

fn describe(token: &Token) -> String {
    match token {
        Token::Num(number) => format!("the number {number}"),
        Token::Str(text) => format!("the string \"{text}\""),
        Token::Ident(name) => format!("'{name}'"),
        Token::Sym(symbol) => format!("'{symbol}'"),
        Token::End => "the end".to_owned(),
    }
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        self.tokens
            .get(self.at)
            .map_or(&Token::End, |lexed| &lexed.token)
    }

    fn column(&self) -> u32 {
        self.tokens.get(self.at).map_or(1, |lexed| lexed.column)
    }

    fn eat(&mut self, symbol: &str) -> bool {
        if matches!(self.peek(), Token::Sym(s) if *s == symbol) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, symbol: &str) -> Result<()> {
        if self.eat(symbol) {
            return Ok(());
        }
        Err(syntax(
            self.column(),
            format!("Expected '{symbol}' but found {}.", describe(self.peek())),
            "Check that every ( has a ) and every [ has a ].",
        ))
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(syntax(
                self.column(),
                "The expression nests too deeply.".to_owned(),
                "Split it into simpler parts.",
            ));
        }
        Ok(())
    }

    fn binary(&mut self, level: usize) -> Result<Node> {
        const LEVELS: &[&[(&str, Op)]] = &[
            &[("||", Op::Or)],
            &[("&&", Op::And)],
            &[
                ("==", Op::Eq),
                ("!=", Op::Ne),
                ("<=", Op::Le),
                (">=", Op::Ge),
                ("<", Op::Lt),
                (">", Op::Gt),
            ],
            &[("+", Op::Add), ("-", Op::Sub)],
            &[("*", Op::Mul), ("/", Op::Div)],
        ];
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        self.enter()?;
        let mut left = self.binary(level + 1)?;
        'outer: loop {
            for (symbol, op) in *ops {
                if self.eat(symbol) {
                    let right = self.binary(level + 1)?;
                    left = Node::Binary(*op, Box::new(left), Box::new(right));
                    // Comparisons do not chain (a < b < c is a mistake).
                    if level == 2 {
                        break 'outer;
                    }
                    continue 'outer;
                }
            }
            break;
        }
        self.depth -= 1;
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node> {
        self.enter()?;
        let node = if self.eat("!") {
            Node::Not(Box::new(self.unary()?))
        } else if self.eat("-") {
            Node::Neg(Box::new(self.unary()?))
        } else {
            self.primary()?
        };
        self.depth -= 1;
        Ok(node)
    }

    fn primary(&mut self) -> Result<Node> {
        let column = self.column();
        let token = self.peek().clone();
        self.at += 1;
        match token {
            Token::Num(number) => Ok(Node::Literal(Val::Num(number))),
            Token::Str(text) => Ok(Node::Literal(Val::Str(text))),
            Token::Sym("(") => {
                let node = self.binary(0)?;
                self.expect(")")?;
                // `(a + b).x` and `(a + b)[0]` pick a vector item.
                let mut steps = Vec::new();
                loop {
                    if self.eat(".") {
                        let index = match self.peek() {
                            Token::Ident(letter) => {
                                ["x", "y", "z", "w"].iter().position(|l| l == letter)
                            }
                            _ => None,
                        };
                        let Some(index) = index else {
                            return Err(syntax(
                                self.column(),
                                "After a parenthesised value only .x .y .z .w pick an item."
                                    .to_owned(),
                                "Write it like (a + b).x or (a + b)[0].",
                            ));
                        };
                        self.at += 1;
                        steps.push(Step::Index(index));
                    } else if self.eat("[") {
                        let Token::Num(number) = self.peek().clone() else {
                            return Err(syntax(
                                self.column(),
                                "A list index must be a whole number.".to_owned(),
                                "Write it like (a + b)[0].",
                            ));
                        };
                        self.at += 1;
                        self.expect("]")?;
                        steps.push(Step::Index(number.max(0.0) as usize));
                    } else {
                        break;
                    }
                }
                if steps.is_empty() {
                    Ok(node)
                } else {
                    Ok(Node::Select(Box::new(node), steps))
                }
            }
            Token::Sym("[") => {
                let mut items = Vec::new();
                if !self.eat("]") {
                    loop {
                        items.push(self.binary(0)?);
                        if self.eat("]") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                Ok(Node::List(items))
            }
            Token::Ident(name) => match name.as_str() {
                "true" => Ok(Node::Literal(Val::Bool(true))),
                "false" => Ok(Node::Literal(Val::Bool(false))),
                "null" => Ok(Node::Literal(Val::Null)),
                _ if self.eat("(") => self.call(&name, column),
                _ => self.path(name, column),
            },
            other => Err(syntax(
                column,
                format!("Expected a value but found {}.", describe(&other)),
                "A value is a path (Transform.translation.y), a number, a \"string\", a [list], or a function call such as has(Camera).",
            )),
        }
    }

    fn call(&mut self, name: &str, column: u32) -> Result<Node> {
        let mut args = Vec::new();
        if name == "has" || name == "count" {
            let component = match self.peek().clone() {
                Token::Ident(component) => {
                    self.at += 1;
                    component
                }
                other => {
                    return Err(syntax(
                        self.column(),
                        format!("{name}() takes a component name, not {}.", describe(&other)),
                        if name == "has" {
                            "Write it like has(Camera)."
                        } else {
                            "Write it like count(Camera)."
                        },
                    ));
                }
            };
            self.check_component(&component, column)?;
            self.expect(")")?;
            return Ok(if name == "has" {
                Node::Has(component)
            } else {
                Node::Count(component)
            });
        }
        let (func, arity) = match name {
            "abs" => (Func::Abs, 1),
            "len" => (Func::Len, 1),
            "length" => (Func::Length, 1),
            "distance" => (Func::Distance, 2),
            "min" => (Func::Min, 2),
            "max" => (Func::Max, 2),
            "matches" => (Func::Matches, 2),
            _ => {
                return Err(syntax(
                    column,
                    format!("There is no function '{name}'."),
                    &unknown_name_hint(name, FUNCTIONS),
                )
                .with_candidates(FUNCTIONS.iter().copied()));
            }
        };
        if !self.eat(")") {
            loop {
                args.push(self.binary(0)?);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        if args.len() != arity {
            return Err(syntax(
                column,
                format!(
                    "{name}() takes {arity} argument(s), but got {}.",
                    args.len()
                ),
                "Check the function's arguments, for example distance(Transform.translation, [0, 0, 0]).",
            ));
        }
        Ok(Node::Call(func, args))
    }

    fn component_names(&self) -> Vec<&str> {
        // Resources are named like components: `Stats.bounces > 10`.
        self.registry
            .components()
            .map(|(_, info)| info.name())
            .chain(self.registry.resources().map(|(_, info)| info.name()))
            .collect()
    }

    fn check_component(&self, name: &str, column: u32) -> Result<()> {
        let names = self.component_names();
        if names.contains(&name) {
            return Ok(());
        }
        let mut valid = names.clone();
        valid.push("id");
        Err(Diagnostic::error(
            "expr_unknown_name",
            format!("There is no component called '{name}'."),
            unknown_name_hint(name, &valid),
        )
        .at_line(1, Some(column))
        .with_candidates(valid))
    }

    fn path(&mut self, name: String, column: u32) -> Result<Node> {
        if name == "id" {
            return Ok(Node::Id);
        }
        self.check_component(&name, column)?;
        let schema = self
            .registry
            .components()
            .map(|(_, info)| info)
            .chain(self.registry.resources().map(|(_, info)| info))
            .find(|info| info.name() == name)
            .map(|info| wish_schema::json_schema::json_schema(info.schema()));
        let mut steps = Vec::new();
        let mut current = schema;
        loop {
            if self.eat(".") {
                let step_column = self.column();
                let Token::Ident(field) = self.peek().clone() else {
                    return Err(syntax(
                        step_column,
                        format!(
                            "Expected a field name after '.' but found {}.",
                            describe(self.peek())
                        ),
                        "Write a field name, such as Transform.translation.",
                    ));
                };
                self.at += 1;
                // Vector components by letter.
                let is_array = current
                    .as_ref()
                    .is_some_and(|schema| schema.get("type") == Some(&Value::from("array")));
                if is_array
                    && let Some(index) = ["x", "y", "z", "w"].iter().position(|l| *l == field)
                {
                    steps.push(Step::Index(index));
                    current = current.and_then(|schema| schema.get("items").cloned());
                    continue;
                }
                if let Some(properties) = current
                    .as_ref()
                    .and_then(|schema| schema.get("properties"))
                    .and_then(Value::as_object)
                {
                    if !properties.contains_key(&field) {
                        let fields: Vec<&str> = properties.keys().map(String::as_str).collect();
                        return Err(Diagnostic::error(
                            "expr_unknown_name",
                            format!("'{name}' has no field '{field}' on this path."),
                            unknown_name_hint(&field, &fields),
                        )
                        .at_line(1, Some(step_column))
                        .with_candidates(fields));
                    }
                    current = properties.get(&field).cloned();
                } else if is_array {
                    return Err(syntax(
                        step_column,
                        format!("'.{field}' does not pick an item of a list."),
                        "Pick list items with .x .y .z .w or [index].",
                    ));
                } else {
                    current = None;
                }
                steps.push(Step::Field(field));
            } else if self.eat("[") {
                let Token::Num(number) = self.peek().clone() else {
                    return Err(syntax(
                        self.column(),
                        "A list index must be a whole number.".to_owned(),
                        "Write it like Transform.translation[1].",
                    ));
                };
                self.at += 1;
                self.expect("]")?;
                if number < 0.0 || number.fract() != 0.0 {
                    return Err(syntax(
                        self.column(),
                        format!("{number} is not a list index."),
                        "Use a whole number from 0.",
                    ));
                }
                steps.push(Step::Index(number as usize));
                current = current.and_then(|schema| schema.get("items").cloned());
            } else {
                break;
            }
        }
        Ok(Node::Path(name, steps))
    }
}

impl Expr {
    /// Compiles `text`, checking component and field names against
    /// `registry`.
    pub fn compile(text: &str, registry: &Registry) -> Result<Self> {
        if text.chars().count() > MAX_EXPRESSION_LEN {
            return Err(syntax(
                1,
                format!("The expression is longer than {MAX_EXPRESSION_LEN} characters."),
                "Split the query into simpler parts.",
            ));
        }
        if text.trim().is_empty() {
            return Err(syntax(
                1,
                "The expression is empty.".to_owned(),
                "Write a condition such as Transform.translation.y < 0.",
            ));
        }
        let mut parser = Parser {
            tokens: lex(text)?,
            at: 0,
            registry,
            depth: 0,
        };
        let node = parser.binary(0)?;
        if *parser.peek() != Token::End {
            let found = describe(parser.peek());
            let hint = if matches!(
                parser.peek(),
                Token::Sym("<" | ">" | "<=" | ">=" | "==" | "!=")
            ) {
                "Comparisons do not chain: write a < b && b < c."
            } else {
                "Join conditions with && or ||."
            };
            return Err(syntax(
                parser.column(),
                format!("Unexpected {found} after a complete expression."),
                hint,
            ));
        }
        Ok(Self { node })
    }

    /// Evaluates the expression for one entity.
    pub fn eval(&self, subject: &dyn Subject) -> Val {
        eval(&self.node, subject)
    }

    /// Whether `has(...)` appears under a negation (`!has(Coin)`,
    /// `has(Coin) == false`): over "some entity", such a condition holds as
    /// soon as one entity lacks the component, which is rarely what was meant.
    pub fn negates_has(&self) -> bool {
        fn walk(node: &Node, negated: bool) -> bool {
            match node {
                Node::Has(_) => negated,
                Node::Not(inner) => walk(inner, !negated),
                Node::Select(inner, _) | Node::Neg(inner) => walk(inner, negated),
                Node::List(items) | Node::Call(_, items) => {
                    items.iter().any(|item| walk(item, negated))
                }
                // Inside a comparison, has() may stand for either truth.
                Node::Binary(op, left, right) if *op != Op::And && *op != Op::Or => {
                    walk(left, true) || walk(right, true)
                }
                Node::Binary(_, left, right) => walk(left, negated) || walk(right, negated),
                Node::Literal(_) | Node::Id | Node::Path(..) | Node::Count(_) => false,
            }
        }
        walk(&self.node, false)
    }

    /// Whether the expression is true for one entity.
    pub fn matches(&self, subject: &dyn Subject) -> bool {
        self.eval(subject).is_true()
    }

    /// Every component path the expression reads (`Score.points`,
    /// `Transform.translation[1]`), each with its value for `subject`, in
    /// the order they appear, once each. For saying why a condition did not
    /// hold.
    pub fn path_values(&self, subject: &dyn Subject) -> Vec<(String, Val)> {
        let mut found = Vec::new();
        collect_paths(&self.node, subject, &mut found);
        found
    }
}

fn collect_paths(node: &Node, subject: &dyn Subject, found: &mut Vec<(String, Val)>) {
    match node {
        Node::Path(component, steps) => {
            let mut text = component.clone();
            for step in steps {
                match step {
                    Step::Field(field) => {
                        text.push('.');
                        text.push_str(field);
                    }
                    Step::Index(index) => text.push_str(&format!("[{index}]")),
                }
            }
            if !found.iter().any(|(seen, _)| *seen == text) {
                found.push((text, eval(node, subject)));
            }
        }
        Node::List(items) | Node::Call(_, items) => {
            for item in items {
                collect_paths(item, subject, found);
            }
        }
        Node::Select(inner, _) | Node::Not(inner) | Node::Neg(inner) => {
            collect_paths(inner, subject, found);
        }
        Node::Binary(_, left, right) => {
            collect_paths(left, subject, found);
            collect_paths(right, subject, found);
        }
        Node::Literal(_) | Node::Id | Node::Has(_) | Node::Count(_) => {}
    }
}

fn eval(node: &Node, subject: &dyn Subject) -> Val {
    match node {
        Node::Literal(value) => value.clone(),
        Node::List(items) => Val::List(items.iter().map(|item| eval(item, subject)).collect()),
        Node::Id => Val::Str(subject.id().to_owned()),
        Node::Has(component) => Val::Bool(subject.component(component).is_some()),
        Node::Count(component) => subject
            .count(component)
            .map_or(Val::Null, |n| Val::Num(n as f64)),
        Node::Path(component, steps) => {
            let Some(mut value) = subject.component(component) else {
                return Val::Null;
            };
            for step in steps {
                value = match (step, value) {
                    (Step::Field(field), Value::Object(mut map)) => {
                        map.remove(field).unwrap_or(Value::Null)
                    }
                    (Step::Index(index), Value::Array(mut items)) if *index < items.len() => {
                        items.swap_remove(*index)
                    }
                    _ => Value::Null,
                };
            }
            Val::from_json(&value)
        }
        Node::Select(inner, steps) => {
            let mut value = eval(inner, subject);
            for step in steps {
                value = match (step, value) {
                    (Step::Index(index), Val::List(mut items)) if *index < items.len() => {
                        items.swap_remove(*index)
                    }
                    _ => Val::Null,
                };
            }
            value
        }
        Node::Not(inner) => match eval(inner, subject) {
            Val::Bool(flag) => Val::Bool(!flag),
            _ => Val::Null,
        },
        Node::Neg(inner) => match eval(inner, subject) {
            Val::Num(number) => Val::Num(-number),
            Val::List(items) => Val::List(
                items
                    .into_iter()
                    .map(|item| match item {
                        Val::Num(number) => Val::Num(-number),
                        _ => Val::Null,
                    })
                    .collect(),
            ),
            _ => Val::Null,
        },
        // Three-valued logic: a condition about something missing is
        // unknown (null) and stays unknown under `!`, so `!(Camera.fov > 10)`
        // does not hold for an entity without a camera (M5 closing review).
        Node::Binary(Op::And, left, right) => match eval(left, subject) {
            Val::Bool(false) => Val::Bool(false),
            Val::Bool(true) => truth(eval(right, subject)),
            _ => match eval(right, subject) {
                Val::Bool(false) => Val::Bool(false),
                _ => Val::Null,
            },
        },
        Node::Binary(Op::Or, left, right) => match eval(left, subject) {
            Val::Bool(true) => Val::Bool(true),
            Val::Bool(false) => truth(eval(right, subject)),
            _ => match eval(right, subject) {
                Val::Bool(true) => Val::Bool(true),
                _ => Val::Null,
            },
        },
        Node::Binary(op, left, right) => binary(*op, eval(left, subject), eval(right, subject)),
        Node::Call(func, args) => {
            let values: Vec<Val> = args.iter().map(|arg| eval(arg, subject)).collect();
            call(*func, &values)
        }
    }
}

fn arithmetic(op: Op, a: f64, b: f64) -> Val {
    let result = match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        Op::Div if b != 0.0 => a / b,
        _ => return Val::Null,
    };
    if result.is_finite() {
        Val::Num(result)
    } else {
        Val::Null
    }
}

/// A value as a condition: true, false, or unknown.
fn truth(value: Val) -> Val {
    match value {
        Val::Bool(flag) => Val::Bool(flag),
        _ => Val::Null,
    }
}

fn binary(op: Op, left: Val, right: Val) -> Val {
    if matches!(op, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge)
        && (left == Val::Null || right == Val::Null)
    {
        // Comparing with something missing is unknown, not false.
        return Val::Null;
    }
    match op {
        Op::Eq => Val::Bool(left == right),
        Op::Ne => Val::Bool(left != right),
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let ordering = match (&left, &right) {
                (Val::Num(a), Val::Num(b)) => a.partial_cmp(b),
                (Val::Str(a), Val::Str(b)) => Some(a.cmp(b)),
                _ => None,
            };
            ordering.map_or(Val::Null, |ordering| {
                Val::Bool(match op {
                    Op::Lt => ordering.is_lt(),
                    Op::Le => ordering.is_le(),
                    Op::Gt => ordering.is_gt(),
                    _ => ordering.is_ge(),
                })
            })
        }
        Op::Add | Op::Sub | Op::Mul | Op::Div => match (&left, &right) {
            (Val::Num(a), Val::Num(b)) => arithmetic(op, *a, *b),
            (Val::Str(a), Val::Str(b)) if op == Op::Add => Val::Str(format!("{a}{b}")),
            _ => {
                // Element-wise on vectors, or a vector and a number.
                let (Some(a), Some(b)) = (vector(&left), vector(&right)) else {
                    return Val::Null;
                };
                let n = a.len().max(b.len());
                if (a.len() != b.len() && a.len() != 1 && b.len() != 1) || n == 0 {
                    return Val::Null;
                }
                let pick = |v: &[f64], i: usize| if v.len() == 1 { v[0] } else { v[i] };
                Val::List(
                    (0..n)
                        .map(|i| arithmetic(op, pick(&a, i), pick(&b, i)))
                        .collect(),
                )
            }
        },
        Op::And | Op::Or => Val::Null,
    }
}

fn vector(value: &Val) -> Option<Vec<f64>> {
    match value {
        Val::Num(number) => Some(vec![*number]),
        other => other.numbers(),
    }
}

/// Matches `text` against a pattern where `*` is any run of characters and
/// `?` is one character.
pub fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            mark = t;
            p += 1;
        } else if let Some(position) = star {
            p = position + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn call(func: Func, args: &[Val]) -> Val {
    match (func, args) {
        (Func::Abs, [Val::Num(x)]) => Val::Num(x.abs()),
        (Func::Len, [Val::List(items)]) => Val::Num(items.len() as f64),
        (Func::Len, [Val::Str(text)]) => Val::Num(text.chars().count() as f64),
        (Func::Length, [value]) => value.numbers().map_or(Val::Null, |v| {
            Val::Num(v.iter().map(|x| x * x).sum::<f64>().sqrt())
        }),
        (Func::Distance, [a, b]) => match (a.numbers(), b.numbers()) {
            (Some(a), Some(b)) if a.len() == b.len() => Val::Num(
                a.iter()
                    .zip(&b)
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f64>()
                    .sqrt(),
            ),
            _ => Val::Null,
        },
        (Func::Min, [Val::Num(a), Val::Num(b)]) => Val::Num(a.min(*b)),
        (Func::Max, [Val::Num(a), Val::Num(b)]) => Val::Num(a.max(*b)),
        (Func::Matches, [Val::Str(text), Val::Str(pattern)]) => Val::Bool(glob(pattern, text)),
        _ => Val::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wish_ecs::World;

    struct Entity {
        id: &'static str,
        components: Value,
    }

    impl Subject for Entity {
        fn id(&self) -> &str {
            self.id
        }
        fn component(&self, name: &str) -> Option<Value> {
            self.components.get(name).cloned()
        }
    }

    fn registry() -> World {
        let mut world = World::new();
        crate::types::register_engine_types(&mut world).unwrap();
        world
    }

    fn eval_on(text: &str, entity: &Entity) -> Val {
        let world = registry();
        Expr::compile(text, world.registry()).unwrap().eval(entity)
    }

    fn player() -> Entity {
        Entity {
            id: "player",
            components: json!({
                "Transform": { "translation": [1.0, -2.0, 3.0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] },
                "Camera": { "fov_y_deg": 60.0, "near": 0.1, "far": 1000.0 }
            }),
        }
    }

    #[test]
    fn paths_operators_and_functions() {
        let p = player();
        let truths = [
            "Transform.translation.y < 0",
            "Transform.translation[2] == 3",
            "has(Camera) && !has(Model)",
            "id == \"player\"",
            "matches(id, \"pl*r\") && matches(id, 'p?ayer')",
            "distance(Transform.translation, [1, -2, 0]) == 3",
            "length([3, 4]) == 5",
            "abs(Transform.translation.y) == 2 && min(1, 2) == 1 && max(1, 2) == 2",
            "Camera.fov_y_deg / 2 + 1 == 31",
            "(Transform.translation + [1, 1, 1]).x == 2",
            "Transform.translation * 2 == [2, -4, 6]",
            "-Transform.translation.x == -1",
            "len(id) == 6 && len(Transform.translation) == 3",
            "\"a\" < \"b\" || false",
            "Model.asset == null || true",
        ];
        for text in truths {
            assert!(eval_on(text, &p).is_true(), "{text}");
        }
        let falsehoods = [
            "has(Model)",
            "Model.asset == \"x\"",
            "Model.asset != \"x\"",
            "id < 3",
            "1 / 0 == 1 / 0",
            "!(1 == 1)",
            "Transform.translation.y > 0 && 1 / 0",
        ];
        for text in falsehoods {
            assert!(!eval_on(text, &p).is_true(), "{text}");
        }
    }

    #[test]
    fn mistakes_have_a_column_and_a_hint() {
        let world = registry();
        let cases = [
            ("Transform.translation.y = 0", "expr_syntax", 25),
            ("Transfrom.translation.y < 0", "expr_unknown_name", 1),
            ("Transform.translaton.y < 0", "expr_unknown_name", 11),
            ("has(Camra)", "expr_unknown_name", 1),
            ("hsa(Camera)", "expr_syntax", 1),
            ("1 < 2 < 3", "expr_syntax", 7),
            ("(1 < 2", "expr_syntax", 7),
            ("\"open", "expr_syntax", 1),
            ("distance([1])", "expr_syntax", 1),
            ("", "expr_syntax", 1),
            ("a & b", "expr_syntax", 3),
        ];
        for (text, code, column) in cases {
            let problem = Expr::compile(text, world.registry()).unwrap_err();
            assert_eq!(problem.code, code, "{text}: {}", problem.message);
            assert_eq!(problem.location.column, Some(column), "{text}");
            assert!(!problem.hint.is_empty(), "{text}");
        }
        let problem = Expr::compile("Transfrom.translation.y < 0", world.registry()).unwrap_err();
        assert_eq!(problem.hint, "Did you mean 'Transform'?");
        let problem = Expr::compile("Transform.translaton.y < 0", world.registry()).unwrap_err();
        assert_eq!(problem.hint, "Did you mean 'translation'?");
        let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        assert_eq!(
            Expr::compile(&deep, world.registry()).unwrap_err().code,
            "expr_syntax"
        );
    }

    #[test]
    fn glob_patterns() {
        assert!(glob("crate_*", "crate_7"));
        assert!(glob("*", ""));
        assert!(glob("a*b*c", "aXXbYc"));
        assert!(!glob("a*b", "ac"));
        assert!(glob("?", "x"));
        assert!(!glob("?", ""));
    }
}
