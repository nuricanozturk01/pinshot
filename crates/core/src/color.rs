//! 8-bit RGBA colour with `#rrggbb` / `#rrggbbaa` parsing.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Perceived brightness 0..255 (used to pick a readable text colour on top of this colour).
    pub fn luminance(self) -> f32 {
        0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32
    }

    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(t.get(i..i + 2)?, 16).ok();
        match t.len() {
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Rgba::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid colour {text:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        assert_eq!(Rgba::parse("#ff3b30"), Some(Rgba::rgb(255, 59, 48)));
        assert_eq!(Rgba::parse("0A84FF"), Some(Rgba::rgb(10, 132, 255)));
        assert_eq!(
            Rgba::parse("#11223344"),
            Some(Rgba::new(0x11, 0x22, 0x33, 0x44))
        );
        assert_eq!(Rgba::parse("#12"), None);
        assert_eq!(Rgba::parse("#gg0000"), None);
        assert_eq!(Rgba::rgb(255, 59, 48).to_hex(), "#ff3b30");
    }

    #[test]
    fn serde_roundtrip() {
        let c = Rgba::new(1, 2, 3, 200);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Rgba>(&json).unwrap(), c);
        assert!(serde_json::from_str::<Rgba>("\"nope\"").is_err());
    }
}
