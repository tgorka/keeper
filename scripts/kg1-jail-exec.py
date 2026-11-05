"""Story 110.1 proof-only jail launcher (never shipped).

Run as root as the transient scope's command:

    /usr/bin/python3 -I -S kg1-jail-exec.py ROOT UID GID NAME=VALUE... -- COMMAND ARGS...

In this one process: a private mount namespace, the pinned rootfs's /proc /sys /dev
recursively bound from the host (slave), chroot to ROOT, chdir /, drop to UID/GID with
exactly [GID] as groups (verified), then execve of COMMAND with exactly the NAME=VALUE
environment. Every step ends in execve, so COMMAND keeps this pid and no launcher
process remains in the scope. Host coreutils are not used: uutils coreutils 0.2.2
`chroot` forks and waits instead of exec'ing (round 18).

After chroot the host interpreter's standard library is unreachable (round 20: a lazy
`import warnings` inside os.execvpe failed there). So everything the post-chroot path
needs is decided before chroot: COMMAND is resolved to an absolute in-root executable
(PATH searched against ROOT), the argv and environment are built, and after chroot only
posix builtins run (chdir, set*id, get*id, execve, write, _exit). Any failure exits
nonzero before COMMAND runs; nothing is mounted on the host.
"""
import ctypes
import os
import sys

MS_BIND, MS_REC, MS_PRIVATE, MS_SLAVE = 0x1000, 0x4000, 1 << 18, 1 << 19


def resolve(root, command, path):
    """The absolute in-root path that execve will run, decided against ROOT before chroot.
    The kernel still resolves it inside the chroot; a mismatch fails the execve."""
    if '/' in command:
        if not command.startswith('/'):
            raise SystemExit(f'relative command path refused: {command}')
        candidates = [command]
    else:
        candidates = [d.rstrip('/') + '/' + command for d in path.split(':') if d.startswith('/')]
    for candidate in candidates:
        inside = root.rstrip('/') + candidate
        if os.path.isfile(inside) and os.access(inside, os.X_OK):
            return candidate
    raise SystemExit(f'{command}: no executable under {root} on PATH={path}')


def fail(message):
    """Post-chroot failure: builtins only, no traceback machinery (it may import)."""
    os.write(2, ('kg1-jail-exec: ' + message + '\n').encode())
    os._exit(126)


def main(argv):
    sep = argv.index('--')
    root, uid, gid, *assignments = argv[1:sep]
    command = argv[sep + 1:]
    if not command or not os.path.isabs(root):
        raise SystemExit('usage: kg1-jail-exec.py ROOT UID GID NAME=VALUE... -- COMMAND ARGS...')
    uid, gid = int(uid), int(gid)
    env = dict(a.split('=', 1) for a in assignments)
    executable = resolve(root, command[0], env.get('PATH', ''))
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
    # ---- the host standard library is unreachable from here on: posix builtins only
    try:
        os.chdir('/')
        if os.getgroups() != [gid]:
            os.setgroups([gid])
        os.setgid(gid)
        os.setuid(uid)
        if os.getresuid() != (uid, uid, uid) or os.getresgid() != (gid, gid, gid) or os.getgroups() != [gid]:
            fail(f'identity not dropped: {os.getresuid()!r} {os.getresgid()!r} {os.getgroups()!r}')
        os.execve(executable, command, env)
    except BaseException as error:
        fail(f'{type(error).__name__}: {error}')


if __name__ == '__main__':
    main(sys.argv)
