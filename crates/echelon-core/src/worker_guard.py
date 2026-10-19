"""Child-only resource and syscall policy, inherited by every sandbox descendant."""
import ctypes
import errno
import os
import resource
import sys

resource.setrlimit(resource.RLIMIT_CPU, (15, 15))
resource.setrlimit(resource.RLIMIT_AS, (201326592, 201326592))
resource.setrlimit(resource.RLIMIT_FSIZE, (131072, 131072))
resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))

# libseccomp resolves syscall numbers for the running architecture. This policy
# applies to the unprivileged child; the controller retains its authenticated RPC.
lib = ctypes.CDLL("libseccomp.so.2", use_errno=True)
lib.seccomp_init.argtypes = [ctypes.c_uint32]
lib.seccomp_init.restype = ctypes.c_void_p
lib.seccomp_syscall_resolve_name.argtypes = [ctypes.c_char_p]
lib.seccomp_rule_add.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int, ctypes.c_uint]
lib.seccomp_load.argtypes = [ctypes.c_void_p]
lib.seccomp_release.argtypes = [ctypes.c_void_p]
ctx = lib.seccomp_init(0x7FFF0000)
if not ctx:
    raise RuntimeError("sandbox policy initialization failed")
try:
    for name in (
        "socket", "socketpair", "connect", "bind", "listen", "accept", "accept4",
        "sendto", "sendmsg", "sendmmsg", "recvfrom", "recvmsg", "recvmmsg",
        "io_uring_setup", "io_uring_enter", "io_uring_register", "ptrace",
        "process_vm_readv", "process_vm_writev", "bpf", "perf_event_open",
        "mount", "umount2", "setns", "unshare",
    ):
        number = lib.seccomp_syscall_resolve_name(name.encode())
        if number >= 0 and lib.seccomp_rule_add(ctx, 0x00050000 | errno.EPERM, number, 0):
            raise RuntimeError("sandbox policy rule failed")
    if lib.seccomp_load(ctx):
        raise RuntimeError("sandbox policy load failed")
finally:
    lib.seccomp_release(ctx)
os.closerange(3, 1024)
os.execve(
    "/bin/sh", ["sh", "-eu", "-c", sys.argv[1]],
    {"PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": os.getcwd(),
     "LANG": "C.UTF-8", "TMPDIR": os.getcwd()},
)
