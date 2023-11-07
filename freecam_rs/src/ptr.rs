use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::UnsafeCell;
use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

/// Highly unsafe Cell type used for interfacing with game patches.
///
/// Patches would write to this memory, usually without synchronisation.
/// Breaks several Rust guarantees with regard to exclusive `mut` ownership. If something mis-compiles, this is likely to blame.
#[derive(Default, Debug)]
#[repr(transparent)]
pub struct GameCell<T: ?Sized>(UnsafeCell<T>);

impl<T> GameCell<T> {
    pub fn new(item: T) -> Self {
        Self(UnsafeCell::new(item))
    }

    pub unsafe fn as_ref(&self) -> &T {
        &*self.0.get()
    }

    pub unsafe fn as_mut(&self) -> &mut T {
        &mut *self.0.get()
    }

    pub const fn get_ptr(&self) -> *const T {
        self.0.get()
    }

    pub const fn get_mut_ptr(&self) -> *mut T {
        self.0.get()
    }
}

#[derive(serde::Deserialize, Clone, Copy, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct NonNullPtr<T = u8>(#[serde(deserialize_with = "from_hex")] pub NonNull<T>);

impl<T> Serialize for NonNullPtr<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("{:#X}", (self.0.as_ptr() as usize)).serialize(serializer)
    }
}

impl<T> Debug for NonNullPtr<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("NonNullPtr")
            .field(&format_args!("{:#X}", self.0.as_ptr() as usize))
            .finish()
    }
}

impl<T> Deref for NonNullPtr<T> {
    type Target = NonNull<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for NonNullPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<usize> for NonNullPtr<T> {
    fn from(value: usize) -> Self {
        Self(NonNull::new(value as *mut T).expect("Passed null pointer"))
    }
}

fn from_hex<'de, D, T>(deserializer: D) -> Result<NonNull<T>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    // do better hex decoding than this
    let value = usize::from_str_radix(&s[2..], 16).map_err(D::Error::custom)?;

    NonNull::new(value as *mut T).ok_or_else(|| D::Error::custom("Invalid pointer"))
}
