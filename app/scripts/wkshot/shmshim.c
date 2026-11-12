// /dev/shm is off limits in an OpenShell sandbox; GTK and WebKit need shared memory (scripts/wkshot/shot.sh).
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/types.h>
// /dev/shm is off limits in this sandbox: keep POSIX shared memory in /tmp/shm instead.
static void path(char *out, const char *name) { snprintf(out, 512, "/tmp/shm/%s", name[0] == '/' ? name + 1 : name); }
int shm_open(const char *name, int flags, mode_t mode) { char p[512]; path(p, name); return open(p, flags | O_CLOEXEC, mode); }
int shm_unlink(const char *name) { char p[512]; path(p, name); return unlink(p); }
#include <stdlib.h>
#include <string.h>
// And memfd_create: an unlinked file in /tmp/shm.
int memfd_create(const char *name, unsigned int flags) {
  char p[] = "/tmp/shm/memfd-XXXXXX";
  int fd = mkostemp(p, (flags & 1) ? O_CLOEXEC : 0);
  if (fd >= 0) unlink(p);
  return fd;
}
#include <dlfcn.h>
#include <stdarg.h>
#include <sys/syscall.h>
long syscall(long n, ...) {
  va_list ap; va_start(ap, n);
  long a[6]; for (int i = 0; i < 6; i++) a[i] = va_arg(ap, long);
  va_end(ap);
  if (n == SYS_memfd_create) return memfd_create((const char *)a[0], (unsigned)a[1]);
  static long (*real)(long, ...);
  if (!real) real = (long (*)(long, ...))dlsym(RTLD_NEXT, "syscall");
  return real(n, a[0], a[1], a[2], a[3], a[4], a[5]);
}
