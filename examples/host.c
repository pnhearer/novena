/* The smallest possible host: asks for two functions by name, calls one,
 * and prints the census. Build and run with examples/run-host.sh. */
#include "novena.h"

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv)
{
    if (novena_host_interface_version() != NOVENA_HOST_INTERFACE_VERSION) {
        fprintf(stderr, "novena was built for another host interface version\n");
        return 1;
    }

    printf("novena %s, %u functions\n", novena_version(), novena_function_count());

    novena_instance *instance = novena_instance_create(NULL);

    /* A real host does this when the program calls its bootstrap function. */
    const char *first = novena_function_name(0);
    uint32_t function = novena_instance_request(instance, first);
    if (function == NOVENA_FUNCTION_NONE) {
        fprintf(stderr, "%s is missing from the table\n", first);
        return 1;
    }
    novena_instance_request(instance, "aNameTheLibraryDoesNotKnow");

    /* And this when the program calls the pointer it was given. */
    novena_registers registers = { 0 };
    registers.x[0] = 1234;
    novena_status status = novena_instance_call(instance, function, &registers);
    if (status != NOVENA_UNIMPLEMENTED || registers.x[0] != 0) {
        fprintf(stderr, "unexpected result from an unimplemented function\n");
        return 1;
    }
    printf("%s called %llu time(s)\n", first,
           (unsigned long long)novena_instance_call_count(instance, function));

    if (argc > 1 && novena_instance_write_census(instance, argv[1]) != NOVENA_OK) {
        fprintf(stderr, "could not write %s\n", argv[1]);
        return 1;
    }

    novena_instance_destroy(instance);
    return 0;
}
