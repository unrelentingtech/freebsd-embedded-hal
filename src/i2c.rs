//! Implementation of [`embedded-hal`] I2C traits using the FreeBSD iic(4) device interface
//!
//! [`embedded-hal`]: https://docs.rs/embedded-hal

use std::{
    error, fmt,
    fs::OpenOptions,
    io,
    os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
};

pub struct I2cBus(RawFd);

/// An i2c wrapper around std::io::Error.
///
/// NOTE: values will be super wrong without https://reviews.freebsd.org/D33707
#[derive(Debug)]
pub struct I2cError(io::Error);

impl fmt::Display for I2cError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<io::Error> for I2cError {
    fn from(err: io::Error) -> I2cError {
        I2cError(err)
    }
}

impl error::Error for I2cError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.0)
    }
}

impl embedded_hal::i2c::Error for I2cError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        use embedded_hal::i2c::ErrorKind::*;
        match self.0.raw_os_error() {
            Some(libc::EALREADY) => Bus,
            Some(libc::EOVERFLOW) => Overrun, // I guess
            // Unfortunately both IIC_ENOACK and lots of other things translate to EIO
            _ => Other,
        }
    }
}

impl FromRawFd for I2cBus {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        I2cBus(fd)
    }
}

impl IntoRawFd for I2cBus {
    fn into_raw_fd(self) -> RawFd {
        self.0
    }
}

impl AsRawFd for I2cBus {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl Drop for I2cBus {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl I2cBus {
    pub fn from_unit(unit: u32) -> Result<I2cBus, I2cError> {
        Self::from_path(format!("/dev/iic{}", unit))
    }

    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> Result<I2cBus, I2cError> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(|f| I2cBus(f.into_raw_fd()))
            .map_err(|e| e.into())
    }
}

impl embedded_hal::i2c::blocking::Read for I2cBus {
    type Error = I2cError;

    fn read(&mut self, address: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        rdwr(
            self.0,
            &[iic_msg {
                addr: (address as u16) << 1,
                flags: IIC_M_RD,
                len: buffer.len() as u16,
                buf: buffer as *const _ as *mut _,
            }],
        )
    }
}

impl embedded_hal::i2c::blocking::Write for I2cBus {
    type Error = I2cError;

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        rdwr(
            self.0,
            &[iic_msg {
                addr: (address as u16) << 1,
                flags: IIC_M_WR,
                len: bytes.len() as u16,
                buf: bytes as *const _ as *mut _,
            }],
        )
    }
}

impl embedded_hal::i2c::blocking::WriteIter for I2cBus {
    type Error = I2cError;

    fn write_iter<B>(&mut self, address: u8, bytes: B) -> Result<(), Self::Error>
    where
        B: IntoIterator<Item = u8>,
    {
        use embedded_hal::i2c::blocking::Write;
        self.write(address, &mut bytes.into_iter().collect::<Vec<_>>())
    }
}

impl embedded_hal::i2c::blocking::WriteRead for I2cBus {
    type Error = I2cError;

    fn write_read(
        &mut self,
        address: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        rdwr(
            self.0,
            &[
                iic_msg {
                    addr: (address as u16) << 1,
                    flags: IIC_M_WR | IIC_M_NOSTOP,
                    len: bytes.len() as u16,
                    buf: bytes as *const _ as *mut _,
                },
                iic_msg {
                    addr: (address as u16) << 1,
                    flags: IIC_M_RD,
                    len: buffer.len() as u16,
                    buf: buffer as *const _ as *mut _,
                },
            ],
        )
    }
}

impl embedded_hal::i2c::blocking::WriteIterRead for I2cBus {
    type Error = I2cError;

    fn write_iter_read<B>(
        &mut self,
        address: u8,
        bytes: B,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error>
    where
        B: IntoIterator<Item = u8>,
    {
        use embedded_hal::i2c::blocking::WriteRead;
        self.write_read(address, &mut bytes.into_iter().collect::<Vec<_>>(), buffer)
    }
}

#[derive(PartialEq)]
enum OpState {
    First,
    WasRead,
    WasWrite,
}

impl embedded_hal::i2c::blocking::Transactional for I2cBus {
    type Error = I2cError;

    fn exec<'a>(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::blocking::Operation<'a>],
    ) -> Result<(), Self::Error> {
        let mut st = OpState::First;
        let mut msgs = Vec::with_capacity(operations.len());
        let mut it = operations.into_iter().peekable();

        while let Some(op) = it.next() {
            use embedded_hal::i2c::blocking::Operation;

            msgs.push(match op {
                Operation::Read(buffer) => {
                    let prev_was_read = st == OpState::WasRead;
                    st = OpState::WasRead;
                    iic_msg {
                        addr: (address as u16) << 1,
                        flags: IIC_M_RD
                            | if it.peek().is_some() { IIC_M_NOSTOP } else { 0 }
                            | if prev_was_read { IIC_M_NOSTART } else { 0 },
                        len: buffer.len() as u16,
                        buf: buffer as *const _ as *mut _,
                    }
                },
                Operation::Write(bytes) => {
                    let prev_was_write = st == OpState::WasWrite;
                    st = OpState::WasWrite;
                    iic_msg {
                        addr: (address as u16) << 1,
                        flags: IIC_M_WR
                            | if it.peek().is_some() { IIC_M_NOSTOP } else { 0 }
                            | if prev_was_write { IIC_M_NOSTART } else { 0 },
                        len: bytes.len() as u16,
                        buf: bytes as *const _ as *mut _,
                    }
                },
            });
        }

        rdwr(self.0, &msgs[..])
    }
}

impl embedded_hal::i2c::blocking::TransactionalIter for I2cBus {
    type Error = I2cError;

    fn exec_iter<'a, O>(&mut self, address: u8, operations: O) -> Result<(), Self::Error>
    where
        O: IntoIterator<Item = embedded_hal::i2c::blocking::Operation<'a>>,
    {
        // Not trying to execute on-the-fly, because
        // *lots* of hardware has automatic start-stop handling,
        // for which the drivers implement validation of nostart/nostop flags
        // by looking behind in the list of operations.
        use embedded_hal::i2c::blocking::Transactional;
        self.exec(address, &mut operations.into_iter().collect::<Vec<_>>())
    }
}

const IIC_M_WR: u16 = 0x00;
const IIC_M_RD: u16 = 0x01;
const IIC_M_NOSTOP: u16 = 0x02;
const IIC_M_NOSTART: u16 = 0x04;

#[repr(C)]
#[allow(non_camel_case_types)]
struct iic_msg {
    addr: u16,
    flags: u16,
    len: u16,
    buf: *mut u8,
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct iic_rdwr_data {
    msgs: *const iic_msg,
    nmsgs: u32,
}

fn rdwr(fd: RawFd, msgs: &[iic_msg]) -> Result<(), I2cError> {
    let mut dat = iic_rdwr_data { msgs: msgs.as_ptr(), nmsgs: msgs.len() as u32 };
    let res = unsafe {
        libc::ioctl(fd, 0x80106906 /*I2CRDWR*/, &mut dat as *mut _)
    };
    if res == -1 {
        return Err(I2cError(io::Error::last_os_error()));
    }
    Ok(())
}
