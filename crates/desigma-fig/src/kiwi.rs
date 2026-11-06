//! Generic Kiwi (https://github.com/evanw/kiwi) binary schema + message codec.
//!
//! Decoding is fully dynamic: the schema embedded in the `.fig` file drives the
//! decoder, producing a [`KiwiValue`] tree. A matching encoder is provided so
//! tests (and a future `.fig` writer) can synthesize files.

use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KiwiError {
    #[error("unexpected end of data at offset {0}")]
    Eof(usize),
    #[error("malformed data: {0}")]
    Bad(String),
}

type Result<T> = std::result::Result<T, KiwiError>;

const MAX_DEPTH: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefKind {
    Enum,
    Struct,
    Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    Bool,
    Byte,
    Int,
    Uint,
    Float,
    String,
    Int64,
    Uint64,
}

impl Native {
    fn from_id(id: i64) -> Option<Native> {
        Some(match id {
            -1 => Native::Bool,
            -2 => Native::Byte,
            -3 => Native::Int,
            -4 => Native::Uint,
            -5 => Native::Float,
            -6 => Native::String,
            -7 => Native::Int64,
            -8 => Native::Uint64,
            _ => return None,
        })
    }
    fn id(self) -> i64 {
        match self {
            Native::Bool => -1,
            Native::Byte => -2,
            Native::Int => -3,
            Native::Uint => -4,
            Native::Float => -5,
            Native::String => -6,
            Native::Int64 => -7,
            Native::Uint64 => -8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Native(Native),
    /// Index into [`Schema::defs`].
    Def(usize),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: Arc<str>,
    pub ty: FieldType,
    pub is_array: bool,
    /// Message: field id. Enum: variant value. Struct: ordinal (unused).
    pub value: u32,
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub kind: DefKind,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub defs: Vec<Def>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KiwiValue {
    Bool(bool),
    Byte(u8),
    Int(i32),
    Uint(u32),
    Float(f32),
    String(String),
    Int64(i64),
    Uint64(u64),
    /// `byte[]` (image hashes, blobs...).
    Bytes(Vec<u8>),
    Enum(Arc<str>),
    /// Enum value without a known name.
    EnumRaw(u32),
    Array(Vec<KiwiValue>),
    Struct(Vec<(Arc<str>, KiwiValue)>),
    Message(Vec<(Arc<str>, KiwiValue)>),
}

impl KiwiValue {
    pub fn get(&self, name: &str) -> Option<&KiwiValue> {
        match self {
            KiwiValue::Struct(f) | KiwiValue::Message(f) => f.iter().find(|(n, _)| &**n == name).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[KiwiValue]> {
        match self {
            KiwiValue::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            KiwiValue::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- reader

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }
    pub fn byte(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or(KiwiError::Eof(self.pos))?;
        self.pos += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(KiwiError::Eof(self.pos));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn var_u64(&mut self) -> Result<u64> {
        let mut v: u64 = 0;
        let mut shift = 0u32;
        // kiwi's 64-bit varint: up to eight 7-bit groups, then a ninth byte
        // that carries a full 8 bits (no continuation flag).
        loop {
            let b = self.byte()?;
            if shift == 56 {
                return Ok(v | ((b as u64) << 56));
            }
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    pub fn var_u32(&mut self) -> Result<u32> {
        Ok(self.var_u64()? as u32)
    }
    pub fn var_i32(&mut self) -> Result<i32> {
        let v = self.var_u32()?;
        Ok(((v >> 1) as i32) ^ -((v & 1) as i32))
    }
    pub fn var_i64(&mut self) -> Result<i64> {
        let v = self.var_u64()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }
    pub fn var_f32(&mut self) -> Result<f32> {
        let first = *self.data.get(self.pos).ok_or(KiwiError::Eof(self.pos))?;
        if first == 0 {
            self.pos += 1;
            return Ok(0.0);
        }
        let b = self.take(4)?;
        let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        Ok(f32::from_bits(bits.rotate_right(9)))
    }
    pub fn string(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let n = rest.iter().position(|&b| b == 0).ok_or(KiwiError::Eof(self.data.len()))?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.pos += n + 1;
        Ok(s)
    }
}

// ---------------------------------------------------------------- schema

impl Schema {
    /// Decodes the binary schema (`encodeBinarySchema` in kiwi).
    pub fn decode(data: &[u8]) -> Result<Schema> {
        let mut r = Reader::new(data);
        let count = r.var_u32()? as usize;
        if count > r.remaining() {
            return Err(KiwiError::Bad("definition count exceeds data".into()));
        }
        let mut defs = Vec::with_capacity(count);
        for _ in 0..count {
            let name = r.string()?;
            let kind = match r.byte()? {
                0 => DefKind::Enum,
                1 => DefKind::Struct,
                2 => DefKind::Message,
                k => return Err(KiwiError::Bad(format!("bad definition kind {k}"))),
            };
            let fc = r.var_u32()? as usize;
            if fc > r.remaining() {
                return Err(KiwiError::Bad("field count exceeds data".into()));
            }
            let mut fields = Vec::with_capacity(fc);
            for _ in 0..fc {
                let fname = r.string()?;
                let tid = r.var_i32()? as i64;
                let is_array = r.byte()? != 0;
                let value = r.var_u32()?;
                let ty = if tid < 0 {
                    // Enums carry type 0 with no meaning; natives are negative.
                    FieldType::Native(Native::from_id(tid).ok_or_else(|| KiwiError::Bad(format!("bad native type {tid}")))?)
                } else {
                    FieldType::Def(tid as usize)
                };
                fields.push(Field { name: fname.into(), ty, is_array, value });
            }
            defs.push(Def { name, kind, fields });
        }
        let schema = Schema { defs };
        for d in &schema.defs {
            for f in &d.fields {
                if let FieldType::Def(i) = f.ty {
                    if i >= schema.defs.len() {
                        return Err(KiwiError::Bad(format!("type index {i} out of range")));
                    }
                }
            }
        }
        Ok(schema)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.var_u64(self.defs.len() as u64);
        for d in &self.defs {
            w.string(&d.name);
            w.byte(match d.kind {
                DefKind::Enum => 0,
                DefKind::Struct => 1,
                DefKind::Message => 2,
            });
            w.var_u64(d.fields.len() as u64);
            for f in &d.fields {
                w.string(&f.name);
                w.var_i64(match f.ty {
                    FieldType::Native(n) => n.id(),
                    FieldType::Def(i) => i as i64,
                });
                w.byte(f.is_array as u8);
                w.var_u64(f.value as u64);
            }
        }
        w.buf
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.defs.iter().position(|d| d.name == name)
    }

    /// Decodes a root message of definition `root`.
    pub fn decode_message(&self, root: usize, data: &[u8]) -> Result<KiwiValue> {
        let mut r = Reader::new(data);
        self.decode_def(root, &mut r, 0)
    }

    /// Picks the `Message` definition (Figma's root), falling back to the last message def.
    pub fn root_message(&self) -> Option<usize> {
        self.find("Message").filter(|&i| self.defs[i].kind == DefKind::Message).or_else(|| self.defs.iter().rposition(|d| d.kind == DefKind::Message))
    }

    fn decode_def(&self, idx: usize, r: &mut Reader, depth: usize) -> Result<KiwiValue> {
        if depth > MAX_DEPTH {
            return Err(KiwiError::Bad("nesting too deep".into()));
        }
        let def = &self.defs[idx];
        match def.kind {
            DefKind::Enum => {
                let v = r.var_u32()?;
                Ok(match def.fields.iter().find(|f| f.value == v) {
                    Some(f) => KiwiValue::Enum(f.name.clone()),
                    None => KiwiValue::EnumRaw(v),
                })
            }
            DefKind::Struct => {
                let mut out = Vec::with_capacity(def.fields.len());
                for f in &def.fields {
                    out.push((f.name.clone(), self.decode_field(f, r, depth)?));
                }
                Ok(KiwiValue::Struct(out))
            }
            DefKind::Message => {
                let mut out: Vec<(Arc<str>, KiwiValue)> = Vec::new();
                loop {
                    let id = r.var_u32()?;
                    if id == 0 {
                        break;
                    }
                    let f = def.fields.iter().find(|f| f.value == id).ok_or_else(|| KiwiError::Bad(format!("unknown field id {id} in {}", def.name)))?;
                    out.push((f.name.clone(), self.decode_field(f, r, depth)?));
                }
                Ok(KiwiValue::Message(out))
            }
        }
    }

    fn decode_field(&self, f: &Field, r: &mut Reader, depth: usize) -> Result<KiwiValue> {
        if !f.is_array {
            return self.decode_type(f.ty, r, depth + 1);
        }
        let n = r.var_u32()? as usize;
        if f.ty == FieldType::Native(Native::Byte) {
            return Ok(KiwiValue::Bytes(r.take(n)?.to_vec()));
        }
        if n > r.remaining() {
            return Err(KiwiError::Bad(format!("array length {n} exceeds data")));
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.decode_type(f.ty, r, depth + 1)?);
        }
        Ok(KiwiValue::Array(v))
    }

    fn decode_type(&self, ty: FieldType, r: &mut Reader, depth: usize) -> Result<KiwiValue> {
        Ok(match ty {
            FieldType::Native(n) => match n {
                Native::Bool => KiwiValue::Bool(r.byte()? != 0),
                Native::Byte => KiwiValue::Byte(r.byte()?),
                Native::Int => KiwiValue::Int(r.var_i32()?),
                Native::Uint => KiwiValue::Uint(r.var_u32()?),
                Native::Float => KiwiValue::Float(r.var_f32()?),
                Native::String => KiwiValue::String(r.string()?),
                Native::Int64 => KiwiValue::Int64(r.var_i64()?),
                Native::Uint64 => KiwiValue::Uint64(r.var_u64()?),
            },
            FieldType::Def(i) => self.decode_def(i, r, depth)?,
        })
    }

    /// Encodes `value` as definition `root` (test/export helper).
    pub fn encode_message(&self, root: usize, value: &KiwiValue) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        self.encode_def(root, value, &mut w, 0)?;
        Ok(w.buf)
    }

    fn encode_def(&self, idx: usize, v: &KiwiValue, w: &mut Writer, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(KiwiError::Bad("nesting too deep".into()));
        }
        let def = &self.defs[idx];
        match (def.kind, v) {
            (DefKind::Enum, KiwiValue::Enum(n)) => {
                let f = def.fields.iter().find(|f| f.name == *n).ok_or_else(|| KiwiError::Bad(format!("unknown enum variant {n}")))?;
                w.var_u64(f.value as u64);
            }
            (DefKind::Enum, KiwiValue::EnumRaw(x)) => w.var_u64(*x as u64),
            (DefKind::Struct, KiwiValue::Struct(fs)) => {
                for f in &def.fields {
                    let fv = fs.iter().find(|(n, _)| *n == f.name).map(|(_, v)| v).ok_or_else(|| KiwiError::Bad(format!("missing struct field {}", f.name)))?;
                    self.encode_field(f, fv, w, depth)?;
                }
            }
            (DefKind::Message, KiwiValue::Message(fs)) => {
                for (n, fv) in fs {
                    let f = def.fields.iter().find(|f| f.name == *n).ok_or_else(|| KiwiError::Bad(format!("unknown field {n}")))?;
                    w.var_u64(f.value as u64);
                    self.encode_field(f, fv, w, depth)?;
                }
                w.byte(0);
            }
            _ => return Err(KiwiError::Bad(format!("value does not match definition {}", def.name))),
        }
        Ok(())
    }

    fn encode_field(&self, f: &Field, v: &KiwiValue, w: &mut Writer, depth: usize) -> Result<()> {
        if !f.is_array {
            return self.encode_type(f.ty, v, w, depth + 1);
        }
        match v {
            KiwiValue::Bytes(b) => {
                w.var_u64(b.len() as u64);
                w.buf.extend_from_slice(b);
            }
            KiwiValue::Array(a) => {
                w.var_u64(a.len() as u64);
                for x in a {
                    self.encode_type(f.ty, x, w, depth + 1)?;
                }
            }
            _ => return Err(KiwiError::Bad(format!("field {} expects an array", f.name))),
        }
        Ok(())
    }

    fn encode_type(&self, ty: FieldType, v: &KiwiValue, w: &mut Writer, depth: usize) -> Result<()> {
        match (ty, v) {
            (FieldType::Native(Native::Bool), KiwiValue::Bool(b)) => w.byte(*b as u8),
            (FieldType::Native(Native::Byte), KiwiValue::Byte(b)) => w.byte(*b),
            (FieldType::Native(Native::Int), KiwiValue::Int(i)) => w.var_i64(*i as i64),
            (FieldType::Native(Native::Uint), KiwiValue::Uint(u)) => w.var_u64(*u as u64),
            (FieldType::Native(Native::Float), KiwiValue::Float(x)) => w.var_f32(*x),
            (FieldType::Native(Native::String), KiwiValue::String(s)) => w.string(s),
            (FieldType::Native(Native::Int64), KiwiValue::Int64(i)) => w.var_i64(*i),
            (FieldType::Native(Native::Uint64), KiwiValue::Uint64(u)) => w.var_u64(*u),
            (FieldType::Def(i), v) => self.encode_def(i, v, w, depth)?,
            _ => return Err(KiwiError::Bad("value does not match field type".into())),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- writer

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn byte(&mut self, b: u8) {
        self.buf.push(b);
    }
    pub fn var_u64(&mut self, mut v: u64) {
        // Mirrors `Reader::var_u64`: the ninth byte, if reached, holds the top 8 bits.
        for _ in 0..8 {
            if v < 0x80 {
                break;
            }
            self.buf.push((v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }
    pub fn var_i64(&mut self, v: i64) {
        self.var_u64(((v << 1) ^ (v >> 63)) as u64);
    }
    pub fn var_f32(&mut self, v: f32) {
        let bits = v.to_bits().rotate_left(9);
        if bits & 0xff == 0 {
            self.buf.push(0);
        } else {
            self.buf.extend_from_slice(&bits.to_le_bytes());
        }
    }
    pub fn string(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }
}

// ---------------------------------------------------------------- JSON

/// Converts a decoded value to JSON: enums by name, `byte[]` as lowercase hex,
/// int64/uint64 as numbers (strings when beyond 2^53).
pub fn to_json(v: &KiwiValue) -> serde_json::Value {
    use serde_json::{Number, Value};
    match v {
        KiwiValue::Bool(b) => Value::Bool(*b),
        KiwiValue::Byte(b) => Value::from(*b),
        KiwiValue::Int(i) => Value::from(*i),
        KiwiValue::Uint(u) => Value::from(*u),
        KiwiValue::Float(f) => clean_f32(*f),
        KiwiValue::String(s) => Value::String(s.clone()),
        KiwiValue::Int64(i) => {
            if i.unsigned_abs() < (1 << 53) {
                Value::from(*i)
            } else {
                Value::String(i.to_string())
            }
        }
        KiwiValue::Uint64(u) => {
            if *u < (1 << 53) {
                Value::from(*u)
            } else {
                Value::String(u.to_string())
            }
        }
        KiwiValue::Bytes(b) => Value::String(hex(b)),
        KiwiValue::Enum(n) => Value::String(n.to_string()),
        KiwiValue::EnumRaw(u) => Value::Number(Number::from(*u)),
        KiwiValue::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        KiwiValue::Struct(f) | KiwiValue::Message(f) => {
            let mut m = serde_json::Map::new();
            for (k, v) in f {
                m.insert(k.to_string(), to_json(v));
            }
            Value::Object(m)
        }
    }
}

/// f32 -> JSON number without f32->f64 widening noise (0.1f32 -> 0.1).
pub fn clean_f32(f: f32) -> serde_json::Value {
    if !f.is_finite() {
        return serde_json::Value::Null;
    }
    let s = format!("{f}");
    s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
}

pub fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        let _ = write!(s, "{x:02x}");
    }
    s
}

// ---------------------------------------------------------------- builder

/// Small helper for building schemas programmatically (tests, exporter).
#[derive(Default)]
pub struct SchemaBuilder {
    schema: Schema,
    names: BTreeMap<String, usize>,
}

impl SchemaBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn define(&mut self, name: &str, kind: DefKind) -> usize {
        let i = self.schema.defs.len();
        self.schema.defs.push(Def { name: name.into(), kind, fields: Vec::new() });
        self.names.insert(name.into(), i);
        i
    }
    pub fn index(&self, name: &str) -> usize {
        self.names[name]
    }
    pub fn field(&mut self, def: usize, name: &str, ty: FieldType, is_array: bool, value: u32) {
        self.schema.defs[def].fields.push(Field { name: name.into(), ty, is_array, value });
    }
    pub fn variant(&mut self, def: usize, name: &str, value: u32) {
        self.field(def, name, FieldType::Def(0), false, value);
    }
    pub fn build(self) -> Schema {
        self.schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        let mut w = Writer::default();
        for v in [0i64, 1, -1, 63, -64, 64, 1 << 31, i32::MIN as i64, i64::MAX, i64::MIN] {
            w.var_i64(v);
        }
        let mut r = Reader::new(&w.buf);
        for v in [0i64, 1, -1, 63, -64, 64, 1 << 31, i32::MIN as i64, i64::MAX, i64::MIN] {
            assert_eq!(r.var_i64().unwrap(), v);
        }
    }

    #[test]
    fn varint64_ninth_byte_is_full() {
        // kiwi encodes u64::MAX as nine 0xff bytes; a LEB128 decoder would read a tenth.
        let mut w = Writer::default();
        w.var_u64(u64::MAX);
        w.var_u64(5);
        assert_eq!(w.buf, [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 5]);
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.var_u64().unwrap(), u64::MAX);
        assert_eq!(r.var_u64().unwrap(), 5);
        assert!(r.at_end());

        // A random seed with the high bit set (Figma's `strokeSeed`), followed by another value.
        let seed = 15_639_266_553_043_455_682u64;
        let mut w = Writer::default();
        w.var_u64(seed);
        w.var_u64(48);
        assert_eq!(w.buf.len(), 10);
        let mut r = Reader::new(&w.buf);
        assert_eq!(r.var_u64().unwrap(), seed);
        assert_eq!(r.var_u64().unwrap(), 48);
    }

    #[test]
    fn float_roundtrip() {
        let vals = [0.0f32, 1.0, -1.0, 0.1, 123456.78, -3.5e-5, f32::MAX, 1e-30];
        let mut w = Writer::default();
        for v in vals {
            w.var_f32(v);
        }
        let mut r = Reader::new(&w.buf);
        for v in vals {
            assert_eq!(r.var_f32().unwrap(), v);
        }
        // Known kiwi encoding of 1.0: bits 0x3f800000 rotl 9 = 0x7f; high bytes...
        let mut w = Writer::default();
        w.var_f32(1.0);
        assert_eq!(w.buf.len(), 4);
        assert_eq!(w.buf[0], 127);
    }

    #[test]
    fn truncated_never_panics() {
        let mut b = SchemaBuilder::new();
        let m = b.define("Message", DefKind::Message);
        b.field(m, "a", FieldType::Native(Native::String), false, 1);
        b.field(m, "b", FieldType::Native(Native::Float), true, 2);
        let s = b.build();
        let enc = s.encode();
        for n in 0..enc.len() {
            let _ = Schema::decode(&enc[..n]);
        }
        let msg = KiwiValue::Message(vec![
            ("a".into(), KiwiValue::String("hi".into())),
            ("b".into(), KiwiValue::Array(vec![KiwiValue::Float(1.5), KiwiValue::Float(2.5)])),
        ]);
        let bytes = s.encode_message(0, &msg).unwrap();
        assert_eq!(s.decode_message(0, &bytes).unwrap(), msg);
        for n in 0..bytes.len() {
            assert!(s.decode_message(0, &bytes[..n]).is_err());
        }
    }
}
