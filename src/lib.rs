#![feature(step_trait)]

pub mod cgen;
pub mod compile;
pub mod ir;

pub trait IndexExt<'a, Id> {
    type Item: 'a;
    fn enumerate_ids(&'a self) -> impl Iterator<Item = (Id, &'a Self::Item)>;
    fn enumerate_ids_mut(&'a mut self) -> impl Iterator<Item = (Id, &'a mut Self::Item)>;
    fn ids(&self) -> std::ops::Range<Id>;
}

pub trait IndexVecExt<Id> {
    type Item;
    fn retain_ids(&mut self, f: impl FnMut(Id) -> bool);
    fn add(&mut self, item: Self::Item) -> Id;
}

#[macro_export]
macro_rules! index_ext {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($vis_idx:vis $idx:ty) $(for $($item:ty),*$(,)?)?) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name($vis_idx $idx);
        impl From<usize> for $name {
            #[inline]
            fn from(value: usize) -> Self { $name(value as $idx) }
        }
        impl $name {
            #[inline]
            $vis fn index(&self) -> usize { self.0 as usize }
            #[inline]
            $vis fn invalid() -> Self { $name(<$idx>::MAX) }
        }
        impl Default for $name {
            #[inline]
            fn default() -> Self { Self::invalid() }
        }
        impl std::iter::Step for $name {
            #[inline]
            fn steps_between(start: &Self, end: &Self) -> (usize, Option<usize>) {
                usize::steps_between(&(start.0 as usize), &(end.0 as usize))
            }
            #[inline]
            fn forward_checked(start: Self, count: usize) -> Option<Self> {
                usize::forward_checked(start.0 as usize, count).map(Self::from)
            }
            #[inline]
            fn backward_checked(start: Self, count: usize) -> Option<Self> {
                usize::backward_checked(start.0 as usize, count).map(Self::from)
            }
        }
        $crate::index_ext!(containers $name, bool, i8, i16, i32, i64, u8, u16, u32, u64, isize, usize);
        $($crate::index_ext!(containers $name, $($item),*);)?
    };
    (containers $idx:ty $(, $item:ty)*$(,)?) => {
        $(
            $crate::index_ext!(slice $idx, Vec<$item>, [$item]);
            impl $crate::IndexVecExt<$idx> for Vec<$item> {
                type Item = $item;
                fn retain_ids(&mut self, mut f: impl FnMut($idx) -> bool) {
                    let repl = self
                        .drain(..)
                        .enumerate()
                        .filter_map(|(i, item)| f(<$idx>::from(i)).then_some(item))
                        .collect::<Vec<$item>>();
                    *self = repl;
                }
                fn add(&mut self, item: Self::Item) -> $idx {
                    let idx = self.len();
                    self.push(item);
                    <$idx>::from(idx)
                }
            }
        )*
    };
    (slice $idx:ty $(, $ty:ty)+) => {
        $(impl std::ops::Index<$idx> for $ty {
            type Output = <$ty as std::ops::Index<usize>>::Output;
            #[inline]
            fn index(&self, index: $idx) -> &Self::Output {
                <$ty as std::ops::Index<usize>>::index(self, index.0 as usize)
            }
        }
        impl std::ops::IndexMut<$idx> for $ty {
            #[inline]
            fn index_mut(&mut self, index: $idx) -> &mut Self::Output {
                <$ty as std::ops::IndexMut<usize>>::index_mut(self, index.0 as usize)
            }
        }
        impl<'a> $crate::IndexExt<'a, $idx> for $ty {
            type Item = <$ty as std::ops::Index<usize>>::Output;
            fn enumerate_ids(&'a self) -> impl Iterator<Item = ($idx, &'a Self::Item)> {
                self.iter().enumerate().map(|(i, item)| (<$idx>::from(i), item))
            }
            fn enumerate_ids_mut(&'a mut self) -> impl Iterator<Item = ($idx, &'a mut Self::Item)> {
                self.iter_mut().enumerate().map(|(i, item)| (<$idx>::from(i), item))
            }
            #[inline]
            fn ids(&self) -> std::ops::Range<$idx> {
                <$idx>::from(0)..<$idx>::from(self.len())
            }
        })+
    };
}
