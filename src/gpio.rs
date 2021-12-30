//! Implementation of [`embedded-hal`] digital input/output traits using the FreeBSD gpio(3) library
//!
//! [`embedded-hal`]: https://docs.rs/embedded-hal

use std::{
    ffi::{CStr, CString},
    io,
    marker::PhantomData,
    ptr,
};

pub struct GpioChip(libc::c_int);
pub struct GpioPins<'c> {
    chip: &'c GpioChip,
    base: *mut gpio_config_t,
    offset: isize,
    max_pin: libc::c_int,
}
pub struct Unknown;
pub struct Input;
pub struct Output<const OM: u32>;
pub type OutputPushPull = Output<GPIO_PIN_PUSHPULL>;
pub type OutputOpenDrain = Output<GPIO_PIN_OPENDRAIN>;
pub struct GpioPin<'c, M> {
    mode: PhantomData<M>,
    chip: &'c GpioChip,
    num: u32,
    caps: u32,
}

impl std::os::unix::io::FromRawFd for GpioChip {
    unsafe fn from_raw_fd(fd: std::os::unix::io::RawFd) -> Self {
        GpioChip(fd.into())
    }
}

impl GpioChip {
    pub fn from_unit(unit: u32) -> io::Result<GpioChip> {
        let res = unsafe { gpio_open(unit as _) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(GpioChip(res))
    }

    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> io::Result<GpioChip> {
        use std::os::unix::ffi::OsStrExt;
        let path_str = CString::new(path.as_ref().as_os_str().as_bytes())?;
        let res = unsafe { gpio_open_device(path_str.as_ptr()) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(GpioChip(res))
    }

    pub fn pins(&mut self) -> io::Result<GpioPins> {
        let mut base = ptr::null_mut();
        let pins = unsafe { gpio_pin_list(self.0, &mut base as _) };
        if pins == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(GpioPins { chip: self, base, offset: 0, max_pin: pins })
    }
}

impl<'c> Drop for GpioPins<'c> {
    fn drop(&mut self) {
        unsafe { libc::free(self.base as *mut _) };
    }
}

impl<'c> GpioPins<'c> {
    pub fn total(&self) -> usize {
        1 + self.max_pin as usize
    }
}

impl<'c> Iterator for GpioPins<'c> {
    type Item = (String, GpioPin<'c, Unknown>);

    fn next(&mut self) -> Option<(String, GpioPin<'c, Unknown>)> {
        if self.offset > self.max_pin as isize {
            return None;
        }
        let cnf = unsafe { &*self.base.offset(self.offset) };
        self.offset += 1;
        let name_end = cnf
            .g_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(cnf.g_name.len());
        let name = CStr::from_bytes_with_nul(&cnf.g_name[..name_end + 1])
            .ok()?
            .to_string_lossy()
            .to_string();
        Some((
            name,
            GpioPin {
                mode: PhantomData,
                chip: self.chip,
                num: cnf.g_pin,
                caps: cnf.g_caps,
            },
        ))
    }
}

impl<'c, M> GpioPin<'c, M> {
    #[inline(always)]
    pub fn number(&self) -> u32 {
        self.num
    }

    #[inline(always)]
    fn set_flags(&self, flags: u32) -> io::Result<()> {
        let mut pcfg =
            gpio_config_t { g_pin: self.num, g_name: [0; 64], g_caps: 0, g_flags: 0 };
        if unsafe { gpio_pin_config(self.chip.0, &mut pcfg) } == -1 {
            return Err(io::Error::last_os_error());
        }
        pcfg.g_flags = flags;
        if unsafe { gpio_pin_set_flags(self.chip.0, &mut pcfg) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn into_output_generic<const OM: u32>(
        self,
        state: embedded_hal::digital::PinState,
    ) -> io::Result<GpioPin<'c, Output<OM>>> {
        let (iflags, istate) = match state {
            embedded_hal::digital::PinState::Low
                if self.caps & GPIO_PIN_PRESET_LOW == GPIO_PIN_PRESET_LOW =>
            {
                (GPIO_PIN_PRESET_LOW, None)
            },
            embedded_hal::digital::PinState::High
                if self.caps & GPIO_PIN_PRESET_HIGH == GPIO_PIN_PRESET_HIGH =>
            {
                (GPIO_PIN_PRESET_HIGH, None)
            },
            x => (0, Some(x)),
        };
        self.set_flags(GPIO_PIN_OUTPUT | OM | iflags)?;
        let mut res = GpioPin {
            mode: PhantomData,
            chip: self.chip,
            num: self.num,
            caps: self.caps,
        };
        use embedded_hal::digital::blocking::OutputPin;
        match istate {
            Some(embedded_hal::digital::PinState::Low) => res.set_low()?,
            Some(embedded_hal::digital::PinState::High) => res.set_high()?,
            _ => (),
        };
        Ok(res)
    }

    pub fn into_open_drain_output_pin(
        self,
        state: embedded_hal::digital::PinState,
    ) -> io::Result<GpioPin<'c, OutputOpenDrain>> {
        self.into_output_generic(state)
    }
}

impl<'c, M>
    embedded_hal::digital::blocking::IoPin<GpioPin<'c, Input>, GpioPin<'c, OutputPushPull>>
    for GpioPin<'c, M>
{
    type Error = io::Error;

    fn into_input_pin(self) -> Result<GpioPin<'c, Input>, Self::Error> {
        self.set_flags(GPIO_PIN_INPUT)?;
        Ok(GpioPin {
            mode: PhantomData,
            chip: self.chip,
            num: self.num,
            caps: self.caps,
        })
    }

    fn into_output_pin(
        self,
        state: embedded_hal::digital::PinState,
    ) -> Result<GpioPin<'c, OutputPushPull>, Self::Error> {
        self.into_output_generic(state)
    }
}

impl<'c> embedded_hal::digital::blocking::InputPin for GpioPin<'c, Input> {
    type Error = io::Error;

    fn is_high(&self) -> Result<bool, Self::Error> {
        let res = unsafe { gpio_pin_get(self.chip.0, self.num) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(res == 1)
    }

    fn is_low(&self) -> Result<bool, Self::Error> {
        self.is_high().map(|val| !val)
    }
}

impl<'c, const OM: u32> embedded_hal::digital::blocking::OutputPin for GpioPin<'c, Output<OM>> {
    type Error = io::Error;

    fn set_low(&mut self) -> Result<(), Self::Error> {
        let res = unsafe { gpio_pin_set(self.chip.0, self.num, 0) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        let res = unsafe { gpio_pin_set(self.chip.0, self.num, 1) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl<'c, const OM: u32> embedded_hal::digital::blocking::StatefulOutputPin
    for GpioPin<'c, Output<OM>>
{
    fn is_set_high(&self) -> Result<bool, Self::Error> {
        let res = unsafe { gpio_pin_get(self.chip.0, self.num) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(res == 1)
    }

    fn is_set_low(&self) -> Result<bool, Self::Error> {
        self.is_set_high().map(|val| !val)
    }
}

impl<'c, const OM: u32> embedded_hal::digital::blocking::ToggleableOutputPin
    for GpioPin<'c, Output<OM>>
{
    type Error = io::Error;

    fn toggle(&mut self) -> Result<(), Self::Error> {
        let res = unsafe { gpio_pin_toggle(self.chip.0, self.num) };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

const GPIO_PIN_INPUT: u32 = 0x01;
const GPIO_PIN_OUTPUT: u32 = 0x02;
const GPIO_PIN_OPENDRAIN: u32 = 0x04;
const GPIO_PIN_PUSHPULL: u32 = 0x08;
const GPIO_PIN_PRESET_LOW: u32 = 0x400;
const GPIO_PIN_PRESET_HIGH: u32 = 0x800;

#[repr(C)]
#[allow(non_camel_case_types)]
struct gpio_config_t {
    g_pin: u32,
    g_name: [libc::c_uchar; 64],
    g_caps: u32,
    g_flags: u32,
}

#[link(name = "gpio")]
extern "C" {
    fn gpio_open(unit: libc::c_uint) -> libc::c_int;
    fn gpio_open_device(device: *const libc::c_char) -> libc::c_int;
    fn gpio_pin_list(handle: libc::c_int, pcfgs: *mut *mut gpio_config_t) -> libc::c_int;
    fn gpio_pin_get(handle: libc::c_int, pin: u32) -> libc::c_int;
    fn gpio_pin_toggle(handle: libc::c_int, pin: u32) -> libc::c_int;
    fn gpio_pin_set(handle: libc::c_int, pin: u32, val: libc::c_int) -> libc::c_int;
    fn gpio_pin_config(handle: libc::c_int, pcfg: *mut gpio_config_t) -> libc::c_int;
    fn gpio_pin_set_flags(handle: libc::c_int, pcfg: *mut gpio_config_t) -> libc::c_int;
}
