//! Fill in the placeholders Bambu Studio leaves in its machine G-code.
//!
//! Bambu's machine profiles do not store a start or an end sequence, they store
//! a *template*: `{nozzle_temperature_initial_layer[initial_extruder]}`,
//! `{max_layer_z + 0.5}`, and `{if curr_bed_type=="Textured PEI Plate"} …
//! {endif}` blocks. Handing that text to CuraEngine unchanged would print the
//! braces, so it is resolved here, once, against the values the slice actually
//! resolved.
//!
//! Two rules matter more than the grammar:
//!
//! * a name that is not known is an error naming the name, never an empty
//!   string — a machine that heats to nothing is worse than a slice that
//!   refuses;
//! * when the text is finished, no `{` may be left. `render` checks that
//!   itself, so a template that grows a new placeholder fails loudly instead of
//!   reaching a printer.

use std::collections::HashMap;

use crate::error::Error;

/// A value the template can read: a number, some text, a flag, or a list of any
/// of those (one entry per extruder).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Text(String),
    Flag(bool),
    List(Vec<Value>),
}

impl Value {
    /// How the value is written into G-code. Numbers lose their trailing zeros:
    /// `10.5` stays `10.5`, `200.0` becomes `200`, because a firmware that is
    /// handed `M104 S200.0` is being asked a slightly different question.
    pub fn render(&self) -> String {
        match self {
            Value::Num(number) => {
                if !number.is_finite() {
                    return "0".to_string();
                }
                let mut text = format!("{number:.4}");
                while text.ends_with('0') {
                    text.pop();
                }
                if text.ends_with('.') {
                    text.pop();
                }
                if text == "-0" {
                    text = "0".to_string();
                }
                text
            }
            Value::Text(text) => text.clone(),
            Value::Flag(flag) => if *flag { "true" } else { "false" }.to_string(),
            Value::List(items) => items.first().map(Value::render).unwrap_or_default(),
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Value::Num(number) => Some(*number),
            Value::Text(text) => text.trim().parse::<f64>().ok(),
            Value::Flag(flag) => Some(if *flag { 1.0 } else { 0.0 }),
            Value::List(items) => items.first().and_then(Value::as_number),
        }
    }

    fn as_flag(&self) -> bool {
        match self {
            Value::Num(number) => *number != 0.0,
            Value::Text(text) => !text.is_empty() && text != "0" && text != "false",
            Value::Flag(flag) => *flag,
            Value::List(items) => items.first().is_some_and(Value::as_flag),
        }
    }

    fn same_as(&self, other: &Value) -> bool {
        if let (Some(left), Some(right)) = (self.as_number(), other.as_number()) {
            return (left - right).abs() < 1e-9;
        }
        match (self, other) {
            (Value::Text(left), Value::Text(right)) => left == right,
            (Value::Flag(left), Value::Flag(right)) => left == right,
            _ => self.render() == other.render(),
        }
    }
}

/// The names a template may use.
#[derive(Clone, Debug, Default)]
pub struct Values {
    names: HashMap<String, Value>,
}

/// How a setting written as text becomes a value: a number when it reads as
/// one, a flag when it is a Cura boolean, text otherwise.
pub fn value_of(text: &str) -> Value {
    match text.trim() {
        "True" | "true" => return Value::Flag(true),
        "False" | "false" => return Value::Flag(false),
        other => {
            if let Ok(number) = other.parse::<f64>() {
                return Value::Num(number);
            }
        }
    }
    Value::Text(text.trim().to_string())
}

impl Values {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a name. Settings arrive as text; a numeric string becomes
    /// a number so that `{layer_height * 2}` is arithmetic rather than
    /// concatenation.
    pub fn set(&mut self, name: &str, value: impl Into<Value>) {
        self.names.insert(name.to_string(), value.into());
    }

    /// Read a setting by name, as the resolver left it.
    pub fn set_setting(&mut self, name: &str, value: &str) {
        let trimmed = value.trim();
        if let Ok(number) = trimmed.parse::<f64>() {
            if trimmed.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) {
                self.set(name, Value::Num(number));
                return;
            }
        }
        match trimmed {
            "True" | "true" => self.set(name, Value::Flag(true)),
            "False" | "false" => self.set(name, Value::Flag(false)),
            _ => self.set(name, Value::Text(trimmed.to_string())),
        }
    }

    pub fn set_list(&mut self, name: &str, items: Vec<Value>) {
        self.names.insert(name.to_string(), Value::List(items));
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.names.get(name)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Num(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Num(value as f64)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Text(value.to_string())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Text(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Flag(value)
    }
}

/// Fill in every placeholder in `template`.
pub fn render(template: &str, values: &Values) -> Result<String, Error> {
    let mut out = String::with_capacity(template.len() + 256);
    // One entry per open `{if}`: whether this branch is being written, and
    // whether any branch of it already was.
    let mut stack: Vec<Branch> = Vec::new();
    for piece in split(template)? {
        let active = stack.iter().all(|branch| branch.active);
        match piece {
            Piece::Text(text) => {
                if active {
                    out.push_str(&text);
                }
            }
            Piece::Expression(expression) => {
                if active {
                    out.push_str(&eval(&expression, values)?.render());
                }
            }
            Piece::If(condition) => {
                let holds = active && eval(&condition, values)?.as_flag();
                stack.push(Branch {
                    active: holds,
                    taken: holds,
                });
            }
            Piece::Else => {
                if stack.is_empty() {
                    return Err(Error::message(
                        "the machine G-code template has an `{else}` outside any `{if}`",
                    ));
                }
                let parent = stack[..stack.len() - 1].iter().all(|item| item.active);
                let branch = stack.last_mut().expect("checked above");
                branch.active = parent && !branch.taken;
                branch.taken = true;
            }
            Piece::Endif => {
                if stack.pop().is_none() {
                    return Err(Error::message(
                        "the machine G-code template closes an `{if}` that was never opened",
                    ));
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err(Error::message(
            "the machine G-code template has an `{if}` with no `{endif}`",
        ));
    }
    if out.contains('{') {
        return Err(Error::message(
            "the machine G-code template still has a placeholder in it after filling",
        ));
    }
    Ok(out)
}

struct Branch {
    active: bool,
    taken: bool,
}

/// A template cut into the four things it can contain.
enum Piece {
    Text(String),
    Expression(String),
    If(String),
    Else,
    Endif,
}

/// Cut the template at its braces, filling `[name]` reads on the way through.
fn split(template: &str) -> Result<Vec<Piece>, Error> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut at = 0usize;
    while at < template.len() {
        let rest = &template[at..];
        let ch = rest.chars().next().expect("a char");
        match ch {
            '{' => {
                let Some(close) = find_close(template, at) else {
                    return Err(Error::message(format!(
                        "the machine G-code template has an unclosed `{{` at byte {at}"
                    )));
                };
                if !text.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                }
                let body = template[at + 1..close].trim().to_string();
                pieces.push(classify(&body));
                at = close + 1;
            }
            '[' => {
                // `H[nozzle_diameter]`: Bambu's second spelling for a value.
                let filled = find_bracket(template, at)
                    .map(|close| (close, template[at + 1..close].to_string()))
                    .filter(|(_, name)| is_name(name));
                match filled {
                    Some((close, name)) => {
                        if !text.is_empty() {
                            pieces.push(Piece::Text(std::mem::take(&mut text)));
                        }
                        pieces.push(Piece::Expression(name));
                        at = close + 1;
                    }
                    None => {
                        text.push(ch);
                        at += 1;
                    }
                }
            }
            _ => {
                text.push(ch);
                at += ch.len_utf8();
            }
        }
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

fn classify(body: &str) -> Piece {
    if let Some(condition) = body.strip_prefix("if ") {
        Piece::If(condition.trim().to_string())
    } else if body == "endif" {
        Piece::Endif
    } else if body == "else" {
        Piece::Else
    } else {
        Piece::Expression(body.to_string())
    }
}

fn find_close(template: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in template[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_bracket(template: &str, open: usize) -> Option<usize> {
    template[open..].find(']').map(|offset| open + offset)
}

fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Evaluate one expression: arithmetic, comparisons, and the two logical
/// operators Bambu's templates use.
fn eval(expression: &str, values: &Values) -> Result<Value, Error> {
    let tokens = tokenize(expression)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        values,
        expression,
    };
    let value = parser.or_expression()?;
    if parser.at != parser.tokens.len() {
        return Err(parser.error("could not read the rest of the expression"));
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Text(String),
    Name(String),
    Open,
    Close,
    OpenIndex,
    CloseIndex,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
}

fn tokenize(expression: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut chars = expression.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => continue,
            '0'..='9' | '.' => {
                let mut text = String::from(ch);
                while let Some(next) = chars.peek() {
                    if next.is_ascii_digit() || *next == '.' {
                        text.push(*next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let number = text.parse::<f64>().map_err(|_| {
                    Error::message(format!("`{text}` is not a number in a machine G-code template"))
                })?;
                tokens.push(Token::Number(number));
            }
            '"' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some(other) => text.push(other),
                        None => {
                            return Err(Error::message(
                                "a machine G-code template has an unclosed string",
                            ))
                        }
                    }
                }
                tokens.push(Token::Text(text));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::from(ch);
                while let Some(next) = chars.peek() {
                    if next.is_ascii_alphanumeric() || *next == '_' {
                        name.push(*next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Name(name));
            }
            '(' => tokens.push(Token::Open),
            ')' => tokens.push(Token::Close),
            '[' => tokens.push(Token::OpenIndex),
            ']' => tokens.push(Token::CloseIndex),
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            '*' => tokens.push(Token::Star),
            '/' => tokens.push(Token::Slash),
            '=' => {
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Eq);
                } else {
                    tokens.push(Token::Eq);
                }
            }
            '!' => {
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Ne);
                } else {
                    tokens.push(Token::Not);
                }
            }
            '<' => {
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Le);
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '>' => {
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Ge);
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '&' => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Token::And);
                } else {
                    return Err(Error::message("a machine G-code template uses a single `&`"));
                }
            }
            '|' => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Token::Or);
                } else {
                    return Err(Error::message("a machine G-code template uses a single `|`"));
                }
            }
            other => {
                return Err(Error::message(format!(
                    "a machine G-code template has a `{other}` this renderer cannot read"
                )))
            }
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    values: &'a Values,
    expression: &'a str,
}

impl<'a> Parser<'a> {
    fn error(&self, why: &str) -> Error {
        Error::message(format!(
            "{why}: `{}` in a machine G-code template",
            self.expression
        ))
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        if token.is_some() {
            self.at += 1;
        }
        token
    }

    fn peek_is(&self, token: &Token) -> bool {
        self.tokens.get(self.at) == Some(token)
    }

    fn or_expression(&mut self) -> Result<Value, Error> {
        let mut left = self.and_expression()?;
        while self.peek_is(&Token::Or) {
            self.at += 1;
            let right = self.and_expression()?;
            left = Value::Flag(left.as_flag() || right.as_flag());
        }
        Ok(left)
    }

    fn and_expression(&mut self) -> Result<Value, Error> {
        let mut left = self.compare_expression()?;
        while self.peek_is(&Token::And) {
            self.at += 1;
            let right = self.compare_expression()?;
            left = Value::Flag(left.as_flag() && right.as_flag());
        }
        Ok(left)
    }

    fn compare_expression(&mut self) -> Result<Value, Error> {
        let left = self.sum_expression()?;
        let Some(token) = self.tokens.get(self.at).cloned() else {
            return Ok(left);
        };
        let holds = match token {
            Token::Eq => left.same_as(&self.sum_after()?),
            Token::Ne => !left.same_as(&self.sum_after()?),
            Token::Lt => {
                let right = self.sum_after()?;
                self.number(&left)? < self.number(&right)?
            }
            Token::Le => {
                let right = self.sum_after()?;
                self.number(&left)? <= self.number(&right)?
            }
            Token::Gt => {
                let right = self.sum_after()?;
                self.number(&left)? > self.number(&right)?
            }
            Token::Ge => {
                let right = self.sum_after()?;
                self.number(&left)? >= self.number(&right)?
            }
            _ => return Ok(left),
        };
        Ok(Value::Flag(holds))
    }

    fn sum_after(&mut self) -> Result<Value, Error> {
        self.at += 1;
        self.sum_expression()
    }

    /// A subscript is usually a literal, but Bambu also writes
    /// `filament_type[initial_no_support_extruder]`: a name that holds the
    /// index. Resolve that name first.
    fn index_of(&self, value: &Value) -> Result<usize, Error> {
        let value = match value {
            Value::Text(name) => self
                .values
                .get(name)
                .cloned()
                .ok_or_else(|| self.error(&format!("no value named `{name}` for a subscript")))?,
            other => other.clone(),
        };
        match value {
            Value::Num(number) => Ok(number.max(0.0) as usize),
            Value::Flag(flag) => Ok(usize::from(flag)),
            _ => Err(self.error("a subscript must be a number")),
        }
    }

    fn number(&self, value: &Value) -> Result<f64, Error> {
        value
            .as_number()
            .ok_or_else(|| self.error("expected a number"))
    }

    fn sum_expression(&mut self) -> Result<Value, Error> {
        let mut left = self.product_expression()?;
        loop {
            let token = self.tokens.get(self.at).cloned();
            let number = match token {
                Some(Token::Plus) | Some(Token::Minus) => {
                    self.at += 1;
                    let right = self.product_expression()?;
                    let left_number = self.number(&left)?;
                    let right_number = self.number(&right)?;
                    if token == Some(Token::Plus) {
                        left_number + right_number
                    } else {
                        left_number - right_number
                    }
                }
                _ => return Ok(left),
            };
            left = Value::Num(number);
        }
    }

    fn product_expression(&mut self) -> Result<Value, Error> {
        let mut left = self.unary_expression()?;
        loop {
            let token = self.tokens.get(self.at).cloned();
            let number = match token {
                Some(Token::Star) | Some(Token::Slash) => {
                    self.at += 1;
                    let right = self.unary_expression()?;
                    let left_number = self.number(&left)?;
                    let right_number = self.number(&right)?;
                    if token == Some(Token::Star) {
                        left_number * right_number
                    } else if right_number == 0.0 {
                        return Err(self.error("a division by zero"));
                    } else {
                        left_number / right_number
                    }
                }
                _ => return Ok(left),
            };
            left = Value::Num(number);
        }
    }

    fn unary_expression(&mut self) -> Result<Value, Error> {
        match self.tokens.get(self.at).cloned() {
            Some(Token::Minus) => {
                self.at += 1;
                let value = self.unary_expression()?;
                Ok(Value::Num(-self.number(&value)?))
            }
            Some(Token::Plus) => {
                self.at += 1;
                self.unary_expression()
            }
            Some(Token::Not) => {
                self.at += 1;
                let value = self.unary_expression()?;
                Ok(Value::Flag(!value.as_flag()))
            }
            _ => self.primary_expression(),
        }
    }

    fn primary_expression(&mut self) -> Result<Value, Error> {
        match self.next() {
            Some(Token::Number(number)) => Ok(Value::Num(number)),
            Some(Token::Text(text)) => Ok(Value::Text(text)),
            Some(Token::Open) => {
                let value = self.or_expression()?;
                if self.next() != Some(Token::Close) {
                    return Err(self.error("a bracket was not closed"));
                }
                Ok(value)
            }
            Some(Token::Name(name)) => {
                let Some(mut value) = self.values.get(&name).cloned() else {
                    return Err(Error::message(format!(
                        "the machine G-code template asks for `{name}`, which this slice has no value for"
                    )));
                };
                // `name[index]`, where the index is itself an expression.
                if self.peek_is(&Token::OpenIndex) {
                    self.at += 1;
                    let index = self.or_expression()?;
                    if !self.peek_is(&Token::CloseIndex) {
                        return Err(self.error("a `[` was not closed"));
                    }
                    self.at += 1;
                    let at = self.index_of(&index)?;
                    value = match value {
                        Value::List(items) => items.get(at).cloned().unwrap_or(Value::Num(0.0)),
                        other => other,
                    };
                }
                Ok(value)
            }
            other => Err(self.error(&format!("unexpected {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        let mut values = Values::new();
        values.set("max_layer_z", 10.0);
        values.set("layer_height", 0.2);
        values.set("bed_temp", 60.0);
        values.set("spiral_mode", false);
        values.set("curr_bed_type", "Textured PEI Plate");
        values.set("print_sequence", "all");
        values.set("nozzle_diameter", 0.4);
        values.set_list(
            "nozzle_temperature_initial_layer",
            vec![Value::Num(200.0)],
        );
        values.set_list("filament_type", vec![Value::Text("PLA".into())]);
        values
    }

    fn error_text(template: &str) -> String {
        match render(template, &values()) {
            Ok(text) => panic!("expected a refusal, got {text:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_number_lands_in_the_text() {
        let text = render("G1 Z{max_layer_z + 0.5} F900", &values()).unwrap();
        assert_eq!(text, "G1 Z10.5 F900");
    }

    #[test]
    fn a_whole_number_loses_its_point() {
        let text = render("M104 S{nozzle_temperature_initial_layer[0]}", &values()).unwrap();
        assert_eq!(text, "M104 S200");
    }

    #[test]
    fn a_negative_literal_survives() {
        let text = render("G1 X{-0.02} Y{0 - 0.02}", &values()).unwrap();
        assert_eq!(text, "G1 X-0.02 Y-0.02");
    }

    #[test]
    fn arithmetic_follows_the_school_order() {
        let text = render("{2 + 3 * 4} {12 / 4} {(2 + 3) * 4}", &values()).unwrap();
        assert_eq!(text, "14 3 20");
    }

    #[test]
    fn a_branch_is_taken_only_when_it_holds() {
        let took = render("{if bed_temp > 50}hot{else}cold{endif}", &values()).unwrap();
        assert_eq!(took, "hot");
        let left = render("{if bed_temp < 50}hot{else}cold{endif}", &values()).unwrap();
        assert_eq!(left, "cold");
    }

    #[test]
    fn text_can_be_compared() {
        let plate = render(
            "{if curr_bed_type==\"Textured PEI Plate\"}textured{endif}",
            &values(),
        )
        .unwrap();
        assert_eq!(plate, "textured");
        let smooth = render(
            "{if curr_bed_type==\"Smooth PEI Plate\"}smooth{endif}",
            &values(),
        )
        .unwrap();
        assert_eq!(smooth, "");
    }

    #[test]
    fn a_flag_can_be_negated() {
        let text = render(
            "{if !spiral_mode && print_sequence != \"by object\"}timelapse{endif}",
            &values(),
        )
        .unwrap();
        assert_eq!(text, "timelapse");
    }

    #[test]
    fn a_branch_inside_a_branch_needs_both() {
        let both = render(
            "{if bed_temp > 50}{if layer_height == 0.2}fine{endif}{endif}",
            &values(),
        )
        .unwrap();
        assert_eq!(both, "fine");
        let neither = render(
            "{if bed_temp < 50}{if layer_height == 0.2}fine{endif}{endif}",
            &values(),
        )
        .unwrap();
        assert_eq!(neither, "");
    }


    #[test]
    fn a_subscript_can_be_a_name() {
        let mut values = Values::new();
        values.set("initial_no_support_extruder", 1);
        values.set_list(
            "filament_type",
            vec![Value::Text("PLA".into()), Value::Text("PETG".into())],
        );
        let text = render("{filament_type[initial_no_support_extruder]}", &values).unwrap();
        assert_eq!(text, "PETG");
    }

    #[test]
    fn a_bracket_name_outside_braces_is_filled() {
        let text = render("M983 A0.3 H[nozzle_diameter] ; cali", &values()).unwrap();
        assert_eq!(text, "M983 A0.3 H0.4 ; cali");
    }

    #[test]
    fn an_unknown_name_is_named_and_refused() {
        let message = error_text("M104 S{nozzle_temperature_at_noon}");
        assert!(message.contains("nozzle_temperature_at_noon"), "{message}");
    }

    #[test]
    fn an_unclosed_block_is_refused() {
        let message = error_text("{if bed_temp > 50}hot");
        assert!(message.contains("no `{endif}`"), "{message}");
        let stray = error_text("hot{endif}");
        assert!(stray.contains("never opened"), "{stray}");
    }

    #[test]
    fn settings_arrive_as_numbers_when_they_are_numbers() {
        let mut values = Values::new();
        values.set_setting("layer_height", "0.16");
        values.set_setting("cool_fan_enabled", "False");
        values.set_setting("material_type", "PLA");
        assert_eq!(values.get("layer_height"), Some(&Value::Num(0.16)));
        assert_eq!(values.get("cool_fan_enabled"), Some(&Value::Flag(false)));
        assert_eq!(
            values.get("material_type"),
            Some(&Value::Text("PLA".into()))
        );
    }
}
