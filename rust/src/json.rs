//! Минимальный JSON: парсер и сериализатор без внешних крейтов.
//!
//! Telegram присылает и принимает JSON, а crates.io в этом окружении
//! недоступен, поэтому пишем свой. Поддерживается всё нужное:
//! объекты, массивы, строки с эскейпами и `\uXXXX`, числа, bool, null.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn obj() -> Self {
        Value::Object(BTreeMap::new())
    }

    pub fn arr(items: Vec<Value>) -> Self {
        Value::Array(items)
    }

    pub fn str(s: impl Into<String>) -> Self {
        Value::Str(s.into())
    }

    /// Вставляет поле в объект. Для не-объектов — no-op.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        if let Value::Object(map) = self {
            map.insert(key.to_string(), value.into());
        }
    }

    /// Строит объект из пар «ключ → значение».
    pub fn from_pairs(pairs: Vec<(&str, Value)>) -> Self {
        let mut map = BTreeMap::new();
        for (key, value) in pairs {
            map.insert(key.to_string(), value);
        }
        Value::Object(map)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Достаёт вложенный объект и создаёт его при отсутствии.
    pub fn entry(&mut self, key: &str) -> &mut Value {
        if let Value::Object(map) = self {
            map.entry(key.to_string()).or_insert_with(Value::obj)
        } else {
            self
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Object(map) => map.get_mut(key),
            _ => None,
        }
    }

    /// Мутабельный доступ к объекту-значению.
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Мутабельный доступ к массиву.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Строка или пустая строка — удобно для текстов из Telegram.
    pub fn text(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            _ => String::new(),
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(*v as i64),
            Value::Str(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            Value::Int(v) => Some(*v != 0),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Целое с дефолтом.
    pub fn i64_or(&self, fallback: i64) -> i64 {
        self.as_i64().unwrap_or(fallback)
    }

    /// Строка с дефолтом.
    pub fn str_or(&self, fallback: &str) -> String {
        match self {
            Value::Str(s) if !s.is_empty() => s.clone(),
            _ => fallback.to_string(),
        }
    }

    /// `get` с дефолтом.
    pub fn get_str(&self, key: &str) -> String {
        self.get(key).map(|v| v.text()).unwrap_or_default()
    }

    pub fn get_i64(&self, key: &str, fallback: i64) -> i64 {
        self.get(key).map(|v| v.i64_or(fallback)).unwrap_or(fallback)
    }

    pub fn get_bool(&self, key: &str, fallback: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(fallback)
    }

    /// Массив строк из значения (для списков выбранных типов).
    pub fn str_array(&self) -> Vec<String> {
        match self {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match item {
                    Value::Str(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn get_arr(&self, key: &str) -> Option<&Vec<Value>> {
        self.get(key).and_then(|v| v.as_array())
    }

    pub fn get_obj(&self, key: &str) -> Option<&Value> {
        match self.get(key) {
            Some(v @ Value::Object(_)) => Some(v),
            _ => None,
        }
    }

    // ------------------------------------------------------------- парсинг

    pub fn parse(text: &str) -> Result<Value, String> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.value()?;
        parser.skip_ws();
        Ok(value)
    }

    // -------------------------------------------------------- сериализация

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Int(v) => {
                let _ = write!(out, "{}", v);
            }
            Value::Float(v) => {
                // JSON не знает Infinity/NaN
                if v.is_finite() {
                    let _ = write!(out, "{}", v);
                } else {
                    out.push_str("null");
                }
            }
            Value::Str(s) => write_str(out, s),
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Value::Object(map) => {
                out.push('{');
                for (index, (key, value)) in map.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_str(out, key);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_str(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// Конверсии для удобной сборки запросов
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Int(v as i64)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v as i64)
    }
}

impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Value::Int(v as i64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Value::Array(items)
    }
}

// ---------------------------------------------------------------- парсер

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> u8 {
        if self.pos < self.bytes.len() {
            self.bytes[self.pos]
        } else {
            0
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Value::Str),
            b't' => {
                self.expect("true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.expect("false")?;
                Ok(Value::Bool(false))
            }
            b'n' => {
                self.expect("null")?;
                Ok(Value::Null)
            }
            0 => Err("неожиданный конец JSON".to_string()),
            _ => self.number(),
        }
    }

    fn expect(&mut self, literal: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(format!("ожидался «{}» на позиции {}", literal, self.pos))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.pos += 1; // {
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == b'}' {
            self.pos += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() == b':' {
                self.pos += 1;
            }
            let value = self.value()?;
            map.insert(key, value);
            self.skip_ws();
            match self.peek() {
                b',' => {
                    self.pos += 1;
                }
                b'}' => {
                    self.pos += 1;
                    break;
                }
                other => {
                    return Err(format!("ожидалась , или }} на позиции {}: {:?}", self.pos, other as char));
                }
            }
        }
        Ok(Value::Object(map))
    }

    fn array(&mut self) -> Result<Value, String> {
        self.pos += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == b']' {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                b',' => {
                    self.pos += 1;
                }
                b']' => {
                    self.pos += 1;
                    break;
                }
                other => {
                    return Err(format!("ожидалась , или ] на позиции {}: {:?}", self.pos, other as char));
                }
            }
        }
        Ok(Value::Array(items))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != b'"' {
            return Err(format!("ожидалась строка на позиции {}", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err("незакрытая строка".to_string());
            }
            let byte = self.bytes[self.pos];
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    if self.pos >= self.bytes.len() {
                        return Err("незавершённый escape".to_string());
                    }
                    let escaped = self.bytes[self.pos];
                    self.pos += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code = self.hex4()?;
                            // суррогатная пара для эмодзи и прочих > U+FFFF
                            if (0xD800..0xDC00).contains(&code) {
                                if self.pos + 1 < self.bytes.len()
                                    && self.bytes[self.pos] == b'\\'
                                    && self.bytes[self.pos + 1] == b'u'
                                {
                                    self.pos += 2;
                                    let low = self.hex4()?;
                                    let combined = 0x10000
                                        + ((code - 0xD800) << 10)
                                        + (low.wrapping_sub(0xDC00) & 0x3FF);
                                    out.push(char::from_u32(combined).unwrap_or('\u{FFFD}'));
                                } else {
                                    out.push('\u{FFFD}');
                                }
                            } else {
                                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => out.push(other as char),
                    }
                }
                _ => {
                    // UTF-8: копируем последовательность целиком
                    let start = self.pos - 1;
                    let width = utf8_width(byte);
                    let end = (start + width).min(self.bytes.len());
                    if let Ok(chunk) = std::str::from_utf8(&self.bytes[start..end]) {
                        out.push_str(chunk);
                    }
                    self.pos = end;
                }
            }
        }
        Ok(out)
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.bytes.len() {
            return Err("обрезанный \\uXXXX".to_string());
        }
        let chunk = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| "некорректный \\uXXXX".to_string())?;
        let value = u32::from_str_radix(chunk, 16).map_err(|_| "некорректный \\uXXXX".to_string())?;
        self.pos += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let byte = self.bytes[self.pos];
            if byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let raw = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "некорректное число".to_string())?;
        if raw.is_empty() {
            return Err(format!("неожиданный символ на позиции {}", start));
        }
        if raw.contains(['.', 'e', 'E']) {
            raw.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| format!("некорректное число «{}»", raw))
        } else {
            raw.parse::<i64>()
                .map(Value::Int)
                .map_err(|_| format!("некорректное число «{}»", raw))
        }
    }
}

fn utf8_width(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn разбирает_объект() {
        let value = Value::parse(r#"{"a":1,"b":"x","c":[1,2,3],"d":true,"e":null}"#).unwrap();
        assert_eq!(value.get_i64("a", 0), 1);
        assert_eq!(value.get_str("b"), "x");
        assert_eq!(value.get_arr("c").unwrap().len(), 3);
        assert_eq!(value.get_bool("d", false), true);
        assert!(value.get("e").unwrap() == &Value::Null);
    }

    #[test]
    fn разбирает_эмодзи_через_суррогаты() {
        let value = Value::parse(r#"{"t":"\ud83d\udc8e"}"#).unwrap();
        assert_eq!(value.get_str("t"), "💎");
    }

    #[test]
    fn разбирает_кириллицу() {
        let value = Value::parse(r#"{"t":"Привет, мир"}"#).unwrap();
        assert_eq!(value.get_str("t"), "Привет, мир");
    }

    #[test]
    fn сериализация_и_обратно() {
        let original = r#"{"текст":"кавычка \" и \\ и \n"}"#;
        let parsed = Value::parse(original).unwrap();
        let written = parsed.to_string();
        let again = Value::parse(&written).unwrap();
        assert_eq!(parsed, again);
    }

    #[test]
    fn вложенность() {
        let value = Value::parse(r#"{"a":{"b":{"c":[{"d":42}]}}}"#).unwrap();
        let deep = value
            .get_obj("a")
            .and_then(|v| v.get_obj("b"))
            .and_then(|v| v.get_arr("c"))
            .and_then(|v| v.first())
            .and_then(|v| v.get("d"))
            .and_then(|v| v.as_i64());
        assert_eq!(deep, Some(42));
    }

    #[test]
    fn ошибка_на_мусоре() {
        assert!(Value::parse("{не json}").is_err());
        assert!(Value::parse("").is_err());
    }
}
