use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl ValueType {
    pub fn size(&self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::U8 => "Byte (u8)",
            Self::U16 => "2 Bytes (u16)",
            Self::U32 => "4 Bytes (u32)",
            Self::U64 => "8 Bytes (u64)",
            Self::I8 => "Byte (i8)",
            Self::I16 => "2 Bytes (i16)",
            Self::I32 => "4 Bytes (i32)",
            Self::I64 => "8 Bytes (i64)",
            Self::F32 => "Float (f32)",
            Self::F64 => "Double (f64)",
        }
    }

    pub const ALL: &[ValueType] = &[
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::F32,
        Self::F64,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScanValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl ScanValue {
    pub fn from_bytes(bytes: &[u8], value_type: ValueType) -> Option<Self> {
        if bytes.len() < value_type.size() {
            return None;
        }
        Some(match value_type {
            ValueType::U8 => Self::U8(bytes[0]),
            ValueType::U16 => Self::U16(u16::from_le_bytes(bytes[..2].try_into().ok()?)),
            ValueType::U32 => Self::U32(u32::from_le_bytes(bytes[..4].try_into().ok()?)),
            ValueType::U64 => Self::U64(u64::from_le_bytes(bytes[..8].try_into().ok()?)),
            ValueType::I8 => Self::I8(bytes[0] as i8),
            ValueType::I16 => Self::I16(i16::from_le_bytes(bytes[..2].try_into().ok()?)),
            ValueType::I32 => Self::I32(i32::from_le_bytes(bytes[..4].try_into().ok()?)),
            ValueType::I64 => Self::I64(i64::from_le_bytes(bytes[..8].try_into().ok()?)),
            ValueType::F32 => Self::F32(f32::from_le_bytes(bytes[..4].try_into().ok()?)),
            ValueType::F64 => Self::F64(f64::from_le_bytes(bytes[..8].try_into().ok()?)),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::U8(v) => vec![*v],
            Self::U16(v) => v.to_le_bytes().to_vec(),
            Self::U32(v) => v.to_le_bytes().to_vec(),
            Self::U64(v) => v.to_le_bytes().to_vec(),
            Self::I8(v) => vec![*v as u8],
            Self::I16(v) => v.to_le_bytes().to_vec(),
            Self::I32(v) => v.to_le_bytes().to_vec(),
            Self::I64(v) => v.to_le_bytes().to_vec(),
            Self::F32(v) => v.to_le_bytes().to_vec(),
            Self::F64(v) => v.to_le_bytes().to_vec(),
        }
    }

    pub fn value_type(&self) -> ValueType {
        match self {
            Self::U8(_) => ValueType::U8,
            Self::U16(_) => ValueType::U16,
            Self::U32(_) => ValueType::U32,
            Self::U64(_) => ValueType::U64,
            Self::I8(_) => ValueType::I8,
            Self::I16(_) => ValueType::I16,
            Self::I32(_) => ValueType::I32,
            Self::I64(_) => ValueType::I64,
            Self::F32(_) => ValueType::F32,
            Self::F64(_) => ValueType::F64,
        }
    }

    pub fn parse(s: &str, value_type: ValueType) -> Option<Self> {
        Some(match value_type {
            ValueType::U8 => Self::U8(s.parse().ok()?),
            ValueType::U16 => Self::U16(s.parse().ok()?),
            ValueType::U32 => Self::U32(s.parse().ok()?),
            ValueType::U64 => Self::U64(s.parse().ok()?),
            ValueType::I8 => Self::I8(s.parse().ok()?),
            ValueType::I16 => Self::I16(s.parse().ok()?),
            ValueType::I32 => Self::I32(s.parse().ok()?),
            ValueType::I64 => Self::I64(s.parse().ok()?),
            ValueType::F32 => Self::F32(s.parse().ok()?),
            ValueType::F64 => Self::F64(s.parse().ok()?),
        })
    }

    pub fn display_value(&self) -> String {
        match self {
            Self::U8(v) => v.to_string(),
            Self::U16(v) => v.to_string(),
            Self::U32(v) => v.to_string(),
            Self::U64(v) => v.to_string(),
            Self::I8(v) => v.to_string(),
            Self::I16(v) => v.to_string(),
            Self::I32(v) => v.to_string(),
            Self::I64(v) => v.to_string(),
            Self::F32(v) => format!("{v:.6}"),
            Self::F64(v) => format!("{v:.6}"),
        }
    }
}

impl std::fmt::Display for ScanValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_value())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanType {
    ExactValue,
    UnknownInitial,
    Increased,
    Decreased,
    Changed,
    Unchanged,
    GreaterThan,
    LessThan,
}

impl ScanValue {
    pub fn matches(&self, other: &ScanValue) -> bool {
        self == other
    }

    pub fn greater_than(&self, other: &ScanValue) -> bool {
        match (self, other) {
            (Self::U8(a), Self::U8(b)) => a > b,
            (Self::U16(a), Self::U16(b)) => a > b,
            (Self::U32(a), Self::U32(b)) => a > b,
            (Self::U64(a), Self::U64(b)) => a > b,
            (Self::I8(a), Self::I8(b)) => a > b,
            (Self::I16(a), Self::I16(b)) => a > b,
            (Self::I32(a), Self::I32(b)) => a > b,
            (Self::I64(a), Self::I64(b)) => a > b,
            (Self::F32(a), Self::F32(b)) => a > b,
            (Self::F64(a), Self::F64(b)) => a > b,
            _ => false,
        }
    }

    pub fn less_than(&self, other: &ScanValue) -> bool {
        match (self, other) {
            (Self::U8(a), Self::U8(b)) => a < b,
            (Self::U16(a), Self::U16(b)) => a < b,
            (Self::U32(a), Self::U32(b)) => a < b,
            (Self::U64(a), Self::U64(b)) => a < b,
            (Self::I8(a), Self::I8(b)) => a < b,
            (Self::I16(a), Self::I16(b)) => a < b,
            (Self::I32(a), Self::I32(b)) => a < b,
            (Self::I64(a), Self::I64(b)) => a < b,
            (Self::F32(a), Self::F32(b)) => a < b,
            (Self::F64(a), Self::F64(b)) => a < b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes_u32() {
        let bytes = 42u32.to_le_bytes();
        let val = ScanValue::from_bytes(&bytes, ValueType::U32).unwrap();
        assert_eq!(val, ScanValue::U32(42));
    }

    #[test]
    fn test_from_bytes_f32() {
        let bytes = 3.14f32.to_le_bytes();
        let val = ScanValue::from_bytes(&bytes, ValueType::F32).unwrap();
        assert_eq!(val, ScanValue::F32(3.14));
    }

    #[test]
    fn test_to_bytes_roundtrip() {
        let original = ScanValue::I64(-12345);
        let bytes = original.to_bytes();
        let restored = ScanValue::from_bytes(&bytes, ValueType::I64).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_parse_values() {
        assert_eq!(ScanValue::parse("255", ValueType::U8), Some(ScanValue::U8(255)));
        assert_eq!(ScanValue::parse("-1", ValueType::I8), Some(ScanValue::I8(-1)));
        assert_eq!(ScanValue::parse("3.14", ValueType::F64), Some(ScanValue::F64(3.14)));
        assert_eq!(ScanValue::parse("abc", ValueType::U32), None);
    }

    #[test]
    fn test_comparisons() {
        let a = ScanValue::U32(10);
        let b = ScanValue::U32(20);
        assert!(b.greater_than(&a));
        assert!(a.less_than(&b));
        assert!(!a.greater_than(&b));
        assert!(a.matches(&ScanValue::U32(10)));
        assert!(!a.matches(&b));
    }

    #[test]
    fn test_from_bytes_too_short() {
        let bytes = [0u8; 2];
        assert!(ScanValue::from_bytes(&bytes, ValueType::U64).is_none());
    }

    #[test]
    fn test_value_type_size() {
        assert_eq!(ValueType::U8.size(), 1);
        assert_eq!(ValueType::U16.size(), 2);
        assert_eq!(ValueType::U32.size(), 4);
        assert_eq!(ValueType::U64.size(), 8);
        assert_eq!(ValueType::F32.size(), 4);
        assert_eq!(ValueType::F64.size(), 8);
    }
}
