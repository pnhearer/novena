/* novena host interface, version 3. See docs/host-interface.md. */
#ifndef NOVENA_H
#define NOVENA_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NOVENA_HOST_INTERFACE_VERSION 3u

/* Returned by lookups for a name the library does not know. */
#define NOVENA_FUNCTION_NONE UINT32_MAX

typedef enum novena_status {
    NOVENA_OK = 0,
    /* The function has no behaviour yet. The call was counted and the result
     * registers were set to zero. */
    NOVENA_UNIMPLEMENTED = 1,
    /* The function id is outside the table. */
    NOVENA_BAD_FUNCTION = 2,
    /* A pointer argument was null, or a file could not be written. */
    NOVENA_BAD_ARGUMENT = 3
} novena_status;

/* What the host provides. Both callbacks return 0 on success and any other
 * value when the range is not accessible.
 *
 * The host guarantees that `user` and both callbacks stay usable until the
 * instance is destroyed, and that they tolerate being used from several
 * threads at once: a program can call the graphics API from any thread. */
typedef struct novena_host {
    void *user;
    int32_t (*read_memory)(void *user, uint64_t address, uint8_t *out, uint64_t size);
    int32_t (*write_memory)(void *user, uint64_t address, const uint8_t *data, uint64_t size);
    void (*present)(void *user, uint64_t window_object, uint32_t width, uint32_t height,
                    const uint8_t *rgba, uint64_t stride_bytes);
} novena_host;

/* Argument and result registers of one call under the program's standard
 * calling convention: eight integer registers, the low 64 bits of eight
 * floating-point registers, and the stack pointer for arguments passed on
 * the stack. On return x[0], x[1] and d[0] hold the results. */
typedef struct novena_registers {
    uint64_t x[8];
    uint64_t d[8];
    uint64_t sp;
} novena_registers;

typedef struct novena_instance novena_instance;

/* Library version as a NUL-terminated string owned by the library. */
const char *novena_version(void);

/* Host interface version the library was built with. A host compares it with
 * NOVENA_HOST_INTERFACE_VERSION and refuses to continue when they differ. */
uint32_t novena_host_interface_version(void);

/* The function table. Valid ids are 0 .. count-1. */
uint32_t novena_function_count(void);
const char *novena_function_name(uint32_t function);
uint32_t novena_function_lookup(const char *name);

/* Instances.
 *
 * Every function below that takes an instance needs the pointer returned by
 * novena_instance_create, not yet destroyed. Apart from destroy, they may be
 * called on one instance from several threads at once.
 *
 * host may be null; the structure is copied. */
novena_instance *novena_instance_create(const novena_host *host);

/* Null is accepted. No other call on the instance may be running, and none
 * may be made afterwards. */
void novena_instance_destroy(novena_instance *instance);

/* The program asked its bootstrap function for a name. Records the request
 * and returns the function id, or NOVENA_FUNCTION_NONE. */
uint32_t novena_instance_request(const novena_instance *instance, const char *name);

/* The program called a function. `registers` must not be read or written by
 * anything else until the call returns; each thread uses its own. */
novena_status novena_instance_call(const novena_instance *instance, uint32_t function,
                                   novena_registers *registers);

/* The original implementation of a function returned; `registers` holds its
 * result registers. Only for hosts that let the original implementation run,
 * so that results are sampled along with arguments. */
novena_status novena_instance_returned(const novena_instance *instance, uint32_t function,
                                       const novena_registers *registers);

/* What was requested and called so far. */
uint64_t novena_instance_call_count(const novena_instance *instance, uint32_t function);
novena_status novena_instance_write_census(const novena_instance *instance, const char *path);

/* What the argument and result registers held in the first calls of each
 * function: ranges, a few distinct values, and which were readable addresses.
 * Never the memory behind an address. */
novena_status novena_instance_write_shapes(const novena_instance *instance, const char *path);

#ifdef __cplusplus
}
#endif

#endif /* NOVENA_H */
