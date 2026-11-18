//! Minimal JSON for no_std WASM plugins. Values borrow input slices where
//! possible; `to_bytes` serializes with escaping. Numbers are stored as text.
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn object() -> Value { Value::Obj(Vec::new()) }
    pub fn str(s: &str) -> Value { Value::Str(s.to_string()) }
    pub fn num(n: i64) -> Value { Value::Num(n.to_string()) }
    pub fn insert(&mut self, key: &str, value: Value) {
        if let Value::Obj(entries) = self {
            entries.push((key.to_string(), value));
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        if let Value::Obj(entries) = self {
            entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else { None }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Value::Num(n) = self { n.parse().ok() } else { None }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Value::Arr(a) = self { Some(a) } else { None }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Value::Null => out.extend_from_slice(b"null"),
            Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Num(n) => out.extend_from_slice(n.as_bytes()),
            Value::Str(s) => write_string(s, out),
            Value::Arr(items) => {
                out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { out.push(b','); }
                    item.write(out);
                }
                out.push(b']');
            }
            Value::Obj(entries) => {
                out.push(b'{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 { out.push(b','); }
                    write_string(k, out);
                    out.push(b':');
                    v.write(out);
                }
                out.push(b'}');
            }
        }
    }
}

fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.bytes() {
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0..=0x1f => {
                out.extend_from_slice(b"\\u00");
                let hi = c >> 4; let lo = c & 0xf;
                out.push(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 });
                out.push(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 });
            }
            _ => out.push(c),
        }
    }
    out.push(b'"');
}

pub fn parse(input: &[u8]) -> Result<Value, &'static str> {
    let mut p = Parser { data: input, pos: 0 };
    p.skip_ws();
    let value = p.value()?;
    Ok(value)
}

struct Parser<'a> { data: &'a [u8], pos: usize }
impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> { self.data.get(self.pos).copied() }
    fn bump(&mut self) -> Option<u8> { let c = self.peek(); if c.is_some() { self.pos += 1; } c }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) { self.pos += 1; }
    }
    fn value(&mut self) -> Result<Value, &'static str> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err("unexpected token"),
        }
    }
    fn literal(&mut self, text: &str, value: Value) -> Result<Value, &'static str> {
        if self.data[self.pos..].starts_with(text.as_bytes()) {
            self.pos += text.len(); Ok(value)
        } else { Err("bad literal") }
    }
    fn number(&mut self) -> Result<Value, &'static str> {
        let start = self.pos;
        if self.peek() == Some(b'-') { self.pos += 1; }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || c == b'+' || c == b'-') {
            self.pos += 1;
        }
        let text = core::str::from_utf8(&self.data[start..self.pos]).map_err(|_| "number")?;
        Ok(Value::Num(text.to_string()))
    }
    fn string(&mut self) -> Result<String, &'static str> {
        if self.bump() != Some(b'"') { return Err("expected string"); }
        let mut out = String::new();
        loop {
            match self.bump() {
                Some(b'"') => return Ok(out),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'b') => out.push('\u{8}'),
                    Some(b'f') => out.push('\u{c}'),
                    Some(b'u') => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let c = self.bump().ok_or("escape")?;
                            let d = match c { b'0'..=b'9' => c - b'0', b'a'..=b'f' => c - b'a' + 10, b'A'..=b'F' => c - b'A' + 10, _ => return Err("hex") } as u32;
                            code = code * 16 + d;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    _ => return Err("escape"),
                },
                Some(c) => {
                    // collect a full UTF-8 sequence
                    let start = self.pos - 1;
                    let len = if c < 0x80 { 1 } else if c >> 5 == 0b110 { 2 } else if c >> 4 == 0b1110 { 3 } else { 4 };
                    self.pos = start + len;
                    let s = core::str::from_utf8(&self.data[start..self.pos]).map_err(|_| "utf8")?;
                    out.push_str(s);
                }
                None => return Err("unterminated string"),
            }
        }
    }
    fn array(&mut self) -> Result<Value, &'static str> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') { self.bump(); return Ok(Value::Arr(items)); }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Value::Arr(items)),
                _ => return Err("array"),
            }
        }
    }
    fn object(&mut self) -> Result<Value, &'static str> {
        self.bump(); // {
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') { self.bump(); return Ok(Value::Obj(entries)); }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.bump() != Some(b':') { return Err("expected colon"); }
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Value::Obj(entries)),
                _ => return Err("object"),
            }
        }
    }
}

#[allow(dead_code)]
fn _boxed(_: Box<u8>) {}