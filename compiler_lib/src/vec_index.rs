// Copyright (c) 2026 Robert Grosse. All rights reserved.
pub trait NodeIndex: Copy + Eq + std::hash::Hash + Ord {
    fn from_usize(i: usize) -> Self;
    fn to_usize(self) -> usize;
}

#[macro_export]
macro_rules! index_type {
    ($name:ident) => {
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(std::num::NonZeroU32);
        impl $name {
            #[allow(dead_code)]
            pub const MAX: Self = Self(std::num::NonZeroU32::MAX);
            pub fn new(i: usize) -> Self {
                let i = (i + 1) as u32;
                let i = std::num::NonZeroU32::new(i).expect("Index overflow");
                Self(i)
            }

            pub fn i(self) -> usize {
                (self.0.get() - 1) as usize
            }
        }
        impl $crate::vec_index::NodeIndex for $name {
            fn from_usize(i: usize) -> Self {
                Self::new(i)
            }
            fn to_usize(self) -> usize {
                self.i()
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.i().fmt(f)
            }
        }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.i().fmt(f)
            }
        }
    };
}
