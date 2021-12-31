//! Implementation of [`embedded-hal`] traits for FreeBSD devices
//!
//! [`embedded-hal`]: https://docs.rs/embedded-hal

pub mod gpio;
pub use gpio::GpioChip;

pub mod i2c;
pub use i2c::I2cBus;
