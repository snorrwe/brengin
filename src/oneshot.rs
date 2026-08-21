//! Single use channel

use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicI8, Ordering},
};

const NOT_READY: i8 = 0;
const READY: i8 = 1;
const WRITING: i8 = 2;
const READING: i8 = 3;

pub struct OneShot<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    status: AtomicI8,
}

impl<T> Default for OneShot<T> {
    fn default() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            status: AtomicI8::new(NOT_READY),
        }
    }
}

impl<T> Drop for OneShot<T> {
    fn drop(&mut self) {
        let _ = self.try_receive();
    }
}

unsafe impl<T> Send for OneShot<T> {}
unsafe impl<T> Sync for OneShot<T> {}

impl<T> OneShot<T> {
    pub fn send(&self, value: T) {
        if self
            .status
            .compare_exchange(NOT_READY, WRITING, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            panic!("Only one send is supported");
        }
        unsafe {
            let slot = self.value.get().as_mut().unwrap();
            slot.write(value);
            self.status.store(READY, Ordering::Release);
        }
    }

    pub fn try_receive(&self) -> Option<T> {
        if self
            .status
            .compare_exchange(READY, READING, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        let value = unsafe { self.value.get().as_mut_unchecked().assume_init_read() };

        self.status.store(NOT_READY, Ordering::Release);

        Some(value)
    }

    pub fn receive(&self) -> T {
        self.try_receive()
            .expect("receive called on an uninitialized channel")
    }

    pub fn is_ready(&self) -> bool {
        self.status.load(Ordering::Relaxed) == 1
    }
}
