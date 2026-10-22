//! The transposition table: one entry per slot, replaced by depth and age.
//!
//! Threads share it without locks. Each slot holds the entry's data and the
//! position's hash XORed with that data; a slot torn by two threads writing at
//! once fails the check on probe and reads as empty.

use crate::types::Move;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering::Relaxed};

pub const BOUND_UPPER: u8 = 1;
pub const BOUND_LOWER: u8 = 2;
pub const BOUND_EXACT: u8 = 3;

#[derive(Clone, Copy, Default)]
pub struct Entry {
    pub mv: Move,
    pub score: i16,
    pub eval: i16,
    pub depth: u8,
    /// Bound in the low two bits, search generation above.
    flags: u8,
}

impl Entry {
    #[inline(always)]
    pub fn bound(&self) -> u8 {
        self.flags & 3
    }

    #[inline(always)]
    fn pack(&self) -> u64 {
        self.mv.0 as u64
            | (self.score as u16 as u64) << 16
            | (self.eval as u16 as u64) << 32
            | (self.depth as u64) << 48
            | (self.flags as u64) << 56
    }

    #[inline(always)]
    fn unpack(d: u64) -> Entry {
        Entry { mv: Move(d as u16), score: (d >> 16) as i16, eval: (d >> 32) as i16, depth: (d >> 48) as u8, flags: (d >> 56) as u8 }
    }
}

#[derive(Default)]
struct Slot {
    check: AtomicU64,
    data: AtomicU64,
}

pub struct Tt {
    slots: Vec<Slot>,
    age: AtomicU8,
}

impl Tt {
    pub fn new(mb: usize) -> Tt {
        let n = (mb * 1024 * 1024 / std::mem::size_of::<Slot>()).max(1024);
        Tt { slots: (0..n).map(|_| Slot::default()).collect(), age: AtomicU8::new(0) }
    }

    pub fn clear(&self) {
        for s in &self.slots {
            s.check.store(0, Relaxed);
            s.data.store(0, Relaxed);
        }
        self.age.store(0, Relaxed);
    }

    /// Marks a new search, so entries from older ones are replaced first.
    pub fn new_search(&self) {
        self.age.store((self.age.load(Relaxed) + 1) & 63, Relaxed);
    }

    #[inline(always)]
    fn slot(&self, hash: u64) -> &Slot {
        &self.slots[((hash as u128 * self.slots.len() as u128) >> 64) as usize]
    }

    #[inline(always)]
    pub fn probe(&self, hash: u64) -> Option<Entry> {
        let s = self.slot(hash);
        let data = s.data.load(Relaxed);
        (s.check.load(Relaxed) ^ data == hash && data >> 56 != 0).then(|| Entry::unpack(data))
    }

    /// Scores must already be adjusted to be relative to this node (see `search`).
    #[inline(always)]
    pub fn store(&self, hash: u64, mv: Move, score: i32, eval: i32, depth: i32, bound: u8) {
        let s = self.slot(hash);
        let age = self.age.load(Relaxed);
        let old_data = s.data.load(Relaxed);
        let old = Entry::unpack(old_data);
        let same = s.check.load(Relaxed) ^ old_data == hash;
        if same && bound != BOUND_EXACT && depth + 4 <= old.depth as i32 && old.flags >> 2 == age {
            return;
        }
        let e = Entry {
            mv: if mv == Move::NONE && same { old.mv } else { mv },
            score: score as i16,
            eval: eval as i16,
            depth: depth.clamp(0, 255) as u8,
            flags: bound | age << 2,
        };
        let data = e.pack();
        s.data.store(data, Relaxed);
        s.check.store(hash ^ data, Relaxed);
    }

    /// Permille of a sample of slots written by the current search.
    pub fn hashfull(&self) -> usize {
        let n = self.slots.len().min(1000);
        let age = self.age.load(Relaxed);
        self.slots[..n]
            .iter()
            .filter(|s| {
                let f = (s.data.load(Relaxed) >> 56) as u8;
                f != 0 && f >> 2 == age
            })
            .count()
            * 1000
            / n
    }
}
