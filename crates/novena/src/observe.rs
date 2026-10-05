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
/// Largest value printed as it is.
const WIDE: u64 = u32::MAX as u64;

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

/// A value read as a float, when that reading looks like a number a program
/// would pass on purpose. Integers and addresses read as floats are tiny,
/// huge or not numbers at all, and printing those would bury the real ones.
fn plausible_float(value: f64) -> Option<f64> {
    let magnitude = value.abs();
    (value == 0.0 || (1e-6..1e9).contains(&magnitude)).then_some(value)
}

fn float_readings(values: &[u64]) -> Option<(String, &'static str)> {
    let join = |readings: Vec<Option<f64>>| {
        readings
            .into_iter()
            .map(|reading| reading.map(|value| format!("{value}")))
            .collect::<Option<Vec<_>>>()
            .map(|list| list.join(" "))
    };
    // A single-precision value occupies the low 32 bits with the rest zero.
    let singles = values
        .iter()
        .map(|value| {
            (*value <= u64::from(u32::MAX))
                .then(|| plausible_float(f64::from(f32::from_bits(*value as u32))))
                .flatten()
        })
        .collect();
    if let Some(list) = join(singles) {
        return Some((list, "f32"));
    }
    let doubles = values
        .iter()
        .map(|value| plausible_float(f64::from_bits(*value)))
        .collect();
    join(doubles).map(|list| (list, "f64"))
}

impl fmt::Display for RegisterShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind())?;
        if self.samples == 0 {
            return Ok(());
        }
        // Addresses identify objects in one run and mean nothing outside it,
        // so they are counted and never printed.
        if self.readable > 0 {
            let count = self.distinct.len();
            let more = if self.more_distinct { "+" } else { "" };
            write!(f, " distinct={count}{more}")?;
            if self.readable < self.samples {
                write!(f, " readable={}/{}", self.readable, self.samples)?;
            }
            if self.zeros > 0 {
                write!(f, " zero={}/{}", self.zeros, self.samples)?;
            }
            return Ok(());
        }
        // A value too large for 32 bits that the host could not read may
        // still be an address (of memory the host guards), so it is shown
        // as "wide" and only its float reading, if plausible, is printed.
        if self.max <= WIDE {
            write!(f, " min={:#x} max={:#x}", self.min, self.max)?;
        }
        f.write_str(" values=[")?;
        for (index, value) in self.distinct.iter().enumerate() {
            if index > 0 {
                f.write_char(' ')?;
            }
            if *value <= WIDE {
                write!(f, "{value:#x}")?;
            } else {
                f.write_str("wide")?;
            }
        }
        f.write_str(if self.more_distinct { " ...]" } else { "]" })?;
        let all_zero = self.max == 0;
        if !all_zero {
            if let Some((list, kind)) = float_readings(&self.distinct) {
                write!(f, " {kind}=[{list}]")?;
            }
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
            registers.x[4] = 0xdead_beef_0000 + call; // wide, not readable
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
        // Small integers are not offered as floats.
        assert!(
            text.contains("  x2 constant min=0x7 max=0x7 values=[0x7]\n"),
            "{text}"
        );
        // Addresses are counted, never printed.
        assert!(text.contains("  x0 address distinct=6+\n"), "{text}");
        assert!(
            text.contains("  x3 address-or-null distinct=2 readable=5/10 zero=5/10\n"),
            "{text}"
        );
        assert!(!text.contains("0x100"), "{text}");
        assert!(
            text.contains("  x4 number values=[wide wide wide wide wide wide ...]\n"),
            "{text}"
        );
        assert!(!text.contains("dead"), "{text}");
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
