//! Canonical public wire bytes and domain-separated primitives shared by
//! clients and the opaque relay. This crate has no event or storage model.

#![forbid(unsafe_code)]

pub mod cbor;
pub mod crypto;
