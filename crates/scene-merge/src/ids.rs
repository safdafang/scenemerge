//! Deterministic generation of Godot-style identifiers.

use std::collections::HashSet;

/// Godot's `Resource::generate_scene_unique_id` alphabet: because of an
/// off-by-one (`'z' - 'a'`) it never produces `z` or `9`.
const ALPHABET: &[u8; 34] = b"abcdefghijklmnopqrstuvwxy012345678";

/// FNV-1a, used to seed the generator from stable inputs so that the same
/// merge always produces the same ids.
pub(crate) fn fnv(data: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A small deterministic generator (splitmix64).
pub(crate) struct IdGen {
    state: u64,
}

impl IdGen {
    pub(crate) fn new(seed: u64) -> Self {
        IdGen { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Five characters from Godot's id alphabet.
    pub(crate) fn chars5(&mut self) -> String {
        let mut v = self.next();
        let mut s = String::with_capacity(5);
        for _ in 0..5 {
            let c = ALPHABET.get((v % 34) as usize).copied().unwrap_or(b'a');
            s.push(char::from(c));
            v /= 34;
        }
        s
    }

    /// An unused id `{prefix}_{5 chars}`.
    pub(crate) fn unique(&mut self, prefix: &str, used: &HashSet<String>) -> String {
        loop {
            let id = format!("{prefix}_{}", self.chars5());
            if !used.contains(&id) {
                return id;
            }
        }
    }

    /// An unused positive 31-bit node `unique_id`.
    pub(crate) fn unique_node_id(&mut self, used: &HashSet<i64>) -> i64 {
        loop {
            let v = (self.next() & 0x7fff_ffff) as i64;
            if v != 0 && !used.contains(&v) {
                return v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_use_godots_alphabet_and_are_deterministic() {
        let mut a = IdGen::new(42);
        let mut b = IdGen::new(42);
        let used = HashSet::new();
        for _ in 0..200 {
            let x = a.unique("BoxMesh", &used);
            assert_eq!(x, b.unique("BoxMesh", &used));
            let tail = x.strip_prefix("BoxMesh_").unwrap();
            assert_eq!(tail.len(), 5);
            assert!(tail.bytes().all(|c| ALPHABET.contains(&c)), "{x}");
            assert!(!tail.contains(['z', '9']));
        }
    }
}
