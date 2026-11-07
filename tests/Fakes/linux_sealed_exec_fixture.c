#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

enum { IMAGE_FD = 198, EXIT_USAGE = 40, EXIT_IMAGE_MISMATCH = 41,
       EXIT_UNSEALED = 42, EXIT_BAD_IMAGE = 43, EXIT_OK = 0 };

static int same_object(const struct stat *a, const struct stat *b) {
    return a->st_dev == b->st_dev && a->st_ino == b->st_ino;
}

int main(int argc, char **argv) {
    (void)argv;
    if (argc != 2 || (strcmp(argv[1], "run") != 0 &&
                      strcmp(argv[1], "sleep") != 0)) return EXIT_USAGE;

    struct stat proc_image;
    struct stat inherited_image;
    if (stat("/proc/self/exe", &proc_image) != 0 ||
        fstat(IMAGE_FD, &inherited_image) != 0 ||
        !S_ISREG(proc_image.st_mode) || !S_ISREG(inherited_image.st_mode))
        return EXIT_BAD_IMAGE;
    if (!same_object(&proc_image, &inherited_image)) return EXIT_IMAGE_MISMATCH;

    int seals = fcntl(IMAGE_FD, F_GET_SEALS);
    if (seals < 0 || (seals & (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK |
                               F_SEAL_SEAL)) !=
                          (F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK |
                           F_SEAL_SEAL))
        return EXIT_UNSEALED;
    if (strcmp(argv[1], "sleep") == 0) {
        struct timespec delay = { .tv_sec = 10, .tv_nsec = 0 };
        nanosleep(&delay, NULL);
    }
    return EXIT_OK;
}
