//! A pragmatic subset of the expression language Cura stores in `value` fields.
//!
//! Cura evaluates these with a restricted Python interpreter inside Uranium.
//! Shale does not link Uranium (it pulls in PyQt6). This module parses and
//! evaluates the constructs that actually appear in printer definitions:
//! arithmetic, comparisons, `and`/`or`/`not`, ternary `a if c else b`, lists,
//! calls such as `min`, `round`, `math.ceil`, and the Cura helpers
//! `extruderValue`, `extruderValues`, and `resolveOrValue`.
//!
//! Not supported, and reported as formula errors so the resolver can fall back
//! to `default_value`: lambdas, list comprehensions (generator expressions
//! passed to `all()` are supported), f-strings, slices, and arbitrary Python.
//! `min`/`max` accept `key=some_list.index`, which is how adhesion type is
//! ordered. `extruderValueFromContainer` reads the value from below the user
//! overrides when the container index is greater than zero.

use crate::value::Value;

#[derive(Debug, Clone)]
pub struct FormulaError(pub String);

impl std::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FormulaError {}

impl FormulaError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Lookups the evaluator cannot resolve on its own.
///
/// The profile resolver implements this. Tests use a flat map.
pub trait FormulaHost {
    fn setting(&mut self, name: &str) -> Result<Value, FormulaError>;
    fn extruder_value(&mut self, index: i32, key: &str) -> Result<Value, FormulaError>;
    fn extruder_values(&mut self, key: &str) -> Result<Value, FormulaError>;
    fn resolve_or_value(&mut self, key: &str) -> Result<Value, FormulaError>;
    fn any_extruder_with_material(&mut self, key: &str) -> Result<Value, FormulaError>;
    fn default_extruder_position(&mut self) -> Result<Value, FormulaError>;
    /// `extruderValueFromContainer(extruder, key, container_index)`.
    ///
    /// The default ignores the container index and behaves as `extruderValue`.
    /// The profile resolver uses the index to skip user overrides.
    fn extruder_value_from_container(
        &mut self,
        index: i32,
        key: &str,
        _container_index: i32,
    ) -> Result<Value, FormulaError> {
        self.extruder_value(index, key)
    }
    /// Loop variable for `all(x == y for x in ...)`. Default ignores the binding.
    fn bind(&mut self, _name: &str, _value: Value) {}
    fn unbind(&mut self, _name: &str) {}
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Bool(bool),
    Str(String),
    Null,
    Name(String),
    List(Vec<Expr>),
    UnaryNeg(Box<Expr>),
    UnaryNot(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Call {
        func: String,
        args: Vec<Expr>,
        keywords: Vec<(String, Expr)>,
    },
    /// `('raft', 'brim').index` as a value, used as `min(..., key=....index)`.
    Attr {
        base: Box<Expr>,
        name: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `x == y for x in items`, only as the argument of `all()`.
    Comprehension {
        body: Box<Expr>,
        var: String,
        iter: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    In,
    NotIn,
}

pub fn parse(source: &str) -> Result<Expr, FormulaError> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        source,
        tokens,
        index: 0,
    };
    let expr = parser.parse_if()?;
    if !matches!(parser.peek(), Tok::Eof) {
        return Err(parser.error("unexpected input after the expression"));
    }
    Ok(expr)
}

pub fn eval(expr: &Expr, host: &mut dyn FormulaHost) -> Result<Value, FormulaError> {
    match expr {
        Expr::Num(n) => Ok(Value::Num(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Null => Ok(Value::Null),
        Expr::Name(name) => eval_name(name, host),
        Expr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval(item, host)?);
            }
            Ok(Value::List(out))
        }
        Expr::UnaryNeg(inner) => {
            let n = number(&eval(inner, host)?)?;
            Ok(Value::Num(-n))
        }
        Expr::UnaryNot(inner) => Ok(Value::Bool(!eval(inner, host)?.truthy())),
        Expr::Binary(op, lhs, rhs) => eval_binary(*op, lhs, rhs, host),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if eval(cond, host)?.truthy() {
                eval(then_branch, host)
            } else {
                eval(else_branch, host)
            }
        }
        Expr::Call {
            func,
            args,
            keywords,
        } => eval_call(func, args, keywords, host),
        Expr::Attr { name, .. } => Err(FormulaError::new(format!(
            "attribute '.{name}' is not supported on its own"
        ))),
        Expr::Index { base, index } => eval_index(base, index, host),
        Expr::Comprehension { .. } => Err(FormulaError::new(
            "a comprehension is only supported as the argument of all()",
        )),
    }
}

pub fn eval_str(source: &str, host: &mut dyn FormulaHost) -> Result<Value, FormulaError> {
    eval(&parse(source)?, host)
}

fn eval_name(name: &str, host: &mut dyn FormulaHost) -> Result<Value, FormulaError> {
    match name {
        "math.pi" | "pi" => Ok(Value::Num(std::f64::consts::PI)),
        "math.e" => Ok(Value::Num(std::f64::consts::E)),
        "math.tau" => Ok(Value::Num(std::f64::consts::TAU)),
        "math.inf" => Ok(Value::Num(f64::INFINITY)),
        _ => host.setting(name),
    }
}

fn eval_binary(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    host: &mut dyn FormulaHost,
) -> Result<Value, FormulaError> {
    if op == BinOp::And {
        let left = eval(lhs, host)?;
        return if left.truthy() {
            eval(rhs, host)
        } else {
            Ok(left)
        };
    }
    if op == BinOp::Or {
        let left = eval(lhs, host)?;
        return if left.truthy() {
            Ok(left)
        } else {
            eval(rhs, host)
        };
    }
    let left = eval(lhs, host)?;
    let right = eval(rhs, host)?;
    match op {
        BinOp::Add => add(&left, &right),
        BinOp::Sub => Ok(Value::Num(number(&left)? - number(&right)?)),
        BinOp::Mul => Ok(Value::Num(number(&left)? * number(&right)?)),
        BinOp::Div => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(FormulaError::new("division by zero"));
            }
            Ok(Value::Num(number(&left)? / divisor))
        }
        BinOp::FloorDiv => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(FormulaError::new("division by zero"));
            }
            Ok(Value::Num((number(&left)? / divisor).floor()))
        }
        BinOp::Mod => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(FormulaError::new("modulo by zero"));
            }
            Ok(Value::Num(py_mod(number(&left)?, divisor)))
        }
        BinOp::Pow => Ok(Value::Num(number(&left)?.powf(number(&right)?))),
        BinOp::Eq => Ok(Value::Bool(left.py_eq(&right))),
        BinOp::Ne => Ok(Value::Bool(!left.py_eq(&right))),
        BinOp::Lt => Ok(Value::Bool(compare(&left, &right)? < 0)),
        BinOp::Gt => Ok(Value::Bool(compare(&left, &right)? > 0)),
        BinOp::Le => Ok(Value::Bool(compare(&left, &right)? <= 0)),
        BinOp::Ge => Ok(Value::Bool(compare(&left, &right)? >= 0)),
        BinOp::In => Ok(Value::Bool(contains(&right, &left)?)),
        BinOp::NotIn => Ok(Value::Bool(!contains(&right, &left)?)),
        BinOp::And | BinOp::Or => unreachable!("logical operators are handled above"),
    }
}

fn add(left: &Value, right: &Value) -> Result<Value, FormulaError> {
    match (left, right) {
        (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
        (Value::List(a), Value::List(b)) => {
            let mut out = a.clone();
            out.extend(b.iter().cloned());
            Ok(Value::List(out))
        }
        _ => Ok(Value::Num(number(left)? + number(right)?)),
    }
}

fn compare(left: &Value, right: &Value) -> Result<i32, FormulaError> {
    match (left.as_number(), right.as_number()) {
        (Some(a), Some(b)) => Ok(if a < b {
            -1
        } else if a > b {
            1
        } else {
            0
        }),
        _ => match (left, right) {
            (Value::Str(a), Value::Str(b)) => Ok(if a < b {
                -1
            } else if a > b {
                1
            } else {
                0
            }),
            _ => Err(FormulaError::new(
                "cannot compare values of different types",
            )),
        },
    }
}

fn contains(haystack: &Value, needle: &Value) -> Result<bool, FormulaError> {
    match haystack {
        Value::List(items) => Ok(items.iter().any(|item| item.py_eq(needle))),
        Value::Str(text) => match needle {
            Value::Str(part) => Ok(text.contains(part.as_str())),
            _ => Err(FormulaError::new(
                "only strings can be tested for membership in a string",
            )),
        },
        _ => Err(FormulaError::new(
            "'in' expects a list or a string on the right",
        )),
    }
}

fn py_mod(left: f64, right: f64) -> f64 {
    let mut remainder = left % right;
    if remainder != 0.0 && remainder.signum() != right.signum() {
        remainder += right;
    }
    remainder
}

fn number(value: &Value) -> Result<f64, FormulaError> {
    value
        .as_number()
        .ok_or_else(|| FormulaError::new(format!("expected a number, found {}", describe(value))))
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Num(_) => "a number",
        Value::Bool(_) => "a boolean",
        Value::Str(_) => "a string",
        Value::List(_) => "a list",
        Value::Null => "null",
    }
}

fn eval_call(
    func: &str,
    args: &[Expr],
    keywords: &[(String, Expr)],
    host: &mut dyn FormulaHost,
) -> Result<Value, FormulaError> {
    if func == "all" && args.len() == 1 {
        if let Expr::Comprehension { body, var, iter } = &args[0] {
            return eval_all_comprehension(body, var, iter, host);
        }
    }
    if func == "min" || func == "max" {
        return eval_extreme(func, args, keywords, host);
    }
    if func == "map" {
        return eval_map(args, host);
    }
    if func == "extruderValue" {
        let index = number(&eval(nth(args, 0, func)?, host)?)?;
        let key = expect_str(&eval(nth(args, 1, func)?, host)?)?;
        return host.extruder_value(index as i32, &key);
    }
    if func == "extruderValueFromContainer" {
        let index = number(&eval(nth(args, 0, func)?, host)?)?;
        let key = expect_str(&eval(nth(args, 1, func)?, host)?)?;
        let container = number(&eval(nth(args, 2, func)?, host)?)?;
        return host.extruder_value_from_container(index as i32, &key, container as i32);
    }
    if func == "extruderValues" {
        let key = expect_str(&eval(nth(args, 0, func)?, host)?)?;
        return host.extruder_values(&key);
    }
    if func == "resolveOrValue" {
        let key = expect_str(&eval(nth(args, 0, func)?, host)?)?;
        return host.resolve_or_value(&key);
    }
    if func == "anyExtruderWithMaterial" {
        let key = expect_str(&eval(nth(args, 0, func)?, host)?)?;
        return host.any_extruder_with_material(&key);
    }
    if func == "defaultExtruderPosition" {
        return host.default_extruder_position();
    }

    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval(arg, host)?);
    }
    apply_fn(func, &values)
}

fn eval_extreme(
    func: &str,
    args: &[Expr],
    keywords: &[(String, Expr)],
    host: &mut dyn FormulaHost,
) -> Result<Value, FormulaError> {
    let mut items = Vec::new();
    if args.len() == 1 {
        match eval(&args[0], host)? {
            Value::List(list) => items = list,
            other => items.push(other),
        }
    } else {
        for arg in args {
            items.push(eval(arg, host)?);
        }
    }
    if let Some((_, key)) = keywords.iter().find(|(name, _)| name == "key") {
        let order = index_key(key, host)?;
        return extreme_by_index(&items, &order, func == "min");
    }
    if !keywords.is_empty() {
        return Err(FormulaError::new(format!(
            "{func}() only supports the key= keyword, and only as .index"
        )));
    }
    extreme(&items, func == "min")
}

fn index_key(expr: &Expr, host: &mut dyn FormulaHost) -> Result<Vec<Value>, FormulaError> {
    let Expr::Attr { base, name } = expr else {
        return Err(FormulaError::new(
            "key= expects something.index, as in key=('raft', 'brim').index",
        ));
    };
    if name != "index" {
        return Err(FormulaError::new(format!(
            "key= only supports .index, not .{name}"
        )));
    }
    match eval(base, host)? {
        Value::List(items) => Ok(items),
        Value::Str(text) => Ok(text.chars().map(|ch| Value::Str(ch.to_string())).collect()),
        _ => Err(FormulaError::new(".index expects a list or a string")),
    }
}

fn extreme_by_index(
    items: &[Value],
    order: &[Value],
    minimum: bool,
) -> Result<Value, FormulaError> {
    let rank = |item: &Value| {
        order
            .iter()
            .position(|candidate| candidate.py_eq(item))
            .unwrap_or(usize::MAX)
    };
    let chosen = items.iter().min_by_key(|item| {
        let rank = rank(item);
        if minimum {
            rank
        } else {
            usize::MAX - rank
        }
    });
    chosen
        .cloned()
        .ok_or_else(|| FormulaError::new("min()/max() of an empty list"))
}

fn eval_all_comprehension(
    body: &Expr,
    var: &str,
    iter: &Expr,
    host: &mut dyn FormulaHost,
) -> Result<Value, FormulaError> {
    let Value::List(items) = eval(iter, host)? else {
        return Err(FormulaError::new(
            "all(... for x in ...) expects a list after in",
        ));
    };
    for item in items {
        host.bind(var, item);
        let ok = eval(body, host)?.truthy();
        host.unbind(var);
        if !ok {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

fn eval_index(
    base: &Expr,
    index: &Expr,
    host: &mut dyn FormulaHost,
) -> Result<Value, FormulaError> {
    let collection = eval(base, host)?;
    let mut index = number(&eval(index, host)?)? as isize;
    match collection {
        Value::List(items) => {
            if index < 0 {
                index += items.len() as isize;
            }
            items
                .get(index as usize)
                .cloned()
                .ok_or_else(|| FormulaError::new("list index out of range"))
        }
        Value::Str(text) => {
            let chars: Vec<char> = text.chars().collect();
            if index < 0 {
                index += chars.len() as isize;
            }
            chars
                .get(index as usize)
                .map(|ch| Value::Str(ch.to_string()))
                .ok_or_else(|| FormulaError::new("string index out of range"))
        }
        _ => Err(FormulaError::new("only lists and strings can be indexed")),
    }
}

fn eval_map(args: &[Expr], host: &mut dyn FormulaHost) -> Result<Value, FormulaError> {
    let fname = match args.first() {
        Some(Expr::Name(name)) => name.as_str(),
        _ => {
            return Err(FormulaError::new(
                "map() expects a function name as its first argument",
            ))
        }
    };
    let list = eval(nth(args, 1, "map")?, host)?;
    let Value::List(items) = list else {
        return Err(FormulaError::new(
            "map() expects a list as its second argument",
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(apply_fn(fname, &[item])?);
    }
    Ok(Value::List(out))
}

fn nth<'a>(args: &'a [Expr], index: usize, func: &str) -> Result<&'a Expr, FormulaError> {
    args.get(index).ok_or_else(|| {
        FormulaError::new(format!(
            "{func}() expects at least {} argument(s)",
            index + 1
        ))
    })
}

fn expect_str(value: &Value) -> Result<String, FormulaError> {
    match value {
        Value::Str(text) => Ok(text.clone()),
        _ => Err(FormulaError::new("expected a string argument")),
    }
}

fn apply_fn(func: &str, args: &[Value]) -> Result<Value, FormulaError> {
    match func {
        "min" => extreme(args, true),
        "max" => extreme(args, false),
        "abs" | "math.fabs" => Ok(Value::Num(number(first(args, func)?)?.abs())),
        "int" => {
            let n = number(first(args, func)?)?;
            Ok(Value::Num(n.trunc()))
        }
        "float" => float_of(first(args, func)?),
        "str" => Ok(Value::Str(value_to_py_string(first(args, func)?))),
        "round" => {
            let n = number(first(args, func)?)?;
            let digits = match args.get(1) {
                Some(value) => number(value)? as i32,
                None => 0,
            };
            let scale = 10f64.powi(digits);
            Ok(Value::Num((n * scale).round() / scale))
        }
        "math.floor" => Ok(Value::Num(number(first(args, func)?)?.floor())),
        "math.ceil" => Ok(Value::Num(number(first(args, func)?)?.ceil())),
        "math.sqrt" => Ok(Value::Num(number(first(args, func)?)?.sqrt())),
        "math.sin" => Ok(Value::Num(number(first(args, func)?)?.sin())),
        "math.cos" => Ok(Value::Num(number(first(args, func)?)?.cos())),
        "math.tan" => Ok(Value::Num(number(first(args, func)?)?.tan())),
        "math.atan" => Ok(Value::Num(number(first(args, func)?)?.atan())),
        "math.atan2" => {
            let y = number(first(args, func)?)?;
            let x = number(
                args.get(1)
                    .ok_or_else(|| FormulaError::new("math.atan2() expects two arguments"))?,
            )?;
            Ok(Value::Num(y.atan2(x)))
        }
        "math.radians" => Ok(Value::Num(number(first(args, func)?)?.to_radians())),
        "math.degrees" => Ok(Value::Num(number(first(args, func)?)?.to_degrees())),
        "math.log" => {
            let value = number(first(args, func)?)?;
            let base = match args.get(1) {
                Some(base) => number(base)?,
                None => std::f64::consts::E,
            };
            if value <= 0.0 || base <= 0.0 || base == 1.0 {
                return Err(FormulaError::new("math.log() domain error"));
            }
            Ok(Value::Num(value.log(base)))
        }
        "math.log10" => Ok(Value::Num(number(first(args, func)?)?.log10())),
        "bool" => Ok(Value::Bool(first(args, func)?.truthy())),
        "len" => len_of(first(args, func)?),
        "sum" => {
            let mut total = 0.0;
            for item in flatten(args) {
                total += number(&item)?;
            }
            Ok(Value::Num(total))
        }
        "any" => Ok(Value::Bool(
            flatten(args).into_iter().any(|item| item.truthy()),
        )),
        "all" => Ok(Value::Bool(
            flatten(args).into_iter().all(|item| item.truthy()),
        )),
        other => Err(FormulaError::new(format!("unsupported function '{other}'"))),
    }
}

fn float_of(value: &Value) -> Result<Value, FormulaError> {
    match value {
        Value::Str(text) if text == "inf" || text == "+inf" || text == "Infinity" => {
            Ok(Value::Num(f64::INFINITY))
        }
        Value::Str(text) if text == "-inf" || text == "-Infinity" => {
            Ok(Value::Num(f64::NEG_INFINITY))
        }
        Value::Str(text) => text
            .parse::<f64>()
            .map(Value::Num)
            .map_err(|_| FormulaError::new(format!("float() cannot parse '{text}'"))),
        other => Ok(Value::Num(number(other)?)),
    }
}

fn first<'a>(args: &'a [Value], func: &str) -> Result<&'a Value, FormulaError> {
    args.first()
        .ok_or_else(|| FormulaError::new(format!("{func}() expects an argument")))
}

fn len_of(value: &Value) -> Result<Value, FormulaError> {
    match value {
        Value::List(items) => Ok(Value::Num(items.len() as f64)),
        Value::Str(text) => Ok(Value::Num(text.chars().count() as f64)),
        _ => Err(FormulaError::new("len() expects a list or a string")),
    }
}

fn flatten(args: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Value::List(items) => out.extend(items.iter().cloned()),
            other => out.push(other.clone()),
        }
    }
    out
}

fn extreme(args: &[Value], want_min: bool) -> Result<Value, FormulaError> {
    let items = flatten(args);
    let mut best: Option<Value> = None;
    for item in items {
        best = Some(match best {
            None => item,
            Some(current) => {
                let order = compare(&item, &current)?;
                if want_min {
                    if order < 0 {
                        item
                    } else {
                        current
                    }
                } else if order > 0 {
                    item
                } else {
                    current
                }
            }
        });
    }
    best.ok_or_else(|| FormulaError::new("min/max() of an empty sequence"))
}

fn value_to_py_string(value: &Value) -> String {
    match value {
        Value::Num(n) => {
            if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.1}")
            } else {
                let text = format!("{n:.6}");
                trim_float(&text)
            }
        }
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Str(text) => text.clone(),
        Value::Null => "None".to_string(),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(value_to_py_string).collect();
            format!("[{}]", parts.join(", "))
        }
    }
}

fn trim_float(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    True,
    False,
    NoneKw,
    If,
    Else,
    And,
    Or,
    Not,
    In,
    Plus,
    Minus,
    Star,
    Slash,
    SlashSlash,
    Percent,
    StarStar,
    Eq,
    Assign,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Dot,
    Eof,
}

fn lex(source: &str) -> Result<Vec<Tok>, FormulaError> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut tokens = Vec::new();
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch == '#' {
            return Err(FormulaError::new("comments are not supported in formulas"));
        }
        let start = index;
        let token = match ch {
            '+' => {
                index += 1;
                Tok::Plus
            }
            '-' => {
                index += 1;
                Tok::Minus
            }
            '%' => {
                index += 1;
                Tok::Percent
            }
            '(' => {
                index += 1;
                Tok::LParen
            }
            ')' => {
                index += 1;
                Tok::RParen
            }
            '[' => {
                index += 1;
                Tok::LBrack
            }
            ']' => {
                index += 1;
                Tok::RBrack
            }
            ',' => {
                index += 1;
                Tok::Comma
            }
            '*' => {
                if bytes.get(index + 1) == Some(&b'*') {
                    index += 2;
                    Tok::StarStar
                } else {
                    index += 1;
                    Tok::Star
                }
            }
            '/' => {
                if bytes.get(index + 1) == Some(&b'/') {
                    index += 2;
                    Tok::SlashSlash
                } else {
                    index += 1;
                    Tok::Slash
                }
            }
            '=' => {
                if bytes.get(index + 1) == Some(&b'=') {
                    index += 2;
                    Tok::Eq
                } else {
                    index += 1;
                    Tok::Assign
                }
            }
            '!' => {
                if bytes.get(index + 1) == Some(&b'=') {
                    index += 2;
                    Tok::Ne
                } else {
                    return Err(snippet(source, start, "expected '!='"));
                }
            }
            '<' => {
                if bytes.get(index + 1) == Some(&b'=') {
                    index += 2;
                    Tok::Le
                } else {
                    index += 1;
                    Tok::Lt
                }
            }
            '>' => {
                if bytes.get(index + 1) == Some(&b'=') {
                    index += 2;
                    Tok::Ge
                } else {
                    index += 1;
                    Tok::Gt
                }
            }
            '\'' | '"' => {
                let quote = bytes[index];
                index += 1;
                let mut text = String::new();
                let mut closed = false;
                while index < bytes.len() {
                    let current = bytes[index];
                    if current == b'\\' {
                        let next = *bytes.get(index + 1).ok_or_else(|| {
                            snippet(source, index, "unterminated escape in string")
                        })?;
                        text.push(match next {
                            b'n' => '\n',
                            b't' => '\t',
                            b'\\' => '\\',
                            b'\'' => '\'',
                            b'"' => '"',
                            other => other as char,
                        });
                        index += 2;
                        continue;
                    }
                    if current == quote {
                        index += 1;
                        closed = true;
                        break;
                    }
                    text.push(current as char);
                    index += 1;
                }
                if !closed {
                    return Err(snippet(source, start, "unterminated string"));
                }
                Tok::Str(text)
            }
            _ if ch.is_ascii_digit()
                || (ch == '.' && bytes.get(index + 1).is_some_and(|d| d.is_ascii_digit())) =>
            {
                Tok::Num(lex_number(source, bytes, &mut index)?)
            }
            _ if ch == '.' => {
                index += 1;
                Tok::Dot
            }
            _ if ch.is_ascii_alphabetic() || ch == '_' => {
                Tok::from_ident(lex_ident(bytes, &mut index))
            }
            _ => {
                return Err(snippet(
                    source,
                    start,
                    &format!("unexpected character '{ch}'"),
                ))
            }
        };
        tokens.push(token);
    }
    tokens.push(Tok::Eof);
    Ok(tokens)
}

fn lex_number(source: &str, bytes: &[u8], index: &mut usize) -> Result<f64, FormulaError> {
    let start = *index;
    while *index < bytes.len() && bytes[*index].is_ascii_digit() {
        *index += 1;
    }
    if bytes.get(*index) == Some(&b'.') && bytes.get(*index + 1).is_none_or(|d| d.is_ascii_digit())
    {
        *index += 1;
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            *index += 1;
        }
    }
    if matches!(bytes.get(*index), Some(b'e' | b'E')) {
        let mut end = *index + 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        if bytes.get(end).is_some_and(|d| d.is_ascii_digit()) {
            *index = end;
            while *index < bytes.len() && bytes[*index].is_ascii_digit() {
                *index += 1;
            }
        }
    }
    source[start..*index]
        .parse::<f64>()
        .map_err(|_| snippet(source, start, "invalid number"))
}

fn lex_ident(bytes: &[u8], index: &mut usize) -> String {
    let start = *index;
    *index += 1;
    while *index < bytes.len() {
        let ch = bytes[*index];
        if ch.is_ascii_alphanumeric() || ch == b'_' {
            *index += 1;
            continue;
        }
        // `math.ceil` is one name. A dot after `)` is an attribute, not part of an ident.
        if ch == b'.' {
            let next = bytes.get(*index + 1).copied().unwrap_or(0);
            if next.is_ascii_alphabetic() || next == b'_' {
                *index += 1;
                continue;
            }
        }
        break;
    }
    String::from_utf8_lossy(&bytes[start..*index]).into_owned()
}

impl Tok {
    fn from_ident(ident: String) -> Self {
        match ident.as_str() {
            "if" => Self::If,
            "else" => Self::Else,
            "and" => Self::And,
            "or" => Self::Or,
            "not" => Self::Not,
            "in" => Self::In,
            "True" => Self::True,
            "False" => Self::False,
            "None" => Self::NoneKw,
            _ => Self::Ident(ident),
        }
    }
}

fn snippet(source: &str, index: usize, message: &str) -> FormulaError {
    let end = (index + 24).min(source.len());
    let start = index.saturating_sub(16);
    FormulaError::new(format!(
        "{message} near `{}`",
        source.get(start..end).unwrap_or("")
    ))
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Tok>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn parse_if(&mut self) -> Result<Expr, FormulaError> {
        let value = self.parse_or()?;
        if self.eat_if() {
            let cond = self.parse_or()?;
            self.expect_else()?;
            let else_branch = self.parse_if()?;
            return Ok(Expr::If {
                cond: Box::new(cond),
                then_branch: Box::new(value),
                else_branch: Box::new(else_branch),
            });
        }
        Ok(value)
    }

    fn parse_or(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_and()?;
        while self.eat_or() {
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_not()?;
        while self.eat_and() {
            let right = self.parse_not()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, FormulaError> {
        if self.eat_not() {
            let inner = self.parse_not()?;
            return Ok(Expr::UnaryNot(Box::new(inner)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_sum()?;
        while let Some(op) = self.eat_comparison() {
            let right = self.parse_sum()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_sum(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_term()?;
        loop {
            let op = if self.eat_plus() {
                BinOp::Add
            } else if self.eat_minus() {
                BinOp::Sub
            } else {
                break;
            };
            let right = self.parse_term()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.parse_factor()?;
        loop {
            let op = if self.eat_star() {
                BinOp::Mul
            } else if self.eat_slashslash() {
                BinOp::FloorDiv
            } else if self.eat_slash() {
                BinOp::Div
            } else if self.eat_percent() {
                BinOp::Mod
            } else {
                break;
            };
            let right = self.parse_factor()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, FormulaError> {
        if self.eat_minus() {
            let inner = self.parse_factor()?;
            return Ok(Expr::UnaryNeg(Box::new(inner)));
        }
        if self.eat_plus() {
            return self.parse_factor();
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr, FormulaError> {
        let base = self.parse_primary()?;
        if self.eat_starstar() {
            let exp = self.parse_factor()?;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_atom()?;
        loop {
            if matches!(self.peek(), Tok::LParen) {
                let (args, keywords) = self.parse_args()?;
                let func = match expr {
                    Expr::Name(name) => name,
                    _ => {
                        return Err(self.error("only named functions can be called"));
                    }
                };
                expr = Expr::Call {
                    func,
                    args,
                    keywords,
                };
            } else if self.eat_dot() {
                let name = self.expect_ident()?;
                expr = Expr::Attr {
                    base: Box::new(expr),
                    name,
                };
            } else if matches!(self.peek(), Tok::LBrack) {
                self.bump();
                let index = self.parse_if()?;
                if !matches!(self.bump(), Some(Tok::RBrack)) {
                    return Err(self.error("expected ']'"));
                }
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_atom(&mut self) -> Result<Expr, FormulaError> {
        match self.bump() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Ident(name)) => Ok(Expr::Name(name)),
            Some(Tok::True) => Ok(Expr::Bool(true)),
            Some(Tok::False) => Ok(Expr::Bool(false)),
            Some(Tok::NoneKw) => Ok(Expr::Null),
            Some(Tok::LBrack) => self.parse_list(),
            Some(Tok::LParen) => self.parse_paren(),
            _ => Err(self.error("expected a value")),
        }
    }

    fn parse_list(&mut self) -> Result<Expr, FormulaError> {
        let mut items = Vec::new();
        if matches!(self.peek(), Tok::RBrack) {
            self.bump();
            return Ok(Expr::List(items));
        }
        loop {
            items.push(self.parse_if()?);
            if self.eat_comma() {
                if matches!(self.peek(), Tok::RBrack) {
                    break;
                }
                continue;
            }
            break;
        }
        if !matches!(self.bump(), Some(Tok::RBrack)) {
            return Err(self.error("expected ']'"));
        }
        Ok(Expr::List(items))
    }

    fn parse_paren(&mut self) -> Result<Expr, FormulaError> {
        if matches!(self.peek(), Tok::RParen) {
            self.bump();
            return Ok(Expr::List(Vec::new()));
        }
        let first = self.parse_if()?;
        if self.eat_comma() {
            let mut items = vec![first];
            if !matches!(self.peek(), Tok::RParen) {
                loop {
                    items.push(self.parse_if()?);
                    if self.eat_comma() {
                        if matches!(self.peek(), Tok::RParen) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            if !matches!(self.bump(), Some(Tok::RParen)) {
                return Err(self.error("expected ')'"));
            }
            return Ok(Expr::List(items));
        }
        if !matches!(self.bump(), Some(Tok::RParen)) {
            return Err(self.error("expected ')'"));
        }
        Ok(first)
    }

    fn parse_args(&mut self) -> Result<(Vec<Expr>, Vec<(String, Expr)>), FormulaError> {
        if !matches!(self.bump(), Some(Tok::LParen)) {
            return Err(self.error("expected '('"));
        }
        let mut positional = Vec::new();
        let mut keywords = Vec::new();
        if matches!(self.peek(), Tok::RParen) {
            self.bump();
            return Ok((positional, keywords));
        }
        loop {
            if self.keyword_ahead() {
                let name = self.expect_ident()?;
                if !matches!(self.bump(), Some(Tok::Assign)) {
                    return Err(self.error("expected '=' after a keyword argument"));
                }
                keywords.push((name, self.parse_if()?));
            } else {
                if !keywords.is_empty() {
                    return Err(self.error("positional argument follows a keyword argument"));
                }
                positional.push(self.parse_if()?);
                if positional.len() == 1 && self.eat_word("for") {
                    let var = self.expect_ident()?;
                    if !matches!(self.peek(), Tok::In) {
                        return Err(self.error("expected 'in' in a comprehension"));
                    }
                    self.bump();
                    let iter = self.parse_if()?;
                    if !matches!(self.bump(), Some(Tok::RParen)) {
                        return Err(self.error("expected ')'"));
                    }
                    let body = positional.pop().unwrap();
                    return Ok((
                        vec![Expr::Comprehension {
                            body: Box::new(body),
                            var,
                            iter: Box::new(iter),
                        }],
                        Vec::new(),
                    ));
                }
            }
            if self.eat_comma() {
                if matches!(self.peek(), Tok::RParen) {
                    break;
                }
                continue;
            }
            break;
        }
        if !matches!(self.bump(), Some(Tok::RParen)) {
            return Err(self.error("expected ')'"));
        }
        Ok((positional, keywords))
    }

    fn keyword_ahead(&self) -> bool {
        matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Assign)
    }

    fn peek(&self) -> &Tok {
        self.tokens.get(self.index).unwrap_or(&Tok::Eof)
    }

    fn peek_at(&self, offset: usize) -> &Tok {
        self.tokens.get(self.index + offset).unwrap_or(&Tok::Eof)
    }

    fn bump(&mut self) -> Option<Tok> {
        if self.index >= self.tokens.len() {
            return None;
        }
        let token = std::mem::replace(&mut self.tokens[self.index], Tok::Eof);
        self.index += 1;
        Some(token)
    }

    fn eat_if(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::If))
    }
    fn eat_or(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Or))
    }
    fn eat_and(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::And))
    }
    fn eat_not(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Not))
    }
    fn eat_plus(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Plus))
    }
    fn eat_minus(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Minus))
    }
    fn eat_star(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Star))
    }
    fn eat_starstar(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::StarStar))
    }
    fn eat_slash(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Slash))
    }
    fn eat_slashslash(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::SlashSlash))
    }
    fn eat_percent(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Percent))
    }
    fn eat_comma(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Comma))
    }
    fn eat_dot(&mut self) -> bool {
        self.eat(|tok| matches!(tok, Tok::Dot))
    }

    fn eat_comparison(&mut self) -> Option<BinOp> {
        let op = match self.peek() {
            Tok::Eq => BinOp::Eq,
            Tok::Ne => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Gt => BinOp::Gt,
            Tok::Le => BinOp::Le,
            Tok::Ge => BinOp::Ge,
            Tok::In => BinOp::In,
            Tok::Not if matches!(self.peek_at(1), Tok::In) => {
                self.index += 2;
                return Some(BinOp::NotIn);
            }
            _ => return None,
        };
        self.index += 1;
        Some(op)
    }

    fn eat(&mut self, pred: impl Fn(&Tok) -> bool) -> bool {
        if pred(self.peek()) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect_else(&mut self) -> Result<(), FormulaError> {
        if self.eat(|tok| matches!(tok, Tok::Else)) {
            Ok(())
        } else {
            Err(self.error("expected 'else'"))
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if let Tok::Ident(name) = self.peek() {
            if name == word {
                self.index += 1;
                return true;
            }
        }
        false
    }

    fn expect_ident(&mut self) -> Result<String, FormulaError> {
        match self.peek() {
            Tok::Ident(name) => {
                let name = name.clone();
                self.index += 1;
                Ok(name)
            }
            _ => Err(self.error("expected a name")),
        }
    }

    fn error(&self, message: &str) -> FormulaError {
        snippet(self.source, 0, message)
    }
}

#[cfg(test)]
fn walk_formulas(dir: &std::path::Path, visit: &mut dyn FnMut(&str, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_formulas(&path, visit);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.ends_with(".def.json") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    collect_json_formulas(&json, &path.display().to_string(), visit);
                }
            }
        } else if name.ends_with(".inst.cfg") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                collect_cfg_formulas(&text, &path.display().to_string(), visit);
            }
        }
    }
}

#[cfg(test)]
fn collect_json_formulas(
    json: &serde_json::Value,
    origin: &str,
    visit: &mut dyn FnMut(&str, &str),
) {
    let Some(object) = json.as_object() else {
        if let Some(items) = json.as_array() {
            for item in items {
                collect_json_formulas(item, origin, visit);
            }
        }
        return;
    };
    const KEYS: &[&str] = &[
        "value",
        "resolve",
        "minimum_value",
        "maximum_value",
        "minimum_value_warning",
        "maximum_value_warning",
        "enabled",
    ];
    for key in KEYS {
        if let Some(source) = object.get(*key).and_then(|value| value.as_str()) {
            let source = source.trim();
            if !source.is_empty() {
                visit(origin, source);
            }
        }
    }
    for (key, value) in object {
        if KEYS.contains(&key.as_str()) {
            continue;
        }
        collect_json_formulas(value, origin, visit);
    }
}

#[cfg(test)]
fn collect_cfg_formulas(text: &str, origin: &str, visit: &mut dyn FnMut(&str, &str)) {
    let mut in_values = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_values = line == "[values]";
            continue;
        }
        if !in_values || line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let Some((_, raw)) = line.split_once('=') else {
            continue;
        };
        let raw = raw.trim();
        let Some(formula) = raw.strip_prefix('=') else {
            continue;
        };
        let formula = formula.trim();
        if !formula.is_empty() {
            visit(origin, formula);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapHost {
        values: HashMap<String, Value>,
        bindings: HashMap<String, Value>,
    }

    impl FormulaHost for MapHost {
        fn setting(&mut self, name: &str) -> Result<Value, FormulaError> {
            if let Some(value) = self.bindings.get(name).cloned() {
                return Ok(value);
            }
            self.values
                .get(name)
                .cloned()
                .ok_or_else(|| FormulaError::new(format!("unknown setting {name}")))
        }

        fn bind(&mut self, name: &str, value: Value) {
            self.bindings.insert(name.to_string(), value);
        }

        fn unbind(&mut self, name: &str) {
            self.bindings.remove(name);
        }
        fn extruder_value(&mut self, _: i32, key: &str) -> Result<Value, FormulaError> {
            self.setting(key)
        }
        fn extruder_values(&mut self, key: &str) -> Result<Value, FormulaError> {
            Ok(Value::List(vec![self.setting(key)?]))
        }
        fn resolve_or_value(&mut self, key: &str) -> Result<Value, FormulaError> {
            self.setting(key)
        }
        fn any_extruder_with_material(&mut self, _: &str) -> Result<Value, FormulaError> {
            Ok(Value::Num(-1.0))
        }
        fn default_extruder_position(&mut self) -> Result<Value, FormulaError> {
            Ok(Value::Num(0.0))
        }
    }

    fn eval_with(source: &str, values: &[(&str, Value)]) -> Value {
        let mut host = MapHost {
            values: values
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
            bindings: HashMap::new(),
        };
        eval_str(source, &mut host).unwrap_or_else(|err| panic!("{source}: {err}"))
    }

    #[test]
    fn arithmetic_comparison_and_ternary() {
        let values = [
            ("speed_print", Value::Num(50.0)),
            ("layer_height", Value::Num(0.2)),
            ("line_width", Value::Num(0.4)),
        ];
        assert_eq!(eval_with("-2 ** 2", &values), Value::Num(-4.0));
        assert_eq!(
            eval_with(
                "150.0 if speed_print < 60 else 250.0 if speed_print > 100 else speed_print * 2.5",
                &values
            ),
            Value::Num(150.0)
        );
        assert_eq!(
            eval_with(
                "math.floor(math.degrees(math.atan(line_width / 2 / layer_height)))",
                &values
            ),
            Value::Num(45.0)
        );
    }

    #[test]
    fn membership_lists_and_helpers() {
        let values = [
            ("infill_pattern", Value::Str("cubic".into())),
            ("infill_sparse_density", Value::Num(20.0)),
            ("line_width", Value::Num(0.4)),
            ("support_enable", Value::Bool(true)),
        ];
        assert_eq!(
            eval_with(
                "'lines' if infill_sparse_density > 50 else 'cubic'",
                &values
            ),
            Value::Str("cubic".into())
        );
        assert_eq!(
            eval_with("infill_pattern in ['honeycomb', 'cubic', 'grid']", &values),
            Value::Bool(true)
        );
        assert_eq!(
            eval_with(
                "0 if infill_sparse_density == 0 else (line_width * 100) / infill_sparse_density",
                &values
            ),
            Value::Num(2.0)
        );
        assert_eq!(
            eval_with("any(extruderValues('support_enable'))", &values),
            Value::Bool(true)
        );
        assert_eq!(
            eval_with(
                "int(anyExtruderWithMaterial('material_is_support_material'))",
                &values
            ),
            Value::Num(-1.0)
        );
        assert_eq!(
            eval_with(
                "min(extruderValues('line_width'), key=('a', 'b').index)",
                &values
            ),
            Value::Num(0.4)
        );
        assert_eq!(eval_with("round(1.2345, 2)", &values), Value::Num(1.23));
        assert_eq!(eval_with("not False and True", &values), Value::Bool(true));
    }

    #[test]
    fn log_index_keyed_min_and_all_comprehension() {
        assert_eq!(eval_with("math.log(8) / math.log(2)", &[]), Value::Num(3.0));
        assert_eq!(
            eval_with(
                "min(['skirt', 'raft'], key=('raft', 'brim', 'skirt', 'none').index)",
                &[]
            ),
            Value::Str("raft".into())
        );
        assert_eq!(
            eval_with("['pla', 'petg'][0]", &[]),
            Value::Str("pla".into())
        );
        assert_eq!(
            eval_with(
                "all(mode == 'interleaved' for mode in ['interleaved', 'interleaved'])",
                &[]
            ),
            Value::Bool(true)
        );
        assert_eq!(
            eval_with(
                "all(mode == 'interleaved' for mode in ['interleaved', 'normal'])",
                &[]
            ),
            Value::Bool(false)
        );
        assert_eq!(eval_with("bool(0)", &[]), Value::Bool(false));
        assert_eq!(eval_with("bool('x')", &[]), Value::Bool(true));
        assert_eq!(
            eval_with(
                "extruderValueFromContainer(0, 'bottom_layers', 2)",
                &[("bottom_layers", Value::Num(4.0))]
            ),
            Value::Num(4.0)
        );
        assert_eq!(eval_with("math.pi > 3", &[]), Value::Bool(true));
        parse(
            "'interleaved' if all(mode == 'interleaved' for mode in extruderValues('prime_tower_mode')) else 'interleaved' if (all(material_type_var == extruderValues('material_type')[0] for material_type_var in extruderValues('material_type')) and all(material_brand_var == extruderValues('material_brand')[0] for material_brand_var in extruderValues('material_brand'))) else 'normal'",
        )
        .expect("prime_tower_mode resolve");
        parse("all(p != 'surface' for p in extruderValues('magic_mesh_surface_mode'))")
            .expect("surface-mode comprehension");
        parse("2 if extruderValueFromContainer(top_bottom_extruder_nr, 'bottom_layers', 2) == bottom_layers else bottom_layers")
            .expect("s8 initial_bottom_layers");
    }

    #[test]
    fn every_definition_formula_parses_when_resources_are_present() {
        let Ok(root) = std::env::var("SHALE_CURA_DIR") else {
            return;
        };
        let resources = std::path::Path::new(&root).join("resources");
        let root = if resources.is_dir() {
            resources
        } else {
            std::path::PathBuf::from(root)
        };
        let mut failures = Vec::new();
        let mut parsed = 0usize;
        walk_formulas(&root, &mut |origin, source| {
            parsed += 1;
            if let Err(err) = parse(source) {
                if failures.len() < 30 {
                    failures.push(format!(
                        "{origin}: {err} :: {}",
                        source.chars().take(160).collect::<String>()
                    ));
                }
            }
        });
        assert!(
            parsed > 1000,
            "only parsed {parsed} formulas under {}",
            root.display()
        );
        assert!(
            failures.is_empty(),
            "formula parse failures ({parsed} checked):\n{}",
            failures.join("\n")
        );
    }
}
