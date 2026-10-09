# Security policy

Huldra is a hobby operating system. It has no users or permissions yet,
and its package repository is served over plain HTTP. **Do not use it
where security matters.** Even so, we want to know about vulnerabilities,
especially ones that would survive into a future multi-user design:
memory safety bugs in the kernel, system calls that can corrupt kernel
state, file system images that crash the kernel, network packets that
crash or hang the TCP/IP stack, and package archives that write outside the
store.

## Supported versions

Only the latest release and `main` get fixes.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub security advisories](https://github.com/villetacore/huldra/security/advisories/new).

Include:

- what is affected (kernel subsystem, program, library) and the commit or
  release;
- how to reproduce it: a crafted file, a disk image, a packet capture or a
  program;
- what happens (panic, hang, wrong result, escape).

You can expect an acknowledgement within a week. Once a fix is released, we
will credit you in the changelog unless you prefer otherwise.
