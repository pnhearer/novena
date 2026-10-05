//! Records the shape of the values a program passes to and gets back from
//! each function, while the original implementation still does the work.
//!
//! Nothing is known about any function's signature at the start. Watching
//! what the argument registers hold across many calls shows which of them
//! carry pointers, small numbers, constants or floating-point values, and
//! that is the evidence a signature is worked out from.
//!
//! Only the shape is kept: ranges, a handful of distinct values, and whether
//! a value was a readable address. The memory an address points to is never
//! copied.

use crate::instance::{Host, Registers};
use std::fmt::{self, Write};

/// Calls sampled per function. Shapes settle quickly, and the busiest
/// functions are called a million times a minute.
pub const SAMPLE_LIMIT: u64 = 2048;

const DISTINCT_LIMIT: usize = 6;
/// Values below this are never addresses in a program's address space.
const LOWEST_ADDRESS: u64 = 0x1_0000;
const ADDRESS_LIMIT: u64 = 1 << 48;

/// What one register held across the sampled calls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisterShape {
    samples: u64,
    min: u64,
    max: u64,
    zeros: u64,
    /// How many sampled values were addresses the host could read.
    readable: u64,
    /// The first few distinct values, in order of appearance.
    distinct: Vec<u64>,
    more_distinct: bool,
}

impl RegisterShape {
    fn record(&mut self, value: u64, readable: bool) {
        if self.samples == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.samples += 1;
        self.zeros += u64::from(value == 0);
        self.readable += u64::from(readable);
        if !self.distinct.contains(&value) {
            if self.distinct.len() < DISTINCT_LIMIT {
                self.distinct.push(value);
            } else {
                self.more_distinct = true;
            }
        }
    }

    /// A one-word reading of the register, for the report.
    fn kind(&self) -> &'static str {
        if self.samples == 0 {
            "unseen"
        } else if self.distinct.len() == 1 && !self.more_distinct {
            if self.readable == self.samples {
                "constant-address"
            } else {
                "constant"
            }
        } else if self.readable == self.samples {
            "address"
        } else if self.readable + self.zeros == self.samples && self.readable > 0 {
            "address-or-null"
        } else if self.max < LOWEST_ADDRESS {
            "small"
        } else if self.readable == 0 {
            "number"
        } else {
            "mixed"
        }
    }
}

impl fmt::Display for RegisterShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind())?;
        if self.samples == 0 {
            return Ok(());
        }
        write!(f, " min={:#x} max={:#x}", self.min, self.max)?;
        if self.readable > 0 && self.readable < self.samples {
            write!(f, " readable={}/{}", self.readable, self.samples)?;
        }
        // Addresses identify objects in one run and mean nothing outside it,
        // so only their number is reported. Other values are shown, with the
        // floating-point readings of the same bits.
        if self.readable == 0 {
            f.write_str(" values=[")?;
            for (index, value) in self.distinct.iter().enumerate() {
                if index > 0 {
                    f.write_char(' ')?;
                }
                write!(f, "{value:#x}")?;
            }
            f.write_str(if self.more_distinct { " ...]" } else { "]" })?;
            let singles: Vec<String> = self
                .distinct
                .iter()
                .map(|value| format!("{}", f32::from_bits(*value as u32)))
                .collect();
            let doubles: Vec<String> = self
                .distinct
                .iter()
                .map(|value| format!("{}", f64::from_bits(*value)))
                .collect();
            write!(
                f,
                " f32=[{}] f64=[{}]",
                singles.join(" "),
                doubles.join(" ")
            )?;
        } else {
            let count = self.distinct.len();
            let more = if self.more_distinct { "+" } else { "" };
            write!(f, " distinct={count}{more}")?;
        }
        Ok(())
    }
}

/// What one function's calls looked like.
#[derive(Debug, Clone, Default)]
pub struct FunctionShape {
    sampled_calls: u64,
    sampled_returns: u64,
    x: [RegisterShape; 8],
    d: [RegisterShape; 8],
    result_x0: RegisterShape,
    result_x1: RegisterShape,
    result_d0: RegisterShape,
}

fn is_readable_address(host: Option<&Host>, value: u64) -> bool {
    if !(LOWEST_ADDRESS..ADDRESS_LIMIT).contains(&value) {
        return false;
    }
    let Some(host) = host else {
        return false;
    };
    let Some(read) = host.read_memory else {
        return false;
    };
    let mut probe = [0u8; 8];
    // SAFETY: the buffer is valid for the eight bytes requested. The host
    // promised a callback that reports an inaccessible range with a non-zero
    // result instead of faulting.
    unsafe { read(host.user, value, probe.as_mut_ptr(), probe.len() as u64) == 0 }
}

impl FunctionShape {
    pub(crate) fn record_call(&mut self, host: Option<&Host>, registers: &Registers) {
        if self.sampled_calls >= SAMPLE_LIMIT {
            return;
        }
        self.sampled_calls += 1;
        for (shape, value) in self.x.iter_mut().zip(registers.x) {
            shape.record(value, is_readable_address(host, value));
        }
        for (shape, value) in self.d.iter_mut().zip(registers.d) {
            shape.record(value, false);
        }
    }

    pub(crate) fn record_return(&mut self, host: Option<&Host>, registers: &Registers) {
        if self.sampled_returns >= SAMPLE_LIMIT {
            return;
        }
        self.sampled_returns += 1;
        let x0 = registers.x[0];
        self.result_x0.record(x0, is_readable_address(host, x0));
        let x1 = registers.x[1];
        self.result_x1.record(x1, is_readable_address(host, x1));
        self.result_d0.record(registers.d[0], false);
    }

    pub fn sampled_calls(&self) -> u64 {
        self.sampled_calls
    }

    /// Write the report for one function. Registers that never changed from
    /// call to call are still listed: a register a function does not use
    /// holds whatever the caller left in it, and only the reader can tell a
    /// constant argument from a leftover.
    pub(crate) fn write(&self, out: &mut String, name: &str, calls: u64) {
        let _ = writeln!(
            out,
            "{name} calls={calls} sampled={} returns_sampled={}",
            self.sampled_calls, self.sampled_returns
        );
        for (index, shape) in self.x.iter().enumerate() {
            let _ = writeln!(out, "  x{index} {shape}");
        }
        for (index, shape) in self.d.iter().enumerate() {
            let _ = writeln!(out, "  d{index} {shape}");
        }
        if self.sampled_returns > 0 {
            let _ = writeln!(out, "  result x0 {}", self.result_x0);
            let _ = writeln!(out, "  result x1 {}", self.result_x1);
            let _ = writeln!(out, "  result d0 {}", self.result_d0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    /// A host whose readable memory is one range.
    unsafe extern "C" fn read(_user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32 {
        if (0x10_0000..0x20_0000).contains(&address) {
            // SAFETY: the caller passes a buffer of `size` bytes.
            unsafe { std::ptr::write_bytes(out, 0, size as usize) };
            0
        } else {
            1
        }
    }

    fn host() -> Host {
        Host {
            user: std::ptr::null_mut(),
            read_memory: Some(read),
            write_memory: None,
        }
    }

    #[test]
    fn registers_are_classified_by_what_they_held() {
        let host = host();
        let mut shape = FunctionShape::default();
        for call in 0..10u64 {
            let mut registers = Registers::default();
            registers.x[0] = 0x10_0000 + call * 0x100; // different objects
            registers.x[1] = call % 3; // a small number
            registers.x[2] = 7; // never changes
            registers.x[3] = if call % 2 == 0 { 0 } else { 0x10_8000 }; // optional pointer
            registers.x[4] = 0xdead_beef_0000 + call; // large, not readable
            registers.d[0] = u64::from(1.5f32.to_bits());
            shape.record_call(Some(&host), &registers);
        }
        assert_eq!(shape.x[0].kind(), "address");
        assert_eq!(shape.x[1].kind(), "small");
        assert_eq!(shape.x[2].kind(), "constant");
        assert_eq!(shape.x[3].kind(), "address-or-null");
        assert_eq!(shape.x[4].kind(), "number");
        assert_eq!(shape.d[0].kind(), "constant");

        let mut text = String::new();
        shape.write(&mut text, "someFunction", 10);
        assert!(
            text.starts_with("someFunction calls=10 sampled=10 returns_sampled=0\n"),
            "{text}"
        );
        assert!(
            text.contains("  x1 small min=0x0 max=0x2 values=[0x0 0x1 0x2]"),
            "{text}"
        );
        assert!(
            text.contains(
                "  d0 constant min=0x3fc00000 max=0x3fc00000 values=[0x3fc00000] f32=[1.5]"
            ),
            "{text}"
        );
        // Addresses are counted, not listed.
        assert!(
            text.contains("  x0 address min=0x100000 max=0x100900 distinct=6+"),
            "{text}"
        );
        assert!(!text.contains("result"), "{text}");
    }

    #[test]
    fn sampling_stops_at_the_limit() {
        let mut shape = FunctionShape::default();
        for _ in 0..SAMPLE_LIMIT + 10 {
            shape.record_call(None, &Registers::default());
            shape.record_return(None, &Registers::default());
        }
        assert_eq!(shape.sampled_calls(), SAMPLE_LIMIT);
        assert_eq!(shape.sampled_returns, SAMPLE_LIMIT);
    }

    #[test]
    fn without_a_host_nothing_counts_as_an_address() {
        let mut shape = FunctionShape::default();
        let mut registers = Registers::default();
        registers.x[0] = 0x10_0000;
        shape.record_call(None, &registers);
        assert_eq!(shape.x[0].kind(), "constant");
    }
}
