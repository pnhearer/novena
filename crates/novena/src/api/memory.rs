//! Memory pools. Signatures: docs/signatures/0003-objects.md.
//!
//! The program gives a pool its storage, a block of its own memory. novena
//! treats that storage's program address as the pool's graphics address too,
//! so a "gpu address" handed back to the program is the program address of
//! the same bytes. The real implementation hands out addresses in another
//! space; nothing observed requires novena to do the same.

use super::{objects::Object, succeed, Handler};
use crate::instance::{Instance, Registers, Status};

fn builder_update(
    instance: &Instance,
    registers: &Registers,
    change: impl FnOnce(&mut Object),
) -> Status {
    instance.objects.update(registers.x[0], change);
    Status::Ok
}

fn pool_field(
    instance: &Instance,
    address: u64,
    pick: impl FnOnce(u64, u64, u64) -> u64,
) -> Option<u64> {
    match instance.objects.get(address) {
        Some(Object::MemoryPool {
            flags,
            storage,
            size,
            ..
        }) => Some(pick(flags, storage, size)),
        _ => None,
    }
}

pub fn handler(name: &str) -> Option<Handler> {
    Some(match name {
        "nvnMemoryPoolBuilderSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::MemoryPoolBuilder {
                    device: 0,
                    flags: 0,
                    storage: 0,
                    size: 0,
                },
            );
            Status::Ok
        },
        "nvnMemoryPoolBuilderSetDevice" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::MemoryPoolBuilder { device, .. } = object {
                    *device = value;
                }
            })
        },
        "nvnMemoryPoolBuilderSetFlags" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::MemoryPoolBuilder { flags, .. } = object {
                    *flags = value;
                }
            })
        },
        "nvnMemoryPoolBuilderSetStorage" => |instance, _, registers| {
            let (memory, bytes) = (registers.x[1], registers.x[2]);
            builder_update(instance, registers, |object| {
                if let Object::MemoryPoolBuilder { storage, size, .. } = object {
                    *storage = memory;
                    *size = bytes;
                }
            })
        },
        "nvnMemoryPoolInitialize" => |instance, _, registers| {
            let Some(Object::MemoryPoolBuilder {
                device,
                flags,
                storage,
                size,
            }) = instance.objects.get(registers.x[1])
            else {
                return Status::BadArgument;
            };
            instance.objects.put(
                registers.x[0],
                Object::MemoryPool {
                    device,
                    flags,
                    storage,
                    size,
                },
            );
            succeed(registers)
        },
        "nvnMemoryPoolFinalize" => |instance, _, registers| {
            instance.objects.remove(registers.x[0]);
            Status::Ok
        },
        // Mapping a pool gives the program a pointer to its contents; the
        // contents are the program's own storage.
        "nvnMemoryPoolMap" | "nvnMemoryPoolGetBufferAddress" => |instance, _, registers| {
            match pool_field(instance, registers.x[0], |_, storage, _| storage) {
                Some(value) => {
                    registers.x[0] = value;
                    Status::Ok
                }
                None => Status::BadArgument,
            }
        },
        "nvnMemoryPoolGetSize" => {
            |instance, _, registers| match pool_field(instance, registers.x[0], |_, _, size| size) {
                Some(value) => {
                    registers.x[0] = value;
                    Status::Ok
                }
                None => Status::BadArgument,
            }
        }
        "nvnMemoryPoolGetFlags" => {
            |instance, _, registers| match pool_field(instance, registers.x[0], |flags, _, _| flags)
            {
                Some(value) => {
                    registers.x[0] = value;
                    Status::Ok
                }
                None => Status::BadArgument,
            }
        }
        _ => return None,
    })
}
