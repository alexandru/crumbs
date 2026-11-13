//! Shared text constraints and implementations for distinct domain types.

use std::hash::{Hash, Hasher};

use unicase::UniCase;
use unicode_normalization::UnicodeNormalization;

/// A trimmed display name with Unicode canonical caseless identity.
#[derive(Debug, Clone)]
pub(crate) struct CaselessName {
    display: String,
    key: String,
}

impl CaselessName {
    pub(crate) fn new(value: &str) -> Result<Self, TextError> {
        let display = value.trim();
        nonblank(display)?;

        let decomposed = display.nfd().collect::<String>();
        let key = UniCase::new(decomposed).to_folded_case().nfd().collect();
        Ok(Self {
            display: display.to_owned(),
            key,
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.display
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

impl PartialEq for CaselessName {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for CaselessName {}

impl Hash for CaselessName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextError {
    Blank,
    Nul,
}

pub(crate) fn nonblank(value: &str) -> Result<(), TextError> {
    if value.trim().is_empty() {
        Err(TextError::Blank)
    } else {
        Ok(())
    }
}

pub(crate) fn nonblank_without_nul(value: &str) -> Result<(), TextError> {
    nonblank(value)?;
    if value.contains('\0') {
        Err(TextError::Nul)
    } else {
        Ok(())
    }
}

macro_rules! string_serde {
    ($name:ident) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

macro_rules! string_newtype {
    (
        $(#[$type_doc:meta])* $vis:vis struct $name:ident;
        $(#[$new_doc:meta])* fn new -> $error:ty;
        validate = $validate:expr;
    ) => {
        $(#[$type_doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name(String);

        impl $name {
            $(#[$new_doc])*
            pub fn new(value: impl Into<String>) -> Result<Self, $error> {
                let value = value.into();
                ($validate)(&value)?;
                Ok(Self(value))
            }

            /// Returns the text as supplied.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = $error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        $crate::text::string_serde!($name);
    };
}

macro_rules! name_newtype {
    (
        $(#[$type_doc:meta])* $vis:vis struct $name:ident;
        $(#[$new_doc:meta])* fn new -> $error:ty;
        blank = $blank:expr;
        $key_vis:vis fn key;
    ) => {
        $(#[$type_doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        $vis struct $name($crate::text::CaselessName);

        impl $name {
            $(#[$new_doc])*
            pub fn new(value: impl AsRef<str>) -> Result<Self, $error> {
                $crate::text::CaselessName::new(value.as_ref())
                    .map(Self)
                    .map_err(|_| $blank)
            }

            /// Returns the trimmed display spelling.
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            $key_vis fn key(&self) -> &str {
                self.0.key()
            }
        }
    };
}

pub(crate) use {name_newtype, string_newtype, string_serde};

#[cfg(test)]
#[path = "../unit-tests/text.rs"]
mod tests;
