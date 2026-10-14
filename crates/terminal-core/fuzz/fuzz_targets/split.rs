//! The same output gives the same state and events however it is cut (ARC-03).
//! `terminal_core::fuzz` says how an input is read.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| terminal_core::fuzz::split(data));
