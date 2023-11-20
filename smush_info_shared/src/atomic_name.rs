use core::sync::atomic::{AtomicU64, Ordering};
use serde::{Serialize, Serializer, Deserialize, Deserializer};

use core::fmt;

#[repr(transparent)]
pub struct AtomicName([AtomicU64; 4]);

union Transmute {
    chars: [u8; 32],
    bits: [u64; 4],
}

impl AtomicName {
    pub const fn new(val: Option<[u8; 32]>) -> Self {
        let chars = if let Some(x) = val {
            x
        } else {
            [0; 32]
        };

        unsafe {
            let mut x = Transmute { bits: [0; 4] };
            x.chars = chars;
            Self([AtomicU64::new(x.bits[0]), AtomicU64::new(x.bits[1]), AtomicU64::new(x.bits[2]), AtomicU64::new(x.bits[3])])
        }
    }

    pub fn from_str(val: &str) -> Self {
        let utf16le: Vec<u16> = val.encode_utf16().collect();
        let mut result = [0; 32];
        for (i, &c) in utf16le.iter().take(32).enumerate() {
            result[i * 2] = (c & 0xFF) as u8;
            result[i * 2 + 1] = ((c >> 8) & 0xFF) as u8;
        }
        Self::new(Some(result))
    }

    pub fn load(&self, order: Ordering) -> Option<[u8; 32]> {
        unsafe {
            let bits0 = self.0[0].load(order);
            let bits1 = self.0[1].load(order);
            let bits2 = self.0[2].load(order);
            let bits3 = self.0[3].load(order);
            match (Transmute { bits: [bits0, bits1, bits2, bits3] }.chars) {
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] => None,
                x => Some(x),
            }
        }
    }

    pub fn load_string(&self, order: Ordering) -> Option<String> {
        unsafe {
            match (Transmute {
                bits: [self.0[0].load(order), self.0[1].load(order), self.0[2].load(order), self.0[3].load(order)],
            }
            .chars)
            {
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] => None,
                x => {
                    let utf16_chars: Vec<u16> = x.chunks(2).map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])).collect();
                    let null_index = utf16_chars.iter().position(|&c| c == 0).unwrap_or(utf16_chars.len());
                    String::from_utf16(&utf16_chars[0..null_index]).ok()
                },
            }
        }
    }

    pub fn store(&self, val: Option<[u8; 32]>, order: Ordering) {
        let chars = val.unwrap_or([0; 32]);
        self.0[0].store(unsafe { Transmute { chars }.bits[0] }, order);
        self.0[1].store(unsafe { Transmute { chars }.bits[1] }, order);
        self.0[2].store(unsafe { Transmute { chars }.bits[2] }, order);
        self.0[3].store(unsafe { Transmute { chars }.bits[3] }, order);
    }

    pub fn store_str(&self, val: Option<&str>, order: Ordering) {
        self.store(val.map(|x| {
            let utf16le: Vec<u16> = x.encode_utf16().collect();
            let mut result = [0; 32];
            let mut i = 0;
            for &c in utf16le.iter().take(16) {
                result[i] = (c & 0xFF) as u8;
                result[i + 1] = ((c >> 8) & 0xFF) as u8;
                i += 2;
            }
            result
        }), order);
    }
}

impl fmt::Debug for AtomicName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <Option<String> as fmt::Debug>::fmt(&self.load_string(Ordering::SeqCst), f)
    }
}

impl Serialize for AtomicName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.load_string(Ordering::SeqCst) {
            Some(s) => serializer.serialize_str(&String::from_utf16_lossy(&s.encode_utf16().collect::<Vec<u16>>())),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for AtomicName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let x = <Option<String>>::deserialize(deserializer)?;
        Ok(AtomicName::new(x.map(|x| {
            let utf16_bytes = x.encode_utf16().collect::<Vec<u16>>();
            let mut bytes = utf16_bytes.iter().flat_map(|&c| [c as u8, (c >> 8) as u8]);
            [
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
                bytes.next().unwrap_or(0),
            ]
        })))
    }
}

#[cfg(test)]
mod atomic_name_tests {
    use super::*;

    #[test]
    fn test_serde_round_trip() {
        let x = AtomicName::from_str("つыůš");
        let json = serde_json::to_string(&x).unwrap();
        // println!("{}to_string(&x));
        assert_eq!(json, "\"つыůš\"");
        let y: AtomicName = serde_json::from_str(&json).unwrap();
        assert_eq!(x.load(Ordering::SeqCst), y.load(Ordering::SeqCst));
    }

    #[derive(Serialize, Deserialize)]
    struct Test {
        pub val: AtomicName,
    }

    #[test]
    fn test_serde_round_trip_struct() {
        let x = Test {
            val: AtomicName::from_str("つыůš"),
        };
        let json = serde_json::to_string(&x).unwrap();
        assert_eq!(json, "{\"val\":\"つыůš\"}");
        let y: Test = serde_json::from_str(&json).unwrap();
        assert_eq!(x.val.load(Ordering::SeqCst), y.val.load(Ordering::SeqCst));
    }
}
