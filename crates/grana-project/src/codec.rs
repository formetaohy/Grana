use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use grana_codec::{Decoder, Encoder, Fault};
use grana_store::SpillStore;

pub(crate) trait Encode {
    fn encode(&self, writer: &mut Encoder<'_>);
}

pub(crate) trait Decode: Sized {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, Fault>;
}

pub(crate) struct Reader<'a> {
    decoder: Decoder<'a>,
    store: Arc<SpillStore>,
}

impl<'a> Reader<'a> {
    pub fn new(
        source: &'a mut dyn Read,
        length: usize,
        store: Arc<SpillStore>,
        cancel: &'a AtomicBool,
        report: &'a mut dyn FnMut(usize),
    ) -> Self {
        Self {
            decoder: Decoder::stream(source, length, cancel, report),
            store,
        }
    }

    pub fn store(&self) -> &Arc<SpillStore> {
        &self.store
    }
}

impl<'a> Deref for Reader<'a> {
    type Target = Decoder<'a>;

    fn deref(&self) -> &Self::Target {
        &self.decoder
    }
}

impl DerefMut for Reader<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.decoder
    }
}

macro_rules! encode_primitive {
    ($type:ty, $method:ident) => {
        impl Encode for $type {
            fn encode(&self, writer: &mut Encoder<'_>) {
                writer.$method(*self);
            }
        }
    };
}

encode_primitive!(bool, bool);
encode_primitive!(u8, u8);
encode_primitive!(u16, u16);
encode_primitive!(u32, u32);
encode_primitive!(i32, i32);
encode_primitive!(u64, u64);
encode_primitive!(f32, f32);
encode_primitive!(f64, f64);

impl Encode for String {
    fn encode(&self, writer: &mut Encoder<'_>) {
        writer.string(self);
    }
}

impl<T: Encode + ?Sized> Encode for Arc<T> {
    fn encode(&self, writer: &mut Encoder<'_>) {
        (**self).encode(writer);
    }
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, writer: &mut Encoder<'_>) {
        (**self).encode(writer);
    }
}

macro_rules! decode_primitive {
    ($type:ty, $method:ident) => {
        impl Decode for $type {
            fn decode(reader: &mut Reader<'_>) -> Result<Self, Fault> {
                reader.$method()
            }
        }
    };
}

decode_primitive!(bool, bool);
decode_primitive!(u8, u8);
decode_primitive!(u16, u16);
decode_primitive!(u32, u32);
decode_primitive!(i32, i32);
decode_primitive!(u64, u64);
decode_primitive!(f32, f32);
decode_primitive!(f64, f64);
decode_primitive!(String, string);

impl<T: Decode> Decode for Arc<T> {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, Fault> {
        Ok(Arc::new(T::decode(reader)?))
    }
}
