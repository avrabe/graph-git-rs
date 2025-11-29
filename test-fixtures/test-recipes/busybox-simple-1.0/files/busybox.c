/* Minimal busybox simulation for testing */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "busybox.h"

int main(int argc, char *argv[]) {
    printf("BusyBox v%s - The Swiss Army Knife of Embedded Linux\n", BUSYBOX_VERSION);

    if (argc > 1) {
        if (strcmp(argv[1], "--help") == 0) {
            printf("Usage: busybox [command] [args...]\n");
            printf("Available commands:\n");
            printf("  sh, ls, cat, echo, grep, ...\n");
            return 0;
        }
        printf("Running command: %s\n", argv[1]);
    }

    return 0;
}
