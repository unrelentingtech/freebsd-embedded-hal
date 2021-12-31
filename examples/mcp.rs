//! MCP23017 GPIO expander test:
//! connect A7 to any B pin, touch B0 to ground, watch that other B pin follow
//!
//! See https://github.com/lucazulian/mcp23017 for a full driver

const IODIRA: u8 = 0x00;
const IODIRB: u8 = 0x01;
const GPIOA: u8 = 0x12;
const GPIOB: u8 = 0x13;
const GPPUB: u8 = 0x0d;

fn main() {
    let adr = 0x20;
    use embedded_hal::i2c::blocking::*;
    let mut iic = freebsd_embedded_hal::I2cBus::from_unit(1).unwrap();
    iic.write(adr, &[IODIRA, 0]).unwrap(); // A* are outputs
    iic.write(adr, &[IODIRB, 0xff]).unwrap(); // B* are inputs
    iic.write(adr, &[GPPUB, 0xff]).unwrap(); // B* are pulled up
    loop {
        let mut bank_b: [u8; 1] = [0x69];
        iic.write_read(adr, &[GPIOB], &mut bank_b).unwrap();
        eprintln!("GPIOB: {:x} ({:b})", bank_b[0], bank_b[0]);
        std::thread::sleep(std::time::Duration::from_millis(10));
        iic.write(adr, &[GPIOA, (bank_b[0] & 1) << 7]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
