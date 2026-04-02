// TODO: Consider replacing raw ioctl code with the `framework_lib` crate
//       (https://crates.io/crates/framework_lib) which provides typed
//       EcRequestRgbKbdSetColor + CrosEc abstraction. Worth it if we add more
//       EC interactions; overkill for just one command.

use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;

pub const CROS_EC_DEV: &str = "/dev/cros_ec";
const EC_CMD_RGBKBD_SET_COLOR: u32 = 0x013A;
pub const NUM_LEDS: usize = 8;

// ioctl definition: magic 0xEC, command 0, read/write CrosEcCommandV2
// The kernel struct uses a flexible array member (data[]) which has size 0,
// so the ioctl number must encode only the 5×u32 header size (20 bytes),
// not our Rust struct's full size which includes the data buffer.
nix::ioctl_readwrite_bad!(
    cros_ec_cmd,
    nix::request_code_readwrite!(0xEC, 0, 20),
    CrosEcCommandV2
);

#[repr(C)]
pub struct CrosEcCommandV2 {
    version: u32,
    command: u32,
    outsize: u32,
    insize: u32,
    result: u32,
    data: [u8; 256],
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RgbS {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl std::fmt::Display for RgbS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

pub fn set_fan_colors(dev: &File, colors: &[RgbS; NUM_LEDS]) -> io::Result<()> {
    let mut cmd = CrosEcCommandV2 {
        version: 0,
        command: EC_CMD_RGBKBD_SET_COLOR,
        outsize: (2 + NUM_LEDS * 3) as u32, // start_key + length + color data
        insize: 0,
        result: 0,
        data: [0u8; 256],
    };

    // Pack the request: start_key=0, length=NUM_LEDS, then RGB triplets
    cmd.data[0] = 0; // start_key
    cmd.data[1] = NUM_LEDS as u8; // length
    for (i, color) in colors.iter().enumerate() {
        cmd.data[2 + i * 3] = color.r;
        cmd.data[2 + i * 3 + 1] = color.g;
        cmd.data[2 + i * 3 + 2] = color.b;
    }

    // Safety: we're passing a properly initialized struct to the kernel ioctl
    unsafe {
        cros_ec_cmd(dev.as_raw_fd(), &mut cmd).map_err(io::Error::other)?;
    }

    if cmd.result != 0 {
        return Err(io::Error::other(format!(
            "EC returned error: {}",
            cmd.result
        )));
    }

    Ok(())
}

/// Map a load value (0.0 - 1.0) to a color: blue → purple → red.
pub fn load_to_color(load: f64) -> RgbS {
    let load = load.clamp(0.0, 1.0);
    RgbS {
        r: (load * 255.0) as u8,
        g: 0,
        b: ((1.0 - load) * 255.0) as u8,
    }
}

pub fn print_color_bar(colors: &[RgbS; NUM_LEDS], avg_load: f64) {
    // Print colored blocks using ANSI true color escape codes
    print!("\r  ");
    for color in colors {
        print!("\x1b[48;2;{};{};{}m   \x1b[0m", color.r, color.g, color.b);
    }
    print!("  avg: {:.0}%  ", avg_load * 100.0);
    use std::io::Write;
    std::io::stdout().flush().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_to_color_zero() {
        let c = load_to_color(0.0);
        assert_eq!(c, RgbS { r: 0, g: 0, b: 255 });
    }

    #[test]
    fn test_load_to_color_full() {
        let c = load_to_color(1.0);
        assert_eq!(c, RgbS { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_load_to_color_mid() {
        let c = load_to_color(0.5);
        assert_eq!(
            c,
            RgbS {
                r: 127,
                g: 0,
                b: 127
            }
        );
    }

    #[test]
    fn test_load_to_color_clamps() {
        let low = load_to_color(-0.5);
        assert_eq!(low, RgbS { r: 0, g: 0, b: 255 });
        let high = load_to_color(1.5);
        assert_eq!(high, RgbS { r: 255, g: 0, b: 0 });
    }
}
