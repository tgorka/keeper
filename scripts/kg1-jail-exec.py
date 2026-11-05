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

Before chroot no path is resolved in the host namespace (round 24): COMMAND's in-root
path is resolved by `in_root` (links re-rooted at ROOT, bounded, no escape), and the
/proc /sys /dev mount points must be real directories directly under ROOT.
"""
import ctypes
import os
import sys

MS_BIND, MS_REC, MS_PRIVATE, MS_SLAVE = 0x1000, 0x4000, 1 << 18, 1 << 19
S_IFMT, S_IFLNK, S_IFREG, S_IFDIR = 0o170000, 0o120000, 0o100000, 0o040000
MAX_HOPS = 40


class Unresolvable(Exception):
    """A path that cannot be resolved inside ROOT: missing, a loop, or an escape."""


def in_root(root, path):
    """PATH (absolute, as seen inside ROOT) with every symlink resolved inside ROOT: an
    absolute link target is re-rooted at ROOT, a relative one is followed from its own
    directory, at most MAX_HOPS links are followed, and `..` above ROOT is refused.
    Only lstat/readlink are used, on ROOT-prefixed paths whose every component is already
    resolved, so no link is ever followed in the host namespace (round 24)."""
    root = root.rstrip('/')
    pending = [c for c in path.split('/') if c]
    done, hops = [], 0
    while pending:
        part = pending.pop(0)
        if part == '.':
            continue
        if part == '..':
            if not done:
                raise Unresolvable(f'{path}: escapes ROOT')
            done.pop()
            continue
        here = root + '/' + '/'.join(done + [part])
        try:
            st = os.lstat(here)
        except OSError as error:
            raise Unresolvable(f'{path}: /{"/".join(done + [part])}: {error.strerror}') from None
        if st.st_mode & S_IFMT == S_IFLNK:
            hops += 1
            if hops > MAX_HOPS:
                raise Unresolvable(f'{path}: more than {MAX_HOPS} links (loop)')
            target = os.readlink(here)
            if target.startswith('/'):
                done = []
            pending = [c for c in target.split('/') if c] + pending
        else:
            done.append(part)
    return '/' + '/'.join(done)


def resolve(root, command, path):
    """The absolute in-root path that execve will run, decided inside ROOT before chroot
    (the kernel then resolves the same path inside the chroot). A bare name is searched
    along PATH; a missing candidate moves on, a loop or an escape refuses outright."""
    if '/' in command:
        if not command.startswith('/'):
            raise SystemExit(f'relative command path refused: {command}')
        candidates = [command]
    else:
        candidates = [d.rstrip('/') + '/' + command for d in path.split(':') if d.startswith('/')]
    for candidate in candidates:
        try:
            real = in_root(root, candidate)
        except Unresolvable as error:
            if 'escapes ROOT' in str(error) or '(loop)' in str(error):
                raise SystemExit(f'{command}: refused: {error}') from None
            continue
        st = os.lstat(root.rstrip('/') + real)  # no link component remains
        if st.st_mode & S_IFMT == S_IFREG and st.st_mode & 0o111:
            return candidate
    raise SystemExit(f'{command}: no executable under {root} on PATH={path}')


def mount_targets(root):
    """The in-root /proc /sys /dev mount points. mount(2) follows a symlinked target in the
    host namespace, so each must be a real directory directly under ROOT."""
    targets = []
    for name in ('proc', 'sys', 'dev'):
        target = root.rstrip('/') + '/' + name
        try:
            mode = os.lstat(target).st_mode
        except OSError as error:
            raise SystemExit(f'mount target {target}: {error.strerror}') from None
        if mode & S_IFMT != S_IFDIR:
            raise SystemExit(f'mount target {target} is not a real directory in ROOT')
        targets.append((name, target))
    return targets


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
    targets = mount_targets(root)
    libc = ctypes.CDLL(None, use_errno=True)

    def mount(source, target, flags):
        if libc.mount(source.encode() if source else None, target.encode(), None, flags, None) != 0:
            errno = ctypes.get_errno()
            raise OSError(errno, f'mount {source} {target}: {os.strerror(errno)}')
    os.unshare(os.CLONE_NEWNS)
    mount(None, '/', MS_REC | MS_PRIVATE)
    for name, target in targets:
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
