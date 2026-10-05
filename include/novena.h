/* novena host interface. See docs/design.md. */
#ifndef NOVENA_H
#define NOVENA_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NOVENA_HOST_INTERFACE_VERSION 0u

/* Library version as a NUL-terminated string owned by the library. */
const char *novena_version(void);

/* Host interface version the library was built with. A host compares it with
 * NOVENA_HOST_INTERFACE_VERSION and refuses to continue when they differ. */
uint32_t novena_host_interface_version(void);

#ifdef __cplusplus
}
#endif

#endif /* NOVENA_H */
