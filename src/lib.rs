#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
compile_error!("lilvault requires Linux openat2; no insecure path-resolution fallback is provided");

pub mod config;
pub mod error;
pub mod http;
pub mod service;
pub mod vault;

pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
