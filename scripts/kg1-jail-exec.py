"""Story 110.1 proof-only jail launcher (never shipped).

Run as root as the transient scope's command:

    /usr/bin/python3 -I -S kg1-jail-exec.py ROOT UID GID NAME=VALUE... -- COMMAND ARGS...

In this one process: a private mount namespace, the pinned rootfs's /proc /sys /dev
recursively bound from the host (slave), chroot to ROOT, chdir /, drop to UID/GID with
exactly [GID] as groups (verified), then execve of COMMAND with exactly the NAME=VALUE
environment (PATH searched in-process). Every step ends in execve, so COMMAND keeps
this pid and no launcher process remains in the scope. Host coreutils are not used:
uutils coreutils 0.2.2 `chroot` forks and waits instead of exec'ing (round 18).
Any failure exits nonzero before COMMAND runs; nothing is mounted on the host.
"""
import ctypes
import os
import sys

MS_BIND, MS_REC, MS_PRIVATE, MS_SLAVE = 0x1000, 0x4000, 1 << 18, 1 << 19


def main(argv):
    sep = argv.index('--')
    root, uid, gid, *assignments = argv[1:sep]
    command = argv[sep + 1:]
    if not command or not os.path.isabs(root):
        raise SystemExit('usage: kg1-jail-exec.py ROOT UID GID NAME=VALUE... -- COMMAND ARGS...')
    uid, gid = int(uid), int(gid)
    env = dict(a.split('=', 1) for a in assignments)
    libc = ctypes.CDLL(None, use_errno=True)

    def mount(source, target, flags):
        if libc.mount(source.encode() if source else None, target.encode(), None, flags, None) != 0:
            errno = ctypes.get_errno()
            raise OSError(errno, f'mount {source} {target}: {os.strerror(errno)}')
    os.unshare(os.CLONE_NEWNS)
    mount(None, '/', MS_REC | MS_PRIVATE)
    for name in ('proc', 'sys', 'dev'):
        target = os.path.join(root, name)
        mount('/' + name, target, MS_BIND | MS_REC)
        mount(None, target, MS_REC | MS_SLAVE)
    os.chroot(root)
    os.chdir('/')
    if os.getgroups() != [gid]:
        os.setgroups([gid])
    os.setgid(gid)
    os.setuid(uid)
    if os.getresuid() != (uid,) * 3 or os.getresgid() != (gid,) * 3 or os.getgroups() != [gid]:
        raise SystemExit(f'identity not dropped: {os.getresuid()} {os.getresgid()} {os.getgroups()}')
    os.execvpe(command[0], command, env)


if __name__ == '__main__':
    main(sys.argv)
