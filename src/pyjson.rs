//! The crate's own JSON reader and writer, speaking the
//! language the oracle reader speaks.
//!
//! The fixture files are JSON, and the oracle reads them with
//! Python's json module, whose readings differ from a stock
//! JSON parser in ways the contract makes observable. A field
//! given twice keeps its last value at the position of its
//! first giving. Integers are exact at any length up to the
//! digit limit. The non finite literals read as non finite
//! values. A string escape naming a code point in the surrogate
//! range is kept as that code point, so a string differing from
//! another only there is a different string. A document nested
//! 9991 containers deep loads, and one nested 9992 deep raises
//! the uncaught family, as does an integer literal of 4301
//! digits, the sign not counted. Both ceilings are this crate's
//! own probed values.
//!
//! The writer serves the canonical comparison key of the
//! judgment seam, sorted keys with the oracle's default
//! separators and ASCII escaping, an astral character written
//! as a surrogate pair.

use crate::errors::Error;
use crate::pytext::{PyStr, py_float_repr};

/// The deepest container nesting the reader admits, probed
/// against the oracle at 9991 admitted and 9992 uncaught.
pub const MAX_CONTAINER_DEPTH: usize = 9991;

/// The most digits of an integer literal the reader converts,
/// probed against the oracle at 4300 converted and 4301
/// uncaught, the sign not counted.
pub const MAX_INT_DIGITS: usize = 4300;

/// A JSON value in the oracle reader's model.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// An integer, held as its canonical decimal text.
    Int(String),
    Float(f64),
    Str(PyStr),
    Array(Vec<Value>),
    Object(Vec<(PyStr, Value)>),
}

impl Value {
    /// The value under a field name, for an object.
    pub fn get(&self, field: &str) -> Option<&Value> {
        match self {
            Value::Object(pairs) => pairs
                .iter()
                .find(|(key, _)| key == &field)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Whether the value is an object carrying the field.
    pub fn has(&self, field: &str) -> bool {
        self.get(field).is_some()
    }

    /// The field names of an object, in position order.
    pub fn field_names(&self) -> Vec<PyStr> {
        match self {
            Value::Object(pairs) => pairs.iter().map(|(key, _)| key.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The string the value holds, when it is a string.
    pub fn as_str(&self) -> Option<&PyStr> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The float a number converts to, as the oracle's float
    /// conversion reads it, where an integer too large to
    /// convert is not finite, matching the oracle's overflow.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(text) => text.parse::<f64>().ok(),
            Value::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// Whether the value is a number in the oracle's sense, an
    /// integer or a float and not a boolean, whose float
    /// conversion is finite.
    pub fn is_number(&self) -> bool {
        match self.as_f64() {
            Some(value) => value.is_finite(),
            None => false,
        }
    }
}

/// How a read can fail. A grammar failure is an ordinary
/// invalid document. The uncaught failure is the reader hitting
/// a ceiling the oracle never caught.
#[derive(Debug)]
pub enum LoadsError {
    Invalid,
    Uncaught(String),
}

impl From<LoadsError> for Error {
    fn from(error: LoadsError) -> Error {
        match error {
            LoadsError::Invalid => Error::Judgment("the document is not valid JSON".to_string()),
            LoadsError::Uncaught(line) => Error::Uncaught(line),
        }
    }
}

/// The line the port prints for a reader ceiling, its own one
/// line standing where the oracle printed a traceback.
pub const RECURSION_LINE: &str = "badbatch: the fixture reader exceeded its nesting ceiling";
/// The line the port prints for the digit limit.
pub const DIGIT_LINE: &str = "badbatch: the fixture reader exceeded its integer digit limit";

struct Parser {
    points: Vec<u32>,
    index: usize,
}

impl Parser {
    fn peek(&self) -> Option<u32> {
        self.points.get(self.index).copied()
    }

    fn is_digit(point: u32) -> bool {
        (0x30..=0x39).contains(&point)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0A | 0x0D)) {
            self.index += 1;
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, LoadsError> {
        self.skip_whitespace();
        match self.peek() {
            Some(0x7B) => self.parse_object(depth),
            Some(0x5B) => self.parse_array(depth),
            Some(0x22) => Ok(Value::Str(self.parse_string()?)),
            Some(0x74) => self.parse_literal("true", Value::Bool(true)),
            Some(0x66) => self.parse_literal("false", Value::Bool(false)),
            Some(0x6E) => self.parse_literal("null", Value::Null),
            Some(0x4E) => self.parse_literal("NaN", Value::Float(f64::NAN)),
            Some(0x49) => self.parse_literal("Infinity", Value::Float(f64::INFINITY)),
            Some(0x2D) => {
                if self.points[self.index + 1..].starts_with(&[0x49]) {
                    self.parse_literal("-Infinity", Value::Float(f64::NEG_INFINITY))
                } else {
                    self.parse_number()
                }
            }
            Some(point) if point == 0x2B || Self::is_digit(point) => self.parse_number(),
            _ => Err(LoadsError::Invalid),
        }
    }

    fn parse_literal(&mut self, word: &str, value: Value) -> Result<Value, LoadsError> {
        let points: Vec<u32> = word.chars().map(|ch| ch as u32).collect();
        if self.points[self.index..].starts_with(&points) {
            self.index += points.len();
            Ok(value)
        } else {
            Err(LoadsError::Invalid)
        }
    }

    fn enter_container(&self, depth: usize) -> Result<usize, LoadsError> {
        let next = depth + 1;
        if next > MAX_CONTAINER_DEPTH {
            return Err(LoadsError::Uncaught(RECURSION_LINE.to_string()));
        }
        Ok(next)
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, LoadsError> {
        let depth = self.enter_container(depth)?;
        self.index += 1;
        let mut pairs: Vec<(PyStr, Value)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(0x7D) {
            self.index += 1;
            return Ok(Value::Object(pairs));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(0x22) {
                return Err(LoadsError::Invalid);
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(0x3A) {
                return Err(LoadsError::Invalid);
            }
            self.index += 1;
            let value = self.parse_value(depth)?;
            match pairs.iter_mut().find(|(held, _)| *held == key) {
                Some(slot) => slot.1 = value,
                None => pairs.push((key, value)),
            }
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => {
                    self.index += 1;
                }
                Some(0x7D) => {
                    self.index += 1;
                    return Ok(Value::Object(pairs));
                }
                _ => return Err(LoadsError::Invalid),
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, LoadsError> {
        let depth = self.enter_container(depth)?;
        self.index += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(0x5D) {
            self.index += 1;
            return Ok(Value::Array(items));
        }
        loop {
            let value = self.parse_value(depth)?;
            items.push(value);
            self.skip_whitespace();
            match self.peek() {
                Some(0x2C) => {
                    self.index += 1;
                }
                Some(0x5D) => {
                    self.index += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(LoadsError::Invalid),
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, LoadsError> {
        let mut value: u32 = 0;
        for _ in 0..4 {
            let point = self.peek().ok_or(LoadsError::Invalid)?;
            let digit = match point {
                0x30..=0x39 => point - 0x30,
                0x61..=0x66 => point - 0x61 + 10,
                0x41..=0x46 => point - 0x41 + 10,
                _ => return Err(LoadsError::Invalid),
            };
            value = value * 16 + digit;
            self.index += 1;
        }
        Ok(value)
    }

    fn parse_string(&mut self) -> Result<PyStr, LoadsError> {
        self.index += 1;
        let mut points: Vec<u32> = Vec::new();
        loop {
            let point = self.peek().ok_or(LoadsError::Invalid)?;
            self.index += 1;
            match point {
                0x22 => return Ok(PyStr(points)),
                0x5C => {
                    let escape = self.peek().ok_or(LoadsError::Invalid)?;
                    self.index += 1;
                    match escape {
                        0x22 => points.push(0x22),
                        0x5C => points.push(0x5C),
                        0x2F => points.push(0x2F),
                        0x62 => points.push(0x08),
                        0x66 => points.push(0x0C),
                        0x6E => points.push(0x0A),
                        0x72 => points.push(0x0D),
                        0x74 => points.push(0x09),
                        0x75 => {
                            let first = self.parse_hex4()?;
                            if (0xD800..=0xDBFF).contains(&first) {
                                let saved = self.index;
                                if self.peek() == Some(0x5C)
                                    && self.points.get(self.index + 1) == Some(&0x75)
                                {
                                    self.index += 2;
                                    if let Ok(second) = self.parse_hex4()
                                        && (0xDC00..=0xDFFF).contains(&second)
                                    {
                                        points.push(
                                            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00),
                                        );
                                        continue;
                                    }
                                }
                                self.index = saved;
                                points.push(first);
                            } else {
                                points.push(first);
                            }
                        }
                        _ => return Err(LoadsError::Invalid),
                    }
                }
                0x00..=0x1F => return Err(LoadsError::Invalid),
                _ => points.push(point),
            }
        }
    }

    fn parse_number(&mut self) -> Result<Value, LoadsError> {
        let start = self.index;
        if self.peek() == Some(0x2D) {
            self.index += 1;
        }
        // The integer part: a single zero, or a non zero digit
        // and any digits. A leading zero before more digits is a
        // grammar failure, never the digit limit.
        match self.peek() {
            Some(0x30) => {
                self.index += 1;
            }
            Some(point) if Self::is_digit(point) => {
                while matches!(self.peek(), Some(digit) if Self::is_digit(digit)) {
                    self.index += 1;
                }
            }
            _ => return Err(LoadsError::Invalid),
        }
        let mut is_float = false;
        if self.peek() == Some(0x2E) {
            is_float = true;
            self.index += 1;
            if !matches!(self.peek(), Some(digit) if Self::is_digit(digit)) {
                return Err(LoadsError::Invalid);
            }
            while matches!(self.peek(), Some(digit) if Self::is_digit(digit)) {
                self.index += 1;
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            is_float = true;
            self.index += 1;
            if matches!(self.peek(), Some(0x2B | 0x2D)) {
                self.index += 1;
            }
            if !matches!(self.peek(), Some(digit) if Self::is_digit(digit)) {
                return Err(LoadsError::Invalid);
            }
            while matches!(self.peek(), Some(digit) if Self::is_digit(digit)) {
                self.index += 1;
            }
        }
        let text: String = self.points[start..self.index]
            .iter()
            .map(|point| char::from_u32(*point).unwrap_or('\u{FFFD}'))
            .collect();
        if is_float {
            return Ok(Value::Float(
                text.parse::<f64>().map_err(|_| LoadsError::Invalid)?,
            ));
        }
        let digits = text.strip_prefix('-').unwrap_or(&text);
        if digits.len() > MAX_INT_DIGITS {
            return Err(LoadsError::Uncaught(DIGIT_LINE.to_string()));
        }
        let canonical = digits.trim_start_matches('0');
        let canonical = if canonical.is_empty() {
            "0".to_string()
        } else if text.starts_with('-') {
            format!("-{canonical}")
        } else {
            canonical.to_string()
        };
        Ok(Value::Int(canonical))
    }
}

/// Read one JSON document in the oracle reader's language.
pub fn loads(text: &PyStr) -> Result<Value, LoadsError> {
    let mut parser = Parser {
        points: text.0.clone(),
        index: 0,
    };
    let value = parser.parse_value(0)?;
    parser.skip_whitespace();
    if parser.index != parser.points.len() {
        return Err(LoadsError::Invalid);
    }
    Ok(value)
}

/// The escape of one code point inside a JSON string under the
/// writer's ASCII rule.
fn escape_point(point: u32, out: &mut String) {
    match point {
        0x22 => out.push_str("\\\""),
        0x5C => out.push_str("\\\\"),
        0x0A => out.push_str("\\n"),
        0x0D => out.push_str("\\r"),
        0x09 => out.push_str("\\t"),
        0x08 => out.push_str("\\b"),
        0x0C => out.push_str("\\f"),
        0x20..=0x7E => {
            if let Some(ch) = char::from_u32(point) {
                out.push(ch);
            }
        }
        0x00..=0xFFFF => {
            out.push_str(&format!("\\u{point:04x}"));
        }
        _ => {
            let rest = point - 0x10000;
            out.push_str(&format!(
                "\\u{:04x}\\u{:04x}",
                0xD800 + (rest >> 10),
                0xDC00 + (rest & 0x3FF)
            ));
        }
    }
}

/// The repr of a JSON value, as Python's repr reads the value
/// the oracle's parser builds from the same text. Object pairs
/// print in position order.
pub fn py_value_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Int(text) => text.clone(),
        Value::Float(number) => {
            if number.is_nan() {
                "nan".to_string()
            } else if number.is_infinite() {
                if number.is_sign_negative() {
                    "-inf".to_string()
                } else {
                    "inf".to_string()
                }
            } else {
                py_float_repr(*number)
            }
        }
        Value::Str(text) => crate::pytext::py_repr(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_value_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(key, item)| {
                    format!("{}: {}", crate::pytext::py_repr(key), py_value_repr(item))
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Write a value as a quoted JSON string under the ASCII rule.
pub fn dumps_str(text: &PyStr, out: &mut String) {
    out.push('"');
    for &point in &text.0 {
        escape_point(point, out);
    }
    out.push('"');
}

/// Write one value in the canonical key's language, keys
/// sorted, items separated by a comma and a space, a key
/// followed by a colon and a space.
pub fn dumps_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(text) => out.push_str(text),
        Value::Float(number) => {
            if number.is_nan() {
                out.push_str("NaN");
            } else if number.is_infinite() {
                out.push_str(if number.is_sign_negative() {
                    "-Infinity"
                } else {
                    "Infinity"
                });
            } else {
                out.push_str(&py_float_repr(*number));
            }
        }
        Value::Str(text) => dumps_str(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                dumps_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(pairs) => {
            let mut sorted: Vec<&(PyStr, Value)> = pairs.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            out.push('{');
            for (index, (key, item)) in sorted.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                dumps_str(key, out);
                out.push_str(": ");
                dumps_canonical(item, out);
            }
            out.push('}');
        }
    }
}
