/// One-based human-readable source coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LineColumn {
    line: LineNumber,
    column: ColumnNumber,
}

impl LineColumn {
    pub fn new(line: u32, column: u32) -> Option<Self> {
        let line = LineNumber::new(line)?;
        let column = ColumnNumber::new(column)?;
        Some(Self { line, column })
    }

    pub const fn new_unchecked(line: LineNumber, column: ColumnNumber) -> Self {
        Self { line, column }
    }

    pub const fn line(self) -> LineNumber {
        self.line
    }

    pub const fn column(self) -> ColumnNumber {
        self.column
    }

    pub const fn line_index(self) -> LineIndex {
        self.line.to_index()
    }

    pub const fn column_index(self) -> ColumnIndex {
        self.column.to_index()
    }
}

macro_rules! declare_dual_number_and_index_type {
    ($name:ident, $description:literal) => {
        paste::paste! {
            declare_dual_number_and_index_type!([<$name Index>], [<$name Number>], $description);
        }
    };

    ($index_name:ident, $number_name:ident, $description:literal) => {
        #[doc = concat!("A zero-indexed ", $description, " number")]
        #[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(
            feature = "zerocopy",
            derive(
                zerocopy::FromBytes,
                zerocopy::Immutable,
                zerocopy::IntoBytes,
                zerocopy::KnownLayout,
            )
        )]
        pub struct $index_name(pub u32);

        impl $index_name {
            #[doc = concat!("Convert to a [", stringify!($number_name), "]")]
            pub const fn number(self) -> $number_name {
                let number = self
                    .0
                    .checked_add(1)
                    .expect("line or column index exceeds the representable one-based range");
                $number_name(
                    ::core::num::NonZeroU32::new(number)
                        .expect("a checked one-based index is non-zero"),
                )
            }

            /// Get the raw index value as a usize
            #[inline(always)]
            pub const fn to_usize(self) -> usize {
                self.0 as usize
            }

            /// Get the raw index value as a u32
            #[inline(always)]
            pub const fn to_u32(self) -> u32 {
                self.0
            }

            /// Add `offset` to this index, returning `None` on overflow
            pub fn checked_add(self, offset: u32) -> Option<Self> {
                self.0.checked_add(offset).map(Self)
            }

            /// Add a signed `offset` to this index, returning `None` on overflow
            pub fn checked_add_signed(self, offset: i32) -> Option<Self> {
                self.0.checked_add_signed(offset).map(Self)
            }

            /// Subtract `offset` from this index, returning `None` on underflow
            pub fn checked_sub(self, offset: u32) -> Option<Self> {
                self.0.checked_sub(offset).map(Self)
            }

            /// Add `offset` to this index, saturating to `u32::MAX` on overflow
            pub const fn saturating_add(self, offset: u32) -> Self {
                Self(self.0.saturating_add(offset))
            }

            /// Add a signed `offset` to this index, saturating to `0` on underflow, and `u32::MAX`
            /// on overflow.
            pub const fn saturating_add_signed(self, offset: i32) -> Self {
                Self(self.0.saturating_add_signed(offset))
            }

            /// Subtract `offset` from this index, saturating to `0` on overflow
            pub const fn saturating_sub(self, offset: u32) -> Self {
                Self(self.0.saturating_sub(offset))
            }
        }

        impl From<u32> for $index_name {
            #[inline]
            fn from(index: u32) -> Self {
                Self(index)
            }
        }

        impl From<$number_name> for $index_name {
            #[inline]
            fn from(index: $number_name) -> Self {
                Self(index.to_u32() - 1)
            }
        }

        impl ::core::ops::Add<u32> for $index_name {
            type Output = Self;

            #[inline]
            fn add(self, rhs: u32) -> Self {
                Self(self.0 + rhs)
            }
        }

        impl ::core::ops::AddAssign<u32> for $index_name {
            fn add_assign(&mut self, rhs: u32) {
                let result = *self + rhs;
                *self = result;
            }
        }

        impl ::core::ops::Add<i32> for $index_name {
            type Output = Self;

            fn add(self, rhs: i32) -> Self {
                self.checked_add_signed(rhs).expect("invalid offset: overflow occurred")
            }
        }

        impl ::core::ops::AddAssign<i32> for $index_name {
            fn add_assign(&mut self, rhs: i32) {
                let result = *self + rhs;
                *self = result;
            }
        }

        impl ::core::ops::Sub<u32> for $index_name {
            type Output = Self;

            #[inline]
            fn sub(self, rhs: u32) -> Self {
                Self(self.0 - rhs)
            }
        }

        impl ::core::ops::SubAssign<u32> for $index_name {
            fn sub_assign(&mut self, rhs: u32) {
                let result = *self - rhs;
                *self = result;
            }
        }

        impl ::core::fmt::Display for $index_name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        #[cfg(feature = "arbitrary")]
        impl ::proptest::prelude::Arbitrary for $index_name {
            type Parameters = ();
            type Strategy = ::proptest::prelude::BoxedStrategy<Self>;

            fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
                use ::proptest::prelude::*;
                any::<u32>().prop_map(Self).boxed()
            }
        }

        #[doc = concat!("A one-indexed ", $description, " number")]
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(
            feature = "zerocopy",
            derive(
                zerocopy::Immutable,
                zerocopy::IntoBytes,
                zerocopy::KnownLayout,
                zerocopy::TryFromBytes,
            )
        )]
        pub struct $number_name(::core::num::NonZeroU32);

        impl Default for $number_name {
            fn default() -> Self {
                #[allow(unsafe_code)]
                Self(unsafe { ::core::num::NonZeroU32::new_unchecked(1) })
            }
        }

        impl $number_name {
            pub const fn new(number: u32) -> Option<Self> {
                match ::core::num::NonZeroU32::new(number) {
                    Some(num) => Some(Self(num)),
                    None => None,
                }
            }

            #[doc = concat!("Convert to a [", stringify!($index_name), "]")]
            pub const fn to_index(self) -> $index_name {
                $index_name(self.to_u32().saturating_sub(1))
            }

            /// Get the raw value as a usize
            #[inline(always)]
            pub const fn to_usize(self) -> usize {
                self.0.get() as usize
            }

            /// Get the raw value as a u32
            #[inline(always)]
            pub const fn to_u32(self) -> u32 {
                self.0.get()
            }

            /// Add `offset` to this index, returning `None` on overflow
            pub fn checked_add(self, offset: u32) -> Option<Self> {
                self.0.checked_add(offset).map(Self)
            }

            /// Add a signed `offset` to this index, returning `None` on overflow
            pub fn checked_add_signed(self, offset: i32) -> Option<Self> {
                self.0.get().checked_add_signed(offset).and_then(Self::new)
            }

            /// Subtract `offset` from this index, returning `None` on underflow
            pub fn checked_sub(self, offset: u32) -> Option<Self> {
                self.0.get().checked_sub(offset).and_then(Self::new)
            }

            /// Add `offset` to this index, saturating to `u32::MAX` on overflow
            pub const fn saturating_add(self, offset: u32) -> Self {
                #[allow(unsafe_code)]
                Self(unsafe {
                    ::core::num::NonZeroU32::new_unchecked(self.0.get().saturating_add(offset))
                })
            }

            /// Add a signed `offset` to this index, saturating to `0` on underflow, and `u32::MAX`
            /// on overflow.
            pub fn saturating_add_signed(self, offset: i32) -> Self {
                Self::new(self.to_u32().saturating_add_signed(offset)).unwrap_or_default()
            }

            /// Subtract `offset` from this index, saturating to `0` on overflow
            pub fn saturating_sub(self, offset: u32) -> Self {
                Self::new(self.to_u32().saturating_sub(offset)).unwrap_or_default()
            }
        }

        impl From<::core::num::NonZeroU32> for $number_name {
            #[inline]
            fn from(index: ::core::num::NonZeroU32) -> Self {
                Self(index)
            }
        }

        impl From<$index_name> for $number_name {
            #[inline]
            fn from(index: $index_name) -> Self {
                index.number()
            }
        }

        impl ::core::ops::Add<u32> for $number_name {
            type Output = Self;

            #[inline]
            fn add(self, rhs: u32) -> Self {
                self.checked_add(rhs)
                    .expect("line or column number overflowed while adding an offset")
            }
        }

        impl ::core::ops::AddAssign<u32> for $number_name {
            fn add_assign(&mut self, rhs: u32) {
                let result = *self + rhs;
                *self = result;
            }
        }

        impl ::core::ops::Add<i32> for $number_name {
            type Output = Self;

            fn add(self, rhs: i32) -> Self {
                self.to_u32()
                    .checked_add_signed(rhs)
                    .and_then(Self::new)
                    .expect("invalid offset: overflow occurred")
            }
        }

        impl ::core::ops::AddAssign<i32> for $number_name {
            fn add_assign(&mut self, rhs: i32) {
                let result = *self + rhs;
                *self = result;
            }
        }

        impl ::core::ops::Sub<u32> for $number_name {
            type Output = Self;

            #[inline]
            fn sub(self, rhs: u32) -> Self {
                self.to_u32()
                    .checked_sub(rhs)
                    .and_then(Self::new)
                    .expect("invalid offset: overflow occurred")
            }
        }

        impl ::core::ops::SubAssign<u32> for $number_name {
            fn sub_assign(&mut self, rhs: u32) {
                let result = *self - rhs;
                *self = result;
            }
        }

        impl ::core::fmt::Display for $number_name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        #[cfg(feature = "arbitrary")]
        impl ::proptest::prelude::Arbitrary for $number_name {
            type Parameters = ();
            type Strategy = ::proptest::prelude::BoxedStrategy<Self>;

            fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
                use proptest::prelude::*;
                any::<::core::num::NonZeroU32>().prop_map(Self).boxed()
            }
        }
    };
}

declare_dual_number_and_index_type!(Line, "line");
declare_dual_number_and_index_type!(Column, "column");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "exceeds the representable one-based range")]
    fn maximum_index_cannot_be_converted_to_a_one_based_number() {
        let _ = LineIndex(u32::MAX).number();
    }

    #[test]
    #[should_panic(expected = "overflowed while adding an offset")]
    fn one_based_number_addition_panics_on_overflow() {
        let maximum = LineNumber::new(u32::MAX).unwrap();
        let _ = maximum + 1_u32;
    }
}
