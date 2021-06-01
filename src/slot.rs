// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Explicit stack slots, which can be used for stack emplacement.
//!
//! A [`Slot`] is uninitialized storage on the stack that can be manipulated
//! explicitly. Notionally, a [`Slot<T>`] represents a `let x: T;` in some
//! function's stack.
//! 
//! [`Slot`]s mut be created with the [`slot!()`] macro:
//! ```
//! # use moveit::{slot};
//! slot!(storage);
//! let mut x = storage.put(42);
//! *x /= 2;
//! assert_eq!(*x, 21);
//! ```
//! Unfortunately, due to the constrains of Rust today, it is not possible to
//! produce a [`Slot`] as part of a larger expression; since it needs to expand
//! to a `let` to bind the stack location, [`slot!()`] must be a statement, not
//! an expression.
//!
//! [`Slot`]s can also be used to implement a sort of "guaranteed RVO":
//! ```
//! # use moveit::{slot, Slot, move_ref::MoveRef};
//! fn returns_on_the_stack(val: i32, storage: Slot<i32>) -> Option<MoveRef<i32>> {
//!   if val == 0 {
//!     return None
//!   }
//!   Some(storage.put(val))
//! }
//!
//! slot!(storage);
//! let val = returns_on_the_stack(42, storage);
//! assert_eq!(*val.unwrap(), 42);
//! ```
//!
//! [`Slot`]s provide a natural location for emplacing [`Ctor`]s on the stack.
//! The [`emplace!()`] macro is intended to make this operation
//! straight-forward. 

use core::mem::MaybeUninit;
use core::pin::Pin;

use crate::ctor;
use crate::ctor::Ctor;
use crate::ctor::TryCtor;
use crate::move_ref::MoveRef;

#[cfg(doc)]
use alloc::boxed::Box;

/// An empty slot on the stack into which a value could be emplaced.
///
/// The `'frame` lifetime refers to the lifetime of the stack frame this
/// `Slot`'s storage is allocated on.
///
/// See [`slot!()`].
pub struct Slot<'frame, T>(&'frame mut MaybeUninit<T>);

impl<'frame, T> Slot<'frame, T> {
  /// Creates a new `Slot` with the given pointer as its basis.
  ///
  /// To safely construct a `Slot`, use [`slot!()`].
  ///
  /// # Safety
  /// `ptr` must not be outlived by any other pointers to its allocation.
  pub unsafe fn new_unchecked(ptr: &'frame mut MaybeUninit<T>) -> Self {
    Self(ptr)
  }

  /// Put `val` into this slot, returning a new [`MoveRef`].
  ///
  /// The [`stackbox!()`] macro is a shorthand for this function.
  pub fn put(self, val: T) -> MoveRef<'frame, T> {
    *self.0 = MaybeUninit::new(val);
    unsafe { MoveRef::new_unchecked(&mut *(self.0 as *mut _ as *mut T)) }
  }

  /// Pin `val` into this slot, returning a new, pinned [`MoveRef`].
  pub fn pin(self, val: T) -> Pin<MoveRef<'frame, T>> {
    self.emplace(ctor::new(val))
  }

  /// Emplace `ctor` into this slot, returning a new, pinned [`MoveRef`].
  ///
  /// The [`emplace!()`] macro is a shorthand for this function.
  pub fn emplace<C: Ctor<Output = T>>(
    self,
    ctor: C,
  ) -> Pin<MoveRef<'frame, T>> {
    unsafe {
      ctor.ctor(Pin::new_unchecked(self.0));
      Pin::new_unchecked(MoveRef::new_unchecked(
        &mut *(self.0 as *mut _ as *mut T),
      ))
    }
  }

  /// Try to emplace `ctor` into this slot, returning a new, pinned [`MoveRef`].
  pub fn try_emplace<C: TryCtor<Output = T>>(
    self,
    ctor: C,
  ) -> Result<Pin<MoveRef<'frame, T>>, C::Error> {
    unsafe {
      ctor.try_ctor(Pin::new_unchecked(self.0))?;
      Ok(Pin::new_unchecked(MoveRef::new_unchecked(
        &mut *(self.0 as *mut _ as *mut T),
      )))
    }
  }
}

#[doc(hidden)]
pub mod __macro {
  pub use core;
}

/// Create a [`Slot`] and immediately emplace a [`Ctor`] into it.
///
/// This macro is a convenience for calling [`slot!()`] followed by
/// [`Slot::emplace()`]; the resulting type is a `Pin<MoveRef<T>>`.
/// 
/// ```
/// # use moveit::{emplace, ctor, move_ref::MoveRef};
/// # use core::pin::Pin;
/// emplace!(let x = ctor::default::<i32>());
/// emplace! {
///   let y: Pin<MoveRef<i32>> = ctor::from(*x);
///   let mut z = ctor::new(*y as u64);
/// }
/// # let _ = z;
/// ```
#[macro_export]
#[cfg(doc)]
macro_rules! emplace {
  ($(let $(mut)? $name:ident $(: $ty:ty)? = $expr:expr);*) => { ... }
}

/// Shh...
#[macro_export]
#[cfg(not(doc))]
macro_rules! emplace {
  (let $name:ident $(: $ty:ty)? = $expr:expr $(; $($rest:tt)*)?) => {
    $crate::emplace!(@emplace $name, $($ty)?, $expr);
    $crate::emplace!($($($rest)*)?);
  };
  (let mut $name:ident $(: $ty:ty)? = $expr:expr $(; $($rest:tt)*)?) => {
    $crate::emplace!(@emplace(mut) $name, $($ty)?, $expr);
    $crate::emplace!($($($rest)*)?);
  };
  ($(;)?) => {};

  (@emplace $(($mut:tt))? $name:ident, $($ty:ty)?, $expr:expr) => {
    $crate::slot!($name);
    let $($mut)? $name $(: $ty)? = $name.emplace($expr);
  };
}

/// Constructs a new [`Slot`].
///
/// Because [`Slot`]s need to own data on the stack, but that data cannot
/// move with the [`Slot`], it must be constructed using this macro. For
/// example:
/// ```
/// moveit::slot!(x, y: bool);
/// let x = x.put(5);
/// let y = y.put(false);
/// ```
///
/// This macro is especially useful for passing data into functions that want to
/// emplace a value into the caller.
#[macro_export]
#[cfg(doc)]
macro_rules! slot {
  ($($name:ident $(: $ty:ty)?),*) => { ... }
}

/// Shh...
#[macro_export]
#[cfg(not(doc))]
macro_rules! slot {
  ($($name:ident $(: $ty:ty)?),* $(,)*) => {$(
    let mut uninit = $crate::stackbox::__macro::core::mem::MaybeUninit::<$
      crate::slot!(@tyof $($ty)?)
    >::uninit();
    let $name = unsafe { $crate::stackbox::Slot::new_unchecked(&mut uninit) };
  )*};
  (@tyof) => {_};
  (@tyof $ty:ty) => {$ty};
}