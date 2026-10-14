//! Hostile output never breaks the Terminal Core's rules, and never gets a reply that holds
//! what the program wrote (REL-02, SEC-05). `terminal_core::fuzz` says how an input is read.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| terminal_core::fuzz::interpret(data));
