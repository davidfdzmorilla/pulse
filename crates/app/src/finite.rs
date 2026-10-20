//! Finding the numbers JSON can't hold. serde_json writes a NaN or an infinity
//! as `null`, which then doesn't read back as a number: one bad value (from a
//! Live set, a plugin, a division by zero) would leave a project that doesn't
//! open. Before writing, [`repair`] finds them by walking the value the way it
//! is serialized, so each one's place in the JSON is known, and puts 0 there.

use serde::ser::{self, Serialize};
use serde_json::Value;

/// One step into a JSON value: an object's key or an array's index.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Key(String),
    Index(usize),
}

/// Where `value` (serialized as JSON) has NaN or infinite numbers.
pub fn non_finite<T: Serialize + ?Sized>(value: &T) -> Vec<Vec<Step>> {
    let mut finder = Finder { path: Vec::new(), found: Vec::new() };
    // The finder itself never fails; a type's own Serialize might.
    let _ = value.serialize(&mut finder);
    finder.found
}

/// The JSON of `value` with every non-finite number replaced by 0, and how many
/// there were.
pub fn repair<T: Serialize + ?Sized>(value: &T) -> Result<(Value, usize), String> {
    let mut json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let found = non_finite(value);
    for path in &found {
        let mut at = &mut json;
        for step in path {
            at = match (step, at) {
                (Step::Key(key), Value::Object(map)) => map.get_mut(key).ok_or_else(|| format!("{key}: no está"))?,
                (Step::Index(i), Value::Array(items)) => items.get_mut(*i).ok_or_else(|| format!("[{i}]: no está"))?,
                (step, _) => return Err(format!("No se encontró un valor no válido ({step:?})")),
            };
        }
        *at = Value::from(0.0);
    }
    Ok((json, found.len()))
}

struct Finder {
    path: Vec<Step>,
    found: Vec<Vec<Step>>,
}

impl Finder {
    fn number(&mut self, finite: bool) -> Result<(), Never> {
        if !finite {
            self.found.push(self.path.clone());
        }
        Ok(())
    }

    fn inside<T: Serialize + ?Sized>(&mut self, step: Step, value: &T) -> Result<(), Never> {
        self.path.push(step);
        let result = value.serialize(&mut *self);
        self.path.pop();
        result
    }
}

/// The finder's error: types may still report their own.
#[derive(Debug)]
pub struct Never(String);

impl std::fmt::Display for Never {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Never {}

impl ser::Error for Never {
    fn custom<M: std::fmt::Display>(message: M) -> Self {
        Never(message.to_string())
    }
}

/// A sequence, a struct or a map being walked; `variant` was pushed for an
/// enum's variant (`{"Variant": …}`) and is popped at the end.
pub struct Compound<'a> {
    finder: &'a mut Finder,
    index: usize,
    key: Option<String>,
    variant: bool,
}

impl Compound<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> {
        let index = self.index;
        self.index += 1;
        self.finder.inside(Step::Index(index), value)
    }

    fn field<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<(), Never> {
        self.finder.inside(Step::Key(key.to_string()), value)
    }

    fn end(self) -> Result<(), Never> {
        if self.variant {
            self.finder.path.pop();
        }
        Ok(())
    }
}

/// A map key as JSON writes it: strings as they are, numbers and the rest as text.
fn key_text<T: Serialize + ?Sized>(key: &T) -> String {
    match serde_json::to_value(key) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

impl<'a> ser::Serializer for &'a mut Finder {
    type Ok = ();
    type Error = Never;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, _: bool) -> Result<(), Never> { Ok(()) }
    fn serialize_i8(self, _: i8) -> Result<(), Never> { Ok(()) }
    fn serialize_i16(self, _: i16) -> Result<(), Never> { Ok(()) }
    fn serialize_i32(self, _: i32) -> Result<(), Never> { Ok(()) }
    fn serialize_i64(self, _: i64) -> Result<(), Never> { Ok(()) }
    fn serialize_u8(self, _: u8) -> Result<(), Never> { Ok(()) }
    fn serialize_u16(self, _: u16) -> Result<(), Never> { Ok(()) }
    fn serialize_u32(self, _: u32) -> Result<(), Never> { Ok(()) }
    fn serialize_u64(self, _: u64) -> Result<(), Never> { Ok(()) }
    fn serialize_f32(self, v: f32) -> Result<(), Never> { self.number(v.is_finite()) }
    fn serialize_f64(self, v: f64) -> Result<(), Never> { self.number(v.is_finite()) }
    fn serialize_char(self, _: char) -> Result<(), Never> { Ok(()) }
    fn serialize_str(self, _: &str) -> Result<(), Never> { Ok(()) }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), Never> { Ok(()) }
    fn serialize_none(self) -> Result<(), Never> { Ok(()) }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Never> { value.serialize(self) }
    fn serialize_unit(self) -> Result<(), Never> { Ok(()) }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Never> { Ok(()) }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<(), Never> { Ok(()) }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<(), Never> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> Result<(), Never> {
        self.inside(Step::Key(variant.to_string()), value)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Compound<'a>, Never> {
        Ok(Compound { finder: self, index: 0, key: None, variant: false })
    }

    fn serialize_tuple(self, _: usize) -> Result<Compound<'a>, Never> {
        Ok(Compound { finder: self, index: 0, key: None, variant: false })
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Never> {
        Ok(Compound { finder: self, index: 0, key: None, variant: false })
    }

    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Compound<'a>, Never> {
        self.path.push(Step::Key(variant.to_string()));
        Ok(Compound { finder: self, index: 0, key: None, variant: true })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Compound<'a>, Never> {
        Ok(Compound { finder: self, index: 0, key: None, variant: false })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Never> {
        Ok(Compound { finder: self, index: 0, key: None, variant: false })
    }

    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, _: usize) -> Result<Compound<'a>, Never> {
        self.path.push(Step::Key(variant.to_string()));
        Ok(Compound { finder: self, index: 0, key: None, variant: true })
    }
}

impl ser::SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> { self.element(value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> { self.element(value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> { self.element(value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> { self.element(value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Never> {
        self.key = Some(key_text(key));
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Never> {
        let key = self.key.take().unwrap_or_default();
        self.field(&key, value)
    }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Never> { self.field(key, value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Never> { self.field(key, value) }
    fn end(self) -> Result<(), Never> { Compound::end(self) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    enum Shape {
        Line(f32),
        Curve { bend: f64 },
        Pair(f64, f64),
        Flat,
    }

    #[derive(Serialize)]
    #[serde(untagged)]
    enum Either {
        Number(f64),
        Text(String),
    }

    #[derive(Serialize)]
    struct Thing {
        gain: f32,
        points: Vec<(f64, f32)>,
        shapes: Vec<Shape>,
        maybe: Option<f64>,
        either: Vec<Either>,
        named: std::collections::BTreeMap<u32, f64>,
        #[serde(flatten)]
        extra: Extra,
    }

    #[derive(Serialize)]
    struct Extra {
        depth: f64,
    }

    #[test]
    fn every_bad_number_is_found_where_json_has_it() {
        let thing = Thing {
            gain: f32::NAN,
            points: vec![(1.0, 0.5), (f64::INFINITY, 0.5)],
            shapes: vec![Shape::Flat, Shape::Line(f32::NEG_INFINITY), Shape::Curve { bend: f64::NAN }, Shape::Pair(0.0, f64::NAN)],
            maybe: Some(f64::NAN),
            either: vec![Either::Text("a".into()), Either::Number(f64::NAN)],
            named: [(7, f64::NAN)].into(),
            extra: Extra { depth: f64::INFINITY },
        };
        let (json, count) = repair(&thing).unwrap();
        assert_eq!(count, 9);
        let text = serde_json::to_string(&json).unwrap();
        assert!(!text.contains("null"), "{text}");
        assert_eq!(json["shapes"][2]["Curve"]["bend"], 0.0);
        assert_eq!(json["shapes"][3]["Pair"][1], 0.0);
        assert_eq!(json["either"][1], 0.0);
        assert_eq!(json["named"]["7"], 0.0);
        assert_eq!(json["depth"], 0.0);
        assert_eq!(json["points"][0][1], 0.5, "good numbers stay");
    }
}
