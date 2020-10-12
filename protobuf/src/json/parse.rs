use std::num::ParseFloatError;
use std::num::ParseIntError;

use std::f32;
use std::f64;
use std::fmt;

use super::base64;

use crate::enums::ProtobufEnum;
use crate::json::base64::FromBase64Error;
use crate::message::Message;
use crate::reflect::EnumDescriptor;
use crate::reflect::EnumValueDescriptor;
use crate::reflect::FieldDescriptor;
use crate::reflect::MessageDescriptor;
use crate::reflect::ReflectValueBox;
use crate::reflect::RuntimeFieldType;
use crate::reflect::RuntimeTypeBox;
use crate::text_format::lexer::Lexer;
use crate::text_format::lexer::LexerError;
use crate::text_format::lexer::Loc;
use crate::text_format::lexer::ParserLanguage;
use crate::text_format::lexer::Token;
use crate::text_format::lexer::Tokenizer;
use crate::text_format::lexer::TokenizerError;

use super::float;
use super::rfc_3339;
use crate::text_format::lexer::JsonNumberLit;

use crate::json::well_known_wrapper::WellKnownWrapper;
use crate::message_dyn::MessageDyn;
use crate::well_known_types::value;
use crate::well_known_types::Any;
use crate::well_known_types::BoolValue;
use crate::well_known_types::BytesValue;
use crate::well_known_types::DoubleValue;
use crate::well_known_types::Duration;
use crate::well_known_types::FieldMask;
use crate::well_known_types::FloatValue;
use crate::well_known_types::Int32Value;
use crate::well_known_types::Int64Value;
use crate::well_known_types::ListValue;
use crate::well_known_types::NullValue;
use crate::well_known_types::StringValue;
use crate::well_known_types::Struct;
use crate::well_known_types::Timestamp;
use crate::well_known_types::UInt32Value;
use crate::well_known_types::UInt64Value;
use crate::well_known_types::Value;

#[derive(Debug)]
enum ParseErrorWithoutLocInner {
    TokenizerError(TokenizerError),
    UnknownFieldName(String),
    UnknownEnumVariantName(String),
    UnknownEnumVariantNumber(i32),
    FromBase64Error(FromBase64Error),
    IncorrectStrLit(LexerError),
    IncorrectDuration,
    Rfc3339(rfc_3339::Rfc3339ParseError),
    ParseIntError(ParseIntError),
    ParseFloatError(ParseFloatError),
    ExpectingBool,
    ExpectingStrOrInt,
    ExpectingNumber,
    UnexpectedToken,
    AnyParsingIsNotImplemented,
    MessageNotInitialized,
}

/// JSON parse error.
#[derive(Debug)]
struct ParseErrorWithoutLoc(ParseErrorWithoutLocInner);

impl fmt::Display for ParseErrorWithoutLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ParseErrorWithoutLocInner::TokenizerError(e) => write!(f, "{}", e),
            ParseErrorWithoutLocInner::UnknownFieldName(n) => {
                write!(f, "unknown field name: {}", n)
            }
            ParseErrorWithoutLocInner::UnknownEnumVariantName(n) => {
                write!(f, "unknown enum variant name: {}", n)
            }
            ParseErrorWithoutLocInner::UnknownEnumVariantNumber(n) => {
                write!(f, "unknown enum value: {}", n)
            }
            ParseErrorWithoutLocInner::FromBase64Error(e) => write!(f, "{}", e),
            ParseErrorWithoutLocInner::IncorrectStrLit(e) => write!(f, "{}", e),
            ParseErrorWithoutLocInner::IncorrectDuration => write!(f, "incorrect duration"),
            ParseErrorWithoutLocInner::Rfc3339(e) => write!(f, "RFC3339 parse error: {}", e),
            ParseErrorWithoutLocInner::ParseIntError(e) => write!(f, "{}", e),
            ParseErrorWithoutLocInner::ParseFloatError(e) => write!(f, "{}", e),
            ParseErrorWithoutLocInner::ExpectingBool => write!(f, "expecting bool"),
            ParseErrorWithoutLocInner::ExpectingStrOrInt => {
                write!(f, "expecting string or integer")
            }
            ParseErrorWithoutLocInner::ExpectingNumber => write!(f, "expecting number"),
            ParseErrorWithoutLocInner::UnexpectedToken => write!(f, "unexpected token"),
            ParseErrorWithoutLocInner::AnyParsingIsNotImplemented => {
                write!(f, "Any parsing is not implemented")
            }
            ParseErrorWithoutLocInner::MessageNotInitialized => {
                write!(f, "Message not initialized")
            }
        }
    }
}

impl std::error::Error for ParseErrorWithoutLoc {}

impl From<TokenizerError> for ParseErrorWithoutLoc {
    fn from(e: TokenizerError) -> Self {
        ParseErrorWithoutLoc(ParseErrorWithoutLocInner::TokenizerError(e))
    }
}

impl From<FromBase64Error> for ParseErrorWithoutLoc {
    fn from(e: FromBase64Error) -> Self {
        ParseErrorWithoutLoc(ParseErrorWithoutLocInner::FromBase64Error(e))
    }
}

impl From<ParseIntError> for ParseErrorWithoutLoc {
    fn from(e: ParseIntError) -> Self {
        ParseErrorWithoutLoc(ParseErrorWithoutLocInner::ParseIntError(e))
    }
}

impl From<ParseFloatError> for ParseErrorWithoutLoc {
    fn from(e: ParseFloatError) -> Self {
        ParseErrorWithoutLoc(ParseErrorWithoutLocInner::ParseFloatError(e))
    }
}

impl From<rfc_3339::Rfc3339ParseError> for ParseErrorWithoutLoc {
    fn from(e: rfc_3339::Rfc3339ParseError) -> Self {
        ParseErrorWithoutLoc(ParseErrorWithoutLocInner::Rfc3339(e))
    }
}

/// JSON parse error
#[derive(Debug)]
pub struct ParseError {
    error: ParseErrorWithoutLoc,
    loc: Loc,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.error, self.loc)
    }
}

impl std::error::Error for ParseError {}

type ParseResultWithoutLoc<A> = Result<A, ParseErrorWithoutLoc>;
type ParseResult<A> = Result<A, ParseError>;

#[derive(Clone)]
struct Parser<'a> {
    tokenizer: Tokenizer<'a>,
    parse_options: ParseOptions,
}

trait FromJsonNumber: PartialEq + Sized {
    fn from_f64(v: f64) -> Self;
    fn to_f64(&self) -> f64;
    fn from_string(v: &str) -> ParseResultWithoutLoc<Self>;
}

impl FromJsonNumber for u32 {
    fn from_f64(v: f64) -> Self {
        v as u32
    }

    fn to_f64(&self) -> f64 {
        *self as f64
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        Ok(v.parse()?)
    }
}

impl FromJsonNumber for u64 {
    fn from_f64(v: f64) -> Self {
        v as u64
    }

    fn to_f64(&self) -> f64 {
        *self as f64
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        Ok(v.parse()?)
    }
}

impl FromJsonNumber for i32 {
    fn from_f64(v: f64) -> Self {
        v as i32
    }

    fn to_f64(&self) -> f64 {
        *self as f64
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        Ok(v.parse()?)
    }
}

impl FromJsonNumber for i64 {
    fn from_f64(v: f64) -> Self {
        v as i64
    }

    fn to_f64(&self) -> f64 {
        *self as f64
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        Ok(v.parse()?)
    }
}

impl FromJsonNumber for f32 {
    fn from_f64(v: f64) -> Self {
        v as f32
    }

    fn to_f64(&self) -> f64 {
        *self as f64
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        if v == float::PROTOBUF_JSON_INF {
            Ok(f32::INFINITY)
        } else if v == float::PROTOBUF_JSON_MINUS_INF {
            Ok(f32::NEG_INFINITY)
        } else if v == float::PROTOBUF_JSON_NAN {
            Ok(f32::NAN)
        } else {
            Ok(v.parse()?)
        }
    }
}

impl FromJsonNumber for f64 {
    fn from_f64(v: f64) -> Self {
        v
    }

    fn to_f64(&self) -> f64 {
        *self
    }

    fn from_string(v: &str) -> Result<Self, ParseErrorWithoutLoc> {
        if v == float::PROTOBUF_JSON_INF {
            Ok(f64::INFINITY)
        } else if v == float::PROTOBUF_JSON_MINUS_INF {
            Ok(f64::NEG_INFINITY)
        } else if v == float::PROTOBUF_JSON_NAN {
            Ok(f64::NAN)
        } else {
            Ok(v.parse()?)
        }
    }
}

impl<'a> Parser<'a> {
    fn read_bool(&mut self) -> ParseResultWithoutLoc<bool> {
        if self.tokenizer.next_ident_if_eq("true")? {
            Ok(true)
        } else if self.tokenizer.next_ident_if_eq("false")? {
            Ok(false)
        } else {
            Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::ExpectingBool,
            ))
        }
    }

    fn parse_bool(&self, s: &str) -> ParseResultWithoutLoc<bool> {
        if s == "true" {
            Ok(true)
        } else if s == "false" {
            Ok(false)
        } else {
            Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::ExpectingBool,
            ))
        }
    }

    fn read_json_number_opt(&mut self) -> ParseResultWithoutLoc<Option<JsonNumberLit>> {
        Ok(self.tokenizer.next_token_if_map(|t| match t {
            Token::JsonNumber(v) => Some(v.clone()),
            _ => None,
        })?)
    }

    fn read_number<V: FromJsonNumber>(&mut self) -> ParseResultWithoutLoc<V> {
        if let Some(v) = self.read_json_number_opt()? {
            V::from_string(&v.0)
        } else if self.tokenizer.lookahead_is_str_lit()? {
            let v = self.read_string()?;
            self.parse_number(&v)
        } else {
            Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::ExpectingNumber,
            ))
        }
    }

    fn parse_number<V: FromJsonNumber>(&self, s: &str) -> ParseResultWithoutLoc<V> {
        V::from_string(s)
    }

    fn merge_wrapper<W>(&mut self, w: &mut W) -> ParseResultWithoutLoc<()>
    where
        W: WellKnownWrapper,
        W::Underlying: FromJsonNumber,
    {
        *w.get_mut() = self.read_number()?;
        Ok(())
    }

    fn merge_bool_value(&mut self, w: &mut BoolValue) -> ParseResultWithoutLoc<()> {
        w.value = self.read_bool()?;
        Ok(())
    }

    fn merge_string_value(&mut self, w: &mut StringValue) -> ParseResultWithoutLoc<()> {
        w.value = self.read_string()?;
        Ok(())
    }

    fn merge_bytes_value(&mut self, w: &mut BytesValue) -> ParseResultWithoutLoc<()> {
        w.value = self.read_bytes()?;
        Ok(())
    }

    fn read_u32(&mut self) -> ParseResultWithoutLoc<u32> {
        self.read_number()
    }

    fn read_u64(&mut self) -> ParseResultWithoutLoc<u64> {
        self.read_number()
    }

    fn read_i32(&mut self) -> ParseResultWithoutLoc<i32> {
        self.read_number()
    }

    fn read_i64(&mut self) -> ParseResultWithoutLoc<i64> {
        self.read_number()
    }

    fn read_f32(&mut self) -> ParseResultWithoutLoc<f32> {
        self.read_number()
    }

    fn read_f64(&mut self) -> ParseResultWithoutLoc<f64> {
        self.read_number()
    }

    fn read_string(&mut self) -> ParseResultWithoutLoc<String> {
        let str_lit = self.tokenizer.next_str_lit()?;

        let mut lexer = Lexer::new(&str_lit.escaped, ParserLanguage::Json);
        let mut r = String::new();
        while !lexer.eof() {
            r.push(
                lexer
                    .next_json_char_value()
                    .map_err(ParseErrorWithoutLocInner::IncorrectStrLit)
                    .map_err(ParseErrorWithoutLoc)?,
            );
        }
        Ok(r)
    }

    fn read_bytes(&mut self) -> ParseResultWithoutLoc<Vec<u8>> {
        let s = self.read_string()?;
        self.parse_bytes(&s)
    }

    fn parse_bytes(&self, s: &str) -> ParseResultWithoutLoc<Vec<u8>> {
        Ok(base64::decode(s)?)
    }

    fn read_enum(
        &mut self,
        descriptor: &EnumDescriptor,
    ) -> ParseResultWithoutLoc<EnumValueDescriptor> {
        if descriptor.is::<NullValue>() {
            return Ok(self.read_wk_null_value()?.descriptor());
        }

        if self.tokenizer.lookahead_is_str_lit()? {
            let name = self.read_string()?;
            self.parse_enum(name, descriptor)
        } else if self.tokenizer.lookahead_is_json_number()? {
            let number = self.read_i32()?;
            match descriptor.get_value_by_number(number) {
                Some(v) => Ok(v),
                // TODO: EnumValueOrUnknown
                None => Err(ParseErrorWithoutLoc(
                    ParseErrorWithoutLocInner::UnknownEnumVariantNumber(number),
                )),
            }
        } else {
            Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::ExpectingStrOrInt,
            ))
        }
    }

    fn parse_enum(
        &self,
        name: String,
        descriptor: &EnumDescriptor,
    ) -> ParseResultWithoutLoc<EnumValueDescriptor> {
        // TODO: can map key be int
        match descriptor.get_value_by_name(&name) {
            Some(v) => Ok(v),
            None => Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::UnknownEnumVariantName(name),
            )),
        }
    }

    fn read_wk_null_value(&mut self) -> ParseResultWithoutLoc<NullValue> {
        self.tokenizer.next_ident_expect_eq("null")?;
        Ok(NullValue::NULL_VALUE)
    }

    fn read_message(
        &mut self,
        descriptor: &MessageDescriptor,
    ) -> ParseResultWithoutLoc<Box<dyn MessageDyn>> {
        let mut m = descriptor.new_instance();
        self.merge_inner(&mut *m)?;
        Ok(m)
    }

    fn read_value(&mut self, t: &RuntimeTypeBox) -> ParseResultWithoutLoc<ReflectValueBox> {
        match t {
            RuntimeTypeBox::I32 => self.read_i32().map(ReflectValueBox::from),
            RuntimeTypeBox::I64 => self.read_i64().map(ReflectValueBox::from),
            RuntimeTypeBox::U32 => self.read_u32().map(ReflectValueBox::from),
            RuntimeTypeBox::U64 => self.read_u64().map(ReflectValueBox::from),
            RuntimeTypeBox::F32 => self.read_f32().map(ReflectValueBox::from),
            RuntimeTypeBox::F64 => self.read_f64().map(ReflectValueBox::from),
            RuntimeTypeBox::Bool => self.read_bool().map(ReflectValueBox::from),
            RuntimeTypeBox::String => self.read_string().map(ReflectValueBox::from),
            RuntimeTypeBox::VecU8 => self.read_bytes().map(ReflectValueBox::from),
            RuntimeTypeBox::Enum(e) => self.read_enum(&e).map(ReflectValueBox::from),
            RuntimeTypeBox::Message(m) => self.read_message(&m).map(ReflectValueBox::from),
        }
    }

    fn merge_singular_field(
        &mut self,
        message: &mut dyn MessageDyn,
        field: &FieldDescriptor,
        t: &RuntimeTypeBox,
    ) -> ParseResultWithoutLoc<()> {
        field.set_singular_field(message, self.read_value(t)?);
        Ok(())
    }

    fn read_list<C>(&mut self, mut read_item: C) -> ParseResultWithoutLoc<()>
    where
        C: for<'b> FnMut(&'b mut Self) -> ParseResultWithoutLoc<()>,
    {
        if self.tokenizer.next_ident_if_eq("null")? {
            return Ok(());
        }

        // TODO: better error reporting on wrong field type
        self.tokenizer.next_symbol_expect_eq('[')?;
        let mut first = true;
        while !self.tokenizer.next_symbol_if_eq(']')? {
            if !first {
                self.tokenizer.next_symbol_expect_eq(',')?;
            }
            first = false;

            read_item(self)?;
        }

        Ok(())
    }

    fn merge_repeated_field(
        &mut self,
        message: &mut dyn MessageDyn,
        field: &FieldDescriptor,
        t: &RuntimeTypeBox,
    ) -> ParseResultWithoutLoc<()> {
        let mut repeated = field.mut_repeated(message);
        repeated.clear();

        self.read_list(|s| {
            repeated.push(s.read_value(t)?);
            Ok(())
        })
    }

    fn merge_wk_list_value(&mut self, list: &mut ListValue) -> ParseResultWithoutLoc<()> {
        list.values.clear();

        self.read_list(|s| {
            list.values.push(s.read_wk_value()?);
            Ok(())
        })
    }

    fn read_map<K, Fk, Fi>(
        &mut self,
        mut parse_key: Fk,
        mut read_value_and_insert: Fi,
    ) -> ParseResultWithoutLoc<()>
    where
        Fk: for<'b> FnMut(&Self, String) -> ParseResultWithoutLoc<K>,
        Fi: for<'b> FnMut(&mut Self, K) -> ParseResultWithoutLoc<()>,
    {
        if self.tokenizer.next_ident_if_eq("null")? {
            return Ok(());
        }

        self.tokenizer.next_symbol_expect_eq('{')?;
        let mut first = true;
        while !self.tokenizer.next_symbol_if_eq('}')? {
            if !first {
                self.tokenizer.next_symbol_expect_eq(',')?;
            }
            first = false;

            let key_string = self.read_string()?;
            let k = parse_key(self, key_string)?;

            self.tokenizer.next_symbol_expect_eq(':')?;
            read_value_and_insert(self, k)?;
        }

        Ok(())
    }

    fn parse_key(&self, key: String, t: &RuntimeTypeBox) -> ParseResultWithoutLoc<ReflectValueBox> {
        match t {
            RuntimeTypeBox::I32 => self.parse_number::<i32>(&key).map(ReflectValueBox::I32),
            RuntimeTypeBox::I64 => self.parse_number::<i64>(&key).map(ReflectValueBox::I64),
            RuntimeTypeBox::U32 => self.parse_number::<u32>(&key).map(ReflectValueBox::U32),
            RuntimeTypeBox::U64 => self.parse_number::<u64>(&key).map(ReflectValueBox::U64),
            RuntimeTypeBox::Bool => self.parse_bool(&key).map(ReflectValueBox::Bool),
            RuntimeTypeBox::String => Ok(ReflectValueBox::String(key)),
            t @ RuntimeTypeBox::F32
            | t @ RuntimeTypeBox::F64
            | t @ RuntimeTypeBox::VecU8
            | t @ RuntimeTypeBox::Enum(..) => panic!("{} cannot be a map key", t),
            RuntimeTypeBox::Message(_) => panic!("message cannot be a map key"),
        }
    }

    fn merge_map_field(
        &mut self,
        message: &mut dyn MessageDyn,
        field: &FieldDescriptor,
        kt: &RuntimeTypeBox,
        vt: &RuntimeTypeBox,
    ) -> ParseResultWithoutLoc<()> {
        let mut map = field.mut_map(message);
        map.clear();

        self.read_map(
            |ss, s| ss.parse_key(s, kt),
            |s, k| {
                let v = s.read_value(vt)?;
                map.insert(k, v);
                Ok(())
            },
        )
    }

    fn merge_wk_struct(&mut self, struct_value: &mut Struct) -> ParseResultWithoutLoc<()> {
        struct_value.fields.clear();

        self.read_map(
            |_, s| Ok(s),
            |s, k| {
                let v = s.read_wk_value()?;
                struct_value.fields.insert(k, v);
                Ok(())
            },
        )
    }

    fn skip_json_value(&mut self) -> ParseResultWithoutLoc<()> {
        if self
            .tokenizer
            .next_ident_if_in(&["true", "false", "null"])?
            .is_some()
        {
        } else if self.tokenizer.lookahead_is_str_lit()? {
            self.tokenizer.next_str_lit()?;
        } else if self.tokenizer.lookahead_is_json_number()? {
            self.read_json_number_opt()?;
        } else if self.tokenizer.lookahead_is_symbol('[')? {
            self.read_list(|s| s.skip_json_value())?;
        } else if self.tokenizer.lookahead_is_symbol('{')? {
            self.read_map(|_, _| Ok(()), |s, ()| s.skip_json_value())?;
        } else {
            return Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::UnexpectedToken,
            ));
        }
        Ok(())
    }

    fn merge_field(
        &mut self,
        message: &mut dyn MessageDyn,
        field: &FieldDescriptor,
    ) -> ParseResultWithoutLoc<()> {
        match field.runtime_field_type() {
            RuntimeFieldType::Singular(t) => self.merge_singular_field(message, field, &t),
            RuntimeFieldType::Repeated(t) => self.merge_repeated_field(message, field, &t),
            RuntimeFieldType::Map(kt, vt) => self.merge_map_field(message, field, &kt, &vt),
        }
    }

    fn merge_inner(&mut self, message: &mut dyn MessageDyn) -> ParseResultWithoutLoc<()> {
        if let Some(duration) = message.downcast_mut() {
            return self.merge_wk_duration(duration);
        }

        if let Some(timestamp) = message.downcast_mut() {
            return self.merge_wk_timestamp(timestamp);
        }

        if let Some(field_mask) = message.downcast_mut() {
            return self.merge_wk_field_mask(field_mask);
        }

        if let Some(value) = message.downcast_mut() {
            return self.merge_wk_value(value);
        }

        if let Some(value) = message.downcast_mut() {
            return self.merge_wk_any(value);
        }

        if let Some(value) = message.downcast_mut::<DoubleValue>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<FloatValue>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<Int64Value>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<UInt64Value>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<Int32Value>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<UInt32Value>() {
            return self.merge_wrapper(value);
        }

        if let Some(value) = message.downcast_mut::<BoolValue>() {
            return self.merge_bool_value(value);
        }

        if let Some(value) = message.downcast_mut::<StringValue>() {
            return self.merge_string_value(value);
        }

        if let Some(value) = message.downcast_mut::<BytesValue>() {
            return self.merge_bytes_value(value);
        }

        if let Some(value) = message.downcast_mut::<ListValue>() {
            return self.merge_wk_list_value(value);
        }

        if let Some(value) = message.downcast_mut::<Struct>() {
            return self.merge_wk_struct(value);
        }

        let descriptor = message.descriptor_dyn();

        self.tokenizer.next_symbol_expect_eq('{')?;
        let mut first = true;
        while !self.tokenizer.next_symbol_if_eq('}')? {
            if !first {
                self.tokenizer.next_symbol_expect_eq(',')?;
            }
            first = false;

            let field_name = self.read_string()?;
            // Proto3 JSON parsers are required to accept both
            // the converted `lowerCamelCase` name and the proto field name.
            match descriptor.get_field_by_name_or_json_name(&field_name) {
                Some(field) => {
                    self.tokenizer.next_symbol_expect_eq(':')?;
                    self.merge_field(message, &field)?;
                }
                None if self.parse_options.ignore_unknown_fields => {
                    self.tokenizer.next_symbol_expect_eq(':')?;
                    self.skip_json_value()?;
                }
                None => {
                    return Err(ParseErrorWithoutLoc(
                        ParseErrorWithoutLocInner::UnknownFieldName(field_name),
                    ))
                }
            };
        }
        Ok(())
    }

    fn merge_wk_duration(&mut self, duration: &mut Duration) -> ParseResultWithoutLoc<()> {
        let s = self.read_string()?;
        let mut lexer = Lexer::new(&s, ParserLanguage::Json);

        fn next_dec(lexer: &mut Lexer) -> ParseResultWithoutLoc<(u64, u32)> {
            let s = lexer.take_while(|c| c >= '0' && c <= '9');

            if s.len() == 0 {
                Ok((0, 0))
            } else {
                match s.parse() {
                    Ok(n) => Ok((n, s.len() as u32)),
                    Err(_) => Err(ParseErrorWithoutLoc(
                        ParseErrorWithoutLocInner::IncorrectDuration,
                    )),
                }
            }
        }

        let minus = lexer.next_char_if_eq('-');
        let seconds = match next_dec(&mut lexer)? {
            (_, 0) => {
                return Err(ParseErrorWithoutLoc(
                    ParseErrorWithoutLocInner::IncorrectDuration,
                ))
            }
            (s, _) => s,
        };
        let nanos = if lexer.next_char_if_eq('.') {
            let (mut a, mut b) = next_dec(&mut lexer)?;
            if b > 9 {
                return Err(ParseErrorWithoutLoc(
                    ParseErrorWithoutLocInner::IncorrectDuration,
                ));
            }
            while b != 9 {
                b += 1;
                a *= 10;
            }

            if a > 999_999_999 {
                return Err(ParseErrorWithoutLoc(
                    ParseErrorWithoutLocInner::IncorrectDuration,
                ));
            }

            a
        } else {
            0
        };

        // The suffix "s" is required
        if !lexer.next_char_if_eq('s') {
            return Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::IncorrectDuration,
            ));
        }

        if !lexer.eof() {
            return Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::IncorrectDuration,
            ));
        }

        if minus {
            duration.seconds = -(seconds as i64);
            duration.nanos = -(nanos as i32);
        } else {
            duration.seconds = seconds as i64;
            duration.nanos = nanos as i32;
        }
        Ok(())
    }

    fn merge_wk_timestamp(&mut self, timestamp: &mut Timestamp) -> ParseResultWithoutLoc<()> {
        let s = self.read_string()?;
        let (seconds, nanos) = rfc_3339::TmUtc::parse_rfc_3339(&s)?;
        timestamp.seconds = seconds;
        timestamp.nanos = nanos as i32;
        Ok(())
    }

    fn merge_wk_field_mask(&mut self, field_mask: &mut FieldMask) -> ParseResultWithoutLoc<()> {
        let s = self.read_string()?;
        if !s.is_empty() {
            field_mask.paths = s.split(',').map(|s| s.to_owned()).collect();
        }
        Ok(())
    }

    fn read_wk_list_value(&mut self) -> ParseResultWithoutLoc<ListValue> {
        let mut r = ListValue::new();
        self.merge_wk_list_value(&mut r)?;
        Ok(r)
    }

    fn read_wk_struct(&mut self) -> ParseResultWithoutLoc<Struct> {
        let mut r = Struct::new();
        self.merge_wk_struct(&mut r)?;
        Ok(r)
    }

    fn merge_wk_value(&mut self, value: &mut Value) -> ParseResultWithoutLoc<()> {
        if self.tokenizer.lookahead_is_ident("null")? {
            value.kind = Some(value::Kind::null_value(self.read_wk_null_value()?.into()));
        } else if self.tokenizer.lookahead_is_ident("true")?
            || self.tokenizer.lookahead_is_ident("false")?
        {
            value.kind = Some(value::Kind::bool_value(self.read_bool()?));
        } else if self.tokenizer.lookahead_is_json_number()? {
            value.kind = Some(value::Kind::number_value(self.read_f64()?));
        } else if self.tokenizer.lookahead_is_str_lit()? {
            value.kind = Some(value::Kind::string_value(self.read_string()?));
        } else if self.tokenizer.lookahead_is_symbol('[')? {
            value.kind = Some(value::Kind::list_value(self.read_wk_list_value()?));
        } else if self.tokenizer.lookahead_is_symbol('{')? {
            value.kind = Some(value::Kind::struct_value(self.read_wk_struct()?));
        } else {
            return Err(ParseErrorWithoutLoc(
                ParseErrorWithoutLocInner::UnexpectedToken,
            ));
        }
        Ok(())
    }

    fn merge_wk_any(&mut self, _value: &mut Any) -> ParseResultWithoutLoc<()> {
        Err(ParseErrorWithoutLoc(
            ParseErrorWithoutLocInner::AnyParsingIsNotImplemented,
        ))
    }

    fn read_wk_value(&mut self) -> ParseResultWithoutLoc<Value> {
        let mut v = Value::new();
        self.merge_wk_value(&mut v)?;
        Ok(v)
    }

    fn merge(&mut self, message: &mut dyn MessageDyn) -> ParseResult<()> {
        match self.merge_inner(message) {
            Ok(()) => Ok(()),
            Err(error) => Err(ParseError {
                error,
                loc: self.tokenizer.loc(),
            }),
        }
    }
}

/// JSON parse options.
///
/// # Examples
///
/// ```
/// use protobuf::json;
/// let parse_options = json::ParseOptions {
///     ignore_unknown_fields: true,
///     ..Default::default()
/// };
/// ```
#[derive(Default, Debug, Clone)]
pub struct ParseOptions {
    /// Ignore unknown fields when parsing.
    ///
    /// When `true` fields with unknown names are ignored.
    /// When `false` parser returns an error on unknown field.
    pub ignore_unknown_fields: bool,
    /// Prevent initializing `ParseOptions` enumerating all field.
    pub _future_options: (),
}

/// Merge JSON into provided message
pub fn merge_from_str_with_options(
    message: &mut dyn MessageDyn,
    json: &str,
    parse_options: &ParseOptions,
) -> ParseResult<()> {
    let mut parser = Parser {
        tokenizer: Tokenizer::new(json, ParserLanguage::Json),
        parse_options: parse_options.clone(),
    };
    parser.merge(message)
}

/// Merge JSON into provided message
pub fn merge_from_str(message: &mut dyn MessageDyn, json: &str) -> ParseResult<()> {
    merge_from_str_with_options(message, json, &ParseOptions::default())
}

/// Parse JSON to protobuf message.
pub fn parse_dynamic_from_str_with_options(
    d: &MessageDescriptor,
    json: &str,
    parse_options: &ParseOptions,
) -> ParseResult<Box<dyn MessageDyn>> {
    let mut m = d.new_instance();
    merge_from_str_with_options(&mut *m, json, parse_options)?;
    if let Err(_) = m.check_initialized_dyn() {
        return Err(ParseError {
            error: ParseErrorWithoutLoc(ParseErrorWithoutLocInner::MessageNotInitialized),
            loc: Loc::start(),
        });
    }
    Ok(m)
}

/// Parse JSON to protobuf message.
pub fn parse_dynamic_from_str(
    d: &MessageDescriptor,
    json: &str,
) -> ParseResult<Box<dyn MessageDyn>> {
    parse_dynamic_from_str_with_options(d, json, &ParseOptions::default())
}

/// Parse JSON to protobuf message.
pub fn parse_from_str_with_options<M: Message>(
    json: &str,
    parse_options: &ParseOptions,
) -> ParseResult<M> {
    let m = parse_dynamic_from_str_with_options(&M::descriptor_static(), json, parse_options)?;
    Ok(*m.downcast_box().unwrap())
}

/// Parse JSON to protobuf message.
pub fn parse_from_str<M: Message>(json: &str) -> ParseResult<M> {
    parse_from_str_with_options(json, &ParseOptions::default())
}
