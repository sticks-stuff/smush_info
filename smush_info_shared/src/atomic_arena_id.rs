use core::sync::atomic::{AtomicU64, Ordering};
use serde::{Serialize, Serializer, Deserialize, Deserializer};

use core::fmt;

#[repr(transparent)]
pub struct AtomicArenaId(AtomicU64); 

union Transmute { chars: [u8; 5], bits: u64 }

impl AtomicArenaId {
    pub const fn new(val: Option<[u8; 5]>) -> Self {
        let chars = if let Some(x) = val {
            x
        } else {
            [0; 5]
        };

        unsafe {
            let mut x = Transmute { bits: 0 };
            x.chars = chars;
            Self(AtomicU64::new(x.bits))
        }
    }

    pub fn from_str(val: &str) -> Self {
        let mut bytes = val.bytes();
        
        Self::new(Some([
            bytes.next().unwrap(),
            bytes.next().unwrap(),
            bytes.next().unwrap(),
            bytes.next().unwrap(),
            bytes.next().unwrap(),
        ]))
    }

    pub fn load(&self, order: Ordering) -> Option<[u8; 5]> {
        unsafe {
            let bits = self.0.load(order);
            match (Transmute { bits }.chars) {
                [0, 0, 0, 0, 0] => None,
                x => Some(x)
            }
        }
    }

    pub fn load_string(&self, order: Ordering) -> Option<String> {
        unsafe {
            match (Transmute { bits: self.0.load(order) }.chars) {
                [0, 0, 0, 0, 0] => None,
                x => Some(x.iter().map(|c| *c as char).collect())
            }
        }
    }

    pub fn store(&self, val: Option<[u8; 5]>, order: Ordering) {
        let chars = val.unwrap_or([0; 5]);
        self.0.store(unsafe { Transmute { chars }.bits }, order);
    }

    pub fn store_str(&self, val: Option<&str>, order: Ordering) {
        self.store(val.map(|x|{
            let mut bytes = x.bytes();
            [
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
            ]
        }), order);
    }
}

impl fmt::Debug for AtomicArenaId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <Option<String> as fmt::Debug>::fmt(&self.load_string(Ordering::SeqCst), f)
    }
}

impl Serialize for AtomicArenaId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer,
    {
        match self.load_string(Ordering::SeqCst) {
            Some(s) => serializer.serialize_str(&s),
            None => serializer.serialize_none()
        }
    }
}

impl<'de> Deserialize<'de> for AtomicArenaId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
    {
        let x = <Option<String>>::deserialize(deserializer)?;
        Ok(AtomicArenaId::new(x.map(|x|{
            let mut bytes = x.bytes();
            [
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
                bytes.next().unwrap(),
            ]
        })))
    }
}

#[cfg(test)]
mod atomic_arena_tests {
    use super::*;

    #[test]
    fn test_serde_round_trip() {
        let x = AtomicArenaId::from_str("ABCDE");
        let json = serde_json::to_string(&x).unwrap();
        assert_eq!(json, "\"ABCDE\"");
        let y: AtomicArenaId = serde_json::from_str(&json).unwrap();
        assert_eq!(x.load(Ordering::SeqCst), y.load(Ordering::SeqCst));
    }

    #[derive(Serialize, Deserialize)]
    struct Test {
        pub val: AtomicArenaId
    }

    #[test]
    fn test_serde_round_trip_struct() {
        let x = Test{ val: AtomicArenaId::from_str("ABCDE") };
        let json = serde_json::to_string(&x).unwrap();
        assert_eq!(json, "{\"val\":\"ABCDE\"}");
        let y: Test = serde_json::from_str(&json).unwrap();
        assert_eq!(x.val.load(Ordering::SeqCst), y.val.load(Ordering::SeqCst));
    }
}
