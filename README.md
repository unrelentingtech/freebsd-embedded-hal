[![crates.io](https://img.shields.io/crates/v/freebsd-embedded-hal.svg)](https://crates.io/crates/freebsd-embedded-hal)
[![unlicense](https://img.shields.io/badge/un-license-green.svg?style=flat)](https://unlicense.org)

# freebsd-embedded-hal

Implementation of [`embedded-hal`] traits for FreeBSD devices:

- `gpio`: using [`libgpio`], with stateful and toggleable support, with support for true initial output values if the device is capable, with cool type-state tracking, with open-drain outputs
- `i2c`: using [`iic`], with transaction support (not using iterators on-the-fly because many drivers have to reinterpret start/stop flags between neighboring messages for hardware start-stop)

[`libgpio`]: https://www.freebsd.org/cgi/man.cgi?query=gpio&sektion=3
[`iic`]: https://www.freebsd.org/cgi/man.cgi?query=iic&sektion=4
[`embedded-hal`]: https://docs.rs/embedded-hal

## License

This is free and unencumbered software released into the public domain.  
For more information, please refer to the `UNLICENSE` file or [unlicense.org](https://unlicense.org).
